# Safe export names, one rework rule and the sync rerun Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-22-ticket-intake-hardening-design.md
> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** An export refuses tree names that leave its folder. One `keeps_rework` rule decides which parks keep a plain rework. A sync that stopped after its merge records resume `Rereview`, in the job and in `park.json`, so `factory sync` or `factory retry` finishes it.

**Commit:** fix(factory): safe export names and sync rerun

**Architecture:** Three changes:
- `entry_name` in `crates/factory-git/src/repo.rs` is a pure name rule used by `export_entry`.
- `ParkKind::keeps_rework` and `ParkKind::from_name` in `crates/factory-core/src/park.rs` become the one rule that `next_with_resume`, `Job::rework_hint`, the CLI rework refusal and the dashboard card follow.
- `set_park_resume` in `crates/factory-engine/src/merge.rs` changes a park's resume stage in the job and in `park.json`. `sync_requested_by` uses it, together with an `unfinished` condition on its `UpToDate` return.

**Tech Stack:** Rust, `git2`.

## Global Constraints

- Cargo commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- Never shell out to `git`. `factory-git` stays the only git layer.
- `park.json` keeps its shape. `set_park_resume` changes only `resume` and `next`.
- The dashboard is plain JavaScript with no build step. Its tests run with `node --test`.
- Error texts, exactly: `a path under {root:?} is not UTF-8` and `unsafe path component {name:?} under {root:?}`, both as `GitError::BadInput`, as today.

## Review Focus

1. A tree entry named `.git` in any case (`.GIT`): a user expects it skipped, not refused or written. Pinned by `entry_name_skips_dot_git`.
2. A plain name with a dot, such as `.env` or `a.b`: a user expects it written. Pinned by `entry_name_accepts_a_plain_name`.
3. A `review-failed` park with a resume stage: a user expects plain `rework` still refused with the retry hint. Already pinned by `plain_rework_is_allowed_on_machine_and_stopped_parks`, which must pass unchanged.
4. A parked job whose reviews all have verdicts (here `Changes`): a user expects `factory sync` to still say up to date. Pinned by `a_parked_job_with_only_verdicts_stays_up_to_date`. The approved, up-to-date case stays pinned by the existing `sync_requested_on_an_up_to_date_job_changes_nothing`.
5. The card after a `checks-failed` sync park: a user expects `park.json` to offer both retry and rework, matching the board. Pinned by `a_sync_parked_by_its_checks_resumes_at_rereview`.

## File Structure

- Modify: `crates/factory-git/src/repo.rs`. Add `entry_name`, use it in `export_entry`, and add a new `#[cfg(test)] mod tests` at the end.
- Modify: `crates/factory-core/src/park.rs`. Add `keeps_rework` and `from_name`, use them in `next_with_resume`, and add tests.
- Modify: `crates/factory-core/src/board.rs`. Make `rework_hint` read `keeps_rework`, and add a test.
- Modify: `crates/factory-cli/src/commands.rs`. Make the rework refusal read `keeps_rework`.
- Modify: `crates/factory-cli/tests/retry_cmd.rs`. Add one test.
- Modify: `dashboard/static/projects.js` and `dashboard/tests/projects.test.js`. The card actions follow the same rule, with one test.
- Modify: `crates/factory-engine/src/merge.rs`. Add `set_park_resume`.
- Modify: `crates/factory-engine/src/sync.rs`. Add the `set_park_resume` call and the `unfinished` condition in `sync_requested_by`.
- Modify: `crates/factory-engine/tests/sync_review.rs`. Add tests and update `an_up_to_date_job_with_tip_equal_head_changes_nothing_in_every_stage`.

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
| `entry_name_skips_dot_git` | `entry_name(Some(n), "src")` for each of `".git"`, `".GIT"` and `".Git"` | `.expect("skip")` equals `None` |
| `entry_name_accepts_a_plain_name` | `entry_name(Some(n), "src")` for each of `"main.rs"`, `".env"` and `"a.b"` | `.expect("ok")` equals `Some(n)` |

