# Plan lint, budget default and review skills Implementation Plan

> **For agentic workers:** This plan is executed by the factory driver using `superpowers:subagent-driven-development`: one fresh `factory-builder` implementer per task, no per-task review, and the driver does the ledger, commits and the final review. Do not commit. Steps use checkbox syntax for tracking.

**Goal:** Lint plans when a job is created, detect drift from the superpowers skill text, lower the default per-job budget to 1M weighted tokens, and adopt the review skills' rules in the reviewer and fix-round prompts.

**Architecture:** A plan linter next to `check_spec` refuses placeholders and warns about structure, reusing the warning field and output line from plan 03. A drift test reads the installed skill text and skips when it is absent. The budget default is one constant. The reviewer and fix prompts in `specs.rs` take their rules from `requesting-code-review`, the scoped re-review in `subagent-driven-development`, and `receiving-code-review`.

**Tech Stack:** Rust, cargo with `-j4`, existing `factory-engine` test fixtures and `factory-cli` tests.

**Spec:** `docs/superpowers/specs/2026-10-18-factory-interview-cards-and-execution-design.md` (decisions 13 and 14, the plan lint, skill drift test and review skill calls).

## Global Constraints

- Every cargo call has `-j4`. Never `--workspace`, `--all` or `CARGO_TARGET_DIR`.
- Never shell out to `git`. Paths reach the driver only through `Layout`.
- A plan placeholder (`TBD`, `TODO`, `FIXME`, `???` outside code fences and backticks) refuses `factory new`; every other lint finding is a warning and never blocks the job.
- The default per-job budget is 1,000,000 weighted tokens; warn at 80 percent and stop at 100 percent as today. A config that sets `budget_weighted` is unchanged. The daily budget stays off by default.
- The drift test must pass with no superpowers install present (skip with a printed note).
- The first line of `factory new` output stays `created job <id> in <folder>`.
- No commit steps in any task. The driver commits.

## Review Focus

- A plan placeholder inside a fenced block or backticks must not refuse `factory new`.
- A series where only plan 2 has a placeholder must refuse before any job is created.
- An existing project config with its own `budget_weighted` must keep that value, and `rocket_parity` must still hold.
- The drift test must not fail on a machine whose plugin cache holds only an older superpowers version; it tests the newest installed version and says which.
- The round 1 reviewer prompt must not contain the scoped re-review rule, and the round 2 prompt must not lose the Review Focus.
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
- [ ] **Step 2: Implement** `lint_plan`, the pre-creation lint pass over `parse_series` results, the warning merge with plan 03's spec warning (both kept, spec warning first), and the test helper that locates the plugin cache from `HOME`.
- [ ] **Step 3: Verify.** Run `cargo test -j4 -p factory-engine -p factory-cli`. Expected: pass, and the drift test prints the version it checked or the skip note.

### Task 2: Lower the default per-job budget

**Files:**
- Modify: `crates/factory-core/src/config.rs` (default for `budget_weighted`), `crates/factory-core/presets/{rust,go}/config.toml` if either sets it, `README.md`, `.claude/software-factory.md`
- Test: `crates/factory-core/src/config.rs` (inline), `crates/factory-core/tests/rocket_parity.rs`, `crates/factory-engine/tests/runloop.rs`

**Interfaces:**
- Produces: `Config::default().budget_weighted == 1_000_000` (named constant `DEFAULT_JOB_BUDGET_WEIGHTED`); a config file that sets `budget_weighted` keeps its value; docs state the default and the 80 percent warning.
- Consumes: the existing warn-at-80-percent and stop-at-100-percent logic in `budget.rs` and `claude.rs`, unchanged.

- [ ] **Step 1: Write the failing tests.** `config.rs`: `the_default_job_budget_is_one_million` and `an_explicit_budget_is_kept`. Check `rocket_parity.rs` and the Rocket fixtures: if the fixture does not set `budget_weighted`, the parity assertion must compare against the explicit old value, so add that value to the fixture or assert the new default. Run `cargo test -j4 -p factory-core config rocket_parity` and confirm the default test fails.
- [ ] **Step 2: Implement** the constant and default; update preset configs and docs that state 3M. Search the repo for the old figure and fix every mention.
- [ ] **Step 3: Verify.** Run `cargo test -j4 -p factory-core -p factory-engine`. Expected: pass.

### Task 3: Review skills in the reviewer and fix prompts

**Files:**
- Modify: `crates/factory-engine/src/specs.rs` (`reviewer_spec`, `builder_resume_spec`), `crates/factory-engine/src/review.rs` (round passed to the reviewer), `.claude/agents/factory-code-reviewer.md`, `crates/factory-core/presets/{rust,go}/agents/factory-code-reviewer.md`
- Test: `crates/factory-engine/tests/review_decide.rs`, `crates/factory-engine/tests/build_checks.rs`, `crates/factory-cli/tests/agent_lint.rs`

**Interfaces:**
- Produces: reviewer prompt and agent text containing the rule that the spec is a vision document and a reasonable person's expectation counts where the spec is silent, a `Declined to judge` list before the verdict, grading as `Critical`, `Important` or `Minor` while keeping the existing `VERDICT` line contract, and "you do not dispatch subagents".
- Produces: for `round >= 2` the reviewer prompt adds the scoped re-review rule: give a verdict for each earlier finding, inspect the fix diff only, and report anything outside it under `Out-of-scope observations` without changing the verdict; round 1 does not contain this text.
- Produces: `builder_resume_spec` text telling the fix builder to follow `superpowers:receiving-code-review`: read all findings first, verify each against the code, work one item at a time, push back with a reason when a finding is wrong, and reply `Status: NEEDS_CONTEXT` instead of guessing when an item is unclear.
- Consumes: plan 01 `ReviewerExtra`, plan 05 reply contract.

- [ ] **Step 1: Write the failing tests.** `review_decide.rs`: `the_reviewer_prompt_has_the_vision_document_rule_and_declined_to_judge_and_grades`, `round_two_carries_the_scoped_rereview_rule_and_round_one_does_not`, `round_two_still_carries_review_focus`. `build_checks.rs`: `the_fix_resume_prompt_names_receiving_code_review_and_the_needs_context_rule`. `agent_lint.rs`: `reviewer_files_carry_the_vision_rule_and_declined_to_judge` over the in-repo and both preset reviewers, and a pin that the `VERDICT` contract text is unchanged. Run them and confirm failure.
- [ ] **Step 2: Implement** the prompt and agent file changes; keep the report shape (`VERDICT` lines, findings at most 60 lines) the driver parses, so `decide.rs` is untouched.
- [ ] **Step 3: Verify.** Run `cargo test -j4 -p factory-engine` and `cargo test -j4 -p factory-cli --test agent_lint`. Expected: pass.

Next plan: none
