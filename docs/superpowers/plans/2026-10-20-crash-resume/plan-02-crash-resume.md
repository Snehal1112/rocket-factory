# Crash resume, Plan 2: Session capture while the builder runs

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Save the builder's session id on the board the moment the `claude` stream reports it, so a driver that dies mid-task leaves `Job.task_session` behind for plan 3 to resume. The runner also writes a `FactorySession` event, and the driver clears `task_session` as soon as the builder run returns, when the task advances and on `cancel`.

**Architecture:** `factory-agent` gets an optional callback on the run request: `SessionHook` in `spec.rs`, `AgentSpec.on_session`, and one call in `ClaudeRunner`'s read loop on the first `StreamEvent::Init`, together with an `EventKind::Session` event. `factory-core` gets the optional board field `Job.task_session: Option<TaskSession>`. `factory-engine`'s `build_task` (`crates/factory-engine/src/build.rs`) attaches a hook that writes the board through its own `BoardStore`. The run itself goes through the `AgentRunner` port (`Engine.runner`, fakes in tests: `ScriptedRunner`), and every board write goes through `BoardStore`. No git call is added. `factory-cli`'s `cancel` clears the field in its existing board update. The API cancel runs the CLI `cancel`, so that one place covers both. Nothing reads `task_session` yet; plan 3 does.

**Tech Stack:** Rust, `std::sync::Arc`, serde (`#[serde(default, skip_serializing_if)]`), `tracing` (engine only), `tempfile`, the `fake-claude` test binary of `factory-agent`.

**Spec:** docs/superpowers/specs/2026-10-20-crash-resume-design.md

## Global Constraints

- Always `-j4`; never `--workspace` or `--all`; never set `CARGO_TARGET_DIR`. Use per-crate commands: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>` (a `--test <file>` or `--lib` selector is allowed in front of the filter).
- Board JSON stays compatible with `factory-server` (`dashboard/`): `task_session` is optional, `#[serde(default, skip_serializing_if = "Option::is_none")]`, so old boards load and a board without a saved session has no `task_session` key. The dashboard ignores unknown `hook_event_name` values, so `FactorySession` lines are safe.
- Board, ledger and events are written only by the driver. The hook runs inside the driver process, on the thread that runs `ClaudeRunner::run`; the agent never writes the board.
- Engine logic stays behind the ports: the builder run goes through the `AgentRunner` port (`Engine.runner`), board writes through `BoardStore`. Tests use fakes (`ScriptedRunner`, `fake-claude`).
- Never read `/proc` directly; process checks go through `procinfo.rs`. This plan adds none.
- A failing board write from the hook is logged at warn and never stops or fails the run.
- `AgentSpec` keeps `#[derive(Debug, Clone)]`, so `SessionHook` has a manual `Debug` and a manual `Clone`. `AgentSpec::new` sets `on_session: None`, and `AgentSpec::args` does not change.
- Each task lists the tests it adds. Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- Task 3 touches `factory-engine`: its logic sits behind the `AgentRunner` port and `BoardStore`. Task 2 adds the board field `Job.task_session`, which is optional. Task 1 adds the events kind `Session`; no new events field is added, the id goes in the existing optional `last_message`.
- Comments are short full sentences ending with a punctuation mark.

## Review Focus

