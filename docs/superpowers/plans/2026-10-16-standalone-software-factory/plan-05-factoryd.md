# factoryd, the Local Supervisor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A local supervisor `factoryd` that takes run and approve requests from a queue, starts the existing `factory` CLI as separate processes (at most N in parallel, at most one per project), keeps jobs running when it stops, and re-adopts or parks them when it starts again.

**Architecture:** A new library crate `factory-supervisor` holds the request queue, the runtime records, a pure scheduler and the supervisor loop. A `Spawner` trait hides process creation, so the loop is tested with a fake and with a shell script standing in for `factory`. The real spawner starts `factory --project <name> run <job>` in its own process group with stdout and stderr in log files. The `factoryd` binary has `run`, `status`, `stop` and `enqueue`. Plan 6 puts an HTTP API in front of the same library functions. A lease heartbeat in the driver keeps long runs from losing the project lease.

**Tech Stack:** Rust 2021 (cargo 1.94), serde and toml (already in the workspace), fs2 (instance lock), std `Command` with `process_group`. No new third-party crate unless `cargo build --offline` shows it is already in the registry.

**Spec:** `docs/superpowers/specs/2026-10-16-standalone-software-factory-design.md` (Components: `factoryd`; Error handling: crash recovery; Acceptance criteria: two projects at once, killing `factoryd` does not stop the job).

## Global Constraints

- Work in `/home/numericlabs/data/rocket/rocket-factory`, on a branch `plan-05-factoryd` created from `main`. Fast-forward `main` at the end.
- Every `cargo` command has `-j4`. Never `--workspace` or `--all`. Never set `CARGO_TARGET_DIR`. Test one crate at a time with `-p <crate>`.
- Do not push anywhere. The repo has no remote.
- Commits use conventional commits and are written through the `dev-workflow-skills:1-git-commit` skill. Stage explicit paths and commit with a pathspec, never `git add -A`, `--all` or `.`.
- No unwrap calls in production paths (tests may use `expect`). No shelling out to `git`.
- Do not edit the byte copies of Rocket files under `.claude/`. The `dashboard/` crate is untouched. Board JSON, `events.jsonl` and `costs.json` formats do not change.
- The guard test `crates/factory-cli/tests/no_hardcoded_paths.rs` scans the `src` folders of the crates it lists; if the new crate must be scanned, add it to the guard's crate list, but never widen the allowlist. Home sub-folder names live in one place (`factory-core/src/home.rs`).
- `factoryd` never kills a job it did not start in the same run, never kills children when it stops, and never restarts a job on its own after a crash. A dead child is recorded as `Lost` and the job stays where it was, for a human.
- Request files are untrusted input (same-user writable folder): every field is validated before it reaches a command line. Project names use the registry rule, job ids are digits only, unknown fields are refused.
- Unix only for process control (`process_group`, `/proc` liveness). Non-unix builds must still compile; the supervisor's process functions return an `Unsupported` error there.
- Existing behavior of the `factory` CLI without the supervisor is unchanged, except the lease heartbeat and the `kb-refresh` lease in Task 3.

## Review Focus

- Two requests for one project start two children at once, or a request starts while a human holds the project lease from a terminal: exactly one runs, the other waits in the queue and starts later.
- `factoryd` restarts while a child runs: the child must not be killed, must not be started a second time, and must be recorded with its real outcome when it ends.
- A child dies without a result (kill -9): the request becomes `Lost` with the reason, the job is not restarted, the next queued request of that project can start once the lease is stale or the owner process is gone.
- A request file with a bad project name, a path in the job id, a negative or huge number, extra fields, or invalid JSON must be rejected without starting anything and must not wedge the queue.
- A disabled or blocked project (repo missing, degraded config): its requests fail with the reason and the queue keeps flowing for other projects.
- Two `factoryd run` instances: the second refuses to start.

---

### Task 1: The supervisor library: config, queue, records, scheduler

**Files:**
- Create: `crates/factory-supervisor/Cargo.toml`, `src/lib.rs`, `src/request.rs`, `src/queue.rs`, `src/records.rs`, `src/scheduler.rs`
- Modify: root `Cargo.toml` (workspace member), `crates/factory-core/src/home.rs` (folder and file names, `config_file()`), `crates/factory-core/src/lib.rs` and a new `crates/factory-core/src/homeconfig.rs` (`HomeConfig`)
- Test: unit tests in each new file

