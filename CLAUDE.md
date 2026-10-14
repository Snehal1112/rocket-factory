# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

This is the software factory driver, a Rust workspace of its own (not part of the root Cargo workspace, not shipped with the app). Read `../../.claude/software-factory.md` first for the pipeline, job model, merge rules and dashboard. Do not repeat it here.

## Commands

Run from `tools/factory/`. Harness rules still apply: always `-j4`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.

```bash
cargo check -j4
cargo test -j4 -p factory-engine                 # one crate
cargo test -j4 -p factory-cli --test e2e         # one integration test file
cargo test -j4 -p factory-core <test_name>       # one test
cargo build -j4 -p factory-cli                   # binary: target/debug/factory
```

## Crate layering

```
factory-core    pure rules: board, ledger, config, risk, verdict, budget (no I/O deps)
factory-git     git2 layer: worktrees, diffs, no-ff merge (never shell out to git)
factory-agent   runs `claude -p` headless, parses the stream, writes usage and events
factory-checks  maps changed paths to check commands and runs them
factory-engine  the run loop: build, checks, review, decide, merge, sync, series (uses all four)
factory-cli     clap commands over the engine (`factory new|run|status|approve|rework|cancel|cleanup|tokens`)
```

- `factory-engine` talks to git, agents and checks through the traits in `ports.rs`. Tests use fakes, so keep new engine logic behind a port rather than calling a crate directly.
- `tick.rs` and `runloop.rs` drive one job through the stages. `decide.rs` holds the merge/fix/needs-human choice, `tasks.rs` the per-task builder loop, `series.rs` and `specs.rs` the multi-plan series.
- Board, ledger and events are written only by the driver. Agents never touch them. Board JSON must stay compatible with `factory-server` (`../factory-server`, separate workspace), so add fields as optional.
- The repo-level config is `.claude/factory/config.toml`. A missing or broken config must make every diff risky, so never default it to permissive.
- `factory-cli/tests/agent_lint.rs` lints `.claude/agents/factory-*.md`. Run it after editing an agent file.
