# Monorepo Subfolder Projects Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Register any folder inside a git repository as its own factory project, with jobs limited to that folder and safe merges when several projects share one repo.

**Architecture:** `ProjectFile` stores an optional `subpath`; `Layout` carries it as a `Scope` and `Driver::new` hands it to `factory_git::Repo`, which then owns every scope rule (workdir, path filtering, branch and worktree names). The engine reads those rules from the repo handle, enforces scope from the job diff, and takes a per-repo `flock` around the one step that writes the main checkout.

**Tech Stack:** Rust workspace (git2, fs2 flock via `lock_board`, clap, serde/toml).

**Spec:** `docs/superpowers/specs/2026-10-21-monorepo-subfolder-projects-design.md`

## Global Constraints

- Cargo: always `-j4`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Git only through `factory_git` (never shell out to `git`); process checks only through `procinfo.rs`; paths only through `Layout` (`factory-cli/tests/no_hardcoded_paths.rs` must pass).
- Board JSON stays compatible with `dashboard/`: new fields are optional.
- A project with no `subpath` behaves exactly as today. Old `project.toml` files without `subpath` still load.
- Scope rule: a job may change only files inside the project folder. The driver enforces it from the git diff. Out-of-scope jobs park as needs-human (`ParkKind::Other`), never auto-merge, and `factory approve` refuses them. The driver never reverts files.
- Overlap rule: in one repo, either one top-folder project or any number of disjoint subfolder projects.
- Subfolder job branches are `factory/<project>/<id>-<slug>`; top-folder branches stay `factory/<id>-<slug>`.
- Merge lock: bounded wait (30 seconds), then park with a clear reason. Never merge without it.
- Config paths (risk paths, check mappings) are relative to the project folder; reviewer and committer diffs stay repo-relative.
- Agent files stay at the worktree root (Claude Code finds project `.claude/agents` by walking up from the working directory, so a subfolder start still sees them; Codex gets the agent text inlined by `prepare_for_codex`). Do not add `.claude/` paths. The only `AgentRunner`-facing change is the spec `cwd` value, and it must work for `agent_cli = "claude"` and `"codex"`.
- The Codex support already on `main` is the base: `project::add` ends with `agent_cli: Option<&str>`, `Repo::common_git_dir()` exists, and `run_agent` calls `sync_provider` and `prepare_for_codex`. Reuse them; do not add a second git-dir helper.
- Comments are short full sentences ending in punctuation. Commit each task with the `dev-workflow-skills:1-git-commit` skill, never a freeform `git commit -m`.

## Review Focus

- A sibling folder with a shared name prefix (`apps/web2` next to `apps/web`) is not inside `apps/web`: compare path components, never string prefixes.
- A project name that starts with digits (`2fa`) must not be read as job id 2 by the top-folder branch scan.
- A rename across the boundary (`apps/web/a.rs` to `apps/api/a.rs`) is out of scope, because both paths are listed.
- Two projects in one repo using the same job id and slug get different branch names, worktree folders and git worktree names.
- A registered subfolder that was deleted, or `subpath` holding `..` or an absolute path, shows as a disabled project with a clear reason and never crashes `list`, `show` or the dashboard API.

---

### Task 1: Core model, layout scope and overlap rules

**Files:**
- Modify: `crates/factory-core/src/project.rs` (ProjectFile, read_file, add, find_by_path, build_layout, tests module)
- Modify: `crates/factory-core/src/layout.rs` (add `Scope`, field `Layout.scope`)
- Modify: `crates/factory-core/src/lib.rs` (export `Scope`)
- Modify (struct literals, add `scope: None`): `crates/factory-engine/tests/lease_heartbeat.rs:28`, `crates/factory-engine/tests/project_lease.rs:17`, `crates/factory-cli/tests/external_layout.rs:33`

**Interfaces:**
- Produces: `pub struct Scope { pub project: String, pub subpath: PathBuf }` (derive `Debug, Clone, PartialEq, Eq`); `Layout.scope: Option<Scope>`; `ProjectFile.subpath: Option<PathBuf>` with `#[serde(default, skip_serializing_if = "Option::is_none")]`; `ProjectFile::folder(&self) -> PathBuf` (repo joined with subpath, or repo); `project::add(home: &Home, name: &str, repo: &Path, subpath: Option<&Path>, preset_name: &str, base: Option<&str>, repo_ok: &dyn Fn(&Path) -> Result<(), String>, agent_cli: Option<&str>) -> Result<PathBuf, CoreError>` (new `subpath` argument after `repo`; `agent_cli` stays last; update the test-only `add` wrapper in the tests module and every caller); `find_by_path` returns the project whose folder contains the path, deepest first.

