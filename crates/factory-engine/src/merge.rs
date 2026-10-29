use factory_core::{park_summary, ParkKind, ParkPart, ParkRecord, Stage};
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
    Park(String, ParkKind, Vec<ParkPart>),
}

impl Step {
    /// A park of kind `Other` with no evidence of its own.
    fn plain(reason: impl Into<String>) -> Step {
        Step::Park(reason.into(), ParkKind::Other, Vec::new())
    }
}

/// Parks the job in needs-human with the real reason. Main is never touched here.
pub fn park(driver: &Driver, id: &str, reason: &str) -> Result<(), EngineError> {
    park_with_parts(driver, id, reason, ParkKind::Other, Vec::new())
}

/// Parks like `park`, with the kind and evidence of the new record. An earlier park keeps its own.
pub fn park_with_parts(
    driver: &Driver,
    id: &str,
    reason: &str,
    kind: ParkKind,
    parts: Vec<ParkPart>,
) -> Result<(), EngineError> {
    let now = now_secs();
    let at = now.max(0.0) as u64;
    // No park keeps an empty evidence list.
    let parts = if parts.is_empty() {
        vec![ParkPart::new("Why it stopped", "driver", reason)]
    } else {
        parts
    };
    let summary = park_summary(reason, &parts);
    let job = driver.store().update_job(id, |job| {
        // A park written earlier, such as a failed check in the sync, keeps its kind and evidence.
        let already = job.stage == Stage::NeedsHuman && job.park_kind.is_some();
        if !already {
            job.park_kind = Some(kind.as_str().to_string());
            job.park_at = Some(at);
            job.park_summary = Some(summary.clone());
        }
        crate::ports::set_stage_logged(job, Stage::NeedsHuman, Some(reason), false, now)?;
        Ok((job.clone(), already))
    })?;
    crate::ports::log_park(id, reason);
    let (job, already) = job;
    if !already {
        // Writing the record is best effort, the park already happened.
        let mut record = ParkRecord::new(kind, reason, id, job.round, at, parts);
        record.next = kind.next_for(
            id,
            job.review_missing(),
            job.last_review.as_ref().map(|r| r.verdict.as_str()),
        );
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
            // Waiver tickets are filed whether or not tickets_on_merge is set, and fail the same soft way.
            match crate::waivers::waiver_tickets_step(driver, id, false) {
                Ok(report) if report.failed => {
                    let text = report.lines.join("; ");
                    tracing::error!(event = "step.error", step = "waivers", msg = %format!("waivers error for job {id}: {text}"));
                    crate::tickets::notify_failure(driver, id, &text);
                }
                Ok(_) => {}
                Err(err) => {
                    tracing::error!(event = "step.error", step = "waivers", msg = %format!("waivers error for job {id}: {err}"));
                    crate::tickets::notify_failure(driver, id, &err.to_string());
                }
            }
            // The minor ticket follows the same soft rule and has no series gate.
            match crate::minors::minor_tickets_step(driver, id, false) {
                Ok(report) if report.failed => {
                    let text = report.lines.join("; ");
                    tracing::error!(event = "step.error", step = "minors", msg = %format!("minors error for job {id}: {text}"));
                    crate::tickets::notify_failure(driver, id, &text);
                }
                Ok(_) => {}
                Err(err) => {
                    tracing::error!(event = "step.error", step = "minors", msg = %format!("minors error for job {id}: {err}"));
                    crate::tickets::notify_failure(driver, id, &err.to_string());
                }
            }
            // The costs refresh runs last so that it counts the kb run of this job.
            if let Err(err) = crate::costs::refresh_costs(driver) {
                tracing::error!(event = "step.error", step = "costs_refresh", msg = %format!("costs refresh error for job {id}: {err}"));
            }
            Ok(MergeResult::Merged(sha))
        }
        Ok(Step::Park(reason, kind, parts)) => {
            park_with_parts(driver, id, &reason, kind, parts)?;
            Ok(MergeResult::Parked(reason))
        }
        Err(err) => {
            let reason = format!("merge failed: {err}");
            let parts = vec![ParkPart::new("Merge error", "git", &err.to_string())];
            park_with_parts(driver, id, &reason, ParkKind::Other, parts)?;
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
            return Ok(Step::plain(reason));
        }
        // merge_no_ff does not check that main has not moved, so base_state decides first.
        let check = git_op("base_state", || repo.base_state(&base, &head))?;
        match check.state {
            // A crash after the merge commit but before the board update lands here.
            BaseState::Merged => return Ok(Step::Merged(check.main_sha)),
            BaseState::BaseNotMain => {
                return Ok(Step::plain(format!(
                    "the job base is not on {main}, a human must look at the branch"
                )));
            }
            BaseState::Current | BaseState::MainMoved => {}
        }
        // A branch that was deleted after a hand merge is handled above, so the tip must exist here.
        let tip = git_op("tip", || repo.tip(&job.branch))?;
        if tip != head {
            let text = format!(
                "reviewed head {head}\nbranch tip {tip}\n\nSomething was committed on the job branch after the review. Run factory rework {id} <note> so it is reviewed, or reset the branch to the reviewed head."
            );
            return Ok(Step::Park(
                format!("branch tip {tip} differs from the reviewed head {head}"),
                ParkKind::Other,
                vec![ParkPart::new("Branch moved", "driver", &text)],
            ));
        }
        if let Preflight::Busy { .. } = git_op("preflight_main", || repo.preflight_main())? {
            // Another project may be mid-merge, which briefly dirties main. Look again under the lock.
            let lock = match take_merge_lock(driver) {
                Ok(lock) => lock,
                Err(reason) => return Ok(Step::plain(reason)),
            };
            let again = git_op("preflight_main", || repo.preflight_main())?;
            drop(lock);
            if let Preflight::Busy { reason, paths } = again {
                let mut text = paths.join("\n");
                if !text.is_empty() {
                    text.push_str("\n\n");
                }
                text.push_str(&format!(
                    "Commit or stash these in the main checkout, then run factory approve {id} again."
                ));
                return Ok(Step::Park(
                    format!("{main} busy: {reason}"),
                    ParkKind::Other,
                    vec![ParkPart::new("Main checkout", "git status", &text)],
                ));
            }
        }
        if check.state == BaseState::MainMoved {
            match sync_stage(driver, id)? {
                SyncOutcome::Synced => continue,
                SyncOutcome::Parked(reason) => return Ok(Step::plain(reason)),
            }
        }
        let message = driver.commit_message(id, MessageKind::Merge)?;
        // Projects of one repo share main, so only one merges at a time. The lock is held until the
        // end of this iteration, and a merge never runs without it.
        let _merge_lock = match take_merge_lock(driver) {
            Ok(lock) => lock,
            Err(reason) => return Ok(Step::plain(reason)),
        };
        // The message run can take a while, so main is checked again before the merge.
        let again = git_op("base_state", || repo.base_state(&base, &head))?;
        match again.state {
            BaseState::Merged => return Ok(Step::Merged(again.main_sha)),
            BaseState::BaseNotMain => {
                return Ok(Step::plain(format!(
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
            MergeOutcome::Conflict { paths } => Ok(Step::plain(format!(
                "merge conflict in {}",
                paths.join(", ")
            ))),
            MergeOutcome::UpToDate => Ok(Step::plain(format!(
                "nothing to merge, head already in {main}"
            ))),
            MergeOutcome::Refused { reason } => Ok(Step::plain(format!("merge refused: {reason}"))),
        };
    }
    Ok(Step::plain(format!(
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
