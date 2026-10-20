# Builder task resume, plan 1: end-of-run context

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Every agent run reports the context it ended with as `end_ctx`, `usage.jsonl` lines carry it with `usage_v` 4, and the COLD STARTS table of `factory tokens --cache` shows an AVG-END column.

**Architecture:** `StreamState` keeps the merged usage of the last main-thread assistant message. `AgentRun` carries `end_ctx`, `usage_line` writes it, `UsageRecord` reads it, and the report averages it per role.

**Tech Stack:** Rust, serde_json.

**Spec:** docs/superpowers/specs/2026-10-20-builder-task-resume-design.md

## Global Constraints

- Always `-j4`. Never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- No `unwrap` in production code. Tests use `.expect("...")` and `expect_err`.
- `end_ctx` is a new optional key of a `usage.jsonl` line; readers treat a missing key as absent. `usage_v` becomes 4.
- A line counts as main thread when `parent_tool_use_id` is null or absent. Lines with an id (helper agents) never feed `end_ctx`.
- `start_ctx`, `start_cc` and `start_cr` keep their meaning.

## Review Focus

1. A builder whose last assistant line comes from a helper agent: `end_ctx` still reflects the main thread (test `end_ctx_ignores_helper_agent_messages`, Task 1).
2. Streaming snapshots of one message id: the merged (largest) usage counts, not the first or last raw line (test `end_ctx_uses_the_merged_usage_of_the_last_message`, Task 1).
3. A run killed before any message: `end_ctx` is 0 and the usage line still carries the key (test `a_failed_run_writes_zero_end_ctx`, Task 2).
4. Old `usage.jsonl` lines: no `end_ctx`, so AVG-END shows `-` and nothing errors (test `old_lines_have_no_end_ctx`, Task 3).
5. A resume of a session that no longer exists must end as `Failed`, because the fallback of plan 3 depends on it (test `a_missing_session_resume_is_classified_as_failed`, Task 1).

## File Structure

- `crates/factory-agent/src/stream.rs` (modify): `last_usage`, `end_ctx`.
- `crates/factory-agent/src/run.rs` (modify): `AgentRun.end_ctx`.
- `crates/factory-agent/src/claude.rs` (modify): fill `end_ctx`; classify a missing-session resume.
- `crates/factory-agent/src/bin/fake-claude.rs` (modify): one new mode.
- `crates/factory-agent/tests/stream.rs`, `tests/runner.rs` (modify): tests.
- `crates/factory-engine/src/ports.rs` (modify): `usage_line`.
- `crates/factory-engine/tests/runloop.rs` (modify): the `usage_v` assertion.
- `crates/factory-core/src/cost.rs` (modify, test only): one test.
- `crates/factory-cli/src/tokens.rs`, `tests/tokens_cache.rs` (modify): record field, column, tests.
- `README.md` (modify): the "Cache report" paragraph.

### Task 1: end_ctx in the stream and the AgentRun

**Files:**
- Modify: `crates/factory-agent/src/stream.rs`, `run.rs`, `claude.rs`, `src/bin/fake-claude.rs`
- Test: `crates/factory-agent/tests/stream.rs`, `crates/factory-agent/tests/runner.rs`

**Interfaces:**
- Consumes: `StreamState::on_assistant`, `usage_by_message` (merged with `max_with`), `Usage`, `feed_lines` and `MSG_1`/`MSG_2` in `tests/stream.rs` (MSG_2 has input 5, creation 0, read 1100), the `spec(mode, dir)` and `runner(sink)` helpers in `tests/runner.rs`.
- Produces: `StreamState::end_ctx(&self) -> u64`; `AgentRun.end_ctx: u64` (0 in `AgentRun::done`); a fake-claude mode `missing-session`. The logic is in the agent crate and sits behind the `AgentRunner` port.

- [ ] **Step 1: Read** `stream.rs` (`first_usage`, `on_assistant`, `start_ctx`), `run.rs` (`AgentRun`, `done`, `failed`), the end of `run_inner` in `claude.rs` (where `StopReason` is chosen from the result event and exit code), `src/bin/fake-claude.rs` (modes are chosen by the first stdin line) and the `start_ctx_*` tests in `tests/stream.rs`.
- [ ] **Step 2: Write the failing tests.** In `tests/stream.rs` add a line builder and the tests. One full example:

