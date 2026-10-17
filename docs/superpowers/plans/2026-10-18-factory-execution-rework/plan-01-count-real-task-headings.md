# Count real task headings Implementation Plan

> **For agentic workers:** This plan is executed by the factory driver using `superpowers:subagent-driven-development`: one fresh `factory-builder` implementer per task, no per-task review, and the driver does the ledger, commits and the final review. Do not commit. Steps use checkbox syntax for tracking.

**Goal:** Make `parse_tasks` count only real `### Task N:` headings outside code fences, and keep `tick_plan` working for checkbox plans.

**Architecture:** One fence-aware parser feeds the task count, the builder loop and the series check. `tick_plan` keeps its own checkbox reader so merge lines still work.

**Tech Stack:** Rust, cargo with `-j4`, serde, existing `factory-engine` test fixtures (`ScriptedRunner`, `change_effect`).

**Spec:** `docs/superpowers/specs/2026-10-18-factory-interview-cards-and-execution-design.md` (Design 3).

## Global Constraints

- Every cargo call has `-j4`. Never `--workspace`, `--all` or `CARGO_TARGET_DIR`.
- Never shell out to `git`. Paths reach the driver only through `Layout`.
- No commit steps in any task. The driver commits.

## Review Focus

- A plan with a checkbox-only task list (`- [ ] Task 1`) must still reach the legacy whole-plan builder and still get `tick_plan` merge lines.

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

Next plan: docs/superpowers/plans/2026-10-18-factory-execution-rework/plan-02-builder-status-and-park.md
