mod common;
use common::Fixture;

#[test]
fn a_job_goes_from_new_to_merged_through_the_cli() {
    let fx = Fixture::new();
    let plans = fx.write_plan("plan-01-hello.md", 1, "none");
    fx.run(&["new", "hello", "say hello", "--plans", plans.to_str().expect("utf8")]).expect("new");
    let before = fx.main_tip();
    assert_eq!(fx.run(&["run", "1"]).expect("run"), "merged");
    assert_ne!(fx.main_tip(), before);
    assert!(fx.main_message().starts_with("chore(factory): merge job 001 hello"));
    assert!(fx.main_file("docs/001-0.txt").exists());
    let status = fx.run(&["status"]).expect("status");
    assert!(status.contains("merged") && status.contains("1/1"), "{status}");
    let show = fx.run(&["show", "1"]).expect("show");
    assert!(show.contains("\"stage\": \"merged\""), "{show}");
    assert!(fx.run(&["tokens", "1"]).expect("tokens").contains("weighted"));
    assert!(fx.worktree_path(1).exists());
    fx.run(&["cleanup", "1"]).expect("cleanup");
    assert!(!fx.worktree_path(1).exists());
}

#[test]
fn approve_merges_a_risky_job_that_run_parked() {
    let fx = Fixture::with_risk_path("docs/**");
    let plans = fx.write_plan("plan-01-hello.md", 1, "none");
    fx.run(&["new", "hello", "touch risky path", "--plans", plans.to_str().expect("utf8")]).expect("new");
    let before = fx.main_tip();
    assert!(fx.run(&["run", "1"]).expect("run").starts_with("needs-human"));
    assert_eq!(fx.main_tip(), before, "a parked job leaves main untouched");
    assert!(fx.run(&["approve", "1"]).expect("approve").starts_with("merged"));
    assert!(fx.main_file("docs/001-0.txt").exists());
}

#[test]
fn rework_sends_a_parked_job_back_to_build() {
    let fx = Fixture::with_risk_path("docs/**");
    let plans = fx.write_plan("plan-01-hello.md", 1, "none");
    fx.run(&["new", "hello", "x", "--plans", plans.to_str().expect("utf8")]).expect("new");
    assert!(fx.run(&["run", "1"]).expect("run").starts_with("needs-human"));
    fx.run(&["rework", "1", "use a shorter name"]).expect("rework");
    let show = fx.run(&["show", "1"]).expect("show");
    assert!(show.contains("\"stage\": \"build\""), "{show}");
}

#[test]
fn rework_revives_a_cancelled_job_so_approve_can_merge_it() {
    let fx = Fixture::with_risk_path("docs/**");
    let plans = fx.write_plan("plan-01-hello.md", 1, "none");
    fx.run(&["new", "hello", "x", "--plans", plans.to_str().expect("utf8")]).expect("new");
    assert!(fx.run(&["run", "1"]).expect("run").starts_with("needs-human"));
    fx.run(&["cancel", "1"]).expect("cancel");
    let e = fx.run(&["approve", "1"]).expect_err("cancelled");
    assert!(e.to_string().contains("was cancelled"), "{e}");
    fx.run(&["rework", "1", "revive the job"]).expect("rework");
    assert!(fx.run(&["run", "1"]).expect("run again").starts_with("needs-human"));
    assert!(fx.run(&["approve", "1"]).expect("approve").starts_with("merged"));
}

#[test]
fn guard_rails_refuse_with_clear_messages() {
    let fx = Fixture::new();
    let plans = fx.write_plan("plan-01-hello.md", 1, "none");
    fx.run(&["new", "hello", "x", "--plans", plans.to_str().expect("utf8")]).expect("new");
    let e = fx.run(&["rework", "1", "note"]).expect_err("not needs-human");
    assert!(e.to_string().contains("needs-human"), "{e}");
    let e = fx.run(&["cleanup", "1"]).expect_err("not merged");
    assert!(e.to_string().contains("merged or cancelled"), "{e}");
    fx.run(&["cancel", "1"]).expect("cancel");
    let e = fx.run(&["rework", "1", "a\nb"]).expect_err("multi-line");
    assert!(e.to_string().contains("one line"), "{e}");
    // A second driver cannot take a live lease.
    fx.hold_lease("001", "someone-else");
    let e = fx.run(&["run", "1"]).expect_err("leased");
    assert!(e.to_string().contains("leased by someone-else"), "{e}");
}

