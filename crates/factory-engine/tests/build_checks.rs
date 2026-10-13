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

#[test]
fn a_rework_note_is_the_findings_of_the_resume_and_is_removed_after() {
    let fx = Fixture::new();
    let runner = scripted(vec![commit_effect("a.txt", 3), commit_effect("b.txt", 2)]);
    let e = fx.engine(&runner);
    e.build_stage().expect("first");
    // A rework resets the round to 0, so there is no changes-r file, only the note.
    std::fs::write(fx.job_dir.join("rework-note.md"), "rename the helper").expect("note");
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    let job = board.find_mut("001").expect("job");
    job.round = 0;
    job.set_stage(Stage::Build, None, true, 1.0).expect("stage");
    board.save(&fx.board_path).expect("save");
    let out = e.build_stage().expect("resume");
    assert_eq!(out, StageOutcome::Advanced(Stage::Review));
    let spec = runner.last_spec().expect("spec");
    assert!(spec.prompt.contains("rework-note.md"), "{}", spec.prompt);
    assert!(!fx.job_dir.join("rework-note.md").exists());
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

const TWO_TASKS: &str = "# P\n\n### Task 1: first thing\n\nx\n\n### Task 2: second thing\n\ny\n";

fn two_task_fixture() -> Fixture {
    let fx = Fixture::new();
    std::fs::write(&fx.plan_file, TWO_TASKS).expect("plan");
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    board.find_mut("001").expect("job").set_stage(Stage::Build, None, false, 1.0).expect("stage");
    board.save(&fx.board_path).expect("save");
    fx
}

fn ledger_of(fx: &Fixture) -> String {
    std::fs::read_to_string(fx.job_dir.join("progress.md")).unwrap_or_default()
}

#[test]
fn the_first_task_call_stays_in_build_and_names_only_task_one() {
    let fx = two_task_fixture();
    let runner = scripted(vec![commit_effect("a.txt", 3)]);
    let out = fx.engine(&runner).build_stage().expect("build");
    assert_eq!(out, StageOutcome::Advanced(Stage::Build));
    let job = fx.job();
    assert_eq!(job.stage, Stage::Build);
    assert_eq!(job.task_index, 1);
    assert!(job.head.is_none());
    assert_eq!(job.builder_id.as_deref(), Some("sess-1"));
    let ledger = ledger_of(&fx);
    assert!(ledger.contains("Task 1: dispatched"), "{ledger}");
    assert!(ledger.contains("Task 1: implementer done"), "{ledger}");
    assert!(ledger.contains("Ruling: per-task builders, 2 tasks"), "{ledger}");
    assert!(!ledger.contains("Task 2:"), "{ledger}");
    let spec = runner.last_spec().expect("spec");
    assert!(spec.prompt.contains("Do only Task 1"), "{}", spec.prompt);
    assert!(!spec.prompt.contains("Do only Task 2"), "{}", spec.prompt);
    assert_eq!(spec.agent.as_deref(), Some("factory-builder"));
    assert_eq!(spec.max_turns, Some(120));
}

#[test]
fn the_last_task_call_records_head_and_moves_to_review() {
    let fx = two_task_fixture();
    let runner = scripted(vec![commit_effect("a.txt", 3), commit_effect("b.txt", 2)]);
    let e = fx.engine(&runner);
    e.build_stage().expect("task 1");
    let first = fx.repo.tip(&fx.job().branch).expect("tip after task 1");
    let out = e.build_stage().expect("task 2");
    assert_eq!(out, StageOutcome::Advanced(Stage::Review));
    let job = fx.job();
    assert_eq!(job.task_index, 2);
    assert_eq!(job.stage, Stage::Review);
    assert!(job.prev_head.is_none());
    assert!(job.head.is_some() && job.head != job.base && job.head.as_deref() != Some(first.as_str()));
    let ledger = ledger_of(&fx);
    assert!(ledger.contains("Task 2: dispatched"), "{ledger}");
    assert!(ledger.contains("Task 2: implementer done"), "{ledger}");
    assert_eq!(ledger.matches("per-task builders").count(), 1, "{ledger}");
    let prompt = runner.last_spec().expect("spec").prompt;
    assert!(prompt.contains("Do only Task 2"), "{prompt}");
    assert!(prompt.contains(&first[..7]) && prompt.contains("test commit"), "{prompt}");
}

#[test]
fn a_task_without_a_commit_parks_the_job_with_its_number() {
    let fx = two_task_fixture();
    let runner = scripted(vec![commit_effect("a.txt", 3), silent_effect()]);
    let e = fx.engine(&runner);
    e.build_stage().expect("task 1");
    match e.build_stage().expect("task 2") {
        StageOutcome::NeedsHuman(r) => assert!(r.contains("task 2"), "{r}"),
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(fx.job().task_index, 1);
}

#[test]
fn a_plan_without_task_headings_runs_one_whole_plan_builder() {
    let fx = Fixture::new();
    let runner = scripted(vec![commit_effect("a.txt", 3)]);
    let out = fx.engine(&runner).build_stage().expect("build");
    assert_eq!(out, StageOutcome::Advanced(Stage::Review));
    assert_eq!(fx.job().task_index, 0);
    assert!(!ledger_of(&fx).contains("per-task builders"));
}

#[test]
fn a_fix_round_resumes_the_session_and_does_not_restart_the_task_loop() {
    let fx = two_task_fixture();
    let runner = scripted(vec![commit_effect("a.txt", 3), commit_effect("b.txt", 2), commit_effect("c.txt", 1)]);
    let e = fx.engine(&runner);
    e.build_stage().expect("task 1");
    e.build_stage().expect("task 2");
    let head = fx.job().head.clone();
    std::fs::write(fx.job_dir.join("changes-r2.md"), "fix it").expect("findings");
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    let job = board.find_mut("001").expect("job");
    job.round = 2;
    job.set_stage(Stage::Build, None, true, 1.0).expect("stage");
    board.save(&fx.board_path).expect("save");
    let out = e.build_stage().expect("fix");
    assert_eq!(out, StageOutcome::Advanced(Stage::Review));
    let spec = runner.last_spec().expect("spec");
    assert_eq!(spec.resume.as_deref(), Some("sess-1"));
    assert!(spec.prompt.contains("changes-r2.md"), "{}", spec.prompt);
    let job = fx.job();
    assert_eq!(job.task_index, 2);
    assert_eq!(job.prev_head, head);
    assert_eq!(ledger_of(&fx).matches("Task 1: dispatched").count(), 1);
}

#[test]
fn a_stopped_first_task_parks_and_a_rework_resumes_at_that_task_with_the_note() {
    let fx = two_task_fixture();
    // An empty script makes the runner report a failed run, like a budget or stall kill.
    let dead = scripted(vec![]);
    match fx.engine(&dead).build_stage().expect("task 1") {
        StageOutcome::NeedsHuman(r) => assert!(r.contains("builder stopped"), "{r}"),
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(fx.job().task_index, 0);
    // A rework puts the job back in build and leaves a note.
    std::fs::write(fx.job_dir.join("rework-note.md"), "use the small helper").expect("note");
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    board.find_mut("001").expect("job").set_stage(Stage::Build, None, true, 1.0).expect("stage");
    board.save(&fx.board_path).expect("save");
    let runner = scripted(vec![commit_effect("a.txt", 3)]);
    let out = fx.engine(&runner).build_stage().expect("task 1 again");
    assert_eq!(out, StageOutcome::Advanced(Stage::Build));
    let spec = runner.last_spec().expect("spec");
    assert!(spec.prompt.contains("use the small helper"), "{}", spec.prompt);
    assert!(spec.prompt.contains("Do only Task 1"), "{}", spec.prompt);
    assert_eq!(fx.job().task_index, 1);
    assert!(!fx.job_dir.join("rework-note.md").exists());
}
