mod common;
use common::{approved_job, set_feature, with_tracker, FakeTracker, MergeFixture};
use factory_engine::{merge_stage, tickets_step, MergeResult};

const SPEC_WITH_TWO: &str = "## Goal\nsomething with enough words to pass the check here\n\n## Follow-ups\n\n- Shared name helpers. More detail here.\n- Merge QueryParam and Header\n\n## Acceptance criteria\n- a\n";
const SPEC_NONE: &str =
    "## Goal\nsomething with enough words to pass the check here\n\n## Acceptance criteria\n- a\n";

/// A merged job `001` whose feature text cites issue 76, with a fake tracker for `o/r`.
fn merged(spec: Option<&str>, tracker: &FakeTracker) -> MergeFixture {
    let fx = with_tracker(approved_job(), tracker.clone());
    set_feature(&fx, "dedup cleanups https://github.com/o/r/issues/76", spec);
    assert!(matches!(
        merge_stage(&fx.driver, &fx.id).expect("merge"),
        MergeResult::Merged(_)
    ));
    fx
}

fn tracker() -> FakeTracker {
    FakeTracker::new("o/r").with_labels(76, &["dedup", "refactor", "tracking", "tier-2"])
}

#[test]
fn files_one_ticket_per_follow_up_comments_once_and_closes() {
    let t = tracker();
    let fx = merged(Some(SPEC_WITH_TWO), &t);
    let report = tickets_step(&fx.driver, &fx.id, false).expect("step");
    assert!(!report.failed && !report.skipped, "{report:?}");
    let s = t.state.lock().expect("lock");
    assert_eq!(s.created.len(), 2);
    assert_eq!(s.created[0].0, "Shared name helpers");
    assert_eq!(s.created[1].0, "Merge QueryParam and Header");
    assert_eq!(
        s.created[0].2,
        vec![
            "dedup".to_string(),
            "refactor".to_string(),
            "tier-2".to_string()
        ]
    );
    assert!(s.created[0].1.contains("#76"));
    assert_eq!(s.comments.len(), 1);
    assert_eq!(s.comments[0].0, 76);
    assert!(
        s.comments[0].1.contains("#201") && s.comments[0].1.contains("#202"),
        "{}",
        s.comments[0].1
    );
    assert_eq!(s.closes, vec![76]);
    let marker = std::fs::read_to_string(
        fx.driver
            .job_dir(&fx.driver.job(&fx.id).expect("job"))
            .join("tickets.json"),
    )
    .expect("marker");
    assert!(marker.contains("Shared name helpers") && marker.contains("201"));
}

#[test]
fn no_spec_file_means_no_tickets_but_the_parent_closes() {
    let t = tracker();
    let fx = merged(None, &t);
    let report = tickets_step(&fx.driver, &fx.id, false).expect("step");
    assert!(!report.failed, "{report:?}");
    let s = t.state.lock().expect("lock");
    assert!(s.created.is_empty());
    assert_eq!(s.comments.len(), 1);
    assert!(
        s.comments[0].1.contains("No remaining work"),
        "{}",
        s.comments[0].1
    );
    assert_eq!(s.closes, vec![76]);
}

#[test]
fn an_empty_follow_ups_section_only_comments_and_closes() {
    let t = tracker();
    let fx = merged(Some("## Goal\nsomething with enough words to pass the check here\n\n## Follow-ups\n\n## Acceptance criteria\n- a\n"), &t);
    tickets_step(&fx.driver, &fx.id, false).expect("step");
    let s = t.state.lock().expect("lock");
    assert!(s.created.is_empty());
    assert_eq!(s.closes, vec![76]);
}

#[test]
fn an_already_closed_parent_is_not_closed_again() {
    let t = tracker();
    t.state.lock().expect("lock").closed.insert(76);
    let fx = merged(Some(SPEC_NONE), &t);
    let report = tickets_step(&fx.driver, &fx.id, false).expect("step");
    assert!(!report.failed, "{report:?}");
    assert_eq!(t.count("close"), 0);
}

#[test]
fn rerun_after_a_failed_create_files_once() {
    let t = tracker();
    let fx = merged(Some(SPEC_WITH_TWO), &t);
    t.fail_on("create");
    let first = tickets_step(&fx.driver, &fx.id, false).expect("step");
    assert!(first.failed);
    assert_eq!(
        t.count("comment"),
        0,
        "the parent is not commented when filing failed"
    );
    assert_eq!(t.count("close"), 0);
    t.clear_failures();
    let second = tickets_step(&fx.driver, &fx.id, false).expect("step");
    assert!(!second.failed, "{second:?}");
    let s = t.state.lock().expect("lock");
    assert_eq!(s.created.len(), 2);
    assert_eq!(s.comments.len(), 1);
    assert_eq!(s.closes, vec![76]);
}

