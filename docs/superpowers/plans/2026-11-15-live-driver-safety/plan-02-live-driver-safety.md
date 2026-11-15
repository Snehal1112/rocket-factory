# Live driver safety: the factory-procgroup crate (plan 2 of 3)

> **Spec:** docs/superpowers/specs/2026-11-15-live-driver-safety-design.md
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** A new crate that spawns children in their own process group, kills the group on drop and on a shutdown signal, records children in a registry file, and reaps the leftovers of a dead owner.

**Architecture:** `GroupChild` wraps `std::process::Child`, spawned with `process_group(0)`. A process-wide set holds the live groups. A registry file lists `{pid, start_ms, owner}`. The crate reads no process facts: callers pass `start_of` and `running`, so `procinfo.rs` stays the only reader.

**Tech Stack:** Rust, `libc` (`killpg`, `getpgid`, `kill`), a signal crate. Unix only, other systems get plain spawns and no-ops.

## Global Constraints

- New crate `crates/factory-procgroup`, added to the root `Cargo.toml` workspace members. It depends on no factory crate.
- `libc` is pinned with `=` to the version in `Cargo.lock` (0.2.191). The signal crate (`signal-hook`) is pinned with `=` to a version at least two weeks old: run `cargo info signal-hook` and pick the newest version published before that, never a range.
- No `/proc` read. The crate calls `libc` only for `killpg`, `getpgid`, `kill` and signal setup.
- Registry writes are atomic (write a temp file in the same folder, then rename). A failed registry write is ignored, never an error for the caller.
- Cargo: `-j4`, one crate at a time. Tests use `.expect("...")` and `expect_err`, never the unwrap call. Code comments are short full sentences that end with a period.

## Review Focus

1. A child that starts a grandchild: killing the group ends both. Pinned by `killing_a_group_child_ends_its_grandchild`.
2. A group already gone when killed (`ESRCH`): no error, no panic. Pinned by `killing_an_exited_child_is_quiet`.
3. A reused pid that is not a group leader or has another start time: the reaper leaves it alone. Pinned by `reap_leaves_a_start_time_mismatch_alone` and `reap_leaves_a_non_leader_alone`.
4. A missing, empty or corrupt registry file: the reaper kills nothing and says so. Pinned by `reap_with_a_missing_or_corrupt_registry_kills_nothing`.
5. Two children spawned at once from two threads: both are recorded. Pinned by `two_threads_spawning_both_reach_the_registry`.

## File Structure

- `crates/factory-procgroup/Cargo.toml`, `src/lib.rs` (spawn, `GroupChild`, groups set, `kill_all`), `src/registry.rs` (file format, atomic rewrite, `reap_leftovers`), `src/shutdown.rs` (`install_shutdown_handler`), `src/bin/shutdown-demo.rs` (test helper binary).
- Tests: `crates/factory-procgroup/tests/group.rs`, `tests/reap.rs`, `tests/shutdown.rs`.
- Root `Cargo.toml` members.

---

### Task 1: Spawn in a group, kill the group, keep the registry

**Files:**
- Create: `crates/factory-procgroup/Cargo.toml`, `src/lib.rs`, `src/registry.rs`
- Modify: root `Cargo.toml` (members)
- Test: `crates/factory-procgroup/tests/group.rs`

**Interfaces:**
- Consumes: nothing from other crates.
- Produces:
  - `pub type StartFn = fn(u32) -> Option<u64>;`
  - `pub struct GroupChild` with `Deref` and `DerefMut` to `std::process::Child`, `pub fn pgid(&self) -> i32`, `pub fn kill(&mut self) -> std::io::Result<()>` (SIGKILL to the group, `ESRCH` is `Ok`, removes the registry entry), and `Drop` that does the same.
  - `pub fn spawn(cmd: &mut std::process::Command) -> std::io::Result<GroupChild>`.
  - `pub fn set_registry(file: std::path::PathBuf, owner: String, start_of: StartFn)`.
  - `pub fn kill_all()`.
  - `pub(crate)` in `registry.rs`: `struct Entry { pid: u32, start_ms: Option<u64>, owner: String }` (serde), `fn read(file) -> Result<Vec<Entry>, String>`, `fn write(file, &[Entry])`.

