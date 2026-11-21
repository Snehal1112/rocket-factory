mod common;

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{any, failed_run, resumed, scripted, BuilderScript, Effect, Fixture, Step};
use factory_agent::{AgentRun, AgentRunner};
use factory_core::board::TaskSession;
use factory_core::{Board, BuilderLast, Config, Job, ResumeAt, Stage};
use factory_engine::{
    retry_job, run_job, Driver, EngineError, LostFlag, LostReason, MachineWait, RunEnd,
    RunnerFactory, StageOutcome,
};

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
        "blocked" => (Some(AgentRun::done("Status: BLOCKED: no way")), vec![]),
        "needs-context" => (
            Some(AgentRun::done("Status: NEEDS_CONTEXT: which one")),
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
    /// The elapsed seconds now.
    elapsed: Arc<dyn Fn() -> f64 + Send + Sync>,
}

fn fake_machine(
    free: impl Fn(f64) -> u64 + Send + Sync + 'static,
    on_sleep: impl Fn(u32, f64) + Send + Sync + 'static,
) -> FakeMachine {
    let clock = Arc::new(Mutex::new(T0));
    let disk_calls = Arc::new(Mutex::new(Vec::new()));
    let sleeps = Arc::new(AtomicU32::new(0));
    let (c1, c2, c3, c4) = (clock.clone(), clock.clone(), clock.clone(), clock);
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
    let elapsed = Arc::new(move || *c4.lock().expect("clock") - T0);
    FakeMachine {
        wait,
        disk_calls,
        elapsed,
    }
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

const OTHER: &str = "claude reported success: it broke";
const NETWORK: &str = "claude reported success: API Error: Can't reach the API server (EAI_AGAIN)";
const AUTH: &str = "claude reported success: API Error: 401 Invalid API key";
const GONE: &str =
    "claude reported error_during_execution: No conversation found with session ID: sess-1";
/// A phrase only the fresh fix round prompt holds.
const FRESH_FIX: &str = "picking up a fix round in a fresh session";
/// A phrase only the first build prompt holds.
const FIRST_BUILD: &str = "Your working directory is the job worktree, already on branch";

fn rerun_lines(fx: &Fixture) -> usize {
    ledger(fx).matches("rerunning once").count()
}

fn script(inner: Vec<Effect>, steps: Vec<Step>) -> BuilderScript {
    BuilderScript::new(Arc::new(scripted(inner)), steps)
}

#[test]
fn other_failure_reruns_once_then_parks_with_error() {
    let fx = Fixture::new();
    let s = script(
        vec![],
        vec![
            step(Some(failed_run(OTHER, 3, 50, 500))),
            step(Some(failed_run(OTHER, 3, 50, 500))),
        ],
    );
    let out = fx.engine(&s).build_stage().expect("build");
    assert!(matches!(out, StageOutcome::NeedsHuman(_)), "{out:?}");
    let job = fx.job();
    assert_eq!(job.park_kind.as_deref(), Some("builder-stopped"));
    assert_eq!(job.park_resume, Some(ResumeAt::Build));
    let reason = job.reason.clone().unwrap_or_default();
    assert!(reason.contains("it broke"), "{reason}");
    assert!(reason.starts_with("builder stopped: Failed: "), "{reason}");
    assert_eq!(s.specs().len(), 2);
    assert_eq!(rerun_lines(&fx), 1, "{}", ledger(&fx));
    assert!(
        ledger(&fx).contains("builder failed (claude reported success: it broke), rerunning once"),
        "{}",
        ledger(&fx)
    );
}

#[test]
fn other_failure_rerun_succeeds() {
    let fx = Fixture::new();
    let s = script(
        vec![change_effect()],
        vec![step(Some(failed_run(OTHER, 3, 50, 500)))],
    );
    let out = fx.engine(&s).build_stage().expect("build");
    assert_eq!(out, StageOutcome::Advanced(Stage::Review));
    assert_eq!(s.specs().len(), 2);
}

/// A driver over a new repo whose job `001` waits at Build with a one-line plan.
fn build_driver(runner: Arc<dyn AgentRunner>) -> (tempfile::TempDir, Driver) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let main = tmp.path().join("repo");
    common::init_repo(&main);
    let factory: RunnerFactory = Box::new(move |_job_id| runner.clone());
    let driver = Driver::new(
        factory_core::Layout::in_repo(&main),
        common::test_config(Config::default()),
        factory,
    )
    .expect("driver");
    let mut job = Job::new(
        "001",
        "2026-10-13-001-hello",
        "hello",
        "2026-10-13",
        "say hello",
        0.0,
    );
    job.set_stage(Stage::Build, None, false, 1.0)
        .expect("stage");
    let dir = driver.job_dir(&job);
    std::fs::create_dir_all(&dir).expect("job dir");
    std::fs::write(dir.join("plan.md"), "- [ ] Task 1: hello\n").expect("plan");
    driver.ledger(&job).create("# SDD ledger").expect("ledger");
    driver
        .store()
        .update(|b| {
            b.jobs.push(job);
            Ok(())
        })
        .expect("board");
    (tmp, driver)
}

