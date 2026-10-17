# Global /factory Command and Daily Budget Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One `/factory` command installed under the user's Claude commands folder that works in any registered project, and a daily token budget across all projects that `factoryd` enforces and shows.

**Architecture:** `factory-core` gets a path-to-project lookup (`project::find_by_path`) and the global command text as an embedded asset. `factory-cli` gets `factory project which [<path>]` and `factory install`, which writes the asset into the commands folder (the folder name literal lives in the allowlisted `layout.rs`). `factoryd` gets a `daily_budget_weighted` home setting; the scheduler holds new `Run` requests while today's weighted usage (UTC day, all projects) is at or over the budget, and `factoryd status`, `GET /usage` and the dashboard show it. A `factoryd is-running` check lets the command choose between queueing through `factoryd` and running the driver directly.

**Tech Stack:** Rust 2021 (cargo 1.94), serde, toml, clap (all already in the workspace), vanilla JS in `dashboard/static`. No new third-party crate.

**Spec:** `docs/superpowers/specs/2026-10-16-standalone-software-factory-design.md` (Components: global `/factory` command, `factoryd` enforces the parallel limit and the daily budget; Goals: a daily budget across projects is added, per-job budget unchanged; API: `GET /usage`).

## Global Constraints

- Work in `/home/numericlabs/data/rocket/rocket-factory`, on a branch `plan-07-global-command-and-daily-budget` created from `main`. Fast-forward `main` at the end. No push (no remote).
- Every `cargo` command has `-j4`. Never `--workspace` or `--all`. Never set `CARGO_TARGET_DIR`. Test one crate at a time with `-p <crate>`; the dashboard is its own workspace: `cargo test -j4 --manifest-path dashboard/Cargo.toml`, and node tests run as `node --test dashboard/tests/*.test.js`.
- Commits are conventional commits written through the `dev-workflow-skills:1-git-commit` skill. Stage explicit paths and commit with a pathspec, never `git add -A`, `--all` or `.`.
- No unwrap calls in production paths (tests may use expect). No shelling out to git (use git2 through `factory-git`).
- `crates/factory-cli/tests/no_hardcoded_paths.rs` forbids the literals `".superpowers`, `".claude` and `"main"` in `src` of the scanned crates. Never widen its allowlist. Ruling: the global commands folder name lives in `crates/factory-core/src/layout.rs` (already allowlisted) as `layout::claude_commands_dir(user_home: &Path) -> PathBuf`.
- Do not edit the byte copies of Rocket files under `.claude/` in this repo (`.claude/commands/factory.md`, `.claude/agents/*`, `.claude/software-factory.md`, `.claude/factory/*`). `agent_lint.rs` still checks them. The global command is a new asset, with its own lint test.
- Budget units are weighted tokens (`factory_core::budget::weighted_tokens`: cache reads count one tenth). A day is a UTC day, the same rule as `GET /usage`. The per-job budget is unchanged. `usage.jsonl`, board JSON and `events.jsonl` formats do not change; new fields are optional.
- Plan 5 and 6 rules hold: `factoryd` never kills children it did not start, never restarts a job after a crash, request files and API bodies are untrusted and validated, the API token never appears in logs or replies, stable API error codes never change, `factory-supervisor` does not depend on `factory-cli`.
- A missing or invalid daily budget setting must fail closed for `factoryd`: an invalid `config.toml` stops `factoryd` from starting (existing behavior); an unset budget means no daily limit.
- Without `factoryd` the `factory` CLI behaves as before.

## Review Focus

- `/factory` run in a directory that is inside no registered repo, in a git worktree or subfolder of a registered repo, through a symlinked path, or in a repo registered under another spelling: the command must resolve exactly the right project or stop with a clear message, never pick the wrong one.
- `factory install` run twice, with an existing user-edited `factory.md`, with a read-only commands folder, or with `$HOME` unset: no silent overwrite of an edited file, no partial file, a clear error.
- Daily budget exactly at the limit, one token under, a budget lowered below today's usage while jobs run, a day boundary passing while requests wait, and a `usage.jsonl` with bad lines: running children are never killed; only new `Run` requests wait; `Approve` requests are never held by the daily budget.
- A held `Run` request must not block other projects' requests behind it, must not be lost or reordered, and must start by itself after the day rolls over or the budget is raised.
- `daily_budget_weighted = 0`, a negative number, a string or an unknown key in `config.toml`: `factoryd` refuses to start with a clear message.

---

### Task 1: Project lookup, `factory install` and the global command

