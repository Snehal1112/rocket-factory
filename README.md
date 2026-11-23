# rocket-factory

[![CI](https://github.com/Snehal1112/rocket-factory/actions/workflows/ci.yml/badge.svg)](https://github.com/Snehal1112/rocket-factory/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Snehal1112/rocket-factory)](https://github.com/Snehal1112/rocket-factory/releases)
[![License: MIT](https://img.shields.io/github/license/Snehal1112/rocket-factory)](LICENSE)
![Platforms](https://img.shields.io/badge/platforms-linux%20%7C%20macOS-lightgrey)

A standalone software factory. You describe a feature, and a Rust driver builds, checks, reviews and merges it one task at a time, each job in its own git worktree. Agents (`claude -p`, or Codex) write and review the code. The driver runs the checks, makes the commits, decides the merge and keeps the ledger, so no agent ever touches the board or the base branch. A local dashboard shows what it is doing across all your projects.

It was extracted from Rocket's `tools/factory` and now drives several projects at once.

## How it works

You run `/factory` in Claude Code. It interviews you, writes a spec and plans (at most 3 tasks each) and creates a job. The driver then takes over:

1. A fresh builder agent implements each task in the job worktree.
2. The driver maps the changed paths to check commands (build, test, format, lint) and runs them.
3. A reviewer agent (and optionally a security reviewer) reads the diff. Changes requested start a fix round, 2 at most.
4. The driver merges into the base branch, or parks the job as `needs-human` with the reason and the next commands.

Yellow steps spend LLM tokens, teal steps are the Rust driver, which spends none, and orange marks a stop for a human.

![One job, start to merge](docs/assets/images/factory-lifecycle.svg)

More diagrams: [what can stop a merge](docs/assets/images/factory-gates.svg), [who talks to whom](docs/assets/images/factory-processes.svg), [how an agent is started](docs/assets/images/factory-agents.svg) and [agent turns in one job](docs/assets/images/factory-turns.svg). All five as one page: [Rocket Factory Workflow](https://claude.ai/code/artifact/64fc3640-5deb-4003-a67d-60b5fc29f80b) (source `docs/assets/factory-workflow.html`).

## What you get

- **Safe merges.** A job merges only when its checks ran and passed and every reviewer approved. A diff on a risk path, a diff with no checks, or one outside its project folder waits for you. A missing or broken config makes every diff risky.
- **Many projects, one home.** Register any git repo, or a subfolder of a monorepo, with a preset (`rust`, `go`, `ruby`, `csharp`, `node`). Settings layer preset, `project.toml`, then the repo's `.factory/override.toml`.
- **A supervisor and a dashboard.** `factoryd` queues runs, runs projects in parallel, enforces a daily token budget and serves a local API. The dashboard shows every project on one page and can create, approve, retry, rework and cancel jobs.
- **Token control.** A per-job budget, prompt cache lifetime, builder session carry, a stronger reviewer model only for large or risky diffs, and `factory tokens` / `factory costs` reports to check the effect.
- **Ticket hygiene.** After a merge the driver files the spec's follow-ups as GitHub issues, comments on the parent tickets and closes them.
- **Crash safety.** Project and job leases, process groups for every child, and resume of an interrupted builder session.

## Quick start

```bash
cargo build -j4 -p factory-cli -p factory-supervisor     # target/debug/factory, factoryd
claude plugin marketplace add Snehal1112/rocket-factory
claude plugin install factory@rocket-factory             # the /factory command
factory project add /path/to/repo --preset rust
```

Then run `/factory <feature>` inside the repo. The full walk-through, with checks after each step, is [Getting started](docs/guides/getting-started.md).

To upgrade: `git pull`, rebuild, `factory upgrade --dry-run`, then `factory upgrade` with no jobs running, and restart `factoryd`. See [After you update the factory](docs/guides/getting-started.md#15-after-you-update-the-factory).

## Documentation

| I want to | Read |
|---|---|
| Install and run my first feature | [docs/guides/getting-started.md](docs/guides/getting-started.md) |
| Learn every command and recipe in plain words | [docs/guides/factory-user-guide.md](docs/guides/factory-user-guide.md) |
| Understand the pipeline, gates, merge rules, leases, logging | [docs/reference/software-factory.md](docs/reference/software-factory.md) |
| Look up a `project.toml` setting and its default | [docs/reference/project-toml-reference.md](docs/reference/project-toml-reference.md) |
| Look up the flags of `factory` and `factoryd` | [docs/reference/factory-admin-manual.md](docs/reference/factory-admin-manual.md) |
| Manage projects, presets, upgrades, leases and `factoryd` | [docs/reference/projects-and-supervisor.md](docs/reference/projects-and-supervisor.md) |
| Call the `factoryd` HTTP API | [docs/reference/factoryd-api.md](docs/reference/factoryd-api.md) |
| Run the dashboard | [dashboard/README.md](dashboard/README.md) |
| Cut a release | [docs/operations/releasing.md](docs/operations/releasing.md) |
| Find any other doc | [docs/README.md](docs/README.md) |

Contributors: read [CLAUDE.md](CLAUDE.md) for the build commands, crate layering and rules. The design is in [docs/superpowers/specs/2026-10-16-standalone-software-factory-design.md](docs/superpowers/specs/2026-10-16-standalone-software-factory-design.md).

## License

MIT, see [LICENSE](LICENSE).
