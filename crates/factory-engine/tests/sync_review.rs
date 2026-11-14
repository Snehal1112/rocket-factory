mod common;
use common::{approved_job_with, reply, scripted};
use factory_engine::{sync_requested, sync_stage, SyncOutcome};
use std::sync::Arc;

const GOOD: &str =
    "---BEGIN MESSAGE---\nfeat(hello): add the hello docs\n\nBody.\n---END MESSAGE---";
const APPROVE: &str = "ok\nVERDICT: APPROVE";
const CHANGES: &str = "- [blocker] fix it\nVERDICT: CHANGES";

/// A fixture in round 1 with a round review already saved.
fn fixture(effects: Vec<common::Effect>) -> common::MergeFixture {
    let fx = approved_job_with(Arc::new(scripted(effects)), true);
    fx.driver
        .store()
        .update_job(&fx.id, |j| {
            j.round = 1;
            Ok(())
        })
        .expect("round");
    let job = fx.driver.job(&fx.id).expect("job");
    std::fs::write(
        fx.driver.job_dir(&job).join("review-code-r1.md"),
        "round review\n",
    )
    .expect("write");
    fx
}

fn job_file(fx: &common::MergeFixture, name: &str) -> std::path::PathBuf {
    let job = fx.driver.job(&fx.id).expect("job");
    fx.driver.job_dir(&job).join(name)
}

const SECURITY_OK: &str = "No findings.\nVERDICT: APPROVE";

/// Amends the job branch tip in its worktree, so the reviewed head is no longer an ancestor.
fn rewrite_branch(fx: &common::MergeFixture) -> String {
    let repo = git2::Repository::open(common::worktree_of(fx)).expect("open worktree");
    let tip = repo.head().expect("head").peel_to_commit().expect("commit");
    tip.amend(Some("HEAD"), None, None, None, Some("rewritten\n"), None)
        .expect("amend")
        .to_string()
}

fn with_security_agent(mut fx: common::MergeFixture) -> common::MergeFixture {
    fx.driver = fx.driver.with_agent_files(vec![factory_core::AgentFile {
        name: "factory-security-reviewer.md".to_string(),
        content: "agent".to_string(),
    }]);
    fx
}

fn ledger(fx: &common::MergeFixture) -> String {
    std::fs::read_to_string(job_file(fx, "progress.md")).expect("ledger")
}

fn review_calls(fx: &common::MergeFixture) -> usize {
    fx.runner.specs_for("reviewer").len() + fx.runner.specs_for("security-review").len()
}

#[test]
fn sync_requested_merges_main_into_a_job_that_is_behind() {
    let fx = fixture(vec![reply(GOOD), reply(APPROVE)]);
    let head = fx.head.clone();
    common::commit_file(&fx.main, "other.md", "o");
    assert_eq!(
        sync_requested(&fx.driver, &fx.id).expect("sync"),
        SyncOutcome::Synced
    );
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.prev_head.as_deref(), Some(head.as_str()));
    assert_ne!(job.head.as_deref(), Some(head.as_str()));
    assert!(job_file(&fx, "review-code-r1-sync1.md").exists());
    assert!(!ledger(&fx).contains("branch rewritten"));
}

#[test]
fn sync_requested_does_not_count_against_max_syncs() {
    let fx = fixture(vec![reply(GOOD), reply(APPROVE)]);
    fx.driver
        .store()
        .update_job(&fx.id, |j| {
            j.syncs = 2;
            Ok(())
        })
        .expect("syncs");
    common::commit_file(&fx.main, "other.md", "o");
    assert_eq!(
        sync_requested(&fx.driver, &fx.id).expect("sync"),
        SyncOutcome::Synced
    );
    assert_eq!(fx.driver.job(&fx.id).expect("job").syncs, 2);
}

#[test]
fn sync_requested_resets_the_floor_after_a_rebase_and_reviews_the_whole_branch() {
    let fx = fixture(vec![reply(APPROVE), reply(SECURITY_OK)]);
    let fx = with_security_agent(fx);
    fx.driver
        .store()
        .update_job(&fx.id, |j| {
            j.prev_head = Some("0".repeat(40));
            Ok(())
        })
        .expect("prev head");
    let tip = rewrite_branch(&fx);
    assert_eq!(
        sync_requested(&fx.driver, &fx.id).expect("sync"),
        SyncOutcome::Synced
    );
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.prev_head, None);
    assert_eq!(job.head.as_deref(), Some(tip.as_str()));
    assert!(ledger(&fx).contains(&format!(
        "branch rewritten outside the factory, floor reset to base, head is {tip}"
    )));
    assert!(job_file(&fx, "review-code-r1-rebase.md").exists());
    assert!(job_file(&fx, "review-code-r1.md").exists());
}

#[test]
fn a_rebase_review_runs_the_security_reviewer() {
    let fx = with_security_agent(fixture(vec![reply(APPROVE), reply(SECURITY_OK)]));
    fx.driver
        .store()
        .update_job(&fx.id, |j| {
            j.set_reviewers(&["code".to_string()]);
            Ok(())
        })
        .expect("reviewers");
    rewrite_branch(&fx);
    sync_requested(&fx.driver, &fx.id).expect("sync");
    assert_eq!(fx.runner.specs_for("reviewer").len(), 1);
    assert_eq!(fx.runner.specs_for("security-review").len(), 1);
    let job = fx.driver.job(&fx.id).expect("job");
    assert!(job.reviews.contains_key("security"));
}

