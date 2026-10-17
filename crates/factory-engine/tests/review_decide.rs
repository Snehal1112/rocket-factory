mod common;

use common::{committing_fixture, scripted, scripted_auto, Effect, Fixture, ScriptedRunner};
use factory_core::{BudgetState, DiffClass, Stage, Verdict};
use factory_engine::{decide, DecideInput, Decision, ReviewerExtra, StageOutcome};

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
        Decision::NeedsHuman { reason: r, kind } => {
            assert!(r.contains("src-tauri/src/lib.rs"), "{r}");
            assert_eq!(kind, factory_core::ParkKind::RiskyDiff);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn stopped_budget_needs_a_human() {
    let v = Verdict::Approve;
    let mut i = input(&v);
    i.budget = BudgetState::Stop;
    assert!(matches!(decide(&i), Decision::NeedsHuman { reason: r, kind } if r.contains("budget") && kind == factory_core::ParkKind::BudgetStopped));
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
    assert!(matches!(decide(&i), Decision::NeedsHuman { reason: r, kind } if r.contains("cap") && kind == factory_core::ParkKind::DiffTooLarge));
}

#[test]
fn failing_checks_give_a_fix_round_then_a_human() {
    let v = Verdict::Approve;
    let mut i = input(&v);
    i.checks_ok = false;
    assert_eq!(decide(&i), Decision::FixRound);
    i.round = 3;
    assert!(matches!(decide(&i), Decision::NeedsHuman { reason: r, kind } if r.contains("fix rounds") && kind == factory_core::ParkKind::ChecksFailed));
}

#[test]
fn changes_and_missing_verdicts_give_a_fix_round_then_a_human() {
    for v in [Verdict::Changes, Verdict::Missing] {
        let mut i = input(&v);
        assert_eq!(decide(&i), Decision::FixRound);
        i.round = 3;
        assert!(matches!(decide(&i), Decision::NeedsHuman { kind: factory_core::ParkKind::ChangesRemain, .. }));
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
fn the_reviewer_prompt_carries_the_kb_line_only_when_set() {
    for kb in [Some("/kb/rocketapi"), None] {
        let mut fx = committing_fixture();
        fx.config.kb_path = kb.map(String::from);
        build_and_pass_checks(&fx, &["a.txt"]);
        let rr = reviewer("ok\nVERDICT: APPROVE");
        fx.engine(&rr).review_stage().expect("review");
        let spec = rr.last_spec().expect("spec");
        assert_eq!(spec.allowed_tools, vec!["Read".to_string(), "Grep".to_string(), "Glob".to_string()]);
        match kb {
            Some(dir) => {
                assert!(spec.prompt.contains(&format!("Knowledge base: {dir}")), "{}", spec.prompt);
                assert!(spec.prompt.contains("README.md"));
                assert_eq!(spec.add_dirs, vec![std::path::PathBuf::from(dir)]);
            }
            None => {
                assert!(!spec.prompt.contains("Knowledge base"));
                assert!(spec.add_dirs.is_empty());
            }
        }
    }
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
    assert_eq!(spec.tools, Some("Read,Grep,Glob".to_string()));
    assert_eq!(spec.agent, Some("factory-code-reviewer".to_string()));
    assert_eq!(spec.budget_weighted, Some(fx.config.budget_weighted));
    assert!(spec.prompt.contains("offset"));
    assert!(spec.prompt.contains("1500"));
    assert!(spec.prompt.contains("60 lines"));
    assert!(job_dir.join("review-code-r1.md").exists());
}

#[test]
fn reviewer_prompt_names_the_spec_and_the_earlier_findings_when_they_exist() {
    let fx = Fixture::new();
    build_and_pass_checks(&fx, &["a.txt"]);
    // Round 1 has no spec file and no earlier findings, so neither is named.
    let rr = reviewer("ok\nVERDICT: APPROVE");
    fx.engine(&rr).review_stage().expect("review");
    let prompt = rr.last_spec().expect("spec").prompt;
    assert!(!prompt.contains("The spec is at"), "{prompt}");
    assert!(!prompt.contains("findings of round"), "{prompt}");
    // With a spec and a round 1 review on disk, a round 2 reviewer is pointed at both.
    std::fs::write(fx.job_dir.join("spec.md"), "# Spec\n").expect("spec");
    std::fs::write(fx.job_dir.join("review-code-r1.md"), "1. [blocker] x\n").expect("earlier review");
    let spec = fx.engine(&rr).reviewer_spec(&fx.main, &fx.job_dir.join("d.diff"), &fx.job_dir.join("n.txt"), 2, &[], &ReviewerExtra::default());
    assert!(spec.prompt.contains(&format!("The spec is at {}", fx.job_dir.join("spec.md").display())), "{}", spec.prompt);
    assert!(spec.prompt.contains("review-code-r1.md"), "{}", spec.prompt);
}

#[test]
fn reviewer_rules_come_from_the_config_and_are_absent_by_default() {
    let mut fx = committing_fixture();
    build_and_pass_checks(&fx, &["a.txt"]);
    let rr = reviewer("ok\nVERDICT: APPROVE");
    let none = fx.engine(&rr).reviewer_spec(&fx.main, &fx.job_dir.join("d.diff"), &fx.job_dir.join("n.txt"), 1, &fx.config.reviewer_rules, &ReviewerExtra::default());
    for banned in ["unwrap", "shadcn", ".claude/rules", "DDD"] {
        assert!(!none.prompt.contains(banned), "{banned} leaked: {}", none.prompt);
    }
    fx.config.reviewer_rules = vec!["Check the error handling.".into(), "Check the naming.".into()];
    fx.engine(&rr).review_stage().expect("review");
    let prompt = rr.last_spec().expect("spec").prompt;
    assert!(prompt.contains("Check the error handling. Check the naming."), "{prompt}");
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
    assert_eq!(job.fix_source.as_deref(), Some("review"));
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
        Decision::NeedsHuman { reason: r, kind } => {
            assert!(r.contains("src-tauri/src/lib.rs"), "{r}");
            assert_eq!(*kind, factory_core::ParkKind::RiskyDiff);
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(matches!(e.apply_decision(&out).expect("apply"), StageOutcome::NeedsHuman(_)));
    assert_eq!(fx.job().stage, Stage::NeedsHuman);
    // The reviewer ran, so the park offers a plain approve.
    let rec = factory_core::ParkRecord::read(&fx.job_dir).expect("park.json");
    assert_eq!(rec.next, vec!["factory approve 001", "factory rework 001 <note>"]);
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

const FOCUS_PLAN: &str = "# P\n\nIntro line.\n\n## Review Focus\n\nCheck the lease handling.\n\n### Task 1: first thing\n\nBODY-MARKER one\n";

fn review_prompt_for(plan: &str, ledger: &str) -> String {
    let fx = committing_fixture();
    std::fs::write(&fx.plan_file, plan).expect("plan");
    build_and_pass_checks(&fx, &["a.txt"]);
    let mut text = std::fs::read_to_string(fx.job_dir.join("progress.md")).unwrap_or_default();
    text.push_str(ledger);
    std::fs::write(fx.job_dir.join("progress.md"), text).expect("ledger");
    let rr = reviewer("ok\nVERDICT: APPROVE");
    fx.engine(&rr).review_stage().expect("review");
    rr.last_spec().expect("spec").prompt
}

#[test]
fn the_reviewer_prompt_carries_focus_notes_and_outline() {
    let ledger = "Ruling: builder task 1: kept the old name\nRuling: builder task 1 concerns: slow query\nRuling: other thing\n";
    let prompt = review_prompt_for(FOCUS_PLAN, ledger);
    assert!(prompt.contains("Check the lease handling."), "{prompt}");
    assert_eq!(prompt.matches("Check the lease handling.").count(), 1, "{prompt}");
    assert!(prompt.contains("\nbuilder task 1: kept the old name"), "{prompt}");
    assert!(prompt.contains("builder task 1 concerns: slow query"), "{prompt}");
    assert!(!prompt.contains("other thing"), "{prompt}");
    assert!(prompt.contains("### Task 1: first thing") && prompt.contains("Intro line."), "{prompt}");
    assert!(!prompt.contains("BODY-MARKER"), "{prompt}");
    assert!(!prompt.contains("## Review Focus"), "{prompt}");
    assert!(prompt.contains("Read the diff in pages"), "{prompt}");
    assert!(prompt.contains("VERDICT: APPROVE"), "{prompt}");
}

#[test]
fn a_plan_without_review_focus_has_no_empty_section() {
    let prompt = review_prompt_for("# P\n\nIntro line.\n\n### Task 1: first thing\n\nbody\n", "");
    assert!(!prompt.contains("Review focus"), "{prompt}");
    assert!(prompt.contains("### Task 1: first thing"), "{prompt}");
}

#[test]
fn the_reviewer_spec_is_strict_and_capped() {
    let fx = committing_fixture();
    build_and_pass_checks(&fx, &["a.txt"]);
    let rr = reviewer("ok\nVERDICT: APPROVE");
    fx.engine(&rr).review_stage().expect("review");
    let spec = rr.last_spec().expect("a spec was recorded");
    assert!(spec.strict_mcp);
    assert_eq!(spec.max_turns, Some(factory_engine::REVIEWER_MAX_TURNS));
    assert_eq!(spec.max_turns, Some(40));
}

fn model_of_review(fx: &Fixture, files: &[&'static str]) -> Option<String> {
    build_and_pass_checks(fx, files);
    let rr = reviewer("ok\nVERDICT: APPROVE");
    fx.engine(&rr).review_stage().expect("review");
    rr.last_spec().expect("spec").model
}

fn large_fixture(over: u32) -> Fixture {
    let mut fx = committing_fixture();
    fx.config.reviewer_model = "sonnet".into();
    fx.config.reviewer_model_large = Some("opus".into());
    fx.config.reviewer_large_over_lines = over;
    fx
}

#[test]
fn a_risky_diff_uses_the_large_model_when_set() {
    let fx = large_fixture(800);
    assert_eq!(model_of_review(&fx, &["src-tauri/src/lib.rs"]), Some("opus".to_string()));
}

#[test]
fn a_diff_over_the_line_threshold_uses_the_large_model() {
    // Two files of one line each change two lines. At the threshold the normal model stays, one over it switches.
    let at = large_fixture(2);
    assert_eq!(model_of_review(&at, &["a.txt", "b.txt"]), Some("sonnet".to_string()));
    let over = large_fixture(1);
    assert_eq!(model_of_review(&over, &["a.txt", "b.txt"]), Some("opus".to_string()));
}

#[test]
fn a_small_safe_diff_uses_the_normal_model() {
    let fx = large_fixture(800);
    assert_eq!(model_of_review(&fx, &["a.txt"]), Some("sonnet".to_string()));
}

#[test]
fn with_no_large_model_set_nothing_changes() {
    for (files, over) in [(vec!["a.txt"], 800), (vec!["a.txt", "b.txt"], 1), (vec!["src-tauri/src/lib.rs"], 800)] {
        let mut fx = large_fixture(over);
        fx.config.reviewer_model_large = None;
        assert_eq!(model_of_review(&fx, &files), Some("sonnet".to_string()), "{files:?}");
        let ledger = std::fs::read_to_string(fx.job_dir.join("progress.md")).unwrap_or_default();
        assert!(!ledger.contains("review model:"), "{ledger}");
    }
}

#[test]
fn the_choice_is_written_to_the_ledger() {
    let fx = large_fixture(800);
    model_of_review(&fx, &["src-tauri/src/lib.rs"]);
    let ledger = std::fs::read_to_string(fx.job_dir.join("progress.md")).expect("ledger");
    assert!(ledger.contains("review model: opus (risky diff)"), "{ledger}");
    let fx = large_fixture(1);
    model_of_review(&fx, &["a.txt", "b.txt"]);
    let ledger = std::fs::read_to_string(fx.job_dir.join("progress.md")).expect("ledger");
    assert!(ledger.contains("review model: opus (2 changed lines, over 1)"), "{ledger}");
}
