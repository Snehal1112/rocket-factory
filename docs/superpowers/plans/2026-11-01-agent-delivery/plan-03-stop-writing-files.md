# Agent delivery, Plan 03: Stop writing agent files into the repo

> **Spec:** docs/superpowers/specs/2026-11-01-agent-delivery-design.md (sections What goes away, Test double and probe, Compatibility)
>
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** The driver writes no factory agent file into a worktree or the main checkout. Job agents, the spec reviewer and `factory probe` all get their agents through `--agents`.

**Architecture:** The spec reviewer's separate file writer is replaced by an `agents.json` in the spec reviews folder. Then the file writing path (`write_agent_files`, `restore_agent_file`, the git exclude, the symlink guards and the shadow ruling) is deleted. `factory probe` delivery `agent` writes the agents file with the marker in the prompt. Docs name the `claude` version floor.

**Tech Stack:** Rust, `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`, `cargo test -j4 -p factory-core`.

## Global Constraints

- Always `-j4` and `-p <crate>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Each task ends with `cargo fmt -p <crate> -- --check` and `cargo clippy -j4 -p <crate> --all-targets -- -D warnings` for every crate it touched. No `#[allow(...)]` to silence a lint.
- No unwrap in production Rust. Tests use `.expect("...")` and `expect_err`.
- Code comments are short full sentences that end with a period.
- No new board, ledger or events field, and no new setting. Paths reach the driver only through `Layout`, so `no_hardcoded_paths` must stay green.
- Codex is untouched: it never read agent files from a worktree.
- Agent names that become JSON keys must still pass `is_agent_file_name`; `check_agent_names` stays and runs before the file is written.
- Texts are exact: the `claude` floor sentence is `The factory needs a claude CLI that accepts --agents together with --agent. claude 2.1.316 does.`

## Review Focus

1. The spec reviewer must still find its agent when the repo tracks no agent file and when it tracks its own. Pinned by `the_spec_reviewer_runs_through_the_agents_file` and `a_tracked_spec_reviewer_text_wins`.
2. No `.claude/agents` folder may appear in a worktree or the main checkout after a full job. Pinned by `no_agent_file_is_written_into_the_worktree`.
3. The "repo agent files shadow the resolved ones" ruling must be gone from the ledger. Pinned by `no_shadow_ruling_is_written`.
4. A job on a repo whose tracked agent files are deleted by the job must reach merge-ready review with the committer and reviewer having run. Pinned by `a_job_that_deletes_the_tracked_agent_files_reaches_review`.
5. `factory probe` with delivery `agent` must still show the marker through `--agents`. Pinned by `the_agent_delivery_probe_uses_the_agents_file`.

## File Structure

| File | Responsibility |
|---|---|
| `crates/factory-engine/src/spec_check.rs`, `crates/factory-engine/src/specs.rs`, `crates/factory-engine/src/compat.rs` | the spec reviewer uses the agents file |
| `crates/factory-engine/src/build.rs`, `crates/factory-engine/src/ports.rs` | remove the file writing path |
| `crates/factory-core/src/layout.rs` | remove helpers nothing calls |
| `crates/factory-engine/src/probe.rs` | the `agent` delivery |
| `crates/factory-engine/tests/agent_files.rs`, `spec_review.rs`, `probe.rs`, `crates/factory-cli/tests/project_flag.rs` | tests updated to the agents file |
| `docs/reference/software-factory.md`, `README.md`, `CLAUDE.md` | the agent file text and the version floor |

### Task 1: The spec reviewer through the agents file

**Files:**
- Modify: `crates/factory-engine/src/spec_check.rs`, `crates/factory-engine/src/specs.rs`, `crates/factory-engine/src/compat.rs`, `crates/factory-engine/tests/spec_review.rs`
- Test: `crates/factory-engine/tests/spec_review.rs`

**Interfaces:**
- Consumes: `agents_json`, `agents_file(dir)`, and the effective text rule of `Engine::effective_agent_files`, which this task copies onto `Driver` as `pub(crate) fn effective_agent_files(&self) -> Result<Vec<AgentFile>, EngineError>` (the same base branch precedence, using `Driver::repo()` and `Driver::agent_files()`).
- Changes: `write_reviewer_file(driver)` in `spec_check.rs` is replaced by `fn reviewer_agents_file(driver: &Driver) -> Option<PathBuf>` that writes `agents_file(&layout.spec_reviews_dir())` and returns its path, or `None` on any error. The spec reviewer's `AgentSpec` gets `agents_file` set to it when the project is not on Codex. The guards for symlinks, the git exclude and the "no marker means the user's own file" rule are removed with the writer, because nothing is written into the repo.

- [ ] **Step 1: Read the code this task touches**