#[test]
fn retry_after_failed_first_build_runs_first_spec() {
    let s = Arc::new(script(
        vec![change_effect()],
        vec![
            step(Some(failed_run(OTHER, 3, 50, 500))),
            step(Some(failed_run(OTHER, 3, 50, 500))),
        ],
    ));
    let (_tmp, driver) = build_driver(s.clone());
    let end = run_job(&driver, "001").expect("run");
    assert!(matches!(end, RunEnd::NeedsHuman(_)), "{end:?}");
    let job = driver.job("001").expect("job");
    assert_eq!(job.park_kind.as_deref(), Some("builder-stopped"));
    assert_eq!(job.park_resume, Some(ResumeAt::Build));
    let end = retry_job(&driver, "001");
    assert!(end.is_ok(), "{end:?}");
    let specs = s.specs();
    assert!(specs.len() >= 3, "{}", specs.len());
    assert!(specs[2].prompt.contains(FIRST_BUILD), "{}", specs[2].prompt);
    assert!(specs[2].resume.is_none());
    let job = driver.job("001").expect("job");
    let reason = job.reason.unwrap_or_default();
    assert!(!reason.contains("missing findings file"), "{reason}");
}

/// A whole-plan job in fix round 2 with the stored builder session `sess-1`.
fn fix_round_fixture() -> Fixture {
    let fx = Fixture::new();
    let first = scripted(vec![change_effect()]);
    fx.engine(&first).build_stage().expect("first build");
    factory_engine::BoardStore::new(&fx.board_path, Duration::from_secs(5))
        .update_job("001", |j| {
            j.round = 2;
            j.set_stage(Stage::Build, None, true, 1.0)
        })
        .expect("set fix round");
    std::fs::write(fx.job_dir.join("changes-r2.md"), "- fix it\n").expect("findings");
    assert_eq!(fx.job().builder_id.as_deref(), Some("sess-1"));
    fx
}

#[test]
fn failed_resume_reruns_same_session() {
    let fx = fix_round_fixture();
    let s = script(
        vec![],
        vec![
            Step {
                when: resumed,
                run: Some(failed_run(OTHER, 3, 50, 500)),
            },
            step(Some(failed_run(OTHER, 3, 50, 500))),
        ],
    );
    let _ = fx.engine(&s).build_stage();
    let specs = s.specs();
    assert_eq!(specs.len(), 2);
    assert_eq!(specs[0].resume.as_deref(), Some("sess-1"));
    assert_eq!(specs[1].resume.as_deref(), Some("sess-1"));

    // A gone session reruns as the fresh fix round.
    let fx = fix_round_fixture();
    let s = script(
        vec![],
        vec![
            Step {
                when: resumed,
                run: Some(failed_run(GONE, 0, 0, 0)),
            },
            step(Some(failed_run(OTHER, 3, 50, 500))),
        ],
    );
    let _ = fx.engine(&s).build_stage();
    let specs = s.specs();
    assert_eq!(specs.len(), 2);
    assert_eq!(specs[0].resume.as_deref(), Some("sess-1"));
    assert!(specs[1].resume.is_none());
    assert!(specs[1].prompt.contains(FRESH_FIX), "{}", specs[1].prompt);
    assert!(
        specs[1].prompt.contains("changes-r2.md"),
        "{}",
        specs[1].prompt
    );
}