- [ ] **Step 1: Read** `crates/factory-agent/src/child.rs` lines 85-130 and `crates/factory-checks/src/run.rs` lines 143-200 (the two spawn sites that Plan 3 moves onto this crate), and `crates/factory-engine/src/procinfo.rs` for the `start_ms` meaning (whole seconds, times 1000).
- [ ] **Step 2: Write the failing tests** in `tests/group.rs`; each uses `sleep` or `sh -c`:
  - `a_child_runs_in_its_own_group`: `spawn(Command::new("sleep").arg("30"))`; `libc::getpgid(child.id())` equals `child.id()` and differs from `libc::getpgid(0)`; `child.kill()`; `child.wait()`.
  - `killing_a_group_child_ends_its_grandchild`: `sh -c 'sleep 30 & echo $!; wait'` with stdout piped; read the grandchild pid from the first line; `child.kill()`; poll `libc::kill(pid, 0)` for up to 2 s until it returns -1 with `ESRCH`; assert it did.
  - `dropping_a_group_child_kills_it`: spawn `sleep 30`, remember the pid, drop the `GroupChild`, poll as above.
  - `killing_an_exited_child_is_quiet`: spawn `true`, `wait()`, then `child.kill().expect("quiet")`.
  - `the_registry_lists_a_spawned_child_and_drops_it_on_exit`: `set_registry(dir/"children.json", "driver-1-1".into(), |_| Some(5000))`; spawn `sleep 30`; the file holds exactly one entry with the child pid, owner `driver-1-1`, `start_ms` `Some(5000)`; after `child.kill()` the file holds an empty list.
  - `two_threads_spawning_both_reach_the_registry`: two threads each spawn `sleep 30` into one registry file; after both are spawned the file lists two pids; kill both.
  - `kill_all_ends_every_registered_group`: spawn two `sleep 30` children, `kill_all()`, both pids are gone within 2 s.
  Because the registry and group set are process-wide, give each test a distinct registry folder, and run the file with `-- --test-threads=1` in the commands below.
- [ ] **Step 3: Run** `cargo test -j4 -p factory-procgroup --test group -- --test-threads=1`. Expected: the crate does not exist yet, then `cannot find function spawn in this crate`.
- [ ] **Step 4: Implement** the signatures above. `spawn` uses `std::os::unix::process::CommandExt::process_group(0)` under `#[cfg(unix)]`; on other targets it spawns plainly and `GroupChild::kill` calls `Child::kill`. `GroupChild::kill` and `Drop` send `libc::killpg(pgid, SIGKILL)`, treat `ESRCH` as success, remove the pid from the group set and the registry entry, and never panic. The group set is a `Mutex<HashSet<i32>>`. The registry is a `Mutex<Option<(PathBuf, String, StartFn)>>`; adding or removing an entry reads the file (a missing or corrupt file counts as empty), changes the list and writes it atomically, all under the mutex. Add the crate to the root `Cargo.toml` members.
- [ ] **Step 5: Run** the command of Step 3. Expected: 7 passed, 0 failed. Then `cargo clippy -j4 -p factory-procgroup --all-targets -- -D warnings` and `cargo fmt -p factory-procgroup -- --check`.

### Task 2: The reaper and the shutdown handler

**Files:**
- Modify: `crates/factory-procgroup/src/registry.rs`, `src/lib.rs`, `Cargo.toml`
- Create: `crates/factory-procgroup/src/shutdown.rs`, `src/bin/shutdown-demo.rs`
- Test: `crates/factory-procgroup/tests/reap.rs`, `tests/shutdown.rs`

