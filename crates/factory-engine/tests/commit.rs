mod common;

use std::path::Path;

use common::*;
use factory_agent::AgentSpec;
use factory_engine::{CommitKind, CommitOutcome, CommitRequest};
use factory_git::{JobRef, Worktree};
use git2::{Repository, ResetType, Signature};

const GOOD: &str = "feat(x): add the thing\n\nThe thing was missing.";

/// Writes the prompt context file into the main checkout, where the engine reads it.
fn context(fx: &Fixture) {
    let path = fx.main.join(".claude/factory/commit-context.md");
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(path, "TEST CONTEXT\n").expect("context");
}

fn worktree(fx: &Fixture) -> Worktree {
    fx.repo.create_worktree(&JobRef { id: "001".into(), slug: "demo".into() }).expect("worktree")
}

fn request(floor: &str) -> CommitRequest<'_> {
    CommitRequest {
        kind: CommitKind::Task,
        plan_title: "demo plan",
        task_title: "Add the thing.",
        goal: "add the thing",
        floor,
        issues: &[],
        snapshots: &[],
    }
}

fn write_change(wt: &Worktree, name: &str) {
    std::fs::write(wt.path.join(name), "content\n").expect("write change");
}

/// Stages the given paths and commits them on HEAD, like a committer would.
fn commit_paths(dir: &Path, paths: &[&str], message: &str) -> String {
    let repo = Repository::open(dir).expect("open");
    let mut index = repo.index().expect("index");
    for p in paths {
        index.add_path(Path::new(p)).expect("add");
    }
    index.write().expect("write index");
    let tree = repo.find_tree(index.write_tree().expect("tree")).expect("find tree");
    let sig = Signature::now("t", "t@example.com").expect("sig");
    let parent = repo.head().expect("head").peel_to_commit().expect("commit");
    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&parent]).expect("commit").to_string()
}

/// Stages the given paths and amends HEAD with them, like `git commit --amend`.
fn amend_with(dir: &Path, paths: &[&str], message: &str) -> String {
    let repo = Repository::open(dir).expect("open");
    let mut index = repo.index().expect("index");
    for p in paths {
        index.add_path(Path::new(p)).expect("add");
    }
    index.write().expect("write index");
    let tree = repo.find_tree(index.write_tree().expect("tree")).expect("find tree");
    let head = repo.head().expect("head").peel_to_commit().expect("commit");
    head.amend(Some("HEAD"), None, None, None, Some(message), Some(&tree)).expect("amend").to_string()
}

/// Like `commit_paths`, but with a fixed commit time so the sha differs from an earlier twin.
fn commit_paths_at(dir: &Path, paths: &[&str], message: &str, secs: i64) -> String {
    let repo = Repository::open(dir).expect("open");
    let mut index = repo.index().expect("index");
    for p in paths {
        index.add_path(Path::new(p)).expect("add");
    }
    index.write().expect("write index");
    let tree = repo.find_tree(index.write_tree().expect("tree")).expect("find tree");
    let sig = Signature::new("t", "t@example.com", &git2::Time::new(secs, 0)).expect("sig");
    let parent = repo.head().expect("head").peel_to_commit().expect("commit");
    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&parent]).expect("commit").to_string()
}

/// Moves the branch and the index to `rev`, keeping the files in the work tree.
fn reset_mixed(dir: &Path, rev: &str) {
    let repo = Repository::open(dir).expect("open");
    let obj = repo.revparse_single(rev).expect("rev");
    repo.reset(&obj, ResetType::Mixed, None).expect("reset");
}

fn effect(f: impl Fn(&Path) -> String + Send + Sync + 'static) -> Effect {
    Box::new(move |spec: &AgentSpec| f(&spec.cwd))
}

fn nothing() -> Effect {
    effect(|_| "did nothing".to_string())
}

fn ledger(fx: &Fixture) -> String {
    std::fs::read_to_string(fx.job_dir.join("progress.md")).unwrap_or_default()
}

