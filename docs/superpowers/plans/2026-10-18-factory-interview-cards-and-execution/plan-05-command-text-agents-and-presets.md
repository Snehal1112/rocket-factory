# Command text, agent files and preset refresh Implementation Plan

> **For agentic workers:** This plan is executed by the factory driver using `superpowers:subagent-driven-development`: one fresh `factory-builder` implementer per task, no per-task review, and the driver does the ledger, commits and the final review. Do not commit. Steps use checkbox syntax for tracking.

**Goal:** Point the interview, the plans and the builder at the superpowers 7.0.0 skills as they are, keep only the layer the driver needs, and let seeded homes receive the new agent text.

**Architecture:** The builder and reviewer agent files, the builder prompt in `specs.rs`, and the embedded command are rewritten to name `superpowers:subagent-driven-development` and the status and ruling format that plan 01 parses. The 6.4.1-era rules are removed from both command copies, and the lint tests that pinned them are flipped in the same change. A new `factory presets refresh` updates seeded preset files without touching project or repo overrides.

**Tech Stack:** Rust, cargo with `-j4`, clap, Markdown agent and command files, `agent_lint.rs` and `global_command.rs` tests.

**Spec:** `docs/superpowers/specs/2026-10-18-factory-interview-cards-and-execution-design.md` (Design 1, Design 3, seeded homes).

## Global Constraints

- Every cargo call has `-j4`. Never `--workspace`, `--all` or `CARGO_TARGET_DIR`. Run `cargo test -j4 -p factory-cli --test agent_lint` after editing any `.claude/agents/factory-*.md`.
- The embedded command (`crates/factory-core/assets/global-command.md`) must not contain (lowercased) `tools/factory`, `cargo build`, `opencollection`, `factory-ctl`, `python3`, `set-stage`, `sendmessage` or `rocket`.
- Rocket's in-repo copy keeps its two Rocket-only plan rules (per-crate cargo commands, the opencollection read line).
- No file the driver uses (agent files, presets, `specs.rs` prompts, both commands) may name `superpowers:executing-plans` after this plan.
- The user's own answers go in `## Decisions` as list items containing `Decided by: user`; the author's calls contain `Decided by: call`. Plan 03 counts the first form.
- Refresh never writes under `<home>/projects/` or a repo's `.factory/`.
- No commit steps in any task. The driver commits.

## Review Focus

- The command must still produce a spec with `## Goal` (8 or more words) and `## Acceptance criteria`, or `factory new` refuses it.
- A user who takes the skill's opt-out must be told that `factory new` needs a spec, not left with no spec and no explanation.
- A preset file edited by hand must be refused by `refresh` without `--force`, and must be left byte-identical.
- A preset file equal to the current embedded text must be reported as skipped, not rewritten.
- An old builder reply with no status line must still work, so the `Done` default from plan 01 must stay covered by a test.

---

### Task 1: Builder and reviewer text names subagent-driven-development

**Files:**
- Modify: `.claude/agents/factory-builder.md`, `.claude/agents/factory-code-reviewer.md`, `crates/factory-core/presets/{rust,go}/agents/factory-builder.md`, `crates/factory-core/presets/{rust,go}/agents/factory-code-reviewer.md`
- Modify: `crates/factory-engine/src/specs.rs` (`task_excerpt`, `builder_first_spec`, `builder_task_spec`, `builder_resume_spec`), `.claude/software-factory.md` (builder row near line 46)
- Test: `crates/factory-cli/tests/agent_lint.rs`, `crates/factory-engine/tests/build_checks.rs`

**Interfaces:**
- Produces: builder agent text that says the plan is run through `superpowers:subagent-driven-development` as its implementer, drops the inline fallback and the skip-list for `executing-plans`, keeps test-first, debugging and verification skills and "no commits", and ends with the reply contract: first line `Status: DONE | DONE_WITH_CONCERNS | BLOCKED | NEEDS_CONTEXT`, then at most five lines of concerns or the blocker, then `Ruling: <what> — <why> — <cost if wrong>` lines; no commits and no report file; a first-line `BLOCKED: <reason>` is still accepted.
- Produces: reviewer agent Input line naming the plan's Review Focus, the builder rulings and the builder concerns.
- Produces: every builder prompt in `specs.rs` names `superpowers:subagent-driven-development`, and the excerpt sentence reads "If a skill such as superpowers:subagent-driven-development tells you to read or load the plan file, use this excerpt instead and do not read the plan file in full."

