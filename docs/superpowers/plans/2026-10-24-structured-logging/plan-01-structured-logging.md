# Structured Logging Engine Events Implementation Plan

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Log a richer `agent.end`, `task.start`/`task.end` and `commit.done` from `factory-engine`.

**Architecture:** All three are `tracing::info!` events with `event = "<noun.verb>"` inside the existing job and stage spans, so `job_id` and `stage` arrive for free. No new types and no new crate dependency; the JSON layer and redaction in `factory-telemetry` stay as they are.

**Tech Stack:** Rust, `tracing`, `factory_telemetry::capture` for tests.

**Spec:** `docs/superpowers/specs/2026-10-24-structured-logging-design.md`

## Global Constraints

- All new events are `info` and use the names `agent.end`, `task.start`, `task.end`, `commit.done`; existing fields stay, new fields are additive.
- New field names must not contain `token`, `key`, `secret` or `password` (`redact_field` would write `[redacted]`); `weighted_tokens` stays.
- A log failure never changes a stage, a merge or a command.
- Cargo per crate with `-j4`: `cargo test -j4 -p factory-engine <filter>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- This plan adds no field to the board, ledger or events log. The port behind the logic stays `AgentRunner` (fakes in `tests/common/mod.rs`); git and checks are called directly.

## Review Focus

1. A run that reports no session id: `agent.end.session_id` is the empty string, not missing (`agent_end_session_id_is_empty_without_a_session`, Task 1).
2. A task title of more than 300 characters: `task.start.title` is one line and at most 300 characters (`task_start_title_is_one_line_and_capped`, Task 2).
3. A task re-entered after a park logs a second `task.start` for the same task and no stray `task.end` (`task_start_repeats_when_the_task_is_re_entered`, Task 2).
4. A commit step with nothing staged logs no `commit.done` (`no_commit_done_when_nothing_is_staged`, Task 3).
5. The plain fallback commit logs `commit.done` with `fallback` true (`commit_done_marks_the_fallback`, Task 3).

## File Structure

- Modify `crates/factory-engine/src/ports.rs`: `run_agent` logs the richer `agent.end`.
- Modify `crates/factory-engine/src/build.rs`: `build_task` logs `task.start`, `task.end`; the batch snapshot commit logs `commit.done`.
- Modify `crates/factory-engine/src/commit.rs`: `commit_step` becomes a wrapper that logs `commit.done`.
- Tests: `crates/factory-engine/tests/log_events.rs`, `tests/task_carry.rs`, `tests/commit.rs`.

---

### Task 1: Richer `agent.end`

**Files:**
- Modify: `crates/factory-engine/src/ports.rs` (the `agent.end` event in `run_agent`)
- Test: `crates/factory-engine/tests/log_events.rs`, `crates/factory-engine/tests/task_carry.rs`

**Interfaces:**
- Consumes: `AgentRun { usage: Usage, turns: u32, start_ctx: u64, end_ctx: u64, session_id: Option<String>, .. }` and `AgentSpec.resume` from `factory-agent`; `Engine::job()` for the round.
- Produces: `agent.end` carries `input`, `cache_creation`, `cache_read`, `output` (u64), `turns` (u32), `resumed` (bool), `round` (u32), `start_ctx`, `end_ctx` (u64), `session_id` (string), besides today's `role`, `model`, `stop_reason`, `weighted_tokens`, `duration_ms`.

- [ ] **Step 1: Read `run_agent` and `log_usage` in `ports.rs`, and the helpers in `tests/log_events.rs` (`events_named`, `builder_effects`)**

- [ ] **Step 2: Write the failing tests**

In `tests/log_events.rs` add a runner that sets usage on builder runs, like `Tagged` in `tests/common/mod.rs` does for `end_ctx`:

```rust
struct UsageRunner(common::ScriptedRunner);

