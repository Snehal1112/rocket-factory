//! Parks a job and records why, with the evidence, in `park.json`.

use std::path::Path;

use factory_core::{ParkKind, ParkPart, ParkRecord};

use crate::ports::now;
use crate::{Engine, EngineError, StageOutcome};

/// The reviewer's `[blocker]` lines, or the whole reply when it has none.
pub(crate) fn blocker_lines(review_text: &str) -> String {
    let blockers: Vec<&str> = review_text.lines().filter(|l| l.contains("[blocker]")).collect();
    if blockers.is_empty() {
        review_text.to_string()
    } else {
        blockers.join("\n")
    }
}

/// An evidence part read from a file in the job folder. A missing file gives no part.
pub(crate) fn part_from_file(dir: &Path, file: &str, title: &str) -> Option<ParkPart> {
    let text = std::fs::read_to_string(dir.join(file)).ok()?;
    Some(ParkPart::new(title, file, &text))
}

/// Replaces `park.json` in `dir`. The old record goes first, so a failed write leaves none behind.
pub(crate) fn write_record(dir: &Path, record: &ParkRecord) {
    let _ = std::fs::remove_file(dir.join(factory_core::PARK_FILE));
    // A job folder that cannot be written must not stop the park.
    let _ = record.write(dir);
}

impl Engine<'_> {
    /// Records why the job parks, then parks it. Writing `park.json` is best effort.
    pub(crate) fn park_with(&self, kind: ParkKind, reason: String, parts: Vec<ParkPart>) -> Result<StageOutcome, EngineError> {
        let job = self.job().ok();
        let round = job.as_ref().map_or(1, |j| j.round);
        let review_missing = job.as_ref().is_some_and(|j| j.review_missing);
        let at = now().max(0.0) as u64;
        let mut record = ParkRecord::new(kind, &reason, &self.job_id, round, at, parts);
        // A park that offers approve says so when the review has not run, because a plain approve is refused.
        record.next = kind.next_for(&self.job_id, review_missing);
        write_record(&self.job_dir, &record);
        self.update(|j| {
            j.park_kind = Some(kind.as_str().to_string());
            j.park_at = Some(at);
            Ok(())
        })?;
        self.park(reason)
    }

    /// Evidence for a park the decision step chose. Anything that cannot be read is left out.
    pub(crate) fn decision_parts(&self, kind: ParkKind, reason: &str, round: u32) -> Vec<ParkPart> {
        match kind {
            ParkKind::ChecksFailed => part_from_file(&self.job_dir, "checks.md", "Checks").into_iter().collect(),
            ParkKind::ChangesRemain => {
                let file = format!("review-code-r{round}.md");
                match std::fs::read_to_string(self.job_dir.join(&file)) {
                    Ok(text) => vec![ParkPart::new("Reviewer findings", &file, &blocker_lines(&text))],
                    Err(_) => Vec::new(),
                }
            }
            ParkKind::RiskyDiff | ParkKind::DiffTooLarge => {
                let mut parts = vec![ParkPart::new("Why it stopped", "driver", reason)];
                if let Some(stat) = self.diff_stat_text() {
                    parts.push(ParkPart::new("Changed files", "diff stat", &stat));
                }
                parts
            }
            ParkKind::BudgetStopped => {
                let text = format!(
                    "{} weighted tokens used against a budget of {}",
                    self.job_weighted_tokens(),
                    self.config.budget_weighted
                );
                vec![ParkPart::new("Budget", "board", &text)]
            }
            _ => Vec::new(),
        }
    }

    /// The `--stat` text of the whole job diff, or `None` when the diff cannot be made.
    fn diff_stat_text(&self) -> Option<String> {
        let job = self.job().ok()?;
        let (base, head) = (job.base?, job.head?);
        let files = self.repo.diff_to_files(&base, &head, &self.job_dir).ok()?;
        std::fs::read_to_string(files.stat).ok()
    }
}

