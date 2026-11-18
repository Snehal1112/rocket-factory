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

/// A fixture whose config runs `command` as the only check.
fn fixture_with_check(command: &str, effects: Vec<common::Effect>) -> common::MergeFixture {
    let config = factory_core::Config {
        checks: vec![factory_core::CheckRule {
            paths: vec!["**".into()],
            commands: vec![command.to_string()],
            ..factory_core::CheckRule::default()
        }],
        ..factory_core::Config::default()
    };
    let fx = common::approved_job_config(Arc::new(scripted(effects)), true, config);
    fx.driver
        .store()
        .update_job(&fx.id, |j| {
            j.round = 1;
            Ok(())
        })
        .expect("round");
    fx
}

fn force_stage(fx: &common::MergeFixture, stage: factory_core::Stage) {
    fx.driver
        .store()
        .update_job(&fx.id, |j| j.set_stage(stage, Some("test"), true, 2.0))
        .expect("stage");
}

fn last_review(verdict: &str) -> factory_core::LastReview {
    factory_core::LastReview {
        round: 1,
        verdict: verdict.to_string(),
        blockers: 1,
        file: "review-code-r1.md".to_string(),
        source: "review".to_string(),
    }
}

#[test]
fn a_rewrite_resets_the_review_states_and_keeps_last_review() {
    let fx = fixture_with_check("false", vec![]);
    fx.driver
        .store()
        .update_job(&fx.id, |j| {
            j.set_reviewers(&["code".to_string(), "security".to_string()]);
            j.set_review("code", factory_core::ReviewState::Pass);
            j.set_review("security", factory_core::ReviewState::Pass);
            j.last_review = Some(last_review("approve"));
            Ok(())
        })
        .expect("reviews");
    rewrite_branch(&fx);
    assert!(matches!(
        sync_requested(&fx.driver, &fx.id).expect("sync"),
        SyncOutcome::Parked(..)
    ));
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.stage, factory_core::Stage::NeedsHuman);
    assert_eq!(
        job.reviews.get("code"),
        Some(&factory_core::ReviewState::Pending)
    );
    assert_eq!(
        job.reviews.get("security"),
        Some(&factory_core::ReviewState::Pending)
    );
    assert_eq!(job.last_review, Some(last_review("approve")));
    let refusal = job.approve_refusal(false, false).expect("refused");
    assert!(
        refusal.contains("has no code and security review yet"),
        "{refusal}"
    );
}

#[test]
fn a_rewrite_keeps_open_blockers_so_approve_without_review_is_still_refused() {
    let fx = fixture_with_check("false", vec![]);
    fx.driver
        .store()
        .update_job(&fx.id, |j| {
            j.last_review = Some(last_review("changes"));
            Ok(())
        })
        .expect("last review");
    rewrite_branch(&fx);
    sync_requested(&fx.driver, &fx.id).expect("sync");
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.last_review, Some(last_review("changes")));
    let refusal = job.approve_refusal(true, false).expect("refused");
    assert!(refusal.contains("open review blockers"), "{refusal}");
}

#[test]
fn a_second_sync_after_a_stopped_first_one_still_reviews_the_whole_branch() {
    // A first sync that stopped right after its reset left this state: stage review, pending
    // reviews, no floor, and main not merged yet.
    let fx = with_security_agent(fixture(behind_main_effects()));
    common::commit_file(&fx.main, "other.md", "o");
    fx.driver
        .store()
        .update_job(&fx.id, |j| {
            j.prev_head = None;
            j.reset_reviews();
            j.set_stage(factory_core::Stage::Review, None, true, 2.0)
        })
        .expect("reset state");
    assert_eq!(
        sync_requested(&fx.driver, &fx.id).expect("second sync"),
        SyncOutcome::Synced
    );
    assert_eq!(fx.runner.specs_for("reviewer").len(), 1);
    assert_eq!(fx.runner.specs_for("security-review").len(), 1);
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.prev_head, None);
    assert!(job_file(&fx, "review-code-r1-rebase.md").exists());
}

/// The texts of every saved review diff in the job folder.
fn saved_diffs(fx: &common::MergeFixture) -> Vec<String> {
    let job = fx.driver.job(&fx.id).expect("job");
    std::fs::read_dir(fx.driver.job_dir(&job))
        .expect("job dir")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "diff"))
        .map(|path| std::fs::read_to_string(path).expect("diff"))
        .collect()
}

