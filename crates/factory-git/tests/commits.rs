mod common;

use common::*;
use factory_git::{GitError, JobRef, Repo, Worktree};
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
fn commits_above_lists_commits_oldest_first_with_their_files() {
    let fx = Fixture::new();
    let (repo, wt) = job_worktree(&fx);
    commit_files(&inside(&wt), &[("a.txt", "a\n")], "feat(x): add a");
    commit_files(&inside(&wt), &[("b.txt", "b\n"), ("c.txt", "c\n")], "feat(x): add b and c");
    let tip = repo.tip(&wt.branch).expect("tip");

    let commits = repo.commits_above(&wt.base, &tip).expect("commits");
    assert_eq!(commits.len(), 2);
    assert_eq!(commits[0].subject, "feat(x): add a");
    assert_eq!(commits[0].files, vec!["a.txt"]);
    assert_eq!(commits[1].subject, "feat(x): add b and c");
    assert_eq!(commits[1].files, vec!["b.txt", "c.txt"]);
    assert_eq!(commits[1].sha, tip);
}

#[test]
fn commits_above_is_empty_when_floor_equals_tip() {
    let fx = Fixture::new();
    let (repo, wt) = job_worktree(&fx);
    assert!(repo.commits_above(&wt.base, &wt.base).expect("commits").is_empty());
}

#[test]
fn commit_staged_commits_the_index_and_returns_the_new_tip() {
    let fx = Fixture::new();
    let (repo, wt) = job_worktree(&fx);
    std::fs::write(wt.path.join("new.txt"), "new\n").expect("write");
    repo.stage_all(&wt).expect("stage");

    let sha = repo.commit_staged(&wt, "chore: add new\n\nbody line").expect("commit");
    assert_eq!(sha, repo.tip(&wt.branch).expect("tip"));
    let commits = repo.commits_above(&wt.base, &sha).expect("commits");
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].files, vec!["new.txt"]);
    let message = inside(&wt).head().expect("head").peel_to_commit().expect("commit").message().map(str::to_string);
    assert_eq!(message.as_deref(), Some("chore: add new\n\nbody line"));
    assert!(repo.worktree_is_clean(&wt).expect("clean"));
}

#[test]
fn commit_staged_with_nothing_staged_is_refused() {
    let fx = Fixture::new();
    let (repo, wt) = job_worktree(&fx);
    let before = repo.tip(&wt.branch).expect("tip");
    assert!(matches!(repo.commit_staged(&wt, "chore: nothing"), Err(GitError::Refused(_))));
    assert_eq!(repo.tip(&wt.branch).expect("tip"), before);
}
