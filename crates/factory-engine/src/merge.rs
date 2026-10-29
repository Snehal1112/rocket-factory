use factory_core::{ParkKind, ParkRecord, Stage};
use factory_git::{BaseState, MergeOutcome, Preflight};

use crate::park::write_record;
use crate::{
    now_secs, sync_stage, tick_plan, Driver, EngineError, MessageKind, StoreError, SyncOutcome,
};

pub const MAX_SYNCS: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeResult {
    Merged(String),
    Parked(String),
}

enum Step {
    Merged(String),
    Park(String),
}

/// Parks the job in needs-human with the real reason. Main is never touched here.
pub fn park(driver: &Driver, id: &str, reason: &str) -> Result<(), EngineError> {
    let now = now_secs();
    let at = now.max(0.0) as u64;
    let job = driver.store().update_job(id, |job| {
        // A park written earlier, such as a failed check in the sync, keeps its kind and evidence.
        let already = job.stage == Stage::NeedsHuman && job.park_kind.is_some();
        if !already {
            job.park_kind = Some(ParkKind::Other.as_str().to_string());
            job.park_at = Some(at);
        }
        crate::ports::set_stage_logged(job, Stage::NeedsHuman, Some(reason), false, now)?;
        Ok((job.clone(), already))
    })?;
    crate::ports::log_park(id, reason);
    let (job, already) = job;
    if !already {
        // Writing the record is best effort, the park already happened.
        let mut record = ParkRecord::new(ParkKind::Other, reason, id, job.round, at, Vec::new());
        record.next = ParkKind::Other.next_for(id, job.review_missing(), job.changes_open());
        write_record(&driver.job_dir(&job), &record);
    }
    // The ledger takes one line, so the reason is folded into a single line.
    let one_line = reason.split_whitespace().collect::<Vec<_>>().join(" ");
    driver
        .ledger(&job)
        .ruling(&format!("parked in needs-human: {one_line}"))?;
    Ok(())
}

/// Merges the reviewed head into main, syncing first when main moved. Any failure parks the job.
pub fn merge_stage(driver: &Driver, id: &str) -> Result<MergeResult, EngineError> {
    match try_merge(driver, id) {
        Ok(Step::Merged(sha)) => {
            finish_merge(driver, id, &sha)?;
            tracing::info!(event = "merge.done", sha = %sha);
            // The merge is done, so a failed refresh must not change the result.
            match driver.kb_refresh(id, false) {
                Ok(text) if text.starts_with("kb refresh failed") => {
                    tracing::error!(event = "step.error", step = "kb_refresh", msg = %format!("kb refresh error for job {id}: {text}"));
                }
                Ok(_) => {}
                Err(err) => {
                    tracing::error!(event = "step.error", step = "kb_refresh", msg = %format!("kb refresh error for job {id}: {err}"));
                }
            }
            // Ticket hygiene is best effort too: a failure is written to the ledger, the job output and the events, and never changes the merge.
            if driver.config().tickets_on_merge {
                match crate::tickets::tickets_step(driver, id, false) {
                    Ok(report) if report.failed => {
                        let text = report.lines.join("; ");
                        tracing::error!(event = "step.error", step = "tickets", msg = %format!("tickets error for job {id}: {text}"));
                        crate::tickets::notify_failure(driver, id, &text);
                    }
                    Ok(_) => {}
                    Err(err) => {
                        tracing::error!(event = "step.error", step = "tickets", msg = %format!("tickets error for job {id}: {err}"));
                        crate::tickets::notify_failure(driver, id, &err.to_string());
                    }
                }
            }
            // The costs refresh runs last so that it counts the kb run of this job.
            if let Err(err) = crate::costs::refresh_costs(driver) {
                tracing::error!(event = "step.error", step = "costs_refresh", msg = %format!("costs refresh error for job {id}: {err}"));
            }
            Ok(MergeResult::Merged(sha))
        }
        Ok(Step::Park(reason)) => {
            park(driver, id, &reason)?;
            Ok(MergeResult::Parked(reason))
        }
        Err(err) => {
            let reason = format!("merge failed: {err}");
            park(driver, id, &reason)?;
            Ok(MergeResult::Parked(reason))
        }
    }
}

/// Runs one repository call and logs it as a debug `git.op` event.
pub(crate) fn git_op<T, E>(op: &str, f: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
    let t = std::time::Instant::now();
    let r = f();
    tracing::debug!(
        event = "git.op",
        op = op,
        ok = r.is_ok(),
        duration_ms = t.elapsed().as_millis() as u64
    );
    r
}