#[test]
fn rerun_after_a_failed_comment_does_not_create_again() {
    let t = tracker();
    let fx = merged(Some(SPEC_WITH_TWO), &t);
    t.fail_on("comment");
    assert!(
        tickets_step(&fx.driver, &fx.id, false)
            .expect("step")
            .failed
    );
    assert_eq!(t.count("create"), 2);
    assert_eq!(
        t.count("close"),
        0,
        "the parent is not closed before it is commented"
    );
    t.clear_failures();
    assert!(
        !tickets_step(&fx.driver, &fx.id, false)
            .expect("step")
            .failed
    );
    assert_eq!(t.count("create"), 2);
    assert_eq!(t.state.lock().expect("lock").comments.len(), 1);
    assert_eq!(t.state.lock().expect("lock").closes, vec![76]);
}

#[test]
fn rerun_after_a_failed_close_does_not_comment_again() {
    let t = tracker();
    let fx = merged(Some(SPEC_NONE), &t);
    t.fail_on("close");
    assert!(
        tickets_step(&fx.driver, &fx.id, false)
            .expect("step")
            .failed
    );
    t.clear_failures();
    assert!(
        !tickets_step(&fx.driver, &fx.id, false)
            .expect("step")
            .failed
    );
    let s = t.state.lock().expect("lock");
    assert_eq!(s.comments.len(), 1);
    assert_eq!(s.closes, vec![76]);
}

#[test]
fn dry_run_makes_only_read_calls() {
    let t = tracker();
    let fx = merged(Some(SPEC_WITH_TWO), &t);
    let report = tickets_step(&fx.driver, &fx.id, true).expect("step");
    assert!(!report.failed, "{report:?}");
    for call in t.calls() {
        assert!(
            call == "repo_name" || call.starts_with("labels_of") || call.starts_with("is_closed"),
            "write call {call}"
        );
    }
    let text = report.lines.join("\n");
    assert!(text.contains("would create: Shared name helpers"), "{text}");
    assert!(text.contains("would comment on #76"), "{text}");
    assert!(text.contains("would close #76"), "{text}");
    let job = fx.driver.job(&fx.id).expect("job");
    assert!(!fx.driver.job_dir(&job).join("tickets.json").exists());
}

#[test]
fn skips_a_job_that_is_not_merged_without_any_call() {
    let t = tracker();
    let fx = with_tracker(approved_job(), t.clone());
    set_feature(&fx, "work on #76", Some(SPEC_WITH_TWO));
    let report = tickets_step(&fx.driver, &fx.id, false).expect("step");
    assert!(report.skipped && !report.failed);
    assert!(report.lines.join(" ").contains("not merged"), "{report:?}");
    assert!(t.calls().is_empty());
}

#[test]
fn skips_a_job_that_is_not_the_last_plan() {
    let t = tracker();
    let fx = merged(Some(SPEC_WITH_TWO), &t);
    fx.driver
        .store()
        .update_job(&fx.id, |j| {
            j.series = Some(factory_core::SeriesPos {
                plan_file: "p".into(),
                index: 1,
                total: 2,
                id: None,
            });
            Ok(())
        })
        .expect("series");
    let report = tickets_step(&fx.driver, &fx.id, false).expect("step");
    assert!(report.skipped, "{report:?}");
    assert!(report.lines.join(" ").contains("last plan"), "{report:?}");
    assert_eq!(t.count("create"), 0);
}

#[test]
fn skips_a_feature_without_issue_refs() {
    let t = tracker();
    let fx = merged(Some(SPEC_WITH_TWO), &t);
    set_feature(&fx, "no tickets here", None);
    let report = tickets_step(&fx.driver, &fx.id, false).expect("step");
    assert!(report.skipped, "{report:?}");
    assert!(report.lines.join(" ").contains("no issue"), "{report:?}");
}

#[test]
fn skips_a_url_for_another_repository() {
    let t = tracker();
    let fx = merged(Some(SPEC_WITH_TWO), &t);
    set_feature(&fx, "see https://github.com/other/thing/issues/5", None);
    let report = tickets_step(&fx.driver, &fx.id, false).expect("step");
    assert!(report.skipped, "{report:?}");
    assert!(report.lines.join(" ").contains("other/thing"), "{report:?}");
    assert_eq!(t.count("create"), 0);
}

#[test]
fn several_parents_each_get_one_comment_and_one_close() {
    let t = tracker().with_labels(77, &["bug"]);
    let fx = merged(Some(SPEC_NONE), &t);
    set_feature(&fx, "fix #76 and #77", None);
    tickets_step(&fx.driver, &fx.id, false).expect("step");
    let s = t.state.lock().expect("lock");
    assert_eq!(
        s.comments.iter().map(|c| c.0).collect::<Vec<_>>(),
        vec![76, 77]
    );
    assert_eq!(s.closes, vec![76, 77]);
}

#[test]
fn a_failing_tracker_leaves_the_merged_job_merged() {
    let t = tracker();
    let fx = merged(Some(SPEC_WITH_TWO), &t);
    t.fail_on("repo_name");
    let report = tickets_step(&fx.driver, &fx.id, false).expect("step");
    assert!(report.failed);
    assert_eq!(
        fx.driver.job(&fx.id).expect("job").stage,
        factory_core::Stage::Merged
    );
}

use factory_core::Config;

