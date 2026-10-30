# Harness Hardening Design

## Goal

Make the factory prove what reaches each agent's context, give Codex runs the skills their role allows, keep shared text harness-neutral, ship `/factory` as a Claude Code plugin, and add a live smoke run before releases.

## Overview

The factory runs two harnesses: `claude -p` through `ClaudeRunner` and `codex exec` through `CodexRunner`. Superpowers' porting guide (https://github.com/obra/superpowers/blob/main/docs/porting-to-a-new-harness.md) sets rules for loading skills and agent text reliably across harnesses: prove every assumption with a unique-marker test, name actions rather than tools in shared text with one tool map per harness, install through the harness's own mechanism, and drive a live instance before calling a port done.

Evidence from 2026-10-30 (codex-cli 0.163.0, Claude Code 2.1.314):

- Codex runs with the factory's flags already list every user skill in `~/.agents/skills`, `~/.codex/skills` and bundled skills, catalog only, while the preface tells them to use none.
- `-c skills.include_instructions=false` gives a run with no skills; `-c skills.bundled.enabled=false` drops bundled skills; `-c 'skills.config=[{path="<abs SKILL.md>",enabled=false}]'` disables one skill. A path through a symlink did not disable the skill, so paths must be canonical.
- Tests run only against `fake-claude`, so no test shows whether `--settings` `enabledPlugins` fires superpowers' SessionStart bootstrap in `-p` mode, or whether `--disable-slash-commands` blocks skills.

Tracking issue #40, sub-issues #35 to #39.

## Current state

| Area | Today | Reference |
|---|---|---|
| Codex preface | "Do not load or invoke skills and do not start subagents." | `crates/factory-engine/src/codex_prep.rs:8` |
| Codex flags | user config, apps and plugins off; no skill control | `crates/factory-agent/src/codex.rs:29-50` |
| Claude plugins per role | `--settings {"enabledPlugins":...}` from the role's plugin list | `crates/factory-agent/src/spec.rs:185` |
| Role plugin lists | `builder_plugins` (default `["superpowers"]`), `reviewer_plugins`, `commit_plugins`, `kb_plugins` | `crates/factory-core/src/config.rs:138-145`, `:204` |
| Reviewer slash commands | `--disable-slash-commands` | `crates/factory-engine/src/specs.rs:657`, `:698` |
| Codex skill install | Claude command rewritten by 14 exact sentence pairs | `crates/factory-cli/src/install.rs:108`, `:118`, `:127` |
| Command install | written into `~/.claude/commands` and `~/.agents/skills` | `crates/factory-cli/src/install.rs:16`, `:205` |
| Real CLI coverage | none; e2e uses `fake-claude` | `crates/factory-agent/src/bin/fake-claude.rs` |

Gaps: no way to see what an agent receives; Codex roles see every user skill but are told to ignore them; the Codex text depends on exact sentence matches; the install edits user config folders directly; nothing runs one real job before a release.

## Non-goals

- Codex plugin packaging. `~/.agents/skills` is Codex's own discovery folder and stays the Codex install target.
- Per-run skill filtering on Claude. Claude has no switch for it; the probe only reports what it sees.
- Changing the prompt strings in `specs.rs` and `kb.rs`, which golden tests pin.
- Running any real CLI in `cargo test` or CI.

## Design

Flow: plan 01 adds the probe, plans 02 and 03 change what agents receive, plan 04 changes how users install `/factory`, plan 05 runs the probe and one real job before a release.

### 1. Marker probe (plan 01, #35)

`factory [--project <name>] probe [--harness claude|codex] [--role <role>]... [--json] [--delivery agent|append|stdin]`. Roles: `builder`, `reviewer`, `spec-reviewer`, `committer`, `kb`; default all, default harness from the project config. For each role it builds the real spec, replaces only the prompt with `probe_prompt(marker)`, appends `Probe marker: FPROBE-<12 hex>` to the agent text, turns tools off (Claude: no allowlist, `dontAsk`, one turn; Codex: read-only sandbox, no network) and parses four answer lines: `MARKER:`, `BOOTSTRAP:`, `TOOLS:`, `SKILLS:`.

