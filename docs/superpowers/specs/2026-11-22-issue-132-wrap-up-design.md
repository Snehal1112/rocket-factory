# Issue 132 wrap-up: docs, live probe findings and review fixes

**Commit:** fix(factory): close the last issue 132 leftovers

## Goal

Close issue 132: keep command-only text out of Codex role prompts and shut off every skill for a Codex role allowed none, make the probe pass a role whose named skill is not installed, stop the `/factory` command from wasting refused calls, make the path lints and five tests guard what they claim, fix the one test that fails only on macOS, and bring the reference docs in line with the code and the live results.

## Overview

Areas 1 to 3 of issue 132 have merged (jobs 097, 100 and 101). This last area covers the live checks (issues 35, 36, 37 and the umbrella 40), the stale docs facts (issue 94) and the findings of an independent review of job 101.

Evidence gathered on 2026-11-22, before this spec:

- Live Claude probe, `claude` 2.1.330 (`probes/20261122-065724-claude`): agent text reaches every role; the builder gets the superpowers bootstrap and skills; the reviewer and spec reviewer list no skills, so `--disable-slash-commands` blocks skills; the builder and kb also list 21 user and plugin skills outside their allowlist (Claude runs no `no_extra_skills` check for them).
- Live Codex probe, `codex-cli` 0.169.0 (`probes/20261122-144412-codex`), after job 101: every role runs on `gpt-5.6-luna` and gets its agent text; the builder lists exactly the 14 superpowers skills; the reviewer, spec reviewer and kb list `factory`, and kb lacks `kb-refresh`, which exists only in `~/.claude/skills`.
- Follow-up Codex run with the reviewer's settings (`--ignore-user-config`, apps and plugins off, `skills.include_instructions=false`) and a neutral prompt: `SKILLS: NONE`. The catalog is already empty. The `factory` name comes from the agent text: every Codex role gets the whole `tool-map-codex.md`, whose `### Model option` section is meant for the `$factory` skill only ("this Codex skill does not accept it", "Skill invocation as `$factory`").
- Manual runs, headless: `claude -p "/factory status"` and `codex exec '$factory status'` in this repo both ran `factory project which`, then `factory --project rocket-factory status`, and answered correctly. Codex used no Claude tool name. Claude first tried `B=${FACTORY_BIN:-factory}; ...` twice, and the permission check refused both ("A variable in this command can't be checked before it runs") before it ran plain `factory`.
- Review of job 101 (four advisers and a verifier, each finding shown by a mutation): both path lints stop at the first `#[cfg(test)]` text in a file, so large production parts of six files are never scanned; the paths lint has a fixed list of 7 crates and misses `factory-telemetry` and `factory-procgroup`; four tests pass with the code they guard broken.

## Current state

