mod common;

use common::{commit_file, scripted, Effect, Fixture, ScriptedRunner};
use factory_core::Stage;
use factory_engine::StageOutcome;

fn commit_effect(file: &'static str, lines: usize) -> Effect {
    Box::new(move |spec| {
        let body: String = (0..lines).map(|n| format!("line {n}\n")).collect();
        let sha = commit_file(&spec.cwd, file, &body);
        format!("done\nHEAD: {sha}")
    })
}

fn silent_effect() -> Effect {
    Box::new(|_| "nothing to do".to_string())
}

#[test]
fn first_build_records_head_session_and_moves_to_review() {
    let fx = Fixture::new();
    let runner = scripted(vec![commit_effect("a.txt", 3)]);
    let e = fx.engine(&runner);
    let out = e.build_stage().expect("build");
    assert_eq!(out, StageOutcome::Advanced(Stage::Review));
    let job = fx.job();
    assert!(job.head.is_some());
    assert_ne!(job.head, job.base);
    assert!(job.prev_head.is_none());
    assert_eq!(job.builder_id.as_deref(), Some("sess-1"));
    assert_eq!(job.stage, Stage::Review);
}

#[test]
fn first_build_with_no_commit_parks_the_job() {
    let fx = Fixture::new();
    let runner = scripted(vec![silent_effect()]);
    let out = fx.engine(&runner).build_stage().expect("build");
    match out {
        StageOutcome::NeedsHuman(r) => assert!(r.contains("no commits"), "{r}"),
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(fx.job().stage, Stage::NeedsHuman);
}

#[test]
fn resume_uses_the_session_and_records_prev_head() {
    let fx = Fixture::new();
    let runner = scripted(vec![commit_effect("a.txt", 3), commit_effect("b.txt", 2)]);
    let e = fx.engine(&runner);
    e.build_stage().expect("first");
    let first_head = fx.job().head.clone();
    // Simulate a failed check round: round 2, findings file, stage build.
    std::fs::write(fx.job_dir.join("changes-r2.md"), "fix it").expect("findings");
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    let job = board.find_mut("001").expect("job");
    job.round = 2;
    job.set_stage(Stage::Build, None, true, 1.0).expect("stage");
    board.save(&fx.board_path).expect("save");
    let out = e.build_stage().expect("resume");
    assert_eq!(out, StageOutcome::Advanced(Stage::Review));
    let job = fx.job();
    assert_eq!(job.prev_head, first_head);
    assert_ne!(job.head, first_head);
}

#[test]
fn resume_with_no_new_commit_parks_the_job() {
    let fx = Fixture::new();
    let runner = scripted(vec![commit_effect("a.txt", 3), silent_effect()]);
    let e = fx.engine(&runner);
    e.build_stage().expect("first");
    std::fs::write(fx.job_dir.join("changes-r2.md"), "fix it").expect("findings");
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    let job = board.find_mut("001").expect("job");
    job.round = 2;
    job.set_stage(Stage::Build, None, true, 1.0).expect("stage");
    board.save(&fx.board_path).expect("save");
    match e.build_stage().expect("resume") {
        StageOutcome::NeedsHuman(r) => assert!(r.contains("no new commit"), "{r}"),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn diff_over_the_cap_parks_the_job() {
    let mut fx = Fixture::new();
    fx.config.diff_cap_lines = 5;
    let runner = scripted(vec![commit_effect("big.txt", 50)]);
    match fx.engine(&runner).build_stage().expect("build") {
        StageOutcome::NeedsHuman(r) => assert!(r.contains("cap"), "{r}"),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn bad_config_is_rejected_when_the_engine_is_built() {
    let mut fx = Fixture::new();
    fx.config.checks = vec![factory_core::CheckRule {
        paths: vec!["**".into()],
        commands: vec!["cargo test --workspace -j4".into()],
    }];
    let runner = scripted(vec![]);
    let res = factory_engine::Engine::new(
        fx.board_path.clone(), fx.job_dir.clone(), "001".into(), fx.plan_file.clone(),
        &fx.repo, &runner, fx.config.clone(), &fx.events,
    );
    assert!(res.is_err());
}

fn built(fx: &Fixture) -> ScriptedRunner {
    let runner = scripted(vec![commit_effect("a.txt", 3)]);
    fx.engine(&runner).build_stage().expect("build");
    runner
}

#[test]
fn passing_checks_write_checks_md_and_stay_in_review() {
    let fx = Fixture::new();
    let runner = built(&fx);
    let out = fx.engine(&runner).checks_stage().expect("checks");
    assert_eq!(out, StageOutcome::Advanced(Stage::Review));
    let md = std::fs::read_to_string(fx.job_dir.join("checks.md")).expect("checks.md");
    assert!(md.contains("PASS true ("), "{md}");
}

#[test]
fn failing_checks_start_a_fix_round() {
    let mut fx = Fixture::new();
    fx.config.checks = vec![factory_core::CheckRule { paths: vec!["**".into()], commands: vec!["false".into()] }];
    let runner = built(&fx);
    let out = fx.engine(&runner).checks_stage().expect("checks");
    assert_eq!(out, StageOutcome::Advanced(Stage::Build));
    let job = fx.job();
    assert_eq!(job.round, 2);
    assert_eq!(job.stage, Stage::Build);
    let findings = std::fs::read_to_string(fx.job_dir.join("changes-r2.md")).expect("findings");
    assert!(findings.contains("false"), "{findings}");
    let md = std::fs::read_to_string(fx.job_dir.join("checks.md")).expect("checks.md");
    assert!(md.contains("FAIL false (exit 1)"), "{md}");
}

#[test]
fn failing_checks_beyond_fix_rounds_need_a_human() {
    let mut fx = Fixture::new();
    fx.config.fix_rounds = 1;
    fx.config.checks = vec![factory_core::CheckRule { paths: vec!["**".into()], commands: vec!["false".into()] }];
    let runner = built(&fx);
    let e = fx.engine(&runner);
    assert_eq!(e.checks_stage().expect("round 1"), StageOutcome::Advanced(Stage::Build));
    // Put the job back in review at round 2, as a second build would.
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    board.find_mut("001").expect("job").set_stage(Stage::Review, None, false, 2.0).expect("stage");
    board.save(&fx.board_path).expect("save");
    match e.checks_stage().expect("round 2") {
        StageOutcome::NeedsHuman(r) => assert!(r.contains("fix rounds"), "{r}"),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn docs_only_diff_still_writes_a_pass_line() {
    let mut fx = Fixture::new();
    fx.config.checks = vec![factory_core::CheckRule { paths: vec!["src/**".into()], commands: vec!["false".into()] }];
    let runner = built(&fx);
    fx.engine(&runner).checks_stage().expect("checks");
    let md = std::fs::read_to_string(fx.job_dir.join("checks.md")).expect("checks.md");
    assert!(md.contains("PASS none"), "{md}");
}