**Interfaces:**
- Consumes: `Home`, `project::{list, get}`, `write_atomic` (factory-core `fsutil`), `Layout`, `project_lease_status` (factory-engine).
- Produces, used by tasks 2 and 3 and by plan 6:
  - `Home::config_file() -> PathBuf` (`<dir>/config.toml`), `queue_dir()`, `results_dir()`, `run_dir()`, `logs_dir(project)`.
  - `HomeConfig { pub max_parallel: u32 }` (default 2, minimum 1, `deny_unknown_fields`, serde defaults), `HomeConfig::load(home) -> Result<HomeConfig, CoreError>` (a missing file is the default; an invalid file is an error), plan 6 and 7 add keys.
  - `factory_supervisor::Request { id: String, project: String, action: Action, job: String, series: bool, ts_ms: u64 }`, `Action::{Run, Approve}` (`series` is only valid for `Run`), `Request::validate(&self) -> Result<(), RequestError>` (project name via `project::validate_name`, job is 1 to 6 ASCII digits, id is `<ts_ms>-<project>-<job>` shaped and path-safe), `Request::new(project, action, job, series, now_ms) -> Result<Request, RequestError>`.
  - `queue::{enqueue(home, &Request) -> Result<(), QueueError>, list(home) -> Vec<QueueItem>, remove(home, id)}`; `QueueItem` is `Ok(Request)` or `Bad { file: String, reason: String }`; `list` sorts by `ts_ms` then id and never fails because of one bad file.
  - `records::{ChildRecord { pid: u32, project: String, job: String, request_id: String, started_ms: u64, out: PathBuf, err: PathBuf }, write_child, read_children, remove_child}` stored at `run/children/<project>.json`; `records::{Outcome, ResultRecord, write_result, read_results}` with `Outcome::{Merged, NeedsHuman { reason }, Failed { code: Option<i32>, stderr_tail: String }, Lost { reason }, Rejected { reason }}` stored at `results/<request id>.json`.
  - `scheduler::decide(input: &SchedulerInput) -> Vec<Decision>` where `SchedulerInput { max_parallel: u32, queue: Vec<Request>, running_projects: Vec<String>, project_states: BTreeMap<String, ProjectState> }`, `ProjectState::{Ready, LeaseHeld { owner: String }, Disabled { reason: String }}`, and `Decision::{Start(Request), Reject { request: Request, reason: String }}`.

- [ ] **Step 1: Failing tests for `HomeConfig` and `Home` names**

In `homeconfig.rs` tests: missing file gives `max_parallel == 2`; `max_parallel = 4` parses; `max_parallel = 0` is an error; an unknown key is an error; invalid TOML is an error. In `home.rs` tests: `config_file`, `queue_dir`, `results_dir`, `run_dir` and `logs_dir("p")` are under the home.
Run: `cargo test -j4 -p factory-core homeconfig home` — Expected: FAIL to compile.

- [ ] **Step 2: Implement `HomeConfig` and the `Home` names**

Add the folder and file names as constants in `home.rs` (`CONFIG_FILE`, `QUEUE_DIR`, `RESULTS_DIR`, `RUN_DIR`, `LOGS_DIR`) and the accessors. Implement `HomeConfig` in `homeconfig.rs` (serde derive, `#[serde(default, deny_unknown_fields)]`, a manual validation after parse). Export both from `lib.rs`.
Run: `cargo test -j4 -p factory-core` — Expected: PASS.

- [ ] **Step 3: Failing tests for requests, queue and records**

Create the crate skeleton (workspace member, `Cargo.toml` with the dependencies factory-core, factory-engine, serde, serde_json, thiserror, fs2; dev-dependencies tempfile) and tests in `request.rs`, `queue.rs`, `records.rs`:
- Request validation accepts `("rocket", Run, "1", true)` and `("rocket-vault", Approve, "003", false)`; refuses an empty project, `A`, `../x`, `a/b`, a job of `""`, `"abc"`, `"1/2"`, `"1234567"`, `"-1"`, `Approve` with `series = true`, an id with a separator, and a JSON file with an unknown field or a missing field (via `serde_json::from_str`).
- `enqueue` writes an atomic file named after the id; enqueueing the same id twice is an error; `list` returns requests sorted by `ts_ms` then id; a file with invalid JSON and a file failing validation become `Bad` entries with a reason, and the good ones are still listed; `remove` deletes by id and an unknown id is an error; a file whose name does not match its id is `Bad`.
- Records: child and result records round-trip; `read_children` skips a broken file (reported as `Err` entries, not a failure); a result file name is the request id.
Run: `cargo test -j4 -p factory-supervisor` — Expected: FAIL to compile.

