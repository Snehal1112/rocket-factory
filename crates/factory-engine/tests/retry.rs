mod common;

use common::{
    approved_job_dyn, approving_runner, driver_for, scripted, Effect, MergeFixture, RoleFailing,
    ScriptedRunner,
};
use factory_agent::{AgentSpec, StopReason};
use factory_core::{CheckRule, Config, LastReview, ResumeAt, Stage};
use factory_engine::{
    read_waivers, record_waiver, retry_job, run_job, run_series, sync_requested_by, EngineError,
    RunEnd, SyncCaller, SyncOutcome, Waiver, MAX_SYNCS,
};
use std::sync::atomic::{AtomicUsize, Ordering};
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
fn failed_review_fixture(inner: Arc<ScriptedRunner>) -> MergeFixture {
    let failing = RoleFailing::new("reviewer", StopReason::Failed, 1, inner.clone());
    let fx = approved_job_dyn(Arc::new(failing), inner, Config::default());
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
    let fx = failed_review_fixture(approving_runner(2));
    let before = fx.driver.job(&fx.id).expect("job");
    let end = retry_job(&fx.driver, &fx.id).expect("retry");
    assert_eq!(end, RunEnd::Merged);
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.round, before.round);
    // The retry reviews the head recorded at the park, it does not build a new one.
    assert!(before.head.is_some());
    assert_eq!(job.head, before.head);
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
    let inner = Arc::new(scripted(vec![reviewer_changes, builder, reviewer_ok]));
    let fx = failed_review_fixture(inner);
    let end = retry_job(&fx.driver, &fx.id).expect("retry");
    assert_eq!(end, RunEnd::Merged, "{}", ledger(&fx));
    assert!(ledger(&fx).contains("fix round 2"), "{}", ledger(&fx));
    assert_eq!(fx.runner.specs_for("builder").len(), 1);
}

#[test]
fn retry_after_no_checks_runs_the_checks() {
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
    let fx = approved_job_dyn(runner.clone(), runner.clone(), uncovered);
    force_stage(&fx, Stage::Review);
    let end = run_job(&fx.driver, &fx.id).expect("run");
    assert!(matches!(end, RunEnd::NeedsHuman(_)), "{end:?}");
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.park_kind.as_deref(), Some("no-checks"));
    assert_eq!(job.park_resume, Some(ResumeAt::Review));
    // A second driver on the same repo maps the changed path, as a config fix would.
    let with_mapping = Config {
        checks: vec![CheckRule {
            paths: vec!["docs/**".into()],
            commands: vec!["true".into()],
            ..CheckRule::default()
        }],
        code_paths: vec!["**".into()],
        ..Config::default()
    };
    let second = driver_for(&fx.main, runner, with_mapping);
    let end = retry_job(&second, &fx.id).expect("retry");
    assert_eq!(end, RunEnd::Merged);
    // The mapped check ran and left its output file.
    let job = fx.driver.job(&fx.id).expect("job");
    assert!(fx.driver.job_dir(&job).join("checks.md").exists());
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

fn remove_park_file(fx: &MergeFixture) {
    let job = fx.driver.job(&fx.id).expect("job");
    let path = fx.driver.job_dir(&job).join(factory_core::PARK_FILE);
    std::fs::remove_file(&path).expect("remove park.json");
}

#[test]
fn retry_still_resumes_when_park_json_is_missing() {
    let fx = common::approved_job();
    dirty_main_park(&fx);
    // Retry reads the resume stage from the board, so the park file is not needed.
    remove_park_file(&fx);
    let end = retry_job(&fx.driver, &fx.id).expect("retry");
    assert_eq!(end, RunEnd::Merged);
    assert!(ledger(&fx).contains("retry at merge"), "{}", ledger(&fx));
}

