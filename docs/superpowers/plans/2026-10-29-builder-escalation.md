# Builder Escalation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When a job's first fix round does not fix it, run the next fix round on a stronger builder model in a fresh session, so struggling jobs get more capability and easy jobs stay cheap.

**Architecture:** The factory adapts on what the job shows, not on a prediction made before the build. `fix_or_park` already decides every fix round, so it also decides escalation and stores the stronger model on the job. The builder model is resolved in one place: the job's `--model` first, then the escalated model, then `builder_model`. An escalated round starts a fresh builder with a fix prompt, because a resumed session cannot reuse the cheaper model's prompt cache and would pay a full cache write on the stronger model. Task 4 applies the same fresh fix prompt to an ordinary fix round whose session cache has expired; drop it if you want escalation only.

**Tech Stack:** Rust (factory-core, factory-engine), TOML config, Markdown docs.

**Spec:** No separate spec. The evidence and decisions come from the 2026-10-29 adaptive pipeline research (memory note `factory-adaptive-pipeline-research`):
- Pre-build signals do not predict review problems in 49 real jobs.
- SWE-Router (2026) found that routing on a job's own progress beats routing on the prompt.
- Rocket job 041 spent 950k on five resumed rework builders. Each paid a 120-165k cache rebuild after idle gaps.
The decisions are below.

## Global Constraints

- Cargo: always `-j4`, one crate per command (`-p <crate>`), never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Code comments are short full sentences ending with a period.
- Every new `Config` key is listed in `crates/factory-core/src/settings_block.rs` `GROUPS` (the drift test fails otherwise) and gets a row in `docs/reference/project-toml-reference.md`.
- Board JSON must stay compatible with the dashboard: new `Job` fields are `Option` with `#[serde(default, skip_serializing_if = "Option::is_none")]`.
- Commit each task with the `dev-workflow-skills:1-git-commit` skill.
- Decision: `escalation_model` defaults to `"claude-opus-5-5"`. A Codex config without the key gets `None` (no escalation), the same rule `Config::parse` applies to `reviewer_model_large`. A blank or Fable value is refused, like every other model key.
- Decision: `escalate_after_fix_rounds` defaults to `1`. A fix round escalates when at least this many fix rounds were already spent. With the default `fix_rounds = 2` that is the second, last fix round. `0` escalates the first fix round. A value at or above `fix_rounds` never escalates.
- Decision: escalation changes the builder only. The reviewer keeps its own rule (`reviewer_model_large` for risky or large diffs). The committer, merge message and KB refresher stay on their models.
- Decision: a job's own `--model` always wins. A job with `--model` set never escalates.
- Decision: once escalated, the job stays escalated for its remaining fix rounds. The next job of a series starts unescalated.
- Decision: the escalated round's builder is fresh, not resumed. It gets the findings file, the base and head commits and the plan, and reads the diff with `git diff`.

## Review Focus

- A job created with `factory new --model sonnet-like-X`: an escalation must not replace X, and the ledger must not claim an escalation.
- A Codex project with no `escalation_model` key: no escalation, and no Claude model name ever reaches `codex exec`.
- A rework note from the user during an escalated job: the note wins over `changes-rN.md`, exactly as in a resumed round, and the escalated model stays.
- An escalated round whose builder changes nothing after a checks failure: the existing `nothing_changed_after_checks` handling still applies (fix source "checks").
- A series: job N escalates, and job N+1 must start on `builder_model` with `escalated_model` unset.

---

### Task 1: Config keys `escalation_model` and `escalate_after_fix_rounds`

**Files:**
- Modify: `crates/factory-core/src/config.rs`: fields, defaults, the Codex rule in `parse`, the Fable list in `parse`, tests.
- Modify: `crates/factory-core/src/settings_block.rs`: `GROUPS`, in the "Models." group after `reviewer_large_over_lines`.
- Modify: `docs/reference/project-toml-reference.md`: Models table rows. Add one paragraph to `README.md` after the `reviewer_model_large` bullets.
- Test: `crates/factory-core/src/config.rs` tests.

**Interfaces:**
- Produces: `Config::escalation_model: Option<String>` (doc: "A stronger builder model for fix rounds after `escalate_after_fix_rounds` failed ones. `None` turns escalation off."). `Config::escalate_after_fix_rounds: u32` (doc: "Fix rounds spent before the next one escalates the builder.").

