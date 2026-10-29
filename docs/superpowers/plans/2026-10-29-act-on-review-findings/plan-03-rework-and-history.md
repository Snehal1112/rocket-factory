# Act On Review Findings, Plan 03: Rework And Review History

> **Executor:** run with superpowers:subagent-driven-development: one fresh implementer per task, then a reviewer per task.

**Goal:** A rework is a real next round that gives the builder the reviewer's open findings with the human note, the re-review rules on the latest earlier review, and no review file is ever overwritten.

**Architecture:** The CLI asks the engine two things: whether the job is parked mid-task (keep today's `rework-note.md` path) and what the rework findings are (note first, then the last review's text or the failing checks). A next-round rework writes `changes-r{N}.md` and bumps `round` and `rework_base`, so the existing fix-round machinery runs it. The reviewer prompt names `last_review.file` instead of guessing `review-code-r{round-1}.md`, and re-reviews tag fixed findings `[fixed]`. A sync review writes its own file.

**Tech Stack:** Rust.

**Spec:** `docs/superpowers/specs/2026-10-29-act-on-review-findings-design.md` (sections 2a, 4, 5)

**Depends on:** plan 01 (`last_review`, `rework_base`, `open_blockers`), plan 02 (`approve --waive` for the sync case).

## Global Constraints

- Cargo per crate with `-j4`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- No file of an earlier round is overwritten: `review-code-r*.md`, `builder-r*.md`, `changes-r*.md`.
- A rework grants a full new set of fix rounds (`rework_base = new_round - 1`).
- A job parked mid-task keeps today's path: `rework-note.md`, no change to `round` or `rework_base`.
- Run `cargo test -j4 -p factory-cli --test agent_lint` after editing any agent file.

## Review Focus

1. A rework of a job parked in its first round (board `round = 0`) moves to round 2 and keeps `review-code-r1.md` and `builder-r1.md` (`rework_of_a_round_one_job_keeps_round_one_files`, Task 1).
2. A rework after a `NothingChanged` park that followed a review fix round: the builder gets the review findings even though `reviews.code` was reset (`rework_after_nothing_changed_carries_the_review`, Task 1).
3. A rework after `ChecksFailed`: the findings are the failing checks, not an older review (`rework_after_failing_checks_carries_the_checks`, Task 1).
4. A round whose predecessor ran no review (checks-only fix round) re-reviews against the latest review that exists instead of falling back to a full review (`rereview_uses_the_latest_review_file`, Task 2).
5. Two syncs in one round write `-sync1` and `-sync2` and leave `review-code-rN.md` alone (`two_sync_reviews_get_their_own_files`, Task 3).

## File Structure

- Modify `crates/factory-engine/src/build.rs`: extract the mid-task test; treat `fix_source == "rework"` like `review` on a no-change round.
- Create `crates/factory-engine/src/rework.rs`: the mid-task query and the findings text.
- Modify `crates/factory-engine/src/{lib.rs,compat.rs}`: `Driver` methods for the CLI.
- Modify `crates/factory-cli/src/{cli.rs,commands.rs}`: the new rework.
- Modify `crates/factory-engine/src/specs.rs`: the re-review names `last_review.file` and the `[fixed]` rule.
- Modify `.claude/agents/factory-code-reviewer.md` and `crates/factory-core/presets/{rust,ruby,node,csharp,go}/agents/factory-code-reviewer.md`.
- Modify `crates/factory-engine/src/{review.rs,sync.rs}`: sync review file names.
- Docs: `docs/reference/software-factory.md`, `docs/guides/factory-user-guide.md`, `docs/reference/factory-admin-manual.md`.

---

### Task 1: Rework as the next round

**Files:**
- Create: `crates/factory-engine/src/rework.rs`
- Modify: `crates/factory-engine/src/build.rs`, `crates/factory-engine/src/lib.rs`, `crates/factory-engine/src/compat.rs`
- Modify: `crates/factory-cli/src/cli.rs` (the Rework doc line), `crates/factory-cli/src/commands.rs` (`rework`)
- Test: `crates/factory-cli/tests/new_to_merged.rs`, `crates/factory-cli/tests/operator_events.rs`, and the engine tests that set `round = 0` for a rework (`build_checks.rs`, `escalation.rs`, `fix_fresh.rs`, `task_carry.rs`)

