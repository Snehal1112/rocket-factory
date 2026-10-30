mod common;
mod support;
use common::Fixture;
use support::{build, review, Scenario, TestResult, APPROVE, CHANGES};

#[test]
fn a_job_goes_from_new_to_merged_through_the_cli() {
    let fx = Fixture::new();
    let plans = fx.write_plan("plan-01-hello.md", 1, "none");
    fx.run(&[
        "new",
        "hello",
        "say hello",
        "--plans",
        plans.to_str().expect("utf8"),
    ])
    .expect("new");
    let before = fx.main_tip();
    assert_eq!(fx.run(&["run", "1"]).expect("run"), "merged");
    assert_ne!(fx.main_tip(), before);
    assert!(fx
        .main_message()
        .starts_with("chore(factory): merge job 001 hello"));
    assert!(fx.main_file("docs/001-0.txt").exists());
    let status = fx.run(&["status"]).expect("status");
    assert!(
        status.contains("merged") && status.contains("1/1"),
        "{status}"
    );
    let show = fx.run(&["show", "1"]).expect("show");
    assert!(show.contains("\"stage\": \"merged\""), "{show}");
    assert!(fx
        .run(&["tokens", "1"])
        .expect("tokens")
        .contains("weighted"));
    assert!(fx.worktree_path(1).exists());
    fx.run(&["cleanup", "1"]).expect("cleanup");
    assert!(!fx.worktree_path(1).exists());
}

#[test]
fn approve_merges_a_risky_job_that_run_parked() {
    let fx = Fixture::with_risk_path("docs/**");
    let plans = fx.write_plan("plan-01-hello.md", 1, "none");
    fx.run(&[
        "new",
        "hello",
        "touch risky path",
        "--plans",
        plans.to_str().expect("utf8"),
    ])
    .expect("new");
    let before = fx.main_tip();
    assert!(fx
        .run(&["run", "1"])
        .expect("run")
        .starts_with("needs-human"));
    assert_eq!(fx.main_tip(), before, "a parked job leaves main untouched");
    assert!(fx
        .run(&["approve", "1"])
        .expect("approve")
        .starts_with("merged"));
    assert!(fx.main_file("docs/001-0.txt").exists());
}

#[test]
fn rework_sends_a_parked_job_back_to_build() {
    let fx = Fixture::with_risk_path("docs/**");
    let plans = fx.write_plan("plan-01-hello.md", 1, "none");
    fx.run(&[
        "new",
        "hello",
        "x",
        "--plans",
        plans.to_str().expect("utf8"),
    ])
    .expect("new");
    assert!(fx
        .run(&["run", "1"])
        .expect("run")
        .starts_with("needs-human"));
    fx.run(&["rework", "1", "use a shorter name"])
        .expect("rework");
    let show = fx.run(&["show", "1"]).expect("show");
    assert!(show.contains("\"stage\": \"build\""), "{show}");
}

#[test]
fn rework_revives_a_cancelled_job_so_approve_can_merge_it() {
    let fx = Fixture::with_risk_path("docs/**");
    let plans = fx.write_plan("plan-01-hello.md", 1, "none");
    fx.run(&[
        "new",
        "hello",
        "x",
        "--plans",
        plans.to_str().expect("utf8"),
    ])
    .expect("new");
    assert!(fx
        .run(&["run", "1"])
        .expect("run")
        .starts_with("needs-human"));
    fx.run(&["cancel", "1"]).expect("cancel");
    let e = fx.run(&["approve", "1"]).expect_err("cancelled");
    assert!(e.to_string().contains("was cancelled"), "{e}");
    fx.run(&["rework", "1", "revive the job"]).expect("rework");
    assert!(fx
        .run(&["run", "1"])
        .expect("run again")
        .starts_with("needs-human"));
    assert!(fx
        .run(&["approve", "1"])
        .expect("approve")
        .starts_with("merged"));
}

#[test]
fn guard_rails_refuse_with_clear_messages() {
    let fx = Fixture::new();
    let plans = fx.write_plan("plan-01-hello.md", 1, "none");
    fx.run(&[
        "new",
        "hello",
        "x",
        "--plans",
        plans.to_str().expect("utf8"),
    ])
    .expect("new");
    let e = fx
        .run(&["rework", "1", "note"])
        .expect_err("not needs-human");
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
    fx.run(&[
        "new",
        "hello",
        "x",
        "--plans",
        plans.to_str().expect("utf8"),
    ])
    .expect("new");
    fx.run(&["run", "1"]).expect("run");
    assert_eq!(
        fx.run(&["kb-refresh", "--job", "1"]).expect("kb"),
        "kb_path is not set"
    );
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
    fx.run(&[
        "new",
        "hello",
        "x",
        "--plans",
        plans.to_str().expect("utf8"),
    ])
    .expect("new");
    fx.run(&["run", "1"]).expect("run");
    // The role runner answers non-builder roles with a verdict text, which counts as a page line.
    let out = fx.run(&["kb-refresh", "--job", "1"]).expect("kb");
    assert!(out.starts_with("kb refreshed, pages:"), "{out}");
}

