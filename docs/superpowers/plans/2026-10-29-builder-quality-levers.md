# Builder Quality Levers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make factory builders write code with fewer missed edge cases, stronger tests, less duplication and less wasted work, and make the reviewer and the checks catch what still slips through.

**Architecture:** Five levers, cheapest first in effect but ordered here so each task stands alone: a quality bar in the builder agent files, efficiency and edge-case items in the reviewer agent files, a `simplify` pass driven by a new `builder_simplify` setting, `opus` as the default large-diff reviewer, and clippy gates for the clean Rocket crates. Agent text lives in the preset files that `factory presets refresh` installs; the two settings live in `Config` and reach every project through the built-in defaults.

**Tech Stack:** Rust (factory-core, factory-engine, factory-cli), Markdown agent files, TOML project files.

**Spec:** No separate spec. The evidence and decisions come from the 2026-10-29 investigation (43 reviews, about 100 findings: correctness and edge cases 16, weak tests 13, error handling 8, duplication 5, complexity 2, performance 2; post-merge fixes were mostly correctness; Rocket has no clippy gate). The decisions are recorded below.

## Global Constraints

- Cargo: always `-j4`, one crate per command (`-p <crate>`), never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- After editing any `.claude/agents/factory-*.md` or preset agent file, run `cargo test -j4 -p factory-cli --test agent_lint`.
- Code comments are short full sentences ending with a period.
- Every new `Config` key must be listed in `crates/factory-core/src/settings_block.rs` `GROUPS`; the drift test fails otherwise.
- Every new `Config` key gets a row in `docs/reference/project-toml-reference.md`.
- Commit each task with the `dev-workflow-skills:1-git-commit` skill, never a freeform `git commit -m`.
- `RULES_VERSION` stays 1. The new text lives only in the agent files, not in the prompts, so the slim/full prompt switch is unaffected.
- Decision: clippy's `perf` group is already on by default, so `-D warnings` covers it. No extra `-D clippy::perf` flag anywhere.
- Decision: `reviewer_model_large` defaults to `"opus"`. A project turns the switch off by setting it to the same name as `reviewer_model`.
- Decision: `builder_simplify` defaults to `true` and applies to Claude builder runs only, never to Codex, fix rounds or crash resumes.
- Decision: a Rocket crate gets a clippy check only when `cargo clippy -j4 -p <crate> --all-targets -- -D warnings` passes today. Crates with a backlog stay ungated; the backlog is a separate job.

## Review Focus

- A Codex project: the `simplify` line must not reach a Codex builder, and a new Codex project must not get `"opus"` as `reviewer_model_large` (it would trip the Claude-model check in `project show`).
- A fix round or crash resume: the `simplify` line must not appear, so a fix stays limited to the findings.
- A carried builder (task 2 and later with `builder_carry_session` on): the `simplify` line must appear in the carry prompt, since that is a new task.
- A project file that sets `reviewer_model_large` explicitly keeps its value; one that leaves it unset now gets `"opus"`.
- A Rocket crate whose clippy backlog is not empty must not get a `-D warnings` check, or every job touching it would park after two fix rounds.

---

### Task 1: Quality bar in the builder agent files

**Files:**
- Modify: `crates/factory-core/presets/{rust,go,ruby,csharp,node}/agents/factory-builder.md` (new section after `## Rules in short`)
- Modify: `.claude/agents/factory-builder.md` (same section, after `## Rules in short`)
- Test: `crates/factory-cli/tests/agent_lint.rs`

**Interfaces:**
- Produces: the heading `## Quality bar` and the bullet labels `Edge cases:`, `Tests:`, `Reuse:`, `No dead code:`, `No needless work:` in every builder file. Task 2's reviewer items mirror these labels.

- [ ] **Step 1: Write the failing lint test** `builder_files_carry_the_quality_bar` in `agent_lint.rs`: for `.claude/agents/factory-builder.md` and each of `PRESET_BUILDERS`, `require_all` on `["## Quality bar", "Edge cases:", "Tests:", "Reuse:", "No dead code:", "No needless work:", "Before `Status: DONE`, read your own diff once"]`.

- [ ] **Step 2: Run it and confirm it fails**

Run: `cargo test -j4 -p factory-cli --test agent_lint builder_files_carry_the_quality_bar`
Expected: FAIL naming the first builder file without `## Quality bar`.

- [ ] **Step 3: Add the section to all six builder files**, exactly this text:

