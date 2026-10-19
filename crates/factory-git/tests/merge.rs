mod common;

use common::*;
use factory_git::{BaseState, GitError, MergeOutcome, Preflight};

const MSG: &str = "chore(factory): merge job 001 x";

#[test]
fn preflight_is_ready_on_a_clean_main_and_ignores_untracked_files() {
    let fx = Fixture::new();
    std::fs::write(fx.path.join("scratch.txt"), "x").expect("write");
    match fx.repo().preflight_main().expect("preflight") {
        Preflight::Ready { main_sha } => assert_eq!(main_sha, fx.main_sha()),
        other => panic!("expected Ready, got {other:?}"),
    }
}

fn busy_reason(fx: &Fixture) -> String {
    match fx.repo().preflight_main().expect("preflight") {
        Preflight::Busy { reason } => reason,
        other => panic!("expected Busy, got {other:?}"),
    }
}

#[test]
fn preflight_is_busy_for_tracked_changes() {
    let fx = Fixture::new();
    std::fs::write(fx.path.join("base.txt"), "changed\n").expect("write");
    assert!(busy_reason(&fx).contains("tracked changes"));
}

#[test]
fn preflight_is_busy_when_main_is_not_checked_out() {
    let fx = Fixture::new();
    let git = fx.git();
    let main = git
        .find_commit(git2::Oid::from_str(&fx.main_sha()).expect("oid"))
        .expect("c");
    git.branch("other", &main, false).expect("branch");
    git.set_head("refs/heads/other").expect("set head");
    assert!(busy_reason(&fx).contains("not on main"));
}

#[test]
fn preflight_is_busy_in_the_middle_of_a_merge() {
    let fx = Fixture::new();
    let merge_head = fx.path.join(".git").join("MERGE_HEAD");
    std::fs::write(merge_head, format!("{}\n", fx.main_sha())).expect("write");
    assert!(busy_reason(&fx).contains("in progress"));
}

#[test]
fn base_state_reports_current_main_moved_merged_and_tokens() {
    let fx = Fixture::new();
    let (base, head) = fx.job_branch("factory/001-x", &[("job.txt", "j\n")]);
    let repo = fx.repo();
    let check = repo.base_state(&base, &head).expect("state");
    assert_eq!(check.state, BaseState::Current);
    assert_eq!(check.main_sha, base);
    assert_eq!(check.state.as_token(), "ok");

    commit_files(&fx.git(), &[("other.txt", "o\n")], "main moves");
    let moved = repo.base_state(&base, &head).expect("moved");
    assert_eq!(moved.state, BaseState::MainMoved);
    assert_eq!(moved.state.as_token(), "main-moved");

    point_branch_at(&fx.git(), "main", &head);
    let merged = repo.base_state(&base, &head).expect("merged");
    assert_eq!(merged.state, BaseState::Merged);
    assert_eq!(merged.state.as_token(), "merged");
}

#[test]
fn base_state_says_base_not_main_when_the_base_has_unrelated_commits() {
    let fx = Fixture::new();
    let (_, unrelated) = fx.job_branch("factory/001-x", &[("unrelated.txt", "u\n")]);
    // The job base is the unrelated commit, which is not on main.
    let head =
        commit_to_branch(&fx.git(), "factory/001-x", &[("mine.txt", "m\n")], "mine").to_string();
    let check = fx.repo().base_state(&unrelated, &head).expect("state");
    assert_eq!(check.state, BaseState::BaseNotMain);
    assert_eq!(check.state.as_token(), "base-not-main");
}

#[test]
fn base_state_refuses_a_job_with_no_commits() {
    let fx = Fixture::new();
    let sha = fx.main_sha();
    assert!(matches!(
        fx.repo().base_state(&sha, &sha),
        Err(GitError::Refused(_))
    ));
}

