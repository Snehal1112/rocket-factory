# Waiver written before the approve Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-22-ticket-intake-hardening-design.md
> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** `approve --waive` writes the waiver before it moves the job to Approve, and a retry with a new reason stores that reason.

**Commit:** fix(waivers): write the waiver before approve

**Architecture:** `record_waiver` in `crates/factory-engine/src/waivers.rs` updates the last entry on a retry. `approve` in `crates/factory-cli/src/commands.rs` records the waiver before its stage change. Both are file I/O in the job folder; no port changes.

**Tech Stack:** Rust.

## Global Constraints

- Cargo commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- `waivers.json` keeps its shape. A retry changes only the last entry's `reason` and `at`.

## Review Focus

1. A waiver retry with the same reason: a user expects one entry and no error. Already pinned by `a_retry_does_not_duplicate_the_waiver`, which must pass unchanged.
2. A successful `approve --waive`: a user expects the merge and one waiver, as today. Already pinned by `approve_with_waive_merges_and_records_the_waiver`, which must pass unchanged.
3. The `operator.approve` event: a user expects it only after the stage moved. Already pinned by `approve_with_waive_logs_the_reason` in `crates/factory-cli/tests/operator_events.rs`, which must pass unchanged.
4. A lease that cannot be taken: a user expects no waiver written. The waiver is recorded after `hold_project` and `renew_lease`, per Step 4.
5. A plain approve with no `--waive`: a user expects no change. Already pinned by the `new_to_merged` approve tests.

## File Structure

- Modify: `crates/factory-engine/src/waivers.rs` (`record_waiver`) and `crates/factory-engine/tests/waivers.rs`.
- Modify: `crates/factory-cli/src/commands.rs` (`approve`) and `crates/factory-cli/tests/new_to_merged.rs`.

---

### Task 1: the waiver is written first and a retry keeps the latest reason

**Files:**
- Modify: `crates/factory-engine/src/waivers.rs`
- Modify: `crates/factory-engine/tests/waivers.rs`
- Modify: `crates/factory-cli/src/commands.rs`
- Modify: `crates/factory-cli/tests/new_to_merged.rs`

**Interfaces:**
- Port: none. This is file I/O in the job folder.
- Produces: `pub fn record_waiver(job_dir: &Path, waiver: Waiver) -> Result<(), EngineError>`, same signature. A retry now writes the file.

**Tests added:** `a_retry_with_a_new_reason_updates_the_waiver`, `a_failed_waiver_write_leaves_the_job_in_needs_human`.

- [ ] **Step 1: Read the code**

Read `record_waiver`, `read_waivers` and `write_waivers` in `crates/factory-engine/src/waivers.rs`, and `approve` in `crates/factory-cli/src/commands.rs`, from the `approve_refusal` check through the ledger ruling. Read the waiver tests and helpers:
- in `crates/factory-engine/tests/waivers.rs`, the helpers `waiver`, `two_blockers` and `job_dir`, and the test `a_retry_does_not_duplicate_the_waiver`;
- in `crates/factory-cli/tests/new_to_merged.rs`, the helper `parked_with_changes` and the test `approve_with_waive_merges_and_records_the_waiver`.

Read the spec's Design section 5.

- [ ] **Step 2: Write the failing tests**

```rust
#[test]
fn a_retry_with_a_new_reason_updates_the_waiver() {
    let t = FakeTracker::new("o/r");
    let fx = with_tracker(approved_job(), t.clone());
    record_waiver(&job_dir(&fx), two_blockers()).expect("first");
    let mut again = two_blockers();
    again.reason = "a better reason".into();
    again.at = 20;
    record_waiver(&job_dir(&fx), again).expect("retry");
    let list = read_waivers(&job_dir(&fx));
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].reason, "a better reason");
    assert_eq!(list[0].at, 20);
    assert_eq!(list[0].blockers.len(), 2);
}
```

| Test (file) | Setup | Assertion |
|---|---|---|
| `a_failed_waiver_write_leaves_the_job_in_needs_human` (`crates/factory-cli/tests/new_to_merged.rs`) | `let sc = parked_with_changes()?;` then, for each entry of `std::fs::read_dir(sc.root.join(".superpowers/sdd"))?`, create a directory named `waivers.json` in it with `std::fs::create_dir`, so the atomic write cannot replace it | `sc.run(&["approve", "1", "--waive", "accepted for now"])` returns `Err` (`expect_err("waiver write fails")`), and `sc.stage("001")?` equals `"needs-human"` |

- [ ] **Step 3: Run them and see them fail**

Run: `cargo test -j4 -p factory-engine --test waivers`, then `cargo test -j4 -p factory-cli --test new_to_merged a_failed_waiver_write_leaves_the_job_in_needs_human`.
Expected:
- `a_retry_with_a_new_reason_updates_the_waiver` fails, because the reason stays `the helper is fine for now`.
- `a_failed_waiver_write_leaves_the_job_in_needs_human` fails, because the stage is `approve`.

- [ ] **Step 4: Implement**

- `record_waiver`: when the last entry is a retry (same `head` and `file`, not `superseded`), set its `reason` and `at` from the new waiver and write the list. Otherwise push and write, as today. Update the doc comment.
- `approve`: after `hold_project` and `renew_lease`, and just before the `update_job` that moves the stage to Approve, record the waiver and build the ruling text.
- Emit the `operator.approve` log after `update_job` succeeds.
- The without-review ruling keeps reading `job.stage` from the `job` snapshot taken before the move.
- Keep the ruling texts and the non-waive path as they are, and use the same `now` for both.

- [ ] **Step 5: Run them and see them pass**

Run: `cargo test -j4 -p factory-engine --test waivers`, then `cargo test -j4 -p factory-cli --test new_to_merged`, then `cargo test -j4 -p factory-cli --test operator_events`.
Expected: all pass, 0 failed.

- [ ] **Step 6: Format and lint**

Run: `cargo fmt -p factory-engine` and `cargo fmt -p factory-cli`, then `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings` and `cargo clippy -j4 -p factory-cli --all-targets -- -D warnings`.
Expected: clippy finishes with no warnings.

Next plan: none
