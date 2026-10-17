# Plan 9: Register Rocket, dry run, one real job, remove `tools/factory` from Rocket

Part of the standalone software factory series. Read `00-plan-index.md` first.

Plans 1 to 8 are merged. The factory in `rocket-factory` has every Rocket factory change up to Rocket's `main`. Nothing is registered yet: `~/.factory` does not exist.

This plan has 3 tasks. Tasks 1 and 2 are safe and run without asking. Task 3 spends real tokens and edits Rocket's `main`, so it starts only when the user says go, once for the real job and once for the removal.

## Rules for this plan

- Rocket is read-only until Task 3 step 2. Tasks 1 and 2 only read it.
- Never push. Neither repo is pushed by this plan.
- `cargo` always with `-j4`, never `--workspace` or `--all`, never `CARGO_TARGET_DIR`.
- Commits go through the `dev-workflow-skills:1-git-commit` skill.
- Rocket's `.claude/factory/config.toml` was copied to `crates/factory-core/tests/fixtures/rocket-config.toml` in plan 8. This plan never edits that fixture.

## What Rocket needs that the `rust` preset does not give

| Item | Rocket value | Where it goes |
|---|---|---|
| Numeric and flag settings | `diff_cap_lines`, `budget_weighted`, `fix_rounds`, `stall_minutes`, models, `commit_*`, `tickets_on_merge = true` | `project.toml [settings]` |
| `risk_paths` | Rocket's list, minus the `.claude/factory/**` entries | `project.toml [settings]` |
| `checks` | the `crates/*`, `src-tauri`, `src` rules, minus the two `tools/factory*` rules | `project.toml [[settings.checks]]` |
| `read_first`, `builder_rules`, `check_hints`, `reviewer_rules` | Rocket's own: `CLAUDE.md` and `.claude/rules/00-shortcuts.md` first, `yarn` for the frontend, the hard rules from `CLAUDE.md` | `project.toml [settings]` |
| The four agent files | Rocket's current `.claude/agents/factory-*.md` | `~/.factory/projects/rocket/agents/` |
| Committer context | Rocket's `.claude/factory/commit-context.md` | `~/.factory/projects/rocket/commit-context.md` |
| Past jobs 001 to 015 | `board.json`, `costs.json`, `events.jsonl` | `~/.factory/projects/rocket/state/` |

### Task 1: A registration file for Rocket, with a parity test

**Files:**
- Create: `crates/factory-core/tests/fixtures/rocket-project.toml`
- Create: `crates/factory-core/tests/rocket_parity.rs`
- Create: `docs/rocket-registration.md`

- [ ] **Step 1: Write the fixture.** `rocket-project.toml` is a full `project.toml` for Rocket: `repo = "/home/numericlabs/data/rocket/rocket"`, `preset = "rust"`, `[settings]` carrying every row of the table above except the agent files, the context and the state. Take values from `rocket-config.toml`, not from memory. Read the current Rocket `.claude/factory/config.toml` once and check the fixture still matches it.
- [ ] **Step 2: Write the failing test.** `rocket_parity.rs` builds a temp `Home` with the real `rust` preset seeded, writes `rocket-project.toml` as the project, resolves the settings and asserts:
  - Every key of `rocket-config.toml` that is not about `tools/factory*` or `.claude/factory/**` resolves to the same value.
  - `risk_paths` and `checks` contain no path under `tools/factory`, `tools/factory-server` or `.claude/factory`.
  - `read_first` and `builder_rules` do not contain a bare `cargo check -j4` line without `-p`/`--manifest-path` and do not name `CLAUDE.md` alone.
  - Resolving does not degrade to fail-closed (`risk_paths != ["**"]`).
  Run it and see it fail for the right reason before the fixture is complete.
- [ ] **Step 3: Make it pass.** Fix the fixture, not the test.
- [ ] **Step 4: Write `docs/rocket-registration.md`.** The exact commands of Task 2, in order, so a later machine can repeat them.
- [ ] **Step 5: Check and commit.** `cargo test -j4 -p factory-core --test rocket_parity`, `cargo check -j4 -p factory-core`, then `no_hardcoded_paths` in `factory-cli` still passes. Commit with the commit skill: `test(core): check the Rocket registration resolves like its old config`.

