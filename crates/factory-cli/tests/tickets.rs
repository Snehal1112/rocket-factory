mod common;
use std::sync::Arc;
use std::time::Duration;

use common::Fixture;
use factory_engine::{IssueInfo, Tracker};

/// A tracker that fails the test when any method is called.
struct NoCalls;

impl Tracker for NoCalls {
    fn repo_name(&self) -> Result<String, String> {
        panic!("repo_name was called")
    }
    fn labels_of(&self, _: u32) -> Result<Vec<String>, String> {
        panic!("labels_of was called")
    }
    fn is_closed(&self, _: u32) -> Result<bool, String> {
        panic!("is_closed was called")
    }
    fn create(&self, _: &str, _: &str, _: &[String]) -> Result<u32, String> {
        panic!("create was called")
    }
    fn find_created(&self, _: &str, _: &str) -> Result<Option<u32>, String> {
        panic!("find_created was called")
    }
    fn comment(&self, _: u32, _: &str) -> Result<(), String> {
        panic!("comment was called")
    }
    fn close(&self, _: u32) -> Result<(), String> {
        panic!("close was called")
    }
    fn list_labelled(&self, _: &str) -> Result<Vec<IssueInfo>, String> {
        Err("not used".into())
    }
    fn labelled_by(&self, _: u32, _: &str) -> Result<Option<factory_engine::LabelEvent>, String> {
        Err("not used".into())
    }
    fn edit_labels(&self, _: u32, _: &[String], _: &[String]) -> Result<(), String> {
        Err("not used".into())
    }
    fn current_user(&self) -> Result<String, String> {
        Err("not used".into())
    }
}

#[test]
fn tickets_on_an_unmerged_job_skips_without_calls() {
    let fx = unmerged_job();
    let out = fx.run(&["tickets", "1"]).expect("tickets");
    assert!(
        out.contains("skipped") && out.contains("not merged"),
        "{out}"
    );
}

/// A fixture with an unmerged job `001` and the `NoCalls` tracker.
fn unmerged_job() -> Fixture {
    let mut fx = Fixture::new();
    let plans = fx.write_plan("plan-01-hello.md", 1, "none");
    fx.run(&[
        "new",
        "hello",
        "work on #76",
        "--plans",
        plans.to_str().expect("utf8"),
    ])
    .expect("new");
    fx.ctx = fx.ctx.clone().with_tracker(Arc::new(NoCalls));
    fx
}

fn job_lease(fx: &Fixture) -> Option<factory_engine::Lease> {
    let store = factory_engine::BoardStore::new(
        &factory_core::Layout::in_repo(&fx.root).board_file(),
        Duration::from_secs(15),
    );
    store
        .load()
        .expect("load board")
        .jobs
        .into_iter()
        .find(|j| j.id == "001")
        .expect("job 001")
        .lease
}

#[test]
fn tickets_dry_run_is_refused_by_another_owners_live_lease() {
    let fx = unmerged_job();
    fx.hold_lease("001", "someone-else");
    let err = fx.run(&["tickets", "1", "--dry-run"]).expect_err("refused");
    assert!(err.to_string().contains("leased by someone-else"), "{err}");
}

#[test]
fn tickets_refuses_while_another_owner_holds_the_job_lease() {
    let fx = unmerged_job();
    fx.hold_lease("001", "someone-else");
    let err = fx.run(&["tickets", "1"]).expect_err("refused");
    assert!(err.to_string().contains("leased by someone-else"), "{err}");
}

#[test]
fn tickets_releases_its_lease_after_a_run() {
    let fx = unmerged_job();
    fx.run(&["tickets", "1"]).expect("tickets");
    assert!(job_lease(&fx).is_none());
}

#[test]
fn tickets_runs_while_the_project_is_busy() {
    let fx = unmerged_job();
    let state = factory_core::Layout::in_repo(&fx.root).state_dir;
    factory_engine::acquire_project_lease(
        &state,
        "003",
        "someone-else",
        600.0,
        factory_engine::now_secs(),
        Duration::from_secs(5),
    )
    .expect("project lease");
    let out = fx.run(&["tickets", "1"]).expect("tickets");
    assert!(out.contains("skipped"), "{out}");
}

#[test]
fn tickets_prints_no_waiver_line_without_waivers() {
    let mut fx = Fixture::new();
    let plans = fx.write_plan("plan-01-hello.md", 1, "none");
    fx.run(&[
        "new",
        "hello",
        "say hello",
        "--plans",
        plans.to_str().expect("utf8"),
    ])
    .expect("new");
    assert_eq!(fx.run(&["run", "1"]).expect("run"), "merged");
    fx.ctx = fx.ctx.clone().with_tracker(Arc::new(NoCalls));
    let out = fx.run(&["tickets", "1"]).expect("tickets");
    assert!(!out.contains("waiver"), "{out}");
    assert!(!out.contains("\n\n"), "{out}");
}
