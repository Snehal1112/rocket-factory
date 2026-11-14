use std::path::PathBuf;

use factory_core::{Job, ParkKind, ReviewState, Stage};
use factory_git::{MergeOutcome, Worktree};

use crate::{now_secs, Decision, Driver, EngineError, MessageKind, StageOutcome, MAX_SYNCS};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncOutcome {
    Synced,
    /// A requested sync found nothing to do.
    UpToDate,
    Parked(String),
}

/// Who asks for the sync. Only an automatic sync counts against `MAX_SYNCS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncKind {
    Auto,
    Requested,
}

/// Merges main into the job worktree, reruns checks, and runs one reviewer pass on the sync diff.
pub fn sync_stage(driver: &Driver, id: &str) -> Result<SyncOutcome, EngineError> {
    sync_with(driver, id, SyncKind::Auto, false)
}

/// The worktree of a job, or the error that a job without one cannot sync.
fn job_worktree(driver: &Driver, job: &Job) -> Result<Worktree, EngineError> {
    let path = job
        .worktree
        .clone()
        .ok_or_else(|| EngineError::Msg("job has no worktree to sync".into()))?;
    Ok(Worktree {
        name: driver.repo().job_ref(&job.id, &job.slug).dir_name(),
        path: PathBuf::from(path),
        branch: job.branch.clone(),
        base: job.base.clone().unwrap_or_default(),
    })
}

/// Syncs a job on request, in any open stage. It resets the floor of a branch that was rewritten
/// outside the factory, merges main when the branch is behind, then runs one review.
pub fn sync_requested(driver: &Driver, id: &str) -> Result<SyncOutcome, EngineError> {
    let job = driver.job(id)?;
    if job.cancelled {
        return Err(EngineError::Msg(format!("job {id} is cancelled")));
    }
    if !matches!(
        job.stage,
        Stage::Build | Stage::Review | Stage::Approve | Stage::NeedsHuman
    ) {
        return Err(EngineError::Msg(format!("job {id} is {}", job.stage)));
    }
    let wt = job_worktree(driver, &job)?;
    if !crate::merge::git_op("worktree_is_clean", || driver.repo().worktree_is_clean(&wt))? {
        return Err(EngineError::Msg(format!(
            "the worktree of job {id} has uncommitted changes"
        )));
    }
    let head = job
        .head
        .clone()
        .ok_or_else(|| EngineError::Msg(format!("job {id} has no head to sync")))?;
    let tip = crate::merge::git_op("tip", || driver.repo().tip(&job.branch))?;
    let rewritten =
        !crate::merge::git_op("is_ancestor", || driver.repo().is_ancestor(&head, &tip))?;
    if rewritten {
        let now = now_secs();
        driver.store().update_job(id, |j| {
            j.head = Some(tip.clone());
            j.prev_head = None;
            crate::ports::set_stage_logged(j, Stage::Review, None, true, now)
        })?;
        driver.ledger(&job).ruling(&format!(
            "branch rewritten outside the factory, floor reset to base, head is {tip}"
        ))?;
    }
    sync_with(driver, id, SyncKind::Requested, rewritten)
}

/// The shared sync steps. `rewritten` is true when a requested sync already reset the floor, so
/// the merge keeps `prev_head` empty and the one review covers the whole branch.
fn sync_with(
    driver: &Driver,
    id: &str,
    kind: SyncKind,
    rewritten: bool,
) -> Result<SyncOutcome, EngineError> {
    let job = driver.job(id)?;
    if kind == SyncKind::Auto && job.syncs >= MAX_SYNCS {
        return Ok(SyncOutcome::Parked(format!(
            "{} moved again after {} syncs",
            driver.repo().base_branch(),
            job.syncs
        )));
    }
    let wt = job_worktree(driver, &job)?;
    let old_head = job.head.clone().unwrap_or_default();
    // A branch that already holds main needs no merge and no message run.
    let behind = !crate::merge::git_op("is_ancestor", || {
        let tip = driver.repo().tip(&job.branch)?;
        driver.repo().is_ancestor(driver.repo().base_branch(), &tip)
    })?;
    if !behind && !rewritten {
        return Ok(match kind {
            SyncKind::Requested => SyncOutcome::UpToDate,
            SyncKind::Auto => SyncOutcome::Synced,
        });
    }
    let merged = if behind {
        let message = driver.commit_message(id, MessageKind::Sync)?;
        match crate::merge::git_op("merge_main_into", || {
            driver.repo().merge_main_into(&wt, &message)
        })? {
            MergeOutcome::Conflict { paths } => {
                return Ok(SyncOutcome::Parked(format!(
                    "sync conflict in {}",
                    paths.join(", ")
                )));
            }
            MergeOutcome::Refused { reason } => {
                return Ok(SyncOutcome::Parked(format!("sync refused: {reason}")))
            }
            MergeOutcome::UpToDate => None,
            MergeOutcome::Merged { merge_sha } => Some(merge_sha),
        }
    } else {
        None
    };
    if let Some(new_head) = merged {
        let main7: String =
            crate::merge::git_op("tip", || driver.repo().tip(driver.repo().base_branch()))?
                .chars()
                .take(7)
                .collect();
        let now = now_secs();
        driver.store().update_job(id, |j| {
            // After a rewrite the floor stays empty, so the review covers base..head.
            if !rewritten {
                j.prev_head = Some(old_head.clone());
            }
            j.head = Some(new_head.clone());
            if kind == SyncKind::Auto {
                j.syncs += 1;
            }
            // Approve back to review is a backward move, so it is forced and logged as a ruling below.
            crate::ports::set_stage_logged(j, Stage::Review, None, true, now)
        })?;
        driver.ledger(&job).ruling(&format!(
            "head sync {main7} is {new_head}, job back to review"
        ))?;
    }
    // The checks and one reviewer pass run on the sync diff (prev_head..head), or on base..head after a rewrite.
    match driver.checks(id)? {
        StageOutcome::Advanced(Stage::Review) => {}
        StageOutcome::Advanced(_) => {
            return Ok(SyncOutcome::Parked("checks failed after sync".into()))
        }
        StageOutcome::NeedsHuman(why) => {
            return Ok(SyncOutcome::Parked(format!("after sync: {why}")))
        }
    }
    let outcome = if rewritten {
        // The job may hold no security role, and a rewritten branch always gets that review.
        driver.store().update_job(id, |j| {
            if !j.reviews.contains_key("security") {
                j.set_review("security", ReviewState::Pending);
            }
            Ok(())
        })?;
        driver.review_rebase(id)?
    } else {
        driver.review_sync(id)?
    };
    match &outcome.decision {
        Decision::AutoMerge => match driver.apply(id, &outcome)? {
            StageOutcome::Advanced(Stage::Approve) => Ok(SyncOutcome::Synced),
            _ => Ok(SyncOutcome::Parked("final checks failed after sync".into())),
        },
        Decision::FixRound => Ok(SyncOutcome::Parked("sync review asked for changes".into())),
        Decision::NeedsHuman { kind, reason: why } => {
            let text = format!("after sync: {why}");
            if *kind != ParkKind::RiskyDiff {
                return Ok(SyncOutcome::Parked(text));
            }
            let parked = driver.park_after_sync(id, &text);
            if let Err(err) = &parked {
                tracing::error!(event = "park.error", msg = %format!("park after sync failed for job {id}: {err}"));
            }
            Ok(SyncOutcome::Parked(crate::park::sync_park_text(
                parked, &text,
            )))
        }
    }
}
