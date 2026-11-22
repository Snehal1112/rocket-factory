# Engine small fixes

## Goal

Fix seven small engine bugs, #18, #20, #22, #25, #50, #51 and #139, so that parks, check reports, review files and filed tickets stay correct when a write fails, a file is missing or a sync ran.

## Overview

Each bug was found in a job audit or a final review and filed as size S. Each one is confirmed in the current source:

- #22: the sync review number counts files, so a gap reuses a number and overwrites an older review.
- #139: with no `park.json`, the next commands ignore the review state and offer an approve that `approve` refuses.
- #51: the Engine park saves the park fields and the stage move in two board saves.
- #50: a failed `checks.md` write turns a real check result into "could not run".
- #25: a rework after failing checks hands the builder only the `FAIL` header lines.
- #18: a failed record write after `tracker.create` loses the new issue number.
- #20: after a sync, the minor ticket reads only the sync review and misses the round review's minors.

## Current state

| Bug | Where | Today |
|---|---|---|
| #22 | `crates/factory-engine/src/review.rs:132-139` | `ReviewRun::Sync` counts the entries that start with `review-code-r{round}-sync` and names the new file `taken + 1`. |
| #139 | `crates/factory-core/src/park.rs:305-316` | `park_commands` reads `park.json`, else calls `ParkKind::next(id)`. `next_for` (`park.rs:160`) is the review-aware form. Callers: `no_resume_refusal` (`park.rs:319`), `crates/factory-engine/src/retry.rs:35` and `crates/factory-supervisor/src/admit.rs:194`. |
| #51 | `crates/factory-engine/src/park.rs:238-260`, `park.rs:93-105` | `park_with_target` runs `in_park_order` with an `update` step (park fields) and a `stage` step (`move_to_needs_human`, `ports.rs:229`), and each step is its own `BoardStore::update_job`. `merge.rs:90-103` (`park_with_parts_target`) does both in one closure. |
| #50 | `crates/factory-engine/src/checks_stage.rs:87-93`, `park.rs:134-150` | `run_checks_now` returns `Err` when `write_checks_md` or `append_notes` fails after the checks ran. `sync_park_choice` maps any `Err` to `RiskyDiff` with "final checks could not run". |
| #25 | `crates/factory-engine/src/rework.rs:30-47` | `open_findings` keeps only the lines of `checks.md` that start with `FAIL`. `CheckResult.tail` (`crates/factory-checks/src/run.rs:22`, the last 60 lines) and `ChecksReport::failure_text` (`run.rs:42`) are used for fix rounds (`checks_stage.rs:425`) but never saved. |
| #18 | `crates/factory-engine/src/minors.rs:68-72`, `crates/factory-engine/src/waivers.rs:164-170` | After `tracker.create` returns `Ok(number)`, `write_atomic(...)?` and `write_waivers(...)?` return early on a write error. The number reaches no report line and no ledger ruling. |
| #20 | `crates/factory-engine/src/minors.rs:42-50` | `open_minors` runs on the text of `job.last_review` only. After a sync, `last_review.file` is `review-code-rN-syncK.md`. The ticket body names that one file. |

Gaps: a deleted review file can be overwritten; a refusal can name a command that is then refused; a crash can leave park fields on a job still in review; a failing check can be offered for approve; a rework builder works without error output; a ticket can be filed twice; minors can be lost.

## Non-goals

