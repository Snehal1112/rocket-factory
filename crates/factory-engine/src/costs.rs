//! Builds the cost record of every board job from its folder and git.

use factory_core::{count_tasks, group_of, usage_summary, JobCost};

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
        let usage = std::fs::read_to_string(dir.join("usage.jsonl")).map(|t| usage_summary(&t, &job.id)).unwrap_or_default();
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
        });
    }
    Ok(out)
}
