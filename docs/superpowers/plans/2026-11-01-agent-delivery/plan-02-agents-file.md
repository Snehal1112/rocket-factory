# Agent delivery, Plan 02: The agents file

> **Spec:** docs/superpowers/specs/2026-11-01-agent-delivery-design.md (sections Delivery of agents on Claude, Precedence of a repo's own agent file, Test double and probe)
>
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Every Claude run in a job gets its agents through `--agents <job folder>/agents.json`, built from the resolved agents with the repo's tracked agent text taking precedence, while the old file writing still runs, so this plan only adds.

**Architecture:** A pure function in `factory-core` turns agent files into the `--agents` JSON. `AgentSpec` and `Caps` learn the flag. The engine decides each agent's effective text (the base branch's tracked text, else the resolved text), writes `agents.json` in the job folder before a run, and reads the rules marker from that same text. Codex is not touched.

**Tech Stack:** Rust, `cargo test -j4 -p factory-agent`, `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-engine`.

## Global Constraints

- Always `-j4` and `-p <crate>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Each task ends with `cargo fmt -p <crate> -- --check` and `cargo clippy -j4 -p <crate> --all-targets -- -D warnings` for every crate it touched. No `#[allow(...)]` to silence a lint.
- No unwrap in production Rust. Tests use `.expect("...")` and `expect_err`.
- Code comments are short full sentences that end with a period.
- No new board, ledger or events field, and no new setting. Paths reach the driver only through `Layout`, so `no_hardcoded_paths` must stay green.
- Codex runs send no `--agents`. The Codex path (`codex_prep.rs`, the Codex runner args) does not change.
- The JSON shape is exact: an object keyed by the agent name (the file name without `.md`), each value `{"description": <string>, "prompt": <string>}` plus `"tools": [<string>, ...]` only when the frontmatter has a `tools:` line (split on commas, trimmed). `prompt` is the text after the closing `---` of the frontmatter with leading blank lines removed. A file without frontmatter gives a description equal to its name and the whole text as the prompt.
- The file name is exactly `agents.json`.

## Review Focus

1. A tracked agent file that a job deletes or edits in its worktree must not change what the agents run on: the JSON carries the base branch text. Pinned by `a_job_that_deletes_a_tracked_agent_file_still_gets_its_agents`.
2. A repo's tracked agent text must win over the preset text in the JSON. Pinned by `a_tracked_agent_text_wins_in_the_agents_file`.
3. A read-only role must keep its tool limit through the JSON. Pinned by `tools_line_becomes_a_tools_list` and `a_frontmatter_without_tools_gives_no_tools_key`.
4. The rules marker check must read the effective text, not the worktree, so a deleted worktree file cannot make the prompt fall back to the long form. Pinned by `the_rules_marker_is_read_from_the_effective_text`.
5. A Codex run must carry no `agents_file`. Pinned by `a_codex_run_sends_no_agents_file`.
6. A tracked agent file that cannot be read must stop the run and name the file, not fall back to the preset text. Pinned by `an_unreadable_tracked_agent_file_stops_the_run`.

## File Structure

| File | Responsibility |
|---|---|
| `crates/factory-agent/src/spec.rs` | `Caps.agents_file`, `AgentSpec.agents_file`, the `--agents` argument, tests |
| `crates/factory-agent/src/bin/fake-claude.rs` | reads `--agents <file>` before `.claude/agents` |
| `crates/factory-core/src/agents.rs` (new), `crates/factory-core/src/lib.rs`, `crates/factory-core/src/layout.rs` | `agents_json`, `agents_file`, their tests |
| `crates/factory-engine/src/agents_file.rs` (new), `crates/factory-engine/src/lib.rs`, `crates/factory-engine/src/ports.rs`, `crates/factory-engine/src/specs.rs` | effective text, writing the file, wiring into `run_agent`, the marker check |
| `crates/factory-engine/tests/agent_files.rs` | the engine tests |

### Task 1: The `--agents` flag and the test double

**Files:**
- Modify: `crates/factory-agent/src/spec.rs`, `crates/factory-agent/src/bin/fake-claude.rs`
- Test: `crates/factory-agent/src/spec.rs` (tests module)