```markdown
## Quality bar

Before `Status: DONE`, read your own diff once and fix what you find:

- Edge cases: empty, missing and invalid input, and an error from every call you added. Each edge case and error path the task makes reachable has a test.
- Tests: each test fails without your change. No assertion that always passes.
- Reuse: search for an existing helper before you write one. Call it instead of copying its body.
- No dead code: no unused functions, no branch that cannot run, no commented-out code.
- No needless work: nothing recomputed or re-read inside a loop, no needless clones or copies, no blocking file or network work on an async path.
```

- [ ] **Step 4: Run the whole lint file and confirm it passes**

Run: `cargo test -j4 -p factory-cli --test agent_lint`
Expected: PASS, every test.

- [ ] **Step 5: Commit** with the `dev-workflow-skills:1-git-commit` skill.

### Task 2: Edge-case, reuse and efficiency items in the reviewer agent files

**Files:**
- Modify: `crates/factory-core/presets/{rust,go,ruby,csharp,node}/agents/factory-code-reviewer.md` (`## Check` and `## Judging`)
- Modify: `.claude/agents/factory-code-reviewer.md` (same two sections)
- Test: `crates/factory-cli/tests/agent_lint.rs`

**Interfaces:**
- Consumes: the Task 1 labels, so builder and reviewer use the same words.

- [ ] **Step 1: Write the failing lint test** `reviewer_files_check_edge_cases_reuse_and_efficiency`: for `.claude/agents/factory-code-reviewer.md` and the five preset reviewer files (node included), `require_all` on `["edge cases and error paths:", "reuse and simplicity:", "efficiency:", "Grade a reuse, simplicity or efficiency finding `[minor]`"]`.

- [ ] **Step 2: Run it and confirm it fails**

Run: `cargo test -j4 -p factory-cli --test agent_lint reviewer_files_check_edge_cases_reuse_and_efficiency`
Expected: FAIL naming the first reviewer file.

- [ ] **Step 3: Edit the six reviewer files.** In `## Check`, add these three items after the `tests:` item:

```markdown
- edge cases and error paths: empty, missing and invalid input, and every new error path, each with a test that would fail without the change
- reuse and simplicity: no copy of logic that already exists in the code base, no dead code, no abstraction the task does not need
- efficiency: no work repeated inside a loop, no needless clones or copies, no blocking file or network work on an async path
```

In `## Judging`, add this bullet after the grading bullet:

```markdown
- Grade a reuse, simplicity or efficiency finding `[minor]`, unless the waste is plain from the code and sits on a path that runs per request, per item or per file: then it is `[blocker]`. A missing test for an edge case or error path the code makes reachable is `[blocker]`.
```

- [ ] **Step 4: Run the whole lint file and confirm it passes**

Run: `cargo test -j4 -p factory-cli --test agent_lint`
Expected: PASS.

- [ ] **Step 5: Commit** with the `dev-workflow-skills:1-git-commit` skill.

### Task 3: `builder_simplify` setting and the simplify line

**Files:**
- Modify: `crates/factory-core/src/config.rs` (field, default, test)
- Modify: `crates/factory-core/src/settings_block.rs` (`GROUPS`, "Builder and reviewer prompts." group, after `builder_excerpt`)
- Modify: `crates/factory-engine/src/specs.rs` (`SIMPLIFY_LINE`, `simplify_line()`, the three new-task builder prompts)
- Modify: `docs/reference/project-toml-reference.md` (Builder prompt table), `README.md` (one sentence near `builder_carry_session`)
- Test: `crates/factory-core/src/config.rs` tests, `crates/factory-engine/tests/build_checks.rs`, `crates/factory-engine/tests/task_carry.rs`

**Interfaces:**
- Produces: `Config::builder_simplify: bool` (default `true`, doc comment "Ask each new-task builder to run the `simplify` skill on its changes. Claude runs only."); `pub(crate) const SIMPLIFY_LINE: &str` in `specs.rs`; `fn simplify_line(&self) -> &'static str` returning `SIMPLIFY_LINE` plus `\n` when `builder_simplify` is on and `agent_cli()` is `"claude"`, else `""`.
- The line text, exactly: `After your checks pass, invoke the \`simplify\` skill on your uncommitted changes. Run it inline without helper agents, apply only fixes inside the files of this task, then run the checks again.`

- [ ] **Step 1: Write the failing core test** `builder_simplify_is_on_by_default` in `config.rs`: `Config::default().builder_simplify` is true, and `Config::parse("builder_simplify = false")` gives false.

