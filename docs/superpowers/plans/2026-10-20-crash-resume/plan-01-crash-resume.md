# Crash resume, Plan 1: Dead-owner job lease

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** A job lease whose owner is a `driver-<pid>-<secs>` process that no longer runs is not live, so `run`, `approve`, `cancel` without `--force`, `tickets`, `kb-refresh` and `cleanup` treat it as free at once, the same rule the project lease already uses. A unit test pins that `factoryd` counts an adopted child whose last line is `merged` as `Merged`.

**Architecture:** The private `owner_is_dead` in `crates/factory-engine/src/project_lease.rs` becomes `pub fn lease_owner_dead(owner: &str) -> bool`, re-exported from `factory-engine`. `lease_is_live` and the private `check_free` in `crates/factory-engine/src/lease.rs` add `!lease_owner_dead(&l.owner)` to their expiry test. Every job-lease reader already goes through `lease_is_live` (cancel, `remove_cancelled_card`, tickets, kb-refresh) or through `acquire_lease` and `check_free` (`Driver::renew_lease`, used by run, approve and the run loop, and the heartbeat), so the CLI needs no logic change, only tests that prove each reader follows. `factory-supervisor` keeps `classify` as it is and gets a comment and a unit test.

**Tech Stack:** Rust, `tempfile`, `git2` (CLI fixture), `sysinfo` through `factory_engine::procinfo` (unchanged).

**Spec:** docs/superpowers/specs/2026-10-20-crash-resume-design.md

## Global Constraints