- [ ] **Step 1: Write failing tests** in the `project.rs` tests module, using the existing home and repo helpers there:
  - `add_stores_subpath_and_old_files_load`: add with `Some("apps/web")`, read back `subpath == Some("apps/web")`; a hand-written `project.toml` without `subpath` loads with `None`.
  - `subpath_must_be_relative_and_clean`: `add` and `read_file` reject `/abs`, `../x`, `a/../b`, and an empty path with a `CoreError::Config`.
  - `overlap_is_refused`: with `web` at `apps/web`, adding `apps/web/inner`, `apps/web`, and a top-folder project (`None`) all fail with a message containing `web`; adding `apps/web2` and `apps/api` succeeds. Adding `apps/web` when a top-folder project exists in that repo fails.
  - `find_by_path_picks_deepest_project`: `which` for `apps/web/src` returns `web`, for the repo root with only subfolder projects returns `None`, and for `apps/web2/x` returns `None`.
  - `layout_carries_scope`: `layout()` of the `web` project has `scope == Some(Scope { project: "web", subpath: "apps/web" })`; a top-folder project has `None`.
- [ ] **Step 2: Run** `cargo test -j4 -p factory-core project` and expect the new tests to fail to compile or fail.
- [ ] **Step 3: Implement** the interfaces above. Overlap is checked inside `add` before any folder is created, against every readable existing project with the same canonical `repo`, comparing with `Path::starts_with` (component-wise); the error names the other project: `project {other} already covers {path}`. `find_by_path` keeps its worktree handling and replaces the "ambiguous" error with the deepest match; two projects with identical folders remain an error. `read_file` rejects an invalid `subpath` with the existing `bad {path}: ...` error shape.
- [ ] **Step 4: Run** `cargo test -j4 -p factory-core` and `cargo check -j4` (the three test literals must compile). Expected: PASS.
- [ ] **Step 5: Commit** with the `dev-workflow-skills:1-git-commit` skill.

### Task 2: Git layer scope API

**Files:**
- Modify: `crates/factory-git/src/repo.rs` (fields, `open_top`, scope methods, `git_dir`, `own_branch_names`)
- Modify: `crates/factory-git/src/worktree.rs` (`JobRef.project`, `branch_name`, `dir_name`, `validate`)
- Modify: `crates/factory-git/src/error.rs` (variant `NotTopFolder`)
- Test: `crates/factory-git/tests/repo_basics.rs`, `crates/factory-git/tests/worktree.rs` (use `tests/common` `Fixture`)

**Interfaces:**
- Produces:
  - `GitError::NotTopFolder(PathBuf)`, display `"{path} is not the top folder of a git repository"` (the exact text the CLI prints today).
  - `Repo::open_top(path: &Path) -> Result<Repo>`: `Repo::open` plus the canonical-equality check that `open_repo` has today.
  - `Repo::with_scope(self, project: &str, subpath: &Path) -> Repo`; `Repo::project(&self) -> Option<&str>`; `Repo::subpath(&self) -> Option<&Path>`.
  - `Repo::workdir_in(&self, worktree: &Path) -> PathBuf` (worktree joined with subpath, or the worktree).
  - `Repo::worktree_root_of(&self, path: &Path) -> Option<PathBuf>` (the first folder under `worktrees_dir()` that holds `path`).
  - `Repo::out_of_scope(&self, paths: &[String]) -> Vec<String>` (paths not under the subpath; empty without a scope).
  - `Repo::scoped_paths(&self, paths: &[String]) -> Vec<String>` (subpath prefix removed; identity without a scope).
  - `Repo::own_branch_names(&self) -> Result<Vec<String>>`: with a scope, branches under `factory/<project>/` returned as `factory/<rest>`, other branches dropped; without a scope, `local_branch_names()` minus any `factory/` branch whose remainder contains `/`.
  - `Repo::job_ref(&self, id: &str, slug: &str) -> JobRef`; `JobRef.project: Option<String>`; `branch_name()` is `factory/<project>/<id>-<slug>` or `factory/<id>-<slug>`; `dir_name()` is `factory-<project>-<id>-<slug>` or `factory-<id>-<slug>`; `validate` also requires a project to be lowercase letters, digits and dashes.