#[test]
fn kb_refresh_refuses_while_another_driver_holds_the_lease() {
    let fx = Fixture::new();
    let plans = fx.write_plan("plan-01-hello.md", 1, "none");
    fx.run(&[
        "new",
        "hello",
        "x",
        "--plans",
        plans.to_str().expect("utf8"),
    ])
    .expect("new");
    fx.run(&["run", "1"]).expect("run");
    fx.hold_lease("001", "someone-else");
    let e = fx.run(&["kb-refresh", "--job", "1"]).expect_err("leased");
    assert!(e.to_string().contains("leased by someone-else"), "{e}");
}

#[test]
fn kb_refresh_refuses_a_job_that_is_not_merged() {
    let fx = Fixture::new();
    let plans = fx.write_plan("plan-01-hello.md", 1, "none");
    fx.run(&[
        "new",
        "hello",
        "x",
        "--plans",
        plans.to_str().expect("utf8"),
    ])
    .expect("new");
    let e = fx
        .run(&["kb-refresh", "--job", "1"])
        .expect_err("not merged");
    assert!(e.to_string().contains("not merged"), "{e}");
}

#[test]
fn approve_refuses_a_job_with_a_pending_batch_commit() {
    let fx = Fixture::with_risk_path("docs/**");
    let plans = fx.write_plan("plan-01-hello.md", 1, "none");
    fx.run(&[
        "new",
        "hello",
        "x",
        "--plans",
        plans.to_str().expect("utf8"),
    ])
    .expect("new");
    assert!(fx
        .run(&["run", "1"])
        .expect("run")
        .starts_with("needs-human"));
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

/// A risky job parked in needs-human, with its code review reset as if the reviewer never ran.
fn parked_without_review(fx: &Fixture) {
    let plans = fx.write_plan("plan-01-hello.md", 1, "none");
    fx.run(&[
        "new",
        "hello",
        "x",
        "--plans",
        plans.to_str().expect("utf8"),
    ])
    .expect("new");
    assert!(fx
        .run(&["run", "1"])
        .expect("run")
        .starts_with("needs-human"));
    let store = factory_engine::BoardStore::new(
        &factory_core::Layout::in_repo(&fx.root).board_file(),
        std::time::Duration::from_secs(15),
    );
    store
        .update_job("001", |j| {
            j.set_review("code", factory_core::ReviewState::Pending);
            Ok(())
        })
        .expect("reset review");
}

#[test]
fn approve_refuses_a_needs_human_job_whose_review_never_ran() {
    let fx = Fixture::with_risk_path("docs/**");
    parked_without_review(&fx);
    let before = fx.main_tip();
    let e = fx.run(&["approve", "1"]).expect_err("no review");
    let text = e.to_string();
    assert!(text.contains("job 001 has no code review yet"), "{text}");
    assert!(
        text.contains("rework") && text.contains("--without-review"),
        "{text}"
    );
    assert_eq!(fx.main_tip(), before);
    let show = fx.run(&["show", "1"]).expect("show");
    assert!(show.contains("\"stage\": \"needs-human\""), "{show}");
}

#[test]
fn approve_without_review_merges_and_the_ledger_says_so() {
    let fx = Fixture::with_risk_path("docs/**");
    parked_without_review(&fx);
    assert!(fx
        .run(&["approve", "1", "--without-review"])
        .expect("approve")
        .starts_with("merged"));
    let sdd = fx.root.join(".superpowers/sdd");
    let mut ledgers = String::new();
    for entry in std::fs::read_dir(&sdd).expect("sdd").flatten() {
        ledgers.push_str(
            &std::fs::read_to_string(entry.path().join("progress.md")).unwrap_or_default(),
        );
    }
    assert!(
        ledgers.contains("approved by the user without a review"),
        "{ledgers}"
    );
}

fn branch_exists(fx: &Fixture, name: &str) -> bool {
    git2::Repository::open(&fx.root)
        .expect("open")
        .find_branch(name, git2::BranchType::Local)
        .is_ok()
}

#[test]
fn cleanup_twice_succeeds_and_says_nothing_was_left() {
    let fx = Fixture::new();
    let plans = fx.write_plan("plan-01-hello.md", 1, "none");
    fx.run(&[
        "new",
        "hello",
        "say hello",
        "--plans",
        plans.to_str().expect("utf8"),
    ])
    .expect("new");
    assert_eq!(fx.run(&["run", "1"]).expect("run"), "merged");
    fx.run(&["cleanup", "1"]).expect("first cleanup");
    let second = fx.run(&["cleanup", "1"]).expect("second cleanup");
    assert_eq!(
        second,
        "nothing to remove for job 001, the worktree and branch are already gone"
    );
}

#[test]
fn cleanup_deletes_the_branch_when_the_worktree_folder_was_removed_by_hand() {
    let fx = Fixture::with_risk_path("docs/**");
    let plans = fx.write_plan("plan-01-hello.md", 1, "none");
    fx.run(&[
        "new",
        "hello",
        "x",
        "--plans",
        plans.to_str().expect("utf8"),
    ])
    .expect("new");
    assert!(fx
        .run(&["run", "1"])
        .expect("run")
        .starts_with("needs-human"));
    fx.run(&["cancel", "1"]).expect("cancel");
    std::fs::remove_dir_all(fx.worktree_path(1)).expect("rm worktree");
    assert!(branch_exists(&fx, "factory/001-hello"));
    fx.run(&["cleanup", "1", "--discard-commits"])
        .expect("cleanup");
    assert!(!branch_exists(&fx, "factory/001-hello"));
}

#[test]
fn cleanup_of_a_merged_job_works_when_the_worktree_folder_was_removed_by_hand() {
    let fx = Fixture::new();
    let plans = fx.write_plan("plan-01-hello.md", 1, "none");
    fx.run(&[
        "new",
        "hello",
        "say hello",
        "--plans",
        plans.to_str().expect("utf8"),
    ])
    .expect("new");
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
    fx.run(&[
        "new",
        "hello",
        "x",
        "--plans",
        plans.to_str().expect("utf8"),
    ])
    .expect("new");
    assert!(fx
        .run(&["run", "1"])
        .expect("run")
        .starts_with("needs-human"));
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
    fx.run(&[
        "new",
        "hello",
        "say hello",
        "--plans",
        plans.to_str().expect("utf8"),
    ])
    .expect("new");
    assert_eq!(fx.run(&["run", "1"]).expect("run"), "merged");
    fx.run(&["cleanup", "1"]).expect("first cleanup");
    leave_debris(&fx);
    let out = fx.run(&["cleanup", "1"]).expect("cleanup");
    assert_eq!(
        out,
        "removed leftover folder for job 001; the worktree and branch were already gone"
    );
    assert!(!fx.worktree_path(1).exists());
}

#[test]
fn cleanup_removes_a_leftover_folder_and_the_merged_branch() {
    let fx = Fixture::new();
    let plans = fx.write_plan("plan-01-hello.md", 1, "none");
    fx.run(&[
        "new",
        "hello",
        "say hello",
        "--plans",
        plans.to_str().expect("utf8"),
    ])
    .expect("new");
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
    fx.run(&[
        "new",
        "hello",
        "x",
        "--plans",
        plans.to_str().expect("utf8"),
    ])
    .expect("new");
    assert!(fx
        .run(&["run", "1"])
        .expect("run")
        .starts_with("needs-human"));
    fx.run(&["cancel", "1"]).expect("cancel");
    leave_debris(&fx);
    let e = fx.run(&["cleanup", "1"]).expect_err("unmerged");
    assert!(e.to_string().contains("--discard-commits"), "{e}");
    assert!(fx.worktree_path(1).exists());
    assert!(branch_exists(&fx, "factory/001-hello"));
    fx.run(&["cleanup", "1", "--discard-commits"])
        .expect("forced");
    assert!(!fx.worktree_path(1).exists());
    assert!(!branch_exists(&fx, "factory/001-hello"));
}

fn board_ids(fx: &Fixture) -> Vec<String> {
    let store = factory_engine::BoardStore::new(
        &factory_core::Layout::in_repo(&fx.root).board_file(),
        std::time::Duration::from_secs(15),
    );
    store
        .load()
        .expect("board")
        .jobs
        .into_iter()
        .map(|j| j.id)
        .collect()
}

fn parked_and_cancelled(fx: &Fixture, slug: &str) {
    let plans = fx.write_plan(&format!("plan-01-{slug}.md"), 1, "none");
    fx.run(&["new", slug, "x", "--plans", plans.to_str().expect("utf8")])
        .expect("new");
}

#[test]
fn cleanup_of_a_cancelled_job_removes_its_card_and_keeps_the_job_folder() {
    let fx = Fixture::with_risk_path("docs/**");
    parked_and_cancelled(&fx, "hello");
    assert!(fx
        .run(&["run", "1"])
        .expect("run")
        .starts_with("needs-human"));
    fx.run(&["cancel", "1"]).expect("cancel");
    let jobs_dir = factory_core::Layout::in_repo(&fx.root).jobs_dir;
    let folders_before = std::fs::read_dir(&jobs_dir).expect("jobs").count();
    let out = fx
        .run(&["cleanup", "1", "--discard-commits"])
        .expect("cleanup");
    assert!(out.ends_with("; card removed from the board"), "{out}");
    assert!(board_ids(&fx).is_empty());
    assert_eq!(
        std::fs::read_dir(&jobs_dir).expect("jobs").count(),
        folders_before,
        "the job folder stays"
    );
    let e = fx.run(&["cleanup", "1"]).expect_err("gone");
    assert!(e.to_string().contains("no job"), "{e}");
}

#[test]
fn cleanup_of_a_merged_job_keeps_its_card() {
    let fx = Fixture::new();
    parked_and_cancelled(&fx, "hello");
    assert_eq!(fx.run(&["run", "1"]).expect("run"), "merged");
    let out = fx.run(&["cleanup", "1"]).expect("cleanup");
    assert!(!out.contains("card"), "{out}");
    assert_eq!(board_ids(&fx), vec!["001".to_string()]);
}

#[derive(Clone, Copy)]
enum Sibling {
    Queued,
    Merged,
    Review,
    Cancelled,
}

/// Jobs 001 and 002 in series S3, 001 cancelled and 002 in the given state. The fixture also gets a
/// registered home and project name `p`.
fn series_pair(sibling: Sibling) -> (Fixture, factory_core::Home) {
    let mut fx = Fixture::with_risk_path("docs/**");
    parked_and_cancelled(&fx, "hello");
    parked_and_cancelled(&fx, "other");
    let store = factory_engine::BoardStore::new(
        &factory_core::Layout::in_repo(&fx.root).board_file(),
        std::time::Duration::from_secs(15),
    );
    store
        .update(|b| {
            for (n, j) in b.jobs.iter_mut().enumerate() {
                j.series = Some(factory_core::SeriesPos {
                    plan_file: "p.md".into(),
                    index: n as u32 + 1,
                    total: 2,
                    id: Some("S3".into()),
                });
                if n == 1 {
                    let stage = match sibling {
                        Sibling::Merged => Some(factory_core::Stage::Merged),
                        Sibling::Review => Some(factory_core::Stage::Review),
                        _ => None,
                    };
                    if let Some(stage) = stage {
                        j.set_stage(stage, None, false, factory_engine::now_secs())
                            .expect("stage");
                    }
                }
            }
            Ok(())
        })
        .expect("series");
    fx.run(&["cancel", "1"]).expect("cancel");
    if matches!(sibling, Sibling::Cancelled) {
        fx.run(&["cancel", "2"]).expect("cancel sibling");
    }
    let home = factory_core::Home::at(fx.tmp.path().join("home"));
    fx.ctx.home = Some(home.clone());
    fx.ctx.project_name = Some("p".to_string());
    (fx, home)
}

fn queue_request(
    home: &factory_core::Home,
    project: &str,
    action: factory_supervisor::Action,
    job: &str,
    series: bool,
) -> factory_supervisor::Request {
    let request =
        factory_supervisor::Request::new(project, action, job, series, 1).expect("request");
    factory_supervisor::queue::enqueue(home, &request).expect("enqueue");
    request
}

fn both_ids() -> Vec<String> {
    vec!["001".to_string(), "002".to_string()]
}

fn only_second() -> Vec<String> {
    vec!["002".to_string()]
}

fn cleanup_one(fx: &Fixture) -> String {
    fx.run(&["cleanup", "1", "--discard-commits"])
        .expect("cleanup")
}

#[test]
fn cleanup_removes_cancelled_card_when_series_sibling_is_merged() {
    let (fx, _home) = series_pair(Sibling::Merged);
    let out = cleanup_one(&fx);
    assert!(out.ends_with("; card removed from the board"), "{out}");
    assert_eq!(board_ids(&fx), only_second());
}

#[test]
fn cleanup_removes_cancelled_card_when_series_sibling_is_cancelled() {
    let (fx, _home) = series_pair(Sibling::Cancelled);
    let out = cleanup_one(&fx);
    assert!(out.ends_with("; card removed from the board"), "{out}");
    assert_eq!(board_ids(&fx), only_second());
}

#[test]
fn cleanup_keeps_the_card_of_a_cancelled_series_member_with_a_sibling() {
    let (fx, _home) = series_pair(Sibling::Queued);
    let out = cleanup_one(&fx);
    assert!(out.contains("series S3 still has active jobs"), "{out}");
    assert_eq!(board_ids(&fx), both_ids());
}

#[test]
fn cleanup_keeps_cancelled_card_when_series_sibling_is_in_review() {
    let (fx, _home) = series_pair(Sibling::Review);
    let out = cleanup_one(&fx);
    assert!(out.contains("series S3 still has active jobs"), "{out}");
    assert_eq!(board_ids(&fx), both_ids());
}

#[test]
fn cleanup_blocks_a_queued_request_for_a_zero_padded_series_member() {
    let (fx, home) = series_pair(Sibling::Merged);
    let request =
        factory_supervisor::Request::new("p", factory_supervisor::Action::Run, "2", true, 1)
            .expect("request");
    factory_supervisor::queue::enqueue(&home, &request).expect("enqueue");
    let out = cleanup_one(&fx);
    assert!(out.contains("series S3 has queued requests"), "{out}");
    assert_eq!(board_ids(&fx), both_ids());
}

#[test]
fn cleanup_blocks_an_approve_request_for_series_member() {
    let (fx, home) = series_pair(Sibling::Merged);
    queue_request(&home, "p", factory_supervisor::Action::Approve, "2", false);
    let out = cleanup_one(&fx);
    assert!(out.contains("series S3 has queued requests"), "{out}");
    assert_eq!(board_ids(&fx), both_ids());
}

#[test]
fn cleanup_ignores_unrelated_project_and_job_queue_requests() {
    let (fx, home) = series_pair(Sibling::Merged);
    queue_request(&home, "other", factory_supervisor::Action::Run, "2", true);
    queue_request(&home, "p", factory_supervisor::Action::Run, "3", true);
    let out = cleanup_one(&fx);
    assert!(out.ends_with("; card removed from the board"), "{out}");
    assert_eq!(board_ids(&fx), only_second());
}

#[test]
fn cleanup_fails_closed_when_registered_project_queue_is_unreadable() {
    let (fx, home) = series_pair(Sibling::Merged);
    std::fs::create_dir_all(&home.dir).expect("home dir");
    std::fs::write(home.queue_dir(), "not a directory").expect("queue file");
    let out = cleanup_one(&fx);
    assert!(out.contains("queued work could not be checked"), "{out}");
    assert_eq!(board_ids(&fx), both_ids());
}

#[test]
fn cleanup_ignores_malformed_queue_request() {
    let (fx, home) = series_pair(Sibling::Merged);
    std::fs::create_dir_all(home.queue_dir()).expect("queue dir");
    std::fs::write(home.queue_dir().join("junk.json"), "{not json").expect("junk");
    let out = cleanup_one(&fx);
    assert!(out.ends_with("; card removed from the board"), "{out}");
    assert_eq!(board_ids(&fx), only_second());
}

#[test]
fn cleanup_can_remove_card_when_retried_after_queue_request_finishes() {
    let (fx, home) = series_pair(Sibling::Merged);
    let request = queue_request(&home, "p", factory_supervisor::Action::Run, "2", true);
    let out = cleanup_one(&fx);
    assert!(out.contains("series S3 has queued requests"), "{out}");
    factory_supervisor::queue::remove(&home, &request.id).expect("remove");
    let out = cleanup_one(&fx);
    assert!(out.ends_with("; card removed from the board"), "{out}");
    assert_eq!(board_ids(&fx), only_second());
}

#[test]
fn cleanup_keeps_card_for_single_cancelled_job_with_matching_queue_request() {
    let mut fx = Fixture::with_risk_path("docs/**");
    parked_and_cancelled(&fx, "hello");
    fx.run(&["cancel", "1"]).expect("cancel");
    let home = factory_core::Home::at(fx.tmp.path().join("home"));
    fx.ctx.home = Some(home.clone());
    fx.ctx.project_name = Some("p".to_string());
    queue_request(&home, "p", factory_supervisor::Action::Run, "1", false);
    let out = cleanup_one(&fx);
    assert!(out.contains("job 001 has a queued request"), "{out}");
    assert_eq!(board_ids(&fx), vec!["001".to_string()]);
}

#[test]
fn cleanup_without_registered_context_uses_board_and_lease_guards() {
    let fx = Fixture::with_risk_path("docs/**");
    parked_and_cancelled(&fx, "hello");
    fx.run(&["cancel", "1"]).expect("cancel");
    let out = cleanup_one(&fx);
    assert!(out.ends_with("; card removed from the board"), "{out}");
}

#[test]
fn cleanup_keeps_the_card_of_a_cancelled_job_with_a_live_lease() {
    let fx = Fixture::with_risk_path("docs/**");
    parked_and_cancelled(&fx, "hello");
    fx.run(&["cancel", "1"]).expect("cancel");
    fx.hold_lease("001", "someone-else");
    let out = fx
        .run(&["cleanup", "1", "--discard-commits"])
        .expect("cleanup");
    assert!(
        out.contains("card kept on the board: job 001 has a live lease"),
        "{out}"
    );
    assert_eq!(board_ids(&fx), vec!["001".to_string()]);
}

/// The folder named in the `no plan yet: put plan.md at <dir>/plan.md ...` line.
fn plan_dir(out: &str) -> std::path::PathBuf {
    let line = out
        .lines()
        .find(|l| l.starts_with("no plan yet: put plan.md at "))
        .expect("no plan yet line");
    let rest = line.trim_start_matches("no plan yet: put plan.md at ");
    let file = rest.strip_suffix(" before running it").expect("suffix");
    std::path::Path::new(file)
        .parent()
        .expect("parent")
        .to_path_buf()
}

#[test]
fn a_job_without_plans_parks_without_calling_the_builder() {
    let fx = Fixture::new();
    fx.run(&["new", "hello", "say hello"]).expect("new");
    let end = fx.run(&["run", "1"]).expect("run");
    assert_eq!(end, "needs-human: no plan.md in the job folder");
    assert_eq!(
        fx.builder_calls(),
        0,
        "the builder must not run without a plan"
    );
}

#[test]
fn a_job_without_plans_builds_once_plan_md_is_placed_by_hand() {
    let fx = Fixture::new();
    let out = fx.run(&["new", "hello", "say hello"]).expect("new");
    assert!(fx
        .run(&["run", "1"])
        .expect("run")
        .starts_with("needs-human"));
    assert_eq!(fx.builder_calls(), 0);
    std::fs::write(plan_dir(&out).join("plan.md"), "# Plan\n\n### Task 1: t\n").expect("plan");
    fx.run(&["rework", "1", "plan.md is placed now"])
        .expect("rework");
    assert_eq!(fx.run(&["run", "1"]).expect("run again"), "merged");
    assert!(fx.builder_calls() >= 1);
}

#[test]
fn new_without_plans_says_a_plan_is_needed_and_new_with_plans_does_not() {
    let fx = Fixture::new();
    let out = fx.run(&["new", "hello", "say hello"]).expect("new");
    let dir = plan_dir(&out);
    assert!(dir.is_absolute(), "{out}");
    let want = format!(
        "no plan yet: put plan.md at {}/plan.md before running it",
        dir.display()
    );
    assert!(out.lines().any(|l| l == want), "{out}\n want {want}");
    assert!(out.starts_with("created job 001 in "), "{out}");
    let plans = fx.write_plan("plan-01-two.md", 1, "none");
    let out = fx
        .run(&["new", "two", "x", "--plans", plans.to_str().expect("utf8")])
        .expect("new");
    assert!(!out.contains("no plan yet"), "{out}");
}

/// A risky job parked with open review blockers.
fn parked_with_changes() -> Result<Scenario, Box<dyn std::error::Error>> {
    let mut steps = vec![build(vec![("secrets/token.txt", "x\n")], 1_000)];
    for _ in 0..6 {
        steps.push(review(CHANGES));
        steps.push(build(vec![("secrets/token.txt", "y\n")], 400));
    }
    let sc = Scenario::new("", steps)?;
    let end = sc.start(&sc.plan_b, false)?;
    assert!(end.starts_with("needs-human"), "{end}");
    Ok(sc)
}

#[test]
fn approve_refuses_open_changes_and_names_waive() -> TestResult {
    let sc = parked_with_changes()?;
    let before = sc.main_tip()?;
    let e = sc.run(&["approve", "1"]).expect_err("open changes");
    let text = e.to_string();
    assert!(text.contains("open review blockers"), "{text}");
    assert!(text.contains("--waive"), "{text}");
    assert_eq!(sc.stage("001")?, "needs-human");
    assert_eq!(sc.main_tip()?, before);
    Ok(())
}

#[test]
fn approve_with_waive_on_a_clean_job_is_refused() -> TestResult {
    let sc = Scenario::new(
        "",
        vec![
            build(vec![("secrets/token.txt", "x\n")], 1_000),
            review(APPROVE),
        ],
    )?;
    let end = sc.start(&sc.plan_b, false)?;
    assert!(end.starts_with("needs-human"), "{end}");
    let e = sc
        .run(&["approve", "1", "--waive", "x"])
        .expect_err("nothing to waive");
    let text = e.to_string();
    assert!(text.contains("no open review findings to waive"), "{text}");
    assert_eq!(sc.stage("001")?, "needs-human");
    Ok(())
}

#[test]
fn waive_reason_must_be_one_short_line() -> TestResult {
    let sc = parked_with_changes()?;
    let cases = [
        ("".to_string(), "the waive reason is empty"),
        ("   ".to_string(), "the waive reason is empty"),
        (
            "two\nlines".to_string(),
            "the waive reason must be one line",
        ),
        ("a\rb".to_string(), "the waive reason must be one line"),
        ("\r".to_string(), "the waive reason is empty"),
        (
            "x".repeat(201),
            "the waive reason is longer than 200 characters",
        ),
    ];
    for (reason, message) in cases {
        let e = sc
            .run(&["approve", "1", "--waive", &reason])
            .expect_err("bad reason");
        let text = e.to_string();
        assert!(text.contains(message), "{text}");
        assert_eq!(sc.stage("001")?, "needs-human");
    }
    // Exactly 200 characters is accepted.
    let out = sc.run(&["approve", "1", "--waive", &"x".repeat(200)])?;
    assert!(out.starts_with("merged"), "{out}");
    assert_eq!(sc.stage("001")?, "merged");
    Ok(())
}

#[test]
fn approve_with_waive_merges_and_records_the_waiver() -> TestResult {
    let sc = parked_with_changes()?;
    let out = sc.run(&["approve", "1", "--waive", "accepted for now"])?;
    assert!(out.starts_with("merged"), "{out}");
    assert_eq!(sc.stage("001")?, "merged");
    let sdd = sc.root.join(".superpowers/sdd");
    let mut ledgers = String::new();
    let mut waivers = String::new();
    for entry in std::fs::read_dir(&sdd)?.flatten() {
        ledgers.push_str(
            &std::fs::read_to_string(entry.path().join("progress.md")).unwrap_or_default(),
        );
        waivers.push_str(
            &std::fs::read_to_string(entry.path().join("waivers.json")).unwrap_or_default(),
        );
    }
    assert!(ledgers.contains("waived 1 blockers"), "{ledgers}");
    let list: serde_json::Value = serde_json::from_str(&waivers)?;
    assert_eq!(list.as_array().map(Vec::len), Some(1), "{waivers}");
    assert_eq!(list[0]["reason"], "accepted for now");
    Ok(())
}

/// A tracker that counts created issues and answers every other call.
#[derive(Default)]
struct CountCreates(std::sync::atomic::AtomicU32);

impl factory_engine::Tracker for CountCreates {
    fn repo_name(&self) -> Result<String, String> {
        Ok("o/r".into())
    }
    fn labels_of(&self, _: u32) -> Result<Vec<String>, String> {
        Ok(vec![])
    }
    fn is_closed(&self, _: u32) -> Result<bool, String> {
        Ok(false)
    }
    fn create(&self, _: &str, _: &str, _: &[String]) -> Result<u32, String> {
        Ok(300 + self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst))
    }
    fn comment(&self, _: u32, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn close(&self, _: u32) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn tickets_with_minor_tickets_off_says_so() -> TestResult {
    let tracker = std::sync::Arc::new(CountCreates::default());
    let sc = Scenario::new(
        "minor_tickets = false\n",
        vec![
            build(vec![("b.txt", "b\n")], 1_000),
            review("- [minor] nit\nVERDICT: APPROVE"),
        ],
    )?
    .with_tracker(tracker.clone());
    assert_eq!(sc.start(&sc.plan_b, false)?, "merged");
    let out = sc.run(&["tickets", "1"])?;
    assert!(out.contains("skipped: minor_tickets is off"), "{out}");
    assert_eq!(tracker.0.load(std::sync::atomic::Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn a_rework_supersedes_a_waiver_that_never_merged() -> TestResult {
    let tracker = std::sync::Arc::new(CountCreates::default());
    let sc = reworkable(
        0,
        vec![
            build(vec![("secrets/token.txt", "x\n")], 1_000),
            review(CHANGES),
            build(vec![("secrets/token.txt", "y\n")], 400),
            review(APPROVE),
        ],
    )?
    .with_tracker(tracker.clone());
    let end = sc.start(&sc.plan_b, false)?;
    assert!(end.starts_with("needs-human"), "{end}");
    // A dirty main parks the waived approve as main busy.
    std::fs::write(sc.root.join("README.md"), "dirty\n")?;
    let out = sc.run(&["approve", "1", "--waive", "accepted for now"])?;
    assert!(out.contains("main busy"), "{out}");
    std::fs::write(sc.root.join("README.md"), "demo\n")?;
    // A stale rework note must not shadow the round's findings.
    std::fs::write(job_file(&sc, "rework-note.md"), "stale")?;
    assert_eq!(
        sc.run(&["rework", "1", "fix the helper"])?,
        "job 001 back to build, round 2"
    );
    assert!(!job_file(&sc, "rework-note.md").exists());
    let waivers: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(job_file(&sc, "waivers.json"))?)?;
    assert_eq!(waivers[0]["superseded"], true, "{waivers}");
    let ledger = std::fs::read_to_string(job_file(&sc, "progress.md"))?;
    assert!(
        ledger.contains("waiver superseded by rework round 2"),
        "{ledger}"
    );
    let end = sc.run(&["run", "1"])?;
    assert!(end.starts_with("needs-human"), "{end}");
    assert!(sc.run(&["approve", "1"])?.starts_with("merged"));
    assert_eq!(tracker.0.load(std::sync::atomic::Ordering::SeqCst), 0);
    Ok(())
}

/// A risky job with `fix_rounds` fix rounds, scripted by `steps`.
fn reworkable(
    fix_rounds: u32,
    steps: Vec<support::Step>,
) -> Result<Scenario, Box<dyn std::error::Error>> {
    let cfg = format!("fix_rounds = {fix_rounds}\nrisky_fix_rounds = {fix_rounds}\n");
    Scenario::new(&cfg, steps)
}

fn job_file(sc: &Scenario, name: &str) -> std::path::PathBuf {
    let dir = sc.job("1").map(|j| j.dir).unwrap_or_default();
    sc.root.join(".superpowers/sdd").join(dir).join(name)
}

const NOTE_WINS: &str =
    "The note above is from the user and wins where it conflicts with the findings below.";

#[test]
fn rework_of_a_round_one_job_keeps_round_one_files() -> TestResult {
    let sc = reworkable(
        0,
        vec![
            build(vec![("secrets/token.txt", "x\n")], 1_000),
            review(CHANGES),
            build(vec![("secrets/token.txt", "y\n")], 400),
            review(CHANGES),
        ],
    )?;
    let end = sc.start(&sc.plan_b, false)?;
    assert!(end.starts_with("needs-human"), "{end}");
    let before = std::fs::read_to_string(job_file(&sc, "review-code-r1.md"))?;
    let line = sc.run(&["rework", "1", "shorter"])?;
    assert_eq!(line, "job 001 back to build, round 2");
    let job = sc.job("1")?;
    assert_eq!((job.round, job.rework_base), (2, 1));
    sc.run(&["run", "1"])?;
    for name in ["review-code-r1.md", "builder-r2.md", "review-code-r2.md"] {
        assert!(job_file(&sc, name).exists(), "{name}");
    }
    assert_eq!(
        std::fs::read_to_string(job_file(&sc, "review-code-r1.md"))?,
        before
    );
    Ok(())
}

#[test]
fn rework_sends_the_note_and_the_open_findings() -> TestResult {
    let sc = reworkable(
        0,
        vec![
            build(vec![("secrets/token.txt", "x\n")], 1_000),
            review(CHANGES),
        ],
    )?;
    sc.start(&sc.plan_b, false)?;
    sc.run(&["rework", "1", "use a shorter name"])?;
    let text = std::fs::read_to_string(job_file(&sc, "changes-r2.md"))?;
    let want = format!("use a shorter name\n\n{NOTE_WINS}\n\n{CHANGES}");
    assert_eq!(text, want);
    Ok(())
}

#[test]
fn rework_after_nothing_changed_carries_the_review() -> TestResult {
    let sc = reworkable(
        1,
        vec![
            build(vec![("secrets/token.txt", "x\n")], 1_000),
            review(CHANGES),
            build(vec![], 400),
        ],
    )?;
    let end = sc.start(&sc.plan_b, false)?;
    assert!(end.starts_with("needs-human"), "{end}");
    assert_eq!(sc.job("1")?.park_kind.as_deref(), Some("nothing-changed"));
    sc.run(&["rework", "1", "try again"])?;
    let round = sc.job("1")?.round;
    let text = std::fs::read_to_string(job_file(&sc, &format!("changes-r{round}.md")))?;
    assert!(text.contains(CHANGES), "{text}");
    Ok(())
}

#[test]
fn rework_after_failing_checks_carries_the_checks() -> TestResult {
    let sc = reworkable(
        0,
        vec![build(
            vec![("secrets/token.txt", "x\n"), ("bad.flag", "x\n")],
            1_000,
        )],
    )?;
    let end = sc.start(&sc.plan_b, false)?;
    assert!(end.starts_with("needs-human"), "{end}");
    assert_eq!(sc.job("1")?.park_kind.as_deref(), Some("checks-failed"));
    sc.run(&["rework", "1", "fix the flag"])?;
    let text = std::fs::read_to_string(job_file(&sc, "changes-r2.md"))?;
    assert!(text.starts_with("fix the flag\n\n"), "{text}");
    assert!(text.contains("FAIL test ! -f bad.flag"), "{text}");
    Ok(())
}

#[test]
fn rework_grants_a_full_set_of_fix_rounds() -> TestResult {
    let sc = reworkable(
        1,
        vec![
            build(vec![("secrets/token.txt", "x\n")], 1_000),
            review(CHANGES),
            build(vec![("secrets/token.txt", "y\n")], 400),
            review(CHANGES),
            build(vec![("secrets/token.txt", "z\n")], 400),
            review(CHANGES),
            build(vec![("secrets/token.txt", "w\n")], 400),
            review(CHANGES),
        ],
    )?;
    sc.start(&sc.plan_b, false)?;
    assert_eq!(sc.job("1")?.round, 2);
    sc.run(&["rework", "1", "again"])?;
    assert_eq!(sc.job("1")?.round, 3);
    assert_eq!(sc.job("1")?.rework_base, 2);
    let end = sc.run(&["run", "1"])?;
    assert!(end.starts_with("needs-human"), "{end}");
    // The CHANGES review of the rework round still started a fix round.
    assert_eq!(sc.job("1")?.round, 4);
    assert_eq!(sc.stage("1")?, "needs-human");
    assert_eq!(sc.job("1")?.park_kind.as_deref(), Some("changes-remain"));
    Ok(())
}

#[test]
fn rework_mid_task_keeps_the_note_path() -> TestResult {
    let sc = reworkable(1, vec![])?;
    let plan = sc.tmp.path().join("plans/plan-03-two.md");
    std::fs::write(
        &plan,
        "# Two\n\n### Task 1: a\n\n### Task 2: b\n\nNext plan: none\n",
    )?;
    let end = sc.start(&plan, false)?;
    assert!(end.starts_with("needs-human"), "{end}");
    let round = sc.job("1")?.round;
    let line = sc.run(&["rework", "1", "use the small helper"])?;
    assert_eq!(line, "job 001 back to build, same task");
    assert_eq!(
        std::fs::read_to_string(job_file(&sc, "rework-note.md"))?,
        "use the small helper"
    );
    let job = sc.job("1")?;
    assert_eq!((job.round, job.rework_base), (round, 0));
    Ok(())
}