- [ ] **Step 4: Implement requests, queue and records**

Implement the interfaces above with serde (`deny_unknown_fields` on `Request`), file names derived from the validated id only (never from file content), atomic writes via `write_atomic`. Request ids: `format!("{ts_ms:013}-{project}-{job}")`.
Run: `cargo test -j4 -p factory-supervisor` — Expected: PASS.

- [ ] **Step 5: Failing tests for the scheduler**

In `scheduler.rs` (pure function, no I/O): table-driven tests with requests for projects `a`, `b`, `c`:
1. `max_parallel = 2`, three ready projects, one request each: the first two by order start, the third waits (no decision for it).
2. Two requests for project `a`: only the first can start; the second waits even when capacity remains.
3. `a` already running (in `running_projects`): its requests wait; `b` starts.
4. `LeaseHeld` project: its request waits (no decision) and does not count against capacity.
5. `Disabled { reason }`: the request is a `Reject` with that reason and never counts against capacity.
6. A request for a project missing from `project_states` is a `Reject` ("unknown project").
7. Capacity is `max_parallel - running_projects.len()`, never negative (more running than the limit starts nothing).
8. Order is FIFO by `ts_ms` then id even when the input is shuffled; a `Reject` does not consume a capacity slot and does not block later requests.
Run: `cargo test -j4 -p factory-supervisor scheduler` — Expected: FAIL.

- [ ] **Step 6: Implement the scheduler, run everything, commit**

Implement `decide` as a single ordered pass. Run one at a time: `cargo test -j4 -p factory-core`, `-p factory-supervisor`, `cargo clippy -j4 -p factory-supervisor -p factory-core --all-targets` (no new warnings), `cargo check -j4 --manifest-path dashboard/Cargo.toml`.
Commit via the commit skill: `feat(core): add the home config and supervisor folders`, `feat(supervisor): add requests, queue, records and scheduler`.

---

### Task 2: The process layer, the loop and the `factoryd` binary

**Files:**
- Create: `crates/factory-supervisor/src/spawn.rs`, `src/supervisor.rs`, `src/lock.rs`, `src/bin/factoryd.rs`
- Modify: `crates/factory-supervisor/Cargo.toml` (`[[bin]] name = "factoryd"`), `src/lib.rs`, `crates/factory-engine/src/project_lease.rs` (make `owner_pid` and `process_gone` public and export them)
- Test: `crates/factory-supervisor/tests/supervisor.rs` with a shell script standing in for `factory`

