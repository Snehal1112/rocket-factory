# Driver commit messages: presets, the /factory command and the docs (plan 4 of 4)

> **Spec:** docs/superpowers/specs/2026-11-19-driver-commit-messages-design.md (phase 1)
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Presets no longer ship or load the committer agent or the commit context, every plan writer (the `/factory` command and the intake drafter) adds the `**Commit:**` line, and the docs describe driver-written commit messages.

**Commit:** chore(factory): retire committer presets and docs

**Architecture:** The embedded preset list in `preset.rs` loses `commit-context.md` and `factory-committer.md`, with their files and tests. Files already installed in a factory home stay on disk, but agent resolution skips the retired committer file so it never reaches `agents.json`. The command text and the intake drafter gain the commit line rule, pinned by the existing text tests. The docs change only where they describe the committer, the message agent or the `commit_*` settings.

**Tech Stack:** Rust (factory-core, factory-cli tests), Markdown. No new dependency.

## Global Constraints

- The commit line rule, the same wherever it is written: `**Commit:** <type>(<scope>): <subject>` (or `<type>: <subject>`) in the plan header next to `**Goal:**`; a lowercase Conventional Commits type (`feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert`); a one-word scope with no spaces; at most 50 characters; no trailing period. It becomes the subject of the job's commit on the base branch. With `--one-job`, the spec carries one `**Commit:**` line for the whole series, right under its `# ` title.
- `global-command.md` and `plugins/factory/commands/factory.md` stay in step (`plugin_files.rs::plugin_files_match_generated_text` checks this). The `## Harness notes` section of `global-command.md` is not edited (`tool_map.rs::claude_map_is_the_command_tail` pins its tail), and the new shared text uses none of the harness words `tool_map.rs` forbids in shared text (`Bash`, `Grep`, `Glob`, `TaskCreate`, `Skill tool` and the rest of its list).
- Installed preset files in a factory home are never deleted by the factory; `factory presets refresh` stops shipping the two files, and agent resolution ignores a leftover `factory-committer.md` in the preset layer.
- Phase 2 (the squash) is not built yet: docs keep describing the merge as a no-ff merge.
- Cargo: `-j4`, one crate at a time (`-p <crate>`), never `--workspace`, `--all` or `CARGO_TARGET_DIR`.
- Tests use `.expect("...")`, never the unwrap call.

## Review Focus

1. A factory home seeded by an older factory still holds `rust/agents/factory-committer.md`: it is not in the resolved agents. Pinned by `a_retired_preset_agent_left_on_disk_is_not_resolved`.
2. The same home: a preset refresh neither fails nor rewrites or deletes the retired files. Pinned by `refresh_leaves_retired_preset_files_alone` in the `preset.rs` unit tests.
3. The command text and the plugin copy drift apart, or the harness tail changes. Pinned by `plugin_files_match_generated_text` and `claude_map_is_the_command_tail`.
4. The intake drafter of a preset other than rust still lacks the line. Pinned by `every_intake_drafter_asks_for_the_commit_line`.
5. A doc still tells users about the committer or to set a `commit_*` key. Pinned by `no_doc_describes_the_committer_or_an_ignored_key`.

## File Structure

- `crates/factory-core/src/preset.rs` (struct field at 18, the five presets at 25-181, `files_of` at 244-258, and the tests named in the first task), `crates/factory-core/src/resolve.rs` (`merged_agents` near 270, the agent list test near 403).
- Deleted: `crates/factory-core/presets/<preset>/agents/factory-committer.md` and `crates/factory-core/presets/<preset>/commit-context.md` for all five presets, and the repo's own `.claude/factory/commit-context.md`.
- `crates/factory-core/presets/<preset>/agents/factory-intake-drafter.md`: the commit line rule.
- `crates/factory-core/assets/global-command.md`, `plugins/factory/commands/factory.md`: the commit line rule.
- Docs: `README.md`, `docs/reference/software-factory.md`, `docs/reference/project-toml-reference.md`, `docs/guides/factory-user-guide.md`, `docs/guides/getting-started.md`, `docs/operations/rocket-registration.md`, `docs/operations/rocketvault-registration.md`.
- Tests: `crates/factory-cli/tests/agent_lint.rs`, `presets_refresh.rs`, `upgrade.rs`, `tool_map.rs`, `global_command.rs`; unit tests in `preset.rs` (including `refresh_leaves_retired_preset_files_alone`) and `resolve.rs`.

