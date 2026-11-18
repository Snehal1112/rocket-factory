mod support;
use factory_cli::CliError;
use serde_json::Value;
use support::{build, review, Scenario, TestResult, APPROVE};

/// A scenario with job 001 parked in needs-human.
fn parked() -> Result<Scenario, Box<dyn std::error::Error>> {
    let sc = Scenario::new(
        "",
        vec![
            build(vec![("secrets/token.txt", "x\n")], 1_000),
            review(APPROVE),
        ],
    )?;
    sc.start(&sc.plan_b, false)?;
    assert_eq!(sc.stage("001")?, "needs-human");
    Ok(sc)
}

fn events<'a>(lines: &'a [Value], name: &str) -> Vec<&'a Value> {
    lines.iter().filter(|l| l["event"] == name).collect()
}

#[test]
fn approve_logs_operator_approve() -> TestResult {
    let sc = parked()?;
    let (out, lines) = factory_telemetry::capture("info", || sc.run(&["approve", "1"]));
    out?;
    let found = events(&lines, "operator.approve");
    assert_eq!(found.len(), 1, "{lines:?}");
    assert_eq!(found[0]["without_review"], false);
    assert_eq!(found[0]["job_id"], "001");
    Ok(())
}

#[test]
fn rework_logs_the_note() -> TestResult {
    let sc = parked()?;
    let note = "say \"hi\" \\ again";
    let (out, lines) = factory_telemetry::capture("info", || sc.run(&["rework", "1", note]));
    out?;
    let found = events(&lines, "operator.rework");
    assert_eq!(found.len(), 1, "{lines:?}");
    assert_eq!(found[0]["note"], note);
    Ok(())
}

#[test]
fn retry_logs_operator_retry() -> TestResult {
    let sc = parked()?;
    // Dirty main, so the approve parks with a merge resume value.
    std::fs::write(sc.root.join("README.md"), "dirty")?;
    sc.run(&["approve", "1"])?;
    assert_eq!(sc.stage("001")?, "needs-human");
    std::fs::write(sc.root.join("README.md"), "demo\n")?;
    let (out, lines) = factory_telemetry::capture("info", || sc.run(&["retry", "1"]));
    out?;
    let found = events(&lines, "operator.retry");
    assert_eq!(found.len(), 1, "{lines:?}");
    assert_eq!(found[0]["resume"], "merge");
    assert_eq!(found[0]["job_id"], "001");
    Ok(())
}

#[test]
fn cancel_logs_reason_and_force() -> TestResult {
    let sc = parked()?;
    let (out, lines) = factory_telemetry::capture("info", || {
        sc.run(&["cancel", "1", "--reason", "not needed", "--force"])
    });
    out?;
    let found = events(&lines, "operator.cancel");
    assert_eq!(found.len(), 1, "{lines:?}");
    assert_eq!(found[0]["reason"], "not needed");
    assert_eq!(found[0]["force"], true);

    let sc = parked()?;
    let (out, lines) = factory_telemetry::capture("info", || sc.run(&["cancel", "1"]));
    out?;
    let found = events(&lines, "operator.cancel");
    assert_eq!(found.len(), 1, "{lines:?}");
    assert_eq!(found[0]["reason"], "cancelled by user");
    assert_eq!(found[0]["force"], false);
    Ok(())
}

#[test]
fn a_second_cancel_updates_the_reason() -> TestResult {
    let sc = parked()?;
    sc.run(&["cancel", "1", "--reason", "first"])?;
    sc.run(&["cancel", "1", "--reason", "second"])?;
    let job = sc.job("001")?;
    assert!(job.cancelled);
    assert_eq!(job.reason.as_deref(), Some("cancelled: second"));
    Ok(())
}

#[test]
fn rework_next_round_clears_cancelled_and_resumes_at_build() -> TestResult {
    let sc = parked()?;
    sc.run(&["cancel", "1"])?;
    sc.run(&["rework", "1", "fix it"])?;
    assert_eq!(sc.stage("001")?, "build");
    assert!(!sc.job("001")?.cancelled);
    Ok(())
}

#[test]
fn approve_still_refuses_a_cancelled_job() -> TestResult {
    let sc = parked()?;
    sc.run(&["cancel", "1"])?;
    let err = sc.run(&["approve", "1"]).expect_err("approve must refuse");
    assert!(err.to_string().contains("was cancelled"), "{err}");
    assert_eq!(sc.stage("001")?, "needs-human");
    Ok(())
}

#[test]
fn approve_resumes_a_driver_parked_job_through_resume() -> TestResult {
    let sc = parked()?;
    sc.run(&["approve", "1"])?;
    assert_eq!(sc.stage("001")?, "merged");
    Ok(())
}

#[test]
fn refused_approve_logs_nothing() -> TestResult {
    let sc = Scenario::new("", vec![])?;
    let plan = sc.plan_b.to_string_lossy().to_string();
    sc.run(&["new", "hello", "say hello", "--plans", &plan])?;
    let (out, lines) = factory_telemetry::capture("info", || sc.run(&["approve", "1"]));
    assert!(matches!(out, Err(CliError::Refused(_))), "{out:?}");
    assert!(events(&lines, "operator.approve").is_empty(), "{lines:?}");
    Ok(())
}

#[test]
fn approve_with_waive_logs_the_reason() -> TestResult {
    let mut steps = vec![build(vec![("secrets/token.txt", "x\n")], 1_000)];
    for _ in 0..6 {
        steps.push(review(support::CHANGES));
        steps.push(build(vec![("secrets/token.txt", "y\n")], 400));
    }
    let sc = Scenario::new("", steps)?;
    sc.start(&sc.plan_b, false)?;
    let (out, lines) = factory_telemetry::capture("info", || {
        sc.run(&["approve", "1", "--waive", "accepted for now"])
    });
    out?;
    let found = events(&lines, "operator.approve");
    assert_eq!(found.len(), 1, "{lines:?}");
    assert_eq!(found[0]["waived"], true);
    assert_eq!(found[0]["waive_reason"], "accepted for now");
    Ok(())
}