#[test]
fn a_sync_without_a_rewrite_runs_the_code_reviewer_only() {
    let fx = with_security_agent(fixture(vec![reply(GOOD), reply(APPROVE)]));
    fx.driver
        .store()
        .update_job(&fx.id, |j| {
            j.set_reviewers(&["code".to_string(), "security".to_string()]);
            Ok(())
        })
        .expect("reviewers");
    common::commit_file(&fx.main, "other.md", "o");
    sync_requested(&fx.driver, &fx.id).expect("sync");
    assert_eq!(fx.runner.specs_for("reviewer").len(), 1);
    assert!(fx.runner.specs_for("security-review").is_empty());
}

#[test]
fn a_rewritten_branch_that_is_behind_main_gets_main_merged_before_one_review() {
    let fx = with_security_agent(fixture(vec![
        reply(GOOD),
        reply(APPROVE),
        reply(SECURITY_OK),
    ]));
    common::commit_file(&fx.main, "other.md", "o");
    rewrite_branch(&fx);
    assert_eq!(
        sync_requested(&fx.driver, &fx.id).expect("sync"),
        SyncOutcome::Synced
    );
    assert_eq!(review_calls(&fx), 2);
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.prev_head, None);
    assert_eq!(job.head.as_deref(), Some(common::branch_tip(&fx).as_str()));
    let repo = git2::Repository::open(&fx.main).expect("open");
    let head = repo
        .find_commit(git2::Oid::from_str(&common::branch_tip(&fx)).expect("oid"))
        .expect("head");
    assert_eq!(head.parent_count(), 2);
    assert!(!job_file(&fx, "review-code-r1-sync1.md").exists());
    assert!(job_file(&fx, "review-code-r1-rebase.md").exists());
}

#[test]
fn sync_requested_on_an_up_to_date_job_changes_nothing() {
    let fx = fixture(vec![]);
    assert_eq!(
        sync_requested(&fx.driver, &fx.id).expect("sync"),
        SyncOutcome::UpToDate
    );
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.stage, factory_core::Stage::Approve);
    assert_eq!(job.head.as_deref(), Some(fx.head.as_str()));
    assert!(fx.runner.last_spec().is_none());
}

#[test]
fn sync_requested_refuses_a_merged_job() {
    let fx = fixture(vec![]);
    fx.driver
        .store()
        .update_job(&fx.id, |j| {
            j.set_stage(factory_core::Stage::Merged, None, true, 2.0)
        })
        .expect("merged");
    let err = sync_requested(&fx.driver, &fx.id).expect_err("refused");
    assert_eq!(err.to_string(), "job 001 is merged");
}

#[test]
fn sync_requested_refuses_a_cancelled_job() {
    let fx = fixture(vec![]);
    fx.driver
        .store()
        .update_job(&fx.id, |j| {
            j.cancelled = true;
            Ok(())
        })
        .expect("cancelled");
    let err = sync_requested(&fx.driver, &fx.id).expect_err("refused");
    assert_eq!(err.to_string(), "job 001 is cancelled");
}

#[test]
fn sync_requested_refuses_a_dirty_worktree() {
    let fx = fixture(vec![]);
    std::fs::write(common::worktree_of(&fx).join("dirty.txt"), "x").expect("write");
    let err = sync_requested(&fx.driver, &fx.id).expect_err("refused");
    assert_eq!(
        err.to_string(),
        "the worktree of job 001 has uncommitted changes"
    );
}

#[test]
fn sync_requested_conflict_parks_and_names_the_paths() {
    let fx = fixture(vec![reply(GOOD)]);
    common::commit_file(&fx.main, "docs/hello.txt", "main version\n");
    match sync_requested(&fx.driver, &fx.id).expect("sync") {
        SyncOutcome::Parked(text) => assert!(text.contains("docs/hello.txt"), "{text}"),
        other => panic!("expected a park, got {other:?}"),
    }
}

#[test]
fn a_sync_review_keeps_the_round_review() {
    let fx = fixture(vec![reply(GOOD), reply(APPROVE)]);
    common::commit_file(&fx.main, "other.md", "o");
    assert_eq!(
        sync_stage(&fx.driver, &fx.id).expect("sync"),
        SyncOutcome::Synced
    );
    assert_eq!(
        std::fs::read_to_string(job_file(&fx, "review-code-r1.md")).expect("r1"),
        "round review\n"
    );
    let sync = std::fs::read_to_string(job_file(&fx, "review-code-r1-sync1.md")).expect("sync1");
    assert!(sync.contains("VERDICT: APPROVE"));
    let ledger = std::fs::read_to_string(job_file(&fx, "progress.md")).expect("ledger");
    assert!(ledger.contains("sync review round 1 (sync) verdict Approve"));
}

#[test]
fn two_sync_reviews_get_their_own_files() {
    let fx = fixture(vec![
        reply(GOOD),
        reply(APPROVE),
        reply(GOOD),
        reply(APPROVE),
    ]);
    common::commit_file(&fx.main, "other.md", "o");
    sync_stage(&fx.driver, &fx.id).expect("sync 1");
    common::commit_file(&fx.main, "other2.md", "o");
    sync_stage(&fx.driver, &fx.id).expect("sync 2");
    assert!(job_file(&fx, "review-code-r1-sync1.md").exists());
    assert!(job_file(&fx, "review-code-r1-sync2.md").exists());
}

#[test]
fn a_sync_review_that_asks_for_changes_sets_last_review() {
    let fx = fixture(vec![reply(GOOD), reply(CHANGES)]);
    common::commit_file(&fx.main, "other.md", "o");
    assert_eq!(
        sync_stage(&fx.driver, &fx.id).expect("sync"),
        SyncOutcome::Parked("sync review asked for changes".into())
    );
    let last = fx
        .driver
        .job(&fx.id)
        .expect("job")
        .last_review
        .expect("last review");
    assert_eq!(last.verdict, "changes");
    assert_eq!(last.source, "sync");
    assert_eq!(last.file, "review-code-r1-sync1.md");
}
