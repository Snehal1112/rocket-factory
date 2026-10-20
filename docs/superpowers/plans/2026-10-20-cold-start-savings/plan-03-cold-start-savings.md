# Cold-start savings, plan 3: slim builder and reviewer prompts

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** The builder and reviewer prompts drop the rules their agent files already carry, but only when the worktree's agent file has a current `<!-- factory-rules: N -->` marker. Without it the prompts stay byte for byte what they are today.

**Architecture:** Golden tests first pin today's four full prompts. A marker reader and `Engine::agent_rules_current` decide per run. The slim text is built beside the full text, so the full text cannot drift. A fallback writes one ledger ruling per job and agent.

**Tech Stack:** Rust.

**Spec:** docs/superpowers/specs/2026-10-20-cold-start-savings-design.md

## Global Constraints

- Always `-j4`. Never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- No `unwrap` in production code. Tests use `.expect("...")` and `expect_err`. No git shell-outs.
- The marker is the exact line `<!-- factory-rules: N -->`. `RULES_VERSION` is 1. A missing file, a missing marker, an unparsable number or a number below `RULES_VERSION` gives the full prompt. A larger number counts as current.
- A fallback never fails the run. Its ledger ruling is written once per job and agent through the `progress.md` contains-check that `write_agent_files` uses.
- Slim prompts keep: branch, read line, `Never edit the plan.`, spec line, excerpt, base and tip, earlier commits, `Do only Task N: ...`, the config rules line, the KB line, and in the task prompt the `Never sleep or poll, never run a command in the background and wait for it ...` line. The first-run prompt has no sleep line today and gets none.
- The interrupted prompt (`builder_task_resume_spec`) is unchanged.

## Review Focus

1. An agent file with the marker inside other text, for example `see <!-- factory-rules: 1 --> here`: not current, full prompt (test `a_marker_inside_other_text_does_not_count`, Task 2).
2. A marker number above the engine's: current (test `a_newer_marker_counts_as_current`, Task 2).
3. A fix round whose first build ran without the file and whose fix round has it: the slim fix prompt, because the check happens when the fix round starts (test `the_fix_round_checks_the_agent_file_when_it_starts`, Task 2).
4. The reviewer worktree lacks the agent file: full prompt and a ruling (test `a_missing_reviewer_file_keeps_the_full_prompt_and_notes_it`, Task 3).
5. Two tasks in one job with an old file: one ruling, not two (test `the_fallback_is_noted_once_per_job_and_agent`, Task 2).

## File Structure

- `crates/factory-engine/tests/slim_prompts.rs` (create): helpers, golden tests and the slim tests of this plan.
- `crates/factory-engine/src/specs.rs` (modify): `RULES_VERSION`, `rules_marker`, `agent_rules_current`, the slim variants.
- `crates/factory-engine/src/lib.rs` (modify): re-export `RULES_VERSION`.

### Task 1: Golden tests for today's full prompts

**Files:**
- Create: `crates/factory-engine/tests/slim_prompts.rs`

