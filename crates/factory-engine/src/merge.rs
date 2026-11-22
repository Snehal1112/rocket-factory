use factory_core::gate_record::{GateKey, GateResult};
use factory_core::{
    next_with_resume, park_summary, ParkKind, ParkPart, ParkRecord, ResumeAt, Stage,
};
use factory_git::{BaseState, MergeOutcome, Preflight};

use crate::gate_record::{fail_key, offers_approve, record_gate, GateTarget};
use crate::park::{evidence_or_reason, write_record};
use crate::tickets::TicketsReport;
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
    /// The merge sha and the reason the commit is unsigned, if it is.
    Merged(String, Option<String>),
    Park(String, ParkKind, Vec<ParkPart>, Option<ResumeAt>),
}

impl Step {
    /// A park of kind `Other` with no evidence of its own.
    fn plain(reason: impl Into<String>) -> Step {
        Step::Park(reason.into(), ParkKind::Other, Vec::new(), None)
    }

    /// A park of kind `Other` that `factory retry` resumes at `resume`.
    fn resume(reason: impl Into<String>, resume: ResumeAt) -> Step {
        Step::Park(reason.into(), ParkKind::Other, Vec::new(), Some(resume))
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
    park_with_parts_resume(driver, id, reason, kind, parts, kind.resume())
}

/// Parks like `park_with_parts`, and records the stage `factory retry` resumes at.
pub fn park_with_parts_resume(
    driver: &Driver,
    id: &str,
    reason: &str,
    kind: ParkKind,
    parts: Vec<ParkPart>,
    resume: Option<ResumeAt>,
) -> Result<(), EngineError> {
    park_with_parts_target(
        driver,
        id,
        reason,
        kind,
        parts,
        resume,
        GateTarget::FromKind,
    )
}

/// Parks like `park_with_parts_resume`, and writes the fail line under the gate `target` names.
pub fn park_with_parts_target(
    driver: &Driver,
    id: &str,
    reason: &str,
    kind: ParkKind,
    parts: Vec<ParkPart>,
    resume: Option<ResumeAt>,
    target: GateTarget,
) -> Result<(), EngineError> {
    let now = now_secs();
    let at = now.max(0.0) as u64;
    let parts = evidence_or_reason(parts, reason);
    let summary = park_summary(reason, &parts);
    let job = driver.store().update_job(id, |job| {
        // A park written earlier, such as a failed check in the sync, keeps its kind and evidence.
        let already = job.stage == Stage::NeedsHuman && job.park_kind.is_some();
        // The stage before the park picks the gate of a park of kind `Other`.
        let stage_before = job.stage;
        if !already {
            job.park_kind = Some(kind.as_str().to_string());
            job.park_at = Some(at);
            job.park_summary = Some(summary.clone());
            job.park_resume = resume;
        }
        crate::ports::set_stage_logged(job, Stage::NeedsHuman, Some(reason), false, now)?;
        Ok((job.clone(), already, stage_before))
    })?;
    let (job, already, stage_before) = job;
    // An earlier park already logged, wrote its record and ruled.
    if already {
        return Ok(());
    }
    crate::ports::log_park(id, reason);
    // Writing the record is best effort, the park already happened.
    let mut record = ParkRecord::new(kind, reason, id, job.round, at, parts);
    record.resume = resume;
    record.next = next_with_resume(
        kind,
        id,
        job.review_missing(),
        job.last_review.as_ref().map(|r| r.verdict.as_str()),
        resume,
    );
    write_record(&driver.job_dir(&job), &record);
    if let Some((key, result)) = fail_key(kind, Some(stage_before), target) {
        record_gate(driver, &job, key, result, reason);
    }
    if offers_approve(&record.next) {
        record_gate(driver, &job, GateKey::Approve, GateResult::Waiting, reason);
    }
    // The ledger takes one line, so the reason is folded into a single line.
    let one_line = reason.split_whitespace().collect::<Vec<_>>().join(" ");
    driver
        .ledger(&job)
        .ruling(&format!("parked in needs-human: {one_line}"))?;
    Ok(())
}

/// Sets the stage `factory retry` resumes at on a parked job, and rewrites `park.json` to match
/// with a recomputed `next`. The kind, reason, round and evidence of the record stay. A missing
/// record stays missing, because the board field is the source of truth.
pub(crate) fn set_park_resume(
    driver: &Driver,
    id: &str,
    resume: ResumeAt,
) -> Result<(), EngineError> {
    let job = driver.store().update_job(id, |job| {
        if job.stage == Stage::NeedsHuman {
            job.park_resume = Some(resume);
        }
        Ok(job.clone())
    })?;
    let dir = driver.job_dir(&job);
    if let Some(mut record) = ParkRecord::read(&dir) {
        record.resume = Some(resume);
        record.next = next_with_resume(
            record.kind,
            id,
            job.review_missing(),
            job.last_review.as_ref().map(|r| r.verdict.as_str()),
            Some(resume),
        );
        write_record(&dir, &record);
    }
    Ok(())
}

/// What went wrong in a soft step.
pub(crate) enum StepFailure {
    Error(EngineError),
    Panic(String),
}

impl std::fmt::Display for StepFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StepFailure::Error(err) => write!(f, "{err}"),
            StepFailure::Panic(text) => write!(f, "{text}"),
        }
    }
}

