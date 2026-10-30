# Act On Review Findings, Plan 01: Decision Order And Evidence

> **Executor:** run with superpowers:subagent-driven-development: one fresh implementer per task, then a reviewer per task.

**Goal:** A CHANGES verdict starts a fix round on a risky diff too, the latest verdict survives fix rounds on the board, and every risky or changes park shows the review first.

**Architecture:** `decide()` moves the risk check to the end, so risk only gates the merge. `review_stage` records the verdict in a new board field `last_review` that `reset_reviews()` does not clear, and writes a richer ledger line. `decision_parts` builds the park evidence (Review, Risk, Rounds, Changed files) from `last_review`, the ledger and the diff. The fix-round count gains a `rework_base` offset and a per-class limit (`risky_fix_rounds`), both unused by rework until plan 03.

**Tech Stack:** Rust, serde, `factory_telemetry::capture` for event tests.

**Spec:** `docs/superpowers/specs/2026-10-29-act-on-review-findings-design.md` (sections 1, 2, 2a counting only, 6, 7 board fields, 8)

## Global Constraints

- Cargo per crate with `-j4`: `cargo test -j4 -p <crate> <filter>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- New board and park fields are optional: `#[serde(default, skip_serializing_if = ...)]`, so boards and `park.json` files written before this plan still load, and the dashboard keeps working.
- Git stays `factory_git::Repo`; never shell out to git. The only traits are `AgentRunner` and `Tracker`.
- A risky diff never reaches main without a human approve of the exact final head. This plan never turns a risky park into an auto-merge.
- `risky_fix_rounds = 0` gives today's results for risky diffs.
- Comments are short full sentences ending with a period, matching the surrounding code.

## Review Focus

1. A board written before this plan (no `last_review`, no `rework_base`) loads, decides and parks as before (`old_board_without_new_fields_loads`, Task 1).
2. A risky diff with an APPROVE verdict and a failing final check starts a fix round instead of parking for the human (`risky_approve_with_failing_final_checks_starts_a_fix_round`, Task 3).
3. A risky diff that is also over budget or over the diff cap parks as `BudgetStopped` or `DiffTooLarge`, not `RiskyDiff` (`risky_budget_stop_and_cap_come_first`, Task 2).
4. A re-review that marks an earlier finding `[fixed]` while repeating `[blocker]` on the same line does not count it as open (`fixed_lines_are_not_open_blockers`, Task 1).
5. A review file that is missing when the park is written still produces a Review part that names the verdict (`review_part_without_the_file_names_the_verdict`, Task 4).

## File Structure

- Modify `crates/factory-core/src/config.rs`: `risky_fix_rounds: Option<u32>` and `fix_rounds_for`.
- Modify `crates/factory-core/src/settings_block.rs`: list the new key.
- Modify `crates/factory-core/src/board.rs`: `LastReview`, `Job.last_review`, `Job.rework_base`.
- Modify `crates/factory-core/src/verdict.rs`: `Verdict::as_word`, `open_blockers`.
- Modify `crates/factory-core/src/park.rs`: optional `verdict` and `risky` on `ParkRecord`.
- Modify `crates/factory-engine/src/decide.rs`, `checks_stage.rs`, `runloop.rs`, `ports.rs` (`JobView`), `review.rs`, `park.rs`.
- Docs: `docs/reference/project-toml-reference.md`, `docs/reference/software-factory.md` (merge rules only).
- Tests: `crates/factory-core/src/{config,board,verdict,park}.rs` unit tests, `crates/factory-engine/tests/review_decide.rs`, `crates/factory-cli/tests/e2e.rs`.

---

### Task 1: Board, config and verdict types

**Files:**
- Modify: `crates/factory-core/src/config.rs`, `crates/factory-core/src/settings_block.rs`
- Modify: `crates/factory-core/src/board.rs`, `crates/factory-core/src/verdict.rs`, `crates/factory-core/src/park.rs`
- Modify: `crates/factory-core/src/lib.rs` (re-exports)
- Modify: `docs/reference/project-toml-reference.md` ("Limits and rounds" table)

