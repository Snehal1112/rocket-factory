mod common;

use std::sync::{Arc, Mutex};

use common::{committing_fixture, scripted_auto, Effect, Fixture, ScriptedRunner};
use factory_agent::{AgentRun, AgentRunner, AgentSpec};
use factory_core::{Board, Stage};
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

/// Gives each builder call its own session id and a fixed end context.
struct Tagged {
    inner: ScriptedRunner,
    end_ctx: u64,
    calls: Mutex<u32>,
    no_session: bool,
}

impl Tagged {
    fn new(inner: ScriptedRunner, end_ctx: u64) -> Self {
        Tagged {
            inner,
            end_ctx,
            calls: Mutex::new(0),
            no_session: false,
        }
    }
}

impl AgentRunner for Tagged {
    fn run(&self, spec: &AgentSpec) -> AgentRun {
        let mut run = self.inner.run(spec);
        if spec.role == "builder" {
            let mut n = self.calls.lock().expect("calls");
            *n += 1;
            run.session_id = if self.no_session {
                None
            } else {
                Some(format!("sess-{n}"))
            };
            run.end_ctx = self.end_ctx;
        }
        run
    }
}

#[test]
fn builder_last_is_set_on_advance() {
    let fx = two_task_fixture();
    let runner = Tagged::new(scripted_auto(vec![change_effect("a.txt", 3)]), 31_000);
    fx.engine(&runner).build_stage().expect("task 1");
    let last = fx.job().builder_last.expect("builder_last");
    assert_eq!(
        (last.task, last.id.as_str(), last.end_ctx),
        (1, "sess-1", 31_000)
    );
    assert!(last.ended_at > 0);
}

#[test]
fn builder_last_is_not_set_when_a_task_parks() {
    let fx = two_task_fixture();
    let blocked: Effect = Box::new(|spec| {
        std::fs::write(spec.cwd.join("a.txt"), "x\n").expect("write");
        "BLOCKED: no schema".to_string()
    });
    let runner = Tagged::new(scripted_auto(vec![blocked]), 31_000);
    let out = fx.engine(&runner).build_stage().expect("task 1");
    assert!(matches!(out, StageOutcome::NeedsHuman(_)), "{out:?}");
    assert!(fx.job().builder_last.is_none());
}

#[test]
fn builder_last_is_cleared_when_a_builder_is_dispatched() {
    let fx = two_task_fixture();
    let noted: Arc<Mutex<Option<Option<u32>>>> = Arc::new(Mutex::new(None));
    let seen = Arc::clone(&noted);
    let board_path = fx.board_path.clone();
    let second: Effect = Box::new(move |spec| {
        let board = Board::load(&board_path).expect("load");
        let last = board.find("001").expect("job").builder_last.clone();
        *seen.lock().expect("noted") = Some(last.map(|l| l.task));
        std::fs::write(spec.cwd.join("b.txt"), "y\n").expect("write");
        "done".to_string()
    });
    let runner = Tagged::new(
        scripted_auto(vec![change_effect("a.txt", 3), second]),
        31_000,
    );
    fx.engine(&runner).build_stage().expect("task 1");
    assert_eq!(fx.job().builder_last.expect("after 1").task, 1);
    fx.engine(&runner).build_stage().expect("task 2");
    assert_eq!(*noted.lock().expect("noted"), Some(None));
    assert_eq!(fx.job().builder_last.expect("after 2").task, 2);
}

#[test]
fn ended_at_is_taken_when_the_run_returns() {
    let fx = two_task_fixture();
    let runner = Tagged::new(scripted_auto(vec![change_effect("a.txt", 3)]), 1);
    let secs = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_secs()
    };
    let before = secs();
    fx.engine(&runner).build_stage().expect("task 1");
    let after = secs();
    let ended = fx.job().builder_last.expect("builder_last").ended_at;
    assert!(
        before <= ended && ended <= after,
        "{before} {ended} {after}"
    );
}

#[test]
fn a_task_without_any_session_id_sets_no_builder_last() {
    let fx = two_task_fixture();
    let mut runner = Tagged::new(scripted_auto(vec![change_effect("a.txt", 3)]), 5);
    runner.no_session = true;
    fx.engine(&runner).build_stage().expect("task 1");
    assert!(fx.job().builder_last.is_none());
}
