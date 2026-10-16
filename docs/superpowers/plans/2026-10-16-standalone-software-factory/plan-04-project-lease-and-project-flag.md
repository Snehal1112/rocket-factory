# Project Lease and the --project Flag Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the existing commands run against a registered project (`--project <name>`) using its resolved settings, layout and agent files, with at most one active job per project, enforced in the driver library.

**Architecture:** A project lease is a small JSON file in the project's state folder, guarded by an exclusive file lock, taken by the driver before any run or approve and renewed with the job lease. A stale lease is taken over and shown as parked, never as running. A global `--project` flag builds the `CliCtx` from `resolve()` (config, layout, agent files) and refuses to run, approve or refresh the KB when the project is degraded or its base branch came from the fallback. Items the plan 3 reviews carried over are closed in the last task.

**Tech Stack:** Rust 2021 (cargo 1.94), fs2 (already used for the board lock), serde_json, clap 4, git2 0.19.

**Spec:** `docs/superpowers/specs/2026-10-16-standalone-software-factory-design.md` (Components: driver lease, `--project`; Error handling: stale lease; Acceptance criteria).

## Global Constraints

- Work in `/home/numericlabs/data/rocket/rocket-factory`, on a branch `plan-04-lease-project` created from `main`. Fast-forward `main` at the end.
- Every `cargo` command has `-j4`. Never `--workspace` or `--all`. Never set `CARGO_TARGET_DIR`. Test one crate at a time with `-p <crate>`.
- Do not push anywhere. The repo has no remote.
- Commits use conventional commits and are written through the `dev-workflow-skills:1-git-commit` skill. Stage explicit paths and commit with a pathspec, never `git add -A`, `--all` or `.`.
- No unwrap calls in production paths (tests may use `expect`). No shelling out to `git` (use `git2`).
- Do not edit the byte copies of Rocket files: `.claude/factory/config.toml`, `.claude/factory/commit-context.md`, `.claude/software-factory.md`, `.claude/commands/factory.md`, `.claude/agents/*.md`. The `dashboard/` crate is untouched.
- Board JSON, `events.jsonl` and `costs.json` formats do not change: no new field in `board.json`. The project lease is its own file.
- The guard test `crates/factory-cli/tests/no_hardcoded_paths.rs` keeps passing without widening its allowlist. New file names live in `crates/factory-core/src/layout.rs`.
- Without `--project` and without a lease owner (library callers and existing tests that build a `Driver` with no owner) behavior is unchanged. With the in-repo layout and an owner, the project lease applies too: one job at a time per repo (spec: one job per project).
- A degraded or invalid project never starts a build, review or merge. Read-only commands (`status`, `show`, `tokens`, `costs`) and `new`, `rework`, `cancel`, `cleanup` keep working on a degraded project.

## Review Focus

- Two processes run jobs of the same project: the second must be refused with a clear message while the first holds the lease, and must succeed once the lease expired or was released. A crash that leaves the file must not block the project for longer than the TTL.
- A series of plans drops the lease between plans, so another process takes the project in the gap: the lease must be held across the whole series.
- `approve` runs `merge_stage` directly, not through `run_job`: it must take the project lease too, and release it on every exit path including an error.
- A corrupt, empty or non-JSON lease file must not block the project forever and must not be silently treated as free in a way that lets two jobs run: decide and test (a corrupt file is treated as stale and replaced, with the problem noted).
- `--project` with a degraded project, an unknown project, a missing repo path or a base branch that came from the fallback must refuse `run` and `approve` before any agent starts. `--root` and `--project` together are refused by the parser.
- A project job leaves the target repo's working tree untouched: no `.superpowers`, no `.claude`, no `.factory` folder is created in it, and `git status` is clean after a full job.

---

### Task 1: The project lease

**Files:**
- Create: `crates/factory-engine/src/project_lease.rs`
- Modify: `crates/factory-core/src/layout.rs` (file name `project.lease` and `Layout::project_lease_file()`)
- Modify: `crates/factory-engine/src/lib.rs`, `compat.rs` (`Driver`), `runloop.rs` (`run_job`, `run_series`)
- Modify: `crates/factory-cli/src/commands.rs` (`run`, `approve`), `status.rs` (a project line)
- Test: `crates/factory-engine/tests/project_lease.rs`, `crates/factory-cli/tests/project_lease_cli.rs`

