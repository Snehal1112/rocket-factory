use factory_core::Stage;
use factory_git::{BaseState, MergeOutcome, Preflight};

use crate::{now_secs, sync_stage, tick_plan, Driver, EngineError, MessageKind, SyncOutcome};

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
    let job = driver.store().update_job(id, |job| {
        job.set_stage(Stage::NeedsHuman, Some(reason), false, now)?;
        Ok(job.clone())
    })?;
    // The ledger takes one line, so the reason is folded into a single line.
    let one_line = reason.split_whitespace().collect::<Vec<_>>().join(" ");
    driver.ledger(&job).ruling(&format!("parked in needs-human: {one_line}"))?;
    Ok(())
}

/// Merges the reviewed head into main, syncing first when main moved. Any failure parks the job.
pub fn merge_stage(driver: &Driver, id: &str) -> Result<MergeResult, EngineError> {
    match try_merge(driver, id) {
        Ok(Step::Merged(sha)) => {
            finish_merge(driver, id, &sha)?;
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

fn try_merge(driver: &Driver, id: &str) -> Result<Step, EngineError> {
    let repo = driver.repo();
    for _ in 0..=MAX_SYNCS {
        let job = driver.job(id)?;
        let head = job.head.clone().ok_or_else(|| EngineError::Msg("job has no recorded head".into()))?;
        let base = job.base.clone().ok_or_else(|| EngineError::Msg("job has no recorded base".into()))?;
        // merge_no_ff does not check that main has not moved, so base_state decides first.
        let check = repo.base_state(&base, &head)?;
        match check.state {
            // A crash after the merge commit but before the board update lands here.
            BaseState::Merged => return Ok(Step::Merged(check.main_sha)),
            BaseState::BaseNotMain => {
                return Ok(Step::Park("the job base is not on main, a human must look at the branch".into()));
            }
            BaseState::Current | BaseState::MainMoved => {}
        }
        // A branch that was deleted after a hand merge is handled above, so the tip must exist here.
        let tip = repo.tip(&job.branch)?;
        if tip != head {
            return Ok(Step::Park(format!("branch tip {tip} differs from the reviewed head {head}")));
        }
        if let Preflight::Busy { reason } = repo.preflight_main()? {
            return Ok(Step::Park(format!("main busy: {reason}")));
        }
        if check.state == BaseState::MainMoved {
            match sync_stage(driver, id)? {
                SyncOutcome::Synced => continue,
                SyncOutcome::Parked(reason) => return Ok(Step::Park(reason)),
            }
        }
        let message = driver.commit_message(id, MessageKind::Merge)?;
        // The message run can take a while, so main is checked again before the merge.
        let again = repo.base_state(&base, &head)?;
        match again.state {
            BaseState::Merged => return Ok(Step::Merged(again.main_sha)),
            BaseState::BaseNotMain => {
                return Ok(Step::Park("the job base is not on main, a human must look at the branch".into()));
            }
            BaseState::MainMoved => continue,
            BaseState::Current => {}
        }
        return match repo.merge_no_ff(&job.branch, &head, &message)? {
            MergeOutcome::Merged { merge_sha } => Ok(Step::Merged(merge_sha)),
            MergeOutcome::Conflict { paths } => Ok(Step::Park(format!("merge conflict in {}", paths.join(", ")))),
            MergeOutcome::UpToDate => Ok(Step::Park("nothing to merge, head already in main".into())),
            MergeOutcome::Refused { reason } => Ok(Step::Park(format!("merge refused: {reason}"))),
        };
    }
    Ok(Step::Park("main moved again after the maximum number of syncs".into()))
}

fn finish_merge(driver: &Driver, id: &str, sha: &str) -> Result<(), EngineError> {
    let job = driver.job(id)?;
    let plan = driver.job_dir(&job).join("plan.md");
    if plan.exists() {
        tick_plan(&plan, &driver.ledger(&job), sha)?;
    }
    let now = now_secs();
    driver.store().update_job(id, |job| job.set_stage(Stage::Merged, None, false, now))?;
    Ok(())
}
