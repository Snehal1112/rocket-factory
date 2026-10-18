mod common;
use common::{approved_job_with, reply, scripted, scripted_auto, Effect};
use factory_agent::AgentSpec;
use factory_engine::{merge_stage, MergeResult};
use std::sync::Arc;

const GOOD: &str = "---BEGIN MESSAGE---\nfeat(hello): add the hello docs\n\nBody.\n---END MESSAGE---";

fn subject(fx: &common::MergeFixture, sha: &str) -> String {
    let repo = git2::Repository::open(&fx.main).expect("open");
    let commit = repo.find_commit(git2::Oid::from_str(sha).expect("oid")).expect("commit");
    commit.message().expect("msg").to_string()
}

fn merged(fx: &common::MergeFixture) -> String {
    match merge_stage(&fx.driver, &fx.id).expect("merge") {
        MergeResult::Merged(sha) => sha,
        other => panic!("expected a merge, got {other:?}"),
    }
}

#[test]
fn merge_uses_the_committer_message() {
    let fx = approved_job_with(Arc::new(scripted(vec![reply(GOOD)])), true);
    let sha = merged(&fx);
    assert!(subject(&fx, &sha).starts_with("feat(hello): add the hello docs"));
    let repo = git2::Repository::open(&fx.main).expect("open");
    let commit = repo.find_commit(git2::Oid::from_str(&sha).expect("oid")).expect("commit");
    assert_eq!(commit.parent_id(1).expect("second parent").to_string(), fx.head);
    let specs = fx.runner.specs_for("message");
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].tools.as_deref(), Some("Read,Skill"));
}

#[test]
fn an_invalid_reply_retries_once_then_uses_the_fixed_message() {
    let fx = approved_job_with(Arc::new(scripted(vec![reply("no markers"), reply("still none")])), true);
    let sha = merged(&fx);
    assert_eq!(subject(&fx, &sha).lines().next(), Some("chore(factory): merge job 001 hello"));
    let specs = fx.runner.specs_for("message");
    assert_eq!(specs.len(), 2);
    assert!(specs[1].resume.is_some());
    assert!(fx.ledger().contains("message failed twice"));
}

#[test]
fn a_valid_retry_is_used() {
    let fx = approved_job_with(Arc::new(scripted(vec![reply("no markers"), reply(GOOD)])), true);
    let sha = merged(&fx);
    assert!(subject(&fx, &sha).starts_with("feat(hello): add the hello docs"));
    assert!(!fx.ledger().contains("failed twice"));
}

#[test]
fn a_failed_run_still_merges() {
    let fx = approved_job_with(Arc::new(scripted(vec![])), true);
    let sha = merged(&fx);
    assert_eq!(subject(&fx, &sha).lines().next(), Some("chore(factory): merge job 001 hello"));
}

#[test]
fn sync_uses_the_committer_message() {
    let effects = vec![reply(GOOD), reply("ok\nVERDICT: APPROVE")];
    let fx = approved_job_with(Arc::new(scripted(effects)), true);
    common::commit_file(&fx.main, "other.md", "o");
    merged(&fx);
    assert_eq!(fx.driver.job(&fx.id).expect("job").syncs, 1);
    let repo = git2::Repository::open(&fx.main).expect("open");
    let branch = fx.driver.job(&fx.id).expect("job").branch;
    let tip = repo.revparse_single(&branch).expect("branch").peel_to_commit().expect("commit");
    let sync_commit = tip;
    assert!(sync_commit.message().expect("msg").starts_with("feat(hello): add the hello docs"));
    let specs = fx.runner.specs_for("message");
    assert!(specs[0].prompt.contains("Kind: sync"));
}

#[test]
fn a_message_run_never_commits_the_index() {
    let fx = approved_job_with(Arc::new(scripted_auto(vec![])), true);
    let wt = common::worktree_of(&fx);
    std::fs::write(wt.join("staged.txt"), "x").expect("write");
    let repo = git2::Repository::open(&wt).expect("open");
    let mut index = repo.index().expect("index");
    index.add_path(std::path::Path::new("staged.txt")).expect("add");
    index.write().expect("write");
    let before = common::branch_tip(&fx);
    fx.driver.commit_message(&fx.id, factory_engine::MessageKind::Merge).expect("message");
    assert_eq!(common::branch_tip(&fx), before);
    assert_eq!(fx.runner.specs_for("message").len(), 2);
    assert!(fx.runner.specs_for("committer").is_empty());
}

