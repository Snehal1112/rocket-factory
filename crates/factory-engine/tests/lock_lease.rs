use factory_engine::{
    acquire_lease, lease_is_live, lock_board, release_lease, BoardStore, LeaseError, StoreError,
};
use std::time::Duration;

/// A pid that no process has: above the kernel's pid limit.
const DEAD_OWNER: &str = "driver-4294967000-1";

fn store_with_job() -> (tempfile::TempDir, BoardStore) {
    let d = tempfile::tempdir().expect("tmp");
    let store = BoardStore::new(
        &d.path().join(".superpowers/factory/board.json"),
        Duration::from_millis(200),
    );
    store
        .update(|b| {
            b.jobs.push(factory_core::Job::new(
                "001",
                "d-001-x",
                "x",
                "2026-10-13",
                "f",
                0.0,
            ));
            Ok(())
        })
        .expect("seed");
    (d, store)
}

#[test]
fn a_held_lock_fails_a_write_after_a_bounded_wait_but_not_a_read() {
    let (d, store) = store_with_job();
    let _held = lock_board(
        &d.path().join(".superpowers/factory/board.json.lock"),
        Duration::from_millis(50),
    )
    .expect("lock");
    let err = store.update(|_| Ok(())).expect_err("locked");
    assert!(matches!(err, StoreError::Locked));
    assert_eq!(store.load().expect("read").jobs.len(), 1);
}

#[test]
fn concurrent_creators_never_collide_on_ids() {
    let d = tempfile::tempdir().expect("tmp");
    let root = d.path().to_path_buf();
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let root = root.clone();
            std::thread::spawn(move || {
                let store = BoardStore::new(&root.join("board.json"), Duration::from_secs(10));
                for _ in 0..5 {
                    store
                        .update(|b| {
                            let id = factory_engine::next_id(b);
                            b.jobs.push(factory_core::Job::new(
                                &id,
                                &format!("d-{id}"),
                                "x",
                                "2026-10-13",
                                "f",
                                0.0,
                            ));
                            Ok(())
                        })
                        .expect("update");
                }
            })
        })
        .collect();
    for h in handles {
        h.join().expect("join");
    }
    let mut ids: Vec<String> = BoardStore::new(&root.join("board.json"), Duration::from_secs(10))
        .load()
        .expect("load")
        .jobs
        .into_iter()
        .map(|j| j.id)
        .collect();
    assert_eq!(ids.len(), 20);
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 20);
}

#[test]
fn a_live_lease_blocks_another_owner_until_it_expires() {
    let (_d, store) = store_with_job();
    acquire_lease(&store, "001", "alice", 60.0, 100.0).expect("alice");
    assert!(matches!(
        acquire_lease(&store, "001", "bob", 60.0, 110.0),
        Err(LeaseError::Held { .. })
    ));
    acquire_lease(&store, "001", "alice", 60.0, 120.0).expect("renew");
    assert!(matches!(
        release_lease(&store, "001", "bob", 130.0),
        Err(LeaseError::Held { .. })
    ));
    assert!(lease_is_live(&store.load().expect("b").jobs[0], 130.0));
    acquire_lease(&store, "001", "bob", 60.0, 500.0).expect("takeover after expiry");
    release_lease(&store, "001", "bob", 510.0).expect("release");
    assert!(store.load().expect("b").jobs[0].lease.is_none());
}

#[test]
fn a_dead_owner_lease_is_taken_over() {
    let (_d, store) = store_with_job();
    acquire_lease(&store, "001", DEAD_OWNER, 1500.0, 1000.0).expect("dead driver");
    acquire_lease(&store, "001", "driver-1111-2", 1500.0, 1010.0).expect("take over");
    let job = &store.load().expect("board").jobs[0];
    let lease = job.lease.as_ref().expect("lease");
    assert_eq!(lease.owner, "driver-1111-2");
    assert_eq!(lease.expires, 2510.0);
}

#[test]
fn a_dead_owner_lease_is_not_live() {
    let (_d, store) = store_with_job();
    acquire_lease(&store, "001", DEAD_OWNER, 1500.0, 1000.0).expect("dead driver");
    let job = store.load().expect("board").jobs[0].clone();
    assert!(!lease_is_live(&job, 1010.0));
    assert!(!lease_is_live(&job, 3000.0));
}

#[test]
fn a_live_owner_lease_stays_live() {
    let (_d, store) = store_with_job();
    let owner = format!("driver-{}-1", std::process::id());
    acquire_lease(&store, "001", &owner, 1500.0, 1000.0).expect("own pid");
    let job = store.load().expect("board").jobs[0].clone();
    assert!(lease_is_live(&job, 1010.0));
    assert!(matches!(
        acquire_lease(&store, "001", "driver-1111-2", 60.0, 1010.0),
        Err(LeaseError::Held { .. })
    ));
    assert!(!lease_is_live(&job, 2600.0));
}

#[test]
fn a_non_driver_owner_is_judged_by_expiry_only() {
    let (_d, store) = store_with_job();
    for owner in [
        "tester",
        "driver-x-1",
        "driver-4294967000",
        "driver-4294967000-x",
    ] {
        acquire_lease(&store, "001", owner, 1500.0, 1000.0).expect("first");
        let job = store.load().expect("board").jobs[0].clone();
        assert!(lease_is_live(&job, 1010.0), "{owner} should be live");
        assert!(matches!(
            acquire_lease(&store, "001", "driver-1111-2", 60.0, 1010.0),
            Err(LeaseError::Held { .. })
        ));
        assert!(!lease_is_live(&job, 2600.0));
        release_lease(&store, "001", owner, 1010.0).expect("clear");
    }
}

#[test]
fn a_dead_owner_lease_can_be_released_by_another_owner() {
    let (_d, store) = store_with_job();
    acquire_lease(&store, "001", DEAD_OWNER, 1500.0, 1000.0).expect("dead driver");
    release_lease(&store, "001", "driver-1111-2", 1010.0).expect("release");
    assert!(store.load().expect("b").jobs[0].lease.is_none());
}