Channels: `agent_text` (marker came back), `bootstrap` (Claude, quote contains `You have superpowers`), `skills` (allowed skills listed), `no_extra_skills` (nothing outside the allowlist). `no_extra_skills` is expected on every Codex role and on Claude only for the reviewer roles. Exit 0 when every expected channel is present, else 1. Report: `<state dir>/probes/<UTC stamp>-<harness>.json` plus transcripts.

The one allowlist is `Config::skill_allow_for(role)`: `plugins_for(role)` plus `dev-workflow-skills` for committer and message and `kb-refresh` for kb.

### 2. Codex skills by role (plan 02, #36)

`discover_skills(roots)` reads `~/.agents/skills`, `$CODEX_HOME/skills` (default `~/.codex/skills`) and `<worktree>/.agents/skills`, follows symlinks, canonicalizes each `SKILL.md`, and collapses duplicates. `codex_skill_args(found, allow)`: no allowed skill gives `skills.include_instructions=false`; otherwise `skills.bundled.enabled=false` plus one `skills.config=[...]` value disabling every other canonical path. `AgentSpec.codex_config` carries the values into `codex_args`. `codex_preface(role, has_skills)` allows catalog skills when there are any; subagents stay off.

### 3. Neutral text and tool maps (plan 03, #37)

`tool-map-claude.md` and `tool-map-codex.md` hold the harness notes with the same nine section names. The command body is neutral; the Claude map is its tail, pinned equal by a test. `build_codex_skill` strips the Claude map, swaps `/factory` for `$factory`, and appends the Codex map. Codex runtime text is preface, Codex map, then the agent body. `HARNESS_WORDS` makes `shared_text_names_no_harness_tool` and `agent_files_name_no_harness_tool` fail on a tool name in shared text.

### 4. Claude Code plugin (plan 04, #38)

The repo root is a marketplace `rocket-factory` with plugin `factory` under `plugins/factory/`. `factory plugin sync [--check]` writes `plugin.json` and `commands/factory.md` from `install::claude_command_text()`. `scripts/release.sh` syncs after the version bump and checks drift in its gate. `factory install` and `factory upgrade` ask `claude plugin list --json` whether `factory@rocket-factory` is installed and print the switch or remove note.

### 5. Live smoke run (plan 05, #39)

`scripts/live-smoke.sh [--harness claude|codex] [--interactive] [--no-probe] [--keep] [--out <dir>]` builds a fixture repo, registers it under a temp `FACTORY_HOME`, runs the probe, then one job, and passes only when the job merges, `greeting.txt` is on main and the transcripts are saved. `--interactive` drives `/factory status` in tmux.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| Probe run fails or answers nothing | every expected channel missing, error in `detail`, exit 1 |
| Model echoes a wrong marker | `agent_text` missing; the prompt never contains the full marker |
| Unknown `--role` or `--harness` | clap error, exit 2 |
| Skill root missing, broken symlink, folder without `SKILL.md` | skipped, run continues |
| Same skill under two roots | one entry by canonical path |
| Path with quote, backslash or non-ASCII | valid TOML basic string |
| Committer's or KB's named skill not installed for Codex | preface keeps `SKILL_SENTENCE` |
| Command does not end with the Claude map | install refuses with `Claude tool map` in the error |
| `claude plugin list` missing or not JSON | plugin counts as not installed |
| Plugin and old command both installed | upgrade leaves the old file and prints the `rm` line |
| Smoke job parks | script exits 1 and still saves board and transcripts |
| `tmux`, `claude` or `codex` missing | script exits 2 naming the tool |

## Testing