/// Runs one post-merge step and turns a panic into a `StepFailure::Panic` with the payload text.
pub(crate) fn run_soft_step<T>(
    f: impl FnOnce() -> Result<T, EngineError>,
) -> Result<T, StepFailure> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(result) => result.map_err(StepFailure::Error),
        Err(payload) => {
            let text = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_string());
            Err(StepFailure::Panic(text))
        }
    }
}

/// The ledger line for a failed soft step. An error of `kb_refresh` or `costs_refresh` gets none.
fn ledger_line(step: &str, failure: &StepFailure) -> Option<String> {
    match failure {
        StepFailure::Panic(text) => Some(format!("{step} panicked: {text}")),
        StepFailure::Error(err) if matches!(step, "tickets" | "waivers" | "minors") => {
            Some(format!("{step} error: {err}"))
        }
        StepFailure::Error(_) => None,
    }
}

/// Reports a failed soft step to stderr, and to the ledger and events when it has a ledger line.
fn report_failure(driver: &Driver, id: &str, step: &str, failure: &StepFailure) {
    tracing::error!(event = "step.error", step, msg = %format!("{step} error for job {id}: {failure}"));
    if let Some(line) = ledger_line(step, failure) {
        write_step_ledger(driver, id, step, &line);
        crate::tickets::notify_failure(driver, id, step, &failure.to_string());
    }
}

/// Reports the result of a step after the merge. A failure never changes the merge result.
fn report_soft_step(
    driver: &Driver,
    id: &str,
    step: &str,
    result: Result<TicketsReport, StepFailure>,
) {
    match result {
        Ok(report) if report.failed => {
            let text = report.lines.join("; ");
            tracing::error!(event = "step.error", step, msg = %format!("{step} error for job {id}: {text}"));
            crate::tickets::notify_failure(driver, id, step, &text);
        }
        Ok(_) => {}
        Err(failure) => report_failure(driver, id, step, &failure),
    }
}

/// Prints and logs an unsigned commit, and returns the ledger line for it.
pub(crate) fn warn_unsigned(kind: &str, sha: &str, reason: &str) -> String {
    let line = format!(
        "commit {} is unsigned: {reason}",
        crate::ports::short_sha(sha)
    );
    eprintln!("warning: {line}");
    tracing::warn!(event = "commit.unsigned", kind, sha, reason);
    line
}

/// Reports an unsigned commit to the ledger, stderr and the events. The job goes on.
pub(crate) fn report_unsigned(driver: &Driver, id: &str, kind: &str, sha: &str, reason: &str) {
    let line = warn_unsigned(kind, sha, reason);
    write_step_ledger(driver, id, "unsigned", &line);
}

/// Writes one folded line to the job's ledger. A missing job skips it and a failed write is only logged.
pub(crate) fn write_step_ledger(driver: &Driver, id: &str, step: &str, line: &str) {
    let Ok(job) = driver.job(id) else {
        return;
    };
    let one_line = line.split_whitespace().collect::<Vec<_>>().join(" ");
    if let Err(e) = driver.ledger(&job).ruling(&one_line) {
        tracing::error!(event = "step.error", step = %format!("{step}_ledger"), msg = %format!("{step} ledger error for job {id}: {e}"));
    }
}