| Area | Today |
|---|---|
| Codex role text | `codex_agent_text` (`crates/factory-engine/src/codex_prep.rs`) is the preface, the whole `TOOL_MAP_CODEX`, then the agent body. `build_codex_skill` (`crates/factory-cli/src/install.rs`) appends the same map to the `$factory` skill. |
| Codex skill args | `codex_skill_args` (`crates/factory-engine/src/codex_skills.rs`): no allowed skill gives only `skills.include_instructions=false`; otherwise `skills.bundled.enabled=false` plus `skills.config=[{path=..,enabled=false},..]` for every other found skill. `tests/codex_prep.rs` `unmatched_allowlist_has_no_skills_preface` and `reviewer_gets_no_skills` pin the one-value form. |
| Probe skills check | `evaluate` (`crates/factory-core/src/probe.rs`) checks the `skills` channel against every entry from `allowed_skill_entries`. `RoleProbe` keeps neither the entries nor the missing list. The engine probe (`probe_run` in `crates/factory-engine/src/probe.rs`) calls `shape_spec`, which reaches `prepare_for_codex` and may move a kb spec's `cwd` to the KB folder. `render_probe` (`crates/factory-cli/src/probe.rs`) prints a detail only for an expected channel that is missing. |
| Command binary rule | `crates/factory-core/assets/global-command.md` "Find the project" step 1 and the `<factory>` line say to use `$FACTORY_BIN` when set; `plugins/factory/commands/factory.md` is a copy. `global_command.rs` `the_command_text_has_the_required_phrases_and_no_rocket_bits` and `plugin_files.rs` `command_names_the_path_failure` require the text `FACTORY_BIN`. `plugins/factory/README.md`, `README.md` and `docs/guides/getting-started.md` say the command honours it. `scripts/live-smoke.sh` `interactive_step` starts `env FACTORY_HOME=$FACTORY_HOME claude` with no `PATH`. |
| Tool-map strip | `build_codex_skill` uses `strip_suffix(TOOL_MAP_CLAUDE)` and refuses with `global command must end with the Claude tool map`. Tests: `shipped_command_builds_a_codex_skill`, `codex_skill_reports_a_missing_claude_map`. |
| Path lints | `crates/factory-cli/tests/no_hardcoded_paths.rs`: `no_source_names_a_state_path_or_the_main_branch` (a fixed list of 7 crates) and `proc_lines` both cut at `text.split("#[cfg(test)]").next()`. `workspace_crates` reads the root `Cargo.toml` itself and drops a member not under `crates/`. |
| Weak tests | `interactive_runs_with_a_fake_tmux` (the fake ignores `send-keys`); `a_retired_agent_is_dropped_from_every_layer_with_a_note` (one-line agent texts, so the preset-layer `retain` in `merged_agents` is untested); both `fix_builder_spec` asserts in `agent_files.rs` use `ends_with("apps/web")`; `a_scoped_job_with_a_merged_base_resumes` uses a fast-forward hand merge only; `probe_cmd.rs` spells `model: gpt-5.6-luna` in two asserts. |
| Docs | `software-factory.md`: title "Software Factory for Rocket", "Status: v3"; "Checking what agents see" says "the roles to all five" (there are four) and ends with "Findings: ... recorded here after the first live run". `project-toml-reference.md`: stamp "2026-10-19 (commit `cfd6c64`)", a `project.rs:37` reference, no `format` or `lint` in the Checks section. `CLAUDE.md`: factory-core is "pure rules ... (no I/O deps)", "only two things ... sit behind traits", and the carried-over section calls the presets and `factory install` "plan 3" and "plan 7". |

## Non-goals

- No change to which skills a role is allowed.
- No new probe channel and no change to the Claude probe rules.
- No harness-neutral rewrite of the `specs.rs` and `kb.rs` prompt strings.

## Design

### 1. Command-only text stays out of Codex role prompts

A new factory-core function returns the map for run prompts:

```rust
/// The Codex map without its command-only `### Model option` section.
pub fn codex_runtime_tool_map() -> String;
```

It removes the `### Model option` heading and its body, up to the next `### ` heading or the end, wherever the section sits. `codex_agent_text` uses it. `build_codex_skill` keeps the whole `TOOL_MAP_CODEX`.

### 2. A Codex role with no allowed skill gets every skill turned off

`codex_skill_args(found, allow)` with no allowed path returns `skills.include_instructions=false`, `skills.bundled.enabled=false` and, when `found` is not empty, `skills.config=[...]` with every found path disabled, in the same sorted, TOML-escaped form as today. The allowed case is unchanged. This is a safety net against a Codex release that hides less with `include_instructions=false`.

Example: `found = [factory]`, `allow = []` gives `["skills.include_instructions=false", "skills.bundled.enabled=false", "skills.config=[{path=\"/h/.agents/skills/factory/SKILL.md\",enabled=false}]"]`.

### 3. The Codex probe follows the skill gap

