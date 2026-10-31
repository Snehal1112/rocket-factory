# Park evidence edge cases Implementation Plan

> **Spec:** docs/superpowers/specs/2026-10-31-park-evidence-edges-design.md
>
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Make the park card complete and truthful after a sync park, an unreadable diff or ledger, and a crash between the two park writes.

**Architecture:** All changes are in `factory-engine`. Small pure helpers carry the logic so each case has a deterministic unit test: `in_park_order` (write order), `lead_parts_from` and `risk_unknown_part` (the Risk part), `sync_park_choice` (kind and reason after the final checks). `Engine::park_after_sync` and `Driver::park_after_sync` use them, and `sync_stage` calls the driver method for a risky decision.

**Tech Stack:** Rust, `cargo test -j4 -p factory-engine`, the merge fixtures in `crates/factory-engine/tests/common/mod.rs`.

## Global Constraints

- Always `-j4` and `-p <crate>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Each task ends with `cargo fmt -p factory-engine -- --check` and `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`. No `#[allow(...)]` to silence a lint.
- No unwrap in production Rust. Tests use `.expect("...")` and `expect_err`.
- No new board, ledger or events field, and no new `park.json` field. Port: none. This logic uses the `Engine` park helpers and the concrete `factory_checks` calls, which CLAUDE.md says are called directly.
- New public items carry a doc comment. Code comments are short full sentences that end with a period.
- Texts are exact: `unknown: the job diff could not be read ({error})`, `final checks failed after sync`, `final checks could not run: {error}`, `run loop did not settle, see the ledger`.

## Review Focus

1. Final checks that cannot run after a sync must still park, with the risky kind and a Checks part that says why. Pinned by `final_checks_that_cannot_run_keep_the_risky_kind_and_say_why`.
2. A failing final check on the sync path must start no fix round and no builder run. Pinned by `failing_final_checks_after_a_sync_park_as_checks_failed_without_a_fix_round`.
3. A ledger that holds only blank lines must leave out the Ledger part like a missing one. Pinned by `a_blank_ledger_leaves_out_the_ledger_part`.
4. A stage move that fails after a successful board update must leave no `park.json`, and a ledger ruling that fails after the record is written must keep it. Pinned by `a_failed_stage_move_leaves_no_park_record` and `a_failed_ruling_leaves_the_park_record`.
5. A sync park that is not risky (a conflict) must keep kind `other`. Pinned by `other_sync_parks_stay_kind_other`.

## File Structure

| File | Responsibility |
|---|---|
| `crates/factory-engine/src/ports.rs` | `Engine::park` split into `move_to_needs_human` and the ruling |
| `crates/factory-engine/src/park.rs` | `in_park_order`, `risk_unknown_part`, `lead_parts_from`, `Engine::lead_parts_for_card`, `sync_park_choice`, `Engine::park_after_sync`, the new `park_with` order, unit tests |
| `crates/factory-engine/src/compat.rs` | `Driver::park_after_sync` |
| `crates/factory-engine/src/sync.rs` | the risky arm calls `park_after_sync` |
| `crates/factory-engine/src/runloop.rs` | `park_unsettled` leaves out an empty Ledger part, tests |
| `crates/factory-engine/tests/park_evidence.rs` | the sync park integration tests |
| `docs/reference/software-factory.md` | the park evidence paragraph |

### Task 1: The write order, the unknown Risk part and the blank ledger

**Files:**
- Modify: `crates/factory-engine/src/park.rs`, `crates/factory-engine/src/ports.rs`, `crates/factory-engine/src/runloop.rs`
- Test: `crates/factory-engine/src/park.rs` (tests module), `crates/factory-engine/src/runloop.rs` (tests module)

**Interfaces:**
- Produces, in `park.rs`:
  - `fn in_park_order(update: impl FnOnce() -> Result<(), EngineError>, stage: impl FnOnce() -> Result<(), EngineError>, record: impl FnOnce(), ruling: impl FnOnce() -> Result<(), EngineError>) -> Result<(), EngineError>`. It runs `update`, `stage`, `record` and `ruling` in that order and stops at the first error without calling the later steps. `record` is infallible, so a failed `ruling` still leaves the record written.
- Produces, in `ports.rs`: `pub(crate) fn move_to_needs_human(&self, reason: &str) -> Result<(), EngineError>` (the `set_stage` and `log_park` calls of `Engine::park`). `Engine::park` calls it, then writes the ruling, so its behaviour is unchanged.
  - `fn risk_unknown_part(err: &EngineError) -> ParkPart`.
  - `fn lead_parts_from(paths: Result<Vec<String>, EngineError>, review: Option<ParkPart>, risk: impl FnOnce(&[String]) -> Option<ParkPart>) -> Vec<ParkPart>`.
  - `pub(crate) fn lead_parts_for_card(&self) -> Vec<ParkPart>` on `Engine`.

