# Builder status, rulings and reviewer inputs Implementation Plan

> **For agentic workers:** This plan is executed by the factory driver using `superpowers:subagent-driven-development`: one fresh `factory-builder` implementer per task, no per-task review, and the driver does the ledger, commits and the final review. Do not commit. Steps use checkbox syntax for tracking.

**Goal:** Make the driver read the builder's status and rulings, park on `BLOCKED` and `NEEDS_CONTEXT`, and give the reviewer the plan's Review Focus, the rulings and the concerns.

**Architecture:** A small reply parser in `factory-core` (next to `blocked_reason`) returns a status and rulings. `build_task` checks the status before staging anything. The reviewer prompt gets three new inputs read from the plan header and the ledger. `parse_tasks` is fixed first so later plans count real task headings only.

**Tech Stack:** Rust, cargo with `-j4`, serde, existing `factory-engine` test fixtures (`ScriptedRunner`, `change_effect`).

**Spec:** `docs/superpowers/specs/2026-10-18-factory-interview-cards-and-execution-design.md` (Design 3).

## Global Constraints

- Every cargo call has `-j4`. Never `--workspace`, `--all` or `CARGO_TARGET_DIR`.
- Never shell out to `git`. Paths reach the driver only through `Layout`.
- Only the driver writes the board, ledger and events. Agents never do.
- New board fields are optional with `#[serde(default, skip_serializing_if = ...)]`.
- A missing status line means `DONE` (builder files in seeded homes are old).
- No commit steps in any task. The driver commits.

## Review Focus

- A reply that quotes `BLOCKED:` mid-text or inside a code fence must not park the job.
- A `Ruling:` line with `Ruling:` already stripped, or with an empty body, must not produce `Ruling: Ruling:` or an empty ledger line.
- A multi-line reply with `Status: DONE_WITH_CONCERNS` and several concern lines must keep the lines, folded to one ledger line.
- A plan with a checkbox-only task list (`- [ ] Task 1`) must still reach the legacy whole-plan builder and still get `tick_plan` merge lines.
- A plan with no `## Review Focus` must give the reviewer a prompt without a broken empty section.

---

### Task 1: Count only real task headings

**Files:**
- Modify: `crates/factory-engine/src/tasks.rs` (`parse_tasks`)
- Modify: `crates/factory-engine/src/tick.rs` (`tick_plan`)
- Test: `crates/factory-engine/src/tasks.rs` (inline tests), `crates/factory-engine/tests/build_checks.rs`, `crates/factory-engine/tests/merge.rs`

**Interfaces:**
- Produces: `pub fn parse_tasks(plan: &str) -> Vec<PlanTask>` now returns only `### Task N:` headings outside code fences, deduplicated by `n`.
- Produces: `pub(crate) fn parse_checkbox_tasks(plan: &str) -> Vec<PlanTask>` in `tasks.rs`: the old behavior (headings plus `- [ ] Task N:` and `- [x] Task N:` lines, fence-aware), used only by `tick_plan`.

- [ ] **Step 1: Write the failing tests** in `tasks.rs`: `parse_tasks_ignores_task_headings_inside_a_fence` (a fenced `### Task 4:` is not counted, three real headings give `n` 1..=3), `parse_tasks_ignores_checkbox_lines` (`- [ ] Task 1: x` gives an empty list), `parse_checkbox_tasks_keeps_the_old_behavior` (checkbox lines and headings both count, deduplicated). Run `cargo test -j4 -p factory-engine parse_` and confirm the first two fail.
- [ ] **Step 2: Implement** `parse_tasks` over `lines_with_fence(plan)` filtered on `!fenced && is_task_heading(line)`, and add `parse_checkbox_tasks` carrying the previous loop body. Point `tick_plan` at `parse_checkbox_tasks`.
- [ ] **Step 3: Update fixtures** that rely on checkbox plans reaching the per-task path: the three excerpt tests in `build_checks.rs` (around the `- [ ] Task` plans) switch to `### Task N:` plans, and `merge.rs` keeps its checkbox plan because `tick_plan` still reads it. Update `tasks.rs` test `ignores_template_headings_and_accepts_checkboxes` to the new split.
- [ ] **Step 4: Verify.** Run `cargo test -j4 -p factory-engine` and `cargo test -j4 -p factory-core`. Expected: all pass, and a plan that quotes `### Task 4:` inside a fence is no longer rejected by the three-task check in `series.rs`.

### Task 2: Parse builder status and park on BLOCKED

**Files:**
- Modify: `crates/factory-core/src/commitmsg.rs`, `crates/factory-core/src/lib.rs` (export)
- Modify: `crates/factory-core/src/park.rs` (`ParkKind`)
- Modify: `crates/factory-engine/src/build.rs` (`build_task`, `build_stage`), `crates/factory-engine/src/park.rs`
- Modify: `dashboard/static/board.js` (`PARK_LABEL`, `PARK_RED`)
- Test: `crates/factory-core/src/commitmsg.rs` (inline), `crates/factory-engine/tests/build_checks.rs`, `dashboard/tests/board.test.js`

