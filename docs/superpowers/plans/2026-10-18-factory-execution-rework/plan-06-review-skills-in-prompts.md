# Review skills in the reviewer and fix prompts Implementation Plan

> **For agentic workers:** This plan is executed by the factory driver using `superpowers:subagent-driven-development`: one fresh `factory-builder` implementer per task, no per-task review, and the driver does the ledger, commits and the final review. Do not commit. Steps use checkbox syntax for tracking.

**Goal:** Adopt the review skills' rules in the reviewer prompt, the round 2 re-review and the fix-round resume prompt.

**Architecture:** The prompts in `specs.rs` take their rules from `requesting-code-review`, the scoped re-review in `subagent-driven-development`, and `receiving-code-review`. The `VERDICT` contract is unchanged, so `decide.rs` is untouched.

**Tech Stack:** Rust, cargo with `-j4`, existing `factory-engine` test fixtures and `factory-cli` tests.

**Spec:** `docs/superpowers/specs/2026-10-18-factory-interview-cards-and-execution-design.md` (decisions 13 and 14, the plan lint, skill drift test and review skill calls).

## Global Constraints

- Every cargo call has `-j4`. Never `--workspace`, `--all` or `CARGO_TARGET_DIR`.
- Never shell out to `git`. Paths reach the driver only through `Layout`.
- No commit steps in any task. The driver commits.

## Review Focus

- The round 1 reviewer prompt must not contain the scoped re-review rule, and the round 2 prompt must not lose the Review Focus.

---

### Task 1: Review skills in the reviewer and fix prompts

**Files:**
- Modify: `crates/factory-engine/src/specs.rs` (`reviewer_spec`, `builder_resume_spec`), `crates/factory-engine/src/review.rs` (round passed to the reviewer), `.claude/agents/factory-code-reviewer.md`, `crates/factory-core/presets/{rust,go}/agents/factory-code-reviewer.md`
- Test: `crates/factory-engine/tests/review_decide.rs`, `crates/factory-engine/tests/build_checks.rs`, `crates/factory-cli/tests/agent_lint.rs`

**Interfaces:**
- Produces: reviewer prompt and agent text containing the rule that the spec is a vision document and a reasonable person's expectation counts where the spec is silent, a `Declined to judge` list before the verdict, grading as `Critical`, `Important` or `Minor` while keeping the existing `VERDICT` line contract, and "you do not dispatch subagents".
- Produces: for `round >= 2` the reviewer prompt adds the scoped re-review rule: give a verdict for each earlier finding, inspect the fix diff only, and report anything outside it under `Out-of-scope observations` without changing the verdict; round 1 does not contain this text.
- Produces: `builder_resume_spec` text telling the fix builder to follow `superpowers:receiving-code-review`: read all findings first, verify each against the code, work one item at a time, push back with a reason when a finding is wrong, and reply `Status: NEEDS_CONTEXT` instead of guessing when an item is unclear.
- Consumes: execution plan 03 `ReviewerExtra`, command plan 01 reply contract.

- [ ] **Step 1: Write the failing tests.** `review_decide.rs`: `the_reviewer_prompt_has_the_vision_document_rule_and_declined_to_judge_and_grades`, `round_two_carries_the_scoped_rereview_rule_and_round_one_does_not`, `round_two_still_carries_review_focus`. `build_checks.rs`: `the_fix_resume_prompt_names_receiving_code_review_and_the_needs_context_rule`. `agent_lint.rs`: `reviewer_files_carry_the_vision_rule_and_declined_to_judge` over the in-repo and both preset reviewers, and a pin that the `VERDICT` contract text is unchanged. Run them and confirm failure.
- [ ] **Step 2: Implement** the prompt and agent file changes; keep the report shape (`VERDICT` lines, findings at most 60 lines) the driver parses, so `decide.rs` is untouched.
- [ ] **Step 3: Verify.** Run `cargo test -j4 -p factory-engine` and `cargo test -j4 -p factory-cli --test agent_lint`. Expected: pass.

Next plan: none
