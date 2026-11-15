# Live driver safety design

## Goal

A driver whose process is alive keeps its lease even when the lease has expired, a corrupt project lease is never taken over silently, a run that lost its lease stops before it starts an agent or merges, and the agent and check children of a driver end when the driver is killed.

## Overview

Issue #87 (audit of 2026-11-15, state integrity) names four holes. All were re-read in the code:

- Expiry alone lets a second driver take a live driver's lease. `acquire_project_lease` takes over when `l.expires <= now` whatever the owner process does (`project_lease.rs:112`). The TTL is 1800 s and the heartbeat is 60 s (`compat.rs`, `LEASE_TTL_SECS`, `HEARTBEAT_SECS`). A laptop sleep or a clock jump over 30 minutes expires a live lease, so two drivers can write to one worktree.
- Liveness is the pid alone. `lease_owner_dead` asks `procinfo::pid_exists` (`project_lease.rs:75`), which counts a zombie as alive and never compares the start time in the owner name with the process. A reused pid keeps a dead lease looking live until the TTL ends. `procinfo::process_start_ms` and `pid_is_zombie` exist and nothing outside `procinfo.rs` uses them.
- A corrupt lease file is taken over as owner `unknown` (`project_lease.rs:124`).
- Children are plain `Command::spawn` calls without a process group: `claude -p` (`factory-agent/src/child.rs:93`) and check commands (`factory-checks/src/run.rs:152`). Only `child.kill()` on a stop or a timeout ends them. After SIGKILL or SIGTERM of the driver the agent keeps running in the worktree, and a new driver may start on it at once because the owner pid is dead.

The run loop checks `guard.lost()` once per stage (`runloop.rs:712`). The agent runs and the merge commit do not check it.

## Current state

| Piece | Today | Gap |
|---|---|---|
| Job lease live test (`lease.rs:16`) | `expires > now && !lease_owner_dead(owner)` | An expired lease of a live owner is free |
| Project lease takeover (`project_lease.rs:99-128`) | free, ours, `expires <= now`, owner dead or corrupt | Same, and corrupt takes over |
| Owner liveness (`project_lease.rs:61-77`) | owner `driver-<pid>-<secs>`, `process_gone(pid)` | No start time, a zombie is alive |
| Status readers (`project_lease_status`, `project_lease.rs:187`; supervisor `supervisor.rs:477`) | use the same rule | Change with it |
| Forced cancel (`commands.rs:846-887`, `force_release_project_lease` `project_lease.rs:170`) | removes a lease it can parse and that names the job | A corrupt file stays, and the cancel gives up the job |
| Release (`project_lease.rs:140-151`) | removes the file when it is ours, expired or corrupt | A failed run deletes a corrupt file without anyone seeing it |
| Lost checks | loop top only (`runloop.rs:712`) | Not before an agent run, not before `merge_no_ff` (`merge.rs:357`) |
| Children | no group, no handler, no record | Survive a killed driver |

## Non-goals

- Making force-cancel stop a live driver's agent: issue #113. It reuses the group kill built here.
- A monotonic clock for the lease age: with the new rule a live owner keeps its lease whatever the wall clock does.
- Windows. Linux and macOS only.
- Leases shared between machines. The owner name holds no host name, so a pid is always read on this machine.

## Design

Flow: a driver starts, takes the project lease, and registers every child it spawns in `children.json`. Another driver reads the lease: live owner means refuse, dead owner means take over and reap the leftovers of that owner. A killed driver's children die with the driver's signal handler, or with the next driver's reaper.

### 1. One liveness rule

New in `project_lease.rs`:

```rust
pub enum OwnerState { Alive, Dead, Unknown }
pub fn owner_state(owner: &str) -> OwnerState;
pub fn lease_held(owner: &str, expires: f64, now: f64) -> bool;
```

