# Factory quality fixes, Plan 01: Agent files and the risk glob

> **Spec:** docs/superpowers/specs/2026-11-01-factory-quality-fixes-design.md (section Plan 1, Compatibility)
>
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Jobs run on the preset agent files and not on Rocket's tracked copies, and the rust preset no longer marks an ordinary `build.rs` source file as risky.

**Architecture:** The preset agent files in `crates/factory-core/presets/<preset>/agents/` become the only copies the repo tests read. The five tracked `.claude/agents/factory-*.md` files are deleted, so `write_agent_files` no longer shadows the resolved files. The rust preset's `risk_paths` swaps one glob for three that match Cargo build scripts only.

**Tech Stack:** Rust, `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-cli --test agent_lint`, `cargo test -j4 -p factory-cli --test tool_map`.

## Global Constraints

- Always `-j4` and `-p <crate>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Each task ends with `cargo fmt -p <crate> -- --check` and `cargo clippy -j4 -p <crate> --all-targets -- -D warnings` for every crate it touched. No `#[allow(...)]` to silence a lint.
- No unwrap in production Rust. Tests use `.expect("...")` and `expect_err`.
- Code comments are short full sentences that end with a period.
- Agent text edits land in all five presets (rust, go, ruby, csharp, node). Config edits land only in the rust preset. The go, ruby, csharp and node `config.toml` files stay byte for byte as they are.
- Shared text names no harness tool, because `crates/factory-cli/tests/tool_map.rs` forbids it.
- Texts are exact: the builder line is `Match the conventions of the neighbouring code and of the project's CLAUDE.md: naming, error handling, comment style, test layout and formatting.` The risk globs are `build.rs`, `*/build.rs` and `*/*/build.rs`.

## Review Focus

1. A source file called `build.rs` inside a `src` folder must not be risky, while a Cargo build script at the root, at `dashboard/build.rs` or at `crates/x/build.rs` must be. Pinned by `rust_preset_marks_cargo_build_scripts_risky` and `rust_preset_leaves_a_source_file_named_build_rs_alone`.
2. A preset that loses its agent file must fail the lint, not pass quietly. Pinned by `every_preset_ships_the_five_factory_agents`.
3. The neutrality lint must keep covering the agent files after the move, for every preset. Pinned by `agent_files_name_no_harness_tool` reading all five preset folders.
4. A tracked factory agent file that comes back must fail a test. Pinned by `no_factory_agent_file_is_tracked_in_the_repo`.
5. A Rocket-only rule in any preset agent file must fail a test. Pinned by `preset_agent_files_name_no_rocket_only_term`.

## File Structure

| File | Responsibility |
|---|---|
| `crates/factory-core/presets/rust/config.toml` | the three build script globs |
| `crates/factory-core/src/preset.rs` | the two risk glob tests (tests module) |
| `docs/reference/project-toml-reference.md` | the preset's risk list in the docs |
| `.claude/agents/factory-*.md` | deleted, five files |
| `crates/factory-cli/tests/agent_lint.rs` | reads the preset files, the new tests |
| `crates/factory-cli/tests/tool_map.rs` | `agent_files_name_no_harness_tool` reads the preset folders |
| `crates/factory-core/presets/*/agents/factory-builder.md` | the match-conventions line, five files |
| `CLAUDE.md`, `docs/reference/software-factory.md`, `docs/operations/rocket-registration.md` | the lines that name `.claude/agents` |

### Task 1: Narrow the build script risk glob

**Files:**
- Modify: `crates/factory-core/presets/rust/config.toml`, `docs/reference/project-toml-reference.md`
- Test: `crates/factory-core/src/preset.rs` (tests module)

**Interfaces:**
- Produces: the rust preset `risk_paths` list with `"**/build.rs"` replaced by `"build.rs"`, `"*/build.rs"`, `"*/*/build.rs"`, in that position.

- [ ] **Step 1: Read the code this task touches**

