mod common;
use common::Fixture;
use factory_engine::{acquire_project_lease, now_secs};
use std::time::Duration;

fn hold_project(fx: &Fixture, owner: &str, now: f64) {
    let state = factory_core::Layout::in_repo(&fx.root).state_dir;
    acquire_project_lease(&state, "003", owner, 600.0, now, Duration::from_secs(5))
        .expect("project lease");
}

fn new_job(fx: &Fixture) {
    let plans = fx.write_plan("plan-01-hello.md", 1, "none");
    fx.run(&[
        "new",
        "hello",
        "say hello",
        "--plans",
        plans.to_str().expect("utf8"),
    ])
    .expect("new");
}

#[test]
fn run_is_refused_while_another_driver_holds_the_project() {
    let fx = Fixture::new();
    new_job(&fx);
    hold_project(&fx, "someone-else", now_secs());
    let before = fx.main_tip();
    let e = fx.run(&["run", "1"]).expect_err("busy");
    assert!(e.to_string().contains("project is busy"), "{e}");
    assert_eq!(fx.main_tip(), before);
    assert!(!fx.worktree_path(1).exists(), "no stage work ran");
    let status = fx.run(&["status"]).expect("status");
    assert!(
        status.starts_with("project: busy, job 003 (owner someone-else, "),
        "{status}"
    );
}

#[test]
fn approve_is_refused_while_another_driver_holds_the_project() {
    let fx = Fixture::with_risk_path("docs/**");
    new_job(&fx);
    assert!(fx
        .run(&["run", "1"])
        .expect("run")
        .starts_with("needs-human"));
    assert!(
        !fx.root.join(".superpowers/factory/project.lease").exists(),
        "run released the lease"
    );
    hold_project(&fx, "someone-else", now_secs());
    let before = fx.main_tip();
    let e = fx.run(&["approve", "1"]).expect_err("busy");
    assert!(e.to_string().contains("project is busy"), "{e}");
    assert_eq!(fx.main_tip(), before);
    assert!(fx
        .run(&["show", "1"])
        .expect("show")
        .contains("\"stage\": \"needs-human\""));
}

#[test]
fn approve_releases_the_project_lease_after_the_merge() {
    let fx = Fixture::with_risk_path("docs/**");
    new_job(&fx);
    fx.run(&["run", "1"]).expect("run");
    assert!(fx
        .run(&["approve", "1"])
        .expect("approve")
        .starts_with("merged"));
    assert_eq!(
        fx.run(&["status"]).expect("status").lines().next(),
        Some("project: free")
    );
}

#[test]
fn an_expired_lease_does_not_block_and_status_says_stale() {
    let fx = Fixture::new();
    new_job(&fx);
    hold_project(&fx, "gone", 0.0);
    let status = fx.run(&["status"]).expect("status");
    assert!(
        status.starts_with("project: stale lease from gone (job 003), parked"),
        "{status}"
    );
    assert_eq!(fx.run(&["run", "1"]).expect("run"), "merged");
    assert_eq!(
        fx.run(&["status"]).expect("status").lines().next(),
        Some("project: free")
    );
}

#[test]
fn a_corrupt_lease_file_is_reported_and_replaced() {
    let fx = Fixture::new();
    new_job(&fx);
    std::fs::write(fx.root.join(".superpowers/factory/project.lease"), b"").expect("write");
    let status = fx.run(&["status"]).expect("status");
    assert!(
        status.starts_with("project: lease file unreadable: "),
        "{status}"
    );
    assert_eq!(fx.run(&["run", "1"]).expect("run"), "merged");
}

#[test]
fn other_commands_are_not_blocked_by_a_held_lease() {
    let fx = Fixture::with_risk_path("docs/**");
    new_job(&fx);
    fx.run(&["run", "1"]).expect("run");
    hold_project(&fx, "someone-else", now_secs());
    fx.run(&["show", "1"]).expect("show");
    fx.run(&["tokens", "1"]).expect("tokens");
    fx.run(&["rework", "1", "try again"]).expect("rework");
    fx.run(&["cancel", "1"]).expect("cancel");
    fx.run(&["cleanup", "1", "--discard-commits"])
        .expect("cleanup");
    let plans = fx.write_plan("plan-01-other.md", 1, "none");
    fx.run(&[
        "new",
        "other",
        "more",
        "--plans",
        plans.to_str().expect("utf8"),
    ])
    .expect("new");
}

#[test]
fn cancel_force_clears_the_lease_of_that_job_only() {
    let fx = Fixture::with_risk_path("docs/**");
    new_job(&fx);
    let state = factory_core::Layout::in_repo(&fx.root).state_dir;
    let lease = fx.root.join(".superpowers/factory/project.lease");
    // A lease of another job stays.
    hold_project(&fx, "someone-else", now_secs());
    assert!(!fx
        .run(&["cancel", "1", "--force"])
        .expect("cancel")
        .contains("project lease released"));
    assert!(lease.exists());
    // The lease of the cancelled job goes, even for a foreign owner.
    std::fs::remove_file(&lease).expect("rm");
    acquire_project_lease(
        &state,
        "001",
        "driver-4294967000-1",
        600.0,
        now_secs(),
        Duration::from_secs(5),
    )
    .expect("lease");
    let out = fx.run(&["cancel", "1", "--force"]).expect("cancel");
    assert!(out.contains("project lease released"), "{out}");
    assert!(!lease.exists());
}

#[test]
fn a_dead_drivers_lease_is_stale_in_status_and_does_not_block_run() {
    let fx = Fixture::new();
    new_job(&fx);
    let state = factory_core::Layout::in_repo(&fx.root).state_dir;
    acquire_project_lease(
        &state,
        "001",
        "driver-4294967000-1",
        100_000.0,
        now_secs(),
        Duration::from_secs(5),
    )
    .expect("lease");
    let status = fx.run(&["status"]).expect("status");
    assert!(
        status.starts_with("project: stale lease from driver-4294967000-1"),
        "{status}"
    );
    assert_eq!(fx.run(&["run", "1"]).expect("run"), "merged");
}

#[test]
fn kb_refresh_is_refused_while_another_driver_holds_the_project() {
    let fx = Fixture::new();
    new_job(&fx);
    assert_eq!(fx.run(&["run", "1"]).expect("run"), "merged");
    hold_project(&fx, "someone-else", now_secs());
    let e = fx.run(&["kb-refresh", "--job", "1"]).expect_err("busy");
    assert!(e.to_string().contains("project is busy"), "{e}");
    let state = factory_core::Layout::in_repo(&fx.root).state_dir;
    factory_engine::force_release_project_lease(&state, "003", Duration::from_secs(5))
        .expect("release");
    assert_eq!(
        fx.run(&["kb-refresh", "--job", "1"]).expect("free"),
        "kb_path is not set"
    );
    assert_eq!(
        fx.run(&["status"]).expect("status").lines().next(),
        Some("project: free")
    );
}