**Interfaces:**
- Produces:
  - `Config.risky_fix_rounds: Option<u32>` (serde default `None`, listed in `settings_block::GROUPS` under "Limits and rounds." after `fix_rounds`), and `impl Config { pub fn fix_rounds_for(&self, risky: bool) -> u32 }`: `risky_fix_rounds.unwrap_or(fix_rounds)` when risky, else `fix_rounds`.
  - `#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)] pub struct LastReview { pub round: u32, pub verdict: String, pub blockers: u32, pub file: String, pub source: String }` in `board.rs`. `verdict` is `approve`, `changes` or `missing`; `source` is `build`, `checks`, `review`, `rework` or `sync`.
  - `Job.last_review: Option<LastReview>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`) and `Job.rework_base: u32` (`#[serde(default, skip_serializing_if = "is_zero")]`), both set in `Job::new`. Doc comments say "New in the act-on-review-findings series, absent in older boards."
  - `impl Job { pub fn changes_open(&self) -> bool }`: true when `last_review` has verdict `changes`.
  - `impl Verdict { pub fn as_word(self) -> &'static str }`: `approve`, `changes`, `missing`.
  - `pub fn open_blockers(review_text: &str) -> Vec<String>` in `verdict.rs`: the trimmed lines that contain `[blocker]` and do not contain `[fixed]`, in order. Re-exported from `factory_core`.
  - `ParkRecord.verdict: Option<String>` and `ParkRecord.risky: Option<bool>`, both `#[serde(default, skip_serializing_if = "Option::is_none")]`, `None` from `ParkRecord::new`.

- [ ] **Step 1: Write the failing tests**

In `config.rs` tests: `risky_fix_rounds_defaults_to_fix_rounds`: `Config::default().fix_rounds_for(true) == 2` and `fix_rounds_for(false) == 2`; `Config::parse("fix_rounds = 3").fix_rounds_for(true) == 3`; `Config::parse("risky_fix_rounds = 0").fix_rounds_for(true) == 0` and `fix_rounds_for(false) == 2`.

In `board.rs` tests: `last_review_and_rework_base_are_optional_and_round_trip`: a new `Job` serializes without `last_review` and `rework_base`; with `last_review = Some(LastReview { round: 2, verdict: "changes".into(), blockers: 1, file: "review-code-r2.md".into(), source: "review".into() })` and `rework_base = 1` it round-trips and `changes_open()` is true; with verdict `approve` it is false. `old_board_without_new_fields_loads`: a job JSON literal without either key parses with `last_review == None`, `rework_base == 0`, `changes_open() == false`.

In `verdict.rs` tests: `open_blockers_keeps_tagged_lines_in_order` (`"a\n- [blocker] one\n- [minor] two\n- [blocker] three"` gives `["- [blocker] one", "- [blocker] three"]`); `fixed_lines_are_not_open_blockers` (`"- [blocker] [fixed] one\n- [fixed] two\n- [blocker] three"` gives `["- [blocker] three"]`); `as_word_names_each_verdict`.