#[test]
fn kb_refresh_without_a_kb_path_says_so() {
    let fx = Fixture::new();
    let plans = fx.write_plan("plan-01-hello.md", 1, "none");
    fx.run(&["new", "hello", "x", "--plans", plans.to_str().expect("utf8")]).expect("new");
    fx.run(&["run", "1"]).expect("run");
    assert_eq!(fx.run(&["kb-refresh", "--job", "1"]).expect("kb"), "kb_path is not set");
}

#[test]
fn kb_refresh_lists_the_pages() {
    let fx = Fixture::new();
    let kb = fx.tmp.path().join("kb");
    std::fs::create_dir_all(&kb).expect("kb");
    let cfg = fx.root.join(".claude/factory/config.toml");
    let text = std::fs::read_to_string(&cfg).expect("config");
    std::fs::write(&cfg, format!("{text}kb_path = \"{}\"\n", kb.display())).expect("config");
    // The real repo ignores `.superpowers/`, and the tamper check relies on it.
    std::fs::write(fx.root.join(".gitignore"), ".superpowers/\n").expect("ignore");
    let plans = fx.write_plan("plan-01-hello.md", 1, "none");
    fx.run(&["new", "hello", "x", "--plans", plans.to_str().expect("utf8")]).expect("new");
    fx.run(&["run", "1"]).expect("run");
    // The role runner answers non-builder roles with a verdict text, which counts as a page line.
    let out = fx.run(&["kb-refresh", "--job", "1"]).expect("kb");
    assert!(out.starts_with("kb refreshed, pages:"), "{out}");
}

#[test]
fn kb_refresh_refuses_while_another_driver_holds_the_lease() {
    let fx = Fixture::new();
    let plans = fx.write_plan("plan-01-hello.md", 1, "none");
    fx.run(&["new", "hello", "x", "--plans", plans.to_str().expect("utf8")]).expect("new");
    fx.run(&["run", "1"]).expect("run");
    fx.hold_lease("001", "someone-else");
    let e = fx.run(&["kb-refresh", "--job", "1"]).expect_err("leased");
    assert!(e.to_string().contains("leased by someone-else"), "{e}");
}

#[test]
fn kb_refresh_refuses_a_job_that_is_not_merged() {
    let fx = Fixture::new();
    let plans = fx.write_plan("plan-01-hello.md", 1, "none");
    fx.run(&["new", "hello", "x", "--plans", plans.to_str().expect("utf8")]).expect("new");
    let e = fx.run(&["kb-refresh", "--job", "1"]).expect_err("not merged");
    assert!(e.to_string().contains("not merged"), "{e}");
}

#[test]
fn approve_refuses_a_job_with_a_pending_batch_commit() {
    let fx = Fixture::with_risk_path("docs/**");
    let plans = fx.write_plan("plan-01-hello.md", 1, "none");
    fx.run(&["new", "hello", "x", "--plans", plans.to_str().expect("utf8")]).expect("new");
    assert!(fx.run(&["run", "1"]).expect("run").starts_with("needs-human"));
    let store = factory_engine::BoardStore::new(
        &factory_core::Layout::in_repo(&fx.root).board_file(),
        std::time::Duration::from_secs(15),
    );
    store
        .update_job("001", |j| {
            j.batch_snapshot = Some("abc1234".to_string());
            Ok(())
        })
        .expect("mark");
    let e = fx.run(&["approve", "1"]).expect_err("pending batch");
    assert!(e.to_string().contains("pending batch commit"), "{e}");
}

fn branch_exists(fx: &Fixture, name: &str) -> bool {
    git2::Repository::open(&fx.root).expect("open").find_branch(name, git2::BranchType::Local).is_ok()
}

#[test]
fn cleanup_twice_succeeds_and_says_nothing_was_left() {
    let fx = Fixture::new();
    let plans = fx.write_plan("plan-01-hello.md", 1, "none");
    fx.run(&["new", "hello", "say hello", "--plans", plans.to_str().expect("utf8")]).expect("new");
    assert_eq!(fx.run(&["run", "1"]).expect("run"), "merged");
    fx.run(&["cleanup", "1"]).expect("first cleanup");
    let second = fx.run(&["cleanup", "1"]).expect("second cleanup");
    assert_eq!(second, "nothing to remove for job 001, the worktree and branch are already gone");
}

#[test]
fn cleanup_deletes_the_branch_when_the_worktree_folder_was_removed_by_hand() {
    let fx = Fixture::with_risk_path("docs/**");
    let plans = fx.write_plan("plan-01-hello.md", 1, "none");
    fx.run(&["new", "hello", "x", "--plans", plans.to_str().expect("utf8")]).expect("new");
    assert!(fx.run(&["run", "1"]).expect("run").starts_with("needs-human"));
    fx.run(&["cancel", "1"]).expect("cancel");
    std::fs::remove_dir_all(fx.worktree_path(1)).expect("rm worktree");
    assert!(branch_exists(&fx, "factory/001-hello"));
    fx.run(&["cleanup", "1", "--discard-commits"]).expect("cleanup");
    assert!(!branch_exists(&fx, "factory/001-hello"));
}