- [ ] **Step 1: Write the failing tests.** `agent_lint.rs`: change `builder_body_pins_the_rules` to require `superpowers:subagent-driven-development`, `Status:`, `DONE_WITH_CONCERNS`, `NEEDS_CONTEXT`, `Ruling:` and to forbid `superpowers:executing-plans`, `inline fallback` and `No handoff lines`; add `presets_name_subagent_driven_development_and_not_executing_plans` over both preset builders; add a reviewer pin for `Review Focus` and `rulings` in the in-repo and preset reviewers. `build_checks.rs`: `every_builder_prompt_names_subagent_driven_development` (first, task and resume prompts) and keep the existing `use this excerpt instead and do not read the plan file in full` pin. Run `cargo test -j4 -p factory-cli --test agent_lint` and `cargo test -j4 -p factory-engine --test build_checks every_builder` and confirm failure.
- [ ] **Step 2: Rewrite** the agent files and the `specs.rs` prompts as described. Replace the final "Reply with one line for Task N" with the status contract. Update the builder row in `.claude/software-factory.md`.
- [ ] **Step 3: Verify.** Run `cargo test -j4 -p factory-cli --test agent_lint`, `cargo test -j4 -p factory-engine`, and `cargo test -j4 -p factory-core` (preset tests). Expected: pass.

### Task 2: Rewrite the command text for 7.0.0

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

### Task 3: Refresh seeded presets

**Files:**
- Read: `crates/factory-core/presets/legacy/` (created in plan 02 Task 2, embedded here with `include_str!`)
- Modify: `crates/factory-core/src/preset.rs`, `crates/factory-core/src/lib.rs`, `crates/factory-cli/src/cli.rs`, `crates/factory-cli/src/main.rs`, `README.md`
- Test: `crates/factory-core/src/preset.rs` (inline), `crates/factory-cli/tests/presets_refresh.rs` (new)

**Interfaces:**
- Produces: `pub fn refresh(presets_dir: &Path, force: bool) -> Result<RefreshReport, CoreError>` and `pub struct RefreshReport { pub written: Vec<PathBuf>, pub updated: Vec<PathBuf>, pub skipped: Vec<PathBuf>, pub refused: Vec<PathBuf> }`. Per embedded file: absent is written; equal is skipped; different but equal to an entry in the legacy texts is updated; any other difference is refused unless `force`, which overwrites. Writes are atomic like `install.rs`.
- Produces: `factory presets refresh [--force]`, handled before the repo context like `install`, printing one line per path with its outcome and a final count; exit non-zero never, but refused paths are listed with the hint to use `--force`.
- Consumes: the embedded preset table and `write_atomic`.

- [ ] **Step 1: Write the failing tests.** `preset.rs`: `refresh_writes_absent_files`, `refresh_skips_equal_files`, `refresh_updates_a_known_legacy_text`, `refresh_refuses_a_hand_edited_file_and_leaves_it_byte_identical`, `refresh_force_overwrites_a_hand_edited_file`, `refresh_never_touches_project_or_repo_layers` (create `projects/p/agents/factory-builder.md` beside the presets and assert it is unchanged). `presets_refresh.rs`: `the_command_reports_each_outcome_and_refused_paths`. Run `cargo test -j4 -p factory-core refresh` and `cargo test -j4 -p factory-cli --test presets_refresh` and confirm failure.
- [ ] **Step 2: Implement** `refresh` and the command, embedding the four legacy files as the known-old texts.
- [ ] **Step 3: Document** `presets refresh` and the need to run `factory install --force` after upgrading in `README.md`.
- [ ] **Step 4: Verify.** Run `cargo test -j4 -p factory-core -p factory-cli`. Expected: pass.

Next plan: none