/// Evidence for a builder that stopped: the stop reason, the error if any, and its last message.
pub(crate) fn builder_stop_parts(run: &factory_agent::AgentRun) -> Vec<ParkPart> {
    let mut text = format!("Stop reason: {}", run.stop_reason.as_str());
    if let Some(error) = &run.error {
        text.push_str(&format!("\nError: {error}"));
    }
    if !run.result_text.trim().is_empty() {
        text.push_str(&format!("\n\nLast message from the builder:\n{}", run.result_text));
    }
    vec![ParkPart::new("Builder stopped", "builder", &text)]
}

/// Evidence for a builder that reported BLOCKED or NEEDS_CONTEXT: the status, its text and the full reply.
pub(crate) fn builder_blocked_parts(status: &factory_core::BuilderStatus, run: &factory_agent::AgentRun) -> Vec<ParkPart> {
    let (label, why) = match status {
        factory_core::BuilderStatus::NeedsContext(why) => ("NEEDS_CONTEXT", why),
        factory_core::BuilderStatus::Blocked(why) => ("BLOCKED", why),
        _ => return Vec::new(),
    };
    let mut text = format!("Status: {label}\nReason: {why}");
    if !run.result_text.trim().is_empty() {
        text.push_str(&format!("\n\nLast message from the builder:\n{}", run.result_text));
    }
    vec![ParkPart::new("Builder blocked", "builder", &text)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_stop_parts_carry_the_stop_reason_and_the_last_message() {
        let mut run = factory_agent::AgentRun::failed("boom");
        run.result_text = "I could not find the file.".to_string();
        run.stop_reason = factory_agent::StopReason::TurnLimit;
        let parts = builder_stop_parts(&run);
        assert_eq!(parts.len(), 1);
        assert!(parts[0].text.contains("turn-limit"), "{}", parts[0].text);
        assert!(parts[0].text.contains("I could not find the file."), "{}", parts[0].text);
    }

    #[test]
    fn builder_stop_parts_without_a_message_still_name_the_reason() {
        let run = factory_agent::AgentRun::failed("boom");
        let parts = builder_stop_parts(&run);
        assert!(parts[0].text.contains("failed"), "{}", parts[0].text);
        assert!(parts[0].text.contains("boom"), "{}", parts[0].text);
    }

    #[test]
    fn blocker_lines_keeps_only_the_blockers() {
        let text = "Summary line\n[blocker] src/a.rs:1 wrong\n[minor] nit\n[blocker] src/b.rs:2 bad\nVERDICT: CHANGES";
        assert_eq!(blocker_lines(text), "[blocker] src/a.rs:1 wrong\n[blocker] src/b.rs:2 bad");
    }

    #[test]
    fn blocker_lines_falls_back_to_the_whole_reply() {
        let text = "Please rework this\nVERDICT: CHANGES";
        assert_eq!(blocker_lines(text), text);
    }

    #[test]
    fn part_from_file_reads_the_file_and_skips_a_missing_one() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        std::fs::write(dir.path().join("checks.md"), "FAIL cargo check")?;
        let p = part_from_file(dir.path(), "checks.md", "Checks");
        assert_eq!(p.map(|p| p.text), Some("FAIL cargo check".to_string()));
        assert!(part_from_file(dir.path(), "missing.md", "Nope").is_none());
        Ok(())
    }

    #[test]
    fn a_failed_write_never_leaves_the_old_record_behind() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let old = ParkRecord::new(ParkKind::Other, "old park", "job", 1, 1, Vec::new());
        old.write(dir.path())?;
        assert!(dir.path().join(factory_core::PARK_FILE).exists());
        // A directory in the temp file's place makes the new write fail.
        let tmp = dir.path().join(format!(".{}.{}.tmp", factory_core::PARK_FILE, std::process::id()));
        std::fs::create_dir(&tmp)?;
        let new = ParkRecord::new(ParkKind::ChecksFailed, "new park", "job", 2, 2, Vec::new());
        write_record(dir.path(), &new);
        assert!(!dir.path().join(factory_core::PARK_FILE).exists());
        assert!(ParkRecord::read(dir.path()).is_none());
        Ok(())
    }
}
