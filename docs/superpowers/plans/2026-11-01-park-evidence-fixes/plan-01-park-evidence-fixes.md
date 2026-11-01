# Park evidence fixes Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-01-park-evidence-fixes-design.md
>
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Make a risky sync park leave one consistent board reason, ledger ruling and `park.json`, never show the card of an earlier park after a crash, and pin the write order and the unknown-risk text with tests that fail when they break.

**Architecture:** All changes are in `factory-engine`. `Engine::park_after_sync` returns the outcome it wrote, and `sync_stage` returns that reason. A pure helper `sync_park_text` decides the text when the park itself fails. `park_with_parts` skips its ruling and log line for an earlier park. `in_park_order` takes a `clear` step so the first write of a park removes the old `park.json`.

**Tech Stack:** Rust, `cargo test -j4 -p factory-engine`, the merge fixtures in `crates/factory-engine/tests/common/mod.rs`.

## Global Constraints

- Always `-j4` and `-p <crate>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Each task ends with `cargo fmt -p factory-engine -- --check` and `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`. No `#[allow(...)]` to silence a lint.
- No unwrap in production Rust. Tests use `.expect("...")` and `expect_err`.
- No new board, ledger or events field, and no new `park.json` field. No new trait: this logic uses the `Engine` park helpers.
- New public items carry a doc comment. Code comments are short full sentences that end with a period.
- Texts are exact: `final checks failed after sync`, `final checks could not run: {error}`, `unknown: the job diff could not be read ({error})`, and for a failed sync park `{text} (the park card could not be written: {error})`.

## Review Focus

1. A checks-failed sync park must show the same reason on the board, in `factory status` and in the ledger, with one ruling line and not two. Pinned by `a_checks_failed_sync_park_keeps_one_reason_and_one_ruling`.
2. A second park whose stage move fails must not leave the record of the first park. Pinned by `a_failed_park_write_removes_the_record_of_an_earlier_park`.
3. A failing board or ledger write inside the sync park must not turn into `merge failed`. Pinned by `a_failed_sync_park_keeps_the_risky_text_and_names_the_error`.
4. A job diff that cannot be read must show a Risk part saying so on a builder-blocked park, not an empty list. Pinned by `a_builder_stop_with_an_unreadable_diff_gets_a_risk_unknown_part`.
5. Final checks that cannot run after a sync must still park as risky with a Checks part that says why. Pinned by `final_checks_that_cannot_run_after_a_sync_park_risky_and_say_why`.

## File Structure

| File | Responsibility |
|---|---|
| `crates/factory-engine/src/park.rs` | `remove_record`, the `clear` step of `in_park_order`, `park_with` wiring, `sync_park_text`, the warn on a failed write, unit tests |
| `crates/factory-engine/src/sync.rs` | the risky arm returns the park's own reason |
| `crates/factory-engine/src/merge.rs` | `park_with_parts` writes no ruling and no log line for an earlier park |
| `crates/factory-engine/src/build.rs` | the builder-blocked park uses `lead_parts_for_card` |
| `crates/factory-engine/tests/park_evidence.rs` | the sync park integration tests |

### Task 1: One reason and one ruling after a sync park

**Files:**
- Modify: `crates/factory-engine/src/park.rs`, `crates/factory-engine/src/sync.rs`, `crates/factory-engine/src/merge.rs`
- Test: `crates/factory-engine/src/park.rs` (tests module), `crates/factory-engine/tests/park_evidence.rs`

**Interfaces:**
- Produces, in `park.rs`: `pub(crate) fn sync_park_text(parked: Result<StageOutcome, EngineError>, text: &str) -> String`. A `StageOutcome::NeedsHuman(reason)` gives `reason`. Any other outcome gives `text`. An `Err(e)` gives `format!("{text} (the park card could not be written: {e})")`.
- Changes: `sync_stage` in `sync.rs` returns `SyncOutcome::Parked(sync_park_text(driver.park_after_sync(id, &text), &text))` for a risky decision, logs the error with `tracing::error!(event = "park.error", ...)` when the park failed, and never propagates the park error with `?`.
- Changes: `park_with_parts` in `merge.rs` returns right after the board update when `already` is true: no `log_park`, no record write, no ruling.

- [ ] **Step 1: Read the code this task touches**

Read `Engine::park_after_sync` in `park.rs`, the risky arm of `sync_stage` in `sync.rs`, `park_with_parts` in `merge.rs`, and the tests `failing_final_checks_after_a_sync_park_as_checks_failed_without_a_fix_round` and `an_earlier_park_keeps_its_evidence` in `park_evidence.rs` (they hold the fixtures this task reuses).

- [ ] **Step 2: Write the failing tests**

In `park.rs` tests: `sync_park_text_uses_the_park_reason`, `sync_park_text_keeps_the_text_for_other_outcomes` and `a_failed_sync_park_keeps_the_risky_text_and_names_the_error` (an `Err(EngineError::Msg("board locked".into()))` gives `after sync: risky diff (the park card could not be written: board locked)` for the text `after sync: risky diff`).

