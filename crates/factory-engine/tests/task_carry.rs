mod common;

use common::{committing_fixture, scripted_auto, Effect, Fixture, Tagged};
use factory_agent::{AgentRun, AgentRunner, AgentSpec};
use factory_core::{Board, Stage, TaskSession};
use factory_engine::StageOutcome;

const TWO_TASKS: &str = "# P\n\n### Task 1: first thing\n\nx\n\n### Task 2: second thing\n\ny\n";

fn change_effect(file: &'static str, lines: usize) -> Effect {
    Box::new(move |spec| {
        let body: String = (0..lines).map(|n| format!("line {n}\n")).collect();
        std::fs::write(spec.cwd.join(file), body).expect("write change");
        "done".to_string()
    })
}

fn two_task_fixture() -> Fixture {
    let fx = committing_fixture();
    std::fs::write(&fx.plan_file, TWO_TASKS).expect("plan");
    let mut board = Board::load(&fx.board_path).expect("load");
    board
        .find_mut("001")
        .expect("job")
        .set_stage(Stage::Build, None, false, 1.0)
        .expect("stage");
    board.save(&fx.board_path).expect("save");
    fx
}

fn dispatched(fx: &Fixture) -> Vec<String> {
    fx.ledger()
        .lines()
        .filter(|l| l.contains(": dispatched"))
        .map(str::to_string)
        .collect()
}

fn two_effects() -> Vec<Effect> {
    vec![change_effect("a.txt", 3), change_effect("b.txt", 2)]
}

#[test]
fn task_two_continues_the_session_when_gap_and_context_allow() {
    let mut fx = two_task_fixture();
    fx.config.builder_carry_session = true;
    let runner = Tagged::new(scripted_auto(two_effects()), 30_000);
    let e = fx.engine(&runner);
    e.build_stage().expect("task 1");
    e.build_stage().expect("task 2");
    let builders = runner.inner.specs_for("builder");
    assert_eq!(builders[0].resume, None);
    assert_eq!(builders[1].resume.as_deref(), Some("sess-1"));
    assert!(
        builders[1].prompt.contains("Do only Task 2: second thing"),
        "{}",
        builders[1].prompt
    );
    assert!(
        dispatched(&fx)[1].ends_with(", continued)"),
        "{:?}",
        dispatched(&fx)
    );
}

#[test]
fn the_carried_builder_logs_agent_end_resumed() {
    let mut fx = two_task_fixture();
    fx.config.builder_carry_session = true;
    let runner = Tagged::new(scripted_auto(two_effects()), 30_000);
    let e = fx.engine(&runner);
    let (_, lines) = factory_telemetry::capture("info", || {
        e.build_stage().expect("task 1");
        e.build_stage().expect("task 2");
    });
    let ends: Vec<_> = lines
        .iter()
        .filter(|l| l["event"] == "agent.end" && l["role"] == "builder")
        .collect();
    assert_eq!(ends.len(), 2, "{lines:?}");
    assert_eq!(ends[0]["resumed"], false);
    assert_eq!(ends[1]["resumed"], true);
}

#[test]
fn the_setting_off_starts_every_task_fresh() {
    let fx = two_task_fixture();
    let runner = Tagged::new(scripted_auto(two_effects()), 30_000);
    let e = fx.engine(&runner);
    e.build_stage().expect("task 1");
    e.build_stage().expect("task 2");
    let builders = runner.inner.specs_for("builder");
    assert_eq!(builders[1].resume, None);
    assert!(
        dispatched(&fx)[1].ends_with(", fresh)"),
        "{:?}",
        dispatched(&fx)
    );
}