- `codex_prep.rs` gets `pub(crate) fn codex_found_skills(&self, cwd: &Path) -> Vec<FoundSkill>`: the user roots plus `codex_worktree_skills_dir(worktree_root_of(cwd) or cwd)`, then `discover_skills`. `prepare_for_codex` calls it, so no new I/O path exists.
- `probe_run`, for a Codex probe, calls it on the base spec's `cwd` before `shape_spec`. An entry of `allowed_skill_entries(role, Codex, config)` is absent when no found skill is `allowed(s, &[entry])`.
- factory-core gets `pub fn evaluate_with_absent(role, harness, config, answer, absent: &[String]) -> RoleProbe`, and `evaluate` calls it with `&[]`. The `skills` channel is checked against the entries minus `absent`. When `absent` is not empty and no entry is left, the channel has `expected = false`, `present = false` and the detail `not installed, short rules used: <entries>`. With entries left, the detail of a pass is `<n> skills listed; not installed, short rules used: <absent>`. `no_extra_skills` still matches extras against the full `allowed_skill_entries` list, so a named skill the agent lists anyway is never an extra; only the `skills` channel drops `absent`.
- On the run-failed branch of `probe_run`, the code calls `evaluate_with_absent` with the same `absent` list and overwrites only the channels whose own `channel.expected` is true, not every name in `expected_channels`. So the `skills` channel of an all-absent role keeps `expected = false` and the not-installed detail and never shows as MISSING. `agent_text` still fails there.
- `render_probe` prints `Not installed (<role>): <entries>, short rules used` for such a role. A role is all-absent when its `skills` channel has `expected == false` and a detail starting with `not installed, short rules used: `; the text after that prefix is `<entries>`. No `RoleProbe` field is added. The exit code follows the channels as today, so a Codex kb role without `kb-refresh` passes.

### 4. The command always calls `factory`

`global-command.md` drops every `FACTORY_BIN` mention. The `<factory>` line becomes: `` `<factory>` is `factory` on PATH. Write it literally in every shell command and never use a shell variable, because the Claude permission check refuses a command that holds one. `` Step 1 reads ``Run `factory project which` from the current directory.`` The plugin copy matches. The two pinning tests require `never use a shell variable` instead of `FACTORY_BIN`. The new test `the_command_names_no_shell_variable` checks that `FACTORY_BIN` is absent from both copies.

Docs: `plugins/factory/README.md` drops "(or set in `$FACTORY_BIN`)"; `README.md` drops the command's `$FACTORY_BIN` mention and keeps factoryd's; the `docs/guides/getting-started.md` row reads "Tells `factoryd` where the `factory` binary is. The `/factory` command always uses `factory` on PATH."

Done by hand and committed on main before the job, because `scripts/**` has no check rule: `interactive_step` starts claude with `env PATH=<folder of FACTORY_BIN>:$PATH` when `FACTORY_BIN` holds a `/`. The job's fake tmux test asserts the logged `new-session` line holds `PATH=<parent of CARGO_BIN_EXE_factory>:`.

### 5. The tool-map strip stays exact

No code change. The new test `codex_skill_refuses_text_after_the_claude_map` builds from the shipped command plus `\nextra\n` and expects the refusal. `software-factory.md` "Harness text" says the shared body must end with the exact Claude map and that install refuses otherwise, so no Claude tool name reaches the Codex skill, and that run prompts get the map without its `### Model option` section.

### 6. Both path lints scan all production code

In `no_hardcoded_paths.rs`:

- `production_lines(text: &str) -> Vec<(usize, &str)>` drops each item that a line equal to `#[cfg(test)]` after trimming introduces. An item whose first line ends in `;` is that one line. A braced item ends at the first later line that is the attribute's own indentation followed by `}` (the rustfmt layout every file uses). A `#[cfg(test)]` after other text on its line, or inside a string, is ordinary text. Line numbers are the original ones.
- `path_hits(rel: &str, text: &str) -> Vec<String>` and `proc_lines` both use it. The paths lint takes its crates from `workspace_crates()`.
- `crates_in(manifest: &str) -> Vec<String>` parses the members; `workspace_crates` calls it on the root `Cargo.toml`. A member not under `crates/` panics with `workspace member {path} is not under crates/; update workspace_crates`.
- A file declared only by a `#[cfg(test)] mod name;` line (`factory-core/src/test_support.rs`, `factory-supervisor/src/api/testutil.rs`) is scanned like production code; neither has a hit today.

A prototype on main at `8aa23a2` keeps 134 of 436 lines of `permissions.rs` (the old cut kept 6) and 390 of 871 of `tasks.rs` (221), and neither lint has a hit in any of the 9 crates.

### 7. Six tests guard what they claim

