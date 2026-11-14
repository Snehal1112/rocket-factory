mod common;
use common::{approved_job_with, scripted, Effect};
use factory_agent::AgentSpec;
use factory_core::{CoreError, Job, Layout, Stage};
use factory_engine::{
    create_job, merge_stage, resume_logged, run_job, BoardStore, EngineError, RunEnd,
};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const GOOD: &str =
    "---BEGIN MESSAGE---\nfeat(hello): add the hello docs\n\nBody.\n---END MESSAGE---";

/// Parks and cancels a job the way `factory cancel` does.
fn cancel_job(main: &Path, id: &str) {
    let store = BoardStore::new(&Layout::in_repo(main).board_file(), Duration::from_secs(5));
    store
        .update_job(id, |j| {
            j.set_stage(Stage::NeedsHuman, Some("cancelled: test"), false, 1.0)?;
            j.cancelled = true;
            Ok(())
        })
        .expect("cancel");
}

type Target = Arc<Mutex<Option<(PathBuf, String)>>>;

/// Cancels the job named in `target`, once.
fn cancel_target(target: &Target) {
    if let Some((main, id)) = target.lock().expect("target").take() {
        cancel_job(&main, &id);
    }
}

#[test]
fn a_cancelled_job_does_not_merge() {
    let fx = common::approved_job();
    let before = common::main_tip(&fx.main);
    cancel_job(&fx.main, &fx.id);
    let err = merge_stage(&fx.driver, &fx.id).expect_err("cancelled");
    assert!(matches!(err, EngineError::Cancelled), "{err}");
    assert_eq!(common::main_tip(&fx.main), before, "main is unchanged");
}

#[test]
fn a_cancel_during_the_commit_message_run_leaves_main_unchanged() {
    let target: Target = Arc::new(Mutex::new(None));
    let seen = target.clone();
    let effect: Effect = Box::new(move |_: &AgentSpec| {
        cancel_target(&seen);
        GOOD.to_string()
    });
    let fx = approved_job_with(Arc::new(scripted(vec![effect])), true);
    *target.lock().expect("target") = Some((fx.main.clone(), fx.id.clone()));
    let before = common::main_tip(&fx.main);
    let err = merge_stage(&fx.driver, &fx.id).expect_err("cancelled");
    assert!(matches!(err, EngineError::Cancelled), "{err}");
    assert_eq!(common::main_tip(&fx.main), before, "main is unchanged");
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.stage, Stage::NeedsHuman);
    assert!(job.cancelled);
}

#[test]
fn a_cancel_during_the_builder_run_ends_the_run_at_needs_human() {
    let target: Target = Arc::new(Mutex::new(None));
    let seen = target.clone();
    let hook: common::RunHook = Arc::new(move |_spec| cancel_target(&seen));
    let fx = common::SeriesFixture::owned("tester", hook);
    let plan = fx.plan_file("plan-01-a.md", 1, "none");
    let job = create_job(&fx.driver, "cancel", "one plan", Some(&plan), None).expect("create");
    *target.lock().expect("target") = Some((fx.main.clone(), job.id.clone()));
    let end = run_job(&fx.driver, &job.id).expect("run");
    match end {
        RunEnd::NeedsHuman(reason) => {
            assert!(reason.contains("cancelled: test"), "{reason}");
        }
        other => panic!("expected needs-human, got {other:?}"),
    }
    let after = fx.driver.job(&job.id).expect("job");
    assert_eq!(after.stage, Stage::NeedsHuman, "never review");
    assert!(after.cancelled);
}

#[test]
fn engine_error_cancelled_comes_from_core_error() {
    assert!(matches!(
        EngineError::from(CoreError::Cancelled),
        EngineError::Cancelled
    ));
    assert!(matches!(
        EngineError::from(CoreError::LedgerLine),
        EngineError::Core(_)
    ));
}

#[test]
fn resume_logged_logs_a_stage_change() {
    let mut job = Job::new("001", "d", "s", "2026-10-13", "f", 100.0);
    job.set_stage(Stage::NeedsHuman, Some("parked"), false, 1.0)
        .expect("park");
    resume_logged(&mut job, Stage::Build, 5.0).expect("resume");
    assert_eq!(job.stage, Stage::Build);
    let err = resume_logged(&mut job, Stage::Review, 6.0).expect_err("not parked");
    assert!(matches!(err, CoreError::StageMove { .. }), "{err}");
}