`owner_state`: no pid in the name gives `Unknown`. The own pid gives `Alive`. `procinfo` says `Gone` or `Zombie` gives `Dead`. `Running` with no known start time gives `Unknown`. `Running` with a start time gives `Alive` when `|start_ms / 1000 - secs| <= START_SLACK_SECS` (10) and `Dead` otherwise, because the pid was reused. `lease_held`: `Alive` is true, `Dead` is false, `Unknown` is `expires > now`. `lease_owner_dead` and `process_gone` are replaced by these, with the same callers: `lease_is_live` (`lease.rs:16`), `acquire_project_lease`, `release_project_lease`, `project_lease_status`. Owners that are not of the form `driver-<pid>-<secs>`, such as test owners, keep today's rule through `Unknown`.

Example: owner `driver-4242-1000`, pid 4242 runs and started at 1003 s: held although `expires` passed. Pid 4242 started at 2000 s: dead, taken over at once.

A hung driver that is alive keeps the lease for good. `factory cancel <id> --force` clears it, as today.

### 2. A corrupt project lease

`acquire_project_lease` returns a new `ProjectLeaseError::Corrupt { path, reason }` for an unreadable or invalid file and writes nothing. The message names the file and says to delete it. A person clears it by deleting the file, as the existing `Held` message already says. `factory cancel <id> --force` also removes a corrupt file (`force_release_project_lease` returns true), but it first marks that job needs-human and cancelled (`commands.rs:850-867`), so the message and the docs say plainly that this gives up the named job. `project_lease_status` keeps `Unreadable`. The holder is not special: if the file turns corrupt while a driver runs, the heartbeat sets the lost flag with the reason `Corrupt`, and the run stops with the `Corrupt` message at the next lost check (`runloop.rs:712`, `Engine::run_agent`, or the merge). `release_project_lease` changes: today it removes a corrupt file (`project_lease.rs:140,151`, `Err(_) => true`); it will leave it in place (`Err(_) => false`), so a failed run does not delete it. Only `force_release_project_lease` removes a corrupt file.

### 3. Lost checks before an agent and before the merge

The driver's lost flag becomes a shared `LostFlag` that holds the reason (`TakenOver`, or `Corrupt` with the message), and `LostFlag::error()` builds the one error text: the `runloop.rs:713` text for `TakenOver` and the `Corrupt` message otherwise. `Engine` (`lib.rs:166`) gets an optional `lost: Option<Arc<LostFlag>>`, set by `Driver::with_engine` (`compat.rs:~548`) from the driver's flag, and `Engine::with_lost` and `Engine::lost()` to match. `Engine::run_agent` (`ports.rs:250`) returns `EngineError::Msg` first thing when `self.lost()` holds, so no agent starts. `merge.rs` does the same next to the cancel check, right before `merge_no_ff` (`merge.rs:352-357`). The run loop, `run_agent` and the merge all return `LostFlag::error()`.

### 4. Process groups, registry, reaper and handler

A new crate `factory-procgroup` (deps: `libc`, a signal crate; both pinned exactly, see Constraints). Unix only, other systems get plain spawns.

```rust
pub fn spawn(cmd: &mut std::process::Command) -> std::io::Result<GroupChild>;
pub type StartFn = fn(u32) -> Option<u64>;
pub fn set_registry(file: std::path::PathBuf, owner: String, start_of: StartFn);
pub fn kill_all();
pub fn install_shutdown_handler();
pub fn reap_leftovers(
    file: &std::path::Path,
    is_dead: &dyn Fn(&str) -> bool,
    running: &dyn Fn(u32) -> bool,
    start_of: StartFn,
) -> Vec<String>;
```