/// Takes the repository merge lock, or returns the reason the job must park.
fn take_merge_lock(driver: &Driver) -> Result<crate::BoardLock, String> {
    match crate::lock_repo_merge(driver.repo(), driver.merge_lock_wait()) {
        Ok(lock) => Ok(lock),
        Err(StoreError::Locked) => Err(
            "another project is merging into this repository, approve again to retry".to_string(),
        ),
        Err(err) => Err(format!("cannot take the merge lock: {err}")),
    }
}

fn try_merge(driver: &Driver, id: &str) -> Result<Step, EngineError> {
    let repo = driver.repo();
    let main = repo.base_branch().to_string();
    for _ in 0..=MAX_SYNCS {
        let job = driver.job(id)?;
        let head = job
            .head
            .clone()
            .ok_or_else(|| EngineError::Msg("job has no recorded head".into()))?;
        let base = job
            .base
            .clone()
            .ok_or_else(|| EngineError::Msg("job has no recorded base".into()))?;
        // A job that touched files outside the project folder is never merged.
        if let Some(reason) = crate::scope::scope_violation(repo, &head)? {
            return Ok(Step::Park(reason));
        }
        // merge_no_ff does not check that main has not moved, so base_state decides first.
        let check = git_op("base_state", || repo.base_state(&base, &head))?;
        match check.state {
            // A crash after the merge commit but before the board update lands here.
            BaseState::Merged => return Ok(Step::Merged(check.main_sha)),
            BaseState::BaseNotMain => {
                return Ok(Step::Park(format!(
                    "the job base is not on {main}, a human must look at the branch"
                )));
            }
            BaseState::Current | BaseState::MainMoved => {}
        }
        // A branch that was deleted after a hand merge is handled above, so the tip must exist here.
        let tip = git_op("tip", || repo.tip(&job.branch))?;
        if tip != head {
            return Ok(Step::Park(format!(
                "branch tip {tip} differs from the reviewed head {head}"
            )));
        }
        if let Preflight::Busy { .. } = git_op("preflight_main", || repo.preflight_main())? {
            // Another project may be mid-merge, which briefly dirties main. Look again under the lock.
            let lock = match take_merge_lock(driver) {
                Ok(lock) => lock,
                Err(reason) => return Ok(Step::Park(reason)),
            };
            let again = git_op("preflight_main", || repo.preflight_main())?;
            drop(lock);
            if let Preflight::Busy { reason } = again {
                return Ok(Step::Park(format!("{main} busy: {reason}")));
            }
        }
        if check.state == BaseState::MainMoved {
            match sync_stage(driver, id)? {
                SyncOutcome::Synced => continue,
                SyncOutcome::Parked(reason) => return Ok(Step::Park(reason)),
            }
        }
        let message = driver.commit_message(id, MessageKind::Merge)?;
        // Projects of one repo share main, so only one merges at a time. The lock is held until the
        // end of this iteration, and a merge never runs without it.
        let _merge_lock = match take_merge_lock(driver) {
            Ok(lock) => lock,
            Err(reason) => return Ok(Step::Park(reason)),
        };
        // The message run can take a while, so main is checked again before the merge.
        let again = git_op("base_state", || repo.base_state(&base, &head))?;
        match again.state {
            BaseState::Merged => return Ok(Step::Merged(again.main_sha)),
            BaseState::BaseNotMain => {
                return Ok(Step::Park(format!(
                    "the job base is not on {main}, a human must look at the branch"
                )));
            }
            BaseState::MainMoved => continue,
            BaseState::Current => {}
        }
        return match git_op("merge_no_ff", || {
            repo.merge_no_ff(&job.branch, &head, &message)
        })? {
            MergeOutcome::Merged { merge_sha } => Ok(Step::Merged(merge_sha)),
            MergeOutcome::Conflict { paths } => Ok(Step::Park(format!(
                "merge conflict in {}",
                paths.join(", ")
            ))),
            MergeOutcome::UpToDate => Ok(Step::Park(format!(
                "nothing to merge, head already in {main}"
            ))),
            MergeOutcome::Refused { reason } => Ok(Step::Park(format!("merge refused: {reason}"))),
        };
    }
    Ok(Step::Park(format!(
        "{main} moved again after the maximum number of syncs"
    )))
}

fn finish_merge(driver: &Driver, id: &str, sha: &str) -> Result<(), EngineError> {
    let job = driver.job(id)?;
    let plan = driver.job_dir(&job).join("plan.md");
    if plan.exists() {
        tick_plan(&plan, &driver.ledger(&job), sha)?;
    }
    let now = now_secs();
    driver.store().update_job(id, |job| {
        crate::ports::set_stage_logged(job, Stage::Merged, None, false, now)
    })?;
    Ok(())
}
