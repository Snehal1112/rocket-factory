# Codex support, Plan 1: The Codex runner

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** `factory-agent` gets a `CodexRunner` that runs `codex exec --json`, reads its events and usage, and shares one child-process loop with `ClaudeRunner`, with a `fake-codex` binary and an ignored real smoke test. Nothing selects it yet.

**Architecture:** The loop in `ClaudeRunner::run_inner` moves into a free function `drive` over a small trait `AgentStream`. `StreamState` implements the trait unchanged. A new `CodexStream` in `codex.rs` implements it for the Codex events, and `CodexRunner` builds the command line and calls `drive`. `AgentSpec` gets three optional fields that only the Codex runner reads.

**Tech Stack:** Rust, `serde_json`, `tempfile` in tests.

**Spec:** docs/superpowers/specs/2026-10-21-codex-support-design.md

## Global Constraints

- Always `-j4`; never `--workspace` or `--all`; never set `CARGO_TARGET_DIR`. Use `cargo test -j4 -p factory-agent <filter>`.
- All Claude runner tests (`tests/runner.rs`, `tests/stream.rs`, `tests/privacy.rs`, the unit tests) pass unchanged by this plan, except the one error-text test named in Task 1.
- `AgentSpec::new` gives the new fields their defaults, so existing struct literals and callers compile: `sandbox: None`, `writable_roots: Vec::new()`, `network: false`.
- The Codex command line is exactly: `exec --json --ignore-user-config --skip-git-repo-check [-m <model>] -c sandbox_mode="<mode>" [-c sandbox_workspace_write.writable_roots=[...]] [-c sandbox_workspace_write.network_access=true] -`, and for a resume `exec resume <thread_id> --json ...` with the same options. The prompt goes to stdin. Never `--ephemeral`. `sandbox` unset means `read-only`.
- Codex runs have no mid-run budget warning or stop and ignore `max_turns`; the engine checks the budget after the run.
- Usage maps as the spec says: input = `input_tokens - cached_input_tokens` (saturating), cache read = `cached_input_tokens`, cache creation = `cache_write_input_tokens`, output = `output_tokens`, 1h part = 0.
- `OPENAI_API_KEY` and `CODEX_API_KEY` are removed from the child unless `FACTORY_ALLOW_API_KEY=1` (`ALLOW_API_KEY_VAR`).
- No new board, ledger or event field in this plan. Tests use `.expect("...")` and `expect_err`, never the unwrap call. Comments are short full sentences ending with a punctuation mark.

## Review Focus

1. A config-notice `error` item before the turn. Expected: ignored, the run still succeeds. Pinned by `config_notice_errors_are_ignored` (Task 2).
2. `cached_input_tokens` above `input_tokens`. Expected: input 0, no underflow. Pinned by `cached_above_input_does_not_underflow` (Task 2).
3. `codex` missing from `PATH`. Expected: a failed run whose error names the program. Pinned by `spawn_error_names_the_program` (Task 3).
4. A Codex child that goes silent. Expected: killed as `Stalled`. Pinned by `codex_stall_kills_the_child` (Task 3).
5. A failed turn (`turn.failed`, exit 1). Expected: `Failed` with the message, not `Done`. Pinned by `turn_failed_ends_failed_with_the_message` (Task 3).

---

## File Structure

- `crates/factory-agent/src/child.rs` (create): `AgentStream`, `ChildConfig`, `drive`, the line reader and stderr tail moved from `claude.rs`.
- `crates/factory-agent/src/claude.rs` (modify): `ClaudeRunner` calls `drive`.
- `crates/factory-agent/src/stream.rs` (modify): `impl AgentStream for StreamState`.
- `crates/factory-agent/src/spec.rs` (modify): `sandbox`, `writable_roots`, `network`.
- `crates/factory-agent/src/codex.rs` (create): `CodexStream`, `CodexRunner`, `codex_args`.
- `crates/factory-agent/src/lib.rs`, `Cargo.toml` (modify): exports, `[[bin]] fake-codex`.
- `crates/factory-agent/src/bin/fake-codex.rs` (create).
- `crates/factory-agent/tests/fixtures/codex-session.jsonl`, `codex-failed.jsonl` (create); `tests/codex_runner.rs`, `tests/codex_smoke.rs` (create).

### Task 1: One child loop for both runners