Read the rust preset tests in `preset.rs` (`rust_config()` helper, `rust_preset_covers_the_root_manifest_and_lockfile`) and `classify_paths` and `glob_match` in `crates/factory-core/src/risk.rs`. In `glob_match`, `*` stays inside one path segment and matching ignores case.

- [ ] **Step 2: Write the failing tests**

In `preset.rs` tests: `rust_preset_marks_cargo_build_scripts_risky` asserts `classify_paths` on `["build.rs"]`, `["dashboard/build.rs"]` and `["crates/x/build.rs"]` is `DiffClass::Risky` for the config of `rust_config()`. `rust_preset_leaves_a_source_file_named_build_rs_alone` asserts `["crates/factory-engine/src/build.rs"]` and `["src/build.rs"]` are `DiffClass::Normal`.

- [ ] **Step 3: Run the tests to verify the second fails**

Run: `cargo test -j4 -p factory-core -- rust_preset_leaves_a_source_file_named_build_rs_alone`.
Expected: FAIL (`**/build.rs` matches).

- [ ] **Step 4: Change the preset and the docs**

Replace the glob in `risk_paths` of the rust preset. In `project-toml-reference.md` change the sentence at the line that lists the preset's risk paths and the example list below it, so both show the three globs. Add one sentence: a Cargo build script sits next to its package manifest, so a build script nested deeper than two folders is not matched, and its `Cargo.toml` stays risky.

- [ ] **Step 5: Run the tests, fmt and clippy**

Run: `cargo test -j4 -p factory-core`, then fmt and clippy for `factory-core`.
Expected: all pass, no warnings.

### Task 2: Delete the tracked agent copies and read the presets

**Files:**
- Delete: `.claude/agents/factory-builder.md`, `factory-code-reviewer.md`, `factory-committer.md`, `factory-kb-refresher.md`, `factory-spec-reviewer.md`
- Modify: `crates/factory-cli/tests/agent_lint.rs`, `crates/factory-cli/tests/tool_map.rs`, `CLAUDE.md`, `docs/reference/software-factory.md`, `docs/operations/rocket-registration.md`
- Test: `crates/factory-cli/tests/agent_lint.rs`, `crates/factory-cli/tests/tool_map.rs`

**Interfaces:**
- Produces, in `agent_lint.rs`: `const PRESETS: [&str; 5] = ["rust", "go", "ruby", "csharp", "node"];` and `fn preset_agent(preset: &str, name: &str) -> String` returning `crates/factory-core/presets/{preset}/agents/{name}`. `repo_root()` walks up until `crates/factory-core/presets` is a folder.
- Produces: tests `every_preset_ships_the_five_factory_agents` and `no_factory_agent_file_is_tracked_in_the_repo`.

- [ ] **Step 1: Read the code this task touches**

Read `agent_lint.rs` helpers (`repo_root`, `read`, `frontmatter`) and every place that names `.claude/agents` (lines for `only_the_known_factory_agents_remain`, the builder, reviewer, committer and kb tests, `spec_reviewer_is_read_only_and_pins_its_rules`, `agent_max_turns_match_the_driver_constants`, `REVIEWER_FILES`, `builder_files_say_the_builder_is_the_implementer_not_the_controller`, `builder_files_carry_the_quality_bar`, `the_committer_and_kb_files_have_no_rules_marker`, `builder_files_carry_the_rules_digest`), the comment above `MARKED`, and `agent_files_name_no_harness_tool` in `tool_map.rs`. The engine and CLI integration tests that write `.claude/agents` build their own temporary repos and are not touched.

- [ ] **Step 2: Write the failing tests**

