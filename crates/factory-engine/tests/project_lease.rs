mod common;

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::time::Duration;

use factory_engine::{
    acquire_project_lease, create_job, project_lease_status, release_project_lease, run_job, run_series, LeaseTake, ProjectLease,
    ProjectLeaseError, ProjectLeaseStatus,
};

const WAIT: Duration = Duration::from_secs(2);

fn lease_file(dir: &Path) -> std::path::PathBuf {
    factory_core::Layout { state_dir: dir.to_path_buf(), ..factory_core::Layout::in_repo(dir) }.project_lease_file()
}

fn read(dir: &Path) -> ProjectLease {
    serde_json::from_str(&std::fs::read_to_string(lease_file(dir)).expect("read lease")).expect("parse lease")
}

#[test]
fn a_second_owner_is_refused_and_the_same_owner_renews() {
    let d = tempfile::tempdir().expect("tmp");
    assert_eq!(acquire_project_lease(d.path(), "001", "a", 100.0, 1000.0, WAIT).expect("fresh"), LeaseTake::Fresh);
    assert!(lease_file(d.path()).exists());
    let err = acquire_project_lease(d.path(), "002", "b", 100.0, 1010.0, WAIT).expect_err("held");
    assert!(matches!(err, ProjectLeaseError::Held { .. }));
    assert_eq!(err.to_string(), "project is busy: job 001 is running (leased by a for another 90 seconds)");
    assert_eq!(acquire_project_lease(d.path(), "001", "a", 100.0, 1050.0, WAIT).expect("renew"), LeaseTake::Renewed);
    assert_eq!(read(d.path()).expires, 1150.0);
}

#[test]
fn an_expired_lease_is_taken_over() {
    let d = tempfile::tempdir().expect("tmp");
    acquire_project_lease(d.path(), "001", "a", 100.0, 1000.0, WAIT).expect("fresh");
    assert_eq!(
        project_lease_status(d.path(), 1200.0),
        ProjectLeaseStatus::Stale { owner: "a".into(), job: "001".into() }
    );
    let take = acquire_project_lease(d.path(), "002", "b", 100.0, 1200.0, WAIT).expect("takeover");
    assert_eq!(take, LeaseTake::TookOverStale { owner: "a".into(), job: "001".into() });
    assert_eq!(
        project_lease_status(d.path(), 1210.0),
        ProjectLeaseStatus::Held { owner: "b".into(), job: "002".into(), secs_left: 90 }
    );
}

#[test]
fn a_corrupt_or_empty_file_is_replaced() {
    for bytes in [&b"\x00\xff\x13garbage"[..], &b""[..]] {
        let d = tempfile::tempdir().expect("tmp");
        std::fs::write(lease_file(d.path()), bytes).expect("write");
        assert!(matches!(project_lease_status(d.path(), 1.0), ProjectLeaseStatus::Unreadable(_)));
        let take = acquire_project_lease(d.path(), "001", "a", 100.0, 1.0, WAIT).expect("take");
        assert_eq!(take, LeaseTake::TookOverStale { owner: "unknown".into(), job: "unknown".into() });
        assert_eq!(read(d.path()).owner, "a");
    }
}

#[test]
fn release_removes_only_what_it_may() {
    let d = tempfile::tempdir().expect("tmp");
    let now = factory_engine::now_secs();
    acquire_project_lease(d.path(), "001", "a", 1000.0, now, WAIT).expect("a");
    release_project_lease(d.path(), "b", WAIT).expect("not ours is no error");
    assert!(lease_file(d.path()).exists(), "another owner's live lease stays");
    release_project_lease(d.path(), "a", WAIT).expect("ours");
    assert!(!lease_file(d.path()).exists());
    acquire_project_lease(d.path(), "001", "a", 1.0, now - 100.0, WAIT).expect("expired a");
    release_project_lease(d.path(), "b", WAIT).expect("expired");
    assert!(!lease_file(d.path()).exists(), "an expired lease is removed");
    release_project_lease(d.path(), "b", WAIT).expect("no file is fine");
}

