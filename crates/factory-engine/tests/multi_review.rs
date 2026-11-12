mod common;

use common::{committing_fixture, scripted, scripted_auto, Effect, Fixture, ScriptedRunner};
use factory_core::{
    parse_verdict, AgentFile, Board, CheckRule, Config, Job, ParkKind, ReviewState, Stage, Verdict,
};
use factory_engine::{Decision, ReviewRun, ReviewerExtra};

/// A builder that writes one file and leaves it uncommitted.
fn change_effect() -> Effect {
    Box::new(|spec| {
        std::fs::write(spec.cwd.join("a.txt"), "line\n").expect("write change");
        "done".to_string()
    })
}

/// A fixture whose only rule plans the given format and lint commands over every path.
fn fixture_with_gates(format: &[&str], lint: &[&str]) -> Fixture {
    let mut fx = committing_fixture();
    fx.config.checks = vec![CheckRule {
        paths: vec!["**".into()],
        format: format.iter().map(|s| s.to_string()).collect(),
        lint: lint.iter().map(|s| s.to_string()).collect(),
        ..CheckRule::default()
    }];
    fx
}

/// Builds one changed file, then runs the final checks, which record the gates.
fn run_checks_over_one_file(fx: &Fixture) -> Job {
    let runner = scripted_auto(vec![change_effect()]);
    let e = fx.engine(&runner);
    e.build_stage().expect("build");
    e.final_checks().expect("checks");
    fx.job()
}

const CODE_OK: &str = "- [minor] nit\nVERDICT: APPROVE";
const SECURITY_OK: &str = "No findings.\nVERDICT: APPROVE";
const SECURITY_BAD: &str = "- [blocker] SQL built from input at db.rs:10\nVERDICT: CHANGES";

fn reply(text: &'static str) -> Effect {
    Box::new(move |_| text.to_string())
}

/// An engine that holds the security agent file, so the review stage may run that role.
fn engine<'a>(fx: &'a Fixture, runner: &'a ScriptedRunner) -> factory_engine::Engine<'a> {
    fx.engine(runner).with_agent_files(vec![AgentFile {
        name: "factory-security-reviewer.md".to_string(),
        content: "agent".to_string(),
    }])
}