**Files:**
- Create: `crates/factory-agent/src/child.rs`
- Modify: `crates/factory-agent/src/claude.rs:84-250`, `stream.rs`, `spec.rs`, `lib.rs`
- Test: `crates/factory-agent/tests/runner.rs`, `crates/factory-agent/src/spec.rs` (unit tests)

**Interfaces:**
- Produces:
  - `pub trait AgentStream { fn feed(&mut self, line: &str) -> Vec<StreamEvent>; fn usage(&self) -> Usage; fn final_usage(&self) -> Usage; fn turns(&self) -> u32; fn has_result(&self) -> bool; fn result_text(&self) -> Option<String>; fn result_is_error(&self) -> bool; fn result_subtype(&self) -> Option<String>; fn session_id(&self) -> Option<String>; fn start_ctx(&self) -> u64; fn end_ctx(&self) -> u64; fn start_cache(&self) -> (u64, u64); }`
  - `pub(crate) struct ChildConfig { program: OsString, args: Vec<String>, stdin: String, label: String, remove_env: Vec<&'static str>, honor_max_turns: bool, mid_run_budget: bool }`
  - `pub(crate) fn drive<S: AgentStream>(cfg: &ChildConfig, spec: &AgentSpec, sink: &EventSink, stream: S) -> Result<AgentRun, AgentError>`
  - `AgentError::Spawn(String, std::io::Error)` with the text `cannot start {0}: {1}`.
  - `AgentSpec { pub sandbox: Option<String>, pub writable_roots: Vec<PathBuf>, pub network: bool }`.

- [ ] **Step 1: Read** `claude.rs` in full, `stream.rs`, `spec.rs`, and `tests/runner.rs` with the `fake-claude` modes.
- [ ] **Step 2: Record the baseline.** Run `cargo test -j4 -p factory-agent` and write down the passed count. This is the count that must be unchanged at Step 6, plus the new tests.
- [ ] **Step 3: Write the failing tests.** In `spec.rs`: `new_spec_has_no_sandbox_roots_or_network` asserts `AgentSpec::new("001","builder","/w","p")` has `sandbox == None`, `writable_roots.is_empty()` and `!network`. In `tests/runner.rs`: `noresult_message_names_the_program` runs the `noresult` mode and asserts the error text contains `claude exited without a result`; `spawn_error_names_the_program` builds `ClaudeRunner::new("no-such-claude-binary", ...)`, runs a spec and asserts `run.error` contains `cannot start no-such-claude-binary`.
- [ ] **Step 4: Run** `cargo test -j4 -p factory-agent new_spec_has_no_sandbox`. Expected: compile failure, no field `sandbox` on `AgentSpec`.
- [ ] **Step 5: Implement.** Add the three `AgentSpec` fields with their defaults. Move the spawn, stdin writer, reader thread, budget, turn, stall and result assembly of `run_inner` into `drive`, keeping behavior byte for byte. Differences come from `ChildConfig`: `label` fills `"{label} exited without a result"` and `"{label} reported {subtype}"`, `remove_env` replaces the API-key removal, `honor_max_turns` gates the `spec.max_turns` check and the `max_turns` subtype mapping, and `mid_run_budget` gates the budget warning and stop inside the loop (true for Claude). Move `event` into a free function used by `drive`. `ClaudeRunner::run_inner` builds a `ChildConfig` from `spec.args(&self.caps)` and `spec.stdin_prompt(&self.caps)`. Implement `AgentStream` for `StreamState` by calling its inherent methods (write `StreamState::usage(self)` so the trait method does not recurse).
- [ ] **Step 6: Run** `cargo test -j4 -p factory-agent`. Expected: the baseline count plus the 3 new tests, all passing. Update any existing assertion on the old `cannot start the agent process` text.

### Task 2: The Codex event stream

**Files:**
- Create: `crates/factory-agent/src/codex.rs` (the `CodexStream` part), `crates/factory-agent/tests/fixtures/codex-session.jsonl`, `crates/factory-agent/tests/fixtures/codex-failed.jsonl`
- Modify: `crates/factory-agent/src/lib.rs`, `crates/factory-agent/src/stream.rs` (make the 80 character target cut `pub(crate)`)
- Test: `crates/factory-agent/src/codex.rs` (unit tests)

**Interfaces:**
- Consumes: `AgentStream`, `StreamEvent`, `Usage` from Task 1 and `usage.rs`.
- Produces: `pub struct CodexStream` (`Default`), `impl AgentStream for CodexStream`, and `pub fn codex_usage(turn_usage: &serde_json::Value) -> Usage`.

