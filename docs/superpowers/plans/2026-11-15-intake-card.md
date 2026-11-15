# Intake Card Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show a dashboard card for an intake issue from the claim on, keep a failed draft visible, and mark the job card with its issue.

**Architecture:** `Draft` gets two optional fields. `run_intake` writes a draft card `issue-<n>` at the claim and marks it failed when drafting fails. Job creation already adopts (removes) the draft by slug. The dashboard reads the new fields and `job.intake_issue`.

**Tech Stack:** Rust (factory-core, factory-engine, dashboard), plain JS (`dashboard/static/board.js`, `node --test`).

**Spec:** `docs/superpowers/specs/2026-11-15-intake-card-design.md`

## Global Constraints

- Board JSON stays compatible with `factory-server`: new fields are optional and skipped when `None`.
- Cargo: always `-j4`, never `--workspace` or `--all`, never `CARGO_TARGET_DIR`.
- Tests use `.expect("...")`, never the unwrap call. Code comments are short full sentences.
- Draft writes in intake are best effort: a failure is logged with `tracing::warn!` and never changes the pass result.
- Stage names: `drafting-from-issue` ("Drafting from issue") and `draft-failed` ("Draft failed").

## Review Focus

1. An old board without `issue` and `note` loads and writes back without them (core test).
2. A relabelled issue whose old `issue-7` card is `draft-failed` gets a fresh card at the next claim (engine test).
3. A job-creating failure (`build-failed`) leaves no stray draft card (engine test).
4. A hostile feature text, note or stage on a draft card is escaped, and an unknown stage shows as plain text (JS test).
5. A wrong-typed `issue` or `note` in `board.json` drops only that field, not the card (dashboard model test).

## File Structure

- `crates/factory-core/src/board.rs`: `Draft` fields, `DRAFT_STAGES`.
- `crates/factory-engine/src/drafts.rs`: `start_intake_draft`, `fail_intake_draft`.
- `crates/factory-engine/src/intake.rs`: calls them; shared `issue_feature`.
- `dashboard/src/model.rs`: `Draft` and `Job` fields.
- `dashboard/static/board.js`: draft card and issue chip. `dashboard/static/*.css`: chip style.

---

### Task 1: Core fields and engine writes

**Files:**
- Modify: `crates/factory-core/src/board.rs` (struct `Draft` ~line 471, `DRAFT_STAGES` line 467)
- Modify: `crates/factory-core/tests/board_roundtrip.rs` (the draft test near line 74)
- Modify: `crates/factory-engine/src/drafts.rs`, `crates/factory-engine/src/intake.rs`
- Test: `crates/factory-engine/tests/intake.rs`

**Interfaces:**
- Consumes: `Driver::store().update(..)`, `remove_draft`, `find_draft`, `crate::clock::now_secs()`.
- Produces: `Draft { issue: Option<u32>, note: Option<String> }`; `pub(crate) fn start_intake_draft(driver: &Driver, issue: u32, feature: &str, date: &str, now: f64) -> Result<Draft, EngineError>`; `pub(crate) fn fail_intake_draft(driver: &Driver, issue: u32, code: &str, now: f64) -> Result<Draft, EngineError>`.

- [ ] **Step 1: Read** `drafts.rs` (`start_draft`, `set_draft_stage`), `intake.rs` (`run_intake`, `draft_jobs`, `settle_failure`) and the fixtures `setup`, `one_plan`, `pass`, `comments` in `tests/intake.rs`.
- [ ] **Step 2: Write the failing tests.**
  - In `board_roundtrip.rs`, extend the existing draft test: set `issue: Some(7)` and `note: Some("draft-refused".into())` on the first draft and assert a round trip equals it. For the bare copy also set `issue = None; note = None` and assert the JSON contains neither `"issue"` nor `"note"`. Change the `DRAFT_STAGES` assertion to `["interviewing", "writing-spec", "writing-plans", "drafting-from-issue", "draft-failed"]`.
  - In `tests/intake.rs` add these, reading the board with `it.driver.store().load().expect("board").drafts`:

| Test | Setup | Assertions |
|---|---|---|
| `the_claim_adds_a_draft_card_while_the_drafter_runs` | `setup` with a closure that, given the draft folder, reads `draft.parent().parent().parent().join("board.json")` into a shared `Arc<Mutex<String>>`, then writes `one_plan(draft)` | parse the captured text with `serde_json::from_str::<factory_core::Board>`: one draft, slug `issue-7`, `issue == Some(7)`, stage `drafting-from-issue`, no jobs |
| `a_merged_issue_leaves_no_draft_card` | `setup(one_plan)`, `pass` | board has 1 job and `drafts.is_empty()`, job `intake_issue == Some(7)` |
| `a_refused_draft_leaves_a_failed_card_with_the_reason_code` | the `refused.md` closure from `a_refused_draft_labels_the_issue_needs_human` | one draft, stage `draft-failed`, `note == Some("draft-refused".into())`, `issue == Some(7)` |
| `a_second_pass_replaces_the_failed_card` | refused closure, `pass` twice (relabel the issue between passes with `it.tracker` the way `a_tracker_failure_...` or the existing multi-pass tests do) | after the second pass still exactly one draft `issue-7`, with a newer `updated_at` |
| `a_build_failure_leaves_no_draft_card` | `setup_changes_for("001", config())` style fixture that parks the job (reuse `a_multi_plan_series_names_the_unmerged_job_in_the_failure_comment`'s setup) | `drafts.is_empty()` |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-core --test board_roundtrip` and `cargo test -j4 -p factory-engine --test intake`. Expected: compile errors `no field issue on type Draft` first, then failing asserts.
- [ ] **Step 4: Implement.**
  - `board.rs`: add to `Draft` `#[serde(default, skip_serializing_if = "Option::is_none")] pub issue: Option<u32>` and the same for `note: Option<String>`, with a doc line each. Extend `DRAFT_STAGES` to the five names above (type `[&str; 5]`). Set both new fields to `None` in `start_draft`.
  - `drafts.rs`: `start_intake_draft` removes any draft with the slug `issue-<issue>` and pushes a new one in one `store().update` (stage `drafting-from-issue`, `spec_file: None`, `issue: Some(issue)`, `note: None`, `created: date`). `fail_intake_draft` sets stage `draft-failed`, `note: Some(code)` and `updated_at` on that slug, and errors with `no draft named ..` when missing.
  - `intake.rs`: extract the feature formatting from `draft_jobs` into `fn issue_feature(issue: &IssueInfo) -> String` and use it in both places. In `run_intake` right after the claim call `start_intake_draft(driver, number, &issue_feature(&issue), &crate::clock::today(), now_secs())` (use the date helper in `clock.rs`; if its name differs, use that one), logging an error with `tracing::warn!(event = "intake.card", ..)` and going on. Give `settle_failure` a `driver: &Driver` parameter: when `f.job.is_none()` call `fail_intake_draft(driver, issue, f.code, now_secs())` and log an error the same way.
- [ ] **Step 5: Run** the two commands from Step 3, then `cargo test -j4 -p factory-engine` and `cargo test -j4 -p factory-cli`. Expected: all pass (the existing intake tests must still pass unchanged).

### Task 2: Dashboard model, card and chip

**Files:**
- Modify: `dashboard/src/model.rs`, `dashboard/static/board.js`, the CSS file that holds `.chip-provider`
- Test: `dashboard/src/state.rs` tests, `dashboard/tests/board.test.js`

**Interfaces:**
- Consumes: Task 1's JSON fields `issue` and `note` on drafts, and the existing `intake_issue` on jobs.
- Produces: `Draft.issue: Option<u32>`, `Draft.note: Option<String>`, `Job.intake_issue: Option<u32>`; JS `issueChip(issue)`.

- [ ] **Step 1: Read** `dashboard/src/model.rs` (`Draft`, `Job`, `lenient`), the draft tests at `state.rs:1019-1080`, `renderDraftCard` and `providerChip` in `board.js`, and `makeDraft` in `tests/board.test.js:1024`.
- [ ] **Step 2: Write the failing tests.**
  - `state.rs`: `a_draft_with_issue_and_note_carries_them_into_the_state_json` (board JSON draft with `"issue": 7, "note": "draft-refused"`, assert both in the draft card JSON); `a_wrong_typed_issue_or_note_is_dropped_not_the_card` (`"issue": "x", "note": 5` gives one draft whose `issue` and `note` are `None`); `a_job_carries_intake_issue` (job JSON with `"intake_issue": 7` parses to `Some(7)`, and a string value gives `None`).
  - `board.test.js`: with `makeDraft({ stage: "drafting-from-issue", issue: 7, feature: "Fix it, #7" })` assert the card matches `/Drafting from issue/`, `/issue #7/` and `/factory cancel/`, and does not match `/factory spec resume/`. With `stage: "draft-failed", note: "draft-refused"` assert `/Draft failed/` and `/draft-refused/`. With `note: "<img src=x>"` and `stage: "<b>"` assert `/&lt;img/`, `/&lt;b&gt;/` and no `<img`. A job test: `renderCard(makeJob({ intake_issue: 7 }))` matches `/chip-issue[^>]*>issue #7</`, and without the field has no `chip-issue`.
- [ ] **Step 3: Run** `cargo test -j4 --manifest-path dashboard/Cargo.toml` and `node --test dashboard/tests/`. Expected: the new tests fail.
- [ ] **Step 4: Implement.** `model.rs`: add the three fields with `#[serde(default, deserialize_with = "lenient")]` (and `skip_serializing_if = "Option::is_none"`), like `cancelled`. `board.js`: add the two stages to `DRAFT_STAGES`; in `renderDraftCard` show the note after the stage as `<span class="draft-note">` (escaped), show `issue #n` in the header when `draft.issue` is a number, and drop the `factory spec resume` chip when `draft.issue` is a number; add `issueChip(issue)` returning `<span class="chip chip-issue" title="Made from this issue">issue #N</span>` only for a positive integer, call it in `renderCard`'s header after `providerChip`, and export it. CSS: give `.chip-issue` the same size rule as `.chip-provider`.
- [ ] **Step 5: Run** the two commands from Step 3. Expected: all pass. Then open the dashboard on a sample board that has an `issue-7` draft and an `intake_issue` job (`dashboard/tests/seed-demo.sh` shows how) and check the card by eye.

Next plan: none