#[test]
fn main_moving_during_the_message_run_syncs_before_merging() {
    let main_dir = Arc::new(std::sync::Mutex::new(std::path::PathBuf::new()));
    let moves = Arc::clone(&main_dir);
    let mover: Effect = Box::new(move |_: &AgentSpec| {
        let dir = moves.lock().expect("dir").clone();
        common::commit_file(&dir, "late-main.md", "m");
        GOOD.to_string()
    });
    let effects = vec![mover, reply(GOOD), reply("ok\nVERDICT: APPROVE"), reply(GOOD)];
    let fx = approved_job_with(Arc::new(scripted(effects)), true);
    *main_dir.lock().expect("dir") = fx.main.clone();
    merged(&fx);
    assert_eq!(fx.driver.job(&fx.id).expect("job").syncs, 1);
}

fn with_issues(fx: &common::MergeFixture, issues: Vec<u32>) {
    fx.driver
        .store()
        .update_job(&fx.id, |j| {
            j.issues = issues;
            Ok(())
        })
        .expect("issues");
}

#[test]
fn merge_message_ends_with_the_fixes_and_job_footers() {
    let fx = approved_job_with(Arc::new(scripted(vec![reply(GOOD)])), true);
    with_issues(&fx, vec![20]);
    let sha = merged(&fx);
    assert!(subject(&fx, &sha).ends_with("Body.\n\nFixes: #20\nFactory-Job: 001"), "{}", subject(&fx, &sha));
    assert!(fx.runner.specs_for("message")[0].prompt.contains("Fixes: #20, Factory-Job: 001"));
}

#[test]
fn the_fixed_merge_message_gets_the_footers_too() {
    let fx = approved_job_with(Arc::new(scripted(vec![])), true);
    with_issues(&fx, vec![20]);
    let sha = merged(&fx);
    assert_eq!(subject(&fx, &sha), "chore(factory): merge job 001 hello\n\nFixes: #20\nFactory-Job: 001");
}

#[test]
fn a_job_without_issues_gets_only_the_job_footer() {
    let fx = approved_job_with(Arc::new(scripted(vec![reply(GOOD)])), true);
    let sha = merged(&fx);
    assert!(subject(&fx, &sha).ends_with("Body.\n\nFactory-Job: 001"), "{}", subject(&fx, &sha));
}

#[test]
fn a_reply_that_has_the_footer_gets_no_duplicate() {
    const WITH: &str = "---BEGIN MESSAGE---\nfeat(hello): add the hello docs\n\nFixes: #20\n---END MESSAGE---";
    let fx = approved_job_with(Arc::new(scripted(vec![reply(WITH)])), true);
    with_issues(&fx, vec![20]);
    let sha = merged(&fx);
    assert_eq!(subject(&fx, &sha).matches("Fixes: #20").count(), 1);
    assert!(subject(&fx, &sha).ends_with("Fixes: #20\nFactory-Job: 001"));
}

#[test]
fn sync_message_uses_relates_to_then_the_job_footer() {
    let effects = vec![reply(GOOD), reply("ok\nVERDICT: APPROVE")];
    let fx = approved_job_with(Arc::new(scripted(effects)), true);
    with_issues(&fx, vec![20]);
    common::commit_file(&fx.main, "other.md", "o");
    merged(&fx);
    let repo = git2::Repository::open(&fx.main).expect("open");
    let branch = fx.driver.job(&fx.id).expect("job").branch;
    let tip = repo.revparse_single(&branch).expect("branch").peel_to_commit().expect("commit");
    assert!(tip.message().expect("msg").ends_with("Relates to: #20\nFactory-Job: 001"), "{}", tip.message().expect("msg"));
}