impl factory_agent::AgentRunner for UsageRunner {
    fn run(&self, spec: &factory_agent::AgentSpec) -> factory_agent::AgentRun {
        let mut run = self.0.run(spec);
        if spec.role == "builder" {
            run.usage = factory_agent::Usage {
                input_tokens: 10,
                cache_creation_input_tokens: 34_419,
                cache_read_input_tokens: 151_613,
                output_tokens: 1_790,
                cache_creation_1h_input_tokens: 0,
            };
            run.turns = 5;
            run.start_ctx = 30_748;
            run.end_ctx = 42_784;
        }
        run
    }
}
```

`agent_end_carries_usage_fields`: build with `UsageRunner(common::scripted_auto(builder_effects()))` under `factory_telemetry::capture("info", ..)`; take the `agent.end` with `role == "builder"` and assert `input == 10`, `cache_creation == 34419`, `cache_read == 151613`, `output == 1790`, `turns == 5`, `resumed == false`, `start_ctx == 30748`, `end_ctx == 42784`, `session_id == "sess-1"`, `round == fx.job().round` (read before the run), and that no value in the line equals the string `[redacted]`.

`agent_end_session_id_is_empty_without_a_session`: same, with a wrapper that sets `run.session_id = None`; assert `session_id == ""` and the field exists.

In `tests/task_carry.rs`, `the_carried_builder_logs_agent_end_resumed`: build as `task_two_continues_the_session_when_gap_and_context_allow` does, wrap both `build_stage` calls in `capture("info", ..)`, assert the first builder `agent.end` has `resumed == false` and the second `resumed == true`.

- [ ] **Step 3: Run them to verify they fail**

Run: `cargo test -j4 -p factory-engine --test log_events agent_end` and `cargo test -j4 -p factory-engine --test task_carry the_carried_builder_logs_agent_end_resumed`
Expected: FAIL, `Null` is not equal to the expected value for the new fields.

- [ ] **Step 4: Add the fields to the `agent.end` event in `Engine::run_agent`**

Read the round with `self.job().map_or(1, |j| j.round)`, the same fallback `log_usage` has. `session_id` is `run.session_id.as_deref().unwrap_or("")`. Keep the event name and every existing field.

- [ ] **Step 5: Run the tests again**

Run: `cargo test -j4 -p factory-engine --test log_events` and `cargo test -j4 -p factory-engine --test task_carry`
Expected: PASS, no existing test fails.

### Task 2: `task.start` and `task.end`

**Files:**
- Modify: `crates/factory-engine/src/build.rs` (`build_task`)
- Test: `crates/factory-engine/tests/task_carry.rs`

**Interfaces:**
- Consumes: `one_line(&str) -> String` (`pub(crate)` in `ports.rs`), `factory_telemetry::cap_message`, the `how` string and `task: &PlanTask { n, title }` already in `build_task`.
- Produces: `task.start` with `task` (u32), `title`, `how`, `model`, `of` (u32); `task.end` with `task`, `sha` (full new tip), `of`, `duration_ms`; both with a short `msg`.

- [ ] **Step 1: Read `build_task` in `build.rs` and the fixtures at the top of `tests/task_carry.rs`**

- [ ] **Step 2: Write the failing tests in `tests/task_carry.rs`** (reuse `two_task_fixture`, `two_effects`, `Tagged`, `scripted_auto`)

`task_start_and_end_are_logged_per_task`: run `e.build_stage()` twice under `capture("info", ..)` on `Tagged::new(scripted_auto(two_effects()), 30_000)`. Assert two `task.start` with `task` 1 and 2, `title` `first thing` and `second thing`, `of == 2`, `how == "fresh"`, `model == fx.config.builder_model`; two `task.end` with `of == 2`, `sha` of 40 hex characters, the second `sha` equal to `fx.repo.tip(&fx.job().branch)`, the two `sha` values different; `duration_ms` present.

`parked_task_has_no_task_end`: one builder effect that changes nothing (`Box::new(|_| "did nothing".to_string())`); assert one `task.start`, no `task.end`, one `park`.

`task_start_repeats_when_the_task_is_re_entered`: run as in the previous test, then reset the stage with `Board::load` / `find_mut("001")` / `set_stage(Stage::Build, None, true, 2.0)` / `save` as `two_task_fixture` does, and run `build_stage` again with an effect that changes `a.txt`; assert two `task.start` for task 1 and one `task.end`.

`task_start_title_is_one_line_and_capped`: a plan whose task heading is `### Task 1: ` plus 400 `x` characters; assert `title` has no newline and at most 300 characters.

