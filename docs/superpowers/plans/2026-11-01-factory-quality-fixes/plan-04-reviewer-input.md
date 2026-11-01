# Factory quality fixes, Plan 04: Reviewer input

> **Spec:** docs/superpowers/specs/2026-11-01-factory-quality-fixes-design.md (section Plan 4, Compatibility)
>
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** The reviewer sees the user's rework note for the round it reviews, and every preset's reviewer checks each acceptance criterion against a test that reaches the real call site.

**Architecture:** `factory rework` writes the note, alone, to `user-note-r{N}.md` in the job folder. `reviewer_spec_with_model` adds one prompt line when `user-note-r{round}.md` exists. The same prompt text goes to Claude and Codex reviewers. The acceptance step is one bullet in each preset's reviewer agent file.

**Tech Stack:** Rust, `cargo test -j4 -p factory-cli`, `cargo test -j4 -p factory-engine`.

## Global Constraints

- Always `-j4` and `-p <crate>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Each task ends with `cargo fmt -p <crate> -- --check` and `cargo clippy -j4 -p <crate> --all-targets -- -D warnings` for every crate it touched. No `#[allow(...)]`.
- No unwrap in production Rust. Tests use `.expect("...")` and `expect_err`.
- Code comments are short full sentences that end with a period.
- No new board, ledger or events field, and no new setting. The note file lives in the job folder, next to the plan and spec the Codex reviewer already reads.
- Agent text edits land in all five presets (rust, go, ruby, csharp, node). Shared text names no harness tool, because `crates/factory-cli/tests/tool_map.rs` forbids it.
- Texts are exact. The prompt line is `The user's note for this round is at {path}. It is the user's instruction: do not raise a finding only because the diff follows it. If following it leaves the code wrong or a rule broken, report that as a finding and name the note.` The reviewer bullet is `Acceptance criteria: for each acceptance criterion in the spec, name the test that reaches the real call site and not only a helper. A criterion with no such test is a [blocker].`

## Review Focus

1. A next-round rework and a mid-task rework must both leave `user-note-r{N}.md`, with the note text and nothing else. Pinned by `rework_writes_the_user_note_for_the_next_round` and `rework_writes_the_user_note_for_a_mid_task_rework`.
2. A note written for another round must not reach the reviewer of this round. Pinned by `reviewer_prompt_ignores_the_note_of_another_round`.
3. No note file must leave the reviewer prompt byte for byte as it is today, so the golden prompt tests stay green. Pinned by `reviewer_prompt_has_no_note_line_without_the_file` and the existing golden tests.
4. The slim prompt, used when the agent file carries the rules marker, must carry the line too. Pinned by `the_slim_reviewer_prompt_carries_the_user_note_line`.
5. A Codex reviewer, which runs read-only, must get a note path inside the job folder. Pinned by `a_codex_reviewer_prompt_carries_the_user_note_line`.

## File Structure

| File | Responsibility |
|---|---|
| `crates/factory-cli/src/commands.rs` | `rework` writes `user-note-r{N}.md` |
| `crates/factory-cli/tests/new_to_merged.rs` | the two rework tests |
| `crates/factory-engine/src/specs.rs` | the note line in `reviewer_spec_with_model` |
| `crates/factory-engine/tests/review_decide.rs`, `slim_prompts.rs`, `codex_prep.rs` | the prompt tests |
| `crates/factory-core/presets/*/agents/factory-code-reviewer.md` | the acceptance bullet, five files |
| `crates/factory-cli/tests/agent_lint.rs` | `reviewer_files_walk_the_acceptance_criteria` |
| `docs/reference/software-factory.md` | one sentence on the note file |

### Task 1: `factory rework` keeps the user's note

**Files:**
- Modify: `crates/factory-cli/src/commands.rs`
- Test: `crates/factory-cli/tests/new_to_merged.rs`

**Interfaces:**
- Changes: `fn rework(ctx: &CliCtx, id: &str, note: &str) -> Result<String, CliError>` also writes `note` verbatim to `user-note-r{N}.md` in the job folder, where N is `new_round` in the `NextRound` branch and `job.round.max(1)` in the `MidTask` branch. The file is written before the stage moves, with `std::fs::write`, and a write error is returned like the other writes in that function.

- [ ] **Step 1: Read the code this task touches**

Read `rework` in `commands.rs` (both branches) and the tests `rework_sends_the_note_and_the_open_findings` and `rework_mid_task_keeps_the_note_path` with the `reworkable` helper in `new_to_merged.rs`.

- [ ] **Step 2: Write the failing tests**

