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

/// A plan in the repo's own format, with `extra` added after the header. `next` is the last line.
fn good_plan(extra: &str, next: &str) -> String {
    format!(
        "# Plan\n\n> Executor: `superpowers:subagent-driven-development`.\n\n**Goal:** g\n\n**Architecture:** a\n\n\
         **Tech Stack:** Rust\n\n**Spec:** s\n\n## Global Constraints\n\n- c\n\n## Review Focus\n\n- r\n\n{extra}\n\
         ### Task 1: t\n\n**Files:**\n- Modify: `a.rs`\n\n**Interfaces:**\n- Produces: x\n\nNext plan: {next}\n"
    )
}

/// Writes a good plan file into the fixture's plans folder.
fn write_plan(
    fx: &common::SeriesFixture,
    name: &str,
    extra: &str,
    next: &str,
) -> std::path::PathBuf {
    let path = fx.plan_file(name, 1, "none");
    let next = if next == "none" {
        next.to_string()
    } else {
        path.with_file_name(next).to_string_lossy().into_owned()
    };
    std::fs::write(&path, good_plan(extra, &next)).expect("plan");
    path
}

/// Writes the spec beside one plan and returns the plan and spec paths.
fn setup(fx: &common::SeriesFixture, spec: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let plan = write_plan(fx, "plan-01-a.md", "", "none");
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
    let job = create_job_with_spec(&fx.driver, "demo", "f", Some(&plan), Some(&spec), None)
        .expect("create");
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
        let job = create_job_with_spec(&fx.driver, "demo", "f", Some(&plan), Some(&spec), None)
            .expect("create");
        assert_eq!(job.spec_warning.as_deref(), Some(WARNING));
        let stored = fx.driver.job(&job.id).expect("job");
        assert_eq!(stored.spec_warning.as_deref(), Some(WARNING));
    }
}

#[test]
fn a_spec_with_a_user_decision_has_no_warning() {
    let fx = common::SeriesFixture::new("x");
    let (plan, spec) = setup(&fx, &spec_with(USER_DECISION));
    let job = create_job_with_spec(&fx.driver, "demo", "f", Some(&plan), Some(&spec), None)
        .expect("create");
    assert_eq!(job.spec_warning, None);
    assert_eq!(fx.driver.job(&job.id).expect("job").spec_warning, None);
}

#[test]
fn a_plan_placeholder_in_plan_two_refuses_before_any_job_exists() {
    let fx = common::SeriesFixture::new("x");
    start_draft(&fx.driver, "demo", "f", "2026-10-18", 1.0).expect("draft");
    let one = write_plan(&fx, "plan-01-a.md", "", "plan-02-b.md");
    write_plan(&fx, "plan-02-b.md", "Finish this TBD part.", "none");
    let spec = one.with_file_name("design.md");
    std::fs::write(&spec, spec_with(USER_DECISION)).expect("spec");
    let err = create_job_with_spec(&fx.driver, "demo", "f", Some(&one), Some(&spec), None)
        .expect_err("refused");
    assert!(err.to_string().contains("plan-02-b.md"), "{err}");
    let board = fx.driver.store().load().expect("board");
    assert!(board.jobs.is_empty());
    assert_eq!(board.drafts.len(), 1);
}

#[test]
fn plan_warnings_are_stored_and_the_job_is_created() {
    let fx = common::SeriesFixture::new("x");
    let plan = fx.plan_file("plan-01-a.md", 1, "none");
    let spec = plan.with_file_name("design.md");
    std::fs::write(&spec, spec_with(CALL_DECISION)).expect("spec");
    let job = create_job_with_spec(&fx.driver, "demo", "f", Some(&plan), Some(&spec), None)
        .expect("create");
    let warning = job.spec_warning.expect("warning");
    assert!(warning.starts_with(WARNING), "{warning}");
    assert!(
        warning.contains("; plan-01-a.md header has no Goal"),
        "{warning}"
    );
    let stored = fx.driver.job(&job.id).expect("job");
    assert_eq!(stored.spec_warning.as_deref(), Some(warning.as_str()));
}