- `spawn` sets `process_group(0)`, spawns, adds the child to an in-process set of groups, and writes `{pid, start_ms, owner}` into the registry file (an atomic rewrite under a mutex). `GroupChild` gives access to `std::process::Child` (stdin, stdout, `try_wait`, `wait`). `GroupChild::kill` and its drop send SIGKILL to the whole group (`killpg`) and remove the entry. Both spawn sites above use it.
- The crate reads no process facts itself. The engine passes `procinfo::process_start_ms` as `start_of` and `procinfo::pid_exists`-based liveness as `running`, so `procinfo.rs` stays the only place that reads them.
- `set_registry` is called once in `main.rs`, right after `main.rs` has built the context in either way (`CliCtx::for_project` with `--project`, `CliCtx::from_env` without), so every command that builds a driver is covered (`run`, `intake`, `approve`, `sync`, KB refresh). It gets the path from `Layout::children_file()` (`children.json` in the state folder), the driver's owner and `procinfo::process_start_ms`. Without a registry the spawn still groups and kills, and records nothing. Tests that build a driver do not set one.
- `install_shutdown_handler` starts a thread that waits for SIGTERM, SIGINT and SIGHUP, calls `kill_all`, and exits with `128 + signal`. `main.rs` calls it next to `set_registry`. A killed driver cannot run it, which is what the reaper is for.
- `reap_leftovers` runs in `Driver::take_project` when the take is `Fresh` or `TookOverStale { owner, .. }`. It reads the registry and handles only entries whose owner `is_dead` accepts. The driver passes `|o| o == taken_owner || owner_state(o) == OwnerState::Dead`, with no taken owner on a `Fresh` take or when the taken owner is `unknown`. So a forced cancel, which deletes the lease file and makes the next take `Fresh`, still gets its leftovers reaped. An entry's group is killed only when all hold: the pid runs, `getpgid(pid) == pid`, and `procinfo::process_start_ms(pid)` equals `start_ms` to the second. An unknown start time kills nothing. Each handled entry is removed and one line comes back (killed, left alone with the reason). The driver writes the lines to the ledger, the events log and stderr, and never fails the lease take because of them.

### 5. Other readers

The supervisor adopts children and the CLI shows status through `project_lease_status` and `lease_is_live`, so they follow section 1 without a code change, with one exception: `supervisor.rs:476-482` maps every status except `Held` to `Ready`, so an `Unreadable` (corrupt) lease would make factoryd start runs that are refused. It maps `Unreadable` to `ProjectState::LeaseHeld { owner: "unreadable lease file" }` instead, so queued runs wait until a person clears the file (`factory cancel <id> --force`). The name has no pid, so it is never reported as an orphan. Three existing tests encode the old rule and change with it. The third is `a_corrupt_lease_file_is_reported_and_replaced` (`factory-cli/tests/project_lease_cli.rs:131-142`): it becomes `a_corrupt_lease_file_is_refused_until_cancel_force` and expects `run 1` to fail with the `Corrupt` message and leave the file, then `cancel 1 --force` and `run 1` to merge. The other two are `a_live_owner_lease_stays_live` (`factory-engine/tests/lock_lease.rs:128-139`) ends with `lease_is_live(&job, 2600.0)` being true, because its owner is the test process, which is `Alive`; and `a_live_lease_owner_without_a_child_record_is_an_orphan_until_it_ends` (`factory-supervisor/tests/recovery.rs:429-456`) builds its owner from the sleeper's pid and its real start secs, so the sleeper reads as `Alive`. Other tests that build `driver-<own pid>-1` stay `Alive`.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| Lease past TTL, owner `Alive` | Held, `ProjectLeaseError::Held`, `secs_left` is 0 |
| Pid reused, start time off by more than 10 s | Dead, taken over |
| Owner zombie | Dead |
| Start time unknown on this system | `Unknown`, expiry decides |
| Owner name without a pid | `Unknown`, expiry decides |
| Project lease file not valid JSON | `Corrupt`, run refused, nothing written |
| Forced cancel with a corrupt file | File removed, `project lease released`, and the named job is cancelled |
| File turns corrupt while a driver runs | Heartbeat sets lost with reason `Corrupt`, the run stops at the next lost check with the `Corrupt` message, the file is left |
| Queued run in factoryd while the lease file is corrupt | The project is `LeaseHeld`, the run waits |
| `lost()` set before an agent | `EngineError::Msg`, no agent starts |
| `lost()` set before `merge_no_ff` | Same error, the branch is not merged |
| SIGTERM, SIGINT or SIGHUP to the driver | Groups killed, exit `128 + signal` |
| SIGKILL of the driver | Children live until the next driver reaps them |
| Reaper: pid reused or not group leader | Left alone, line written |
| Reaper: registry missing or unreadable | Nothing killed, line written |
| Forced cancel, then a fresh take | Reaper still runs for owners that are dead |
| Driver commits and check runs after a lost lease | Not guarded. The run ends at the next agent start or at the merge check (accepted, see Decisions) |
| `killpg` returns ESRCH | Treated as already gone |
| Non-Unix system | Plain spawn, no handler, no reaper |