fn subjects(fx: &Fixture, wt: &Worktree, floor: &str) -> Vec<String> {
    let tip = fx.repo.tip(&wt.branch).expect("tip");
    fx.repo.commits_above(floor, &tip).expect("commits").into_iter().map(|c| c.subject).collect()
}

#[test]
fn one_valid_commit_is_accepted_and_the_run_is_trimmed_and_capped() {
    let fx = Fixture::new();
    context(&fx);
    let wt = worktree(&fx);
    write_change(&wt, "a.txt");
    let runner = scripted(vec![effect(|dir| {
        commit_paths(dir, &["a.txt"], GOOD);
        "done".to_string()
    })]);
    let out = fx.engine(&runner).commit_step(&wt, &request(&wt.base)).expect("step");
    match out {
        CommitOutcome::Committed { tip, fallback } => {
            assert!(!fallback);
            assert_eq!(tip, fx.repo.tip(&wt.branch).expect("tip"));
        }
        other => panic!("unexpected outcome {other:?}"),
    }
    assert_eq!(subjects(&fx, &wt, &wt.base), vec!["feat(x): add the thing"]);
    let spec = runner.last_spec().expect("spec");
    assert_eq!(spec.role, "committer");
    assert_eq!(spec.agent.as_deref(), Some("factory-committer"));
    assert_eq!(spec.budget_weighted, Some(60_000));
    assert_eq!(spec.used_before, 0);
    assert_eq!(spec.env, vec![("HARNESS_GATE".to_string(), "0".to_string())]);
    assert!(spec.strict_mcp);
    assert_eq!(spec.tools.as_deref(), Some("Bash,Read,Skill"));
    assert!(spec.prompt.starts_with("TEST CONTEXT"));
    assert!(spec.prompt.contains(&format!("Floor: {}", wt.base)));
    assert!(spec.prompt.contains("new file:   a.txt"));
}

#[test]
fn a_split_into_two_commits_is_accepted() {
    let fx = Fixture::new();
    context(&fx);
    let wt = worktree(&fx);
    write_change(&wt, "a.txt");
    write_change(&wt, "b.txt");
    let runner = scripted(vec![effect(|dir| {
        commit_paths(dir, &["a.txt"], "feat(x): add a");
        commit_paths(dir, &["b.txt"], "feat(x): add b");
        "done".to_string()
    })]);
    let out = fx.engine(&runner).commit_step(&wt, &request(&wt.base)).expect("step");
    assert!(matches!(out, CommitOutcome::Committed { fallback: false, .. }), "{out:?}");
    assert_eq!(subjects(&fx, &wt, &wt.base), vec!["feat(x): add a", "feat(x): add b"]);
}

#[test]
fn amending_a_commit_above_the_floor_is_accepted() {
    let fx = Fixture::new();
    context(&fx);
    let wt = worktree(&fx);
    write_change(&wt, "a.txt");
    commit_paths(&wt.path, &["a.txt"], GOOD);
    write_change(&wt, "b.txt");
    let runner = scripted(vec![effect(|dir| {
        amend_with(dir, &["b.txt"], "feat(x): add the thing and b");
        "done".to_string()
    })]);
    let out = fx.engine(&runner).commit_step(&wt, &request(&wt.base)).expect("step");
    assert!(matches!(out, CommitOutcome::Committed { fallback: false, .. }), "{out:?}");
    assert_eq!(subjects(&fx, &wt, &wt.base), vec!["feat(x): add the thing and b"]);
}

#[test]
fn rewriting_the_floor_commit_parks_at_once() {
    let fx = Fixture::new();
    context(&fx);
    let wt = worktree(&fx);
    write_change(&wt, "a.txt");
    let floor = commit_paths(&wt.path, &["a.txt"], GOOD);
    write_change(&wt, "b.txt");
    // A retry would still park, so the assertions below prove there was no retry.
    let runner = scripted(vec![effect(|dir| {
        amend_with(dir, &["b.txt"], GOOD);
        "done".to_string()
    })]);
    let out = fx.engine(&runner).commit_step(&wt, &request(&floor)).expect("step");
    match out {
        CommitOutcome::Park(reason) => assert!(reason.contains("at or below the floor"), "{reason}"),
        other => panic!("unexpected outcome {other:?}"),
    }
    // A retry would record a second spec that resumes the session and carries the findings.
    let spec = runner.last_spec().expect("spec");
    assert!(spec.resume.is_none(), "unexpected resume: {:?}", spec.resume);
    assert!(!spec.prompt.contains("Findings:"), "unexpected retry prompt: {}", spec.prompt);
}

