# Rulings and reviewer inputs Implementation Plan

> **For agentic workers:** This plan is executed by the factory driver using `superpowers:subagent-driven-development`: one fresh `factory-builder` implementer per task, no per-task review, and the driver does the ledger, commits and the final review. Do not commit. Steps use checkbox syntax for tracking.

**Goal:** Write the builder's rulings and concerns to the ledger and give the reviewer the plan's Review Focus, the rulings, the concerns and a plan outline.

**Architecture:** `build_task` writes prefixed ledger lines after each task. The reviewer prompt gets three new inputs read from the plan header and the ledger.

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

- A `Ruling:` line with `Ruling:` already stripped, or with an empty body, must not produce `Ruling: Ruling:` or an empty ledger line.
- A multi-line reply with `Status: DONE_WITH_CONCERNS` and several concern lines must keep the lines, folded to one ledger line.
- A plan with no `## Review Focus` must give the reviewer a prompt without a broken empty section.

---

### Task 1: Rulings, concerns and Review Focus reach the reviewer

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

Next plan: docs/superpowers/plans/2026-10-18-factory-execution-rework/plan-04-per-task-checks-and-slim-agents.md