#[test]
fn a_rebase_review_diff_starts_at_the_merge_base() {
    let fx = with_security_agent(fixture(behind_main_effects()));
    common::commit_file(&fx.main, "docs/other.txt", "o\n");
    rewrite_branch(&fx);
    sync_requested(&fx.driver, &fx.id).expect("sync");
    let diffs = saved_diffs(&fx);
    assert!(!diffs.is_empty());
    for diff in &diffs {
        assert!(!diff.contains("docs/other.txt"), "{diff}");
        assert!(diff.contains("docs/hello.txt"), "{diff}");
    }
}

#[test]
fn a_first_round_review_diff_is_unchanged() {
    let fx = fixture(vec![reply(APPROVE)]);
    force_stage(&fx, factory_core::Stage::Review);
    fx.driver.checks(&fx.id).expect("checks");
    fx.driver.review(&fx.id).expect("review");
    // The branch forked from the main tip, so the base and the merge base are the same commit.
    let stem = format!("review-{}..", &common::main_tip(&fx.main)[..7]);
    let job = fx.driver.job(&fx.id).expect("job");
    let found = std::fs::read_dir(fx.driver.job_dir(&job))
        .expect("job dir")
        .any(|e| {
            let name = e.expect("entry").file_name().to_string_lossy().into_owned();
            name.starts_with(&stem) && name.ends_with(".diff")
        });
    assert!(found, "no diff file starts with {stem}");
}

/// The effects of a requested sync of a job behind main: a message, a code review and a security review.
fn behind_main_effects() -> Vec<common::Effect> {
    vec![reply(GOOD), reply(APPROVE), reply(SECURITY_OK)]
}

#[test]
fn a_plain_sync_keeps_the_floor_only_in_approve_with_tip_equal_head() {
    // Approve keeps the floor and runs the sync review.
    let fx = with_security_agent(fixture(vec![reply(GOOD), reply(APPROVE)]));
    let head = fx.head.clone();
    common::commit_file(&fx.main, "other.md", "o");
    sync_requested(&fx.driver, &fx.id).expect("sync");
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.prev_head.as_deref(), Some(head.as_str()));
    assert!(job_file(&fx, "review-code-r1-sync1.md").exists());
    assert!(fx.runner.specs_for("security-review").is_empty());
    // Every other open stage gets the whole-branch review and no floor.
    for stage in [
        factory_core::Stage::Review,
        factory_core::Stage::Build,
        factory_core::Stage::NeedsHuman,
    ] {
        let fx = with_security_agent(fixture(behind_main_effects()));
        common::commit_file(&fx.main, "other.md", "o");
        force_stage(&fx, stage);
        sync_requested(&fx.driver, &fx.id).expect("sync");
        let job = fx.driver.job(&fx.id).expect("job");
        assert_eq!(job.prev_head, None, "{stage}");
        assert!(
            job_file(&fx, "review-code-r1-rebase.md").exists(),
            "{stage}"
        );
        assert!(
            !job_file(&fx, "review-code-r1-sync1.md").exists(),
            "{stage}"
        );
    }
}

#[test]
fn commits_added_on_top_of_head_get_the_whole_branch_review() {
    let fx = with_security_agent(fixture(vec![reply(APPROVE), reply(SECURITY_OK)]));
    let tip = common::commit_file(&common::worktree_of(&fx), "extra.md", "e");
    sync_requested(&fx.driver, &fx.id).expect("sync");
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.head.as_deref(), Some(tip.as_str()));
    assert_eq!(job.prev_head, None);
    assert!(job_file(&fx, "review-code-r1-rebase.md").exists());
    assert_eq!(fx.runner.specs_for("security-review").len(), 1);
}

#[test]
fn an_up_to_date_job_with_tip_equal_head_changes_nothing_in_every_stage() {
    for stage in [
        factory_core::Stage::Approve,
        factory_core::Stage::Review,
        factory_core::Stage::Build,
        factory_core::Stage::NeedsHuman,
    ] {
        let fx = fixture(vec![]);
        force_stage(&fx, stage);
        assert_eq!(
            sync_requested(&fx.driver, &fx.id).expect("sync"),
            SyncOutcome::UpToDate,
            "{stage}"
        );
        let job = fx.driver.job(&fx.id).expect("job");
        assert_eq!(job.stage, stage);
        assert_eq!(job.head.as_deref(), Some(fx.head.as_str()));
        assert!(fx.runner.last_spec().is_none(), "{stage}");
    }
}

