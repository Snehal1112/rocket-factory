# Cold-start savings, plan 1: first-turn cache logging

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Every agent run logs the cache creation and cache read of its first message as `start_cc` and `start_cr` in `usage.jsonl`, with `usage_v` 3.

**Architecture:** `StreamState` already keeps the first message's usage for `start_ctx`. It gets a method that returns the two parts, `AgentRun` carries them, and `usage_line` writes them. Old lines simply lack the keys.

**Tech Stack:** Rust, serde_json.

**Spec:** docs/superpowers/specs/2026-10-20-cold-start-savings-design.md

## Global Constraints

- Always `-j4`. Never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- No `unwrap` in production code. Tests use `.expect("...")` and `expect_err`.
- `start_cc` and `start_cr` are new optional keys of a `usage.jsonl` line. Readers must treat a missing key as absent. `usage_v` becomes 3 for new lines.
- `start_ctx` keeps its meaning: first message creation plus read.

## Review Focus

1. A first assistant message with no `usage` object: `start_cache` returns `(0, 0)` and never panics (test `start_cache_is_zero_for_a_first_message_without_usage`, Task 1).
2. A run that dies before any message (`AgentRun::failed`): both fields are 0 and the usage line still has the keys (test `a_failed_run_still_writes_zero_start_fields`, Task 2).
3. A reader of old lines is not affected: `factory-core` cost code tests `usage_v < 2` and a 3 passes it (test `a_usage_v_3_line_is_not_approximate`, Task 2).

## File Structure

- `crates/factory-agent/src/stream.rs` (modify): `StreamState::start_cache`.
- `crates/factory-agent/src/run.rs` (modify): `AgentRun.start_cc`, `AgentRun.start_cr`.
- `crates/factory-agent/src/claude.rs` (modify): fill the two fields when a run ends.
- `crates/factory-agent/tests/stream.rs` (modify): tests for `start_cache`.
- `crates/factory-engine/src/ports.rs` (modify): `usage_line` and its unit tests.
- `crates/factory-engine/tests/runloop.rs` (modify): the `usage_v` assertion.
- `crates/factory-core/src/cost.rs` (modify, test only): one test.

### Task 1: StreamState::start_cache and the AgentRun fields

**Files:**
- Modify: `crates/factory-agent/src/stream.rs`, `crates/factory-agent/src/run.rs`, `crates/factory-agent/src/claude.rs`
- Test: `crates/factory-agent/tests/stream.rs`

**Interfaces:**
- Consumes: `StreamState::first_usage: Option<Usage>`, `Usage.cache_creation_input_tokens`, `Usage.cache_read_input_tokens`, the fixtures `MSG_1` and `feed_lines` in `tests/stream.rs`.
- Produces: `StreamState::start_cache(&self) -> (u64, u64)` (creation, read), `AgentRun.start_cc: u64`, `AgentRun.start_cr: u64`.

- [ ] **Step 1: Read** `crates/factory-agent/src/stream.rs` (`first_usage`, `start_ctx`), `crates/factory-agent/src/run.rs` (`AgentRun`, `done`, `failed`), the end of `run_inner` in `crates/factory-agent/src/claude.rs` where `AgentRun { .. }` is built, and the `start_ctx_*` tests in `crates/factory-agent/tests/stream.rs`.
- [ ] **Step 2: Write the failing tests** in `crates/factory-agent/tests/stream.rs`, next to the `start_ctx_*` tests. Reuse `MSG_1`, `MSG_2` and `feed_lines` as `start_ctx_is_the_first_messages_cache_creation_plus_read` does.

```rust
#[test]
fn start_cache_is_the_first_messages_creation_and_read() {
    let state = feed_lines(&[MSG_1, MSG_2]);
    assert_eq!(state.start_cache(), (100, 1000));
}

#[test]
fn start_cache_is_zero_without_messages() {
    assert_eq!(StreamState::default().start_cache(), (0, 0));
}
```

  The other tests:

| Test | Input | Assertion |
|---|---|---|
| `start_cache_is_zero_for_a_first_message_without_usage` | one assistant line with `"id":"m"` and no `usage` key | `start_cache() == (0, 0)` |
| `start_cache_adds_up_with_start_ctx` | `feed_lines(&[MSG_1])` | `let (c, r) = state.start_cache(); c + r == state.start_ctx()` |
| `a_done_run_has_zero_start_cache` | `factory_agent::AgentRun::done("x")` | `start_cc == 0 && start_cr == 0` |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-agent --test stream start_cache` and expect a compile failure: `no method named 'start_cache' found for struct 'StreamState'`.
- [ ] **Step 4: Implement** `pub fn start_cache(&self) -> (u64, u64)` in `stream.rs`: `(0, 0)` without a first usage, otherwise `(cache_creation_input_tokens, cache_read_input_tokens)` of `first_usage`. Add `pub start_cc: u64` and `pub start_cr: u64` to `AgentRun` after `start_ctx`, set both to 0 in `AgentRun::done`, and fill them in `claude.rs` from `state.start_cache()` where `start_ctx` is filled. The first-seen snapshot is the source, as for `start_ctx`.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-agent` and expect every test to PASS, then `cargo check -j4 -p factory-engine`. The engine tests build `AgentRun` with `..AgentRun::done(..)`, so they compile unchanged; if one is a full literal, add the two fields with 0.

### Task 2: usage_line writes the fields and usage_v 3

**Files:**
- Modify: `crates/factory-engine/src/ports.rs`, `crates/factory-engine/tests/runloop.rs`
- Test: `crates/factory-engine/src/ports.rs` (unit tests), `crates/factory-core/src/cost.rs` (unit test)

**Interfaces:**
- Consumes: `AgentRun.start_cc`, `AgentRun.start_cr` (Task 1), `usage_line(job_id, spec, run, round, tasks_done, ts, secs) -> serde_json::Value` in `ports.rs`.
- Produces: `usage.jsonl` lines with `"start_cc"`, `"start_cr"` and `"usage_v": 3`. The two keys are new and optional for every reader. `usage_line` is a pure function. Its logic sits behind no port; the `AgentRunner` port is what supplies the `AgentRun`.

- [ ] **Step 1: Read** `usage_line` and its tests in `crates/factory-engine/src/ports.rs` (`run_with`, `usage_line_has_the_counters_and_the_run_details`, the second `usage_v` assertion near line 387), the `usage_v` assertion in `crates/factory-engine/tests/runloop.rs` near line 233, and the `usage_v < 2` check in `crates/factory-core/src/cost.rs` near line 103 with its tests.
- [ ] **Step 2: Write the failing tests.** In `ports.rs` change `run_with` to also set `start_cc: 40, start_cr: 15` (keeping `start_ctx: 55`), and change both `usage_v` assertions from 2 to 3. Add:

```rust
#[test]
fn usage_line_carries_start_cc_and_start_cr() {
    let spec = AgentSpec::new("001", "builder", "/w", "p");
    let line = usage_line("001", &spec, &run_with(StopReason::Done), 1, 0, 10, 5);
    assert_eq!(line["start_cc"], 40);
    assert_eq!(line["start_cr"], 15);
    assert_eq!(line["usage_v"], 3);
}
```

  The other tests:

| Test | Where | Input | Assertion |
|---|---|---|---|
| `a_failed_run_still_writes_zero_start_fields` | `ports.rs` | `AgentRun::failed("x")` | `line["start_cc"] == 0` and `line["start_cr"] == 0` |
| `a_usage_v_3_line_is_not_approximate` | `cost.rs` | a summary over a line with `"usage_v":3` (copy the setup of `a_line_without_usage_v_makes_the_summary_approximate`) | the approximate flag is false |

  In `tests/runloop.rs` change the `usage_v` assertion to 3.
- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine usage_line` and expect FAIL with `left: Number(2)` / `right: 3` and a missing `start_cc`.
- [ ] **Step 4: Implement** in `usage_line`: add `"start_cc": run.start_cc` and `"start_cr": run.start_cr` to the JSON object and set `"usage_v": 3`. Update the doc comment: `usage_v` 3 marks lines with the first-turn cache fields.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine usage_line`, `cargo test -j4 -p factory-engine --test runloop`, `cargo test -j4 -p factory-core cost` and expect PASS in all. Then `cargo clippy -j4 -p factory-agent -p factory-engine --all-targets -- -D warnings` and `cargo fmt -p factory-agent -p factory-engine -- --check`, both clean.

Next plan: docs/superpowers/plans/2026-10-20-cold-start-savings/plan-02-cold-start-savings.md