#[test]
fn an_uncommitted_tree_is_retried_once_by_resuming_the_session() {
    let fx = Fixture::new();
    context(&fx);
    let wt = worktree(&fx);
    write_change(&wt, "a.txt");
    let runner = scripted(vec![
        nothing(),
        effect(|dir| {
            commit_paths(dir, &["a.txt"], GOOD);
            "done".to_string()
        }),
    ]);
    let out = fx.engine(&runner).commit_step(&wt, &request(&wt.base)).expect("step");
    assert!(matches!(out, CommitOutcome::Committed { fallback: false, .. }), "{out:?}");
    let retry = runner.last_spec().expect("retry spec");
    assert_eq!(retry.resume.as_deref(), Some("sess-1"));
    assert!(retry.prompt.contains("uncommitted"), "retry prompt: {}", retry.prompt);
}

#[test]
fn two_failed_runs_fall_back_to_a_plain_commit_and_write_a_ruling() {
    let fx = Fixture::new();
    context(&fx);
    let wt = worktree(&fx);
    write_change(&wt, "a.txt");
    let runner = scripted(vec![nothing(), nothing()]);
    let out = fx.engine(&runner).commit_step(&wt, &request(&wt.base)).expect("step");
    assert!(matches!(out, CommitOutcome::Committed { fallback: true, .. }), "{out:?}");
    assert_eq!(subjects(&fx, &wt, &wt.base), vec!["chore: Add the thing"]);
    assert!(ledger(&fx).contains("fallback commit"), "ledger: {}", ledger(&fx));
}

#[test]
fn a_blocked_reply_twice_falls_back_and_names_the_reason() {
    let fx = Fixture::new();
    context(&fx);
    let wt = worktree(&fx);
    write_change(&wt, "a.txt");
    let runner = scripted(vec![effect(|_| "BLOCKED: no skill".to_string()), effect(|_| "BLOCKED: no skill".to_string())]);
    let out = fx.engine(&runner).commit_step(&wt, &request(&wt.base)).expect("step");
    assert!(matches!(out, CommitOutcome::Committed { fallback: true, .. }), "{out:?}");
    assert!(ledger(&fx).contains("BLOCKED: no skill"), "ledger: {}", ledger(&fx));
}

#[test]
fn an_invalid_subject_after_the_retry_keeps_the_commit_and_writes_a_ruling() {
    let fx = Fixture::new();
    context(&fx);
    let wt = worktree(&fx);
    write_change(&wt, "a.txt");
    let runner = scripted(vec![
        effect(|dir| {
            commit_paths(dir, &["a.txt"], "Added the thing.");
            "done".to_string()
        }),
        nothing(),
    ]);
    let out = fx.engine(&runner).commit_step(&wt, &request(&wt.base)).expect("step");
    assert!(matches!(out, CommitOutcome::Committed { fallback: false, .. }), "{out:?}");
    assert_eq!(subjects(&fx, &wt, &wt.base), vec!["Added the thing."]);
    assert!(ledger(&fx).contains("commit message check failed"), "ledger: {}", ledger(&fx));
}

#[test]
fn more_than_four_commits_are_kept_with_a_ruling() {
    let fx = Fixture::new();
    context(&fx);
    let wt = worktree(&fx);
    for name in ["a.txt", "b.txt", "c.txt", "d.txt", "e.txt"] {
        write_change(&wt, name);
    }
    let runner = scripted(vec![
        effect(|dir| {
            for name in ["a.txt", "b.txt", "c.txt", "d.txt", "e.txt"] {
                commit_paths(dir, &[name], &format!("feat(x): add {name}"));
            }
            "done".to_string()
        }),
        nothing(),
    ]);
    let out = fx.engine(&runner).commit_step(&wt, &request(&wt.base)).expect("step");
    assert!(matches!(out, CommitOutcome::Committed { fallback: false, .. }), "{out:?}");
    assert_eq!(subjects(&fx, &wt, &wt.base).len(), 5);
    assert!(ledger(&fx).contains("5 commits were made"), "ledger: {}", ledger(&fx));
}

