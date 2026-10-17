# Command text for 7.0.0 Implementation Plan

> **For agentic workers:** This plan is executed by the factory driver using `superpowers:subagent-driven-development`: one fresh `factory-builder` implementer per task, no per-task review, and the driver does the ledger, commits and the final review. Do not commit. Steps use checkbox syntax for tracking.

**Goal:** Rewrite the embedded command and Rocket's in-repo copy to use the 7.0.0 skills as they are, with only the layer the driver needs.

**Architecture:** The 6.4.1-era rules are removed from both command copies and the lint tests are flipped in the same change.

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

- The command must still produce a spec with `## Goal` (8 or more words) and `## Acceptance criteria`, or `factory new` refuses it.
- A user who takes the skill's opt-out must be told that `factory new` needs a spec, not left with no spec and no explanation.

---

### Task 1: Rewrite the command text for 7.0.0

**Files:**
- Modify: `crates/factory-core/assets/global-command.md`, `.claude/commands/factory.md`
- Test: `crates/factory-cli/tests/global_command.rs`, `crates/factory-cli/tests/agent_lint.rs` (`command_is_thin_and_pins_the_shapes`)

**Interfaces:**
- Produces, step 1 of both commands: the session first runs `<factory> --project <name> spec start <slug> '<feature>'` (Rocket copy: `<factory> spec start`), then invokes `superpowers:brainstorming` and follows it fully; every question carries a recommended answer plus alternatives; the session writes the draft spec file at the path `spec start` printed and adds each answered question to `## Decisions` as a list item with its options and `Decided by: user` (or `Decided by: call`); it runs `spec stage <slug> writing-spec` and `writing-plans` at the transitions; the 7.0.0 builder check runs before the user sees the document; if the user takes the skill's opt-out it says `factory new` needs a spec and asks whether to continue; the spec has `## Goal` and `## Acceptance criteria`; no plan before explicit approval; commit with a pathspec commit after approval.
- Produces, step 2: `superpowers:writing-plans` in the 7.0.0 style (signatures and named assertions, an Interfaces block and a File Structure map, the plan self-review, no commit steps, `Review Focus` as the skill defines it, at most 3 tasks), an executor line naming `superpowers:subagent-driven-development` and never `executing-plans`, and `Next plan:` as the last line.
- Removes: the classification and "full architectural path" sentence, the subagent ban for design and writing, the topic list, the coverage gate, "propose 2 to 3 approaches", per-section approval, the inline spec self-review, "2 to 5 minute actions with exact code", and the acceptance-criteria reading of Review Focus.
- Consumes: the argument-hint line pinned at `global_command.rs`; update that pin only if the hint changes.

- [ ] **Step 1: Write the failing tests.** `global_command.rs`: require `superpowers:brainstorming`, `superpowers:writing-plans`, `superpowers:subagent-driven-development`, `spec start`, `spec stage`, `recommended answer`, `Decided by: user`, `builder check`, `opt-out`, `Next plan:`, `## Goal`, `## Acceptance criteria`; forbid `superpowers:executing-plans`, `architectural path`, `never classify`, `spec self-review`, `Coverage gate`, `Propose 2 to 3`, `approval after each section`, `2 to 5 minute`; replace the old Decisions and Rejected-alternatives pins with the new ones. `agent_lint.rs`: update `command_is_thin_and_pins_the_shapes` the same way for the Rocket copy, keeping its pins for `at most 3 tasks`, `no placeholders`, `skip the skill's execution handoff`, the approval phrases and the per-crate cargo rule. Run `cargo test -j4 -p factory-cli --test global_command --test agent_lint` and confirm failure.
- [ ] **Step 2: Rewrite** both command files. Keep the project lookup, escaping, `factory new`, run and status sections, the ticket rule and the Follow-ups rule as they are.
- [ ] **Step 3: Verify.** Run `cargo test -j4 -p factory-cli` and `cargo test -j4 -p factory-core` (`rocket_parity`). Expected: pass.

Next plan: docs/superpowers/plans/2026-10-18-factory-command-and-presets/plan-03-presets-refresh.md
