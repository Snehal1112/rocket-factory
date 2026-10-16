# Standalone Software Factory: one factory, many projects

## Goal

Extract the software factory from Rocket into its own project that drives several repositories at the same time, with one job per project, central settings per project, and a supervisor service with an API and a dashboard.

## Why

The factory lives in `tools/factory/` of Rocket and is tied to it. The base branch is the fixed word `main`, the builder prompts name cargo and yarn, the default risk paths are Rocket's, and the agent files must be committed in the target repo. RocketVault (Go and bun, working branch `v-4.0.0`, `.claude/` ignored by git) cannot use it. The user wants one factory for all projects, started from any project's Claude session or from the factory's own CLI and dashboard.

## Decisions

- The new repo is named `rocket-factory` and lives at `~/data/rocket/rocket-factory`.
- Runs on the user's machine first. A server mode for a team stays possible later and is not built now.
- Concurrency: one job per project at a time, different projects in parallel. Several jobs inside one project are out of scope.
- Settings live centrally by default. A project's repo may carry an optional override.
- The code is extracted from `tools/factory` and `tools/factory-server`. Rocket then becomes a normal client and its in-repo factory is removed after the new one has run a real job there.
- Entry points are equal: a global `/factory` command in any project session, and the factory's own CLI and dashboard.
- Architecture: a local supervisor service starts the existing driver as separate processes (approach 2). An in-process multi-project engine (approach 3) is a possible later change. It stays cheap if the driver remains a library, takes its paths and settings explicitly, and the CLI stays a thin wrapper.
- The language stays Rust. The driver logic is reused, not rewritten.
- Token use is not changed by this design. Agent runs are the only token cost and they stay as they are: fresh agent per task, resume for fixes, one reviewer per clean round, per-job budget. A daily budget across projects is added.
- The dashboard creates jobs only from spec and plan files that already exist. The interview stays in a Claude session.
- The first plans of the new project (1 to 3) are built in a normal session, because the factory cannot target another repo before plan 3.

## Layout

The factory home is `~/.factory/`, movable with `FACTORY_HOME`.

```
~/.factory/
  config.toml            global: parallel limit, daily budget, listen address
  presets/               rust, go, node, generic
    <preset>/            config.toml, agents/factory-*.md, commit-context.md
  projects/
    <name>/
      project.toml       repo path, base branch, preset, checks, risk paths, budget, kb_path
      agents/            optional per-project agent overrides
      state/             board.json, events.jsonl, jobs/<date-NNN-slug>/
      worktrees/         factory-NNN-slug
```

All folder paths come from one layout struct that is passed in. No path is hardcoded in the engine.

### Settings and agent resolution

Three layers, later wins: the preset, the project's central folder, then an optional `.factory/` folder in the repo (`override.toml` and `agents/`).

- A repo override may add risk paths. It cannot remove any.
- `.factory/**` in a repo is always a risk path, so any change to it stops for the user.
- Overrides are read from the base branch tip, not from a job branch.

### Agent files in job worktrees

The `claude --agent` lookup uses the working directory. At job start the driver writes the resolved agent files into the job worktree's `.claude/agents/` and adds them to the worktree's git exclude file. Nothing needs to be committed in the target repo, so an ignored `.claude/` does not matter.

## Components

- `factory` CLI: thin wrapper that parses arguments and calls the library. `--project <name>` selects the project. Without a running supervisor it runs the driver directly.
- Driver library: unchanged logic. It takes root, config, layout and runner explicitly. The lock wait becomes a parameter. A new project lease allows one active job per project. The lease is checked in the driver, so the rule holds with and without the supervisor.
- `factoryd` supervisor: reads the registry, starts a driver process per job, enforces the parallel limit and the daily budget, re-adopts or parks jobs after a restart, and serves the API and dashboard.
- Project commands: `factory project add <path> --preset <name> --base <branch>`, `list`, `show`, `remove`.
- Presets: `rust` and `go` first. The cargo and yarn prompt text moves out of the driver into the presets.
- Global `/factory` command: installed once under `~/.claude/commands/`. It finds the project by matching the current repo path in the registry, runs the interview and planning, then calls `factory new` for that project.

## API

Local JSON over HTTP on `127.0.0.1`. Every call needs a token read from a file in `~/.factory/`.

- `GET /projects`, `GET /projects/{p}/jobs`
- `POST /projects/{p}/jobs` with spec path, plans path and optional model
- `POST /projects/{p}/jobs/{id}/approve`, `/rework`, `/cancel`
- `GET /events`: live event stream, filtered by project
- `GET /usage`: tokens per project and per day

## Plan series

1. Extract into a new repo with history kept (`git subtree split`). All existing tests pass. No behavior change.
2. Layout struct, injectable paths, lock wait parameter, `base_branch` setting.
3. Project registry commands, `project.toml`, three-layer settings, `rust` and `go` presets, prompt text moved to presets, agent files written and excluded in worktrees.
4. Project lease and the `--project` CLI flag.
5. `factoryd`: registry, spawning, limits, recovery.
6. API, token, event stream, multi-project dashboard.
7. Global `/factory` command and the daily budget.
8. Move Rocket, dry run, remove `tools/factory` from Rocket, then add RocketVault.

Each plan has at most three tasks.

## Error handling

- If `factoryd` crashes, running jobs continue as separate processes and are re-adopted through their leases on restart.
- A project whose repo path is missing is shown as disabled with the reason. Other projects are not affected.
- A missing or invalid setting falls back to the fail-closed config, as today.
- A refused API call returns a stable error code. A missing or wrong token returns 401 and does nothing.

## Out of scope

- Server mode, login and remote checkouts.
- Several jobs in one project at once.
- An interview inside the dashboard.
- Moving the driver into the supervisor process (approach 3).
- Changing agent prompts beyond moving the Rocket-specific text into presets.
- Languages beyond `rust` and `go` presets in this series. `node` and `generic` are added when a project needs them.

## Acceptance criteria

- A project is registered with `factory project add` and needs no committed factory files in its repo.
- Two registered projects run a job at the same time. A second job in one project waits or is refused with a clear message.
- A repo on a branch other than `main`, such as RocketVault on `v-4.0.0`, runs a full job and merges to that branch.
- A repo with an ignored `.claude/` runs a job, because the agent files are written into the worktree.
- A repo override cannot remove a risk path, and a change under `.factory/` stops for approval.
- Killing `factoryd` during a job does not stop the job, and a restarted `factoryd` shows it again.
- The API rejects a call without the token and accepts one with it.
- The dashboard shows every project and its jobs, and creates a job from existing spec and plan files.
- The existing factory scenario tests pass in the new repo. New tests cover two concurrent projects, the lease, the layering and the API token.
- Rocket runs one real job through the new factory, then its `tools/factory` is removed.

## Risks

- Rocket depends on its in-repo factory files and hooks. Rocket moves last, and its copy stays until a real job has run through the new factory.
- The real-agent dry run has not been done even in Rocket. The first runs in each project may find faults, and the plans allow for that.
- Hidden global state: the check found almost none. It must be kept that way, and a test with two jobs in one process guards it.
- A project lease and a supervisor lock can disagree after a crash. Recovery treats a stale lease as parked, never as running.
