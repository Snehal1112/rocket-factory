use crate::build::is_mid_task;
use crate::checks_stage::CHECKS_FAILURES_FILE;
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
            // Jobs from before `checks-failures.md` have only the `FAIL` lines.
            if let Ok(text) = std::fs::read_to_string(self.job_dir.join(CHECKS_FAILURES_FILE)) {
                if !text.trim().is_empty() {
                    return Ok(text);
                }
            }
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::checks_stage::CHECKS_FAILURES_FILE;
    use crate::park::tests::with_engine;

    const CHECKS_MD: &str = "# Checks\n\nFAIL cargo test (exit 1)\n";

    /// Runs `body` with an engine over a board that holds job `001` parked as `checks-failed`.
    fn with_checks_failed_job(
        last_review: Option<factory_core::LastReview>,
        body: impl FnOnce(&Engine, &Path),
    ) {
        with_engine(
            |board_path| {
                let mut job = factory_core::Job::new("001", "job", "s", "2026-11-22", "f", 1.0);
                job.park_kind = Some("checks-failed".into());
                job.last_review = last_review;
                crate::BoardStore::new(board_path, std::time::Duration::from_secs(1))
                    .update(|board| {
                        board.jobs.push(job);
                        Ok(())
                    })
                    .expect("save board");
            },
            |engine, job_dir, _| body(engine, job_dir),
        );
    }

    #[test]
    fn rework_findings_carry_the_failing_check_output() {
        with_checks_failed_job(None, |engine, job_dir| {
            std::fs::write(job_dir.join("checks.md"), CHECKS_MD).expect("checks");
            std::fs::write(
                job_dir.join(CHECKS_FAILURES_FILE),
                "## cargo test (exit 1)\nerror[E0425]: x\n\n",
            )
            .expect("failures");
            let text = engine.open_findings().expect("findings");
            assert!(text.contains("error[E0425]: x"), "{text}");
        });
    }

    #[test]
    fn rework_findings_fall_back_to_fail_lines_without_the_failures_file() {
        with_checks_failed_job(None, |engine, job_dir| {
            std::fs::write(job_dir.join("checks.md"), CHECKS_MD).expect("checks");
            assert_eq!(
                engine.open_findings().expect("findings"),
                "FAIL cargo test (exit 1)"
            );
            std::fs::write(job_dir.join(CHECKS_FAILURES_FILE), "  \n").expect("blank");
            assert_eq!(
                engine.open_findings().expect("findings"),
                "FAIL cargo test (exit 1)"
            );
        });
    }

    #[test]
    fn rework_findings_prefer_the_review_over_the_failures_file() {
        let last = factory_core::LastReview {
            round: 1,
            verdict: "changes".into(),
            blockers: 1,
            file: "review-code-r1.md".into(),
            source: "build".into(),
        };
        with_checks_failed_job(Some(last), |engine, job_dir| {
            std::fs::write(job_dir.join("review-code-r1.md"), "- [blocker] fix x").expect("review");
            std::fs::write(job_dir.join(CHECKS_FAILURES_FILE), "error[E0425]: x\n")
                .expect("failures");
            assert_eq!(
                engine.open_findings().expect("findings"),
                "- [blocker] fix x"
            );
        });
    }
}
