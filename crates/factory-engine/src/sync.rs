use std::path::PathBuf;

use factory_core::gate_record::GateKey;
use factory_core::{Job, ParkKind, ResumeAt, ReviewState, Stage};
use factory_git::{MergeOutcome, Worktree};

use crate::gate_record::GateTarget;
use crate::{now_secs, Decision, Driver, EngineError, MessageKind, StageOutcome, MAX_SYNCS};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncOutcome {
    Synced,
    /// A requested sync found nothing to do.
    UpToDate,
    /// The park reason, and the kind of a non-risky decision that keeps its own.
    Parked(String, Option<ParkKind>),
}

/// Who asks for the sync. Only an automatic sync counts against `MAX_SYNCS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncKind {
    Auto,
    Requested,
}

/// Who runs a requested sync. A retry also lets a fix round continue the job and never re-parks a pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncCaller {
    Command,
    Retry,
}

/// Merges main into the job worktree, reruns checks, and runs one reviewer pass on the sync diff.
pub fn sync_stage(driver: &Driver, id: &str) -> Result<SyncOutcome, EngineError> {
    sync_with(driver, id, SyncKind::Auto, false, SyncCaller::Command)
}

/// The worktree of a job, or the error that a job without one cannot sync.
fn job_worktree(driver: &Driver, job: &Job) -> Result<Worktree, EngineError> {
    let path = job
        .worktree
        .clone()
        .ok_or_else(|| EngineError::Refused("job has no worktree to sync".into()))?;
    Ok(Worktree {
        name: driver.repo().job_ref(&job.id, &job.slug).dir_name(),
        path: PathBuf::from(path),
        branch: job.branch.clone(),
        base: job.base.clone().unwrap_or_default(),
    })
}

/// Syncs a job on request, in any open stage. Only an approved head keeps its reviewed floor. Any
/// other case resets the reviews and the floor, merges main when the branch is behind, then runs
/// the whole-branch review.
pub fn sync_requested(driver: &Driver, id: &str) -> Result<SyncOutcome, EngineError> {
    sync_requested_by(driver, id, SyncCaller::Command)
}

/// Like `sync_requested`. A retry always runs the checks and the whole-branch review, and a fix
/// round that starts from them ends the sync as `Synced`.
pub fn sync_requested_by(
    driver: &Driver,
    id: &str,
    by: SyncCaller,
) -> Result<SyncOutcome, EngineError> {
    let retry = by == SyncCaller::Retry;
    let job = driver.job(id)?;
    if job.cancelled {
        return Err(EngineError::Refused(format!("job {id} is cancelled")));
    }
    if !matches!(
        job.stage,
        Stage::Build | Stage::Review | Stage::Approve | Stage::NeedsHuman
    ) {
        return Err(EngineError::Refused(format!("job {id} is {}", job.stage)));
    }
    if job.batch_snapshot.is_some() {
        return Err(EngineError::Refused(format!(
            "job {id} has a pending batch commit, use rework to finish it"
        )));
    }
    let wt = job_worktree(driver, &job)?;
    if !crate::merge::git_op("worktree_is_clean", || driver.repo().worktree_is_clean(&wt))? {
        return Err(EngineError::Refused(format!(
            "the worktree of job {id} has uncommitted changes"
        )));
    }
    let head = job
        .head
        .clone()
        .ok_or_else(|| EngineError::Refused(format!("job {id} has no head to sync")))?;
    let tip = crate::merge::git_op("tip", || driver.repo().tip(&job.branch))?;
    let behind = !crate::merge::git_op("is_ancestor", || {
        driver.repo().is_ancestor(driver.repo().base_branch(), &tip)
    })?;
    // A stopped sync must be able to finish, and a build park keeps its resume stage.
    let unfinished = has_no_verdict(&job)
        && (job.stage == Stage::Review
            || (job.stage == Stage::NeedsHuman && job.park_resume == Some(ResumeAt::Rereview)));
    if tip == head && !behind && !retry && !unfinished {
        return Ok(SyncOutcome::UpToDate);
    }
    // Only an approved head keeps its reviewed floor. Any other case reviews the whole branch.
    let whole_branch = retry || tip != head || job.stage != Stage::Approve;
    if whole_branch {
        // The ruling comes first, so a crash leaves a ruling without a change and not the reverse.
        driver.ledger(&job).ruling(&format!(
            "whole-branch review requested, floor reset to base, head is {tip}"
        ))?;
        let now = now_secs();
        // One write resets everything, so a run that stops later cannot be approved unreviewed.
        driver.store().update_job(id, |j| {
            j.head = Some(tip.clone());
            j.prev_head = None;
            j.reset_reviews();
            crate::ports::set_stage_logged(j, Stage::Review, None, true, now)
        })?;
    }
    let outcome = sync_with(driver, id, SyncKind::Requested, whole_branch, by)?;
    // Nobody else parks a requested sync, so the job must not stay in review with a park message.
    // A park that `park_after_sync` already wrote keeps its own kind and evidence.
    if let SyncOutcome::Parked(reason, kind) = &outcome {
        let kind = kind.unwrap_or(ParkKind::Other);
        // A failed check after the sync belongs to the checks gate, whatever stage the job is at.
        let target =
            if reason == "checks failed after sync" || reason == "final checks failed after sync" {
                GateTarget::Key(GateKey::Checks)
            } else {
                GateTarget::FromKind
            };
        crate::merge::park_with_parts_target(
            driver,
            id,
            reason,
            kind,
            Vec::new(),
            kind.resume(),
            target,
        )?;
        // A stopped sync must be able to finish, so a park before any verdict resumes at the review.
        if whole_branch && has_no_verdict(&driver.job(id)?) {
            crate::merge::set_park_resume(driver, id, ResumeAt::Rereview)?;
        }
    } else if !retry && job.stage == Stage::NeedsHuman && driver.job(id)?.stage == Stage::Approve {
        // The parked job was held for a reason the review does not recheck, so a person releases it.
        let reason = format!("sync review passed, run factory approve {id}");
        // No gate failed, so the park writes only `approve: waiting`.
        crate::merge::park_with_parts_target(
            driver,
            id,
            &reason,
            ParkKind::Other,
            Vec::new(),
            ParkKind::Other.resume(),
            GateTarget::NoFailLine,
        )?;
    }
    Ok(outcome)
}