## Testing

- `factory-engine/tests/project_lease.rs`: `an_expired_lease_of_a_live_owner_is_not_taken_over` (owner built from a running `sleep` and its start time), `a_lease_whose_pid_start_does_not_match_is_taken_over`, `a_zombie_owner_is_dead`, `an_owner_without_a_pid_still_expires`, `a_corrupt_project_lease_is_refused_and_untouched`, `a_forced_cancel_removes_a_corrupt_project_lease`, `project_lease_status_reports_a_live_expired_owner_as_held`, and in `lease_heartbeat.rs` `a_corrupt_lease_during_a_run_stops_the_run`, `release_leaves_a_corrupt_file_in_place` in `project_lease.rs`, and in `factory-supervisor/tests/supervisor.rs` `an_unreadable_lease_makes_queued_runs_wait`. Update the existing `lease_owner_dead` assertions (`project_lease.rs:442-447`) to `owner_state`.
- `factory-engine/tests/lease.rs` or the existing lease tests: `lease_is_live_for_an_expired_lease_of_a_live_owner`.
- `factory-engine/tests/build_checks.rs`: `a_lost_engine_starts_no_agent` (`Engine::with_lost` set, `build_stage` fails, the scripted runner has zero calls). `factory-engine/tests/lease_heartbeat.rs`, next to `another_owner_in_the_lease_sets_lost_and_stops_the_run` (line 142): `a_lost_lease_stops_before_the_merge_commit` (branch not merged).
- `factory-procgroup` unit tests: `a_child_runs_in_its_own_group`, `killing_a_group_child_ends_its_grandchild` (a shell that starts `sleep`), `dropping_a_group_child_kills_it`, `the_registry_lists_a_spawned_child_and_drops_it_on_exit`, `a_fresh_take_reaps_a_dead_owners_group`, `reap_kills_a_matching_leftover`, `reap_leaves_a_start_time_mismatch_alone`, `reap_kills_nothing_when_the_start_time_is_unknown`.
- `factory-procgroup/tests/shutdown.rs`, with a small demo binary that installs the handler and spawns `sleep`: `sigterm_to_the_driver_ends_its_child` and `sigkill_then_reap_ends_the_child`. They replace the `fake-claude` test of the issue, because the new crate cannot depend on `factory-agent`. `factory-cli` gets `guard_children_registers_children` and `main_guards_children_for_every_project_command`. `factory-agent/tests/runner.rs` gets `an_agent_child_is_a_group_leader` (a script that prints its pid and process group).
- `factory-cli/tests/no_hardcoded_paths.rs` and `agent_lint.rs` stay green.
- Commands: `cargo test -j4 -p factory-procgroup`, `-p factory-agent`, `-p factory-checks`, `-p factory-engine`, `-p factory-cli`, `-p factory-supervisor`. Then `cargo clippy -j4 -p <crate> --all-targets -- -D warnings` and `cargo fmt -p <crate> -- --check` for each.

## Files changed

- `crates/factory-engine/src/project_lease.rs`, `lease.rs`, `lib.rs`, `compat.rs`, `ports.rs`, `merge.rs`, `runloop.rs`
- `crates/factory-supervisor/src/supervisor.rs`
- `crates/factory-core/src/layout.rs` (`Layout::children_file`)
- `crates/factory-agent/src/child.rs`, `crates/factory-checks/src/run.rs`
- `crates/factory-procgroup/` (new crate), the root `Cargo.toml` workspace members, `crates/factory-agent/Cargo.toml`, `crates/factory-checks/Cargo.toml`, `crates/factory-engine/Cargo.toml`, `crates/factory-cli/Cargo.toml`
- `crates/factory-cli/src/main.rs` and `commands.rs` (`guard_children` call, corrupt message)
- Tests above, `CLAUDE.md` crate layering list, `docs/reference/software-factory.md` (leases and children), `README.md` lease section

## Constraints