#[test]
fn nothing_staged_makes_no_agent_call() {
    let fx = Fixture::new();
    context(&fx);
    let wt = worktree(&fx);
    let runner = scripted(vec![]);
    let out = fx.engine(&runner).commit_step(&wt, &request(&wt.base)).expect("step");
    assert_eq!(out, CommitOutcome::NothingStaged);
    assert!(runner.last_spec().is_none());
}

#[test]
fn a_missing_context_file_uses_the_fallback_without_an_agent_call() {
    let fx = Fixture::new();
    let wt = worktree(&fx);
    write_change(&wt, "a.txt");
    let runner = scripted(vec![]);
    let out = fx.engine(&runner).commit_step(&wt, &request(&wt.base)).expect("step");
    assert!(matches!(out, CommitOutcome::Committed { fallback: true, .. }), "{out:?}");
    assert!(runner.last_spec().is_none());
    assert!(ledger(&fx).contains("commit context"), "ledger: {}", ledger(&fx));
}

#[test]
fn a_valid_commit_with_a_clean_tree_is_accepted_even_if_the_run_failed() {
    let fx = Fixture::new();
    context(&fx);
    let wt = worktree(&fx);
    write_change(&wt, "a.txt");
    let runner = scripted(vec![effect(|dir| {
        commit_paths(dir, &["a.txt"], GOOD);
        "BLOCKED: gave up after committing".to_string()
    })]);
    let out = fx.engine(&runner).commit_step(&wt, &request(&wt.base)).expect("step");
    assert!(matches!(out, CommitOutcome::Committed { fallback: false, .. }), "{out:?}");
}

#[test]
fn a_rewritten_chain_of_earlier_commits_is_not_counted_as_new_commits() {
    let fx = Fixture::new();
    context(&fx);
    let wt = worktree(&fx);
    let names = ["c1.txt", "c2.txt", "c3.txt", "c4.txt"];
    for name in names {
        write_change(&wt, name);
        commit_paths(&wt.path, &[name], &format!("feat(x): add {name}"));
    }
    write_change(&wt, "e.txt");
    let base = wt.base.clone();
    let runner = scripted(vec![effect(move |dir| {
        reset_mixed(dir, &base);
        for (i, name) in ["c1.txt", "c2.txt", "c3.txt", "c4.txt"].iter().enumerate() {
            commit_paths_at(dir, &[name], &format!("feat(x): add {name}"), 1_900_000_000 + i as i64);
        }
        commit_paths(dir, &["e.txt"], "feat(x): add e.txt");
        "done".to_string()
    })]);
    let out = fx.engine(&runner).commit_step(&wt, &request(&wt.base)).expect("step");
    assert!(matches!(out, CommitOutcome::Committed { fallback: false, .. }), "{out:?}");
    assert_eq!(subjects(&fx, &wt, &wt.base).len(), 5);
    let log = ledger(&fx);
    assert!(!log.contains("commits were made"), "ledger: {log}");
    assert!(!log.contains("commit message check failed"), "ledger: {log}");
}

#[test]
fn detaching_head_parks_without_a_retry() {
    let fx = Fixture::new();
    context(&fx);
    let wt = worktree(&fx);
    write_change(&wt, "a.txt");
    let runner = scripted(vec![effect(|dir| {
        let repo = Repository::open(dir).expect("open");
        let oid = repo.head().expect("head").peel_to_commit().expect("commit").id();
        repo.set_head_detached(oid).expect("detach");
        "done".to_string()
    })]);
    let out = fx.engine(&runner).commit_step(&wt, &request(&wt.base)).expect("step");
    match out {
        CommitOutcome::Park(reason) => assert!(reason.contains("job branch"), "{reason}"),
        other => panic!("unexpected outcome {other:?}"),
    }
    let spec = runner.last_spec().expect("spec");
    assert!(spec.resume.is_none(), "unexpected resume: {:?}", spec.resume);
    assert!(!spec.prompt.contains("Findings:"), "unexpected retry prompt: {}", spec.prompt);
}