1. A stream with two `system/init` lines (ids `fake-1`, then `fake-2`). Expected: the hook is called once, with `fake-1`, and exactly one `FactorySession` line is written. Pinned by `the_session_hook_is_called_once_even_with_two_init_lines` (Task 1, new fake-claude mode `reinit`).
2. A run whose spec has no hook (reviewer, committer, message, kb and the legacy builder). Expected: no `FactorySession` event, so their event lines stay `SubagentStart`, `PostToolUse`, `SubagentStop`. Pinned by `a_run_without_a_hook_emits_no_session_event` (Task 1) and `only_task_builders_get_a_session_hook` (Task 3).
3. The hook's board write fails (the board lock is held by someone else while the id arrives). Expected: a warn log, the builder run goes on, the task commits and the job advances; `task_session` stays `None`. Pinned by `a_failing_session_write_does_not_stop_the_run` (Task 3).
4. A builder that edits `plan.md` after the hook fired, so `run_builder_guarded` parks it as tampered. Expected: `task_session` is cleared all the same. Pinned by `the_session_is_cleared_when_a_tampering_builder_parks` (Task 3).
5. The second task of a plan. Expected: the recorded `tip` is the branch tip after task 1's commit, not the job base. Pinned by `the_second_task_session_records_the_tip_after_the_first_commit` (Task 3).
6. The hook is called from a thread other than the one that runs the engine. Expected: the write lands, because the hook owns its paths and builds its own `BoardStore`. Pinned by `the_hook_writes_the_board_through_its_own_store` (Task 3).
7. A job with a saved `task_session` is cancelled. Expected: `cancel` clears it in the same board update that sets `cancelled`. Pinned by `cancel_clears_the_saved_task_session` (Task 3).
8. The exact JSON shape the dashboard and plan 3 see. Expected: `"task_session":{"task":2,"id":"sess-9","tip":"abc1234"}`, and no key at all when unset. Pinned by `task_session_round_trips` (Task 2).

---

## File Structure

- `crates/factory-agent/src/spec.rs` (modify): `SessionHook`, `AgentSpec.on_session`.
- `crates/factory-agent/src/events.rs` (modify): `EventKind::Session`, `hook_name` `FactorySession`, a new `#[cfg(test)] mod tests`.
- `crates/factory-agent/src/claude.rs` (modify): the hook call and the `Session` event in `run_inner`.
- `crates/factory-agent/src/lib.rs` (modify): `pub use spec::{AgentSpec, Caps, SessionHook};`.
- `crates/factory-agent/src/bin/fake-claude.rs` (modify): new mode `reinit`.
- `crates/factory-agent/tests/runner.rs` (modify): the runner tests.
- `crates/factory-core/src/board.rs` (modify): `TaskSession`, `Job.task_session`, unit test.
- `crates/factory-core/src/lib.rs` (modify): re-export `TaskSession`.
- `crates/factory-core/tests/board_roundtrip.rs` (modify): the old-board test.
- `crates/factory-engine/src/build.rs` (modify): `session_hook`, `clear_task_session`, the changes in `build_task`.
- `crates/factory-engine/tests/build_checks.rs` (modify): the engine tests.
- `crates/factory-cli/src/commands.rs` (modify): `cancel` clears `task_session`.
- `crates/factory-cli/tests/project_lease_cli.rs` (modify): the cancel test.

### Task 1: SessionHook, on_session and the Session event in factory-agent

**Files:**
- Modify: `crates/factory-agent/src/spec.rs`, `crates/factory-agent/src/events.rs`, `crates/factory-agent/src/claude.rs`, `crates/factory-agent/src/lib.rs`, `crates/factory-agent/src/bin/fake-claude.rs`, `crates/factory-agent/tests/runner.rs`

**Interfaces:**
- Consumes: `StreamEvent::Init { session_id: String }` from `StreamState::feed(&mut self, line: &str) -> Vec<StreamEvent>` (`crates/factory-agent/src/stream.rs`); `ClaudeRunner::event(&self, spec: &AgentSpec, kind: EventKind, tokens: u64) -> AgentEvent` and `EventSink::emit(&self, ev: &AgentEvent)`.
- Produces:
  - In `spec.rs`: `pub struct SessionHook(pub Arc<dyn Fn(&str) + Send + Sync>);` with
    - `impl SessionHook { pub fn new(f: impl Fn(&str) + Send + Sync + 'static) -> SessionHook; pub fn call(&self, session_id: &str); }`
    - `impl Clone for SessionHook` (an `Arc::clone`, so clones share one closure), `impl std::fmt::Debug for SessionHook` that writes exactly `SessionHook(..)`.
  - `AgentSpec.on_session: Option<SessionHook>` with the doc comment `/// Called once with the session id when the stream reports it, while the run still goes on. Only task builders set it.`; `AgentSpec::new` sets `None`.
  - In `events.rs`: `EventKind::Session` with doc `/// The session id of a run, reported while the run goes on.`; `EventKind::Session.hook_name() == "FactorySession"`.
  - `pub use spec::{AgentSpec, Caps, SessionHook};` in `lib.rs`.
  - `ClaudeRunner::run_inner`: on the first `StreamEvent::Init` of a run, and only when `spec.on_session` is `Some`, call the hook with the id, then emit `self.event(spec, EventKind::Session, used)` with `last_message = Some(session_id)`. Later `Init` events are ignored (a local `bool`). Without a hook nothing is emitted. The call happens before the budget, turn and stall checks of that line, so a run that is killed later has already reported its id.
  - fake-claude mode `reinit`: `init("fake-1")`, `init("fake-2")`, `result("re-init", false, "success")`.

