# Structured Logging, Plan 2: Engine events

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Make the engine emit the structured events of the spec, and replace its `eprintln!` failure sites with `error` events.

**Architecture:** The engine calls `tracing` macros directly. They do nothing without a subscriber, so existing tests are unaffected. New tests run the code under `factory_telemetry::capture` and assert on the captured JSON objects. One helper, `set_stage_logged`, is the single place that logs `stage.change`.

**Tech Stack:** Rust, `tracing` 0.1, `factory-telemetry` from plan 1.

**Spec:** docs/superpowers/specs/2026-10-19-structured-logging-design.md

## Global Constraints

- Always `-j4`; never `--workspace` or `--all`; never set `CARGO_TARGET_DIR`.
- Logging never changes a result: a stage, a merge and the best-effort steps behave exactly as before.
- Events and fields (spec section 4): `job.created` (`slug`, `series`), `stage.change` (`from`, `to`, `reason`, `stage` = the new stage), `agent.start` and `agent.end` (`role`, `model`, `stop_reason`, `weighted_tokens`, `duration_ms`), `check.run` (`command`, `passed`, `summary`), `git.op` at debug (`op`, `ok`, `duration_ms`), `decide` (`choice`, `round`), `merge.done` (`sha`), `park` at warn (`reason`), `step.error` at error (`step`, `msg`).
- Never log prompts, agent output, environment variables or command output. A check `command` goes through `factory_telemetry::redact_command`.
- The old stderr text of each `eprintln!` site stays as the event `msg` so a user sees the same words.
- The board JSON, the ledger and `events.jsonl` are not changed.
- A `msg` is at most 300 characters (the layer cuts it).

## Review Focus

- A `set_stage` to the same stage logs nothing (Task 1 test `same_stage_is_not_logged`).
- A failed `set_stage` (an illegal move) logs nothing and returns the same error (Task 1 test `illegal_stage_move_logs_nothing`).
- A park reason with newlines becomes a single-line `reason` (Task 1 test `park_reason_is_one_line`).
- An agent run that fails still logs `agent.end` with its `stop_reason` (Task 2 test `failed_agent_run_logs_agent_end`).
- A check command with `--token=abc` is logged redacted (Task 2 test `check_command_secret_is_redacted`).
- A failing kb refresh or tickets step logs `step.error` and the merge result stays `Merged` (Task 3 test `kb_failure_logs_step_error_and_merge_result_is_unchanged`).

---

## File Structure

- `crates/factory-engine/Cargo.toml` (modify): `tracing.workspace = true`, `factory-telemetry = { path = "../factory-telemetry" }`.
- `crates/factory-engine/src/ports.rs` (modify): `set_stage_logged`, `Engine::set_stage`, `Engine::park`, `run_agent`.
- `crates/factory-engine/src/runloop.rs`, `merge.rs`, `build.rs`, `checks_stage.rs`, `sync.rs`, `review.rs`, `tickets.rs` (modify): call sites.
- `crates/factory-engine/src/lib.rs` (modify): `pub use ports::set_stage_logged;` and `pub use runloop::job_span;` if not already reachable.
- `crates/factory-engine/tests/log_events.rs` (new): the capture tests of all three tasks, using the fixtures of `crates/factory-engine/tests/common/mod.rs`.

### Task 1: Stage changes, job creation, parks and spans

**Files:**
- Modify: `crates/factory-engine/Cargo.toml`, `src/ports.rs`, `src/runloop.rs`, `src/merge.rs`, `src/build.rs`, `src/checks_stage.rs`, `src/sync.rs`, `src/lib.rs`
- Create: `crates/factory-engine/tests/log_events.rs`

**Interfaces:**
- Produces:
  - `pub fn set_stage_logged(job: &mut factory_core::Job, to: Stage, reason: Option<&str>, force: bool, now: f64) -> Result<(), CoreError>`: reads `job.stage`, calls `job.set_stage(to, reason, force, now)`, and when it succeeds and the stage changed, emits `tracing::info!(event = "stage.change", job_id, from, to, reason, stage = to, msg = "stage <from> -> <to>")`.
  - `pub fn job_span(job_id: &str) -> tracing::Span` (`info_span!("job", job_id)`).
  - `park` events: `tracing::warn!(event = "park", job_id, reason = <one-line reason>)` from `Engine::park` in `ports.rs` and `park` in `merge.rs`.
  - `job.created` emitted at the end of `add_job` in `runloop.rs`.

- [ ] **Step 1: Write the failing tests** in `tests/log_events.rs`, each built on the existing fixtures and wrapped in `factory_telemetry::capture("info", ...)`:
  - `stage_change_is_logged_with_from_and_to`: moving a job build→review gives one `stage.change` object with `from == "build"`, `to == "review"`, `stage == "review"`, `job_id` set.
  - `same_stage_is_not_logged`; `illegal_stage_move_logs_nothing` (merged→build without force returns `Err`, zero events).
  - `park_logs_a_warning`: `park(driver, id, "risky diff")` gives a `level == "warn"` object with `event == "park"` and `reason == "risky diff"`.
  - `park_reason_is_one_line`: a reason `"a\nb"` is logged as `"a b"`.
  - `job_created_is_logged_with_slug`.
  - `run_job_lines_carry_the_job_id`: a full `run_job` of the existing fake-runner fixture gives only objects that have `job_id` or are `job.created`.
