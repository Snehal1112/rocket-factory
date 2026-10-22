# Code Review Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the ten findings of the 2026-10-23 high-effort review of the uncommitted Codex-support diff, without changing Claude behaviour.

**Architecture:** Each finding is a local fix in the file the review named. Codex-only behaviour stays behind provider config (`honor_max_turns`-style flags), Claude paths keep their old behaviour, and dashboard JSON stays backward compatible. Tasks are independent except 5 and 6, which both edit `install.rs` and `upgrade_file`.

**Tech Stack:** Rust (`factory-agent`, `factory-cli`, `factory-engine`), dashboard Rust crate (`dashboard/`, own workspace), vanilla JS with `node --test`.

**Spec:** `docs/superpowers/specs/2026-10-22-codex-support-audit-design.md` (the audit this review covers) and `docs/superpowers/specs/2026-10-21-codex-support-design.md` (event and exit-code table, lines ~76-141).

## Global Constraints

- Always `cargo ... -j4`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Dashboard commands: `cargo test -j4 --manifest-path dashboard/Cargo.toml`; JS: `node --test dashboard/tests/`.
- Claude behaviour and its tests stay unchanged (codex spec, "Claude behavior and its tests stay unchanged").
- Board JSON fields stay optional and compatible with `factory-server`.
- Never shell out to `git`; never read `/proc`.
- Warning texts never contain `"; "` (the board joins and the CLI splits on it).
- The user already has uncommitted work in these files. Commit only the paths each task names, with `git add <paths>`, never `git add -A`.

## Review Focus

1. `error` event, then a retry, then `turn.completed`: the turn succeeds. `turn.failed` after an `error` still fails with the `turn.failed` message.
2. Codex stream that ends with only an `error` event and exit 0 or non-zero: still `Failed`, never `Done`.
3. A Claude run with a success result and a non-zero exit stays `Done`; a Codex run in the same state stays `Failed`.
4. `factory install` and `factory upgrade` on a machine with no `~/.agents` directory: no `~/.agents` tree is created by `upgrade`.
5. A plan with `## Review Focus` before checkbox tasks: the focus does not swallow the tasks.

---

## File Structure

- `crates/factory-agent/src/codex.rs`: `error` handling, approval flags.
- `crates/factory-agent/src/child.rs`: exit-after-result rule per provider.
- `crates/factory-agent/src/claude.rs`: sets the new flag to `false`.
- `crates/factory-cli/src/install.rs`: fallible `codex_skill`, upgrade only an installed skill, keep first backup.
- `crates/factory-engine/src/tasks.rs`, `combine.rs`: Review Focus and File Structure handling.
- `crates/factory-engine/src/planlint.rs`, `ports.rs`: lint-text contract test, provider-sync cost.
- `dashboard/src/state.rs`, `dashboard/static/board.js`, `dashboard/tests/board.test.js`: warning filter and visibility.

---

### Task 1: Non-fatal Codex `error` events (finding 1)

**Files:**
- Modify: `crates/factory-agent/src/codex.rs` (`feed`, `CodexStream`, tests near line 485)
- Test: same file, `#[cfg(test)]` module

**Interfaces:**
- Consumes: `CodexStream::fail(&mut self, message: String, out: &mut Vec<StreamEvent>)`.
- Produces: `CodexStream` field `last_error: Option<String>`; only `turn.failed` calls `fail`.

- [ ] **Step 1: Replace the test `failure_followed_by_completion_stays_failed_and_keeps_diagnostic` with two tests**
  - `turn_failed_then_completed_stays_failed`: feed `{"type":"turn.failed","error":{"message":"turn rejected"}}` then a `turn.completed` with usage 2/3; assert `result_is_error()`, subtype `turn_failed`, text contains `turn rejected`, `final_usage().input_tokens == 2`.
  - `error_event_then_completed_is_a_success`: feed `{"type":"error","message":"Reconnecting... 2/5"}` then `turn.completed`; assert `!result_is_error()`, subtype `Some("success")`.
  - `error_event_alone_is_not_a_result`: feed only the `error` line; assert `!has_result()`.