#[test]
fn retry_without_resume_or_park_json_refuses_with_the_kind_commands() {
    let fx = common::approved_job();
    park(&fx, factory_core::ParkKind::NothingChanged, None);
    remove_park_file(&fx);
    let text = refusal(&fx);
    assert!(text.contains("no stage to retry at"), "{text}");
    assert!(text.contains("factory rework 001 <note>"), "{text}");
    assert!(text.contains("factory cancel 001"), "{text}");
    assert_eq!(fx.driver.job(&fx.id).expect("job").stage, Stage::NeedsHuman);
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

const SECURITY_OK: &str = "No findings.\nVERDICT: APPROVE";

/// Gives the driver a security agent, or the missing one parks the job as `review-failed`.
fn with_security_agent(mut fx: MergeFixture) -> MergeFixture {
    fx.driver = fx.driver.with_agent_files(vec![factory_core::AgentFile {
        name: "factory-security-reviewer.md".to_string(),
        content: "agent".to_string(),
    }]);
    fx
}

/// A runner that answers by role. The first reviewer run replies `first`, later ones approve.
fn role_runner(first: &'static str) -> Arc<ScriptedRunner> {
    let reviews = Arc::new(AtomicUsize::new(0));
    let effects: Vec<Effect> = (0..16)
        .map(|_| {
            let reviews = reviews.clone();
            Box::new(move |spec: &AgentSpec| match spec.role.as_str() {
                "builder" => {
                    std::fs::write(spec.cwd.join("docs/fixed.txt"), "fixed\n").expect("write");
                    "done".to_string()
                }
                "security-review" => SECURITY_OK.to_string(),
                _ if reviews.fetch_add(1, Ordering::SeqCst) == 0 => first.to_string(),
                _ => "ok\nVERDICT: APPROVE".to_string(),
            }) as Effect
        })
        .collect();
    Arc::new(scripted(effects))
}

/// A job parked at `approve` because the branch moved after the review.
fn tip_moved_fixture(first: &'static str) -> (MergeFixture, String) {
    let runner = role_runner(first);
    let fx = with_security_agent(approved_job_dyn(runner.clone(), runner, Config::default()));
    let tip = common::commit_file(&common::worktree_of(&fx), "extra.md", "e");
    let end = run_job(&fx.driver, &fx.id).expect("run");
    assert!(matches!(end, RunEnd::NeedsHuman(_)), "{end:?}");
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.park_resume, Some(ResumeAt::Rereview));
    (fx, tip)
}

#[test]
fn a_rereview_retry_reviews_the_whole_branch_at_the_new_tip() {
    let (fx, tip) = tip_moved_fixture("ok\nVERDICT: APPROVE");
    let end = retry_job(&fx.driver, &fx.id).expect("retry");
    assert_eq!(end, RunEnd::Merged, "{}", ledger(&fx));
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.head.as_deref(), Some(tip.as_str()));
    assert_eq!(job.prev_head, None);
    assert_eq!(fx.runner.specs_for("security-review").len(), 1);
    assert!(ledger(&fx).contains("retry at rereview"), "{}", ledger(&fx));
}

#[test]
fn a_rereview_retry_after_a_failed_sync_review_reviews_again() {
    let runner = role_runner("ok\nVERDICT: APPROVE");
    let failing = RoleFailing::new("reviewer", StopReason::Failed, 1, runner.clone());
    let fx = with_security_agent(approved_job_dyn(
        Arc::new(failing),
        runner,
        Config::default(),
    ));
    common::commit_file(&fx.main, "other.md", "o");
    let end = run_job(&fx.driver, &fx.id).expect("run");
    assert!(matches!(end, RunEnd::NeedsHuman(_)), "{end:?}");
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.park_kind.as_deref(), Some("review-failed"));
    assert_eq!(job.park_resume, Some(ResumeAt::Rereview));
    // The sync already moved the head to the tip, and main has not moved since.
    assert_eq!(job.head.as_deref(), Some(common::branch_tip(&fx).as_str()));
    let end = retry_job(&fx.driver, &fx.id).expect("retry");
    assert_eq!(end, RunEnd::Merged, "{}", ledger(&fx));
    assert_eq!(fx.runner.specs_for("security-review").len(), 1);
}

#[test]
fn a_rereview_retry_does_not_repark_after_a_passed_review() {
    let (fx, _) = tip_moved_fixture("ok\nVERDICT: APPROVE");
    let outcome = sync_requested_by(&fx.driver, &fx.id, SyncCaller::Retry).expect("sync");
    assert_eq!(outcome, SyncOutcome::Synced);
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.stage, Stage::Approve);
    assert_eq!(job.approve_refusal(false, false), None);
}

#[test]
fn a_rereview_retry_with_changes_starts_a_fix_round() {
    let (fx, _) = tip_moved_fixture("- [blocker] fix me\nVERDICT: CHANGES");
    let end = retry_job(&fx.driver, &fx.id).expect("retry");
    assert_eq!(end, RunEnd::Merged, "{}", ledger(&fx));
    assert_eq!(fx.runner.specs_for("builder").len(), 1);
    assert!(ledger(&fx).contains("fix round"), "{}", ledger(&fx));
}

/// A fixture parked at rereview whose only check is `command`, with a runner that answers by role.
fn checked_rereview_fixture(command: &str) -> MergeFixture {
    let config = Config {
        checks: vec![CheckRule {
            paths: vec!["**".into()],
            commands: vec![command.to_string()],
            ..CheckRule::default()
        }],
        ..Config::default()
    };
    let runner = role_runner("ok\nVERDICT: APPROVE");
    let fx = with_security_agent(approved_job_dyn(runner.clone(), runner, config));
    park(&fx, factory_core::ParkKind::Other, Some(ResumeAt::Rereview));
    fx
}