- [ ] **Step 1: Read** spec section 2 (the first two bullets), the Decisions entry "How does the driver learn the builder session id", `crates/factory-agent/src/spec.rs`, `crates/factory-agent/src/claude.rs` (`run_inner`, the read loop and the `Tool` event), `crates/factory-agent/src/stream.rs` (`feed` and `StreamEvent::Init`), `crates/factory-agent/src/events.rs`, `crates/factory-agent/src/bin/fake-claude.rs` (modes `ok` and `silent`), `crates/factory-agent/tests/runner.rs` (helpers `runner` and `spec`, and `ok_run_collects_session_usage_result_and_events`, which must keep passing unchanged) and `crates/factory-agent/tests/stream.rs` (`sample_event`).
- [ ] **Step 2: Write the failing tests.** Add the mode `reinit` to `fake-claude.rs` first; it is test input. In `crates/factory-agent/tests/runner.rs` add `SessionHook` to the `factory_agent` import, `use std::sync::{Arc, Mutex};`, a helper and the tests below. One full example:

```rust
/// A hook that records every id it gets.
fn recording_hook() -> (SessionHook, Arc<Mutex<Vec<String>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let into = Arc::clone(&seen);
    let hook = SessionHook::new(move |id| into.lock().expect("seen").push(id.to_string()));
    (hook, seen)
}

#[test]
fn the_session_hook_is_called_once_with_the_init_id() {
    let tmp = tempfile::tempdir().expect("tmp");
    let (hook, seen) = recording_hook();
    let mut s = spec("ok", tmp.path());
    s.on_session = Some(hook);
    let run = runner(EventSink::null("001")).run(&s);
    assert_eq!(run.stop_reason, StopReason::Done);
    assert_eq!(*seen.lock().expect("seen"), vec!["fake-1".to_string()]);
}
```

  The other runner tests (event lines are read from `events.jsonl` as in `ok_run_collects_session_usage_result_and_events`):

| Test | Input | Assertion |
|---|---|---|
| `a_session_event_is_emitted` | mode `ok`, a recording hook, `EventSink::new(tmp/events.jsonl, "001")` | `hook_event_name` list is `["SubagentStart", "FactorySession", "PostToolUse", "SubagentStop"]`; line 1 has `last_message == "fake-1"`, `agent_id == "001-builder"`, `job_id == "001"` |
| `the_session_hook_fires_before_a_stalled_run_is_killed` | mode `silent`, a recording hook, `stall_after = Some(Duration::from_millis(400))` | `stop_reason == StopReason::Stalled`; seen is `["fake-1"]`; elapsed under 10 s |
| `the_session_hook_is_called_once_even_with_two_init_lines` | mode `reinit`, a recording hook, a file sink | `stop_reason == Done`; seen is `["fake-1"]`; exactly one line has `hook_event_name == "FactorySession"` |
| `a_run_without_a_hook_emits_no_session_event` | mode `ok`, no hook, a file sink | no line has `hook_event_name == "FactorySession"`; the list is `["SubagentStart", "PostToolUse", "SubagentStop"]` |
| `a_new_spec_has_no_session_hook_and_debug_hides_the_closure` | `AgentSpec::new("001", "builder", tmp, "p")`; `SessionHook::new(\|_\| {})` | `on_session.is_none()`; `format!("{hook:?}") == "SessionHook(..)"`; `Arc::ptr_eq(&hook.0, &hook.clone().0)`; `format!("{spec:?}")` still compiles and contains `on_session: None` |

  In a new `#[cfg(test)] mod tests` at the bottom of `crates/factory-agent/src/events.rs`:

| Test | Input | Assertion |
|---|---|---|
| `session_kind_is_the_factory_session_hook` | `EventKind::Session` | `hook_name() == "FactorySession"`, and it differs from the names of `Start`, `Tool`, `Stop`, `BudgetWarn`, `Notice` |
| `a_session_line_carries_the_id_as_last_message` | `EventSink::null("001").line(&ev, 1.0)` with `kind: EventKind::Session`, `agent_id: "001-builder"`, `agent_type: "factory-builder"`, `last_message: Some("sess-1")`, `tokens: 0`, other options `None` | parsed JSON has `hook_event_name == "FactorySession"`, `last_message == "sess-1"`, `job_id == "001"`, no `tool_name` key |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-agent session` and expect a compile failure: `unresolved import factory_agent::SessionHook`, `no field on_session on type AgentSpec` and `no variant named Session found for enum EventKind`.
- [ ] **Step 4: Implement** the items listed under Produces. Add `use std::sync::Arc;` and `use std::fmt;` to `spec.rs`. In `claude.rs`, extend the `for ev in events` loop with a `StreamEvent::Init { session_id }` arm guarded by a `let mut session_seen = false;` declared next to `warned`; keep the `Tool` arm as it is.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-agent session` and expect the 8 new tests to PASS. Then run `cargo test -j4 -p factory-agent` and expect every existing test to PASS, `ok_run_collects_session_usage_result_and_events` unchanged among them. Finish with `cargo check -j4 -p factory-engine` and `cargo check -j4 -p factory-cli` (no `AgentSpec` struct literal exists outside `spec.rs`, so both compile unchanged).

### Task 2: TaskSession and Job.task_session on the board

**Files:**
- Modify: `crates/factory-core/src/board.rs`, `crates/factory-core/src/lib.rs`, `crates/factory-core/tests/board_roundtrip.rs`

**Interfaces:**
- Consumes: `Job::new(id, dir, slug, created, feature, now) -> Job`, `Board::load(path: &Path) -> Result<Board, CoreError>`, the fixture `tests/fixtures/board.json` (`FIXTURE` in `board_roundtrip.rs`).
- Produces (in `board.rs`, re-exported from `factory_core` next to `Lease`):
  - ```rust
    /// The builder session of the plan task that is running now. Only a crash leaves it on the board.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct TaskSession {
        /// The plan task number, as in `### Task N`.
        pub task: u32,
        /// The `claude` session id.
        pub id: String,
        /// The branch tip when the task started.
        pub tip: String,
    }
    ```
  - On `Job`, after `spec_warning`: `/// The builder session of the running task, saved while the builder runs. New in the crash resume series, absent in older boards.` `#[serde(default, skip_serializing_if = "Option::is_none")] pub task_session: Option<TaskSession>,`. `Job::new` sets `task_session: None`.

- [ ] **Step 1: Read** spec section 2 (the `factory-core` bullet) and the edge-case row "Old board without `task_session`", `crates/factory-core/src/board.rs` (the `Job` struct, `Job::new`, the tests `fix_source_is_optional_and_omitted_when_unset` and `batch_snapshot_is_optional_and_omitted_when_unset`), `crates/factory-core/src/lib.rs` (`pub use board::{...}`) and `crates/factory-core/tests/board_roundtrip.rs` (`FIXTURE`, `real_board_round_trips_without_losing_fields`, `spec_warning_is_optional_on_a_job`).
- [ ] **Step 2: Write the failing tests.** In the `board.rs` test module:

