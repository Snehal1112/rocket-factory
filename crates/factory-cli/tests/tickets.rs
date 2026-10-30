mod common;
use std::sync::Arc;

use common::Fixture;
use factory_engine::Tracker;

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
    fn comment(&self, _: u32, _: &str) -> Result<(), String> {
        panic!("comment was called")
    }
    fn close(&self, _: u32) -> Result<(), String> {
        panic!("close was called")
    }
}

#[test]
fn tickets_on_an_unmerged_job_skips_without_calls() {
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
    let out = fx.run(&["tickets", "1"]).expect("tickets");
    assert!(
        out.contains("skipped") && out.contains("not merged"),
        "{out}"
    );
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