- [ ] **Step 2: Write the failing engine tests** in `build_checks.rs`, using `committing_fixture()` and `two_task_fixture()` with `scripted_auto(...)` as the existing prompt tests do:
  - `the_simplify_line_reaches_new_task_builders`: default config, two tasks built; every `runner.specs_for("builder")` prompt contains `SIMPLIFY_LINE`'s first sentence `invoke the \`simplify\` skill`.
  - `builder_simplify_off_drops_the_line`: `fx.config.builder_simplify = false`; no builder prompt contains `simplify`.
  - `a_fix_round_prompt_has_no_simplify_line`: build, `start_fix_round(&fx)`, build again; the last builder spec's prompt does not contain `simplify`.
  - `codex_builders_get_no_simplify_line`: `fx.config.agent_cli = Some("codex".into())`; no builder prompt contains `simplify`.
  - In `task_carry.rs`, `a_carried_task_prompt_has_the_simplify_line`: carry on, two tasks; `runner.inner.specs_for("builder")[1]` has `resume` set and its prompt contains `invoke the \`simplify\` skill`.

- [ ] **Step 3: Run them and confirm they fail**

Run: `cargo test -j4 -p factory-core --lib builder_simplify` then `cargo test -j4 -p factory-engine --test build_checks simplify` and `cargo test -j4 -p factory-engine --test task_carry simplify`
Expected: compile error for the missing field, then FAIL on the prompt assertions.

- [ ] **Step 4: Implement.** Add the field and default (`true`) to `Config`; add `("builder_simplify", None)` to `GROUPS` after `builder_excerpt`; add `SIMPLIFY_LINE` and `simplify_line()` in `specs.rs` next to `rules_line()`; insert `{simplify}` right after `{rules}` in the slim and full variants of `builder_first_spec` and `builder_task_spec`, and append it to the `builder_task_carry_spec` prompt. Do not touch `builder_resume_spec` or `builder_task_resume_spec`.

- [ ] **Step 5: Run the tests and confirm they pass**

Run: `cargo test -j4 -p factory-core` and `cargo test -j4 -p factory-engine`
Expected: PASS, including `every_config_key_is_listed_once`.

- [ ] **Step 6: Document.** Add a `builder_simplify` row (bool, `true`) to the Builder prompt table in `project-toml-reference.md` with the effect "Each new-task builder runs the built-in `simplify` skill on its changes after its checks pass, inline and limited to the task's files. Claude runs only; fix rounds and crash resumes never get it. `false` drops the line." Add one matching sentence to `README.md` after the `builder_carry_session` paragraph.

- [ ] **Step 7: Commit** with the `dev-workflow-skills:1-git-commit` skill.

### Task 4: `opus` as the default large-diff reviewer

**Files:**
- Modify: `crates/factory-core/src/config.rs` (default, the test `the_large_reviewer_model_is_off_by_default`)
- Modify: `crates/factory-core/src/project.rs` (`add` seeds `reviewer_model_large` for Codex)
- Modify: `crates/factory-core/src/settings_block.rs` (`reviewer_model_large` entry becomes `None`, so the block shows the real default)
- Modify: `docs/reference/project-toml-reference.md` (Models table row and the "turn it off" note)
- Test: `crates/factory-core/src/config.rs`, `crates/factory-core/src/project.rs`

**Interfaces:**
- Produces: `Config::default().reviewer_model_large == Some("opus".to_string())`.

- [ ] **Step 1: Rewrite the core test** as `the_large_reviewer_model_defaults_to_opus`: `Config::default().reviewer_model_large` and `Config::parse("")...reviewer_model_large` are `Some("opus")`; `Config::parse("reviewer_model_large = \"sonnet\"")` gives `Some("sonnet")`; `reviewer_large_over_lines` stays 800.

- [ ] **Step 2: Update the Codex add test** `project_add_with_agent_cli_codex_seeds_models_and_no_cache_ttl` in `project.rs`: it now expects `reviewer_model_large == CODEX_DEFAULT_MODEL` among the seeded keys (it asserts the key is absent today; flip that assertion).

- [ ] **Step 3: Run them and confirm they fail**

Run: `cargo test -j4 -p factory-core --lib large_reviewer` and `cargo test -j4 -p factory-core --lib project_add_with_agent_cli_codex`
Expected: FAIL on the `None` default and the missing seeded key.

