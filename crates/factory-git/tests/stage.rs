mod common;

use common::*;
use factory_git::{JobRef, Repo, Worktree};
use git2::Repository;

fn job_worktree(fx: &Fixture) -> (Repo, Worktree) {
    let repo = fx.repo();
    let wt = repo
        .create_worktree(&JobRef { id: "001".into(), slug: "demo".into() })
        .expect("create worktree");
    (repo, wt)
}

fn inside(wt: &Worktree) -> Repository {
    Repository::open(&wt.path).expect("open worktree")
}

#[test]
fn stage_all_stages_new_modified_and_deleted_files() {
    let fx = Fixture::new();
    let (repo, wt) = job_worktree(&fx);
    commit_files(&inside(&wt), &[("keep.txt", "one\n")], "add keep");
    std::fs::write(wt.path.join("keep.txt"), "one\ntwo\n").expect("modify");
    std::fs::write(wt.path.join("new.txt"), "new\n").expect("new");
    std::fs::remove_file(wt.path.join("base.txt")).expect("delete");

    let staged = repo.stage_all(&wt).expect("stage");
    assert_eq!(staged, vec!["base.txt", "keep.txt", "new.txt"]);
    assert!(!repo.worktree_is_clean(&wt).expect("clean check"));
}

#[test]
fn stage_all_skips_gitignored_files() {
    let fx = Fixture::new();
    let (repo, wt) = job_worktree(&fx);
    std::fs::write(wt.path.join(".gitignore"), "*.log\n").expect("gitignore");
    std::fs::write(wt.path.join("scratch.log"), "noise\n").expect("log");
    std::fs::write(wt.path.join("kept.txt"), "keep\n").expect("kept");

    let staged = repo.stage_all(&wt).expect("stage");
    assert_eq!(staged, vec![".gitignore", "kept.txt"]);
}

#[test]
fn stage_all_keeps_spaces_and_unicode_in_paths() {
    let fx = Fixture::new();
    let (repo, wt) = job_worktree(&fx);
    std::fs::create_dir_all(wt.path.join("dir with space")).expect("mkdir");
    std::fs::write(wt.path.join("dir with space").join("ünï.txt"), "x\n").expect("write");

    let staged = repo.stage_all(&wt).expect("stage");
    assert_eq!(staged, vec!["dir with space/ünï.txt"]);
}

#[test]
fn stage_all_on_a_clean_worktree_returns_nothing() {
    let fx = Fixture::new();
    let (repo, wt) = job_worktree(&fx);
    assert!(repo.stage_all(&wt).expect("stage").is_empty());
    assert!(repo.worktree_is_clean(&wt).expect("clean check"));
}

#[test]
fn worktree_is_clean_ignores_ignored_files_and_sees_untracked_ones() {
    let fx = Fixture::new();
    let (repo, wt) = job_worktree(&fx);
    commit_files(&inside(&wt), &[(".gitignore", "*.log\n")], "ignore logs");
    std::fs::write(wt.path.join("noise.log"), "x\n").expect("log");
    assert!(repo.worktree_is_clean(&wt).expect("ignored only"));

    std::fs::write(wt.path.join("stray.txt"), "x\n").expect("stray");
    assert!(!repo.worktree_is_clean(&wt).expect("untracked"));
}

#[test]
fn head_is_on_the_job_branch_until_it_is_detached() {
    let fx = Fixture::new();
    let (repo, wt) = job_worktree(&fx);
    assert!(repo.head_is_on_branch(&wt).expect("on branch"));
    let inside = inside(&wt);
    let oid = inside.head().expect("head").peel_to_commit().expect("commit").id();
    inside.set_head_detached(oid).expect("detach");
    assert!(!repo.head_is_on_branch(&wt).expect("detached"));
}