**Interfaces:**
- Consumes: `Fixture`, `committing_fixture`, `scripted_auto`, `scripted`, `Effect` from `tests/common/mod.rs`; `factory_core::AgentFile { name, content }`; `Engine::with_agent_files(Vec<AgentFile>)`; `Engine::build_stage`, `checks_stage`, `review_stage`; `ScriptedRunner::last_spec_for(role)`. The task fixture is the one in `tests/build_checks.rs` (`two_task_fixture`): `committing_fixture()`, then `std::fs::write(&fx.plan_file, "# P\n\n### Task 1: first thing\n\nx\n\n### Task 2: second thing\n\ny\n")` and the board job set to `Stage::Build`.
- Produces (test helpers, reused by Tasks 2 and 3):
  - `fn agent_file(name: &str, marker: Option<&str>) -> AgentFile` (content is a frontmatter block, then the marker line when given, then `body`).
  - `fn norm(fx: &Fixture, prompt: &str) -> String`: replaces the fixture's temp dir (`fx.tmp.path()`) with `<TMP>`.
  - `fn first_prompt(files: Vec<AgentFile>) -> (Fixture, String)`: `committing_fixture()`, one `build_stage` with a builder that writes `a.txt`, returns the builder prompt.
  - `fn task_prompt(files: Vec<AgentFile>) -> (Fixture, String)`: the same on the task fixture, so task 1 runs.
  - `fn fix_prompt(first: Vec<AgentFile>, fix: Vec<AgentFile>) -> (Fixture, String)`: a first build on `committing_fixture()` with `first`, then `changes-r2.md`, round 2 and stage Build as in `the_resume_prompt_names_check_hints_only_when_set`, then a second `build_stage` on an engine with `fix`; returns the resumed builder prompt.
  - `fn review_prompt(files: Vec<AgentFile>) -> (Fixture, String)`: build and checks as `build_and_pass_checks` in `tests/review_decide.rs`, both engines built with `files`, then `review_stage`; returns the reviewer prompt.
  - `const GOLDEN_FIRST`, `GOLDEN_TASK`, `GOLDEN_FIX`, `GOLDEN_REVIEW: &str`.

- [ ] **Step 1: Read** `crates/factory-engine/tests/common/mod.rs` (`Fixture`, `committing_fixture`, `ScriptedRunner`), `build_checks.rs` (`two_task_fixture`, `change_effect`, the fix round test above), `review_decide.rs` (`build_and_pass_checks`, `reviewer`) and `agent_files.rs` (how `with_agent_files` is used).
- [ ] **Step 2: Write the failing golden tests.** Each constant starts as the empty string. One full example:

```rust
const GOLDEN_TASK: &str = "";

#[test]
fn the_full_task_builder_prompt_matches_golden_text() {
    let (fx, prompt) = task_prompt(Vec::new());
    assert_eq!(norm(&fx, &prompt), GOLDEN_TASK);
}
```

  The other tests: `the_full_first_builder_prompt_matches_golden_text` (`first_prompt`, `GOLDEN_FIRST`), `the_full_fix_prompt_matches_golden_text` (`fix_prompt(Vec::new(), Vec::new())`, `GOLDEN_FIX`), `the_full_reviewer_prompt_matches_golden_text` (`review_prompt(Vec::new())`, `GOLDEN_REVIEW`). With no agent file in the worktree these are today's full prompts.
- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine --test slim_prompts golden` and expect 4 failures that print the actual normalized prompt on the right side of `assert_eq!`.
- [ ] **Step 4: Paste** each printed value into its constant in order (a plain string literal, escapes as the failure shows them). Change no production file in this task. If a prompt differs between two runs, fix the normalization in `norm`, not the prompt.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine --test slim_prompts` and expect 4 PASS on unchanged production code. Keep the goldens unchanged in every later task.

### Task 2: Marker check, fallback note and slim builder prompts

**Files:**
- Modify: `crates/factory-engine/src/specs.rs`, `crates/factory-engine/src/lib.rs`
- Test: `crates/factory-engine/tests/slim_prompts.rs`

**Interfaces:**
- Consumes: Task 1 helpers and goldens; `Engine::ruling(&self, text: &str) -> Result<(), EngineError>`; `self.job_dir`; `BUILDER_AGENT`.
- Produces:
  - `pub const RULES_VERSION: u32 = 1;` in `specs.rs`, re-exported from the crate root.
  - `pub(crate) fn rules_marker(text: &str) -> Option<u32>`: the number of the first line that is exactly `<!-- factory-rules: N -->`.
  - `fn agent_rules_current(&self, cwd: &Path, agent: &str) -> bool`: reads `<cwd>/.claude/agents/<agent>.md` (`agent_files_rel_dir()` from `factory_core`), true when `rules_marker(text) >= Some(RULES_VERSION)`.
  - `fn slim_prompt(&self, cwd: &Path, agent: &str) -> bool`: `agent_rules_current`; when false, writes the ruling `agent file <agent>.md has no current rules marker, prompts keep the full rules` once per job (skip when `progress.md` already contains that text); ruling errors are ignored.
  - `builder_first_spec`, `builder_task_spec` and `builder_resume_spec` call `slim_prompt(cwd, BUILDER_AGENT)` and build the slim text from the Global Constraints list. For the fix round the slim text keeps the KB line, the findings path, the branch, `Follow superpowers:receiving-code-review for the findings.`, the read-all-first, push-back and `NEEDS_CONTEXT` sentences and the check hints, and drops the `git add`/`git commit`/amend sentence, the implementer-rules sentence and `Never push, never merge.`
  - The logic reads the worktree file with `std::fs` and writes the ledger through `Engine::ruling`; the agent run itself still goes through the `AgentRunner` port.