- [ ] **Step 4: Implement.** Set the default to `Some("opus".to_string())`; add `"reviewer_model_large"` to the Codex seed loop in `project::add`; change the `settings_block.rs` entry to `("reviewer_model_large", None)`.

- [ ] **Step 5: Run the crate tests and confirm they pass**

Run: `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-engine --test review_decide`, `cargo test -j4 -p factory-cli --test project_cmd`
Expected: PASS. If a `project_cmd` test pins `reviewer_model_large = unset (default)`, change it to `reviewer_model_large = "opus" (default)`.

- [ ] **Step 6: Document.** In the Models table, set the default to `"opus"` and say: "To turn the switch off, set it to the same name as `reviewer_model`." Update the matching sentence in `README.md` if it says the switch is off by default.

- [ ] **Step 7: Commit** with the `dev-workflow-skills:1-git-commit` skill.

### Task 5: Roll out to the installed presets and the Rocket project

This task changes files in the factory home, not in this repo. Nothing here is committed.

**Files:**
- Modify: `~/.factory/presets/*/agents/factory-{builder,code-reviewer}.md` (through `factory presets refresh`)
- Modify: `~/.factory/projects/rocket/agents/factory-builder.md`, `~/.factory/projects/rocket/agents/factory-code-reviewer.md` (Rocket's own copies, which override the preset)
- Modify: `~/.factory/projects/rocket/project.toml` (`[[settings.checks]]`)

- [ ] **Step 1: Rebuild** with `cargo build -j4 -p factory-cli` so `factory` on PATH (a symlink to `target/debug/factory`) has Tasks 1-4.

- [ ] **Step 2: Refresh the installed presets.** Run `factory presets refresh` without `--force` and read its report. Expected: the ten builder and reviewer preset files updated. If it reports a locally edited file it would not overwrite, stop and ask the user.

- [ ] **Step 3: Copy the Task 1 and Task 2 text into Rocket's own agent files.** Add the `## Quality bar` section after the `## Work` section of `~/.factory/projects/rocket/agents/factory-builder.md`, and the three `## Check` items plus the grading line (as a new last bullet of `## Check`, since this older file has no `## Judging`) to `~/.factory/projects/rocket/agents/factory-code-reviewer.md`. In the builder's `## Work` list, change `Do not run clippy and do not run a release build.` to `Do not run a release build.`, because Step 5 asks the builder to confirm clippy fixes. Change nothing else in those files.

- [ ] **Step 4: Measure the Rocket clippy backlog per crate.** In `/home/numericlabs/data/rocket/rocket`, for each folder in `crates/`, run `cargo clippy -j4 -p <crate> --all-targets -- -D warnings` and record pass or fail. A path dependency with warnings makes its dependents fail too, so record the result per command, not per file. The 2026-10-29 measurement (warning lines per command) was: clean (0) for `rocket-acp`, `rocket-audit`, `rocket-collection`, `rocket-environment`, `rocket-flow`, `rocket-git`, `rocket-grpc`, `rocket-history`, `rocket-http`, `rocket-scripting`, `rocket-shared`, `rocket-workspace`; backlog in `rocket-app` (10), `rocket-import` (5), `rocket-infra` (4), `rocket-export` (1). Re-run it here, because the Rocket repo may have moved on.

- [ ] **Step 5: Gate the clean crates.** For each crate that passed, add one `[[settings.checks]]` table to `~/.factory/projects/rocket/project.toml`, before the existing `crates/*/**` table: `paths = ["crates/<crate>/**"]`, `commands = ["cargo check -j4 -p <crate>", "cargo test -j4 -p <crate>", "cargo clippy -j4 -p <crate> --all-targets -- -D warnings"]`. Leave the existing `crates/*/**` table in place for the crates that failed. Then add the builder rule `"The checks run cargo clippy -j4 -p <crate> --all-targets -- -D warnings for the crates listed in the checks. When a check output shows clippy errors, fix them and run that command once to confirm."` to Rocket's `builder_rules`.

- [ ] **Step 6: Verify every project still resolves.** Run `factory project show <name>` for `rocket`, `rocket-factory`, `rocketvault` and `smoke`. Expected: `problems: none` for each, `builder_simplify = true (default)` and `reviewer_model_large = "opus" (default)` in the settings list.

- [ ] **Step 7: File the backlog.** After the user confirms, open one GitHub issue in the Rocket repo listing the crates that failed Step 4, titled "Clear the clippy backlog so the factory can gate every crate", so each can get its clippy check once clean.
