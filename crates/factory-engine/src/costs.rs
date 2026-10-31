//! Builds the cost record of every board job from its folder and git.

use std::path::PathBuf;

use factory_core::{
    count_tasks, estimate_for, group_of, stats, usage_summary, CostsFile, Estimate, JobCost,
    Layout, Stage,
};

use crate::clock::now_secs;
use crate::specs::SPEC_REVIEW_JOB;
use crate::{Driver, EngineError};

/// One record per board job. A missing plan, usage file or commit gives an empty field, never an error.
pub fn gather_costs(driver: &Driver) -> Result<Vec<JobCost>, EngineError> {
    let board = driver.store().load()?;
    let cfg = driver.config();
    let mut out = Vec::new();
    for job in &board.jobs {
        let dir = driver.job_dir(job);
        let plan = std::fs::read_to_string(dir.join("plan.md")).ok();
        let plan_bytes = plan.as_ref().map_or(0, |t| t.len() as u64);
        let tasks = plan.as_deref().map_or(0, count_tasks);
        let group = plan.as_ref().map(|_| group_of(plan_bytes, cfg));
        let usage = std::fs::read_to_string(dir.join("usage.jsonl"))
            .map(|t| usage_summary(&t, &job.id))
            .unwrap_or_default();
        let counts = match (&job.base, &job.head) {
            (Some(base), Some(head)) => driver.repo().diff_counts(base, head).ok(),
            _ => None,
        };
        out.push(JobCost {
            id: job.id.clone(),
            slug: job.slug.clone(),
            stage: job.stage.as_str().to_string(),
            group,
            plan_bytes,
            tasks,
            lines: counts.map(|c| (c.insertions + c.deletions) as u64),
            files: counts.map(|c| c.files as u64),
            weighted: usage.weighted,
            raw: usage.raw,
            runs: usage.runs,
            approx: usage.approx,
            roles: usage.roles,
            providers: usage.providers,
            provider_roles: usage.provider_roles,
            turns: usage.turns,
            resumed_runs: usage.resumed_runs,
            round: job.round,
            parked: job
                .stage_history
                .iter()
                .any(|e| e.stage == Stage::NeedsHuman),
        });
    }
    // The spec reviews belong to no job, so their log gets a record of its own. It is not merged,
    // so it never feeds the group figures.
    if let Ok(text) = std::fs::read_to_string(driver.layout().spec_review_usage_file()) {
        let usage = usage_summary(&text, SPEC_REVIEW_JOB);
        if usage.runs > 0 {
            out.push(JobCost {
                id: SPEC_REVIEW_JOB.to_string(),
                slug: SPEC_REVIEW_JOB.to_string(),
                stage: SPEC_REVIEW_JOB.to_string(),
                group: None,
                plan_bytes: 0,
                tasks: 0,
                lines: None,
                files: None,
                weighted: usage.weighted,
                raw: usage.raw,
                runs: usage.runs,
                approx: usage.approx,
                roles: usage.roles,
                providers: usage.providers,
                provider_roles: usage.provider_roles,
                turns: usage.turns,
                resumed_runs: usage.resumed_runs,
                round: 0,
                parked: false,
            });
        }
    }
    Ok(out)
}

/// Where the dashboard reads the group figures.
pub fn costs_path(layout: &Layout) -> PathBuf {
    layout.costs_file()
}

/// Rebuilds `costs.json` from the board, the job folders and git.
pub fn refresh_costs(driver: &Driver) -> Result<(), EngineError> {
    let jobs = gather_costs(driver)?;
    let file = CostsFile::build(&jobs, driver.config(), now_secs());
    file.write(&costs_path(driver.layout()))
        .map_err(|e| EngineError::Core(e.to_string()))
}

/// The estimate for a plan of `plan_bytes` bytes, from the history now. `None` without history.
pub fn estimate_for_plan(driver: &Driver, plan_bytes: u64) -> Option<Estimate> {
    let jobs = gather_costs(driver).ok()?;
    estimate_for(plan_bytes, &stats(&jobs), driver.config())
}
