mod common;

use std::path::PathBuf;

use factory_core::{Config, Stage};
use factory_engine::{create_job_with_spec, create_one_job, one_job_estimate, EngineError};

const HEADER: &str = "# Plan\n\n> For agents: use superpowers:subagent-driven-development.\n\n**Goal:** g\n\n**Architecture:** a\n\n**Tech Stack:** t\n\n**Spec:** s\n\n**Global Constraints:** c\n\n**Review Focus:** r\n\n";
const TASK_BODY: &str = "**Files:**\n- Modify: `a.rs`\n\n**Interfaces:**\n- Produces: x\n\n";
const SPEC: &str = "# Spec: Demo\n\n## Goal\n\nBuild the demo so that a user can finish the task in one step.\n\n\
                    ## Acceptance criteria\n\n- It is done.\n\n## Decisions\n\n- Use a queue. Decided by: user\n";

/// One lint clean plan text with `n` tasks.
fn plan_with(n: usize, next: &str) -> String {
    let mut text = String::from(HEADER);
    for t in 1..=n {
        text.push_str(&format!("### Task {t}: t\n\n{TASK_BODY}"));
    }
    text.push_str(&format!("Next plan: {next}\n"));
    text
}

/// Writes four plans of three tasks and a valid spec into one folder. Returns the folder.
fn four_plans(dir: &std::path::Path) -> PathBuf {
    let plans = dir.join("plans");
    std::fs::create_dir_all(&plans).expect("plans dir");
    for n in 1..=4 {
        let next = if n == 4 {
            "none".to_string()
        } else {
            plans
                .join(format!("plan-0{}-x.md", n + 1))
                .to_string_lossy()
                .into_owned()
        };
        std::fs::write(plans.join(format!("plan-0{n}-x.md")), plan_with(3, &next)).expect("plan");
    }
    std::fs::write(plans.join("spec.md"), SPEC).expect("spec");
    plans
}

fn fixture(budget: u64) -> common::SeriesFixture {
    common::SeriesFixture::with_config(
        "x",
        Config {
            budget_weighted: budget,
            ..Config::default()
        },
    )
}

#[test]
fn estimate_is_overhead_plus_per_task() {
    assert_eq!(one_job_estimate(12), 1_310_000);
    assert_eq!(one_job_estimate(3), 410_000);
}

#[test]
fn over_budget_without_consent_creates_nothing() {
    let fx = fixture(1_000_000);
    let plans = four_plans(fx.tmp.path());
    let err = create_one_job(&fx.driver, "demo", "f", &plans, None, None, false)
        .expect_err("needs consent");
    assert!(
        matches!(
            err,
            EngineError::NeedsConsent {
                estimate: 1_310_000,
                budget: 1_000_000
            }
        ),
        "{err}"
    );
    assert!(fx.driver.store().load().expect("board").jobs.is_empty());
    let jobs_dir = &fx.driver.layout().jobs_dir;
    let entries = std::fs::read_dir(jobs_dir)
        .map(|rd| rd.count())
        .unwrap_or(0);
    assert_eq!(entries, 0);
}

#[test]
fn a_plan_with_no_task_is_refused_with_its_name() {
    let fx = fixture(1_000_000);
    let plans = four_plans(fx.tmp.path());
    std::fs::write(
        plans.join("plan-01-x.md"),
        format!(
            "# Plan\n\nNext plan: {}\n",
            plans.join("plan-02-x.md").display()
        ),
    )
    .expect("plan");
    let err =
        create_one_job(&fx.driver, "demo", "f", &plans, None, None, true).expect_err("refused");
    let text = err.to_string();
    assert!(
        text.contains("plan-01-x.md") && text.contains("at least one task"),
        "{text}"
    );
}