- [ ] **Step 2: Run** `cargo test -j4 -p factory-agent codex` — Expected: the two `error` tests FAIL.
- [ ] **Step 3: In `feed`, make the `"error"` arm store `self.last_error = Some(text_of(&value, "message"))` and push nothing.** `turn.failed` is unchanged.
- [ ] **Step 4: In `child.rs` the no-result arm already reports "exited without a result"; add `last_error` to that text** by extending `AgentStream` with `fn diagnostic(&self) -> Option<String>` (default `None`, Codex returns `last_error`) and appending it after the stderr tail. Test: `crates/factory-agent/tests/codex_runner.rs` gets `error_only_stream_fails_with_the_error_text` using a new `fake-codex` scenario `error-only` (edit `src/bin/fake-codex.rs`: print the `error` line, exit 0); assert `StopReason::Failed` and `err.contains("provider unavailable")`.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-agent` — Expected: PASS.
- [ ] **Step 6: Commit** `git add crates/factory-agent && git commit -m "fix(agent): treat codex error events as diagnostics, not failures"`

### Task 2: Verify and fix the approval flags (finding 2)

**Files:**
- Modify: `crates/factory-agent/src/codex.rs:37-38`
- Test: `crates/factory-agent/src/codex.rs` args tests

**Interfaces:** Produces: `codex_args` with a verified approval setting.

- [ ] **Step 1: Probe the real CLI.** Run `codex --version` and `codex exec --ignore-user-config --skip-git-repo-check -c 'approval_policy="on-request"' -c 'approvals_reviewer="auto_review"' -c 'sandbox_mode="read-only"' - <<< 'run: curl -sI https://example.com'` in a scratch directory. Record the outcome in the commit message. Pass means exit 0 with no hang inside 60s and the command either runs or is declined with a model-visible message.
- [ ] **Step 2: Decide.** If the probe passes, keep both keys and add a doc comment on `codex_args` with the codex version tested. If it hangs, errors, or the key is rejected, replace both lines with the single `config("approval_policy=\"never\"".to_string());` (headless `exec` never prompts, the sandbox still bounds the agent) and drop `approvals_reviewer`.
- [ ] **Step 3: Pin the choice** with `codex_args_set_the_approval_policy` asserting the exact `-c` pair is present (and, for `never`, that `approvals_reviewer` is absent), for a builder spec and a read-only reviewer spec.
- [ ] **Step 4: Run** `cargo test -j4 -p factory-agent` — Expected: PASS.
- [ ] **Step 5: Commit** `git add crates/factory-agent/src/codex.rs && git commit -m "fix(agent): pin a verified codex approval policy"`

### Task 3: Exit-after-result rule per provider (finding 3)

**Files:**
- Modify: `crates/factory-agent/src/child.rs:47` (config struct), `:219` (arm), `crates/factory-agent/src/claude.rs:67`, `crates/factory-agent/src/codex.rs:101`
- Test: `crates/factory-agent/tests/codex_runner.rs` (existing nonzero test), `crates/factory-agent/tests/` Claude runner test file using `fake-claude`

**Interfaces:**
- Produces: config field `pub fail_on_exit_after_result: bool` (Claude `false`, Codex `true`).

- [ ] **Step 1: Write `claude_success_then_nonzero_exit_is_done`** next to the existing `fake-claude` tests: a scenario that prints a success result and exits 3 (add the scenario to `fake-claude`); assert `StopReason::Done`, `run.exit_code == Some(3)`.
- [ ] **Step 2: Run** `cargo test -j4 -p factory-agent claude_success_then_nonzero` — Expected: FAIL (`Failed`).
- [ ] **Step 3: Guard the arm** `None if cfg.fail_on_exit_after_result && !status.success()`. Truncate the embedded `result_text` to 500 characters with an ellipsis, and use the truncated text only in the error string.
- [ ] **Step 4: Run** `cargo test -j4 -p factory-agent` — Expected: PASS, including the unchanged Codex test `completed_turn_with_nonzero_exit_is_failed_and_keeps_diagnostics`.
- [ ] **Step 5: Commit** `git add crates/factory-agent && git commit -m "fix(agent): fail on exit after a result only for codex"`

### Task 4: Fallible `codex_skill()` (finding 4)

**Files:**
- Modify: `crates/factory-cli/src/install.rs` (`codex_skill`, `run`, `upgrade_codex_skill`)
- Test: `crates/factory-cli/tests/global_command.rs`

**Interfaces:**
- Produces: `fn codex_skill() -> Result<String, CliError>` and private `fn replace_required(body: String, from: &str, to: &str) -> Result<String, CliError>` returning `CliError::Refused("global command text changed: <first 60 chars of from>")`.

- [ ] **Step 1: Write `shipped_command_builds_a_codex_skill`** (unit test in `install.rs`, `codex_skill().expect("skill")` contains `name: factory`), and `codex_skill_reports_a_changed_command` calling a testable inner `build_codex_skill(command: &str)` with a command lacking the sentences; assert `Err`.
- [ ] **Step 2: Run** `cargo test -j4 -p factory-cli codex_skill` — Expected: FAIL to compile (no `build_codex_skill`).
- [ ] **Step 3: Convert** every `assert!(body.contains(..)); body = body.replace(..)` pair, and the loop over `(claude, codex)` pairs, to `replace_required`. `GLOBAL_COMMAND` passes into `build_codex_skill`.
- [ ] **Step 4: In `run`,** a `codex_skill()` error returns `CliError::Refused("{claude}\nCodex skill install failed: {error}")` after the Claude file is written. In `upgrade_codex_skill` the error propagates; `upgrade::run` already turns it into `codex_error`, so the preset refresh and version marker still run. Add `upgrade_survives_an_unbuildable_codex_skill` in `crates/factory-cli/tests/upgrade.rs` only if `upgrade_codex_skill` gains a seam; otherwise rely on the existing `codex_skill_backup_failure_does_not_block_claude_upgrade` pattern.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-cli` — Expected: PASS.
- [ ] **Step 6: Commit** `git add crates/factory-cli/src/install.rs crates/factory-cli/tests && git commit -m "fix(cli): report a changed global command instead of panicking"`