```rust
#[test]
fn task_session_round_trips() {
    let mut j = Job::new("001", "d", "s", "2026-10-20", "f", 1.0);
    assert!(j.task_session.is_none());
    assert!(!serde_json::to_string(&j).expect("json").contains("task_session"));
    j.task_session = Some(TaskSession {
        task: 2,
        id: "sess-9".to_string(),
        tip: "abc1234".to_string(),
    });
    let value = serde_json::to_value(&j).expect("json");
    assert_eq!(
        value["task_session"],
        serde_json::json!({"task": 2, "id": "sess-9", "tip": "abc1234"})
    );
    let back: Job = serde_json::from_value(value).expect("parse");
    assert_eq!(back.task_session, j.task_session);
}
```

  In `crates/factory-core/tests/board_roundtrip.rs`:

| Test | Input | Assertion |
|---|---|---|
| `a_board_without_task_session_loads` | `Board::load(Path::new(FIXTURE))`, and `serde_json::from_str::<factory_core::Job>` of `serde_json::to_string(&Job::new("001", "d", "s", "2026-10-20", "f", 1.0))` | every fixture job has `task_session == None`; `serde_json::to_string(&board)` has no `task_session`; the parsed job has `task_session == None` |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-core task_session` and expect a compile failure: `cannot find struct TaskSession in this scope` and `no field task_session on type Job`.
- [ ] **Step 4: Implement** the struct, the field and the re-export from the signatures above.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-core task_session` and expect the 2 tests to PASS. Then `cargo test -j4 -p factory-core --test board_roundtrip` and expect `real_board_round_trips_without_losing_fields` and every other test to PASS unchanged. Finish with `cargo check -j4 -p factory-engine` and `cargo check -j4 -p factory-cli`: the only `Job` struct literal is `Job::new`, so both compile unchanged.

### Task 3: build_task saves and clears the session

**Files:**
- Modify: `crates/factory-engine/src/build.rs`, `crates/factory-engine/tests/build_checks.rs`, `crates/factory-cli/src/commands.rs`, `crates/factory-cli/tests/project_lease_cli.rs`

**Interfaces:**
- Consumes: `SessionHook::new`, `SessionHook::call`, `AgentSpec.on_session` (Task 1); `TaskSession`, `Job.task_session` (Task 2); `BoardStore::new(board_path: &Path, wait: Duration) -> BoardStore`, `BoardStore::update_job<R>(&self, id: &str, f: impl FnOnce(&mut Job) -> Result<R, CoreError>) -> Result<R, StoreError>`, `BoardStore::load(&self) -> Result<Board, StoreError>`; `Engine.board_path`, `Engine.job_id`, `Engine.lock_wait`, `Engine::with_lock_wait(self, Duration) -> Engine`; `Engine::update(&self, f: impl FnOnce(&mut Job) -> Result<(), CoreError>) -> Result<(), EngineError>`; `Engine::run_builder_guarded(&self, spec: AgentSpec) -> Result<Result<AgentRun, StageOutcome>, EngineError>`; `factory_engine::lock_board(path: &Path, wait: Duration) -> Result<BoardLock, StoreError>`. Test fixtures: `two_task_fixture()`, `committing_fixture()`, `scripted_auto(effects: Vec<Effect>) -> ScriptedRunner`, `Effect` (`Box<dyn Fn(&AgentSpec) -> String + Send + Sync>`), `change_effect`, `blocked_effect`, `Fixture::job`, `Fixture::ledger`, `ScriptedRunner::specs_for`; in the CLI test, `Fixture::new()`, `new_job(&fx)` and `Fixture::run`.
- Produces (private, in `impl Engine<'_>` in `build.rs`; the port is `AgentRunner` for the run and `BoardStore` for the board):
  - `fn session_hook(&self, task: u32, tip: &str) -> SessionHook`: the closure owns clones of `board_path`, `job_id`, `lock_wait`, `task` and `tip`. On a call it builds `BoardStore::new(&board_path, lock_wait)` and runs `update_job(&job_id, |j| { j.task_session = Some(TaskSession { task, id, tip }); Ok(()) })`. An error is logged as `tracing::warn!(event = "builder.session_save_failed", job_id = ..., task, msg = %e)` and dropped.
  - `fn clear_task_session(&self) -> Result<(), EngineError>`: `self.update(|j| { j.task_session = None; Ok(()) })`.
  - `build_task`: after the rework note is appended, `spec.on_session = Some(self.session_hook(task.n, &tip))`, where `tip` is the tip computed at the start of `build_task`. Right after `run_builder_guarded` returns, and before matching its result (so before the tamper, stop, blocked and committed-on-its-own park checks), call `clear_task_session`. When `run_builder_guarded` returned `Err`, that error is returned and a clear error is only logged at warn; otherwise a clear error is returned. The advance update that sets `builder_id`, `task_index` and `batch_snapshot` also sets `j.task_session = None`.
  - `build_stage`'s whole-plan path, `builder_resume_spec`, the reviewer, the committer, message and kb runs get no hook.
  - `factory-cli` `cancel` (`crates/factory-cli/src/commands.rs`): the `update_job` closure that sets `j.cancelled = true` and `j.lease = None` also sets `j.task_session = None`.