/// True when a review is pending, running, failed or unknown. Pass, changes and skipped are
/// verdicts.
fn has_no_verdict(job: &Job) -> bool {
    job.reviews.values().any(|s| {
        matches!(
            s,
            ReviewState::Pending
                | ReviewState::Running
                | ReviewState::Failed
                | ReviewState::Unknown
        )
    })
}

/// The shared sync steps. `rewritten` is true when a requested sync already reset the floor, so
/// the merge keeps `prev_head` empty and the one review covers the whole branch.
fn sync_with(
    driver: &Driver,
    id: &str,
    kind: SyncKind,
    rewritten: bool,
    by: SyncCaller,
) -> Result<SyncOutcome, EngineError> {
    let retry = by == SyncCaller::Retry;
    let job = driver.job(id)?;
    if kind == SyncKind::Auto && job.syncs >= MAX_SYNCS {
        return Ok(SyncOutcome::Parked(
            format!(
                "{} moved again after {} syncs",
                driver.repo().base_branch(),
                job.syncs
            ),
            None,
        ));
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
                return Ok(SyncOutcome::Parked(
                    format!("sync conflict in {}", paths.join(", ")),
                    None,
                ));
            }
            MergeOutcome::Refused { reason } => {
                return Ok(SyncOutcome::Parked(format!("sync refused: {reason}"), None))
            }
            MergeOutcome::UpToDate => None,
            MergeOutcome::Merged {
                merge_sha,
                unsigned,
            } => {
                if let Some(reason) = unsigned {
                    crate::merge::report_unsigned(driver, id, "sync", &merge_sha, &reason);
                }
                Some(merge_sha)
            }
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
        // A retry lets the fix round that the checks started carry on.
        StageOutcome::Advanced(Stage::Build) if retry => return Ok(SyncOutcome::Synced),
        StageOutcome::Advanced(_) => {
            return Ok(SyncOutcome::Parked("checks failed after sync".into(), None))
        }
        StageOutcome::NeedsHuman(why) => {
            return Ok(SyncOutcome::Parked(format!("after sync: {why}"), None))
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
            StageOutcome::Advanced(Stage::Build) if retry => Ok(SyncOutcome::Synced),
            _ => Ok(SyncOutcome::Parked(
                "final checks failed after sync".into(),
                None,
            )),
        },
        Decision::FixRound if retry => Ok(match driver.apply(id, &outcome)? {
            StageOutcome::NeedsHuman(why) => SyncOutcome::Parked(why, None),
            _ => SyncOutcome::Synced,
        }),
        Decision::FixRound => Ok(SyncOutcome::Parked(
            "sync review asked for changes".into(),
            None,
        )),
        Decision::NeedsHuman { kind, reason: why } => {
            let text = format!("after sync: {why}");
            // A failed review parks here with the resume stage `Rereview`, like a risky diff parks.
            if *kind == ParkKind::ReviewFailed {
                let parked = driver.apply(id, &outcome);
                if let Err(err) = &parked {
                    tracing::error!(event = "park.error", msg = %format!("park after sync failed for job {id}: {err}"));
                }
                return Ok(SyncOutcome::Parked(
                    crate::park::sync_park_text(parked, &text),
                    None,
                ));
            }
            if *kind != ParkKind::RiskyDiff {
                return Ok(SyncOutcome::Parked(text, Some(*kind)));
            }
            let parked = driver.park_after_sync(id, &text);
            if let Err(err) = &parked {
                tracing::error!(event = "park.error", msg = %format!("park after sync failed for job {id}: {err}"));
            }
            Ok(SyncOutcome::Parked(
                crate::park::sync_park_text(parked, &text),
                None,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_no_verdict_counts_unknown() {
        let mut j = Job::new("001", "d", "s", "2026-11-22", "f", 1.0);
        j.set_review("code", ReviewState::Unknown);
        assert!(has_no_verdict(&j));
    }
}