#[test]
fn failed_escalated_round_reruns_fresh_fix() {
    let fx = fix_round_fixture();
    factory_engine::BoardStore::new(&fx.board_path, Duration::from_secs(5))
        .update_job("001", |j| {
            j.escalated_model = Some("claude-opus-5-5".into());
            j.builder_session_model = Some("claude-sonnet-5-5".into());
            Ok(())
        })
        .expect("escalate");
    let s = script(
        vec![],
        vec![
            step(Some(failed_run(OTHER, 3, 50, 500))),
            step(Some(failed_run(OTHER, 3, 50, 500))),
        ],
    );
    let _ = fx.engine(&s).build_stage();
    let specs = s.specs();
    assert_eq!(specs.len(), 2);
    for spec in &specs {
        assert!(spec.resume.is_none());
        assert!(spec.prompt.contains(FRESH_FIX), "{}", spec.prompt);
    }
    assert_eq!(specs[1].model, specs[0].model);
    assert_eq!(specs[1].prompt, specs[0].prompt);
}

#[test]
fn network_failure_probes_until_back() {
    let fx = disk_fixture();
    let m = fake_machine(|_| 10_000, |_, _| {});
    let s = script(
        vec![change_effect()],
        (0..3)
            .map(|_| step(Some(failed_run(NETWORK, 0, 0, 0))))
            .collect(),
    )
    .with_clock(m.elapsed.clone());
    let out = fx
        .engine(&s)
        .with_machine_wait(m.wait)
        .build_stage()
        .expect("build");
    assert_eq!(out, StageOutcome::Advanced(Stage::Review));
    assert_eq!(s.call_times(), vec![0.0, 60.0, 120.0, 240.0]);
    let text = ledger(&fx);
    assert_eq!(text.matches("machine ok after").count(), 1, "{text}");
    assert_eq!(rerun_lines(&fx), 1, "{text}");
    assert!(
        text.contains("builder failed (network), rerunning once"),
        "{text}"
    );
}

#[test]
fn network_wait_reaches_the_rerun_when_the_ledger_is_read_only() {
    use std::os::unix::fs::PermissionsExt;
    let fx = disk_fixture();
    let ledger_path = fx.job_dir.join("progress.md");
    let path = ledger_path.clone();
    let m = fake_machine(
        |_| 10_000,
        move |n, _| {
            if n == 1 {
                let _ = std::fs::OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(&path);
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444))
                    .expect("read only");
            }
        },
    );
    let s = script(
        vec![change_effect()],
        vec![step(Some(failed_run(NETWORK, 0, 0, 0)))],
    )
    .with_clock(m.elapsed.clone());
    let _ = fx.engine(&s).with_machine_wait(m.wait).build_stage();
    // The rerun started although the ledger could not be written.
    assert_eq!(s.call_times().len(), 2, "{:?}", s.call_times());
}

