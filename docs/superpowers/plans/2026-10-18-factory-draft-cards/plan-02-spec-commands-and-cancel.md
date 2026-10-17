# Spec commands and slug cancel Implementation Plan

> **For agentic workers:** This plan is executed by the factory driver using `superpowers:subagent-driven-development`: one fresh `factory-builder` implementer per task, no per-task review, and the driver does the ledger, commits and the final review. Do not commit. Steps use checkbox syntax for tracking.

**Goal:** Add `factory spec start|stage|resume` and let `factory cancel` remove a draft by slug.

**Architecture:** Small engine helpers write drafts only through `BoardStore::update`. The CLI gets a `spec` group and a slug path in `cancel`.

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

- Two drafts with the same slug must be refused by `spec start`, and a slug is compared after `slugify`.
- `factory cancel 12` (numeric) must still cancel job 12 and `factory cancel demo` must reach the draft path; a slug that matches neither must give a clear error, not `BadId`.

---

### Task 1: `factory spec` commands and slug cancel

**Files:**
- Create: `crates/factory-engine/src/drafts.rs` (export in `lib.rs`)
- Modify: `crates/factory-cli/src/cli.rs`, `crates/factory-cli/src/commands.rs`, `crates/factory-cli/src/status.rs`
- Test: `crates/factory-engine/src/drafts.rs` (inline), `crates/factory-cli/tests/draft_cards.rs` (new), `crates/factory-cli/src/cli.rs` (parse tests)

**Interfaces:**
- Produces: `pub fn start_draft(driver: &Driver, slug: &str, feature: &str, date: &str, now: f64) -> Result<Draft, EngineError>` (slugifies, refuses a duplicate with `EngineError`, sets stage `interviewing`, `spec_file` to the repo-relative spec path); `pub fn set_draft_stage(driver: &Driver, slug: &str, stage: &str, now: f64) -> Result<Draft, EngineError>` (stage must be in `DRAFT_STAGES`, bumps `updated_at`); `pub fn remove_draft(driver: &Driver, slug: &str) -> Result<Draft, EngineError>`; `pub fn find_draft(driver: &Driver, slug: &str) -> Result<Option<Draft>, EngineError>`.
- Produces: clap `Command::Spec { cmd: SpecCmd }` with `SpecCmd::Start { slug, feature }`, `SpecCmd::Stage { slug, stage }`, `SpecCmd::Resume { slug }`. `spec resume` prints three lines: `draft <slug>`, `stage <stage>`, `file <path> (exists|missing)`.
- Consumes: `BoardStore::update`, `Layout::draft_spec_file`, the date string `new_job` already uses.

- [ ] **Step 1: Write the failing tests.** `drafts.rs`: `start_creates_an_interviewing_draft`, `start_refuses_a_duplicate_slug_after_slugify`, `stage_rejects_an_unknown_stage_and_bumps_updated_at`. `draft_cards.rs` with `Fixture`: `spec_start_stage_resume_round_trip`, `cancel_by_slug_removes_the_card_and_keeps_the_spec_file` (create the file first, assert it still exists), `cancel_by_number_still_cancels_a_job`, `cancel_of_an_unknown_slug_is_a_clear_error`, `status_lists_drafts`. `cli.rs`: `spec_subcommands_parse`. Run `cargo test -j4 -p factory-engine drafts` and `cargo test -j4 -p factory-cli --test draft_cards` and confirm failure.
- [ ] **Step 2: Implement** the helpers and the `spec` group. In the `Cancel` dispatch arm, when `normalize_id` fails and `find_draft` finds the slug, call a new `cancel_draft(ctx, slug)`; otherwise keep the existing error. `spec` commands skip `ctx.runnable()`. `render_status` appends a `drafts:` block when any exist.
- [ ] **Step 3: Verify.** Run `cargo test -j4 -p factory-engine -p factory-cli`. Expected: pass, including `no_hardcoded_paths`.

Next plan: docs/superpowers/plans/2026-10-18-factory-draft-cards/plan-03-adopt-draft-and-warning.md
