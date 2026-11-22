# Engine small fixes, plan 1: parks and sync reviews

> **Spec:** docs/superpowers/specs/2026-11-22-engine-small-fixes-design.md

> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** A sync review never overwrites an older one (#22), a park with no `park.json` offers only commands that work (#139), and an Engine park saves its fields and its stage move in one board save (#51).

**Commit:** fix(engine): keep parks and sync reviews correct

**Architecture:** Three small, independent changes. A new pure helper picks the sync review number. The factory-core park helpers take the review state and use `ParkKind::next_for`. `in_park_order` loses its separate stage step, and `park_with_target` moves the stage inside its park-field update.

**Tech Stack:** Rust, `tempfile` in tests.

## Global Constraints

- cargo commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`, never --workspace or --all.
- No new board, ledger or events field.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- No git shell-out; no `/proc` reads.

## File Structure

| File | Change |
|---|---|
| `crates/factory-engine/src/review.rs` | Add `next_sync_number`; the `ReviewRun::Sync` arm (lines 132-139) calls it; unit tests in the existing `mod tests` (line 587). |
| `crates/factory-core/src/park.rs` | `park_commands` and `no_resume_refusal` (lines 305-324) take `review_missing` and `verdict`; tests at lines 361-410 updated, one added. |
| `crates/factory-engine/src/retry.rs` | Line 35 passes the review state. |
| `crates/factory-supervisor/src/admit.rs` | Line 194 passes the review state. |
| `crates/factory-engine/tests/retry.rs` | One new test. |
| `crates/factory-engine/src/park.rs` | `in_park_order` (lines 90-105) has four steps; `park_with_target` (lines 238-260) moves the stage in its update; its tests (lines 480-545) rewritten, one added. |

## Review Focus

1. A sync file whose number does not fit `u32` (`review-code-r1-sync99999999999.md`): a user expects it ignored, not a panic. Pinned by `next_sync_number_ignores_other_rounds_and_bad_names`.
2. A `park.json` whose `next` list is non-empty while the review is missing: the user expects the recorded list unchanged. Pinned by the existing `park_commands_prefer_the_park_file`, updated to pass `true, None`.
3. factoryd (`admit.rs`) and the engine (`retry.rs`) giving different refusal text for the same job: a user expects the same commands. Pinned by `retry_refusal_without_park_json_offers_without_review` on the engine side, and `cargo test -j4 -p factory-supervisor` keeps compiling and passing on the other.
4. A cancelled job parked through the Engine: `set_stage` allows only needs-human for a cancelled job, so the park must still succeed. Pinned by the existing park tests in `cargo test -j4 -p factory-engine park`.
5. A stage move refused after the park fields were set in memory: the user expects no park field on disk. Pinned by `a_refused_stage_move_saves_no_park_fields`.

---

### Task 1: Sync review number takes the highest plus one

**Files:**
- Modify: `crates/factory-engine/src/review.rs:132-139`
- Test: `crates/factory-engine/src/review.rs` (`mod tests`, line 587)

**Interfaces:**
- Consumes: `crate::ports::io_err`.
- Produces: `pub(crate) fn next_sync_number(job_dir: &std::path::Path, round: u32) -> Result<u32, EngineError>`.
- Port: none. This is a job-folder read in the review stage, called directly like the code it replaces.

- [ ] **Step 1: Read the code and the reference.** Read `docs/reference/software-factory.md` (the sync review file names) and `crates/factory-engine/src/review.rs:100-145`.

- [ ] **Step 2: Write the failing tests** in `mod tests` of `review.rs`:

```rust
fn touch(dir: &std::path::Path, name: &str) {
    std::fs::write(dir.join(name), "x").expect("write");
}

#[test]
fn next_sync_number_takes_the_highest_plus_one() {
    let dir = tempfile::tempdir().expect("tempdir");
    touch(dir.path(), "review-code-r1-sync1.md");
    touch(dir.path(), "review-code-r1-sync3.md");
    assert_eq!(next_sync_number(dir.path(), 1).expect("number"), 4);
}

#[test]
fn next_sync_number_is_one_in_an_empty_folder() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert_eq!(next_sync_number(dir.path(), 2).expect("number"), 1);
}