### Task 5: Upgrade only an installed Codex skill (finding 5)

**Files:**
- Modify: `crates/factory-cli/src/install.rs` (`upgrade_codex_skill`, `upgrade_file`)
- Test: `crates/factory-cli/tests/upgrade.rs`

**Interfaces:**
- Consumes: Task 4's `codex_skill() -> Result<String, CliError>`.
- Produces: `upgrade_codex_skill` returns `Ok(None)` when `SKILL.md` does not exist; `upgrade_file` keeps the first backup and writes later ones as `<file>.bak.1`, `.bak.2`, and so on.

- [ ] **Step 1: Write tests** in `tests/upgrade.rs`:
  - `upgrade_does_not_create_the_codex_skill`: standard home with only the Claude command; after `upgrade`, `codex_skills_dir(home)` does not exist.
  - `upgrade_keeps_the_first_backup`: skill file holds `original`; run upgrade (backup = `original`), edit the shipped text via a second distinct skill content (write `second` into `SKILL.md`), run upgrade again; assert `SKILL.md.bak` still equals `original` and `SKILL.md.bak.1` equals `second`.
- [ ] **Step 2: Run** `cargo test -j4 -p factory-cli --test upgrade` — Expected: both FAIL.
- [ ] **Step 3: Implement.** In `upgrade_codex_skill`, return `Ok(None)` when the target is `NotFound`. In `upgrade_file`, choose the backup path as the first of `.bak`, `.bak.1`, `.bak.2`, … that does not exist. A dry run reports the chosen path without writing. The existing `updates_the_codex_skill_with_a_backup` test must pre-create the skill; adjust its setup if needed.
- [ ] **Step 4: Run** `cargo test -j4 -p factory-cli` — Expected: PASS.
- [ ] **Step 5: Commit** `git add crates/factory-cli && git commit -m "fix(cli): upgrade only an installed codex skill and keep the first backup"`

### Task 6: Review Focus in checkbox plans (finding 6)

