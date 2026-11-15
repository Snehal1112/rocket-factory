# Live driver safety: lease rules and lost checks (plan 1 of 3)

> **Spec:** docs/superpowers/specs/2026-11-15-live-driver-safety-design.md
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** A live owner keeps its lease past expiry, a corrupt project lease is refused, and a lost lease starts no agent and no merge.

**Architecture:** One `owner_state` rule in `project_lease.rs` feeds every lease reader. `Engine` gets a shared `lost` flag so `run_agent` can refuse, and `merge.rs` checks the driver flag before `merge_no_ff`.

**Tech Stack:** Rust, factory-engine, factory-core. No new dependency in this plan.

## Global Constraints

- No new `/proc` read. Process facts come from `crates/factory-engine/src/procinfo.rs`.
- `START_SLACK_SECS` is 10. A start time unknown to this machine, or an owner without a pid, keeps today's expiry rule.
- Cargo: `-j4`, one crate at a time, never `--workspace`, `--all` or `CARGO_TARGET_DIR`.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call. Code comments are short full sentences that end with a period.
- Board JSON is unchanged. Nothing in this plan adds a board, ledger or event field.

## Review Focus

1. A pid that runs but belongs to another process (reused, start time far from the name): taken over at once. Pinned by `a_lease_whose_pid_start_does_not_match_is_taken_over`.
2. A zombie owner: not alive, so taken over. Pinned by `a_zombie_owner_is_dead`.
3. A system that cannot say the start time (`Unknown`): expiry decides, no regression. Pinned by `an_owner_without_a_pid_still_expires` and the unchanged `other_owners_are_never_judged_by_pid`.
4. A corrupt or empty lease file: refused, file untouched, a forced cancel clears it. Pinned by `a_corrupt_project_lease_is_refused_and_untouched` and `a_forced_cancel_removes_a_corrupt_project_lease`.
5. An engine whose lease is lost must not run the committer or reviewer either, only the builder is tested directly. Pinned by `a_lost_engine_starts_no_agent` calling `build_stage` and a second assertion on `run_agent` through `review_stage`.

## File Structure

- `crates/factory-engine/src/project_lease.rs`: `OwnerState`, `owner_state`, `lease_held`, `ProjectLeaseError::Corrupt`, forced release of a corrupt file.
- `crates/factory-engine/src/lease.rs`: `lease_is_live` uses `lease_held`.
- `crates/factory-engine/src/lib.rs`: exports, `Engine.lost`, `with_lost`, `lost`.
- `crates/factory-engine/src/compat.rs`: `with_engine` passes the flag.
- `crates/factory-engine/src/ports.rs`, `merge.rs`, `runloop.rs`: the three lost checks and the shared error.
- Tests: `crates/factory-engine/tests/project_lease.rs`, `build_checks.rs`, `lease_heartbeat.rs`.

---

### Task 1: One liveness rule and the corrupt lease

**Files:**
- Modify: `crates/factory-engine/src/project_lease.rs`, `crates/factory-engine/src/lease.rs`, `crates/factory-engine/src/lib.rs` (lines 76 and 92, the exports)
- Test: `crates/factory-engine/tests/project_lease.rs`, `crates/factory-engine/tests/lock_lease.rs`, `crates/factory-supervisor/tests/recovery.rs`

**Interfaces:**
- Consumes: `procinfo::process_state`, `procinfo::process_start_ms`, `ProcState` (`procinfo.rs`), `owner_pid`.
- Produces:
  - `pub enum OwnerState { Alive, Dead, Unknown }` and `pub const START_SLACK_SECS: u64 = 10`.
  - `pub fn owner_state(owner: &str) -> OwnerState`.
  - `pub fn lease_held(owner: &str, expires: f64, now: f64) -> bool`.
  - `ProjectLeaseError::Corrupt { path: String, reason: String }`.
  - `lease_owner_dead` and `process_gone` are removed from the module and from `lib.rs`.

