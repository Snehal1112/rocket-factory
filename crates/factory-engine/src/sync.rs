use std::path::PathBuf;

use factory_core::Stage;
use factory_git::{JobRef, MergeOutcome, Worktree};

use crate::{now_secs, Decision, Driver, EngineError, MessageKind, StageOutcome, MAX_SYNCS};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncOutcome {
    Synced,
    Parked(String),
}

/// Merges main into the job worktree, reruns checks, and runs one reviewer pass on the sync diff.
pub fn sync_stage(driver: &Driver, id: &str) -> Result<SyncOutcome, EngineError> {
    let job = driver.job(id)?;
    if job.syncs >= MAX_SYNCS {
        return Ok(SyncOutcome::Parked(format!("{} moved again after {} syncs", driver.repo().base_branch(), job.syncs)));
    }
    let path = job.worktree.clone().ok_or_else(|| EngineError::Msg("job has no worktree to sync".into()))?;
    let old_head = job.head.clone().unwrap_or_default();
    let wt = Worktree {
        name: JobRef { id: job.id.clone(), slug: job.slug.clone() }.dir_name(),
        path: PathBuf::from(path),
        branch: job.branch.clone(),
        base: job.base.clone().unwrap_or_default(),
    };
    let message = driver.commit_message(id, MessageKind::Sync)?;
    let new_head = match driver.repo().merge_main_into(&wt, &message)? {
        MergeOutcome::Conflict { paths } => {
            return Ok(SyncOutcome::Parked(format!("sync conflict in {}", paths.join(", "))));
        }
        MergeOutcome::Refused { reason } => return Ok(SyncOutcome::Parked(format!("sync refused: {reason}"))),
        MergeOutcome::UpToDate => return Ok(SyncOutcome::Synced),
        MergeOutcome::Merged { merge_sha } => merge_sha,
    };
    let main7: String = driver.repo().tip(driver.repo().base_branch())?.chars().take(7).collect();
    let now = now_secs();
    driver.store().update_job(id, |j| {
        j.prev_head = Some(old_head.clone());
        j.head = Some(new_head.clone());
        j.syncs += 1;
        // Approve back to review is a backward move, so it is forced and logged as a ruling below.
        j.set_stage(Stage::Review, None, true, now)
    })?;
    driver.ledger(&job).ruling(&format!("head sync {main7} is {new_head}, job back to review"))?;
    // The checks and one reviewer pass run on the sync diff (prev_head..head).
    match driver.checks(id)? {
        StageOutcome::Advanced(Stage::Review) => {}
        StageOutcome::Advanced(_) => return Ok(SyncOutcome::Parked("checks failed after sync".into())),
        StageOutcome::NeedsHuman(why) => return Ok(SyncOutcome::Parked(format!("after sync: {why}"))),
    }
    let outcome = driver.review(id)?;
    match &outcome.decision {
        Decision::AutoMerge => match driver.apply(id, &outcome)? {
            StageOutcome::Advanced(Stage::Approve) => Ok(SyncOutcome::Synced),
            _ => Ok(SyncOutcome::Parked("final checks failed after sync".into())),
        },
        Decision::FixRound => Ok(SyncOutcome::Parked("sync review asked for changes".into())),
        Decision::NeedsHuman { reason: why, .. } => Ok(SyncOutcome::Parked(format!("after sync: {why}"))),
    }
}
