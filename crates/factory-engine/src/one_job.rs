//! One job for a whole spec: the cost estimate, the consent guard and the entry point.

use std::path::Path;

use factory_core::Job;

use crate::runloop::{validate_series, Validated};
use crate::{parse_series_with, Driver, EngineError, SeriesError};

/// The fixed weighted cost of the review, checks and merge of one job.
pub const ONE_JOB_OVERHEAD_WEIGHTED: u64 = 110_000;
/// The rough weighted cost of building one task.
pub const ONE_JOB_PER_TASK_WEIGHTED: u64 = 100_000;

/// The rough weighted cost of one job that holds `tasks` tasks.
pub fn one_job_estimate(tasks: usize) -> u64 {
    ONE_JOB_OVERHEAD_WEIGHTED + ONE_JOB_PER_TASK_WEIGHTED * tasks as u64
}

#[derive(Debug)]
pub struct OneJob {
    pub job: Job,
    /// Plain lines for the CLI to print as `note:` lines.
    pub notes: Vec<String>,
}

/// Creates one job for the whole plan chain. The guard runs first: an estimate over the budget needs consent.
pub fn create_one_job(
    driver: &Driver,
    _slug: &str,
    _feature: &str,
    plan: &Path,
    spec_path: Option<&Path>,
    _model: Option<&str>,
    accept_over_budget: bool,
) -> Result<OneJob, EngineError> {
    // The task cap is lifted, so a plan with no task needs its own message.
    let Validated { series, .. } = validate_series(driver, plan, spec_path, |root, start| {
        parse_series_with(root, start, None).map_err(|e| match e {
            SeriesError::TaskCount { path, found: 0 } => {
                EngineError::Msg(format!("{}: a plan needs at least one task", path.display()))
            }
            other => EngineError::from(other),
        })
    })?;
    let tasks: usize = series.plans.iter().map(|p| p.tasks.len()).sum();
    let estimate = one_job_estimate(tasks);
    let budget = driver.config().budget_weighted;
    if estimate > budget && !accept_over_budget {
        return Err(EngineError::NeedsConsent { estimate, budget });
    }
    Err(EngineError::Msg("one-job creation is not implemented yet".to_string()))
}