**Interfaces:**
- Consumes: `BoardStore` lock helpers (`lock.rs`), `LEASE_TTL_SECS`, `now_secs()`.
- Produces, used by tasks 2 and 3:
  - `Layout::project_lease_file(&self) -> PathBuf` (`state_dir/project.lease`).
  - `factory_engine::ProjectLease { pub owner: String, pub job: String, pub expires: f64 }` (serde), and free functions in `project_lease.rs`:
    - `acquire_project_lease(state_dir: &Path, job: &str, owner: &str, ttl: f64, now: f64, wait: Duration) -> Result<LeaseTake, ProjectLeaseError>`: under an exclusive lock on `project.lease.lock`, reads the file; free (no file), expired, corrupt or owned by the same owner means take or renew it (write `{owner, job, expires: now + ttl}` atomically); a live lease of another owner is `Err(Held { owner, job, secs_left })`. `LeaseTake` is `Fresh`, `Renewed` or `TookOverStale { owner: String, job: String }` (also used for a corrupt file, with owner `"unknown"` and job `"unknown"`).
    - `release_project_lease(state_dir: &Path, owner: &str, wait: Duration) -> Result<(), ProjectLeaseError>`: removes the file only when it is ours or expired or corrupt; a live lease of another owner is left alone and is not an error.
    - `project_lease_status(state_dir: &Path, now: f64) -> ProjectLeaseStatus`: `Free`, `Held { owner, job, secs_left }`, `Stale { owner, job }` or `Unreadable(String)`.
  - `ProjectLeaseError::Held { owner, job, secs_left }` displays "project is busy: job {job} is running (leased by {owner} for another {secs_left} seconds)"; other variants wrap io and lock errors.
  - `Driver::hold_project(&self, job: &str) -> Result<ProjectGuard<'_>, EngineError>`: takes the lease (needs `lease_owner`; without an owner it returns a no-op guard), and the guard's `Drop` releases it. `Driver::renew_lease` (the per-job renew at the top of each run loop iteration) also renews the project lease when an owner is set. While any guard is alive, `run_job`'s final release does not release the project lease (a counter in the `Driver`).

- [ ] **Step 1: Write the failing lease tests**

Create `crates/factory-engine/tests/project_lease.rs` with unit-level tests of the three free functions, each in a fresh temp state folder:
1. fresh acquire creates the file, a second owner is refused with `Held` and the message text above, the same owner renews (`Renewed`) and the expiry moves forward;
2. an expired lease (call with `now` past `expires`) is taken over (`TookOverStale` names the old owner and job) and `project_lease_status` reports `Stale` before the takeover and `Held` after;
3. a corrupt file (random bytes) and an empty file are taken over, with owner and job `"unknown"`; `project_lease_status` reports `Unreadable` first;
4. `release_project_lease` removes our file, leaves another owner's live lease in place without an error, removes an expired one;
5. two threads acquiring at the same instant for different owners: exactly one wins (use a barrier; repeat 20 times in a loop);
6. the lock wait is honored: hold the lock file with `fs2` in the test, call acquire with a 100 ms wait, expect a lock error and no file written.
Run: `cargo test -j4 -p factory-engine --test project_lease` — Expected: FAIL to compile.

- [ ] **Step 2: Implement `project_lease.rs`**

Implement the interfaces above. Reuse the board lock polling (`lock_board` in `lock.rs`) for the exclusive lock on `project.lease.lock` if it can lock an arbitrary path; otherwise copy its 25 ms polling loop into this module. Write the lease file atomically (write a temp file, rename; the board store has `write_atomic`, reuse it if it is accessible). Add `PROJECT_LEASE_FILE: &str = "project.lease"` and `project_lease_file()` to `layout.rs`. Export the new items from `crates/factory-engine/src/lib.rs`.
Run: `cargo test -j4 -p factory-engine --test project_lease` — Expected: PASS.

- [ ] **Step 3: Write the failing driver tests**

In the same test file (using `tests/common/mod.rs`), with a `Driver` built with a lease owner (`with_lease_owner("a")`) and a second `Driver` on the same layout with owner `"b"`:
1. `a.hold_project("001")` succeeds; `b.hold_project("002")` is refused with the busy message; dropping a's guard lets b succeed;
2. `a.run_job` on a job (use the scripted runner as the existing run-loop tests do) while `b` holds the project lease returns the busy error before any agent runs (assert the runner was not called);
3. a series (`run_series` over two plans) never lets another driver acquire between plans: run the series with a runner hook that, between plan one and plan two, tries `b.hold_project(...)` from the hook and records the result: it must be refused both times;
4. a driver with no lease owner does not take or need a project lease (no `project.lease` file appears);
5. the guard releases the lease when the guarded section returns an error (return `Err` inside a closure and assert the file is gone);
6. `renew_lease` moves the project lease's `expires` forward (read the file before and after, with a controllable `now` if `acquire` takes it, otherwise assert `expires` is greater).
Run: `cargo test -j4 -p factory-engine --test project_lease` — Expected: FAIL to compile.