- factory-core: `probe` unit tests (expected channels per role and harness, parser, marker leak), `skill_allow_for_adds_the_named_skill_once`, layout helpers.
- factory-agent: fake-claude `PROBE` mode for all three delivery paths, `codex_config` flags.
- factory-engine: `tests/probe.rs`, `codex_skills` unit tests with real symlinks, `tests/codex_prep.rs`, `agent_text_orders_preface_map_and_body`.
- factory-cli: `probe_cmd.rs`, `tool_map.rs`, `plugin_files.rs`, `plugin_install.rs`, `live_smoke_script.rs`, plus the existing `agent_lint`, `global_command`, `skill_drift`, `upgrade` and `no_hardcoded_paths`.
- Commands: `cargo test -j4 -p <crate> <filter>` per crate, as each plan states.

## Files changed

New: `factory-core/src/probe.rs`, `factory-engine/src/probe.rs`, `factory-engine/src/codex_skills.rs`, `factory-cli/src/probe.rs`, `factory-cli/src/plugin.rs`, `factory-core/assets/tool-map-{claude,codex}.md`, `.claude-plugin/marketplace.json`, `plugins/factory/`, `scripts/live-smoke.sh`, `scripts/live-smoke/fixture/`. Changed: `config.rs`, `layout.rs`, `spec.rs`, `codex.rs`, `codex_prep.rs`, `ports.rs`, `specs.rs`, `install.rs`, `upgrade.rs`, `cli.rs`, `global-command.md`, agent files and preset agent files, `scripts/release.sh`, reference and operations docs. Each plan's File Structure has the exact list.

## Constraints

- Cargo per crate with `-j4`, never `--workspace` or `--all`, never `CARGO_TARGET_DIR`.
- Git only through `factory_git::Repo`.
- Paths only through `Layout` and `factory_core::layout` helpers.
- No board, ledger or event field changes.
- Tests never use the real `claude` or `codex`.

## Acceptance criteria

- `factory probe` reports each channel per role for Claude and Codex, writes the report under `<state dir>/probes/`, and exits 1 when an expected channel is missing.
- Probe tests cover all three Claude delivery paths through `fake-claude`.
- A Codex builder spec carries `skills.bundled.enabled=false` and disables every non-allowed canonical skill path; a reviewer spec carries `skills.include_instructions=false`.
- The Codex preface allows catalog skills only when the role has allowed skills.
- `shared_text_names_no_harness_tool` and `agent_files_name_no_harness_tool` pass, and `build_codex_skill` uses no sentence pairs.
- `factory plugin sync --check` passes on the committed plugin files, and `scripts/release.sh` syncs and checks them.
- `factory install` and `factory upgrade` print the plugin notes as each plan states.
- `scripts/live-smoke.sh` passes against the test shim and exits 1 or 2 on the failure cases.
- Docs describe the probe, Codex skills, tool maps, the plugin install and the smoke run.

## Out of scope

- Codex plugin packaging and a Codex marketplace.
- Windows support for the smoke script.

## Follow-ups

- Run each plan's "Maintainer steps after merge" with the real CLIs and record the findings in the reference docs.

## Decisions

- Which lessons of the porting guide to take on? Options: all five items, only the probe, none. Chosen: all five, as one tracking issue with sub-issues. Decided by: user
- How to plan them? Options: one plan per sub-issue, one combined plan. Chosen: one plan per sub-issue in one series. Decided by: user
- How to run them? Options: the factory driver, inline in a session. Chosen: the factory driver. Decided by: user
- Codex skills: inject or filter? Options: inject a bootstrap, filter what Codex already lists. Chosen: filter, because the probes show the skills are already listed. Decided by: call
- Codex allowlist source? Options: reuse the role plugin lists, a new setting. Chosen: reuse, through `Config::skill_allow_for`. Decided by: call
- `no_extra_skills` on Claude? Options: every role, reviewer roles only. Chosen: reviewer roles only, since Claude cannot filter user skills. Decided by: call
- Plugin detection? Options: `claude plugin list --json`, reading `installed_plugins.json`. Chosen: the CLI call, a public interface. Decided by: call
- Codex plugin packaging? Options: now, later. Chosen: later; `~/.agents/skills` is Codex's own folder. Decided by: call
