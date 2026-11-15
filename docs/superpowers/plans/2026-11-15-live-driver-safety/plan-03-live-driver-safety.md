# Live driver safety: wiring the children (plan 3 of 3)

> **Spec:** docs/superpowers/specs/2026-11-15-live-driver-safety-design.md
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Agent and check children run in their own process group, every command that builds a driver registers them and kills them on a shutdown signal, and a lease take reaps the leftover groups of dead owners.

**Architecture:** `factory-agent` and `factory-checks` spawn through `factory_procgroup::spawn`. `main.rs` sets the registry and the handler once, so every project command is covered. The engine reads `children.json` through `Layout` and calls `reap_leftovers` when `Driver::take_project` makes a fresh take or takes over a stale owner.

**Tech Stack:** Rust, factory-procgroup (plan 2), factory-agent, factory-checks, factory-core, factory-engine, factory-cli.

## Global Constraints

- Plans 1 and 2 are merged. `factory-procgroup` exposes `spawn`, `GroupChild`, `set_registry`, `kill_all`, `install_shutdown_handler`, `reap_leftovers` and `StartFn`.
- Paths go through `Layout` (`factory-cli/tests/no_hardcoded_paths.rs` must stay green). No new `/proc` read.
- A failed reap, kill or registry write never changes a lease take or a merge. It is reported as one ledger line, one `tracing::warn!` and one stderr line.
- `children.json` is a new file in the state folder. Board JSON is unchanged.
- Cargo: `-j4`, one crate at a time. Tests use `.expect("...")` and `expect_err`, never the unwrap call. Code comments are short full sentences that end with a period.

## Review Focus

1. A check command that times out: its whole group dies, not only the shell. Pinned by `a_timed_out_check_kills_its_grandchild`.
2. An agent run stopped by the driver (stall or budget): the agent's children end too. Pinned by `a_stopped_agent_group_is_killed`.
3. A takeover of an owner that is not dead, or of owner `unknown`: nothing is reaped. Pinned by `a_takeover_of_an_unknown_owner_reaps_nothing`.
4. A leftover entry whose pid now belongs to another process: left alone, reported. Pinned by `a_takeover_leaves_a_mismatched_entry_alone`.
   A forced cancel before the next run (fresh take): leftovers of dead owners are still reaped. Pinned by `a_fresh_take_reaps_a_dead_owners_group`.
5. A project with no `children.json`: the takeover works and says nothing. Pinned by `a_takeover_without_a_registry_still_takes_over`.

## File Structure

- `crates/factory-agent/src/child.rs`, `Cargo.toml`: spawn through the new crate.
- `crates/factory-checks/src/run.rs`, `Cargo.toml`: the same.
- `crates/factory-core/src/layout.rs`, `lib.rs`: `CHILDREN_FILE`, `Layout::children_file()`.
- `crates/factory-engine/src/compat.rs`, `Cargo.toml`: reap on takeover.
- `crates/factory-cli/src/main.rs`, `src/commands.rs`, `Cargo.toml`: `guard_children`, called from `main.rs`.
- Docs: `CLAUDE.md`, `docs/reference/software-factory.md`, `README.md`.
- Tests: `crates/factory-agent/tests/runner.rs`, `crates/factory-checks/src/run.rs` (unit tests), `crates/factory-core/src/layout.rs` (unit test), `crates/factory-engine/tests/reap_on_takeover.rs`, `crates/factory-cli/tests/guard_children.rs`.

---

### Task 1: Agent and check children use the group spawn

**Files:**
- Modify: `crates/factory-agent/src/child.rs` (spawn at line 109, kills at lines 113 and 199), `crates/factory-checks/src/run.rs` (spawn at line 152, kills at 173 and 180), both `Cargo.toml` (add `factory-procgroup = { path = "../factory-procgroup" }`)
- Test: `crates/factory-agent/tests/runner.rs`, unit tests in `crates/factory-checks/src/run.rs`

**Interfaces:**
- Consumes: `factory_procgroup::spawn(&mut Command) -> io::Result<GroupChild>`, `GroupChild::kill`, `Deref<Target = Child>`.
- Produces: no new public item. The two spawn sites hold a `GroupChild`. Every `child.kill()` becomes the group kill by method resolution, and the child is killed on drop.

