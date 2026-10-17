# Standalone Software Factory: plan index

Spec: `docs/superpowers/specs/2026-10-16-standalone-software-factory-design.md`

New repo: `~/data/rocket/rocket-factory` (empty git repo on `main`, no commits yet).

Each plan has at most 3 tasks and runs after the one before it. Plans 1 to 3 are built in a normal Claude session, because the factory cannot target another repo before plan 3. From plan 4 the new factory can build itself.

| Plan | File | What it delivers | Status |
|---|---|---|---|
| 1 | `plan-01-extract-into-rocket-factory.md` | `tools/factory` and `tools/factory-server` extracted into `rocket-factory` with history, tests green, spec and plans copied in | done |
| 2 | `plan-02-layout-and-base-branch.md` | `Layout` struct, injectable paths, lock wait parameter, `base_branch` setting | done |
| 3 | `plan-03-registry-presets-layers.md` | Project registry commands, `project.toml`, three-layer settings, `rust` and `go` presets, prompt text moved to presets, agent files written into worktrees | done |
| 4 | `plan-04-project-lease-and-project-flag.md` | Project lease (one job per project), `--project` CLI flag | done |
| 5 | `plan-05-factoryd.md` | `factoryd`: registry, spawning drivers, limits, recovery | done |
| 6 | `plan-06-api-and-dashboard.md` | API, token, event stream, multi-project dashboard | done |
| 7 | `plan-07-global-command-and-daily-budget.md` | Global `/factory` command, daily budget | done |
| 8 | `plan-08-port-rocket-factory-changes.md` | Port the 19 Rocket factory commits made after the extraction (batch committer, task excerpts, ticket hygiene, live token bar) | done |
| 9 | to be written before it runs | Register Rocket, dry run, one real job, remove `tools/factory` from Rocket | not written |
| 10 | to be written before it runs | Add RocketVault (go preset, bun checks, `v-4.0.0`, ignored `.claude/`) | not written |

The original step 8 of the spec is now plans 8 (this port), 9 (move Rocket: register, dry run, remove `tools/factory`) and 10 (RocketVault). Plans 9 and 10 are not written yet.

## Why plans 2 to 8 are not written yet

They name functions, files and line numbers in the extracted code. Plan 1 moves and edits that code, and each later plan changes what the next one touches. Writing them now would put stale names in them. Each is written from the repo as it stands when the previous plan merges.

## Carried facts for every plan

- Harness rules: `cargo` always with `-j4`, never `--workspace` or `--all`, never `CARGO_TARGET_DIR`.
- Commits: conventional commits, written through the `dev-workflow-skills:1-git-commit` skill, never a freeform `git commit -m`. Dispatched implementers get the same instruction.
- Rust: no `unwrap()` in production paths, no shelling out to `git` (use `git2`).
- Board JSON stays compatible with the dashboard: new fields are optional.
- Rocket's `tools/factory` stays in Rocket until plan 8 has run a real job there.
- Rocket's accurate-usage series merged before plan 1 was re-run (2026-10-17), so the extracted copy has it. Anything merged into Rocket's `tools/factory` after that must be ported the same way: re-run `git filter-repo` on a fresh clone and rebase the local commits.
