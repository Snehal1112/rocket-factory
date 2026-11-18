mod common;

use common::{
    approved_job_dyn, approving_runner, driver_for, scripted_auto, Effect, MergeFixture,
    RoleFailing, ScriptedRunner,
};
use factory_agent::{AgentSpec, StopReason};
use factory_core::{CheckRule, Config, LastReview, ResumeAt, Stage};
use factory_engine::{
    read_waivers, record_waiver, retry_job, run_job, EngineError, RunEnd, Waiver,
};
use std::sync::Arc;

fn ledger(fx: &MergeFixture) -> String {
    let job = fx.driver.job(&fx.id).expect("job");
    std::fs::read_to_string(fx.driver.job_dir(&job).join("progress.md")).expect("ledger")
}

fn force_stage(fx: &MergeFixture, to: Stage) {
    fx.driver
        .store()
        .update_job(&fx.id, |j| j.set_stage(to, None, true, 2.0))
        .expect("stage");
}

fn park(fx: &MergeFixture, kind: factory_core::ParkKind, resume: Option<ResumeAt>) {
    factory_engine::park_with_parts_resume(
        &fx.driver,
        &fx.id,
        "parked for the test",
        kind,
        Vec::new(),
        resume,
    )
    .expect("park");
}

/// A fixture parked as `review-failed` after one failed reviewer run. `inner` answers the retry.
fn failed_review_fixture(inner: Arc<ScriptedRunner>, with_context: bool) -> MergeFixture {
    let failing = RoleFailing::new("reviewer", StopReason::Failed, 1, inner.clone());
    let fx = approved_job_dyn(Arc::new(failing), inner, with_context, Config::default());
    force_stage(&fx, Stage::Review);
    let end = run_job(&fx.driver, &fx.id).expect("run");
    assert!(matches!(end, RunEnd::NeedsHuman(_)), "{end:?}");
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.park_kind.as_deref(), Some("review-failed"));
    assert_eq!(job.park_resume, Some(ResumeAt::Review));
    fx
}

#[test]
fn retry_after_a_failed_review_reviews_the_same_head() {
    let fx = failed_review_fixture(approving_runner(2), false);
    let before = fx.driver.job(&fx.id).expect("job");
    let end = retry_job(&fx.driver, &fx.id).expect("retry");
    assert_eq!(end, RunEnd::Merged);
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.round, before.round);
    assert!(fx.runner.specs_for("builder").is_empty());
    assert!(ledger(&fx).contains("retry at review"), "{}", ledger(&fx));
}

#[test]
fn a_review_retry_with_changes_starts_a_fix_round() {
    let reviewer_changes: Effect =
        Box::new(|_: &AgentSpec| "- [blocker] fix me\nVERDICT: CHANGES".to_string());
    let builder: Effect = Box::new(|spec: &AgentSpec| {
        std::fs::write(spec.cwd.join("docs/fix.txt"), "fixed\n").expect("write");
        "done".to_string()
    });
    let reviewer_ok: Effect = Box::new(|_: &AgentSpec| "ok\nVERDICT: APPROVE".to_string());
    let inner = Arc::new(scripted_auto(vec![reviewer_changes, builder, reviewer_ok]));
    let fx = failed_review_fixture(inner, true);
    let end = retry_job(&fx.driver, &fx.id).expect("retry");
    assert_eq!(end, RunEnd::Merged, "{}", ledger(&fx));
    assert!(ledger(&fx).contains("fix round 2"), "{}", ledger(&fx));
    assert_eq!(fx.runner.specs_for("builder").len(), 1);
}