---

### Task 1: Presets without the committer

**Files:**
- Delete: the ten preset files above and `.claude/factory/commit-context.md`
- Modify: `crates/factory-core/src/preset.rs`, `crates/factory-core/src/resolve.rs`, the five `factory-intake-drafter.md` files (the plan rules at 37-41 in the rust copy)
- Test: in `preset.rs`: `presets_ship_the_seven_agents...` (592), `seed_writes_missing_files_and_never_overwrites` (780-793), the tests at 970-1336 that name `factory-committer.md` or `commit_context`, `shadowed_returns_nothing_when_no_agent_file_was_updated` (1372); in `resolve.rs`: the agent list near 403; in `crates/factory-cli/tests/agent_lint.rs`: `every_preset_ships_the_seven_factory_agents` (121), `committer_frontmatter_is_headless` (353), `committer_body_pins_the_rules` (369), `the_committer_and_kb_files_have_no_rules_marker` (1252); `crates/factory-cli/tests/presets_refresh.rs` (goldens `the_command_reports_each_outcome_and_refused_paths`, `refresh_output_matches_the_golden_text`, `the_refresh_output_is_byte_identical`); `crates/factory-cli/tests/upgrade.rs` (193, 335); `crates/factory-cli/tests/tool_map.rs` (`agent_files_name_no_harness_tool` at 103)

**Interfaces:**
- Produces:
  - `Preset` without the `commit_context` field; `files_of` without the commit-context entry; no preset lists `factory-committer.md`. Each preset now has 7 files, 35 in all.
  - `const RETIRED_AGENTS: [&str; 1] = ["factory-committer.md"];` in `resolve.rs`; `merged_agents` skips these names in the preset layer only (a project or repo layer that defines its own file of that name still wins as today).
  - Each intake drafter's plan rules gain one bullet with the commit line rule from the Global Constraints.
  - Test retargets: tests that delete or age `go/commit-context.md` or `*/agents/factory-committer.md` as a sample file switch to `go/agents/factory-kb-refresher.md` with the same assertions, except the tests that age a file (`an_unedited_file…`, `plan_refresh_reports_what_refresh_then_does`), which switch to `go/agents/factory-spec-reviewer.md` so the deleted and the aged file differ; `shipped("rust", "factory-committer.md")` follows its test's retarget. Counts change with 35 files: a skipped count of 44 becomes 34, 42 becomes 32, and the refresh summary becomes `1 written, 0 updated, 33 unchanged, 1 refused`. `presets_ship_the_seven_agents...` and `every_preset_ships_the_seven_factory_agents` become `..._six_...` without the committer. `the_committer_and_kb_files_have_no_rules_marker` keeps only the kb file and becomes `the_kb_files_have_no_rules_marker`. `committer_frontmatter_is_headless` and `committer_body_pins_the_rules` are deleted. Named counts: `upgrade.rs` 193 and 335, 45 becomes 35. `tool_map.rs::agent_files_name_no_harness_tool` (103): 35 agent files become 30, message `six agent files in each of five presets` (35 there counts agent files, not preset files). In `preset.rs` every 45 becomes 35 (809, 816, 1005, 1012, 1084, and `plan_refresh_on_a_missing_dir_creates_nothing` at 1262), `seed_records_files_equal_to_the_embedded_text` (1034) goes from 44 manifest lines to 34, and `all_rels()` (955) and `embedded_text()` (970) drop the commit-context entry. Leave `agent_layers_replace_by_name_and_the_result_is_sorted` in `resolve.rs` (699-744) as it is: its committer file comes from the project and repo layers. Leave the `commit-context.md` strings in `preset/manifest.rs` tests and `review.rs:505` alone; they are plain strings.

