# Builder task resume, plan 2: settings and the board record

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Two project settings exist (`builder_carry_session`, `builder_carry_max_ctx`), the board can hold `builder_last` (task, session id, end time, end context), and the driver writes it when a task advances and clears it when a builder is dispatched.

**Architecture:** Settings are plain `Config` fields with validation. `BuilderLast` is an optional board field like `task_session`. `build_task` records it from the finished run and clears it at dispatch. Nothing reads it yet; plan 3 does.

**Tech Stack:** Rust, serde.

**Spec:** docs/superpowers/specs/2026-10-20-builder-task-resume-design.md

## Global Constraints

- Always `-j4`. Never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- No `unwrap` in production code. Tests use `.expect("...")` and `expect_err`.
- `builder_last` is an optional board field: absent when `None`, defaulted on load, ignored by the dashboard. The settings are defaulted keys; `Config` keeps `deny_unknown_fields`.
- Defaults: `builder_carry_session = false`, `builder_carry_max_ctx = 100000`; `builder_carry_max_ctx` must be above 0.
- With both defaults, every existing job behaves exactly as before.

## Review Focus

1. A project.toml with `builder_carry_max_ctx = 0`: the config fails closed with a clear message (test `builder_carry_max_ctx_zero_is_refused`, Task 1).
2. An old board file without `builder_last`: it loads and saves unchanged (test `a_board_without_builder_last_loads_and_omits_it`, Task 2).
3. A task that parks: `builder_last` stays unset, and a previous value is gone because it was cleared at dispatch (test `builder_last_is_not_set_when_a_task_parks`, Task 3).
4. `factory cancel` on a job holding `builder_last`: cleared (test `cancel_clears_builder_last`, Task 2).
5. `ended_at` is taken when the builder run returns, not after the checks and the snapshot (test `ended_at_is_taken_when_the_run_returns`, Task 3).

## File Structure

- `crates/factory-core/src/config.rs` (modify): two fields, defaults, validation, tests.
- `docs/project-toml-reference.md` (modify): two table rows.
- `crates/factory-core/src/board.rs`, `lib.rs` (modify): `BuilderLast`, `Job.builder_last`, export.
- `crates/factory-engine/src/ports.rs` (modify): `JobView.builder_last`.
- `crates/factory-engine/src/build.rs` (modify): record and clear.
- `crates/factory-cli/src/commands.rs` (modify): `cancel` clears the field.
- Tests: `crates/factory-core/tests/board_roundtrip.rs`, `crates/factory-cli/tests/project_lease_cli.rs`, `crates/factory-engine/tests/builder_last.rs` (create).

### Task 1: the two settings

**Files:**
- Modify: `crates/factory-core/src/config.rs`, `docs/project-toml-reference.md`
- Test: `crates/factory-core/src/config.rs` (unit tests)

**Interfaces:**
- Consumes: `Config`, `Config::default`, `Config::parse`, the `cache_ttl` validation near lines 217-224 and its test near line 629.
- Produces: `Config.builder_carry_session: bool` (default `false`) and `Config.builder_carry_max_ctx: u64` (default `100_000`). `Config::parse` refuses `builder_carry_max_ctx = 0` with the message `builder_carry_max_ctx must be above 0`. Both are new defaulted keys. The config module is pure and sits behind no port.

- [ ] **Step 1: Read** `config.rs` (the struct, `Default`, `parse`, the `cache_ttl` check and tests) and the `builder_excerpt` and `cache_ttl` rows of `docs/project-toml-reference.md`.
- [ ] **Step 2: Write the failing tests** next to the `cache_ttl` tests. One full example:

```rust
#[test]
fn builder_carry_settings_have_off_and_100000_defaults() {
    let c = Config::default();
    assert!(!c.builder_carry_session);
    assert_eq!(c.builder_carry_max_ctx, 100_000);
    let parsed = Config::parse("builder_carry_session = true\nbuilder_carry_max_ctx = 60000")
        .expect("parse");
    assert!(parsed.builder_carry_session);
    assert_eq!(parsed.builder_carry_max_ctx, 60_000);
}
```

  The other tests:

| Test | Input | Assertion |
|---|---|---|
| `builder_carry_max_ctx_zero_is_refused` | `builder_carry_max_ctx = 0` | `Config::parse(..).expect_err(..)` contains `builder_carry_max_ctx must be above 0` |
| `builder_carry_session_must_be_a_bool` | `builder_carry_session = "yes"` | `expect_err` |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-core builder_carry` and expect a compile failure `no field 'builder_carry_session' on type 'Config'`.
- [ ] **Step 4: Implement** the two fields with doc comments, the defaults and the validation. Add two rows to `docs/project-toml-reference.md` after `builder_excerpt`: `builder_carry_session` (bool, false, resume the previous task's builder session for the next task of a plan while the prompt cache is alive) and `builder_carry_max_ctx` (integer, 100000, the largest end-of-task context that may be carried).
- [ ] **Step 5: Run** `cargo test -j4 -p factory-core` and expect PASS. Then `cargo check -j4 -p factory-engine -p factory-cli`.

### Task 2: the BuilderLast board field

**Files:**
- Modify: `crates/factory-core/src/board.rs`, `crates/factory-core/src/lib.rs`, `crates/factory-engine/src/ports.rs`, `crates/factory-cli/src/commands.rs`
- Test: `crates/factory-core/tests/board_roundtrip.rs`, `crates/factory-core/src/board.rs` (unit), `crates/factory-cli/tests/project_lease_cli.rs`

**Interfaces:**
- Consumes: `TaskSession` and `Job.task_session` (with `#[serde(default, skip_serializing_if = "Option::is_none")]`), the `task_session_round_trips` test, `JobView` in `ports.rs`, the `cancel` update closure in `commands.rs` that sets `j.task_session = None`, and `cancel_clears_the_saved_task_session` in `project_lease_cli.rs`.
- Produces: `pub struct BuilderLast { pub task: u32, pub id: String, pub ended_at: u64, pub end_ctx: u64 }` (`Debug, Clone, PartialEq, Eq, Serialize, Deserialize`), `Job.builder_last: Option<BuilderLast>` (a new optional field, `None` in `Job::new`), `JobView.builder_last: Option<BuilderLast>` copied from the job, and the export `factory_core::BuilderLast`.

- [ ] **Step 1: Read** `board.rs` around `TaskSession` and `Job`, the round-trip tests, `lib.rs` exports, `JobView` and `Engine::job` in `ports.rs`, and `cancel` in `commands.rs`.
- [ ] **Step 2: Write the failing tests.** One full example in `board.rs` unit tests:

```rust
#[test]
fn builder_last_round_trips_and_is_omitted_when_none() {
    let mut j = Job::new("001", "slug", "feature", "factory/001-slug", 1.0);
    assert!(j.builder_last.is_none());
    assert!(!serde_json::to_string(&j).expect("json").contains("builder_last"));
    j.builder_last = Some(BuilderLast { task: 1, id: "sess-1".into(), ended_at: 1_700_000_000, end_ctx: 31_000 });
    let text = serde_json::to_string(&j).expect("json");
    let back: Job = serde_json::from_str(&text).expect("back");
    assert_eq!(back.builder_last, j.builder_last);
}
```

  (Use the real `Job::new` arguments the neighbouring `task_session_round_trips` test uses.) The other tests:

| Test | Where | Input | Assertion |
|---|---|---|---|
| `a_board_without_builder_last_loads_and_omits_it` | `tests/board_roundtrip.rs` | the existing real-board fixture load and save | loads, `builder_last == None`, saved text has no `builder_last` |
| `cancel_clears_builder_last` | `tests/project_lease_cli.rs` | copy `cancel_clears_the_saved_task_session`, seed `builder_last = Some(BuilderLast { task: 1, id: "sess-old", ended_at: 1, end_ctx: 5 })`, run `cancel 1` | the reloaded job has `cancelled == true` and `builder_last == None` |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-core builder_last` and expect a compile failure `cannot find type 'BuilderLast'`.
- [ ] **Step 4: Implement** the struct (doc comment: the last finished task builder of the job, used to decide a carry), the optional field, `Job::new`, the export, the `JobView` field with its copy in `Engine::job`, and `j.builder_last = None;` in the `cancel` closure next to `j.task_session = None;`.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-cli --test project_lease_cli` and `cargo check -j4 -p factory-engine` and expect PASS.

### Task 3: record at advance, clear at dispatch