**Interfaces:**
- Produces: `pub agents_file: bool` on `Caps` (default `true`, like the other flags) and `pub agents_file: Option<std::path::PathBuf>` on `AgentSpec` (default `None`). In `AgentSpec::args`, when `agent` is set, `caps.agent` and `caps.agents_file` are true and `agents_file` is `Some(path)`, push `--agents` and the path right after `--agent <name>`.
- Produces: `fake-claude` reads the value of `--agents` as a JSON file and takes the marker text from the `prompt` of the entry named by `--agent`, and falls back to `.claude/agents/<name>.md` when the flag or the entry is absent.

- [ ] **Step 1: Read the code this task touches**

Read `Caps`, `AgentSpec`, `AgentSpec::args` and its tests in `spec.rs`, and `probe_marker` in `fake-claude.rs`.

- [ ] **Step 2: Write the failing tests**

In the `spec.rs` tests module:
- `agents_flag_follows_the_agent_flag`: with `agent = Some("factory-builder")` and `agents_file = Some("/j/agents.json")` the argument list holds `--agent`, `factory-builder`, `--agents`, `/j/agents.json` in that order.
- `no_agents_flag_without_a_file_or_without_the_capability`: `agents_file = None` gives no `--agents`, and `Caps { agents_file: false, ..Caps::default() }` gives none either.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -j4 -p factory-agent -- agents_flag`.
Expected: FAIL (compile error, the fields do not exist).

- [ ] **Step 4: Implement the fields, the argument and the fake**

Add the two fields, push the argument next to `--agent`, and extend `probe_marker` to read the `--agents` file first. Update every struct literal of `Caps` and `AgentSpec` that the compiler flags.

- [ ] **Step 5: Run the tests, fmt and clippy**

Run: `cargo test -j4 -p factory-agent`, `cargo test -j4 -p factory-engine`, then fmt and clippy for `factory-agent`.
Expected: all pass, no warnings.

### Task 2: Build the agents JSON

**Files:**
- Create: `crates/factory-core/src/agents.rs`
- Modify: `crates/factory-core/src/lib.rs`, `crates/factory-core/src/layout.rs`
- Test: `crates/factory-core/src/agents.rs` (tests module)

**Interfaces:**
- Consumes: `AgentFile { name, content }` from `crates/factory-core/src/resolve.rs`.
- Produces: `pub fn agents_json(files: &[AgentFile]) -> String` with the shape from Global Constraints, keys in file name order, and `pub fn agents_file(dir: &Path) -> PathBuf` in `layout.rs` returning `dir.join("agents.json")`. Both are re-exported from the crate root.

- [ ] **Step 1: Read the code this task touches**

Read `AgentFile` in `resolve.rs`, `rules_marker` in `agent_drift.rs` (how a text is read line by line), and the existing preset builder and reviewer files for the frontmatter shape (`name`, `description`, `maxTurns`, `tools`).

- [ ] **Step 2: Write the failing tests**

In `agents.rs` tests:
- `tools_line_becomes_a_tools_list`: a file with frontmatter `tools: Read, Grep, Glob` gives `"tools":["Read","Grep","Glob"]` for that agent.
- `a_frontmatter_without_tools_gives_no_tools_key`: the builder file of the rust preset (`include_str!` of `../presets/rust/agents/factory-builder.md`) gives no `tools` key and a prompt that starts with `<!-- factory-rules: 1 -->`.
- `name_description_and_prompt_come_from_the_file`: key `factory-builder`, description equal to the frontmatter `description`, prompt equal to the body.
- `a_file_without_frontmatter_uses_its_name_as_the_description`.
- `all_five_preset_agents_become_one_json_object`: for every name in `PRESET_NAMES` the five embedded agent files give an object with five keys that parses as JSON.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -j4 -p factory-core -- agents::`.
Expected: FAIL (compile error).

- [ ] **Step 4: Implement `agents_json` and `agents_file`**

Approach: reuse the frontmatter split already used by `strip_frontmatter` in `crates/factory-engine/src/codex_prep.rs` by moving that function into `agents.rs` as `pub fn strip_frontmatter(text: &str) -> &str` and importing it from the engine, so there is one copy, with its test `strip_frontmatter_removes_only_a_leading_block`. Build the value with `serde_json`.

