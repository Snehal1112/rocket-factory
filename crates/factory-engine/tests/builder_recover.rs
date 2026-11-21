mod common;

use std::sync::Arc;

use common::{any, failed_run, scripted, BuilderScript, Effect, Fixture, Step};
use factory_agent::AgentRun;
use factory_core::{Board, BuilderLast, Stage};

const TWO_TASKS: &str = "# P\n\n### Task 1: first thing\n\nx\n\n### Task 2: second thing\n\ny\n";

fn change_effect() -> Effect {
    Box::new(|spec| {
        std::fs::write(spec.cwd.join("a.txt"), "x\n").expect("write change");
        "done".to_string()
    })
}

fn nothing_effect() -> Effect {
    Box::new(|_| "done".to_string())
}

fn note_path(fx: &Fixture) -> std::path::PathBuf {
    fx.job_dir.join("rework-note.md")
}

fn step(run: Option<AgentRun>) -> Step {
    Step { when: any, run }
}

fn task_fixture() -> Fixture {
    let fx = Fixture::new();
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

#[test]
fn failed_run_keeps_session_fields() {
    let fx = Fixture::new();
    let first = scripted(vec![change_effect()]);
    fx.engine(&first).build_stage().expect("first build");
    let last = BuilderLast {
        task: 1,
        id: "sess-old".into(),
        ended_at: 5,
        end_ctx: 1000,
    };
    let want = last.clone();
    factory_engine::BoardStore::new(&fx.board_path, std::time::Duration::from_secs(5))
        .update_job("001", move |j| {
            j.round = 2;
            j.builder_id = Some("sess-old".into());
            j.builder_session_model = Some("m".into());
            j.builder_last = Some(last);
            Ok(())
        })
        .expect("set fix round");
    std::fs::write(fx.job_dir.join("changes-r2.md"), "- fix it\n").expect("findings");
    let before = fx.job();
    let script = BuilderScript::new(
        Arc::new(scripted(vec![])),
        vec![
            step(Some(failed_run("boom", 3, 10, 10))),
            step(Some(failed_run("boom", 3, 10, 10))),
            step(Some(failed_run("boom", 3, 10, 10))),
        ],
    );
    let _ = fx.engine(&script).build_stage();
    let after = fx.job();
    assert_eq!(after.builder_id, before.builder_id);
    assert_eq!(after.builder_id.as_deref(), Some("sess-old"));
    assert_eq!(after.builder_session_model.as_deref(), Some("m"));
    assert_eq!(after.builder_last, Some(want));
    assert_eq!(after.builder_last, before.builder_last);
}

/// How the builder run ends in one case of the note test.
fn ending(case: &str) -> (Option<AgentRun>, Vec<Effect>) {
    match case {
        "failed" => (Some(failed_run("boom", 3, 10, 10)), vec![]),
        "blocked" => (Some(AgentRun::done("STATUS: BLOCKED: no way")), vec![]),
        "needs-context" => (
            Some(AgentRun::done("STATUS: NEEDS_CONTEXT: which one")),
            vec![],
        ),
        "nothing" => (None, vec![nothing_effect()]),
        _ => (None, vec![change_effect()]),
    }
}

fn run_case(fx: &Fixture, case: &str) {
    let (run, effects) = ending(case);
    let steps = (0..3).map(|_| step(run.clone())).collect();
    let script = BuilderScript::new(Arc::new(scripted(effects)), steps);
    let _ = fx.engine(&script).build_stage();
}

#[test]
fn rework_note_survives_runs_that_do_not_advance() {
    for case in ["failed", "blocked", "needs-context", "nothing"] {
        // Whole-plan path.
        let fx = Fixture::new();
        std::fs::write(note_path(&fx), "mind the schema").expect("note");
        run_case(&fx, case);
        assert!(note_path(&fx).exists(), "whole plan, {case}");
        // Per-task path.
        let fx = task_fixture();
        std::fs::write(note_path(&fx), "mind the schema").expect("note");
        run_case(&fx, case);
        assert!(note_path(&fx).exists(), "per task, {case}");
    }
}

#[test]
fn rework_note_is_removed_when_the_pass_advances() {
    let fx = Fixture::new();
    std::fs::write(note_path(&fx), "mind the schema").expect("note");
    run_case(&fx, "advance");
    assert!(!note_path(&fx).exists(), "whole plan");
    let fx = task_fixture();
    std::fs::write(note_path(&fx), "mind the schema").expect("note");
    run_case(&fx, "advance");
    assert!(!note_path(&fx).exists(), "per task");
}