- [ ] **Step 1: Read** `child.rs` lines 85-210 and `run.rs` lines 143-200, and the helpers `runner(sink)`, `spec(mode, dir)` and the `fake-claude` modes used by `ok_run_collects_session_usage_result_and_events` in `tests/runner.rs`.
- [ ] **Step 2: Write the failing tests.**
  - `tests/runner.rs`: `an_agent_child_is_a_group_leader`. Write a script `agent.sh` into a tempdir that prints one valid result line the runner accepts (copy the line `fake-claude` prints in mode `ok`), after it writes `"$$ $(ps -o pgid= -p $$)"` to `pids.txt` in its working directory, and `chmod 0o755` it. Build the runner as `runner(EventSink::null("001"))` does but with `program` set to the script (read how `ClaudeRunner` is configured in `runner(...)`). After `run`, `pids.txt` holds two equal numbers.
  - `tests/runner.rs`: `a_stopped_agent_group_is_killed`. The script starts `sleep 60 &`, writes the sleeper pid (`$!`) to `grandchild.txt`, then sleeps with no output; run it with a spec that stalls fast (use the stall setting of `the_session_hook_fires_before_a_stalled_run_is_killed`). After `run` returns, `libc::kill(pid, 0)` fails with `ESRCH` within 2 s. Add `libc` as a dev-dependency pinned like plan 2.
  - `run.rs` unit tests: `a_check_command_runs_in_its_own_group` (a script `check.sh` in the tempdir prints `"$$ $(ps -o pgid= -p $$)"`, the command is `./check.sh`; the summary line holds two equal numbers) and `a_timed_out_check_kills_its_grandchild` (the script starts `sleep 60 &`, prints `$!` to stdout, then `wait`s; timeout 1 s; the check fails with `timed out`, the printed pid is gone within 2 s).
- [ ] **Step 3: Run** `cargo test -j4 -p factory-agent --test runner an_agent_child a_stopped_agent` and `cargo test -j4 -p factory-checks run::tests::a_`. Expected: the group-leader tests fail (two different numbers) and the kill tests fail (the grandchild survives).
- [ ] **Step 4: Implement**: replace `command.spawn()` in `child.rs` with `factory_procgroup::spawn(&mut command)` and `Command::new(program)...spawn()` in `run.rs` with `factory_procgroup::spawn(&mut cmd)`. Take `stdin`, `stdout` and `stderr` through the deref as today. Keep every other line.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-agent`, `cargo test -j4 -p factory-checks`, then `cargo clippy -j4 -p factory-agent -p factory-checks --all-targets -- -D warnings` (one crate per command) and `cargo fmt -p factory-agent -p factory-checks -- --check`. Expected: all pass.

### Task 2: Registry, handler, reaper on takeover, and docs

**Files:**
- Modify: `crates/factory-core/src/layout.rs` (next to `project_lease_file`, line ~181) and `lib.rs` (line 74), `crates/factory-engine/src/compat.rs` (`take_project`, line ~380), `crates/factory-engine/Cargo.toml`, `crates/factory-cli/src/main.rs` (after `let ctx = match built { ... }`, lines 87-93, which covers both the `--project` and the no-project branch), `crates/factory-cli/src/commands.rs`, `crates/factory-cli/Cargo.toml`, `CLAUDE.md`, `docs/reference/software-factory.md`, `README.md`
- Test: `crates/factory-core/src/layout.rs` (unit), `crates/factory-engine/tests/reap_on_takeover.rs` (new)

**Interfaces:**
- Consumes: `factory_procgroup::{set_registry, install_shutdown_handler, reap_leftovers}`, `factory_engine::procinfo::{process_start_ms, process_state, ProcState}`, `LeaseTake::TookOverStale { owner, job }`.
- Produces: `pub const CHILDREN_FILE: &str = "children.json"` and `Layout::children_file(&self) -> PathBuf` (`state_dir.join(CHILDREN_FILE)`); `Driver::take_project` calls `reap_leftovers(&self.layout.children_file(), &|o| Some(o) == taken.as_deref() || owner_state(o) == OwnerState::Dead, &running, procinfo::process_start_ms)` after a `Fresh` take or a take over of a stale owner (`taken` is that owner unless it is `"unknown"`, and `None` on a fresh take) and reports each returned line; `pub fn guard_children(ctx: &CliCtx)` in `commands.rs` that calls `set_registry(ctx.layout.children_file(), ctx.owner.clone(), procinfo::process_start_ms)` and `install_shutdown_handler()`, called once from `main.rs` right after the `let ctx = match built { ... }` block, for every command.

- [ ] **Step 1: Read** `layout.rs` (`project_lease_file`, the tests at the bottom), `compat.rs` `take_project` and `renew_lease`, `runloop.rs` `note_failure` (the ledger line pattern), `commands.rs` lines 100-120 and 250-265, `main.rs` lines 70-95, and `tests/lease_heartbeat.rs` for `SeriesFixture::owned`.
- [ ] **Step 2: Write the failing tests.**
  - `layout.rs` unit test `children_file_is_in_the_state_folder`: `Layout::in_repo(dir).children_file()` ends with `children.json` and its parent is `state_dir`.
  - `tests/reap_on_takeover.rs` (`mod common;`, Linux only for the dead pid, `#[cfg(target_os = "linux")]`), each test seeds the state folder before `run_job`: a lease file held by `driver-4294967000-1` (job `009`, far-future `expires`) and a `children.json` entry for a real group leader spawned through `factory_procgroup::spawn(sleep 60)` (add `factory-procgroup` as a dev-dependency):
    - `a_takeover_of_a_dead_owner_reaps_its_registered_group`: entry `{pid, start_ms: process_start_ms(pid), owner: "driver-4294967000-1"}`; after `run_job(&fx.driver, &job.id)` finishes, the sleeper is gone (poll up to 2 s) and the job's `progress.md` contains `killed`.
    - `a_takeover_leaves_a_mismatched_entry_alone`: `start_ms` is `Some(1)`; the sleeper still runs and `progress.md` contains `left alone`.
    - `a_takeover_of_an_unknown_owner_reaps_nothing`: the corrupt-free lease owner is `"driver-4294967000"` (no secs, `Unknown`) with `expires` in the past; the sleeper still runs and `progress.md` has neither line.
    - `a_takeover_without_a_registry_still_takes_over`: no `children.json`; `run_job` succeeds.
    - `a_fresh_take_reaps_a_dead_owners_group`: no lease file at all (as after a forced cancel), the registry entry is owned by `driver-4294967000-1` with a matching start; after `run_job` the sleeper is gone and `progress.md` contains `killed`.
  - `crates/factory-cli/tests/guard_children.rs`: `guard_children_registers_children` (build a `CliCtx` as another CLI test does, call `guard_children(&ctx)`, spawn `sleep 30` through `factory_procgroup::spawn`, the file `ctx.layout.children_file()` lists its pid, kill it) and `main_guards_children_for_every_project_command` (read `src/main.rs` with `include_str!`, assert it contains `guard_children(` exactly once and that the call comes after the `let ctx = match built` block, so both the `from_env` and the `for_project` branch are covered).
    Kill any surviving sleeper at the end of each test.