- [ ] **Step 1: Read** the spec's Design 3 and 4, `StreamState::feed` and `final_usage`.
- [ ] **Step 2: Create the fixtures.** `codex-session.jsonl` holds, one per line, the five probe lines of the spec: `thread.started` (thread id `01a15eac-9d1e-73f0-98dd-ce2929f333e6`), `turn.started`, `item.started` and `item.completed` of the `command_execution` `echo hi`, `item.completed` of `agent_message` with text `OK`, and `turn.completed` with usage 43851, 38656, 0, 443, 199. `codex-failed.jsonl` holds the config-notice `error` item, the top-level `error` line and the `turn.failed` line of the spec.
- [ ] **Step 3: Write the failing tests.** Full example:

```rust
#[test]
fn codex_usage_splits_cached_input_and_keeps_cache_write() {
    let v = serde_json::json!({"input_tokens":43851,"cached_input_tokens":38656,
        "cache_write_input_tokens":0,"output_tokens":443,"reasoning_output_tokens":199});
    let u = codex_usage(&v);
    assert_eq!((u.input_tokens, u.cache_read_input_tokens), (5195, 38656));
    assert_eq!((u.cache_creation_input_tokens, u.output_tokens), (0, 443));
    assert_eq!(u.weighted(), 9503);
}
```

  The others, each feeding the fixture lines (or the inline JSON given) into `CodexStream::feed`:

  | Test | Inputs | Assertion |
  |---|---|---|
  | `codex_stream_reads_thread_id_tools_and_usage` | `codex-session.jsonl` | one `Init` with the thread id; one `Tool { name: "Bash", target }` from `item.started`; `result_text == Some("OK")`; `has_result`; `final_usage().weighted() == 9503` |
  | `tool_target_is_cut_at_80_characters` | `item.started` with a 200 character command | target length 80 |
  | `cached_above_input_does_not_underflow` | usage input 10, cached 50 | `input_tokens == 0`, `cache_read == 50` |
  | `config_notice_errors_are_ignored` | the `item.completed` `error` line only | no events, `has_result == false`, `!result_is_error` |
  | `codex_stream_ignores_reasoning_and_file_change_items` | `item.completed` with types `reasoning` and `file_change` | no events, no panic |
  | `codex_turns_count_agent_messages` | two `agent_message` items | `turns() == 2` |
  | `turn_failed_marks_the_result_as_an_error` | `codex-failed.jsonl` | `has_result`, `result_is_error`, `result_subtype == Some("turn_failed")`, text holds the message |
  | `non_json_lines_are_skipped` | `"garbage"` | no events, no panic |
  | `usage_is_added_only_after_turn_completed` | tool lines, no `turn.completed` | `usage().is_zero()` |

- [ ] **Step 4: Run** `cargo test -j4 -p factory-agent codex::`. Expected: compile failure, cannot find `CodexStream` in this scope.
- [ ] **Step 5: Implement** `CodexStream` and `codex_usage` as the spec's table says: `thread.started` gives one `Init`; `item.started` of `command_execution` gives a `Tool` once per item id; `agent_message` items set the last text and count a turn; `turn.completed` adds `codex_usage` to the total and marks the result; `turn.failed` or a top-level `error` marks an error result with `result_subtype` `turn_failed`; everything else is ignored. `start_ctx`, `end_ctx` and `start_cache` return zeros. Export `CodexStream` from `lib.rs`.
- [ ] **Step 6: Run** `cargo test -j4 -p factory-agent codex::`. Expected: 9 passed.

### Task 3: `CodexRunner`, `fake-codex` and the smoke test

**Files:**
- Modify: `crates/factory-agent/src/codex.rs`, `lib.rs`, `Cargo.toml`
- Create: `crates/factory-agent/src/bin/fake-codex.rs`, `crates/factory-agent/tests/codex_runner.rs`, `crates/factory-agent/tests/codex_smoke.rs`
- Test: the two new test files, and `codex.rs` unit tests for `codex_args`

**Interfaces:**
- Consumes: `drive`, `ChildConfig`, `CodexStream`, the `AgentSpec` fields from Task 1.
- Produces: `pub struct CodexRunner`, `CodexRunner::new(program: impl Into<OsString>, sink: EventSink) -> CodexRunner`, `with_api_key_allowed(self, bool) -> CodexRunner`, `impl AgentRunner for CodexRunner`, `pub fn codex_args(spec: &AgentSpec) -> Vec<String>`.