#[test]
fn exactly_one_of_two_simultaneous_acquirers_wins() {
    for _ in 0..20 {
        let d = tempfile::tempdir().expect("tmp");
        let barrier = Arc::new(Barrier::new(2));
        let handles: Vec<_> = ["a", "b"]
            .into_iter()
            .map(|owner| {
                let dir = d.path().to_path_buf();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    acquire_project_lease(&dir, "001", owner, 100.0, 1000.0, WAIT).is_ok()
                })
            })
            .collect();
        let wins = handles.into_iter().map(|h| h.join().expect("join")).filter(|w| *w).count();
        assert_eq!(wins, 1);
    }
}

#[test]
fn the_lock_wait_is_honored() {
    use fs2::FileExt;
    let d = tempfile::tempdir().expect("tmp");
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(d.path().join(format!("{}.lock", lease_file(d.path()).file_name().expect("name").to_string_lossy())))
        .expect("lock file");
    lock.lock_exclusive().expect("lock");
    let err = acquire_project_lease(d.path(), "001", "a", 100.0, 1.0, Duration::from_millis(100)).expect_err("locked");
    assert!(matches!(err, ProjectLeaseError::Lock(_)), "{err}");
    assert!(!lease_file(d.path()).exists());
}

fn quiet() -> common::RunHook {
    Arc::new(|_| {})
}

#[test]
fn hold_project_refuses_a_second_driver_until_the_guard_drops() {
    let fx = common::SeriesFixture::owned("a", quiet());
    let b = fx.other_driver("b");
    let guard = fx.driver.hold_project("001").expect("a holds");
    let err = b.hold_project("002").err().expect("b refused");
    assert!(err.to_string().starts_with("project is busy: job 001 is running (leased by a"), "{err}");
    drop(guard);
    assert!(!fx.driver.layout().project_lease_file().exists());
    let _b_guard = b.hold_project("002").expect("b succeeds now");
}

#[test]
fn run_job_is_refused_before_any_agent_runs_while_another_driver_holds_the_project() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let fx = common::SeriesFixture::owned("a", Arc::new(move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
    }));
    let plan = fx.plan_file("plan-01-a.md", 1, "none");
    let job = create_job(&fx.driver, "hello", "say hello", Some(&plan), None).expect("create");
    let b = fx.other_driver("b");
    let _held = b.hold_project("009").expect("b holds");
    let err = run_job(&fx.driver, &job.id).expect_err("busy");
    assert!(err.to_string().contains("project is busy"), "{err}");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(fx.driver.job(&job.id).expect("job").lease.is_none(), "no job lease was taken");
}

#[test]
fn run_job_without_a_guard_takes_and_releases_the_project_lease() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let main_dir = Arc::new(Mutex::new(None::<std::path::PathBuf>));
    let dir = main_dir.clone();
    let fx = common::SeriesFixture::owned("a", Arc::new(move |_| {
        let path = dir.lock().expect("dir").clone().expect("set");
        log.lock().expect("log").push(path.exists());
    }));
    *main_dir.lock().expect("dir") = Some(fx.driver.layout().project_lease_file());
    let plan = fx.plan_file("plan-01-a.md", 1, "none");
    let job = create_job(&fx.driver, "hello", "say hello", Some(&plan), None).expect("create");
    run_job(&fx.driver, &job.id).expect("run");
    let seen = seen.lock().expect("log");
    assert!(!seen.is_empty() && seen.iter().all(|x| *x), "the lease exists during every agent run");
    assert!(!fx.driver.layout().project_lease_file().exists(), "released at the end");
}