- Always `-j4`; never `--workspace` or `--all`; never set `CARGO_TARGET_DIR`. Use per-crate commands: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>` (with `--test <file>` for an integration test file).
- Board JSON stays compatible with `factory-server`. This plan adds no board, ledger or events field; `Lease { owner, expires }` keeps its shape.
- Board, ledger and events are written only by the driver; the hook runs inside the driver process.
- Engine logic stays behind the ports in `ports.rs`; tests use fakes. No port in `ports.rs` carries leases: the job lease rule lives in `lease.rs` over `BoardStore`, the project lease in `project_lease.rs`, and both are tested on real temp boards, as `lock_lease.rs` and `project_lease.rs` do today.
- Never read `/proc` directly; process checks go through `procinfo.rs`. `lease_owner_dead` reaches the process table only through `process_gone`, which calls `procinfo::pid_exists`.
- The dead-owner rule is exactly the project lease rule: `owner_pid(owner)` is `Some` and `process_gone(pid)`. This process is alive, a zombie counts as existing (`pid_exists`), and any owner that is not `driver-<pid>-<secs>` is judged by expiry only.
- Each task lists the tests it adds. Tests use `.expect("...")` and `expect_err`, never the unwrap call. The dead owner in every test is `driver-4294967000-1` (a pid above the kernel limit), named `DEAD_OWNER` as in `crates/factory-engine/tests/project_lease.rs`.
- Comments are short full sentences ending with a punctuation mark.

## Review Focus

1. An owner that only looks like a driver (`tester`, `driver-x-1`, `driver-4294967000`, `driver-4294967000-x`) with a live expiry. Expected: the lease is live, another owner is refused with `LeaseError::Held`, and it becomes free only after expiry. Pinned by `a_non_driver_owner_is_judged_by_expiry_only` (Task 1).
2. A lease owned by this very process (`driver-<own pid>-1`). Expected: live, and another owner is refused, so a driver never takes over its own running sibling in the same process. Pinned by `a_live_owner_lease_stays_live` (Task 1) and, through the CLI, `run_is_refused_for_a_job_leased_by_a_live_driver` (Task 2).
3. `release_lease` by another owner on a dead owner's unexpired lease. Expected: `Ok(())` and the lease is gone, because `release_lease` shares `check_free`. Pinned by `a_dead_owner_lease_can_be_released_by_another_owner` (Task 1).
4. `approve` on a parked job whose lease belongs to a dead driver. Expected: the approve takes the lease and merges. Pinned by `approve_takes_over_a_dead_owner_lease` (Task 2).
5. `kb-refresh` on a merged job with a dead owner's lease. Expected: no `leased by` refusal; with no `kb_path` the reply is `kb_path is not set`. Pinned by `kb_refresh_accepts_a_dead_owner_lease` (Task 2).
6. `cleanup` of a cancelled job whose card carries a dead owner's lease (`remove_cancelled_card`). Expected: the card is removed, not kept with `has a live lease`. Pinned by `cleanup_removes_the_card_despite_a_dead_owner_lease` (Task 2).
7. An adopted child (`code == None`) whose `merged` line is followed by another non-empty line. Expected: `Outcome::Lost`, because only the last non-empty line decides. Pinned by `an_adopted_child_with_a_merged_line_is_merged` (Task 3).

---

## File Structure

- `crates/factory-engine/src/project_lease.rs` (modify): `owner_is_dead` becomes `pub fn lease_owner_dead`; its three callers use the new name.
- `crates/factory-engine/src/lease.rs` (modify): `lease_is_live` and `check_free` add the dead-owner test; doc comments of `acquire_lease` and `release_lease`.
- `crates/factory-engine/src/lib.rs` (modify): `lease_owner_dead` joins the `pub use project_lease::{...}` list.
- `crates/factory-engine/tests/lock_lease.rs` (modify): five job-lease tests and a `DEAD_OWNER` const.
- `crates/factory-engine/tests/project_lease.rs` (modify): `the_pid_helpers_are_public` covers `lease_owner_dead`.
- `crates/factory-cli/tests/project_lease_cli.rs` (modify): seven reader tests, a `DEAD_OWNER` const and a `job_lease` helper.
- `crates/factory-cli/src/commands.rs` (modify): the `LeaseGuard` doc comment only.
- `crates/factory-supervisor/src/supervisor.rs` (modify): a comment in `classify` and one unit test.

### Task 1: The dead-owner rule for the job lease

**Files:**
- Modify: `crates/factory-engine/src/project_lease.rs`, `crates/factory-engine/src/lease.rs`, `crates/factory-engine/src/lib.rs`
- Test: `crates/factory-engine/tests/lock_lease.rs`, `crates/factory-engine/tests/project_lease.rs`

**Interfaces:**
- Consumes: `pub fn owner_pid(owner: &str) -> Option<u32>` and `pub fn process_gone(pid: u32) -> bool` (`project_lease.rs`); `factory_core::Job`, `factory_core::Lease { owner: String, expires: f64 }`; the test fixture `store_with_job() -> (tempfile::TempDir, BoardStore)` in `lock_lease.rs`, which seeds job `001`.
- Produces:
  - `pub fn lease_owner_dead(owner: &str) -> bool` in `project_lease.rs`, re-exported as `factory_engine::lease_owner_dead`: `owner_pid(owner).is_some_and(process_gone)`. It replaces `owner_is_dead` in `acquire_project_lease`, `release_project_lease` and `project_lease_status`.
  - `pub fn lease_is_live(job: &Job, now: f64) -> bool` (signature unchanged): true when the job has a lease with `expires > now` and `!lease_owner_dead(&owner)`.
  - `fn check_free(job: &Job, owner: &str, now: f64) -> Result<(), LeaseError>` (private, signature unchanged): `Err(LeaseError::Held { owner, secs_left })` only when the lease is live by the rule above and its owner differs from `owner`. `acquire_lease` and `release_lease` keep their signatures.
- Port: none in `ports.rs`; see Global Constraints.

- [ ] **Step 1: Read** spec sections "Current state" (the first three rows), "1. Dead-owner job lease", the edge-case rows on the owner, and the Decisions on the dead-owner rule and the zombie owner. Read `crates/factory-engine/src/lease.rs` (all), `crates/factory-engine/src/project_lease.rs` (`owner_pid`, `process_gone`, `owner_is_dead` and its three callers), the `pub use project_lease::{...}` block in `crates/factory-engine/src/lib.rs`, `crates/factory-engine/src/procinfo.rs` (`pid_exists`), `Driver::renew_lease` and the heartbeat in `crates/factory-engine/src/compat.rs`, and the tests `store_with_job`, `a_live_lease_blocks_another_owner_until_it_expires` (`lock_lease.rs`), `DEAD_OWNER` and `the_pid_helpers_are_public` (`project_lease.rs`).
- [ ] **Step 2: Write the failing tests.** In `crates/factory-engine/tests/lock_lease.rs` add, below the imports:

```rust
/// A pid that no process has: above the kernel's pid limit.
const DEAD_OWNER: &str = "driver-4294967000-1";
```

  One full example, the spec's example (a lease with 25 minutes left):

```rust
#[test]
fn a_dead_owner_lease_is_taken_over() {
    let (_d, store) = store_with_job();
    acquire_lease(&store, "001", DEAD_OWNER, 1500.0, 1000.0).expect("dead driver");
    acquire_lease(&store, "001", "driver-1111-2", 1500.0, 1010.0).expect("take over");
    let job = &store.load().expect("board").jobs[0];
    let lease = job.lease.as_ref().expect("lease");
    assert_eq!(lease.owner, "driver-1111-2");
    assert_eq!(lease.expires, 2510.0);
}
```

  The other tests in `lock_lease.rs`, all on `store_with_job()`, with the first lease taken by `acquire_lease(&store, "001", <owner>, 1500.0, 1000.0)`:

| Test | Owner of the first lease | Assertions |
|---|---|---|
| `a_dead_owner_lease_is_not_live` | `DEAD_OWNER` | `!lease_is_live(&job, 1010.0)` and `!lease_is_live(&job, 3000.0)` |
| `a_live_owner_lease_stays_live` | `format!("driver-{}-1", std::process::id())` | `lease_is_live(&job, 1010.0)`; `acquire_lease(&store, "001", "driver-1111-2", 60.0, 1010.0)` matches `Err(LeaseError::Held { .. })`; `!lease_is_live(&job, 2600.0)` |
| `a_non_driver_owner_is_judged_by_expiry_only` | each of `tester`, `driver-x-1`, `driver-4294967000`, `driver-4294967000-x` in a loop (clear with `release_lease(&store, "001", owner, 1010.0)` between rounds) | `lease_is_live(&job, 1010.0)` with the owner in the message; `acquire_lease(.., "driver-1111-2", 60.0, 1010.0)` is `Err(LeaseError::Held { .. })`; `!lease_is_live(&job, 2600.0)` |
| `a_dead_owner_lease_can_be_released_by_another_owner` | `DEAD_OWNER` | `release_lease(&store, "001", "driver-1111-2", 1010.0).expect("release")`; then `store.load().expect("b").jobs[0].lease.is_none()` |

  In `crates/factory-engine/tests/project_lease.rs` extend `the_pid_helpers_are_public` with:

```rust
    assert!(!factory_engine::lease_owner_dead(&format!("driver-{}-1", std::process::id())));
    assert!(!factory_engine::lease_owner_dead("tester"));
    assert!(!factory_engine::lease_owner_dead("driver-4294967000"));
    #[cfg(target_os = "linux")]
    assert!(factory_engine::lease_owner_dead(DEAD_OWNER));
