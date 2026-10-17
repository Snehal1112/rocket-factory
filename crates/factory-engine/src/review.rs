use std::path::PathBuf;

use factory_checks::ChecksReport;
use factory_core::{budget_state, ReviewState, classify_paths, parse_verdict, ParkKind, Stage, Verdict};

use crate::decide::{decide, risk_summary, DecideInput, Decision};
use crate::ports::{git_err, io_err};
use crate::specs::ReviewerExtra;
use crate::tasks::{plan_outline, review_focus};
use crate::{Engine, EngineError, StageOutcome};

pub struct ReviewOutcome {
    pub verdict: Verdict,
    pub review_path: PathBuf,
    pub decision: Decision,
}

impl Engine<'_> {
    /// The ledger lines the builder path wrote, `Ruling: builder task N...`, without the `Ruling: ` prefix.
    pub(crate) fn builder_notes(&self) -> Vec<String> {
        let text = std::fs::read_to_string(self.job_dir.join("progress.md")).unwrap_or_default();
        text.lines()
            .filter(|l| l.starts_with("Ruling: builder task "))
            .filter_map(|l| l.strip_prefix("Ruling: "))
            .map(str::to_string)
            .collect()
    }

    /// Writes the diff files, runs the reviewer once, saves its reply and decides.
    pub fn review_stage(&self) -> Result<ReviewOutcome, EngineError> {
        let job = self.job()?;
        let base = job.base.clone().ok_or_else(|| EngineError::Msg("job has no base".into()))?;
        let head = job.head.clone().ok_or_else(|| EngineError::Msg("job has no head".into()))?;
        // The reviewer sees only the new work of this round. Risk and size judge the whole job diff.
        let round_base = job.prev_head.clone().unwrap_or_else(|| base.clone());
        // The file names carry both short shas, so the round diff and the whole diff never overwrite each other.
        let round_diff = self.repo.diff_to_files(&round_base, &head, &self.job_dir).map_err(git_err)?;
        let whole = self.repo.diff_to_files(&base, &head, &self.job_dir).map_err(git_err)?;
        let paths = self.repo.changed_paths(&base, &head).map_err(git_err)?;

        let cwd = self.worktree()?;
        let plan = std::fs::read_to_string(&self.plan_file).unwrap_or_default();
        let extra = ReviewerExtra { focus: review_focus(&plan), notes: self.builder_notes(), plan_outline: plan_outline(&plan) };
        let spec = self.reviewer_spec(&cwd, &round_diff.diff, &round_diff.names, job.round, &self.config.reviewer_rules, &extra);
        self.update(|j| {
            j.set_review("code", ReviewState::Running);
            Ok(())
        })?;
        let run = self.run_agent(spec)?;
        let review_path = self.job_dir.join(format!("review-code-r{}.md", job.round));
        std::fs::write(&review_path, &run.result_text).map_err(io_err)?;

        let verdict = if run.stop_reason == factory_agent::StopReason::Done {
            parse_verdict(&run.result_text)
        } else {
            Verdict::Missing
        };
        let state = if verdict == Verdict::Approve { ReviewState::Pass } else { ReviewState::Changes };
        self.update(|j| {
            j.set_review("code", state);
            Ok(())
        })?;
        self.ruling(&format!("review round {} verdict {:?}", job.round, verdict))?;

        let checks_ok = self.latest_checks_ok()?;
        let class = classify_paths(&paths, &self.config);
        let input = DecideInput {
            checks_ok,
            verdict: &verdict,
            class,
            risky_summary: risk_summary(&paths, &self.config),
            diff_lines: whole.changed_lines(),
            diff_cap: self.config.diff_cap_lines as usize,
            budget: budget_state(self.job_weighted_tokens(), self.config.budget_weighted),
            round: job.round,
            fix_rounds: self.config.fix_rounds,
        };
        let decision = decide(&input);
        Ok(ReviewOutcome { verdict, review_path, decision })
    }

    /// True when `checks.md` exists and has no FAIL line.
    fn latest_checks_ok(&self) -> Result<bool, EngineError> {
        let text = std::fs::read_to_string(self.job_dir.join("checks.md")).map_err(io_err)?;
        Ok(text.lines().any(|l| l.starts_with("PASS ")) && !text.lines().any(|l| l.starts_with("FAIL ")))
    }

    /// Re-runs the mapped checks over the whole job diff on the final head.
    pub fn final_checks(&self) -> Result<ChecksReport, EngineError> {
        self.run_checks_now()
    }

    /// Applies a decision to the board.
    pub fn apply_decision(&self, outcome: &ReviewOutcome) -> Result<StageOutcome, EngineError> {
        match &outcome.decision {
            Decision::NeedsHuman { kind, reason } => {
                let round = self.job()?.round;
                let parts = self.decision_parts(*kind, reason, round);
                self.park_with(*kind, reason.clone(), parts)
            }
            Decision::FixRound => {
                let text = std::fs::read_to_string(&outcome.review_path).map_err(io_err)?;
                let findings = format!("The reviewer asked for changes. Fix these findings only.\n\n{text}");
                self.fix_or_park(&findings, "review asked for changes", ParkKind::ChangesRemain, "review reply")
            }
            Decision::AutoMerge => {
                let report = self.final_checks()?;
                if !report.all_passed() {
                    let findings = format!("The final checks failed. Fix these failures only.\n\n{}", report.failure_text());
                    return self.fix_or_park(&findings, "final checks failed", ParkKind::ChecksFailed, "checks");
                }
                self.set_stage(Stage::Approve, None, false)?;
                self.ruling("auto-merge approved, checks green, reviewer approved")?;
                Ok(StageOutcome::Advanced(Stage::Approve))
            }
        }
    }
}

pub fn review_stage(e: &Engine) -> Result<ReviewOutcome, EngineError> {
    e.review_stage()
}

pub fn apply_decision(e: &Engine, outcome: &ReviewOutcome) -> Result<StageOutcome, EngineError> {
    e.apply_decision(outcome)
}

pub fn final_checks(e: &Engine) -> Result<ChecksReport, EngineError> {
    e.final_checks()
}