#[test]
fn a_series_holds_the_lease_between_plans() {
    let results = Arc::new(Mutex::new(Vec::new()));
    let log = results.clone();
    let state = Arc::new(Mutex::new(None::<std::path::PathBuf>));
    let hook_state = state.clone();
    let fx = common::SeriesFixture::owned("a", Arc::new(move |spec| {
        let dir = hook_state.lock().expect("state").clone().expect("set");
        let refused = acquire_project_lease(&dir, "x", "b", 100.0, factory_engine::now_secs(), Duration::from_secs(1)).is_err();
        log.lock().expect("log").push((spec.job_id.clone(), refused));
    }));
    *state.lock().expect("state") = Some(fx.driver.layout().state_dir.clone());
    let a = fx.plan_file("plan-01-a.md", 1, "plan-02-b.md");
    fx.plan_file("plan-02-b.md", 1, "none");
    let job = create_job(&fx.driver, "series", "two plans", Some(&a), None).expect("create");
    // A watcher samples the lease file in a tight loop, so even a short gap between two plans shows up.
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let poll_stop = stop.clone();
    let file = fx.driver.layout().project_lease_file();
    let board_file = fx.driver.layout().board_file();
    let watcher = std::thread::spawn(move || {
        let mut gaps = 0;
        // Watching starts once the series has taken its lease.
        while !file.exists() && !poll_stop.load(Ordering::SeqCst) {
            std::thread::yield_now();
        }
        while !poll_stop.load(Ordering::SeqCst) {
            if !file.exists() {
                // A missing file after the last plan merged is fine, the series is over by then.
                let all_merged = factory_core::Board::load(&board_file)
                    .map(|b| b.jobs.len() == 2 && b.jobs.iter().all(|j| j.stage == factory_core::Stage::Merged))
                    .unwrap_or(false);
                if !all_merged {
                    gaps += 1;
                }
            }
        }
        gaps
    });
    run_series(&fx.driver, &job.id).expect("series");
    stop.store(true, Ordering::SeqCst);
    assert_eq!(watcher.join().expect("watcher"), 0, "no gap between the plans");
    let results = results.lock().expect("log");
    assert!(results.iter().any(|(id, _)| id == "001") && results.iter().any(|(id, _)| id == "002"));
    assert!(results.iter().all(|(_, refused)| *refused), "another owner was refused in both plans");
    // The gap itself: between run_job of plan one and plan two the guard is still alive, so the file stays.
    assert!(!fx.driver.layout().project_lease_file().exists(), "released when the series returned");
}

#[test]
fn a_driver_without_an_owner_takes_no_project_lease() {
    let fx = common::SeriesFixture::new("fine\nVERDICT: APPROVE");
    let plan = fx.plan_file("plan-01-a.md", 1, "none");
    let job = create_job(&fx.driver, "hello", "say hello", Some(&plan), None).expect("create");
    let guard = fx.driver.hold_project(&job.id).expect("no-op guard");
    assert!(!fx.driver.layout().project_lease_file().exists());
    drop(guard);
    run_job(&fx.driver, &job.id).expect("run");
    assert!(!fx.driver.layout().project_lease_file().exists());
}

#[test]
fn the_guard_releases_on_an_error_path() {
    let fx = common::SeriesFixture::owned("a", quiet());
    let attempt = || -> Result<(), factory_engine::EngineError> {
        let _guard = fx.driver.hold_project("001")?;
        assert!(fx.driver.layout().project_lease_file().exists());
        Err(factory_engine::EngineError::Msg("boom".into()))
    };
    assert!(attempt().is_err());
    assert!(!fx.driver.layout().project_lease_file().exists());
}

#[test]
fn renewing_moves_the_project_lease_forward() {
    let fx = common::SeriesFixture::owned("a", quiet());
    let plan = fx.plan_file("plan-01-a.md", 1, "none");
    let job = create_job(&fx.driver, "hello", "say hello", Some(&plan), None).expect("create");
    let _guard = fx.driver.hold_project(&job.id).expect("hold");
    let before = read(&fx.driver.layout().state_dir).expires;
    std::thread::sleep(Duration::from_millis(1100));
    // run_job renews at the top of its loop and keeps the lease because a guard is alive.
    run_job(&fx.driver, &job.id).expect("run");
    let after = read(&fx.driver.layout().state_dir).expires;
    assert!(after > before, "{after} > {before}");
}