- [ ] **Step 1: Read** `preset.rs` (the struct, one preset literal, `files_of`, and the tests listed above), `merged_agents` and the agent list test in `resolve.rs`, `agent_lint.rs` 115-130, 353-392 and 1252, and `presets_refresh.rs`.
- [ ] **Step 2: Write the failing tests**:
  - `preset.rs`: `no_preset_ships_the_committer_or_the_commit_context` (for every embedded preset, no agent named `factory-committer.md` and no `files_of` path ending `commit-context.md`).
  - `resolve.rs`: `a_retired_preset_agent_left_on_disk_is_not_resolved` (seed a home with `env()`, which goes through `project::add`, write `rust/agents/factory-committer.md` into its presets folder, resolve a rust project: the agent names do not contain `factory-committer.md`); the expected list near 403 loses `factory-committer.md`.
  - `preset.rs` unit tests: `refresh_leaves_retired_preset_files_alone`, built like `an_unedited_file_that_differs_from_the_embedded_text_is_updated_through_the_manifest`: seed, write the older text `"---\nname: factory-committer\n---\nAn older shipped text.\n"` to `rust/agents/factory-committer.md` and to `rust/commit-context.md`, `m.set(rel, sha256_hex(older))` for both, then `refresh(dir, false)` is Ok and both files keep the older bytes. Today refresh overwrites them, so the test fails first.
  - `agent_lint.rs`: `every_intake_drafter_asks_for_the_commit_line` (each preset drafter contains `**Commit:**` and `at most 50 characters`).
- [ ] **Step 3: Run** `cargo test -j4 -p factory-core preset`, `cargo test -j4 -p factory-core resolve` and `cargo test -j4 -p factory-cli --test agent_lint`, and see the new tests fail.
- [ ] **Step 4: Implement** the Produces items, delete the eleven files and apply the test retargets.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-cli`, `cargo clippy -j4 -p factory-core --all-targets -- -D warnings` and `cargo clippy -j4 -p factory-cli --all-targets -- -D warnings`. Expected: pass.

### Task 2: The /factory command asks for the commit line

**Files:**
- Modify: `crates/factory-core/assets/global-command.md` (the spec rules of step 1 at 70-75, the per-plan rules at 76-88, step 3 at 91), `plugins/factory/commands/factory.md` (the same shared text)
- Test: `crates/factory-cli/tests/global_command.rs`

**Interfaces:**
- Produces:
  - A new bullet in the per-plan rules, right after the header bullet at 77: the commit line rule from the Global Constraints, with the example `**Commit:** feat(retry): retry a parked job from the dashboard`.
  - In step 1's spec rules, after the headings bullet at 72, one sentence: when `--one-job` was given, the spec gets one `**Commit:**` line right under its `# ` title, with the same rule; without it the commit subject is `feat: <spec title>`.
  - Step 3 (validate, at 91) also checks that every plan header has a `**Commit:**` line that follows the rule, because `<factory> --project <name> new` refuses a broken one.
  - The `## Harness notes` section and the `### Plan writing` note are not edited.