- [ ] **Step 1: Read** `project_lease.rs`, `lease.rs`, `procinfo.rs` (`lookup`, `process_state`, `process_start_ms`), and the tests `a_corrupt_or_empty_file_is_replaced`, `a_lease_of_a_dead_driver_is_stale_and_taken_over`, `other_owners_are_never_judged_by_pid`, `a_lease_of_this_process_is_held`, `force_release_removes_only_the_lease_of_that_job` and `the_pid_helpers_are_public` in `tests/project_lease.rs`.
- [ ] **Step 2: Write the failing tests** in `tests/project_lease.rs`. Add a helper `fn sleeper_owner() -> (std::process::Child, String, u64)` that spawns `sleep 60` and returns the child, the owner `format!("driver-{pid}-{secs}")` and `secs`, where `secs` comes from the new public `factory_engine::process_start_secs(pid: u32) -> Option<u64>` (Step 4 adds it as `process_start_ms(pid) / 1000`). Tests:
  - `an_expired_lease_of_a_live_owner_is_not_taken_over`: owner from the sleeper with its true start secs; `acquire_project_lease(dir, "001", &owner, 100.0, 1000.0, WAIT)`, then `acquire_project_lease(dir, "002", "b", 100.0, 5000.0, WAIT).expect_err("held")` matches `ProjectLeaseError::Held`; `project_lease_status(dir, 5000.0)` is `Held`. Kill and wait the sleeper at the end.
  - `a_lease_whose_pid_start_does_not_match_is_taken_over`: owner `driver-<sleeper pid>-<true secs + 3600>`; the second acquire at 1010.0 returns `LeaseTake::TookOverStale`.
  - `a_zombie_owner_is_dead` (Linux only, `#[cfg(target_os = "linux")]`): spawn `sh -c 'exit 0'`, do not wait, poll `procinfo` until zombie (up to 2 s), owner from its pid and `now_secs()`; `owner_state(&owner) == OwnerState::Dead`; then wait the child.
  - `an_owner_without_a_pid_still_expires`: owners `"tester"`, `"driver-x-1"`: `owner_state` is `Unknown`, `lease_held(owner, 1010.0, 1000.0)` is false and `lease_held(owner, 990.0, 1000.0)` is true.
  - `an_owner_state_table` (one full example, the others by input): `owner_state(&format!("driver-{}-1", std::process::id()))` is `Alive`; `DEAD_OWNER` is `Dead` on Linux; `"driver-4294967000"` is `Unknown`.
  - `a_corrupt_project_lease_is_refused_and_untouched`: for bytes `b"\x00\xff\x13garbage"` and `b""` write the lease file, call `acquire_project_lease(...)`, `expect_err` matches `ProjectLeaseError::Corrupt`, the message names the file path and contains `factory cancel`, and the file bytes are unchanged. This replaces `a_corrupt_or_empty_file_is_replaced`, which is deleted.
  - `a_forced_cancel_removes_a_corrupt_project_lease`: write a corrupt file, `force_release_project_lease(dir, "001", WAIT).expect("force")` is true and the file is gone.
  - `project_lease_status_reports_a_live_expired_owner_as_held`: same lease as the first test, `project_lease_status(dir, 5000.0)` is `ProjectLeaseStatus::Held { .. }`.
  - In `tests/lock_lease.rs`, change the last assertion of `a_live_owner_lease_stays_live` from `!lease_is_live(&job, 2600.0)` to `lease_is_live(&job, 2600.0)`, and its name to `a_live_owner_lease_stays_live_past_expiry`.
  - In `crates/factory-supervisor/tests/recovery.rs`, in `a_live_lease_owner_without_a_child_record_is_an_orphan_until_it_ends`, build the owner as `format!("driver-{}-{}", stranger.id(), factory_engine::process_start_secs(stranger.id()).expect("start"))` so the sleeper is `Alive`.
  - Change `the_pid_helpers_are_public` to assert `owner_state` as in the table and drop the `lease_owner_dead` and `process_gone` lines.
  - In `lease.rs` add a unit test `lease_is_live_for_an_expired_lease_of_a_live_owner`: a `Job` whose `lease` is `Lease { owner: "driver-<own pid>-1", expires: 1.0 }` is live at `now = 1000.0`; build the job as the tests in `crates/factory-engine/src/lease.rs`' neighbours do, or with `factory_core::Job::new` if no helper exists.
- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine --test project_lease`. Expected: compile errors `cannot find function owner_state`, `no variant Corrupt`, `cannot find value OwnerState`.
- [ ] **Step 4: Implement** the signatures above. `owner_state`: no pid gives `Unknown`; own pid gives `Alive`; `process_state` `Gone` or `Zombie` gives `Dead`; `Running` with no start gives `Unknown`; otherwise `Alive` when `start_ms / 1000` is within `START_SLACK_SECS` of the secs in the name, else `Dead`. `lease_held`: `Alive` true, `Dead` false, `Unknown` is `expires > now`. Use `lease_held` in `lease_is_live`, in `acquire_project_lease` (`Ok(Some(l)) if !lease_held(...)` takes over, otherwise `Held`), in `release_project_lease` and in `project_lease_status`. In `acquire_project_lease` the `Err(reason)` arm returns `Corrupt` with the lease path and writes nothing; its message ends with `delete the file or run factory cancel <id> --force`. `force_release_project_lease` removes the file when `read_lease` errors, and returns true. Export `OwnerState`, `owner_state`, `lease_held`, `START_SLACK_SECS`, `process_start_secs` from `lib.rs`.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine --test project_lease` then `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli` and `cargo test -j4 -p factory-supervisor`. Expected: all pass, 0 failed. Then `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings` and `cargo fmt -p factory-engine -- --check`.

