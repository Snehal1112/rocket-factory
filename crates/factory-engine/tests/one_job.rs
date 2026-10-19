mod common;

use std::path::PathBuf;

use factory_core::Config;
use factory_engine::{create_one_job, one_job_estimate, EngineError};

/// Writes four plans of three tasks and a valid spec into one folder. Returns the folder.
fn four_plans(dir: &std::path::Path) -> PathBuf {
    let plans = dir.join("plans");
    std::fs::create_dir_all(&plans).expect("plans dir");
    for n in 1..=4 {
        let mut text = String::from("# Plan\n\n");
        for t in 1..=3 {
            text.push_str(&format!("### Task {t}: t\n\n"));
        }
        let next = if n == 4 { "none".to_string() } else { plans.join(format!("plan-0{}-x.md", n + 1)).to_string_lossy().into_owned() };
        text.push_str(&format!("Next plan: {next}\n"));
        std::fs::write(plans.join(format!("plan-0{n}-x.md")), text).expect("plan");
    }
    let spec = "# Spec: Demo\n\n## Goal\n\nBuild the demo so that a user can finish the task in one step.\n\n\
                ## Acceptance criteria\n\n- It is done.\n\n## Decisions\n\n- Use a queue. Decided by: user\n";
    std::fs::write(plans.join("spec.md"), spec).expect("spec");
    plans
}

fn fixture(budget: u64) -> common::SeriesFixture {
    common::SeriesFixture::with_config("x", Config { budget_weighted: budget, ..Config::default() })
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
    let err = create_one_job(&fx.driver, "demo", "f", &plans, None, None, false).expect_err("needs consent");
    assert!(matches!(err, EngineError::NeedsConsent { estimate: 1_310_000, budget: 1_000_000 }), "{err}");
    assert!(fx.driver.store().load().expect("board").jobs.is_empty());
    let jobs_dir = &fx.driver.layout().jobs_dir;
    let entries = std::fs::read_dir(jobs_dir).map(|rd| rd.count()).unwrap_or(0);
    assert_eq!(entries, 0);
}

#[test]
fn a_plan_with_no_task_is_refused_with_its_name() {
    let fx = fixture(1_000_000);
    let plans = four_plans(fx.tmp.path());
    std::fs::write(plans.join("plan-01-x.md"), format!("# Plan\n\nNext plan: {}\n", plans.join("plan-02-x.md").display()))
        .expect("plan");
    let err = create_one_job(&fx.driver, "demo", "f", &plans, None, None, true).expect_err("refused");
    let text = err.to_string();
    assert!(text.contains("plan-01-x.md") && text.contains("at least one task"), "{text}");
}

#[test]
#[ignore = "creation arrives in Task 2"]
fn over_budget_with_consent_creates_the_job_with_a_warning() {
    let fx = fixture(1_000_000);
    let plans = four_plans(fx.tmp.path());
    let made = create_one_job(&fx.driver, "demo", "f", &plans, None, None, true).expect("create");
    let warning = made.job.spec_warning.expect("warning");
    assert!(warning.contains("over the budget"), "{warning}");
}

#[test]
#[ignore = "creation arrives in Task 2"]
fn under_budget_creates_the_job_with_a_note() {
    let fx = fixture(1_500_000);
    let plans = four_plans(fx.tmp.path());
    let made = create_one_job(&fx.driver, "demo", "f", &plans, None, None, false).expect("create");
    assert_eq!(made.job.spec_warning, None);
    assert_eq!(made.notes.len(), 1);
    assert!(made.notes[0].contains("1310000"), "{:?}", made.notes);
}
