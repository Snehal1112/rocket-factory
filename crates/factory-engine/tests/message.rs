mod common;
use common::{approved_job_with, reply, scripted};
use factory_engine::{merge_stage, MergeResult};
use std::sync::Arc;

fn subject(fx: &common::MergeFixture, sha: &str) -> String {
    let repo = git2::Repository::open(&fx.main).expect("open");
    let commit = repo
        .find_commit(git2::Oid::from_str(sha).expect("oid"))
        .expect("commit");
    commit.message().expect("msg").to_string()
}

fn merged(fx: &common::MergeFixture) -> String {
    match merge_stage(&fx.driver, &fx.id).expect("merge") {
        MergeResult::Merged(sha) => sha,
        other => panic!("expected a merge, got {other:?}"),
    }
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
    let fx = approved_job_with(Arc::new(scripted(vec![])));
    with_issues(&fx, vec![20]);
    let sha = merged(&fx);
    assert!(
        subject(&fx, &sha).ends_with("\n\nFixes: #20\nFactory-Job: 001"),
        "{}",
        subject(&fx, &sha)
    );
}

const SPEC_WITH_FOLLOW_UPS: &str = "## Goal\nsomething with enough words to pass the check here\n\n## Follow-ups\n\n- More work\n\n## Acceptance criteria\n- a\n";

fn set_series(fx: &common::MergeFixture, index: u32, total: u32) {
    fx.driver
        .store()
        .update_job(&fx.id, move |j| {
            j.series = Some(factory_core::SeriesPos {
                plan_file: "p".into(),
                index,
                total,
                id: None,
            });
            Ok(())
        })
        .expect("series");
}

#[test]
fn the_last_plan_says_fixes_even_with_follow_ups() {
    let fx = approved_job_with(Arc::new(scripted(vec![])));
    common::set_feature(&fx, "x #20", Some(SPEC_WITH_FOLLOW_UPS));
    with_issues(&fx, vec![20]);
    set_series(&fx, 3, 3);
    let sha = merged(&fx);
    assert!(
        subject(&fx, &sha).ends_with("Fixes: #20\nFactory-Job: 001"),
        "{}",
        subject(&fx, &sha)
    );
}

#[test]
fn plan_one_of_three_says_relates_to_even_without_follow_ups() {
    let fx = approved_job_with(Arc::new(scripted(vec![])));
    with_issues(&fx, vec![20]);
    set_series(&fx, 1, 3);
    let sha = merged(&fx);
    assert!(
        subject(&fx, &sha).ends_with("Relates to: #20\nFactory-Job: 001"),
        "{}",
        subject(&fx, &sha)
    );
}

#[test]
fn a_single_plan_job_with_follow_ups_says_fixes() {
    let fx = approved_job_with(Arc::new(scripted(vec![])));
    common::set_feature(&fx, "x #20", Some(SPEC_WITH_FOLLOW_UPS));
    with_issues(&fx, vec![20]);
    let sha = merged(&fx);
    assert!(
        subject(&fx, &sha).ends_with("Fixes: #20\nFactory-Job: 001"),
        "{}",
        subject(&fx, &sha)
    );
}

#[test]
fn a_job_without_issues_gets_only_the_job_footer() {
    let fx = approved_job_with(Arc::new(scripted(vec![])));
    let sha = merged(&fx);
    assert!(
        subject(&fx, &sha).ends_with("\n\nFactory-Job: 001"),
        "{}",
        subject(&fx, &sha)
    );
}

#[test]
fn sync_message_uses_relates_to_then_the_job_footer() {
    let effects = vec![reply("ok\nVERDICT: APPROVE")];
    let fx = approved_job_with(Arc::new(scripted(effects)));
    with_issues(&fx, vec![20]);
    common::commit_file(&fx.main, "other.md", "o");
    merged(&fx);
    let repo = git2::Repository::open(&fx.main).expect("open");
    let branch = fx.driver.job(&fx.id).expect("job").branch;
    let tip = repo
        .revparse_single(&branch)
        .expect("branch")
        .peel_to_commit()
        .expect("commit");
    assert!(
        tip.message()
            .expect("msg")
            .ends_with("Relates to: #20\nFactory-Job: 001"),
        "{}",
        tip.message().expect("msg")
    );
}