- [ ] **Step 1: Read the code this task touches**

Read all of `park.rs` (the `park_with` body at lines 77-108, `decision_parts` at 111-154, `lead_parts` at 163-167), `park_unsettled` and its test `unsettled_park_keeps_the_last_twenty_ledger_lines` in `runloop.rs` (`driver` and `add_job` helpers), `Engine::park` in `ports.rs:177-182` (its `set_stage` and `log_park` calls move to `move_to_needs_human`) and `evidence_or_reason` in `park.rs:58-66`.

- [ ] **Step 2: Write the failing tests**

In the `park.rs` tests module:

```rust
#[test]
fn park_steps_run_update_then_stage_then_record_then_ruling() {
    let order = std::cell::RefCell::new(Vec::new());
    in_park_order(
        || {
            order.borrow_mut().push("update");
            Ok(())
        },
        || {
            order.borrow_mut().push("stage");
            Ok(())
        },
        || order.borrow_mut().push("record"),
        || {
            order.borrow_mut().push("ruling");
            Ok(())
        },
    )
    .expect("parked");
    assert_eq!(*order.borrow(), vec!["update", "stage", "record", "ruling"]);
}

#[test]
fn a_failed_board_update_leaves_no_park_record() {
    let order = std::cell::RefCell::new(Vec::new());
    let err = in_park_order(
        || Err(EngineError::Msg("board locked".into())),
        || {
            order.borrow_mut().push("stage");
            Ok(())
        },
        || order.borrow_mut().push("record"),
        || Ok(()),
    )
    .expect_err("update failed");
    assert_eq!(err.to_string(), "board locked");
    assert!(order.borrow().is_empty());
}

#[test]
fn an_unreadable_diff_gets_a_risk_unknown_part() {
    let review = Some(ParkPart::new("Review", "review-code-r1.md", "verdict approve, round 1"));
    let err = EngineError::Msg("job has no head".into());
    let parts = lead_parts_from(Err(err), review, |_| None);
    let titles: Vec<&str> = parts.iter().map(|p| p.title.as_str()).collect();
    assert_eq!(titles, vec!["Review", "Risk"]);
    assert_eq!(parts[1].source, "driver");
    assert_eq!(
        parts[1].text,
        "unknown: the job diff could not be read (job has no head)"
    );
}
```

| Test | Setup | Assertions |
|---|---|---|
| `a_failed_stage_move_leaves_no_park_record` | `update` returns `Ok(())`, `stage` returns `Err(EngineError::Msg("stage failed".into()))`, `record` and `ruling` push their names | the call fails with `stage failed`; the recorded order is `["update"]`, so neither `record` nor `ruling` ran |
| `a_failed_ruling_leaves_the_park_record` | `update` and `stage` return `Ok(())`, `record` pushes `"record"`, `ruling` returns `Err(EngineError::Msg("ledger locked".into()))` | the call fails with `ledger locked`; the recorded order is `["record"]` plus the earlier steps, so the record was written before the ruling failed |
| `a_readable_diff_keeps_the_lead_parts` | `lead_parts_from(Ok(vec!["a.rs".to_string()]), Some(review), |paths| { assert_eq!(paths, ["a.rs"]); Some(ParkPart::new("Risk", "driver", "risky: a.rs")) })` | titles are `["Review", "Risk"]` and the Risk text is `risky: a.rs` |

In the `runloop.rs` tests module, built like `unsettled_park_keeps_the_last_twenty_ledger_lines`:

```rust
#[test]
fn a_blank_ledger_leaves_out_the_ledger_part() {
    let tmp = tempfile::tempdir().expect("tmp");
    let d = driver(&tmp);
    let job = add_job(&d, "demo", "f", &[], None, Stage::Build, None).expect("job");
    std::fs::write(d.job_dir(&job).join("progress.md"), "\n  \n\n").expect("ledger");
    park_unsettled(&d, &job.id).expect("park");
    let job = d.job(&job.id).expect("job");
    let rec = factory_core::ParkRecord::read(&d.job_dir(&job)).expect("park.json");
    assert_eq!(rec.parts.len(), 1);
    assert_eq!(rec.parts[0].title, "Why it stopped");
    assert_eq!(rec.parts[0].text, "run loop did not settle, see the ledger");
}
```

Add `an_unreadable_ledger_leaves_out_the_ledger_part` the same way, but remove the ledger file and create a directory at its path with `std::fs::create_dir_all`. The ruling write at the end of the park fails in that case, and the record is already written, so call `let _ = park_unsettled(&d, &job.id);` with a one-line comment saying so, then assert the same two lines on the record.

- [ ] **Step 3: Run the tests to see them fail**

