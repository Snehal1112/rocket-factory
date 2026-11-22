# Engine small fixes, plan 3: ticket numbers and round minors

> **Spec:** docs/superpowers/specs/2026-11-22-engine-small-fixes-design.md

> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** A minor or waiver ticket whose record write fails still has its issue number in the report and the ledger (#18), and the minor ticket after a sync lists the minors of the whole merged round (#20).

**Commit:** fix(tickets): record numbers and all round minors

**Architecture:** Both ticket steps stop returning early when the record write fails after `tracker.create`. They push a `failed:` line that names the number, then go on to the ledger ruling. The minor step reads the review files of round `N` (round, rebase, syncs by number, then `last_review.file`) and dedupes their open minors.

**Tech Stack:** Rust, the `Tracker` port with `FakeTracker` in tests.

## Global Constraints

- cargo commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`, never --workspace or --all.
- The ticket steps never change the merge result.
- No new board, ledger or events field.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.

## File Structure

| File | Change |
|---|---|
| `crates/factory-engine/src/minors.rs` | The record-write error is reported (lines 68-72); new `round_review_files` and the collection in `minor_tickets_step` (lines 42-66). |
| `crates/factory-engine/src/waivers.rs` | The record-write error is reported, and the loop stops (lines 164-170). |
| `crates/factory-engine/tests/minors.rs` | Three new tests. |
| `crates/factory-engine/tests/waivers.rs` | One new test. |

## Review Focus

1. A minor in the round review that a sync review repeats with exactly the same text: a user expects one checklist line. Pinned by `minors_after_a_sync_include_the_round_review`.
2. A `last_review.file` outside the round's names (an older layout): a user expects its minors still filed. Pinned by `minors_read_a_last_review_file_outside_the_round_names`.
3. A waiver with two blockers whose first record write fails: a user expects one ticket filed and the second not filed in that run. Pinned by `an_unrecordable_waiver_ticket_reports_its_number_and_stops`.
4. The merge itself while the minor record write fails: a user expects the job merged. Pinned by `an_unrecordable_minor_ticket_reports_its_number` (it merges through `merge_stage`).
5. A round whose review files all lack open minors: a user expects the existing skip line. Pinned by the existing `no_open_minors_no_ticket`.

---

### Task 1: Report a ticket whose record write failed

**Files:**
- Modify: `crates/factory-engine/src/minors.rs:68-72`, `crates/factory-engine/src/waivers.rs:164-170`
- Test: `crates/factory-engine/tests/minors.rs`, `crates/factory-engine/tests/waivers.rs`

**Interfaces:**
- Consumes: `Tracker::create` through `driver.tracker_for(&job)`; `TicketsReport { skipped, failed, lines }` (`crates/factory-engine/src/tickets.rs`).
- Produces: the report line `failed: created #{number} but could not record it in minors.json: {e}`, and the same with `waivers.json`.
- Port: `Tracker` (`crates/factory-engine/src/tracker.rs`), faked by `common::FakeTracker`.

- [ ] **Step 1: Read the code.** Read `crates/factory-engine/src/minors.rs`, `crates/factory-engine/src/waivers.rs:110-190`, `crates/factory-engine/tests/minors.rs:1-60`, `crates/factory-engine/tests/waivers.rs:1-60` and `crates/factory-core/src/fsutil.rs:6-26`. The temp file of `write_atomic` is `.{name}.{pid}.tmp`, and a folder at that path makes the write fail.

- [ ] **Step 2: Write the failing tests.** In `tests/minors.rs`:

```rust
/// Makes the next `write_atomic` of `name` in the job folder fail, by putting a folder at its temp path.
fn block_write(fx: &MergeFixture, name: &str) {
    let job = fx.driver.job(&fx.id).expect("job");
    let tmp = fx.driver.job_dir(&job).join(format!(".{name}.{}.tmp", std::process::id()));
    std::fs::create_dir(tmp).expect("block");
}

#[test]
fn an_unrecordable_minor_ticket_reports_its_number() {
    let t = FakeTracker::new("o/r");
    let fx = with_tracker(approved_job(), t.clone());
    with_review(&fx, REVIEW);
    block_write(&fx, MINORS_FILE);
    merge(&fx);
    assert_eq!(t.count("create Minor review findings from job"), 1);
    assert!(!minors_file(&fx).exists());
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.stage, Stage::Merged);
    let ledger = std::fs::read_to_string(fx.driver.job_dir(&job).join("progress.md")).expect("ledger");
    assert!(ledger.contains("created #201 but could not record it in minors.json"), "{ledger}");
}
```

In `tests/waivers.rs`, add `an_unrecordable_waiver_ticket_reports_its_number_and_stops`. Copy `block_write` there and record `two_blockers()` the way `merged` does. Call `block_write(&fx, "waivers.json")` before `merge_stage`. Assert:
- `t.count("create Waived review finding")` is `1`.
- The ledger contains `created #201 but could not record it in waivers.json`.
- The job stage is `Merged`.

Use the first number `FakeTracker` hands out, which the existing tests show as `201`.

- [ ] **Step 3: Run them.** `cargo test -j4 -p factory-engine --test minors --test waivers unrecordable`. Expected: both fail. The ledger has no `could not record` line, because the `?` returned early.

- [ ] **Step 4: Implement.** In `minor_tickets_step`, on `Ok(number)`, match the `write_atomic` result. On `Err(e)`, set `report.failed = true` and push the minors line. On `Ok`, push `created #{number}: {title}` as today. In `waiver_tickets_step`, match `write_waivers(&dir, &list)`. On `Err(e)`, set `report.failed = true`, push the waivers line and `break 'all`. Both functions then fall through to their ledger ruling unchanged.

- [ ] **Step 5: Run them again.** `cargo test -j4 -p factory-engine --test minors --test waivers`. Expected: all pass.

### Task 2: Minors from the whole merged round

**Files:**
- Modify: `crates/factory-engine/src/minors.rs:42-66`
- Test: `crates/factory-engine/src/minors.rs` (new `mod tests`), `crates/factory-engine/tests/minors.rs`

**Interfaces:**
- Consumes: `factory_core::open_minors(text: &str) -> Vec<String>`, `LastReview { round, file, .. }`.
- Produces: `pub(crate) fn round_review_files(job_dir: &Path, round: u32, last_file: &str) -> Vec<String>`, the bare names that exist, in read order: `review-code-r{N}.md`, `review-code-r{N}-rebase.md`, each `review-code-r{N}-sync{K}.md` by `K` ascending (the same exact-name rule as plan 1's `next_sync_number`: `K` is ASCII digits that parse as `u32`), then `last_file` when it exists and is not yet listed.
- Port: `Tracker` for the create, as in the record-write task above. The file reads are direct.

- [ ] **Step 1: Read the code.** Read `crates/factory-engine/src/minors.rs`, `crates/factory-engine/src/review.rs` (`next_sync_number` and the name rule from plan 1) and `crates/factory-core/src/board.rs:236-257` (`LastReview`, `path_in`).

- [ ] **Step 2: Write the failing tests.** A unit test in a new `mod tests` of `minors.rs`:

```rust
#[test]
fn round_review_files_keep_the_read_order() {
    let dir = tempfile::tempdir().expect("tempdir");
    for name in [
        "review-code-r2-sync10.md", "review-code-r2-sync2.md", "review-code-r2-rebase.md",
        "review-code-r2.md", "review-code-r1.md", "review-code-r2-syncx.md",
    ] {
        std::fs::write(dir.path().join(name), "x").expect("write");
    }
    assert_eq!(
        round_review_files(dir.path(), 2, "review-code-r2-sync10.md"),
        vec!["review-code-r2.md", "review-code-r2-rebase.md", "review-code-r2-sync2.md", "review-code-r2-sync10.md"]
    );
}
```

In `tests/minors.rs`, add a helper `with_reviews(fx, files: &[(&str, &str)], last: &str)`. It writes each file, then sets `last_review` the way `with_review` does, with `round: 1`, `file: last` and `source: "sync"` when `last` contains `sync`. Then:

```rust
#[test]
fn minors_after_a_sync_include_the_round_review() {
    let t = FakeTracker::new("o/r");
    let fx = with_tracker(approved_job(), t.clone());
    with_reviews(&fx, &[
        ("review-code-r1.md", "- [minor] first nit\nVERDICT: APPROVE\n"),
        ("review-code-r1-sync1.md", "- [minor] sync nit\n- [minor] first nit\nVERDICT: APPROVE\n"),
    ], "review-code-r1-sync1.md");
    merge(&fx);
    let s = t.state.lock().expect("lock");
    let (_, body, _) = &s.created[0];
    let items: Vec<&str> = body.lines().filter(|l| l.starts_with("- [ ]")).collect();
    assert_eq!(items, vec!["- [ ] first nit", "- [ ] sync nit"]);
    assert!(body.contains("Review files review-code-r1.md, review-code-r1-sync1.md."), "{body}");
}
```

| Test | Files | Last | Assertion |
|---|---|---|---|
| `minors_include_the_rebase_review` | `r1.md` with `a nit`, `r1-rebase.md` with `rebase nit` | `review-code-r1-rebase.md` | items `a nit`, `rebase nit` in that order |
| `minors_read_a_last_review_file_outside_the_round_names` | `review-code-old.md` with `old nit` | `review-code-old.md` | items `old nit`; body names `review-code-old.md` |

Check the existing `a_merged_job_files_one_minor_ticket`: its `body.contains("review-code-r1.md")` still holds with the new `Review files ...` wording.

- [ ] **Step 3: Run them.** `cargo test -j4 -p factory-engine round_review_files` (expected: compile error `cannot find function 'round_review_files'`), then `cargo test -j4 -p factory-engine --test minors` (expected: `minors_after_a_sync_include_the_round_review` fails, missing `first nit`).

- [ ] **Step 4: Implement** `round_review_files` and use it in `minor_tickets_step`. Read each listed file (skip one that fails to read), run `open_minors` on each, and keep the first occurrence of each line. The body line becomes `Review files {names joined by ", "}. Job {id} merged at {sha}.` Only files that were read are named. An empty list of minors keeps the existing `skipped: the review has no open minor findings`.

- [ ] **Step 5: Run them again.** `cargo test -j4 -p factory-engine round_review_files` (expected: 1 passed), then `cargo test -j4 -p factory-engine --test minors --test waivers --test tickets` (expected: all pass).

Next plan: none
