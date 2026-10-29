mod common;

use common::{committing_fixture, scripted, scripted_auto, Effect, Fixture};
use factory_core::{Board, CheckRule, Stage};
use factory_engine::{create_job, run_series, StageOutcome};

fn failing_checks(fx: &mut Fixture) {
    fx.config.checks = vec![CheckRule {
        paths: vec!["**".into()],
        commands: vec!["false".into()],
    }];
}

fn write_effect() -> Effect {
    Box::new(|spec| {
        std::fs::write(spec.cwd.join("a.txt"), "x\n").expect("write");
        "done".to_string()
    })
}

/// Builds once, then fails the checks, so a fix round starts.
fn build_and_fail(fx: &Fixture) -> StageOutcome {
    let runner = scripted_auto(vec![write_effect()]);
    fx.engine(&runner).build_stage().expect("build");
    fx.engine(&runner).checks_stage().expect("checks")
}

fn set_job_model(fx: &Fixture, model: &str) {
    let mut board = Board::load(&fx.board_path).expect("load");
    board.find_mut("001").expect("job").model = Some(model.to_string());
    board.save(&fx.board_path).expect("save");
}

#[test]
fn the_second_fix_round_is_escalated_and_logged() {
    let mut fx = committing_fixture();
    failing_checks(&mut fx);
    assert_eq!(build_and_fail(&fx), StageOutcome::Advanced(Stage::Build));
    assert_eq!(fx.job().round, 2);
    assert_eq!(fx.job().escalated_model, None);
    assert_eq!(build_and_fail(&fx), StageOutcome::Advanced(Stage::Build));
    assert_eq!(fx.job().round, 3);
    assert_eq!(fx.job().escalated_model.as_deref(), Some("claude-opus-5-5"));
    assert!(
        fx.ledger()
            .contains("escalated the builder to claude-opus-5-5 (fix round 3, checks failed)"),
        "{}",
        fx.ledger()
    );
}

#[test]
fn a_job_with_its_own_model_is_not_escalated() {
    let mut fx = committing_fixture();
    failing_checks(&mut fx);
    set_job_model(&fx, "claude-sonnet-5-5");
    build_and_fail(&fx);
    build_and_fail(&fx);
    assert_eq!(fx.job().round, 3);
    assert_eq!(fx.job().escalated_model, None);
    assert!(!fx.ledger().contains("escalated"), "{}", fx.ledger());
}

#[test]
fn review_findings_escalate_too() {
    let fx = committing_fixture();
    let runner = scripted_auto(vec![write_effect()]);
    for round in 2..=3 {
        fx.engine(&runner).build_stage().expect("build");
        assert_eq!(
            fx.engine(&runner).checks_stage().expect("checks"),
            StageOutcome::Advanced(Stage::Review)
        );
        let reviewer = scripted(vec![Box::new(|_| "fine\nVERDICT: CHANGES".to_string())]);
        let e = fx.engine(&reviewer);
        let out = e.review_stage().expect("review");
        e.apply_decision(&out).expect("apply");
        assert_eq!(fx.job().round, round);
        let expected = (round == 3).then(|| "claude-opus-5-5".to_string());
        assert_eq!(fx.job().escalated_model, expected);
    }
}

#[test]
fn escalation_does_not_carry_into_the_next_series_job() {
    let fx = common::SeriesFixture::new("fine\nVERDICT: APPROVE");
    let a = fx.plan_file("plan-01-a.md", 1, "plan-02-b.md");
    fx.plan_file("plan-02-b.md", 1, "none");
    let job = create_job(&fx.driver, "series", "two plans", Some(&a), None).expect("create");
    fx.driver
        .store()
        .update(|b| {
            b.find_mut(&job.id)?.escalated_model = Some("claude-opus-5-5".to_string());
            Ok(())
        })
        .expect("escalate");
    run_series(&fx.driver, &job.id).expect("series");
    let board = fx.driver.store().load().expect("board");
    assert_eq!(board.jobs.len(), 2);
    assert_eq!(board.jobs[1].escalated_model, None);
}
