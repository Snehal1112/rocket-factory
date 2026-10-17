mod common;

use factory_engine::{create_job_with_spec, start_draft};

const WARNING: &str = "no decision in this spec was answered by the user";

fn spec_with(decisions: &str) -> String {
    format!(
        "# Spec: Demo\n\n## Goal\n\nBuild the demo so that a user can finish the task in one step.\n\n\
         ## Acceptance criteria\n\n- It is done.\n\n{decisions}"
    )
}

const USER_DECISION: &str = "## Decisions\n\n- Use a queue. Decided by: user\n";
const CALL_DECISION: &str = "## Decisions\n\n- Use a queue. Decided by: call\n";

/// Writes the spec beside one plan and returns the plan and spec paths.
fn setup(fx: &common::SeriesFixture, spec: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let plan = fx.plan_file("plan-01-a.md", 1, "none");
    let spec_path = plan.parent().expect("plans folder").join("design.md");
    std::fs::write(&spec_path, spec).expect("spec");
    (plan, spec_path)
}

#[test]
fn new_adopts_the_draft_and_leaves_no_duplicate() {
    let fx = common::SeriesFixture::new("x");
    start_draft(&fx.driver, "demo", "f", "2026-10-18", 1.0).expect("draft");
    let (plan, spec) = setup(&fx, &spec_with(USER_DECISION));
    create_job_with_spec(&fx.driver, "Demo", "f", Some(&plan), Some(&spec), None).expect("create");
    let board = fx.driver.store().load().expect("board");
    assert!(board.drafts.is_empty());
    assert_eq!(board.jobs.len(), 1);
}

#[test]
fn a_failed_spec_check_keeps_the_draft() {
    let fx = common::SeriesFixture::new("x");
    start_draft(&fx.driver, "demo", "f", "2026-10-18", 1.0).expect("draft");
    let (plan, spec) = setup(&fx, "# Spec\n\n## Goal\n\nDo it.\n");
    assert!(create_job_with_spec(&fx.driver, "demo", "f", Some(&plan), Some(&spec), None).is_err());
    let board = fx.driver.store().load().expect("board");
    assert_eq!(board.drafts.len(), 1);
    assert!(board.jobs.is_empty());
}

#[test]
fn new_without_a_draft_is_unchanged() {
    let fx = common::SeriesFixture::new("x");
    start_draft(&fx.driver, "other", "f", "2026-10-18", 1.0).expect("draft");
    let (plan, spec) = setup(&fx, &spec_with(USER_DECISION));
    let job = create_job_with_spec(&fx.driver, "demo", "f", Some(&plan), Some(&spec), None).expect("create");
    let board = fx.driver.store().load().expect("board");
    assert_eq!(board.drafts.len(), 1, "a draft with another slug stays");
    assert_eq!(job.spec_warning, None);
}

#[test]
fn the_draft_is_removed_when_there_is_no_plan() {
    let fx = common::SeriesFixture::new("x");
    start_draft(&fx.driver, "demo", "f", "2026-10-18", 1.0).expect("draft");
    let job = create_job_with_spec(&fx.driver, "demo", "f", None, None, None).expect("create");
    assert!(fx.driver.store().load().expect("board").drafts.is_empty());
    assert_eq!(job.spec_warning, None);
}

#[test]
fn a_spec_with_no_user_decision_sets_the_warning_and_still_creates_the_job() {
    for decisions in ["", CALL_DECISION] {
        let fx = common::SeriesFixture::new("x");
        let (plan, spec) = setup(&fx, &spec_with(decisions));
        let job = create_job_with_spec(&fx.driver, "demo", "f", Some(&plan), Some(&spec), None).expect("create");
        assert_eq!(job.spec_warning.as_deref(), Some(WARNING));
        let stored = fx.driver.job(&job.id).expect("job");
        assert_eq!(stored.spec_warning.as_deref(), Some(WARNING));
    }
}

#[test]
fn a_spec_with_a_user_decision_has_no_warning() {
    let fx = common::SeriesFixture::new("x");
    let (plan, spec) = setup(&fx, &spec_with(USER_DECISION));
    let job = create_job_with_spec(&fx.driver, "demo", "f", Some(&plan), Some(&spec), None).expect("create");
    assert_eq!(job.spec_warning, None);
    assert_eq!(fx.driver.job(&job.id).expect("job").spec_warning, None);
}