#[test]
fn a_floor_rewrite_on_the_retry_run_parks() {
    let fx = Fixture::new();
    context(&fx);
    let wt = worktree(&fx);
    write_change(&wt, "a.txt");
    let floor = commit_paths(&wt.path, &["a.txt"], GOOD);
    write_change(&wt, "b.txt");
    let runner = scripted(vec![
        nothing(),
        effect(|dir| {
            amend_with(dir, &["b.txt"], GOOD);
            "done".to_string()
        }),
    ]);
    let out = fx.engine(&runner).commit_step(&wt, &request(&floor)).expect("step");
    match out {
        CommitOutcome::Park(reason) => assert!(reason.contains("at or below the floor"), "{reason}"),
        other => panic!("unexpected outcome {other:?}"),
    }
    assert_eq!(runner.last_spec().expect("spec").resume.as_deref(), Some("sess-1"));
}

#[test]
fn a_message_fault_fixed_by_the_retry_leaves_no_ruling() {
    let fx = Fixture::new();
    context(&fx);
    let wt = worktree(&fx);
    write_change(&wt, "a.txt");
    let runner = scripted(vec![
        effect(|dir| {
            commit_paths(dir, &["a.txt"], "Added the thing.");
            "done".to_string()
        }),
        effect(|dir| {
            amend_with(dir, &["a.txt"], GOOD);
            "done".to_string()
        }),
    ]);
    let out = fx.engine(&runner).commit_step(&wt, &request(&wt.base)).expect("step");
    assert!(matches!(out, CommitOutcome::Committed { fallback: false, .. }), "{out:?}");
    assert_eq!(subjects(&fx, &wt, &wt.base), vec!["feat(x): add the thing"]);
    assert!(!ledger(&fx).contains("commit message check failed"), "ledger: {}", ledger(&fx));
}

#[test]
fn a_dirty_tree_after_valid_commits_goes_to_the_fallback() {
    let fx = Fixture::new();
    context(&fx);
    let wt = worktree(&fx);
    write_change(&wt, "a.txt");
    write_change(&wt, "b.txt");
    let runner = scripted(vec![
        effect(|dir| {
            // Keep b.txt out of the commit, so it stays in the tree.
            reset_mixed(dir, "HEAD");
            commit_paths(dir, &["a.txt"], GOOD);
            "done".to_string()
        }),
        nothing(),
    ]);
    let out = fx.engine(&runner).commit_step(&wt, &request(&wt.base)).expect("step");
    assert!(matches!(out, CommitOutcome::Committed { fallback: true, .. }), "{out:?}");
    assert_eq!(subjects(&fx, &wt, &wt.base), vec!["feat(x): add the thing", "chore: Add the thing"]);
    assert!(fx.repo.worktree_is_clean(&wt).expect("clean"));
}

fn request_with<'a>(floor: &'a str, issues: &'a [u32]) -> CommitRequest<'a> {
    CommitRequest { issues, ..request(floor) }
}

fn messages(fx: &Fixture, wt: &Worktree, floor: &str) -> Vec<String> {
    let tip = fx.repo.tip(&wt.branch).expect("tip");
    fx.repo.commits_above(floor, &tip).expect("commits").into_iter().map(|c| c.message).collect()
}

#[test]
fn a_commit_without_the_issue_footer_is_kept_with_a_ruling() {
    let fx = Fixture::new();
    context(&fx);
    let wt = worktree(&fx);
    write_change(&wt, "a.txt");
    let runner = scripted(vec![effect(|dir| {
        commit_paths(dir, &["a.txt"], "feat(test): build step");
        "done".to_string()
    })]);
    let out = fx.engine(&runner).commit_step(&wt, &request_with(&wt.base, &[20])).expect("step");
    assert!(matches!(out, CommitOutcome::Committed { fallback: false, .. }), "{out:?}");
    assert_eq!(subjects(&fx, &wt, &wt.base), vec!["feat(test): build step"]);
    assert!(ledger(&fx).contains("missing footer `Relates to: #20`"), "ledger: {}", ledger(&fx));
}