- [ ] **Step 1: Write failing tests**:
  - `open_top_rejects_subfolder`: `open_top(<repo>/apps)` returns `NotTopFolder` whose text equals the old message; the repo root opens.
  - `out_of_scope_uses_components_not_prefixes`: with subpath `apps/web`, `["apps/web/a.rs","apps/web2/b.rs","README.md"]` gives `["apps/web2/b.rs","README.md"]`.
  - `scoped_paths_strips_prefix`: `["apps/web/src/a.rs"]` gives `["src/a.rs"]`; no scope returns the input unchanged.
  - `own_branch_names_ignore_other_projects`: branches `factory/001-a`, `factory/web/002-b`, `factory/2fa/003-c` seen from a top-folder repo give `["factory/001-a"]`; from a `web` scope they give `["factory/002-b"]`; from a `2fa` scope `["factory/003-c"]`.
  - `same_id_and_slug_in_two_projects_do_not_collide`: with two `Repo` handles scoped `web` and `api` over one fixture, `create_worktree(&repo.job_ref("001","x"))` succeeds for both, with different branches, folders and worktree names.
  - `workdir_in_and_worktree_root_of`: `workdir_in(<wt>)` is `<wt>/apps/web`; `worktree_root_of(<worktrees>/factory-web-001-x/apps/web)` is `<worktrees>/factory-web-001-x`.
- [ ] **Step 2: Run** `cargo test -j4 -p factory-git` and expect the new tests to fail.
- [ ] **Step 3: Implement** the interfaces. `create_worktree` keeps taking a `JobRef` and builds names from it. Store scope as two `Option` fields on `Repo`.
- [ ] **Step 4: Run** `cargo test -j4 -p factory-git`. Expected: PASS (existing worktree tests keep passing with `project: None`; update their `JobRef` literals).
- [ ] **Step 5: Commit** with the `dev-workflow-skills:1-git-commit` skill.

### Task 3: CLI and supervisor registration

**Files:**
- Modify: `crates/factory-cli/src/project.rs` (`open_repo`, `add`, `disabled_reason`, `list`, `show`)
- Modify: `crates/factory-cli/src/cli.rs` (help text of `ProjectCmd::Add`)
- Modify: `crates/factory-supervisor/src/api/read.rs` (`open_repo`, `describe`)
- Test: `crates/factory-cli/tests/project_cmd.rs`, the supervisor tests next to `read.rs` (`api/testutil.rs` helpers)

**Interfaces:**
- Consumes: `Repo::open_top`, `ProjectFile::folder`, `project::add` with `subpath` (Tasks 1, 2).
- Produces: both `open_repo(path, base)` functions become thin wrappers over `Repo::open_top` that keep today's error strings (`not a git repository: {e}` for other errors); the project list JSON gets `"subpath"` (string or null).

- [ ] **Step 1: Write failing tests** in `project_cmd.rs` (use its `make_repo`/`run` helpers; create `apps/web` and `apps/api` folders with a committed file each):
  - `add_accepts_a_subfolder`: `project add <repo>/apps/web --preset rust` succeeds, output ends with ` in apps/web`, the stored file has `subpath = "apps/web"` and `repo` is the repo top; name defaults to `web`.
  - `add_refuses_overlap_and_root_after_subfolder`: adding `<repo>` after `web` fails naming `web`.
  - `list_shows_subfolder_project_disabled_when_folder_is_gone`: remove `apps/web` on disk, `list` shows `disabled` with `project folder is missing`.
  - `which_resolves_the_deepest_project`: `which <repo>/apps/web/src` prints `web`.
  - Supervisor: a project list entry for a subfolder project carries `"subpath": "apps/web"`, a top-folder one `null`.