In `park_evidence.rs`: `a_checks_failed_sync_park_keeps_one_reason_and_one_ruling`. It reuses the fixture of the failing-checks test and asserts after `merge_stage`: the board `job.reason` is `final checks failed after sync`, the `MergeResult::Parked` text is the same, the record kind is `ChecksFailed`, and the ledger holds exactly one line that contains `final checks failed after sync`.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -j4 -p factory-engine --test park_evidence a_checks_failed_sync_park_keeps_one_reason_and_one_ruling` and `cargo test -j4 -p factory-engine -- sync_park_text`.
Expected: FAIL (the helper does not exist, the reason is the risky text, the ledger has two lines).

- [ ] **Step 4: Implement `sync_park_text` and the two call-site changes**

Approach: `Engine::park_after_sync` already returns `Result<StageOutcome, EngineError>`, so only the call sites change. In `park_with_parts`, return the `(job, already)` pair as today and make the `if !already` block the only code that logs, writes the record and writes the ruling.

- [ ] **Step 5: Run the tests, fmt and clippy**

Run: `cargo test -j4 -p factory-engine`, then the fmt and clippy commands from Global Constraints.
Expected: all pass, no warnings.

### Task 2: No stale record after a crash, and a warn on a failed write

**Files:**
- Modify: `crates/factory-engine/src/park.rs`
- Test: `crates/factory-engine/src/park.rs` (tests module)

**Interfaces:**
- Consumes: `write_record(dir: &Path, record: &ParkRecord)` and `in_park_order` as they are.
- Produces: `fn remove_record(dir: &Path)` (removes `park.json`, a missing file is fine, any other error is logged with `tracing::warn!(event = "park.record_error", ...)`).
- Changes: `in_park_order` takes a first step `clear: impl FnOnce()` that runs before `update`. `park_with` passes `|| remove_record(&self.job_dir)`. `write_record` calls `remove_record` and logs a failed write with the same warn event instead of `let _`.

- [ ] **Step 1: Write the failing tests**

In the `park.rs` tests module, update the four existing order tests for the new `clear` argument and extend `park_steps_run_update_then_stage_then_record_then_ruling` to `clear_then_update_then_stage_then_record_then_ruling`, which asserts the order `["clear", "update", "stage", "record", "ruling"]`.

Add `a_failed_park_write_removes_the_record_of_an_earlier_park`. It calls the real `Engine::park_with` against an engine whose stage move fails (use the failure injection the runloop tests use for an unreadable ledger: a directory where the file must go, or a job missing from the board), after writing a first record with `write_record` in the job folder. It asserts that `park.json` does not exist afterwards and that `park_with` returned the error. This test exercises `park_with` itself, so swapping the closures in `park_with` fails it.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -j4 -p factory-engine -- a_failed_park_write_removes_the_record_of_an_earlier_park clear_then_update`.
Expected: FAIL (`in_park_order` has no `clear` step, the old record survives).

- [ ] **Step 3: Implement `remove_record` and the `clear` step**

Approach: move the `remove_file` call out of `write_record` into `remove_record`, call it from both. The warn event carries the path and the error text.

- [ ] **Step 4: Run the tests, fmt and clippy**

Run: `cargo test -j4 -p factory-engine`, then the fmt and clippy commands.
Expected: all pass, no warnings.

### Task 3: Close the evidence test gaps

**Files:**
- Modify: `crates/factory-engine/src/build.rs`
- Test: `crates/factory-engine/tests/park_evidence.rs`, `crates/factory-engine/src/park.rs` (tests module)

**Interfaces:**
- Consumes: `Engine::lead_parts_for_card(&self) -> Vec<ParkPart>`.
- Changes: the builder-blocked park in `build.rs` (the `fix_source` of `review` or `rework` branch) uses `self.lead_parts_for_card()` instead of `self.lead_parts(&self.job_changed_paths().unwrap_or_default())`.

- [ ] **Step 1: Read the code this task touches**

Read the builder-blocked park in `build.rs` around the `fix_source` match, the tests `builder_stop_parts_carry_the_stop_reason_and_the_last_message` in `park.rs`, and `a_risky_park_after_a_sync_runs_the_final_checks` in `park_evidence.rs`.

- [ ] **Step 2: Write the failing and the tightened tests**

- `a_builder_stop_with_an_unreadable_diff_gets_a_risk_unknown_part` in `park.rs` tests: the part list of a builder stop for a fix round whose diff cannot be read contains a `Risk` part whose text starts with `unknown: the job diff could not be read`. Fails before the `build.rs` change.
- `final_checks_that_cannot_run_after_a_sync_park_risky_and_say_why` in `park_evidence.rs`: a risky sync fixture whose final checks cannot run (a check rule whose command is missing from the job worktree). It asserts kind `RiskyDiff`, a `Checks` part whose text starts with `final checks could not run:`, and that the board stage is `NeedsHuman`.
- Tighten `a_risky_park_after_a_sync_keeps_the_review_and_risk_evidence` to assert the exact titles `["Review", "Risk", "Rounds", "Changed files"]` with `assert_eq!`, the board stage `NeedsHuman` and the reason `after sync: ` prefix.
- Tighten `a_risky_park_after_a_sync_runs_the_final_checks` to assert the `Checks` part text contains `true`, instead of only the title.

- [ ] **Step 3: Run the tests to verify the first one fails**

Run: `cargo test -j4 -p factory-engine -- a_builder_stop_with_an_unreadable_diff`.
Expected: FAIL.

- [ ] **Step 4: Implement the `build.rs` change**

Approach: replace the one expression, keep the surrounding branch as it is.

- [ ] **Step 5: Run the tests, fmt and clippy**

Run: `cargo test -j4 -p factory-engine`, then the fmt and clippy commands.
Expected: all pass, no warnings.

Next plan: none