Read `write_reviewer_file`, `reviewer_file_text` and the call at the review run in `spec_check.rs`, `spec_reviewer_agent` and `Driver::spec_reviewer_spec` in `specs.rs` and `compat.rs`, and the tests in `spec_review.rs` that read `.claude/agents` (around the symlink and the user's own file cases).

- [ ] **Step 2: Write the failing tests**

In `spec_review.rs`:
- `the_spec_reviewer_runs_through_the_agents_file`: with no tracked agent file, the recorded reviewer spec has an `agents_file` that parses to JSON holding `factory-spec-reviewer`, and no `.claude/agents` folder exists in the main checkout afterwards.
- `a_tracked_spec_reviewer_text_wins`: with the repo tracking `.claude/agents/factory-spec-reviewer.md` with the text `TRACKED-REVIEWER`, the JSON prompt for `factory-spec-reviewer` contains it.
Remove the tests that assert the old writer's guards (symlinked `.claude/agents`, the user's own file without a marker), and say which ones in your final reply.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -j4 -p factory-engine --test spec_review -- agents_file tracked_spec_reviewer`.
Expected: FAIL.

- [ ] **Step 4: Replace the writer**

Delete `write_reviewer_file` and its guards, add `reviewer_agents_file` and set the field on the spec in `spec_check.rs` where the spec reviewer spec is built.

- [ ] **Step 5: Run the tests, fmt and clippy**

Run: `cargo test -j4 -p factory-engine`, then fmt and clippy for `factory-engine`.
Expected: all pass, no warnings.

### Task 2: Delete the file writing path

**Files:**
- Modify: `crates/factory-engine/src/build.rs`, `crates/factory-engine/src/ports.rs`, `crates/factory-core/src/layout.rs`, `crates/factory-engine/tests/agent_files.rs`, `crates/factory-cli/tests/project_flag.rs`
- Test: `crates/factory-engine/tests/agent_files.rs`

**Interfaces:**
- Removes: `Engine::write_agent_files`, `Engine::restore_agent_file`, their call sites in `build.rs` and `ports.rs`, the shadow ruling text, the use of `AGENT_EXCLUDE_PATTERN`, and `AGENT_EXCLUDE_PATTERN` and `agent_files_rel_dir` themselves when `cargo clippy` shows nothing else calls them. `check_agent_names` stays and is called from `start_worktree` and from `write_agents_json`.

- [ ] **Step 1: Read the code this task touches**

Read every caller of `write_agent_files` (`build.rs` lines near the two calls, `ports.rs` in `run_agent`), `restore_agent_file`, and all tests in `agent_files.rs` and `project_flag.rs` that read a file under a worktree's `.claude/agents`.

- [ ] **Step 2: Write the failing tests**

In `agent_files.rs`:
- `no_agent_file_is_written_into_the_worktree`: after a full job run the worktree has no `.claude/agents/factory-*.md` file that the driver wrote, and the info exclude holds no `factory-*.md` line from the driver.
- `no_shadow_ruling_is_written`: with a tracked agent file, the job ledger holds no line that contains `shadow`.
- `a_job_that_deletes_the_tracked_agent_files_reaches_review`: the base branch tracks all five agent files, the builder effect deletes them, and the committer and reviewer runs both happen and carry an `agents_file` with the base branch text.
Rewrite `files_exist_before_the_builder_runs_and_stay_out_of_the_commit` and the other tests that read worktree agent files so they read `spec.agents_file` instead. Name each rewritten or removed test in your final reply.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -j4 -p factory-engine --test agent_files`.
Expected: FAIL.

- [ ] **Step 4: Remove the writer and fix the callers**

Delete the functions, the call sites and the dead constants and helpers. Keep the symlink check out: nothing is written, so nothing is followed. Run `cargo clippy -j4 -p factory-engine -p factory-core --all-targets -- -D warnings` to find leftovers.

- [ ] **Step 5: Run the tests, fmt and clippy**

Run: `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`, `cargo test -j4 -p factory-core`, then fmt and clippy for the three crates.
Expected: all pass, no warnings.

### Task 3: Probe delivery and docs

**Files:**
- Modify: `crates/factory-engine/src/probe.rs`, `docs/reference/software-factory.md`, `README.md`, `CLAUDE.md`
- Test: `crates/factory-engine/tests/probe.rs`

**Interfaces:**
- Changes: in `hide_claude_tools` (`probe.rs`), `Delivery::Agent` writes an `agents.json` in the probe work folder whose entry for the agent holds the agent text with the probe marker, and sets `spec.agents_file` to it. It no longer creates `.claude/agents` in the work folder. `AppendSystemPrompt` and `Stdin` are unchanged.

- [ ] **Step 1: Read the code this task touches**

Read `hide_claude_tools` and `probe_run` in `probe.rs`, the test at `crates/factory-engine/tests/probe.rs` that reads `.claude/agents/factory-builder.md`, and the sentences in `software-factory.md`, `README.md` (the Agent files section) and `CLAUDE.md` that describe how agent files reach the agents.

- [ ] **Step 2: Write the failing test**

`the_agent_delivery_probe_uses_the_agents_file` in `probe.rs`: a probe with delivery `agent` records a spec whose `agents_file` parses to JSON holding the probe marker in the agent prompt, and the work folder has no `.claude/agents`.

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -j4 -p factory-engine --test probe -- the_agent_delivery_probe_uses_the_agents_file`.
Expected: FAIL.

- [ ] **Step 4: Change the delivery and the docs**

Implement the `Agent` branch as described. In the three docs, replace the text that says the driver writes the agent files into worktrees with: the agents reach Claude through `--agents <job folder>/agents.json`, the repo's tracked `.claude/agents/<name>.md` text still wins, and add the floor sentence from Global Constraints once, in `software-factory.md`.

- [ ] **Step 5: Run the tests, fmt and clippy**

Run: `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli --test agent_lint --test tool_map --test no_hardcoded_paths`, then fmt and clippy for `factory-engine`.
Expected: all pass, no warnings.

After the merge run `cargo build -j4 -p factory-cli` so the installed `factory` binary uses the new delivery, then `factory probe` once to confirm the builder and reviewer still see the preset text.

Next plan: none