#[test]
fn a_commit_with_the_issue_footer_is_accepted_without_a_ruling() {
    let fx = Fixture::new();
    context(&fx);
    let wt = worktree(&fx);
    write_change(&wt, "a.txt");
    let runner = scripted(vec![effect(|dir| {
        commit_paths(dir, &["a.txt"], "feat(test): build step\n\nRelates to: #20");
        "done".to_string()
    })]);
    fx.engine(&runner).commit_step(&wt, &request_with(&wt.base, &[20])).expect("step");
    assert!(!ledger(&fx).contains("missing footer"), "ledger: {}", ledger(&fx));
    assert!(runner.last_spec().expect("spec").prompt.contains("Issue footer: Relates to: #20"));
}

#[test]
fn the_fallback_commit_carries_the_issue_footer() {
    let fx = Fixture::new();
    context(&fx);
    let wt = worktree(&fx);
    write_change(&wt, "a.txt");
    let runner = scripted(vec![nothing(), nothing()]);
    let out = fx.engine(&runner).commit_step(&wt, &request_with(&wt.base, &[20])).expect("step");
    assert!(matches!(out, CommitOutcome::Committed { fallback: true, .. }), "{out:?}");
    assert_eq!(messages(&fx, &wt, &wt.base), vec!["chore: Add the thing\n\nRelates to: #20"]);
}

fn batch_request<'a>(floor: &'a str, snaps: &'a [factory_engine::SnapshotRef]) -> CommitRequest<'a> {
    CommitRequest { kind: CommitKind::Batch, task_title: "demo plan", snapshots: snaps, ..request(floor) }
}

/// Makes two snapshot commits on the job branch, then soft-resets to the base like the build stage does.
fn two_snapshots(wt: &Worktree) -> Vec<factory_engine::SnapshotRef> {
    write_change(wt, "a.txt");
    let first = commit_paths(&wt.path, &["a.txt"], "wip: task 1 first thing");
    write_change(wt, "b.txt");
    let second = commit_paths(&wt.path, &["b.txt"], "wip: task 2 second thing");
    reset_mixed(&wt.path, &wt.base);
    vec![
        factory_engine::SnapshotRef { n: 1, sha: first, title: "first thing".into(), files: vec!["a.txt".into()] },
        factory_engine::SnapshotRef { n: 2, sha: second, title: "second thing".into(), files: vec!["b.txt".into()] },
    ]
}

#[test]
fn a_batch_run_is_capped_at_twice_the_commit_budget_and_names_the_tasks() {
    let fx = Fixture::new();
    context(&fx);
    let wt = worktree(&fx);
    let snaps = two_snapshots(&wt);
    let runner = scripted(vec![effect(|dir| {
        commit_paths(dir, &["a.txt"], "feat(x): add a");
        commit_paths(dir, &["b.txt"], "feat(x): add b");
        "done".to_string()
    })]);
    let out = fx.engine(&runner).commit_step(&wt, &batch_request(&wt.base, &snaps)).expect("step");
    assert!(matches!(out, CommitOutcome::Committed { fallback: false, .. }), "{out:?}");
    assert_eq!(subjects(&fx, &wt, &wt.base), vec!["feat(x): add a", "feat(x): add b"]);
    let spec = runner.last_spec().expect("spec");
    assert_eq!(spec.budget_weighted, Some(120_000));
    assert!(spec.prompt.contains("Kind: batch") && spec.prompt.contains("Task 2: second thing"), "{}", spec.prompt);
}