- [ ] **Step 3: Run them and see them fail**

Run: `cargo test -j4 -p factory-git --lib entry_name`
Expected: the build fails with ``cannot find function `entry_name` in this scope``.

- [ ] **Step 4: Implement**

- Write `entry_name` with the rules in spec section 2. The `.git` check uses `eq_ignore_ascii_case(".git")`.
- In `export_entry`, call `entry_name(entry.name(), root)?`. `None` returns `Ok(git2::TreeWalkResult::Skip)`, and `Some(name)` goes on as today.
- Give `entry_name` a doc comment that lists the rules.

- [ ] **Step 5: Run them and see them pass**

Run: `cargo test -j4 -p factory-git`
Expected: all pass, including `crates/factory-git/tests/export_tree.rs`, with 0 failed.

### Task 2: one rule for the parks that keep rework

**Files:**
- Modify: `crates/factory-core/src/park.rs`
- Modify: `crates/factory-core/src/board.rs`
- Modify: `crates/factory-cli/src/commands.rs`
- Modify: `crates/factory-cli/tests/retry_cmd.rs`
- Modify: `dashboard/static/projects.js`
- Modify: `dashboard/tests/projects.test.js`
- Modify: `dashboard/static/board.js`
- Modify: `dashboard/tests/board.test.js`

**Interfaces:**
- Port: none. These are pure rules in factory-core, read by the CLI and mirrored in the dashboard.
- Produces: `pub fn keeps_rework(self) -> bool` and `pub fn from_name(name: &str) -> Option<ParkKind>` on `ParkKind` in `crates/factory-core/src/park.rs`. `keeps_rework` is true for `MachineWaiting`, `BuilderStopped` and `ChecksFailed`, and false for every other kind. `from_name` is the inverse of `as_str` for all 17 kinds.

**Tests added:** `keeps_rework_covers_builder_fixable_parks`, `from_name_reads_every_as_str_name`, `a_checks_failed_park_with_a_resume_offers_retry_and_rework` (all in `park.rs` `mod tests`), `rework_hint_is_rework_for_a_checks_failed_park` (`board.rs` `mod tests`), `plain_rework_is_allowed_on_a_checks_failed_park` (`retry_cmd.rs`), `jobActions offers rework on a checks-failed park with a resume` (`projects.test.js`), and `a checks-failed card with a resume offers retry and rework chips` (`board.test.js`).

- [ ] **Step 1: Read the code**

Read these:
- In `crates/factory-core/src/park.rs`: `ParkKind`, `as_str`, `resume`, `next_with_resume`, and the tests `next_with_resume_puts_retry_first_and_drops_rework` and `builder_stopped_with_build_resume_keeps_rework`.
- `Job::rework_hint` and the text that uses it, in `crates/factory-core/src/board.rs`.
- The rework refusal (`builder_park`) in `crates/factory-cli/src/commands.rs`.
- In `crates/factory-cli/tests/retry_cmd.rs`: `parked_as`, `refusal` and `plain_rework_is_allowed_on_machine_and_stopped_parks`.
- `baseActions` and `jobActions` in `dashboard/static/projects.js`, and the `jobActions` test in `dashboard/tests/projects.test.js`.
- `retryRework` and `retryApprove` in `dashboard/static/board.js` (around lines 49-56), and `renderCard` and `makeJob` in `dashboard/tests/board.test.js`.

Read the spec's Design section 6.

- [ ] **Step 2: Write the failing tests**

```rust
#[test]
fn keeps_rework_covers_builder_fixable_parks() {
    for kind in [ParkKind::MachineWaiting, ParkKind::BuilderStopped, ParkKind::ChecksFailed] {
        assert!(kind.keeps_rework(), "{kind:?}");
    }
    for kind in [ParkKind::ReviewFailed, ParkKind::NoChecks, ParkKind::SeriesWaiting, ParkKind::Other] {
        assert!(!kind.keeps_rework(), "{kind:?}");
    }
}
```