**Interfaces:**
- Produces: `pub enum BuilderStatus { Done, DoneWithConcerns(String), Blocked(String), NeedsContext(String) }` and `pub fn builder_status(reply: &str) -> BuilderStatus` in `commitmsg.rs`. Rules: a first line starting `BLOCKED:` gives `Blocked`; otherwise the first line matching `Status:` or `**Status:**` (optionally after `- `), outside code fences, gives the status, with the rest of the reply after it as the text; no status line gives `Done`.
- Produces: `pub fn builder_rulings(reply: &str) -> Vec<String>`: the bodies of lines starting `Ruling:` outside fences, trimmed, empty bodies dropped.
- Produces: `ParkKind::BuilderBlocked` (kebab `builder-blocked`), `next` offers `[rework]` only, like `BuilderStopped`.
- Consumes: `blocked_reason`, `park_with`, `builder_stop_parts` as they exist.

- [ ] **Step 1: Write the failing parser tests** in `commitmsg.rs`: `builder_status_reads_a_first_line_blocked`, `builder_status_reads_a_status_line_in_a_longer_reply`, `builder_status_ignores_blocked_inside_a_fence`, `builder_status_defaults_to_done_without_a_status_line`, `builder_rulings_strips_prefix_and_drops_empty_bodies`. Run `cargo test -j4 -p factory-core builder_` and confirm failure.
- [ ] **Step 2: Implement** `BuilderStatus`, `builder_status` and `builder_rulings`, and export them from `lib.rs`.
- [ ] **Step 3: Write the failing engine tests** in `build_checks.rs`: `a_blocked_reply_parks_without_committing` (script a builder that changes a file and replies `BLOCKED: no schema`; assert `park_kind == BuilderBlocked`, the reason contains `no schema`, the branch tip is unchanged and the file edit is still in the worktree), `a_needs_context_reply_parks_like_blocked`, `a_reply_without_a_status_line_is_committed_as_before`. Also add `kind_names_are_stable_kebab_case` coverage for the new kind in `park.rs` tests.
- [ ] **Step 4: Implement** the check in `build_task` between the `stop_reason != Done` park and the `BuilderCommitted` check, using `builder_status(&run.result_text)`; park through `park_with(ParkKind::BuilderBlocked, ..)` with a part built like `builder_stop_parts`. Add the same check after the stop check in `build_stage` for the whole-plan path. Add `BuilderBlocked` to `ParkKind::as_str` and `next`.
- [ ] **Step 5: Dashboard label.** Add `builder-blocked` to `PARK_LABEL` ("Builder blocked") and `PARK_RED` in `board.js`, with a test in `board.test.js` that the card shows the label.
- [ ] **Step 6: Verify.** Run `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-engine --test build_checks`, and `node --test dashboard/tests/*.test.js`. Expected: pass.

### Task 3: Rulings, concerns and Review Focus reach the reviewer

**Files:**
- Modify: `crates/factory-engine/src/build.rs` (after `ImplementerDone`), `crates/factory-engine/src/specs.rs` (`reviewer_spec`), `crates/factory-engine/src/review.rs` (call site), `crates/factory-engine/src/tasks.rs`
- Test: `crates/factory-engine/tests/build_checks.rs`, `crates/factory-engine/tests/review_decide.rs`

**Interfaces:**
- Produces: `pub fn review_focus(plan: &str) -> Option<String>` in `tasks.rs`: the body of the plan header's `## Review Focus` section, fence-aware, `None` when absent or empty.
- Produces: `pub(crate) fn builder_notes(&self) -> Vec<String>` on the engine: ledger lines written by the builder path, those starting `builder task `, with the prefix kept.
- Produces: `reviewer_spec(&self, cwd, diff, names, round, rules, extra: &ReviewerExtra) -> AgentSpec` where `ReviewerExtra { focus: Option<String>, notes: Vec<String>, plan_outline: String }`. `plan_outline` is the plan header plus each `### Task N:` heading line, and the prompt still names the full plan path for a step lookup.
- Consumes: `ruling()` (strip a leading `Ruling:` first), `plan_header`, `lines_with_fence`.

- [ ] **Step 1: Write the failing tests.** `review_focus_returns_the_section_body_and_none_when_missing` and `review_focus_ignores_a_fenced_heading` in `tasks.rs`. In `build_checks.rs`: `builder_rulings_land_in_the_ledger_with_a_task_prefix` (reply with two `Ruling:` lines, assert `builder task 1: <text>` appears twice and no `Ruling: Ruling:`), `concerns_are_recorded_for_done_with_concerns` (assert a `builder task 1 concerns:` line and that the task is committed). In `review_decide.rs`: `the_reviewer_prompt_carries_focus_notes_and_outline` (assert the prompt contains the focus text, a `builder task` line and the task heading, and not the whole plan body) and `a_plan_without_review_focus_has_no_empty_section`.
- [ ] **Step 2: Run** `cargo test -j4 -p factory-engine review_focus builder_rulings concerns the_reviewer_prompt` and confirm failure.
- [ ] **Step 3: Implement** `review_focus`; write rulings and concerns through `ruling()` after `ImplementerDone` in `build_task`; build `ReviewerExtra` in `review.rs` from the plan, `review_focus` and `builder_notes`; extend the reviewer prompt between the earlier-findings line and the "Read the diff in pages" line.
- [ ] **Step 4: Verify.** Run `cargo test -j4 -p factory-engine`. Expected: pass.

Next plan: docs/superpowers/plans/2026-10-18-factory-interview-cards-and-execution/plan-02-per-task-checks-and-token-controls.md