#[test]
fn cleanup_of_a_merged_job_works_when_the_worktree_folder_was_removed_by_hand() {
    let fx = Fixture::new();
    let plans = fx.write_plan("plan-01-hello.md", 1, "none");
    fx.run(&["new", "hello", "say hello", "--plans", plans.to_str().expect("utf8")]).expect("new");
    assert_eq!(fx.run(&["run", "1"]).expect("run"), "merged");
    std::fs::remove_dir_all(fx.worktree_path(1)).expect("rm worktree");
    assert!(branch_exists(&fx, "factory/001-hello"));
    fx.run(&["cleanup", "1"]).expect("cleanup");
    assert!(!branch_exists(&fx, "factory/001-hello"));
}

#[test]
fn cleanup_still_refuses_unmerged_commits_when_the_worktree_folder_is_gone() {
    let fx = Fixture::with_risk_path("docs/**");
    let plans = fx.write_plan("plan-01-hello.md", 1, "none");
    fx.run(&["new", "hello", "x", "--plans", plans.to_str().expect("utf8")]).expect("new");
    assert!(fx.run(&["run", "1"]).expect("run").starts_with("needs-human"));
    fx.run(&["cancel", "1"]).expect("cancel");
    std::fs::remove_dir_all(fx.worktree_path(1)).expect("rm worktree");
    let e = fx.run(&["cleanup", "1"]).expect_err("unmerged");
    assert!(e.to_string().contains("--discard-commits"), "{e}");
    assert!(branch_exists(&fx, "factory/001-hello"));
}

/// Leaves only `.claude/agents` in the worktree folder, like agent files written after the worktree was gone.
fn leave_debris(fx: &Fixture) {
    let wt = fx.worktree_path(1);
    let _ = std::fs::remove_dir_all(&wt);
    std::fs::create_dir_all(wt.join(".claude/agents")).expect("debris");
}

#[test]
fn cleanup_removes_a_leftover_folder_when_the_branch_is_gone() {
    let fx = Fixture::new();
    let plans = fx.write_plan("plan-01-hello.md", 1, "none");
    fx.run(&["new", "hello", "say hello", "--plans", plans.to_str().expect("utf8")]).expect("new");
    assert_eq!(fx.run(&["run", "1"]).expect("run"), "merged");
    fx.run(&["cleanup", "1"]).expect("first cleanup");
    leave_debris(&fx);
    let out = fx.run(&["cleanup", "1"]).expect("cleanup");
    assert_eq!(out, "removed leftover folder for job 001; the worktree and branch were already gone");
    assert!(!fx.worktree_path(1).exists());
}

#[test]
fn cleanup_removes_a_leftover_folder_and_the_merged_branch() {
    let fx = Fixture::new();
    let plans = fx.write_plan("plan-01-hello.md", 1, "none");
    fx.run(&["new", "hello", "say hello", "--plans", plans.to_str().expect("utf8")]).expect("new");
    assert_eq!(fx.run(&["run", "1"]).expect("run"), "merged");
    leave_debris(&fx);
    fx.run(&["cleanup", "1"]).expect("cleanup");
    assert!(!fx.worktree_path(1).exists());
    assert!(!branch_exists(&fx, "factory/001-hello"));
}

#[test]
fn cleanup_keeps_debris_and_branch_when_commits_are_unmerged() {
    let fx = Fixture::with_risk_path("docs/**");
    let plans = fx.write_plan("plan-01-hello.md", 1, "none");
    fx.run(&["new", "hello", "x", "--plans", plans.to_str().expect("utf8")]).expect("new");
    assert!(fx.run(&["run", "1"]).expect("run").starts_with("needs-human"));
    fx.run(&["cancel", "1"]).expect("cancel");
    leave_debris(&fx);
    let e = fx.run(&["cleanup", "1"]).expect_err("unmerged");
    assert!(e.to_string().contains("--discard-commits"), "{e}");
    assert!(fx.worktree_path(1).exists());
    assert!(branch_exists(&fx, "factory/001-hello"));
    fx.run(&["cleanup", "1", "--discard-commits"]).expect("forced");
    assert!(!fx.worktree_path(1).exists());
    assert!(!branch_exists(&fx, "factory/001-hello"));
}