In `park.rs` tests: `an_old_park_record_still_parses` keeps passing; new `verdict_and_risky_are_omitted_when_unset_and_round_trip`.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -j4 -p factory-core risky_fix_rounds last_review open_blockers fixed_lines verdict_and_risky old_board`
Expected: FAIL to compile, unknown field or function.

- [ ] **Step 3: Add the fields, functions and the `settings_block` entry**

`every_config_key_is_listed_once` serializes `Config::default()` to JSON, where a `None` stays as `null`, so the key is listed. Add a "Limits and rounds" row for `risky_fix_rounds` to `project-toml-reference.md`: default "same as `fix_rounds`", "Fix rounds a risky diff may use before it parks. 0 parks a risky diff right after its first review, as before."

- [ ] **Step 4: Run the crate tests**

Run: `cargo test -j4 -p factory-core`
Expected: PASS, including `every_config_key_is_listed_once` and `rocket_parity`.

---

### Task 2: Decision order and the fix-round count

**Files:**
- Modify: `crates/factory-engine/src/decide.rs`
- Modify: `crates/factory-engine/src/checks_stage.rs` (`fix_or_park`)
- Modify: `crates/factory-engine/src/ports.rs` (`JobView`, `job()`)
- Modify: `crates/factory-engine/src/review.rs` (the `DecideInput` literal only)
- Modify: `crates/factory-engine/src/runloop.rs:505` (the loop cap)
- Test: `crates/factory-engine/tests/review_decide.rs`

**Interfaces:**
- Consumes: `Config::fix_rounds_for`, `Job.rework_base`, `Job.last_review` (Task 1).
- Produces:
  - `DecideInput` gains `pub rework_base: u32`. `fix_rounds` now carries the limit for this diff's class; callers pass `config.fix_rounds_for(class == DiffClass::Risky)`.
  - `pub(crate) fn fix_rounds_used(round: u32, rework_base: u32) -> u32`: `round.saturating_sub(1).saturating_sub(rework_base)`.
  - `JobView` gains `rework_base: u32` and `last_review: Option<LastReview>`, copied from the board.
  - The decision order of spec section 1:

```
budget Stop            -> NeedsHuman BudgetStopped
diff_lines > diff_cap  -> NeedsHuman DiffTooLarge
!checks_ok             -> rounds left ? FixRound : NeedsHuman ChecksFailed
Changes | Missing      -> rounds left ? FixRound : NeedsHuman ChangesRemain
Approve                -> risky ? NeedsHuman RiskyDiff (reason "risky diff: <summary>") : AutoMerge
```

- [ ] **Step 1: Update and add the unit tests in `review_decide.rs`**

Add `rework_base: 0` to the `input` helper. Replace `risky_diff_is_always_needs_human_even_when_approved` with:
- `risky_approve_needs_a_human`: risky + Approve gives `NeedsHuman { kind: RiskyDiff, .. }` with a reason starting `risky diff: `.
- `risky_changes_gets_a_fix_round_then_changes_remain`: risky + Changes at round 1 gives `FixRound`; at round 3 with `fix_rounds: 2` gives `NeedsHuman { kind: ChangesRemain, .. }`.
- `risky_failing_checks_get_a_fix_round`: risky + `checks_ok: false` at round 1 gives `FixRound`.
- `risky_budget_stop_and_cap_come_first`: risky + `BudgetState::Stop` gives `BudgetStopped`; risky + `diff_lines: 5000` gives `DiffTooLarge`.
- `zero_rounds_for_risky_parks_at_once`: risky + Changes + `fix_rounds: 0` gives `ChangesRemain`; risky + Approve + `fix_rounds: 0` gives `RiskyDiff`.
- `rework_base_resets_the_count`: Changes at round 4, `fix_rounds: 2`, `rework_base: 3` gives `FixRound`; `rework_base: 0` gives `ChangesRemain`.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -j4 -p factory-engine --test review_decide risky rework_base zero_rounds`
Expected: FAIL (risky still parks first; `rework_base` unknown).

- [ ] **Step 3: Reorder `decide`, change `fix_rounds_used`, and pass the class limit**

In `review_stage`, pass `fix_rounds: self.config.fix_rounds_for(risky)` and `rework_base: job.rework_base`. In `fix_or_park`, compute `risky` from `classify_paths(&self.job_changed_paths()?, &self.config)` and use `fix_rounds_used(job.round, job.rework_base)` against `self.config.fix_rounds_for(risky)`. In `runloop.rs`, the loop cap uses the larger of `fix_rounds` and `fix_rounds_for(true)`. Update the doc comment on `decide` to say the risk gate comes last so risky diffs get their fix rounds.

