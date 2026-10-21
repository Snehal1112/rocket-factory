# Monorepo subfolder projects: design

Date: 2026-10-21. Status: draft for review.

## Intent

A monorepo holds several projects in one git repository. Today the factory refuses to register
any folder that is not the top folder of a git repository (`factory: <path> is not the top folder
of a git repository`, `open_repo` in `factory-cli/src/project.rs` and the copy in
`factory-supervisor/src/api/read.rs`). The factory treats "project" and "git repo top folder" as the
same thing. A monorepo user cannot point the factory at `apps/web` and work on that project alone.

The goal: register any folder inside a git repository as its own project, with its own queue,
settings, lease and board, and let the factory change only files inside that folder.

## Decisions (from the user)

- Each subfolder is an independent project: own job queue, settings, lease, board.
- Builds, checks and reviews of different subprojects of one repo run in parallel.
- Merges and syncs into the base branch take a short lock shared by every project of that repo. A
  project that finds the lock held waits for it (a bounded time), then merges. If the wait runs
  out, the job parks with a clear reason. It never merges without the lock.
- A job may change files only inside the selected folder. The factory does not edit shared code
  outside it.
- Codex support (`docs/superpowers/specs/2026-10-21-codex-support-design.md`) landed on `main`
  while this was being planned. This work builds on it and works for both agent CLIs (see Agent
  working directory).

## Goals

- `factory project add <repo>/apps/web [--name web]` works for any folder inside a git repository.
- A project registered at a repo top folder behaves exactly as today (no stored subpath).
- Jobs of two subprojects of one repo can run at the same time and never corrupt each other's
  merges, branches or worktrees.
- The scope rule is enforced by the driver from the git diff, so it holds for any agent.

## Non-goals

- Editing outside the selected folder, including allow-lists for shared paths. A later request.
- Detecting monorepo tools (nx, turbo, cargo workspaces) or inferring subprojects.
- Anything specific to codex or any agent besides the existing `claude -p` runner.
- Changing the board JSON in a way `factory-server` (`dashboard/`) cannot read. New fields are optional.

## Anti-goals

- Loosening the `open_repo` check so it merely accepts the folder. Jobs would then run against the
  wrong root. Registration must be paired with the full scoping below.
- A silent revert of out-of-scope edits. Work must not be lost without the user knowing.

## Design

### Registration and the project file

