# Agent delivery fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Not a factory job.** The user asked for this plan to be run by hand, not through `factory new`. Commit through the `dev-workflow-skills:1-git-commit` skill.

**Goal:** Close the gaps an audit found in the agent-delivery work (jobs 051 to 053): repo-local runs still depend on worktree agent files, the frontmatter parser mishandles three inputs, the spec review verdict ignores the text it ran on, tests miss real `agents.json` content, and docs, comments and one module still describe the old file writing.

**Architecture:** No new subsystem. Five focused changes on the existing `agents.json` path: harden `agents.rs`; let `effective_files` take its agents from the base branch when no agents were resolved; make the spec reviewer use the effective text for its review key and read its own file safely; add the missing tests; delete the dead `exclude_paths` and fix stale text.

**Tech Stack:** Rust, `cargo test -j4 -p <crate>`, `cargo clippy -j4 -p <crate> --all-targets -- -D warnings`.

**Spec:** docs/superpowers/specs/2026-11-01-agent-delivery-design.md (Goal: "a factory job never depends on agent files inside its worktree"; Acceptance criteria).

## Global Constraints

- Always `-j4` and `-p <crate>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Each task ends with `cargo fmt -p <crate> -- --check` and `cargo clippy -j4 -p <crate> --all-targets -- -D warnings` for every crate it touched. No `#[allow(...)]`.
- No unwrap in production Rust. Tests use `.expect("...")` and `expect_err`.
- Comments are short full sentences that end with a period.
- Paths reach the driver only through `Layout`. Git is never shelled out to.
- Codex behaviour is unchanged: `strip_frontmatter` is shared with `codex_prep.rs`, so the Codex tests must stay green after Task 1.
- The go, ruby, csharp and node preset configs stay byte for byte as they are.

## Review Focus

1. A CRLF agent file must keep its `tools` limit in `agents.json` (a read-only role must not get all tools). Pinned in Task 1.
2. An unterminated frontmatter block must not read `tools` from the body. Pinned in Task 1.
3. A repo-local job that deletes a tracked `.claude/agents/factory-*.md` must still get `--agents`. Pinned in Task 2.
4. Editing the spec reviewer text after an APPROVE must invalidate the saved verdict. Pinned in Task 3.
5. An empty or symlinked untracked reviewer file must not become the reviewer prompt. Pinned in Task 3.

## File Structure

| File | Responsibility |
|---|---|
| `crates/factory-core/src/agents.rs` | frontmatter parsing and `agents_json` |
| `crates/factory-engine/src/agents_file.rs` | effective agent files and `agents.json` writing |
| `crates/factory-engine/src/ports.rs`, `spec_check.rs` | the two `--agents` call sites |
| `crates/factory-engine/tests/agent_files.rs`, `spec_review.rs` | engine tests |
| `crates/factory-git/src/exclude.rs`, `lib.rs`, `tests/exclude.rs` | dead module to delete |
| docs, `cli.rs`, comments | stale text |

### Task 1: Harden the frontmatter parser

**Files:**
- Modify: `crates/factory-core/src/agents.rs`
- Test: `crates/factory-core/src/agents.rs` (tests module)

**Interfaces:**
- Consumes: `AgentFile { name, content }`.
- Produces: unchanged signatures `strip_frontmatter(&str) -> &str` and `agents_json(&[AgentFile]) -> String`.

- [ ] **Step 1: Write the failing tests**

In the tests module, with the existing `file(name, content)` helper:
- `an_empty_tools_line_gives_no_tools_key`: `tools:` and `tools: ,` produce an entry with no `tools` key.
- `empty_items_are_dropped_from_the_tools_list`: `tools: Read,,Grep,` gives `["Read","Grep"]`.
- `an_unterminated_frontmatter_block_reads_no_keys`: `"---\ndescription: x\ntools: Bash\n"` gives description equal to the name, no `tools` key, and the whole text as the prompt.
- `a_crlf_file_keeps_its_tools_and_prompt`: `"---\r\nname: x\r\ndescription: d\r\ntools: Read, Grep\r\n---\r\nBody\r\n"` gives description `d`, tools `["Read","Grep"]` and prompt `Body\r\n`.
- `strip_frontmatter_handles_crlf`: the same text gives `Body\r\n`.
- `the_json_is_the_same_for_reordered_input`: two orderings of the same files give equal text.

- [ ] **Step 2: Run them to see the failures**

Run: `cargo test -j4 -p factory-core agents::`
Expected: the empty-tools, unterminated and CRLF tests FAIL.

- [ ] **Step 3: Fix `strip_frontmatter` and `frontmatter_value` in `agents.rs`**

