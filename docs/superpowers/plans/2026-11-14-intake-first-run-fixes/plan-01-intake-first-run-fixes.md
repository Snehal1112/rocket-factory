# Intake first-run fixes Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-14-intake-first-run-fixes-design.md
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Make a real intake run either work or fail with a clear cause: check the drafter agent file before the claim, grant the drafter an Edit rule for its draft folder, add a `drafter-failed` reason code and log the tail of an empty draft's last message locally.

**Architecture:** All changes sit in `factory-engine/src/intake.rs`, one check in `factory-cli/src/project.rs`, and two docs. The drafter still runs through the existing `AgentRunner` port and the issue labels through the `Tracker` port. The new pre-claim check reads the driver's agent files and calls no tracker method.

**Tech Stack:** Rust, the existing intake test fixtures (`setup_full`, `FakeTracker`).

## Global Constraints

- Commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`. Never `--workspace` or `--all`.
- No `Cargo.toml` change, no `git` shell-outs, no `/proc` reads. No board, ledger or events field is added.
- The GitHub comment keeps its fixed text: reason code, job and draft path. No agent text and no log tail reaches it.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call. Comments are short full sentences.

## Review Focus

1. A missing drafter agent file stops the pass before any label or comment change, also for a dry run. Pinned by `intake_refuses_when_the_drafter_agent_file_is_missing`.
2. A driver with no agent files at all is not refused. Pinned by `intake_does_not_check_the_agent_file_when_the_driver_has_none`.
3. A drafter that used all its turns stays `draft-invalid`, not `drafter-failed`. Pinned by `a_drafter_that_hits_the_turn_limit_is_draft_invalid`.
4. The last message of an empty draft never reaches the GitHub comment. Pinned by `an_empty_draft_failure_comment_never_holds_the_last_message`.
5. A path-scoped allow rule that names a tool other than `Read`, `Grep`, `Glob` or `Edit` fails a test. Pinned by `draft_spec_path_rules_use_only_matched_tools`.
6. An empty last message does not break the log text. Pinned by `empty_draft_cause_with_an_empty_message`.

## File Structure

| File | Responsibility |
|---|---|
| `crates/factory-engine/src/intake.rs` | `check_drafter_agent`, the Edit rule, the stop match, `empty_draft_cause` |
| `crates/factory-engine/tests/intake.rs` | Pass-level tests |
| `crates/factory-cli/src/project.rs`, `crates/factory-cli/tests/project_cmd.rs` | The `project show` problem and its tests |
| `docs/guides/factory-user-guide.md`, `docs/reference/software-factory.md` | Reason codes and the refusal |

### Task 1: Refuse before the claim when the drafter agent file is missing

**Files:**
- Modify: `crates/factory-engine/src/intake.rs`, `crates/factory-cli/src/project.rs`
- Test: `crates/factory-engine/tests/intake.rs`, `crates/factory-cli/tests/project_cmd.rs`

**Interfaces:**
- Consumes: `Driver::agent_files() -> &[AgentFile]`, `Driver::with_agent_files(Vec<AgentFile>) -> Driver`, `AgentFile { name, content }`, the Codex check at the top of `run_intake`, and in `project.rs` the `SECURITY_AGENT_FILE` check on `resolved.agents`. The engine logic sits behind the `Tracker` and `AgentRunner` ports; this check uses neither.
- Produces: private `fn check_drafter_agent(driver: &Driver) -> Result<(), EngineError>` in `intake.rs`; private `const DRAFTER_AGENT_FILE: &str = "factory-intake-drafter.md"` in `project.rs`.

- [ ] **Step 1: Read `run_intake` and `write_drafter_agents` in `crates/factory-engine/src/intake.rs`, the `intake_refuses_to_run_under_codex` test and the `setup_with`, `setup_full` fixtures in `crates/factory-engine/tests/intake.rs`, and in `crates/factory-cli/tests/project_cmd.rs` the helper `show_without_security_agent` and the test `project_show_reports_a_missing_security_agent`.**
- [ ] **Step 2: Write the failing tests.**
  - In `tests/intake.rs`, a helper `fn with_agent_files(it: Intake, names: &[&str]) -> Intake` returning `Intake { driver: it.driver.with_agent_files(names.iter().map(|n| AgentFile { name: n.to_string(), content: "x".to_string() }).collect()), ..it }`.
  - `intake_refuses_when_the_drafter_agent_file_is_missing`: `with_agent_files(setup_with(config(), vec![issue(7)], one_plan, "x"), &["factory-builder.md"])`; for `dry_run` in `[false, true]`: `run_intake(&it.driver, &it.tracker, dry_run)` with `expect_err`; the text contains `factory presets refresh` and `factory-intake-drafter.md`; `it.tracker.count("edit_labels") == 0`, `it.tracker.count("comment") == 0` and `it.script.drafter_specs` is empty.
  - `intake_accepts_the_drafter_agent_file_with_or_without_md`: with the names `["factory-intake-drafter.md"]` and then `["factory-intake-drafter"]`, `run_intake(..., true)` returns `IntakeOutcome::DryRun { .. }` (a dry run, so no `agents.json` is written).
  - `intake_does_not_check_the_agent_file_when_the_driver_has_none`: the plain `setup(one_plan)` fixture, `run_intake(..., true)` returns `DryRun`.
  - In `project_cmd.rs`, `project_show_reports_a_missing_drafter_agent_file_when_intake_is_on` (a helper like `show_without_security_agent` that sets `intake = true` and removes `rust/agents/factory-intake-drafter.md`; the output contains `intake is on but factory-intake-drafter.md was not found; run factory presets refresh`) and `project_show_has_no_intake_problem_when_intake_is_off` (the file removed, `intake` unset; the output has no `intake is on`).
- [ ] **Step 3: Run `cargo test -j4 -p factory-engine --test intake intake_` and `cargo test -j4 -p factory-cli --test project_cmd drafter`.** Expected: the refusal and project show tests fail (no error, no problem line); the other two may already pass.
- [ ] **Step 4: Implement `check_drafter_agent`.** It returns `Ok(())` when `driver.agent_files()` is empty or holds a file named `factory-intake-drafter` or `factory-intake-drafter.md`; otherwise `EngineError::Msg("intake: the drafter agent file factory-intake-drafter.md was not found; run factory presets refresh".to_string())`. Call it in `run_intake` right after the Codex check and before `pick_issue`. In `project.rs` `show`, push the problem `intake is on but factory-intake-drafter.md was not found; run factory presets refresh` when `resolved.config.intake` is true and no resolved agent has that name, next to the security check.
- [ ] **Step 5: Run `cargo test -j4 -p factory-engine --test intake` and `cargo test -j4 -p factory-cli --test project_cmd`.** Expected: all pass.

### Task 2: The drafter can write, and a drafter that cannot run says so

**Files:**
- Modify: `crates/factory-engine/src/intake.rs`
- Test: unit tests in `crates/factory-engine/src/intake.rs`, `crates/factory-engine/tests/intake.rs`

**Interfaces:**
- Consumes: `draft_spec` and its `rule` closure, the stop match in `draft_jobs` (`StopReason::BudgetStop`, `other`), `failure(code, issue, cause)`, the empty-draft check, `AgentRun::done(text)` and `StopReason::{Failed, Stalled, TurnLimit}`. The drafter runs behind the `AgentRunner` port.
- Produces: `draft_spec` grants `Edit(//<draft>/**)` and no `Write(...)` rule, with `spec.tools` still `Read,Grep,Glob,Write`; reason code `drafter-failed` for `Failed` and `Stalled`; private `fn empty_draft_cause(last_message: &str) -> String`.

- [ ] **Step 1: Read `draft_spec`, `draft_jobs`, `failure` and the `drafter` helper with the test `draft_spec_grants_only_the_draft_folder` in `crates/factory-engine/src/intake.rs`, and `stalled`, `over_budget` and `a_drafter_that_stops_early_fails_the_issue_with_no_job` in `crates/factory-engine/tests/intake.rs`.**
- [ ] **Step 2: Write the failing tests.**
  - Update `draft_spec_grants_only_the_draft_folder`: filter the allowed tools by `starts_with("Edit")` and expect exactly `["Edit(//state/intake/7/draft/**)"]`, and assert no rule starts with `Write(`.
  - `draft_spec_path_rules_use_only_matched_tools`: for every allowed rule that contains `(`, the text before `(` is one of `Read`, `Grep`, `Glob`, `Edit`.
  - `empty_draft_cause_keeps_the_last_600_characters_on_one_line`: input `"a\n".repeat(400) + "END"`; the result starts with `the draft has no spec.md or no plan; last message: `, the part after that prefix has at most 600 characters, has no `\n`, and ends with `END`.
  - `empty_draft_cause_with_an_empty_message`: the result equals `the draft has no spec.md or no plan; last message: `.
  - In `tests/intake.rs`: a reply `fn failed() -> AgentRun` (`StopReason::Failed`, empty text), `fn turn_limit() -> AgentRun` (`StopReason::TurnLimit`), `fn secret() -> AgentRun { AgentRun::done("SECRET-MARKER") }`. Tests: `a_drafter_that_cannot_start_fails_with_the_drafter_failed_code` (for `failed` and `stalled`, via `setup_full(config(), vec![issue(7)], |_| {}, "x", "fine\nVERDICT: APPROVE", reply)`; the outcome is `Failed { issue: 7, reason: "drafter-failed" }`, the board has no job, and the comment holds `reason: drafter-failed`), `a_drafter_that_hits_the_turn_limit_is_draft_invalid` (the outcome reason is `draft-invalid`), `an_empty_draft_failure_comment_never_holds_the_last_message` (reply `secret`, empty draft folder; reason `draft-invalid`; no comment body contains `SECRET-MARKER`). Change the stalled row of `a_drafter_that_stops_early_fails_the_issue_with_no_job` from `draft-invalid` to `drafter-failed`.
- [ ] **Step 3: Run `cargo test -j4 -p factory-engine --lib intake` and `cargo test -j4 -p factory-engine --test intake`.** Expected: `cannot find function empty_draft_cause in this scope`, and failing reason and rule assertions.
- [ ] **Step 4: Implement.** In `draft_spec` replace `rule("Write", draft)` by `rule("Edit", draft)` and keep a comment that Claude Code applies file write rules through `Edit(path)`. In the stop match, `StopReason::Failed | StopReason::Stalled` return `failure("drafter-failed", n, format!("the drafter stopped: {}", ...as_str()))`; `TurnLimit` keeps `draft-invalid`. Add `empty_draft_cause` (the prefix, then the last 600 characters of the text with line breaks replaced by spaces) and use it as the cause of the empty-draft failure with `run.result_text`.
- [ ] **Step 5: Run `cargo test -j4 -p factory-engine` then `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`.** Expected: all pass, no warnings.

### Task 3: Document the reason codes and the refusal

**Files:**
- Modify: `docs/guides/factory-user-guide.md`, `docs/reference/software-factory.md`

**Interfaces:**
- Consumes: the behaviour built in the first two tasks and the intake section of the guide (`### Issue intake: let labelled issues run on their own`).
- Produces: the doc text only.

- [ ] **Step 1: Read the intake section of `docs/guides/factory-user-guide.md` and the intake paragraph of `docs/reference/software-factory.md`.**
- [ ] **Step 2: Add the text.** In the guide, a table of the six reason codes: `draft-refused` (the drafter wrote `refused.md`), `draft-invalid` (no usable draft, or a drafter that used all its turns), `drafter-failed` (the drafter could not run: read the log and run `factory presets refresh`), `spec-review` (the drafted spec was refused twice), `over-budget` (the drafter hit `intake_budget_weighted`), `build-failed` (a job parked or failed); one sentence that after an upgrade `factory presets refresh` must run so the seeded presets get new agent files; one sentence that file write rules for an agent run are `Edit(path)` rules. In the reference paragraph, one sentence that a pass refuses to start, with no label change, when the drafter agent file is missing.
- [ ] **Step 3: Run `cargo test -j4 -p factory-cli`.** Expected: all pass, nothing pins the changed paragraphs.

Next plan: none