`ProjectFile` (`factory-core/src/project.rs`) gains an optional `subpath`: the folder of the project
relative to the git top folder, stored with `/` separators. Absent means the project is the repo
top folder (today's behavior). `repo` keeps meaning the git top folder (the main checkout). The old
files without `subpath` keep loading.

`factory project add <path>`:

1. Canonicalize `<path>`. Find the git top folder with `Repo::open`.
2. If `<path>` equals the top folder, store no `subpath`. Otherwise store the relative `subpath`.
3. The name defaults to the lowercased folder name of `<path>` (the subfolder), as today.
4. The base branch check is unchanged, and runs against the git top folder.

`open_repo` in both the CLI and the supervisor is replaced by one shared function that takes the
registered `repo` and `subpath`, requires `repo` to be the exact git top folder, and requires
`repo/subpath` to be an existing directory. It lives once (in `factory-git` or `factory-core`) so
the CLI and supervisor stop carrying copies. The error text for a real mismatch stays as it is.

Two projects may not have the same `repo` and overlapping scopes: equal subpaths, one inside the
other, or a top-folder project (which covers the whole repo) next to any subfolder project. `add`
refuses with a message naming the existing project. So scopes never overlap, and a repo holds
either one top-folder project or any number of disjoint subfolder projects. Existing top-folder
projects stay valid because they are alone in their repo today. (Builder-check call.)

`factory project which <path>` resolves to the project whose `repo/subpath` contains `<path>`. The
deepest match wins, consistent with the existing "inner one wins" rule.

### Layout

`Layout` (`factory-core/src/layout.rs`) gains the subpath-aware folder for the agent: a `workdir`
equal to `repo` for a top-folder project and `repo/subpath` otherwise. State, jobs and worktrees
already live under the project's own factory home folder for registered projects, so two
subprojects of one repo do not share them. No new hardcoded paths (the
`no_hardcoded_paths` test must keep passing).

### Job branches and worktrees

Job branches are named `factory/<id>-<slug>` and the next id is derived from the repo's branches
(`factory-core/src/ids.rs`, `factory-git/src/worktree.rs`). Two projects in one repo each have
their own board and their own id counters, so their branches would collide and the id scan would
mix them. The builder must make branch names unique per project, for example by including the
project name, and update the id scan and every `starts_with("factory/")` use so a project only
sees its own branches. Settled calls: a top-folder project keeps `factory/<id>-<slug>` unchanged;
a subfolder project uses `factory/<project>/<id>-<slug>`, and the legacy id scan ignores any branch
with a further `/` after the prefix. The project name is the registry key and is stable (there is
no rename command), so no separate project id is added. The builder chooses the remaining details.
It must keep the `factory/` prefix so
existing cleanup and ownership checks (`worktree.rs:150`, `config.rs`, `project.rs:294`) keep
working, and the existing projects' branches must keep working unchanged.

Each job worktree is a full checkout of the repo. The agent is started in `<worktree>/<subpath>`.

### Scope enforcement

After the builder (and after every fix round), the driver lists the job's changed paths with the
existing `changed_paths(base, head)`. Any path not under `subpath` is out of scope. When any exist:

- The job is parked as needs-human with a reason that lists the offending paths.
- It is never auto-merged, whatever the risk rules say, and `factory approve` refuses it while
  out-of-scope paths remain.
- The user can run the existing rework flow. No new approval command is added. Each check runs
  `changed_paths(base, head)` for the whole job, so the builder must revert the out-of-scope files
  (the driver does not revert them) and the check passes once none remain.
- A rename or copy across the boundary counts as out of scope when either side is outside. The
  builder verifies that `changed_paths` reports both the old and the new path and fixes it if not.

This runs in the driver, not in an agent prompt, so it is the same for every agent. A top-folder
project (no `subpath`) has no scope check. Both the rule and the parked reason are covered by tests
in `factory-engine`.

### Checks and risk paths

All factory config paths (risk paths, check mappings), from whichever source the config comes, are
relative to the project folder for a subfolder project. The diff given to the reviewer and the
committer stays repo-relative, and since out-of-scope jobs are parked it always lies inside the
folder anyway. The engine passes `factory-checks` the changed paths with the `subpath` prefix
removed, and runs check commands with the working directory set to the subfolder. A project at the
top folder is unchanged. The repo-level `.claude/factory/config.toml` rule still holds: a missing
or broken config makes every diff risky.

### The shared merge lock

Merges run in the repo's main checkout with the base branch checked out
(`factory-git/src/merge.rs`). The project lease only prevents two jobs in one project. A new
per-repo lock file, kept in the git common dir (so linked worktrees and all projects of that repo
see the same file), is held for the whole merge or sync, including the safe checkout and the ref
move. Acquisition retries for a bounded time (suggested 30 seconds, builder's call) and then
parks the job with a clear reason and does not lose work. It never proceeds without the lock. The
lock is released on drop and on a crashed holder (a stale lock whose pid is gone is cleared,
through `procinfo.rs`, never `/proc`). A top-folder project takes the same lock, which costs
nothing since it is alone in its repo.

### Supervisor and dashboard

`factoryd` runs one `factory` child per project today. That stays. Projects of one repo are
different projects, so they run side by side, and the merge lock serializes only the merge. The
project list API returns the optional `subpath` for display. The dashboard needs no change beyond
tolerating the new optional field.

## Agent working directory (Claude and Codex)

Both runners start the agent in `AgentSpec.cwd`. For the builder, fix builder and reviewer roles the
driver sets `cwd` to the project folder inside the job worktree (`Repo::workdir_in`); the committer,
message and KB roles keep the worktree root, because they run git on the whole checkout.

- Claude: agent files stay at the worktree root. Claude Code finds project `.claude/agents` by
  walking up from the working directory (documented), so a subfolder start still sees them.
- Codex: agent text is inlined into the prompt by `prepare_for_codex`, so no agent file is looked
  up. The `workspace-write` sandbox is rooted at `cwd`, so a subfolder start also keeps the builder
  from writing outside its folder. This is a second line of defense; the driver's diff check stays
  the rule. The committer's writable git folder is `Repo::common_git_dir()`, unchanged.

Nothing new about `.claude/` is hardcoded, and the `AgentRunner` trait does not change.

## Constraints

- Follow `CLAUDE.md`: `-j4`, no `--workspace`, no `CARGO_TARGET_DIR`; git only through `factory_git`
  (never shell out); processes only through `procinfo.rs`; paths only through `Layout`.
- Board, ledger and events are written only by the driver; board JSON changes are optional fields.
- `factory-cli/tests/agent_lint.rs` and `no_hardcoded_paths.rs` keep passing.
- Docs to update: `README.md` (project add, `which`, scope rule), `docs/project-toml-reference.md`,
  `docs/getting-started.md` (the registration note that the repo "must be given by its top
  folder"), `docs/software-factory.md`.

## Left to the builder

- The exact branch name format and the id scan change.
- Where the shared helper for opening and validating a project repo lives.
- The merge lock timeout and its file name.
- How checks receive the relative paths and working directory.
- Test layout. Required coverage: add at a subfolder, add rejected for overlap, old project files
  still load, out-of-scope diff parks the job, two projects of one repo merge without losing
  either, stale lock cleared.