#[test]
fn retry_after_no_checks_runs_the_checks() {
    let covering = Config {
        checks: vec![CheckRule {
            paths: vec!["src/**".into()],
            commands: vec!["true".into()],
            ..CheckRule::default()
        }],
        code_paths: vec!["**".into()],
        ..Config::default()
    };
    let uncovered = Config {
        checks: vec![CheckRule {
            paths: vec!["nothing/**".into()],
            commands: vec!["true".into()],
            ..CheckRule::default()
        }],
        code_paths: vec!["**".into()],
        ..Config::default()
    };
    let runner = approving_runner(3);
    let fx = approved_job_dyn(runner.clone(), runner.clone(), true, uncovered);
    force_stage(&fx, Stage::Review);
    let end = run_job(&fx.driver, &fx.id).expect("run");
    assert!(matches!(end, RunEnd::NeedsHuman(_)), "{end:?}");
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.park_kind.as_deref(), Some("no-checks"));
    assert_eq!(job.park_resume, Some(ResumeAt::Review));
    // A second driver on the same repo maps the changed path, as a config fix would.
    let mut with_mapping = covering;
    with_mapping.checks[0].paths = vec!["docs/**".into()];
    let second = driver_for(&fx.main, runner, with_mapping);
    let end = retry_job(&second, &fx.id).expect("retry");
    assert_eq!(end, RunEnd::Merged);
}

#[test]
fn a_merge_retry_merges_once_main_is_clean() {
    let fx = common::approved_job();
    std::fs::write(fx.main.join("README.md"), "dirty").expect("write");
    let end = run_job(&fx.driver, &fx.id).expect("run");
    assert!(matches!(end, RunEnd::NeedsHuman(_)), "{end:?}");
    assert_eq!(
        fx.driver.job(&fx.id).expect("job").park_resume,
        Some(ResumeAt::Merge)
    );
    std::fs::write(fx.main.join("README.md"), "hello\n").expect("clean");
    let end = retry_job(&fx.driver, &fx.id).expect("retry");
    assert_eq!(end, RunEnd::Merged);
    assert!(ledger(&fx).contains("retry at merge"), "{}", ledger(&fx));
}

#[test]
fn a_merge_retry_on_dirty_main_parks_again() {
    let fx = common::approved_job();
    std::fs::write(fx.main.join("README.md"), "dirty").expect("write");
    run_job(&fx.driver, &fx.id).expect("run");
    let first = fx.driver.job(&fx.id).expect("job").reason;
    let end = retry_job(&fx.driver, &fx.id).expect("retry");
    let RunEnd::NeedsHuman(reason) = end else {
        panic!("expected a park, got {end:?}");
    };
    assert!(reason.starts_with("main busy"), "{reason}");
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.stage, Stage::NeedsHuman);
    assert_eq!(job.park_resume, Some(ResumeAt::Merge));
    assert_eq!(job.reason, first);
}

/// Records a last review that was skipped, so the job has no verdict to approve on.
fn without_review(fx: &MergeFixture) {
    fx.driver
        .store()
        .update_job(&fx.id, |j| {
            j.reviews.clear();
            j.last_review = None;
            Ok(())
        })
        .expect("clear reviews");
}

fn dirty_main_park(fx: &MergeFixture) {
    std::fs::write(fx.main.join("README.md"), "dirty").expect("write");
    run_job(&fx.driver, &fx.id).expect("run");
    assert_eq!(
        fx.driver.job(&fx.id).expect("job").park_resume,
        Some(ResumeAt::Merge)
    );
    std::fs::write(fx.main.join("README.md"), "hello\n").expect("clean");
}

#[test]
fn a_merge_retry_keeps_an_earlier_without_review_approval() {
    let fx = common::approved_job();
    without_review(&fx);
    dirty_main_park(&fx);
    // A plain approve would refuse this job, but the earlier approval carries over.
    let job = fx.driver.job(&fx.id).expect("job");
    assert!(job.approve_refusal(false, false).is_some());
    let end = retry_job(&fx.driver, &fx.id).expect("retry");
    assert_eq!(end, RunEnd::Merged);
}