**Interfaces:**
- Consumes: The first task's `Entry`, `read`, `write`, `StartFn`, `kill_all`.
- Produces:
  - `pub fn reap_leftovers(file: &std::path::Path, dead_owner: &str, running: &dyn Fn(u32) -> bool, start_of: StartFn) -> Vec<String>`: handles only entries whose owner equals `dead_owner`; kills the group of an entry only when `running(pid)`, `libc::getpgid(pid) == pid` and `start_of(pid) == entry.start_ms` with both `Some`; removes every handled entry from the file; returns one line per entry beginning `killed` or `left alone`, and one line beginning `left alone: registry` when the file is unreadable (and then kills nothing).
  - `pub fn install_shutdown_handler()`: starts one thread (idempotent) that waits for SIGTERM, SIGINT and SIGHUP, calls `kill_all()`, and exits the process with `128 + signal`.
  - Test helper binary `shutdown-demo`: arguments `<registry file> <owner>`; it calls `set_registry` with a `start_of` that returns `Some(7000)`, installs the handler, spawns `sleep 60`, prints the child pid and a newline to stdout, flushes, then sleeps.

- [ ] **Step 1: Read** the first task's code and the signal crate's `Signals` iterator docs (`cargo doc` is not needed: `cargo info signal-hook` shows the version, then read `signal_hook::iterator::Signals`).
- [ ] **Step 2: Write the failing tests.**
  - `tests/reap.rs` (each test writes `children.json` by hand with `serde_json` and uses a real `sleep 60` spawned through `spawn` with its own group):
    - `reap_kills_a_matching_leftover`: entry `{pid, start_ms: Some(5000), owner: "dead"}`, `start_of = |_| Some(5000)`, `running = |_| true`; the group is gone after the call, the line starts with `killed`, the file lists no entry for `dead`.
    - `reap_leaves_a_start_time_mismatch_alone`: `start_of = |_| Some(9999)`; the sleeper still runs, the line starts with `left alone`.
    - `reap_kills_nothing_when_the_start_time_is_unknown`: `start_of = |_| None`; the sleeper still runs.
    - `reap_leaves_a_non_leader_alone`: the entry names a pid that is not its own group leader (a child of a `sh` started without a group: use `sh -c 'sleep 60 & echo $!; wait'` spawned plainly through `std::process::Command`, entry pid is the printed grandchild pid); it still runs afterwards.
    - `reap_ignores_entries_of_other_owners`: an entry of owner `live` stays in the file and its sleeper runs.
    - `reap_with_a_missing_or_corrupt_registry_kills_nothing`: a missing file, then a file with `not json`; both return one line starting `left alone: registry`, and the corrupt file is left on disk.
  - `tests/shutdown.rs`, using `env!("CARGO_BIN_EXE_shutdown-demo")`:
    - `sigterm_to_the_driver_ends_its_child`: spawn the demo with stdout piped, read the child pid line, `libc::kill(demo, SIGTERM)`, `wait()`: exit status code is 143 (`128 + 15`), and the child pid is gone within 2 s.
    - `sigkill_then_reap_ends_the_child`: spawn the demo, read the child pid, `libc::kill(demo, SIGKILL)`, `wait()`; the child still runs; then `reap_leftovers(file, owner, &|_| true, |_| Some(7000))` returns a line starting `killed`, and the child pid is gone within 2 s.
- [ ] **Step 3: Run** `cargo test -j4 -p factory-procgroup --test reap --test shutdown -- --test-threads=1`. Expected: `cannot find function reap_leftovers`, then `cannot find function install_shutdown_handler`.
- [ ] **Step 4: Implement** the signatures above. `reap_leftovers` follows the rules in its Produces line. `ESRCH` while killing counts as already gone. `install_shutdown_handler` uses `signal_hook::iterator::Signals::new([SIGTERM, SIGINT, SIGHUP])` on a named thread `procgroup-shutdown`, guarded by a `std::sync::Once`. Add the pinned `libc` and signal crate to `Cargo.toml`, with `[[bin]] name = "shutdown-demo"` pointing at the demo.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-procgroup -- --test-threads=1`. Expected: 15 passed (7 + 6 + 2), 0 failed. Then `cargo clippy -j4 -p factory-procgroup --all-targets -- -D warnings` and `cargo fmt -p factory-procgroup -- --check`.

Next plan: docs/superpowers/plans/2026-11-15-live-driver-safety/plan-03-live-driver-safety.md