- [ ] **Step 5: Run the tests, fmt and clippy**

Run: `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-engine`, then fmt and clippy for `factory-core` and `factory-engine`.
Expected: all pass, no warnings.

### Task 3: Effective text, the file in the job folder and the wiring

**Files:**
- Create: `crates/factory-engine/src/agents_file.rs`
- Modify: `crates/factory-engine/src/lib.rs`, `crates/factory-engine/src/ports.rs`, `crates/factory-engine/src/specs.rs`
- Test: `crates/factory-engine/tests/agent_files.rs`

**Interfaces:**
- Consumes: `agents_json`, `agents_file`, `Repo::file_at_branch(branch, rel) -> Result<Option<String>>`, `Repo::base_branch()`, `agent_files_rel_dir()`, `Engine.agent_files` and `Engine.job_dir`.
- Produces, on `Engine`: `pub(crate) fn effective_agent_files(&self) -> Result<Vec<factory_core::AgentFile>, EngineError>` returning the resolved files with each file replaced by the base branch's text of `.claude/agents/<name>` when the base branch tracks it, and `pub(crate) fn write_agents_json(&self) -> Result<std::path::PathBuf, EngineError>` writing `agents_file(&self.job_dir)` with `write_atomic` only when the text differs, and returning the path.
- Changes: `run_agent` in `ports.rs` sets `spec.agents_file = Some(self.write_agents_json()?)` when `self.config.agent_cli() != "codex"` and `self.agent_files` is not empty, after `shape_spec`. `agent_rules_current` in `specs.rs` reads the marker from `effective_agent_files()` by name and no longer from the worktree path.

- [ ] **Step 1: Read the code this task touches**

Read `run_agent` in `ports.rs`, `write_agent_files` and `restore_agent_file` in `build.rs` (they stay in this plan), `agent_rules_current` and `slim_prompt` in `specs.rs`, and the tests in `agent_files.rs` (`files_exist_before_the_builder_runs_and_stay_out_of_the_commit`, `a_file_tracked_on_the_base_branch_is_not_overwritten`) with the fixture helpers they use.

- [ ] **Step 2: Write the failing tests**

In `agent_files.rs`:
- `a_tracked_agent_text_wins_in_the_agents_file`: the base branch tracks `.claude/agents/factory-builder.md` with a marker line `TRACKED-TEXT`, the preset file list holds other text. After a builder run the recorded `spec.agents_file` parses to a JSON object whose `factory-builder` prompt contains `TRACKED-TEXT`.
- `a_job_that_deletes_a_tracked_agent_file_still_gets_its_agents`: the same fixture, the builder effect deletes the file in the worktree and the committer, reviewer and message runs that follow still carry an `agents_file` whose JSON holds `TRACKED-TEXT` for `factory-builder` and the resolved text for the other four agents.
- `the_rules_marker_is_read_from_the_effective_text`: with the worktree copy removed, a builder prompt for a file list whose builder text carries `<!-- factory-rules: 1 -->` is the short form.
- `a_codex_run_sends_no_agents_file`: with the project on Codex, every recorded spec has `agents_file == None`.
- `an_unreadable_tracked_agent_file_stops_the_run`: the base branch tracks `.claude/agents/factory-builder.md` as a symlink, and `effective_agent_files` returns an error whose text contains `factory-builder.md`.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -j4 -p factory-engine --test agent_files -- agents_file tracked_agent`.
Expected: FAIL.

- [ ] **Step 4: Implement the three pieces and wire them in**

Put `effective_agent_files` and `write_agents_json` in the new module. A read error from `file_at_branch` stops the run: `effective_agent_files` returns an `EngineError::Msg` that names the file, and nothing falls back to the resolved text.

- [ ] **Step 5: Run the tests, fmt and clippy**

Run: `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`, then fmt and clippy for `factory-engine`.
Expected: all pass, no warnings.

Next plan: docs/superpowers/plans/2026-11-01-agent-delivery/plan-03-stop-writing-files.md
