# Draft card in the Spec column Implementation Plan

> **For agentic workers:** This plan is executed by the factory driver using `superpowers:subagent-driven-development`: one fresh `factory-builder` implementer per task, no per-task review, and the driver does the ledger, commits and the final review. Do not commit. Steps use checkbox syntax for tracking.

**Goal:** Render draft cards first in the Spec column with stage, age, stale marker and the spec warning.

**Architecture:** A `renderDraftCard` function and a column change in `board.js`, plus CSS, README and demo data. A missing `drafts` means an empty list.

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

- A state from an older server without `drafts` must still render the board.
- A feature or slug containing `<script>` must be escaped in the card.

---

### Task 1: Draft card in the Spec column

**Files:**
- Modify: `dashboard/static/board.js`, `dashboard/static/dashboard.css`, `dashboard/README.md`, `dashboard/tests/seed-demo.sh`, `README.md` (home config key)
- Test: `dashboard/tests/board.test.js`, `dashboard/src/server.rs` (page assets test)

**Interfaces:**
- Produces: `renderDraftCard(draft, now)` in `board.js` (exported in `module.exports`) returning `<article class="card card-spec card-draft" data-draft="<slug>">` with the slug, a "Draft" label, the escaped feature, a readable stage label (unknown stages shown escaped as-is), `formatAge(seconds_since_update)`, a text `stale` chip when `stale`, the spec warning when present, and `copyChip` buttons for `factory spec resume <slug>`, `factory cancel <slug>` and the spec file path.
- Produces: `renderBoard` renders `state.drafts || []` first in the `spec` column and counts them in the column total; `renderSummary` notes drafts and stale drafts.
- Consumes: `escapeHtml`, `formatAge`, `copyChip`, `plural`.

- [ ] **Step 1: Write the failing tests** in `board.test.js`: `a_draft_card_shows_slug_stage_age_and_copy_chips`, `a_stale_draft_shows_a_stale_chip_in_text`, `draft_text_is_escaped`, `drafts_come_first_in_the_spec_column_and_are_counted`, `a_state_without_drafts_still_renders_the_board`, `a_job_spec_warning_shows_on_the_card`. In `server.rs`: `the_page_carries_the_draft_card_script_and_styles` (assert `card-draft` appears in the CSS and `renderDraftCard` in the JS). Run `node --test dashboard/tests/board.test.js` and confirm failure.
- [ ] **Step 2: Implement** the renderer, the column change, the CSS for `.card-draft` and the stale chip, the README endpoint row, flag and home-config key, and a demo board in `seed-demo.sh` with one fresh draft and one stale draft (`updated_at = now - 90000`).
- [ ] **Step 3: Verify.** Run `node --test dashboard/tests/*.test.js`, `cargo test -j4 --manifest-path dashboard/Cargo.toml`, and `bash dashboard/tests/smoke.sh`. Expected: pass. Note in the result that the visual check in a real window is left to the user.

Next plan: none