- [ ] **Step 1: Read** spec section 2 (the `build_task` and clearing bullets), the edge-case rows "Session id arrives, board write fails" and "Builder run returns normally", and the Decisions entry "When is `task_session` cleared?". Then read `crates/factory-engine/src/build.rs` (`build_stage`, `build_task`), `crates/factory-engine/src/permissions.rs` (`run_builder_guarded`), `crates/factory-engine/src/ports.rs` (`Engine::update`, `run_agent`), `crates/factory-engine/src/store.rs`, `crates/factory-engine/src/lock.rs`, `crates/factory-engine/tests/common/mod.rs` (`Fixture`, `ScriptedRunner`, `scripted_auto`, `committing_fixture`), `crates/factory-engine/tests/build_checks.rs` (`two_task_fixture`, `change_effect`, `blocked_effect`, `a_blocked_reply_parks_without_committing`), `crates/factory-engine/tests/driver_files_guard.rs` (how a builder that edits `plan.md` parks), `crates/factory-cli/src/commands.rs` (`cancel`) and `crates/factory-cli/tests/project_lease_cli.rs` (`new_job`, `other_commands_are_not_blocked_by_a_held_lease`).
- [ ] **Step 2: Write the failing tests.** In `crates/factory-engine/tests/build_checks.rs` add `use std::sync::{Arc, Mutex};`, `use std::time::Duration;` and `use factory_core::TaskSession;`, one helper and the tests. One full example:

```rust
/// The board's `task_session` as the builder saw it during its run.
type Seen = Arc<Mutex<Option<TaskSession>>>;

/// A task builder that reports `id` through its session hook, notes the board's `task_session`,
/// then writes `file`.
fn session_effect(board: std::path::PathBuf, id: &'static str, file: &'static str, seen: Seen) -> Effect {
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
```

  The other engine tests, all on `two_task_fixture()` with `scripted_auto` unless the row says otherwise:

| Test | Input | Assertion |
|---|---|---|
| `the_hook_writes_the_board_through_its_own_store` | an effect that clones `spec.on_session`, runs `std::thread::spawn(move \|\| hook.call("sess-thread")).join().expect("thread")`, then notes the board's `task_session` into a `Seen` and writes `a.txt` | seen is `Some(TaskSession { task: 1, id: "sess-thread", tip: <base> })`; afterwards `task_session == None` |
| `the_second_task_session_records_the_tip_after_the_first_commit` | effects `change_effect("a.txt", 3)`, then `session_effect(board, "sess-two", "b.txt", seen)`; the tip `first` is read with `fx.repo.tip(&fx.job().branch)` between the two `build_stage` calls | seen is `Some(TaskSession { task: 2, id: "sess-two", tip: first })` and `first != base` |
| `the_session_is_cleared_when_the_run_returns_even_if_it_parks` | an effect that calls the hook with `sess-live`, notes the board into a `Seen`, writes `a.txt` and replies `BLOCKED: no schema` | `NeedsHuman`; `ParkRecord::read(&fx.job_dir)` kind is `ParkKind::BuilderBlocked`; seen is `Some` with `task == 1`; `fx.job().task_session == None`; `task_index == 0` |
| `the_session_is_cleared_when_a_tampering_builder_parks` | an effect that calls the hook with `sess-live`, then appends `"\nextra\n"` to the captured `fx.job_dir.join("plan.md")` and writes `a.txt` | `NeedsHuman` whose reason contains `builder changed plan.md`; `fx.job().task_session == None` |
| `the_session_is_cleared_when_the_task_advances` | the board is seeded before the run with `task_session = Some(TaskSession { task: 1, id: "sess-old", tip: "0000000" })` (`Board::load`, `find_mut`, `save`); effect `change_effect("a.txt", 3)`, which never calls the hook | `Advanced(Stage::Build)`; `task_index == 1`; `task_session == None` |
| `a_failing_session_write_does_not_stop_the_run` | engine `fx.engine(&runner).with_lock_wait(Duration::from_millis(200))`; an effect that takes `factory_engine::lock_board(&board.with_extension("json.lock"), Duration::from_secs(1))`, calls the hook with `sess-lost` while it holds the lock, notes the board into a `Seen`, drops the lock and writes `a.txt` | `Advanced(Stage::Build)`; seen is `None`; `task_index == 1`; `task_session == None`; the ledger contains `Task 1: implementer done` |
| `only_task_builders_get_a_session_hook` | `committing_fixture()` (no task headings, so the whole-plan builder runs); an effect that asserts `spec.on_session.is_none()` and writes `a.txt` | `Advanced(Stage::Review)`; `runner.specs_for("builder")` and `runner.specs_for("committer")` are both non-empty and every spec has `on_session.is_none()` |

  In `crates/factory-cli/tests/project_lease_cli.rs`:

| Test | Input | Assertion |
|---|---|---|
| `cancel_clears_the_saved_task_session` | `Fixture::new()`, `new_job(&fx)`; `BoardStore::new(&factory_core::Layout::in_repo(&fx.root).board_file(), Duration::from_secs(5))`; `update_job("001", ...)` sets `task_session = Some(TaskSession { task: 1, id: "sess-old", tip: "abc1234" })`; then `fx.run(&["cancel", "1"]).expect("cancel")` | the reloaded job has `cancelled == true` and `task_session == None` |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine --test build_checks session` and `cargo test -j4 -p factory-engine --test build_checks hook`, and expect FAIL: the effects panic with `task builders get a session hook` (no hook is set yet), and `the_session_is_cleared_when_the_task_advances` fails with `left: Some(TaskSession { task: 1, id: "sess-old", .. })`. `only_task_builders_get_a_session_hook` already passes; it guards the rule that no other run gets a hook. Run `cargo test -j4 -p factory-cli --test project_lease_cli cancel_clears` and expect FAIL with `left: Some(TaskSession { .. })`.
- [ ] **Step 4: Implement** `session_hook`, `clear_task_session` and the `build_task` changes in `build.rs`, and the one line in `cancel`, from the signatures under Produces. Add `use factory_agent::SessionHook;`, `use factory_core::TaskSession;` and `use crate::BoardStore;` to `build.rs`.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine --test build_checks` and expect every test to PASS: the 8 new ones and all existing ones, `a_fix_round_resumes_the_session_and_does_not_restart_the_task_loop` and `a_stopped_first_task_parks_and_a_rework_resumes_at_that_task_with_the_note` among them. Then `cargo test -j4 -p factory-engine --test driver_files_guard`, `cargo test -j4 -p factory-cli --test project_lease_cli` and `cargo test -j4 -p factory-cli --test draft_cards`, all PASS. Finish with `cargo clippy -j4 -p factory-agent -p factory-core -p factory-engine -p factory-cli --all-targets -- -D warnings` and `cargo fmt -p factory-agent -p factory-core -p factory-engine -p factory-cli -- --check`, both clean.

Next plan: docs/superpowers/plans/2026-10-20-crash-resume/plan-03-crash-resume.md