| Test | Setup | Assertion |
|---|---|---|
| `from_name_reads_every_as_str_name` | every `ParkKind` variant, listed in the test | `ParkKind::from_name(kind.as_str()) == Some(kind)` for each, and `ParkKind::from_name("nope") == None` |
| `a_checks_failed_park_with_a_resume_offers_retry_and_rework` | `next_with_resume(ParkKind::ChecksFailed, "001", false, None, Some(ResumeAt::Rereview))` | the list starts with `factory retry 001` and holds an entry starting with `factory rework 001 ` |
| `rework_hint_is_rework_for_a_checks_failed_park` | a `Job` built as the existing `review_missing_message` tests in `board.rs` build one: `stage = NeedsHuman`, code review `Pending`, `park_kind = Some("checks-failed".into())`, `park_resume = Some(ResumeAt::Rereview)` | `j.review_missing_message()` contains `"use rework, or approve"` and does not contain `"factory retry 001"` |
| `plain_rework_is_allowed_on_a_checks_failed_park` (`retry_cmd.rs`) | `parked_as("checks-failed", factory_core::ResumeAt::Rereview)`, then `fx.run(&["rework", "1", "fix x"])` | `.expect("rework")`, and `job(&fx).stage == factory_core::Stage::Build` |

Dashboard test, in `dashboard/tests/projects.test.js`:

```js
test("jobActions offers rework on a checks-failed park with a resume", () => {
  const job = { stage: "needs-human", park_resume: "rereview", park_kind: "checks-failed" };
  assert.deepEqual(projects.jobActions(job), ["retry", "rework", "cancel"]);
  const other = { stage: "needs-human", park_resume: "rereview", park_kind: "review-failed" };
  assert.deepEqual(projects.jobActions(other), ["retry", "approve", "cancel"]);
});
```

Card chip test, in `dashboard/tests/board.test.js`:

```js
test("a checks-failed card with a resume offers retry and rework chips", () => {
  const html = renderCard(makeJob({ stage: "needs-human", park_kind: "checks-failed", park_resume: "rereview" }));
  assert.match(html, /factory rework 001/);
  assert.doesNotMatch(html, /factory approve/);
});
```

Use the file's own `renderCard` and `makeJob` names, and its job id, as its other card tests do.

- [ ] **Step 3: Run them and see them fail**

Run: `cargo test -j4 -p factory-core park`, then `cargo test -j4 -p factory-cli --test retry_cmd`, then `node --test dashboard/tests/projects.test.js dashboard/tests/board.test.js`.
Expected:
- The factory-core tests fail to build with ``no method named `keeps_rework` `` and ``no function or associated item named `from_name` ``.
- `plain_rework_is_allowed_on_a_checks_failed_park` fails with the `parked outside the builder` refusal.
- The dashboard tests fail: the buttons are `["retry", "approve", "cancel"]` today, and the chips show approve and no rework.

- [ ] **Step 4: Implement**

- `park.rs`: add `keeps_rework` and `from_name`. `from_name` matches the 17 `as_str` names. In `next_with_resume`, replace the local `keep_rework` match with `kind.keeps_rework()`. Update its doc comment.
- `board.rs`: in `rework_hint`, return `"rework"` when `self.park_kind.as_deref().and_then(ParkKind::from_name).is_some_and(ParkKind::keeps_rework)`, before the resume match.
- `commands.rs`: set `builder_park` from the same expression, replacing the string list. Update the comment.
- `projects.js`: in `baseActions`, add a row before the generic resume row: a needs-human job with a non-empty `park_resume` whose `park_kind` is one of `machine-waiting`, `builder-stopped` or `checks-failed` gets `["retry", "rework", "cancel"]`. Keep the `series-waiting` row first. Add a comment naming `ParkKind::keeps_rework` in `crates/factory-core/src/park.rs` as the rule this mirrors.
- `board.js`:
  - `retryRework` is true for `park_resume === "build" && park_kind !== "series-waiting"`, or for a `park_kind` in `["machine-waiting", "builder-stopped", "checks-failed"]`.
  - `retryApprove` returns `""` for `checks-failed` as well.
  - Add the same comment.