- [ ] **Step 1: Read** `specs.rs` (`builder_first_spec`, `builder_task_spec`, `builder_resume_spec`, `NOT_CONTROLLER`), `build.rs` `write_agent_files` (the `progress.md` contains-check) and the Task 1 helpers.
- [ ] **Step 2: Write the failing tests** in `slim_prompts.rs`. One full example:

```rust
#[test]
fn a_current_agent_file_gives_the_slim_task_prompt() {
    let (fx, prompt) = task_prompt(vec![agent_file("factory-builder.md", Some("<!-- factory-rules: 1 -->"))]);
    assert!(prompt.contains("Do only Task 1: first thing"), "{prompt}");
    assert!(prompt.contains(&fx.job().branch), "{prompt}");
    for gone in ["git commit", "Status: DONE", "implementer rules", "Write the failing test first", "Never push"] {
        assert!(!prompt.contains(gone), "{gone} in {prompt}");
    }
    assert!(prompt.contains("Never sleep or poll"), "{prompt}");
}
```

  The other tests, all with the helpers of Task 1:

| Test | Input | Assertion |
|---|---|---|
| `a_current_agent_file_gives_the_slim_first_run_prompt` | `first_prompt` with a marker 1 builder file | no `Status: DONE`, no `git commit`, contains `Never edit the plan.`, no `Never sleep` |
| `a_missing_marker_keeps_the_full_builder_prompt` | `task_prompt` with `agent_file(.., None)` | `norm(..) == GOLDEN_TASK` |
| `an_older_marker_keeps_the_full_builder_prompt` | marker `<!-- factory-rules: 0 -->` | `norm(..) == GOLDEN_TASK` |
| `a_newer_marker_counts_as_current` | marker `<!-- factory-rules: 99 -->` | no `Status: DONE` |
| `a_marker_inside_other_text_does_not_count` | marker `see <!-- factory-rules: 1 --> here` | `norm(..) == GOLDEN_TASK` |
| `the_slim_task_prompt_is_shorter_than_the_full_one` | both prompts | `slim.len() < full.len()` |
| `the_slim_fix_round_prompt_keeps_findings_hints_and_the_kb_line` | `fix_prompt` with marker files, `check_hints` set and `kb_path` set on the fixture before the call (set them in a variant of the helper) | contains the findings path, `receiving-code-review`, the hints and `Knowledge base:`; no `git commit` |
| `the_fix_round_checks_the_agent_file_when_it_starts` | `fix_prompt(first = no files, fix = marker files)` | no `git commit`; and with `fix = no files` it equals `GOLDEN_FIX` |
| `the_fallback_is_noted_once_per_job_and_agent` | run two tasks with an old file (`task_prompt` twice on one fixture, or two `build_stage` calls) | the ledger contains `agent file factory-builder.md has no current rules marker` exactly once |
| `rules_marker_reads_only_exact_lines` | unit test in `specs.rs`: `"---\n---\n<!-- factory-rules: 3 -->\n"` gives `Some(3)`; `"x <!-- factory-rules: 3 -->"`, `"<!-- factory-rules: x -->"` and `""` give `None` | the values |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine --test slim_prompts` and expect the slim tests to FAIL on `Status: DONE` still being in the prompt, while the golden and fallback-equality tests pass. Run `cargo test -j4 -p factory-engine --lib rules_marker` and expect a compile failure `cannot find function 'rules_marker'`.
- [ ] **Step 4: Implement** the items under Produces. Build each full prompt exactly as today and the slim one from the same pieces; do not reformat the full text. Add `RULES_VERSION` to the `pub use specs::{..}` line in `lib.rs`.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine --test slim_prompts`, `cargo test -j4 -p factory-engine --test build_checks`, `cargo test -j4 -p factory-engine --test permission_pins` and `cargo test -j4 -p factory-engine --lib` and expect every test to PASS, the goldens unchanged.