- [ ] **Step 2: Run** `cargo test -j4 -p factory-engine --test log_events` and expect FAIL.
- [ ] **Step 3: Implement** the helper and replace every `job.set_stage(...)` / `j.set_stage(...)` call listed by `grep -n "set_stage(" crates/factory-engine/src` (ports.rs:77, build.rs:564, merge.rs:31 and 148, checks_stage.rs:109, sync.rs:44, runloop.rs:38 and 398) with `set_stage_logged`. `run_job` enters `job_span(id)` for the whole run, and the loop in `run_job_inner` enters `info_span!("stage", stage = job.stage.as_str())` around each match arm. One-line reasons use `split_whitespace().collect::<Vec<_>>().join(" ")`.
- [ ] **Step 4: Run** `cargo test -j4 -p factory-engine` and expect PASS (existing tests unchanged).

### Task 2: Agent runs, checks, git calls and decisions

**Files:**
- Modify: `crates/factory-engine/src/ports.rs`, `src/checks_stage.rs`, `src/review.rs`, `src/merge.rs`, `src/sync.rs`
- Test: `crates/factory-engine/tests/log_events.rs`

**Interfaces:**
- Consumes: Task 1 spans and helpers.
- Produces:
  - In `Engine::run_agent`: `agent.start` before `self.runner.run(&spec)` (`role = spec.role`, `model = spec.model`), `agent.end` after it (`stop_reason` = the `Debug` text of `run.stop_reason`, `weighted_tokens = run.weighted`, `duration_ms` from `std::time::Instant`), both info.
  - In `run_checks_now` and `run_checks_between`: one `check.run` info event per `CheckResult` (`command = redact_command(&r.command)`, `passed`, `summary`).
  - `git.op` debug events: around each `Repo` call in `merge.rs` (`try_merge`) and `sync.rs` (`sync_stage`), take `let t = std::time::Instant::now();` before the call and emit `tracing::debug!(event = "git.op", op = "<call name>", ok = r.is_ok(), duration_ms = t.elapsed().as_millis() as u64)` after it, where `r` is the call's `Result`.
  - `decide` info event where `decide(&DecideInput)` is called (find it with `grep -n "decide(" crates/factory-engine/src`): `choice` is `merge`, `fix` or `needs-human` by the `Decision` variant, `round` is the job round.

- [ ] **Step 1: Write the failing tests**
  - `agent_run_logs_start_and_end`: one run gives `agent.start` then `agent.end`, `role` set, `weighted_tokens` equal to the fake run's weighted value.
  - `failed_agent_run_logs_agent_end`: a runner returning a stop reason other than done still gives `agent.end` with that `stop_reason`.
  - `check_run_is_logged_per_command`: a config with two checks gives two `check.run` objects with `passed` booleans.
  - `check_command_secret_is_redacted`: a check command `echo --token=abc` is logged as `echo --token=[redacted]` and the string `abc` appears in no line.
  - `decide_logs_the_choice`: a clean small diff gives `choice == "merge"`.
  - `git_ops_log_at_debug_only`: the merge run gives `git.op` lines in `capture("debug", ..)` and none in `capture("info", ..)`.
- [ ] **Step 2: Run** `cargo test -j4 -p factory-engine --test log_events` and expect FAIL.
- [ ] **Step 3: Implement** the events as described. Do not log `spec.prompt`, `run.result_text`, `spec.env` or `r.tail`.
- [ ] **Step 4: Run** `cargo test -j4 -p factory-engine` and expect PASS.

### Task 3: Merge result and the former `eprintln!` sites

**Files:**
- Modify: `crates/factory-engine/src/merge.rs`, `src/runloop.rs`, `src/tickets.rs`
- Test: `crates/factory-engine/tests/log_events.rs`

**Interfaces:**
- Consumes: Task 1 spans.
- Produces:
  - `merge.done` info event with `sha` (the full merged sha) in `merge_stage` right after `finish_merge`.
  - Each `eprintln!` in `merge.rs` (kb refresh, tickets, costs refresh), `runloop.rs` (`note_failure`, two lines) and `tickets.rs` (`finish`) becomes `tracing::error!(event = "step.error", step = "<name>", msg = <the same text>)` with `step` one of `kb_refresh`, `tickets`, `costs_refresh`, `note_failure`, `note_failure_ledger`, `tickets_ledger`. The message text is unchanged.

- [ ] **Step 1: Write the failing tests**
  - `merge_logs_sha`: the merged job gives `merge.done` with `sha` equal to the `MergeResult::Merged` value.
  - `kb_failure_logs_step_error_and_merge_result_is_unchanged`: a failing kb refresh (use the existing failing-runner fixture of `tests/kb.rs`) gives a `level == "error"` object with `step == "kb_refresh"` whose `msg` starts with `kb refresh error for job`, and the merge result is `Merged`.
  - `tickets_failure_logs_step_error`: with `tickets_on_merge` and a failing tracker fake from `tests/tickets.rs`, a `step == "tickets"` error object appears and the merge result is `Merged`.
  - `note_failure_logs_step_error_with_the_slug`: a failing ledger note in `new` gives `step == "note_failure"` and `msg` contains the slug.
- [ ] **Step 2: Run** `cargo test -j4 -p factory-engine --test log_events` and expect FAIL.
- [ ] **Step 3: Implement** the replacements. Confirm no `eprintln!` is left outside tests: `grep -n "eprintln!" crates/factory-engine/src` shows only test code.
- [ ] **Step 4: Run** `cargo test -j4 -p factory-engine` and `cargo check -j4 -p factory-cli` and expect PASS.

Next plan: docs/superpowers/plans/2026-10-19-structured-logging/plan-03-structured-logging.md