- [ ] **Step 5: Run them and see them pass**

Run: `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-cli --test retry_cmd`, `node --test dashboard/tests/*.test.js` and `cargo test -j4 --manifest-path dashboard/Cargo.toml`.
Expected: all pass, 0 failed.

### Task 3: a second sync finishes a stopped one

**Files:**
- Modify: `crates/factory-engine/src/merge.rs`
- Modify: `crates/factory-engine/src/sync.rs`
- Modify: `crates/factory-engine/tests/sync_review.rs`

**Interfaces:**
- Port: none new. The reviewer runs through `AgentRunner`, scripted in the tests.
- Consumes:
  - `factory_core::{Stage, ReviewState, ResumeAt, ParkKind, ParkRecord, next_with_resume}`;
  - the job fields `Job.reviews: BTreeMap<String, ReviewState>` and `Job.park_resume: Option<ResumeAt>`;
  - `Config.fix_rounds: u32`;
  - `crate::merge::park_with_parts(driver, id, reason, kind, parts)` and `crate::park::write_record`.
- Produces: `pub(crate) fn set_park_resume(driver: &Driver, id: &str, resume: ResumeAt) -> Result<(), EngineError>` in `crates/factory-engine/src/merge.rs`. `sync_requested_by` keeps its signature.

**Tests added:** `a_sync_stopped_after_its_merge_reviews_the_whole_branch_on_rerun`, `a_parked_job_with_only_verdicts_stays_up_to_date`, `a_build_parked_job_stays_up_to_date_on_sync`, `a_sync_park_resumes_at_rereview`, `a_sync_parked_by_its_checks_resumes_at_rereview`, `a_sync_review_with_changes_keeps_plain_rework`. Test changed: `an_up_to_date_job_with_tip_equal_head_changes_nothing_in_every_stage`.

- [ ] **Step 1: Read the code**

Read `sync_requested` and `sync_requested_by` in `crates/factory-engine/src/sync.rs`. In `crates/factory-engine/tests/sync_review.rs`, read:
- the helpers `fixture`, `fixture_with_check`, `force_stage`, `ledger`, `review_calls` and `with_security_agent`, and the consts `APPROVE` and `SECURITY_OK`;
- the tests `an_up_to_date_job_with_tip_equal_head_changes_nothing_in_every_stage` and `sync_requested_on_an_up_to_date_job_changes_nothing`;
- the test `a_second_sync_after_a_stopped_first_one_still_reviews_the_whole_branch`.

Read these:
- `common::commit_file` in `crates/factory-engine/tests/common/mod.rs`;
- `park_with_parts` and `park_with_parts_resume` in `crates/factory-engine/src/merge.rs`;
- `write_record` in `crates/factory-engine/src/park.rs`;
- `ParkRecord::read` in `crates/factory-core/src/park.rs`.

Read the spec's Design section 4.

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
| `a_sync_review_with_changes_keeps_plain_rework` | `with_security_agent(fixture_roles(vec![("reviewer", reply(CHANGES)), ("security-review", reply(SECURITY_OK))]))`; `force_stage` to `NeedsHuman`; commit `other.md` on main; sync | the sync returns `SyncOutcome::Parked(..)`, and the job's `park_resume == None` |
| `a_sync_parked_by_its_checks_resumes_at_rereview` | build the fixture as `fixture_with_check` does, but with `fix_rounds: 0` in the `Config`, the same flag-file check and `with_security_agent`, effects `vec![reply(APPROVE), reply(SECURITY_OK)]`; `force_stage` to `NeedsHuman`; commit `other.md` on main; first sync | first sync returns `SyncOutcome::Parked(..)`; the job's `park_kind == Some("checks-failed")` and `park_resume == Some(factory_core::ResumeAt::Rereview)`; `factory_core::ParkRecord::read(&job_dir).expect("park.json")` has `resume == Some(Rereview)`, and its `next` holds an entry starting with `factory retry 001` and one starting with `factory rework 001 ` (the job dir is `fx.driver.job_dir(&job)`); after writing the flag file, a second sync is not `UpToDate` and `fx.runner.specs_for("reviewer").len() == 1` |

