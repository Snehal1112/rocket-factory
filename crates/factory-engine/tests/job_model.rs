mod common;

use common::{committing_fixture, scripted, scripted_auto, Effect, Fixture};
use factory_agent::AgentSpec;
use factory_core::Board;

/// A builder that writes one test file and leaves it uncommitted.
fn write_effect() -> Effect {
    Box::new(|spec: &AgentSpec| {
        std::fs::create_dir_all(spec.cwd.join("tests")).expect("mkdir");
        std::fs::write(spec.cwd.join("tests/a.txt"), "x\n").expect("write");
        "done".to_string()
    })
}

fn set_job_model(fx: &Fixture, model: &str) {
    let mut board = Board::load(&fx.board_path).expect("load");
    board.find_mut("001").expect("job").model = Some(model.to_string());
    board.save(&fx.board_path).expect("save");
}

fn assert_runs_on(spec: &AgentSpec, model: &str) {
    assert_eq!(spec.model.as_deref(), Some(model), "{}", spec.role);
    assert!(
        spec.env
            .iter()
            .any(|(k, v)| k == "CLAUDE_CODE_SUBAGENT_MODEL" && v == model),
        "{}: {:?}",
        spec.role,
        spec.env
    );
}

#[test]
fn a_job_model_runs_the_builder_committer_and_reviewer_on_it() {
    let fx = committing_fixture();
    set_job_model(&fx, "claude-opus-5-5");
    let runner = scripted_auto(vec![write_effect()]);
    fx.engine(&runner).build_stage().expect("build");
    fx.engine(&runner).checks_stage().expect("checks");
    let rr = scripted(vec![Box::new(|_| "fine\nVERDICT: APPROVE".to_string())]);
    fx.engine(&rr).review_stage().expect("review");
    let built = runner.specs_for("builder");
    assert!(!built.is_empty());
    built
        .iter()
        .for_each(|s| assert_runs_on(s, "claude-opus-5-5"));
    runner
        .specs_for("committer")
        .iter()
        .for_each(|s| assert_runs_on(s, "claude-opus-5-5"));
    assert_runs_on(
        &rr.last_spec_for("reviewer").expect("reviewer"),
        "claude-opus-5-5",
    );
    let ledger = fx.ledger();
    assert!(ledger.contains("claude-opus-5-5"), "{ledger}");
    assert!(
        ledger.contains("review model: claude-opus-5-5 (job model)"),
        "{ledger}"
    );
}

#[test]
fn without_a_job_model_the_config_models_stay() {
    let fx = committing_fixture();
    let runner = scripted_auto(vec![write_effect()]);
    fx.engine(&runner).build_stage().expect("build");
    let spec = runner.last_spec_for("builder").expect("builder");
    assert_eq!(
        spec.model.as_deref(),
        Some(fx.config.builder_model.as_str())
    );
}

#[test]
fn a_blank_job_model_is_ignored() {
    let fx = committing_fixture();
    set_job_model(&fx, "  ");
    let runner = scripted_auto(vec![write_effect()]);
    fx.engine(&runner).build_stage().expect("build");
    let spec = runner.last_spec_for("builder").expect("builder");
    assert_eq!(
        spec.model.as_deref(),
        Some(fx.config.builder_model.as_str())
    );
}
