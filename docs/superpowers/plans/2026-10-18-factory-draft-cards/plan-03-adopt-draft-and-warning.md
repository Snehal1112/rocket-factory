# Adopt the draft and warn Implementation Plan

> **For agentic workers:** This plan is executed by the factory driver using `superpowers:subagent-driven-development`: one fresh `factory-builder` implementer per task, no per-task review, and the driver does the ledger, commits and the final review. Do not commit. Steps use checkbox syntax for tracking.

**Goal:** Have `factory new` adopt and remove the matching draft and warn when no decision was answered by the user.

**Architecture:** `create_job_with_spec` removes the draft after the series jobs exist and stores a spec warning on the first job.

**Tech Stack:** Rust, cargo with `-j4`, clap, serde, existing `Fixture` test harness in `factory-cli/tests/common`.

**Spec:** `docs/superpowers/specs/2026-10-18-factory-interview-cards-and-execution-design.md` (Design 2).

## Global Constraints

- Every cargo call has `-j4`. Never `--workspace`, `--all` or `CARGO_TARGET_DIR`.
- Never shell out to `git`. Paths reach the driver only through `Layout`; no `".superpowers`, `".claude` or `"main"` literals outside `layout.rs` (`no_hardcoded_paths.rs`).
- Only the driver writes the board. All writes go through `BoardStore::update`.
- Board JSON stays compatible with `dashboard/`: new fields optional and omitted when empty.
- `factory cancel` on a draft removes the card and never deletes the spec file.
- The first line of `factory new` output stays `created job <id> in <folder>` (factoryd parses it).
- No commit steps in any task. The driver commits.

## Review Focus

- `factory new` with a slug that has no draft must behave exactly as before.
- A `factory new` whose spec or chain check fails must leave the draft in place.
- A spec whose `## Decisions` is missing must produce the warning, not an error.

---

### Task 1: Adopt the draft and warn when no decision came from the user

**Files:**
- Modify: `crates/factory-engine/src/runloop.rs` (`create_job_with_spec`), `crates/factory-cli/src/commands.rs` (`new_job` output)
- Test: `crates/factory-engine/tests/new_job.rs` (new, or the nearest existing `create_job` test file), `crates/factory-cli/tests/draft_cards.rs`

**Interfaces:**
- Produces: `pub(crate) fn user_decisions(spec: &str) -> usize` counting list items in the `## Decisions` section (via `section_body`) that contain the text `Decided by: user`; a spec with no section or no such item returns 0.
- Produces: after the series jobs exist, `create_job_with_spec` removes the draft whose slug equals `slugify(slug)` in one `store().update`, and when `user_decisions(&text) == 0` sets `spec_warning = Some("no decision in this spec was answered by the user")` on the first job. `new_job` prints a `warning:` line after the `created job` line.
- Consumes: `section_body`, `is_list_item`, `check_spec`, `add_job`.

- [ ] **Step 1: Write the failing tests.** `new_job.rs`: `user_decisions_counts_only_user_entries` (two user entries, one call entry gives 2), `a_spec_without_a_decisions_section_counts_zero`, `new_adopts_the_draft_and_leaves_no_duplicate`, `a_failed_spec_check_keeps_the_draft`, `new_without_a_draft_is_unchanged`, `a_spec_with_no_user_decision_sets_the_warning_and_still_creates_the_job`, `a_spec_with_a_user_decision_has_no_warning`. `draft_cards.rs`: `new_prints_the_created_line_first_then_the_warning`. Run them and confirm failure.
- [ ] **Step 2: Implement** `user_decisions`, the draft removal after the series loop and before the return, and the warning write through `update_job` on the first job. Cover the no-plan early return too.
- [ ] **Step 3: Verify.** Run `cargo test -j4 -p factory-engine -p factory-cli` and `cargo test -j4 -p factory-supervisor` (factoryd parses `created job`). Expected: pass.

Next plan: docs/superpowers/plans/2026-10-18-factory-draft-cards/plan-04-dashboard-state-and-factoryd.md
