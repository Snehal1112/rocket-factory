use std::path::Path;

use factory_core::{Job, Ledger, SeriesPos, Stage};

use crate::{merge_stage, now_secs, park, parse_series, today, Driver, EngineError, MergeResult, StageOutcome};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunEnd {
    Merged,
    NeedsHuman(String),
}

/// Adds one job to the board and makes its folder, ledger and plan copy. A job that runs now starts at
/// stage build. A plan that waits for its turn starts at stage spec and moves to build when it runs.
fn add_job(
    driver: &Driver,
    slug: &str,
    feature: &str,
    series: Option<SeriesPos>,
    stage: Stage,
    model: Option<&str>,
) -> Result<Job, EngineError> {
    // Folder and branch names keep a new id from colliding with a leftover of an older job.
    let dirs: Vec<String> = std::fs::read_dir(driver.root().join(".superpowers/sdd"))
        .map(|rd| rd.filter_map(|e| e.ok()).filter_map(|e| e.file_name().into_string().ok()).collect())
        .unwrap_or_default();
    let branches = driver.repo().local_branch_names()?;
    let job = driver.store().update(|board| {
        let ids: Vec<String> = board.jobs.iter().map(|j| j.id.clone()).collect();
        let id = factory_core::next_job_id(&ids, &dirs, &branches);
        let slug = factory_core::slugify(slug);
        let date = today();
        let dir = factory_core::job_dir_name(&date, &id, &slug);
        let now = now_secs();
        let mut job = Job::new(&id, &dir, &slug, &date, feature, now);
        if stage != Stage::Spec {
            job.set_stage(stage, None, false, now)?;
        }
        job.series = series;
        job.model = model.map(str::to_string);
        job.budget_weighted = Some(driver.config().budget_weighted);
        board.jobs.push(job.clone());
        Ok(job)
    })?;
    let dir = driver.job_dir(&job);
    std::fs::create_dir_all(&dir)?;
    Ledger::new(dir.join("progress.md"))
        .create(&format!("# SDD ledger, plan: .superpowers/sdd/{}/plan.md", job.dir))?;
    if let Some(pos) = &job.series {
        std::fs::copy(&pos.plan_file, dir.join("plan.md"))?;
    }
    Ok(job)
}

/// Creates the jobs of a series and returns plan 1's job, which starts at stage build. Every later plan
/// gets its job now too, parked at stage spec, so the board shows the whole series from the start.
pub fn create_job(
    driver: &Driver,
    slug: &str,
    feature: &str,
    plan: Option<&Path>,
    model: Option<&str>,
) -> Result<Job, EngineError> {
    let Some(start) = plan else {
        return add_job(driver, slug, feature, None, Stage::Build, model);
    };
    // A broken chain fails here, before any job exists.
    let series = parse_series(driver.root(), start)?;
    let total = series.plans.len() as u32;
    // A single plan needs no series id. A longer chain shares one id between all its jobs.
    let series_id = if total > 1 {
        let used: Vec<String> = driver
            .store()
            .load()?
            .jobs
            .iter()
            .filter_map(|j| j.series.as_ref().and_then(|s| s.id.clone()))
            .collect();
        Some(factory_core::next_series_id(&used))
    } else {
        None
    };
    let pos = |index: usize| SeriesPos {
        plan_file: series.plans[index].path.to_string_lossy().into_owned(),
        index: index as u32 + 1,
        total,
        id: series_id.clone(),
    };
    let first = add_job(driver, slug, feature, Some(pos(0)), Stage::Build, model)?;
    // The spec sits beside the plans. Only plan 1 gets a copy, so later builders are not pointed at it.
    let spec = start.join("spec.md");
    if start.is_dir() && spec.is_file() {
        std::fs::copy(&spec, driver.job_dir(&first).join("spec.md"))?;
    }
    for (index, later) in series.plans.iter().enumerate().skip(1) {
        let stem = later.path.file_stem().and_then(|s| s.to_str()).unwrap_or("plan").to_string();
        add_job(driver, &stem, feature, Some(pos(index)), Stage::Spec, model)?;
    }
    Ok(first)
}

