mod common;

use common::*;
use factory_git::{GitError, JobRef, MergeOutcome, Repo, Worktree};
use git2::Repository;

fn job() -> JobRef {
    JobRef { id: "001".into(), slug: "demo".into() }
}

fn wt_repo(wt: &Worktree) -> Repository {
    Repository::open(&wt.path).expect("open worktree")
}

#[test]
fn create_worktree_makes_the_branch_and_the_directory() {
    let fx = Fixture::new();
    let repo = fx.repo();
    let wt = repo.create_worktree(&job()).expect("create");
    assert_eq!(wt.branch, "factory/001-demo");
    assert_eq!(wt.name, "factory-001-demo");
    assert_eq!(wt.base, fx.main_sha());
    assert!(wt.path.ends_with(".claude/worktrees/factory-001-demo"), "path: {:?}", wt.path);
    assert!(wt.path.join(".git").exists());
    assert!(wt.path.join("base.txt").is_file());
    assert_eq!(repo.tip("factory/001-demo").expect("tip"), fx.main_sha());
    // The worktree resolves back to the main checkout.
    let from_wt = Repo::open(&wt.path).expect("open from worktree");
    let want = std::fs::canonicalize(&fx.path).expect("canon");
    assert_eq!(std::fs::canonicalize(from_wt.root()).expect("canon"), want);
    // Creating it twice is refused.
    assert!(matches!(repo.create_worktree(&job()), Err(GitError::Refused(_))));
}

#[test]
fn create_worktree_rejects_hostile_slugs_before_touching_the_disk() {
    let fx = Fixture::new();
    let repo = fx.repo();
    for slug in ["../evil", "Bad Slug", "a/b", "", "x\n"] {
        let bad = JobRef { id: "001".into(), slug: slug.into() };
        assert!(matches!(repo.create_worktree(&bad), Err(GitError::BadInput(_))), "slug {slug:?}");
    }
    let bad_id = JobRef { id: "1x".into(), slug: "ok".into() };
    assert!(matches!(repo.create_worktree(&bad_id), Err(GitError::BadInput(_))));
    assert!(!fx.path.join(".claude").exists(), "nothing may be created for a rejected job");
}

#[test]
fn remove_worktree_removes_a_clean_worktree_and_its_branch() {
    let fx = Fixture::new();
    let repo = fx.repo();
    let wt = repo.create_worktree(&job()).expect("create");
    repo.remove_worktree(&wt, false).expect("remove");
    assert!(!wt.path.exists());
    assert!(matches!(repo.tip("factory/001-demo"), Err(GitError::NotFound(_))));
}

#[test]
fn remove_worktree_refuses_untracked_work_unless_forced() {
    let fx = Fixture::new();
    let repo = fx.repo();
    let wt = repo.create_worktree(&job()).expect("create");
    std::fs::write(wt.path.join("scratch.txt"), "work in progress").expect("write");
    assert!(matches!(repo.remove_worktree(&wt, false), Err(GitError::Refused(_))));
    assert!(wt.path.join("scratch.txt").is_file());
    assert!(repo.tip("factory/001-demo").is_ok());
    repo.remove_worktree(&wt, true).expect("forced remove");
    assert!(!wt.path.exists());
}

#[test]
fn remove_worktree_refuses_unmerged_commits_unless_forced() {
    let fx = Fixture::new();
    let repo = fx.repo();
    let wt = repo.create_worktree(&job()).expect("create");
    commit_files(&wt_repo(&wt), &[("job.txt", "j\n")], "job work");
    assert!(matches!(repo.remove_worktree(&wt, false), Err(GitError::Refused(_))));
    assert!(wt.path.is_dir());
    assert!(repo.tip("factory/001-demo").is_ok());
    repo.remove_worktree(&wt, true).expect("forced remove");
    assert!(matches!(repo.tip("factory/001-demo"), Err(GitError::NotFound(_))));
}

#[test]
fn remove_worktree_allows_a_merged_job() {
    let fx = Fixture::new();
    let repo = fx.repo();
    let wt = repo.create_worktree(&job()).expect("create");
    let head = commit_files(&wt_repo(&wt), &[("job.txt", "j\n")], "job work").to_string();
    point_branch_at(&fx.git(), "main", &head);
    repo.remove_worktree(&wt, false).expect("remove merged");
    assert!(!wt.path.exists());
}

