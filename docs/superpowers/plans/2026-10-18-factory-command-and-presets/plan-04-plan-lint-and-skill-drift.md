# Plan lint and skill drift test Implementation Plan

> **For agentic workers:** This plan is executed by the factory driver using `superpowers:subagent-driven-development`: one fresh `factory-builder` implementer per task, no per-task review, and the driver does the ledger, commits and the final review. Do not commit. Steps use checkbox syntax for tracking.

**Goal:** Lint plans when a job is created and detect drift from the installed superpowers skill text.

**Architecture:** A plan linter next to `check_spec` refuses placeholders and warns about structure, reusing the warning field and output line from the cards series. A drift test reads the installed skill text and skips when it is absent.

**Tech Stack:** Rust, cargo with `-j4`, existing `factory-engine` test fixtures and `factory-cli` tests.

**Spec:** `docs/superpowers/specs/2026-10-18-factory-interview-cards-and-execution-design.md` (decisions 13 and 14, the plan lint, skill drift test and review skill calls).

## Global Constraints

- Every cargo call has `-j4`. Never `--workspace`, `--all` or `CARGO_TARGET_DIR`.
- Never shell out to `git`. Paths reach the driver only through `Layout`.
- A plan placeholder (`TBD`, `TODO`, `FIXME`, `???` outside code fences and backticks) refuses `factory new`; every other lint finding is a warning and never blocks the job.
- The drift test must pass with no superpowers install present (skip with a printed note).
- The first line of `factory new` output stays `created job <id> in <folder>`.
- No commit steps in any task. The driver commits.

## Review Focus

- A plan placeholder inside a fenced block or backticks must not refuse `factory new`.
- A series where only plan 2 has a placeholder must refuse before any job is created.
- The drift test must not fail on a machine whose plugin cache holds only an older superpowers version; it tests the newest installed version and says which.
- Lint warnings must not appear for a plan that follows this repo's own plan format.

---

### Task 1: Plan lint and skill drift test

**Files:**
- Create: `crates/factory-engine/src/planlint.rs` (export in `lib.rs`), `crates/factory-cli/tests/skill_drift.rs`
- Modify: `crates/factory-engine/src/runloop.rs` (`create_job_with_spec`), `crates/factory-cli/src/commands.rs` (`new_job` output)
- Test: `crates/factory-engine/src/planlint.rs` (inline), `crates/factory-engine/tests/new_job.rs`, `crates/factory-cli/tests/skill_drift.rs`

**Interfaces:**
- Produces: `pub struct PlanLint { pub refuse: Option<String>, pub warnings: Vec<String> }` and `pub fn lint_plan(plan: &str, name: &str) -> PlanLint`. `refuse` is set for a placeholder outside code (reuse `find_placeholder`). Warnings: one per missing header field among Goal, Architecture, Tech Stack, Spec, Global Constraints, Review Focus; a missing executor line naming `superpowers:subagent-driven-development`; each task section without `**Files:**` or without `**Interfaces:**`; a line starting `git commit` or containing a `Commit` step heading.
- Produces: `create_job_with_spec` lints every plan on the chain before creating any job and returns `EngineError` naming the plan file when `refuse` is set; otherwise it appends the warnings (prefixed with the plan file name) to the first job's `spec_warning`, joined with `; `, and `new_job` prints each as a `warning:` line after the `created job` line.
- Produces (test): `skill_drift.rs` finds the newest `superpowers/<version>/skills` directory under the plugin cache (`$HOME/.claude/plugins/cache/*/superpowers/`), asserts that `brainstorming/builder-check-prompt.md` exists, that `writing-plans/SKILL.md` contains `Interfaces:` and `Review Focus`, and that `subagent-driven-development/SKILL.md` and `implementer-prompt.md` exist and the latter contains `Status`, prints the version tested, and returns early with a printed note when no install is found. The version tested against is the constant `SKILLS_TESTED_AGAINST: &str = "7.0.0"`; a different newest version prints a note, it does not fail.
- Consumes: `section_body`, `find_placeholder`, `parse_tasks`, `task_section`, `plan_header`.

- [ ] **Step 1: Write the failing tests.** `planlint.rs`: `a_placeholder_outside_code_refuses`, `a_placeholder_in_a_fence_or_backticks_is_fine`, `a_complete_plan_in_this_repos_format_has_no_warnings` (load plan 01 of this series as the fixture text), `each_missing_header_field_is_one_warning`, `a_missing_executor_line_warns`, `a_task_without_files_or_interfaces_warns`, `a_commit_step_warns`. `new_job.rs`: `a_plan_placeholder_in_plan_two_refuses_before_any_job_exists` (assert the board has no jobs), `plan_warnings_are_stored_and_the_job_is_created`. `skill_drift.rs`: `installed_skills_still_have_what_the_commands_rely_on`. Run `cargo test -j4 -p factory-engine planlint new_job` and `cargo test -j4 -p factory-cli --test skill_drift` and confirm the first group fails.
- [ ] **Step 2: Implement** `lint_plan`, the pre-creation lint pass over `parse_series` results, the warning merge with cards plan 03's spec warning (both kept, spec warning first), and the test helper that locates the plugin cache from `HOME`.
- [ ] **Step 3: Verify.** Run `cargo test -j4 -p factory-engine -p factory-cli`. Expected: pass, and the drift test prints the version it checked or the skip note.

Next plan: none