**Interfaces:**
- Consumes: Task 1 items, `project_lease_status`, `factory_engine::{owner_pid, process_gone}`.
- Produces:
  - `trait Spawner { fn spawn(&self, home: &Home, req: &Request, out: &Path, err: &Path) -> Result<u32, SpawnError>; }` and `ProcessSpawner { bin: PathBuf }` with `ProcessSpawner::from_env() -> Result<ProcessSpawner, SpawnError>` (`FACTORY_BIN`, else a `factory` file next to the running executable). The real spawn runs `<bin> --project <p> run <job> [--series]` or `<bin> --project <p> approve <job>` with `FACTORY_HOME` set to the home, stdin null, stdout and stderr to the two files, `process_group(0)`, and returns the pid. The child is not waited for by `spawn`.
  - `Supervisor::new(home: Home, spawner: Box<dyn Spawner>, clock: Box<dyn Fn() -> u64>) -> Supervisor` and `Supervisor::tick(&mut self) -> Result<TickReport, SupervisorError>` where `TickReport { started: Vec<String>, finished: Vec<(String, Outcome)>, rejected: Vec<(String, String)> }` (request ids). One tick: (1) check each recorded child (`records::read_children`): alive when the pid is alive (`/proc/<pid>` exists and is not gone); a finished child gets a result classified from its out file and err file (see below), then the child record and the queue entry are removed; (2) build the `SchedulerInput` (project states from the registry, resolved with `project::list`: a broken project file or a missing repo path is `Disabled`; `ProjectState::LeaseHeld` when `project_lease_status` says Held and the owner pid is not one of our recorded children); (3) apply `decide`: `Reject` writes a `Rejected` result and removes the queue entry, `Start` spawns, writes the child record with the pid and removes nothing from the queue until the child ends. A spawn error becomes a `Failed` result.
  - Outcome classification of a finished child, from `out` (stdout) and `err`: the last non-empty stdout line equal to `merged` is `Merged`; starting with `needs-human` is `NeedsHuman { reason }` (the rest of the line); otherwise, when the child was ours and we have its exit code, `Failed { code, stderr_tail }` (last 20 lines of stderr, each capped at 500 characters); a child that we only adopted and that left no recognizable last line is `Lost`. For children we spawned, keep the `std::process::Child` in the supervisor and use `try_wait` for the exit code; for adopted children there is no exit code.
  - `lock::InstanceLock::acquire(home) -> Result<InstanceLock, LockError>`: an exclusive `fs2` lock on `run/factoryd.lock`, released on drop; a second acquire fails with a message naming the running pid (written into the file).
  - Stop file: `run/stop`; `Supervisor::run_loop(&mut self, interval: Duration, stop: &dyn Fn() -> bool)` ticks until the stop function says stop; it never kills children.
  - `factoryd` subcommands: `run [--interval-ms N] [--once]` (takes the instance lock, loads `HomeConfig`, runs the loop; `--once` does one tick and exits, for tests and cron), `status` (prints the queue, the running children with project, job, pid and age, and the last 10 results), `stop` (writes the stop file; prints whether an instance is running), `enqueue --project P --job N [--series] [--approve]` (validates and writes a request, prints the id).

- [ ] **Step 1: Make the pid helpers public**

In `crates/factory-engine/src/project_lease.rs` make `owner_pid` and `process_gone` `pub` and export them from `lib.rs`; add a test that `process_gone` is false for this process's pid and true for a pid above the kernel limit (use 4294967000 as the lease tests do).
Run: `cargo test -j4 -p factory-engine project_lease` — Expected: PASS.

- [ ] **Step 2: Failing tests for the lock and the spawner**