- [ ] **Step 1: Write the failing tests** in `config.rs`:
  - `escalation_defaults_to_opus_after_one_fix_round`: `Config::default().escalation_model == Some("claude-opus-5-5")`, `escalate_after_fix_rounds == 1`, and `Config::parse("escalation_model = \"claude-sonnet-5-5\"\nescalate_after_fix_rounds = 0")` gives those values.
  - `a_codex_config_without_the_key_does_not_escalate`: `Config::parse("agent_cli = \"codex\"").escalation_model == None`. `agent_cli = "codex"` with `escalation_model = "gpt-big"` keeps `Some("gpt-big")`.
  - Extend `a_fable_model_is_refused_for_every_role` with the key `escalation_model`. Add `Config::parse("escalation_model = \" \"")` is an error with "escalation_model must not be empty when it is set".

- [ ] **Step 2: Run them.** `cargo test -j4 -p factory-core --lib escalat` and `cargo test -j4 -p factory-core --lib a_fable_model`. Expected: compile errors for the missing fields.

- [ ] **Step 3: Implement.**
  - Add the fields and defaults.
  - In `parse`, clear `escalation_model` for a Codex config whose text has no `escalation_model` key. Use the same `toml::Table` check that already handles `reviewer_model_large`, and extend it rather than parsing a second time.
  - Add `("escalation_model", cfg.escalation_model.as_deref())` to the Fable `named` list.
  - Refuse a blank `escalation_model` like `reviewer_model_large`.
  - Add `("escalation_model", None)` and `("escalate_after_fix_rounds", None)` to `GROUPS`.

- [ ] **Step 4: Run** `cargo test -j4 -p factory-core` and `cargo test -j4 -p factory-cli --test project_cmd`. Expected: PASS, including `every_config_key_is_listed_once`. If `show_prints_every_setting_with_its_value_and_source` pins the full settings list, add the two new lines.

- [ ] **Step 5: Document.**
  - `escalation_model`: string, `"claude-opus-5-5"`. Effect: "The builder model for a fix round once `escalate_after_fix_rounds` fix rounds were spent without fixing the job. The escalated round starts a fresh session. A job's own `--model` wins. A Codex project without the key does not escalate. Not blank."
  - `escalate_after_fix_rounds`: integer, `1`. Effect: "Fix rounds spent before the next one escalates. `0` escalates the first fix round. At or above `fix_rounds` it never escalates."

- [ ] **Step 6: Commit** with the `dev-workflow-skills:1-git-commit` skill.

### Task 2: Decide escalation when a fix round starts

**Files:**
- Modify: `crates/factory-core/src/board.rs`: `Job::escalated_model: Option<String>`, an optional serde field.
- Modify: `crates/factory-engine/src/checks_stage.rs`: `fix_or_park` (`checks_stage.rs:164`), the single place both check failures and review findings start a fix round (`review.rs:170,184`, `checks_stage.rs:246`).
- Modify: `crates/factory-engine/src/ports.rs`: `JobView::escalated_model`, read in `job()`.
- Create: `crates/factory-engine/src/escalation.rs`, with a pure decision function (follow the style of `carry.rs`).
- Test: `crates/factory-engine/src/escalation.rs` unit tests. `crates/factory-engine/tests/escalation.rs` (new).

**Interfaces:**
- Consumes: Task 1 `Config::escalation_model`, `Config::escalate_after_fix_rounds`. `Engine::job_model()` (ports.rs, already on main).
- Produces: `pub(crate) fn escalation_for(fix_rounds_spent: u32, after: u32, escalation_model: Option<&str>, job_model: Option<&str>, already: Option<&str>) -> Option<String>`. It returns the model to store on the job, or `None`. `Job::escalated_model`. The ledger ruling text `escalated the builder to <model> (fix round <n>, <reason>)`.

- [ ] **Step 1: Write the failing unit tests** in `escalation.rs`:
  - `no_escalation_before_the_threshold`: `escalation_for(0, 1, Some("opus"), None, None) == None`.
  - `escalates_at_the_threshold`: `escalation_for(1, 1, Some("opus"), None, None) == Some("opus")`.
  - `zero_escalates_the_first_fix_round`: `escalation_for(0, 0, Some("opus"), None, None) == Some("opus")`.
  - `a_job_model_never_escalates`: `escalation_for(5, 1, Some("opus"), Some("x"), None) == None`.
  - `off_without_a_model`: `escalation_for(5, 1, None, None, None) == None`.
  - `an_escalated_job_stays_escalated`: `escalation_for(5, 1, Some("opus"), None, Some("opus")) == Some("opus")`.