#[test]
fn over_budget_with_consent_creates_the_job_with_a_warning() {
    let fx = fixture(1_000_000);
    let plans = four_plans(fx.tmp.path());
    let made = create_one_job(&fx.driver, "demo", "f", &plans, None, None, true).expect("create");
    let warning = made.job.spec_warning.expect("warning");
    assert!(warning.contains("over the budget"), "{warning}");
    let board = fx.driver.store().load().expect("board");
    assert_eq!(board.jobs.len(), 1);
    let stored = board.jobs[0].spec_warning.clone().expect("stored warning");
    assert!(stored.contains("over the budget"), "{stored}");
}

#[test]
fn accepting_over_budget_under_budget_gives_no_warning() {
    let fx = fixture(1_500_000);
    let plans = four_plans(fx.tmp.path());
    let made = create_one_job(&fx.driver, "demo", "f", &plans, None, None, true).expect("create");
    assert_eq!(made.job.spec_warning, None);
    assert_eq!(
        fx.driver.store().load().expect("board").jobs[0].spec_warning,
        None
    );
}

#[test]
fn under_budget_creates_the_job_with_a_note() {
    let fx = fixture(1_500_000);
    let plans = four_plans(fx.tmp.path());
    let made = create_one_job(&fx.driver, "demo", "f", &plans, None, None, false).expect("create");
    assert_eq!(made.job.spec_warning, None);
    assert_eq!(made.notes.len(), 1);
    assert!(made.notes[0].contains("1310000"), "{:?}", made.notes);
}

#[test]
fn boundary_budget_creates_the_job_with_no_warning() {
    let fx = fixture(1_310_000);
    let plans = four_plans(fx.tmp.path());
    let made = create_one_job(&fx.driver, "demo", "f", &plans, None, None, false).expect("create");
    assert_eq!(made.job.spec_warning, None);
}

#[test]
fn four_plans_become_one_job_with_a_combined_plan() {
    let fx = fixture(1_500_000);
    let plans = four_plans(fx.tmp.path());
    let made = create_one_job(&fx.driver, "demo", "f", &plans, None, None, false).expect("create");
    let board = fx.driver.store().load().expect("board");
    assert_eq!(board.jobs.len(), 1);
    assert!(made.job.series.is_none());
    assert_eq!(made.job.stage, Stage::Build);
    let dir = fx.driver.job_dir(&made.job);
    let plan = std::fs::read_to_string(dir.join("plan.md")).expect("plan.md");
    let numbers: Vec<u32> = factory_engine::parse_tasks(&plan)
        .iter()
        .map(|t| t.n)
        .collect();
    assert_eq!(numbers, (1..=12).collect::<Vec<u32>>());
    assert!(!plan.lines().any(|l| l.starts_with("Next plan:")));
    assert!(
        !plan.contains(&fx.tmp.path().to_string_lossy().into_owned()),
        "absolute path leaked"
    );
    assert!(dir.join("spec.md").exists());
}

#[test]
fn one_plan_takes_the_normal_path() {
    let fx = fixture(1_000_000);
    let plans = fx.tmp.path().join("one");
    std::fs::create_dir_all(&plans).expect("dir");
    std::fs::write(plans.join("plan-01-x.md"), plan_with(2, "none")).expect("plan");
    std::fs::write(plans.join("spec.md"), SPEC).expect("spec");
    let made = create_one_job(&fx.driver, "demo", "f", &plans, None, None, false).expect("create");
    assert!(made.job.series.is_some());
    assert!(
        made.notes
            .iter()
            .any(|n| n.contains("--one-job had no effect: the chain has one plan")),
        "{:?}",
        made.notes
    );

    let big = fx.tmp.path().join("big");
    std::fs::create_dir_all(&big).expect("dir");
    std::fs::write(big.join("plan-01-x.md"), plan_with(4, "none")).expect("plan");
    std::fs::write(big.join("spec.md"), SPEC).expect("spec");
    let err =
        create_one_job(&fx.driver, "demo2", "f", &big, None, None, false).expect_err("task count");
    assert!(
        err.to_string()
            .contains("a plan needs 1 to 3 tasks, found 4"),
        "{err}"
    );
}