#[test]
fn merge_no_ff_builds_a_two_parent_commit_with_the_fixed_message() {
    let fx = Fixture::new();
    let (base, head) = fx.job_branch("factory/001-x", &[("job.txt", "j\n")]);
    let repo = fx.repo();
    let merge_sha = match repo
        .merge_no_ff("factory/001-x", &head, MSG)
        .expect("merge")
    {
        MergeOutcome::Merged { merge_sha } => merge_sha,
        other => panic!("expected Merged, got {other:?}"),
    };
    let git = fx.git();
    assert_eq!(fx.main_sha(), merge_sha);
    let commit = git
        .find_commit(git2::Oid::from_str(&merge_sha).expect("oid"))
        .expect("commit");
    assert_eq!(commit.parent_count(), 2);
    assert_eq!(commit.parent_id(0).expect("p0").to_string(), base);
    assert_eq!(commit.parent_id(1).expect("p1").to_string(), head);
    assert_eq!(commit.message().expect("msg").trim_end(), MSG);
    assert!(
        fx.path.join("job.txt").is_file(),
        "the working tree must have the job file"
    );
    assert!(matches!(
        repo.preflight_main().expect("pf"),
        Preflight::Ready { .. }
    ));
    // A merged job stays merged after its branch is deleted.
    git.find_branch("factory/001-x", git2::BranchType::Local)
        .expect("b")
        .delete()
        .expect("del");
    assert_eq!(
        repo.is_merged(&base, &head).expect("merged"),
        Some(merge_sha)
    );
}

#[test]
fn merge_no_ff_refuses_when_the_branch_tip_is_not_the_reviewed_head() {
    let fx = Fixture::new();
    let (_, head) = fx.job_branch("factory/001-x", &[("job.txt", "j\n")]);
    commit_to_branch(
        &fx.git(),
        "factory/001-x",
        &[("late.txt", "l\n")],
        "unreviewed",
    );
    let before = fx.main_sha();
    match fx
        .repo()
        .merge_no_ff("factory/001-x", &head, MSG)
        .expect("outcome")
    {
        MergeOutcome::Refused { reason } => {
            assert!(reason.contains("reviewed head"), "reason: {reason}")
        }
        other => panic!("expected Refused, got {other:?}"),
    }
    assert_eq!(fx.main_sha(), before);
    assert!(!fx.path.join("late.txt").exists());
}

#[test]
fn merge_no_ff_reports_a_conflict_and_leaves_main_untouched() {
    let fx = Fixture::new();
    let (_, head) = fx.job_branch("factory/001-x", &[("base.txt", "job\n")]);
    commit_files(
        &fx.git(),
        &[("base.txt", "main\n")],
        "main edits the same file",
    );
    let before = fx.main_sha();
    match fx
        .repo()
        .merge_no_ff("factory/001-x", &head, MSG)
        .expect("outcome")
    {
        MergeOutcome::Conflict { paths } => assert_eq!(paths, vec!["base.txt".to_string()]),
        other => panic!("expected Conflict, got {other:?}"),
    }
    assert_eq!(fx.main_sha(), before);
    assert_eq!(
        std::fs::read_to_string(fx.path.join("base.txt")).expect("read"),
        "main\n"
    );
    assert!(matches!(
        fx.repo().preflight_main().expect("pf"),
        Preflight::Ready { .. }
    ));
}

#[test]
fn merge_no_ff_refuses_when_an_untracked_file_would_be_overwritten() {
    let fx = Fixture::new();
    let (_, head) = fx.job_branch("factory/001-x", &[("clash.txt", "from job\n")]);
    std::fs::write(fx.path.join("clash.txt"), "mine\n").expect("write");
    let before = fx.main_sha();
    match fx
        .repo()
        .merge_no_ff("factory/001-x", &head, MSG)
        .expect("outcome")
    {
        MergeOutcome::Refused { .. } => {}
        other => panic!("expected Refused, got {other:?}"),
    }
    assert_eq!(fx.main_sha(), before);
    assert_eq!(
        std::fs::read_to_string(fx.path.join("clash.txt")).expect("read"),
        "mine\n"
    );
    assert!(matches!(
        fx.repo().preflight_main().expect("pf"),
        Preflight::Ready { .. }
    ));
}

#[test]
fn merge_no_ff_refuses_a_busy_main_and_keeps_the_local_edit() {
    let fx = Fixture::new();
    let (_, head) = fx.job_branch("factory/001-x", &[("job.txt", "j\n")]);
    std::fs::write(fx.path.join("base.txt"), "local edit\n").expect("write");
    let before = fx.main_sha();
    match fx
        .repo()
        .merge_no_ff("factory/001-x", &head, MSG)
        .expect("outcome")
    {
        MergeOutcome::Refused { reason } => {
            assert!(reason.contains("tracked changes"), "reason: {reason}")
        }
        other => panic!("expected Refused, got {other:?}"),
    }
    assert_eq!(fx.main_sha(), before);
    assert_eq!(
        std::fs::read_to_string(fx.path.join("base.txt")).expect("read"),
        "local edit\n"
    );
}