- [ ] **Step 3: Run them to verify they fail**

Run: `cargo test -j4 -p factory-engine --test task_carry task_start` and `... parked_task_has_no_task_end`
Expected: FAIL, no `task.start` event found.

- [ ] **Step 4: Log the events in `build_task`**

Take `std::time::Instant::now()` at the top. Log `task.start` right after the first `Dispatched` `ledger_task` call (the retry after a failed resume does not log again). Log `task.end` right after the `ImplementerDone` `ledger_task` call with `sha = %new_tip`. `title` is `cap_message(&one_line(&task.title))`.

- [ ] **Step 5: Run the tests again**

Run: `cargo test -j4 -p factory-engine --test task_carry`
Expected: PASS.

### Task 3: `commit.done`

**Files:**
- Modify: `crates/factory-engine/src/commit.rs` (`commit_step`), `crates/factory-engine/src/build.rs` (the `commit_batch` snapshot commit)
- Test: `crates/factory-engine/tests/commit.rs`, `crates/factory-engine/tests/task_carry.rs`

**Interfaces:**
- Consumes: `CommitOutcome::Committed { tip, fallback }`, `CommitKind::as_str`.
- Produces: `commit.done` with `kind` (`task`, `legacy`, `fix`, `batch`, or `snapshot`), `sha` (full tip), `fallback` (bool).

- [ ] **Step 1: Read `commit_step` in `commit.rs`, the snapshot branch in `build_task`, and `one_valid_commit_is_accepted_and_the_run_is_trimmed_and_capped` in `tests/commit.rs`**

- [ ] **Step 2: Write the failing tests**

In `tests/commit.rs`, each wrapped in `factory_telemetry::capture("info", ..)`:
- `commit_done_is_logged_for_an_agent_commit`: as the test above does; assert one `commit.done` with `kind == "task"`, `fallback == false` and `sha == fx.repo.tip(&wt.branch)`.
- `commit_done_marks_the_fallback`: as `two_failed_runs_fall_back_to_a_plain_commit_and_write_a_ruling` does; assert one `commit.done` with `fallback == true`.
- `no_commit_done_when_nothing_is_staged`: as `nothing_staged_makes_no_agent_call` does; assert no `commit.done`.

In `tests/task_carry.rs`, `batch_snapshot_logs_commit_done_with_kind_snapshot`: set up as `the_batch_snapshot_and_reset_work_with_a_carry` does, wrap both `build_stage` calls in `capture`; assert exactly two `commit.done` with `kind == "snapshot"`, `fallback == false`, and each `sha` of 40 hex characters.

- [ ] **Step 3: Run them to verify they fail**

Run: `cargo test -j4 -p factory-engine --test commit commit_done` and `cargo test -j4 -p factory-engine --test task_carry batch_snapshot_logs`
Expected: FAIL, no `commit.done` event found.

- [ ] **Step 4: Implement**

Rename the current `commit_step` body to a private `commit_step_inner` with the same signature; the public `commit_step(&self, wt, req) -> Result<CommitOutcome, EngineError>` calls it and, on `Ok(CommitOutcome::Committed { tip, fallback })`, logs `commit.done` with `kind = req.kind.as_str()`, `sha = %tip`, `fallback`, and a short `msg`, then returns the outcome unchanged. In `build_task`, right after the snapshot `commit_staged` call returns its sha, log `commit.done` with `kind = "snapshot"`, `fallback = false`.

- [ ] **Step 5: Run the crate's tests and clippy**

Run: `cargo test -j4 -p factory-engine` then `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`
Expected: all tests pass, no warnings.

Next plan: docs/superpowers/plans/2026-10-24-structured-logging/plan-02-structured-logging.md
