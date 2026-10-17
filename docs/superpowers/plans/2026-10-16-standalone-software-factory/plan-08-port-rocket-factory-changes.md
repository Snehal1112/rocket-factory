# Port Rocket's Later Factory Changes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `rocket-factory` gets the 19 factory commits that landed in Rocket's `tools/factory` and `tools/factory-server` after the extraction (batch committer, task excerpts, ticket hygiene, live token bar), so the standalone factory is a full replacement before Rocket moves over.

**Architecture:** The 19 commits are applied by hand in three groups, each a task. For every commit the implementer reads the original diff in the Rocket repo (`git -C /home/numericlabs/data/rocket/rocket show <hash>`), maps its paths (`tools/factory/<x>` is `<x>` here, `tools/factory-server/<x>` is `dashboard/<x>`), and re-implements it on top of the standalone code, which has since gained `Layout`, projects and presets, the lease, `factoryd`, the API and the daily budget. Behavior must match the Rocket originals; the tests that came with each Rocket commit are ported with it. Rocket-specific wording in agent prompts stays out of the presets (it moves into Rocket's project overrides in plan 9); the generic parts go into the `rust` and `go` presets.

**Tech Stack:** Rust 2021 (cargo 1.94), serde, toml, git2 (already in the workspace); vanilla JS for the dashboard. No new third-party crate.

**Spec:** `docs/superpowers/specs/2026-10-16-standalone-software-factory-design.md` (Plan series: step 8 is split; Risks: "Rocket depends on its in-repo factory files and hooks"; plan index "Carried facts": anything merged into Rocket's `tools/factory` after the extraction must be ported). Rocket's own specs for these changes: `docs/superpowers/specs/2026-10-17-factory-cost-reduction-design.md` and `docs/superpowers/specs/2026-10-17-factory-ticket-hygiene-design.md` (read them in the Rocket repo; they define the behavior).

## Global Constraints

- Work in `/home/numericlabs/data/rocket/rocket-factory`, on a branch `plan-08-port-rocket-factory-changes` created from `main`. Fast-forward `main` at the end. No push (no remote). Treat `/home/numericlabs/data/rocket/rocket` as read-only: never write, stage, commit, stash or switch branches there (it holds the user's uncommitted work).
- Every `cargo` command has `-j4`. Never `--workspace` or `--all`. Never set `CARGO_TARGET_DIR`. Test one crate at a time with `-p <crate>`; the dashboard is its own workspace: `cargo test -j4 --manifest-path dashboard/Cargo.toml`, node tests `node --test dashboard/tests/*.test.js`.
- Commits are conventional commits written through the `dev-workflow-skills:1-git-commit` skill. Stage explicit paths and commit with a pathspec, never `git add -A`, `--all` or `.`.
- No unwrap calls in production paths (tests may use expect). No shelling out to git (use `factory-git` / git2).
- `crates/factory-cli/tests/no_hardcoded_paths.rs` and its allowlist stay as they are. All repo paths come from `Layout` and `Home`; new tools such as the tracker command must take the repo and state locations from them.
- Behavior of the existing standalone features must not regress: projects and presets, the lease, `factoryd`, the API token, the daily budget, `factory install`, `project which`. Every test that passes on `main` today must pass after each task (run the crate's full test suite, one crate at a time).
- Board JSON, `events.jsonl`, `usage.jsonl` and `costs.json` formats stay compatible: new fields are optional with defaults.
- Fail closed: an unknown or invalid new setting makes the project degraded, as every other setting does (`deny_unknown_fields`). New keys must be added to `Config` before any preset or project file uses them. Rocket's `tickets_on_merge` must become a valid key (default `false`) in Task 3.
- The files under `.claude/` in this repo (`agents/factory-*.md`, `commands/factory.md`, `software-factory.md`, `factory/*`) are byte copies of Rocket's, kept because `agent_lint.rs` and the doc tests read them. Where a Rocket commit changed such a file, copy Rocket's current version of that file over the copy here (read it from the Rocket repo; do not hand-edit), and keep `agent_lint.rs` and the docs tests passing. The global command asset `crates/factory-core/assets/global-command.md` is separate: update it only if a ported change alters the command flow (the new ticket step is run by the driver, not by the command).
- Harness and commit rules from Rocket do not apply in this repo; the committer still runs with `HARNESS_GATE=0` as in Rocket.

## Review Focus

- The batch committer after a stop or crash: a job stopped between commit groups must not lose or duplicate commits; `restore_tree` after a soft reset leaves the worktree exactly as before.
- Task excerpts: a plan or spec with fenced code that contains `### Task` lines, a missing task heading, an excerpt larger than the plan, Windows line endings, or a task number past the last task must give a clear error or the safe full-plan fallback, never an empty prompt for the builder.
- A stale rework note must not leak into a later fix round, and a rework note with a newline or over 200 characters is still refused.
- Ticket step: a repo without a tracker, no `gh` on PATH, a failing network call, an issue reference that is a URL to another repo, a follow-up list in the spec that is empty: the merge itself must still succeed and the job must end `merged`; the failure is reported, never swallowed silently, and no ticket is created twice when the step reruns.
- Config: Rocket's real `config.toml` (including `tickets_on_merge = true` and the batch/committer keys) loads in the standalone `Config` without a problem; an unknown key still degrades the project.

---

### Task 1: Batch committer

**Rocket commits to port (oldest first):** `df48a0a4` feat: add batch commit switches and git resets; `0ba006dc` test: cover restore_tree after soft reset; `aaa2442f` feat: add the batch committer kind; `59e6a3c6` feat: run one committer per job; `125c5955` fix: recover a batch commit after a stop.

**Files:**
- Modify: whatever those commits touched under `factory-core` (config keys `commit_subagents`, `commit_budget_weighted` and any new batch keys), `factory-git` (soft reset, `restore_tree`), `factory-agent` (committer kind), `factory-engine` (commit/runloop wiring, recovery), the presets' `agents/factory-committer.md` and `commit-context.md` (`crates/factory-core/presets/{rust,go}`), and the byte copies under `.claude/` named in Global Constraints.
- Test: the tests added or changed by the five commits, ported to the standalone layout.

**Interfaces:**
- Consumes: `Layout`, `Home`, project settings layering (`resolve.rs`), the lease and `factoryd` paths unchanged.
- Produces, used by tasks 2 and 3: the same public names the Rocket commits introduced in `factory-engine`/`factory-git`/`factory-core` (keep the names; read them from the diffs), a `Config` that accepts every key the five commits added, presets whose committer agent file knows the `batch` kind, and a generic `Batch runs` section in the presets' `commit-context.md` (Rocket's wording about `HARNESS_GATE`, its scopes and its issue footer rules stays out of the presets).

- [ ] **Step 1: Read the originals**

For each of the five commits run `git -C /home/numericlabs/data/rocket/rocket show <hash> --stat` and then the diff of the `tools/factory` paths. Write a short table in the task report: commit, files, what moves where here, anything that no longer exists in the standalone code. Also read Rocket's `docs/superpowers/specs/2026-10-17-factory-cost-reduction-design.md`.

- [ ] **Step 2: Failing tests first**

Port the tests of the five commits to the standalone layout (their temp-repo helpers and `Fixture`s differ: use `crates/factory-engine/tests/common` and `factory-cli/tests/{common,support}`). Add a test for each Review Focus line of this task: stop between commit groups then resume (no lost, no duplicate commit), `restore_tree` after a soft reset, a failing committer run leaves the worktree as it was.
Run: `cargo test -j4 -p factory-git`, `-p factory-agent`, `-p factory-engine`, `-p factory-core` — Expected: the new tests FAIL (or fail to compile).

- [ ] **Step 3: Implement**

Re-implement the five commits on the standalone code, oldest first, keeping each as its own commit where it is a separate concern (the first two together are fine). Update the presets and the `.claude/` byte copies as described. Do not copy Rocket-specific wording into the presets.

- [ ] **Step 4: Verify**

Run each crate's full suite one at a time: `cargo test -j4 -p factory-core`, `-p factory-git`, `-p factory-agent`, `-p factory-engine`, `-p factory-checks`, `-p factory-cli` (including `agent_lint` and `no_hardcoded_paths`), `-p factory-supervisor`, plus `cargo clippy -j4 -p <crate> -- -D warnings` for the crates changed. Expected: PASS, no warnings. The known flaky test `factory-supervisor/tests/recovery.rs::a_second_locked_supervisor_is_refused_until_the_first_is_dropped` may fail about 1 in 6 runs; rerun it alone before reporting a failure.

- [ ] **Step 5: Commit**

Commit through the commit skill, for example `feat(engine): run one batch committer per job`.

---

### Task 2: Task excerpts and the stale rework note

**Rocket commits to port (oldest first):** `0e8cd258` feat: cut a task excerpt from plan and spec; `a8b5eb9d` fix: ignore fenced lines in excerpts; `ba84f7cf` feat: give task builders only their task; `dd1c219b` fix: pin the builder excerpt prompt; `4574849c` fix: read the goal section and guard HEAD; `d73582e1` fix: keep a stale rework note out of fix rounds.

**Files:**
- Modify: whatever those commits touched in `factory-core` (excerpt parsing), `factory-engine` (builder prompt assembly, rework note handling, HEAD guard), the presets' `agents/factory-builder.md` (the generic task-excerpt rule; Rocket's `yarn`, shadcn, Tauri and V8 text stays out), and the `.claude/` byte copies named in Global Constraints.
- Test: the tests added or changed by the six commits, ported.

**Interfaces:**
- Consumes: Task 1's engine and preset state.
- Produces: task-only builder prompts (the builder receives its task's excerpt, not the whole plan), unchanged public names from the Rocket commits, and presets whose builder agent files describe the excerpt rule in generic terms.

- [ ] **Step 1: Read the originals**

Same as Task 1 step 1 for the six commits, and Rocket's cost-reduction spec sections on excerpts.

- [ ] **Step 2: Failing tests first**

Port the tests of the six commits. Add one test per Review Focus line of this task: fenced code containing `### Task`, a missing task heading, an excerpt requested past the last task, CRLF line endings, an excerpt for a plan with a single task, and a stale rework note not reaching a later fix round.
Run: `cargo test -j4 -p factory-core` and `-p factory-engine` — Expected: the new tests FAIL.

- [ ] **Step 3: Implement**

Re-implement the commits oldest first on the standalone code. When an excerpt cannot be cut, fall back exactly as the Rocket code does (read the original) and make the fallback visible in the job's events.

- [ ] **Step 4: Verify**

Run each crate's full suite one at a time as in Task 1 step 4 (`factory-core`, `factory-engine`, `factory-cli` with `agent_lint` and `no_hardcoded_paths`, `factory-supervisor`) and clippy for the changed crates. Expected: PASS.

- [ ] **Step 5: Commit**

Commit through the commit skill, for example `feat(engine): give task builders only their task`.

---

### Task 3: Ticket hygiene and the live token bar

**Rocket commits to port (oldest first):** `e62ee855` feat: parse issue refs from hash and github urls; `2a4f4dc2` feat: read follow-ups from a spec; `0827aeaf` feat: use Relates to footer when spec has follow-ups; `511e7905` feat: add tracker port and tickets step; `4f811230` feat: run the tickets step after a merge; `dd3addcf` feat: add the tickets command; `c165c022` docs: document ticket hygiene and the tickets command; `dd48d0e5` feat: make the card token bar follow the running agent (this one is in `tools/factory-server`, which is `dashboard/` here).

**Files:**
- Modify: whatever those commits touched in `factory-core` (issue refs, follow-ups, the `tickets_on_merge` key, default false), `factory-engine` (tracker port, tickets step after a merge), `factory-cli` (the `tickets` command), `dashboard/` (token bar: `static/`, `src/`, tests), `README.md` and `CLAUDE.md` (document the command and the key), and the `.claude/` byte copies named in Global Constraints.
- Test: the tests added or changed by the eight commits, ported; plus new tests for the Review Focus lines.

**Interfaces:**
- Consumes: Tasks 1 and 2 state; `Config`, project settings; the tracker is a port with a fake in tests (the real one may use the `gh` CLI as Rocket does: read the original; if it shells out, keep it behind the port and out of any scanned production path that the git-CLI rule covers: only `git` is forbidden, not `gh`).
- Produces: the `tickets_on_merge` setting accepted in `project.toml [settings]` (so Rocket's real config loads), the `factory tickets` command, the dashboard token bar following the running agent, README sections for both.

- [ ] **Step 1: Read the originals**

Same as Task 1 step 1 for the eight commits, and Rocket's `2026-10-17-factory-ticket-hygiene-design.md`.

- [ ] **Step 2: Failing tests first**

Port the tests of the eight commits. Add tests for the Review Focus lines of this task: no tracker, no `gh` on PATH, a failing call, a URL to another repo, an empty follow-up list, a rerun creating no duplicate ticket, the job still ending `merged` in each case, and an end-to-end config test that loads the real Rocket `config.toml` text (copy its current contents into a test fixture) through `Config` with no problems. Dashboard: port the node tests for the token bar, and add one for a bar with no running agent.
Run: `cargo test -j4 -p factory-core`, `-p factory-engine`, `-p factory-cli`, `cargo test -j4 --manifest-path dashboard/Cargo.toml`, `node --test dashboard/tests/*.test.js` — Expected: the new tests FAIL.

- [ ] **Step 3: Implement**

Re-implement the commits oldest first on the standalone code. The tickets step reads the repo and state locations from `Layout`; a failure after a merge is reported in the job's output and events and never turns a merged job into a failed one.

- [ ] **Step 4: Verify and finish**

Run every suite once, one crate at a time: `factory-core`, `factory-git`, `factory-agent`, `factory-checks`, `factory-engine`, `factory-cli` (including `agent_lint`, `no_hardcoded_paths`, `global_command`), `factory-supervisor`, the dashboard cargo tests, the node tests, `bash dashboard/tests/smoke.sh` and `bash dashboard/tests/smoke-home.sh`, and clippy `-D warnings` for every changed crate. Update `docs/superpowers/plans/2026-10-16-standalone-software-factory/00-plan-index.md`: mark plan 8 done and say that the original step 8 is now plans 8 (this port), 9 (move Rocket, dry run, remove `tools/factory`) and 10 (RocketVault), the last two not written. In `README.md` list the ported features. Expected: all PASS.

- [ ] **Step 5: Commit**

Commit through the commit skill, for example `feat(engine): run the tickets step after a merge`.