```

- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine --test lock_lease` and expect FAIL: `a_dead_owner_lease_is_not_live` (`lease_is_live` is true), `a_dead_owner_lease_is_taken_over` (`take over: Held { owner: "driver-4294967000-1", secs_left: 1490 }`) and `a_dead_owner_lease_can_be_released_by_another_owner` (`Held`). `a_live_owner_lease_stays_live`, `a_non_driver_owner_is_judged_by_expiry_only` and the three existing tests PASS, since they pin today's rule. Then run `cargo test -j4 -p factory-engine --test project_lease the_pid_helpers_are_public` and expect a compile failure: `cannot find function lease_owner_dead in crate factory_engine`.
- [ ] **Step 4: Implement** from the Interfaces block. Rename `owner_is_dead` to `pub fn lease_owner_dead` with the doc comment `/// True when the owner is a driver process that no longer runs on this machine. Other owners are never dead.`, update its three callers, and add it to the `pub use project_lease::{...}` list in `lib.rs`. In `lease.rs`, import it with `use crate::lease_owner_dead;`, change `lease_is_live`, and make `check_free` refuse only when the lease is live by the same test (call `lease_is_live(job, now)` or repeat the two conditions; do not add a third). Change the doc comment of `acquire_lease` to `/// Takes or renews the lease. A lease that is expired, or whose driver process is gone, can be taken over by anyone.` and of `release_lease` to `/// Drops the lease. Another owner's live lease is left alone and reported; a dead owner's lease is not live.`
- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine --test lock_lease` and expect all 8 tests to PASS, then `cargo test -j4 -p factory-engine --test project_lease` and expect all tests to PASS (the project lease behaves as before). Then `cargo test -j4 -p factory-engine` (every engine test PASS, including the heartbeat tests in `compat.rs`), `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings` and `cargo fmt -p factory-engine -- --check`, all PASS.

### Task 2: Every CLI reader of the job lease follows the rule

**Files:**
- Modify: `crates/factory-cli/src/commands.rs` (the `LeaseGuard` doc comment only)
- Test: `crates/factory-cli/tests/project_lease_cli.rs`

**Interfaces:**
- Consumes: `factory_engine::lease_is_live(job: &Job, now: f64) -> bool` and the `check_free` rule from Task 1; the fixture `common::Fixture` with `Fixture::new()`, `Fixture::with_risk_path(risk: &str)`, `run(&self, args: &[&str]) -> Result<String, CliError>`, `hold_lease(&self, id: &str, owner: &str)` (a 600 second lease taken through `acquire_lease`), `main_tip(&self) -> String`, `builder_calls(&self) -> usize`; the file's own helper `new_job(fx: &Fixture)` (one-task plan, job `001`); `factory_engine::BoardStore::new(path: &Path, wait: Duration)`, `factory_core::Layout::in_repo(root).board_file()`.
- Produces: no new public item. In the test file: `const DEAD_OWNER: &str = "driver-4294967000-1";` (doc comment `/// A pid that no process has: above the kernel's pid limit.`) and `fn job_lease(fx: &Fixture) -> Option<factory_engine::Lease>`, which loads the board and returns the lease of job `001`.
- The readers and the path each takes, all through Task 1's rule: `run` and `approve` through `Driver::renew_lease` then `acquire_lease`; `cancel` (`commands.rs`, `if lease_is_live(&job, now_secs()) && !force`); `remove_cancelled_card` (`lease_is_live(job, now_secs())` inside the board update); `tickets` and `kb_refresh` (`.filter(|_| lease_is_live(&job, now_secs()))`). The supervisor API forwards `cancel` to a `factory` child, so it has no check of its own.
- Port: none; this task changes no `factory-engine` code.