**Files:**
- Create: `crates/factory-core/assets/global-command.md`, `crates/factory-cli/tests/global_command.rs`
- Modify: `crates/factory-core/src/project.rs` (`find_by_path`), `crates/factory-core/src/layout.rs` (`claude_commands_dir`), `crates/factory-core/src/lib.rs`, `crates/factory-cli/src/cli.rs` (`project which`, `install`), `crates/factory-cli/src/project.rs`, `crates/factory-cli/src/commands.rs` or a new `src/install.rs`, `crates/factory-supervisor/src/bin/factoryd.rs` (`is-running`), `README.md`, `CLAUDE.md`
- Test: unit tests in `project.rs`, `layout.rs`, the install module and `factoryd.rs`; `tests/global_command.rs`; the existing `no_hardcoded_paths` and `agent_lint` tests must still pass

**Interfaces:**
- Consumes: `Home`, `project::{list, get, validate_name}`, `ProjectFile.repo` (canonical absolute path), `factory_git::Repo::open(path)?.root()` for the git root of a path, `supervisor::lock::running(home)`.
- Produces, used by tasks 2 and 3:
  - `project::find_by_path(home: &Home, path: &Path) -> Result<Option<String>, CoreError>`: canonicalizes `path`, then returns the registered project whose canonical `repo` equals the path or contains it; the longest matching repo wins when two registered repos are nested; a project whose entry is unreadable is skipped; no match is `Ok(None)`. A git worktree of a registered repo (a checkout whose `commondir` points into the repo) resolves to that project.
  - `factory project which [<path>]` (default: current directory): prints the project name and exits 0; prints `no registered project contains <path>` to stderr and exits 1 when none matches.
  - `layout::claude_commands_dir(user_home: &Path) -> PathBuf` (user home joined with the Claude config folder and `commands`).
  - `factory install [--dir <path>] [--force]`: writes the embedded `global-command.md` as `factory.md` into `--dir`, else `claude_commands_dir($HOME)` (a missing or empty `$HOME` without `--dir` is an error). The folder is created if absent. An existing file with identical content is a no-op (prints `already installed`); an existing file with different content is an error naming the file unless `--force`; writes are atomic (`write_atomic`). Prints the installed path.
  - `factoryd is-running`: exit 0 when the instance lock is held (a `factoryd` is running for this home), exit 1 otherwise, no output on success.
  - The asset `global-command.md`: frontmatter `description` and `argument-hint: "<feature> [--opus] | status | approve <id> | rework <id> <note> | cancel <id> | cleanup <id>"`. It resolves the project with `factory project which`, uses `FACTORY_BIN` else `factory` on PATH, passes `--project <name>` on every `factory` call, keeps the argument-shape rules, the interview (`superpowers:brainstorming`), the plan writing (`superpowers:writing-plans`, at most 3 tasks per plan, `Next plan:` line), the explicit approvals, the `git commit --only -- <paths>` commits through `dev-workflow-skills:1-git-commit`, and `factory new <slug> '<feature>' --plans ... --spec ... [--model opus]`. For the run it uses `factoryd enqueue --project <name> --job <id> --series` when `factoryd is-running` exits 0, else `factory --project <name> run <id> --series` as a background Bash call; either way it tells the user in one line and does not poll. It contains no Rocket-specific text (no cargo or `tools/factory` build step, no OpenCollection line) and takes the project's checks from the registry, not from the command.

- [ ] **Step 1: Failing tests for `find_by_path` and `claude_commands_dir`**

In `project.rs` tests (temp home, temp git repos made with git2): exact repo root, a subfolder, a path through a symlink to the repo, a git worktree checkout of the registered repo, a path outside every repo (`None`), two registered repos where one is nested inside the other (the longest match wins), a registered project whose repo folder is gone (skipped, other projects still match), and a path that does not exist (an error, not `None`). In `layout.rs`: `claude_commands_dir(Path::new("/h"))` ends with the Claude folder and `commands` under `/h`.
Run: `cargo test -j4 -p factory-core project layout` — Expected: FAIL to compile.

- [ ] **Step 2: Implement `find_by_path` and `claude_commands_dir`**

Compare canonical paths component-wise (`Path::starts_with`), not as strings. For the worktree case read the checkout's git common dir through `factory-git` (add a small accessor there if none exists) and map it back to the registered repo root; keep the git2 usage inside `factory-git`.
Run the same command — Expected: PASS.

- [ ] **Step 3: Failing tests for the CLI pieces**