- No new `/proc` read. Process facts come from `procinfo.rs` only. The new crate may call `libc` for `killpg` and `getpgid` and nothing else about other processes.
- Both new dependencies are pinned with `=` to a version at least two weeks old. `libc` uses the version in `Cargo.lock` (0.2.191).
- Board JSON is unchanged. `children.json` is a new file in the state folder, read only by the driver.
- A failed registry write, a failed kill or a reaper line never changes a lease or merge result.
- Paths go through `Layout`. Never shell out to `git`.

## Acceptance criteria

- An expired lease whose owner pid runs, is not a zombie and matches the start time is not taken over.
- A lease whose pid is gone, a zombie, or reused with another start time is taken over at once.
- An owner without a pid, or a system without start times, keeps today's expiry rule.
- A corrupt project lease is refused and left on disk, a failed run does not delete it, and deleting the file clears it. `factory cancel <id> --force` also removes it and says it gives up that job.
- A driver whose lease was lost starts no agent and does not run `merge_no_ff`.
- Agent and check children run in their own process group, and the whole group ends on drop and on SIGTERM, SIGINT and SIGHUP of the driver.
- After a SIGKILL of the driver, the next driver that takes over its lease kills the registered group when pid, group leader and start time match, and kills nothing otherwise.
- `no_hardcoded_paths.rs` passes, no `/proc` read is added, and the crate tests and clippy are clean.

## Out of scope

- Stopping a live driver's agent on `cancel --force` (#113).
- Leases across machines or containers.
- Killing grandchildren that left the process group on their own.

## Follow-ups

- Stop a live driver's agent and check processes when a job is force-cancelled, using the group kill (issue #113, already filed).

## Decisions

- What does a running driver do when its own project lease file turns corrupt? Options: stop the run with a clear error, rewrite its own lease. Chosen: stop the run with a clear error. Decided by: user
- What does this spec cover? Options: all four problems with #113 separate, lease only, all four plus #113. Chosen: all four problems, #113 separate. Decided by: user
- How does a second driver tell that an owner is alive? Options: pid, no zombie and matching start time; pid only; alive plus a hard cap. Chosen: pid runs, no zombie, start matches the name, unknown falls back to expiry. Decided by: user
- What happens to a corrupt project lease? Options: refuse and let cancel --force clear it; refuse and delete by hand; keep the takeover with a warning. Chosen: refuse and let cancel --force clear it. Decided by: user
- How do children end when the driver is killed? Options: group, guard, signal handler and reaper; group, guard and handler; group and Linux PDEATHSIG. Chosen: group, guard, handler and reaper. Decided by: user
- What must hold before the reaper kills, and what if the start time is unknown? Options: verified match else skip; also kill when unknown; never kill. Chosen: kill only on a verified match, else skip. Decided by: user
- Is the lease age measured on a monotonic clock? Options: monotonic clock, wall clock with the liveness rule. Chosen: no monotonic clock, the liveness rule makes the wall clock irrelevant for a live owner. Decided by: user
- Where do the process-group helpers live? Chosen: a new crate `factory-procgroup`, because `factory-agent` and `factory-checks` both need it and neither depends on the other. Decided by: call
- How far may a start time differ from the owner name? Options: 10 seconds, 60 seconds, 3 seconds. Chosen: 10 seconds, because `env_owner` reads the clock a moment after the process starts and `sysinfo` reports whole seconds. Decided by: user
- Where does the registry live? Options: one file per project, one file per job folder. Chosen: `children.json` in the state folder through `Layout`, one file per project because one driver holds the project lease at a time. Decided by: user
- Where is the lost check added? Options: agent starts and the merge only, also commits and checks. Chosen: in `Engine::run_agent` and before `merge_no_ff`, because the loop top already checks once per stage; the driver's own commits and check runs are not guarded. Decided by: user
- Is the heartbeat killed mid-stage when the lease is lost? Options: stop at the next check, kill the group at once. Chosen: no, the next agent or merge check stops the run. Decided by: user
- What does factoryd do with a project whose lease file is corrupt? Options: queue the run as held until cleared, let each run start and fail. Chosen: queue it as held, so runs wait until a person clears the file. Decided by: user
- How does a person clear a corrupt project lease? Options: delete the file by hand and let cancel --force also work, add `factory lease clear`, only cancel --force. Chosen: delete the file by hand, and cancel --force also works and says it gives up the job. Decided by: user
