mod common;

use common::{
    amend_head, commit_file, committing_fixture, scripted, scripted_auto, Effect, Fixture, ScriptedRunner,
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
    assert!(rec.parts[1].text.contains("big.txt"), "{}", rec.parts[1].text);
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
        fx.board_path.clone(), fx.job_dir.clone(), "001".into(), fx.plan_file.clone(),
        &fx.repo, &runner, fx.config.clone(), &fx.events,
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
    let mut fx = committing_fixture();
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
    let mut fx = committing_fixture();
    fx.config.checks = vec![factory_core::CheckRule { paths: vec!["src/**".into()], commands: vec!["false".into()] }];
    let runner = built(&fx);
    fx.engine(&runner).checks_stage().expect("checks");
    let md = std::fs::read_to_string(fx.job_dir.join("checks.md")).expect("checks.md");
    assert!(md.contains("PASS none"), "{md}");
}

const TWO_TASKS: &str = "# P\n\n### Task 1: first thing\n\nx\n\n### Task 2: second thing\n\ny\n";

fn two_task_fixture() -> Fixture {
    let fx = committing_fixture();
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
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
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
    let spec = runner.last_spec_for("builder").expect("spec");
    assert!(spec.prompt.contains("Do only Task 1"), "{}", spec.prompt);
    assert!(!spec.prompt.contains("Do only Task 2"), "{}", spec.prompt);
    assert_eq!(spec.agent.as_deref(), Some("factory-builder"));
    assert_eq!(spec.max_turns, Some(120));
}

#[test]
fn the_last_task_call_records_head_and_moves_to_review() {
    let fx = two_task_fixture();
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
    assert!(job.head.is_some() && job.head != job.base && job.head.as_deref() != Some(first.as_str()));
    let ledger = ledger_of(&fx);
    assert!(ledger.contains("Task 2: dispatched"), "{ledger}");
    assert!(ledger.contains("Task 2: implementer done"), "{ledger}");
    assert_eq!(ledger.matches("per-task builders").count(), 1, "{ledger}");
    let prompt = runner.last_spec_for("builder").expect("spec").prompt;
    assert!(prompt.contains("Do only Task 2"), "{prompt}");
    assert!(prompt.contains(&first[..7]) && prompt.contains("build step"), "{prompt}");
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
    assert!(rec.reason.contains("task 2 changed nothing"), "{}", rec.reason);
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
    assert!(!ledger_of(&fx).contains("per-task builders"));
}

#[test]
fn a_fix_round_resumes_the_session_and_does_not_restart_the_task_loop() {
    let fx = two_task_fixture();
    let runner = scripted_auto(vec![change_effect("a.txt", 3), change_effect("b.txt", 2), change_effect("c.txt", 1)]);
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
    let rec = factory_core::ParkRecord::read(&fx.job_dir).expect("park.json");
    assert_eq!(rec.kind, factory_core::ParkKind::BuilderStopped);
    assert!(rec.parts[0].text.contains("Stop reason:"), "{}", rec.parts[0].text);
    assert_eq!(rec.next, vec!["factory rework 001 <note>".to_string()]);
    assert_eq!(fx.job().task_index, 0);
    // A rework puts the job back in build and leaves a note.
    std::fs::write(fx.job_dir.join("rework-note.md"), "use the small helper").expect("note");
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    board.find_mut("001").expect("job").set_stage(Stage::Build, None, true, 1.0).expect("stage");
    board.save(&fx.board_path).expect("save");
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    let out = fx.engine(&runner).build_stage().expect("task 1 again");
    assert_eq!(out, StageOutcome::Advanced(Stage::Build));
    let spec = runner.last_spec_for("builder").expect("spec");
    assert!(spec.prompt.contains("use the small helper"), "{}", spec.prompt);
    assert!(spec.prompt.contains("Do only Task 1"), "{}", spec.prompt);
    assert_eq!(fx.job().task_index, 1);
    assert!(!fx.job_dir.join("rework-note.md").exists());
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
    let fx = two_task_fixture();
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    fx.engine(&runner).build_stage().expect("task 1");
    let prompt = runner.last_spec_for("committer").expect("committer spec").prompt;
    assert!(prompt.contains("Kind: task"), "{prompt}");
    assert!(prompt.contains("Task: first thing"), "{prompt}");
    assert!(prompt.contains("Plan: P"), "{prompt}");
    assert!(prompt.contains(&format!("Floor: {}", fx.job().base.expect("base"))), "{prompt}");
    assert!(prompt.contains("new file:   a.txt"), "{prompt}");
}