#[test]
fn the_retry_of_a_batch_run_keeps_the_doubled_cap() {
    let fx = Fixture::new();
    context(&fx);
    let wt = worktree(&fx);
    let snaps = two_snapshots(&wt);
    let runner = scripted(vec![
        nothing(),
        effect(|dir| {
            commit_paths(dir, &["a.txt"], "feat(x): add a");
            commit_paths(dir, &["b.txt"], "feat(x): add b");
            "done".to_string()
        }),
    ]);
    let out = fx.engine(&runner).commit_step(&wt, &batch_request(&wt.base, &snaps)).expect("step");
    assert!(matches!(out, CommitOutcome::Committed { fallback: false, .. }), "{out:?}");
    let retry = runner.last_spec().expect("retry spec");
    assert_eq!(retry.resume.as_deref(), Some("sess-1"));
    assert_eq!(retry.budget_weighted, Some(120_000));
}

#[test]
fn a_failed_batch_falls_back_to_one_chore_commit_per_snapshot() {
    let fx = Fixture::new();
    context(&fx);
    let wt = worktree(&fx);
    let snaps = two_snapshots(&wt);
    let runner = scripted(vec![nothing(), nothing()]);
    let out = fx.engine(&runner).commit_step(&wt, &batch_request(&wt.base, &snaps)).expect("step");
    assert!(matches!(out, CommitOutcome::Committed { fallback: true, .. }), "{out:?}");
    assert_eq!(subjects(&fx, &wt, &wt.base), vec!["chore: first thing", "chore: second thing"]);
    assert!(wt.path.join("a.txt").exists() && wt.path.join("b.txt").exists());
    assert!(fx.repo.worktree_is_clean(&wt).expect("clean"));
    assert!(ledger(&fx).contains("fallback commit"), "ledger: {}", ledger(&fx));
}

#[test]
fn a_batch_fallback_skips_a_snapshot_that_changed_nothing() {
    let fx = Fixture::new();
    context(&fx);
    let wt = worktree(&fx);
    write_change(&wt, "a.txt");
    let first = commit_paths(&wt.path, &["a.txt"], "wip: task 1 first thing");
    // The second snapshot has the same tree as the first.
    let second = {
        let repo = Repository::open(&wt.path).expect("open");
        let head = repo.head().expect("head").peel_to_commit().expect("commit");
        let sig = Signature::now("t", "t@example.com").expect("sig");
        let oid = repo.commit(Some("HEAD"), &sig, &sig, "wip: task 2 empty", &head.tree().expect("tree"), &[&head]).expect("commit");
        oid.to_string()
    };
    reset_mixed(&wt.path, &wt.base);
    let snaps = vec![
        factory_engine::SnapshotRef { n: 1, sha: first, title: "first thing".into(), files: vec!["a.txt".into()] },
        factory_engine::SnapshotRef { n: 2, sha: second, title: "empty thing".into(), files: vec![] },
    ];
    let runner = scripted(vec![nothing(), nothing()]);
    let out = fx.engine(&runner).commit_step(&wt, &batch_request(&wt.base, &snaps)).expect("step");
    assert!(matches!(out, CommitOutcome::Committed { fallback: true, .. }), "{out:?}");
    assert_eq!(subjects(&fx, &wt, &wt.base), vec!["chore: first thing"]);
}

#[test]
fn a_failed_batch_leaves_exactly_the_work_of_the_snapshots() {
    let fx = Fixture::new();
    context(&fx);
    let wt = worktree(&fx);
    let snaps = two_snapshots(&wt);
    let last = snaps.last().expect("snapshot").sha.clone();
    // A stray file the failing committer run left behind must not survive the fallback.
    std::fs::write(wt.path.join("stray.txt"), "x\n").expect("stray");
    let runner = scripted(vec![nothing(), nothing()]);
    let out = fx.engine(&runner).commit_step(&wt, &batch_request(&wt.base, &snaps)).expect("step");
    assert!(matches!(out, CommitOutcome::Committed { fallback: true, .. }), "{out:?}");
    let tip = fx.repo.tip(&wt.branch).expect("tip");
    assert!(fx.repo.same_tree(&tip, &last).expect("same tree"));
    assert!(fx.repo.worktree_is_clean(&wt).expect("clean"));
}
