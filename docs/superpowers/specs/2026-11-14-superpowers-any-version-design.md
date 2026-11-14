# Superpowers any version design

## Goal

Make the factory work with any installed version of the superpowers plugin by removing every check, quote and pin that depends on a specific version of its text.

## Overview

The factory is written against superpowers 6.4.2. Three things tie it to that version, and superpowers 7.0.2 broke the first one on this machine.

- The test `skill_drift` (`crates/factory-cli/tests/skill_drift.rs`) reads the newest installed superpowers from `~/.claude/plugins/cache` and fails when two brainstorming sentences, two writing-plans tokens or a subagent file are missing. Superpowers 7.0.2 removed both brainstorming sentences, so `cargo test -p factory-cli` fails for every job that touches `factory-cli`. Job 066 parked on it.
- The `/factory` command overrides the brainstorming skill by quoting two of its sentences (`global-command.md:122`, mirrored in `tool-map-claude.md:25`), and tests pin the quotes (`global_command.rs:433-435`, `agent_lint.rs:473-474`).
- Docs state the version (`getting-started.md:47`, `factory-user-guide.md:92`).

A code read shows the driver itself depends on no skill text: `builder_plugins` defaults to `["superpowers"]` without a version (`config.rs:266`), and the plan lint checks the executor line by substring and only warns (`planlint.rs:24-26,83,104`). Versions 6.4.2 and 7.0.2 have the same 15 skill names, and `writing-plans` is byte-identical, so only the brainstorming quotes and the test break.

## Current state

| Piece | Today | Gap |
|---|---|---|
| `skill_drift.rs:7` | `SKILLS_TESTED_AGAINST = "6.4.2"`, prints a note on a mismatch | Version pin |
| `skill_drift.rs:56-85` | Asserts two brainstorming sentences, `Interfaces:` and `Review Focus` in writing-plans, `Status` in `implementer-prompt.md` | Text checks that fail on 7.0.2 |
| `global-command.md:122` (and `tool-map-claude.md:25`) | Interview override quotes two skill sentences and names the skill's spike and bounded paths | Quotes a version's text |
| `global-command.md:126` (and `tool-map-claude.md:29`) | Describes the skill's header line as "its line for agentic workers, which offers a choice of executor" | Describes one template wording |
| `global_command.rs:433-435`, `agent_lint.rs:473-474` | Pin the quotes and the "spike and bounded paths" sentence | Pin the version text |
| `getting-started.md:47`, `factory-user-guide.md:92` | "version 6.4.2" | States a version |

## Non-goals

- Changing which skills the factory names (`superpowers:brainstorming`, `superpowers:writing-plans`, `superpowers:subagent-driven-development`) or the plan header and executor line.
- Changing the preset agents, driver prompts, `planlint.rs` or the config defaults.
- Choosing, installing or pinning a plugin version. The user keeps whichever version they want.
- The `6.4.1 rules` negative checks (`global_command.rs:476`, `agent_lint.rs:515`). They guard the command text against old rules and quote no skill.

## Design

```
skill_drift: find newest superpowers -> check the three skill folders exist -> pass
/factory interview: invoke brainstorming, then OUR rules win (no quoted skill text)
docs: "any version"
```

### 1. The drift test checks names only

`skill_drift.rs` keeps `newest_skills` and the rule that it passes when no plugin is installed. It drops `SKILLS_TESTED_AGAINST`, the version note and `version_key` (the version string is still printed). The assertions become: the folders `brainstorming`, `writing-plans` and `subagent-driven-development` exist under `skills/`, each with a `SKILL.md`. Nothing reads the text of a skill and no version is compared. The test name stays `installed_skills_still_have_what_the_commands_rely_on`.

### 2. The interview override states our rules

The Interview section of `global-command.md` and `tool-map-claude.md` is rewritten. It still invokes `superpowers:brainstorming` and tells the assistant to follow it fully with the user in this session. It then says that these rules win over the skill's own question style and its choice of path or size: every question, the first one included, carries a recommended answer and two or three alternatives, so the user can pick one or answer in their own words; the driver needs a written spec, so the work always takes the path that writes a spec and then invokes `superpowers:writing-plans`; the commit after the spec approval comes before the plans step. It quotes no skill sentence and names no version, path or mode of the skill. The existing sentences about the user skipping the questions, doing the writing yourself and not doing the builder's work stay.