- [ ] **Step 1: Read** `ClaudeRunner::new`, `with_api_key_allowed`, the `fake-claude` modes, and `tests/runner.rs` helpers `runner` and `spec`.
- [ ] **Step 2: Write the failing tests.** Unit tests of `codex_args` in `codex.rs`: `fresh_args_have_exec_json_ignore_user_config_model_sandbox_and_stdin_dash` (model `gpt-5.6-luna`, sandbox `workspace-write` gives the exact vector of the Global Constraints, ending in `-`), `resume_args_use_exec_resume_and_the_thread_id` (`spec.resume = Some("abc")` gives `["exec","resume","abc","--json",...]`), `reviewer_args_are_read_only` (sandbox `read-only`, no network flag), `committer_args_add_the_git_dir_root` (one writable root `/repo/.git` gives `-c` `sandbox_workspace_write.writable_roots=["/repo/.git"]`), `builder_args_turn_network_on` (`network = true` adds `sandbox_workspace_write.network_access=true`), `unset_sandbox_means_read_only`. In `tests/codex_runner.rs`, with `fake-codex` and a helper `runner(sink)` / `spec(mode, dir)` like `tests/runner.rs`:

```rust
#[test]
fn ok_run_collects_session_usage_and_result() {
    let tmp = tempfile::tempdir().expect("tmp");
    let run = runner(EventSink::null("001")).run(&spec("ok", tmp.path()));
    assert_eq!(run.stop_reason, StopReason::Done);
    assert_eq!(run.session_id.as_deref(), Some("01a15eac-9d1e-73f0-98dd-ce2929f333e6"));
    assert_eq!(run.result_text, "OK");
    assert_eq!(run.weighted, 9503);
}
```

  | Test | Mode / setup | Assertion |
  |---|---|---|
  | `turn_failed_ends_failed_with_the_message` | `failed` (prints the failed fixture, exit 1) | `Failed`, error contains `invalid_request_error` |
  | `no_turn_completed_is_failed` | `noresult` | `Failed`, error contains `codex exited without a result` |
  | `spawn_error_names_the_program` | program `no-such-codex` | error contains `cannot start no-such-codex` |
  | `codex_stall_kills_the_child` | `silent`, `stall_after` 1 second | `Stalled` |
  | `no_mid_run_budget_stop_on_codex` | `ok` with `budget_weighted = Some(1)` | `Done`, `weighted == 9503` (the engine parks the job after the run) |
  | `api_key_vars_are_removed_unless_allowed` | mode `env OPENAI_API_KEY` with the variable set in `spec.env`, then `with_api_key_allowed(true)` | `<unset>`, then the value |
  | `a_session_event_is_emitted_for_the_thread_id` | `ok` with an `on_session` hook | hook called once with the thread id |

  `tests/codex_smoke.rs` holds `#[test] #[ignore] fn real_codex_smoke()`: run `CodexRunner::new("codex", sink)` in a temp folder with model `gpt-5.6-luna`, sandbox `read-only`, prompt `Reply with the single word OK.`, and assert `Done` and a non-empty `session_id`.
- [ ] **Step 3: Run** `cargo test -j4 -p factory-agent codex`. Expected: compile failure, cannot find `CodexRunner`, and `fake-codex` not built.
- [ ] **Step 4: Implement** `codex_args` and `CodexRunner` (it builds a `ChildConfig` with label `codex`, `remove_env` the two key variables unless allowed, `honor_max_turns` false, `mid_run_budget` false, and the prompt `spec.prompt` joined after `spec.agent_prompt` when present, as `stdin_prompt` does with no flags). Add `[[bin]] name = "fake-codex"`. `fake-codex` reads stdin like `fake-claude` and selects the mode from the first line: `ok` prints the fixture lines, `failed` prints the failed lines and exits 1, `noresult` prints `thread.started` only, `silent` sleeps 30 seconds after `thread.started`, `env <NAME>` reports the variable as the `agent_message` text, `args` reports the argument list. Export `CodexRunner` from `lib.rs`.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-agent`. Expected: all pass, the smoke test shown as ignored, and `cargo clippy -j4 -p factory-agent -- -D warnings` is clean.

Next plan: docs/superpowers/plans/2026-10-21-codex-support/plan-02-codex-support.md
