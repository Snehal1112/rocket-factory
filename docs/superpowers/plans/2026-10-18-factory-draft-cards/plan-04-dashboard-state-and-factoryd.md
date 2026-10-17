# Drafts in the dashboard state and factoryd Implementation Plan

> **For agentic workers:** This plan is executed by the factory driver using `superpowers:subagent-driven-development`: one fresh `factory-builder` implementer per task, no per-task review, and the driver does the ledger, commits and the final review. Do not commit. Steps use checkbox syntax for tracking.

**Goal:** Serve drafts with a computed stale flag from both dashboard modes and from factoryd's API.

**Architecture:** The dashboard has its own board model, so it gets a lenient `Board.drafts` and a `StateView.drafts`. The threshold is a dashboard flag and a factoryd home-config key.

**Tech Stack:** Rust (separate `dashboard/` workspace and `factory-supervisor`), vanilla JS tested with `node --test`.

**Spec:** `docs/superpowers/specs/2026-10-18-factory-interview-cards-and-execution-design.md` (Design 2).

## Global Constraints

- Every cargo call has `-j4`. Dashboard tests: `cargo test -j4 --manifest-path dashboard/Cargo.toml`; JS tests: `node --test dashboard/tests/*.test.js`.
- Old boards (no `drafts`, no `spec_warning`) must load and render unchanged; a malformed draft entry is dropped, never fatal.
- Default stale threshold is 24 hours; computed at read time from `updated_at`; negative ages clamp to 0.
- The API token never appears in any page, reply, header or log line (`home_mode.rs` tests pin this). No new write route.
- No `file://` links; the draft path is shown as text with a copy button.
- No commit steps in any task. The driver commits.

## Review Focus

- A draft exactly at the threshold is not stale; one second past it is.
- `GET /projects/{name}/jobs` from factoryd must keep every existing key when drafts are present or absent.
- A factoryd home config with `draft_stale_hours = 0` must be rejected like `daily_budget_weighted = 0`.
- Both dashboard modes (`--root`, `--home`) must show the same drafts for the same board.

---

### Task 1: Drafts in the dashboard state and in factoryd

**Files:**
- Modify: `dashboard/src/model.rs`, `dashboard/src/state.rs`, `dashboard/src/specs.rs`, `dashboard/src/server.rs`, `dashboard/src/home_mode.rs`, `dashboard/src/main.rs`
- Modify: `crates/factory-core/src/homeconfig.rs`, `crates/factory-supervisor/src/api/read.rs`, `crates/factory-supervisor/src/http.rs`
- Test: inline tests in `state.rs`, `server.rs`, `home_mode.rs`, `homeconfig.rs`, `read.rs`

**Interfaces:**
- Produces (dashboard): `pub struct Draft { slug, feature, stage, spec_file: Option<String>, created, updated_at: f64 }` read with a per-item lenient reader like `lenient_history`; `Board.drafts: Vec<Draft>`; `Job.spec_warning: Option<String>` lenient.
- Produces: `pub struct DraftCard { #[serde(flatten)] pub draft: Draft, pub seconds_since_update: f64, pub stale: bool }`; `pub fn build_draft_cards(drafts: &[Draft], now: f64, stale_after_secs: f64) -> Vec<DraftCard>`; `StateView.drafts: Vec<DraftCard>`; `pub const DEFAULT_DRAFT_STALE_SECS: f64 = 86_400.0`.
- Produces: dashboard flag `--draft-stale-hours N` (default 24, integer above 0) parsed in `parse_args`, carried by `App::with_draft_stale(secs)` (builder style like `with_timeouts`) into `state_reply`; `SpecCard`/job views expose `spec_warning`.
- Produces (core): `HomeConfig.draft_stale_hours: u64` default 24, rejects 0.
- Produces (factoryd): `read::jobs` adds `"drafts": [{slug, stage, updatedAt, stale}]` using `HomeConfig::load`.

- [ ] **Step 1: Write the failing tests.** `state.rs`: `an_old_board_without_drafts_still_loads`, `a_malformed_draft_is_dropped_not_fatal`, `a_draft_is_stale_one_second_past_the_threshold_and_not_at_it`, `the_state_json_has_a_drafts_array`, `a_job_spec_warning_is_passed_through`. `server.rs`: `parse_args_reads_draft_stale_hours_and_rejects_zero`, `api_state_carries_drafts`. `home_mode.rs`: `project_state_carries_drafts`. `homeconfig.rs`: `draft_stale_hours_defaults_to_24_and_rejects_zero`. `read.rs`: `jobs_lists_drafts_and_keeps_existing_keys`. Run `cargo test -j4 --manifest-path dashboard/Cargo.toml` and `cargo test -j4 -p factory-core homeconfig` and confirm failure.
- [ ] **Step 2: Implement** the model, state, flag and home-config key. Keep `build_state`'s signature and add the threshold through `state_reply`, so the existing test calls stay unchanged. Update the dashboard usage text in `main.rs`.
- [ ] **Step 3: Verify.** Run `cargo test -j4 --manifest-path dashboard/Cargo.toml`, `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-supervisor`. Expected: pass.

Next plan: docs/superpowers/plans/2026-10-18-factory-draft-cards/plan-05-draft-card-ui.md