### Task 2: Stop a lost run before an agent and before the merge

**Files:**
- Modify: `crates/factory-engine/src/lib.rs` (`Engine`), `crates/factory-engine/src/compat.rs` (`with_engine`, ~line 548), `crates/factory-engine/src/ports.rs` (`run_agent`, line 250), `crates/factory-engine/src/merge.rs` (before `merge_no_ff`, line ~352), `crates/factory-engine/src/runloop.rs` (line 713)
- Test: `crates/factory-engine/tests/build_checks.rs`, `crates/factory-engine/tests/lease_heartbeat.rs`

**Interfaces:**
- Consumes: `Driver::lease_lost()` (`compat.rs:362`), `Driver.lost: Arc<AtomicBool>`.
- Produces: `Engine.lost: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>`, `Engine::with_lost(self, flag: Arc<AtomicBool>) -> Engine<'a>`, `Engine::lost(&self) -> bool`, and `pub(crate) fn lease_lost_error() -> EngineError` (the text of `runloop.rs:713`) used by `runloop.rs`, `ports.rs` and `merge.rs`.

- [ ] **Step 1: Read** `Engine` and `Engine::new` in `lib.rs`, `with_engine` in `compat.rs`, `run_agent` in `ports.rs`, `try_merge` in `merge.rs`, the test `another_owner_in_the_lease_sets_lost_and_stops_the_run` in `tests/lease_heartbeat.rs`, and `committing_fixture`, `scripted` and `fx.engine(&runner)` as `a_rework_note_is_the_findings_of_the_resume_and_is_removed_after` uses them in `tests/build_checks.rs`.
- [ ] **Step 2: Write the failing tests.**
  - `tests/build_checks.rs`: `a_lost_engine_starts_no_agent`. Build `fx = committing_fixture()`, `runner = scripted(vec![])`, `flag = Arc::new(AtomicBool::new(true))`, `e = fx.engine(&runner).with_lost(flag.clone())`. Assert `e.build_stage().expect_err("lost").to_string().contains("(the lease was taken over)")` and `runner.calls.lock().expect("calls").len() == 0`. Then `e.review_stage()` fails the same way with zero calls. A second test `an_engine_without_a_flag_is_never_lost` asserts `fx.engine(&runner).lost()` is false.
  - `tests/lease_heartbeat.rs`: `a_lost_lease_stops_before_the_merge_commit`. Build the fixture as `another_owner_in_the_lease_sets_lost_and_stops_the_run` does, but the hook fires on the merge message run (`spec.role` contains `"message"` or `"committer"` after the job reached approve; use a counter of runs per role and fire on the last `committer` run, found by reading which role `Driver::commit_message` passes). The hook overwrites the lease file with owner `"other"` and sleeps 1 s. Assert `run_it` errors with `(the lease was taken over)` and that `fx.main`'s `main` branch tip equals its tip before the run (the job branch is not merged), read with `git2::Repository::open(&fx.main)`.
- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine --test build_checks a_lost` and `cargo test -j4 -p factory-engine --test lease_heartbeat a_lost`. Expected: `no method named with_lost found`, then (after a stub) the merge test fails with `merged` or a missing error.
- [ ] **Step 4: Implement** the signatures above. `Engine::new` sets `lost: None`; `with_engine` calls `.with_lost(self.lost.clone())`. `run_agent` returns `Err(lease_lost_error())` before `sync_provider` when `self.lost()` is true. In `merge.rs` add, right after the cancelled check and before `merge_no_ff`, `if driver.lease_lost() { return Err(lease_lost_error()); }`. `runloop.rs:713` calls `lease_lost_error()` instead of the inline text; the text stays byte for byte the same.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`, then `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings` and `cargo fmt -p factory-engine -- --check`. Expected: all pass.

Next plan: docs/superpowers/plans/2026-11-15-live-driver-safety/plan-02-live-driver-safety.md
