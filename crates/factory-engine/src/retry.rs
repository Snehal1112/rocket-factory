//! `factory retry`: continues a parked job from the stage that parked it.

use factory_core::{Job, ParkKind, ParkRecord, ResumeAt, Stage};

use crate::runloop::{cancel_reason, run_series, RunEnd};
use crate::{now_secs, sync_requested_by, Driver, EngineError, SyncCaller, SyncOutcome, MAX_SYNCS};

/// The commands the park of a job without a resume stage offers, joined for a refusal.
fn park_commands(driver: &Driver, job: &Job) -> String {
    if let Some(record) = ParkRecord::read(&driver.job_dir(job)) {
        if !record.next.is_empty() {
            return record.next.join(", ");
        }
    }
    let kind = job
        .park_kind
        .as_deref()
        .and_then(|k| serde_json::from_value::<ParkKind>(serde_json::Value::String(k.into())).ok())
        .unwrap_or(ParkKind::Other);
    kind.next(&job.id).join(", ")
}

/// Continues a parked job at the stage that parked it, then drives it like `run` until it merges
/// or parks. It takes no leases, the caller holds them as for `run_series`.
pub fn retry_job(driver: &Driver, id: &str) -> Result<RunEnd, EngineError> {
    match retry_inner(driver, id) {
        Err(EngineError::Cancelled) => Ok(RunEnd::NeedsHuman(cancel_reason(driver, id))),
        other => other,
    }
}

fn retry_inner(driver: &Driver, id: &str) -> Result<RunEnd, EngineError> {
    let job = driver.job(id)?;
    if job.cancelled {
        return Err(EngineError::Refused(format!("job {id} was cancelled")));
    }
    if job.stage != Stage::NeedsHuman {
        return Err(EngineError::Refused(format!(
            "job {id} is in stage {}, retry needs needs-human",
            job.stage
        )));
    }
    // A machine park comes before the batch recovery, so its retry reruns the gate and then the recovery.
    if job.batch_snapshot.is_some() && job.park_kind.as_deref() != Some("machine-waiting") {
        return Err(EngineError::Refused(format!(
            "job {id} has a pending batch commit, use rework to finish it"
        )));
    }
    let Some(resume) = job.park_resume else {
        return Err(EngineError::Refused(format!(
            "job {id} has no stage to retry at; use {}",
            park_commands(driver, &job)
        )));
    };
    let word = resume.as_str();
    match resume {
        ResumeAt::Build => {
            driver.ledger(&job).ruling(&format!("retry at {word}"))?;
            let now = now_secs();
            driver
                .store()
                .update_job(id, |j| crate::resume_logged(j, Stage::Build, now))?;
        }
        ResumeAt::Review => {
            // The ruling comes first, so a crash leaves a ruling without a change and not the reverse.
            driver.ledger(&job).ruling(&format!("retry at {word}"))?;
            let now = now_secs();
            // One write resets the reviews and moves the stage, so a stop in between cannot approve unreviewed.
            driver.store().update_job(id, |j| {
                j.reset_reviews();
                crate::resume_logged(j, Stage::Review, now)
            })?;
        }
        ResumeAt::Merge => {
            // An approval given before the park carries over. Any other park gets the plain approve rules.
            if job.stage_before_park() != Some(Stage::Approve) {
                if let Some(refusal) = job.approve_refusal(false, false) {
                    return Err(EngineError::Refused(refusal));
                }
            }
            driver.ledger(&job).ruling(&format!("retry at {word}"))?;
            let now = now_secs();
            driver
                .store()
                .update_job(id, |j| crate::resume_logged(j, Stage::Approve, now))?;
        }
        ResumeAt::Rereview => {
            // The sync writes its own ruling for the whole-branch review, so this one only marks the retry.
            driver.ledger(&job).ruling(&format!("retry at {word}"))?;
            let outcome = sync_requested_by(driver, id, SyncCaller::Retry)?;
            tracing::info!(
                event = "operator.retry",
                resume = word,
                msg = "retry at the parked stage"
            );
            if let SyncOutcome::Parked(reason, _) = outcome {
                return Ok(RunEnd::NeedsHuman(reason));
            }
            return run_series(driver, id);
        }
        ResumeAt::Sync => {
            if job.stage_before_park() != Some(Stage::Approve) {
                if let Some(refusal) = job.approve_refusal(false, false) {
                    return Err(EngineError::Refused(refusal));
                }
            }
            driver.ledger(&job).ruling(&format!("retry at {word}"))?;
            let now = now_secs();
            // One more automatic sync is allowed, so the count drops to one below the cap.
            driver.store().update_job(id, |j| {
                j.syncs = j.syncs.min(MAX_SYNCS - 1);
                crate::resume_logged(j, Stage::Approve, now)
            })?;
        }
    }
    tracing::info!(
        event = "operator.retry",
        resume = word,
        msg = "retry at the parked stage"
    );
    run_series(driver, id)
}
