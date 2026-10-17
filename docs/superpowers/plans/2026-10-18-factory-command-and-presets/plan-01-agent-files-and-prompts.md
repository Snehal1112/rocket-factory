# Builder and reviewer text for subagent-driven-development Implementation Plan

> **For agentic workers:** This plan is executed by the factory driver using `superpowers:subagent-driven-development`: one fresh `factory-builder` implementer per task, no per-task review, and the driver does the ledger, commits and the final review. Do not commit. Steps use checkbox syntax for tracking.

**Goal:** Point the builder and reviewer agent files and the builder prompts at `superpowers:subagent-driven-development`, with the status and ruling contract and a rules digest.

**Architecture:** The agent files, presets and `specs.rs` prompts are rewritten together and the lint tests that pinned the old text are flipped in the same change.

**Tech Stack:** Rust, cargo with `-j4`, clap, Markdown agent and command files, `agent_lint.rs` and `global_command.rs` tests.

**Spec:** `docs/superpowers/specs/2026-10-18-factory-interview-cards-and-execution-design.md` (Design 1, Design 3, seeded homes).

## Global Constraints

- Every cargo call has `-j4`. Never `--workspace`, `--all` or `CARGO_TARGET_DIR`. Run `cargo test -j4 -p factory-cli --test agent_lint` after editing any `.claude/agents/factory-*.md`.
- The embedded command (`crates/factory-core/assets/global-command.md`) must not contain (lowercased) `tools/factory`, `cargo build`, `opencollection`, `factory-ctl`, `python3`, `set-stage`, `sendmessage` or `rocket`.
- Rocket's in-repo copy keeps its two Rocket-only plan rules (per-crate cargo commands, the opencollection read line).
- No file the driver uses (agent files, presets, `specs.rs` prompts, both commands) may name `superpowers:executing-plans` after this plan.
- The user's own answers go in `## Decisions` as list items containing `Decided by: user`; the author's calls contain `Decided by: call`. Cards plan 03 counts the first form.
- Refresh never writes under `<home>/projects/` or a repo's `.factory/`.
- No commit steps in any task. The driver commits.

## Review Focus

- An old builder reply with no status line must still work, so the `Done` default from execution plan 02 must stay covered by a test.

---

### Task 1: Builder and reviewer text names subagent-driven-development

**Files:**
- Modify: `.claude/agents/factory-builder.md`, `.claude/agents/factory-code-reviewer.md`, `crates/factory-core/presets/{rust,go}/agents/factory-builder.md`, `crates/factory-core/presets/{rust,go}/agents/factory-code-reviewer.md`
- Modify: `crates/factory-engine/src/specs.rs` (`task_excerpt`, `builder_first_spec`, `builder_task_spec`, `builder_resume_spec`), `.claude/software-factory.md` (builder row near line 46)
- Test: `crates/factory-cli/tests/agent_lint.rs`, `crates/factory-engine/tests/build_checks.rs`

**Interfaces:**
- Produces: builder agent text that says the plan is run through `superpowers:subagent-driven-development` as its implementer, drops the inline fallback and the skip-list for `executing-plans`, keeps test-first, debugging and verification skills and "no commits", and ends with the reply contract: first line `Status: DONE | DONE_WITH_CONCERNS | BLOCKED | NEEDS_CONTEXT`, then at most five lines of concerns or the blocker, then `Ruling: <what> — <why> — <cost if wrong>` lines; no commits and no report file; a first-line `BLOCKED: <reason>` is still accepted.
- Produces: a "Rules in short" digest in every builder agent file, about ten lines, in three parts taken from the skills' own iron laws. Test-first (`superpowers:test-driven-development`): no production code without a failing test first; write one test, run it, watch it fail for the right reason; a test that passes at once tests existing behavior, so fix the test; then write the minimum code, watch it pass, and run the touched crate's suite, not only your file. Debugging (`superpowers:systematic-debugging`): no fix without a root cause; read the whole error, reproduce it, compare with a working example, form one hypothesis, change one thing; for a bug write the failing test first; after three failed fixes stop and reply `Status: NEEDS_CONTEXT` or `Status: BLOCKED` instead of a fourth guess. Verification (`superpowers:verification-before-completion`): no completion claim without fresh evidence; before `Status: DONE` run each check the plan names in full, read the output and exit code, then claim; report anything that failed or was skipped. The three skill names stay in the file. Builders were not loading the skills (no Skill events in jobs 012 to 020), so the digest carries the rules.
- Produces: reviewer agent Input line naming the plan's Review Focus, the builder rulings and the builder concerns.
- Produces: every builder prompt in `specs.rs` names `superpowers:subagent-driven-development`, and the excerpt sentence reads "If a skill such as superpowers:subagent-driven-development tells you to read or load the plan file, use this excerpt instead and do not read the plan file in full."

- [ ] **Step 1: Write the failing tests.** `agent_lint.rs`: change `builder_body_pins_the_rules` to require `superpowers:subagent-driven-development`, `Status:`, `DONE_WITH_CONCERNS`, `NEEDS_CONTEXT`, `Ruling:` and to forbid `superpowers:executing-plans`, `inline fallback` and `No handoff lines`; add `presets_name_subagent_driven_development_and_not_executing_plans` over both preset builders; add a reviewer pin for `Review Focus` and `rulings` in the in-repo and preset reviewers; add `builder_files_carry_the_rules_digest` (in-repo and both presets contain `Rules in short`, `no production code without a failing test first`, `no fix without a root cause`, `no completion claim without fresh evidence`, `watch it fail` and `read the output and exit code`). `build_checks.rs`: `every_builder_prompt_names_subagent_driven_development` (first, task and resume prompts) and keep the existing `use this excerpt instead and do not read the plan file in full` pin. Run `cargo test -j4 -p factory-cli --test agent_lint` and `cargo test -j4 -p factory-engine --test build_checks every_builder` and confirm failure.
- [ ] **Step 2: Rewrite** the agent files and the `specs.rs` prompts as described. Replace the final "Reply with one line for Task N" with the status contract. Update the builder row in `.claude/software-factory.md`.
- [ ] **Step 3: Verify.** Run `cargo test -j4 -p factory-cli --test agent_lint`, `cargo test -j4 -p factory-engine`, and `cargo test -j4 -p factory-core` (preset tests). Expected: pass.

Next plan: docs/superpowers/plans/2026-10-18-factory-command-and-presets/plan-02-command-text.md
