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
        let round = self.job().map_or(1, |j| j.round);
        let at = now().max(0.0) as u64;
        let record = ParkRecord::new(kind, &reason, &self.job_id, round, at, parts);
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

#[cfg(test)]
mod tests {
    use super::*;

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