- [ ] **Step 4: Implement the driver part**

In `compat.rs` add the counter (`Cell<u32>` or an atomic) and `hold_project` and `ProjectGuard`. In `renew_lease`, after the job lease, call `acquire_project_lease` with the job id and the same TTL. In `run_job` (runloop.rs), release the project lease at the end only when the hold counter is zero; if the job lease acquisition inside `run_job` was the first acquisition (no guard held), acquire the project lease there too so a caller that never calls `hold_project` is still protected. In `run_series`, take `hold_project` for the first job id at the start (a guard that lives until the series returns). Map `ProjectLeaseError` into `EngineError` with the busy text intact.
Run: `cargo test -j4 -p factory-engine` (whole crate) — Expected: PASS.

- [ ] **Step 5: CLI wiring and the status line**

In `commands.rs`: `run` takes `d.hold_project(&id)?` before the job lease check and keeps the guard for the whole command (series included); `approve` takes it before `merge_stage` and keeps it through the merge (the existing `LeaseGuard` stays). In `status.rs` print one line at the top of the status output: `project: free`, `project: busy, job 003 (owner X, N s left)`, `project: stale lease from X (job 003), parked` or `project: lease file unreadable: <reason>`, from `project_lease_status(layout.state_dir, now)`. Add `crates/factory-cli/tests/project_lease_cli.rs`: with a lease held by another owner (take it with `acquire_project_lease` in the test), `run 1` and `approve 1` fail with "project is busy" and run no agent; `status` prints the busy line; with an expired lease `run 1` succeeds and `status` before it printed the stale line; `rework`, `cancel`, `show`, `tokens`, `cleanup` and `new` are not blocked by a held lease.
Run: `cargo test -j4 -p factory-cli` (whole crate; existing tests that print `status` must be updated for the new first line only if they compare the whole text; adapt them minimally).

- [ ] **Step 6: Commit**

Run one at a time: `cargo test -j4 -p factory-core`, `-p factory-engine`, `-p factory-cli`, and `cargo clippy -j4 -p factory-engine -p factory-cli --all-targets` (no new warnings).
Commit via the commit skill: `feat(engine): add the project lease`, then `feat(cli): hold the project lease in run and approve`.

---

### Task 2: The `--project` flag

**Files:**
- Modify: `crates/factory-cli/src/cli.rs` (global `--project`), `commands.rs` (`CliCtx`), `main.rs`, `project.rs` (`open_repo` and `base_of` become `pub(crate)`)
- Modify: `crates/factory-engine/src/specs.rs` (reviewer tools)
- Test: `crates/factory-cli/tests/project_flag.rs`, a reviewer-spec test in the engine

**Interfaces:**
- Consumes: `resolve`, `Resolved`, `project::{get, layout}`, `RepoTip`, `Driver::with_agent_files`, `Driver::hold_project`, `factory_checks::validate_config`.
- Produces:
  - Global flag `--project <name>` on `Cli`, `conflicts_with = "root"`.
  - `CliCtx::for_project(home: &Home, name: &str, runner: Option<Arc<dyn AgentRunner>>, owner: &str, lock_wait: Duration) -> Result<CliCtx, CliError>`: reads `project.toml`, opens the repo with its base branch (via `open_repo` and `base_of`), calls `resolve`, and keeps `config`, `layout`, `agents` and a `blocked: Option<String>` reason in the context.
  - New `CliCtx` fields `config: Option<Config>`, `agents: Vec<AgentFile>`, `blocked: Option<String>` (all defaulted by the existing constructors: `None`, empty, `None`).
  - `CliCtx::driver()` uses `config` when it is `Some` (never `Config::load(layout.config_file)` for a project) and calls `with_agent_files(agents)`.
  - `CliCtx::runnable(&self) -> Result<(), CliError>`: `Err(Refused(reason))` when `blocked` is `Some`; called at the top of `run`, `approve` and `kb-refresh`.
  - `blocked` is set when: the resolved config is degraded (reason: the first problem plus "(and N more)" when several); `validate_config` rejects a check command (the text "a check command would be rejected when a job starts: ..."); or the project's `project.toml` has no valid `base_branch` setting (the fallback to `main` is never used for a project). `notes` are printed on stderr at start-up, one `note:` line each.
  - `AgentSpec` for the reviewer sets `tools = Some("Read,Grep,Glob")`.

