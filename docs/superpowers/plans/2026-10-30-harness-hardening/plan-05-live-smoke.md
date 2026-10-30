# Harness Hardening, Plan 05: Live Smoke Script

> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** One command, `scripts/live-smoke.sh`, runs a tiny job through the real `claude` (or `codex`) before a release, and fails with a clear message unless the job merges and the transcripts are saved.

**Architecture:** A bash script builds a one-task fixture repo in a temp dir, registers it under a throwaway `FACTORY_HOME`, runs `factory probe`, then `factory new` and `factory run`, asserts the job reached `merged`, and copies board, events, ledger and agent transcripts to `--out`. An optional `--interactive` step drives `claude` in a detached tmux session and checks `/factory status`. A Rust integration test runs the script with a `claude` shim on `PATH`, so the script is tested in `cargo test` without credentials.

**Tech Stack:** Bash (style of `scripts/release.sh`), Rust integration test with `tempfile` and `std::process::Command`.

**Spec:** `docs/superpowers/specs/2026-10-30-harness-hardening-design.md` (GitHub issue #39, parent #40)

**Depends on plan 01 (#35): plan 01 must merge before this plan.** The script calls `factory probe --harness <h> --json`. Do not start this plan until `factory probe` exists on the base branch.

## Global Constraints

- Cargo per crate with `-j4`: `cargo test -j4 -p factory-cli --test live_smoke_script`; never `--workspace` or `--all`; never set `CARGO_TARGET_DIR`.
- Git only through `factory_git::Repo` in Rust. The bash script may call `git` for the fixture repo only; `gh`, `claude`, `codex`, `tmux` may be spawned.
- Paths reach the driver only through `Layout`; `cargo test -j4 -p factory-cli --test no_hardcoded_paths` must still pass (the new Rust test reads state only through the script's `--out` files).
- The script never writes the user's real factory home or repos: `FACTORY_HOME` is a temp dir, the fixture repo is a temp dir, and `factory install` is never run. Its one read of the real home is `factory project show <key project>`, to copy the API key settings (Task 2).
- The smoke run bills the same credential as real factory jobs: the throwaway `project.toml` gets the key project's `api_key_file` and `allow_api_key` when they are set there. The key itself is never copied, printed or saved to `--out`; only the path to the key file is.
- The real CLI is never used in `cargo test` or CI: the Rust test uses the shim only.
- Comments are short full sentences ending with a period. Docs go in `docs/`.
- The driver makes the commits (with the `dev-workflow-skills:1-git-commit` skill); no task has a commit step.
- At the end `bash -n scripts/live-smoke.sh` is clean, and `shellcheck scripts/live-smoke.sh` is clean when shellcheck is installed.

## Review Focus

1. A job that parks as `needs-human` fails the script (exit 1) and still saves the board and transcripts (`fails_when_the_job_does_not_reach_merged`, Task 2).
2. A failing probe stops the run before any job exists and says so (`fails_when_the_probe_fails`, Task 2).
3. The real `~/.factory` is never written (`passes_on_the_fake_agent_and_saves_transcripts` sets `HOME` to a temp dir and asserts `.factory` does not appear there, Task 1). A key project without key settings, or no key project at all, leaves the throwaway project on the login and says so (`no_key_project_uses_the_login`, Task 2).
4. A missing tool (`tmux`, `claude`, `codex`) exits 2 with the tool name, not a bash trace (`missing_tmux_exits_2`, `missing_agent_program_exits_2`, Tasks 2 and 3).
5. A tmux session is always killed, also on failure or Ctrl-C (trap in Task 3).

## File Structure

- Create `scripts/live-smoke.sh`: the whole flow, one function per step.
- Create `scripts/live-smoke/fixture/`: `README.md`, `check.sh`, `spec.md`, `plans/plan-01-greeting.md`. The script copies it into a temp git repo.
- Create `crates/factory-cli/tests/live_smoke_script.rs` and `crates/factory-cli/tests/support/claude-shim.sh`.
- Modify `docs/operations/releasing.md` and `docs/README.md`.

---

### Task 1: The headless smoke run, with its shim test

**Files:**
- Create: `scripts/live-smoke.sh`, `scripts/live-smoke/fixture/{README.md,check.sh,spec.md,plans/plan-01-greeting.md}`
- Create: `crates/factory-cli/tests/live_smoke_script.rs`, `crates/factory-cli/tests/support/claude-shim.sh`

**Interfaces:**
- Consumes: plan 01's `factory [--project <name>] probe [--harness claude|codex] --json` (exit 0 only when every expected channel is present). Existing commands: `factory project add <path> --preset node --name live-smoke --agent-cli <h>`, `factory project show live-smoke`, `factory --project live-smoke new <slug> <text> --plans <dir> --spec <file>`, `run 001`, `show 001` (pretty JSON with a `"stage"` field), `status`.
- Produces: `scripts/live-smoke.sh [--harness claude|codex] [--interactive] [--no-probe] [--keep] [--key-from <project>] [--out <dir>] [-h|--help]`. `--key-from` names the registered project whose API key settings the run copies (Task 2); default: the output of `factory project which` run from `REPO_ROOT` with the real `FACTORY_HOME`. Exit 0 pass, 1 smoke failure, 2 usage error or missing tool. `--no-probe` exists only so the fake-agent test can pass; the help text says so. Env: `FACTORY_BIN` (default `target/debug/factory`, else `factory` on PATH), `TMUX_BIN` (default `tmux`), `LIVE_SMOKE_AGENT_PROGRAM` (overrides the program name checked with `command -v`, default the harness name; used by the test).
- Produces: the `--out` layout, default `${TMPDIR:-/tmp}/factory-live-smoke/<UTC yyyymmddThhmmssZ>-<harness>`:

```
summary.txt        PASS or FAIL line, harness, cli version, job id, stage, seconds, credentials line
project.toml       the throwaway project's project.toml (key file path only, never the key)
probe/             probe report JSON and its transcripts (absent with --no-probe)
board.json  events.jsonl  log.jsonl   copied from <home>/projects/live-smoke/state/
show-001.json  status.txt             output of factory show 001 and factory status
jobs/<job dir>/    the whole job folder: builder-r*.md, review-code-r*.md, checks*.md, progress.md, usage.jsonl, park.json
interactive/pane.txt                  only with --interactive
```

**Fixture decisions.** The repo holds `README.md` and `check.sh` (`test -f README.md`, so a builder cannot fail it by accident). `spec.md` has a goal line of at least 8 words and acceptance criteria: "Add a file greeting.txt at the repo root whose first line is hello." The plan has one task, `### Task 1: Add greeting.txt`, and the line `Next plan: none`. The script runs `git init -b main`, commits, then `factory project add --preset node` (the preset needs no cargo or go; bun is not needed because the checks are replaced). It then appends to the project's `project.toml` one `[[settings.checks]]` with `paths = ["**"]`, `commands = ["sh check.sh"]`, and `budget_weighted = 300000` under `[settings]`, and requires `factory project show live-smoke` to end with `problems: none`.

**Pass condition (all hold, checked in this order):**
1. `factory probe` exits 0 (skipped with `--no-probe`). Else: `FAIL: marker probe failed (see <out>/probe). The harness is not loading the factory's agent text or skills as expected; no job was run.`
2. After `factory run 001`, `factory show 001` has `"stage": "merged"`. Else: `FAIL: job 001 ended in <stage> (<reason from park.json, or none>), expected merged. See <out>/jobs.`
3. `main` holds `greeting.txt`. Else: `FAIL: job merged but greeting.txt is not on main.`
4. These saved files exist and are non-empty: `board.json`, `events.jsonl`, one `builder-r*.md` and one `review-code-r*.md` under `jobs/`. Else: `FAIL: transcript missing: <name>.`

Evidence is saved before checks 2 to 4, so a failed run still leaves it. On success the temp work dir is removed unless `--keep`; on failure it is kept and its path printed.

- [ ] **Step 1: Write the shim and the failing test**

`support/claude-shim.sh` is a stand-in `claude`, test only. `plugin list --json` prints `[]`. Otherwise it reads stdin, picks the role from the args or stdin text (`factory-builder`, `factory-committer`, `factory-code-reviewer`) and prints stream JSON lines in the shape `fake-claude` uses (`system init`, one `assistant` with usage, `result`). Builder: writes `greeting.txt` with `hello` in its cwd. Committer: `git add -A` and commits in its cwd. Reviewer: result `fine` ending in `VERDICT: APPROVE`, or `VERDICT: CHANGES` with a `- [blocker] x` line when `SHIM_REVIEW=changes`. Any other prompt (the probe): result `NONE`.

In `live_smoke_script.rs`, helper `run_script(args: &[&str], envs: &[(&str, &str)]) -> std::process::Output` runs `bash scripts/live-smoke.sh` from the repo root (`env!("CARGO_MANIFEST_DIR")/../..`) with `FACTORY_BIN=env!("CARGO_BIN_EXE_factory")`, `HOME` a temp dir, `PATH` = a temp dir holding the shim copied as `claude` (mode 0755) in front of the real `PATH`, and `--out <tempdir>/out`. Test `passes_on_the_fake_agent_and_saves_transcripts`: args `--no-probe`; assert exit 0, `out/summary.txt` starts with `PASS`, `out/board.json` and `out/events.jsonl` are non-empty, a `builder-r1.md` exists under `out/jobs/*/`, `out/show-001.json` contains `"stage": "merged"`, and `HOME/.factory` does not exist.

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -j4 -p factory-cli --test live_smoke_script passes_on_the_fake_agent`
Expected: FAIL, the script does not exist.

- [ ] **Step 3: Write the fixture files and `scripts/live-smoke.sh`**

Match `release.sh`: `#!/usr/bin/env bash`, header comment with usage, `set -euo pipefail`, `REPO_ROOT` from `BASH_SOURCE`, the same RED/GREEN/YELLOW/BLUE/NC colors, `usage()`. Functions: `need <tool>` (exit 2, `Missing tool: <name>`), `build_fixture`, `register_project`, `run_probe`, `run_job`, `save_evidence`, `assert_pass`, `main`. `trap cleanup EXIT INT TERM` removes the work dir on success. Read the stage with `grep` on the `show` output, so `jq` is not a prerequisite. `FACTORY_HOME` is exported to the temp dir before any `factory` call.

- [ ] **Step 4: Run the test to see it pass**

Run: `cargo test -j4 -p factory-cli --test live_smoke_script passes_on_the_fake_agent`
Expected: PASS. Also `bash -n scripts/live-smoke.sh`.


### Task 2: Failure paths and the probe step

**Files:**
- Modify: `scripts/live-smoke.sh`, `crates/factory-cli/tests/live_smoke_script.rs`

**Interfaces:**
- Consumes: Task 1's script and `run_script`.
- Produces: the failure messages of Task 1's pass condition, exit codes 1 and 2.
- Produces: `copy_key_settings` in the script, called by `register_project` before the throwaway `FACTORY_HOME` is exported. It runs `factory --project <key project> project show <key project>` with the real `FACTORY_HOME` (`${FACTORY_HOME:-$HOME/.factory}` as the script found it) and reads two lines, ignoring any that end in `(default)`: `api_key_file = "<path>" (<source>)` and `allow_api_key = <true|false> (<source>)`. A leading `~/` in the path becomes `$HOME/`, so the copy is absolute. It appends the found lines under `[settings]` of the throwaway `project.toml`, and writes `credentials: api_key_file <path> (from <key project>)`, or `credentials: login (no key settings in <key project>)`, or `credentials: login (no key project)` to `summary.txt` and stdout. It never reads the key file. When the real factory home folder does not exist, it calls no `factory` command at all (so none can create that folder) and writes `credentials: login (no factory home)`.

- [ ] **Step 1: Write the failing tests**

- `fails_when_the_job_does_not_reach_merged`: `--no-probe` with `SHIM_REVIEW=changes`. Assert exit 1, stderr contains `FAIL: job 001 ended in needs-human` and `changes-remain`, and `out/board.json` and `out/jobs/` exist.
- `fails_when_the_probe_fails`: no `--no-probe`; the shim cannot answer the marker prompt, so `factory probe` exits 1. Assert exit 1, stderr contains `FAIL: marker probe failed`, `out/probe/` exists, `out/jobs` does not.
- `unknown_option_exits_2_and_help_exits_0`: `--bogus` exits 2 and names the option; `--help` exits 0 and prints `--interactive`, `--harness`, `--out`.
- `missing_agent_program_exits_2`: env `LIVE_SMOKE_AGENT_PROGRAM=definitely-missing-claude`. Assert exit 2 and stderr contains `Missing tool: definitely-missing-claude`.
- `copies_the_api_key_settings_from_the_key_project`: build a "real" factory home in a temp dir and register a temp git repo in it as `keysrc` (`factory project add <repo> --preset node --name keysrc` with `FACTORY_HOME=<real home>`), then append `api_key_file = "~/keys/test.key"` and `allow_api_key = true` under its `[settings]`. Run with `FACTORY_HOME=<real home>`, `--no-probe --key-from keysrc`. Assert exit 0; `out/project.toml` contains `api_key_file = "<HOME>/keys/test.key"` (the test's temp `HOME`, expanded) and `allow_api_key = true`; `out/summary.txt` contains `credentials: api_key_file <HOME>/keys/test.key (from keysrc)`; the key file is never created by the script and no file under `out/` contains the text `test.key` other than `project.toml` and `summary.txt`.
- `no_key_project_uses_the_login`: `--no-probe --key-from nosuch` with an empty real home; assert exit 0, `out/project.toml` has no `api_key_file`, and `out/summary.txt` contains `credentials: login`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -j4 -p factory-cli --test live_smoke_script`
Expected: the six new tests FAIL.

- [ ] **Step 3: Implement**

`assert_pass` prints the `FAIL:` lines to stderr and returns 1. `run_probe` writes `factory probe --json` to `out/probe/report.json` and copies the report's `transcript_dir`; on a nonzero exit it prints the `detail` of each channel that is expected and not present, then the FAIL line. The park reason comes from `jobs/*/park.json` with `grep -o`/`sed`. `--harness codex` passes `--agent-cli codex` to `project add`. `copy_key_settings` as specified above; a missing key project or a failing `project show` is not an error, only the `credentials: login (...)` line. `save_evidence` copies the throwaway `project.toml` to `out/project.toml`.

- [ ] **Step 4: Run all tests in the file**

Run: `cargo test -j4 -p factory-cli --test live_smoke_script`
Expected: all PASS.


### Task 3: The `--interactive` tmux step

**Files:**
- Modify: `scripts/live-smoke.sh`, `crates/factory-cli/tests/live_smoke_script.rs`

**Interfaces:**
- Consumes: Task 1's registered project and merged job. `~/.claude/commands/factory.md` (from `factory install`, or the plugin of plan 04) must already exist in the real `HOME`; the script does not install it and fails with `FAIL: /factory is not installed (run factory install first).`
- Produces: `interactive_step` in the script and `out/interactive/pane.txt`. Claude only: with `--harness codex` the script prints `--interactive supports claude only` and exits 2.

**Flow, from the porting guide (Part 5, step 7).** `$TMUX_BIN new-session -d -s factory-smoke-$$ -x 200 -y 50 -c <repo> "env FACTORY_HOME=<home> claude"`. The `env` prefix points `/factory` at the throwaway project; the real home is read only for the login and the command file. `pane()` is `capture-pane -p -t <s> -S -200`. Helper `wait_for <pattern> <what>` polls once per second for 120 s, then prints `FAIL: timed out waiting for <what>` with the last 20 pane lines. Steps:
1. If the pane shows the folder trust prompt (`trust this folder`), send `Enter`.
2. Wait for the input prompt (`? for shortcuts`).
3. `send-keys -t <s> -l '/factory status'` as one call, `sleep 1`, then `send-keys -t <s> Enter` as a separate call.
4. Wait for a pane containing both `merged` and `001`. This proves the slash command loads, the `factory` binary is on PATH, and the throwaway project is the one read.
5. Save the pane to `out/interactive/pane.txt`, send `/exit`, then `kill-session`.

- [ ] **Step 1: Write the failing tests**

`missing_tmux_exits_2`: `--no-probe --interactive` with `TMUX_BIN=definitely-missing-tmux`; assert exit 2, stderr `Missing tool: definitely-missing-tmux`, and that no job ran (`out/board.json` absent), so the check happens first. `interactive_with_codex_exits_2` asserts the message above. The tmux flow itself is not run in `cargo test`.

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -j4 -p factory-cli --test live_smoke_script missing_tmux`
Expected: FAIL.

- [ ] **Step 3: Implement `interactive_step`, the early `need "$TMUX_BIN"` check and the trap**

The trap runs `$TMUX_BIN has-session -t <s>` and then `kill-session`, so a failed or interrupted run leaves no session.

- [ ] **Step 4: Run the file's tests and `bash -n`**

Run: `cargo test -j4 -p factory-cli --test live_smoke_script`
Expected: all PASS.


#### Then: Release docs and cost note

**Files:**
- Modify: `docs/operations/releasing.md` (Files table row; new section `## Before a release: the live smoke run`, placed before "Cutting a release")
- Modify: `docs/README.md` (one index line, in its table's format)

**Interfaces:**
- Consumes: the script's flags, exit codes and `--out` layout from Task 1.

- [ ] **Step 1: Write the docs**

The section gives: the commands (`./scripts/live-smoke.sh`, `--harness codex`, `--interactive`); what it proves (stream format, flags, agent text and skill loading through the probe, one job to merge); the pass condition and exit codes; the `--out` layout; that neither `cargo test` nor CI runs it; the need for a login on the harness, a throwaway `FACTORY_HOME`, and for `--interactive` also `tmux` and an installed `/factory`. It lists the run on both harnesses as a pre-release step before `release.sh`. Cost line: "One run uses the real model. Rough cost: 150k to 300k weighted tokens per harness (the fixture job is capped at `budget_weighted = 300000`). This is an estimate; the first maintainer run replaces it with the measured number from `usage.jsonl`."

- [ ] **Step 2: Verify**

Run: `cargo test -j4 -p factory-cli --test live_smoke_script` and `cargo test -j4 -p factory-cli --test no_hardcoded_paths`
Expected: PASS. `grep -n live-smoke docs/operations/releasing.md docs/README.md` shows the new lines.


## Maintainer steps after merge

These need real logins and tokens, so no task runs them.

1. Build the binary: `cargo build -j4 -p factory-cli`.
2. First real run on Claude: `./scripts/live-smoke.sh --harness claude --out ~/smoke/claude`. Expect `PASS`. If the probe step fails, read `~/smoke/claude/probe` first (plan 01 explains each channel).
3. First real run on Codex: `./scripts/live-smoke.sh --harness codex --out ~/smoke/codex`. A failure caused by skills belongs to plan 02; note it in the issue.
4. Interactive check on Claude, with `/factory` installed: `./scripts/live-smoke.sh --interactive --out ~/smoke/claude-tmux`. The pane patterns (`trust this folder`, `? for shortcuts`, `merged`) are not checked against a live screen yet; the flow follows the porting guide (https://github.com/obra/superpowers/blob/main/docs/porting-to-a-new-harness.md, Part 5 step 7: clear trust and onboarding gates first, send text and Enter as separate calls, poll `capture-pane`). Adjust the patterns if the installed version differs, and read `interactive/pane.txt`. The model may paraphrase the `/factory status` output; if so, tighten the command text rather than loosen the check.
5. Read the real token use from `usage.jsonl` in each saved job folder and replace the estimate in `docs/operations/releasing.md`.

Next plan: none