#[test]
fn remove_worktree_never_touches_main_or_the_main_checkout() {
    let fx = Fixture::new();
    let repo = fx.repo();
    let on_main = Worktree {
        name: "x".into(),
        path: fx.path.join(".claude/worktrees/x"),
        branch: "main".into(),
        base: fx.main_sha(),
    };
    assert!(matches!(repo.remove_worktree(&on_main, true), Err(GitError::Refused(_))));
    let at_root = Worktree {
        name: "factory-001-demo".into(),
        path: fx.path.clone(),
        branch: "factory/001-demo".into(),
        base: fx.main_sha(),
    };
    assert!(matches!(repo.remove_worktree(&at_root, true), Err(GitError::Refused(_))));
    assert!(fx.path.join("base.txt").is_file());
    assert!(repo.tip("main").is_ok());
}

#[test]
fn merge_main_into_adds_a_merge_commit_without_rewriting_history() {
    let fx = Fixture::new();
    let repo = fx.repo();
    let wt = repo.create_worktree(&job()).expect("create");
    let old_head = commit_files(&wt_repo(&wt), &[("job.txt", "j\n")], "job work").to_string();
    commit_files(&fx.git(), &[("other.txt", "o\n")], "main moves");
    let main = fx.main_sha();
    let msg = "chore(factory): sync job 001 demo with main";
    let new_head = match repo.merge_main_into(&wt, msg).expect("sync") {
        MergeOutcome::Merged { merge_sha } => merge_sha,
        other => panic!("expected Merged, got {other:?}"),
    };
    let git = fx.git();
    let commit = git.find_commit(git2::Oid::from_str(&new_head).expect("oid")).expect("commit");
    assert_eq!(commit.parent_id(0).expect("p0").to_string(), old_head);
    assert_eq!(commit.parent_id(1).expect("p1").to_string(), main);
    assert_eq!(repo.tip("factory/001-demo").expect("tip"), new_head);
    assert!(repo.is_ancestor(&old_head, &new_head).expect("ancestor"));
    assert_eq!(fx.main_sha(), main, "main must not move");
    assert!(wt.path.join("job.txt").is_file() && wt.path.join("other.txt").is_file());
    // The stale base still reads as current after the sync.
    let check = repo.base_state(&wt.base, &new_head).expect("state");
    assert_eq!(check.state, factory_git::BaseState::Current);
}

#[test]
fn merge_main_into_is_up_to_date_when_main_has_not_moved() {
    let fx = Fixture::new();
    let repo = fx.repo();
    let wt = repo.create_worktree(&job()).expect("create");
    commit_files(&wt_repo(&wt), &[("job.txt", "j\n")], "job work");
    let outcome = repo.merge_main_into(&wt, "sync").expect("sync");
    assert_eq!(outcome, MergeOutcome::UpToDate);
}

#[test]
fn merge_main_into_reports_a_conflict_and_leaves_the_worktree_untouched() {
    let fx = Fixture::new();
    let repo = fx.repo();
    let wt = repo.create_worktree(&job()).expect("create");
    let old_head = commit_files(&wt_repo(&wt), &[("base.txt", "job\n")], "job edit").to_string();
    commit_files(&fx.git(), &[("base.txt", "main\n")], "main edit");
    match repo.merge_main_into(&wt, "sync").expect("sync") {
        MergeOutcome::Conflict { paths } => assert_eq!(paths, vec!["base.txt".to_string()]),
        other => panic!("expected Conflict, got {other:?}"),
    }
    assert_eq!(repo.tip("factory/001-demo").expect("tip"), old_head);
    assert_eq!(std::fs::read_to_string(wt.path.join("base.txt")).expect("read"), "job\n");
}

#[test]
fn merge_main_into_refuses_a_worktree_with_tracked_changes() {
    let fx = Fixture::new();
    let repo = fx.repo();
    let wt = repo.create_worktree(&job()).expect("create");
    commit_files(&fx.git(), &[("other.txt", "o\n")], "main moves");
    std::fs::write(wt.path.join("base.txt"), "dirty\n").expect("write");
    match repo.merge_main_into(&wt, "sync").expect("sync") {
        MergeOutcome::Refused { reason } => assert!(reason.contains("tracked changes"), "reason: {reason}"),
        other => panic!("expected Refused, got {other:?}"),
    }
    assert_eq!(std::fs::read_to_string(wt.path.join("base.txt")).expect("read"), "dirty\n");
}
