use crate::build::is_mid_task;
use crate::tasks::parse_tasks;
use crate::{Driver, Engine, EngineError};

/// How a rework reaches the builder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReworkKind {
    /// The job parked inside a task, so the note goes to that task through `rework-note.md`.
    MidTask,
    /// The job finished its tasks, so the rework is the next round with a `changes-r` file.
    NextRound,
}

/// The sentence between the user's note and the findings.
const NOTE_WINS: &str =
    "The note above is from the user and wins where it conflicts with the findings below.";

impl Engine<'_> {
    fn rework_kind(&self) -> Result<ReworkKind, EngineError> {
        let job = self.job()?;
        let tasks = parse_tasks(&std::fs::read_to_string(&self.plan_file).unwrap_or_default());
        Ok(if is_mid_task(&job, &tasks) {
            ReworkKind::MidTask
        } else {
            ReworkKind::NextRound
        })
    }

    /// The open findings of the job: the last review when it asked for changes, else the failing checks.
    fn open_findings(&self) -> Result<String, EngineError> {
        let job = self.job()?;
        if let Some(last) = &job.last_review {
            if matches!(last.verdict.as_str(), "changes" | "missing") {
                let path = last.path_in(&self.job_dir);
                if let Some(text) = path.and_then(|p| std::fs::read_to_string(p).ok()) {
                    return Ok(text);
                }
            }
        }
        if job.park_kind.as_deref() == Some("checks-failed") {
            if let Ok(text) = std::fs::read_to_string(self.job_dir.join("checks.md")) {
                let fails: Vec<&str> = text.lines().filter(|l| l.starts_with("FAIL")).collect();
                return Ok(fails.join("\n"));
            }
        }
        Ok(String::new())
    }
}

impl Driver {
    /// Whether a rework of this job is a note for the running task or the next round.
    pub fn rework_kind(&self, id: &str) -> Result<ReworkKind, EngineError> {
        self.with_engine(id, |e| e.rework_kind())
    }

    /// The text of the `changes-r` file of a rework: the note first, then the open findings.
    pub fn rework_findings(&self, id: &str, note: &str) -> Result<String, EngineError> {
        let findings = self.with_engine(id, |e| e.open_findings())?;
        if findings.trim().is_empty() {
            return Ok(note.to_string());
        }
        Ok(format!("{note}\n\n{NOTE_WINS}\n\n{findings}"))
    }
}