#[test]
fn a_whole_plan_build_commits_as_legacy() {
    let fx = committing_fixture();
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    fx.engine(&runner).build_stage().expect("build");
    let prompt = runner.last_spec_for("committer").expect("committer spec").prompt;
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
    let prompt = runner.last_spec_for("committer").expect("committer spec").prompt;
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
    let second = scripted(vec![change_effect("b.txt", 2), amend_effect("feat(test): reword the reviewed commit")]);
    match fx.engine(&second).build_stage().expect("fix") {
        StageOutcome::NeedsHuman(r) => assert!(r.contains("at or below the floor"), "{r}"),
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(fx.job().stage, Stage::NeedsHuman);
}

#[test]
fn a_committer_that_rewrites_an_earlier_task_commit_leaves_a_ruling() {
    let fx = two_task_fixture();
    let first = scripted_auto(vec![change_effect("a.txt", 3)]);
    fx.engine(&first).build_stage().expect("task 1");
    let second = scripted(vec![change_effect("b.txt", 2), amend_effect("feat(test): tasks one and two")]);
    let out = fx.engine(&second).build_stage().expect("task 2");
    assert_eq!(out, StageOutcome::Advanced(Stage::Review));
    let ledger = ledger_of(&fx);
    assert!(ledger.contains("task 2 commit rewrote earlier commits"), "{ledger}");
}

#[test]
fn the_builder_prompts_tell_it_not_to_commit() {
    let fx = two_task_fixture();
    let runner = scripted_auto(vec![change_effect("a.txt", 3), change_effect("b.txt", 2), change_effect("c.txt", 1)]);
    let e = fx.engine(&runner);
    e.build_stage().expect("task 1");
    e.build_stage().expect("task 2");
    start_fix_round(&fx);
    e.build_stage().expect("fix");
    let specs = runner.specs_for("builder");
    assert_eq!(specs.len(), 3);
    for spec in &specs {
        assert!(spec.prompt.contains("Do not run `git add` or `git commit`"), "{}", spec.prompt);
        assert!(!spec.prompt.contains("1-git-commit"), "{}", spec.prompt);
        assert!(!spec.prompt.contains("HEAD: <sha"), "{}", spec.prompt);
    }
    for spec in &specs[..2] {
        assert!(spec.prompt.contains("Skip every commit step of the plan"), "{}", spec.prompt);
    }
    let whole = committing_fixture();
    let whole_runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    whole.engine(&whole_runner).build_stage().expect("whole plan");
    let prompt = whole_runner.last_spec_for("builder").expect("builder spec").prompt;
    assert!(prompt.contains("Do not run `git add` or `git commit`"), "{prompt}");
    assert!(!prompt.contains("1-git-commit"), "{prompt}");
    assert!(prompt.contains("Skip every commit step of the plan"), "{prompt}");
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
    assert_eq!(runner.last_spec_for("builder").expect("spec").add_dirs, vec![std::path::PathBuf::from("/kb/rocketapi")]);
    std::fs::write(fx.job_dir.join("changes-r2.md"), "fix it").expect("findings");
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    let job = board.find_mut("001").expect("job");
    job.round = 2;
    job.set_stage(Stage::Build, None, true, 1.0).expect("stage");
    board.save(&fx.board_path).expect("save");
    e.build_stage().expect("resume");
    let resume = runner.last_spec_for("builder").expect("spec").prompt;
    assert!(resume.contains(KB_PROMPT), "{resume}");
    assert_eq!(runner.last_spec_for("builder").expect("spec").add_dirs, vec![std::path::PathBuf::from("/kb/rocketapi")]);

    let off = committing_fixture();
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    off.engine(&runner).build_stage().expect("first");
    let spec = runner.last_spec_for("builder").expect("spec");
    assert!(!spec.prompt.contains("Knowledge base"));
    assert!(spec.add_dirs.is_empty());
}

#[test]
fn the_task_builder_prompt_carries_the_kb_line_only_when_set() {
    let mut fx = two_task_fixture();
    fx.config.kb_path = Some("/kb/rocketapi".into());
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    fx.engine(&runner).build_stage().expect("build");
    let prompt = runner.last_spec_for("builder").expect("spec").prompt;
    assert!(prompt.contains(KB_PROMPT), "{prompt}");
    assert_eq!(runner.last_spec_for("builder").expect("spec").add_dirs, vec![std::path::PathBuf::from("/kb/rocketapi")]);

    let off = two_task_fixture();
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    off.engine(&runner).build_stage().expect("build");
    let spec = runner.last_spec_for("builder").expect("spec");
    assert!(!spec.prompt.contains("Knowledge base"));
    assert!(spec.add_dirs.is_empty());
}