### Task 2: Register Rocket in `~/.factory` and dry run with no agent

This task touches the real home directory `~/.factory` and `~/.claude/commands/factory.md`, not Rocket's repo. It spends no tokens: nothing in it starts an agent.

- [ ] **Step 1: Build.** `cargo build -j4 -p factory-cli -p factory-supervisor` in `rocket-factory`. Use `target/debug/factory` and `target/debug/factoryd`.
- [ ] **Step 2: Register.** `factory project add /home/numericlabs/data/rocket/rocket --preset rust --name rocket --base main`. Replace the generated `project.toml` with the fixture from Task 1. Copy Rocket's four agent files into `~/.factory/projects/rocket/agents/` and `commit-context.md` next to `project.toml`. Copy `board.json`, `costs.json`, `events.jsonl` into `~/.factory/projects/rocket/state/`.
- [ ] **Step 3: Install the global command.** `factory install`. Report if a file already existed and `--force` was needed.
- [ ] **Step 4: Check the settings.** `factory project show rocket` must print no fail-closed warning, the values from the fixture, `tickets on merge: true`, and the agent files from the project folder. Compare against Rocket's old values by eye and record any difference in the ledger.
- [ ] **Step 5: Dry run.**
  - `factory --project rocket status` lists jobs 001 to 015 with the old costs.
  - `factory --project rocket new dryrun "Dry run of the standalone factory"` creates a job at stage `build`. Rocket's worktree folder must not change; check `git -C <rocket> status --short` is empty before and after.
  - Start `factoryd` in the background, check `factoryd is-running`, `GET /projects` and `GET /usage` with the token from `~/.factory/api.token`, open the dashboard in `--home` mode and fetch the project list. Stop `factoryd`.
  - `factory --project rocket cancel <id>` then `cleanup <id>`. Confirm `git -C <rocket> branch --list 'factory/*'` has no new branch.
  - `gh auth status` is logged in and `git -C <rocket> remote get-url origin` is a GitHub URL, which the ticket step needs.
- [ ] **Step 6: Record.** Add the dry run result and any difference to the plan ledger. No commit in Rocket. Commit only changes in `rocket-factory` (for example a fix to `docs/rocket-registration.md`).

### Task 3: One real job, then remove `tools/factory` from Rocket (needs the user's go, twice)

Do not start this task on the strength of "go ahead with plan 9". Stop after Task 2 and ask.

- [ ] **Step 1: The real job (ask first).** Ask the user for a small real change in Rocket (one crate, no risk paths, a plan of one task). Estimate about 0.25M weighted tokens. Run it with `factory --project rocket new` and `factoryd`, watch it through the API, approve it only if it is a normal merge. Check: the job merged into Rocket's `main`, the old `.claude/worktrees` is not used, no file appeared in Rocket's `.claude/agents/` or `.superpowers/factory/`, tickets were handled if the change has an issue, the token figures match `factory costs`.
- [ ] **Step 2: Remove the old factory (ask first, after the real job is good).** In Rocket, on a branch from `main`, in separate commits:
  - delete `tools/factory/` and `tools/factory-server/`;
  - delete `.claude/commands/factory.md` (it shadows the global command), `.claude/factory/config.toml` and `.claude/factory/commit-context.md` (now in `~/.factory`), and the four `.claude/agents/factory-*.md` files;
  - remove the two `tools/factory` allow rules from `.claude/settings.json`;
  - rewrite `.claude/commands/verify-rocket.md` lines 25 to 37 so they no longer mention the factory crates, and drop the two doc-lint blocks about it from `.claude/harness/tests/run.sh` (lines 275 to 279);
  - change `.claude/software-factory.md` and the line in `CLAUDE.md` to point at the standalone factory and `~/.factory`;
  - run `.claude/harness/tests/run.sh` and `cargo check -j4` for the root crates;
  - merge to `main` locally, do not push.
  Rocket's `.superpowers/factory` and the stale worktrees and `factory/001` to `015` branches are left alone and listed for the user to delete.
- [ ] **Step 3: Plan index and memory.** Mark plan 9 done in the index, write the memory note, record tokens in `.superpowers/plan9-tokens.md`.

## Open questions the user decides

- Which small change is the real job (Task 3 step 1).
- Whether to keep Rocket's old `.superpowers/factory` state after the move.