- No change to the `checks.md` format or to who reads it.
- No pending marker before `tracker.create` (#18 reports the number instead), so a re-run after a failed record write can still file a duplicate; the number is visible in the report and ledger.
- No change to minors filed for rounds before the last review's round.

## Design

```
review stage (sync) -> next_sync_number ------------------------ #22
park (no park.json)  -> park_commands(.., review_missing, verdict) #139
Engine park          -> one update: park fields + stage move ---- #51
run_checks_now       -> run, write checks.md + checks-failures.md,
                        a write error is logged, report returned  #50 #25
rework               -> checks-failures.md, else FAIL lines ----- #25
minor / waiver step  -> create, record; a record error is reported #18
minor step           -> minors of rN, rN-rebase, rN-syncK ------- #20
```

### 1. Sync review number (#22)

`fn next_sync_number(job_dir: &Path, round: u32) -> Result<u32, EngineError>` in `review.rs`. It reads the job folder and, for each name of the exact form `review-code-r{round}-sync{K}.md` with `K` all ASCII digits that parse as `u32`, takes the largest `K`. It returns that plus 1, or 1 when none match. Other names (another round, `.tmp`, no digits) are ignored. A folder read error maps through `io_err` as today. Example: `sync1.md` and `sync3.md` give 4.

### 2. Review-aware fallback commands (#139)

```rust
pub fn park_commands(dir: &Path, id: &str, park_kind: Option<&str>, review_missing: bool, verdict: Option<&str>) -> Vec<String>
pub fn no_resume_refusal(dir: &Path, id: &str, park_kind: Option<&str>, review_missing: bool, verdict: Option<&str>) -> String
```

A non-empty `park.json` `next` list still wins. The fallback becomes `kind.next_for(id, review_missing, verdict)`, with `Other` for a missing or unknown kind. `retry.rs` and `admit.rs` pass `job.review_missing()` and `job.last_review.as_ref().map(|r| r.verdict.as_str())`. Example: an `Other` park with no record and a code review still pending gives `factory rework 001 <note>, factory cancel 001, factory approve 001 --without-review (the review has not run)`.

### 3. One board save for an Engine park (#51)

`in_park_order` loses its separate `stage` step: `in_park_order(clear, update, record, ruling)`. In `park_with_target`, the `update` closure sets `park_kind`, `park_resume`, `park_at` and `park_summary` and then calls `set_stage_logged(j, Stage::NeedsHuman, Some(&reason), false, now())` in the same `self.update` call. `log_park(&self.job_id, &reason)` runs after that update succeeds, as `move_to_needs_human` does today. When the stage move is refused, the closure returns `Err`, `BoardStore::update` saves nothing (`store.rs:61-76`), and no park field is written. `move_to_needs_human` stays for its other callers.

### 4. Keep the check report when a write fails (#50)

`run_checks_now` keeps running after the checks ran. Before writing, it removes the old `checks.md` and `checks-failures.md` (a missing file is fine), so a failed write leaves no stale evidence from an earlier run: a missing `checks.md` gives no Checks part on the park card and makes `latest_checks_ok` (`review.rs:368`) return `Err`, never a stale pass. A failure of `write_checks_md`, `append_notes` or the new failures file (piece 5) is logged as a `tracing::error!(event = "step.error", step = "checks_md", ...)` and as a ledger ruling `checks.md could not be written: <err>`. A ruling error is logged too. Then `record_gates`, the `checks_pass` update and `gate_checks_run` run as today and the report is returned. So a failing run after a sync parks as `checks-failed`, and a passing run keeps its pass. `planned_checks` errors (no worktree) still return `Err`, so "final checks could not run" stays for checks that did not run.

### 5. Failing output for a rework (#25)

`pub const CHECKS_FAILURES_FILE: &str = "checks-failures.md";` in `checks_stage.rs`. After writing `checks.md`, `run_checks_now` writes `report.failure_text()` to that file with `write_atomic` when any check failed. When all passed, no file is written, so the removal in piece 4 leaves none. `open_findings` for a `checks-failed` park reads `checks-failures.md` first. When it is missing or blank, it falls back to the `FAIL` lines of `checks.md` as today. There is no extra cap: each tail is already at most 60 lines, the same text a fix round gets.

### 6. Report a ticket whose record write failed (#18)

In `minor_tickets_step`, after `Ok(number)`, a failed `write_atomic` of `minors.json` sets `report.failed = true` and pushes `failed: created #{number} but could not record it in minors.json: {e}`. The function goes on to the ledger ruling and returns `Ok(report)`. In `waiver_tickets_step`, a failed `write_waivers` after `Ok(number)` pushes `failed: created #{number} but could not record it in waivers.json: {e}`, sets `failed` and leaves the loop (`break 'all`), because later records would fail the same way. The ruling then carries the number. `factory tickets` prints report lines to stderr as today.

### 7. Minors from the whole merged round (#20)

The minor step reads, for round `N = last_review.round`: `review-code-rN.md`, then `review-code-rN-rebase.md`, then every `review-code-rN-syncK.md` by `K` ascending (the same exact-name rule as piece 1). A missing file is skipped. The open minors of all of them are joined in that order, and a line already seen (exact text after `open_minors`) is dropped. When `last_review.file` is not among those names, it is read last, so the minors of today's file are never lost. The body says `Review files a, b.` with every file that was read, in read order. No file read and no minors gives the existing skip.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| Sync files 1 and 3, no 2 | Next file is sync4. |
| `review-code-r1-syncx.md` or `review-code-r10-sync1.md` in round 1 | Ignored by the number and by the minor step. |
| No `park.json`, kind `other`, review pending | Offers `approve --without-review`, not `approve`. |
| `park.json` with a non-empty `next` | Its list is used unchanged. |
| Stage move refused during an Engine park | No park field saved, `Err` returned, no record written. |
| `checks.md` is a directory, checks fail | Report returned, a `checks-failed` park, ruling names the write error. |
| A passing `checks.md` from an earlier run, then a failing run whose write fails | The old file is removed first, so no stale PASS remains. |
| `set_stage_logged` logs `stage.change` and the board save then fails | The event is logged without a saved move, as `merge.rs` does today; accepted. |
| No worktree for checks | `Err` as today, "final checks could not run". |
| All checks pass after a failing run | `checks-failures.md` is removed. |
| `checks-failures.md` missing on an old job | Rework falls back to `FAIL` lines. |
| `minors.json` cannot be written | Report failed, line names `#N`, ruling written. |
| Second waiver blocker after a failed record | Not filed in this run. |
| Same minor in round review and sync review | Listed once. |
| Only a sync review file exists | Its minors are filed, body names it. |

## Testing

factory-engine unit tests (`cargo test -j4 -p factory-engine <name>`):

- `review.rs`: `next_sync_number_takes_the_highest_plus_one` (files sync1 and sync3 give 4), `next_sync_number_is_one_in_an_empty_folder`, `next_sync_number_ignores_other_rounds_and_bad_names`.
- `park.rs`: the `in_park_order` tests rewritten for four steps (`park_steps_run_clear_then_update_then_record_then_ruling`, `a_failed_board_update_leaves_no_park_record`, `a_failed_ruling_leaves_the_park_record`; `a_failed_stage_move_leaves_no_park_record` is removed with the step), and `a_refused_stage_move_saves_no_park_fields` (a job whose move to needs-human is refused keeps `park_kind == None` and the stage it had).
- `rework.rs`: `rework_findings_carry_the_failing_check_output` (a `checks-failures.md` tail line is in the findings), `rework_findings_fall_back_to_fail_lines_without_the_failures_file`.

factory-core unit tests (`cargo test -j4 -p factory-core park`): `fallback_commands_follow_the_review_state` (kind `other`, review missing, no record gives the `--without-review` list) and the existing `park_commands` tests updated to pass `false, None`.

factory-engine integration tests:

- `tests/build_checks.rs` or `tests/gates.rs`: `a_failed_checks_md_write_keeps_the_report` (a `checks.md` directory in the job folder, a failing check: the report is `Ok` with one failure and the ledger names the write error), `checks_failures_file_follows_the_last_run` (written after a failing run, removed after a passing one), `a_failed_checks_md_write_leaves_no_stale_pass` (a passing `checks.md` from an earlier run, then a failing run whose `checks.md` write fails: no `checks.md` with a `PASS` line remains).
- `tests/park_evidence.rs`: `a_failed_checks_md_write_after_sync_parks_as_checks_failed`.
- `tests/minors.rs`: `an_unrecordable_minor_ticket_reports_its_number`, `minors_after_a_sync_include_the_round_review` (round review with `first nit`, sync review with `sync nit` and `first nit`: two checklist lines, body names both files), `minors_include_the_rebase_review`.
- `tests/waivers.rs`: `an_unrecordable_waiver_ticket_reports_its_number_and_stops`.
- `tests/retry.rs`: `retry_refusal_without_park_json_offers_without_review`.
- factory-supervisor: `cargo test -j4 -p factory-supervisor admit` keeps passing with the new arguments.

A record write is made to fail by making the job folder read-only after setup. The test returns early when a probe write in that folder still succeeds (running as root).

## Files changed

- `crates/factory-engine/src/review.rs`, `park.rs`, `checks_stage.rs`, `rework.rs`, `minors.rs`, `waivers.rs`, `retry.rs`
- `crates/factory-core/src/park.rs`
- `crates/factory-supervisor/src/admit.rs`
- `crates/factory-engine/tests/minors.rs`, `waivers.rs`, `retry.rs`, `park_evidence.rs`, `build_checks.rs` or `gates.rs`
- `docs/reference/software-factory.md:46`: add `checks-failures.md` (failing check output tails, read by a rework) to the job directory list

## Constraints

- No new board, ledger or events field. `checks-failures.md` is a new job-folder file written only by the driver.
- No git shell-out; no `/proc` reads; paths come from the job folder the engine already holds.
- The ticket steps never change the merge result.

## Acceptance criteria

- A sync review after sync1 and sync3 is saved as sync4.
- A retry refusal with no `park.json` and a missing review offers `approve --without-review`, in the engine and in factoryd.
- An Engine park whose stage move is refused saves no park field.
- A failed `checks.md` write after failing checks parks as `checks-failed`, and the ledger names the write error.
- A rework after failing checks includes the failing output tail; old jobs still get the `FAIL` lines.
- `checks-failures.md` is gone after a passing run.
- A failed `checks.md` write never leaves a `checks.md` from an earlier run.
- A minor or waiver ticket whose record write fails has its number in the report and the ledger.
- A minor ticket after a sync lists the round, rebase and sync review minors once each and names every file read.
- `cargo test -j4 -p factory-engine`, `-p factory-core` and `-p factory-supervisor` pass.

## Out of scope

- Groups B (#145, #55, #43, #96, #95, #118, #140) and C (#75, #76, #68), which get their own specs.

## Decisions

- How are the 17 size S tickets grouped? Options: three themed specs, one spec with several series, skip the decision tickets, one spec per ticket. Chosen: three themed specs, this one first. Decided by: user
- #25: how does a rework get the failing check output? Options: a new checks-failures.md, tails inside checks.md, re-run the checks. Chosen: a new checks-failures.md written by run_checks_now. Decided by: user
- #50: what happens when checks ran but checks.md cannot be written? Options: keep the report and log the error, park as checks-failed, a new error type. Chosen: keep the report and log the error. Decided by: user
- #18: how is a ticket whose record write failed handled? Options: report the number, a pending marker first, both. Chosen: report the number. Decided by: user
- #20: which reviews does the minor ticket read? Options: round plus syncs, also the rebase review, every round. Chosen: the round, its rebase and its syncs. Decided by: user
- #139: how does the fallback learn the review state? Options: pass review_missing and verdict, pass the Job, read the board inside. Chosen: pass review_missing and verdict. Decided by: user
- How are the fixes packaged? Options: three plans as one job, three plans as a series, one plan per ticket. Chosen: three plans as a series. Decided by: user
- A waiver step whose record write fails stops filing further blockers in that run. Decided by: call
- Read order of review files for minors: round, rebase, syncs ascending, then last_review.file if not yet read. Decided by: call
- `checks-failures.md` gets no extra cap beyond the 60-line tail. Decided by: call
- `run_checks_now` removes the old `checks.md` and `checks-failures.md` before writing, so a failed write leaves no stale evidence. Decided by: call
- A `stage.change` event logged inside a board update whose save then fails is accepted, as in `merge.rs`. Decided by: call
