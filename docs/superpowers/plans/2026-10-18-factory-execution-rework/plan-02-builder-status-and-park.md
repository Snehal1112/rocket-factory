# Builder status and park on blocked Implementation Plan

> **For agentic workers:** This plan is executed by the factory driver using `superpowers:subagent-driven-development`: one fresh `factory-builder` implementer per task, no per-task review, and the driver does the ledger, commits and the final review. Do not commit. Steps use checkbox syntax for tracking.

**Goal:** Make the driver read the builder reply status and park the job, without committing, on `BLOCKED` and `NEEDS_CONTEXT`.

**Architecture:** A small reply parser in `factory-core` returns the status. `build_task` checks it before staging anything and parks through `park_with` with a new `BuilderBlocked` kind.

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

---

### Task 1: Parse builder status and park on BLOCKED

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

Next plan: docs/superpowers/plans/2026-10-18-factory-execution-rework/plan-03-rulings-and-reviewer-inputs.md