Also update the existing `an_up_to_date_job_with_tip_equal_head_changes_nothing_in_every_stage`: in its loop, before `force_stage`, set the code review to `ReviewState::Pass` with `update_job` and `j.set_review("code", ReviewState::Pass)`. A `Pending` review in Review, or in needs-human with resume `Rereview`, now reruns. With `Pass` the test keeps meaning "an up-to-date job with finished reviews changes nothing". Keep its assertions unchanged.

`fixture_with_check` takes `(&str, Vec<common::Effect>)`. `reply`, `APPROVE`, `SECURITY_OK`, `with_security_agent` and `review_calls` are in the file, `SyncOutcome` derives `PartialEq` and `Debug`, and `tempfile` is a dev-dependency. The security agent is needed because the sync review always adds a pending `security` review.

- [ ] **Step 3: Run them and see the rerun test fail**

Run: `cargo test -j4 -p factory-engine --test sync_review`
Expected:
- `a_sync_stopped_after_its_merge_reviews_the_whole_branch_on_rerun` fails at `assert_ne!(second, SyncOutcome::UpToDate)`, because today the second sync returns `UpToDate`.
- `a_sync_park_resumes_at_rereview` and `a_sync_parked_by_its_checks_resumes_at_rereview` fail, because today the sync park records no `Rereview` (the second also because the checks park keeps `None`).
- `a_parked_job_with_only_verdicts_stays_up_to_date`, `a_build_parked_job_stays_up_to_date_on_sync` and the updated existing test pass.

- [ ] **Step 4: Implement**

In `sync_requested_by`, make both changes from spec section 4:
- In `merge.rs`, add `set_park_resume`:
  - In one `update_job`, set `park_resume = Some(resume)` on a job in needs-human, and leave any other stage alone.
  - Then read `park.json` with `ParkRecord::read` and set its `resume`.
  - Recompute its `next` with `next_with_resume(record.kind, id, job.review_missing(), verdict, Some(resume))`, with the verdict from `job.last_review`, as `park_with_parts_resume` does.
  - Write it with `crate::park::write_record`. A missing record is left missing; the board field is the source of truth.
  - Give it a doc comment.
- In `sync_requested_by`, where the requested sync parks, keep the `park_with_parts` call. Then read the job again. When `whole_branch` is true and it still has a `Pending`, `Running` or `Failed` review (the same `no_verdict` test), call `crate::merge::set_park_resume(driver, id, ResumeAt::Rereview)?`. A park after a verdict ("sync review asked for changes") keeps no resume stage. It runs last, so it wins over a park the checks stage wrote earlier. The `else if` branch for "sync review passed" stays as it is.
- Compute `no_verdict` and `unfinished` as in the spec's Design section 4 code block, and add `&& !unfinished` to the `UpToDate` condition.

Add a one-line comment above each change saying why: a stopped sync must be able to finish, and a build park must keep its resume stage.

- [ ] **Step 5: Run them and see them pass**

Run: `cargo test -j4 -p factory-engine --test sync_review`, then `cargo test -j4 -p factory-engine --test retry` and `cargo test -j4 -p factory-cli --test sync_cmd`.
Expected: all pass, 0 failed.

- [ ] **Step 6: Format and lint**

Run `cargo fmt -p factory-git`, `cargo fmt -p factory-core`, `cargo fmt -p factory-engine` and `cargo fmt -p factory-cli`. Then run `cargo clippy -j4 -p <crate> --all-targets -- -D warnings` for each of those four crates.
Expected: clippy finishes with no warnings.

Next plan: docs/superpowers/plans/2026-11-22-ticket-intake-hardening/plan-03-ticket-intake-hardening.md