**Interfaces:**
- Consumes: `JobView.last_review`, `Job.rework_base` (plan 01); `parse_tasks` (tasks.rs).
- Produces:
  - In `build.rs`: `pub(crate) fn is_mid_task(job: &JobView, tasks: &[PlanTask]) -> bool`, the test `build_stage` uses today (`!legacy && task_index < tasks.len()`, `legacy = session_id.is_some() && task_index == 0`); `build_stage` calls it.
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum ReworkKind { MidTask, NextRound }`.
  - `impl Driver { pub fn rework_kind(&self, id: &str) -> Result<ReworkKind, EngineError> }` (through `with_engine`, reading the plan file).
  - `impl Driver { pub fn rework_findings(&self, id: &str, note: &str) -> Result<String, EngineError> }`: `"{note}\n\nThe note above is from the user and wins where it conflicts with the findings below.\n\n{findings}"`, where findings is the text of `last_review.file` when its verdict is `changes` or `missing`, else the FAIL sections of `checks.md` when `park_kind == "checks-failed"`, else empty (and then the text is the note alone).
  - CLI `rework` for `NextRound`: `new_round = max(round, 1) + 1`; write `changes-r{new_round}.md` with `rework_findings`; ledger `rework note: {note}`; in one board update: stage `build`, `round = new_round`, `rework_base = new_round - 1`, `fix_source = Some("rework")`, `reset_reviews()`, `cancelled = false`. Result line `job {id} back to build, round {new_round}`.
  - CLI `rework` for `MidTask`: write `rework-note.md` as today, leave `round` and `rework_base`, set the stage and `cancelled` as today. Result line `job {id} back to build, same task`.
  - `build.rs`: a no-change round with `fix_source == "rework"` parks `NothingChanged` like `review` does (with the Review part from plan 01).

- [ ] **Step 1: Write the failing tests**

In `new_to_merged.rs` (`Scenario` with `review(CHANGES)` until `ChangesRemain`, then rework):
- `rework_of_a_round_one_job_keeps_round_one_files`: a job parked in round 1 is reworked; the board says `round` 2, `rework_base` 1; after the next run `review-code-r1.md`, `builder-r1.md`, `review-code-r2.md` all exist and `review-code-r1.md` is unchanged.
- `rework_sends_the_note_and_the_open_findings`: `changes-r{N}.md` starts with the note, then the sentence about the note winning, then the last review's text.
- `rework_after_nothing_changed_carries_the_review`: a builder that changes nothing on the fix round parks `NothingChanged`; the rework's `changes-r` file holds the review text.
- `rework_after_failing_checks_carries_the_checks`: a job parked `ChecksFailed` gets the failing check output.
- `rework_grants_a_full_set_of_fix_rounds`: after the rework, a CHANGES review still starts a fix round.
- `rework_mid_task_keeps_the_note_path`: a job parked inside task 1 of a 2-task plan writes `rework-note.md` and keeps `round`.

Update `rework_sends_a_parked_job_back_to_build` for the new result line. In `operator_events.rs`, `rework_logs_the_note` keeps passing. Engine tests that set `j.round = 0` to simulate a rework set `round`, `rework_base`, `fix_source = "rework"` and a `changes-r` file instead, unless they test the legacy `rework-note.md` path.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -j4 -p factory-cli --test new_to_merged rework`
Expected: FAIL.

- [ ] **Step 3: Implement `rework.rs`, the `Driver` methods, the CLI change and the build tweak**

- [ ] **Step 4: Run the tests**

Run: `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`
Expected: PASS.

---

### Task 2: Re-review against the latest review, and `[fixed]` tags

**Files:**
- Modify: `crates/factory-engine/src/specs.rs` (`reviewer_spec_with_model`, the re-review block)
- Modify: `.claude/agents/factory-code-reviewer.md`, `crates/factory-core/presets/{rust,ruby,node,csharp,go}/agents/factory-code-reviewer.md`
- Modify: `crates/factory-cli/tests/agent_lint.rs`
- Test: `crates/factory-engine/tests/review_decide.rs`, `crates/factory-engine/tests/slim_prompts.rs`