#[test]
fn next_sync_number_ignores_other_rounds_and_bad_names() {
    let dir = tempfile::tempdir().expect("tempdir");
    for name in [
        "review-code-r10-sync5.md",
        "review-code-r1-syncx.md",
        "review-code-r1-sync.md",
        "review-code-r1-sync2.md.tmp",
        "review-code-r1-sync99999999999.md",
        "review-code-r1.md",
    ] {
        touch(dir.path(), name);
    }
    touch(dir.path(), "review-code-r1-sync2.md");
    assert_eq!(next_sync_number(dir.path(), 1).expect("number"), 3);
}
```

- [ ] **Step 3: Run them.** `cargo test -j4 -p factory-engine next_sync_number`. Expected: compile error `cannot find function 'next_sync_number' in this scope`.

- [ ] **Step 4: Implement** `next_sync_number` in `review.rs`. A name counts only when it is exactly `review-code-r{round}-sync{K}.md` with `K` non-empty ASCII digits that parse as `u32`. It returns the largest `K` plus 1, or 1 when none match. A `read_dir` error maps through `io_err`. Replace the counting in the `ReviewRun::Sync` arm with `let k = next_sync_number(&self.job_dir, job.round)?;` and `self.job_dir.join(format!("{prefix}{k}.md"))`.

- [ ] **Step 5: Run them again.** `cargo test -j4 -p factory-engine next_sync_number`. Expected: 3 passed. Then `cargo test -j4 -p factory-engine --test sync_review`. Expected: all pass.

### Task 2: Review-aware fallback commands

**Files:**
- Modify: `crates/factory-core/src/park.rs:305-324`, its tests at lines 361-410
- Modify: `crates/factory-engine/src/retry.rs:35`
- Modify: `crates/factory-supervisor/src/admit.rs:194`
- Test: `crates/factory-engine/tests/retry.rs`

**Interfaces:**
- Consumes: `ParkKind::next_for(self, id: &str, review_missing: bool, verdict: Option<&str>) -> Vec<String>` (`park.rs:160`), `Job::review_missing(&self) -> bool`, `Job.last_review: Option<LastReview>`.
- Produces: `pub fn park_commands(dir: &Path, id: &str, park_kind: Option<&str>, review_missing: bool, verdict: Option<&str>) -> Vec<String>` and `pub fn no_resume_refusal(dir: &Path, id: &str, park_kind: Option<&str>, review_missing: bool, verdict: Option<&str>) -> String`.
- Port: none. `retry.rs` is engine logic over the board, with no trait.

- [ ] **Step 1: Read the code.** Read `crates/factory-core/src/park.rs:150-330`, `crates/factory-engine/src/retry.rs:1-45` and `crates/factory-supervisor/src/admit.rs:170-200`.

- [ ] **Step 2: Write the failing tests.** In `factory-core/src/park.rs` `mod tests`, add:

```rust
#[test]
fn fallback_commands_follow_the_review_state() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert_eq!(
        park_commands(dir.path(), "001", Some("other"), true, None),
        ParkKind::Other.next_for("001", true, None)
    );
    assert_eq!(
        park_commands(dir.path(), "001", Some("other"), true, None),
        vec![
            "factory rework 001 <note>".to_string(),
            "factory cancel 001".to_string(),
            "factory approve 001 --without-review (the review has not run)".to_string(),
        ]
    );
}
```

Update the existing calls in `no_resume_refusal_joins_the_commands`, `park_commands_prefer_the_park_file`, `park_commands_fall_back_to_the_kind` and `park_commands_read_an_unknown_kind_as_other` to pass `false, None`. Their expected values stay the same, because `next_for` with `false, None` equals `next`. In `park_commands_prefer_the_park_file`, pass `true, None` so a recorded list is shown to win over the review state.

In `crates/factory-engine/tests/retry.rs`, add:

```rust
#[test]
fn retry_refusal_without_park_json_offers_without_review() {
    let fx = common::approved_job();
    park(&fx, factory_core::ParkKind::Other, None);
    remove_park_file(&fx);
    fx.driver
        .store()
        .update_job(&fx.id, |j| {
            j.set_review("code", factory_core::ReviewState::Pending);
            Ok(())
        })
        .expect("review pending");
    let text = refusal(&fx);
    assert!(text.contains("factory approve 001 --without-review"), "{text}");
    assert!(!text.contains("factory approve 001,"), "{text}");
}
```

- [ ] **Step 3: Run them.** `cargo test -j4 -p factory-core park_commands`. Expected: compile error `this function takes 3 arguments but 5 arguments were supplied`.

- [ ] **Step 4: Implement.** Change both signatures as in Interfaces. The fallback in `park_commands` becomes `.unwrap_or(ParkKind::Other).next_for(id, review_missing, verdict)`, and `no_resume_refusal` passes its new arguments on. In `retry.rs:35` and `admit.rs:194`, pass `job.review_missing()` and `job.last_review.as_ref().map(|r| r.verdict.as_str())`.

- [ ] **Step 5: Run them again.** `cargo test -j4 -p factory-core park`, `cargo test -j4 -p factory-engine --test retry` and `cargo test -j4 -p factory-supervisor`. Expected: all pass.

### Task 3: One board save for an Engine park

**Files:**
- Modify: `crates/factory-engine/src/park.rs:90-105` (`in_park_order`), `park.rs:238-260` (`park_with_target`), tests at `park.rs:480-545`

**Interfaces:**
- Consumes: `crate::ports::set_stage_logged(job: &mut Job, to: Stage, reason: Option<&str>, force: bool, now: f64) -> Result<(), CoreError>`, `crate::ports::log_park(id: &str, reason: &str)`, `Engine::update`.
- Produces: `pub(crate) fn in_park_order(clear: impl FnOnce(), update: impl FnOnce() -> Result<(), EngineError>, record: impl FnOnce(), ruling: impl FnOnce() -> Result<(), EngineError>) -> Result<(), EngineError>`.
- Port: none. The board goes through `BoardStore` (`store.rs`), which saves nothing when the closure returns `Err` (`store.rs:61-76`).

- [ ] **Step 1: Read the code.** Read `crates/factory-engine/src/park.rs:85-260`, `crates/factory-engine/src/ports.rs:160-235`, `crates/factory-engine/src/store.rs:61-86` and `crates/factory-engine/src/merge.rs:85-103`.

- [ ] **Step 2: Write the failing tests.** In `park.rs` `mod tests`, rewrite the order tests for four steps. Rename `park_steps_run_clear_then_update_then_stage_then_record_then_ruling` to `park_steps_run_clear_then_update_then_record_then_ruling`, with the expected order `vec!["clear", "update", "record", "ruling"]`. Remove `a_failed_stage_move_leaves_no_park_record`. In `a_failed_board_update_leaves_no_park_record` and `a_failed_ruling_leaves_the_park_record`, drop the stage closure; the latter expects `vec!["update", "record"]`. Then add an Engine-level test. `Engine::park_with` is `pub(crate)`, so the test lives in this module. Add a fixture `with_board_job(stage: Stage, body: impl FnOnce(&Engine, &Path))`, built like `with_unreadable_board` (`park.rs`, about line 645), that also saves a board holding one job `001` (from `Job::new("001", "job", "s", "2026-11-22", "f", 1.0)`, forced to `stage` with `set_stage(stage, None, true, 1.0)`) through `BoardStore::new(&board_path, Duration::from_secs(1)).update(...)`. `body` gets the board path.

```rust
#[test]
fn a_refused_stage_move_saves_no_park_fields() {
    // A merged job cannot move to needs-human without force (`Stage::check_move`), so the one update fails as a whole.
    with_board_job(Stage::Merged, |engine, board_path| {
        engine
            .park_with(ParkKind::Other, "test park".to_string(), Vec::new())
            .expect_err("the move is refused");
        let board = BoardStore::new(board_path, Duration::from_secs(1)).load().expect("board");
        let job = board.find("001").expect("job");
        assert_eq!(job.stage, Stage::Merged);
        assert_eq!(job.park_kind, None);
        assert_eq!(job.park_summary, None);
        assert_eq!(job.park_at, None);
    });
}
```

- [ ] **Step 3: Run them.** `cargo test -j4 -p factory-engine park_steps_run a_refused_stage_move a_failed_board_update a_failed_ruling`. Expected: compile error `this function takes 5 arguments but 4 arguments were supplied`.

- [ ] **Step 4: Implement.** Drop the `stage` parameter of `in_park_order`, so the order is clear, update, record, ruling. In `park_with_target`, the update closure sets the four park fields and then calls `set_stage_logged(j, Stage::NeedsHuman, Some(&reason), false, now())`, inside the same `self.update` call. Call `log_park(&self.job_id, &reason)` right after that update returns `Ok`, inside the update step closure before it returns. Keep `move_to_needs_human` for its other callers, and leave it unused here.

- [ ] **Step 5: Run them again.** `cargo test -j4 -p factory-engine park`. Expected: all pass. Then `cargo test -j4 -p factory-engine --test park_evidence --test retry --test runloop`. Expected: all pass.

Next plan: docs/superpowers/plans/2026-11-22-engine-small-fixes/plan-02-engine-small-fixes.md