#[test]
fn auth_failure_with_readable_key_file_waits() {
    use std::os::unix::fs::PermissionsExt;
    let mut fx = disk_fixture();
    let key = fx.tmp.path().join("anthropic.key");
    std::fs::write(&key, "sk-test\n").expect("key");
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).expect("mode");
    fx.config.agent_cli = Some("claude".into());
    fx.config.api_key_file = Some(key.display().to_string());
    let m = fake_machine(|_| 10_000, |_, _| {});
    let s = script(
        vec![change_effect()],
        (0..2)
            .map(|_| step(Some(failed_run(AUTH, 0, 0, 0))))
            .collect(),
    )
    .with_clock(m.elapsed.clone());
    let out = fx
        .engine(&s)
        .with_machine_wait(m.wait)
        .build_stage()
        .expect("build");
    assert_eq!(out, StageOutcome::Advanced(Stage::Review));
    assert_eq!(s.call_times(), vec![0.0, 60.0, 120.0]);
    assert!(ledger(&fx).contains("builder failed (auth), rerunning once"));
}

#[test]
fn probe_that_did_work_counts_as_the_rerun() {
    let fx = disk_fixture();
    let m = fake_machine(|_| 10_000, |_, _| {});
    let s = script(
        vec![change_effect()],
        vec![
            step(Some(failed_run(NETWORK, 0, 0, 0))),
            step(Some(failed_run(NETWORK, 5, 0, 0))),
        ],
    );
    let out = fx
        .engine(&s)
        .with_machine_wait(m.wait)
        .build_stage()
        .expect("build");
    assert!(matches!(out, StageOutcome::NeedsHuman(_)), "{out:?}");
    assert_eq!(fx.job().park_kind.as_deref(), Some("builder-stopped"));
    assert_eq!(s.specs().len(), 2);
}

#[test]
fn budget_stop_blocks_rerun() {
    let fx = Fixture::new();
    let budget = fx.config.budget_weighted;
    let s = script(
        vec![change_effect()],
        vec![step(Some(failed_run(OTHER, 3, 50, budget)))],
    );
    let out = fx.engine(&s).build_stage().expect("build");
    assert!(matches!(out, StageOutcome::NeedsHuman(_)), "{out:?}");
    let job = fx.job();
    assert_eq!(job.park_kind.as_deref(), Some("budget-stopped"));
    assert_eq!(job.reason.as_deref(), Some("budget is stopped"));
    assert_eq!(s.specs().len(), 1);
}

#[test]
fn budget_stop_blocks_probe() {
    let fx = disk_fixture();
    let budget = fx.config.budget_weighted;
    let m = fake_machine(|_| 10_000, |_, _| {});
    let s = script(
        vec![change_effect()],
        vec![step(Some(failed_run(NETWORK, 0, 0, budget)))],
    );
    let out = fx
        .engine(&s)
        .with_machine_wait(m.wait)
        .build_stage()
        .expect("build");
    assert!(matches!(out, StageOutcome::NeedsHuman(_)), "{out:?}");
    assert_eq!(fx.job().park_kind.as_deref(), Some("budget-stopped"));
    assert_eq!(s.specs().len(), 1);
}

const BLOCKED_DISK: &str = "Status: BLOCKED: disk is full";

/// A free-space answer that is short only on its third call, the check after the builder run.
fn short_on_third_call() -> impl Fn(f64) -> u64 + Send + Sync + 'static {
    let calls = AtomicU32::new(0);
    move |_| {
        if calls.fetch_add(1, Ordering::SeqCst) + 1 == 3 {
            100
        } else {
            10_000
        }
    }
}

/// An effect that makes a change and counts the runs that still found the rework note.
fn change_seeing_note(note: std::path::PathBuf, seen: Arc<AtomicU32>) -> Effect {
    Box::new(move |spec| {
        if note.exists() {
            seen.fetch_add(1, Ordering::SeqCst);
        }
        std::fs::write(spec.cwd.join("a.txt"), "x\n").expect("write change");
        "done".to_string()
    })
}

