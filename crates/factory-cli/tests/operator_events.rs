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
fn refused_approve_logs_nothing() -> TestResult {
    let sc = Scenario::new("", vec![])?;
    let plan = sc.plan_b.to_string_lossy().to_string();
    sc.run(&["new", "hello", "say hello", "--plans", &plan])?;
    let (out, lines) = factory_telemetry::capture("info", || sc.run(&["approve", "1"]));
    assert!(matches!(out, Err(CliError::Refused(_))), "{out:?}");
    assert!(events(&lines, "operator.approve").is_empty(), "{lines:?}");
    Ok(())
}