- [ ] **Step 2: Run** `cargo test -j4 -p factory-cli --test project_cmd` and `cargo test -j4 -p factory-supervisor`; expect the new tests to fail.
- [ ] **Step 3: Implement** `add`: find the top with `Repo::open(path)` (canonical `root()`), derive `subpath` by stripping the top from the canonical `path` (None when equal), name from the given folder, run the probe and base check against the top, pass `subpath` to `project::add`. `disabled_reason` and the supervisor `describe` add `if !file.folder().is_dir()` returning `project folder is missing`. `show` and `list` print the subpath when set.
- [ ] **Step 4: Run** both test commands again. Expected: PASS.
- [ ] **Step 5: Commit** with the `dev-workflow-skills:1-git-commit` skill.

### Task 4: Engine wiring: scope on the repo, branch scan, agent working directory

**Files:**
- Modify: `crates/factory-engine/src/compat.rs` (`Driver::new` applies the scope)
- Modify: `crates/factory-engine/src/runloop.rs:36` (use `own_branch_names`)
- Modify: `crates/factory-engine/src/build.rs:215,359`, `crates/factory-engine/src/sync.rs:30`, `crates/factory-cli/src/commands.rs:700` (build `JobRef` with `repo.job_ref`)
- Modify: `crates/factory-engine/src/build.rs` (add `Engine::agent_dir`; builder and fix specs use it; `write_agent_files` stays on the worktree root), `crates/factory-engine/src/review.rs:55`, `crates/factory-engine/src/ports.rs:175`
- Test: `crates/factory-engine/tests/agent_files.rs`, `crates/factory-engine/tests/new_job.rs`