#[test]
fn a_stale_gap_starts_fresh_and_names_the_reason() {
    let mut fx = two_task_fixture();
    fx.config.builder_carry_session = true;
    let runner = Tagged::new(scripted_auto(two_effects()), 30_000);
    let e = fx.engine(&runner);
    e.build_stage().expect("task 1");
    let mut board = Board::load(&fx.board_path).expect("load");
    board
        .find_mut("001")
        .expect("job")
        .builder_last
        .as_mut()
        .expect("builder_last")
        .ended_at = 0;
    board.save(&fx.board_path).expect("save");
    e.build_stage().expect("task 2");
    assert_eq!(runner.inner.specs_for("builder")[1].resume, None);
    let line = dispatched(&fx)[1].clone();
    assert!(line.contains(", fresh (idle"), "{line}");
    assert!(line.contains("over 240s"), "{line}");
}

#[test]
fn an_oversized_context_starts_fresh() {
    let mut fx = two_task_fixture();
    fx.config.builder_carry_session = true;
    let runner = Tagged::new(scripted_auto(two_effects()), 150_000);
    let e = fx.engine(&runner);
    e.build_stage().expect("task 1");
    e.build_stage().expect("task 2");
    assert_eq!(runner.inner.specs_for("builder")[1].resume, None);
    assert!(
        dispatched(&fx)[1].ends_with(", fresh (context 150000 over 45000))"),
        "{:?}",
        dispatched(&fx)
    );
}

#[test]
fn a_rework_note_starts_fresh() {
    let mut fx = two_task_fixture();
    fx.config.builder_carry_session = true;
    let runner = Tagged::new(scripted_auto(two_effects()), 30_000);
    let e = fx.engine(&runner);
    e.build_stage().expect("task 1");
    std::fs::write(fx.job_dir.join("rework-note.md"), "mind the schema").expect("note");
    e.build_stage().expect("task 2");
    let builders = runner.inner.specs_for("builder");
    assert_eq!(builders[1].resume, None);
    assert!(builders[1].prompt.contains("mind the schema"));
    assert!(
        dispatched(&fx)[1].ends_with(", fresh (rework note))"),
        "{:?}",
        dispatched(&fx)
    );
}

#[test]
fn task_one_is_never_carried() {
    let mut fx = two_task_fixture();
    fx.config.builder_carry_session = true;
    let runner = Tagged::new(scripted_auto(two_effects()), 30_000);
    fx.engine(&runner).build_stage().expect("task 1");
    assert_eq!(runner.inner.specs_for("builder")[0].resume, None);
    assert!(
        dispatched(&fx)[0].ends_with(", fresh (first task))"),
        "{:?}",
        dispatched(&fx)
    );
}

#[test]
fn the_whole_plan_builder_is_never_carried() {
    let mut fx = committing_fixture();
    fx.config.builder_carry_session = true;
    let runner = Tagged::new(scripted_auto(vec![change_effect("a.txt", 3)]), 30_000);
    fx.engine(&runner).build_stage().expect("build");
    assert_eq!(runner.inner.specs_for("builder")[0].resume, None);
}

#[test]
fn the_carried_spec_has_permissions_agent_tools_and_turns() {
    let mut fx = two_task_fixture();
    fx.config.builder_carry_session = true;
    let runner = Tagged::new(scripted_auto(two_effects()), 30_000);
    let e = fx.engine(&runner);
    e.build_stage().expect("task 1");
    e.build_stage().expect("task 2");
    let b = runner.inner.specs_for("builder");
    assert_eq!(b[1].agent, b[0].agent);
    assert_eq!(b[1].tools, b[0].tools);
    assert_eq!(b[1].max_turns, b[0].max_turns);
    assert_eq!(b[1].strict_mcp, b[0].strict_mcp);
    assert_eq!(b[1].allowed_tools, b[0].allowed_tools);
    assert_eq!(b[1].disallowed_tools, b[0].disallowed_tools);
    assert_eq!(b[1].add_dirs, b[0].add_dirs);
    assert_eq!(b[1].model, b[0].model);
    assert_ne!(b[1].resume, b[0].resume);
}