Add one private helper `split_frontmatter(text: &str) -> Option<(&str, &str)>` that returns `(block, body)` only when the text starts with `---` plus `\n` or `\r\n` and a closing `---` line exists (compare with `trim_end()`). Build both functions on it: `strip_frontmatter` returns the body (leading newlines trimmed as today) or the whole text when `None`; `frontmatter_value` searches only `block` lines, trimming `\r`, and returns `None` when `split_frontmatter` is `None`. In `agents_json`, split `tools` on commas, trim, drop empty items, and set the `tools` key only when the list is not empty.

- [ ] **Step 4: Run the tests**

Run: `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-engine codex`.
Expected: all pass, including the Codex tests that use `strip_frontmatter`.

- [ ] **Step 5: fmt and clippy for `factory-core`, then commit** (use the 1-git-commit skill).

### Task 2: Repo-local runs take their agents from the base branch

**Files:**
- Modify: `crates/factory-engine/src/agents_file.rs`, `crates/factory-engine/src/ports.rs`, `crates/factory-engine/src/spec_check.rs`
- Test: `crates/factory-engine/tests/agent_files.rs`

**Interfaces:**
- Consumes: `Repo::dir_at_branch(branch, rel) -> Result<Vec<(String, String)>>` (name, content), `agent_files_rel_dir()`, `is_agent_file_name`.
- Produces: `effective_files(repo, files)` returns the base branch's tracked `factory-*.md` files when `files` is empty. The `--agents` guards in `ports.rs::run_agent` and `spec_check.rs` use the effective list, not `agent_files`, to decide whether to send the flag.

- [ ] **Step 1: Write the failing tests** in `tests/agent_files.rs`, built on the existing tracked-file fixtures:
- `a_repo_local_job_gets_its_tracked_agents_in_the_agents_file`: a driver with no agent files and a base branch that tracks `.claude/agents/factory-builder.md` runs a builder step. The spec has `agents_file` set, and the JSON holds `factory-builder` with the tracked text.
- `a_repo_local_job_that_deletes_a_tracked_agent_file_still_gets_it`: the worktree deletes the file. The JSON is unchanged.
- `a_repo_without_tracked_agents_sends_no_agents_file`: no resolved files and nothing tracked sends no `agents_file`.
- `a_repo_local_codex_run_sends_no_agents_file`.

- [ ] **Step 2: Run them to see the failures**

Run: `cargo test -j4 -p factory-engine --test agent_files repo_local`
Expected: the first two FAIL.

- [ ] **Step 3: Implement**

In `agents_file.rs`: when `files` is empty, list `repo.dir_at_branch(repo.base_branch(), &rel_dir)` and keep entries where `is_agent_file_name(name)`; a listing error is an `EngineError::Msg` that names the folder. Change the guards in `ports.rs` (the `!self.agent_files.is_empty()` check) and `spec_check.rs` (`!driver.agent_files().is_empty()`) to compute the effective list once and send the flag when it is non-empty and the CLI is not Codex. `write_agents_json` must write that same list. The `ReviewerMissing` check in `spec_check.rs` keeps looking at resolved files only.

- [ ] **Step 4: Run the tests**

Run: `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`.
Expected: all pass.

- [ ] **Step 5: fmt and clippy for `factory-engine`, then commit.**

### Task 3: Spec reviewer: review key, own file, shared name check

**Files:**
- Modify: `crates/factory-engine/src/spec_check.rs`, `crates/factory-engine/src/agents_file.rs`, `crates/factory-engine/src/build.rs`
- Test: `crates/factory-engine/tests/spec_review.rs`

**Interfaces:**
- Produces: `fn check_names(files: &[AgentFile]) -> Result<(), EngineError>` in `agents_file.rs`, called by `Engine::check_agent_names` and by `reviewer_agents_file`, first, before any branch read.

- [ ] **Step 1: Write the failing tests** in `spec_review.rs`:
- `editing_a_tracked_reviewer_text_invalidates_a_saved_approve`: save an APPROVE, change the tracked reviewer file on the base branch, run again. The reviewer runs again.
- `editing_the_users_own_reviewer_file_invalidates_a_saved_approve`.
- `an_empty_untracked_reviewer_file_is_ignored`: zero bytes and whitespace only keep the resolved text.
- `a_symlinked_untracked_reviewer_file_is_ignored` (unix only).
- `a_non_utf8_untracked_reviewer_file_counts_as_no_file`.
- `a_bad_agent_name_is_refused_before_any_branch_read`.
- `the_reviewer_agents_file_lives_in_the_spec_reviews_folder`: the path is under `layout().spec_reviews_dir()` and the repo holds no `agents.json`.
- `an_agents_file_write_error_ends_the_review_as_failed`: make the target a directory so the write fails. The refusal is `ReviewFailed`.

- [ ] **Step 2: Run them to see the failures**

Run: `cargo test -j4 -p factory-engine --test spec_review`
Expected: the two invalidation tests, the empty, symlink and name-order tests FAIL.

- [ ] **Step 3: Implement**