- `every_preset_ships_the_five_factory_agents`: for each name in `PRESETS`, the preset's `agents` folder holds exactly `factory-builder.md`, `factory-code-reviewer.md`, `factory-committer.md`, `factory-kb-refresher.md` and `factory-spec-reviewer.md` among the files that start with `factory-`.
- `no_factory_agent_file_is_tracked_in_the_repo`: no file named `factory-*.md` exists in `.claude/agents` of the repo root (the folder itself may be missing).
- In `tool_map.rs`, make `agent_files_name_no_harness_tool` loop over the five preset folders and assert it saw 25 files.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -j4 -p factory-cli --test agent_lint -- no_factory_agent_file_is_tracked_in_the_repo` and `cargo test -j4 -p factory-cli --test tool_map -- agent_files_name_no_harness_tool`.
Expected: the first FAILS (the five files exist).

- [ ] **Step 4: Retarget the lint and delete the files**

Point every `.claude/agents/factory-X.md` read of `agent_lint.rs` at `preset_agent("rust", "factory-X.md")`, drop the `.claude/agents` entry from `REVIEWER_FILES` (6 to 5 entries) and from the other path lists, change `only_the_known_factory_agents_remain` to the new preset loop, remove the `MARKED` comment about the `.claude/agents` copies, then `git rm` the five files. Run the whole of `agent_lint`. A pin that fails against the rust preset text is handled by one rule: when the pin names a Rocket-only term (`DDD`, `Tauri`, `shadcn`, `rocket-infra`, `yarn`, `V8`), delete the pin; otherwise add the missing line to the rust preset file, and to the other presets when the pin runs over all of them. Name each deleted or added pin in your final reply.

- [ ] **Step 5: Update the docs that name `.claude/agents`**

In `CLAUDE.md` change the `agent_lint` bullet to say it lints the preset agent files in `crates/factory-core/presets/<preset>/agents/`, and remove `agents/factory-*.md` from the "Carried over from Rocket" paragraph. In the two docs files change each sentence that says the repo's `.claude/agents` files are the agents so it names the preset folders and the resolution order (the project's own `agents` folder, then the preset).

- [ ] **Step 6: Run the tests, fmt and clippy**

Run: `cargo test -j4 -p factory-cli --test agent_lint`, `cargo test -j4 -p factory-cli --test tool_map`, `cargo test -j4 -p factory-engine`, then fmt and clippy for `factory-cli`.
Expected: all pass, no warnings.

### Task 3: One builder line and a Rocket-term lint for all presets

**Files:**
- Modify: `crates/factory-core/presets/rust/agents/factory-builder.md`, and the same file in `go`, `ruby`, `csharp` and `node`
- Test: `crates/factory-cli/tests/agent_lint.rs`

**Interfaces:**
- Consumes: `PRESETS` and `preset_agent` from the previous task.
- Produces: tests `builder_files_tell_the_builder_to_match_conventions` and `preset_agent_files_name_no_rocket_only_term`.

- [ ] **Step 1: Read the code this task touches**

Read the Quality bar section of the rust preset builder file (the line about searching for an existing helper) and the five preset builder files' equivalent, plus `builder_files_carry_the_quality_bar`.

- [ ] **Step 2: Write the failing tests**

- `builder_files_tell_the_builder_to_match_conventions`: every preset builder file contains the exact builder line from Global Constraints.
- `preset_agent_files_name_no_rocket_only_term`: no file in any preset's `agents` folder contains `shadcn`, `lucide`, `Zustand`, `rocket-infra` or `Rocket`. `yarn` is also forbidden in every preset except `node`.

- [ ] **Step 3: Run the tests to verify the first fails**

Run: `cargo test -j4 -p factory-cli --test agent_lint -- builder_files_tell_the_builder_to_match_conventions`.
Expected: FAIL.

- [ ] **Step 4: Add the line to the five builder files**

Add the builder line as a bullet of the Quality bar list, directly after the line about searching for an existing helper. If a preset file already trips the Rocket-term test, remove that sentence from the preset.

- [ ] **Step 5: Run the tests, fmt and clippy**

Run: `cargo test -j4 -p factory-cli --test agent_lint`, `cargo test -j4 -p factory-core`, then fmt and clippy for `factory-cli`.
Expected: all pass, no warnings.

After the merge, run `factory presets refresh` on each machine and then `factory probe`, so the builder and reviewer show the preset text.

Next plan: docs/superpowers/plans/2026-11-01-factory-quality-fixes/plan-02-check-gates.md