#[test]
fn the_carried_prompt_names_the_new_task_and_overrides_later_tasks() {
    let mut fx = two_task_fixture();
    fx.config.builder_carry_session = true;
    let runner = Tagged::new(scripted_auto(two_effects()), 30_000);
    let e = fx.engine(&runner);
    e.build_stage().expect("task 1");
    e.build_stage().expect("task 2");
    let prompt = runner.inner.specs_for("builder")[1].prompt.clone();
    assert!(
        prompt.contains("Task 1 is done and committed as"),
        "{prompt}"
    );
    assert!(
        prompt.contains("The driver now gives you Task 2 in this same session"),
        "{prompt}"
    );
    assert!(
        prompt.contains("Do not redo or rework earlier tasks"),
        "{prompt}"
    );
    assert!(!prompt.contains("clean"), "{prompt}");
}

/// Answers a resumed builder run with a failure, after the inner runner has recorded the spec.
struct FailResume {
    inner: Tagged,
}

impl AgentRunner for FailResume {
    fn run(&self, spec: &AgentSpec) -> AgentRun {
        let run = self.inner.run(spec);
        if spec.role == "builder" && spec.resume.is_some() {
            return AgentRun::failed("no conversation");
        }
        run
    }
}

fn noop_effect() -> Effect {
    Box::new(|_spec| "done".to_string())
}

#[test]
fn a_failed_carry_gets_one_fresh_builder() {
    let mut fx = two_task_fixture();
    fx.config.builder_carry_session = true;
    let runner = FailResume {
        inner: Tagged::new(
            scripted_auto(vec![
                change_effect("a.txt", 3),
                noop_effect(),
                change_effect("b.txt", 2),
            ]),
            30_000,
        ),
    };
    let e = fx.engine(&runner);
    e.build_stage().expect("task 1");
    e.build_stage().expect("task 2");
    let builders = runner.inner.inner.specs_for("builder");
    assert_eq!(
        builders.len(),
        3,
        "one fresh, one failed carry, one fresh again"
    );
    assert!(builders[1].resume.is_some() && builders[2].resume.is_none());
    assert!(
        fx.ledger().contains("fresh (carry failed)"),
        "{}",
        fx.ledger()
    );
}

#[test]
fn a_crash_session_beats_a_carry() {
    let mut fx = two_task_fixture();
    fx.config.builder_carry_session = true;
    let runner = Tagged::new(scripted_auto(two_effects()), 30_000);
    let e = fx.engine(&runner);
    e.build_stage().expect("task 1");
    let mut board = Board::load(&fx.board_path).expect("load");
    let job = board.find_mut("001").expect("job");
    let tip = fx.repo.tip(&job.branch).expect("tip");
    job.task_session = Some(TaskSession {
        task: 2,
        id: "sess-crash".into(),
        tip,
    });
    board.save(&fx.board_path).expect("save");
    e.build_stage().expect("task 2");
    let spec = runner.inner.last_spec_for("builder").expect("spec");
    assert_eq!(spec.resume.as_deref(), Some("sess-crash"));
    assert!(
        spec.prompt.contains("You were interrupted"),
        "{}",
        spec.prompt
    );
    assert!(
        dispatched(&fx)[1].ends_with(", resumed)"),
        "{:?}",
        dispatched(&fx)
    );
}

