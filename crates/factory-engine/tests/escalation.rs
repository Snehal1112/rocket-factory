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

/// A builder that appends one line, so every round has a change to commit.
fn append_effect() -> Effect {
    Box::new(|spec| {
        let path = spec.cwd.join("a.txt");
        let mut text = std::fs::read_to_string(&path).unwrap_or_default();
        text.push_str("x\n");
        std::fs::write(path, text).expect("write");
        "done".to_string()
    })
}

/// Builds with a fresh runner, then runs the checks, and returns the runner for its specs.
fn build_round(fx: &Fixture) -> common::ScriptedRunner {
    let runner = scripted_auto(vec![append_effect()]);
    fx.engine(&runner).build_stage().expect("build");
    fx.engine(&runner).checks_stage().expect("checks");
    runner
}

/// Fails the checks twice, so the job is in escalated fix round 3.
fn reach_round_three(fx: &mut Fixture) {
    failing_checks(fx);
    build_round(fx);
    build_round(fx);
    assert_eq!(fx.job().round, 3);
    assert_eq!(fx.job().escalated_model.as_deref(), Some("claude-opus-5-5"));
}

fn passing_checks(fx: &mut Fixture) {
    fx.config.checks = vec![CheckRule {
        paths: vec!["**".into()],
        commands: vec!["true".into()],
    }];
}

#[test]
fn the_escalated_round_runs_a_fresh_builder_on_the_stronger_model() {
    let mut fx = committing_fixture();
    reach_round_three(&mut fx);
    let runner = build_round(&fx);
    let spec = runner.last_spec_for("builder").expect("builder spec");
    assert_eq!(spec.model.as_deref(), Some("claude-opus-5-5"));
    assert_eq!(spec.resume, None);
    assert!(
        spec.prompt
            .contains("picking up a fix round in a fresh session"),
        "{}",
        spec.prompt
    );
    assert!(spec.prompt.contains("changes-r3.md"), "{}", spec.prompt);
    assert!(
        spec.env.contains(&(
            "CLAUDE_CODE_SUBAGENT_MODEL".to_string(),
            "claude-opus-5-5".to_string()
        )),
        "{:?}",
        spec.env
    );
}

#[test]
fn the_reviewer_and_committer_are_not_escalated() {
    let mut fx = committing_fixture();
    reach_round_three(&mut fx);
    passing_checks(&mut fx);
    let runner = build_round(&fx);
    let committer = runner.last_spec_for("committer").expect("committer spec");
    assert_eq!(
        committer.model.as_deref(),
        Some(fx.config.commit_model.as_str())
    );
    let reviewer = scripted(vec![Box::new(|_| "fine\nVERDICT: APPROVE".to_string())]);
    fx.engine(&reviewer).review_stage().expect("review");
    let spec = reviewer.last_spec_for("reviewer").expect("reviewer spec");
    assert_eq!(
        spec.model.as_deref(),
        Some(fx.config.reviewer_model.as_str())
    );
}

#[test]
fn a_rework_note_wins_in_an_escalated_round() {
    let mut fx = committing_fixture();
    reach_round_three(&mut fx);
    std::fs::write(fx.job_dir.join("rework-note.md"), "do it differently").expect("note");
    let runner = build_round(&fx);
    let prompt = runner
        .last_spec_for("builder")
        .expect("builder spec")
        .prompt;
    assert!(prompt.contains("rework-note.md"), "{prompt}");
    assert!(!prompt.contains("changes-r3.md"), "{prompt}");
}

#[test]
fn the_first_fix_round_still_resumes() {
    let mut fx = committing_fixture();
    failing_checks(&mut fx);
    build_round(&fx);
    assert_eq!(fx.job().round, 2);
    let session = fx.job().builder_id.expect("session");
    let runner = build_round(&fx);
    let spec = runner.last_spec_for("builder").expect("builder spec");
    assert_eq!(spec.resume, Some(session));
    assert_eq!(
        spec.model.as_deref(),
        Some(fx.config.builder_model.as_str())
    );
}

#[test]
fn a_later_escalated_round_resumes_the_strong_session() {
    let mut fx = committing_fixture();
    fx.config.fix_rounds = 3;
    reach_round_three(&mut fx);
    let third = build_round(&fx);
    assert_eq!(third.last_spec_for("builder").expect("spec").resume, None);
    assert_eq!(fx.job().round, 4);
    let fourth = build_round(&fx);
    let spec = fourth.last_spec_for("builder").expect("builder spec");
    assert!(spec.resume.is_some(), "{spec:?}");
    assert_eq!(spec.model.as_deref(), Some("claude-opus-5-5"));
}
