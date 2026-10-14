mod common;

use std::path::Path;

use common::*;
use factory_agent::AgentSpec;
use factory_engine::{CommitKind, CommitOutcome, CommitRequest, COMMIT_CONTEXT_FILE};
use factory_git::{JobRef, Worktree};
use git2::{Repository, Signature};

const GOOD: &str = "feat(x): add the thing\n\nThe thing was missing.";

/// Writes the prompt context file into the main checkout, where the engine reads it.
fn context(fx: &Fixture) {
    let path = fx.main.join(COMMIT_CONTEXT_FILE);
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