#[test]
fn a_tampering_carried_builder_parks_without_a_second_run() {
    let mut fx = two_task_fixture();
    fx.config.builder_carry_session = true;
    let plan = fx.job_dir.join("plan.md");
    let tamper: Effect = Box::new(move |spec| {
        std::fs::write(spec.cwd.join("b.txt"), "line 0\n").expect("write change");
        let mut text = std::fs::read_to_string(&plan).expect("read");
        text.push_str("tampered\n");
        std::fs::write(&plan, text).expect("write");
        "done".to_string()
    });
    let runner = Tagged::new(
        scripted_auto(vec![change_effect("a.txt", 3), tamper]),
        30_000,
    );
    let e = fx.engine(&runner);
    e.build_stage().expect("task 1");
    match e.build_stage().expect("task 2") {
        StageOutcome::NeedsHuman(r) => assert!(r.contains("builder changed plan.md"), "{r}"),
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(runner.inner.specs_for("builder").len(), 2);
}

#[test]
fn a_fix_round_after_a_chain_resumes_the_last_session() {
    let mut fx = two_task_fixture();
    fx.config.builder_carry_session = true;
    let mut effects = two_effects();
    effects.push(change_effect("c.txt", 2));
    let runner = Tagged::new(scripted_auto(effects), 30_000);
    let e = fx.engine(&runner);
    e.build_stage().expect("task 1");
    e.build_stage().expect("task 2");
    std::fs::write(fx.job_dir.join("changes-r2.md"), "fix it").expect("findings");
    let mut board = Board::load(&fx.board_path).expect("load");
    let job = board.find_mut("001").expect("job");
    job.round = 2;
    job.set_stage(Stage::Build, None, true, 1.0).expect("stage");
    board.save(&fx.board_path).expect("save");
    e.build_stage().expect("fix round");
    let spec = runner.inner.last_spec_for("builder").expect("spec");
    assert_eq!(spec.resume.as_deref(), Some("sess-2"));
    assert!(spec.prompt.contains("changes-r2"), "{}", spec.prompt);
}

#[test]
fn the_batch_snapshot_and_reset_work_with_a_carry() {
    let mut fx = two_task_fixture();
    fx.config.builder_carry_session = true;
    fx.config.commit_batch = true;
    let runner = Tagged::new(scripted_auto(two_effects()), 30_000);
    let e = fx.engine(&runner);
    e.build_stage().expect("task 1");
    e.build_stage().expect("task 2");
    let job = fx.job();
    assert_eq!(job.stage, Stage::Review);
    assert!(job.batch_snapshot.is_none());
    assert!(job.head.is_some());
}

#[test]
fn batch_snapshot_logs_commit_done_with_kind_snapshot() {
    let mut fx = two_task_fixture();
    fx.config.builder_carry_session = true;
    fx.config.commit_batch = true;
    let runner = Tagged::new(scripted_auto(two_effects()), 30_000);
    let e = fx.engine(&runner);
    let (_, lines) = factory_telemetry::capture("info", || {
        e.build_stage().expect("task 1");
        e.build_stage().expect("task 2");
    });
    let snaps: Vec<_> = lines
        .iter()
        .filter(|l| l["event"] == "commit.done" && l["kind"] == "snapshot")
        .collect();
    assert_eq!(snaps.len(), 2, "{lines:?}");
    for s in snaps {
        assert_eq!(s["fallback"], false);
        let sha = s["sha"].as_str().expect("sha");
        assert_eq!(sha.len(), 40);
        assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));
    }
}

#[test]
fn a_checkbox_only_plan_runs_one_whole_plan_builder_and_never_carries() {
    // The task loop reads `### Task N:` headings only, so a checkbox plan never reaches the carry.
    let mut fx = two_task_fixture();
    fx.config.builder_carry_session = true;
    std::fs::write(&fx.plan_file, "# P\n\n- [ ] Task 1: a\n- [ ] Task 2: b\n").expect("plan");
    let runner = Tagged::new(scripted_auto(vec![change_effect("a.txt", 3)]), 30_000);
    fx.engine(&runner).build_stage().expect("build");
    let b = runner.inner.specs_for("builder");
    assert_eq!(b.len(), 1);
    assert_eq!(b[0].resume, None);
}

#[test]
fn builder_last_survives_a_carried_advance() {
    let mut fx = two_task_fixture();
    fx.config.builder_carry_session = true;
    let runner = Tagged::new(scripted_auto(two_effects()), 30_000);
    let e = fx.engine(&runner);
    e.build_stage().expect("task 1");
    e.build_stage().expect("task 2");
    let job = fx.job();
    let last = job.builder_last.expect("builder_last");
    assert_eq!(last.task, 2);
    assert_eq!(last.id, "sess-2");
}

fn events<'a>(lines: &'a [serde_json::Value], name: &str) -> Vec<&'a serde_json::Value> {
    lines.iter().filter(|l| l["event"] == name).collect()
}