- [ ] **Step 1: Write the failing tests**

`crates/factory-cli/tests/project_flag.rs`, using a temp `Home`, a temp git repo on branch `v-4.0.0` (also a local `main` branch pointing to the first commit), `project::run` to add it with `--preset rust --base v-4.0.0` (the repo has files only under `src/` and `Cargo.toml`: no `.claude`, no `.superpowers`, no `.factory`), and the fake `RoleRunner` from `tests/common/mod.rs` (extend it, in the test file or the common module, so the builder records the text of `<cwd>/.claude/agents/factory-builder.md` and the reviewer records `factory-code-reviewer.md` when they run):
1. `Cli::try_parse_from(["factory","--project","p","--root","/x","status"])` is an error (conflict); `--project p status` parses.
2. A full job through `CliCtx::for_project`: `new`, `run 1` reaches `merged` on `v-4.0.0`, `main` is untouched, the state is under the home (`<home>/projects/p/state/board.json`), the worktree is under `<home>/projects/p/worktrees`, the builder and reviewer saw the preset agent text, and afterwards the target repo has no `.superpowers`, `.claude` or `.factory` folder and `git2` `statuses` of the main checkout is empty.
3. A repo with `.factory/override.toml` adding `risk_paths = ["docs/**"]` committed on `v-4.0.0` makes a job that touches `docs/x.txt` park as risky (`needs-human`), while the same job without the override merges. (Plan checks are empty for a repo layout the rust preset does not match, so the job passes the checks stage; assert the outcome, not the checks.)
4. `blocked` cases, each refusing `run 1` and `approve 1` before any agent runs (assert the runner call count is zero) with the reason text, while `status` and `show` still work: a broken `.factory/override.toml` (degraded), an override check command with `--workspace` (rejected command), and a `project.toml` whose `settings` lacks `base_branch` (hand-edit the file in the test).
5. An unknown project name is `CliError` with a clear message; a project whose repo folder was deleted is refused with "repo path is missing" and `status` is refused too (there is nothing to show).
6. The reviewer `AgentSpec` built by `reviewer_spec` has `tools == Some("Read,Grep,Glob")` (engine test next to the other spec tests).
Run: `cargo test -j4 -p factory-cli --test project_flag` — Expected: FAIL to compile.

- [ ] **Step 2: Implement the flag and the context**

Add the global flag in `cli.rs`. In `main.rs`, after the `Project` subcommand branch: when `cli.project` is `Some(name)`, build `Home::from_env()` and `CliCtx::for_project(...)` (owner and lock wait read the same way `from_env` does: factor that into one helper), print notes to stderr, then `dispatch`. Implement `for_project`, the three fields, `runnable`, and the `driver()` change in `commands.rs`; make `open_repo` and `base_of` in `project.rs` `pub(crate)`. In `reviewer_spec` set `spec.tools`. In `base_of` keep today's behavior for `project show`, but add `fn base_of_strict(file) -> Option<String>` used by `for_project` (returns `None` when `settings.base_branch` is absent or invalid).
Run: `cargo test -j4 -p factory-cli --test project_flag` and `-p factory-engine` — Expected: PASS.

- [ ] **Step 3: Run everything and commit**

Run one at a time: `cargo test -j4 -p factory-core`, `-p factory-git`, `-p factory-engine`, `-p factory-cli`, `cargo clippy -j4 -p factory-cli -p factory-engine --all-targets`.
Commit via the commit skill: `feat(cli): add --project`, `fix(engine): restrict the reviewer tools`.

---

### Task 3: Hardening carried over from plan 3