/// Builds the files, passes the checks and gives the job one pending entry per role.
fn reviewed_fixture_with(mut fx: Fixture, reviewers: &[&str], files: &[&'static str]) -> Fixture {
    fx.config.reviewers = reviewers.iter().map(|s| s.to_string()).collect();
    let files: Vec<&'static str> = files.to_vec();
    let builder: Effect = Box::new(move |spec| {
        for f in &files {
            if let Some(parent) = std::path::Path::new(f).parent() {
                std::fs::create_dir_all(spec.cwd.join(parent)).expect("mkdir");
            }
            std::fs::write(spec.cwd.join(f), "x\n").expect("write");
        }
        "done".to_string()
    });
    let runner = scripted_auto(vec![builder]);
    fx.engine(&runner).build_stage().expect("build");
    fx.engine(&runner).checks_stage().expect("checks");
    let mut board = Board::load(&fx.board_path).expect("load");
    let roles: Vec<String> = reviewers.iter().map(|s| s.to_string()).collect();
    board.find_mut("001").expect("job").set_reviewers(&roles);
    board.save(&fx.board_path).expect("save");
    fx
}

fn reviewed_fixture(reviewers: &[&str]) -> Fixture {
    reviewed_fixture_with(committing_fixture(), reviewers, &["a.txt"])
}

fn agent_calls(runner: &ScriptedRunner) -> usize {
    runner.specs_for("reviewer").len() + runner.specs_for("security-review").len()
}

#[test]
fn security_changes_start_a_fix_round_holding_its_findings() {
    let fx = reviewed_fixture(&["code", "security"]);
    let runner = scripted(vec![reply(CODE_OK), reply(SECURITY_BAD)]);
    let e = engine(&fx, &runner);
    let outcome = e.review_stage().expect("review");
    assert_eq!(outcome.verdict, Verdict::Changes);
    assert!(matches!(outcome.decision, Decision::FixRound));
    let file = std::fs::read_to_string(&outcome.review_path).expect("file");
    assert!(file.contains("## security review") && file.contains("SQL built from input"));
    assert_eq!(parse_verdict(&file), Verdict::Changes);
    let job = fx.job();
    assert_eq!(job.reviews.get("code"), Some(&ReviewState::Pass));
    assert_eq!(job.reviews.get("security"), Some(&ReviewState::Changes));
    e.apply_decision(&outcome).expect("apply");
    let findings =
        std::fs::read_to_string(fx.job_dir.join("changes-r2.md")).expect("fix round findings");
    assert!(findings.contains("## security review") && findings.contains("SQL built from input"));
}

#[test]
fn both_reviewers_approve_and_the_job_merges() {
    let fx = reviewed_fixture(&["code", "security"]);
    let runner = scripted(vec![reply(CODE_OK), reply(SECURITY_OK)]);
    let outcome = engine(&fx, &runner).review_stage().expect("review");
    assert_eq!(outcome.verdict, Verdict::Approve);
    assert!(matches!(outcome.decision, Decision::AutoMerge));
    let job = fx.job();
    assert_eq!(job.reviews.get("code"), Some(&ReviewState::Pass));
    assert_eq!(job.reviews.get("security"), Some(&ReviewState::Pass));
}

#[test]
fn both_roles_run_when_the_first_asks_for_changes() {
    let fx = reviewed_fixture(&["code", "security"]);
    let runner = scripted(vec![
        reply("- [blocker] bad\nVERDICT: CHANGES"),
        reply(SECURITY_OK),
    ]);
    engine(&fx, &runner).review_stage().expect("review");
    assert_eq!(runner.specs_for("reviewer").len(), 1);
    assert_eq!(runner.specs_for("security-review").len(), 1);
}

#[test]
fn one_missing_verdict_is_a_fix_round() {
    let fx = reviewed_fixture(&["code", "security"]);
    let runner = scripted(vec![reply(CODE_OK), reply("looks fine to me")]);
    let outcome = engine(&fx, &runner).review_stage().expect("review");
    assert_eq!(outcome.verdict, Verdict::Missing);
    assert!(matches!(outcome.decision, Decision::FixRound));
}

#[test]
fn the_review_file_has_one_section_per_role_and_one_verdict_line() {
    let fx = reviewed_fixture(&["code", "security"]);
    let runner = scripted(vec![reply(CODE_OK), reply(SECURITY_BAD)]);
    let outcome = engine(&fx, &runner).review_stage().expect("review");
    let file = std::fs::read_to_string(&outcome.review_path).expect("file");
    let verdict_lines: Vec<&str> = file.lines().filter(|l| l.starts_with("VERDICT:")).collect();
    assert_eq!(verdict_lines, vec!["VERDICT: CHANGES"]);
    assert_eq!(file.lines().last(), Some("VERDICT: CHANGES"));
    assert!(file.contains("Role verdict: APPROVE") && file.contains("Role verdict: CHANGES"));
    assert!(file.contains("## code review\n- [minor] nit\nRole verdict: APPROVE\n"));
    assert_eq!(parse_verdict(&file), outcome.verdict);
    let raw_code = std::fs::read_to_string(fx.job_dir.join("review-code-r1.raw.md")).expect("raw");
    assert_eq!(raw_code, CODE_OK);
    let raw_sec =
        std::fs::read_to_string(fx.job_dir.join("review-security-r1.raw.md")).expect("raw");
    assert_eq!(raw_sec, SECURITY_BAD);
    let last = fx.job().last_review.expect("last review");
    assert_eq!(last.file, "review-code-r1.md");
    assert_eq!(last.blockers, 1);
}

#[test]
fn a_code_only_config_writes_the_review_file_as_before() {
    let fx = reviewed_fixture(&["code"]);
    let runner = scripted(vec![reply(CODE_OK)]);
    let outcome = engine(&fx, &runner).review_stage().expect("review");
    let file = std::fs::read_to_string(&outcome.review_path).expect("file");
    assert_eq!(file, CODE_OK);
    assert!(!fx.job_dir.join("review-code-r1.raw.md").exists());
    assert_eq!(agent_calls(&runner), 1);
}

#[test]
fn security_is_skipped_for_a_docs_only_round() {
    let fx = reviewed_fixture_with(
        committing_fixture(),
        &["code", "security"],
        &["docs/a.md", "docs/guide/b.md"],
    );
    let runner = scripted(vec![reply(CODE_OK)]);
    let outcome = engine(&fx, &runner).review_stage().expect("review");
    assert_eq!(
        fx.job().reviews.get("security"),
        Some(&ReviewState::Skipped)
    );
    assert_eq!(agent_calls(&runner), 1);
    let file = std::fs::read_to_string(&outcome.review_path).expect("file");
    assert_eq!(file, CODE_OK);
    assert_eq!(outcome.verdict, Verdict::Approve);
}

/// Reviews a round that changed only `files` and returns the calls made and the security state.
fn security_for(files: &[&'static str], code_paths: Option<Vec<String>>) -> (usize, ReviewState) {
    let mut base = committing_fixture();
    if let Some(code_paths) = code_paths {
        base.config.code_paths = code_paths;
    }
    let fx = reviewed_fixture_with(base, &["code", "security"], files);
    let runner = scripted(vec![reply(CODE_OK), reply(SECURITY_OK)]);
    engine(&fx, &runner).review_stage().expect("review");
    let state = fx
        .job()
        .reviews
        .get("security")
        .cloned()
        .expect("security entry");
    (agent_calls(&runner), state)
}

#[test]
fn security_runs_when_only_a_workflow_file_changes() {
    let (calls, state) = security_for(&[".github/workflows/ci.yml"], Some(vec!["src/**".into()]));
    assert_eq!(calls, 2);
    assert_ne!(state, ReviewState::Skipped);
}

#[test]
fn security_runs_for_a_claude_settings_file() {
    assert_eq!(security_for(&[".claude/settings.json"], None).0, 2);
}

#[test]
fn security_runs_for_an_agent_file() {
    assert_eq!(
        security_for(&[".claude/agents/factory-builder.md"], None).0,
        2
    );
    let preset = "crates/factory-core/presets/rust/agents/factory-code-reviewer.md";
    assert_eq!(security_for(&[preset], None).0, 2);
}

#[test]
fn security_runs_for_claude_md() {
    assert_eq!(security_for(&["CLAUDE.md"], None).0, 2);
}

#[test]
fn security_runs_for_markdown_outside_docs() {
    assert_eq!(security_for(&["README.md"], None).0, 2);
    let command = "crates/factory-core/assets/global-command.md";
    assert_eq!(security_for(&[command], None).0, 2);
}

#[test]
fn a_sync_review_runs_the_code_reviewer_only() {
    let fx = reviewed_fixture(&["code", "security"]);
    let runner = scripted(vec![reply(CODE_OK)]);
    let outcome = engine(&fx, &runner)
        .review_stage_as(ReviewRun::Sync)
        .expect("review");
    assert_eq!(agent_calls(&runner), 1);
    assert_eq!(runner.specs_for("reviewer").len(), 1);
    assert_eq!(
        outcome.review_path.file_name().and_then(|n| n.to_str()),
        Some("review-code-r1-sync1.md")
    );
}

#[test]
fn security_usage_is_logged_under_its_own_role() {
    let fx = reviewed_fixture(&["code", "security"]);
    let runner = scripted(vec![reply(CODE_OK), reply(SECURITY_OK)]);
    engine(&fx, &runner).review_stage().expect("review");
    let usage = std::fs::read_to_string(fx.job_dir.join("usage.jsonl")).expect("usage");
    assert!(usage.contains("\"security-review\""), "{usage}");
}

#[test]
fn a_missing_security_agent_parks_with_a_clear_reason() {
    let fx = reviewed_fixture(&["code", "security"]);
    let runner = scripted(vec![reply(CODE_OK), reply(SECURITY_OK)]);
    let outcome = fx.engine(&runner).review_stage().expect("review");
    assert_eq!(agent_calls(&runner), 0);
    match outcome.decision {
        Decision::NeedsHuman { kind, reason } => {
            assert_eq!(kind, ParkKind::Other);
            assert!(
                reason.contains("factory-security-reviewer.md not found"),
                "{reason}"
            );
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn a_risky_diff_still_goes_to_a_human_after_both_approve() {
    let mut base = committing_fixture();
    base.config.risk_paths = vec!["secrets/**".into()];
    let fx = reviewed_fixture_with(base, &["code", "security"], &["secrets/key.txt"]);
    let runner = scripted(vec![reply(CODE_OK), reply(SECURITY_OK)]);
    let outcome = engine(&fx, &runner).review_stage().expect("review");
    assert_eq!(outcome.verdict, Verdict::Approve);
    match outcome.decision {
        Decision::NeedsHuman { kind, .. } => assert_eq!(kind, ParkKind::RiskyDiff),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn the_security_spec_uses_its_own_agent_role_and_rules() {
    let mut fx = committing_fixture();
    fx.config.security_rules = vec!["No shell calls.".to_string()];
    let runner = scripted_auto(vec![]);
    let diff = fx.job_dir.join("diff.patch");
    let names = fx.job_dir.join("names.txt");
    let spec = fx.engine(&runner).security_spec_with_model(
        &fx.main,
        &diff,
        &names,
        1,
        &ReviewerExtra::default(),
        "claude-sonnet-5-5",
    );
    assert_eq!(spec.agent.as_deref(), Some("factory-security-reviewer"));
    assert_eq!(spec.role, "security-review");
    assert_eq!(spec.tools.as_deref(), Some("Read,Grep,Glob"));
    assert_eq!(spec.model.as_deref(), Some("claude-sonnet-5-5"));
    assert!(spec.prompt.contains("No shell calls."), "{}", spec.prompt);
}

#[test]
fn a_code_spec_is_unchanged() {
    let fx = committing_fixture();
    let runner = scripted_auto(vec![]);
    let spec = fx.engine(&runner).reviewer_spec(
        &fx.main,
        &fx.job_dir.join("diff.patch"),
        &fx.job_dir.join("names.txt"),
        1,
        &[],
        &ReviewerExtra::default(),
    );
    assert_eq!(spec.agent.as_deref(), Some("factory-code-reviewer"));
    assert_eq!(spec.role, "reviewer");
}

#[test]
fn security_review_gets_the_reviewer_plugins_and_sandbox() {
    let config = Config::parse("reviewer_plugins = [\"x@market\"]\n").expect("parse");
    assert_eq!(config.plugins_for("reviewer"), ["x@market"]);
    assert_eq!(
        config.plugins_for("security-review"),
        config.plugins_for("reviewer")
    );
    assert_eq!(
        config.skill_allow_for("security-review"),
        config.skill_allow_for("reviewer")
    );
}

#[test]
fn format_and_lint_results_land_on_the_board_after_the_checks() {
    let fx = fixture_with_gates(&["true"], &["false"]);
    let job = run_checks_over_one_file(&fx);
    assert_eq!(job.reviews.get("format"), Some(&ReviewState::Pass));
    assert_eq!(job.reviews.get("lint"), Some(&ReviewState::Changes));
}

#[test]
fn a_gate_with_no_commands_has_no_entry() {
    let mut fx = committing_fixture();
    fx.config.checks = vec![CheckRule {
        paths: vec!["**".into()],
        commands: vec!["true".into()],
        ..CheckRule::default()
    }];
    let job = run_checks_over_one_file(&fx);
    assert!(!job.reviews.contains_key("format"), "{:?}", job.reviews);
    assert!(!job.reviews.contains_key("lint"), "{:?}", job.reviews);
}

#[test]
fn a_new_round_clears_the_gate_entries() {
    let fx = fixture_with_gates(&["true"], &["false"]);
    let runner = scripted_auto(vec![change_effect()]);
    let e = fx.engine(&runner);
    e.build_stage().expect("build");
    e.final_checks().expect("checks");
    assert!(fx.job().reviews.contains_key("lint"));
    // The failing lint command makes the checks stage start a fix round.
    e.checks_stage().expect("checks stage");
    let job = fx.job();
    assert_eq!(job.stage, Stage::Build);
    assert_eq!(job.round, 2);
    assert!(!job.reviews.contains_key("format"), "{:?}", job.reviews);
    assert!(!job.reviews.contains_key("lint"), "{:?}", job.reviews);
    assert_eq!(job.reviews.get("code"), Some(&ReviewState::Pending));
}

#[test]
fn the_final_checks_record_the_gates_again() {
    let fx = fixture_with_gates(&["true"], &["echo lint"]);
    let runner = scripted_auto(vec![change_effect()]);
    let e = fx.engine(&runner);
    e.build_stage().expect("build");
    e.checks_stage().expect("checks stage");
    assert_eq!(fx.job().reviews.get("lint"), Some(&ReviewState::Pass));
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    let job = board.find_mut("001").expect("job");
    job.reviews.remove("format");
    job.reviews.remove("lint");
    board.save(&fx.board_path).expect("save");
    e.final_checks().expect("final checks");
    let job = fx.job();
    assert_eq!(job.reviews.get("format"), Some(&ReviewState::Pass));
    assert_eq!(job.reviews.get("lint"), Some(&ReviewState::Pass));
}
