mod common;

use common::{committing_fixture, scripted, scripted_auto, Effect, Fixture, ScriptedRunner};
use factory_core::{CheckRule, ParkKind, ParkRecord, Stage};
use factory_engine::{Decision, StageOutcome};

/// A builder that writes each file with the given body and leaves it uncommitted.
fn write_effect(files: Vec<(&'static str, &'static str)>) -> Effect {
    Box::new(move |spec| {
        for (name, body) in &files {
            if let Some(parent) = std::path::Path::new(name).parent() {
                std::fs::create_dir_all(spec.cwd.join(parent)).expect("mkdir");
            }
            std::fs::write(spec.cwd.join(name), body).expect("write file");
        }
        "done".to_string()
    })
}

/// Builds the job with one builder run that writes `files`.
fn built(fx: &Fixture, files: Vec<(&'static str, &'static str)>) -> ScriptedRunner {
    let runner = scripted_auto(vec![write_effect(files)]);
    fx.engine(&runner).build_stage().expect("build");
    runner
}

fn only_src_checks(fx: &mut Fixture) {
    fx.config.checks = vec![CheckRule {
        paths: vec!["src/**".into()],
        commands: vec!["false".into()],
    }];
}

fn reviewer(reply: &'static str) -> ScriptedRunner {
    scripted(vec![Box::new(move |_| reply.to_string())])
}