#[test]
fn a_sync_runs_the_checks() {
    let marker = tempfile::tempdir().expect("marker dir");
    let file = marker.path().join("checks-ran");
    let seen = file.clone();
    // The review reply approves only when the checks already ran.
    let review: common::Effect = Box::new(move |_| {
        if seen.exists() {
            APPROVE.to_string()
        } else {
            CHANGES.to_string()
        }
    });
    let fx = fixture_with_check(
        &format!("touch {}", file.display()),
        vec![reply(GOOD), review],
    );
    common::commit_file(&fx.main, "other.md", "o");
    assert_eq!(
        sync_requested(&fx.driver, &fx.id).expect("sync"),
        SyncOutcome::Synced
    );
    assert!(file.exists());
}

#[test]
fn sync_refuses_a_job_with_no_worktree() {
    let fx = fixture(vec![]);
    fx.driver
        .store()
        .update_job(&fx.id, |j| {
            j.worktree = None;
            Ok(())
        })
        .expect("no worktree");
    let err = sync_requested(&fx.driver, &fx.id).expect_err("refused");
    assert_eq!(err.to_string(), "job has no worktree to sync");
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
        "whole-branch review requested, floor reset to base, head is {tip}"
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
    assert_conflict_parks(&fx);
}

#[test]
fn sync_requested_conflict_on_a_rewritten_branch_parks_in_needs_human() {
    let fx = fixture(vec![reply(GOOD)]);
    common::commit_file(&fx.main, "docs/hello.txt", "main version\n");
    rewrite_branch(&fx);
    assert_conflict_parks(&fx);
    // The floor reset stays, so a later review covers the whole branch.
    assert_eq!(fx.driver.job(&fx.id).expect("job").prev_head, None);
}

/// Asserts that a requested sync hits the conflict, parks the job and writes a park record.
fn assert_conflict_parks(fx: &common::MergeFixture) {
    match sync_requested(&fx.driver, &fx.id).expect("sync") {
        SyncOutcome::Parked(text, _) => assert!(text.contains("docs/hello.txt"), "{text}"),
        other => panic!("expected a park, got {other:?}"),
    }
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.stage, factory_core::Stage::NeedsHuman);
    let rec = factory_core::ParkRecord::read(&fx.driver.job_dir(&job)).expect("park.json");
    assert!(rec.reason.contains("docs/hello.txt"), "{}", rec.reason);
}

#[test]
fn a_sync_conflict_on_a_needs_human_job_replaces_the_park_record() {
    let fx = fixture(vec![reply(GOOD)]);
    factory_engine::park_with_parts(
        &fx.driver,
        &fx.id,
        "nothing changed",
        factory_core::ParkKind::NothingChanged,
        vec![factory_core::ParkPart::new(
            "Old",
            "driver",
            "old evidence text",
        )],
    )
    .expect("old park");
    common::commit_file(&fx.main, "docs/hello.txt", "main version\n");
    assert!(matches!(
        sync_requested(&fx.driver, &fx.id).expect("sync"),
        SyncOutcome::Parked(..)
    ));
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.park_kind.as_deref(), Some("other"));
    let rec = factory_core::ParkRecord::read(&fx.driver.job_dir(&job)).expect("park.json");
    assert_eq!(rec.kind, factory_core::ParkKind::Other);
    assert!(rec.reason.contains("docs/hello.txt"), "{}", rec.reason);
    let text = serde_json::to_string(&rec).expect("json");
    assert!(!text.contains("old evidence text"), "{text}");
    assert!(
        ledger(&fx).contains("parked in needs-human: sync conflict"),
        "{}",
        ledger(&fx)
    );
}

#[test]
fn a_checks_gate_failure_during_a_sync_keeps_its_own_park_kind() {
    let config = factory_core::Config {
        checks: vec![factory_core::CheckRule {
            paths: vec!["src/**".into()],
            commands: vec!["true".into()],
            ..factory_core::CheckRule::default()
        }],
        code_paths: vec!["**".into()],
        ..factory_core::Config::default()
    };
    let fx = common::approved_job_config(Arc::new(scripted(vec![])), true, config);
    force_stage(&fx, factory_core::Stage::NeedsHuman);
    rewrite_branch(&fx);
    assert!(matches!(
        sync_requested(&fx.driver, &fx.id).expect("sync"),
        SyncOutcome::Parked(..)
    ));
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.stage, factory_core::Stage::NeedsHuman);
    assert_eq!(
        job.park_kind.as_deref(),
        Some("no-checks"),
        "{:?}",
        job.reason
    );
    let rec = factory_core::ParkRecord::read(&fx.driver.job_dir(&job)).expect("park.json");
    assert_eq!(rec.kind, factory_core::ParkKind::NoChecks);
    assert!(rec.reason.contains("no check covers"), "{}", rec.reason);
}

