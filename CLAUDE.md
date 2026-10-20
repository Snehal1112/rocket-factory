# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

This is rocket-factory, the standalone software factory: a Rust driver workspace at the repo root and a local dashboard in `dashboard/` (its own workspace). It was extracted from Rocket's `tools/factory` and `tools/factory-server` with history. Read `docs/software-factory.md` first for the pipeline, job model, merge rules and dashboard. Do not repeat it here. Every `project.toml` setting, with defaults and examples, is in `docs/project-toml-reference.md`. A new user starts with `docs/getting-started.md` (build, PATH, install, register, first run). The design for the standalone version is `docs/superpowers/specs/2026-10-16-standalone-software-factory-design.md`.
Releases go through `scripts/release.sh` and `docs/releasing.md`; never tag by hand.

## Commands

Run from the repo root. Always `-j4`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`. Rocket's hooks enforced these, and this repo has no hooks yet, so follow them by hand.

```bash
cargo check -j4
cargo test -j4 -p factory-engine                 # one crate
cargo test -j4 -p factory-cli --test e2e         # one integration test file
cargo test -j4 -p factory-core <test_name>       # one test
cargo build -j4 -p factory-cli                   # binary: target/debug/factory
cargo test -j4 -p factory-supervisor             # factoryd
cargo run -j4 --manifest-path dashboard/Cargo.toml -- --root <checkout> --port 8765   # dashboard, one project
cargo run -j4 --manifest-path dashboard/Cargo.toml -- --home ~/.factory --port 8765     # dashboard, every project via factoryd's API
```

## Crate layering

```
factory-core    pure rules: board, ledger, config, risk, verdict, budget (no I/O deps)
factory-git     git2 layer: worktrees, diffs, no-ff merge (never shell out to git)
factory-telemetry  structured logging: tracing layers, redaction, rotating log file (no factory deps)
factory-agent   runs `claude -p` headless, parses the stream, writes usage and events
factory-checks  maps changed paths to check commands and runs them
factory-engine  the run loop: build, checks, review, decide, merge, sync, series (uses all five)
factory-cli     clap commands over the engine (`factory new|run|status|approve|rework|cancel|cleanup|tokens|tickets|project`)
factory-supervisor  `factoryd`: queue of run requests, one `factory` child per project, adopts children after a restart
```

- Only two things in `factory-engine` sit behind traits: `AgentRunner` (`factory-agent/src/runner.rs`, fakes in tests) and `Tracker` (`tracker.rs`). Git is the concrete `factory_git::Repo`, and checks (`factory_checks`) and `procinfo.rs` are called directly. `ports.rs` holds no traits, only error mappers and board/ledger helpers on `Engine`. For new agent or tracker logic use the existing trait; do not assume a fake exists for git or checks.
- `tick.rs` and `runloop.rs` drive one job through the stages. `decide.rs` holds the merge/fix/needs-human choice, `tasks.rs` the per-task builder loop, `series.rs` and `specs.rs` the multi-plan series.
- Board, ledger and events are written only by the driver. Agents never touch them. Board JSON must stay compatible with `factory-server` (`dashboard/`, separate workspace), so add fields as optional. The dashboard has a `--home` mode that holds the API token server-side (`dashboard/README.md`); keep the token out of every reply.
- The repo-level config is `.claude/factory/config.toml`. A missing or broken config must make every diff risky, so never default it to permissive.
- `factory-cli/tests/agent_lint.rs` lints `.claude/agents/factory-*.md`. Run it after editing an agent file.

## Ticket hygiene

- `factory tickets <id> [--dry-run]` and the setting `tickets_on_merge` (default false, a valid `Config` key) are documented in `README.md` (Ticket hygiene). Logic lives in `factory-engine/src/tickets.rs`, the `Tracker` port and the `gh` implementation in `tracker.rs`; `factory-core/src/commitmsg.rs` parses issue refs.
- The step runs after a merge in `merge.rs` and must never change the merge result: report failures to the ledger, stderr and events. Rocket's real `config.toml` is a test fixture in `factory-core/tests/fixtures/`; refresh it when Rocket's changes.
- Only `git` may not be shelled out to. `gh` is allowed, behind the port.

## Home, projects and leases

- `README.md` documents the factory home (`$FACTORY_HOME`, else `~/.factory`), presets, `factory --project <name>`, the project lease and `factoryd`. Read it before touching `factory-core` (`home`, `project`, `preset`, `layout`) or `factory-supervisor`.
- Paths reach the driver only through `Layout`. `factory-cli/tests/no_hardcoded_paths.rs` fails on hardcoded ones.
- Process checks (does a pid exist, is it a zombie, when did it start) go through `factory-engine/src/procinfo.rs` only, built on `sysinfo`, for Linux and macOS alike. It refreshes only the pid it asks about. macOS reports an unreaped zombie as gone, Linux as a zombie; neither is alive. Never read `/proc` directly.
- `factory-agent` ships a `fake-claude` binary that tests use instead of the real `claude`.

## Carried over from Rocket

These files in `.claude/` are copies of Rocket's factory files. Tests read them. They are Rocket-specific until later plans replace them: `agents/factory-*.md` and `factory/config.toml` become presets (plan 3), the Rocket copy `commands/factory.md` was removed, so the only command is `factory-core/assets/global-command.md`, installed with `factory install` (plan 7), and `factory-cli/tests/agent_lint.rs` pins its text. The prose docs `software-factory.md` and `getting-started.md` live in `docs/`.