/// Drives one job to merged or needs-human. The board stage is the only state, so a crash resumes here.
pub fn run_job(driver: &Driver, id: &str) -> Result<RunEnd, EngineError> {
    let result = run_job_inner(driver, id);
    driver.release_lease(id);
    result
}

fn run_job_inner(driver: &Driver, id: &str) -> Result<RunEnd, EngineError> {
    // Each plan task costs one build call, so the allowance grows with the task count.
    let task_calls = {
        let job = driver.job(id)?;
        let plan = std::fs::read_to_string(driver.job_dir(&job).join("plan.md")).unwrap_or_default();
        crate::tasks::parse_tasks(&plan).len()
    };
    let limit = driver.config().fix_rounds as usize * 4 + 12 + task_calls;
    for _ in 0..limit {
        // A live lease of another driver stops this one here, before any stage work.
        driver.renew_lease(id)?;
        let job = driver.job(id)?;
        match job.stage {
            Stage::Merged => return Ok(RunEnd::Merged),
            Stage::NeedsHuman => return Ok(RunEnd::NeedsHuman(job.reason.unwrap_or_default())),
            Stage::Spec => {
                if !driver.job_dir(&job).join("plan.md").exists() {
                    park(driver, id, "no plan.md in the job folder")?;
                } else {
                    let now = now_secs();
                    driver.store().update_job(id, |j| j.set_stage(Stage::Build, None, false, now))?;
                }
            }
            Stage::Build => {
                driver.build(id)?;
            }
            Stage::Review => {
                // The checks stage starts the fix round itself and parks the job when the rounds are
                // used up, so anything but a pass loops back to the board.
                if driver.checks(id)? != StageOutcome::Advanced(Stage::Review) {
                    continue;
                }
                let outcome = driver.review(id)?;
                // A decision approves after the final checks, starts a fix round, or parks.
                driver.apply(id, &outcome)?;
            }
            Stage::Approve => {
                if let MergeResult::Parked(why) = merge_stage(driver, id)? {
                    return Ok(RunEnd::NeedsHuman(why));
                }
            }
        }
    }
    park(driver, id, "run loop did not settle, see the ledger")?;
    Ok(RunEnd::NeedsHuman("run loop did not settle".into()))
}

/// Runs plan k, then creates and runs plan k+1, one at a time. Stops on needs-human.
pub fn run_series(driver: &Driver, id: &str) -> Result<RunEnd, EngineError> {
    let mut current = id.to_string();
    loop {
        let end = run_job(driver, &current)?;
        if end != RunEnd::Merged {
            return Ok(end);
        }
        let job = driver.job(&current)?;
        let Some(pos) = job.series.clone() else { return Ok(RunEnd::Merged) };
        let series = parse_series(driver.root(), Path::new(&pos.plan_file))?;
        let Some(next) = series.plans[0].next.clone() else { return Ok(RunEnd::Merged) };
        // Resume: the next job may already exist from an earlier run.
        let board = driver.store().load()?;
        let next_name = next.to_string_lossy().into_owned();
        let existing = board
            .jobs
            .iter()
            .find(|j| j.series.as_ref().is_some_and(|s| s.id == pos.id && s.plan_file == next_name && s.index == pos.index + 1));
        current = match existing {
            Some(j) => j.id.clone(),
            None => {
                // A series made before plans got their jobs up front: create this plan's job now.
                let stem = next.file_stem().and_then(|s| s.to_str()).unwrap_or("plan").to_string();
                let next_pos =
                    SeriesPos { plan_file: next_name, index: pos.index + 1, total: pos.total, id: pos.id.clone() };
                add_job(driver, &stem, &job.feature, Some(next_pos), Stage::Build, job.model.as_deref())?.id
            }
        };
    }
}