- [ ] **Step 4: Run the engine tests**

Run: `cargo test -j4 -p factory-engine`
Expected: PASS. `risky_diff_with_approve_needs_a_human_naming_the_path` (an APPROVE on a risky diff) still parks as `RiskyDiff`.

---

### Task 3: Record the verdict and gate a risky approve on the final checks

**Files:**
- Modify: `crates/factory-engine/src/review.rs` (`review_stage`, `apply_decision`)
- Test: `crates/factory-engine/tests/review_decide.rs`, `crates/factory-cli/tests/e2e.rs`

**Interfaces:**
- Consumes: `LastReview`, `open_blockers`, `Verdict::as_word` (Task 1); the new order (Task 2).
- Produces:
  - After saving the review file, `review_stage` sets `j.last_review = Some(LastReview { round: job.round, verdict: verdict.as_word().into(), blockers: open_blockers(&text).len() as u32, file: "<file name>".into(), source })`, where `source` is `job.fix_source` or `build` when none.
  - The ledger line becomes `review round {round} ({source}) verdict {verdict:?}, {blockers} blockers`.
  - The `decide` event gains `verdict` (the word) and `risky` (bool).
  - `fn final_gate(&self) -> Result<Option<StageOutcome>, EngineError>` in `review.rs`: the final-checks and `gate_checks` block that AutoMerge runs today, returning `Some(outcome)` when it started a fix round or parked, and `None` when everything passed (after the `checks ran: ...` ruling).
  - `apply_decision`: AutoMerge calls `final_gate` and then moves to Approve as today. `NeedsHuman { kind: RiskyDiff, .. }` calls `final_gate` first and parks only when it returns `None`.

- [ ] **Step 1: Write the failing tests**

In `review_decide.rs` (Fixture with a risky path, scripted reviewer):
- `review_writes_last_review_and_a_counted_ledger_line`: a CHANGES reply with two `[blocker]` lines leaves `last_review` `{round: 1, verdict: "changes", blockers: 2, file: "review-code-r1.md", source: "build"}` and `progress.md` contains `review round 1 (build) verdict Changes, 2 blockers`.
- `last_review_survives_a_fix_round`: after `apply_decision` on that CHANGES outcome, `reviews.code` is pending and `last_review.verdict` is still `changes`.
- `risky_changes_starts_a_fix_round`: on a risky fixture, CHANGES then `apply_decision` writes `changes-r2.md` and moves to `build`.
- `risky_approve_with_failing_final_checks_starts_a_fix_round`: on a risky fixture with a check that fails on the final run, APPROVE then `apply_decision` starts a fix round instead of parking.
- `decide_event_carries_verdict_and_risky`: under `capture("info", ..)`, the `decide` event has `verdict == "changes"` and `risky == true`.