**Files:**
- Modify: `crates/factory-engine/src/build.rs`
- Test: `crates/factory-engine/tests/builder_last.rs` (create)

**Interfaces:**
- Consumes: `BuilderLast`, `Job.builder_last`, `JobView.builder_last` (Task 2), `AgentRun.end_ctx` and `session_id` (plan 1), the clock `crate::ports::now() -> f64`, the dispatch block in `build_task` that drops a stale `task_session`, and the advance update that sets `builder_id`, `task_index`, `batch_snapshot`, `task_session`.
- Produces: in `build_task`, `builder_last` is cleared in the update that runs right before `run_builder_guarded` (the same update that drops a stale `task_session`), and set in the advance update to `BuilderLast { task: task.n, id, ended_at, end_ctx: run.end_ctx }` where `id` is `run.session_id` else `job.session_id` (no value is set when both are missing) and `ended_at` is `now() as u64` read immediately after the last builder run of the task returns (the fallback run when there is one), before the checks and the snapshot. It is not set when the task parks. The logic writes the board through `Engine::update` (the `BoardStore`) and runs the builder through the `AgentRunner` port.

- [ ] **Step 1: Read** `build_task` in `build.rs` (dispatch block, the two `run_builder_guarded` calls, the advance update), `tests/build_checks.rs` (`two_task_fixture`, `change_effect`) and `tests/common/mod.rs` (`ScriptedRunner`, `scripted_auto`, `Fixture`).
- [ ] **Step 2: Write the failing tests** in the new `tests/builder_last.rs`. Copy `two_task_fixture` and `change_effect` from `build_checks.rs`, and add a runner that gives each builder call its own session id and a fixed `end_ctx`:

```rust
struct Tagged { inner: ScriptedRunner, end_ctx: u64, calls: std::sync::Mutex<u32> }
impl AgentRunner for Tagged {
    fn run(&self, spec: &AgentSpec) -> AgentRun {
        let mut run = self.inner.run(spec);
        if spec.role == "builder" {
            let mut n = self.calls.lock().expect("calls");
            *n += 1;
            run.session_id = Some(format!("sess-{n}"));
            run.end_ctx = self.end_ctx;
        }
        run
    }
}

#[test]
fn builder_last_is_set_on_advance() {
    let fx = two_task_fixture();
    let runner = Tagged { inner: scripted_auto(vec![change_effect("a.txt", 3)]), end_ctx: 31_000, calls: Default::default() };
    fx.engine(&runner).build_stage().expect("task 1");
    let last = fx.job().builder_last.expect("builder_last");
    assert_eq!((last.task, last.id.as_str(), last.end_ctx), (1, "sess-1", 31_000));
    assert!(last.ended_at > 0);
}
```

  The other tests:

| Test | Input | Assertion |
|---|---|---|
| `builder_last_is_not_set_when_a_task_parks` | an effect that writes `a.txt` and replies `BLOCKED: no schema` | `NeedsHuman`; `builder_last == None` |
| `builder_last_is_cleared_when_a_builder_is_dispatched` | run task 1; then, with an effect for task 2 that loads the board during its run and notes `builder_last` | the noted value is `None`; after the advance it holds task 2 |
| `ended_at_is_taken_when_the_run_returns` | wall clock `before` and `after` around `build_stage`, a task-1 effect that sleeps 0 | `before <= ended_at && ended_at <= after` (whole seconds) |
| `a_task_without_any_session_id_sets_no_builder_last` | a runner that returns `session_id: None`, fixture job without `session_id` | `builder_last == None` after the advance |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine --test builder_last` and expect FAIL: `builder_last` is `None` after the advance, so `expect("builder_last")` panics.
- [ ] **Step 4: Implement** the clearing and the setting as described under Produces. The ledger text and every prompt stay unchanged.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine --test builder_last`, `cargo test -j4 -p factory-engine --test build_checks`, `cargo test -j4 -p factory-engine --test slim_prompts` and expect PASS. Then `cargo clippy -j4 -p factory-core -p factory-engine -p factory-cli --all-targets -- -D warnings` and `cargo fmt -p factory-core -p factory-engine -p factory-cli -- --check`, both clean.

Next plan: docs/superpowers/plans/2026-10-20-builder-task-resume/plan-03-builder-task-resume.md
