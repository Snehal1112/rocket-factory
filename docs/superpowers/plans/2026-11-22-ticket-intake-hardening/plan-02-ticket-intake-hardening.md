# Safe export names, sync rerun and waiver order Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-22-ticket-intake-hardening-design.md
> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** These three changes close the rest of the spec:
- An export refuses tree names that leave its folder.
- A second `factory sync` finishes a sync that stopped after its merge.
- `approve --waive` writes the waiver before it approves and keeps the latest reason.

**Commit:** fix(factory): safe export, sync rerun, waivers

**Architecture:** Three independent changes:
- `entry_name` in `crates/factory-git/src/repo.rs`, a pure name rule used by `export_entry`.
- An `unfinished` condition on the `UpToDate` return of `sync_requested_by` in `crates/factory-engine/src/sync.rs`.
- The waiver-first order in `approve` in `crates/factory-cli/src/commands.rs`, plus a retry update in `record_waiver` in `crates/factory-engine/src/waivers.rs`.

**Tech Stack:** Rust, `git2`.

## Global Constraints

- Cargo commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- Never shell out to `git`. `factory-git` stays the only git layer.
- `waivers.json` keeps its shape. A retry changes only the last entry's `reason` and `at`.
- Error texts, exactly: `a path under {root:?} is not UTF-8` and `unsafe path component {name:?} under {root:?}`, both as `GitError::BadInput`, as today.

## Review Focus

1. A tree entry named exactly `.git`: a user expects it skipped, not refused. Pinned by `entry_name_skips_dot_git`.
2. A plain name with a dot, such as `.env` or `a.b`: a user expects it written. Pinned by `entry_name_accepts_a_plain_name`.
3. A parked job whose reviews all have verdicts (here `Changes`): a user expects `factory sync` to still say up to date, not re-review. Pinned by `a_parked_job_with_only_verdicts_stays_up_to_date`. The approved, up-to-date case stays pinned by the existing `sync_requested_on_an_up_to_date_job_changes_nothing`.
4. A waiver retry with the same reason: a user expects one entry and no error. Already pinned by `a_retry_does_not_duplicate_the_waiver`, which must pass unchanged.
5. A successful `approve --waive`: a user expects the merge and one waiver, as today. Already pinned by `approve_with_waive_merges_and_records_the_waiver`, which must pass unchanged.

## File Structure

- Modify: `crates/factory-git/src/repo.rs`. Add `entry_name`, use it in `export_entry`, and add a new `#[cfg(test)] mod tests` at the end.
- Modify: `crates/factory-engine/src/sync.rs`. Add the `unfinished` condition in `sync_requested_by`.
- Modify: `crates/factory-engine/tests/sync_review.rs`. Add two tests and update `an_up_to_date_job_with_tip_equal_head_changes_nothing_in_every_stage`.
- Modify: `crates/factory-engine/src/waivers.rs`. Add the retry update in `record_waiver`.
- Modify: `crates/factory-engine/tests/waivers.rs`. Add one test.
- Modify: `crates/factory-cli/src/commands.rs`. Write the waiver before the stage change in `approve`.
- Modify: `crates/factory-cli/tests/new_to_merged.rs`. Add one test.

---

### Task 1: export refuses names that leave the folder

**Files:**
- Modify: `crates/factory-git/src/repo.rs`

**Interfaces:**
- Port: none. `factory-git` is the concrete git layer.
- Produces: `fn entry_name<'a>(name: Option<&'a str>, root: &str) -> Result<Option<&'a str>>`, private to `repo.rs`. `Result` is the crate's `Result<T, GitError>`. `Ok(None)` means skip.

**Tests added:** `entry_name_refuses_a_non_utf8_name`, `entry_name_skips_dot_git`, `entry_name_refuses_unsafe_names`, `entry_name_accepts_a_plain_name`.

- [ ] **Step 1: Read the code**

Read `export_tree`, `export_commit` and `export_entry` in `crates/factory-git/src/repo.rs`, the `GitError` enum, and `crates/factory-git/tests/export_tree.rs`. Read the spec's Design section 2.

- [ ] **Step 2: Write the failing tests**

Add `#[cfg(test)] mod tests { use super::*; ... }` at the end of `repo.rs`. One full example:

```rust
#[test]
fn entry_name_refuses_unsafe_names() {
    for name in ["", ".", "..", "a/b", "a\\b", "a\0b"] {
        let err = entry_name(Some(name), "src").expect_err(name);
        assert!(
            err.to_string().contains("unsafe path component"),
            "{name:?}: {err}"
        );
    }
}
```

