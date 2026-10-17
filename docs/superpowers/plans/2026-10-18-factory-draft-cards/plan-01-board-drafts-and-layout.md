# Board drafts, spec path and warning field Implementation Plan

> **For agentic workers:** This plan is executed by the factory driver using `superpowers:subagent-driven-development`: one fresh `factory-builder` implementer per task, no per-task review, and the driver does the ledger, commits and the final review. Do not commit. Steps use checkbox syntax for tracking.

**Goal:** Add the optional `drafts[]` board field, the draft spec path on `Layout`, and the `spec_warning` job field.

**Architecture:** New optional board fields with the usual serde rules, plus a `Layout` method so no path literal leaves `layout.rs`.

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

- A board written by an old binary (no `drafts` key) must load, and a board with empty drafts must serialize byte-identical to today.

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

Next plan: docs/superpowers/plans/2026-10-18-factory-draft-cards/plan-02-spec-commands-and-cancel.md