### 3. The plan-writing section does not describe the skill's template

The Plan writing section of both files says that the header's line about who executes the plan is replaced by the executor line, whatever the skill's own header says. The executor line text, the Review Focus rule and the self-review sentence stay.

### 4. Tests and docs follow

`global_command.rs` and `agent_lint.rs` drop the pins of the two quoted sentences and the "spike and bounded paths" sentence, and pin the new rule sentences instead. `getting-started.md:47` and `factory-user-guide.md:92` say "the `superpowers` plugin (any recent version)" and give the three skill names the factory uses.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| No superpowers installed | `skill_drift` prints a skip line and passes, as today |
| Two versions installed | The newest by number is checked, as today |
| A named skill folder is missing | The test fails and names it |
| A skill's text changes | Nothing fails; the command's own rules still win |
| A future version asks open questions first | The command's rules win, so every question still carries a recommended answer |
| The skill adds a new required step | The assistant follows it unless it conflicts with the command's rules |

## Testing

- `crates/factory-cli/tests/skill_drift.rs`: `installed_skills_still_have_what_the_commands_rely_on` keeps its name and checks names only; `skill_names_check_passes_with_no_plugin_installed` (a temp `HOME` with no cache passes).
- `crates/factory-cli/tests/global_command.rs`: the interview test pins `every question, the first one included, carries a recommended answer`, `these rules win`, and `writes a spec`, and no longer contains the two quoted sentences; `command_names_no_superpowers_version` (the command and tool map contain neither `6.4` nor `7.0`).
- `crates/factory-cli/tests/agent_lint.rs`: the same pins moved with the text; `tool_map.rs` `claude_map_is_the_command_tail` keeps passing, so both files change together.
- Commands: `cargo test -j4 -p factory-cli`, then `cargo clippy -j4 -p factory-cli`.

## Files changed

- `crates/factory-cli/tests/skill_drift.rs`, `global_command.rs`, `agent_lint.rs`
- `crates/factory-core/assets/global-command.md`, `tool-map-claude.md`
- `docs/guides/getting-started.md`, `docs/guides/factory-user-guide.md`

## Constraints

- Both asset files change together, and `claude_map_is_the_command_tail` must pass.
- No `Cargo.toml` change, no `git` shell-outs, no `/proc` reads. Use `-j4`. Comments are short full sentences.
- The command text keeps the invocation names exactly as they are, because the Codex skill must not contain them (`global_command.rs`).

## Acceptance criteria

- `cargo test -p factory-cli` passes with superpowers 6.4.2 installed, with 7.0.2 installed and with both installed.
- `skill_drift` reads no skill text and compares no version.
- The `/factory` command and the tool map quote no skill sentence and name no version.
- The interview still requires a recommended answer and two or three alternatives on every question.
- The two guides name no superpowers version.
- `claude_map_is_the_command_tail` and the Codex-skill tests still pass.

## Out of scope

- Pinning or locking a plugin version, and reverting the plugin registry.
- Rewriting the interview to follow 7.0.2's open-questions-first style.

## Follow-ups

None.

## Decisions

- What replaces the `skill_drift` test? Options: keep only a skill-name check, delete the test, keep it as a warning only. Chosen: keep only a skill-name check. Decided by: user
- How does the command override the brainstorming skill? Options: rules that do not quote the skill, stop invoking the skill and inline the procedure, keep the quotes and add 7.0.2 ones. Chosen: rules that do not quote the skill. Decided by: user
- Should the factory work with every superpowers version, with no version-specific checks? Chosen: yes. Decided by: user
- Is the `6.4.1 rules` negative check kept? Options: keep, remove. Chosen: keep, because it quotes no skill. Decided by: call
- Does the plan-writing text change? Options: leave it, make it template-neutral. Chosen: make it template-neutral, because it describes one wording of the skill's header. Decided by: call
