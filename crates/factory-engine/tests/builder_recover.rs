mod common;

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{any, failed_run, scripted, BuilderScript, Effect, Fixture, Step};
use factory_agent::AgentRun;
use factory_core::{Board, BuilderLast, Stage};
use factory_engine::{EngineError, LostFlag, LostReason, MachineWait, StageOutcome};

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

/// The fake clock starts here, so the elapsed time is the reading minus this.
const T0: f64 = 1000.0;

/// A fake machine: a counter clock that `sleep` advances, a free-space answer by elapsed seconds, and a
/// hook run on each sleep with its 1-based call number and the elapsed seconds after it.
struct FakeMachine {
    wait: MachineWait,
    /// The elapsed seconds of every `free_mb` call.
    disk_calls: Arc<Mutex<Vec<f64>>>,
}

fn fake_machine(
    free: impl Fn(f64) -> u64 + Send + Sync + 'static,
    on_sleep: impl Fn(u32, f64) + Send + Sync + 'static,
) -> FakeMachine {
    let clock = Arc::new(Mutex::new(T0));
    let disk_calls = Arc::new(Mutex::new(Vec::new()));
    let sleeps = Arc::new(AtomicU32::new(0));
    let (c1, c2, c3) = (clock.clone(), clock.clone(), clock);
    let calls = disk_calls.clone();
    let wait = MachineWait {
        free_mb: Arc::new(move |_| {
            let elapsed = *c1.lock().expect("clock") - T0;
            calls.lock().expect("calls").push(elapsed);
            Some(free(elapsed))
        }),
        sleep: Arc::new(move |d: Duration| {
            assert!(d <= Duration::from_secs(5), "sleep step over 5 s: {d:?}");
            let elapsed = {
                let mut now = c2.lock().expect("clock");
                *now += d.as_secs_f64();
                *now - T0
            };
            let n = sleeps.fetch_add(1, Ordering::SeqCst) + 1;
            on_sleep(n, elapsed);
        }),
        now: Arc::new(move || *c3.lock().expect("clock")),
    };
    FakeMachine { wait, disk_calls }
}

/// A whole-plan fixture at Build that checks for 2048 MB free.
fn disk_fixture() -> Fixture {
    let mut fx = Fixture::new();
    fx.config.min_free_disk_mb = 2048;
    factory_engine::BoardStore::new(&fx.board_path, Duration::from_secs(5))
        .update_job("001", |j| j.set_stage(Stage::Build, None, false, 1.0))
        .expect("stage");
    fx
}

fn ledger(fx: &Fixture) -> String {
    std::fs::read_to_string(fx.job_dir.join("progress.md")).unwrap_or_default()
}

#[test]
fn disk_wait_starts_builder_when_disk_frees() {
    let fx = disk_fixture();
    let m = fake_machine(|t| if t > 200.0 { 10_000 } else { 100 }, |_, _| {});
    let runner = scripted(vec![change_effect()]);
    let out = fx
        .engine(&runner)
        .with_machine_wait(m.wait)
        .build_stage()
        .expect("build");
    assert!(matches!(out, StageOutcome::Advanced(_)), "{out:?}");
    assert_eq!(runner.specs_for("builder").len(), 1);
    assert_eq!(fx.job().park_kind, None);
    assert!(
        ledger(&fx).contains("machine ok after 4 min"),
        "{}",
        ledger(&fx)
    );
}

#[test]
fn disk_wait_parks_after_25_minutes() {
    let fx = disk_fixture();
    let m = fake_machine(|_| 100, |_, _| {});
    let runner = scripted(vec![change_effect()]);
    let out = fx
        .engine(&runner)
        .with_machine_wait(m.wait)
        .build_stage()
        .expect("build");
    assert!(matches!(out, StageOutcome::NeedsHuman(_)), "{out:?}");
    let job = fx.job();
    assert_eq!(job.park_kind.as_deref(), Some("machine-waiting"));
    let reason = job.reason.clone().unwrap_or_default();
    assert!(reason.contains("waited 25 min"), "{reason}");
    assert!(reason.starts_with("disk full: "), "{reason}");
    assert_eq!(job.machine_wait, None);
    assert_eq!(runner.call_count(), 0);
    let calls = m.disk_calls.lock().expect("calls").clone();
    assert_eq!(calls, vec![0.0, 60.0, 120.0, 240.0, 480.0, 900.0, 1500.0]);
}

#[test]
fn machine_wait_is_set_during_and_cleared_after() {
    let fx = disk_fixture();
    let board = fx.board_path.clone();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen_in = seen.clone();
    let m = fake_machine(
        |t| if t > 100.0 { 10_000 } else { 100 },
        move |_, _| {
            let b = Board::load(&board).expect("load");
            let wait = b.find("001").expect("job").machine_wait.clone();
            seen_in.lock().expect("seen").push(wait);
        },
    );
    let runner = scripted(vec![change_effect()]);
    fx.engine(&runner)
        .with_machine_wait(m.wait)
        .build_stage()
        .expect("build");
    let seen = seen.lock().expect("seen").clone();
    assert!(!seen.is_empty());
    for wait in &seen {
        let wait = wait.as_deref().unwrap_or_default();
        assert!(
            wait.starts_with("disk: ") && wait.ends_with("GB free"),
            "{wait}"
        );
    }
    assert_eq!(fx.job().machine_wait, None);
}

