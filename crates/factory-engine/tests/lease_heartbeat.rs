mod common;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use factory_engine::{acquire_project_lease, create_job, now_secs, project_lease_status, run_job, ProjectLease, ProjectLeaseStatus};

const WAIT: Duration = Duration::from_secs(2);

/// What the blocked agent step saw while it waited.
#[derive(Default)]
struct Seen {
    samples: u32,
    not_held: u32,
    refused: u32,
    first_expires: Option<f64>,
    last_expires: f64,
}

/// Blocks for `block` like a slow agent step and samples the project lease every 200 ms.
/// With `probe` it also tries to take the lease as another owner, which must be refused.
fn watch(state_dir: &std::path::Path, block: Duration, probe: bool, seen: &Mutex<Seen>) {
    let lease_file = factory_core::Layout { state_dir: state_dir.to_path_buf(), ..factory_core::Layout::in_repo(state_dir) }.project_lease_file();
    let end = Instant::now() + block;
    while Instant::now() < end {
        let now = now_secs();
        let held = matches!(project_lease_status(state_dir, now), ProjectLeaseStatus::Held { .. });
        let expires = std::fs::read_to_string(&lease_file)
            .ok()
            .and_then(|t| serde_json::from_str::<ProjectLease>(&t).ok())
            .map(|l| l.expires);
        let refused = probe && held && acquire_project_lease(state_dir, "x", "intruder", 100.0, now, WAIT).is_err();
        let mut seen = seen.lock().expect("seen");
        seen.samples += 1;
        seen.not_held += u32::from(!held);
        seen.refused += u32::from(refused);
        if seen.first_expires.is_none() {
            seen.first_expires = expires;
        }
        seen.last_expires = expires.unwrap_or(0.0);
        drop(seen);
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// An owned fixture with the given timing whose first agent run blocks and watches the lease.
fn fixture(ttl: f64, beat: Duration, probe: bool, seen: Arc<Mutex<Seen>>) -> common::SeriesFixture {
    let first = Arc::new(AtomicBool::new(true));
    let slot = Arc::new(Mutex::new(None::<PathBuf>));
    let hook_slot = slot.clone();
    let fx = common::SeriesFixture::owned_timed(
        "a",
        Arc::new(move |_| {
            if first.swap(false, Ordering::SeqCst) {
                let dir = hook_slot.lock().expect("slot").clone().expect("set");
                watch(&dir, Duration::from_secs(5), probe, &seen);
            }
        }),
        ttl,
        beat,
    );
    *slot.lock().expect("slot") = Some(fx.driver.layout().state_dir.clone());
    fx
}

fn run_it(fx: &common::SeriesFixture) -> Result<factory_engine::RunEnd, factory_engine::EngineError> {
    let plan = fx.plan_file("plan-01-a.md", 1, "none");
    let job = create_job(&fx.driver, "hello", "say hello", Some(&plan), None).expect("create");
    run_job(&fx.driver, &job.id)
}

#[test]
fn the_heartbeat_keeps_the_lease_alive_through_a_long_agent_step() {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let fx = fixture(2.0, Duration::from_millis(200), true, seen.clone());
    run_it(&fx).expect("run");
    let seen = seen.lock().expect("seen");
    assert!(seen.samples >= 15, "sampled {} times", seen.samples);
    assert_eq!(seen.not_held, 0, "the lease was held at every sample");
    assert_eq!(seen.refused, seen.samples, "another owner was refused at every sample");
    let first = seen.first_expires.expect("first");
    assert!(seen.last_expires > first + 1.0, "expires moved from {first} to {}", seen.last_expires);
    assert!(!fx.driver.layout().project_lease_file().exists(), "released after the run");
}

#[test]
fn without_a_heartbeat_the_lease_goes_stale() {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let fx = fixture(2.0, Duration::from_secs(3600), false, seen.clone());
    run_it(&fx).expect("run");
    let seen = seen.lock().expect("seen");
    assert!(seen.not_held > 0, "the lease went stale during the step");
}

#[test]
fn dropping_the_guard_ends_the_heartbeat_quickly() {
    let fx = fixture(2.0, Duration::from_secs(3600), false, Arc::new(Mutex::new(Seen::default())));
    let guard = fx.driver.hold_project("001").expect("hold");
    assert!(!guard.lost());
    let started = Instant::now();
    drop(guard);
    assert!(started.elapsed() < Duration::from_secs(1), "drop took {:?}", started.elapsed());
    assert!(!fx.driver.layout().project_lease_file().exists());
}

#[test]
fn another_owner_in_the_lease_sets_lost_and_stops_the_run() {
    let slot = Arc::new(Mutex::new(None::<PathBuf>));
    let hook_slot = slot.clone();
    let first = Arc::new(AtomicBool::new(true));
    let fx = common::SeriesFixture::owned_timed(
        "a",
        Arc::new(move |_| {
            if first.swap(false, Ordering::SeqCst) {
                let path = hook_slot.lock().expect("slot").clone().expect("set");
                let other = ProjectLease { owner: "other".into(), job: "009".into(), expires: now_secs() + 1.0e6 };
                std::fs::write(path, serde_json::to_vec(&other).expect("json")).expect("overwrite");
                std::thread::sleep(Duration::from_secs(1));
            }
        }),
        2.0,
        Duration::from_millis(200),
    );
    *slot.lock().expect("slot") = Some(fx.driver.layout().project_lease_file());
    let err = run_it(&fx).expect_err("lost");
    assert!(err.to_string().contains("project is busy"), "{err}");
    assert!(fx.driver.lease_lost());
    let text = std::fs::read_to_string(fx.driver.layout().project_lease_file()).expect("lease stays");
    assert!(text.contains("\"other\""), "the other owner keeps the lease: {text}");
}