`tests/global_command.rs` (Home and repos in tempdirs, `Cli::parse_from` plus `dispatch`, as `project_cmd.rs` does): `project which` prints the name for a registered repo and for a subfolder, exits non-zero with the message for an unregistered folder; `install --dir <tmp>` writes `factory.md` with the embedded text; a second run says `already installed`; a user-edited file is refused with the file named and is replaced with `--force`; an unwritable folder gives an error and leaves no partial file; no `$HOME` and no `--dir` is an error (use `EnvVarGuard`). `factoryd is-running` unit test: exit 1 with no lock, exit 0 while an `InstanceLock` is held.
Lint tests on the asset text (in the same file): required phrases `factory project which`, `FACTORY_BIN`, `--project`, `superpowers:brainstorming`, `superpowers:writing-plans`, `at most 3 tasks`, `explicit approval`, `Do not write any plan before that approval`, `Do not run`, `git commit --only --`, `dev-workflow-skills:1-git-commit`, `factoryd is-running`, `factoryd enqueue`, `--series`, `run_in_background`, `Do not poll`, `exact shape`, `rework needs a note`; forbidden phrases `tools/factory`, `cargo build`, `opencollection`, `factory-ctl`, `python3`, `set-stage`, `SendMessage`, `Rocket`.
Run: `cargo test -j4 -p factory-cli --test global_command` — Expected: FAIL.

- [ ] **Step 4: Implement the CLI pieces and the asset**

`project which` and `install` in `factory-cli` (the install code in its own small module); `is-running` in `factoryd.rs`. Write `global-command.md` by adapting the existing `.claude/commands/factory.md` (read it, do not edit it): keep its argument rules and step structure, replace the Rocket bits as described in Interfaces, and say in one sentence what happens when `factory project which` fails (tell the user to run `factory project add <path> --preset <name>`, then stop).
Run: `cargo test -j4 -p factory-cli --test global_command`, `cargo test -j4 -p factory-cli --test no_hardcoded_paths`, `cargo test -j4 -p factory-cli --test agent_lint`, `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-supervisor` and `cargo clippy -j4 -p factory-cli -p factory-core -p factory-supervisor -- -D warnings` — Expected: PASS, no warnings.

- [ ] **Step 5: README, CLAUDE.md, commit**

Document `factory install`, `factory project which`, `factoryd is-running` and the command's flow in `README.md`; update the `CLAUDE.md` line that says the command becomes global in plan 7. Commit: `feat(cli): install a global /factory command`.

---

### Task 2: Daily budget in factoryd

**Files:**
- Create: `crates/factory-supervisor/src/daily.rs`
- Modify: `crates/factory-core/src/homeconfig.rs` (`daily_budget_weighted`), `crates/factory-supervisor/src/scheduler.rs`, `src/supervisor.rs`, `src/api/usage.rs`, `src/bin/factoryd.rs`, `src/lib.rs`, `README.md`
- Test: unit tests in each changed file, `crates/factory-supervisor/tests/daily_budget.rs`

**Interfaces:**
- Consumes: `api::usage::{per_day, project_usage}` (make `project_usage` crate-visible), `project::list`, `budget_state`, `Supervisor::tick`, `SchedulerInput`, `Decision`, the injected clock.
- Produces, used by task 3:
  - `HomeConfig.daily_budget_weighted: Option<u64>` (`daily_budget_weighted` key in `config.toml`; absent means no limit; `0` is an error; `deny_unknown_fields` still applies).
  - `daily::DailyBudget { pub limit: u64, pub used: u64, pub resets_at_ms: u64 }` and `daily::status(home: &Home, limit: Option<u64>, now_ms: u64) -> Option<DailyBudget>` (`None` when no limit is set); `used` is the sum, over all registered projects, of today's UTC weighted tokens from every `jobs/*/usage.jsonl`; an unreadable project or bad line counts as zero; `resets_at_ms` is the next UTC midnight.
  - `SchedulerInput.daily_exhausted: bool`; when true, `decide` makes no `Start` decision for a `Run` request (it stays queued, uses no slot, and does not block later requests of other projects or any `Approve`), and `Reject` decisions are unchanged.
  - `GET /usage` response gains an optional top-level `"dailyBudget": {"limit", "used", "resetsAt"}` (RFC 3339 UTC string for `resetsAt`) when a limit is set; existing fields are unchanged.
  - `factoryd status` prints a `daily budget: <used> of <limit> weighted tokens today (<pct>%), resets at <time>` line when a limit is set, and appends `reached: new runs are waiting` at 100% or more and `warning` from 80%.

- [ ] **Step 1: Failing tests for config and `daily::status`**

`homeconfig.rs`: `daily_budget_weighted = 5000000` parses; absent is `None`; `0`, `-1`, `"x"` and `1.5` are errors. `daily.rs` (temp home with two registered projects and fixture `usage.jsonl` lines, a fixed `now_ms`): sums today's lines of both projects and ignores yesterday's, a cache-read line is weighted at one tenth, a bad line and a project with a missing folder count as zero, no limit gives `None`, `resets_at_ms` is the next UTC midnight (also across a month and a leap-day boundary), usage exactly at the limit reports `used == limit`.
Run: `cargo test -j4 -p factory-core homeconfig` and `cargo test -j4 -p factory-supervisor daily` — Expected: FAIL to compile.