In `lock.rs` tests: the first acquire succeeds, a second in the same process (separate file open) fails with a message containing the first pid, dropping the first allows a third. In `spawn.rs` tests (`#[cfg(unix)]`): write a shell script `fake-factory` in a temp dir (`#!/bin/sh`, echo the arguments to stdout, exit 0) and spawn it through `ProcessSpawner`: the out file contains `--project p run 7` and a line for the `FACTORY_HOME` env value, the returned pid is a real process, a `--series` request adds the flag, an approve request runs `approve`; a missing binary is a `SpawnError` naming the path; the child is in its own process group (read `/proc/<pid>/stat` field 5 or compare `getpgid` through `/proc`: the process group id equals the child's own pid).
Run: `cargo test -j4 -p factory-supervisor lock spawn` — Expected: FAIL to compile.

- [ ] **Step 3: Implement lock and spawner**

Implement as specified. Use `std::os::unix::process::CommandExt::process_group(0)` under `#[cfg(unix)]`; elsewhere return `SpawnError::Unsupported`.
Run: `cargo test -j4 -p factory-supervisor` — Expected: PASS.

- [ ] **Step 4: Failing tests for the supervisor loop**

`crates/factory-supervisor/tests/supervisor.rs`, with a temp `Home`, two registered projects (use `project::add` with temp git repos made by `git2`, or write `project.toml` files directly if simpler; the repos only need to exist), a fake `factory` script controlled by files: the script reads `<script dir>/behavior-<project>-<job>` (`merged`, `needs-human: risky`, `fail`, `sleep`) and acts (print, exit code, or sleep 60 for `sleep`):
1. Enqueue one request: tick starts it (the script is spawned), a second tick after it ends writes a `Merged` result, removes the queue file and the child record.
2. `needs-human: risky` gives `NeedsHuman { reason: "risky" }`; `fail` (exit 1 with a stderr line) gives `Failed { code: Some(1), stderr_tail }`.
3. With `max_parallel = 2`: three requests for three projects start two, then the third after one ends (use `sleep` behaviour, end children by deleting a gate file the script polls). Two requests for one project run strictly one after the other.
4. A project whose repo path was deleted: its request gets `Rejected` with a reason; other projects' requests still start in the same tick.
5. A bad request file in the queue is skipped, reported by `list` as `Bad`, and does not stop the tick.
6. A human holds the project lease (write a `project.lease` for owner `driver-<this pid>-1` far in the future with `acquire_project_lease`): the request waits, starts after the lease is released.
7. `run_loop` with a stop function that returns true after 3 ticks ends without killing a running `sleep` child (the pid is still alive afterwards; the test kills it at the end).
8. Spawn failure (bin path missing) gives a `Failed` result and the loop continues.
Run: `cargo test -j4 -p factory-supervisor --test supervisor` — Expected: FAIL to compile.

- [ ] **Step 5: Implement the supervisor and the binary**

Implement `Supervisor::tick`, `run_loop`, the classification, and `src/bin/factoryd.rs` with the four subcommands (hand-rolled arg parsing is fine; `clap` is available in the workspace if it keeps the code shorter: use it). `status` output is plain text, one block per section.
Run: `cargo test -j4 -p factory-supervisor` — Expected: PASS.

- [ ] **Step 6: Run everything and commit**

Run one at a time: `cargo test -j4 -p factory-supervisor`, `-p factory-engine`, `-p factory-cli`, `cargo clippy -j4 -p factory-supervisor -p factory-engine --all-targets`, `cargo build -j4 -p factory-supervisor --bin factoryd`.
Commit via the commit skill: `feat(engine): export the pid helpers`, `feat(supervisor): add the instance lock and the process spawner`, `feat(supervisor): add the supervisor loop and factoryd`.

---

### Task 3: Recovery, heartbeat and the carried lease items

**Files:**
- Modify: `crates/factory-supervisor/src/supervisor.rs` (adoption and recovery)
- Modify: `crates/factory-engine/src/compat.rs` (`ProjectGuard` heartbeat, injectable timing), `project_lease.rs`
- Modify: `crates/factory-cli/src/commands.rs` (`kb-refresh` takes the project lease)
- Modify: `README.md`
- Test: `crates/factory-supervisor/tests/recovery.rs`, `crates/factory-engine/tests/lease_heartbeat.rs`, a `kb-refresh` case in `crates/factory-cli/tests/project_lease_cli.rs`

**Interfaces:**
- Consumes: Tasks 1 and 2.
- Produces:
  - `Supervisor` start-up recovery (`Supervisor::recover(&mut self) -> Result<RecoveryReport, SupervisorError>`, called once at the start of `run` and by the first tick): for every child record whose supervisor did not spawn it in this process: pid alive means adopted (stays a running child, no second spawn for that project); pid gone means classify from the out and err files (merged or needs-human lines give those outcomes, anything else `Lost { reason: "the process ended without a result" }`), write the result, remove the record and the queue entry. A child record whose request file is gone is still handled (result written, record removed). The job is never restarted.
  - `Driver::with_lease_timing(self, ttl: f64, heartbeat: Duration) -> Driver` (defaults: `LEASE_TTL_SECS` and 60 seconds) and a heartbeat thread started by `hold_project` (owner set) that, every `heartbeat`, renews the project lease for the held job and the job lease on the board (same owner, same ttl) until the guard drops, then is joined. A renewal that finds another owner holding the project stops the thread and records the loss in a flag the guard exposes (`ProjectGuard::lost(&self) -> bool`); `run_job_inner` checks the flag at the top of each iteration and returns the busy error.
  - `kb-refresh` in the CLI takes `d.hold_project(...)` like `run` (a busy project refuses it with the same message).

- [ ] **Step 1: Failing tests for recovery**

`crates/factory-supervisor/tests/recovery.rs` (unix): 
1. Start a supervisor, enqueue a request for a `sleep` fake job, tick (child running), then DROP the supervisor (simulates `factoryd` stopping) and check the child pid is still alive and its process group is not the test's. Create a NEW supervisor on the same home: `recover` adopts it (no second spawn: the fake script appends a line to a counter file on each start, assert one line), the queue still has the request; end the child; the next tick writes the real result (`Merged` from the out file) and clears the record.
2. Child killed with SIGKILL while no supervisor runs (kill the pid, then create a new supervisor): recovery writes `Lost { reason }`, removes the child record and the queue entry, and a later request for the same project starts normally; the lost job is not restarted (counter stays at one).
3. A child record whose queue file was already removed: recovery still writes a result and removes the record.
4. A corrupt child record file: reported, skipped, other records handled.
5. Two supervisors on the same home (without the instance lock, as library objects) do not start the same request twice: the second sees the child record and the running pid for the project and waits.
Run: `cargo test -j4 -p factory-supervisor --test recovery` — Expected: FAIL to compile.

- [ ] **Step 2: Implement recovery**

Implement `recover` and call it from `run_loop` before the first tick and from `factoryd run`. Reuse the classification from Task 2. Adoption is only a record that stays in place and a pid watched by `/proc`.
Run: `cargo test -j4 -p factory-supervisor` — Expected: PASS.

- [ ] **Step 3: Failing tests for the heartbeat and the kb lease**

`crates/factory-engine/tests/lease_heartbeat.rs`: with `with_lease_timing(ttl = 2.0, heartbeat = 200ms)` and a scripted runner whose agent step blocks for 5 seconds (sleep inside the fake runner): the project lease file's `expires` is later than the first value after 1 second, `project_lease_status` is `Held` for the whole 5 seconds (poll every 200 ms from the test thread), and another driver's `hold_project` is refused throughout; after the run the lease is released and the heartbeat thread has ended (no thread leak: the guard drop joins it; assert the drop returns promptly). Without the heartbeat the same scenario lets the lease expire at 2 seconds (add this as a negative control using `heartbeat = Duration::from_secs(3600)`, assert the status becomes stale before the run ends). A renewal that finds another owner (overwrite the lease file as another owner while the heartbeat runs, with a far-future expiry): `lost()` becomes true and the next loop iteration errors with the busy message. CLI test: `kb-refresh --job 1` is refused with "project is busy" while another owner holds the lease and runs when free.
Run: `cargo test -j4 -p factory-engine --test lease_heartbeat` and `-p factory-cli --test project_lease_cli` — Expected: FAIL.

- [ ] **Step 4: Implement the heartbeat and the kb lease**

Add the timing fields to `Driver` (ttl and heartbeat) and use the ttl in every acquire (project and job). In `hold_project`, when an owner is set, spawn the heartbeat thread (`std::thread`, a stop channel or `AtomicBool` plus `park_timeout`), which uses only cloned values (paths, owner, job id, ttl): it calls `acquire_project_lease` and `acquire_lease` on a fresh `BoardStore`, and on `Held` sets the lost flag and exits. The guard's drop sets the stop flag, unparks and joins. Add the lost check in `run_job_inner`. In `commands.rs` take `hold_project` in `kb_refresh`.
Run: `cargo test -j4 -p factory-engine` and `-p factory-cli` (whole crates) — Expected: PASS.

- [ ] **Step 5: Document, run everything, commit**

README: a `factoryd` section (what it does, the home folders `queue`, `results`, `run`, `logs`, `config.toml` with `max_parallel`, the four subcommands with an example, that stopping `factoryd` never stops jobs and a restart adopts them, `Lost` results and what to do, `FACTORY_BIN`), and in the project lease section the caveat that the dead-owner check reads `/proc` on this machine only: a state folder shared through a container bind mount or a network filesystem can show a live owner as gone, so run drivers for one project from one machine.
Run one at a time: `cargo test -j4 -p factory-core`, `-p factory-git`, `-p factory-agent`, `-p factory-checks`, `-p factory-engine`, `-p factory-cli`, `-p factory-supervisor`, `cargo clippy -j4 --all-targets` for engine, cli, supervisor (no new warnings), `cargo check -j4 --manifest-path dashboard/Cargo.toml`. Run the supervisor recovery tests three times in a row and report whether any was flaky.
Commit via the commit skill: `feat(supervisor): adopt or park children on start`, `feat(engine): add a lease heartbeat`, `feat(cli): take the project lease in kb-refresh`, `docs: document factoryd`.