#[test]
fn a_rereview_retry_with_failing_first_checks_starts_a_fix_round() {
    let fx = checked_rereview_fixture("false");
    let outcome = sync_requested_by(&fx.driver, &fx.id, SyncCaller::Retry).expect("sync");
    assert_eq!(outcome, SyncOutcome::Synced);
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.stage, Stage::Build);
    assert_eq!(job.round, 2, "{}", ledger(&fx));
}

#[test]
fn a_rereview_retry_with_failing_final_checks_starts_a_fix_round() {
    let marker = tempfile::tempdir().expect("marker dir");
    let file = marker.path().join("ran-once");
    // The first run passes and leaves the marker, so the final run fails.
    let script = marker.path().join("check.sh");
    std::fs::write(
        &script,
        format!(
            "if [ -f {0} ]; then exit 1; else touch {0}; fi\n",
            file.display()
        ),
    )
    .expect("script");
    let fx = checked_rereview_fixture(&format!("sh {}", script.display()));
    let outcome = sync_requested_by(&fx.driver, &fx.id, SyncCaller::Retry).expect("sync");
    assert_eq!(outcome, SyncOutcome::Synced);
    assert!(file.exists());
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.stage, Stage::Build);
    assert_eq!(job.round, 2, "{}", ledger(&fx));
}

#[test]
fn a_sync_retry_runs_one_more_sync_and_merges() {
    let runner = role_runner("ok\nVERDICT: APPROVE");
    let fx = approved_job_dyn(runner.clone(), runner, Config::default());
    fx.driver
        .store()
        .update_job(&fx.id, |j| {
            j.syncs = MAX_SYNCS;
            Ok(())
        })
        .expect("syncs");
    common::commit_file(&fx.main, "other.md", "o");
    let end = run_job(&fx.driver, &fx.id).expect("run");
    assert!(matches!(end, RunEnd::NeedsHuman(_)), "{end:?}");
    assert_eq!(
        fx.driver.job(&fx.id).expect("job").park_resume,
        Some(ResumeAt::Sync)
    );
    let end = retry_job(&fx.driver, &fx.id).expect("retry");
    assert_eq!(end, RunEnd::Merged, "{}", ledger(&fx));
    assert_eq!(fx.driver.job(&fx.id).expect("job").syncs, MAX_SYNCS);
    assert_eq!(fx.runner.specs_for("reviewer").len(), 1);
    assert!(ledger(&fx).contains("retry at sync"), "{}", ledger(&fx));
}

#[test]
fn a_retry_that_merges_continues_the_series() {
    let fx = common::SeriesFixture::new("fine\nVERDICT: APPROVE");
    let a = fx.plan_file("plan-01-a.md", 1, "plan-02-b.md");
    fx.plan_file("plan-02-b.md", 1, "none");
    let job = factory_engine::create_job(&fx.driver, "series", "two plans", Some(&a), None)
        .expect("create");
    std::fs::write(fx.main.join("README.md"), "dirty").expect("dirty");
    let end = run_series(&fx.driver, &job.id).expect("series");
    assert!(matches!(end, RunEnd::NeedsHuman(_)), "{end:?}");
    assert_eq!(
        fx.driver.job(&job.id).expect("job").park_resume,
        Some(ResumeAt::Merge)
    );
    std::fs::write(fx.main.join("README.md"), "hello\n").expect("clean");
    let end = retry_job(&fx.driver, &job.id).expect("retry");
    assert_eq!(end, RunEnd::Merged);
    let board = fx.driver.store().load().expect("board");
    assert_eq!(board.jobs.len(), 2);
    assert_eq!(board.jobs[0].stage, Stage::Merged);
    assert_ne!(board.jobs[1].stage, Stage::Spec);
}

#[test]
fn a_review_retry_with_failing_checks_starts_a_fix_round() {
    let config = Config {
        checks: vec![CheckRule {
            paths: vec!["**".into()],
            commands: vec!["test -f docs/fixed.txt".into()],
            ..CheckRule::default()
        }],
        ..Config::default()
    };
    let runner = role_runner("ok\nVERDICT: APPROVE");
    let fx = approved_job_dyn(runner.clone(), runner, config);
    force_stage(&fx, Stage::Review);
    park(
        &fx,
        factory_core::ParkKind::ReviewFailed,
        Some(ResumeAt::Review),
    );
    let end = retry_job(&fx.driver, &fx.id).expect("retry");
    assert_eq!(end, RunEnd::Merged, "{}", ledger(&fx));
    assert_eq!(fx.runner.specs_for("builder").len(), 1);
    assert!(ledger(&fx).contains("fix round"), "{}", ledger(&fx));
}
