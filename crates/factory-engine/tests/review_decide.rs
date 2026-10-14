mod common;

use common::{committing_fixture, scripted, scripted_auto, Effect, Fixture, ScriptedRunner};
use factory_core::{BudgetState, DiffClass, Stage, Verdict};
use factory_engine::{decide, DecideInput, Decision, StageOutcome};

fn input(verdict: &Verdict) -> DecideInput<'_> {
    DecideInput {
        checks_ok: true,
        verdict,
        class: DiffClass::Normal,
        risky_summary: String::new(),
        diff_lines: 100,
        diff_cap: 4000,
        budget: BudgetState::Ok,
        round: 1,
        fix_rounds: 2,
    }
}

#[test]
fn clean_normal_diff_auto_merges() {
    assert_eq!(decide(&input(&Verdict::Approve)), Decision::AutoMerge);
}

#[test]
fn doc_only_diff_auto_merges() {
    let v = Verdict::Approve;
    let mut i = input(&v);
    i.class = DiffClass::DocOnly;
    assert_eq!(decide(&i), Decision::AutoMerge);
}

#[test]
fn risky_diff_is_always_needs_human_even_when_approved() {
    let v = Verdict::Approve;
    let mut i = input(&v);
    i.class = DiffClass::Risky;
    i.risky_summary = "risky paths (1): src-tauri/src/lib.rs".into();
    match decide(&i) {
        Decision::NeedsHuman(r) => assert!(r.contains("src-tauri/src/lib.rs"), "{r}"),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn stopped_budget_needs_a_human() {
    let v = Verdict::Approve;
    let mut i = input(&v);
    i.budget = BudgetState::Stop;
    assert!(matches!(decide(&i), Decision::NeedsHuman(r) if r.contains("budget")));
}

#[test]
fn warn_budget_still_merges() {
    let v = Verdict::Approve;
    let mut i = input(&v);
    i.budget = BudgetState::Warn;
    assert_eq!(decide(&i), Decision::AutoMerge);
}

#[test]
fn diff_over_cap_needs_a_human() {
    let v = Verdict::Approve;
    let mut i = input(&v);
    i.diff_lines = 4001;
    assert!(matches!(decide(&i), Decision::NeedsHuman(r) if r.contains("cap")));
}

#[test]
fn failing_checks_give_a_fix_round_then_a_human() {
    let v = Verdict::Approve;
    let mut i = input(&v);
    i.checks_ok = false;
    assert_eq!(decide(&i), Decision::FixRound);
    i.round = 3;
    assert!(matches!(decide(&i), Decision::NeedsHuman(r) if r.contains("fix rounds")));
}

#[test]
fn changes_and_missing_verdicts_give_a_fix_round_then_a_human() {
    for v in [Verdict::Changes, Verdict::Missing] {
        let mut i = input(&v);
        assert_eq!(decide(&i), Decision::FixRound);
        i.round = 3;
        assert!(matches!(decide(&i), Decision::NeedsHuman(_)));
    }
}

fn build_and_pass_checks(fx: &Fixture, files: &[&'static str]) -> ScriptedRunner {
    let effects: Vec<Effect> = vec![Box::new({
        let files: Vec<&'static str> = files.to_vec();
        move |spec| {
            for f in &files {
                if let Some(parent) = std::path::Path::new(f).parent() {
                    std::fs::create_dir_all(spec.cwd.join(parent)).expect("mkdir");
                }
                std::fs::write(spec.cwd.join(f), "x\n").expect("write");
            }
            "done".to_string()
        }
    })];
    let runner = scripted_auto(effects);
    fx.engine(&runner).build_stage().expect("build");
    // The review stage reads the checks.md that the checks stage wrote.
    fx.engine(&runner).checks_stage().expect("checks");
    runner
}

fn reviewer(reply: &'static str) -> ScriptedRunner {
    scripted(vec![Box::new(move |_| reply.to_string())])
}

#[test]
fn reviewer_spec_is_read_only_and_tells_it_to_page_the_diff() {
    let fx = committing_fixture();
    build_and_pass_checks(&fx, &["a.txt"]);
    let rr = reviewer("ok\nVERDICT: APPROVE");
    let e = fx.engine(&rr);
    let job_dir = fx.job_dir.clone();
    e.review_stage().expect("review");
    let spec = rr.last_spec().expect("a spec was recorded");
    assert_eq!(spec.role, "reviewer");
    assert_eq!(spec.model, Some(fx.config.reviewer_model.clone()));
    assert_eq!(spec.allowed_tools, vec!["Read".to_string(), "Grep".to_string(), "Glob".to_string()]);
    assert_eq!(spec.budget_weighted, Some(fx.config.budget_weighted));
    assert!(spec.prompt.contains("offset"));
    assert!(spec.prompt.contains("1500"));
    assert!(spec.prompt.contains("60 lines"));
    assert!(job_dir.join("review-code-r1.md").exists());
}

#[test]
fn approve_on_a_normal_diff_is_auto_merge_and_apply_sets_approve() {
    let fx = committing_fixture();
    build_and_pass_checks(&fx, &["a.txt"]);
    let rr = reviewer("fine\nVERDICT: APPROVE");
    let e = fx.engine(&rr);
    let out = e.review_stage().expect("review");
    assert_eq!(out.decision, Decision::AutoMerge);
    assert_eq!(e.apply_decision(&out).expect("apply"), StageOutcome::Advanced(Stage::Approve));
    assert_eq!(fx.job().stage, Stage::Approve);
    let md = std::fs::read_to_string(fx.job_dir.join("checks.md")).expect("checks.md");
    assert!(md.contains("PASS"), "{md}");
}

#[test]
fn verdict_not_on_the_last_line_counts_as_changes() {
    for reply in ["VERDICT: APPROVE\nbut also this", "```\nVERDICT: APPROVE\n```", "verdict: approve", "looks good"] {
        let fx = committing_fixture();
        build_and_pass_checks(&fx, &["a.txt"]);
        let rr = reviewer(reply);
        let out = fx.engine(&rr).review_stage().expect("review");
        assert_eq!(out.verdict, Verdict::Missing, "{reply}");
        assert_eq!(out.decision, Decision::FixRound, "{reply}");
    }
}

#[test]
fn changes_verdict_starts_a_fix_round_with_the_review_text() {
    let fx = committing_fixture();
    build_and_pass_checks(&fx, &["a.txt"]);
    let rr = reviewer("1. [blocker] src/a.rs:3 bad\nVERDICT: CHANGES");
    let e = fx.engine(&rr);
    let out = e.review_stage().expect("review");
    assert_eq!(out.decision, Decision::FixRound);
    assert_eq!(e.apply_decision(&out).expect("apply"), StageOutcome::Advanced(Stage::Build));
    let job = fx.job();
    assert_eq!(job.round, 2);
    let findings = std::fs::read_to_string(fx.job_dir.join("changes-r2.md")).expect("findings");
    assert!(findings.contains("[blocker] src/a.rs:3"), "{findings}");
}

#[test]
fn risky_diff_with_approve_needs_a_human_naming_the_path() {
    let fx = committing_fixture();
    build_and_pass_checks(&fx, &["src-tauri/src/lib.rs"]);
    let rr = reviewer("fine\nVERDICT: APPROVE");
    let e = fx.engine(&rr);
    let out = e.review_stage().expect("review");
    match &out.decision {
        Decision::NeedsHuman(r) => assert!(r.contains("src-tauri/src/lib.rs"), "{r}"),
        other => panic!("unexpected {other:?}"),
    }
    assert!(matches!(e.apply_decision(&out).expect("apply"), StageOutcome::NeedsHuman(_)));
    assert_eq!(fx.job().stage, Stage::NeedsHuman);
}

#[test]
fn final_check_failure_blocks_auto_merge() {
    let mut fx = committing_fixture();
    build_and_pass_checks(&fx, &["a.txt"]);
    let rr = reviewer("fine\nVERDICT: APPROVE");
    // The checks pass during review, then the mapped command starts failing.
    let e = fx.engine(&rr);
    let out = e.review_stage().expect("review");
    assert_eq!(out.decision, Decision::AutoMerge);
    fx.config.checks = vec![factory_core::CheckRule { paths: vec!["**".into()], commands: vec!["false".into()] }];
    let e2 = fx.engine(&rr);
    assert_eq!(e2.apply_decision(&out).expect("apply"), StageOutcome::Advanced(Stage::Build));
    assert_eq!(fx.job().round, 2);
}

fn code_review_state(fx: &Fixture) -> factory_core::ReviewState {
    fx.job().reviews.get("code").copied().expect("code review state")
}

#[test]
fn review_state_is_running_during_the_run_and_pass_after_approve() {
    let fx = committing_fixture();
    build_and_pass_checks(&fx, &["a.txt"]);
    let board = fx.board_path.clone();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(None));
    let seen_in = seen.clone();
    let rr = scripted(vec![Box::new(move |_| {
        let state = factory_core::Board::load(&board)
            .ok()
            .and_then(|b| b.find("001").ok().and_then(|j| j.reviews.get("code").copied()));
        if let Ok(mut g) = seen_in.lock() {
            *g = state;
        }
        "ok\nVERDICT: APPROVE".to_string()
    })]);
    let e = fx.engine(&rr);
    e.review_stage().expect("review");
    let during = seen.lock().map(|g| *g).unwrap_or(None);
    assert_eq!(during, Some(factory_core::ReviewState::Running));
    assert_eq!(code_review_state(&fx), factory_core::ReviewState::Pass);
}

#[test]
fn review_state_is_changes_after_changes_and_after_a_missing_verdict() {
    for reply in ["bad\nVERDICT: CHANGES", "no verdict line here"] {
        let fx = committing_fixture();
        build_and_pass_checks(&fx, &["a.txt"]);
        let rr = reviewer(reply);
        let e = fx.engine(&rr);
        e.review_stage().expect("review");
        assert_eq!(code_review_state(&fx), factory_core::ReviewState::Changes, "{reply}");
    }
}

#[test]
fn a_fix_round_resets_the_review_state() {
    let fx = committing_fixture();
    build_and_pass_checks(&fx, &["a.txt"]);
    let rr = reviewer("bad\nVERDICT: CHANGES");
    let e = fx.engine(&rr);
    let out = e.review_stage().expect("review");
    assert_eq!(code_review_state(&fx), factory_core::ReviewState::Changes);
    e.apply_decision(&out).expect("apply");
    assert_eq!(fx.job().stage, Stage::Build);
    assert_eq!(code_review_state(&fx), factory_core::ReviewState::Pending);
}