/// Merges the reviewed head into main, syncing first when main moved. Any failure parks the job.
pub fn merge_stage(driver: &Driver, id: &str) -> Result<MergeResult, EngineError> {
    match try_merge(driver, id) {
        Ok(Step::Merged(sha, unsigned)) => {
            finish_merge(driver, id, &sha)?;
            tracing::info!(event = "merge.done", sha = %sha);
            // The merge lock was released when `try_merge` returned, so the reports go here.
            if let Some(reason) = &unsigned {
                report_unsigned(driver, id, "merge", &sha, reason);
            }
            record_merge_pass(driver, id, &sha, unsigned.as_deref());
            // The merge is done, so a failed refresh must not change the result.
            match run_soft_step(|| driver.kb_refresh(id, false)) {
                Ok(text) if text.starts_with("kb refresh failed") => {
                    tracing::error!(event = "step.error", step = "kb_refresh", msg = %format!("kb refresh error for job {id}: {text}"));
                }
                Ok(_) => {}
                Err(failure) => report_failure(driver, id, "kb_refresh", &failure),
            }
            // Ticket hygiene is best effort too: a failure is written to the ledger, the job output and the events, and never changes the merge.
            if driver.config().tickets_on_merge {
                report_soft_step(
                    driver,
                    id,
                    "tickets",
                    run_soft_step(|| crate::tickets::tickets_step(driver, id, false)),
                );
            }
            // Waiver tickets are filed whether or not tickets_on_merge is set, and fail the same soft way.
            report_soft_step(
                driver,
                id,
                "waivers",
                run_soft_step(|| crate::waivers::waiver_tickets_step(driver, id, false)),
            );
            // The minor ticket follows the same soft rule and has no series gate.
            report_soft_step(
                driver,
                id,
                "minors",
                run_soft_step(|| crate::minors::minor_tickets_step(driver, id, false)),
            );
            // The costs refresh runs last so that it counts the kb run of this job.
            if let Err(failure) = run_soft_step(|| crate::costs::refresh_costs(driver)) {
                report_failure(driver, id, "costs_refresh", &failure);
            }
            // The cleanup is the last soft step, and what it does never changes the merge result.
            if driver.config().cleanup_on_merge {
                crate::cleanup::cleanup_after_merge(driver, id);
            }
            Ok(MergeResult::Merged(sha))
        }
        // Every park here is a merge-step park, so its fail line goes under `merge`.
        Ok(Step::Park(reason, kind, parts, resume)) => {
            park_with_parts_target(
                driver,
                id,
                &reason,
                kind,
                parts,
                resume,
                GateTarget::Key(GateKey::Merge),
            )?;
            Ok(MergeResult::Parked(reason))
        }
        // A cancelled job stays as the cancel left it, and the run ends as needs-human.
        Err(EngineError::Cancelled) => Err(EngineError::Cancelled),
        Err(err) => {
            let reason = format!("merge failed: {err}");
            let parts = vec![ParkPart::new("Merge error", "git", &err.to_string())];
            // An automatic sync moves the head and the stage to Review, so that head needs a new review.
            let resume = match driver.job(id) {
                Ok(job) if job.stage != Stage::Approve => ResumeAt::Rereview,
                _ => ResumeAt::Merge,
            };
            park_with_parts_target(
                driver,
                id,
                &reason,
                ParkKind::Other,
                parts,
                Some(resume),
                GateTarget::Key(GateKey::Merge),
            )?;
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
            "another project is merging into this repository, run factory retry <id>".to_string(),
        ),
        Err(err) => Err(format!("cannot take the merge lock: {err}")),
    }
}

/// The approve command the park's `next` hint offers, without its note in brackets.
fn approve_form(job: &factory_core::Job, id: &str) -> String {
    ParkKind::Other
        .next_for(
            id,
            job.review_missing(),
            job.last_review.as_ref().map(|r| r.verdict.as_str()),
        )
        .into_iter()
        .find(|n| n.starts_with("factory approve "))
        .map(|n| match n.split_once(" (") {
            Some((cmd, _)) => cmd.to_string(),
            None => n,
        })
        .unwrap_or_else(|| format!("factory approve {id}"))
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
            BaseState::Merged => return Ok(Step::Merged(check.main_sha, None)),
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
                "reviewed head {head}\nbranch tip {tip}\n\nSomething was committed on the job branch after the review. Run factory retry {id} to review the new tip, or reset the branch to the reviewed head."
            );
            return Ok(Step::Park(
                format!("branch tip {tip} differs from the reviewed head {head}"),
                ParkKind::Other,
                vec![ParkPart::new("Branch moved", "driver", &text)],
                Some(ResumeAt::Rereview),
            ));
        }
        if let Preflight::Busy { .. } = git_op("preflight_main", || repo.preflight_main())? {
            // Another project may be mid-merge, which briefly dirties main. Look again under the lock.
            let lock = match take_merge_lock(driver) {
                Ok(lock) => lock,
                Err(reason) => return Ok(Step::resume(reason, ResumeAt::Merge)),
            };
            let again = git_op("preflight_main", || repo.preflight_main())?;
            drop(lock);
            if let Preflight::Busy { reason, paths } = again {
                let mut text = paths.join("\n");
                if !text.is_empty() {
                    text.push_str("\n\n");
                }
                let approve = approve_form(&job, id);
                text.push_str(&format!(
                    "Commit or stash these in the main checkout, then run {approve} again."
                ));
                if approve == format!("factory approve {id}") {
                    text.push_str(
                        " Plain approve retries the merge, because the last review still matches the head.",
                    );
                }
                return Ok(Step::Park(
                    format!("{main} busy: {reason}"),
                    ParkKind::Other,
                    vec![ParkPart::new("Main checkout", "git status", &text)],
                    Some(ResumeAt::Merge),
                ));
            }
        }
        if check.state == BaseState::MainMoved {
            if job.syncs >= MAX_SYNCS {
                return Ok(Step::Park(
                    format!("{main} moved again after {} syncs", job.syncs),
                    ParkKind::Other,
                    Vec::new(),
                    Some(ResumeAt::Sync),
                ));
            }
            match sync_stage(driver, id)? {
                SyncOutcome::Synced | SyncOutcome::UpToDate => continue,
                SyncOutcome::Parked(reason, _) => return Ok(Step::plain(reason)),
            }
        }
        let message = driver.commit_message(id, MessageKind::Merge)?;
        // Projects of one repo share main, so only one merges at a time. The lock is held until the
        // end of this iteration, and a merge never runs without it.
        let _merge_lock = match take_merge_lock(driver) {
            Ok(lock) => lock,
            Err(reason) => return Ok(Step::resume(reason, ResumeAt::Merge)),
        };
        // The message run can take a while, so main is checked again before the merge.
        let again = git_op("base_state", || repo.base_state(&base, &head))?;
        match again.state {
            BaseState::Merged => return Ok(Step::Merged(again.main_sha, None)),
            BaseState::BaseNotMain => {
                return Ok(Step::plain(format!(
                    "the job base is not on {main}, a human must look at the branch"
                )));
            }
            BaseState::MainMoved => continue,
            BaseState::Current => {}
        }
        // A cancel can land during the message run. Only the git merge is left after this check.
        if driver.job(id)?.cancelled {
            return Err(EngineError::Cancelled);
        }
        // The lease can be lost during the message run, and then another driver may own the project.
        if driver.lease_lost() {
            return Err(driver.lost_error());
        }
        return match git_op("merge_no_ff", || {
            repo.merge_no_ff(&job.branch, &head, &message)
        })? {
            MergeOutcome::Merged {
                merge_sha,
                unsigned,
            } => Ok(Step::Merged(merge_sha, unsigned)),
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
    Ok(Step::resume(
        format!("{main} moved again after the maximum number of syncs"),
        ResumeAt::Sync,
    ))
}