/// Like `merged` but the merge itself runs the hook, so the fixture stops at the approved stage.
fn approved_with_hook(on: bool, spec: Option<&str>, tracker: &FakeTracker) -> MergeFixture {
    let config = Config {
        tickets_on_merge: on,
        ..Config::default()
    };
    let fx = with_tracker(
        common::approved_job_config(common::approving_runner(3), false, config),
        tracker.clone(),
    );
    set_feature(&fx, "dedup cleanups #76", spec);
    fx
}

#[test]
fn with_the_key_off_no_merge_calls_the_tracker() {
    let t = tracker();
    let fx = approved_with_hook(false, Some(SPEC_WITH_TWO), &t);
    assert!(matches!(
        merge_stage(&fx.driver, &fx.id).expect("merge"),
        MergeResult::Merged(_)
    ));
    assert!(t.calls().is_empty(), "{:?}", t.calls());
}

#[test]
fn with_the_key_on_the_merge_files_tickets_and_closes_the_parent() {
    let t = tracker();
    let fx = approved_with_hook(true, Some(SPEC_WITH_TWO), &t);
    assert!(matches!(
        merge_stage(&fx.driver, &fx.id).expect("merge"),
        MergeResult::Merged(_)
    ));
    let s = t.state.lock().expect("lock");
    assert_eq!(s.created.len(), 2);
    assert_eq!(s.closes, vec![76]);
}

#[test]
fn a_failing_tracker_never_changes_the_merge_result() {
    let t = tracker();
    t.fail_on("repo_name");
    let fx = approved_with_hook(true, Some(SPEC_WITH_TWO), &t);
    assert!(matches!(
        merge_stage(&fx.driver, &fx.id).expect("merge"),
        MergeResult::Merged(_)
    ));
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.stage, factory_core::Stage::Merged);
    let ledger =
        std::fs::read_to_string(fx.driver.job_dir(&job).join("progress.md")).expect("ledger");
    assert!(ledger.contains("tickets failed"), "{ledger}");
}

fn events_text(fx: &MergeFixture) -> String {
    std::fs::read_to_string(fx.driver.events_path()).unwrap_or_default()
}

#[test]
fn a_failing_tracker_after_a_merge_is_reported_in_the_events() {
    let t = tracker();
    t.fail_on("repo_name");
    let fx = approved_with_hook(true, Some(SPEC_WITH_TWO), &t);
    assert!(matches!(
        merge_stage(&fx.driver, &fx.id).expect("merge"),
        MergeResult::Merged(_)
    ));
    let events = events_text(&fx);
    assert!(
        events.contains("FactoryNotice") && events.contains("tickets step: failed"),
        "{events}"
    );
}

#[test]
fn a_broken_marker_after_a_merge_is_reported_and_the_job_stays_merged() {
    let t = tracker();
    let fx = approved_with_hook(true, Some(SPEC_WITH_TWO), &t);
    let job = fx.driver.job(&fx.id).expect("job");
    std::fs::write(fx.driver.job_dir(&job).join("tickets.json"), "{ not json").expect("marker");
    assert!(matches!(
        merge_stage(&fx.driver, &fx.id).expect("merge"),
        MergeResult::Merged(_)
    ));
    assert_eq!(
        fx.driver.job(&fx.id).expect("job").stage,
        factory_core::Stage::Merged
    );
    let events = events_text(&fx);
    assert!(
        events.contains("tickets step:") && events.contains("not valid JSON"),
        "{events}"
    );
}

#[test]
fn no_gh_binary_after_a_merge_leaves_the_job_merged_and_reports_it() {
    let config = Config {
        tickets_on_merge: true,
        ..Config::default()
    };
    let fx = common::approved_job_config(common::approving_runner(3), false, config);
    let missing =
        factory_engine::GhTracker::with_program("definitely-not-a-real-program-xyz", &fx.main);
    let fx = MergeFixture {
        driver: fx.driver.with_tracker(Box::new(missing)),
        ..fx
    };
    set_feature(&fx, "dedup cleanups #76", Some(SPEC_WITH_TWO));
    assert!(matches!(
        merge_stage(&fx.driver, &fx.id).expect("merge"),
        MergeResult::Merged(_)
    ));
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.stage, factory_core::Stage::Merged);
    let ledger =
        std::fs::read_to_string(fx.driver.job_dir(&job).join("progress.md")).expect("ledger");
    assert!(
        ledger.contains("tickets failed") && ledger.contains("could not start"),
        "{ledger}"
    );
    assert!(events_text(&fx).contains("could not start"));
}

#[test]
fn a_rerun_after_the_merge_hook_creates_no_second_ticket() {
    let t = tracker();
    let fx = approved_with_hook(true, Some(SPEC_WITH_TWO), &t);
    assert!(matches!(
        merge_stage(&fx.driver, &fx.id).expect("merge"),
        MergeResult::Merged(_)
    ));
    assert_eq!(t.state.lock().expect("lock").created.len(), 2);
    tickets_step(&fx.driver, &fx.id, false).expect("rerun");
    let s = t.state.lock().expect("lock");
    assert_eq!(s.created.len(), 2);
    assert_eq!(s.comments.len(), 1);
    assert_eq!(s.closes, vec![76]);
}