**Files:**
- Modify: `crates/factory-engine/src/tasks.rs:137` (`review_focus`), `crates/factory-engine/src/combine.rs:206` (`split_plan` checkbox branch)
- Test: `crates/factory-engine/src/tasks.rs` (`late_review_focus_tests`), `combine.rs` tests

**Interfaces:** Consumes `parse_checkbox_tasks(line)`, `is_focus_heading`, `ends_section`, `lines_with_fence`.

- [ ] **Step 1: Write tests**
  - `review_focus_before_checkbox_tasks_stops_at_the_first_task`: plan `# P\n\n## Review Focus\n\n1. Empty input works.\n\n- [ ] Task 1: work\n`; assert focus is `"1. Empty input works."`.
  - `late_focus_in_a_checkbox_plan_is_not_in_the_last_task` (combine.rs): plan with checkbox tasks then `## Review Focus\n\n1. Root returns 200.`; `combine_plans` output contains the focus line exactly once and the Task 1 block does not contain `Review Focus`.
- [ ] **Step 2: Run** `cargo test -j4 -p factory-engine review_focus combine` — Expected: FAIL.
- [ ] **Step 3: `review_focus`:** also end the section at a non-fenced line with `!parse_checkbox_tasks(l).is_empty()`.
- [ ] **Step 4: `split_plan` checkbox branch:** while collecting section lines, a non-fenced `## Review Focus` heading starts a skipped region that ends at the next non-fenced `# `/`## ` heading or a new task line. Skipped lines are not added to any section.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine` — Expected: PASS.
- [ ] **Step 6: Commit** `git add crates/factory-engine/src/tasks.rs crates/factory-engine/src/combine.rs && git commit -m "fix(engine): keep Review Focus out of checkbox task bodies"`

### Task 7: Combine File Structure from every plan (finding 7)

**Files:**
- Modify: `crates/factory-engine/src/combine.rs` (collection loop ~line 62, emission ~line 166)
- Test: `crates/factory-engine/src/combine.rs` tests

**Interfaces:** Consumes `HeaderParts::section(heading)`. Produces a combined `## File Structure` section.

- [ ] **Step 1: Write `combining_keeps_every_plans_file_structure`:** two compact plans, each with a `## File Structure` body naming a distinct file; combined text has one `## File Structure` heading whose body contains both file names, the second prefixed `From plan 2 (now Tasks A to B):` (same prefix style as the focus).
- [ ] **Step 2: Run** `cargo test -j4 -p factory-engine combining_keeps_every` — Expected: FAIL.
- [ ] **Step 3: Collect** `files: Vec<String>` beside `focus`, using the same single-plan vs multi-plan formatting; emit `## File Structure` after the focus and skip that heading in the `first_parts.sections` loop (`const FILE_STRUCTURE: &str = "## File Structure";`). A single plan's output is byte-identical to today's.
- [ ] **Step 4: Run** `cargo test -j4 -p factory-engine` — Expected: PASS (existing single-plan golden tests unchanged).
- [ ] **Step 5: Commit** `git add crates/factory-engine/src/combine.rs && git commit -m "fix(engine): combine File Structure from every plan"`

### Task 8: Harden the dashboard warning filter (finding 8)

**Files:**
- Modify: `dashboard/src/state.rs:219` (`visible_spec_warning`), `crates/factory-engine/src/planlint.rs` (test only)
- Test: `dashboard/src/state.rs` tests, `crates/factory-engine/src/planlint.rs` tests

**Interfaces:** `visible_spec_warning(paths: &Paths, job: &Job) -> Option<String>` keeps its signature.

- [ ] **Step 1: Write tests**
  - Dashboard `a_prose_mention_does_not_make_a_plan_compact`: a non-compact plan whose task body quotes the Executor sentence; warning `x.md header has no Goal` stays visible.
  - Dashboard `no_plan_file_is_read_without_an_old_header_finding`: warning `x.md has no Review Focus` with a missing `plan.md` returns the warning unchanged (no read needed).
  - Engine `lint_texts_the_dashboard_filters_on`: `lint_plan` of a non-compact empty-header plan yields exactly `<name> header has no Goal` and the other five `HEADER_FIELDS` texts, the `does not name superpowers:subagent-driven-development as its executor` text, and `<name> task 1 has no **Files:**`. Comment: "dashboard/src/state.rs filters on these texts, update both together".