- Fake tmux: it appends its arguments to a log in the tempdir and prints `merged` and `001` only after a `send-keys` line holding `/factory status` is logged. The test asserts the log holds that line, a `kill-session` line and the `PATH=` part of section 4.
- Retired agent: the preset copy in `a_retired_agent_is_dropped_from_every_layer_with_a_note` gets a `## Report` section that the project and tip copies lack, so the existing `count() == 2` note assert fails when the preset-layer `retain` is removed.
- Fix builder cwd: `fix_builder_spec` returns `(Fixture, AgentSpec)`, and both asserts become `assert_eq!(spec.cwd, worktree(&fx).join("apps/web"), ..)`.
- Scoped resume: `merge_by_hand_no_ff(fx: &MergeFixture) -> String` in `tests/common/mod.rs` builds a commit with head's tree and parents (main tip, head), moves `refs/heads/main` and force-checks out, as `merge_by_hand` does. The new test `a_scoped_job_merged_with_a_merge_commit_resumes` then commits `apps/api/later.txt` on main and asserts `MergeResult::Merged(later_sha)`, where `later_sha` is that commit and differs from the merge commit. The `BaseState::Merged` branch in `merge.rs` returns the main tip.
- KB folder on macOS: `crates/factory-engine/tests/kb.rs` `add_dirs` assert compares with `fx.config.kb_dir().expect("kb dir").canonicalize().expect("canon")`, because `kb_refresh_step` stores the canonical folder and macOS resolves `/var` to `/private/var`. A code read of every test found no other path that differs on macOS.
- Codex model: both `probe_cmd.rs` asserts build `format!("model: {}", factory_core::project::CODEX_DEFAULT_MODEL)`.

### 8. Docs

- `software-factory.md`:
  - The title becomes "Software Factory", and the status line names the standalone design spec.
  - "Checking what agents see": replace the Findings line with a dated list taken from the Overview (both CLI versions; `--disable-slash-commands` blocks skills; the bootstrap reaches the builder only; the 21 extra Claude skills; the Codex skill result and its cause; the headless manual runs). Add the not-installed rule of section 3, and fix "all five" to "all four".
  - "Using Codex": the role plugin lists are the skill allowlist that replaced the planned `codex_builder_skills` setting, and a role with no allowed skill gets every found and bundled skill turned off.
