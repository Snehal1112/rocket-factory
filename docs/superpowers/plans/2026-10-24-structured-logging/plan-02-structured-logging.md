# Structured Logging CLI Side Implementation Plan

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Add `factory log --event <name-or-prefix>`, log `operator.approve`, `operator.rework` and `operator.cancel` from the CLI commands, and document every new event.

**Architecture:** The filter lives in `factory-telemetry::read` (`LogFilter.event`) and is passed through `run_log` from the `Log` command. The operator events are `tracing::info!` calls in `commands.rs`, inside the `job_span` those commands already enter. Plan 1 already added the engine events; this plan reads and documents them.

**Tech Stack:** Rust, `clap`, `tracing`, `factory_telemetry::capture`.

**Spec:** `docs/superpowers/specs/2026-10-24-structured-logging-design.md`

## Global Constraints

- `--event` matches a line when its `event` equals the value, or starts with the value followed by `.`; an empty value is refused with `empty event, use a name such as task or agent.end`.
- The filter combines with `--job`, `--level` and `--tail` by AND, and `--tail` applies last.
- Operator events fire right after the board `update_job` call that changes the state succeeds; a refused command logs no `operator.*` line. They are `info`, and only the CLI commands log them.
- A log failure never changes a stage, a merge or a command.
- Cargo per crate with `-j4`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- This plan adds no field to the board, ledger or events log. The logic sits behind no port: it is the CLI layer and `factory-telemetry`.

## Review Focus

1. `--event tas` (a partial word) matches nothing (`event_filter_ignores_a_partial_word`, Task 1).
2. `--event agent.end` matches `agent.end` but not `agent.start` or `agent.endless`: only the exact name or the `name.` prefix match (`event_filter_matches_exact_and_prefix`, Task 1).
3. A rework note with quotes and a backslash is written as valid JSON and read back equal (`rework_logs_the_note`, Task 2).
4. A refused `approve` (a job in `build`) logs nothing (`refused_approve_logs_nothing`, Task 2).
5. `cancel --force` records `force` true (`cancel_logs_reason_and_force`, Task 2).

## File Structure

- Modify `crates/factory-telemetry/src/read.rs`: `LogFilter.event` and the match.
- Modify `crates/factory-cli/src/cli.rs`, `crates/factory-cli/src/log.rs`, `crates/factory-cli/src/commands.rs`: the flag, `run_log`, the operator events.
- Create `crates/factory-cli/tests/operator_events.rs`.
- Modify tests `crates/factory-cli/tests/log_cmd.rs` (new `run_log` argument).
- Modify docs `docs/factory-admin-manual.md`, `docs/software-factory.md`.

---

### Task 1: `factory log --event`

**Files:**
- Modify: `crates/factory-telemetry/src/read.rs`, `crates/factory-cli/src/cli.rs`, `crates/factory-cli/src/log.rs`, `crates/factory-cli/src/commands.rs`, `docs/factory-admin-manual.md`
- Test: `crates/factory-telemetry/src/read.rs` (unit), `crates/factory-cli/src/cli.rs` (unit), `crates/factory-cli/tests/log_cmd.rs`

**Interfaces:**
- Consumes: `LogFilter { job, min_level, tail }`, `read_log(&Path, &LogFilter)`, `run_log(ctx, job, level, tail)`.
- Produces: `LogFilter { job, min_level, event: Option<String>, tail }`; `run_log(ctx: &CliCtx, job: Option<&str>, level: Option<&str>, event: Option<&str>, tail: usize) -> Result<String, CliError>`; `Command::Log { job, level, event: Option<String>, tail }` with `--event <EVENT>`.

- [ ] **Step 1: Read `read.rs`, `log.rs`, the `Log` variant in `cli.rs` and `tests/log_cmd.rs`**

- [ ] **Step 2: Write the failing tests**

In `read.rs` tests, with the existing `line` helper extended to take an event (a second helper `event_line(event, job, level)` building `{"event":"..","job_id":"..","level":".."}`):
- `event_filter_matches_exact_and_prefix`: lines `task.start`, `task.end`, `agent.end`; `event: Some("task")` returns the first two, `event: Some("agent.end")` returns the third.
- `event_filter_ignores_a_partial_word`: `event: Some("tas")` returns nothing.
- `event_filter_combines_with_job_and_level`: with a job and a level set too, only lines matching all three come back.
- `event_filter_then_tail`: two matching lines and `tail: 1` returns the last one.

Update the two existing `LogFilter { .. }` literals in `read.rs` tests (add `event: None`, or `..Default::default()`).

In `cli.rs` tests: `log_parses_event_flag` for `["factory", "log", "--event", "task"]` giving `event: Some("task")`, and update `log_tail_defaults_to_100` and `log_parses_flags` for the new field.