- [ ] **Step 1: Read** spec section 1 (the readers bullet) and the acceptance criteria on `run`, `approve`, `cancel`, `tickets` and `kb-refresh`. Read in `crates/factory-cli/src/commands.rs` the functions `run`, `LeaseGuard`, `approve`, `cancel`, `cleanup`, `remove_cancelled_card`, `tickets` and `kb_refresh`; `crates/factory-cli/tests/common/mod.rs` (`Fixture`, `RoleRunner`, `hold_lease`); `crates/factory-cli/tests/project_lease_cli.rs` (all); and in `crates/factory-cli/tests/new_to_merged.rs` the tests that call `hold_lease` (`leased by someone-else`, `kb_refresh_refuses_while_another_driver_holds_the_lease`, `cleanup_keeps_the_card_of_a_cancelled_job_with_a_live_lease`), which must keep passing.
- [ ] **Step 2: Write the tests** in `crates/factory-cli/tests/project_lease_cli.rs`, with the const and helper from Interfaces. One full example:

```rust
#[test]
fn run_resumes_a_job_whose_driver_died_with_a_live_lease() {
    let fx = Fixture::new();
    new_job(&fx);
    fx.hold_lease("001", DEAD_OWNER);
    assert_eq!(
        job_lease(&fx).expect("held").owner,
        DEAD_OWNER,
        "the dead driver's lease is on the board"
    );
    assert_eq!(fx.run(&["run", "1"]).expect("run"), "merged");
    assert!(job_lease(&fx).is_none(), "the run released the lease it took over");
}
```

  The other tests:

| Test | Setup | Assertions |
|---|---|---|
| `run_is_refused_for_a_job_leased_by_a_live_driver` | `Fixture::new()`; `new_job`; `before = fx.main_tip()`; `fx.hold_lease("001", &format!("driver-{}-1", std::process::id()))` | `fx.run(&["run", "1"]).expect_err("leased")` text contains `leased by driver-`; `fx.main_tip() == before`; `fx.builder_calls() == 0` |
| `approve_takes_over_a_dead_owner_lease` | `Fixture::with_risk_path("docs/**")`; `new_job`; `fx.run(&["run", "1"])` starts with `needs-human`; `fx.hold_lease("001", DEAD_OWNER)` | `fx.run(&["approve", "1"]).expect("approve")` starts with `merged`; `job_lease(&fx).is_none()` |
| `cancel_without_force_accepts_a_dead_owner_lease` | `Fixture::new()`; `new_job`; `fx.hold_lease("001", DEAD_OWNER)` | `fx.run(&["cancel", "1"]).expect("cancel")` contains `cancelled` and not `project lease released`; `job_lease(&fx).is_none()` |
| `tickets_accepts_a_dead_owner_lease` | `Fixture::new()`; `new_job` (not merged); `fx.hold_lease("001", DEAD_OWNER)` | `fx.run(&["tickets", "1"]).expect("tickets")` contains `skipped` and `not merged` |
| `kb_refresh_accepts_a_dead_owner_lease` | `Fixture::new()`; `new_job`; `fx.run(&["run", "1"])` equals `merged`; `fx.hold_lease("001", DEAD_OWNER)` | `fx.run(&["kb-refresh", "--job", "1"]).expect("kb")` equals `kb_path is not set` |
| `cleanup_removes_the_card_despite_a_dead_owner_lease` | `Fixture::with_risk_path("docs/**")`; `new_job`; `run 1` (needs-human); `cancel 1`; `fx.hold_lease("001", DEAD_OWNER)` | `fx.run(&["cleanup", "1", "--discard-commits"]).expect("cleanup")` contains `card removed from the board`; the loaded board has no jobs |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-cli --test project_lease_cli`. Without Task 1 the six dead-owner tests fail with `job is leased by driver-4294967000-1 for another <N> seconds` (run, approve), `job has a live lease, use --force to cancel anyway` (cancel), `job 1 is leased by driver-4294967000-1` (tickets, kb-refresh) and `card kept on the board: job 001 has a live lease` (cleanup). With Task 1 in place, expect all to PASS, because every reader goes through `lease_is_live` or `check_free`. A test that still fails names a reader that bypasses the rule: route that reader through `lease_is_live` in Step 4 and add no second liveness check. `run_is_refused_for_a_job_leased_by_a_live_driver` PASSes in both cases.
- [ ] **Step 4: Implement.** Change the `LeaseGuard` doc comment in `commands.rs` to `/// Releases the lease on every exit path. A crashed driver's lease is free once its process is gone, or when it expires.` Change no other line of `commands.rs` unless Step 3 showed a reader that bypasses the rule.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-cli --test project_lease_cli` and expect every test to PASS, then `cargo test -j4 -p factory-cli --test new_to_merged` and `cargo test -j4 -p factory-cli --test tickets` and expect PASS (the `someone-else` refusals are unchanged). Finish with `cargo test -j4 -p factory-cli --test no_hardcoded_paths`, `cargo clippy -j4 -p factory-cli --all-targets -- -D warnings` and `cargo fmt -p factory-cli -- --check`, all PASS.

### Task 3: The adopted child with a merged line

**Files:**
- Modify: `crates/factory-supervisor/src/supervisor.rs`

**Interfaces:**
- Consumes: `pub fn classify(out: &str, err: &str, code: Option<Option<i32>>) -> Outcome` (`supervisor.rs`; `code` is `None` for an adopted child), `Outcome::{Merged, Lost { reason }}` (`crates/factory-supervisor/src/records.rs`), `factory_core::outcome_lines::merged_line(sha: Option<&str>) -> String`.
- Produces: no signature change. A comment in `classify`, above the `clean_exit` test, and the unit test `an_adopted_child_with_a_merged_line_is_merged` in the existing `#[cfg(test)] mod tests`.
- Port: none; this task changes no `factory-engine` code.

- [ ] **Step 1: Read** spec section 4 and the Decision on the adopted child. Read `classify`, the adopted-child branch of the tick loop (`ChildPoll::Unknown => (!adopted_alive(&child), None)` and the `classify(&out, &err, code)` call), and the tests `classifies_by_the_last_stdout_line` and `classifies_the_approve_shapes` in `crates/factory-supervisor/src/supervisor.rs`.
- [ ] **Step 2: Write the test** in the `tests` module of `supervisor.rs`:

```rust
#[test]
fn an_adopted_child_with_a_merged_line_is_merged() {
    use factory_core::outcome_lines::merged_line;
    assert_eq!(classify(&format!("{}\n", merged_line(None)), "", None), Outcome::Merged);
    assert_eq!(
        classify(&format!("{}\n", merged_line(Some("abc1234"))), "", None),
        Outcome::Merged
    );
    assert_eq!(classify("plan 1 of 2\nmerged\n\n", "", None), Outcome::Merged);
    assert!(matches!(
        classify("merged\nplan 2 of 2\n", "", None),
        Outcome::Lost { .. }
    ));
}
```

- [ ] **Step 3: Run** `cargo test -j4 -p factory-supervisor an_adopted_child_with_a_merged_line_is_merged`. This test pins today's rule, so expect it to PASS on the current code. If it fails, `classify` has drifted from the spec: stop and report, and change no logic.
- [ ] **Step 4: Implement** the comment only, above `let clean_exit = ...` in `classify`: `// An adopted child has no exit code, so its last line decides. A series and approve print merged only at the very end, so that line is a finished result.` Keep the existing comment on the exit code.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-supervisor` and expect every test to PASS, then `cargo clippy -j4 -p factory-supervisor --all-targets -- -D warnings` and `cargo fmt -p factory-supervisor -- --check`, both PASS.

Next plan: docs/superpowers/plans/2026-10-20-crash-resume/plan-02-crash-resume.md