- [ ] **Step 2: Run** the three tests — Expected: the first two FAIL, the engine contract test PASSES (it pins current text).
- [ ] **Step 3: Implement.** Return early when no part matches an old-header or old-files finding, before reading `plan.md`. Compute `compact` only on the plan text before the first `### Task ` heading or first `- [ ]` task line. Split on `"; "` stays, since lint texts never contain it (Global Constraints).
- [ ] **Step 4: Run** `cargo test -j4 --manifest-path dashboard/Cargo.toml` and `cargo test -j4 -p factory-engine planlint` — Expected: PASS.
- [ ] **Step 5: Commit** `git add dashboard/src/state.rs crates/factory-engine/src/planlint.rs && git commit -m "fix(dashboard): read plan.md only when an old-header warning needs it"`

### Task 9: Visible spec warnings (finding 9)

**Files:**
- Modify: `dashboard/static/board.js:586`, `dashboard/static/dashboard.css` if the open state needs styling
- Test: `dashboard/tests/board.test.js:933`

**Interfaces:** `renderCard(job, now, specs)` output.

- [ ] **Step 1: Write tests**
  - `a_needs_human_card_shows_its_spec_warning_open`: `makeJob({ stage: "needs-human", reason: "x", spec_warning: "a; b" })` renders `<details class="spec-warning" open>` and a summary `Spec warnings (2)`.
  - `a_build_card_keeps_its_spec_warning_collapsed`: stage `build` renders no `open` attribute, summary still shows the count.
- [ ] **Step 2: Run** `node --test dashboard/tests/board.test.js` — Expected: FAIL.
- [ ] **Step 3: Implement.** Count = number of `"; "`-separated parts; `open` when `job.stage === "needs-human"`; summary `Spec warnings (N)`; update the existing assertion at line 934 to the new markup.
- [ ] **Step 4: Run** `node --test dashboard/tests/` — Expected: PASS.
- [ ] **Step 5: Commit** `git add dashboard/static dashboard/tests && git commit -m "fix(dashboard): open spec warnings on needs-human cards and show a count"`

### Task 10: Sync the provider once per engine (finding 10)

**Files:**
- Modify: `crates/factory-engine/src/ports.rs:213` (`sync_provider`) and the `Engine` struct plus every place that builds it
- Test: `crates/factory-engine/tests/codex_prep.rs`

**Interfaces:** Produces `Engine` field `provider_synced: std::cell::Cell<bool>` (default `false`); `sync_provider` returns early when it is `true` and sets it after a successful sync.

- [ ] **Step 1: Write `provider_sync_runs_once_per_engine`:** an engine with a board whose job has `provider: None` and config `agent_cli = codex`; call `sync_provider()`, then change `job.provider` on disk back to `None` (and set a `task_session`), call it again; assert the second call left `task_session` untouched. Then build a fresh `Engine` and assert the clear happens.
- [ ] **Step 2: Run** `cargo test -j4 -p factory-engine --test codex_prep provider_sync_runs_once` — Expected: FAIL (or fail to compile on the field).
- [ ] **Step 3: Implement** the field and early return. Keep the call in `build_stage` and in `run_agent`, so reviewer and committer runs of a fresh engine still sync.
- [ ] **Step 4: Run** `cargo test -j4 -p factory-engine` — Expected: PASS, including the existing provider-switch tests in `codex_prep.rs`.
- [ ] **Step 5: Commit** `git add crates/factory-engine && git commit -m "perf(engine): sync the provider once per engine"`

---

### Final check

- [ ] Run `cargo check -j4`, `cargo test -j4 -p factory-agent`, `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`, `cargo test -j4 -p factory-core`, `cargo test -j4 --manifest-path dashboard/Cargo.toml`, `node --test dashboard/tests/`. The three `slim_prompts` golden failures on macOS (`/private/tmp` vs `/tmp`) are known and unrelated (audit spec line 47); report any other failure.