- [ ] **Step 2: Write the failing engine tests** in `tests/escalation.rs`, using `committing_fixture()`. Use a check rule whose command is `false` to fail checks, as `gates.rs::only_src_checks` does.
  - `the_second_fix_round_is_escalated_and_logged`: build, fail checks (fix round 2, `job().escalated_model == None`), build, fail checks again (fix round 3). `job().escalated_model == Some("claude-opus-5-5")`, and the ledger contains `escalated the builder to claude-opus-5-5 (fix round 3, checks failed)`.
  - `a_job_with_its_own_model_is_not_escalated`: same flow with the board job's `model = Some("claude-sonnet-5-5")`. `escalated_model` stays `None` and the ledger has no `escalated` line.
  - `review_findings_escalate_too`: build, checks pass, reviewer `VERDICT: CHANGES` twice. The second fix round sets `escalated_model`.

- [ ] **Step 3: Run them.** `cargo test -j4 -p factory-engine --lib escalation` and `cargo test -j4 -p factory-engine --test escalation`. Expected: compile errors, then FAIL.

- [ ] **Step 4: Implement.**
  - Add the field and `escalation_for`.
  - In `fix_or_park`, after the park check and before `self.update`, compute `escalation_for(used, ...)`.
  - When it returns a model the job did not already have, set `j.escalated_model` inside the same `update` closure. Write the ruling with the fix round number `next` and `reason`.
  - Make sure series job creation (`runloop.rs` `add_job`) does not copy `escalated_model`. `Job::new` defaults it to `None`, so this only needs the series test from Review Focus.

- [ ] **Step 5: Add the series test** from Review Focus to `tests/escalation.rs`. `escalation_does_not_carry_into_the_next_series_job`: use `SeriesFixture` from `tests/common`, escalate job 1 by hand on the board, advance the series. The next job has `escalated_model == None`.

- [ ] **Step 6: Run** `cargo test -j4 -p factory-engine` and `cargo test -j4 -p factory-core`. Expected: PASS.

- [ ] **Step 7: Commit** with the `dev-workflow-skills:1-git-commit` skill.

### Task 3: Run the escalated round on the stronger model in a fresh session

**Files:**
- Modify: `crates/factory-engine/src/ports.rs`: `builder_model()` and `run_agent`.
- Modify: `crates/factory-engine/src/specs.rs`: new `builder_fix_fresh_spec`.
- Modify: `crates/factory-engine/src/build.rs`: the spec choice at `build.rs:65-82` and the `new_work` flag at `build.rs:51`.
- Test: `crates/factory-engine/tests/escalation.rs`, and `crates/factory-engine/tests/slim_prompts.rs` (golden text for the new prompt).

**Interfaces:**
- Consumes: Task 2 `JobView::escalated_model`.
- Produces:
  - `Engine::builder_model()` resolves `job_model()`, then the job's `escalated_model`, then `config.builder_model`.
  - `run_agent` sets `spec.model` to the escalated model for role `"builder"` only.
  - `pub(crate) fn builder_fix_fresh_spec(&self, job: &JobView, cwd: &Path, findings: &Path) -> AgentSpec`. Same tools, permissions, agent file, `max_turns` and `with_limits` as `builder_resume_spec`, with no `resume`.
  - Its prompt is fixed copy:
    > `You are the builder in a software factory, picking up a fix round in a fresh session. You are on branch \`{branch}\`; the job's work so far is the diff from {base7} to {head7} (run \`git diff {base7}..{head7}\` to see it). The plan is at {plan}. Fix only the problems listed in {findings}. Follow \`superpowers:receiving-code-review\` for the findings. Run the relevant checks{hints}. Do not run \`git add\` or \`git commit\`; the driver commits for you. Reply with Status: DONE, Status: DONE_WITH_CONCERNS, Status: BLOCKED or Status: NEEDS_CONTEXT as the first line.`
  - `{hints}` is the same check-hint text `builder_resume_spec` builds. Move it into a shared private fn rather than copying it.
  - `Job::builder_session_model: Option<String>` (optional serde field, and on `JobView`) records the model the stored builder session ran on. It is written in the same `update` that writes `builder_id`.
  - Define a fresh session as needed when `job.escalated_model` is `Some(m)` and `builder_session_model != Some(m)`.

- [ ] **Step 1: Write the failing tests** in `tests/escalation.rs`:
  - `the_escalated_round_runs_a_fresh_builder_on_the_stronger_model`: reach fix round 3 as in Task 2. The last builder spec has `model == Some("claude-opus-5-5")`, `resume == None`, and a prompt containing `picking up a fix round in a fresh session` and `changes-r3.md`. Its env has `CLAUDE_CODE_SUBAGENT_MODEL=claude-opus-5-5`.
  - `the_reviewer_and_committer_are_not_escalated`: in the same flow, the last reviewer spec model equals `config.reviewer_model`, and the committer spec model equals `config.commit_model`.
  - `a_rework_note_wins_in_an_escalated_round`: write `rework-note.md` before the escalated build. The fresh prompt names `rework-note.md`, not `changes-r3.md`.
  - `the_first_fix_round_still_resumes`: fix round 2 builder spec has `resume == Some(<session>)` and the configured model.
  - In `slim_prompts.rs`, add `GOLDEN_FIX_FRESH` and `the_fresh_fix_prompt_matches_golden_text`, using the copy above with `<TMP>` and `<SHA>` placeholders as the existing goldens do.

