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
    common::commit_file(&fx.main, "other.md", "o");
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

#[test]
fn a_base_off_the_base_branch_parks_with_the_branch_name() {
    let config = factory_core::Config { base_branch: "trunk".to_string(), ..factory_core::Config::default() };
    let fx = common::approved_job_config(common::approving_runner(3), false, config);
    // The branch trunk has its own root commit, so the job base is not on it.
    let repo = git2::Repository::open(&fx.main).expect("open");
    let sig = git2::Signature::now("t", "t@example.com").expect("sig");
    let tree = repo.find_tree(repo.treebuilder(None).expect("tb").write().expect("tree")).expect("find");
    let root = repo.commit(None, &sig, &sig, "root", &tree, &[]).expect("commit");
    repo.branch("trunk", &repo.find_commit(root).expect("commit"), true).expect("branch");
    let r = merge_stage(&fx.driver, &fx.id).expect("stage");
    assert!(matches!(&r, MergeResult::Parked(m) if m.contains("not on trunk")), "got {r:?}");
}

/// A fixture whose base is `trunk` while a local branch `main` also exists. The checkout sits on trunk.
fn trunk_fixture() -> common::MergeFixture {
    let config = factory_core::Config { base_branch: "trunk".to_string(), ..factory_core::Config::default() };
    let fx = common::approved_job_config(common::approving_runner(3), false, config);
    let repo = git2::Repository::open(&fx.main).expect("open");
    repo.set_head("refs/heads/trunk").expect("head on trunk");
    repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force())).expect("checkout");
    fx
}

fn tip_of(fx: &common::MergeFixture, branch: &str) -> String {
    let repo = git2::Repository::open(&fx.main).expect("open");
    let id = repo.revparse_single(branch).expect("branch").peel_to_commit().expect("commit").id();
    id.to_string()
}

#[test]
fn a_sync_merges_the_base_branch_and_leaves_main_alone() {
    let fx = trunk_fixture();
    let main_before = tip_of(&fx, "main");
    common::commit_file(&fx.main, "other.md", "o");
    let r = merge_stage(&fx.driver, &fx.id).expect("stage");
    assert!(matches!(r, MergeResult::Merged(_)), "got {r:?}");
    assert_eq!(fx.driver.job(&fx.id).expect("job").syncs, 1);
    assert_eq!(tip_of(&fx, "main"), main_before, "the branch main is untouched");
    assert!(fx.main.join("docs/hello.txt").exists(), "the job reached trunk");
}

#[test]
fn the_sync_cap_text_names_the_base_branch() {
    let fx = trunk_fixture();
    fx.driver
        .store()
        .update_job(&fx.id, |j| {
            j.syncs = 2;
            Ok(())
        })
        .expect("syncs");
    common::commit_file(&fx.main, "other.txt", "o");
    let r = merge_stage(&fx.driver, &fx.id).expect("stage");
    assert!(matches!(&r, MergeResult::Parked(m) if m.starts_with("trunk moved again after 2 syncs")), "got {r:?}");
}