- Compute the effective reviewer text once (tracked text, else a valid own file, else resolved) in a helper `effective_reviewer_text(driver) -> Result<String, String>`, and use it both for `review_key` and for `reviewer_agents_file`. A read error there is the same `ReviewFailed` reason.
- The own-file read accepts only a regular file (`symlink_metadata` then `is_file`) whose trimmed text is not empty and has no rules marker; any read error counts as no file.
- Move the name loop into `check_names` and call it first.
- Drop the extra `is_tracked_at_branch` call: have `effective_files` report which names were tracked, or compare against the resolved text, whichever keeps one tree lookup per file.

- [ ] **Step 4: Run the tests**

Run: `cargo test -j4 -p factory-engine`.
Expected: all pass.

- [ ] **Step 5: fmt and clippy for `factory-engine`, then commit.**

### Task 4: Fill the test gaps

**Files:**
- Test: `crates/factory-engine/tests/agent_files.rs`, `crates/factory-agent/src/spec.rs` (tests module), `crates/factory-core/src/preset.rs` (tests module)

- [ ] **Step 1: Write the tests** (they pass on the current code; check each fails when its behaviour is broken by a quick local revert, then restore):
- `the_agents_file_carries_tools_and_descriptions`: parse the real `agents.json` of a rust-preset job. The reviewer and spec reviewer have `tools` equal to `["Read","Grep","Glob"]`; the builder and committer have no `tools` key; every entry has a non-empty description.
- `a_tracked_edit_in_the_worktree_does_not_change_the_agents_file`: the builder commits a modified tracked agent file onto the job branch. The JSON still holds the base branch text.
- `a_non_utf8_tracked_agent_file_stops_the_run`: a tracked blob that is not UTF-8, in place of the symlink case. The error names the file and no agent runs.
- `an_agents_json_write_error_stops_the_run`: make `agents.json` a directory. The step fails with an error and no agent runs.
- `a_codex_spec_reviewer_sends_no_agents_file`, `a_codex_reviewer_run_sends_no_agents_file`.
- In `spec.rs` tests: `a_codex_spec_gets_no_agents_flag_even_with_a_file` (caps `agents_file` false).
- In `preset.rs` tests: replace the substring check of `only_the_rust_preset_plans_clippy` by a pinned SHA-256 or exact text per other preset config, so any change to them fails.

- [ ] **Step 2: Run the tests**

Run: `cargo test -j4 -p factory-engine --test agent_files`, `cargo test -j4 -p factory-agent`, `cargo test -j4 -p factory-core preset`.
Expected: all pass.

- [ ] **Step 3: fmt and clippy for the touched crates, then commit.**

### Task 5: Remove dead code and fix stale text

**Files:**
- Delete: `crates/factory-git/src/exclude.rs`, `crates/factory-git/tests/exclude.rs`
- Modify: `crates/factory-git/src/lib.rs` (remove `mod exclude;`)
- Modify (text only): `docs/guides/getting-started.md` (the `.git/info/exclude` sentence), `README.md` (the marker sentence: the marker is read from the effective text, not the worktree), `docs/guides/factory-user-guide.md` and `crates/factory-cli/src/cli.rs` (`agent` delivery now means `--agents <job folder>/agents.json`), `crates/factory-agent/src/spec.rs` (the flags comment: four flags, claude 2.1.316)
- Modify (comments only): `layout.rs`, `resolve.rs`, `engine/src/lib.rs` (two), `compat.rs` (two), `cli/src/commands.rs`, `probe.rs` (two), `agents_file.rs`

- [ ] **Step 1: Confirm nothing else calls `exclude_paths`**

Run: `grep -rn "exclude_paths" crates`
Expected: hits only in `exclude.rs` and its own test.

- [ ] **Step 2: Delete the module and its test, edit the texts**

Reword each comment so it says agents travel in `agents.json` and nothing is written into worktrees or checkouts. Change no code in this step beyond the deletion.

- [ ] **Step 3: Check nothing stale remains**

Run: `grep -rnE "written agent file|written into (each|new)|writes into checkouts|info/exclude" crates docs README.md CLAUDE.md`
Expected: no hit that describes agent files being written (hits about other git excludes are fine).

- [ ] **Step 4: Run the tests**

Run: `cargo test -j4 -p factory-git`, `cargo test -j4 -p factory-cli`, `cargo test -j4 -p factory-engine`.
Expected: all pass, including `no_hardcoded_paths`.

- [ ] **Step 5: fmt and clippy for `factory-git`, `factory-cli`, `factory-engine`, then commit.**

## Out of scope

- A misleading ledger note when `agent_rules_current` swallows a tracked-file read error (`specs.rs:48`). The run still stops with the real error. File it as an issue.
- YAML block scalars and quoted frontmatter values. The preset files use neither.
- Adding clippy to this repo's own `project.toml` checks. That is a manual edit outside the repo.