#[test]
fn without_the_flag_the_same_plans_still_make_four_jobs() {
    let fx = fixture(1_000_000);
    let plans = four_plans(fx.tmp.path());
    create_job_with_spec(&fx.driver, "demo", "f", Some(&plans), None, None).expect("series");
    assert_eq!(fx.driver.store().load().expect("board").jobs.len(), 4);

    let big = fx.tmp.path().join("big");
    std::fs::create_dir_all(&big).expect("dir");
    std::fs::write(big.join("plan-01-x.md"), plan_with(4, "none")).expect("plan");
    std::fs::write(big.join("spec.md"), SPEC).expect("spec");
    let err = create_job_with_spec(&fx.driver, "demo2", "f", Some(&big), None, None)
        .expect_err("task count");
    assert!(err.to_string().contains("found 4"), "{err}");
}

#[test]
fn a_placeholder_in_a_later_plan_refuses_the_call() {
    let fx = fixture(1_500_000);
    let plans = four_plans(fx.tmp.path());
    let second = std::fs::read_to_string(plans.join("plan-02-x.md")).expect("read");
    std::fs::write(
        plans.join("plan-02-x.md"),
        second.replace("### Task 1: t", "### Task 1: TBD"),
    )
    .expect("write");
    let err =
        create_one_job(&fx.driver, "demo", "f", &plans, None, None, true).expect_err("placeholder");
    let text = err.to_string();
    assert!(text.contains("TBD") || text.contains("plan-02"), "{text}");
    assert!(fx.driver.store().load().expect("board").jobs.is_empty());
}

/// One lint clean plan text with `n` checkbox tasks.
fn checkbox_plan(n: usize, next: &str) -> String {
    let mut text = String::from(HEADER);
    for t in 1..=n {
        text.push_str(&format!("- [ ] Task {t}: t\n\n{TASK_BODY}"));
    }
    text.push_str(&format!("Next plan: {next}\n"));
    text
}

#[test]
fn checkbox_only_plans_become_one_job() {
    let fx = fixture(1_500_000);
    let plans = fx.tmp.path().join("cb");
    std::fs::create_dir_all(&plans).expect("dir");
    let second = plans.join("plan-02-x.md");
    std::fs::write(
        plans.join("plan-01-x.md"),
        checkbox_plan(3, &second.to_string_lossy()),
    )
    .expect("plan");
    std::fs::write(&second, checkbox_plan(3, "none")).expect("plan");
    std::fs::write(plans.join("spec.md"), SPEC).expect("spec");
    let made = create_one_job(&fx.driver, "demo", "f", &plans, None, None, false).expect("create");
    assert!(made.job.series.is_none());
    let plan =
        std::fs::read_to_string(fx.driver.job_dir(&made.job).join("plan.md")).expect("plan.md");
    let numbers: Vec<u32> = factory_engine::parse_checkbox_tasks(&plan)
        .iter()
        .map(|t| t.n)
        .collect();
    assert_eq!(numbers, (1..=6).collect::<Vec<u32>>());
}

#[test]
fn plans_inside_the_repo_use_relative_paths() {
    let fx = fixture(1_500_000);
    let plans = four_plans(&fx.main);
    let made = create_one_job(&fx.driver, "demo", "f", &plans, None, None, false).expect("create");
    let plan =
        std::fs::read_to_string(fx.driver.job_dir(&made.job).join("plan.md")).expect("plan.md");
    let line = plan
        .lines()
        .find(|l| l.contains("Combined from:"))
        .expect("combined line");
    assert!(line.contains("plans/plan-01-x.md"), "{line}");
    assert!(
        !line.contains(&fx.tmp.path().to_string_lossy().into_owned()),
        "{line}"
    );
}
