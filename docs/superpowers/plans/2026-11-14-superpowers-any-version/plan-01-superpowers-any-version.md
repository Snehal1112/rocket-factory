# Superpowers any version Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-14-superpowers-any-version-design.md
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Remove every check, quote and pin that ties the factory to one version of the superpowers plugin text, so it works with any installed version.

**Architecture:** The drift test shrinks to a skill-name check. The `/factory` command and its tool-map copy state their interview rules as their own rules instead of quoting skill sentences, and the tests that pinned the quotes pin the new rules. Two guides lose the version number. No driver code, trait or config changes: `factory-engine` is untouched.

**Tech Stack:** Rust integration tests, Markdown assets included with `include_str!`.

## Global Constraints

- Commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`. Never `--workspace` or `--all`.
- `crates/factory-core/assets/global-command.md` and `tool-map-claude.md` change together: `claude_map_is_the_command_tail` in `crates/factory-cli/tests/tool_map.rs` requires the command to end with the tool map text.
- The Codex skill must still contain none of the three `superpowers:` skill names (`global_command.rs`).
- No `Cargo.toml` change, no `git` shell-outs, no `/proc` reads. No board, ledger or events field is added.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call. Comments are short full sentences.

## Review Focus

1. A machine with no superpowers installed still passes the drift test. Pinned by `skill_names_check_passes_with_no_plugin_installed`.
2. A cache that lacks one of the three skill folders fails and names it. Pinned by `a_missing_skill_folder_is_named`.
3. Neither the command nor the tool map names a superpowers version. Pinned by `command_names_no_superpowers_version`.
4. Every interview question still must carry a recommended answer and two or three alternatives. Pinned by the updated interview pins in `global_command.rs` and `agent_lint.rs`.
5. The command still tells the assistant to commit the approved spec before the plans. Pinned by the existing order assertion in `global_command.rs`.

## File Structure

| File | Responsibility |
|---|---|
| `crates/factory-cli/tests/skill_drift.rs` | Skill-name check, no text, no version |
| `crates/factory-core/assets/global-command.md`, `tool-map-claude.md` | The interview and plan-writing rules, without quotes |
| `crates/factory-cli/tests/global_command.rs`, `agent_lint.rs` | Pins of the new rule sentences |
| `docs/guides/getting-started.md`, `docs/guides/factory-user-guide.md` | The plugin line without a version |

### Task 1: The drift test checks skill names only

**Files:**
- Modify: `crates/factory-cli/tests/skill_drift.rs`
- Test: `crates/factory-cli/tests/skill_drift.rs`

**Interfaces:**
- Consumes: `newest_skills(home: &Path) -> Option<(String, PathBuf)>` as it is.
- Produces: `fn missing_skills(skills: &Path) -> Vec<&'static str>` returning the names among `brainstorming`, `writing-plans` and `subagent-driven-development` whose `<name>/SKILL.md` is not a file. The test `installed_skills_still_have_what_the_commands_rely_on` keeps its name and asserts this list is empty.

- [ ] **Step 1: Read `crates/factory-cli/tests/skill_drift.rs` end to end.**
- [ ] **Step 2: Write the failing tests.**
  - `skill_names_check_passes_with_no_plugin_installed`: an empty temp dir as `HOME`; `newest_skills` is `None`.
  - `a_missing_skill_folder_is_named`: a temp skills folder holding `brainstorming/SKILL.md` and `writing-plans/SKILL.md` only; `missing_skills` is `vec!["subagent-driven-development"]`.
  - `all_three_skill_folders_pass`: all three present; the list is empty.
- [ ] **Step 3: Run `cargo test -j4 -p factory-cli --test skill_drift`.** Expected: `cannot find function missing_skills in this scope`.
- [ ] **Step 4: Implement.** Delete `SKILLS_TESTED_AGAINST`, the version-mismatch note and every assertion on skill text (the two brainstorming sentences, `Interfaces:`, `Review Focus`, `Status`). Keep `newest_skills`, the skip lines for no `HOME` and no install, and the printed version. Add `missing_skills` and the one assertion that it is empty.
- [ ] **Step 5: Run `cargo test -j4 -p factory-cli --test skill_drift`.** Expected: 4 passed, including the real check against the installed plugin.

### Task 2: The command and the docs name no version and quote no skill

**Files:**
- Modify: `crates/factory-core/assets/global-command.md`, `crates/factory-core/assets/tool-map-claude.md`, `crates/factory-cli/tests/global_command.rs`, `crates/factory-cli/tests/agent_lint.rs`, `docs/guides/getting-started.md`, `docs/guides/factory-user-guide.md`
- Test: `crates/factory-cli/tests/global_command.rs`, `crates/factory-cli/tests/agent_lint.rs`, `crates/factory-cli/tests/tool_map.rs`

**Interfaces:**
- Consumes: the Interview and Plan writing sections as they are, the pins listed in the Review Focus and `claude_map_is_the_command_tail`.
- Produces: the Interview paragraph states that its rules win over the skill's own question style and its choice of path or size, and keeps the sentences "Every question, the first one included, carries your recommended answer and two or three alternatives, so the user can pick one or answer in their own words." and "The user asked for this because open questions were too hard to answer."; it says the work always takes the path that writes a spec and then invokes `superpowers:writing-plans`; it drops the two quoted skill sentences and the sentence about the skill's spike and bounded paths. The Plan writing paragraph says that the header's line about who executes the plan is replaced by the executor line, whatever the skill's own wording is, and drops "which offers a choice of executor".

- [ ] **Step 1: Read the Interview and Plan writing sections of `global-command.md` and `tool-map-claude.md`, the pins in `global_command.rs` (around the "overrides of the brainstorming skill" list and the removed-phrase list) and in `agent_lint.rs` (around the same list), `tool_map.rs`, and the plugin line in the two guides.**
- [ ] **Step 2: Write the failing tests.** In `global_command.rs`: update the interview pins so the list requires `"These rules win over the skill's own question style and its choice of path or size."` and `"always take the path that writes a spec"`, keeps the recommended-answer and "because open questions were too hard to answer" pins, and no longer requires the two quoted sentences or "the skill's spike and bounded paths do not apply"; add `command_names_no_superpowers_version` asserting that neither the command text nor the tool map contains `6.4` or `7.0` (match the digits with a dot, not bare digits), and a `forbidden` assertion that the command contains none of `"ask one focused question about purpose or intended use"`, `"open-ended is fine too"` and `"spike and bounded"`. Make the same pin change in `agent_lint.rs`.
- [ ] **Step 3: Run `cargo test -j4 -p factory-cli --test global_command --test agent_lint`.** Expected: the new pins fail, the command still has the quotes.
- [ ] **Step 4: Edit the two asset files identically** (the tool map must remain the tail of the command), then the guides: `getting-started.md` row says "The `superpowers` plugin for Claude Code (any recent version)", and `factory-user-guide.md` says "The **superpowers** plugin (any recent version)". The `6.4.1` negative phrase checks stay.
- [ ] **Step 5: Run `cargo test -j4 -p factory-cli`, then `cargo clippy -j4 -p factory-cli --all-targets`.** Expected: all pass, no warnings.

Next plan: none