```rust
fn msg(id: &str, parent: Option<&str>, input: u64, cc: u64, cr: u64) -> String {
    let parent = parent.map_or("null".to_string(), |p| format!("\"{p}\""));
    format!(
        r#"{{"type":"assistant","parent_tool_use_id":{parent},"message":{{"id":"{id}","role":"assistant","content":[],"usage":{{"input_tokens":{input},"cache_creation_input_tokens":{cc},"cache_read_input_tokens":{cr},"output_tokens":1}}}},"session_id":"s"}}"#
    )
}

#[test]
fn end_ctx_is_the_last_messages_input_creation_and_read() {
    let state = feed_lines(&[MSG_1, MSG_2]);
    assert_eq!(state.end_ctx(), 5 + 0 + 1100);
}

#[test]
fn end_ctx_ignores_helper_agent_messages() {
    let a = msg("a", None, 1, 100, 1000);
    let h = msg("h", Some("toolu_1"), 9, 9, 9);
    let state = feed_lines(&[a.as_str(), h.as_str()]);
    assert_eq!(state.end_ctx(), 1 + 100 + 1000);
}
```

  The other tests:

| Test | Input | Assertion |
|---|---|---|
| `end_ctx_uses_the_merged_usage_of_the_last_message` | `msg("a", None, 1, 100, 1000)` then `msg("a", None, 1, 100, 1500)` | `end_ctx() == 1 + 100 + 1500` |
| `end_ctx_is_zero_without_messages` | `StreamState::default()` | `end_ctx() == 0` |
| `end_ctx_is_zero_when_only_a_helper_spoke` | one `msg("h", Some("toolu_1"), 9, 9, 9)` | `end_ctx() == 0` |
| `end_ctx_does_not_overflow` | one main-thread message with `u64::MAX` creation and read | `end_ctx() == u64::MAX` |
| `a_done_run_has_zero_end_ctx` | `AgentRun::done("x")` | `end_ctx == 0` |

  In `tests/runner.rs`: `a_missing_session_resume_is_classified_as_failed` runs `spec("missing-session", tmp.path())` with `resume = Some("no-such-session")` and asserts `run.stop_reason == StopReason::Failed`.
