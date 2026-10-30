# Harness Hardening, Plan 03: Neutral Text And One Tool Map Per Harness

> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** The `/factory` command and the factory agent files name no harness tool outside a mapping section, and `build_codex_skill` assembles the Codex skill without any exact-sentence replacement.

**Architecture:** The shared command body becomes the text the Codex skill has today (already neutral), with `/factory` kept. Each harness has one mapping file of "harness notes": tool names plus the skill-workflow and option sentences that differ. The Claude map is the tail of `global-command.md`, pinned equal to `tool-map-claude.md` by a test, so `GLOBAL_COMMAND` stays one `&str` and the install code and tests keep working. The Codex skill is the shared body with `/factory` changed to `$factory`, followed by `tool-map-codex.md`. Codex runtime agent text is preface, then `tool-map-codex.md`, then the agent body. Claude agent runs need no map: Claude reads the agent files directly and knows its own tools.

**Tech Stack:** Rust, `include_str!` assets, Markdown.

**Spec:** `docs/superpowers/specs/2026-10-30-harness-hardening-design.md` (GitHub issue #37, parent #40)

## Global Constraints

- Cargo per crate with `-j4`: `cargo test -j4 -p <crate> <filter>`; never `--workspace` or `--all`; never set `CARGO_TARGET_DIR`.
- Git only through `factory_git::Repo`; never shell out to git. `gh`, `claude`, `codex` may be spawned.
- Paths reach the driver only through `Layout` / `factory_core::layout` helpers; `no_hardcoded_paths.rs` fails on hardcoded ones.
- Board JSON stays compatible with the dashboard: this plan adds no board field.
- Tests use `fake-claude`, never the real CLI; no task needs network or a login.
- After editing `.claude/agents/factory-*.md` or `global-command.md`, run `cargo test -j4 -p factory-cli --test agent_lint`, `--test global_command` and `--test skill_drift`.
- Do not bump the `<!-- factory-rules: N -->` marker: the agent edits are wording only.
- Comments are short full sentences ending with a period. Docs go in `docs/`.
- Change only the sentences this plan names. Everything else in the command stays byte-identical, so `factory upgrade` churns once (one `.bak`) and is a no-op after.
- The driver makes the commits (with the `dev-workflow-skills:1-git-commit` skill); no task has a commit step.

## Review Focus

1. A Claude-only word left in shared text, or a Codex-only word in the Claude body (`shared_text_names_no_harness_tool` and `agent_files_name_no_harness_tool`, Tasks 1 and 2).
2. The Claude map and the command tail drift apart after a later edit (`claude_map_is_the_command_tail`, Task 1).
3. A command without the Claude map at its end gives a clear error and installs no Codex skill (`codex_skill_reports_a_missing_claude_map`, Task 1).
4. The Codex skill keeps every pinned phrase and gains no Claude word (`codex_skill_has_no_claude_words`, Task 1).
5. Codex agent text puts the map after the preface and after the frontmatter strip, never inside the frontmatter (`agent_text_orders_preface_map_and_body`, Task 3).

## File Structure

- Create `crates/factory-core/assets/tool-map-claude.md` and `tool-map-codex.md`: the harness notes.
- Modify `crates/factory-core/src/lib.rs`: `TOOL_MAP_CLAUDE` and `TOOL_MAP_CODEX` consts next to `GLOBAL_COMMAND` (line 30).
- Modify `crates/factory-core/assets/global-command.md`: neutral body plus the Claude map as its tail.
- Modify `crates/factory-cli/src/install.rs`: `build_codex_skill` without `replace_required`.
- Modify `.claude/agents/factory-*.md` and `crates/factory-core/presets/{csharp,go,legacy,node,ruby,rust}/agents/factory-*.md`: neutral wording (24 files name a tool).
- Modify `crates/factory-engine/src/codex_prep.rs`: insert the Codex map.
- Create `crates/factory-cli/tests/tool_map.rs`: the guard tests. Modify `global_command.rs` and `agent_lint.rs` where they pin changed wording.
- Docs: `docs/reference/software-factory.md` (short "Harness text" paragraph).

### Task 1: Mapping files and consts

**Files:** Create both map files and `tests/tool_map.rs`; modify `lib.rs` and `global-command.md` (append only).

**Interfaces:**
- Produces: `factory_core::TOOL_MAP_CLAUDE: &str` and `factory_core::TOOL_MAP_CODEX: &str`. Each file starts with its heading line, `## Harness notes: Claude Code` or `## Harness notes: Codex`, then one `### <name>` section per note, the same nine names in both files: `Task list`, `Shell and background runs`, `Skills`, `Subagents`, `File tools`, `Interview`, `Plan writing`, `Commit`, `Model option`. The shared body points at a note by name.
- Claude file: the real names `TaskCreate`/`TaskUpdate`; the shell tool `Bash` with `run_in_background` true for the series run; the `Skill` tool; the `Agent` tool; `Read`/`Edit`/`Write`/`Grep`/`Glob`. `Interview`, `Plan writing`, `Commit` and `Model option` carry the Claude-only sentences that Task 1 removes from the body: move them from the left sides of the 14 pairs in `install.rs:127-190` (the `superpowers:brainstorming` paragraph and its overrides, the `superpowers:writing-plans` and executor-line sentences, the `dev-workflow-skills:1-git-commit` clause, the `--opus` / `claude-opus-5-5` sentences).
- Codex file: `update_plan` for the task list; `exec_command` and `write_stdin` for shell, with the handle rule (keep the session handle, never poll); `Skills` says to use only the catalog the run preface allows; `Subagents` says none are available in factory runs; `File tools` names `apply_patch` and shell reads; `Model option` holds the right side of the `--opus` pair from `install.rs` ("`--opus` is a Claude-only option ..."); the other notes say "follow the shared text". Names come from `~/.claude/plugins/cache/claude-plugins-official/superpowers/7.0.0/skills/using-superpowers/references/codex-tools.md`; the Maintainer steps correct them against the live CLI.

- [ ] **Step 1: Write the failing tests** in `tool_map.rs`: `maps_name_their_own_tools` (Claude map contains `TaskCreate`, `run_in_background`, `Skill`; Codex map contains `update_plan`, `exec_command`, `$factory`; each starts with its heading and contains all nine `### ` names) and `claude_map_is_the_command_tail` (`GLOBAL_COMMAND.ends_with(TOOL_MAP_CLAUDE)`, message: "edit tool-map-claude.md and the tail of global-command.md together").
- [ ] **Step 2:** Run `cargo test -j4 -p factory-cli --test tool_map`. Expected: compile error `cannot find value 'TOOL_MAP_CLAUDE'`.
- [ ] **Step 3:** Create the two files and the two `pub const ... = include_str!("../assets/...")` lines. Append `\n` and the Claude map text to `global-command.md` (a copy; the old sentences stay in the body until Task 1).
- [ ] **Step 4:** Run the same command. Expected: 2 passed.

#### Then: Neutral command and the new `build_codex_skill`

**Files:** Modify `global-command.md`, `install.rs`, `tests/global_command.rs`, `tests/agent_lint.rs`; extend `tests/tool_map.rs`.

**Interfaces:**
- Consumes: `TOOL_MAP_CLAUDE`, `TOOL_MAP_CODEX`.
- Produces: `fn build_codex_skill(command: &str) -> Result<String, CliError>` (private, same signature). Rule: take the text after the frontmatter; drop the `# Factory\n\nArguments: $ARGUMENTS\n` prefix when present (as today); `strip_suffix(TOOL_MAP_CLAUDE)`, else `CliError::Refused("global command must end with the Claude tool map")`; replace `/factory` with `$factory` in that remaining body only; output is the current header (`---\nname: factory\ndescription: {SKILL_DESCRIPTION}\n---\n\n# Factory\n\nUse the user's accompanying request as the factory arguments.\n\n`), the body, a blank line, then `TOOL_MAP_CODEX`. Delete `replace_required` and the pair table.
- Produces: `HARNESS_WORDS` in `tool_map.rs`, matched as whole words, case-sensitive. Claude: `TaskCreate`, `TaskUpdate`, `run_in_background`, `Bash`, `Skill tool`, `Agent tool`, `Read tool`, `Workflow tool`, `Edit and Write`, `AskUserQuestion`, `Grep`, `Glob`, `WebFetch`, `--opus`, `claude-opus-5-5`. Codex: `apply_patch`, `exec_command`, `write_stdin`, `update_plan`, `spawn_agent`, `wait_agent`, `followup_task`, `request_user_input`, `$factory`. Derivation: the command words come from the 14 old pairs (`TaskCreate`, `TaskUpdate`, `run_in_background`, `Bash`, `--opus`, `claude-opus-5-5`); the agent words from the agent files (`Skill tool`, `Agent tool`, `Read tool`, `Workflow tool`, `Edit and Write`, and `Grep`/`Glob` in the frontmatter `tools:` line, which the test strips with the frontmatter).

- [ ] **Step 1: Record the old Codex skill.** With a temp `HOME` outside the repo, run `cargo run -j4 -p factory-cli -- install` and copy `~/.agents/skills/factory/SKILL.md` to a temp file `old-skill.md`.
- [ ] **Step 2: Write the failing tests** in `tool_map.rs`: `shared_text_names_no_harness_tool` (the command with frontmatter and the `TOOL_MAP_CLAUDE` suffix removed contains no `HARNESS_WORDS` entry; the message names the word and its line) and `codex_skill_has_no_claude_words` (install with a temp `HOME` as `install_defaults_to_the_claude_commands_folder_under_home` does; the Codex skill text before `## Harness notes: Codex` has no Claude-list word).
- [ ] **Step 3:** Run `cargo test -j4 -p factory-cli --test tool_map`. Expected: `shared_text_names_no_harness_tool` FAILS naming `TaskCreate`.
- [ ] **Step 4: Rewrite the body of `global-command.md`.** Each body sentence that has a pair in `install.rs` takes the pair's right side (the text the Codex skill has today), with these edits: no Codex-specific tool wording ("the available command execution tool" becomes "a long-running command, see the `Shell and background runs` note"); `--opus` leaves "Pick the subcommand" and step 4, where only `--one-job` stays (the `--opus` handling is the `Model option` note); the Bash output sentence says "command output". A shared bullet that has a Claude extension ends with "See the `Interview` note." (likewise `Plan writing`, `Commit`, `Model option`). The frontmatter is untouched (its argument hint may name `--opus`).
- [ ] **Step 5:** Make the Claude tail in `global-command.md` equal `tool-map-claude.md` again, with the moved sentences complete.
- [ ] **Step 6: Implement `build_codex_skill`.** Replace the unit test `codex_skill_reports_a_changed_command` with `codex_skill_reports_a_missing_claude_map` (input `"# Factory\n\nno map here\n"`, the error contains `Claude tool map`). `shipped_command_builds_a_codex_skill` stays.
- [ ] **Step 7:** Update assertions that pin moved wording: `global_command.rs:318-360` (the run-end phrases move to the neutral step 5 and 6 text; keep each phrase except the one naming "the available command execution tool"), and `agent_lint.rs:380-410` (`run_in_background` and `superpowers:brainstorming` are still found through the tail). Do not rename tests.
- [ ] **Step 8:** Install again into a second temp `HOME` and `diff` against `old-skill.md`. Expected: only the appended `## Harness notes: Codex` section and the rewritten tool sentences differ; no pinned phrase is lost.
- [ ] **Step 9:** Run `cargo test -j4 -p factory-cli --test tool_map`, `--test global_command`, `--test agent_lint`, `--test skill_drift`, and `cargo test -j4 -p factory-cli --lib install`. Expected: all pass.

### Task 2: Neutral agent files

**Files:** Modify `.claude/agents/factory-*.md` and the six preset folders' `agents/factory-*.md`; `tests/agent_lint.rs:164`; extend `tool_map.rs`.

**Interfaces:**
- Consumes: `HARNESS_WORDS` (Task 1).
- Produces: the test `agent_files_name_no_harness_tool`; no code interface.

Rewrites (frontmatter `tools:` lines stay, they are Claude config):

| Old | New |
|---|---|
| ``Invoke the `X` skill with the Skill tool`` (12 uses) | ``Invoke the `X` skill`` |
| `use the Agent tool, run up to 3 topics in parallel tool calls` | `start one subagent per topic, up to 3 at a time` |
| `Never use the Workflow tool.` | `Never use workflow scripts.` |
| `Page the diff with the Read tool: offset 1 and limit 1500, ...` | `Page the diff: read 1500 lines per call, starting at line 1, ...` |
| `Use Edit and Write for every file change.` | `Use the file editing tools for every file change.` |

- [ ] **Step 1: Write the failing test** `agent_files_name_no_harness_tool` in `tool_map.rs`: every `.claude/agents/factory-*.md` and every agent file of every preset in `factory_core::preset` has, after its frontmatter, no `HARNESS_WORDS` entry. Run `cargo test -j4 -p factory-cli --test tool_map`. Expected: FAIL naming `Skill tool`.
- [ ] **Step 2:** Apply the table to all files (find each with a search, not from memory).
- [ ] **Step 3:** Change the `agent_lint.rs:164` phrase `Use Edit and Write` to `Use the file editing tools`. Keep the reviewer check at `agent_lint.rs:294`.
- [ ] **Step 4:** Run `cargo test -j4 -p factory-cli --test tool_map` and `--test agent_lint`. Expected: all pass.

### Task 3: Codex runtime agent text

**Files:** Modify `crates/factory-engine/src/codex_prep.rs`; docs.

**Interfaces:**
- Port: `AgentRunner`. The map only changes the agent text in the `AgentSpec` the runner receives; no new trait.
- Consumes: `TOOL_MAP_CODEX`; `codex_preface` as it exists when this task runs (`codex_preface(role)` today, `codex_preface(role, has_skills)` after plan 02). Do not change its signature or text.
- Produces: `fn codex_agent_text(preface: &str, body: &str) -> String` returning `{preface}\n\n{TOOL_MAP_CODEX}\n\n{body}`; the body is already frontmatter-stripped. `prepare_for_codex` builds `spec.agent_prompt` with it and passes whatever preface expression the call to `codex_preface` has at that line. The preface comes first so its skill and subagent rules win over the map.

- [ ] **Step 1: Write the failing test** `agent_text_orders_preface_map_and_body`: for `preface = "P"` and `body = "B"` the result starts with `P\n\n## Harness notes: Codex` and ends with `\n\nB`. A second assertion runs `strip_frontmatter` on `---\nname: x\n---\nBody` first and checks the result has no `name: x` line.
- [ ] **Step 2:** Run `cargo test -j4 -p factory-engine codex_prep`. Expected: `cannot find function 'codex_agent_text'`.
- [ ] **Step 3:** Implement it and call it in `prepare_for_codex`. The existing preface and sandbox tests stay unchanged.
- [ ] **Step 4:** Add the "Harness text" paragraph to `docs/reference/software-factory.md`: shared text is neutral, the maps hold tool names, and a third harness needs one map file and its words in `HARNESS_WORDS`. Say that the prompt strings in `specs.rs` and `kb.rs` still name the Read, Agent and Workflow tools and are out of scope here.
- [ ] **Step 5:** Run `cargo test -j4 -p factory-engine codex_prep` and `cargo test -j4 -p factory-cli`. Expected: all pass.

## Maintainer steps after merge

These need the real CLIs and are outside the tasks.

1. Run `factory probe --harness claude --json` and `factory probe --harness codex --json` (plan 01). Compare each `RoleProbe.tools` list with the tool names in `tool-map-claude.md`, `tool-map-codex.md` and `HARNESS_WORDS`. Fix a wrong name in the map and the word list together.
2. Run `factory upgrade`. Expect one `.bak` for the Claude command, and none on a second run.
3. Run a short `/factory` interview in Claude Code and `$factory` in Codex on a registered project, and check that the notes are followed (task list, background run, the `--opus` refusal in Codex).

Next plan: docs/superpowers/plans/2026-10-30-harness-hardening/plan-04-plugin-install.md