**Files:**
- Modify: `crates/factory-engine/src/build.rs` (`restore_agent_file`)
- Modify: `crates/factory-core/src/project.rs` (`add`, `layout`), `crates/factory-cli/src/project.rs` (`remove`, `show`)
- Modify: `crates/factory-core/presets/rust/config.toml`
- Modify: test files that change the process cwd or `FACTORY_HOME`
- Test: `crates/factory-engine/tests/agent_files.rs`, `crates/factory-core` unit tests, `crates/factory-cli/tests/project_cmd.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces: no new public interface. Behavior changes listed below.

- [ ] **Step 1: Failing tests**

1. Hardlink-safe restore (`agent_files.rs`): create a hardlink (`std::fs::hard_link`) from an outside file to `.claude/agents/factory-builder.md` in a worktree before a step with different resolved content; after the step the outside file still has its old content and the agent file has the resolved content and its own inode (unix `MetadataExt::ino` differs from the outside file's).
2. `project::add` race (core unit test): two threads add the same name with different repos at once (barrier): exactly one succeeds, the other gets "already exists", and the winner's folder and `project.toml` are intact (no `remove_dir_all` of a folder it did not create). Also: a failure after the folder was created (make the presets folder unwritable by creating a file where the `presets` directory should be) removes only the folder this call created.
3. `project::layout` with a `preset` containing a separator or `..` does not produce a path outside the home: make `layout` return a `Result` or use the validated name only; test that `resolve` for such a project reports a problem and the layout stays inside the home.
4. `remove` of a project with an unreadable `project.toml`: refused without `--force` with a message that names `--force`; with `--force` the home folder is removed. With `--force` or after the board check, remove also prunes the git worktree registrations of that project's worktrees folder (open the repo from the readable `project.toml` when possible: use `git2` `Repository::worktrees()` and prune the ones whose path is inside the project's worktrees folder; a stale registration whose folder is gone is pruned, a valid one is pruned with the working folder). It lists the `factory/*` job branches it left in place in the output, and never deletes branches. Test with a real worktree created by `Repo::create_worktree`.
5. `show` of a project whose repo is missing prints `disabled: <reason>` and succeeds, like `list` (adapt the test that asserted the failure).
6. Test hygiene: the relative-path test in `project_cmd.rs` restores the cwd through a drop guard (a small `struct CwdGuard`), also on panic; the `FACTORY_HOME` test in `home.rs` uses a guard struct that restores the previous value on drop and one process-wide mutex shared by every test that sets or reads the variable (a `static ENV_LOCK` in `factory-core`'s test support, exported for the cli tests if they need it).
7. Untested focus items from plan 3: an override with `checks = []` leaves the preset checks intact; `project show` reads the override from the base branch tip even when the repo's HEAD is on another branch (commit a different `.factory/override.toml` on a side branch and check out that branch); a symlinked `.claude` folder in the repo working tree is refused by the agent-file writer (`.claude` itself a symlink in the worktree).
8. Rust preset on a single-crate repo: a repo with `Cargo.toml` and `src/lib.rs` at its root, `src/x.rs` changed, gets a check plan from the preset (`plan_checks` returns `cargo check -j4` and `cargo test -j4`); a workspace repo still gets the `{crate}` commands. Use `factory_checks::plan_checks` with the parsed preset config.
Run each new test: Expected FAIL (or FAIL to compile).

- [ ] **Step 2: Implement**

1. `restore_agent_file`: for an existing regular file with different content, `remove_file` then `create_new` and write (a new inode), keeping the symlink and directory handling.
2. `project::add`: claim the project folder with a non-recursive `create_dir` (error `AlreadyExists` is "already exists"); create `agents`, `state`, `worktrees` inside it; write `project.toml` with `create_new`; on any later failure remove only the folder this call created. Do the duplicate-repo scan after the claim and remove our folder on a duplicate.
3. `project::layout`: validate the preset name (the same rule `resolve` uses) and return `Result<Layout, CoreError>`; adapt callers.
4. `remove` and `show` as in the tests above.
5. Rust preset: add a second `[[checks]]` rule `paths = ["src/**", "tests/**", "benches/**", "examples/**", "build.rs", "Cargo.toml"]` with `commands = ["cargo check -j4", "cargo test -j4"]`. Check how `plan_checks` de-duplicates commands and that a workspace diff matching both rules does not run the root commands in a virtual workspace (a root `Cargo.toml` without a package makes `cargo check -j4` check the whole workspace: only add the root-crate rule's paths that cannot match a workspace member, i.e. keep `src/**` style paths, and note in the README that a virtual workspace uses the `crates/*/**` rule).
6. Test guards as in the tests above.

- [ ] **Step 3: Run everything and commit**

Run one at a time: `cargo test -j4 -p factory-core`, `-p factory-git`, `-p factory-agent`, `-p factory-checks`, `-p factory-engine`, `-p factory-cli`, `cargo clippy -j4 --all-targets` for core, engine and cli (no new warnings), `cargo check -j4 --manifest-path dashboard/Cargo.toml`.
Update `README.md`: the project lease and what the busy message means, `--project`, when a project is blocked, `project remove` pruning behavior, the single-crate rust check rule.
Commit via the commit skill in small commits: `fix(engine): restore agent files into a new inode`, `fix(core): claim the project folder atomically`, `fix(cli): remove broken projects and prune worktrees`, `test: guard the process-global state in tests`, `feat(core): check single-crate rust repos`, `docs: document the lease and --project`.