Run: `cargo test -j4 -p factory-engine park_steps` and `cargo test -j4 -p factory-engine ledger_part`
Expected: compile errors `cannot find function 'in_park_order' in this scope`, and the two ledger tests fail because the record still holds a `Ledger` part.

- [ ] **Step 4: Implement**

Add the helpers above in `park.rs`. `risk_unknown_part` returns `ParkPart::new("Risk", "driver", &format!("unknown: the job diff could not be read ({err})"))`. `lead_parts_from` returns the review part (when any) followed by the risk part from the closure on `Ok`, or by `risk_unknown_part` on `Err`. `lead_parts_for_card` calls it with `self.job_changed_paths()`, `self.review_part()` and a closure over `self.risk_part`. `decision_parts` uses `lead_parts_for_card` in place of `self.lead_parts(&self.job_changed_paths().unwrap_or_default())`. In `ports.rs`, move the `set_stage` and `log_park` calls of `Engine::park` into `move_to_needs_human`; `park` calls it and then `self.ruling(...)?`, so every other caller behaves as before. `park_with` builds the record as today, then calls `in_park_order` with the board update, `self.move_to_needs_human(&reason)`, `write_record(&self.job_dir, &record)` and `self.ruling(&format!("needs-human, {reason}"))`, and returns `Ok(StageOutcome::NeedsHuman(reason))` on success. `park_unsettled` passes an empty list of parts when the tail is blank, so `evidence_or_reason` adds `Why it stopped`.

- [ ] **Step 5: Run the tests to see them pass**

Run: `cargo test -j4 -p factory-engine`
Expected: all pass, the seven new tests included, and the existing park tests unchanged.

- [ ] **Step 6: Format and lint**

Run `cargo fmt -p factory-engine -- --check` and `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`. Fix any finding, then run the same command once to confirm.

### Task 2: The risky park after a sync

**Files:**
- Modify: `crates/factory-engine/src/park.rs`, `crates/factory-engine/src/compat.rs`, `crates/factory-engine/src/sync.rs`, `docs/reference/software-factory.md`
- Test: `crates/factory-engine/src/park.rs` (tests module), `crates/factory-engine/tests/park_evidence.rs`

**Interfaces:**
- Consumes: `lead_parts_for_card`, `rounds_part`, `stat_part`, `part_from_file` and `park_with` from `park.rs`, and `Engine::final_checks` from `review.rs`.
- Produces: `fn sync_park_choice(checks: &Result<ChecksReport, EngineError>, reason: &str) -> (ParkKind, String, Option<String>)` (the third value is the note for a Checks part that could not be read from `checks.md`); `pub(crate) fn park_after_sync(&self, kind: ParkKind, reason: &str) -> Result<StageOutcome, EngineError>` on `Engine`; `pub fn park_after_sync(&self, id: &str, kind: ParkKind, reason: &str) -> Result<StageOutcome, EngineError>` on `Driver`, next to `apply`.

- [ ] **Step 1: Read the code this task touches**

Read `sync_stage` (`sync.rs:15-87`), `apply_decision` and `final_gate` (`review.rs:226-291`), `park_gate` (`checks_stage.rs:121-137`), `ChecksReport` and `CheckResult` (`factory-checks/src/run.rs:17-45`), `merge_stage` and `park_with_parts` (`merge.rs:36-78,99-150`), the tests `main_moved_triggers_a_sync_then_merges` and `a_sync_conflict_parks` in `tests/merge.rs`, and `merge_record` plus `an_earlier_park_keeps_its_evidence` in `tests/park_evidence.rs`.

- [ ] **Step 2: Write the failing tests**

In the `park.rs` tests module:

```rust
fn report(passed: bool) -> factory_checks::ChecksReport {
    factory_checks::ChecksReport {
        results: vec![factory_checks::CheckResult {
            command: "cargo check".into(),
            passed,
            summary: if passed { "ok".into() } else { "exit 1".into() },
            tail: String::new(),
        }],
    }
}

#[test]
fn passing_final_checks_keep_the_risky_kind() {
    let (kind, reason, note) = sync_park_choice(&Ok(report(true)), "after sync: risky diff: a.rs");
    assert_eq!(kind, ParkKind::RiskyDiff);
    assert_eq!(reason, "after sync: risky diff: a.rs");
    assert_eq!(note, None);
}

#[test]
fn failing_final_checks_park_as_checks_failed() {
    let (kind, reason, note) = sync_park_choice(&Ok(report(false)), "after sync: risky diff: a.rs");
    assert_eq!(kind, ParkKind::ChecksFailed);
    assert_eq!(reason, "final checks failed after sync");
    assert_eq!(note, None);
}

#[test]
fn final_checks_that_cannot_run_keep_the_risky_kind_and_say_why() {
    let err: Result<factory_checks::ChecksReport, EngineError> =
        Err(EngineError::Check("no worktree".into()));
    let (kind, reason, note) = sync_park_choice(&err, "after sync: risky diff: a.rs");
    assert_eq!(kind, ParkKind::RiskyDiff);
    assert_eq!(reason, "after sync: risky diff: a.rs");
    assert_eq!(
        note.as_deref(),
        Some("final checks could not run: checks: no worktree")
    );
}
```

