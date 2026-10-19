use factory_engine::{parse_series, parse_series_with, SeriesError};
use std::path::Path;

fn plan(dir: &Path, name: &str, tasks: u32, next: &str) {
    let mut s = String::from("# Plan\n\n");
    for n in 1..=tasks {
        s.push_str(&format!("### Task {n}: t\n\n"));
    }
    s.push_str(&format!("Next plan: {next}\n"));
    std::fs::write(dir.join(name), s).expect("write plan");
}

#[test]
fn a_valid_chain_parses_in_order() {
    let d = tempfile::tempdir().expect("tmp");
    plan(d.path(), "plan-01-a.md", 2, "plan-02-b.md");
    plan(d.path(), "plan-02-b.md", 3, "none");
    let s = parse_series(d.path(), d.path()).expect("series");
    assert_eq!(s.plans.len(), 2);
    assert_eq!(s.plans[0].tasks, vec![1, 2]);
    assert!(s.plans[1].next.is_none());
}

#[test]
fn a_cycle_is_rejected() {
    let d = tempfile::tempdir().expect("tmp");
    plan(d.path(), "plan-01-a.md", 1, "plan-02-b.md");
    plan(d.path(), "plan-02-b.md", 1, "plan-01-a.md");
    let e = parse_series(d.path(), d.path()).expect_err("cycle");
    assert!(matches!(e, SeriesError::Cycle(_)), "{e}");
}

#[test]
fn bad_plans_are_rejected_with_clear_errors() {
    let d = tempfile::tempdir().expect("tmp");
    plan(d.path(), "plan-01-a.md", 4, "none");
    assert!(matches!(parse_series(d.path(), d.path()), Err(SeriesError::TaskCount { .. })));
    plan(d.path(), "plan-01-a.md", 0, "none");
    assert!(matches!(parse_series(d.path(), d.path()), Err(SeriesError::TaskCount { .. })));
    plan(d.path(), "plan-01-a.md", 1, "plan-09-missing.md");
    assert!(matches!(parse_series(d.path(), d.path()), Err(SeriesError::Missing(_))));
    std::fs::write(d.path().join("plan-01-a.md"), "### Task 1: t\n\nno pointer here\n").expect("w");
    assert!(matches!(parse_series(d.path(), d.path()), Err(SeriesError::NoNextLine(_))));
}

#[test]
fn an_orphan_plan_file_and_a_seventh_plan_are_rejected() {
    let d = tempfile::tempdir().expect("tmp");
    plan(d.path(), "plan-01-a.md", 1, "none");
    plan(d.path(), "plan-02-orphan.md", 1, "none");
    assert!(matches!(parse_series(d.path(), d.path()), Err(SeriesError::Orphan(_))));
    let e = tempfile::tempdir().expect("tmp");
    for i in 1..=7 {
        let next = if i == 7 { "none".to_string() } else { format!("plan-0{}-x.md", i + 1) };
        plan(e.path(), &format!("plan-0{i}-x.md"), 1, &next);
    }
    assert!(matches!(parse_series(e.path(), e.path()), Err(SeriesError::TooMany(_))));
}

#[test]
fn a_fenced_task_heading_does_not_count_toward_the_task_limit() {
    let d = tempfile::tempdir().expect("tmp");
    let text = "# Plan\n\n### Task 1: a\n\n### Task 2: b\n\n### Task 3: c\n\n```md\n### Task 4: quoted\n```\n\nNext plan: none\n";
    std::fs::write(d.path().join("plan-01-a.md"), text).expect("write plan");
    let s = parse_series(d.path(), d.path()).expect("series");
    assert_eq!(s.plans[0].tasks, vec![1, 2, 3]);
}

#[test]
fn a_checkbox_only_plan_is_still_accepted() {
    let d = tempfile::tempdir().expect("tmp");
    let text = "# Plan\n\n- [ ] Task 1: a\n- [x] Task 2: b\n\nNext plan: none\n";
    std::fs::write(d.path().join("plan-01-a.md"), text).expect("write plan");
    let s = parse_series(d.path(), d.path()).expect("series");
    assert_eq!(s.plans[0].tasks, vec![1, 2]);
}

#[test]
fn parse_series_with_none_accepts_five_tasks() {
    let d = tempfile::tempdir().expect("tmp");
    plan(d.path(), "plan-01-a.md", 5, "none");
    let s = parse_series_with(d.path(), d.path(), None).expect("series");
    assert_eq!(s.plans[0].tasks, vec![1, 2, 3, 4, 5]);
}

#[test]
fn parse_series_with_none_still_refuses_zero_tasks() {
    let d = tempfile::tempdir().expect("tmp");
    plan(d.path(), "plan-01-a.md", 0, "none");
    let e = parse_series_with(d.path(), d.path(), None).expect_err("zero tasks");
    assert!(matches!(e, SeriesError::TaskCount { .. }), "{e}");
}

#[test]
fn parse_series_still_refuses_four_tasks() {
    let d = tempfile::tempdir().expect("tmp");
    plan(d.path(), "plan-01-a.md", 4, "none");
    let e = parse_series(d.path(), d.path()).expect_err("four tasks");
    assert!(matches!(e, SeriesError::TaskCount { .. }), "{e}");
}