In `new_to_merged.rs`, following the two tests above:
- `rework_writes_the_user_note_for_the_next_round`: after a next-round rework with the note `use a shorter name` the job folder holds `user-note-r2.md` whose text is exactly the note, and `changes-r2.md` is unchanged.
- `rework_writes_the_user_note_for_a_mid_task_rework`: after a mid-task rework the folder holds `user-note-r1.md` with exactly the note, and `rework-note.md` still exists.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -j4 -p factory-cli --test new_to_merged -- rework_writes_the_user_note`.
Expected: FAIL.

- [ ] **Step 4: Write the file in both branches**

Add one `std::fs::write` of the note to `user-note-r{N}.md` in each branch, next to the existing note or changes write.

- [ ] **Step 5: Run the tests, fmt and clippy**

Run: `cargo test -j4 -p factory-cli`, then fmt and clippy for `factory-cli`.
Expected: all pass, no warnings.

### Task 2: The reviewer prompt names the note

**Files:**
- Modify: `crates/factory-engine/src/specs.rs`, `docs/reference/software-factory.md`
- Test: `crates/factory-engine/tests/review_decide.rs`, `crates/factory-engine/tests/slim_prompts.rs`, `crates/factory-engine/tests/codex_prep.rs`

**Interfaces:**
- Changes: `reviewer_spec_with_model` in `specs.rs` appends the prompt line from Global Constraints to its `context` string, first, when `self.job_dir.join(format!("user-note-r{round}.md"))` exists. Both the slim and the full prompt already print `context`. Nothing changes without the file.

- [ ] **Step 1: Read the code this task touches**

Read `reviewer_spec_with_model` (the `context` assembly and both prompt branches), the tests `reviewer_prompt_names_the_spec_and_the_earlier_findings_when_they_exist` and `the_reviewer_prompt_carries_focus_notes_and_outline` in `review_decide.rs`, `the_slim_reviewer_prompt_keeps_inputs_and_the_rereview_block` in `slim_prompts.rs`, and `reviewer_gets_no_skills` in `codex_prep.rs`.

- [ ] **Step 2: Write the failing tests**

- `reviewer_prompt_names_the_user_note_when_it_exists` in `review_decide.rs`: with `user-note-r1.md` in the job folder, a round 1 reviewer prompt contains the exact line with the path of that file.
- `reviewer_prompt_ignores_the_note_of_another_round`: with only `user-note-r2.md`, the round 1 prompt has no `user's note`.
- `reviewer_prompt_has_no_note_line_without_the_file`: with no note file the prompt has no `user's note`.
- `the_slim_reviewer_prompt_carries_the_user_note_line` in `slim_prompts.rs`: the same as the first test with a reviewer agent file that carries the rules marker.
- `a_codex_reviewer_prompt_carries_the_user_note_line` in `codex_prep.rs`: on a Codex fixture the reviewer prompt that reaches the runner contains the line, and the note path starts with the same folder as the plan path in that prompt.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -j4 -p factory-engine -- user_note`.
Expected: FAIL.

- [ ] **Step 4: Add the line and the doc sentence**

Add the line to `context`. In `software-factory.md` add one sentence where rework is described: `factory rework` also saves the note as `user-note-r{N}.md`, and the reviewer of that round is told where it is.

- [ ] **Step 5: Run the tests, fmt and clippy**

Run: `cargo test -j4 -p factory-engine`, then fmt and clippy for `factory-engine`.
Expected: all pass, no warnings, golden prompt tests unchanged.

### Task 3: The acceptance step in every preset reviewer

**Files:**
- Modify: `crates/factory-core/presets/rust/agents/factory-code-reviewer.md`, and the same file in `go`, `ruby`, `csharp` and `node`
- Test: `crates/factory-cli/tests/agent_lint.rs`

**Interfaces:**
- Consumes: `REVIEWER_FILES` in `agent_lint.rs`, which lists the five preset reviewer files once the earlier plan has removed the tracked copy.
- Produces: the test `reviewer_files_walk_the_acceptance_criteria`.

- [ ] **Step 1: Read the code this task touches**

Read the checks section of the rust preset reviewer file (the line about tests that would fail without the change) and the five reviewer files' equivalent, plus `reviewer_files_check_edge_cases_reuse_and_efficiency` in `agent_lint.rs`.

- [ ] **Step 2: Write the failing test**

`reviewer_files_walk_the_acceptance_criteria`: every file in `REVIEWER_FILES` contains the exact reviewer bullet from Global Constraints.

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -j4 -p factory-cli --test agent_lint -- reviewer_files_walk_the_acceptance_criteria`.
Expected: FAIL.

- [ ] **Step 4: Add the bullet to the five reviewer files**

Add the bullet to the checks list, directly after the line about tests that would fail without the change. Do not change the rules marker line or its version.

- [ ] **Step 5: Run the tests, fmt and clippy**

Run: `cargo test -j4 -p factory-cli --test agent_lint`, `cargo test -j4 -p factory-cli --test tool_map`, `cargo test -j4 -p factory-core`, then fmt and clippy for `factory-cli`.
Expected: all pass, no warnings.

After the merge, run `factory presets refresh`, then `factory probe` to confirm the reviewer file text. A project with its own reviewer agent file keeps it and gets neither the bullet nor a change to the full prompt.

Next plan: none