**Interfaces:**
- Consumes: `Layout.scope`, `Repo::with_scope`, `own_branch_names`, `job_ref`, `workdir_in`, `worktree_root_of` (Tasks 1, 2).
- Produces: `Engine::agent_dir(&self) -> Result<PathBuf, EngineError>` (the repo's `workdir_in` of the job worktree), used as the `cwd` of the builder, fix and reviewer specs; agent files are written to `repo.worktree_root_of(&spec.cwd)` in `run_agent`.

- [ ] **Step 1: Write failing tests**:
  - `new_job_ids_ignore_other_projects_branches` (`new_job.rs`): a scoped driver for `web` over a repo that has `factory/001-x` and `factory/api/005-y` allocates id `001`; a top-folder driver over the same branches allocates `002`.
  - `builder_starts_in_the_project_folder` (`agent_files.rs`): scoped to `apps/web`, the builder spec's `cwd` is `<worktree>/apps/web`, and the agent file exists at `<worktree>/.claude/agents/` with no `.claude` created under `apps/web`.
  - `top_folder_project_cwd_is_unchanged`: without a scope the `cwd` equals the worktree path.
  - `codex_builder_runs_in_the_project_folder` (extend `tests/codex_prep.rs`): with `agent_cli = "codex"` and a scope, the builder spec after `prepare_for_codex` has `cwd == <worktree>/apps/web`, `sandbox == Some("workspace-write")`, and the committer spec keeps `cwd == <worktree>` with `repo.common_git_dir()` in `writable_roots`.
- [ ] **Step 2: Run** `cargo test -j4 -p factory-engine --test new_job --test agent_files`; expect failures.
- [ ] **Step 3: Implement.** `Driver::new` adds `.with_scope(project, subpath)` when `layout.scope` is set. Replace the four `JobRef { id, slug }` literals with `repo.job_ref(...)`. Where a spec is built with the worktree as `cwd`, use `agent_dir()` for builder, fix and reviewer roles only; committer, message and kb roles keep the worktree root.
- [ ] **Step 4: Run** `cargo test -j4 -p factory-engine` and `cargo test -j4 -p factory-cli`. Expected: PASS.
- [ ] **Step 5: Commit** with the `dev-workflow-skills:1-git-commit` skill.

### Task 5: Project-relative paths for checks, risk and gates

**Files:**
- Modify: `crates/factory-engine/src/checks_stage.rs:80-82,90-100,135-140` and `crates/factory-engine/src/review.rs:53,185`
- Test: `crates/factory-engine/tests/build_checks.rs`, `crates/factory-engine/tests/gates.rs`

**Interfaces:**
- Consumes: `Repo::scoped_paths`, `Engine::agent_dir` (Tasks 2, 4).
- Produces: `Engine::job_changed_paths` returns project-relative paths (scoped); a new `Engine::job_changed_paths_raw(&self) -> Result<Vec<String>, EngineError>` returns repo-relative ones (used by Task 6). Check commands run with the project folder as working directory (`run_checks(&self.agent_dir()?, &plan)`); `diff_to_files` and the reviewer and committer diffs stay repo-relative.

- [ ] **Step 1: Write failing tests** with a scoped driver for `apps/web`:
  - `checks_map_project_relative_paths`: a change to `apps/web/src/a.rs` runs the check mapped to `src/**` in the config.
  - `risk_paths_are_project_relative`: a change to `apps/web/migrations/1.sql` is risky when `migrations/**` is a risk path; a change to `apps/web/src/a.rs` is not.
  - `checks_run_in_the_project_folder`: a check command that prints its working directory shows `<worktree>/apps/web`.
  - `top_folder_project_paths_are_unchanged` (existing tests keep passing).
- [ ] **Step 2: Run** `cargo test -j4 -p factory-engine --test build_checks --test gates`; expect the new tests to fail.
- [ ] **Step 3: Implement.** Apply `repo.scoped_paths` to the path lists fed to `plan_checks`, the no-checks and no-tests gates, and the risk classification at the two `review.rs` sites; keep the raw list for anything that reads files from the diff.
- [ ] **Step 4: Run** `cargo test -j4 -p factory-engine`. Expected: PASS.
- [ ] **Step 5: Commit** with the `dev-workflow-skills:1-git-commit` skill.

### Task 6: Scope enforcement

**Files:**
- Create: `crates/factory-engine/src/scope.rs`
- Modify: `crates/factory-engine/src/lib.rs` (module and export), `crates/factory-engine/src/checks_stage.rs` (`checks_stage` start), `crates/factory-engine/src/merge.rs` (`try_merge`), `crates/factory-cli/src/commands.rs:522` (`approve`)
- Test: `crates/factory-engine/tests/gates.rs`, `crates/factory-engine/tests/merge.rs`, `crates/factory-cli/tests/project_cmd.rs` (or a new `crates/factory-cli/tests/scope.rs`)

**Interfaces:**
- Consumes: `Repo::out_of_scope`, `Engine::job_changed_paths_raw` (Tasks 2, 5).
- Produces: `pub fn scope_violation(repo: &Repo, base: &str, head: &str) -> Result<Option<String>, EngineError>`: `None` when there is no scope or nothing is outside; otherwise the reason `the job changed files outside {subpath}: {a}, {b}` listing at most 10 paths then `and N more`.

- [ ] **Step 1: Write failing tests**:
  - `out_of_scope_edit_parks_at_checks` (`gates.rs`): a builder that also writes `README.md` at the repo root parks in `NeedsHuman` with a reason containing `README.md`, and no checks ran.
  - `rename_across_the_boundary_is_out_of_scope`: moving `apps/web/a.rs` to `apps/api/a.rs` parks and lists both paths.
  - `merge_refuses_an_out_of_scope_head` (`merge.rs`): `merge_stage` returns `MergeResult::Parked` with the same reason, and main did not move.
  - `approve_refuses_while_paths_remain` (CLI): `factory approve <id>` on such a parked job fails with `CliError::Refused` containing the path; after the builder reverts the file via rework and checks pass, approve succeeds.
  - `top_folder_project_has_no_scope_check`.
- [ ] **Step 2: Run** the named test files; expect failures.
- [ ] **Step 3: Implement.** `checks_stage` calls `scope_violation` first and returns `self.park_gate(ParkKind::Other, reason)` on `Some`. `try_merge` returns `Step::Park(reason)` before the base-state checks. `approve` checks it after the existing stage checks. Do not add a `ParkKind` variant.
- [ ] **Step 4: Run** `cargo test -j4 -p factory-engine` and `cargo test -j4 -p factory-cli`. Expected: PASS.
- [ ] **Step 5: Commit** with the `dev-workflow-skills:1-git-commit` skill.

### Task 7: Shared per-repo merge lock

**Files:**
- Create: `crates/factory-engine/src/repo_lock.rs`
- Modify: `crates/factory-engine/src/lib.rs`, `crates/factory-engine/src/compat.rs` (Driver field `merge_lock_wait`), `crates/factory-engine/src/merge.rs` (`try_merge`)
- Test: `crates/factory-engine/tests/merge.rs`

**Interfaces:**
- Consumes: `lock_board` and `BoardLock` (`lock.rs`), the existing `Repo::common_git_dir()`.
- Produces: `pub const MERGE_LOCK_FILE: &str = "factory-merge.lock"`; `pub const MERGE_LOCK_WAIT: Duration = Duration::from_secs(30)`; `pub fn lock_repo_merge(repo: &Repo, wait: Duration) -> Result<BoardLock, StoreError>` (file in `repo.common_git_dir()`); `Driver::set_merge_lock_wait(&mut self, wait: Duration)`. The advisory lock is released by the OS when its holder dies, so no stale-lock cleanup is needed (this replaces the spec's pid-clearing sentence; same effect).

- [ ] **Step 1: Write failing tests** in `merge.rs`:
  - `merge_waits_for_the_repo_lock`: hold `lock_repo_merge(repo, 0)` in a thread for 300 ms with the driver wait set to 5 s; `merge_stage` returns `Merged` after the lock is released.
  - `merge_parks_when_the_lock_wait_runs_out`: hold the lock, set the driver wait to 100 ms; `merge_stage` returns `Parked` with a reason containing `another project is merging`, main did not move, and the job is in `NeedsHuman`.
  - `two_projects_merge_into_one_repo`: two scoped drivers over one repo (`web`, `api`), each with an approved job, merge from two threads; both end `Merged`, main contains both changes, and no commit is lost.
- [ ] **Step 2: Run** `cargo test -j4 -p factory-engine --test merge`; expect failures.
- [ ] **Step 3: Implement.** In `try_merge`, take the lock just before the second `base_state` check and hold it through `merge_no_ff` (drop at the end of the iteration). `StoreError::Locked` becomes `Step::Park("another project is merging into this repository, approve again to retry")`. Check whether `preflight_main`, `kb_refresh` or the tickets step write the main checkout; if one does, take the same lock around that write.
- [ ] **Step 4: Run** `cargo test -j4 -p factory-engine`. Expected: PASS.
- [ ] **Step 5: Commit** with the `dev-workflow-skills:1-git-commit` skill.

### Task 8: End to end and docs

**Files:**
- Create: `crates/factory-cli/tests/monorepo.rs`
- Modify: `README.md` (project add, `which`, scope rule, merge lock), `docs/project-toml-reference.md` (`subpath`, relative paths), `docs/getting-started.md` (registration note at line ~151), `docs/software-factory.md` (one short monorepo section)

- [ ] **Step 1: Write the failing test** `two_subfolder_projects_run_side_by_side` (reuse the setup of `project_flag.rs` and the `RoleRunner` in `tests/common`): one repo with `apps/web` and `apps/api`; register both; run one job in each with a builder that writes a file inside its own folder; assert both jobs reach `merged`, main holds both files, the branches are `factory/web/001-...` and `factory/api/001-...`, and `factory --project web status` shows only its own job. Add `out_of_scope_job_is_parked_end_to_end`: a third job whose builder also writes outside its folder parks and `approve` refuses.
- [ ] **Step 2: Run** `cargo test -j4 -p factory-cli --test monorepo`; expect failure until Tasks 1 to 7 are in (they will be), so fix only what this test exposes.
- [ ] **Step 3: Update the four docs** to match the spec (usage, overlap rule, scope rule, lock behavior, `subpath` key, the agent working directory for Claude and Codex).
- [ ] **Step 4: Run** `cargo check -j4`, `cargo clippy -j4`, `cargo test -j4 -p factory-core`, `-p factory-git`, `-p factory-engine`, `-p factory-cli`, `-p factory-supervisor`, and `cargo test -j4 -p factory-cli --test agent_lint --test no_hardcoded_paths`. Expected: all PASS with no clippy warnings.
- [ ] **Step 5: Commit** with the `dev-workflow-skills:1-git-commit` skill.

---

## Self-review notes

- Spec coverage: registration and file (Tasks 1, 3), overlap (1), layout/scope (1, 4), branches and worktrees (2, 4), scope enforcement incl. approve and renames (6), checks and risk paths (5), merge lock (7), supervisor field (3), docs and agent working directory for both CLIs (4, 8).
- Spec deviation, flagged: the lock needs no pid-based stale clearing because it is an OS advisory lock (Task 7). The spec's "whole merge or sync" lock is narrowed to the code that writes the main checkout; sync runs in the job worktree.
