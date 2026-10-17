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
fn reset_soft_moves_the_branch_and_keeps_the_work_staged() {
    let fx = Fixture::new();
    let (repo, wt) = job_worktree(&fx);
    commit_files(&inside(&wt), &[("a.txt", "a\n")], "wip: task 1 a");
    commit_files(&inside(&wt), &[("b.txt", "b\n")], "wip: task 2 b");
    repo.reset_soft(&wt, &wt.base).expect("reset");
    assert_eq!(repo.tip(&wt.branch).expect("tip"), wt.base);
    assert_eq!(repo.stage_all(&wt).expect("stage"), vec!["a.txt".to_string(), "b.txt".to_string()]);
    assert!(wt.path.join("b.txt").exists());
}

#[test]
fn reset_hard_drops_the_work() {
    let fx = Fixture::new();
    let (repo, wt) = job_worktree(&fx);
    commit_files(&inside(&wt), &[("a.txt", "a\n")], "wip: task 1 a");
    repo.reset_hard(&wt, &wt.base).expect("reset");
    assert_eq!(repo.tip(&wt.branch).expect("tip"), wt.base);
    assert!(!wt.path.join("a.txt").exists());
    assert!(repo.worktree_is_clean(&wt).expect("clean"));
}

#[test]
fn restore_tree_makes_the_files_and_index_match_a_commit_and_keeps_the_branch() {
    let fx = Fixture::new();
    let (repo, wt) = job_worktree(&fx);
    commit_files(&inside(&wt), &[("a.txt", "a\n")], "wip: task 1 a");
    let first = repo.tip(&wt.branch).expect("tip");
    commit_files(&inside(&wt), &[("b.txt", "b\n")], "wip: task 2 b");
    repo.reset_hard(&wt, &wt.base).expect("reset");
    repo.restore_tree(&wt, &first).expect("restore");
    assert_eq!(repo.tip(&wt.branch).expect("tip"), wt.base);
    assert!(wt.path.join("a.txt").exists());
    assert!(!wt.path.join("b.txt").exists());
    assert_eq!(repo.stage_all(&wt).expect("stage"), vec!["a.txt".to_string()]);
}

#[test]
fn restore_tree_removes_staged_files_that_the_target_tree_lacks() {
    let fx = Fixture::new();
    let (repo, wt) = job_worktree(&fx);
    commit_files(&inside(&wt), &[("a.txt", "a\n")], "wip: task 1 a");
    let first = repo.tip(&wt.branch).expect("tip");
    commit_files(&inside(&wt), &[("b.txt", "b\n")], "wip: task 2 b");
    repo.reset_soft(&wt, &wt.base).expect("reset");
    repo.restore_tree(&wt, &first).expect("restore");
    assert!(wt.path.join("a.txt").exists());
    assert!(!wt.path.join("b.txt").exists());
    assert_eq!(repo.stage_all(&wt).expect("stage"), vec!["a.txt".to_string()]);
}

#[test]
fn same_tree_ignores_history() {
    let fx = Fixture::new();
    let (repo, wt) = job_worktree(&fx);
    let first = commit_files(&inside(&wt), &[("a.txt", "a\n")], "wip: task 1 a").to_string();
    repo.reset_soft(&wt, &wt.base).expect("reset");
    repo.stage_all(&wt).expect("stage");
    let squashed = repo.commit_staged(&wt, "feat: a").expect("commit");
    assert_ne!(first, squashed);
    assert!(repo.same_tree(&first, &squashed).expect("same"));
    assert!(!repo.same_tree(&wt.base, &squashed).expect("differs"));
}