- [ ] **Step 1: Read** lines 60-100 of `global-command.md`, `global_command.rs`, `plugin_files.rs::plugin_files_match_generated_text` and the forbidden-word list in `tool_map.rs`.
- [ ] **Step 2: Write the failing test** `the_command_asks_for_a_commit_line_in_every_plan` in `global_command.rs`: the text contains `**Commit:**`, `at most 50 characters` and `no trailing period`; the plan bullet comes after the header bullet (needle `the header names the plan`) and before `3. Always validate`; the `--one-job` spec sentence comes after `spec stage <slug> writing-spec` and before `2. Write the plans`.
- [ ] **Step 3: Run** `cargo test -j4 -p factory-cli --test global_command` and see it fail.
- [ ] **Step 4: Implement** the Produces items in both files.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-cli --test global_command --test agent_lint --test tool_map --test plugin_files` and `cargo clippy -j4 -p factory-cli --all-targets -- -D warnings`. Expected: pass.

### Task 3: Docs describe driver-written messages

**Files:**
- Modify: `docs/reference/software-factory.md` (50, 55, 64, 72-78, 87, 236, 315, 326, 341, 380, 388, 484), `README.md` (45, 162, 204, 213, 256, 641), `docs/guides/factory-user-guide.md` (347, 603, 951, 1345), `docs/guides/getting-started.md` (23, 48, 376), `docs/reference/project-toml-reference.md` (60, 77, 88-90, 219, 298, 313, 337), `docs/operations/rocket-registration.md`, `docs/operations/rocketvault-registration.md`
- Test: `crates/factory-cli/tests/agent_lint.rs` (next to `docs_describe_the_new_design`)

**Interfaces:**
- Produces:
  - Every place that describes the committer agent, the message agent, `commit-context.md` or batch commits says instead: the builder ends its reply with a commit note; the driver makes `wip:` snapshot commits on the job branch and writes the merge message from the plan's `**Commit:**` line, its Goal and the notes, with `Fixes: #N` or `Relates to: #N` and `Factory-Job: <id>`; a missing or bad note uses a fallback with a ledger line and never parks. `software-factory.md` shows an example builder note (`---BEGIN COMMIT---` ... `---END COMMIT---`) and an example merge message in the spec's section 4 layout, and its footer table says `wip:` snapshot commits carry no footer.
  - The `factory probe --role` lists drop `committer` (four roles); `stall_minutes`, the turn limits and the codex sandbox table drop committer and message; `factory tokens` keeps `committer` and `message` only as roles of older jobs.
  - `project-toml-reference.md`: the five `commit_*` keys move into one entry under the exact heading `### Ignored settings`, which says they still load, have no effect and make `factory run` print a warning; the `commit_plugins` mention at 219 goes there too; the `### Commits` table (88-90) is renamed `### Ignored settings`, the `commit_model` row at 77 and the `commit-context.md` bullet at 298 go, and the "Cheaper runs" example at 337 uses `builder_model = "haiku"` instead of `commit_model`.
  - `getting-started.md`: the `dev-workflow-skills` row says `/factory` uses `dev-workflow-skills:1-git-commit` when it commits the spec and plans (the driver writes the job's commit messages itself); the `commit_model` mentions at 23 and 376 go.
  - The user guide: in Step 2 'Write the spec and plans' (near 192-199), add a bullet: each plan header has a `**Commit:**` line (the rule from the Global Constraints), and `new` refuses a broken one.

- [ ] **Step 1: Read** each listed doc around the listed lines (`grep -n -i "committer\|commit_model\|commit_batch\|commit_plugins\|commit_subagents\|commit_budget\|commit-context\|message agent\|message run"`).
- [ ] **Step 2: Write the failing test** `no_doc_describes_the_committer_or_an_ignored_key` in `agent_lint.rs`: in the seven listed docs none of these substrings occurs: `committer agent`, `` `factory-committer` ``, `commit-context.md`, `commit_context_file`, `batch committer`, `message-only mode`, `the committer's message`, `committer 40`, `` `committer` or `kb` ``; none of `commit_model`, `commit_subagents`, `commit_budget_weighted`, `commit_batch`, `commit_plugins` occurs in any listed doc except `project-toml-reference.md`, and there every occurrence lies between the `### Ignored settings` heading and the next line that starts with `#`; `software-factory.md` contains `---BEGIN COMMIT---` and `**Commit:**`. The forbidden phrases may not appear even to say the agent is gone.
- [ ] **Step 3: Run** `cargo test -j4 -p factory-cli --test agent_lint` and see the new test fail.
- [ ] **Step 4: Implement** the doc edits.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-cli --test agent_lint --test check_coverage_docs` and `cargo clippy -j4 -p factory-cli --all-targets -- -D warnings`. Expected: pass.

Next plan: none