- [ ] **Step 2: Implement the setting and `daily.rs`**

Reuse `per_day` and `project_usage` rather than parsing usage lines again.
Run the same commands — Expected: PASS.

- [ ] **Step 3: Failing tests for the scheduler and the supervisor**

`scheduler.rs`: with `daily_exhausted = true` a `Run` for a ready project gives no decision, an `Approve` for a ready project still starts, held `Run` requests of two projects do not block an `Approve` queued behind them, a `Reject` for a disabled project still rejects, and order and parallel-limit behavior are unchanged when `daily_exhausted = false`. `tests/daily_budget.rs` (fake `Spawner`, injected clock, temp home with usage fixtures): usage one token under the limit starts the run; exactly at the limit holds it; the held request file stays in the queue unchanged; raising the budget in `config.toml` (reload on the next tick) or moving the clock past UTC midnight starts it; a running child is never touched when the budget is crossed; an `Approve` request starts while the budget is exhausted.
Run: `cargo test -j4 -p factory-supervisor scheduler daily_budget` — Expected: FAIL to compile.

- [ ] **Step 4: Implement the hold**

`Supervisor::tick` computes `daily::status` once per tick from the home config (re-read `config.toml` each tick, keeping the last valid value if the file becomes invalid mid-run and logging one line), sets `daily_exhausted = used >= limit`, and passes it to `decide`. Log one line when the state flips, not on every tick.
Run: `cargo test -j4 -p factory-supervisor` — Expected: PASS.

- [ ] **Step 5: Status and API output, docs, commit**

Add the `daily budget` line to `factoryd status` and the `dailyBudget` field to `GET /usage` (tests: present with a limit, absent without, `resetsAt` shape). Document the setting, the hold rule (new runs wait, approvals and running jobs are never stopped) and the UTC day in `README.md`. Run `cargo clippy -j4 -p factory-supervisor -p factory-core -- -D warnings`. Commit: `feat(supervisor): hold new runs when the daily budget is reached`.

---

### Task 3: Show the daily budget in the dashboard

**Files:**
- Modify: `dashboard/src/home_mode.rs` (usage passthrough), `dashboard/src/server.rs`, `dashboard/static/app.js`, `dashboard/static/projects.js`, `dashboard/static/dashboard.html`, `dashboard/static/dashboard.css`, `dashboard/README.md`, `dashboard/tests/smoke-home.sh`
- Test: Rust unit tests in the changed modules, `dashboard/tests/projects.test.js`, `smoke-home.sh`

**Interfaces:**
- Consumes: `GET /usage` with the optional `dailyBudget` object from task 2.
- Produces: dashboard route `GET /api/usage` (home mode only; forwards to factoryd `GET /usage?days=14` with the token added server-side, same error mapping as the other API routes, 404 in `--root` mode) and a header strip in home mode showing today's usage against the limit with the reset time; pure helper `FactoryProjects.budgetStrip(usage) -> {text, level}` where `level` is `none` (no limit), `ok`, `warn` (>= 80%) or `stop` (>= 100%).

- [ ] **Step 1: Failing tests**

Rust (fake API): `/api/usage` forwards and returns the body, adds no token to the reply, maps an API 401 to `token_rejected`, is 404 in root mode. Node: `budgetStrip` returns `none` without `dailyBudget`, `ok` at 79%, `warn` at 80% and 99%, `stop` at 100% and above, formats counts readably, shows the reset time as UTC text, and escapes nothing unsafe (the text contains numbers and a fixed phrase only).
Run: `cargo test -j4 --manifest-path dashboard/Cargo.toml` and `node --test dashboard/tests/projects.test.js` — Expected: FAIL.

- [ ] **Step 2: Implement route, helper and strip**

The strip refreshes on the existing poll. At `stop` it says new runs are waiting. It is hidden when there is no limit.
Run the same commands — Expected: PASS.

- [ ] **Step 3: Smoke test, docs, plan index, commit**

Extend `smoke-home.sh`: with `daily_budget_weighted` set in the temp home's `config.toml` and a fixture `usage.jsonl`, `/api/usage` shows `dailyBudget`; without it the field is absent. Update `dashboard/README.md`. Mark plan 7 done in the plan index. Run before the final commit: `cargo test -j4 -p factory-supervisor`, `cargo test -j4 -p factory-cli --test global_command --test no_hardcoded_paths --test agent_lint`, `cargo test -j4 --manifest-path dashboard/Cargo.toml`, `node --test dashboard/tests/*.test.js`, `bash dashboard/tests/smoke-home.sh` — Expected: all PASS. Commit: `feat(dashboard): show the daily budget`.
