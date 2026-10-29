mod common;
use common::{approved_job, with_tracker, FakeTracker, MergeFixture};
use factory_engine::{
    merge_stage, read_waivers, record_waiver, waived_blockers, waiver_tickets_step, MergeResult,
    Waiver,
};

fn waiver(file: &str, blockers: &[&str]) -> Waiver {
    Waiver {
        head: "abc123".into(),
        round: 1,
        file: file.into(),
        reason: "the helper is fine for now".into(),
        at: 10,
        blockers: blockers.iter().map(|b| b.to_string()).collect(),
        tickets: blockers.iter().map(|_| None).collect(),
    }
}

fn job_dir(fx: &MergeFixture) -> std::path::PathBuf {
    fx.driver.job_dir(&fx.driver.job(&fx.id).expect("job"))
}

/// A job with the given waivers recorded, merged, with a fake tracker.
fn merged(waivers: Vec<Waiver>, tracker: &FakeTracker) -> MergeFixture {
    let fx = with_tracker(approved_job(), tracker.clone());
    for w in waivers {
        record_waiver(&job_dir(&fx), w).expect("record");
    }
    assert!(matches!(
        merge_stage(&fx.driver, &fx.id).expect("merge"),
        MergeResult::Merged(_)
    ));
    fx
}

fn two_blockers() -> Waiver {
    waiver(
        "review-code-r1.md",
        &["- [blocker] first problem", "- [blocker] second problem"],
    )
}

#[test]
fn a_waiver_files_one_ticket_per_blocker_after_merge() {
    let t = FakeTracker::new("o/r");
    let fx = merged(vec![two_blockers()], &t);
    assert_eq!(t.count("create Waived review finding from job"), 2);
    let stored = read_waivers(&job_dir(&fx));
    // The fake hands out 201 first.
    assert_eq!(stored[0].tickets, vec![Some(201), Some(202)]);
    let s = t.state.lock().expect("lock");
    assert!(s.created[0].0.ends_with("- [blocker] first problem"));
    assert!(s.created[0].1.contains("the helper is fine for now"));
    assert!(s.created[0].1.contains(&fx.id));
    assert!(s.created[0].2.is_empty());
}

#[test]
fn untagged_changes_waive_one_blocker() {
    let found = waived_blockers("The helper is wrong.\nVERDICT: CHANGES");
    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn a_second_waiver_is_appended() {
    let t = FakeTracker::new("o/r");
    let first = waiver("review-code-r1.md", &["- [blocker] one"]);
    let mut second = waiver("review-code-r2.md", &["- [blocker] two"]);
    second.head = "def456".into();
    let fx = merged(vec![first, second], &t);
    assert_eq!(read_waivers(&job_dir(&fx)).len(), 2);
    assert_eq!(t.count("create"), 2);
}

#[test]
fn a_retry_does_not_duplicate_the_waiver() {
    let t = FakeTracker::new("o/r");
    let fx = with_tracker(approved_job(), t.clone());
    record_waiver(&job_dir(&fx), two_blockers()).expect("first");
    record_waiver(&job_dir(&fx), two_blockers()).expect("retry");
    assert_eq!(read_waivers(&job_dir(&fx)).len(), 1);
}

#[test]
fn waiver_tickets_are_idempotent() {
    let t = FakeTracker::new("o/r");
    let fx = merged(vec![two_blockers()], &t);
    let before = t.count("create");
    let report = waiver_tickets_step(&fx.driver, &fx.id, false).expect("step");
    assert!(!report.failed, "{report:?}");
    assert_eq!(t.count("create"), before);
}

#[test]
fn a_tracker_failure_keeps_the_merge() {
    let t = FakeTracker::new("o/r");
    t.fail_on("create");
    let fx = merged(vec![two_blockers()], &t);
    assert_eq!(
        fx.driver.job(&fx.id).expect("job").stage,
        factory_core::Stage::Merged
    );
    assert_eq!(read_waivers(&job_dir(&fx))[0].tickets, vec![None, None]);
    let report = waiver_tickets_step(&fx.driver, &fx.id, false).expect("step");
    assert!(report.failed, "{report:?}");
}

#[test]
fn waiver_tickets_retry_through_factory_tickets() {
    let t = FakeTracker::new("o/r");
    t.fail_on("create");
    let fx = merged(vec![two_blockers()], &t);
    t.clear_failures();
    let report = waiver_tickets_step(&fx.driver, &fx.id, false).expect("step");
    assert!(!report.failed, "{report:?}");
    assert_eq!(
        read_waivers(&job_dir(&fx))[0].tickets,
        vec![Some(201), Some(202)]
    );
}
