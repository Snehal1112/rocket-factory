# Review Gate Follow-ups Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close GitHub issues #28, #29, #30 and #31: the missing tests, small cleanups and doc fixes left by the act-on-review-findings series (PR #17).

**Architecture:** No new feature. Five tasks add tests that pin behavior already shipped. Two tasks are cleanups whose existing tests must stay green. One task changes the `factory tickets` output, and one fixes docs. Each task touches one area, so a reviewer can judge it alone.

**Tech Stack:** Rust (factory-core, factory-checks, factory-engine, factory-cli, factory-supervisor), Markdown docs.

**Spec:** The issue bodies are the requirements: `gh issue view N --repo Snehal1112/rocket-factory` for N = 28, 29, 30, 31. The behavior they pin is in `docs/superpowers/specs/2026-10-29-act-on-review-findings-design.md`.

**Base:** Branch from `worktree-act-on-review-findings` (PR #17) while it is open, or from `main` after it merges. Line numbers below are from head `1eaf6c5`. Find code by function name when they drift.

## Global Constraints

- Cargo: always `-j4`, one crate at a time with `-p <crate>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Before each commit: `cargo fmt -p <crate> -- --check` and `cargo clippy -j4 -p <crate> --all-targets -- -D warnings` for every crate the task touched. Paste the output in the report.
- Every commit goes through the `dev-workflow-skills:1-git-commit` skill, with its analysis run inline (implementers start no subagents). Subjects are 50 characters or fewer.
- Code never shells out to git; git goes through `factory-git` (git2).
- Comments are short full sentences that end with a period.
- Board fields stay optional. Do not change the board JSON shape.
- After editing `.claude/agents/*.md`, a preset agent file, `README.md` or a file under `docs/`, run `cargo test -j4 -p factory-cli --test agent_lint`.
- Close an issue only through the commit footer of the task that finishes it: `Closes: #N` on the last commit of that issue's work.

## Review Focus

1. A hand-edited board whose `last_review.file` is absolute or contains `..`: every reader stays inside the job folder. Pinned in Task 1.
2. A waive reason that is only `\r`, or whitespace only: refused with the exact message, on the CLI and the API alike. Pinned in Tasks 5 and 6.
3. A reason of exactly 200 characters is accepted and 201 is refused, on both surfaces. Pinned in Tasks 5 and 6.
4. `factory tickets` on a job whose waivers were all superseded by a rework: it says so instead of claiming there are no waivers. Pinned in Task 5.
5. A `{crate}` check rule in a repo whose `Cargo.toml` is broken: the job still runs its other checks and the report says the map was unavailable. Pinned in Task 4.

---

### Task 1: factory-core: LastReview path helper, docs and core tests

Issues: #28 (old-board test), #29 (drift note), #30 (`last_review.file` join, `LastReview` docs), #31 (`changes_open_message` note).

**Files:**
- Modify: `crates/factory-core/src/board.rs` (`LastReview` at ~209-221, `changes_open_message` at ~285, test `old_board_without_new_fields_loads` at ~863)
- Modify: `crates/factory-core/src/resolve.rs` (tests module, `an_old_override_gives_a_drift_note` at ~631)
- Modify: `crates/factory-engine/src/specs.rs:561-567`, `crates/factory-engine/src/park.rs:171`, `crates/factory-engine/src/rework.rs:34`, `crates/factory-engine/src/minors.rs:42`, `crates/factory-cli/src/commands.rs:668-670`

**Interfaces:**
- Produces: `impl LastReview { pub fn path_in(&self, job_dir: &Path) -> Option<PathBuf> }` in `factory-core/src/board.rs`. It joins only the last component of `self.file`, and returns `None` when `file` has no file name (empty, `..`, `/`).

- [ ] **Step 1: Write the failing tests** in `board.rs` `mod tests`:

```rust
#[test]
fn last_review_path_stays_in_the_job_folder() {
    let dir = Path::new("/jobs/001");
    let mut last = LastReview { round: 2, verdict: "changes".into(), blockers: 1,
        file: "review-code-r2.md".into(), source: "review".into() };
    assert_eq!(last.path_in(dir), Some(dir.join("review-code-r2.md")));
    last.file = "../../etc/passwd".into();
    assert_eq!(last.path_in(dir), Some(dir.join("passwd")));
    last.file = "/abs/review.md".into();
    assert_eq!(last.path_in(dir), Some(dir.join("review.md")));
    last.file = "..".into();
    assert_eq!(last.path_in(dir), None);
    last.file = String::new();
    assert_eq!(last.path_in(dir), None);
}
```

  Replace the body of `old_board_without_new_fields_loads` so that it parses a hand-written literal with none of the new fields:

```rust
let text = r#"{"id":"001","dir":"d","slug":"s","created":"2026-10-13","feature":"f",
  "branch":"factory/001-s","stage":"needs-human","round":2,"stage_since":1.0,
  "reviews":{"code":"changes"}}"#;
let back: Job = serde_json::from_str(text).expect("old job loads");
assert_eq!(back.last_review, None);
assert_eq!(back.rework_base, 0);
assert!(!back.changes_open());
```

  In `resolve.rs` add `an_old_override_lists_every_missing_item`. Use the same setup as `an_old_override_gives_a_drift_note` and assert the whole note with `assert_eq!`. The rust preset reviewer has the sections `Stack`, `How to read`, `Check`, `Judging` and `Report`. Build the expected literal from `agent_drift`'s item wording (`crates/factory-core/src/agent_drift.rs:14-27`): `"agent drift: factory-code-reviewer.md lacks the rules marker, the section … and the section Report; compare it with the rust preset or remove the override"`. Add `a_base_tip_override_gives_a_drift_note`, which puts the old file in the tip with `tip.dirs.insert(AGENTS_DIR.to_string(), vec![("factory-code-reviewer.md".into(), "old\n".into())])` (pattern at resolve.rs:667) and asserts that the same note appears in `r.notes`.

- [ ] **Step 2: Run** `cargo test -j4 -p factory-core`. Expected: `last_review_path_stays_in_the_job_folder` fails to compile (no `path_in`). The two drift tests pass or fail on wording; fix the literal to match the real items, not the code.

- [ ] **Step 3: Implement `path_in`**, and give the `round` and `file` fields of `LastReview` doc comments: round is "the job round the review ran in"; file is "the review file's bare name in the job folder, for example `review-code-r2.md` or `review-code-r1-sync1.md`". Extend the doc comment of `changes_open_message` with: "It is reached only while `changes_open()` holds, so a `last_review` exists and its round is 1 or more."

- [ ] **Step 4: Switch the five joins to `path_in`** (specs.rs, park.rs, rework.rs, minors.rs, commands.rs). A `None` takes the same path the code takes today for a missing file. Keep each site's existing fallback.

- [ ] **Step 5: Run** `cargo test -j4 -p factory-core`, `-p factory-engine` and `-p factory-cli`, plus fmt and clippy for those three crates. Expected: all pass.

- [ ] **Step 6: Commit** through `dev-workflow-skills:1-git-commit`.

### Task 2: Engine cleanups with no behavior change

Issue: #30 (all bullets except the hint constant, the `last_review.file` join and the "no waivers" line).

**Files:**
- Modify: `crates/factory-engine/src/runloop.rs:599-602`, `checks_stage.rs:115-140,194-214`, `review.rs:279-282`, `park.rs:67-189`, `merge.rs:44-49,98-138`, `rework.rs:39,49-56`, `ports.rs:72-130`, `decide.rs:77`

**Interfaces:**
- Produces: `pub(crate) fn evidence_or_reason(parts: Vec<ParkPart>, reason: &str) -> Vec<ParkPart>` in `park.rs`, used by `park_with` and `merge.rs`.
- Produces: `lead_parts(&self, paths: &[String]) -> Vec<ParkPart>`, `risk_part(&self, paths: &[String]) -> Option<ParkPart>`, and `park_gate(&self, kind: ParkKind, reason: String, paths: &[String])`. Callers pass the paths they already computed.
- Produces: `JobView.park_kind: Option<String>`. Removes `JobView.changes_open`.
- Produces: `fn report_soft_step(driver: &Driver, id: &str, step: &str, result: Result<TicketsReport, EngineError>)` in `merge.rs`.

- [ ] **Step 1:** In `run_job_inner`, read `driver.config()` once: `let cfg = driver.config(); let rounds = cfg.fix_rounds.max(cfg.fix_rounds_for(true));`.
- [ ] **Step 2:** Drop the unused `_report` parameter of `gate_checks`, and update its two callers (checks_stage.rs:267, review.rs:280).
- [ ] **Step 3:** Thread the changed paths through. `fix_or_park`, `checks_stage` and `final_gate` already compute `paths`, so pass them to `park_gate` and `lead_parts`, and stop `risk_part` from calling `job_changed_paths`. `park_with` keeps its one `is_risky` call. Then a park computes the paths twice, not three times.
- [ ] **Step 4:** Move the "Why it stopped" fallback into `evidence_or_reason`, and call it from `park_with` and `merge.rs:44-49`.
- [ ] **Step 5:** In `park_with`, pass `record.verdict.as_deref()` straight to `kind.next_for`, delete the `changes_open` hint lines, and remove `JobView.changes_open` and its initializer. Add one comment above `let job = self.job().ok();` (park.rs:73): "A park must still happen when the board cannot be read, so a missing job falls back to round 1."
- [ ] **Step 6:** Add `park_kind` to `JobView`, filled in `Engine::job` from `job.park_kind.clone()`. In `open_findings`, use `job.park_kind.as_deref() == Some("checks-failed")`, and delete `park_kind_is_checks_failed`.
- [ ] **Step 7:** Replace the three ticket/waiver/minor result blocks in `merge_stage` (merge.rs:98-138) with `report_soft_step` calls. Keep the step names `"tickets"`, `"waivers"`, `"minors"`, the log text, and the `tickets_on_merge` gate on the first one.
- [ ] **Step 8:** Wrap the `decide.rs:77` doc comment to two lines of at most 100 columns.
- [ ] **Step 9: Run** `cargo test -j4 -p factory-engine`, `-p factory-cli` and `-p factory-supervisor`, plus fmt and clippy for each. Expected: every existing test passes unchanged. If a test needs editing, this task changed behavior: stop and report.
- [ ] **Step 10: Commit** through `dev-workflow-skills:1-git-commit`, grouped by concern (park evidence, merge steps, small reads).

### Task 3: Engine tests for park evidence and fix rounds

Issue: #28 (engine bullets).

**Files:**
- Test: `crates/factory-engine/tests/review_decide.rs`, `crates/factory-engine/tests/park_evidence.rs`, `crates/factory-engine/src/park.rs` (`mod tests`), `crates/factory-engine/src/runloop.rs` (`mod tests`)

Rulings made while planning:
- The `ChangesRemain` risky branch of `fix_or_park` cannot be reached, because `decide` parks first with the same limit. Do not test it. Pin the `ChecksFailed` branch in full instead.
- The Ledger part is tested as a unit test in `runloop.rs`, because only an exhausted run loop reaches it.
- `round_line` is tested as a unit test in `park.rs`.

- [ ] **Step 1: `risky_final_check_park_keeps_the_risk_part`** (review_decide.rs:1083): replace the `&titles(&rec)[..2]` check with the full list `["Review", "Risk", "Failing checks"]`. Also assert that the last part's source is `"checks"` and that its text is not empty.
- [ ] **Step 2: `risky_failing_checks_with_zero_rounds_park_at_once`** (review_decide.rs:915): replace the control fixture's `fx.job().round > parked_round` with `assert!(control.job_dir.join("changes-r2.md").exists())`, so each fixture checks its own files.
- [ ] **Step 3: `risky_park_evidence_starts_with_the_review`** (review_decide.rs:983-988): replace the `||` assertion with `assert_eq!(rec.parts[2].text, "round 1 (build): approve, 0 blockers")`.
- [ ] **Step 4:** Add `a_round_two_gate_park_shows_the_round_one_review` in review_decide.rs:
  1. Round 1 is reviewed with `TWO_BLOCKERS`, and `apply_decision` starts round 2.
  2. A builder writes `b.txt`, and `fx.config.checks` is set so that no rule maps `b.txt` (rule `src/**` -> `false`, like `gates.rs:27`).
  3. `checks_stage()` parks.
  4. Assert: kind `NoChecks`, `fx.job().round == 2`, `parts[0].title == "Review"`, and `parts[0].text.starts_with("verdict changes, round 1")`.
- [ ] **Step 5:** Add to park_evidence.rs:
  - `a_merge_error_is_its_own_part`: `common::approved_job()`, then clear `head` with `fx.driver.store().update_job(&fx.id, |j| { j.head = None; Ok(()) })`, then `merge_stage`. Assert `MergeResult::Parked(_)`, and that the only part has title `"Merge error"`, source `"git"`, and text containing `"no recorded head"`.
  - `a_job_without_a_plan_names_the_missing_file`: `SeriesFixture::new("x")`, `create_job(&fx.driver, "x", "f", None, None)`, `run_job`. Assert `parts[0].title == "Missing plan"`, and that its text starts with `"expected "` and ends with `"plan.md"`.
- [ ] **Step 6:** Add `unsettled_park_keeps_the_last_twenty_ledger_lines` in `runloop.rs` `mod tests`. Use `driver(&tmp)` and `add_job(..., Stage::Build, None)` as the tests at ~745 do, write a 25-line `progress.md`, and call `park_unsettled`. Assert: reason `"run loop did not settle, see the ledger"`; one part titled `"Ledger"` with source `"progress.md"`; its text has 20 lines and starts with line 6.
- [ ] **Step 7:** Add `round_line_renders_round_sync_and_old_lines` in `park.rs` `mod tests`:

```rust
assert_eq!(round_line("Ruling: sync review round 1 (sync) verdict Approve, 0 blockers"),
    Some("round 1 (sync): approve, 0 blockers".to_string()));
assert_eq!(round_line("Ruling: review round 2 (rework) verdict Changes, 3 blockers"),
    Some("round 2 (rework): changes, 3 blockers".to_string()));
assert_eq!(round_line("Ruling: review round 2 verdict Changes"),
    Some("round 2 (?): changes, ? blockers".to_string()));
assert_eq!(round_line("Task 1: complete"), None);
```

- [ ] **Step 8: Run** `cargo test -j4 -p factory-engine`, plus fmt and clippy. Expected: all pass. A new test that fails points to a real bug: report it, do not change production code.
- [ ] **Step 9: Commit** through `dev-workflow-skills:1-git-commit`.

### Task 4: Tests for the check gate and the workspace map

Issue: #29 (gate and workspace bullets).

**Files:**
- Test: `crates/factory-engine/tests/gates.rs`, `crates/factory-engine/tests/build_checks.rs`, `crates/factory-checks/src/workspace.rs` (`mod tests`)

- [ ] **Step 1: Do this one first.** Add `a_risky_approve_meets_the_no_checks_gate` to gates.rs. Copy the flow of `the_final_checks_gate_parks_an_approved_job` (gates.rs:101), but build `src-tauri/src/lib.rs`, so the decision is `Decision::NeedsHuman { kind: ParkKind::RiskyDiff, .. }`. After `only_src_checks(&mut fx)` and `apply_decision`, assert:
  - kind `NoChecks` (not `RiskyDiff`);
  - the part titles start with `["Review", "Risk"]`;
  - `rec.risky == Some(true)` and `rec.verdict.as_deref() == Some("approve")`;
  - `fx.job().stage == Stage::NeedsHuman`.
- [ ] **Step 2:** In `risky_diff_with_an_unchecked_path_parks` (gates.rs:75), add a local `titles` helper and assert:
  - titles `["Risk", "Checks", "Why it stopped"]`;
  - `rec.parts[0].text.starts_with("risky paths (1): src-tauri/src/lib.rs")`;
  - `rec.risky == Some(true)`.
- [ ] **Step 3:** In build_checks.rs, add `a_broken_workspace_map_still_runs_the_checks`. Copy `a_capped_away_dependents_check_still_leaves_its_note` (build_checks.rs:1736), but write `Cargo.toml` as `"not toml\n"`, and add a second rule with no placeholder that matches the same path and runs `true`. Assert that `checks-task1.md` contains `NOTE workspace map unavailable` and the plain rule's command line.
- [ ] **Step 4:** In workspace.rs, add `load_reads_a_real_workspace` and `load_fails_without_a_manifest`. The first writes a two-member workspace into a `tempfile::tempdir()` (manifests as in build_checks.rs:1736) and asserts that `owner` and `dependents` give the member names. The second calls `Workspace::load` on an empty temp dir and asserts the error text starts with `"cargo metadata failed"`. These tests need `cargo` on PATH, which `cargo test` provides.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine` and `-p factory-checks`, plus fmt and clippy. Expected: all pass.
- [ ] **Step 6: Commit** through `dev-workflow-skills:1-git-commit`.

### Task 5: CLI tests and the `factory tickets` waiver line

Issues: #28 (CLI bullets), #29 (CLI and spec-check bullets), #30 ("no waivers" line).

**Files:**
- Modify: `crates/factory-engine/src/waivers.rs:128-134`, `crates/factory-cli/src/commands.rs:1021-1058` (`tickets`)
- Test: `crates/factory-cli/tests/{e2e,new_to_merged,one_job,agent_lint,tickets,tokens_cache}.rs`, `crates/factory-engine/src/spec_check.rs` (`mod tests`), `crates/factory-engine/tests/waivers.rs`

Ruling made while planning: "not useful" means the line shows on every merged job that never had a waiver. With no waivers file or an empty list, the waiver step returns a skipped report with no lines, and `factory tickets` prints nothing for that part. When every waiver was superseded, the line reads `skipped: every waiver was superseded by a rework`.

- [ ] **Step 1: Waiver line tests first.**
  - In engine `tests/waivers.rs`, add `no_waivers_gives_no_line`: merge a job with no waivers and run `waiver_tickets_step`, then assert `report.skipped` and `report.lines.is_empty()`.
  - Add `superseded_waivers_say_so`: reuse the waive-park-rework fixture that fcb784c added, and assert that `report.lines == ["skipped: every waiver was superseded by a rework"]`.
  - In `crates/factory-cli/tests/tickets.rs`, add `tickets_prints_no_waiver_line_without_waivers`. Assert the output lacks `waiver` and has no blank line.
- [ ] **Step 2:** Implement it. In `waivers.rs`, split the empty-list case from the all-superseded case. In `commands.rs` `tickets`, skip a part whose text is empty. Run `cargo test -j4 -p factory-engine --test waivers` and `-p factory-cli --test tickets`. Expected: pass.
- [ ] **Step 3: e2e.** In `a_risky_changes_verdict_runs_a_fix_round_then_waits_for_approve` (e2e.rs:109), capture `let before = sc.main_tip()?;` after `start`. Assert `sc.main_tip()? == before` before `approve`, and `sc.stage("001")? == "merged"` after it, as e2e.rs:82-106 does.
- [ ] **Step 4: Waive reasons.** In `waive_reason_must_be_one_short_line` (new_to_merged.rs:867), table each reason with its exact message:
  - `""` and `"   "` give `the waive reason is empty`;
  - `"two\nlines"` and `"a\rb"` give `the waive reason must be one line`;
  - 201 `x` give `the waive reason is longer than 200 characters`.

  After each refusal the stage stays `needs-human`. Then approve with a 200-character reason and assert it is accepted (stage `merged`). In `one_job.rs:181`, tighten the `--accept-spec` reason test the same way.
- [ ] **Step 5: Rework.** In `rework_grants_a_full_set_of_fix_rounds` (new_to_merged.rs:1074), also assert:
  - `rework_base == 2` after the rework;
  - after the final `run`, stage `needs-human` and `park_kind == Some("changes-remain")`.
- [ ] **Step 6: Spec check.** In `one_job.rs`, replace both `!status.contains("demo")` checks (lines 177 and 192) with a board load that asserts `board.jobs.is_empty()`, as one_job.rs:~108 does. Then add:
  - `a_series_with_a_risky_plan_is_refused`: `plans_folder(&fx, 2, 1)`, with the risky path in plan-01;
  - `one_job_with_a_risky_plan_is_refused`: the same plan plus `--one-job`.

  Both expect `"no decision in it was answered by the user"` and an empty board. In `spec_check.rs` `mod tests`, add:
  - `paths_outside_files_blocks_are_ignored`: a backticked path in prose before the block, and another after the block ended by a prose line;
  - `two_paths_on_one_line_are_both_read`: `` - Modify: `a.rs` and `b.rs` `` gives `["a.rs", "b.rs"]`;
  - `a_degraded_config_makes_every_path_risky`: `risk_paths = ["**"]` with `SPEC_NONE` gives `Some(..)`, and a plan with no Files paths gives `None`.
- [ ] **Step 7: Minor tickets off.** Add `tickets_with_minor_tickets_off_says_so` to `crates/factory-cli/tests/new_to_merged.rs`:
  - `Scenario::new("minor_tickets = false\n", ..)`, with a review `"- [minor] nit\nVERDICT: APPROVE"`, and a create-counting tracker (`CountCreates`, ~905);
  - run to merged, then `factory tickets 1`;
  - assert that the output contains `skipped: minor_tickets is off` and the tracker saw no `create`.
- [ ] **Step 8: Spec-review tokens.** Add `cache_report_counts_spec_review_usage` to `tokens_cache.rs`. Write one usage line with `"job":"spec-review","role":"spec-reviewer"` to `Layout::in_repo(&fx.root).spec_review_usage_file()`, run `tokens --cache`, and assert that the spec-reviewer row appears. Read the role label from `render_cache` in `crates/factory-cli/src/tokens.rs`.
- [ ] **Step 9: agent_lint.**
  - Add `const REVIEWER_FILES: [&str; 6]` (`.claude/agents/factory-code-reviewer.md` and the five presets, node included), and use it in the four tests that list reviewer paths (agent_lint.rs ~829, 906, 920, 935).
  - In `reviewer_files_tag_fixed_findings`, pin the whole re-review sentence that starts with "- When the prompt says this is a re-review" (`.claude/agents/factory-code-reviewer.md:39`) with `require_all`.
  - If the node preset fails a test it was missing from, report it and do not edit the preset in this task.
- [ ] **Step 10: Run** `cargo test -j4 -p factory-cli` and `-p factory-engine`, plus fmt and clippy. Expected: all pass.
- [ ] **Step 11: Commit** through `dev-workflow-skills:1-git-commit`, with the waiver-line change in its own commit.

### Task 6: Supervisor API tests and the shared refusal hint

Issues: #28 (API bullet), #29 (stdout-only hint), #30 (hint constant).

**Files:**
- Modify: `crates/factory-engine/src/spec_check.rs:30-32`, `crates/factory-engine/src/lib.rs:97`, `crates/factory-supervisor/src/api/write.rs:271-272,325-328`
- Test: `crates/factory-supervisor/src/api/write.rs` (`mod tests`)

**Interfaces:**
- Produces: `pub const SPEC_OVERRIDE_HINT: &str` in `factory_engine::spec_check`, re-exported from `factory_engine`. It is today's `OVERRIDE_HINT` text, unchanged.

Ruling made while planning: the supervisor matches the full sentence. Both binaries come from one build, so the sentence cannot drift. The ReviewerMissing refusal stays a 500 here; issue #19 owns it.

- [ ] **Step 1: Write the tests** in `write.rs` `mod tests`, reusing `fx`, `post`, `code`, `queued`, `board_with`, `job_json` and `write_board`. The "open-changes board" is the needs-human job with `last_review` verdict `changes` that `approve_with_open_changes_needs_a_waive_reason` (~898) builds.
  - `waive_with_without_review_is_a_bad_request`: open-changes board, body `{"waive_reason":"x","without_review":true}`. Expect `(400, "bad_request")` and nothing queued.
  - `waive_without_open_changes_is_a_conflict`: `board_with(&f, "needs-human", false)`, body `{"waive_reason":"known flake"}`. Expect `(409, "conflict")`, a message containing `has no open review findings to waive`, and nothing queued.
  - `waive_reason_length_is_checked`: on the open-changes board, 201 `x` give `(400, "bad_request")` with `the waive reason is longer than 200 characters`, and 200 `x` give 202. Also `"\r"` and `"   "` give 400.
  - `create_refusal_on_stdout_is_a_conflict`: `f.cli.reply(1, &text, "")`, where `text` is built with `SPEC_OVERRIDE_HINT`. Expect `(409, "conflict")` and one CLI call.
- [ ] **Step 2: Run** `cargo test -j4 -p factory-supervisor`. Expected: the stdout test fails to compile until the constant exists. The others pass or fail on wording; fix the literal, not the code.
- [ ] **Step 3: Implement it.** Make `OVERRIDE_HINT` `pub const SPEC_OVERRIDE_HINT`, re-export it in `lib.rs`, and delete the supervisor's `SPEC_REFUSAL_HINT`. Match `factory_engine::SPEC_OVERRIDE_HINT` instead, and update the existing test text at write.rs:970 and spec_check.rs:347 to use the constant.
- [ ] **Step 4: Run** `cargo test -j4 -p factory-supervisor`, `-p factory-engine` and `-p factory-cli --test agent_lint`, plus fmt and clippy. Expected: all pass.
- [ ] **Step 5: Commit** through `dev-workflow-skills:1-git-commit`. This task holds the last items of #28, #29 and #30, so its last commit carries `Closes: #28`, `Closes: #29` and `Closes: #30`.

### Task 7: Docs for the review gate

Issue: #31.

**Files:**
- Modify: `docs/reference/software-factory.md` (diagram 9-26, lines 32, 217), `docs/guides/factory-user-guide.md` (refusal table 450-461), `docs/reference/project-toml-reference.md` (lines 59, 226, 233), `README.md` (line 481, `### Upgrading` at ~255)

- [ ] **Step 1: Diagram.** In the `factory run` box, add after the merge line: `main moved -> merge main in, re-run checks and a sync review (review-code-r<N>-sync<K>.md, at most 2) -> merge`.
- [ ] **Step 2: Refusal table.** In the user guide, replace the paraphrased row with the real text from `board.rs` `changes_open_message`: `job N has B open review blockers from round R (FILE): use rework, cancel, or approve --waive "<reason>" to merge anyway`. Add rows for:
  - `job N has no open review findings to waive`;
  - `job N review did not finish in round R: use rework, cancel, or approve --without-review to merge anyway`.
- [ ] **Step 3: 409 wording.** In software-factory.md:32 and README.md:481, say that a refusal answers 409 `conflict` when it carries the `--accept-spec` override hint (a missing user decision, or a spec review that asked for changes or did not finish). Every other failure of `factory new` stays a 500.
- [ ] **Step 4: Case.** In project-toml-reference.md:
  - add "Matching ignores case." to the `unchecked_paths` row;
  - add `unchecked_paths` to the glob list at line 226;
  - add `unchecked_paths` to the case-insensitive line at 233.
- [ ] **Step 5: Earlier review.** Extend software-factory.md:217: "In round 2 or later, before the new review runs, the Review part is the previous round's review, and its text names that round."
- [ ] **Step 6: Release note.** Under README `### Upgrading`, add: "Boards written before this release have no `last_review`. A job parked there with a CHANGES review approves as before, with no `--waive`. The first review after the upgrade writes the field."
- [ ] **Step 7: Run** `cargo test -j4 -p factory-cli --test agent_lint`. Expected: pass. If a doc pin fails, keep the pinned words and reword around them.
- [ ] **Step 8: Commit** through `dev-workflow-skills:1-git-commit`, with `Closes: #31`.