### Task 3: Slim reviewer prompt

**Files:**
- Modify: `crates/factory-engine/src/specs.rs`
- Test: `crates/factory-engine/tests/slim_prompts.rs`

**Interfaces:**
- Consumes: `slim_prompt(cwd, agent)` (Task 2), `REVIEWER_AGENT`, `review_prompt` and `GOLDEN_REVIEW` (Task 1).
- Produces: `reviewer_spec_with_model` calls `slim_prompt(cwd, REVIEWER_AGENT)`. When true the prompt is the intro line, the inputs line (diff, names, plan, worktree, round), the spec line, the earlier-findings line, the builder-reply line, the scoped re-review block, the context block, the config rules line and the KB line. It drops the paging sentence, the `Check:` list, the vision-document sentence, the read-only sentence, the 60 line and grading sentence, the `Declined to judge` sentence and the `VERDICT` sentence. Logic runs through the same file read and ledger ruling as Task 2.

- [ ] **Step 1: Read** `reviewer_spec_with_model` in `specs.rs` and `crates/factory-engine/tests/review_decide.rs` (the three reviewer prompt tests, which keep passing because they use a worktree or checkout without agent files).
- [ ] **Step 2: Write the failing tests** in `slim_prompts.rs`. One full example:

```rust
#[test]
fn the_slim_reviewer_prompt_drops_the_verdict_and_check_text() {
    let files = vec![agent_file("factory-code-reviewer.md", Some("<!-- factory-rules: 1 -->")), agent_file("factory-builder.md", Some("<!-- factory-rules: 1 -->"))];
    let (_fx, prompt) = review_prompt(files);
    for gone in ["VERDICT:", "60 lines", "Declined to judge", "vision document", "1500", "Check: the diff does"] {
        assert!(!prompt.contains(gone), "{gone} in {prompt}");
    }
    assert!(prompt.contains("Review this round's diff for the job."), "{prompt}");
    assert!(prompt.contains("Round 1."), "{prompt}");
}
```

  The other tests:

| Test | Input | Assertion |
|---|---|---|
| `the_slim_reviewer_prompt_keeps_inputs_and_the_rereview_block` | a round 2 setup as in `reviewer_prompt_names_the_spec_and_the_earlier_findings_when_they_exist`, but with a marker 1 reviewer file inside the worktree cwd | contains `findings of round 1` and `scoped re-review` |
| `a_missing_reviewer_file_keeps_the_full_prompt_and_notes_it` | `review_prompt(Vec::new())` | `norm(..) == GOLDEN_REVIEW` and the ledger contains `agent file factory-code-reviewer.md has no current rules marker` |
| `a_marker_zero_reviewer_file_keeps_the_full_prompt` | marker `<!-- factory-rules: 0 -->` | `norm(..) == GOLDEN_REVIEW` |
| `the_slim_reviewer_prompt_is_shorter_than_the_full_one` | both | `slim.len() < full.len()` |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine --test slim_prompts reviewer` and expect FAIL: the slim prompt still contains `VERDICT:`.
- [ ] **Step 4: Implement** the slim reviewer text beside the full one, as in Task 2.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine --test slim_prompts`, `cargo test -j4 -p factory-engine --test review_decide`, `cargo test -j4 -p factory-engine --test gates` and expect PASS. Then `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings` and `cargo fmt -p factory-engine -- --check`, both clean.

Next plan: docs/superpowers/plans/2026-10-20-cold-start-savings/plan-04-cold-start-savings.md
