# Refresh seeded presets Implementation Plan

> **For agentic workers:** This plan is executed by the factory driver using `superpowers:subagent-driven-development`: one fresh `factory-builder` implementer per task, no per-task review, and the driver does the ledger, commits and the final review. Do not commit. Steps use checkbox syntax for tracking.

**Goal:** Add `factory presets refresh` so seeded homes can receive the new agent text without touching project or repo overrides.

**Architecture:** `preset::refresh` compares each embedded file with the seeded one and the known legacy texts, and refuses hand edits without `--force`.

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

- A preset file edited by hand must be refused by `refresh` without `--force`, and must be left byte-identical.
- A preset file equal to the current embedded text must be reported as skipped, not rewritten.

---

### Task 1: Refresh seeded presets

**Files:**
- Read: `crates/factory-core/presets/legacy/` (created in execution plan 04, embedded here with `include_str!`)
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

Next plan: docs/superpowers/plans/2026-10-18-factory-command-and-presets/plan-04-plan-lint-and-skill-drift.md