- [ ] **Step 2: Run them.** `cargo test -j4 -p factory-engine --test escalation` and `--test slim_prompts`. Expected: FAIL.

- [ ] **Step 3: Implement.**
  - In `build.rs`, the spec choice becomes three arms, in this order:
    - A fix round (worktree exists, `job.round > 1`) where a fresh session is needed (see Interfaces): `builder_fix_fresh_spec`.
    - A stored session: `builder_resume_spec` (unchanged).
    - Otherwise: `builder_first_spec`.
  - `new_work` must be `false` for the fresh fix arm, so the commit kind is `Fix`, the reply is saved, and the nothing-changed-after-checks path still applies.
  - Store the escalated run's session id and `builder_session_model` as usual. A later escalated round then resumes the strong session, which is the same model and so cache-safe.
  - Add the test `a_later_escalated_round_resumes_the_strong_session` for this. Set `fix_rounds = 3` and fail checks three times. The fourth builder spec has `resume` set and `model == Some("claude-opus-5-5")`.

- [ ] **Step 4: Run** `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli` and `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`. Expected: PASS.

- [ ] **Step 5: Commit** with the `dev-workflow-skills:1-git-commit` skill.

### Task 4 (optional): A fresh fix builder when the resumed session's cache has expired

Keep or drop this task at plan review. It reuses Task 3's fresh fix spec for ordinary fix rounds and rework notes. Job 041's no-op resumes each paid 120-165k to rebuild an expired cache.

**Files:**
- Modify: `crates/factory-engine/src/build.rs` (the same spec choice), `crates/factory-engine/src/carry.rs` (reuse the idle limit), `crates/factory-core/src/config.rs` (`fix_fresh_over_ctx: u32`, default `60_000`), `settings_block.rs`, the reference doc.
- Test: `crates/factory-engine/tests/escalation.rs` or a new `tests/fix_fresh.rs`.

**Interfaces:**
- Consumes: the idle limit in `carry.rs` (240s, or 3300s with `cache_ttl = "1h"`), and the last builder's `end_ctx` and end time already recorded for carry (`BuilderLast`).
- Produces: a fix round whose last builder ended longer ago than the idle limit, with `end_ctx` above `fix_fresh_over_ctx`, runs `builder_fix_fresh_spec` instead of resuming. The ledger ruling is `fresh fix builder (idle {secs}s over {limit}s, context {ctx} over {max})`.

- [ ] **Step 1: Write the failing tests:**
  - `an_idle_large_session_gets_a_fresh_fix_builder`: last builder idle past the limit with `end_ctx` 150000. The fix spec has `resume == None`, and the ledger shows the ruling.
  - `a_warm_session_still_resumes`: idle under the limit means `resume` is set.
  - `a_small_idle_session_still_resumes`: `end_ctx` 30000 means `resume` is set.
  - Use the `Tagged` runner from `tests/common` to control `end_ctx`, as `task_carry.rs` does.
- [ ] **Step 2: Run them.** Expected: FAIL.
- [ ] **Step 3: Implement** the condition next to the Task 3 escalation arm. Reuse the carry idle-limit function; do not copy it.
- [ ] **Step 4: Run** `cargo test -j4 -p factory-engine` and `cargo test -j4 -p factory-core`. Expected: PASS.
- [ ] **Step 5: Document** the key and the rule in the reference doc. Commit with the `dev-workflow-skills:1-git-commit` skill.

### Task 5: Measure

This task changes the factory home, not the repo. Nothing is committed.

- [ ] **Step 1:** Rebuild with `cargo build -j4 -p factory-cli`, then run `factory project show` for all 4 projects. Expected: `escalation_model = "claude-opus-5-5" (default)` and `escalate_after_fix_rounds = 1 (default)`, and `problems: none`.
- [ ] **Step 2:** Run one smoke job whose plan needs a second fix round, for example a check that fails on the first fix. Confirm in its ledger and `usage.jsonl` that the third builder run is on `claude-opus-5-5` with `resumed=false`, and that the reviewer and committer stayed on Sonnet.
- [ ] **Step 3:** Add the escalation counts to the open Rocket evaluation (memory note `factory-quality-levers-eval`): escalated jobs, extra weighted tokens, and whether the escalated round fixed the job.
