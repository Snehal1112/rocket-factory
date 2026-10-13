use std::path::Path;

use factory_core::{Job, Ledger, SeriesPos, Stage};

use crate::{merge_stage, now_secs, park, parse_series, today, Driver, EngineError, MergeResult, StageOutcome};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunEnd {
    Merged,
    NeedsHuman(String),
}

/// Creates the job folder, ledger, plan copy and board entry. The job starts at stage build.
pub fn create_job(
    driver: &Driver,
    slug: &str,
    feature: &str,
    plan: Option<&Path>,
    model: Option<&str>,
) -> Result<Job, EngineError> {
    let series_pos = match plan {
        Some(p) => {
            let series = parse_series(driver.root(), p)?;
            Some(SeriesPos { plan_file: p.to_string_lossy().into_owned(), index: 1, total: series.plans.len() as u32 })
        }
        None => None,
    };
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
        job.set_stage(Stage::Build, None, false, now)?;
        job.series = series_pos;
        job.model = model.map(str::to_string);
        board.jobs.push(job.clone());
        Ok(job)
    })?;
    let dir = driver.job_dir(&job);
    std::fs::create_dir_all(&dir)?;
    Ledger::new(dir.join("progress.md"))
        .create(&format!("# SDD ledger, plan: .superpowers/sdd/{}/plan.md", job.dir))?;
    if let Some(p) = plan {
        std::fs::copy(p, dir.join("plan.md"))?;
    }
    Ok(job)
}

/// Drives one job to merged or needs-human. The board stage is the only state, so a crash resumes here.
pub fn run_job(driver: &Driver, id: &str) -> Result<RunEnd, EngineError> {
    let limit = driver.config().fix_rounds as usize * 4 + 12;
    for _ in 0..limit {
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
            .find(|j| j.series.as_ref().is_some_and(|s| s.plan_file == next_name && s.index == pos.index + 1));
        current = match existing {
            Some(j) => j.id.clone(),
            None => {
                let stem = next.file_stem().and_then(|s| s.to_str()).unwrap_or("plan").to_string();
                let created = create_job(driver, &stem, &job.feature, Some(&next), job.model.as_deref())?;
                driver.store().update_job(&created.id, |j| {
                    j.series = Some(SeriesPos { plan_file: next_name, index: pos.index + 1, total: pos.total });
                    Ok(())
                })?;
                created.id
            }
        };
    }
}