#[test]
fn disk_blocked_report_waits_then_reruns_with_note() {
    // Per task.
    let mut fx = task_fixture();
    fx.config.min_free_disk_mb = 2048;
    std::fs::write(note_path(&fx), "mind the schema").expect("note");
    let m = fake_machine(short_on_third_call(), |_, _| {});
    let s = script(
        vec![change_effect()],
        vec![step(Some(AgentRun::done(BLOCKED_DISK)))],
    );
    let out = fx
        .engine(&s)
        .with_machine_wait(m.wait)
        .build_stage()
        .expect("build");
    assert_eq!(out, StageOutcome::Advanced(Stage::Build), "{}", ledger(&fx));
    let specs = s.specs();
    assert_eq!(specs.len(), 2);
    assert!(
        specs[1].prompt.contains("mind the schema"),
        "{}",
        specs[1].prompt
    );
    assert!(ledger(&fx).contains("builder failed (disk), rerunning once"));

    // Whole plan.
    let mut fx = disk_fixture();
    std::fs::write(note_path(&fx), "mind the schema").expect("note");
    fx.config.min_free_disk_mb = 2048;
    let seen = Arc::new(AtomicU32::new(0));
    let m = fake_machine(short_on_third_call(), |_, _| {});
    let s = script(
        vec![change_seeing_note(note_path(&fx), seen.clone())],
        vec![step(Some(AgentRun::done(BLOCKED_DISK)))],
    );
    let out = fx
        .engine(&s)
        .with_machine_wait(m.wait)
        .build_stage()
        .expect("build");
    assert_eq!(out, StageOutcome::Advanced(Stage::Review));
    let specs = s.specs();
    assert_eq!(specs.len(), 2);
    assert!(
        specs[1].prompt.contains("rework-note.md"),
        "{}",
        specs[1].prompt
    );
    assert_eq!(seen.load(Ordering::SeqCst), 1);

    // With the disk fine, the same reply parks as blocked.
    let fx = disk_fixture();
    let m = fake_machine(|_| 10_000, |_, _| {});
    let s = script(
        vec![change_effect()],
        vec![step(Some(AgentRun::done(BLOCKED_DISK)))],
    );
    let out = fx
        .engine(&s)
        .with_machine_wait(m.wait)
        .build_stage()
        .expect("build");
    assert!(matches!(out, StageOutcome::NeedsHuman(_)), "{out:?}");
    assert_eq!(fx.job().park_kind.as_deref(), Some("builder-blocked"));
    assert_eq!(s.specs().len(), 1);
}

#[test]
fn per_task_machine_failure_waits_before_fresh_fallback() {
    let fx = task_fixture();
    // A failed first run leaves the worktree, then a saved session points at the branch tip.
    let dead = scripted(vec![]);
    let _ = fx.engine(&dead).build_stage();
    let mut board = Board::load(&fx.board_path).expect("load");
    let job = board.find_mut("001").expect("job");
    job.set_stage(Stage::Build, None, true, 1.0).expect("stage");
    let tip = fx.repo.tip(&job.branch).expect("tip");
    job.task_session = Some(TaskSession {
        task: 1,
        id: "sess-old".into(),
        tip,
    });
    board.save(&fx.board_path).expect("save");
    let before = ledger(&fx);
    let m = fake_machine(|_| 10_000, |_, _| {});
    let s = script(
        vec![change_effect()],
        vec![Step {
            when: resumed,
            run: Some(failed_run(AUTH, 0, 0, 0)),
        }],
    )
    .with_clock(m.elapsed.clone());
    let out = fx
        .engine(&s)
        .with_machine_wait(m.wait)
        .build_stage()
        .expect("build");
    assert_eq!(out, StageOutcome::Advanced(Stage::Build));
    assert_eq!(s.call_times(), vec![0.0, 60.0]);
    let specs = s.specs();
    assert_eq!(specs[0].resume.as_deref(), Some("sess-old"));
    assert!(specs[1].resume.is_none());
    assert!(
        specs[1].prompt.contains("Do only Task 1"),
        "{}",
        specs[1].prompt
    );
    let after = ledger(&fx);
    let new = &after[before.len()..];
    assert_eq!(new.matches(", fresh)").count(), 1, "{new}");
    assert_eq!(new.matches(", resumed)").count(), 1, "{new}");
}
