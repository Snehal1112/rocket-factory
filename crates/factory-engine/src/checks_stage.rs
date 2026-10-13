use factory_checks::{plan_checks, run_checks, write_checks_md, ChecksReport};
use factory_core::Stage;

use crate::ports::{git_err, io_err, now};
use crate::{Engine, EngineError, StageOutcome};

impl Engine<'_> {
    /// Runs the mapped checks over the whole job diff and writes `checks.md`.
    pub(crate) fn run_checks_now(&self) -> Result<ChecksReport, EngineError> {
        let job = self.job()?;
        let base = job.base.clone().ok_or_else(|| EngineError::Msg("job has no base".into()))?;
        let head = job.head.clone().ok_or_else(|| EngineError::Msg("job has no head".into()))?;
        let paths = self.repo.changed_paths(&base, &head).map_err(git_err)?;
        let plan = plan_checks(&paths, &self.config);
        let report = run_checks(&self.worktree()?, &plan);
        write_checks_md(&self.job_dir.join("checks.md"), &report).map_err(|e| EngineError::Check(e.to_string()))?;
        Ok(report)
    }

    /// Starts a fix round with `findings`, or parks the job when the rounds are used up.
    pub(crate) fn fix_or_park(&self, findings: &str, reason: &str) -> Result<StageOutcome, EngineError> {
        let job = self.job()?;
        let used = job.round.saturating_sub(1);
        if used >= self.config.fix_rounds {
            return self.park(format!("{reason} after {used} fix rounds"));
        }
        let next = job.round + 1;
        std::fs::write(self.job_dir.join(format!("changes-r{next}.md")), findings).map_err(io_err)?;
        // A fix round moves review back to build. That is a backward move, so it is forced and logged as a ruling.
        self.update(|j| {
            j.round = next;
            j.reset_reviews();
            j.set_stage(Stage::Build, None, true, now())
        })?;
        self.ruling(&format!("fix round {next}, {reason}"))?;
        Ok(StageOutcome::Advanced(Stage::Build))
    }

    /// Runs the checks. On a failure the job goes back to `build` with the trimmed output.
    pub fn checks_stage(&self) -> Result<StageOutcome, EngineError> {
        let report = self.run_checks_now()?;
        let round = self.job()?.round;
        if report.all_passed() {
            self.ruling(&format!("checks round {round} pass"))?;
            return Ok(StageOutcome::Advanced(Stage::Review));
        }
        self.ruling(&format!("checks round {round} fail"))?;
        let findings = format!("The checks failed. Fix these failures only.\n\n{}", report.failure_text());
        self.fix_or_park(&findings, "checks failed")
    }
}

pub fn checks_stage(e: &Engine) -> Result<StageOutcome, EngineError> {
    e.checks_stage()
}