#[test]
fn task_start_and_end_are_logged_per_task() {
    let fx = two_task_fixture();
    let runner = Tagged::new(scripted_auto(two_effects()), 30_000);
    let e = fx.engine(&runner);
    let (_, lines) = factory_telemetry::capture("info", || {
        e.build_stage().expect("task 1");
        e.build_stage().expect("task 2");
    });
    let starts = events(&lines, "task.start");
    assert_eq!(starts.len(), 2, "{lines:?}");
    assert_eq!(starts[0]["task"], 1);
    assert_eq!(starts[1]["task"], 2);
    assert_eq!(starts[0]["title"], "first thing");
    assert_eq!(starts[1]["title"], "second thing");
    assert_eq!(starts[0]["of"], 2);
    assert_eq!(starts[0]["how"], "fresh");
    assert_eq!(starts[0]["model"], fx.config.builder_model.as_str());
    let ends = events(&lines, "task.end");
    assert_eq!(ends.len(), 2, "{lines:?}");
    for end in &ends {
        assert_eq!(end["of"], 2);
        let sha = end["sha"].as_str().expect("sha");
        assert_eq!(sha.len(), 40);
        assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(!end["duration_ms"].is_null());
    }
    assert_ne!(ends[0]["sha"], ends[1]["sha"]);
    // The batch committer may rewrite the tip afterwards, so the ledger short sha is the anchor.
    let ledger = fx.ledger();
    for end in &ends {
        let sha = end["sha"].as_str().expect("sha");
        assert!(ledger.contains(&sha[..7]), "{ledger}");
    }
}

#[test]
fn parked_task_has_no_task_end() {
    let fx = two_task_fixture();
    let effects: Vec<Effect> = vec![Box::new(|_| "did nothing".to_string())];
    let runner = Tagged::new(scripted_auto(effects), 30_000);
    let e = fx.engine(&runner);
    let (_, lines) = factory_telemetry::capture("info", || {
        e.build_stage().expect("task 1");
    });
    assert_eq!(events(&lines, "task.start").len(), 1, "{lines:?}");
    assert!(events(&lines, "task.end").is_empty(), "{lines:?}");
    assert_eq!(events(&lines, "park").len(), 1, "{lines:?}");
}

#[test]
fn task_start_repeats_when_the_task_is_re_entered() {
    let fx = two_task_fixture();
    let effects: Vec<Effect> = vec![
        Box::new(|_| "did nothing".to_string()),
        change_effect("a.txt", 3),
    ];
    let runner = Tagged::new(scripted_auto(effects), 30_000);
    let e = fx.engine(&runner);
    let (_, lines) = factory_telemetry::capture("info", || {
        e.build_stage().expect("parks");
        let mut board = Board::load(&fx.board_path).expect("load");
        board
            .find_mut("001")
            .expect("job")
            .set_stage(Stage::Build, None, true, 2.0)
            .expect("stage");
        board.save(&fx.board_path).expect("save");
        e.build_stage().expect("task 1 again");
    });
    let starts = events(&lines, "task.start");
    assert_eq!(starts.len(), 2, "{lines:?}");
    assert!(starts.iter().all(|s| s["task"] == 1));
    assert_eq!(events(&lines, "task.end").len(), 1, "{lines:?}");
}

#[test]
fn task_start_title_is_one_line_and_capped() {
    let fx = two_task_fixture();
    std::fs::write(
        &fx.plan_file,
        format!("# P\n\n### Task 1: {}\n\nx\n", "x".repeat(400)),
    )
    .expect("plan");
    let runner = Tagged::new(scripted_auto(vec![change_effect("a.txt", 3)]), 30_000);
    let e = fx.engine(&runner);
    let (_, lines) = factory_telemetry::capture("info", || {
        e.build_stage().expect("task 1");
    });
    let starts = events(&lines, "task.start");
    assert_eq!(starts.len(), 1, "{lines:?}");
    let title = starts[0]["title"].as_str().expect("title");
    assert!(!title.contains('\n'));
    assert!(title.chars().count() <= 300, "{}", title.len());
}