In `tests/park_evidence.rs`, with a helper that builds the fixture and moves main:

```rust
fn risky_sync_fixture(checks: Vec<factory_core::CheckRule>) -> common::MergeFixture {
    let config = factory_core::Config {
        risk_paths: vec!["docs/**".to_string()],
        checks,
        ..factory_core::Config::default()
    };
    let fx = common::approved_job_config(common::approving_runner(3), false, config);
    // Main moves, so the merge syncs first.
    common::commit_file(&fx.main, "other.md", "o");
    fx
}

#[test]
fn a_risky_park_after_a_sync_keeps_the_review_and_risk_evidence() {
    let fx = risky_sync_fixture(Vec::new());
    let r = merge_stage(&fx.driver, &fx.id).expect("stage");
    assert!(matches!(&r, MergeResult::Parked(m) if m.contains("after sync")), "{r:?}");
    let rec = merge_record(&fx);
    assert_eq!(rec.kind, ParkKind::RiskyDiff);
    assert_eq!(rec.risky, Some(true));
    let titles: Vec<&str> = rec.parts.iter().map(|p| p.title.as_str()).collect();
    assert!(titles.starts_with(&["Review", "Risk"]), "{titles:?}");
}
```

| Test | Setup | Assertions |
|---|---|---|
| `a_risky_park_after_a_sync_runs_the_final_checks` | `risky_sync_fixture` with a rule `paths: ["docs/**"]`, `commands: ["true"]` | `merge_stage` parks; the job's `checks.md` has a line starting `PASS true`; the record has a part titled `Checks` |
| `failing_final_checks_after_a_sync_park_as_checks_failed_without_a_fix_round` | rule `paths: ["docs/**"]`, `commands: ["false"]`. The sync diff only touches `other.md`, so only the final checks (the whole job diff) hit the rule | `merge_stage` parks; the record kind is `ChecksFailed` and its reason contains `final checks failed after sync`; `fx.runner.specs_for("builder")` is empty; the record has Review, Risk and Checks parts |
| `other_sync_parks_stay_kind_other` | build as `a_sync_conflict_parks` does, with the risky config of `risky_sync_fixture` and no moved main | `merge_stage` parks with a message containing `conflict`; `merge_record(&fx).kind == ParkKind::Other` |

- [ ] **Step 3: Run the tests to see them fail**

Run: `cargo test -j4 -p factory-engine sync_park_choice` and `cargo test -j4 -p factory-engine --test park_evidence sync`
Expected: compile errors `cannot find function 'sync_park_choice' in this scope`; the integration tests fail because the record kind is `Other` and holds one part.

- [ ] **Step 4: Implement**

`sync_park_choice` returns `(ParkKind::RiskyDiff, reason, None)` for a passing report, `(ParkKind::ChecksFailed, "final checks failed after sync".to_string(), None)` for a failing one, and `(ParkKind::RiskyDiff, reason, Some(format!("final checks could not run: {err}")))` for an error. `Engine::park_after_sync` runs `self.final_checks()`, calls `sync_park_choice`, builds the parts as `lead_parts_for_card()`, then `rounds_part()`, then `stat_part()`, then the Checks part (from `checks.md` with `part_from_file`, or a `ParkPart::new("Checks", "driver", note)` when the note is `Some`), and returns `self.park_with(kind, reason, parts)`. `Driver::park_after_sync` is `self.with_engine(id, |e| e.park_after_sync(kind, reason))`. In `sync_stage`, the `Decision::NeedsHuman { kind, reason: why }` arm builds the text `after sync: {why}`; for `ParkKind::RiskyDiff` it calls `driver.park_after_sync(id, *kind, &text)?` first, and in every case it still returns `SyncOutcome::Parked(text)`. Add one sentence to the park evidence paragraph of `docs/reference/software-factory.md` (near line 211): after a sync, a risky diff that needs a human runs the final checks again and parks with the same parts, a failing final check parks as `checks-failed` without a fix round, and an unreadable diff gives a Risk part that starts with `unknown:`.

- [ ] **Step 5: Run the tests to see them pass**

Run: `cargo test -j4 -p factory-engine`
Expected: all pass, the seven new tests included, and `a_sync_conflict_parks` and `an_earlier_park_keeps_its_evidence` unchanged.

- [ ] **Step 6: Format and lint**

Run `cargo fmt -p factory-engine -- --check` and `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`. Fix any finding, then run the same command once to confirm.

Next plan: none
