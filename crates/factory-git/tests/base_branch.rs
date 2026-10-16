mod common;

use common::*;
use factory_git::{JobRef, MergeOutcome, Preflight};
use git2::Oid;

const BASE: &str = "v-4.0.0";

fn job() -> JobRef {
    JobRef { id: "001".into(), slug: "x".into() }
}

/// A repo on `v-4.0.0` with a `main` branch that sits on a different commit.
fn two_branch_fixture() -> Fixture {
    let fx = Fixture::on_branch(BASE);
    let git = fx.git();
    let tip = git.find_commit(Oid::from_str(&fx.branch_tip(BASE)).expect("oid")).expect("c");
    git.branch("main", &tip, false).expect("branch main");
    commit_to_branch(&git, "main", &[("main.txt", "m\n")], "main work");
    fx
}

#[test]
fn worktree_starts_from_the_base_branch() {
    let fx = two_branch_fixture();
    let repo = fx.repo().with_base_branch(BASE);
    let wt = repo.create_worktree(&job()).expect("create");
    assert_eq!(wt.base, fx.branch_tip(BASE));
    assert_ne!(wt.base, fx.branch_tip("main"));
    assert_eq!(repo.tip(&wt.branch).expect("tip"), fx.branch_tip(BASE));
}

#[test]
fn merge_moves_the_base_branch_only() {
    let fx = two_branch_fixture();
    let main_before = fx.branch_tip("main");
    let git = fx.git();
    let base_tip = git.find_commit(Oid::from_str(&fx.branch_tip(BASE)).expect("oid")).expect("c");
    git.branch("factory/001-x", &base_tip, false).expect("branch");
    let head = commit_to_branch(&git, "factory/001-x", &[("job.txt", "j\n")], "job work");
    let base_before = fx.branch_tip(BASE);
    let repo = fx.repo().with_base_branch(BASE);
    let merge_sha = match repo
        .merge_no_ff("factory/001-x", &head.to_string(), "chore(factory): merge job 001 x")
        .expect("merge")
    {
        MergeOutcome::Merged { merge_sha } => merge_sha,
        other => panic!("expected Merged, got {other:?}"),
    };
    assert_ne!(fx.branch_tip(BASE), base_before);
    assert_eq!(fx.branch_tip(BASE), merge_sha);
    assert_eq!(fx.branch_tip("main"), main_before);
}

#[test]
fn state_checks_use_the_base_branch() {
    let fx = two_branch_fixture();
    match fx.repo().with_base_branch(BASE).preflight_main().expect("preflight") {
        Preflight::Ready { main_sha } => assert_eq!(main_sha, fx.branch_tip(BASE)),
        other => panic!("expected Ready, got {other:?}"),
    }
    match fx.repo().preflight_main().expect("preflight") {
        Preflight::Busy { reason } => assert!(reason.contains("not on main"), "reason: {reason}"),
        other => panic!("expected Busy, got {other:?}"),
    }
}

#[test]
fn worktrees_dir_can_be_outside_the_repo() {
    let fx = Fixture::new();
    let out = tempfile::TempDir::new().expect("tempdir");
    let dir = out.path().join("wt");
    let repo = fx.repo().with_worktrees_dir(dir.clone());
    assert_eq!(repo.worktrees_dir(), dir.as_path());
    let wt = repo.create_worktree(&job()).expect("create");
    assert_eq!(wt.path, dir.join("factory-001-x"));
    assert!(wt.path.join("base.txt").is_file());
    assert!(!fx.path.join(".claude").exists());
}