- `project-toml-reference.md`: a new stamp (the date and short commit of the job's base); "`#[serde(deny_unknown_fields)]` on `ProjectFile`" in place of `project.rs:37`; `format` and `lint` in the Checks section (`CheckRule` string lists: `format` runs after `commands` and shows as the `format` gate, `lint` runs after `format` and shows as the `lint` gate).
- `CLAUDE.md`:
  - factory-core is "domain rules plus home, project and preset loading (no I/O crate deps)".
  - The traits line names the seven traits: `AgentRunner` and `AgentStream` (factory-agent), `Tracker` (factory-engine), `BaseTipFiles` (factory-core), `FactoryCli` and `Spawner` (factory-supervisor), `LineSink` (factory-telemetry). It says which have fakes in tests.
  - The carried-over section drops "plan 3" and "plan 7" and says what each `.claude/` copy is used for today.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| `### Model option` section moved or missing in the map | `codex_runtime_tool_map` removes it wherever it is, and returns the map unchanged when it is missing. |
| Codex role, no allowed skill, nothing found | `include_instructions=false` and `bundled.enabled=false`, no `skills.config`. |
| Codex role, allowlist matches one skill of several | Unchanged: the other found skills are disabled. |
| Probe, two entries, one absent | `skills` is checked against the other entry only. |
| Probe, named skill absent but listed by the agent anyway | Not an extra; `no_extra_skills` matches the full allowlist. |
| Probe, named skill absent and an extra skill seen | `skills` is not expected; `no_extra_skills` still fails on the extra. |
| Probe run fails for an all-absent role | `skills` keeps the not-installed detail; `agent_text` fails. |
| Claude probe | `absent` is always empty, so nothing changes. |
| `#[cfg(test)]` inside a string literal (`gate.rs`) | Not an attribute line; the text is scanned. |
| `#[cfg(test)] use x;` near the top | Only that line is dropped. |
| `"}"` or `"{"` in a string inside a test item | Ignored; only the indented `}` line ends the item. |
| Workspace member outside `crates/` | The lint panics with the update message. |
| `FACTORY_BIN` is a bare name in live smoke | `PATH` is left as it is. |

## Testing

- factory-core `lib.rs` or its tests: `codex_runtime_tool_map_drops_only_the_model_option_section`.
- factory-engine `tests/codex_prep.rs`: `codex_role_text_names_no_factory_skill` (no `$factory`, no `### Model option`); the changed `unmatched_allowlist_has_no_skills_preface` (expects `include_instructions` and `bundled` with an empty find) and `reviewer_gets_no_skills` (those two plus `skills.config` holding `std::fs::canonicalize(root.join("superpowers/tdd/SKILL.md"))`, because `FoundSkill.path` is the canonical `SKILL.md` path).
- factory-engine `codex_skills.rs`: `empty_or_unmatched_allowlist_turns_every_skill_off` (replaces `empty_or_unmatched_allowlist_turns_instructions_off`), `an_empty_find_with_no_allowlist_has_no_skills_config`.
- factory-core `probe.rs`: `a_not_installed_named_skill_is_not_expected`, `one_absent_entry_of_two_checks_only_the_other`, `an_extra_skill_still_fails_when_the_named_one_is_absent`, `evaluate_matches_evaluate_with_absent_on_an_empty_list`.
- factory-engine `tests/probe.rs`: `a_codex_kb_probe_without_kb_refresh_passes`, `a_failed_codex_run_keeps_the_not_installed_detail`.
- factory-cli `src/probe.rs` unit test: `the_probe_report_names_a_not_installed_skill`, on a hand-built `ProbeReport`, because the CLI probe reads the real `HOME` and `CODEX_HOME` skill folders. `tests/probe_cmd.rs`: the two `CODEX_DEFAULT_MODEL` asserts, and `probe_notes_a_delivery_codex_ignores` still passes unchanged.
- factory-cli `install.rs`: `codex_skill_refuses_text_after_the_claude_map`. `global_command.rs` and `plugin_files.rs`: the changed asserts. `the_command_names_no_shell_variable` sits in `crates/factory-cli/tests/plugin_files.rs` and reads `GLOBAL_COMMAND` and `plugins/factory/commands/factory.md`.
- factory-cli `no_hardcoded_paths.rs`: `production_lines_drops_only_test_items` (an early `#[cfg(test)] use`, a marker in a string, a test module mid-file with `"}"` and `"{"` strings, a production fn after it), `the_proc_lint_catches_a_read_after_an_early_test_use`, `the_paths_lint_catches_main_after_an_early_test_use` (through `path_hits`), `a_member_outside_crates_panics` (`#[should_panic(expected = "is not under crates/")]` through `crates_in`).
- The changed tests of section 7.
- Commands: `cargo test -j4 -p <crate> <filter>` per crate, `cargo test -j4 -p factory-cli --test agent_lint`, `cargo test -j4 -p factory-cli --test tool_map`, clippy and fmt per crate.

## Files changed

- `crates/factory-core/src/lib.rs`, `crates/factory-core/src/probe.rs`, `crates/factory-core/src/resolve.rs` (tests only), `crates/factory-core/assets/global-command.md`
- `crates/factory-engine/src/codex_prep.rs`, `crates/factory-engine/src/codex_skills.rs`, `crates/factory-engine/src/probe.rs`, `crates/factory-engine/tests/codex_prep.rs`, `crates/factory-engine/tests/probe.rs`, `crates/factory-engine/tests/agent_files.rs`, `crates/factory-engine/tests/merge.rs`, `crates/factory-engine/tests/kb.rs`, `crates/factory-engine/tests/common/mod.rs`
- `crates/factory-cli/src/probe.rs`, `crates/factory-cli/src/install.rs` (tests only), `crates/factory-cli/tests/no_hardcoded_paths.rs`, `crates/factory-cli/tests/live_smoke_script.rs`, `crates/factory-cli/tests/probe_cmd.rs`, `crates/factory-cli/tests/global_command.rs`, `crates/factory-cli/tests/plugin_files.rs`
- `plugins/factory/commands/factory.md`, `plugins/factory/README.md`, `README.md`, `CLAUDE.md`, `docs/guides/getting-started.md`, `docs/reference/software-factory.md`, `docs/reference/project-toml-reference.md`
- By hand on main before the job: `scripts/live-smoke.sh`.

## Constraints

- Cargo commands are per crate with `-j4`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- No task edits a `Cargo.toml`, `.github/**`, `scripts/**` or `journeybook/**`.
- No board, ledger or event field. `RoleProbe` and the probe JSON keep their fields.
- A test that fails only on the macOS CI run is gated with `#[cfg(target_os = "linux")]` after the job.

## Acceptance criteria

- Codex role prompts carry the Codex map without `### Model option` and name no `$factory`; the installed `$factory` skill keeps the whole map.
- A Codex role with no allowed skill gets `skills.bundled.enabled=false` and every found skill disabled.
- A Codex probe passes a role whose named skill is not installed, prints `Not installed (<role>): ...`, and checks a partly absent allowlist against the rest.
- `global-command.md` and the plugin copy contain no `FACTORY_BIN`; the pinning tests check the no-variable rule; the live smoke tmux session gets the binary's folder on `PATH`.
- Text after the Claude tool map makes `build_codex_skill` refuse, pinned by a test.
- Both path lints scan every workspace crate's production code past an early `#[cfg(test)]` and fail for an injected `/proc` read or `"main"` literal after one; a member outside `crates/` panics.
- Each of the five weak tests in section 7 fails when the code it guards is broken, and the `kb.rs` assert compares canonical paths.
- `software-factory.md` records the live findings, the allowlist note and the exact-strip rule; `project-toml-reference.md`, `CLAUDE.md`, `README.md`, `plugins/factory/README.md` and `getting-started.md` carry the facts of sections 4 and 8.
- Every touched crate's tests, clippy and fmt pass, and `agent_lint` and `tool_map` pass.

## Out of scope

- Moving `kb-refresh` into a Codex skill folder.
- `fake_claude()` under `--release` or `--target`, which no build uses.
- Rebuilding a stale existing `fake-claude` binary.

## Decisions

- Should a Codex role with no allowed skills see any skill? Options: disable every found and bundled skill, record only, probe first. Chosen: disable every found and bundled skill, with a unit test and a probe re-run after the job. Decided by: user
- What should the probe do when a role's named skill is not installed under Codex? Options: follow the gap and show a not-installed detail, add the Claude skill folder to the Codex roots, keep MISSING and document the install. Chosen: follow the gap, so the probe passes and shows `not installed, short rules used`. Decided by: user
- How strict is the tool-map strip in `build_codex_skill`? Options: keep exact and pin it, allow trailing whitespace, split on a marker line. Chosen: keep the exact strip, pin it with tests and document it. Decided by: user
- When do the manual command run and the final probe happen? Options: now before the spec, after the job by hand, skip. Chosen: now, so the results go into the spec and the builder writes them; one Codex probe re-run after the job for the skill fix. Decided by: user
- How does the command pick the factory binary? Options: drop `FACTORY_BIN` from the command text, ask with `printenv`, record only. Chosen: drop it; the command always calls `factory` on PATH, and the live smoke script puts its binary first on PATH by a hand edit before the job. Decided by: user
- The neutral catalog run showed the Codex `factory` name comes from the tool map's command-only section. What is the fix? Options: keep command-only text out of role prompts, both that and turning every skill off, have the probe ignore `factory`. Chosen: both changes. Decided by: user
- How do the lints find test code? Options: cut at the first `#[cfg(test)] mod`, drop each `#[cfg(test)]` item by brace count, drop each item up to its indented closing brace. Chosen: the indented closing brace, because string braces break counting in `plugin.rs` and `tasks.rs` has production code after a test module. Decided by: call
- Which review findings are fixed? Options: all eight, the six real ones plus the model literal. Chosen: six plus the model literal; the `--release` lookup is out of scope because no build tests that way. Decided by: call
- Does the scoped resume helper change? Options: change `merge_by_hand` to no-ff, add a second helper. Chosen: a second helper, so the tests that use the fast-forward helper keep their expectations. Decided by: call
- How does the probe learn the absent skills? Options: a new `mark_not_installed` after `evaluate`, an `evaluate_with_absent` that recomputes the channel. Chosen: `evaluate_with_absent`, because a partly absent allowlist needs the entry list. Decided by: call