In `e2e.rs`: `a_risky_changes_verdict_runs_a_fix_round_then_waits_for_approve`: `Scenario` with a `src-tauri/**` risk path and steps `build(src-tauri file), review(CHANGES), build(fix), review(APPROVE)`; after `run` the stage is `needs-human`, `review-code-r1.md` and `review-code-r2.md` both exist, then `approve` merges the reviewed sha.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -j4 -p factory-engine --test review_decide last_review risky_ decide_event` and `cargo test -j4 -p factory-cli --test e2e a_risky_changes`
Expected: FAIL.

- [ ] **Step 3: Implement the recording, `final_gate` and the risky-approve path**

- [ ] **Step 4: Run both crates' tests**

Run: `cargo test -j4 -p factory-engine` then `cargo test -j4 -p factory-cli --test e2e`
Expected: PASS, including `a_risky_diff_waits_for_approve_and_then_merges_the_reviewed_sha` and `a_changes_verdict_runs_one_fix_round_then_merges`.

---

### Task 4: Park evidence shows the review first

**Files:**
- Modify: `crates/factory-engine/src/park.rs` (`decision_parts`, `park_with`)
- Modify: `crates/factory-engine/src/checks_stage.rs` (`park_gate`, the out-of-rounds park in `fix_or_park`)
- Modify: `crates/factory-engine/src/build.rs` (the `NothingChanged` park after a review fix round)
- Modify: `crates/factory-engine/src/review.rs` (record the first-round risky paths)
- Modify: `docs/reference/software-factory.md` (merge rules: the new order and the evidence)
- Test: `crates/factory-engine/tests/review_decide.rs`

**Interfaces:**
- Consumes: `JobView.last_review`, `risk_summary`, `blocker_lines`, `open_blockers`.
- Produces:
  - `fn review_part(&self) -> Option<ParkPart>`: title `Review`, source `last_review.file`, text `verdict <word>, round <N>` followed by the open blocker lines (`blocker_lines` fallback to the whole reply when a CHANGES has none); `verdict <word>, findings file missing` when the file cannot be read; `None` when there is no `last_review`.
  - `fn risk_part(&self) -> Option<ParkPart>`: title `Risk`, the `risk_summary` text, then `added by fix rounds: <paths>` for risky paths not in `risky-paths.txt`; `None` when the diff is not risky.
  - `fn rounds_part(&self) -> Option<ParkPart>`: title `Rounds`, one line per `Ruling: review round` or `Ruling: sync review round` line in `progress.md`, rewritten as `round N (<source>): <verdict>, <B> blockers`; old lines without source or count show `?`.
  - `review_stage` writes `risky-paths.txt` (one path per line) on the first review of a risky job when the file does not exist yet.
  - Part order for `RiskyDiff` and `ChangesRemain`: Review, Risk, Rounds, Changed files. `NothingChanged` after a `review` or `rework` fix source, and `NoChecks`/`NoTests` gate parks on a risky diff, put Review (and Risk when risky) before their current parts.
  - `park_with` sets `record.verdict` from `last_review` and `record.risky = Some(true)` when the diff is risky.

- [ ] **Step 1: Write the failing tests**

- `risky_park_evidence_starts_with_the_review`: a risky APPROVE park has part titles `["Review", "Risk", "Rounds", "Changed files"]`, the Review text starts `verdict approve, round 1`, and `park.json` has `"verdict":"approve"` and `"risky":true`.
- `changes_remain_evidence_lists_the_open_blockers`: CHANGES twice past the limit gives a first part `Review` whose text contains each `[blocker]` line of the last review and no `[fixed]` line.
- `risk_part_names_paths_added_by_fix_rounds`: round 1 touches `src-tauri/a.rs`, the fix round adds `src-tauri/b.rs`; the Risk text contains `added by fix rounds: src-tauri/b.rs`.
- `review_part_without_the_file_names_the_verdict`: delete the review file before the park; the Review text is `verdict changes, findings file missing`.
- `nothing_changed_after_a_review_fix_shows_the_review`: the builder changes nothing on the fix round; the `NothingChanged` park's first part is `Review`.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -j4 -p factory-engine --test review_decide evidence risk_part review_part nothing_changed_after`
Expected: FAIL.

- [ ] **Step 3: Build the parts and wire them into the parks**

Keep `factory status` in mind: it shows only the first 12 lines of the first part, so the Review part's first line is the verdict line.

- [ ] **Step 4: Update the merge rules in `software-factory.md`**

Replace the "risky diff always stops first" wording with the order of Task 2 and a short list of the park parts.

- [ ] **Step 5: Run the tests and clippy**

Run: `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`, then `cargo clippy -j4 -p factory-core -p factory-engine --all-targets -- -D warnings`
Expected: all tests pass, no warnings.

Next plan: docs/superpowers/plans/2026-10-29-act-on-review-findings/plan-02-approve-and-waive.md
