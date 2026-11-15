#![cfg(target_os = "linux")]

mod common;

use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use factory_engine::procinfo::process_start_ms;
use factory_engine::{create_job, now_secs, run_job, ProjectLease};
use factory_procgroup::GroupChild;

/// A pid no real process has, so the owner counts as dead.
const DEAD_OWNER: &str = "driver-4294967000-1";

fn fixture() -> common::SeriesFixture {
    common::SeriesFixture::owned("a", Arc::new(|_| {}))
}

/// A real group leader that sleeps. The caller keeps the child, which kills the group on drop.
fn sleeper() -> GroupChild {
    factory_procgroup::spawn(Command::new("sleep").arg("60")).expect("spawn sleeper")
}

fn write_lease(fx: &common::SeriesFixture, owner: &str, expires: f64) {
    let lease = ProjectLease {
        owner: owner.to_string(),
        job: "009".to_string(),
        expires,
    };
    let text = serde_json::to_string(&lease).expect("lease json");
    std::fs::create_dir_all(&fx.driver.layout().state_dir).expect("state dir");
    std::fs::write(fx.driver.layout().project_lease_file(), text).expect("write lease");
}

fn write_registry(fx: &common::SeriesFixture, owner: &str, pid: u32, start_ms: Option<u64>) {
    std::fs::create_dir_all(&fx.driver.layout().state_dir).expect("state dir");
    let text = serde_json::json!([{ "pid": pid, "start_ms": start_ms, "owner": owner }]);
    std::fs::write(fx.driver.layout().children_file(owner), text.to_string()).expect("registry");
}

/// Runs a new job and returns the text of its ledger.
fn run_and_read_ledger(fx: &common::SeriesFixture) -> String {
    let plan = fx.plan_file("plan-01-a.md", 1, "none");
    let job = create_job(&fx.driver, "hello", "say hello", Some(&plan), None).expect("create");
    run_job(&fx.driver, &job.id).expect("run");
    let job = fx.driver.job(&job.id).expect("job");
    std::fs::read_to_string(fx.driver.job_dir(&job).join("progress.md")).expect("progress")
}

/// True when the sleeper ends within 2 s. It is our child, so `try_wait` reaps the killed process.
fn gone(child: &mut GroupChild) -> bool {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if child.try_wait().expect("try_wait").is_some() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn alive(pid: u32) -> bool {
    // SAFETY: signal 0 only checks that the pid exists.
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[test]
fn a_takeover_of_a_dead_owner_reaps_its_registered_group() {
    let fx = fixture();
    let mut child = sleeper();
    let pid = child.id();
    write_lease(&fx, DEAD_OWNER, now_secs() + 1e6);
    write_registry(&fx, DEAD_OWNER, pid, process_start_ms(pid));
    let ledger = run_and_read_ledger(&fx);
    assert!(ledger.contains("killed"), "{ledger}");
    assert!(gone(&mut child), "the sleeper survived");
}

#[test]
fn a_takeover_leaves_a_mismatched_entry_alone() {
    let fx = fixture();
    let child = sleeper();
    let pid = child.id();
    write_lease(&fx, DEAD_OWNER, now_secs() + 1e6);
    write_registry(&fx, DEAD_OWNER, pid, Some(1));
    let ledger = run_and_read_ledger(&fx);
    assert!(ledger.contains("left alone"), "{ledger}");
    assert!(alive(pid), "the sleeper was killed");
}

#[test]
fn a_takeover_never_kills_an_unverified_entry() {
    let fx = fixture();
    let child = sleeper();
    let pid = child.id();
    // No start time in the owner name, so the state is `Unknown` and the lease expiry rules.
    let owner = "driver-4294967000";
    write_lease(&fx, owner, now_secs() - 100.0);
    write_registry(&fx, owner, pid, Some(1));
    let ledger = run_and_read_ledger(&fx);
    assert!(ledger.contains("left alone"), "{ledger}");
    assert!(!ledger.contains("killed"), "{ledger}");
    assert!(alive(pid), "the sleeper was killed");
}

#[test]
fn a_takeover_without_a_registry_still_takes_over() {
    let fx = fixture();
    write_lease(&fx, DEAD_OWNER, now_secs() + 1e6);
    let ledger = run_and_read_ledger(&fx);
    assert!(!ledger.contains("left alone"), "{ledger}");
    assert!(!ledger.contains("killed"), "{ledger}");
}

#[test]
fn a_fresh_take_reaps_a_dead_owners_group() {
    let fx = fixture();
    let mut child = sleeper();
    let pid = child.id();
    write_registry(&fx, DEAD_OWNER, pid, process_start_ms(pid));
    let ledger = run_and_read_ledger(&fx);
    assert!(ledger.contains("killed"), "{ledger}");
    assert!(gone(&mut child), "the sleeper survived");
}