**Interfaces:**
- Consumes: `JobView.last_review` (plan 01).
- Produces:
  - The earlier-findings file is `last_review.file` when `round > 1` and `last_review.round < round`; the prompt line names that file and its round (`The findings of round {last_review.round} are at {path}. Check that each was fixed.`). The scoped re-review runs whenever that file exists.
  - The re-review sentence becomes: `A finding that is fixed is tagged [fixed] and keeps no other tag. A finding that is not fixed keeps its original [blocker] or [minor] tag.` (backticks around each tag, as in the current text). The same sentence goes into the reviewer agent files' re-review rule.

- [ ] **Step 1: Update and add the tests**

In `review_decide.rs`: replace `round_two_without_a_round_one_review_is_a_normal_review` with `rereview_uses_the_latest_review_file`: round 3, `review-code-r1.md` exists, no `review-code-r2.md`, `last_review` names r1: the prompt contains `scoped re-review` and names `review-code-r1.md` as round 1's findings. Keep `round_one_has_no_rereview_line`. Update `the_rereview_rule_keeps_the_tag_of_an_unfixed_finding` to the new sentence and add `the_rereview_rule_tags_fixed_findings` (prompt contains `tagged \`[fixed]\``).

In `agent_lint.rs`: add `reviewer_files_tag_fixed_findings`, looping the root file and all five presets (including node), requiring `[fixed]` and `keeps no other tag`.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -j4 -p factory-engine --test review_decide rereview` and `cargo test -j4 -p factory-cli --test agent_lint reviewer_files_tag_fixed`
Expected: FAIL.

- [ ] **Step 3: Change the prompt and the six agent files**

- [ ] **Step 4: Run the tests**

Run: `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli --test agent_lint`
Expected: PASS.

---

### Task 3: Sync reviews keep their own file, and the docs

**Files:**
- Modify: `crates/factory-engine/src/review.rs`, `crates/factory-engine/src/sync.rs`, `crates/factory-engine/src/compat.rs`
- Modify: `docs/reference/software-factory.md`, `docs/guides/factory-user-guide.md`, `docs/reference/factory-admin-manual.md`
- Test: `crates/factory-engine/tests/sync.rs` (or the existing sync tests file; find it with `grep -l sync_stage crates/factory-engine/tests`)

**Interfaces:**
- Consumes: `LastReview` and the ledger line format (plan 01).
- Produces:
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum ReviewRun { Round, Sync }`.
  - `pub fn review_stage_as(&self, run: ReviewRun) -> Result<ReviewOutcome, EngineError>`; `review_stage()` calls it with `Round`.
  - For `Sync`, the file is `review-code-r{round}-sync{n}.md` with `n` one more than the existing sync files of that round; the ledger line starts `sync review round {round} (sync)`; `last_review.source` is `sync` and `last_review.file` is that file.
  - `impl Driver { pub fn review_sync(&self, id: &str) -> Result<ReviewOutcome, EngineError> }`; `sync_stage` calls it instead of `review`.

- [ ] **Step 1: Write the failing tests**

- `a_sync_review_keeps_the_round_review`: after a sync, `review-code-r1.md` is byte-identical to before and `review-code-r1-sync1.md` holds the sync review.
- `two_sync_reviews_get_their_own_files`: two syncs in round 1 write `-sync1` and `-sync2`.
- `a_sync_review_that_asks_for_changes_sets_last_review`: `last_review` is `{verdict: "changes", source: "sync", file: "review-code-r1-sync1.md"}`, and the job parks with `sync review asked for changes`.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -j4 -p factory-engine sync_review`
Expected: FAIL.

- [ ] **Step 3: Implement `review_stage_as`, `review_sync` and the `sync_stage` change**

- [ ] **Step 4: Update the docs**

`software-factory.md`: the pipeline diagram and the rework section (rework is the next round, findings carried, files kept, sync review files). `factory-user-guide.md` and `factory-admin-manual.md`: what `factory rework` now does and its result line.

- [ ] **Step 5: Run every touched suite and clippy**

Run: `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`, `cargo test -j4 -p factory-supervisor`, then `cargo clippy -j4 -p factory-core -p factory-engine -p factory-cli -p factory-supervisor --all-targets -- -D warnings`
Expected: all pass, no warnings.

Next plan: docs/superpowers/plans/2026-10-29-act-on-review-findings/plan-04-park-reasons.md
