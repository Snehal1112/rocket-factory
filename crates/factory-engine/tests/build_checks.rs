mod common;

use common::{
    amend_head, commit_file, commit_index, committing_fixture, scripted, scripted_auto, Effect,
    Fixture, ScriptedRunner,
};
use factory_core::Stage;
use factory_engine::StageOutcome;

/// A builder that writes a file and leaves it uncommitted, as the real builder now does.
fn change_effect(file: &'static str, lines: usize) -> Effect {
    Box::new(move |spec| {
        let body: String = (0..lines).map(|n| format!("line {n}\n")).collect();
        std::fs::write(spec.cwd.join(file), body).expect("write change");
        "done".to_string()
    })
}

/// A committer that amends HEAD with the staged index instead of making a new commit.
fn amend_effect(message: &'static str) -> Effect {
    Box::new(move |spec| {
        amend_head(&spec.cwd, message);
        "done".to_string()
    })
}

/// A builder that commits on its own, which the driver must refuse.
fn commits_on_its_own_effect(file: &'static str) -> Effect {
    Box::new(move |spec| {
        commit_file(&spec.cwd, file, "x\n");
        "done".to_string()
    })
}

fn silent_effect() -> Effect {
    Box::new(|_| "nothing to do".to_string())
}

#[test]
fn first_build_records_head_session_and_moves_to_review() {
    let fx = committing_fixture();
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
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
fn first_build_with_no_changes_parks_the_job() {
    let fx = committing_fixture();
    let runner = scripted_auto(vec![silent_effect()]);
    let out = fx.engine(&runner).build_stage().expect("build");
    match out {
        StageOutcome::NeedsHuman(r) => assert!(r.contains("no changes"), "{r}"),
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(fx.job().stage, Stage::NeedsHuman);
}

#[test]
fn resume_uses_the_session_and_records_prev_head() {
    let fx = committing_fixture();
    let runner = scripted_auto(vec![change_effect("a.txt", 3), change_effect("b.txt", 2)]);
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
fn resume_with_no_changes_parks_the_job() {
    let fx = committing_fixture();
    let runner = scripted_auto(vec![change_effect("a.txt", 3), silent_effect()]);
    let e = fx.engine(&runner);
    e.build_stage().expect("first");
    std::fs::write(fx.job_dir.join("changes-r2.md"), "fix it").expect("findings");
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    let job = board.find_mut("001").expect("job");
    job.round = 2;
    job.set_stage(Stage::Build, None, true, 1.0).expect("stage");
    board.save(&fx.board_path).expect("save");
    match e.build_stage().expect("resume") {
        StageOutcome::NeedsHuman(r) => assert!(r.contains("no changes on resume"), "{r}"),
        other => panic!("unexpected {other:?}"),
    }
}

/// Builds once, then puts the job into a fix round of the given source, and returns the head before it.
fn fix_round_from(fx: &Fixture, runner: &ScriptedRunner, source: Option<&str>) -> Option<String> {
    fx.engine(runner).build_stage().expect("first");
    let head = fx.job().head.clone();
    start_fix_round(fx);
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    board.find_mut("001").expect("job").fix_source = source.map(str::to_string);
    board.save(&fx.board_path).expect("save");
    head
}

#[test]
fn a_no_change_fix_round_after_failed_checks_goes_to_review_when_the_checks_now_pass() {
    let fx = committing_fixture();
    let runner = scripted_auto(vec![change_effect("a.txt", 3), silent_effect()]);
    let head = fix_round_from(&fx, &runner, Some("checks"));
    let out = fx.engine(&runner).build_stage().expect("resume");
    assert_eq!(out, StageOutcome::Advanced(Stage::Review));
    let job = fx.job();
    assert_eq!(job.stage, Stage::Review);
    assert_eq!(job.head, head, "the head is unchanged");
    let ledger = std::fs::read_to_string(fx.job_dir.join("progress.md")).expect("ledger");
    assert!(ledger.contains("fix round 2 changed nothing and the checks now pass, the earlier failure did not reproduce"), "{ledger}");
}

#[test]
fn a_no_change_fix_round_after_failed_checks_parks_as_checks_failed_when_they_still_fail() {
    let mut fx = committing_fixture();
    let runner = scripted_auto(vec![change_effect("a.txt", 3), silent_effect()]);
    fx.config.checks = vec![factory_core::CheckRule {
        paths: vec!["**".into()],
        commands: vec!["false".into()],
    }];
    fix_round_from(&fx, &runner, Some("checks"));
    match fx.engine(&runner).build_stage().expect("resume") {
        StageOutcome::NeedsHuman(r) => {
            assert!(r.contains("false") && r.contains("still fail"), "{r}")
        }
        other => panic!("unexpected {other:?}"),
    }
    let job = fx.job();
    assert_eq!(job.stage, Stage::NeedsHuman);
    assert_eq!(job.park_kind.as_deref(), Some("checks-failed"));
    let rec = factory_core::ParkRecord::read(&fx.job_dir).expect("park.json");
    assert_eq!(rec.next, vec!["factory rework 001 <note>"]);
}

#[test]
fn a_no_change_fix_round_after_a_review_still_parks_as_nothing_changed() {
    let fx = committing_fixture();
    let runner = scripted_auto(vec![change_effect("a.txt", 3), silent_effect()]);
    fix_round_from(&fx, &runner, Some("review"));
    match fx.engine(&runner).build_stage().expect("resume") {
        StageOutcome::NeedsHuman(r) => assert!(r.contains("no changes on resume"), "{r}"),
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(fx.job().park_kind.as_deref(), Some("nothing-changed"));
}

fn reply_effect(text: &'static str) -> Effect {
    Box::new(move |_| text.to_string())
}

#[test]
fn a_resume_that_pushes_back_on_everything_parks_with_the_reply_as_a_part() {
    let fx = committing_fixture();
    let reason = "Finding 1 is wrong: the lease is released in Drop, see lease.rs:40.";
    let runner = scripted_auto(vec![change_effect("a.txt", 3), reply_effect(reason)]);
    fix_round_from(&fx, &runner, Some("review"));
    match fx.engine(&runner).build_stage().expect("resume") {
        StageOutcome::NeedsHuman(r) => assert!(r.contains("no changes on resume"), "{r}"),
        other => panic!("unexpected {other:?}"),
    }
    let rec = factory_core::ParkRecord::read(&fx.job_dir).expect("park.json");
    assert_eq!(rec.kind, factory_core::ParkKind::NothingChanged);
    assert_eq!(rec.parts.len(), 1, "{:?}", rec.parts);
    assert!(
        rec.parts[0].text.contains("lease is released in Drop"),
        "{}",
        rec.parts[0].text
    );
    let saved = std::fs::read_to_string(fx.job_dir.join("builder-r2.md")).expect("builder-r2.md");
    assert!(saved.contains("lease is released in Drop"), "{saved}");
}

#[test]
fn a_resume_that_changes_code_still_saves_its_reply() {
    let fx = committing_fixture();
    let runner = scripted_auto(vec![
        change_effect("a.txt", 3),
        Box::new(|spec| {
            std::fs::write(spec.cwd.join("b.txt"), "x\n").expect("write");
            "Fixed 1. Finding 2 is wrong because of the cap.".to_string()
        }),
    ]);
    fix_round_from(&fx, &runner, Some("review"));
    let out = fx.engine(&runner).build_stage().expect("resume");
    assert_eq!(out, StageOutcome::Advanced(Stage::Review));
    let saved = std::fs::read_to_string(fx.job_dir.join("builder-r2.md")).expect("builder-r2.md");
    assert!(
        saved.contains("Finding 2 is wrong because of the cap."),
        "{saved}"
    );
}

#[test]
fn a_resume_reply_of_needs_context_or_blocked_parks_without_committing() {
    for (reply, word) in [
        (
            "Status: NEEDS_CONTEXT which of the two findings applies?",
            "which of the two findings",
        ),
        ("BLOCKED: the schema file is gone", "schema file is gone"),
    ] {
        let fx = committing_fixture();
        let runner = scripted_auto(vec![
            change_effect("a.txt", 3),
            Box::new(move |spec| {
                std::fs::write(spec.cwd.join("b.txt"), "x\n").expect("write");
                reply.to_string()
            }),
        ]);
        fix_round_from(&fx, &runner, Some("review"));
        let tip = fx.repo.tip(&fx.job().branch).expect("tip");
        match fx.engine(&runner).build_stage().expect("resume") {
            StageOutcome::NeedsHuman(r) => assert!(r.contains(word), "{r}"),
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(fx.job().park_kind.as_deref(), Some("builder-blocked"));
        assert_eq!(
            fx.repo.tip(&fx.job().branch).expect("tip"),
            tip,
            "nothing was committed"
        );
        let rec = factory_core::ParkRecord::read(&fx.job_dir).expect("park.json");
        assert!(rec.reason.contains(word), "{}", rec.reason);
    }
}

#[test]
fn diff_over_the_cap_parks_the_job() {
    let mut fx = committing_fixture();
    fx.config.diff_cap_lines = 5;
    let runner = scripted_auto(vec![change_effect("big.txt", 50)]);
    match fx.engine(&runner).build_stage().expect("build") {
        StageOutcome::NeedsHuman(r) => assert!(r.contains("cap"), "{r}"),
        other => panic!("unexpected {other:?}"),
    }
    let rec = factory_core::ParkRecord::read(&fx.job_dir).expect("park.json");
    assert_eq!(rec.kind, factory_core::ParkKind::DiffTooLarge);
    let titles: Vec<&str> = rec.parts.iter().map(|p| p.title.as_str()).collect();
    assert_eq!(titles, vec!["Why it stopped", "Changed files"]);
    assert!(
        rec.parts[1].text.contains("big.txt"),
        "{}",
        rec.parts[1].text
    );
}

#[test]
fn bad_config_is_rejected_when_the_engine_is_built() {
    let mut fx = committing_fixture();
    fx.config.checks = vec![factory_core::CheckRule {
        paths: vec!["**".into()],
        commands: vec!["cargo test --workspace -j4".into()],
    }];
    let runner = scripted(vec![]);
    let res = factory_engine::Engine::new(
        fx.board_path.clone(),
        fx.job_dir.clone(),
        "001".into(),
        fx.plan_file.clone(),
        &fx.repo,
        &runner,
        fx.config.clone(),
        &fx.events,
    );
    assert!(res.is_err());
}

#[test]
fn a_rework_note_is_the_findings_of_the_resume_and_is_removed_after() {
    let fx = committing_fixture();
    let runner = scripted_auto(vec![change_effect("a.txt", 3), change_effect("b.txt", 2)]);
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
    let spec = runner.last_spec_for("builder").expect("spec");
    assert!(spec.prompt.contains("rework-note.md"), "{}", spec.prompt);
    assert!(!fx.job_dir.join("rework-note.md").exists());
}

fn built(fx: &Fixture) -> ScriptedRunner {
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    fx.engine(&runner).build_stage().expect("build");
    runner
}

#[test]
fn passing_checks_write_checks_md_and_stay_in_review() {
    let fx = committing_fixture();
    let runner = built(&fx);
    let out = fx.engine(&runner).checks_stage().expect("checks");
    assert_eq!(out, StageOutcome::Advanced(Stage::Review));
    let md = std::fs::read_to_string(fx.job_dir.join("checks.md")).expect("checks.md");
    assert!(md.contains("PASS true ("), "{md}");
}

#[test]
fn failing_checks_start_a_fix_round() {
    let mut fx = committing_fixture();
    fx.config.checks = vec![factory_core::CheckRule {
        paths: vec!["**".into()],
        commands: vec!["false".into()],
    }];
    let runner = built(&fx);
    let out = fx.engine(&runner).checks_stage().expect("checks");
    assert_eq!(out, StageOutcome::Advanced(Stage::Build));
    let job = fx.job();
    assert_eq!(job.round, 2);
    assert_eq!(job.stage, Stage::Build);
    assert_eq!(job.fix_source.as_deref(), Some("checks"));
    let findings = std::fs::read_to_string(fx.job_dir.join("changes-r2.md")).expect("findings");
    assert!(findings.contains("false"), "{findings}");
    let md = std::fs::read_to_string(fx.job_dir.join("checks.md")).expect("checks.md");
    assert!(md.contains("FAIL false (exit 1)"), "{md}");
}

#[test]
fn failing_checks_beyond_fix_rounds_need_a_human() {
    let mut fx = committing_fixture();
    fx.config.fix_rounds = 1;
    fx.config.checks = vec![factory_core::CheckRule {
        paths: vec!["**".into()],
        commands: vec!["false".into()],
    }];
    let runner = built(&fx);
    let e = fx.engine(&runner);
    assert_eq!(
        e.checks_stage().expect("round 1"),
        StageOutcome::Advanced(Stage::Build)
    );
    // Put the job back in review at round 2, as a second build would.
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    board
        .find_mut("001")
        .expect("job")
        .set_stage(Stage::Review, None, false, 2.0)
        .expect("stage");
    board.save(&fx.board_path).expect("save");
    match e.checks_stage().expect("round 2") {
        StageOutcome::NeedsHuman(r) => assert!(r.contains("fix rounds"), "{r}"),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn docs_only_diff_still_writes_a_pass_line() {
    let mut fx = committing_fixture();
    fx.config.checks = vec![factory_core::CheckRule {
        paths: vec!["src/**".into()],
        commands: vec!["false".into()],
    }];
    let runner = scripted_auto(vec![change_effect("a.md", 3)]);
    fx.engine(&runner).build_stage().expect("build");
    let out = fx.engine(&runner).checks_stage().expect("checks");
    assert_eq!(out, StageOutcome::Advanced(Stage::Review));
    let md = std::fs::read_to_string(fx.job_dir.join("checks.md")).expect("checks.md");
    assert!(md.contains("PASS none"), "{md}");
}

const TWO_TASKS: &str = "# P\n\n### Task 1: first thing\n\nx\n\n### Task 2: second thing\n\ny\n";

fn two_task_fixture() -> Fixture {
    let fx = committing_fixture();
    std::fs::write(&fx.plan_file, TWO_TASKS).expect("plan");
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    board
        .find_mut("001")
        .expect("job")
        .set_stage(Stage::Build, None, false, 1.0)
        .expect("stage");
    board.save(&fx.board_path).expect("save");
    fx
}

#[test]
fn the_first_task_call_stays_in_build_and_names_only_task_one() {
    let fx = two_task_fixture();
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    let out = fx.engine(&runner).build_stage().expect("build");
    assert_eq!(out, StageOutcome::Advanced(Stage::Build));
    let job = fx.job();
    assert_eq!(job.stage, Stage::Build);
    assert_eq!(job.task_index, 1);
    assert!(job.head.is_none());
    assert_eq!(job.builder_id.as_deref(), Some("sess-1"));
    let ledger = fx.ledger();
    assert!(ledger.contains("Task 1: dispatched"), "{ledger}");
    assert!(ledger.contains("Task 1: implementer done"), "{ledger}");
    assert!(
        ledger.contains("Ruling: per-task builders, 2 tasks"),
        "{ledger}"
    );
    assert!(!ledger.contains("Task 2:"), "{ledger}");
    let spec = runner.last_spec_for("builder").expect("spec");
    assert!(spec.prompt.contains("Do only Task 1"), "{}", spec.prompt);
    assert!(!spec.prompt.contains("Do only Task 2"), "{}", spec.prompt);
    assert_eq!(spec.agent.as_deref(), Some("factory-builder"));
    assert_eq!(spec.max_turns, Some(120));
}

#[test]
fn the_last_task_call_records_head_and_moves_to_review() {
    let mut fx = two_task_fixture();
    fx.config.commit_batch = false;
    let runner = scripted_auto(vec![change_effect("a.txt", 3), change_effect("b.txt", 2)]);
    let e = fx.engine(&runner);
    e.build_stage().expect("task 1");
    let first = fx.repo.tip(&fx.job().branch).expect("tip after task 1");
    let out = e.build_stage().expect("task 2");
    assert_eq!(out, StageOutcome::Advanced(Stage::Review));
    let job = fx.job();
    assert_eq!(job.task_index, 2);
    assert_eq!(job.stage, Stage::Review);
    assert!(job.prev_head.is_none());
    assert!(
        job.head.is_some() && job.head != job.base && job.head.as_deref() != Some(first.as_str())
    );
    let ledger = fx.ledger();
    assert!(ledger.contains("Task 2: dispatched"), "{ledger}");
    assert!(ledger.contains("Task 2: implementer done"), "{ledger}");
    assert_eq!(ledger.matches("per-task builders").count(), 1, "{ledger}");
    let prompt = runner.last_spec_for("builder").expect("spec").prompt;
    assert!(prompt.contains("Do only Task 2"), "{prompt}");
    assert!(
        prompt.contains(&first[..7]) && prompt.contains("build step"),
        "{prompt}"
    );
}

#[test]
fn a_task_that_changes_nothing_parks_the_job_with_its_number() {
    let fx = two_task_fixture();
    let runner = scripted_auto(vec![change_effect("a.txt", 3), silent_effect()]);
    let e = fx.engine(&runner);
    e.build_stage().expect("task 1");
    match e.build_stage().expect("task 2") {
        StageOutcome::NeedsHuman(r) => {
            assert!(r.contains("task 2"), "{r}");
            assert!(r.contains("changed nothing"), "{r}");
        }
        other => panic!("unexpected {other:?}"),
    }
    let rec = factory_core::ParkRecord::read(&fx.job_dir).expect("park.json");
    assert_eq!(rec.kind, factory_core::ParkKind::NothingChanged);
    assert!(
        rec.reason.contains("task 2 changed nothing"),
        "{}",
        rec.reason
    );
    assert_eq!(fx.job().park_kind.as_deref(), Some("nothing-changed"));
    assert!(!rec.next.is_empty());
    assert_eq!(fx.job().task_index, 1);
}

#[test]
fn a_plan_without_task_headings_runs_one_whole_plan_builder() {
    let fx = committing_fixture();
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    let out = fx.engine(&runner).build_stage().expect("build");
    assert_eq!(out, StageOutcome::Advanced(Stage::Review));
    assert_eq!(fx.job().task_index, 0);
    assert!(!fx.ledger().contains("per-task builders"));
}

#[test]
fn a_fix_round_resumes_the_session_and_does_not_restart_the_task_loop() {
    let fx = two_task_fixture();
    let runner = scripted_auto(vec![
        change_effect("a.txt", 3),
        change_effect("b.txt", 2),
        change_effect("c.txt", 1),
    ]);
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
    let spec = runner.last_spec_for("builder").expect("spec");
    assert_eq!(spec.resume.as_deref(), Some("sess-1"));
    assert!(spec.prompt.contains("changes-r2.md"), "{}", spec.prompt);
    let job = fx.job();
    assert_eq!(job.task_index, 2);
    assert_eq!(job.prev_head, head);
    assert_eq!(fx.ledger().matches("Task 1: dispatched").count(), 1);
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
    let rec = factory_core::ParkRecord::read(&fx.job_dir).expect("park.json");
    assert_eq!(rec.kind, factory_core::ParkKind::BuilderStopped);
    assert!(
        rec.parts[0].text.contains("Stop reason:"),
        "{}",
        rec.parts[0].text
    );
    assert_eq!(rec.next, vec!["factory rework 001 <note>".to_string()]);
    assert_eq!(fx.job().task_index, 0);
    // A rework puts the job back in build and leaves a note.
    std::fs::write(fx.job_dir.join("rework-note.md"), "use the small helper").expect("note");
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    board
        .find_mut("001")
        .expect("job")
        .set_stage(Stage::Build, None, true, 1.0)
        .expect("stage");
    board.save(&fx.board_path).expect("save");
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    let out = fx.engine(&runner).build_stage().expect("task 1 again");
    assert_eq!(out, StageOutcome::Advanced(Stage::Build));
    let spec = runner.last_spec_for("builder").expect("spec");
    assert!(
        spec.prompt.contains("use the small helper"),
        "{}",
        spec.prompt
    );
    assert!(spec.prompt.contains("Do only Task 1"), "{}", spec.prompt);
    assert_eq!(fx.job().task_index, 1);
    assert!(!fx.job_dir.join("rework-note.md").exists());
}

fn blocked_effect(file: &'static str, reply: &'static str) -> Effect {
    Box::new(move |spec| {
        std::fs::write(spec.cwd.join(file), "half done\n").expect("write change");
        reply.to_string()
    })
}

#[test]
fn a_blocked_reply_parks_without_committing() {
    let fx = two_task_fixture();
    let runner = scripted_auto(vec![blocked_effect("a.txt", "BLOCKED: no schema")]);
    match fx.engine(&runner).build_stage().expect("task 1") {
        StageOutcome::NeedsHuman(r) => assert!(r.contains("no schema"), "{r}"),
        other => panic!("unexpected {other:?}"),
    }
    let rec = factory_core::ParkRecord::read(&fx.job_dir).expect("park.json");
    assert_eq!(rec.kind, factory_core::ParkKind::BuilderBlocked);
    assert!(rec.reason.contains("no schema"), "{}", rec.reason);
    assert_eq!(rec.next, vec!["factory rework 001 <note>".to_string()]);
    let job = fx.job();
    assert_eq!(job.task_index, 0);
    let wt = job.worktree.clone().expect("worktree");
    assert_eq!(
        fx.repo.tip(&job.branch).expect("tip"),
        job.base.clone().expect("base")
    );
    assert!(std::path::Path::new(&wt).join("a.txt").exists());
}

#[test]
fn a_needs_context_reply_parks_like_blocked() {
    let fx = two_task_fixture();
    let runner = scripted_auto(vec![blocked_effect(
        "a.txt",
        "Did some of it.\nStatus: NEEDS_CONTEXT which table?",
    )]);
    match fx.engine(&runner).build_stage().expect("task 1") {
        StageOutcome::NeedsHuman(r) => assert!(r.contains("which table?"), "{r}"),
        other => panic!("unexpected {other:?}"),
    }
    let rec = factory_core::ParkRecord::read(&fx.job_dir).expect("park.json");
    assert_eq!(rec.kind, factory_core::ParkKind::BuilderBlocked);
    assert!(
        rec.parts[0].text.contains("NEEDS_CONTEXT"),
        "{}",
        rec.parts[0].text
    );
}

#[test]
fn a_reply_without_a_status_line_is_committed_as_before() {
    let fx = two_task_fixture();
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    let out = fx.engine(&runner).build_stage().expect("task 1");
    assert_eq!(out, StageOutcome::Advanced(Stage::Build));
    assert_eq!(fx.job().task_index, 1);
    assert!(
        fx.ledger()
            .contains("builder task 1: no status line, treated as done"),
        "{}",
        fx.ledger()
    );
}

fn start_fix_round(fx: &Fixture) {
    std::fs::write(fx.job_dir.join("changes-r2.md"), "fix it").expect("findings");
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    let job = board.find_mut("001").expect("job");
    job.round = 2;
    job.set_stage(Stage::Build, None, true, 1.0).expect("stage");
    board.save(&fx.board_path).expect("save");
}

fn assert_short_base(reason: &str, base: &str) {
    let marker = "the branch was at ";
    let at = reason.find(marker).expect("marker") + marker.len();
    let short = reason.get(at..at + 7).expect("short sha");
    assert!(base.starts_with(short), "{short} vs {base}");
}

#[test]
fn a_whole_plan_builder_that_commits_on_its_own_parks_before_any_committer_run() {
    let fx = committing_fixture();
    let runner = scripted_auto(vec![commits_on_its_own_effect("a.txt")]);
    match fx.engine(&runner).build_stage().expect("build") {
        StageOutcome::NeedsHuman(r) => {
            assert!(r.contains("committed on its own"), "{r}");
            assert!(r.contains("before the run"), "{r}");
            assert_short_base(&r, fx.job().base.as_deref().expect("job base"));
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(runner.specs_for("committer").is_empty());
}

#[test]
fn a_task_builder_that_commits_on_its_own_parks_with_its_number() {
    let fx = two_task_fixture();
    let runner = scripted_auto(vec![commits_on_its_own_effect("a.txt")]);
    match fx.engine(&runner).build_stage().expect("task 1") {
        StageOutcome::NeedsHuman(r) => {
            assert!(r.contains("committed on its own in task 1"), "{r}");
            assert!(r.contains("before the run"), "{r}");
            assert_short_base(&r, fx.job().base.as_deref().expect("job base"));
        }
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(fx.job().task_index, 0);
    assert!(runner.specs_for("committer").is_empty());
}

#[test]
fn the_committer_prompt_names_the_kind_the_task_the_plan_and_the_base_as_floor() {
    let mut fx = two_task_fixture();
    fx.config.commit_batch = false;
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    fx.engine(&runner).build_stage().expect("task 1");
    let prompt = runner
        .last_spec_for("committer")
        .expect("committer spec")
        .prompt;
    assert!(prompt.contains("Kind: task"), "{prompt}");
    assert!(prompt.contains("Task: first thing"), "{prompt}");
    assert!(prompt.contains("Plan: P"), "{prompt}");
    assert!(
        prompt.contains(&format!("Floor: {}", fx.job().base.expect("base"))),
        "{prompt}"
    );
    assert!(prompt.contains("new file:   a.txt"), "{prompt}");
}

#[test]
fn a_whole_plan_build_commits_as_legacy() {
    let fx = committing_fixture();
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    fx.engine(&runner).build_stage().expect("build");
    let prompt = runner
        .last_spec_for("committer")
        .expect("committer spec")
        .prompt;
    assert!(prompt.contains("Kind: legacy"), "{prompt}");
}

#[test]
fn a_fix_round_commits_above_the_reviewed_head() {
    let fx = committing_fixture();
    let runner = scripted_auto(vec![change_effect("a.txt", 3), change_effect("b.txt", 2)]);
    let e = fx.engine(&runner);
    e.build_stage().expect("first");
    let reviewed = fx.job().head.clone().expect("reviewed head");
    start_fix_round(&fx);
    e.build_stage().expect("fix");
    let prompt = runner
        .last_spec_for("committer")
        .expect("committer spec")
        .prompt;
    assert!(prompt.contains("Kind: fix"), "{prompt}");
    assert!(prompt.contains(&format!("Floor: {reviewed}")), "{prompt}");
}

#[test]
fn a_fix_round_that_changes_nothing_parks_without_a_committer_run() {
    let fx = committing_fixture();
    let first = scripted_auto(vec![change_effect("a.txt", 3)]);
    fx.engine(&first).build_stage().expect("first");
    start_fix_round(&fx);
    let second = scripted_auto(vec![silent_effect()]);
    match fx.engine(&second).build_stage().expect("fix") {
        StageOutcome::NeedsHuman(r) => assert!(r.contains("no changes on resume"), "{r}"),
        other => panic!("unexpected {other:?}"),
    }
    assert!(second.specs_for("committer").is_empty());
}

#[test]
fn a_committer_that_rewrites_the_reviewed_head_parks_the_fix_round() {
    let fx = committing_fixture();
    let first = scripted_auto(vec![change_effect("a.txt", 3)]);
    fx.engine(&first).build_stage().expect("first");
    start_fix_round(&fx);
    let second = scripted(vec![
        change_effect("b.txt", 2),
        amend_effect("feat(test): reword the reviewed commit"),
    ]);
    match fx.engine(&second).build_stage().expect("fix") {
        StageOutcome::NeedsHuman(r) => assert!(r.contains("at or below the floor"), "{r}"),
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(fx.job().stage, Stage::NeedsHuman);
}

#[test]
fn a_committer_that_rewrites_an_earlier_task_commit_leaves_a_ruling() {
    let mut fx = two_task_fixture();
    fx.config.commit_batch = false;
    let first = scripted_auto(vec![change_effect("a.txt", 3)]);
    fx.engine(&first).build_stage().expect("task 1");
    let second = scripted(vec![
        change_effect("b.txt", 2),
        amend_effect("feat(test): tasks one and two"),
    ]);
    let out = fx.engine(&second).build_stage().expect("task 2");
    assert_eq!(out, StageOutcome::Advanced(Stage::Review));
    let ledger = fx.ledger();
    assert!(
        ledger.contains("task 2 commit rewrote earlier commits"),
        "{ledger}"
    );
}

#[test]
fn the_builder_prompts_tell_it_not_to_commit() {
    let fx = two_task_fixture();
    let runner = scripted_auto(vec![
        change_effect("a.txt", 3),
        change_effect("b.txt", 2),
        change_effect("c.txt", 1),
    ]);
    let e = fx.engine(&runner);
    e.build_stage().expect("task 1");
    e.build_stage().expect("task 2");
    start_fix_round(&fx);
    e.build_stage().expect("fix");
    let specs = runner.specs_for("builder");
    assert_eq!(specs.len(), 3);
    for spec in &specs {
        assert!(
            spec.prompt.contains("Do not run `git add` or `git commit`"),
            "{}",
            spec.prompt
        );
        assert!(!spec.prompt.contains("1-git-commit"), "{}", spec.prompt);
        assert!(!spec.prompt.contains("HEAD: <sha"), "{}", spec.prompt);
    }
    for spec in &specs[..2] {
        assert!(
            spec.prompt.contains("Skip every commit step of the plan"),
            "{}",
            spec.prompt
        );
    }
    let whole = committing_fixture();
    let whole_runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    whole
        .engine(&whole_runner)
        .build_stage()
        .expect("whole plan");
    let prompt = whole_runner
        .last_spec_for("builder")
        .expect("builder spec")
        .prompt;
    assert!(
        prompt.contains("Do not run `git add` or `git commit`"),
        "{prompt}"
    );
    assert!(!prompt.contains("1-git-commit"), "{prompt}");
    assert!(
        prompt.contains("Skip every commit step of the plan"),
        "{prompt}"
    );
    assert!(!prompt.contains("HEAD: <sha"), "{prompt}");
}

const KB_PROMPT: &str = "Knowledge base: /kb/rocketapi. Read /kb/rocketapi/README.md first";

#[test]
fn the_first_and_resume_builder_prompts_carry_the_kb_line_only_when_set() {
    let mut fx = committing_fixture();
    fx.config.kb_path = Some("/kb/rocketapi".into());
    let runner = scripted_auto(vec![change_effect("a.txt", 3), change_effect("b.txt", 2)]);
    let e = fx.engine(&runner);
    e.build_stage().expect("first");
    let first = runner.last_spec_for("builder").expect("spec").prompt;
    assert!(first.contains(KB_PROMPT), "{first}");
    assert_eq!(
        runner.last_spec_for("builder").expect("spec").add_dirs,
        vec![
            std::path::PathBuf::from("/kb/rocketapi"),
            std::fs::canonicalize(&fx.job_dir).expect("job dir")
        ]
    );
    std::fs::write(fx.job_dir.join("changes-r2.md"), "fix it").expect("findings");
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    let job = board.find_mut("001").expect("job");
    job.round = 2;
    job.set_stage(Stage::Build, None, true, 1.0).expect("stage");
    board.save(&fx.board_path).expect("save");
    e.build_stage().expect("resume");
    let resume = runner.last_spec_for("builder").expect("spec").prompt;
    assert!(resume.contains(KB_PROMPT), "{resume}");
    assert_eq!(
        runner.last_spec_for("builder").expect("spec").add_dirs,
        vec![
            std::path::PathBuf::from("/kb/rocketapi"),
            std::fs::canonicalize(&fx.job_dir).expect("job dir")
        ]
    );

    let off = committing_fixture();
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    off.engine(&runner).build_stage().expect("first");
    let spec = runner.last_spec_for("builder").expect("spec");
    assert!(!spec.prompt.contains("Knowledge base"));
    assert_eq!(
        spec.add_dirs,
        vec![std::fs::canonicalize(&off.job_dir).expect("job dir")]
    );
}

#[test]
fn the_task_builder_prompt_carries_the_kb_line_only_when_set() {
    let mut fx = two_task_fixture();
    fx.config.kb_path = Some("/kb/rocketapi".into());
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    fx.engine(&runner).build_stage().expect("build");
    let prompt = runner.last_spec_for("builder").expect("spec").prompt;
    assert!(prompt.contains(KB_PROMPT), "{prompt}");
    assert_eq!(
        runner.last_spec_for("builder").expect("spec").add_dirs,
        vec![
            std::path::PathBuf::from("/kb/rocketapi"),
            std::fs::canonicalize(&fx.job_dir).expect("job dir")
        ]
    );

    let off = two_task_fixture();
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    off.engine(&runner).build_stage().expect("build");
    let spec = runner.last_spec_for("builder").expect("spec");
    assert!(!spec.prompt.contains("Knowledge base"));
    assert_eq!(
        spec.add_dirs,
        vec![std::fs::canonicalize(&off.job_dir).expect("job dir")]
    );
}

#[test]
fn the_first_builder_prompt_has_no_project_wording_by_default() {
    let fx = committing_fixture();
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    fx.engine(&runner).build_stage().expect("build");
    let prompt = runner.last_spec_for("builder").expect("spec").prompt;
    assert!(
        prompt.contains(&format!("Read the plan at {}.", fx.plan_file.display())),
        "{prompt}"
    );
    for banned in ["cargo", "yarn", "Rocket", "Rules:", "CLAUDE.md"] {
        assert!(!prompt.contains(banned), "{banned} leaked: {prompt}");
    }
}

#[test]
fn the_first_builder_prompt_takes_read_first_and_rules_from_the_config() {
    let mut fx = committing_fixture();
    fx.config.read_first = vec!["CLAUDE.md".into(), "docs/rules".into()];
    fx.config.builder_rules = vec!["Run cargo check -j4.".into(), "Keep it small.".into()];
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    fx.engine(&runner).build_stage().expect("build");
    let prompt = runner.last_spec_for("builder").expect("spec").prompt;
    assert!(
        prompt.contains(&format!(
            "Read CLAUDE.md, docs/rules, and the plan at {}.",
            fx.plan_file.display()
        )),
        "{prompt}"
    );
    assert!(
        prompt.contains("Rules: Run cargo check -j4. Keep it small."),
        "{prompt}"
    );
}

#[test]
fn the_resume_prompt_names_check_hints_only_when_set() {
    for hints in [
        vec![],
        vec!["make test".to_string(), "make lint".to_string()],
    ] {
        let mut fx = committing_fixture();
        fx.config.check_hints = hints.clone();
        let runner = scripted_auto(vec![change_effect("a.txt", 3), change_effect("b.txt", 2)]);
        let e = fx.engine(&runner);
        e.build_stage().expect("first");
        std::fs::write(fx.job_dir.join("changes-r2.md"), "fix it").expect("findings");
        let mut board = factory_core::Board::load(&fx.board_path).expect("load");
        let job = board.find_mut("001").expect("job");
        job.round = 2;
        job.set_stage(Stage::Build, None, true, 1.0).expect("stage");
        board.save(&fx.board_path).expect("save");
        e.build_stage().expect("resume");
        let prompt = runner.last_spec_for("builder").expect("spec").prompt;
        if hints.is_empty() {
            assert!(
                prompt.contains("Run the relevant checks before you finish."),
                "{prompt}"
            );
        } else {
            assert!(prompt.contains("Run the relevant checks (for example `make test`, `make lint`) before you finish."), "{prompt}");
        }
    }
}

#[test]
fn the_task_builder_prompt_takes_read_first_and_rules_from_the_config() {
    let mut fx = two_task_fixture();
    fx.config.read_first = vec!["CLAUDE.md".into()];
    fx.config.builder_rules = vec!["Keep it small.".into()];
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    fx.engine(&runner).build_stage().expect("build");
    let prompt = runner.last_spec_for("builder").expect("spec").prompt;
    assert!(
        prompt.contains(&format!(
            "Read CLAUDE.md. The plan is at {}.",
            fx.plan_file.display()
        )),
        "{prompt}"
    );
    assert!(!prompt.contains("and the plan at"), "{prompt}");
    assert!(prompt.contains("Rules: Keep it small."), "{prompt}");

    let off = two_task_fixture();
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    off.engine(&runner).build_stage().expect("build");
    let prompt = runner.last_spec_for("builder").expect("spec").prompt;
    assert!(
        !prompt.contains("Rules:") && !prompt.contains("CLAUDE.md"),
        "{prompt}"
    );
    assert!(
        prompt.contains(&format!("The plan is at {}.", off.plan_file.display())),
        "{prompt}"
    );
    assert!(!prompt.contains("Read the plan at"), "{prompt}");
}

#[test]
fn without_an_excerpt_the_task_builder_prompt_keeps_the_read_the_plan_wording() {
    let mut fx = two_task_fixture();
    fx.config.builder_excerpt = false;
    fx.config.read_first = vec!["CLAUDE.md".into()];
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    fx.engine(&runner).build_stage().expect("build");
    let prompt = runner.last_spec_for("builder").expect("spec").prompt;
    assert!(
        prompt.contains(&format!(
            "Read CLAUDE.md, and the plan at {}.",
            fx.plan_file.display()
        )),
        "{prompt}"
    );
    assert!(!prompt.contains("Task excerpt"), "{prompt}");

    // A plan without the task heading falls back to the full-file prompt too.
    let mut fx = two_task_fixture();
    std::fs::write(&fx.plan_file, "# P\n\nno task headings here\n").expect("plan");
    fx.config.builder_excerpt = true;
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    let _ = fx.engine(&runner).build_stage();
    if let Some(spec) = runner.last_spec_for("builder") {
        assert!(!spec.prompt.contains("Task excerpt"), "{}", spec.prompt);
    }
}

const THREE_TASKS: &str = "# P\n\n### Task 1: first thing\n\nx\n\n### Task 2: second thing\n\ny\n\n### Task 3: third thing\n\nz\n";

fn three_task_fixture() -> Fixture {
    let fx = committing_fixture();
    std::fs::write(&fx.plan_file, THREE_TASKS).expect("plan");
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    board
        .find_mut("001")
        .expect("job")
        .set_stage(Stage::Build, None, false, 1.0)
        .expect("stage");
    board.save(&fx.board_path).expect("save");
    fx
}

#[test]
fn a_three_task_plan_runs_one_committer_and_leaves_no_snapshots() {
    let fx = three_task_fixture();
    let runner = scripted_auto(vec![
        change_effect("a.txt", 3),
        change_effect("b.txt", 2),
        change_effect("c.txt", 4),
    ]);
    let e = fx.engine(&runner);
    assert_eq!(
        e.build_stage().expect("task 1"),
        StageOutcome::Advanced(Stage::Build)
    );
    assert_eq!(
        e.build_stage().expect("task 2"),
        StageOutcome::Advanced(Stage::Build)
    );
    let prompt = runner.last_spec_for("builder").expect("spec").prompt;
    assert!(prompt.contains("wip: task 1 first thing"), "{prompt}");
    assert_eq!(runner.specs_for("committer").len(), 0);
    assert_eq!(
        e.build_stage().expect("task 3"),
        StageOutcome::Advanced(Stage::Review)
    );
    assert_eq!(runner.specs_for("committer").len(), 1);
    let batch = runner.last_spec_for("committer").expect("committer");
    assert!(batch.prompt.contains("Kind: batch"), "{}", batch.prompt);
    for want in [
        "Task 1: first thing",
        "Task 2: second thing",
        "Task 3: third thing",
    ] {
        assert!(batch.prompt.contains(want), "missing {want}");
    }
    let job = fx.job();
    assert_eq!(job.stage, Stage::Review);
    assert_eq!(job.task_index, 3);
    assert!(job.batch_snapshot.is_none());
    let head = job.head.clone().expect("head");
    let base = job.base.clone().expect("base");
    let commits = fx.repo.commits_above(&base, &head).expect("commits");
    assert!(!commits.is_empty());
    assert!(
        commits.iter().all(|c| !c.subject.starts_with("wip:")),
        "{commits:?}"
    );
    assert_eq!(fx.repo.tip(&job.branch).expect("tip"), head);
}

#[test]
fn an_empty_task_still_parks_with_the_batch_flag_on() {
    let fx = three_task_fixture();
    let runner = scripted_auto(vec![change_effect("a.txt", 3), silent_effect()]);
    let e = fx.engine(&runner);
    e.build_stage().expect("task 1");
    match e.build_stage().expect("task 2") {
        StageOutcome::NeedsHuman(r) => assert!(r.contains("task 2 changed nothing"), "{r}"),
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(fx.job().task_index, 1);
    assert_eq!(runner.specs_for("committer").len(), 0);
}

#[test]
fn the_flag_off_keeps_one_committer_per_task() {
    let mut fx = two_task_fixture();
    fx.config.commit_batch = false;
    let runner = scripted_auto(vec![change_effect("a.txt", 3), change_effect("b.txt", 2)]);
    let e = fx.engine(&runner);
    e.build_stage().expect("task 1");
    e.build_stage().expect("task 2");
    assert_eq!(runner.specs_for("committer").len(), 2);
    assert!(fx.job().batch_snapshot.is_none());
}

#[test]
fn a_parked_batch_resumes_on_the_next_build_call_without_new_builders() {
    let fx = two_task_fixture();
    let detach: Effect = Box::new(|spec| {
        let repo = git2::Repository::open(&spec.cwd).expect("open");
        let id = repo.head().expect("head").target().expect("target");
        repo.set_head_detached(id).expect("detach");
        "done".to_string()
    });
    let runner = scripted(vec![
        change_effect("a.txt", 3),
        change_effect("b.txt", 2),
        detach,
    ]);
    let e = fx.engine(&runner);
    e.build_stage().expect("task 1");
    match e.build_stage().expect("task 2") {
        StageOutcome::NeedsHuman(r) => assert!(r.contains("HEAD"), "{r}"),
        other => panic!("unexpected {other:?}"),
    }
    let parked = fx.job();
    assert!(parked.batch_snapshot.is_some());
    assert_eq!(parked.task_index, 2);
    // The person fixes HEAD and the next call finishes the batch.
    let wt = parked.worktree.clone().expect("worktree");
    let repo = git2::Repository::open(&wt).expect("open");
    repo.set_head(&format!("refs/heads/{}", parked.branch))
        .expect("reattach");
    let runner2 = scripted_auto(vec![]);
    let out = fx.engine(&runner2).build_stage().expect("resume");
    assert_eq!(out, StageOutcome::Advanced(Stage::Review));
    assert_eq!(runner2.specs_for("builder").len(), 0);
    let job = fx.job();
    assert!(job.batch_snapshot.is_none() && job.head.is_some());
}

/// Parks the second task's batch by detaching HEAD in the committer run. Returns the worktree path.
fn park_a_batch(fx: &Fixture) -> std::path::PathBuf {
    let detach: Effect = Box::new(|spec| {
        let repo = git2::Repository::open(&spec.cwd).expect("open");
        let id = repo.head().expect("head").target().expect("target");
        repo.set_head_detached(id).expect("detach");
        "done".to_string()
    });
    let runner = scripted(vec![
        change_effect("a.txt", 3),
        change_effect("b.txt", 2),
        detach,
    ]);
    let e = fx.engine(&runner);
    e.build_stage().expect("task 1");
    match e.build_stage().expect("task 2") {
        StageOutcome::NeedsHuman(r) => assert!(r.contains("HEAD"), "{r}"),
        other => panic!("unexpected {other:?}"),
    }
    let parked = fx.job();
    let wt = std::path::PathBuf::from(parked.worktree.clone().expect("worktree"));
    let repo = git2::Repository::open(&wt).expect("open");
    repo.set_head(&format!("refs/heads/{}", parked.branch))
        .expect("reattach");
    wt
}

#[test]
fn a_one_task_plan_makes_one_snapshot_and_one_committer_run() {
    let fx = committing_fixture();
    std::fs::write(&fx.plan_file, "# P\n\n### Task 1: only thing\n\nx\n").expect("plan");
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    board
        .find_mut("001")
        .expect("job")
        .set_stage(Stage::Build, None, false, 1.0)
        .expect("stage");
    board.save(&fx.board_path).expect("save");
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    assert_eq!(
        fx.engine(&runner).build_stage().expect("build"),
        StageOutcome::Advanced(Stage::Review)
    );
    assert_eq!(runner.specs_for("builder").len(), 1);
    let batch = runner.last_spec_for("committer").expect("committer");
    assert!(
        batch.prompt.contains("Kind: batch") && batch.prompt.contains("Task 1: only thing"),
        "{}",
        batch.prompt
    );
    assert_eq!(runner.specs_for("committer").len(), 1);
    let job = fx.job();
    assert_eq!(job.stage, Stage::Review);
    assert!(job.batch_snapshot.is_none());
    let commits = fx
        .repo
        .commits_above(
            &job.base.clone().expect("base"),
            &job.head.clone().expect("head"),
        )
        .expect("commits");
    assert!(
        commits.iter().all(|c| !c.subject.starts_with("wip:")),
        "{commits:?}"
    );
}

#[test]
fn a_batch_committer_that_fails_twice_falls_back_to_one_chore_commit_per_snapshot() {
    let fx = two_task_fixture();
    let runner = scripted(vec![
        change_effect("a.txt", 3),
        change_effect("b.txt", 2),
        silent_effect(),
        silent_effect(),
    ]);
    let e = fx.engine(&runner);
    e.build_stage().expect("task 1");
    assert_eq!(
        e.build_stage().expect("task 2"),
        StageOutcome::Advanced(Stage::Review)
    );
    assert_eq!(runner.specs_for("committer").len(), 2);
    let job = fx.job();
    assert_eq!(job.stage, Stage::Review);
    assert!(job.batch_snapshot.is_none());
    let commits = fx
        .repo
        .commits_above(
            &job.base.clone().expect("base"),
            &job.head.clone().expect("head"),
        )
        .expect("commits");
    assert_eq!(commits.len(), 2, "{commits:?}");
    assert!(
        commits.iter().all(|c| c.subject.starts_with("chore:")),
        "{commits:?}"
    );
}

#[test]
fn a_park_right_after_the_soft_reset_resumes_from_the_base() {
    let fx = two_task_fixture();
    let wt = park_a_batch(&fx);
    let job = fx.job();
    assert_eq!(
        fx.repo.tip(&job.branch).expect("tip"),
        job.base.clone().expect("base")
    );
    let repo = git2::Repository::open(&wt).expect("open");
    let staged = repo
        .statuses(None)
        .expect("statuses")
        .iter()
        .filter(|s| s.status().is_index_new())
        .count();
    assert_eq!(staged, 2);
    let runner = scripted_auto(vec![]);
    assert_eq!(
        fx.engine(&runner).build_stage().expect("resume"),
        StageOutcome::Advanced(Stage::Review)
    );
    assert_eq!(runner.specs_for("builder").len(), 0);
    assert_eq!(runner.specs_for("committer").len(), 1);
    assert!(fx.job().batch_snapshot.is_none());
}

#[test]
fn a_finished_batch_with_a_stale_flag_is_recognised_without_a_committer_run() {
    let fx = two_task_fixture();
    let wt = park_a_batch(&fx);
    // The committer finished its commit, but the driver stopped before it cleared the flag.
    commit_index(&wt, "feat(test): both tasks");
    let runner = scripted_auto(vec![]);
    assert_eq!(
        fx.engine(&runner).build_stage().expect("resume"),
        StageOutcome::Advanced(Stage::Review)
    );
    assert_eq!(runner.specs_for("committer").len(), 0);
    assert_eq!(runner.specs_for("builder").len(), 0);
    let job = fx.job();
    assert!(job.batch_snapshot.is_none() && job.head.is_some());
    assert!(
        fx.ledger().contains("batch commit already done"),
        "{}",
        fx.ledger()
    );
}

#[test]
fn a_branch_that_moved_to_other_work_parks_with_the_recovery_command() {
    let fx = two_task_fixture();
    let wt = park_a_batch(&fx);
    // A commit with a different tree than the snapshots.
    commit_file(&wt, "extra.txt", "x\n");
    let runner = scripted_auto(vec![]);
    match fx.engine(&runner).build_stage().expect("resume") {
        StageOutcome::NeedsHuman(r) => {
            let base = fx.job().base.expect("base");
            assert!(
                r.contains(&format!("git reset --soft {}", &base[..7])),
                "{r}"
            );
            assert!(r.contains("factory rework 001"), "{r}");
        }
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(runner.specs_for("committer").len(), 0);
    assert!(fx.job().batch_snapshot.is_some());
}

#[test]
fn a_stop_between_commit_groups_resumes_without_losing_or_repeating_a_commit() {
    let fx = two_task_fixture();
    let wt = park_a_batch(&fx);
    let job = fx.job();
    let snapshot = job.batch_snapshot.clone().expect("snapshot");
    // The committer made its first group and the driver stopped before the second one.
    {
        let repo = git2::Repository::open(&wt).expect("open");
        let mut index = repo.index().expect("index");
        index
            .remove_path(std::path::Path::new("b.txt"))
            .expect("unstage b");
        let tree = repo
            .find_tree(index.write_tree().expect("tree"))
            .expect("find tree");
        let sig = git2::Signature::now("t", "t@example.com").expect("sig");
        let parent = repo.head().expect("head").peel_to_commit().expect("commit");
        repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            "feat(test): add a",
            &tree,
            &[&parent],
        )
        .expect("commit");
    }
    // The half-finished batch is not mistaken for a finished one and nothing runs on top of it.
    let runner = scripted_auto(vec![]);
    match fx.engine(&runner).build_stage().expect("resume") {
        StageOutcome::NeedsHuman(r) => assert!(r.contains("git reset --soft"), "{r}"),
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(runner.specs_for("committer").len(), 0);
    assert_eq!(runner.specs_for("builder").len(), 0);
    // The recovery command from the park reason, then a rework: one committer, all the work, no duplicate.
    let base = job.base.clone().expect("base");
    let name = wt.file_name().expect("name").to_string_lossy().to_string();
    let wtree = factory_git::Worktree {
        name,
        path: wt.clone(),
        branch: job.branch.clone(),
        base: base.clone(),
    };
    fx.repo.reset_soft(&wtree, &base).expect("reset");
    let runner = scripted_auto(vec![]);
    assert_eq!(
        fx.engine(&runner).build_stage().expect("resume"),
        StageOutcome::Advanced(Stage::Review)
    );
    assert_eq!(runner.specs_for("committer").len(), 1);
    let done = fx.job();
    assert!(done.batch_snapshot.is_none());
    let head = done.head.clone().expect("head");
    assert!(fx.repo.same_tree(&head, &snapshot).expect("same tree"));
    let commits = fx.repo.commits_above(&base, &head).expect("commits");
    assert!(
        commits.iter().all(|c| !c.subject.starts_with("wip:")),
        "{commits:?}"
    );
    let subjects: std::collections::HashSet<_> =
        commits.iter().map(|c| c.subject.clone()).collect();
    assert_eq!(
        subjects.len(),
        commits.len(),
        "duplicate commit: {commits:?}"
    );
}

const EXCERPT_PLAN: &str = "# Demo Plan\n\n**Goal:** make it.\n\n## Global Constraints\n\n- Use -j4.\n\n---\n\n\
### Task 1: first thing\n\nBODY-ONE-MARKER\n\n### Task 2: second thing\n\nBODY-TWO-MARKER\n\nNext plan: none\n";
const EXCERPT_SPEC: &str =
    "# Spec\n\nGoal: ship the thing quickly.\n\n## Background\n\nBACKGROUND-MARKER\n\n\
## Acceptance criteria\n\n- ACCEPT-MARKER\n\n## Testing\n\nTESTING-MARKER\n";

fn excerpt_fixture() -> Fixture {
    let fx = committing_fixture();
    std::fs::write(&fx.plan_file, EXCERPT_PLAN).expect("plan");
    std::fs::write(fx.job_dir.join("spec.md"), EXCERPT_SPEC).expect("spec");
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    board
        .find_mut("001")
        .expect("job")
        .set_stage(Stage::Build, None, false, 1.0)
        .expect("stage");
    board.save(&fx.board_path).expect("save");
    fx
}

fn events_of(fx: &Fixture) -> String {
    std::fs::read_to_string(fx.job_dir.join("events.jsonl")).unwrap_or_default()
}

#[test]
fn a_task_builder_gets_its_own_task_the_header_and_the_spec_excerpt() {
    let fx = excerpt_fixture();
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    fx.engine(&runner).build_stage().expect("task 1");
    let prompt = runner.last_spec_for("builder").expect("spec").prompt;
    for want in [
        "Use -j4.",
        "BODY-ONE-MARKER",
        "Goal: ship the thing quickly.",
        "ACCEPT-MARKER",
        "Do only Task 1",
    ] {
        assert!(prompt.contains(want), "missing {want}: {prompt}");
    }
    for gone in ["BODY-TWO-MARKER", "BACKGROUND-MARKER", "TESTING-MARKER"] {
        assert!(!prompt.contains(gone), "unexpected {gone}: {prompt}");
    }
    assert!(
        prompt.contains(&fx.plan_file.display().to_string()),
        "{prompt}"
    );
    assert!(prompt.contains("spec.md"), "{prompt}");
    assert!(
        prompt.contains("use this excerpt instead and do not read the plan file in full"),
        "{prompt}"
    );
    assert!(
        !events_of(&fx).contains("excerpt"),
        "no fallback event when the excerpt was cut"
    );
}

/// The first builder prompt of a fixture whose plan is `plan`, with the excerpt switch as given.
fn first_builder_prompt(plan: &str, excerpt_on: bool) -> (String, String) {
    let mut fx = excerpt_fixture();
    fx.config.builder_excerpt = excerpt_on;
    std::fs::write(&fx.plan_file, plan).expect("plan");
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    fx.engine(&runner).build_stage().expect("task 1");
    let prompt = runner.last_spec_for("builder").expect("spec").prompt;
    // The fixtures differ only in the temp dir, so the paths are normalized.
    (
        prompt.replace(&fx.tmp.path().display().to_string(), "TMP"),
        events_of(&fx),
    )
}

#[test]
fn the_flag_off_gives_the_old_prompt_without_the_excerpt() {
    let (off, events) = first_builder_prompt(EXCERPT_PLAN, false);
    for gone in [
        "Task excerpt",
        "--- plan header ---",
        "--- end of excerpt ---",
        "ACCEPT-MARKER",
    ] {
        assert!(!off.contains(gone), "unexpected {gone}: {off}");
    }
    assert!(off.contains("Do only Task 1: first thing"));
    assert!(
        !events.contains("excerpt"),
        "switch off is a choice, not a fallback: {events}"
    );
}

#[test]
fn a_fenced_task_heading_does_not_cut_the_builder_excerpt_short() {
    let plan = "# P\n\n- Use -j4.\n\n### Task 1: first thing\n\nBODY-ONE-MARKER\n\n```text\n### Task 2: quoted\nNext plan: quoted\nQUOTED-MARKER\n```\n\nAFTER-FENCE-MARKER\n\n### Task 2: second thing\n\nBODY-TWO-MARKER\n";
    let (prompt, _) = first_builder_prompt(plan, true);
    for want in ["BODY-ONE-MARKER", "QUOTED-MARKER", "AFTER-FENCE-MARKER"] {
        assert!(prompt.contains(want), "missing {want}: {prompt}");
    }
    assert!(!prompt.contains("BODY-TWO-MARKER"), "{prompt}");
}

#[test]
fn a_crlf_plan_gives_the_same_excerpt() {
    let (prompt, events) = first_builder_prompt(&EXCERPT_PLAN.replace('\n', "\r\n"), true);
    assert!(
        prompt.contains("BODY-ONE-MARKER")
            && prompt.contains("Use -j4.")
            && !prompt.contains("BODY-TWO-MARKER"),
        "{prompt}"
    );
    assert!(!events.contains("excerpt"), "{events}");
}

#[test]
fn a_single_task_plan_gets_its_excerpt() {
    let plan =
        "# P\n\n- Use -j4.\n\n### Task 1: only thing\n\nBODY-ONLY-MARKER\n\nNext plan: later\n";
    let (prompt, _) = first_builder_prompt(plan, true);
    assert!(
        prompt.contains("BODY-ONLY-MARKER")
            && prompt.contains("Use -j4.")
            && !prompt.contains("later"),
        "{prompt}"
    );
}

#[test]
fn the_excerpt_prompt_is_smaller_than_the_plan_and_spec_it_replaces() {
    let big_task = format!(
        "### Task 2: second thing\n\n{}\n",
        "filler line\n".repeat(400)
    );
    let plan =
        format!("# Demo Plan\n\n- Use -j4.\n\n### Task 1: first thing\n\nbody\n\n{big_task}");
    let (prompt, _) = first_builder_prompt(&plan, true);
    // The old prompt only named the files and the builder read them in full, about plan + spec bytes.
    let replaced = plan.len() + EXCERPT_SPEC.len();
    assert!(
        prompt.len() < replaced / 2,
        "prompt {} bytes, plan and spec {replaced} bytes",
        prompt.len()
    );
    assert!(!prompt.contains("filler line"));
}

#[test]
fn a_builder_that_detaches_head_parks_before_the_snapshot() {
    let fx = two_task_fixture();
    let detach_and_change: Effect = Box::new(|spec| {
        std::fs::write(spec.cwd.join("a.txt"), "x\n").expect("write");
        let repo = git2::Repository::open(&spec.cwd).expect("open");
        let id = repo.head().expect("head").target().expect("target");
        repo.set_head_detached(id).expect("detach");
        "done".to_string()
    });
    let runner = scripted_auto(vec![detach_and_change]);
    match fx.engine(&runner).build_stage().expect("task 1") {
        StageOutcome::NeedsHuman(r) => {
            assert!(r.contains("moved HEAD off the job branch in task 1"), "{r}");
            assert!(r.contains("factory rework 001"), "{r}");
            assert!(
                r.contains("git rev-parse HEAD") && r.contains("HEAD is at "),
                "{r}"
            );
        }
        other => panic!("unexpected {other:?}"),
    }
    let job = fx.job();
    assert_eq!(job.task_index, 0);
    assert_eq!(
        fx.repo.tip(&job.branch).expect("tip"),
        job.base.clone().expect("base")
    );
}

#[test]
fn a_rework_note_left_by_a_batch_goes_to_the_ledger_and_a_fix_round_uses_the_findings() {
    let fx = two_task_fixture();
    park_a_batch(&fx);
    std::fs::write(
        fx.job_dir.join("rework-note.md"),
        "please look at the batch",
    )
    .expect("note");
    let runner = scripted_auto(vec![]);
    assert_eq!(
        fx.engine(&runner).build_stage().expect("resume"),
        StageOutcome::Advanced(Stage::Review)
    );
    assert!(!fx.job_dir.join("rework-note.md").exists());
    let ledger = fx.ledger();
    assert!(
        ledger.contains("rework note not sent to a builder")
            && ledger.contains("please look at the batch"),
        "{ledger}"
    );
    // A fix round now resumes with the findings file.
    std::fs::write(fx.job_dir.join("changes-r2.md"), "fix it").expect("findings");
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    let job = board.find_mut("001").expect("job");
    job.round = 2;
    job.set_stage(Stage::Build, None, true, 1.0).expect("stage");
    board.save(&fx.board_path).expect("save");
    let fix = scripted_auto(vec![change_effect("c.txt", 2)]);
    assert_eq!(
        fx.engine(&fix).build_stage().expect("fix round"),
        StageOutcome::Advanced(Stage::Review)
    );
    let prompt = fix.last_spec_for("builder").expect("builder").prompt;
    assert!(prompt.contains("changes-r2.md"), "{prompt}");
    assert!(!prompt.contains("please look at the batch"), "{prompt}");
}

#[test]
fn a_long_stale_note_is_cut_to_300_characters_in_the_ledger() {
    let fx = two_task_fixture();
    park_a_batch(&fx);
    std::fs::write(
        fx.job_dir.join("rework-note.md"),
        format!("{}\nTAIL-MARKER", "n".repeat(400)),
    )
    .expect("note");
    let runner = scripted_auto(vec![]);
    fx.engine(&runner).build_stage().expect("resume");
    let ledger = fx.ledger();
    assert!(
        ledger.contains(&"n".repeat(300))
            && !ledger.contains(&"n".repeat(301))
            && !ledger.contains("TAIL-MARKER"),
        "{ledger}"
    );
}

#[test]
fn builder_rulings_land_in_the_ledger_with_a_task_prefix() {
    let fx = two_task_fixture();
    let runner = scripted_auto(vec![blocked_effect(
        "a.txt",
        "Done.\nRuling: kept the old name\nRuling: used a Vec\n```\nRuling: fenced\n```\nStatus: DONE",
    )]);
    fx.engine(&runner).build_stage().expect("task 1");
    let ledger = fx.ledger();
    assert!(
        ledger.contains("Ruling: builder task 1: kept the old name"),
        "{ledger}"
    );
    assert!(
        ledger.contains("Ruling: builder task 1: used a Vec"),
        "{ledger}"
    );
    assert!(
        !ledger.contains("fenced") && !ledger.contains("Ruling: Ruling:"),
        "{ledger}"
    );
}

#[test]
fn concerns_are_recorded_for_done_with_concerns() {
    let fx = two_task_fixture();
    let runner = scripted_auto(vec![blocked_effect(
        "a.txt",
        "Done.\nStatus: DONE_WITH_CONCERNS\nThe query is slow.\nThe index may be missing.",
    )]);
    let out = fx.engine(&runner).build_stage().expect("task 1");
    assert_eq!(out, StageOutcome::Advanced(Stage::Build));
    assert_eq!(fx.job().task_index, 1);
    let ledger = fx.ledger();
    assert!(
        ledger.contains(
            "Ruling: builder task 1 concerns: The query is slow. The index may be missing."
        ),
        "{ledger}"
    );
}

fn rule(path: &str, command: &str) -> factory_core::CheckRule {
    factory_core::CheckRule {
        paths: vec![path.into()],
        commands: vec![command.into()],
    }
}

#[test]
fn each_task_writes_a_checks_file_for_its_own_range() {
    for batch in [true, false] {
        let mut fx = two_task_fixture();
        fx.config.commit_batch = batch;
        fx.config.checks = vec![rule("a.txt", "echo task-a"), rule("b.txt", "echo task-b")];
        let runner = scripted_auto(vec![change_effect("a.txt", 3), change_effect("b.txt", 2)]);
        let e = fx.engine(&runner);
        e.build_stage().expect("task 1");
        e.build_stage().expect("task 2");
        let one =
            std::fs::read_to_string(fx.job_dir.join("checks-task1.md")).expect("checks-task1.md");
        let two =
            std::fs::read_to_string(fx.job_dir.join("checks-task2.md")).expect("checks-task2.md");
        assert!(
            one.contains("PASS echo task-a") && !one.contains("task-b"),
            "{one}"
        );
        assert!(
            two.contains("PASS echo task-b") && !two.contains("task-a"),
            "{two}"
        );
        assert!(
            fx.ledger().contains("task 1 checks: pass"),
            "{}",
            fx.ledger()
        );
        assert!(
            !fx.job_dir.join("checks.md").exists(),
            "the whole-branch file is not written here"
        );
    }
}

#[test]
fn a_failing_task_check_is_a_ledger_line_not_a_park() {
    let mut fx = two_task_fixture();
    fx.config.checks = vec![rule("a.txt", "false")];
    let before_round = fx.job().round;
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    let out = fx.engine(&runner).build_stage().expect("task 1");
    assert_eq!(out, StageOutcome::Advanced(Stage::Build));
    let job = fx.job();
    assert_eq!(job.task_index, 1);
    assert_eq!(job.stage, Stage::Build);
    assert_eq!(job.round, before_round);
    let ledger = fx.ledger();
    assert!(ledger.contains("task 1 checks: fail false"), "{ledger}");
    let md = std::fs::read_to_string(fx.job_dir.join("checks-task1.md")).expect("checks-task1.md");
    assert!(md.contains("FAIL false"), "{md}");
}

#[test]
fn a_failing_check_after_task_two_is_reported_against_task_two() {
    let mut fx = two_task_fixture();
    fx.config.checks = vec![rule("a.txt", "true"), rule("b.txt", "false")];
    let runner = scripted_auto(vec![change_effect("a.txt", 3), change_effect("b.txt", 2)]);
    let e = fx.engine(&runner);
    e.build_stage().expect("task 1");
    e.build_stage().expect("task 2");
    let ledger = fx.ledger();
    assert!(ledger.contains("task 2 checks: fail false"), "{ledger}");
    assert!(ledger.contains("task 1 checks: pass"), "{ledger}");
    assert!(!ledger.contains("task 1 checks: fail"), "{ledger}");
}

#[test]
fn a_task_with_no_mapped_checks_writes_nothing() {
    let mut fx = two_task_fixture();
    fx.config.checks = vec![rule("src/**", "false")];
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    fx.engine(&runner).build_stage().expect("task 1");
    assert!(!fx.job_dir.join("checks-task1.md").exists());
    assert!(!fx.ledger().contains("task 1 checks"), "{}", fx.ledger());
}

fn assert_builder_spec_is_strict(spec: &factory_agent::AgentSpec) {
    assert!(spec.strict_mcp);
    assert_eq!(spec.tools.as_deref(), Some(factory_engine::BUILDER_TOOLS));
    assert_eq!(
        spec.tools.as_deref(),
        Some("Bash,Read,Edit,Write,Grep,Glob,Skill,Agent")
    );
    assert_eq!(spec.max_turns, Some(120));
}

#[test]
fn builder_specs_are_strict_and_name_their_tools() {
    // The first spec and the resume spec come from a plan without task headings.
    let fx = committing_fixture();
    let runner = scripted_auto(vec![change_effect("a.txt", 3), change_effect("b.txt", 2)]);
    let e = fx.engine(&runner);
    e.build_stage().expect("first");
    std::fs::write(fx.job_dir.join("rework-note.md"), "rename the helper").expect("note");
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    let job = board.find_mut("001").expect("job");
    job.round = 0;
    job.set_stage(Stage::Build, None, true, 1.0).expect("stage");
    board.save(&fx.board_path).expect("save");
    e.build_stage().expect("resume");
    let specs = runner.specs_for("builder");
    assert_eq!(specs.len(), 2);
    assert!(specs[1].resume.is_some());
    specs.iter().for_each(assert_builder_spec_is_strict);
    // The task spec comes from a plan with task headings.
    let fx = two_task_fixture();
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    fx.engine(&runner).build_stage().expect("task 1");
    assert_builder_spec_is_strict(&runner.last_spec_for("builder").expect("spec"));
}

/// The first, resume and task specs of a builder, in that order.
fn three_builder_specs() -> (Fixture, Fixture, Vec<factory_agent::AgentSpec>) {
    let fx = committing_fixture();
    let runner = scripted_auto(vec![change_effect("a.txt", 3), change_effect("b.txt", 2)]);
    let e = fx.engine(&runner);
    e.build_stage().expect("first");
    std::fs::write(fx.job_dir.join("rework-note.md"), "rename the helper").expect("note");
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    let job = board.find_mut("001").expect("job");
    job.round = 0;
    job.set_stage(Stage::Build, None, true, 1.0).expect("stage");
    board.save(&fx.board_path).expect("save");
    e.build_stage().expect("resume");
    let mut specs = runner.specs_for("builder");
    let tfx = two_task_fixture();
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    tfx.engine(&runner).build_stage().expect("task 1");
    specs.push(runner.last_spec_for("builder").expect("task spec"));
    (fx, tfx, specs)
}

#[test]
fn builder_specs_carry_the_permission_flags() {
    let (fx, tfx, specs) = three_builder_specs();
    assert_eq!(specs.len(), 3);
    assert!(specs[1].resume.is_some());
    for (spec, f) in specs.iter().zip([&fx, &fx, &tfx]) {
        let job_dir = std::fs::canonicalize(&f.job_dir).expect("canonical");
        assert_eq!(spec.permission_mode.as_deref(), Some("acceptEdits"));
        assert_eq!(
            spec.allowed_tools,
            factory_engine::builder_allowed_tools(&f.config)
        );
        assert_eq!(
            spec.disallowed_tools,
            factory_engine::builder_denied_tools(&f.job_dir, &[])
        );
        assert!(spec.add_dirs.contains(&job_dir), "{:?}", spec.add_dirs);
    }
}

#[test]
fn builder_specs_still_pass_assert_builder_spec_is_strict() {
    let (_fx, _tfx, specs) = three_builder_specs();
    specs.iter().for_each(assert_builder_spec_is_strict);
}

#[test]
fn builder_permissions_false_restores_the_old_argument_list() {
    let mut fx = committing_fixture();
    fx.config.builder_permissions = false;
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    fx.engine(&runner).build_stage().expect("build");
    let spec = runner.specs_for("builder").remove(0);
    assert_eq!(spec.permission_mode, None);
    assert!(spec.allowed_tools.is_empty());
    assert!(spec.disallowed_tools.is_empty());
    let expected: Vec<String> = [
        "-p",
        "--output-format",
        "stream-json",
        "--verbose",
        "--strict-mcp-config",
        "--model",
        "sonnet",
        "--tools",
        "Bash,Read,Edit,Write,Grep,Glob,Skill,Agent",
        "--agent",
        "factory-builder",
        "--max-turns",
        "120",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(spec.args(&factory_agent::Caps::default()), expected);
}

#[test]
fn builder_permissions_false_restores_all_three_builder_specs() {
    let mut fx = committing_fixture();
    fx.config.builder_permissions = false;
    let runner = scripted_auto(vec![change_effect("a.txt", 3), change_effect("b.txt", 2)]);
    let e = fx.engine(&runner);
    e.build_stage().expect("first");
    std::fs::write(fx.job_dir.join("rework-note.md"), "rename the helper").expect("note");
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    let job = board.find_mut("001").expect("job");
    job.round = 0;
    job.set_stage(Stage::Build, None, true, 1.0).expect("stage");
    board.save(&fx.board_path).expect("save");
    e.build_stage().expect("resume");
    let mut specs = runner.specs_for("builder");
    let mut tfx = two_task_fixture();
    tfx.config.builder_permissions = false;
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    tfx.engine(&runner).build_stage().expect("task 1");
    specs.push(runner.last_spec_for("builder").expect("task spec"));
    assert_eq!(specs.len(), 3);
    for spec in &specs {
        assert_eq!(spec.permission_mode, None);
        assert!(spec.allowed_tools.is_empty());
        assert!(spec.disallowed_tools.is_empty());
        let args = spec.args(&factory_agent::Caps::default());
        for flag in ["--permission-mode", "--allowedTools", "--disallowedTools"] {
            assert!(!args.iter().any(|a| a == flag), "{flag} in {args:?}");
        }
    }
}

#[test]
fn kb_folder_is_denied_for_edit_and_write() {
    let mut fx = committing_fixture();
    let kb = fx.tmp.path().join("kb").join("rocketapi");
    std::fs::create_dir_all(&kb).expect("mkdir kb");
    fx.config.kb_path = Some(kb.to_string_lossy().into_owned());
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    fx.engine(&runner).build_stage().expect("build");
    let spec = runner.specs_for("builder").remove(0);
    let canon = kb.canonicalize().expect("canonical").display().to_string();
    let p = canon.trim_start_matches('/');
    assert!(
        spec.disallowed_tools.contains(&format!("Edit(//{p}/**)")),
        "{:?}",
        spec.disallowed_tools
    );
    assert!(
        spec.disallowed_tools.contains(&format!("Write(//{p}/**)")),
        "{:?}",
        spec.disallowed_tools
    );
}

#[test]
fn a_path_with_a_rule_character_leaves_the_spec_unchanged() {
    for name in ["job)1", "job,1"] {
        let mut fx = committing_fixture();
        let odd = fx.tmp.path().join(name);
        std::fs::create_dir_all(&odd).expect("mkdir");
        std::fs::copy(&fx.plan_file, odd.join("plan.md")).expect("plan");
        fx.plan_file = odd.join("plan.md");
        fx.events = factory_agent::EventSink::new(odd.join("events.jsonl"), "001");
        fx.job_dir = odd;
        let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
        fx.engine(&runner).build_stage().expect("build");
        let spec = runner.specs_for("builder").remove(0);
        assert_eq!(spec.permission_mode, None, "{name}");
        assert!(spec.allowed_tools.is_empty(), "{name}");
    }
    let mut fx = committing_fixture();
    let kb = fx.tmp.path().join("kb)1");
    std::fs::create_dir_all(&kb).expect("mkdir kb");
    fx.config.kb_path = Some(kb.to_string_lossy().into_owned());
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    fx.engine(&runner).build_stage().expect("build");
    let spec = runner.specs_for("builder").remove(0);
    assert_eq!(spec.permission_mode, None);
    assert!(spec.allowed_tools.is_empty());
}

#[test]
fn glob_character_in_job_dir_leaves_the_spec_unchanged() {
    let mut fx = committing_fixture();
    let odd = fx.tmp.path().join("job[1]");
    std::fs::create_dir_all(&odd).expect("mkdir");
    std::fs::copy(&fx.plan_file, odd.join("plan.md")).expect("plan");
    fx.plan_file = odd.join("plan.md");
    fx.events = factory_agent::EventSink::new(odd.join("events.jsonl"), "001");
    fx.job_dir = odd;
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    fx.engine(&runner).build_stage().expect("build");
    let spec = runner.specs_for("builder").remove(0);
    assert_eq!(spec.permission_mode, None);
    assert!(spec.allowed_tools.is_empty());
    assert!(spec.disallowed_tools.is_empty());
}

#[test]
fn the_fix_resume_prompt_names_receiving_code_review_and_the_needs_context_rule() {
    let fx = committing_fixture();
    let runner = scripted_auto(vec![change_effect("a.txt", 3), change_effect("b.txt", 2)]);
    let e = fx.engine(&runner);
    e.build_stage().expect("first");
    std::fs::write(fx.job_dir.join("changes-r2.md"), "fix it").expect("findings");
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    let job = board.find_mut("001").expect("job");
    job.round = 2;
    job.set_stage(Stage::Build, None, true, 1.0).expect("stage");
    board.save(&fx.board_path).expect("save");
    e.build_stage().expect("resume");
    let prompt = runner.last_spec_for("builder").expect("spec").prompt;
    for needle in [
        "superpowers:receiving-code-review",
        "Read all the findings first",
        "verify each against the code",
        "one item at a time",
        "push back with a reason",
        "Status: NEEDS_CONTEXT",
    ] {
        assert!(prompt.contains(needle), "{needle}: {prompt}");
    }
}

#[test]
fn every_builder_prompt_names_subagent_driven_development() {
    let fx = two_task_fixture();
    let runner = scripted_auto(vec![
        change_effect("a.txt", 3),
        change_effect("b.txt", 2),
        change_effect("c.txt", 1),
    ]);
    let e = fx.engine(&runner);
    e.build_stage().expect("task 1");
    e.build_stage().expect("task 2");
    start_fix_round(&fx);
    e.build_stage().expect("fix");
    let specs = runner.specs_for("builder");
    assert_eq!(specs.len(), 3);
    for spec in &specs {
        assert!(
            spec.prompt
                .contains("superpowers:subagent-driven-development"),
            "{}",
            spec.prompt
        );
        assert!(!spec.prompt.contains("executing-plans"), "{}", spec.prompt);
        assert!(spec.prompt.contains(NOT_CONTROLLER), "{}", spec.prompt);
    }
    for spec in &specs[..2] {
        assert!(spec.prompt.contains("Status: DONE"), "{}", spec.prompt);
        assert!(spec.prompt.contains("Ruling:"), "{}", spec.prompt);
    }
    assert!(
        specs[0]
            .prompt
            .contains("use this excerpt instead and do not read the plan file in full"),
        "{}",
        specs[0].prompt
    );
    let whole = committing_fixture();
    let whole_runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    whole
        .engine(&whole_runner)
        .build_stage()
        .expect("whole plan");
    let prompt = whole_runner
        .last_spec_for("builder")
        .expect("builder spec")
        .prompt;
    assert!(
        prompt.contains("superpowers:subagent-driven-development"),
        "{prompt}"
    );
    assert!(prompt.contains(NOT_CONTROLLER), "{prompt}");
    assert!(prompt.contains("Status: DONE"), "{prompt}");
}

/// Every builder prompt keeps the builder an implementer, so it never runs a per-task review.
const NOT_CONTROLLER: &str =
    "You are the implementer, not the controller: do not dispatch implementer or reviewer subagents and do not run a task review.";

#[test]
fn cache_ttl_reaches_every_agent_run_as_a_claude_variable() {
    let mut fx = committing_fixture();
    fx.config.cache_ttl = Some("5m".to_string());
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    fx.engine(&runner).build_stage().expect("build");
    for role in ["builder", "committer"] {
        let specs = runner.specs_for(role);
        assert!(!specs.is_empty(), "no {role} run");
        for spec in specs {
            assert!(
                spec.env
                    .contains(&("FORCE_PROMPT_CACHING_5M".to_string(), "1".to_string())),
                "{role}: {:?}",
                spec.env
            );
        }
    }
}

#[test]
fn no_cache_ttl_leaves_the_claude_environment_alone() {
    let fx = committing_fixture();
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    fx.engine(&runner).build_stage().expect("build");
    let builder = runner.last_spec_for("builder").expect("builder run");
    assert!(
        builder
            .env
            .iter()
            .all(|(k, _)| !k.contains("PROMPT_CACHING")),
        "{:?}",
        builder.env
    );
}

#[test]
fn every_agent_run_loads_only_its_roles_plugins() {
    let mut fx = committing_fixture();
    fx.config.builder_plugins = vec!["superpowers".to_string(), "rust-analyzer-lsp".to_string()];
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    fx.engine(&runner).build_stage().expect("build");
    let builders = runner.specs_for("builder");
    assert!(!builders.is_empty(), "no builder run");
    for spec in builders {
        assert_eq!(
            spec.plugins,
            Some(vec![
                "superpowers".to_string(),
                "rust-analyzer-lsp".to_string()
            ])
        );
    }
    let committers = runner.specs_for("committer");
    assert!(!committers.is_empty(), "no committer run");
    for spec in committers {
        assert_eq!(spec.plugins, Some(vec!["dev-workflow-skills".to_string()]));
    }
}

use factory_core::board::TaskSession;

/// Leaves job 001 as a driver crash in task 1 does: a worktree, stage Build, task index 0 and a saved
/// session. Returns the branch tip recorded in the session.
fn crashed_in_task_one(fx: &Fixture, session: &str) -> String {
    // An empty script makes the runner report a failed run, so the worktree exists and nothing is committed.
    let dead = scripted(vec![]);
    fx.engine(&dead).build_stage().expect("first run");
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    let job = board.find_mut("001").expect("job");
    job.set_stage(Stage::Build, None, true, 1.0).expect("stage");
    let tip = fx.repo.tip(&job.branch).expect("tip");
    job.task_session = Some(TaskSession {
        task: 1,
        id: session.to_string(),
        tip: tip.clone(),
    });
    board.save(&fx.board_path).expect("save");
    tip
}

/// The `Task N: dispatched` lines of the ledger, in order.
fn dispatched_lines(fx: &Fixture) -> Vec<String> {
    fx.ledger()
        .lines()
        .filter(|l| l.contains(": dispatched"))
        .map(str::to_string)
        .collect()
}

#[test]
fn a_rerun_resumes_the_saved_session() {
    let fx = two_task_fixture();
    crashed_in_task_one(&fx, "sess-old");
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    let out = fx.engine(&runner).build_stage().expect("rerun");
    assert_eq!(out, StageOutcome::Advanced(Stage::Build));
    let builders = runner.specs_for("builder");
    assert_eq!(builders.len(), 1);
    assert_eq!(builders[0].resume.as_deref(), Some("sess-old"));
    assert_eq!(builders[0].agent.as_deref(), Some("factory-builder"));
    assert_eq!(builders[0].max_turns, Some(120));
    assert!(builders[0].strict_mcp);
    let job = fx.job();
    assert_eq!(job.task_index, 1);
    assert!(job.task_session.is_none());
    assert_eq!(job.builder_id.as_deref(), Some("sess-1"));
}

const INTERRUPTED: &str =
    "You were interrupted. Run git status and git diff, check what is already done, continue task";

#[test]
fn a_resume_prompt_names_the_task() {
    let fx = two_task_fixture();
    crashed_in_task_one(&fx, "sess-old");
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    fx.engine(&runner).build_stage().expect("rerun");
    let spec = runner.last_spec_for("builder").expect("spec");
    assert_eq!(
        spec.prompt,
        format!("{INTERRUPTED} 1, and do not redo finished work.")
    );

    let fx = two_task_fixture();
    let first = scripted_auto(vec![change_effect("a.txt", 3)]);
    fx.engine(&first).build_stage().expect("task 1");
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    let job = board.find_mut("001").expect("job");
    let tip = fx.repo.tip(&job.branch).expect("tip");
    job.task_session = Some(TaskSession {
        task: 2,
        id: "sess-two".into(),
        tip,
    });
    board.save(&fx.board_path).expect("save");
    let second = scripted_auto(vec![change_effect("b.txt", 2)]);
    fx.engine(&second).build_stage().expect("task 2");
    let spec = second.last_spec_for("builder").expect("spec");
    assert_eq!(spec.resume.as_deref(), Some("sess-two"));
    assert_eq!(
        spec.prompt,
        format!("{INTERRUPTED} 2, and do not redo finished work.")
    );
}

#[test]
fn the_ledger_says_resumed_or_fresh() {
    let fx = two_task_fixture();
    crashed_in_task_one(&fx, "sess-old");
    let runner = scripted_auto(vec![change_effect("a.txt", 3), change_effect("b.txt", 2)]);
    let e = fx.engine(&runner);
    e.build_stage().expect("task 1");
    e.build_stage().expect("task 2");
    let base = fx.job().base.expect("base");
    assert_eq!(
        dispatched_lines(&fx),
        vec![
            format!("Task 1: dispatched (BASE {base}, sonnet, fresh)"),
            format!("Task 1: dispatched (BASE {base}, sonnet, resumed)"),
            format!("Task 2: dispatched (BASE {base}, sonnet, fresh)"),
        ]
    );
    let tasks = factory_core::Ledger::new(fx.job_dir.join("progress.md"))
        .tasks()
        .expect("tasks");
    // The longer detail text must still parse: both tasks are found by their state labels.
    let dispatched = tasks
        .iter()
        .filter(|t| t.1 == factory_core::TaskState::Dispatched)
        .count();
    assert_eq!(dispatched, 3, "{tasks:?}");
}

#[test]
fn the_resume_is_used_once() {
    let fx = two_task_fixture();
    crashed_in_task_one(&fx, "sess-old");
    let runner = scripted_auto(vec![change_effect("a.txt", 3), change_effect("b.txt", 2)]);
    let e = fx.engine(&runner);
    e.build_stage().expect("task 1");
    e.build_stage().expect("task 2");
    let builders = runner.specs_for("builder");
    assert!(builders[1].resume.is_none());
    assert!(builders[1].prompt.contains("Do only Task 2"));
    assert_eq!(fx.job().task_index, 2);
}

#[test]
fn the_whole_plan_dispatched_line_is_unchanged() {
    let fx = committing_fixture();
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    fx.engine(&runner).build_stage().expect("build");
    let base = fx.job().base.expect("base");
    assert_eq!(
        dispatched_lines(&fx),
        vec![format!("Task 1: dispatched (BASE {base}, sonnet)")]
    );
}

use factory_agent::{AgentRun, AgentRunner, AgentSpec, StopReason};

/// Answers every builder run that resumes a session with `stop`, and passes all other runs to `inner`.
struct ResumeStops {
    inner: ScriptedRunner,
    stop: StopReason,
    resumed: std::sync::Mutex<Vec<AgentSpec>>,
}

impl AgentRunner for ResumeStops {
    fn run(&self, spec: &AgentSpec) -> AgentRun {
        if spec.role == "builder" && spec.resume.is_some() {
            self.resumed.lock().expect("resumed").push(spec.clone());
            return AgentRun {
                stop_reason: self.stop,
                ..AgentRun::failed("resume stopped")
            };
        }
        self.inner.run(spec)
    }
}

fn resume_stops(stop: StopReason, inner: ScriptedRunner) -> ResumeStops {
    ResumeStops {
        inner,
        stop,
        resumed: std::sync::Mutex::new(Vec::new()),
    }
}

#[test]
fn a_failed_resume_falls_back_to_a_fresh_builder() {
    let fx = two_task_fixture();
    crashed_in_task_one(&fx, "sess-old");
    let runner = resume_stops(
        StopReason::Failed,
        scripted_auto(vec![change_effect("a.txt", 3)]),
    );
    let out = fx.engine(&runner).build_stage().expect("rerun");
    assert_eq!(out, StageOutcome::Advanced(Stage::Build));
    let resumed = runner.resumed.lock().expect("resumed").clone();
    assert_eq!(resumed.len(), 1);
    assert_eq!(resumed[0].resume.as_deref(), Some("sess-old"));
    let fresh = runner.inner.specs_for("builder");
    assert_eq!(fresh.len(), 1);
    assert!(fresh[0].resume.is_none());
    assert!(
        fresh[0].prompt.contains("Do only Task 1"),
        "{}",
        fresh[0].prompt
    );
    let job = fx.job();
    assert_eq!(job.task_index, 1);
    assert!(job.task_session.is_none());
    let lines = dispatched_lines(&fx);
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(lines[1].ends_with(", sonnet, resumed)"), "{lines:?}");
    assert!(lines[2].ends_with(", sonnet, fresh)"), "{lines:?}");
}

/// Runs a resume that stops with `stop` and checks it parks without a fresh builder.
fn assert_resume_not_retried(stop: StopReason) {
    let fx = two_task_fixture();
    crashed_in_task_one(&fx, "sess-old");
    let runner = resume_stops(stop, scripted_auto(vec![change_effect("a.txt", 3)]));
    match fx.engine(&runner).build_stage().expect("rerun") {
        StageOutcome::NeedsHuman(r) => {
            assert!(
                r.contains(&format!("builder stopped in task 1: {stop:?}")),
                "{r}"
            );
        }
        other => panic!("unexpected {other:?}"),
    }
    let rec = factory_core::ParkRecord::read(&fx.job_dir).expect("park.json");
    assert_eq!(rec.kind, factory_core::ParkKind::BuilderStopped);
    assert!(runner.inner.specs_for("builder").is_empty());
    let lines = dispatched_lines(&fx);
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[1].ends_with("resumed)"), "{lines:?}");
    let job = fx.job();
    assert!(job.task_session.is_none());
    assert_eq!(job.task_index, 0);
}

#[test]
fn a_stalled_resume_is_not_retried() {
    assert_resume_not_retried(StopReason::Stalled);
}

#[test]
fn a_resume_that_hits_a_limit_is_not_retried() {
    assert_resume_not_retried(StopReason::BudgetStop);
    assert_resume_not_retried(StopReason::TurnLimit);
}

#[test]
fn a_failed_fallback_parks_after_one_fresh_run() {
    let fx = two_task_fixture();
    crashed_in_task_one(&fx, "sess-old");
    let runner = resume_stops(StopReason::Failed, scripted(vec![]));
    match fx.engine(&runner).build_stage().expect("rerun") {
        StageOutcome::NeedsHuman(r) => {
            assert!(r.contains("builder stopped in task 1: Failed"), "{r}");
        }
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(runner.resumed.lock().expect("resumed").len(), 1);
    let fresh = runner.inner.specs_for("builder");
    assert_eq!(fresh.len(), 1);
    assert!(fresh[0].resume.is_none());
    let lines = dispatched_lines(&fx);
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(lines[0].ends_with("fresh)"), "{lines:?}");
    assert!(lines[1].ends_with("resumed)"), "{lines:?}");
    assert!(lines[2].ends_with("fresh)"), "{lines:?}");
}

/// Edits the saved session of job 001 through the board file.
fn edit_session(fx: &Fixture, f: impl FnOnce(&mut TaskSession)) {
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    let job = board.find_mut("001").expect("job");
    f(job.task_session.as_mut().expect("session"));
    board.save(&fx.board_path).expect("save");
}

fn assert_started_fresh(fx: &Fixture, runner: &ScriptedRunner) {
    let builders = runner.specs_for("builder");
    assert_eq!(builders.len(), 1);
    assert!(builders[0].resume.is_none());
    assert!(builders[0].prompt.contains("Do only Task 1"));
    let lines = dispatched_lines(fx);
    assert!(lines.last().expect("line").ends_with("fresh)"), "{lines:?}");
    let job = fx.job();
    assert!(job.task_session.is_none());
    assert_eq!(job.task_index, 1);
}

#[test]
fn a_session_of_another_task_is_ignored() {
    let fx = two_task_fixture();
    crashed_in_task_one(&fx, "sess-old");
    edit_session(&fx, |s| s.task = 2);
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    fx.engine(&runner).build_stage().expect("rerun");
    assert_started_fresh(&fx, &runner);
}

#[test]
fn a_moved_tip_starts_fresh() {
    let fx = two_task_fixture();
    crashed_in_task_one(&fx, "sess-old");
    edit_session(&fx, |s| s.tip = "0".repeat(40));
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    fx.engine(&runner).build_stage().expect("rerun");
    assert_started_fresh(&fx, &runner);
}

#[test]
fn a_fresh_fallback_keeps_the_partial_edits() {
    let fx = two_task_fixture();
    crashed_in_task_one(&fx, "sess-old");
    let worktree = fx.job().worktree.expect("worktree");
    let partial = std::path::Path::new(&worktree).join("partial.txt");
    std::fs::write(&partial, "half\n").expect("partial");
    let seen = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = seen.clone();
    let effect: Effect = Box::new(move |spec| {
        flag.store(
            spec.cwd.join("partial.txt").exists(),
            std::sync::atomic::Ordering::SeqCst,
        );
        std::fs::write(spec.cwd.join("a.txt"), "a\n").expect("write");
        "done".to_string()
    });
    let runner = resume_stops(StopReason::Failed, scripted_auto(vec![effect]));
    fx.engine(&runner).build_stage().expect("rerun");
    assert!(seen.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(std::fs::read_to_string(&partial).expect("read"), "half\n");
    assert_eq!(fx.job().task_index, 1);
}

#[test]
fn a_tampered_resume_parks_without_a_fresh_run() {
    let fx = two_task_fixture();
    crashed_in_task_one(&fx, "sess-old");
    let plan = fx.job_dir.join("plan.md");
    let effect: Effect = Box::new(move |_| {
        std::fs::write(&plan, "# changed\n").expect("tamper");
        "done".to_string()
    });
    let runner = scripted_auto(vec![effect]);
    fx.engine(&runner).build_stage().expect("rerun");
    let rec = factory_core::ParkRecord::read(&fx.job_dir).expect("park.json");
    assert_eq!(rec.kind, factory_core::ParkKind::BuilderTampered);
    let builders = runner.specs_for("builder");
    assert_eq!(builders.len(), 1);
    assert_eq!(builders[0].resume.as_deref(), Some("sess-old"));
    assert!(fx.job().task_session.is_none());
    assert_eq!(
        std::fs::read_to_string(fx.job_dir.join("plan.md")).expect("plan"),
        TWO_TASKS
    );
}

#[test]
fn a_blocked_resume_parks_without_a_fresh_run() {
    let fx = two_task_fixture();
    crashed_in_task_one(&fx, "sess-old");
    let runner = scripted_auto(vec![blocked_effect("a.txt", "BLOCKED: no schema")]);
    fx.engine(&runner).build_stage().expect("rerun");
    let rec = factory_core::ParkRecord::read(&fx.job_dir).expect("park.json");
    assert_eq!(rec.kind, factory_core::ParkKind::BuilderBlocked);
    let builders = runner.specs_for("builder");
    assert_eq!(builders.len(), 1);
    assert_eq!(builders[0].resume.as_deref(), Some("sess-old"));
    assert_eq!(fx.job().task_index, 0);
}

use std::sync::{Arc, Mutex};

/// The board's `task_session` as the builder saw it during its run.
type Seen = Arc<Mutex<Option<TaskSession>>>;

/// A task builder that reports `id` through its session hook, notes the board's `task_session`,
/// then writes `file`.
fn session_effect(
    board: std::path::PathBuf,
    id: &'static str,
    file: &'static str,
    seen: Seen,
) -> Effect {
    Box::new(move |spec| {
        spec.on_session
            .as_ref()
            .expect("task builders get a session hook")
            .call(id);
        let job = factory_core::Board::load(&board)
            .expect("load")
            .find("001")
            .expect("job")
            .clone();
        *seen.lock().expect("seen") = job.task_session;
        std::fs::write(spec.cwd.join(file), "x\n").expect("write change");
        "done".to_string()
    })
}

#[test]
fn build_task_saves_the_session_while_the_builder_runs() {
    let fx = two_task_fixture();
    let seen: Seen = Arc::new(Mutex::new(None));
    let runner = scripted_auto(vec![session_effect(
        fx.board_path.clone(),
        "sess-live",
        "a.txt",
        Arc::clone(&seen),
    )]);
    let out = fx.engine(&runner).build_stage().expect("task 1");
    assert_eq!(out, StageOutcome::Advanced(Stage::Build));
    let base = fx.job().base.clone().expect("base");
    assert_eq!(
        *seen.lock().expect("seen"),
        Some(TaskSession {
            task: 1,
            id: "sess-live".to_string(),
            tip: base,
        })
    );
    assert_eq!(fx.job().task_session, None);
    assert_eq!(fx.job().task_index, 1);
}

#[test]
fn the_hook_writes_the_board_through_its_own_store() {
    let fx = two_task_fixture();
    let seen: Seen = Arc::new(Mutex::new(None));
    let board = fx.board_path.clone();
    let noted = Arc::clone(&seen);
    let effect: Effect = Box::new(move |spec| {
        let hook = spec.on_session.clone().expect("hook");
        std::thread::spawn(move || hook.call("sess-thread"))
            .join()
            .expect("thread");
        let job = factory_core::Board::load(&board)
            .expect("load")
            .find("001")
            .expect("job")
            .clone();
        *noted.lock().expect("seen") = job.task_session;
        std::fs::write(spec.cwd.join("a.txt"), "x\n").expect("write change");
        "done".to_string()
    });
    let runner = scripted_auto(vec![effect]);
    fx.engine(&runner).build_stage().expect("task 1");
    let base = fx.job().base.clone().expect("base");
    assert_eq!(
        *seen.lock().expect("seen"),
        Some(TaskSession {
            task: 1,
            id: "sess-thread".to_string(),
            tip: base,
        })
    );
    assert_eq!(fx.job().task_session, None);
}

#[test]
fn the_second_task_session_records_the_tip_after_the_first_commit() {
    let mut fx = two_task_fixture();
    fx.config.commit_batch = false;
    let seen: Seen = Arc::new(Mutex::new(None));
    let runner = scripted_auto(vec![
        change_effect("a.txt", 3),
        session_effect(
            fx.board_path.clone(),
            "sess-two",
            "b.txt",
            Arc::clone(&seen),
        ),
    ]);
    let e = fx.engine(&runner);
    e.build_stage().expect("task 1");
    let first = fx.repo.tip(&fx.job().branch).expect("tip after task 1");
    e.build_stage().expect("task 2");
    assert_ne!(Some(first.clone()), fx.job().base);
    assert_eq!(
        *seen.lock().expect("seen"),
        Some(TaskSession {
            task: 2,
            id: "sess-two".to_string(),
            tip: first,
        })
    );
}

#[test]
fn the_session_is_cleared_when_the_run_returns_even_if_it_parks() {
    let fx = two_task_fixture();
    let seen: Seen = Arc::new(Mutex::new(None));
    let board = fx.board_path.clone();
    let noted = Arc::clone(&seen);
    let effect: Effect = Box::new(move |spec| {
        spec.on_session.as_ref().expect("hook").call("sess-live");
        let job = factory_core::Board::load(&board)
            .expect("load")
            .find("001")
            .expect("job")
            .clone();
        *noted.lock().expect("seen") = job.task_session;
        std::fs::write(spec.cwd.join("a.txt"), "x\n").expect("write change");
        "BLOCKED: no schema".to_string()
    });
    let runner = scripted_auto(vec![effect]);
    let out = fx.engine(&runner).build_stage().expect("task 1");
    assert!(matches!(out, StageOutcome::NeedsHuman(_)), "{out:?}");
    let rec = factory_core::ParkRecord::read(&fx.job_dir).expect("park.json");
    assert_eq!(rec.kind, factory_core::ParkKind::BuilderBlocked);
    assert_eq!(seen.lock().expect("seen").as_ref().map(|s| s.task), Some(1));
    assert_eq!(fx.job().task_session, None);
    assert_eq!(fx.job().task_index, 0);
}

#[test]
fn a_failing_session_write_does_not_stop_the_run() {
    let fx = two_task_fixture();
    let seen: Seen = Arc::new(Mutex::new(None));
    let board = fx.board_path.clone();
    let noted = Arc::clone(&seen);
    let effect: Effect = Box::new(move |spec| {
        let guard = factory_engine::lock_board(
            &board.with_extension("json.lock"),
            std::time::Duration::from_secs(1),
        )
        .expect("lock");
        spec.on_session.as_ref().expect("hook").call("sess-lost");
        let job = factory_core::Board::load(&board)
            .expect("load")
            .find("001")
            .expect("job")
            .clone();
        *noted.lock().expect("seen") = job.task_session;
        drop(guard);
        std::fs::write(spec.cwd.join("a.txt"), "x\n").expect("write change");
        "done".to_string()
    });
    let runner = scripted_auto(vec![effect]);
    let out = fx
        .engine(&runner)
        .with_lock_wait(std::time::Duration::from_millis(200))
        .build_stage()
        .expect("task 1");
    assert_eq!(out, StageOutcome::Advanced(Stage::Build));
    assert_eq!(*seen.lock().expect("seen"), None);
    assert_eq!(fx.job().task_index, 1);
    assert_eq!(fx.job().task_session, None);
    assert!(fx.ledger().contains("Task 1: implementer done"));
}

#[test]
fn only_task_builders_get_a_session_hook() {
    let fx = committing_fixture();
    let effect: Effect = Box::new(|spec| {
        assert!(spec.on_session.is_none());
        std::fs::write(spec.cwd.join("a.txt"), "x\n").expect("write change");
        "done".to_string()
    });
    let runner = scripted_auto(vec![effect]);
    let out = fx.engine(&runner).build_stage().expect("build");
    assert_eq!(out, StageOutcome::Advanced(Stage::Review));
    for role in ["builder", "committer"] {
        let specs = runner.specs_for(role);
        assert!(!specs.is_empty(), "{role}");
        assert!(specs.iter().all(|s| s.on_session.is_none()), "{role}");
    }
}