#[test]
fn a_merge_retry_keeps_an_earlier_waive_approval() {
    let tracker = common::FakeTracker::new("o/r");
    let fx = common::with_tracker(common::approved_job(), tracker.clone());
    fx.driver
        .store()
        .update_job(&fx.id, |j| {
            j.last_review = Some(LastReview {
                round: 1,
                verdict: "changes".into(),
                blockers: 1,
                file: "review-code-r1.md".into(),
                source: "build".into(),
            });
            j.set_review("code", factory_core::ReviewState::Changes);
            Ok(())
        })
        .expect("last review");
    let dir = fx.driver.job_dir(&fx.driver.job(&fx.id).expect("job"));
    record_waiver(
        &dir,
        Waiver {
            head: fx.head.clone(),
            round: 1,
            file: "review-code-r1.md".into(),
            reason: "fine for now".into(),
            at: 10,
            blockers: vec!["- [blocker] one".into()],
            tickets: vec![None],
            superseded: false,
        },
    )
    .expect("waiver");
    dirty_main_park(&fx);
    let end = retry_job(&fx.driver, &fx.id).expect("retry");
    assert_eq!(end, RunEnd::Merged);
    let waivers = read_waivers(&dir);
    assert_eq!(waivers.len(), 1);
    assert!(!waivers[0].superseded);
    assert_eq!(tracker.count("create Waived review finding from job"), 1);
}

#[test]
fn a_merge_retry_from_review_asks_for_the_approval_again() {
    let fx = common::approved_job();
    without_review(&fx);
    force_stage(&fx, Stage::Review);
    park(&fx, factory_core::ParkKind::Other, Some(ResumeAt::Merge));
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.stage_before_park(), Some(Stage::Review));
    let err = retry_job(&fx.driver, &fx.id).expect_err("refused");
    let EngineError::Refused(text) = err else {
        panic!("expected a refusal, got {err:?}");
    };
    assert!(text.contains("no code review"), "{text}");
    assert!(text.contains("factory rework 001 --force <note>"), "{text}");
    assert_eq!(fx.driver.job(&fx.id).expect("job").stage, Stage::NeedsHuman);
}

fn refusal(fx: &MergeFixture) -> String {
    match retry_job(&fx.driver, &fx.id) {
        Err(EngineError::Refused(text)) => text,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn retry_refuses_a_park_without_resume() {
    let fx = common::approved_job();
    park(&fx, factory_core::ParkKind::NothingChanged, None);
    let text = refusal(&fx);
    assert!(text.contains("factory rework 001 <note>"), "{text}");
    assert!(text.contains("factory cancel 001"), "{text}");
}

#[test]
fn retry_refuses_a_builder_park() {
    let fx = common::approved_job();
    park(&fx, factory_core::ParkKind::ChecksFailed, None);
    let text = refusal(&fx);
    assert!(text.contains("factory rework 001 <note>"), "{text}");
}

#[test]
fn retry_refuses_a_cancelled_job() {
    let fx = common::approved_job();
    park(
        &fx,
        factory_core::ParkKind::ReviewFailed,
        Some(ResumeAt::Review),
    );
    fx.driver
        .store()
        .update_job(&fx.id, |j| {
            j.cancelled = true;
            Ok(())
        })
        .expect("cancel");
    assert!(refusal(&fx).contains("cancelled"));
    assert_eq!(fx.driver.job(&fx.id).expect("job").stage, Stage::NeedsHuman);
}

#[test]
fn retry_refuses_outside_needs_human() {
    let fx = common::approved_job();
    let text = refusal(&fx);
    assert!(text.contains("stage approve"), "{text}");
}

#[test]
fn retry_refuses_a_pending_batch_commit() {
    let fx = common::approved_job();
    park(
        &fx,
        factory_core::ParkKind::ReviewFailed,
        Some(ResumeAt::Review),
    );
    fx.driver
        .store()
        .update_job(&fx.id, |j| {
            j.batch_snapshot = Some("abc123".into());
            Ok(())
        })
        .expect("snapshot");
    assert!(refusal(&fx).contains("pending batch commit"));
}
