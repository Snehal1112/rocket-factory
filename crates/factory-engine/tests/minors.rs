mod common;
use common::{
    approved_job, approved_job_config, approving_runner, with_tracker, FakeTracker, MergeFixture,
};
use factory_core::{Config, LastReview, Stage};
use factory_engine::{merge_stage, minor_tickets_step, MergeResult, MINORS_FILE};

const REVIEW: &str =
    "- [minor] first nit\n- [minor] [fixed] done nit\n- [minor] second nit\nVERDICT: APPROVE\n";

/// Records a last review with the given text in the job folder, before the merge.
fn with_review(fx: &MergeFixture, text: &str) {
    let job = fx.driver.job(&fx.id).expect("job");
    std::fs::write(fx.driver.job_dir(&job).join("review-code-r1.md"), text).expect("review");
    fx.driver
        .store()
        .update_job(&fx.id, |j| {
            j.last_review = Some(LastReview {
                round: 1,
                verdict: "approve".into(),
                blockers: 0,
                file: "review-code-r1.md".into(),
                source: "build".into(),
            });
            Ok(())
        })
        .expect("last review");
}

fn merge(fx: &MergeFixture) {
    assert!(matches!(
        merge_stage(&fx.driver, &fx.id).expect("merge"),
        MergeResult::Merged(_)
    ));
}

fn minors_file(fx: &MergeFixture) -> std::path::PathBuf {
    fx.driver
        .job_dir(&fx.driver.job(&fx.id).expect("job"))
        .join(MINORS_FILE)
}

fn merged(text: &str, tracker: &FakeTracker) -> MergeFixture {
    let fx = with_tracker(approved_job(), tracker.clone());
    with_review(&fx, text);
    merge(&fx);
    fx
}

#[test]
fn a_merged_job_files_one_minor_ticket() {
    let t = FakeTracker::new("o/r");
    let fx = merged(REVIEW, &t);
    assert_eq!(t.count("create Minor review findings from job"), 1);
    let s = t.state.lock().expect("lock");
    let (title, body, labels) = &s.created[0];
    assert!(
        title.contains(&fx.id) && title.ends_with("hello"),
        "{title}"
    );
    assert_eq!(body.lines().filter(|l| l.starts_with("- [ ]")).count(), 2);
    assert!(body.contains("review-code-r1.md") && body.contains(&fx.id));
    assert!(labels.is_empty());
    drop(s);
    let stored = std::fs::read_to_string(minors_file(&fx)).expect("minors.json");
    assert!(stored.contains("201"), "{stored}");
}

#[test]
fn minor_tickets_are_idempotent() {
    let t = FakeTracker::new("o/r");
    let fx = merged(REVIEW, &t);
    let report = minor_tickets_step(&fx.driver, &fx.id, false).expect("step");
    assert!(report.skipped && !report.failed, "{report:?}");
    assert_eq!(t.count("create"), 1);
}

#[test]
fn no_open_minors_no_ticket() {
    let t = FakeTracker::new("o/r");
    let fx = merged("- [minor] [fixed] done\nVERDICT: APPROVE\n", &t);
    assert_eq!(t.count("create"), 0);
    assert!(!minors_file(&fx).exists());
}

#[test]
fn minor_tickets_off_files_nothing() {
    let t = FakeTracker::new("o/r");
    let config = Config {
        minor_tickets: false,
        ..Config::default()
    };
    let fx = with_tracker(
        approved_job_config(approving_runner(3), false, config),
        t.clone(),
    );
    with_review(&fx, REVIEW);
    merge(&fx);
    let report = minor_tickets_step(&fx.driver, &fx.id, false).expect("step");
    assert!(report.skipped, "{report:?}");
    assert_eq!(t.count("create"), 0);
}

#[test]
fn a_series_job_files_its_minor_ticket() {
    let t = FakeTracker::new("o/r");
    let fx = with_tracker(approved_job(), t.clone());
    with_review(&fx, REVIEW);
    fx.driver
        .store()
        .update_job(&fx.id, |j| {
            j.series = Some(factory_core::SeriesPos {
                plan_file: "p".into(),
                index: 1,
                total: 3,
                id: None,
            });
            Ok(())
        })
        .expect("series");
    merge(&fx);
    assert_eq!(t.count("create Minor review findings from job"), 1);
}

#[test]
fn a_tracker_failure_keeps_the_merge() {
    let t = FakeTracker::new("o/r");
    t.fail_on("create");
    let fx = merged(REVIEW, &t);
    assert_eq!(fx.driver.job(&fx.id).expect("job").stage, Stage::Merged);
    assert!(!minors_file(&fx).exists());
    t.clear_failures();
    let report = minor_tickets_step(&fx.driver, &fx.id, false).expect("step");
    assert!(!report.failed, "{report:?}");
    assert!(minors_file(&fx).exists());
}
