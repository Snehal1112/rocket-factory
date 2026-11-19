//! The merge message of a job, built by the driver from the plan and the builders' notes.

use factory_core::{
    append_footers, build_job_message, check_job_subject, note_bullets, title_subject,
    validate_message, CommitNote, JobMessage, NoteSection,
};

use crate::message::{driver_footers, fixed_message, MessageKind, MessageRequest};
use crate::notes::read_notes;
use crate::tasks::{goal_line, header_commit_line, plan_header};
use crate::{Engine, EngineError};

/// The plan's own `# ` heading, when it has one.
fn plan_heading(plan: &str) -> Option<&str> {
    plan.lines()
        .find_map(|line| line.strip_prefix("# "))
        .map(str::trim)
        .filter(|title| !title.is_empty())
}

/// One message section from a note.
fn section(note: &CommitNote) -> NoteSection {
    NoteSection {
        title: note.subject.clone(),
        bullets: note_bullets(&note.body),
    }
}

impl Engine<'_> {
    /// Builds the merge message: subject, goal, one section per note, review fixes and footers.
    /// A message that fails the check is replaced by the fixed message.
    pub fn job_message(&self, req: &MessageRequest<'_>) -> Result<String, EngineError> {
        let plan = std::fs::read_to_string(&self.plan_file).unwrap_or_default();
        let from_line = match header_commit_line(&plan) {
            Some(line) => match check_job_subject(&line) {
                Ok(()) => Some(line),
                Err(why) => {
                    self.ruling(&format!(
                        "merge message: plan **Commit:** line refused ({why}), plan title used"
                    ))?;
                    None
                }
            },
            None => None,
        };
        let subject = from_line.unwrap_or_else(|| match plan_heading(&plan) {
            Some(title) => title_subject(title),
            None => title_subject(req.title),
        });
        let why = goal_line(&plan_header(&plan));

        let notes = read_notes(&self.job_dir);
        let mut sections: Vec<NoteSection> = Vec::new();
        if let Some(note) = &notes.build {
            sections.push(section(note));
        }
        sections.extend(notes.tasks.iter().map(|(_, note)| section(note)));
        let fixes: Vec<String> = notes
            .rounds
            .iter()
            .flat_map(|(_, note)| note_bullets(&note.body))
            .collect();
        if notes.is_empty() {
            let bullets: Vec<String> = self
                .repo
                .commits_above(req.base, req.tip)
                .unwrap_or_default()
                .into_iter()
                .map(|c| c.subject)
                .filter(|s| !s.starts_with("wip:") && !s.starts_with("chore(factory): sync"))
                .collect();
            if !bullets.is_empty() {
                sections.push(NoteSection {
                    title: "Changes".to_string(),
                    bullets,
                });
            }
        }

        let footers = driver_footers(req);
        let message = build_job_message(&JobMessage {
            subject: &subject,
            why: why.as_deref(),
            tasks: &sections,
            fixes: &fixes,
            footers: &footers,
        });
        match validate_message(&message) {
            Ok(()) => Ok(message),
            Err(why) => {
                self.ruling(&format!("merge message: {why}, fixed message used"))?;
                Ok(append_footers(
                    &fixed_message(
                        MessageKind::Merge,
                        req.job_id,
                        req.slug,
                        self.repo.base_branch(),
                    ),
                    &footers,
                ))
            }
        }
    }
}