- [ ] **Step 3: Run** `cargo test -j4 -p factory-core children_file` and `cargo test -j4 -p factory-engine --test reap_on_takeover`. Expected: `no method named children_file`, then the reap tests fail (sleeper survives, no ledger line).
- [ ] **Step 4: Implement** the signatures above. In `take_project`, after `acquire_project_lease` returns `LeaseTake::Fresh` or `LeaseTake::TookOverStale { owner, .. }` (not `Renewed`), build `taken = Some(owner)` when the owner is not `"unknown"` and `None` otherwise, `running = |pid| procinfo::process_state(pid) == ProcState::Running`, call `reap_leftovers` as the Interfaces line says, then for each line write a ledger ruling for job `id` (as `note_failure` does), `tracing::warn!(event = "children.reap", msg = %line)` and `eprintln!`. Never return an error from this step. In `main.rs`, call `factory_cli::guard_children(&ctx)` once after the context is built. Docs: add `factory-procgroup` to the crate layering list in `CLAUDE.md` (process groups, registry, reaper, handler, deps `libc` and a signal crate), describe in `docs/reference/software-factory.md` and the lease section of `README.md` that a live owner keeps its lease past expiry, that a corrupt project lease is refused and `factory cancel <id> --force` clears it, that children run in their own group and die with a SIGTERM, SIGINT or SIGHUP of the driver, and that after a SIGKILL the next driver reaps the leftovers when pid, group leader and start time match.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`, `cargo test -j4 -p factory-supervisor`, then `cargo clippy -j4 -p <crate> --all-targets -- -D warnings` and `cargo fmt -p <crate> -- --check` for each of the four crates. Expected: all pass, `no_hardcoded_paths` green.

Next plan: none