#[test]
fn a_code_diff_with_no_mapped_check_parks_as_no_checks_before_the_review() {
    let mut fx = committing_fixture();
    only_src_checks(&mut fx);
    let runner = built(&fx, vec![("a.txt", "x\n")]);
    let out = fx.engine(&runner).checks_stage().expect("checks");
    match out {
        StageOutcome::NeedsHuman(r) => {
            assert!(r.contains("no check ran") && r.contains("a.txt"), "{r}")
        }
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(fx.job().stage, Stage::NeedsHuman);
    let rec = ParkRecord::read(&fx.job_dir).expect("park.json");
    assert_eq!(rec.kind, ParkKind::NoChecks);
    assert!(
        rec.next.iter().any(|n| n.contains("--without-review")),
        "{:?}",
        rec.next
    );
    assert!(
        rec.parts.iter().any(|p| p.text.contains("PASS none")),
        "{:?}",
        rec.parts
    );
}

#[test]
fn a_docs_only_diff_with_no_mapped_check_is_not_parked() {
    let mut fx = committing_fixture();
    only_src_checks(&mut fx);
    let runner = built(&fx, vec![("a.md", "x\n")]);
    let out = fx.engine(&runner).checks_stage().expect("checks");
    assert_eq!(out, StageOutcome::Advanced(Stage::Review));
}

#[test]
fn a_risky_code_diff_with_no_mapped_check_is_left_to_decide() {
    let mut fx = committing_fixture();
    only_src_checks(&mut fx);
    let runner = built(&fx, vec![("src-tauri/src/lib.rs", "x\n")]);
    let out = fx.engine(&runner).checks_stage().expect("checks");
    assert_eq!(out, StageOutcome::Advanced(Stage::Review));
    let rr = reviewer("fine\nVERDICT: APPROVE");
    let review = fx.engine(&rr).review_stage().expect("review");
    match review.decision {
        Decision::NeedsHuman { kind, .. } => assert_eq!(kind, ParkKind::RiskyDiff),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn the_final_checks_gate_parks_an_approved_job() {
    let mut fx = committing_fixture();
    let runner = built(&fx, vec![("a.txt", "x\n")]);
    fx.engine(&runner)
        .checks_stage()
        .expect("checks mapped at round 1");
    let rr = reviewer("fine\nVERDICT: APPROVE");
    let review = fx.engine(&rr).review_stage().expect("review");
    assert_eq!(review.decision, Decision::AutoMerge);
    // A fix round changed the diff so that no rule maps to it any more.
    only_src_checks(&mut fx);
    let out = fx.engine(&rr).apply_decision(&review).expect("apply");
    assert!(matches!(out, StageOutcome::NeedsHuman(_)), "{out:?}");
    assert_eq!(
        ParkRecord::read(&fx.job_dir).expect("park.json").kind,
        ParkKind::NoChecks
    );
    assert_ne!(fx.job().stage, Stage::Approve);
}

fn ledger(fx: &Fixture) -> String {
    std::fs::read_to_string(fx.job_dir.join("progress.md")).unwrap_or_default()
}

const NOTE: &str = "No test was added in this job.";
const LEDGER_NOTE: &str = "no test file or test marker in a code diff";

fn reviewer_prompt(fx: &Fixture) -> String {
    let rr = reviewer("fine\nVERDICT: APPROVE");
    fx.engine(&rr).review_stage().expect("review");
    rr.last_spec().expect("reviewer spec").prompt
}

#[test]
fn a_code_diff_with_no_test_warns_and_does_not_park() {
    let fx = committing_fixture();
    let runner = built(&fx, vec![("a.txt", "x\n")]);
    let out = fx.engine(&runner).checks_stage().expect("checks");
    assert_eq!(out, StageOutcome::Advanced(Stage::Review));
    assert!(ledger(&fx).contains(LEDGER_NOTE), "{}", ledger(&fx));
    assert!(reviewer_prompt(&fx).contains(NOTE));
}

#[test]
fn a_new_test_path_gives_no_note() {
    let fx = committing_fixture();
    let runner = built(&fx, vec![("a.txt", "x\n"), ("tests/a.txt", "x\n")]);
    fx.engine(&runner).checks_stage().expect("checks");
    assert!(!ledger(&fx).contains(LEDGER_NOTE));
    assert!(!reviewer_prompt(&fx).contains(NOTE));
}

#[test]
fn an_added_test_marker_counts_as_a_test() {
    let fx = committing_fixture();
    let runner = built(&fx, vec![("src/a.rs", "#[cfg(test)]\nmod t {}\n")]);
    fx.engine(&runner).checks_stage().expect("checks");
    assert!(!ledger(&fx).contains(LEDGER_NOTE));
    assert!(!reviewer_prompt(&fx).contains(NOTE));
}

#[test]
fn require_tests_parks_a_code_diff_with_no_test() {
    let mut fx = committing_fixture();
    fx.config.require_tests = true;
    let runner = built(&fx, vec![("a.txt", "x\n")]);
    let out = fx.engine(&runner).checks_stage().expect("checks");
    assert!(matches!(out, StageOutcome::NeedsHuman(_)), "{out:?}");
    let rec = ParkRecord::read(&fx.job_dir).expect("park.json");
    assert_eq!(rec.kind, ParkKind::NoTests);
    assert!(
        rec.next.iter().any(|n| n.contains("--without-review")),
        "{:?}",
        rec.next
    );
}

#[test]
fn require_tests_does_not_park_a_docs_only_diff() {
    let mut fx = committing_fixture();
    fx.config.require_tests = true;
    let runner = built(&fx, vec![("a.md", "x\n")]);
    let out = fx.engine(&runner).checks_stage().expect("checks");
    assert_eq!(out, StageOutcome::Advanced(Stage::Review));
}

#[test]
fn a_failing_check_still_wins_over_the_no_tests_rule() {
    let mut fx = committing_fixture();
    fx.config.require_tests = true;
    fx.config.checks = vec![CheckRule {
        paths: vec!["**".into()],
        commands: vec!["false".into()],
    }];
    let runner = built(&fx, vec![("a.txt", "x\n")]);
    let out = fx.engine(&runner).checks_stage().expect("checks");
    assert_eq!(
        out,
        StageOutcome::Advanced(Stage::Build),
        "a fix round, not a no-tests park"
    );
}

fn approve(fx: &Fixture) {
    let rr = reviewer("fine\nVERDICT: APPROVE");
    let e = fx.engine(&rr);
    let review = e.review_stage().expect("review");
    assert_eq!(review.decision, Decision::AutoMerge);
    assert_eq!(
        e.apply_decision(&review).expect("apply"),
        StageOutcome::Advanced(Stage::Approve)
    );
}

#[test]
fn the_ledger_names_the_commands_that_ran_before_the_approval_line() {
    let fx = committing_fixture();
    let runner = built(&fx, vec![("tests/a.txt", "x\n")]);
    fx.engine(&runner).checks_stage().expect("checks");
    approve(&fx);
    let text = ledger(&fx);
    let ran = text.find("checks ran: true").expect("a checks ran line");
    let approved = text.find("auto-merge approved").expect("the approval line");
    assert!(ran < approved, "{text}");
}

#[test]
fn the_ledger_says_none_when_no_command_ran() {
    let mut fx = committing_fixture();
    only_src_checks(&mut fx);
    let runner = built(&fx, vec![("a.md", "x\n")]);
    fx.engine(&runner).checks_stage().expect("checks");
    approve(&fx);
    assert!(ledger(&fx).contains("checks ran: none"), "{}", ledger(&fx));
}

#[test]
fn a_docs_only_job_is_not_stopped_by_the_gates_at_the_end() {
    let mut fx = committing_fixture();
    only_src_checks(&mut fx);
    fx.config.require_tests = true;
    let runner = built(&fx, vec![("docs/a.md", "x\n")]);
    fx.engine(&runner).checks_stage().expect("checks");
    approve(&fx);
    assert_eq!(fx.job().stage, Stage::Approve);
}

/// A fixture scoped to the `web` project in `apps/web`, which exists on the base branch.
fn scoped_fixture() -> Fixture {
    let mut fx = committing_fixture();
    common::commit_file(&fx.main, "apps/web/index.txt", "web\n");
    fx.repo = factory_git::Repo::open(&fx.main)
        .expect("open")
        .with_scope("web", std::path::Path::new("apps/web"));
    fx
}

fn worktree_of_job(fx: &Fixture) -> std::path::PathBuf {
    std::path::PathBuf::from(fx.job().worktree.expect("worktree"))
}

/// A check script that records its working directory. Check commands run without a shell.
const CHECK_SCRIPT: (&str, &str) = ("check.sh", "pwd > cwd.out\n");

fn pwd_check(fx: &mut Fixture) {
    fx.config.checks = vec![CheckRule {
        paths: vec!["**".into()],
        commands: vec!["sh check.sh".into()],
    }];
}

#[test]
fn checks_map_project_relative_paths() {
    let mut fx = scoped_fixture();
    only_src_checks(&mut fx);
    let runner = built(&fx, vec![("src/a.rs", "x\n")]);
    let out = fx.engine(&runner).checks_stage().expect("checks");
    // The `src/**` check ran and failed, so the no-checks gate did not park the job.
    assert_eq!(out, StageOutcome::Advanced(Stage::Build));
}

#[test]
fn risk_paths_are_project_relative() {
    let mut fx = scoped_fixture();
    fx.config.risk_paths = vec!["migrations/**".into()];
    let runner = built(&fx, vec![("migrations/1.sql", "x\n")]);
    fx.engine(&runner).checks_stage().expect("checks");
    let rr = reviewer("fine\nVERDICT: APPROVE");
    let review = fx.engine(&rr).review_stage().expect("review");
    match review.decision {
        Decision::NeedsHuman { kind, .. } => assert_eq!(kind, ParkKind::RiskyDiff),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn a_non_risk_path_in_the_project_is_not_risky() {
    let mut fx = scoped_fixture();
    fx.config.risk_paths = vec!["migrations/**".into()];
    let runner = built(&fx, vec![("src/a.rs", "x\n")]);
    fx.engine(&runner).checks_stage().expect("checks");
    let rr = reviewer("fine\nVERDICT: APPROVE");
    let review = fx.engine(&rr).review_stage().expect("review");
    assert_eq!(review.decision, Decision::AutoMerge);
}

#[test]
fn checks_run_in_the_project_folder() {
    let mut fx = scoped_fixture();
    pwd_check(&mut fx);
    let runner = built(&fx, vec![("src/a.rs", "x\n"), CHECK_SCRIPT]);
    fx.engine(&runner).checks_stage().expect("checks");
    let wt = worktree_of_job(&fx);
    let seen = std::fs::read_to_string(wt.join("apps/web/cwd.out")).expect("cwd.out");
    let seen = std::fs::canonicalize(seen.trim()).expect("canonical");
    let want = std::fs::canonicalize(wt.join("apps/web")).expect("canonical");
    assert_eq!(seen, want);
}

#[test]
fn top_folder_project_paths_are_unchanged() {
    let mut fx = committing_fixture();
    pwd_check(&mut fx);
    let runner = built(&fx, vec![("src/a.rs", "x\n"), CHECK_SCRIPT]);
    fx.engine(&runner).checks_stage().expect("checks");
    assert!(worktree_of_job(&fx).join("cwd.out").exists());
}
