//! One job for a whole spec: the cost estimate, the consent guard and the entry point.

use std::path::Path;

use factory_core::{Job, Stage};

use crate::runloop::{
    add_job, add_warning, adopt_draft, create_job_with_spec, user_decisions, validate_series, Validated, NO_USER_DECISION,
};
use crate::tasks::parse_tasks;
use crate::{combine_plans, parse_series_with, Driver, EngineError, SeriesError};

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
    slug: &str,
    feature: &str,
    plan: &Path,
    spec_path: Option<&Path>,
    model: Option<&str>,
    accept_over_budget: bool,
) -> Result<OneJob, EngineError> {
    // The task cap is lifted, so a plan with no task needs its own message.
    let Validated { series, plan_warnings, spec_text } = validate_series(driver, plan, spec_path, |root, start| {
        parse_series_with(root, start, None).map_err(|e| match e {
            SeriesError::TaskCount { path, found: 0 } => {
                EngineError::Msg(format!("{}: a plan needs at least one task", path.display()))
            }
            other => EngineError::from(other),
        })
    })?;
    // One plan has nothing to combine, so it takes the normal path and skips the guard.
    if series.plans.len() == 1 {
        let job = create_job_with_spec(driver, slug, feature, Some(plan), spec_path, model)?;
        let notes = vec!["--one-job had no effect: the chain has one plan".to_string()];
        return Ok(OneJob { job, notes });
    }
    let tasks: usize = series.plans.iter().map(|p| p.tasks.len()).sum();
    let estimate = one_job_estimate(tasks);
    let budget = driver.config().budget_weighted;
    if estimate > budget && !accept_over_budget {
        return Err(EngineError::NeedsConsent { estimate, budget });
    }
    // Repo-relative paths keep machine paths out of the combined plan comment.
    let mut texts: Vec<(String, String)> = Vec::new();
    for p in &series.plans {
        let text = std::fs::read_to_string(&p.path)
            .map_err(|e| EngineError::Msg(format!("cannot read {}: {e}", p.path.display())))?;
        let name = match p.path.strip_prefix(driver.root()) {
            Ok(rel) => rel.to_string_lossy().into_owned(),
            Err(_) => p.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        };
        texts.push((name, text));
    }
    let combined = combine_plans(&texts);
    let found = parse_tasks(&combined.text).len();
    if found != combined.tasks {
        return Err(EngineError::Msg(format!(
            "combining the plans failed: the combined plan has {found} tasks but {} were expected",
            combined.tasks
        )));
    }
    let issues = factory_core::parse_issues(feature);
    let mut job = add_job(driver, slug, feature, &issues, None, Stage::Build, model)?;
    let dir = driver.job_dir(&job);
    std::fs::write(dir.join("plan.md"), &combined.text)?;
    if let Some(text) = &spec_text {
        std::fs::write(dir.join("spec.md"), text)?;
    }
    // The estimate is best effort, as in add_job for series jobs.
    if let Some(est) = crate::costs::estimate_for_plan(driver, combined.text.len() as u64) {
        let stored = est.clone();
        let written = driver.store().update_job(&job.id, |j| {
            j.estimate = Some(stored);
            Ok(())
        });
        if written.is_ok() {
            job.estimate = Some(est);
        }
    }
    adopt_draft(driver, slug, &job);
    let mut notes = Vec::new();
    if spec_text.as_deref().is_some_and(|text| user_decisions(text) == 0) {
        job = add_warning(driver, slug, job, NO_USER_DECISION);
    }
    for warning in plan_warnings.iter().chain(&combined.warnings) {
        job = add_warning(driver, slug, job, warning);
    }
    if estimate > budget {
        let text = format!("estimated {estimate} weighted tokens, over the budget of {budget}");
        job = add_warning(driver, slug, job, &text);
    } else {
        notes.push(format!("estimated {estimate} weighted tokens, budget {budget}"));
    }
    Ok(OneJob { job, notes })
}