- [ ] **Step 3: Run** `cargo test -j4 -p factory-agent --test stream end_ctx` and expect a compile failure `no method named 'end_ctx' found for struct 'StreamState'`.
- [ ] **Step 4: Implement** `last_usage: Option<Usage>` in `StreamState`, set in `on_assistant` to the merged usage of the message id when the line is main thread (`value["parent_tool_use_id"]` is null or missing); `pub fn end_ctx(&self) -> u64` as the saturating sum of input, creation and read of `last_usage`, 0 when none. Add `end_ctx: u64` to `AgentRun` after `start_ctx`, 0 in `done`, filled in `claude.rs` from `state.end_ctx()`. Add the fake-claude mode `missing-session`: it prints a result line with `"is_error":true` and `"subtype":"error_during_execution"` and exits with code 1, as the real `claude --resume` of an unknown id does (`No conversation found with session ID` on stderr). If the new runner test shows another stop reason, map an `is_error` result with that subtype and exit code 1 to `StopReason::Failed` in `claude.rs`.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-agent` and expect every test to PASS, then `cargo check -j4 -p factory-engine`. The engine tests build `AgentRun` with `..AgentRun::done(..)` and compile unchanged.

### Task 2: usage_line writes end_ctx and usage_v 4

**Files:**
- Modify: `crates/factory-engine/src/ports.rs`, `crates/factory-engine/tests/runloop.rs`
- Test: `crates/factory-engine/src/ports.rs` (unit tests), `crates/factory-core/src/cost.rs` (unit test)

**Interfaces:**
- Consumes: `AgentRun.end_ctx` (Task 1), `usage_line(job_id, spec, run, round, tasks_done, ts, secs) -> serde_json::Value`.
- Produces: `usage.jsonl` lines with `"end_ctx"` and `"usage_v": 4`. `end_ctx` is a new optional key for every reader. `usage_line` is a pure function; the `AgentRunner` port supplies the `AgentRun`.

- [ ] **Step 1: Read** `usage_line` and its tests in `ports.rs` (`run_with`, the `usage_v` assertions near lines 334, 391 and 400), the `usage_v` assertion in `tests/runloop.rs` near line 233, and the `usage_v < 2` check in `cost.rs` with `a_usage_v_3_line_is_not_approximate`.
- [ ] **Step 2: Write the failing tests.** In `ports.rs` make `run_with` also set `end_ctx: 70`, change every `usage_v` assertion from 3 to 4, and add:

```rust
#[test]
fn usage_line_carries_end_ctx() {
    let spec = AgentSpec::new("001", "builder", "/w", "p");
    let line = usage_line("001", &spec, &run_with(StopReason::Done), 1, 0, 10, 5);
    assert_eq!(line["end_ctx"], 70);
    assert_eq!(line["usage_v"], 4);
}
```

  The other tests:

| Test | Where | Input | Assertion |
|---|---|---|---|
| `a_failed_run_writes_zero_end_ctx` | `ports.rs` | `AgentRun::failed("x")` | `line["end_ctx"] == 0` |
| `a_usage_v_4_line_is_not_approximate` | `cost.rs` | copy the setup of `a_usage_v_3_line_is_not_approximate` with `"usage_v":4` | the approximate flag is false |

  In `tests/runloop.rs` change the `usage_v` assertion to 4.
- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine usage_line` and expect FAIL with `left: Number(3)` / `right: 4` and a missing `end_ctx`.
- [ ] **Step 4: Implement** in `usage_line`: add `"end_ctx": run.end_ctx` and set `"usage_v": 4`; update the doc comment (`usage_v` 4 marks lines with `end_ctx`).
- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine usage_line`, `cargo test -j4 -p factory-engine --test runloop`, `cargo test -j4 -p factory-core cost` and expect PASS in all.

### Task 3: AVG-END in the COLD STARTS table

**Files:**
- Modify: `crates/factory-cli/src/tokens.rs`, `README.md`
- Test: `crates/factory-cli/src/tokens.rs` (unit tests), `crates/factory-cli/tests/tokens_cache.rs`

**Interfaces:**
- Consumes: `UsageRecord`, `load_usage`, `RoleCold`, `cache_report`, `render_cold` and the `cold_rec` test helper from the COLD STARTS report.
- Produces:
  - `UsageRecord.end_ctx: Option<u64>`, `None` on old lines and on non-numeric values (a new optional field).
  - `RoleCold.end_rows: u64` and `RoleCold.end_sum: u64` (over every counted run of the role, fresh or resumed, that has `end_ctx`) and `RoleCold::avg_end(&self) -> Option<u64>` (`end_sum / end_rows`, `None` when `end_rows == 0`).
  - A column AVG-END after FIRST-CC% in the role table of the COLD STARTS section, `-` when `avg_end()` is `None`. The job table is unchanged. The logic is a pure function of loaded rows and sits behind no port.

- [ ] **Step 1: Read** `tokens.rs` (`UsageRecord`, `load_usage`, `RoleCold`, `cache_report`, `render_cold`, the cold unit tests and `cold_rec`) and `tests/tokens_cache.rs`. Existing render tests that match the header text must be updated for the new column.
- [ ] **Step 2: Write the failing tests.** One full example:

```rust
#[test]
fn cold_report_averages_end_ctx_over_runs_that_have_it() {
    let mut a = cold_rec("001", "builder", false, 10, 1000, Some((400, 100)));
    a.end_ctx = Some(30_000);
    let mut b = cold_rec("001", "builder", true, 20, 1000, Some((400, 100)));
    b.end_ctx = Some(50_000);
    let c = cold_rec("001", "builder", false, 30, 1000, Some((400, 100)));
    let r = cache_report(&[vec![a, b, c]], None, 100);
    let role = r.cold_roles.iter().find(|x| x.role == "builder").expect("builder");
    assert_eq!((role.end_rows, role.end_sum), (2, 80_000));
    assert_eq!(role.avg_end(), Some(40_000));
}
```

  The other tests:

| Test | Where | Input | Assertion |
|---|---|---|---|
| `load_usage_reads_end_ctx` | `tokens.rs` | a line with `"end_ctx":31000,"usage_v":4` | `end_ctx == Some(31000)` |
| `old_lines_have_no_end_ctx` | `tokens.rs` | `{"job":"001","role":"builder","usage_v":3}` | `end_ctx == None` |
| `a_non_numeric_end_ctx_is_absent` | `tokens.rs` | `"end_ctx":"big"` | `None`, no error |
| `cold_section_shows_the_average_end_context` | `tokens.rs` | the report above | output contains `AVG-END` and `40000` |
| `cold_section_dashes_avg_end_without_data` | `tokens.rs` | rows without `end_ctx` | the builder row ends with `-` in that column |
| `tokens_cache_prints_avg_end` | `tests/tokens_cache.rs` | one job with a usage line holding `start_cc`, `start_cr`, `end_ctx` | output contains `AVG-END` |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-cli --lib end_ctx` and expect a compile failure `no field 'end_ctx' on type 'UsageRecord'`.
- [ ] **Step 4: Implement** the fields, the aggregation inside `cache_report`, `avg_end` and the column in `render_cold`. Update the README "Cache report" paragraph: name the AVG-END column (mean end-of-run context of the role's runs that logged it) and say older runs show `-`.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-cli --lib tokens`, `cargo test -j4 -p factory-cli --test tokens_cache` and expect PASS. Then `cargo clippy -j4 -p factory-agent -p factory-engine -p factory-core -p factory-cli --all-targets -- -D warnings` and `cargo fmt -p factory-agent -p factory-engine -p factory-core -p factory-cli -- --check`, both clean.

Next plan: docs/superpowers/plans/2026-10-20-builder-task-resume/plan-02-builder-task-resume.md