In `tests/log_cmd.rs`: `log_event_empty_is_refused` (`run_log(&ctx, None, None, Some(""), 100)` is `CliError::Refused`), `log_event_filters_by_prefix` (a file with `task.start`, `task.end` and `agent.end` lines; `Some("task")` returns the first two), and add `None` as the new argument in every existing `run_log` call there.

- [ ] **Step 3: Run them to verify they fail**

Run: `cargo test -j4 -p factory-telemetry event_filter`
Expected: FAIL to compile, no field `event` on `LogFilter`.

- [ ] **Step 4: Implement**

`LogFilter.event` (derives `Default`, so it stays `None`). In `read_log`, skip a line unless `v["event"].as_str()` is `Some(e)` with `e == want` or `e.starts_with(&format!("{want}."))`. Add `#[arg(long)] event: Option<String>` to `Command::Log`, the `event` parameter to `run_log` (refuse an empty value before reading the file), and pass `event.as_deref()` from `dispatch` in `commands.rs`. In `docs/factory-admin-manual.md` extend the `log` row with `--event <name-or-prefix>` and an example `factory --project rocket-api log --event task`.

- [ ] **Step 5: Run the tests again**

Run: `cargo test -j4 -p factory-telemetry` and `cargo test -j4 -p factory-cli --lib` and `cargo test -j4 -p factory-cli --test log_cmd`
Expected: PASS.

### Task 2: Operator events and the logging docs

**Files:**
- Modify: `crates/factory-cli/src/commands.rs` (`approve`, `rework`, `cancel`), `docs/software-factory.md`
- Create: `crates/factory-cli/tests/operator_events.rs`

**Interfaces:**
- Consumes: `factory_telemetry::cap_message`, `factory_engine::job_span` (already entered by each command), the `Scenario` harness in `tests/support/mod.rs` (`Scenario::new`, `sc.start`, `sc.run`).
- Produces: `operator.approve` with `without_review` (bool); `operator.rework` with `note` (capped); `operator.cancel` with `reason` (capped) and `force` (bool); each with a short `msg`.

- [ ] **Step 1: Read `approve`, `rework` and `cancel` in `commands.rs`, `tests/support/mod.rs`, and `a_risky_diff_waits_for_approve_and_then_merges_the_reviewed_sha` in `tests/e2e.rs`**

- [ ] **Step 2: Write the failing tests in `tests/operator_events.rs`**

Park a job as that e2e test does (`Scenario::new("", vec![build(vec![("secrets/token.txt", "x\n")], 1_000), review(APPROVE)])` then `sc.start(&sc.plan_b, false)`), and wrap each command in `factory_telemetry::capture("info", || sc.run(..))`:
- `approve_logs_operator_approve`: `["approve", "1"]`; one `operator.approve` with `without_review == false` and `job_id == "001"`.
- `rework_logs_the_note`: `["rework", "1", "say \"hi\" \\ again"]`; one `operator.rework` whose `note` equals that exact text.
- `cancel_logs_reason_and_force`: `["cancel", "1", "--reason", "not needed", "--force"]`; one `operator.cancel` with `reason == "not needed"` and `force == true`. A second case without `--force` and the default reason: `reason == "cancelled by user"`, `force == false`.
- `refused_approve_logs_nothing`: create the job with `sc.run(&["new", ..])` as `Scenario::start` does but do not run it, so it is not in `needs-human`; `["approve", "1"]` is `Err(CliError::Refused(_))` and the captured lines hold no `operator.approve`.

- [ ] **Step 3: Run them to verify they fail**

Run: `cargo test -j4 -p factory-cli --test operator_events`
Expected: FAIL, no `operator.*` event found.

- [ ] **Step 4: Log the events**

`approve`: after the `update_job` that sets `Stage::Approve`, before the ledger ruling and the merge. `rework`: after the `update_job` that sets `Stage::Build`. `cancel`: after the `update_job` that sets `cancelled`. Each with `tracing::info!(event = "operator.<name>", .., msg = ..)`; `note` and `reason` through `cap_message`.

- [ ] **Step 5: Document the events**

In `docs/software-factory.md`, add `task.start`, `task.end`, `commit.done`, `operator.approve`, `operator.rework` and `operator.cancel` to the `event` row of the Logging table, add a sentence listing the extra `agent.end` fields (`input`, `cache_creation`, `cache_read`, `output`, `turns`, `resumed`, `round`, `start_ctx`, `end_ctx`, `session_id`), and extend the `factory log` line with `[--event <name-or-prefix>]` and its matching rule.

- [ ] **Step 6: Run the crate's tests, clippy and format check**

Run: `cargo test -j4 -p factory-cli` then `cargo clippy -j4 -p factory-telemetry -p factory-cli --all-targets -- -D warnings` then `cargo fmt --check`
Expected: all tests pass, no warnings, no format diff.

Next plan: none