| Test | Call | Assertion |
|---|---|---|
| `entry_name_refuses_a_non_utf8_name` | `entry_name(None, "src")` | `expect_err`, and its `to_string()` contains `is not UTF-8` |
| `entry_name_skips_dot_git` | `entry_name(Some(".git"), "src")` | `.expect("skip")` equals `None` |
| `entry_name_accepts_a_plain_name` | `entry_name(Some(n), "src")` for each of `"main.rs"`, `".env"` and `"a.b"` | `.expect("ok")` equals `Some(n)` |

- [ ] **Step 3: Run them and see them fail**

Run: `cargo test -j4 -p factory-git --lib entry_name`
Expected: the build fails with ``cannot find function `entry_name` in this scope``.

- [ ] **Step 4: Implement**

- Write `entry_name` with the rules in spec section 2.
- In `export_entry`, call `entry_name(entry.name(), root)?`. `None` returns `Ok(git2::TreeWalkResult::Skip)`, and `Some(name)` goes on as today.
- Give `entry_name` a doc comment that lists the rules.

- [ ] **Step 5: Run them and see them pass**

Run: `cargo test -j4 -p factory-git`
Expected: all pass, including `crates/factory-git/tests/export_tree.rs`, with 0 failed.

### Task 2: a second sync finishes a stopped one

**Files:**
- Modify: `crates/factory-engine/src/sync.rs`
- Modify: `crates/factory-engine/tests/sync_review.rs`

**Interfaces:**
- Port: none new. The reviewer runs through `AgentRunner`, scripted in the tests.
- Consumes: `factory_core::{Stage, ReviewState, ResumeAt}`, `Job.reviews: BTreeMap<String, ReviewState>`, `Job.park_resume: Option<ResumeAt>`, `Config.fix_rounds: u32` and `crate::merge::park_with_parts(driver, id, reason, kind, parts)`.
- Produces: no new items. `sync_requested_by` keeps its signature.

**Tests added:** `a_sync_stopped_after_its_merge_reviews_the_whole_branch_on_rerun`, `a_parked_job_with_only_verdicts_stays_up_to_date`, `a_build_parked_job_stays_up_to_date_on_sync`, `a_sync_park_resumes_at_rereview`, `a_sync_parked_by_its_checks_resumes_at_rereview`. Test changed: `an_up_to_date_job_with_tip_equal_head_changes_nothing_in_every_stage`.

- [ ] **Step 1: Read the code**

Read `sync_requested` and `sync_requested_by` in `crates/factory-engine/src/sync.rs`. In `crates/factory-engine/tests/sync_review.rs`, read:
- the helpers `fixture`, `fixture_with_check`, `force_stage`, `ledger`, `review_calls` and `with_security_agent`, and the consts `APPROVE` and `SECURITY_OK`;
- the tests `an_up_to_date_job_with_tip_equal_head_changes_nothing_in_every_stage` and `sync_requested_on_an_up_to_date_job_changes_nothing`;
- the test `a_second_sync_after_a_stopped_first_one_still_reviews_the_whole_branch`.

Read `common::commit_file` in `crates/factory-engine/tests/common/mod.rs`, and `park_with_parts` and `park_with_parts_resume` in `crates/factory-engine/src/merge.rs`. Read the spec's Design section 4.

- [ ] **Step 2: Write the tests**

```rust
#[test]
fn a_sync_stopped_after_its_merge_reviews_the_whole_branch_on_rerun() {
    let flags = tempfile::tempdir().expect("tempdir");
    let flag = flags.path().join("ok");
    let fx = with_security_agent(fixture_with_check(
        &format!("test -f {}", flag.display()),
        vec![reply(APPROVE), reply(SECURITY_OK)],
    ));
    force_stage(&fx, factory_core::Stage::NeedsHuman);
    common::commit_file(&fx.main, "other.md", "o");
    let first = sync_requested(&fx.driver, &fx.id).expect("first sync");
    assert!(matches!(first, SyncOutcome::Parked(..)), "{first:?}");
    assert_eq!(review_calls(&fx), 0);
    std::fs::write(&flag, "").expect("flag");
    let second = sync_requested(&fx.driver, &fx.id).expect("second sync");
    assert_ne!(second, SyncOutcome::UpToDate);
    assert_eq!(fx.runner.specs_for("reviewer").len(), 1);
    assert_eq!(fx.runner.specs_for("security-review").len(), 1);
    assert_eq!(fx.driver.job(&fx.id).expect("job").prev_head, None);
}
```

