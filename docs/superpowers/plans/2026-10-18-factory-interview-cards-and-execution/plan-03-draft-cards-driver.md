# Draft spec cards, driver side Implementation Plan

> **For agentic workers:** This plan is executed by the factory driver using `superpowers:subagent-driven-development`: one fresh `factory-builder` implementer per task, no per-task review, and the driver does the ledger, commits and the final review. Do not commit. Steps use checkbox syntax for tracking.

**Goal:** Let a `/factory` session register a draft card from the start of the interview, move it through its stages, resume or cancel it, and have `factory new` adopt it and warn when no decision was answered by the user.

**Architecture:** A new optional top-level `drafts[]` on the board (not `Job` records). Small engine helpers write it only through `BoardStore::update`. The CLI gets a `spec` subcommand group and a slug path in `cancel`. `create_job_with_spec` removes the draft after the series jobs exist and stores a spec warning on the first job.

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
- Two drafts with the same slug must be refused by `spec start`, and a slug is compared after `slugify`.
- `factory cancel 12` (numeric) must still cancel job 12 and `factory cancel demo` must reach the draft path; a slug that matches neither must give a clear error, not `BadId`.
- A board written by an old binary (no `drafts` key) must load, and a board with empty drafts must serialize byte-identical to today.
- A spec whose `## Decisions` is missing must produce the warning, not an error.

---

### Task 1: Board field, spec path and warning field

**Files:**
- Modify: `crates/factory-core/src/board.rs`, `crates/factory-core/src/lib.rs`, `crates/factory-core/src/layout.rs`
- Test: `crates/factory-core/src/board.rs` (inline), `crates/factory-core/tests/board_roundtrip.rs`, `crates/factory-core/src/layout.rs` (inline)

**Interfaces:**
- Produces: `pub struct Draft { pub slug: String, pub feature: String, pub stage: String, pub spec_file: Option<String>, pub created: String, pub updated_at: f64 }` with `#[serde(default, skip_serializing_if = ...)]` on `spec_file`; `pub const DRAFT_STAGES: [&str; 3] = ["interviewing", "writing-spec", "writing-plans"]`.
- Produces: `Board.drafts: Vec<Draft>` with `#[serde(default, skip_serializing_if = "Vec::is_empty")]`; `Job.spec_warning: Option<String>` optional and omitted when `None`, set to `None` in `Job::new`.
- Produces: `Layout::draft_spec_file(&self, date: &str, slug: &str) -> PathBuf` returning `<repo>/docs/superpowers/specs/<date>-<slug>-design.md`. No new `Layout` field.

- [ ] **Step 1: Write the failing tests.** `board_roundtrip.rs`: `an_old_board_without_drafts_loads`, `a_board_with_no_drafts_serializes_without_the_key`, `drafts_round_trip_with_every_field`, `spec_warning_is_optional_on_a_job`. `layout.rs`: `draft_spec_file_is_under_the_repo_specs_folder`. Run `cargo test -j4 -p factory-core drafts spec_warning draft_spec` and confirm failure.
- [ ] **Step 2: Implement** the fields, the constant, the `Layout` method and the `lib.rs` export. Fix the `Board { jobs: ... }` literals in `board.rs` tests with `..Default::default()`.
- [ ] **Step 3: Verify.** Run `cargo test -j4 -p factory-core` and `cargo test -j4 -p factory-cli --test no_hardcoded_paths`. Expected: pass.

### Task 2: `factory spec` commands and slug cancel

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

### Task 3: Adopt the draft and warn when no decision came from the user

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

Next plan: docs/superpowers/plans/2026-10-18-factory-interview-cards-and-execution/plan-04-draft-cards-dashboard.md