#[test]
fn machine_wait_is_cleared_at_next_build_pass() {
    let fx = Fixture::new();
    factory_engine::BoardStore::new(&fx.board_path, Duration::from_secs(5))
        .update_job("001", |j| {
            j.machine_wait = Some("network".into());
            Ok(())
        })
        .expect("stale");
    let runner = scripted(vec![change_effect()]);
    fx.engine(&runner).build_stage().expect("build");
    assert_eq!(fx.job().machine_wait, None);
}

#[test]
fn wait_ends_on_cancel() {
    let fx = disk_fixture();
    let board = fx.board_path.clone();
    let m = fake_machine(
        |_| 100,
        move |n, _| {
            if n == 2 {
                factory_engine::BoardStore::new(&board, Duration::from_secs(5))
                    .update_job("001", |j| {
                        j.set_stage(Stage::NeedsHuman, Some("cancelled: x"), false, 1.0)?;
                        j.cancelled = true;
                        Ok(())
                    })
                    .expect("cancel");
            }
        },
    );
    let runner = scripted(vec![change_effect()]);
    let out = fx.engine(&runner).with_machine_wait(m.wait).build_stage();
    assert!(matches!(out, Err(EngineError::Cancelled)), "{out:?}");
    assert_eq!(*m.disk_calls.lock().expect("calls"), vec![0.0]);
    assert_eq!(runner.call_count(), 0);
}

#[test]
fn wait_ends_on_lost_lease() {
    let fx = disk_fixture();
    let flag = Arc::new(LostFlag::new());
    let flag_in = flag.clone();
    let m = fake_machine(
        |_| 100,
        move |n, _| {
            if n == 2 {
                flag_in.set(LostReason::TakenOver);
            }
        },
    );
    let runner = scripted(vec![change_effect()]);
    let out = fx
        .engine(&runner)
        .with_lost(flag)
        .with_machine_wait(m.wait)
        .build_stage();
    let err = out.expect_err("lost lease");
    assert!(err.to_string().contains("project is busy"), "{err}");
    assert_eq!(*m.disk_calls.lock().expect("calls"), vec![0.0]);
    assert_eq!(runner.call_count(), 0);
    assert_eq!(fx.job().machine_wait, None);
}

#[test]
fn wait_survives_unwritable_ledger() {
    use std::os::unix::fs::PermissionsExt;
    let fx = disk_fixture();
    let progress = fx.job_dir.join("progress.md");
    std::fs::write(&progress, "# Progress\n").expect("ledger");
    std::fs::set_permissions(&progress, std::fs::Permissions::from_mode(0o444)).expect("ro");
    let path = progress.clone();
    let m = fake_machine(
        |t| if t >= 120.0 { 10_000 } else { 100 },
        move |_, t| {
            if t >= 120.0 {
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
                    .expect("rw");
            }
        },
    );
    let runner = scripted(vec![change_effect()]);
    let out = fx
        .engine(&runner)
        .with_machine_wait(m.wait)
        .build_stage()
        .expect("build");
    assert!(matches!(out, StageOutcome::Advanced(_)), "{out:?}");
    assert_eq!(runner.specs_for("builder").len(), 1);
    let text = ledger(&fx);
    assert!(!text.contains("waiting for the machine"), "{text}");
    assert!(text.contains("machine ok after 2 min"), "{text}");
}

#[test]
fn missing_key_file_waits_then_runs() {
    use std::os::unix::fs::PermissionsExt;
    let mut fx = disk_fixture();
    let key = fx.tmp.path().join("keys").join("anthropic.key");
    fx.config.agent_cli = Some("claude".into());
    fx.config.api_key_file = Some(key.display().to_string());
    let key_in = key.clone();
    let m = fake_machine(
        |_| 10_000,
        move |n, _| {
            if n == 1 {
                std::fs::create_dir_all(key_in.parent().expect("parent")).expect("mkdir");
                std::fs::write(&key_in, "sk-test\n").expect("key");
                std::fs::set_permissions(&key_in, std::fs::Permissions::from_mode(0o600))
                    .expect("mode");
            }
        },
    );
    let runner = scripted(vec![change_effect()]);
    let out = fx
        .engine(&runner)
        .with_machine_wait(m.wait)
        .build_stage()
        .expect("build");
    assert!(matches!(out, StageOutcome::Advanced(_)), "{out:?}");
    assert_eq!(runner.specs_for("builder").len(), 1);
    let text = ledger(&fx);
    assert!(text.contains("waiting for the machine: auth: "), "{text}");
}
