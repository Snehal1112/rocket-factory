mod common;
use common::{approved_job_with, reply, scripted};
use factory_engine::{sync_stage, SyncOutcome};
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