| Test | Setup | Assertion |
|---|---|---|
| `a_parked_job_with_only_verdicts_stays_up_to_date` | `fixture(vec![])`, no commit on main; `update_job` sets `j.set_review("code", factory_core::ReviewState::Changes)` and `j.park_resume = Some(factory_core::ResumeAt::Rereview)`; then `force_stage(&fx, factory_core::Stage::NeedsHuman)` | `sync_requested(&fx.driver, &fx.id).expect("sync")` equals `SyncOutcome::UpToDate`, and `review_calls(&fx) == 0` |
| `a_build_parked_job_stays_up_to_date_on_sync` | `fixture(vec![])`, no commit on main; the code review stays `Pending`; `update_job` sets `j.park_resume = Some(factory_core::ResumeAt::Build)`; then `force_stage(&fx, factory_core::Stage::NeedsHuman)` | `sync_requested` equals `SyncOutcome::UpToDate`, `review_calls(&fx) == 0`, and `fx.driver.job(&fx.id).expect("job").park_resume == Some(factory_core::ResumeAt::Build)` |
| `a_sync_park_resumes_at_rereview` | the first half of `a_sync_stopped_after_its_merge_reviews_the_whole_branch_on_rerun`, up to the first sync | after the first sync, `fx.driver.job(&fx.id).expect("job").park_resume == Some(factory_core::ResumeAt::Rereview)` |
| `a_sync_parked_by_its_checks_resumes_at_rereview` | build the fixture as `fixture_with_check` does, but with `fix_rounds: 0` in the `Config`, the same flag-file check and `with_security_agent`, effects `vec![reply(APPROVE), reply(SECURITY_OK)]`; `force_stage` to `NeedsHuman`; commit `other.md` on main; first sync | first sync returns `SyncOutcome::Parked(..)`; the job's `park_resume == Some(factory_core::ResumeAt::Rereview)`; after writing the flag file, a second sync is not `UpToDate` and `fx.runner.specs_for("reviewer").len() == 1` |

Also update the existing `an_up_to_date_job_with_tip_equal_head_changes_nothing_in_every_stage`: in its loop, before `force_stage`, set the code review to `ReviewState::Pass` with `update_job` and `j.set_review("code", ReviewState::Pass)`. A `Pending` review in Review or NeedsHuman now reruns. With `Pass` the test keeps meaning "an up-to-date job with finished reviews changes nothing". Keep its assertions unchanged.

`fixture_with_check` takes `(&str, Vec<common::Effect>)`. `reply`, `APPROVE`, `SECURITY_OK`, `with_security_agent` and `review_calls` are in the file, `SyncOutcome` derives `PartialEq` and `Debug`, and `tempfile` is a dev-dependency. The security agent is needed because the sync review always adds a pending `security` review.

- [ ] **Step 3: Run them and see the rerun test fail**

Run: `cargo test -j4 -p factory-engine --test sync_review`
Expected:
- `a_sync_stopped_after_its_merge_reviews_the_whole_branch_on_rerun` fails at `assert_ne!(second, SyncOutcome::UpToDate)`, because today the second sync returns `UpToDate`.
- `a_sync_park_resumes_at_rereview` and `a_sync_parked_by_its_checks_resumes_at_rereview` fail, because today the sync park records no `Rereview` (the second also because the checks park keeps `None`).
- `a_parked_job_with_only_verdicts_stays_up_to_date`, `a_build_parked_job_stays_up_to_date_on_sync` and the updated existing test pass.

- [ ] **Step 4: Implement**

In `sync_requested_by`, make both changes from spec section 4:
- Where the requested sync parks, keep the `park_with_parts` call. Then, when `whole_branch` is true, set `j.park_resume = Some(ResumeAt::Rereview)` in one `update_job`, as spec section 4 shows. The write runs last, so it wins over a park the checks stage wrote earlier. The `else if` branch for "sync review passed" stays as it is.
- Compute `no_verdict` and `unfinished` as shown, and add `&& !unfinished` to the `UpToDate` condition.

Add a one-line comment above each change saying why: a stopped sync must be able to finish, and a build park must keep its resume stage.

- [ ] **Step 5: Run them and see them pass**

Run: `cargo test -j4 -p factory-engine --test sync_review`, then `cargo test -j4 -p factory-engine --test retry` and `cargo test -j4 -p factory-cli --test sync_cmd`.
Expected: all pass, 0 failed.

### Task 3: the waiver is written first and a retry keeps the latest reason

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

Run: `cargo fmt -p factory-git`, `cargo fmt -p factory-engine` and `cargo fmt -p factory-cli`. Then run clippy for each: `cargo clippy -j4 -p factory-git --all-targets -- -D warnings`, `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings` and `cargo clippy -j4 -p factory-cli --all-targets -- -D warnings`.
Expected: clippy finishes with no warnings.

Next plan: none