/// Records `merge: pass` with the short merge sha, then `approve: pass "retried"` when the last
/// `approve` line of this round is `waiting`. A job that cannot be read gets no line.
fn record_merge_pass(driver: &Driver, id: &str, sha: &str, unsigned: Option<&str>) {
    if !driver.config().gate_record {
        return;
    }
    let Ok(job) = driver.job(id) else {
        return;
    };
    let short = crate::ports::short_sha(sha);
    let detail = match unsigned {
        Some(reason) => format!("{short}, unsigned: {reason}"),
        None => short.to_string(),
    };
    record_gate(driver, &job, GateKey::Merge, GateResult::Pass, &detail);
    let round = job.round.max(1);
    let last = factory_core::gate_record::read(&driver.job_dir(&job))
        .into_iter()
        .rev()
        .find(|l| l.gate == GateKey::Approve && l.round == round);
    if last.is_some_and(|l| l.result == GateResult::Waiting) {
        record_gate(driver, &job, GateKey::Approve, GateResult::Pass, "retried");
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_soft_step_turns_every_payload_kind_into_text() {
        let a = run_soft_step::<()>(|| panic!("boom"));
        assert!(matches!(a, Err(StepFailure::Panic(ref t)) if t == "boom"));
        let b = run_soft_step::<()>(|| panic!("{}", String::from("owned")));
        assert!(matches!(b, Err(StepFailure::Panic(ref t)) if t == "owned"));
        let c = run_soft_step::<()>(|| std::panic::panic_any(7_u32));
        assert!(matches!(c, Err(StepFailure::Panic(ref t)) if t == "unknown panic"));
        let d = run_soft_step(|| Ok::<_, EngineError>(5));
        assert!(matches!(d, Ok(5)));
        let e = run_soft_step::<()>(|| Err(EngineError::Msg("bad".into())));
        assert!(matches!(e, Err(StepFailure::Error(_))));
    }

    #[test]
    fn a_kb_or_costs_error_gets_no_ledger_line() {
        for step in ["kb_refresh", "costs_refresh"] {
            let failure = StepFailure::Error(EngineError::Msg("bad".into()));
            assert_eq!(ledger_line(step, &failure), None);
        }
    }

    #[test]
    fn a_kb_or_costs_panic_gets_a_ledger_line() {
        let failure = StepFailure::Panic("boom".into());
        assert_eq!(
            ledger_line("kb_refresh", &failure).as_deref(),
            Some("kb_refresh panicked: boom")
        );
    }

    #[test]
    fn a_ticket_step_error_gets_a_ledger_line() {
        let failure = StepFailure::Error(EngineError::Msg("bad".into()));
        let line = ledger_line("tickets", &failure).expect("line");
        assert!(line.starts_with("tickets error: "), "{line}");
    }
}
