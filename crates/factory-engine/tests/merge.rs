mod common;
use factory_engine::{merge_stage, MergeResult};

#[test]
fn merges_the_reviewed_head_with_the_fixed_message() {
    let fx = common::approved_job();
    let before = common::main_tip(&fx.main);
    let MergeResult::Merged(sha) = merge_stage(&fx.driver, &fx.id).expect("merge") else {
        panic!("expected a merge");
    };
    assert_ne!(sha, before);
    let repo = git2::Repository::open(&fx.main).expect("open");
    let commit = repo.find_commit(git2::Oid::from_str(&sha).expect("oid")).expect("commit");
    assert_eq!(commit.message().expect("msg").lines().next(), Some("chore(factory): merge job 001 hello"));
    assert_eq!(commit.parent_id(1).expect("second parent").to_string(), fx.head);
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.stage, factory_core::Stage::Merged);
    let ledger = std::fs::read_to_string(fx.driver.job_dir(&job).join("progress.md")).expect("ledger");
    assert!(ledger.contains(&format!("Task 1: complete (merge {sha})")));
}

#[test]
fn a_blocking_untracked_file_parks_the_job_and_leaves_main_alone() {
    let fx = common::approved_job();
    // The reviewed change adds docs/hello.txt, so an untracked file at that path blocks the merge.
    std::fs::create_dir_all(fx.main.join("docs")).expect("mkdir");
    std::fs::write(fx.main.join("docs/hello.txt"), "mine").expect("write");
    let before = common::main_tip(&fx.main);
    let MergeResult::Parked(reason) = merge_stage(&fx.driver, &fx.id).expect("stage") else {
        panic!("expected a park");
    };
    assert!(!reason.is_empty());
    assert_eq!(common::main_tip(&fx.main), before);
    assert_eq!(std::fs::read_to_string(fx.main.join("docs/hello.txt")).expect("read"), "mine");
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.stage, factory_core::Stage::NeedsHuman);
    assert_eq!(job.reason.as_deref(), Some(reason.as_str()));
}

#[test]
fn a_moved_branch_tip_parks_without_merging() {
    let fx = common::approved_job();
    common::commit_file(&common::worktree_of(&fx), "late.txt", "x");
    let before = common::main_tip(&fx.main);
    assert!(matches!(merge_stage(&fx.driver, &fx.id).expect("stage"), MergeResult::Parked(_)));
    assert_eq!(common::main_tip(&fx.main), before);
}

#[test]
fn a_dirty_main_parks_as_main_busy() {
    let fx = common::approved_job();
    std::fs::write(fx.main.join("README.md"), "dirty").expect("write");
    let r = merge_stage(&fx.driver, &fx.id).expect("stage");
    assert!(matches!(&r, MergeResult::Parked(m) if m.starts_with("main busy")), "got {r:?}");
}

#[test]
fn main_moved_triggers_a_sync_then_merges() {
    let fx = common::approved_job();
    common::commit_file(&fx.main, "other.txt", "o");
    let r = merge_stage(&fx.driver, &fx.id).expect("stage");
    assert!(matches!(r, MergeResult::Merged(_)), "got {r:?}");
    assert_eq!(fx.driver.job(&fx.id).expect("job").syncs, 1);
}

#[test]
fn a_sync_conflict_parks() {
    let fx = common::approved_job();
    let wt = common::worktree_of(&fx);
    common::commit_file(&wt, "same.txt", "branch");
    // Re-review is out of scope here: record the new head as the reviewed head.
    common::record_head(&fx, &common::branch_tip(&fx));
    common::commit_file(&fx.main, "same.txt", "main");
    let r = merge_stage(&fx.driver, &fx.id).expect("stage");
    assert!(matches!(&r, MergeResult::Parked(m) if m.contains("conflict")), "got {r:?}");
    assert_eq!(fx.driver.job(&fx.id).expect("job").stage, factory_core::Stage::NeedsHuman);
}

#[test]
fn a_third_sync_is_refused() {
    let fx = common::approved_job();
    fx.driver
        .store()
        .update_job(&fx.id, |j| {
            j.syncs = 2;
            Ok(())
        })
        .expect("syncs");
    common::commit_file(&fx.main, "other.txt", "o");
    let before = common::main_tip(&fx.main);
    let r = merge_stage(&fx.driver, &fx.id).expect("stage");
    assert!(matches!(&r, MergeResult::Parked(m) if m.contains("syncs")), "got {r:?}");
    assert_eq!(common::main_tip(&fx.main), before);
}

#[test]
fn park_on_an_unparked_job_writes_an_other_record() {
    let fx = common::approved_job();
    factory_engine::park(&fx.driver, &fx.id, "main busy").expect("park");
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.stage, factory_core::Stage::NeedsHuman);
    assert_eq!(job.park_kind.as_deref(), Some("other"));
    let rec = factory_core::ParkRecord::read(&fx.driver.job_dir(&job)).expect("park.json");
    assert_eq!(rec.kind, factory_core::ParkKind::Other);
    assert_eq!(rec.reason, "main busy");
}

#[test]
fn park_keeps_the_record_of_a_job_that_is_already_parked() {
    let fx = common::approved_job();
    let job = fx.driver.job(&fx.id).expect("job");
    let dir = fx.driver.job_dir(&job);
    // This is what a failed check inside the sync leaves behind.
    let part = factory_core::ParkPart::new("Checks", "checks.md", "FAIL cargo check");
    let rec = factory_core::ParkRecord::new(factory_core::ParkKind::ChecksFailed, "checks failed", &fx.id, job.round, 7, vec![part]);
    rec.write(&dir).expect("write record");
    fx.driver
        .store()
        .update_job(&fx.id, |j| {
            j.park_kind = Some("checks-failed".to_string());
            j.park_at = Some(7);
            j.set_stage(factory_core::Stage::NeedsHuman, Some("checks failed"), false, 7.0)
        })
        .expect("park job");
    factory_engine::park(&fx.driver, &fx.id, "after sync: checks failed").expect("park");
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.park_kind.as_deref(), Some("checks-failed"));
    assert_eq!(job.park_at, Some(7));
    assert_eq!(job.reason.as_deref(), Some("after sync: checks failed"));
    let rec = factory_core::ParkRecord::read(&dir).expect("park.json");
    assert_eq!(rec.kind, factory_core::ParkKind::ChecksFailed);
    assert_eq!(rec.parts.len(), 1);
}