#[test]
fn a_changes_remain_decision_keeps_its_park_kind_after_a_sync() {
    let fx = with_security_agent(fixture(vec![reply(CHANGES), reply(SECURITY_OK)]));
    fx.driver
        .store()
        .update_job(&fx.id, |j| {
            j.round = 9;
            Ok(())
        })
        .expect("round");
    rewrite_branch(&fx);
    assert!(matches!(
        sync_requested(&fx.driver, &fx.id).expect("sync"),
        SyncOutcome::Parked(_, Some(factory_core::ParkKind::ChangesRemain))
    ));
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.stage, factory_core::Stage::NeedsHuman);
    let rec = factory_core::ParkRecord::read(&fx.driver.job_dir(&job)).expect("park.json");
    assert_eq!(rec.kind, factory_core::ParkKind::ChangesRemain);
    assert!(
        rec.next
            .iter()
            .any(|n| n.starts_with("factory approve 001 --waive")),
        "{:?}",
        rec.next
    );
}

#[test]
fn a_passing_whole_branch_review_re_parks_a_needs_human_job() {
    let fx = with_security_agent(fixture(vec![reply(APPROVE), reply(SECURITY_OK)]));
    force_stage(&fx, factory_core::Stage::NeedsHuman);
    rewrite_branch(&fx);
    assert_eq!(
        sync_requested(&fx.driver, &fx.id).expect("sync"),
        SyncOutcome::Synced
    );
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.stage, factory_core::Stage::NeedsHuman);
    assert_eq!(
        job.reason.as_deref(),
        Some("sync review passed, run factory approve 001")
    );
    assert!(job.last_review.is_some());
    assert_eq!(job.approve_refusal(false, false), None);
}

#[test]
fn a_rebase_review_keeps_the_round_raw_files() {
    let fx = with_security_agent(fixture(vec![reply(APPROVE), reply(SECURITY_OK)]));
    for role in ["code", "security"] {
        std::fs::write(
            job_file(&fx, &format!("review-{role}-r1.raw.md")),
            "round raw\n",
        )
        .expect("round raw");
    }
    rewrite_branch(&fx);
    sync_requested(&fx.driver, &fx.id).expect("sync");
    for role in ["code", "security"] {
        assert_eq!(
            std::fs::read_to_string(job_file(&fx, &format!("review-{role}-r1.raw.md")))
                .expect("round raw"),
            "round raw\n"
        );
        assert!(job_file(&fx, &format!("review-{role}-r1-rebase.raw.md")).exists());
    }
}

#[test]
fn sync_refuses_a_pending_batch_snapshot() {
    let fx = fixture(vec![]);
    fx.driver
        .store()
        .update_job(&fx.id, |j| {
            j.batch_snapshot = Some("abc1234".to_string());
            Ok(())
        })
        .expect("snapshot");
    let err = sync_requested(&fx.driver, &fx.id).expect_err("refused");
    assert_eq!(
        err.to_string(),
        "job 001 has a pending batch commit, use rework to finish it"
    );
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
        SyncOutcome::Parked("sync review asked for changes".into(), None)
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

#[test]
fn a_failed_requested_sync_review_parks_with_resume_rereview() {
    let inner = Arc::new(scripted(vec![reply(APPROVE)]));
    let failing = common::RoleFailing::new(
        "reviewer",
        factory_agent::StopReason::Failed,
        1,
        inner.clone(),
    );
    let fx = common::approved_job_dyn(
        Arc::new(failing),
        inner,
        true,
        factory_core::Config::default(),
    );
    fx.driver
        .store()
        .update_job(&fx.id, |j| {
            j.round = 1;
            Ok(())
        })
        .expect("round");
    common::commit_file(&fx.main, "other.md", "o");
    let outcome = sync_requested(&fx.driver, &fx.id).expect("sync");
    assert!(
        matches!(&outcome, SyncOutcome::Parked(m, None) if m.contains("review failed")),
        "{outcome:?}"
    );
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.stage, factory_core::Stage::NeedsHuman);
    assert_eq!(job.park_kind.as_deref(), Some("review-failed"));
    assert_eq!(job.park_resume, Some(factory_core::ResumeAt::Rereview));
    let rec = factory_core::ParkRecord::read(&fx.driver.job_dir(&job)).expect("park.json");
    assert_eq!(rec.kind, factory_core::ParkKind::ReviewFailed);
    assert_eq!(rec.resume, Some(factory_core::ResumeAt::Rereview));
}
