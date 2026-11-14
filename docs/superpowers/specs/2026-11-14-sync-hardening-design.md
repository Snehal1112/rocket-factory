# Sync hardening design

## Goal

Make `factory sync <id>` safe in every stage a job can be in: no code reaches a merge without a review of its current head, a stopped sync can be run again without losing what it knew, and the whole-branch review sees only the job's own work.

## Overview

Job 080 added `factory sync <id>` (`crates/factory-engine/src/sync.rs:44`), see `docs/superpowers/specs/2026-11-14-sync-after-rebase-design.md`. A spec audit found it complete against that spec. A separate code and safety review of the merged change found holes in the design itself. All tests start the job in `approve`, so none reach them. The three that matter most were read in the code:

- A rewrite sets `head` and `prev_head` and forces stage `review`, but leaves `reviews` as they were (`sync.rs:68-74`). A park before the rebase review leaves a `needs-human` job whose unreviewed tip is `head` and whose reviews still read Pass, so `approve` can merge it.
- "Rewritten" lives only in a local variable (`sync.rs:66-79`). If the first run stops after the reset, the next run sees `head == tip` and treats it as a plain sync.
- A plain requested sync always sets `prev_head = old head` (`sync.rs:146-148`) as if that head had been reviewed and approved. That is true after an automatic sync, which runs only after approval, but a requested sync runs in `build`, `review` and `needs-human` too.

The rest are smaller: a wrong diff base, stale park evidence, a lost park kind, an overwritten raw review file, a crash window between board and ledger, and a refusal that exits as an engine error.

## Current state

| Piece | Today | Gap |
|---|---|---|
| Rewrite test (`sync.rs:66-67`) | `!is_ancestor(head, tip)` | Commits added by hand on top of `head` are not a rewrite, so they are folded into the sync diff with the code reviewer only |
| Reset (`sync.rs:68-77`) | Sets `head`, `prev_head = None`, stage `review` | Reviews stay Pass, and the ledger is a second write |
| Floor on a plain sync (`sync.rs:146-148`) | `prev_head = old head` in any stage | Wrong unless the old head was reviewed and approved |
| Round diff (`review.rs:58`) | With no `prev_head`, diffs from `job.base` | After a rebase or a merge of main the diff holds every main change since the original fork. `range_start` (`checks_stage.rs:149`) already uses the merge base for the whole diff |
| Park of a requested sync (`sync.rs:80-84`, `merge.rs:49-61`) | `merge::park` | A `needs-human` job keeps the old `park_kind`, summary and `park.json` |
| Non-risky review decision (`sync.rs:188-191`) | Returned as text, re-parked as `ParkKind::Other` | Kind, evidence and `next` hints are lost |
| Raw review files (`review.rs:199-202`) | `review-<role>-r<round>.raw.md` | A rebase review overwrites the round's raw files |
| Refusals (`sync.rs:47-64`) | `EngineError::Msg` | Reach the CLI as an engine error, not `CliError::Refused` |

## Non-goals

- Rebasing the branch, or detecting a rebase by patch-id.
- Changing the automatic sync at merge time.
- A dashboard chip for `factory sync`.

## Design

### 1. A reviewed floor only after an approved head

A requested sync decides from three facts: the stage, whether the branch tip equals `head`, and whether the branch is behind the base branch.

| `tip == head` | Behind main | Stage | Result |
|---|---|---|---|
| yes | no | any | `UpToDate`, nothing changes, as today |
| yes | yes | `approve` | Merge main, keep the floor (`prev_head = old head`), code reviewer on the sync diff, as an automatic sync |
| yes | yes | `build`, `review`, `needs-human` | Whole-branch review: merge main, `prev_head = None`, `ReviewRun::Rebase`, code and security |
| no | any | any | Whole-branch review as above |

A branch is changed outside the factory when `tip != head`, not only when `head` is not an ancestor of `tip`. Commits added by hand on top of `head` count, so they get the whole-branch review too. A job in `build` that is behind main gets the whole-branch review of its current work, and a pending `batch_snapshot` refuses the sync.

### 2. A durable reset

When the sync takes the whole-branch path, one `store().update_job` does all of: `head = tip`, `prev_head = None`, `reset_reviews()` and the forced stage `review`. `last_review` is kept until the new review writes its verdict, so open blockers still refuse `approve`, also with `--without-review` (`board.rs:296-344`). A run that stops afterwards leaves the job in `review` with pending reviews, so `approve` refuses it, and the next `factory sync` takes the whole-branch path again because the stage is not `approve`.

The ledger ruling is written first, then the board update, so a crash between them leaves a ruling without a change, not a change without a ruling.

### 2a. A passing review of a parked job does not release it

When the sync started from `needs-human` and the whole-branch review passes, the job goes back to `needs-human` with the reason `sync review passed, run factory approve <id>`. The review result and `last_review` are recorded, so a plain `factory approve <id>` then passes. The job was parked for a reason the sync review does not recheck (the diff cap, the budget, the fix rounds), so a person still releases it. A job that started in `approve`, `review` or `build` follows the usual decision.

### 3. The whole-branch diff

For a review with no `prev_head`, `review_stage_as` uses `range_start(base, head)`, the merge base with the base branch, instead of `job.base`. The first-round review of a plain job is unchanged: its merge base is `job.base`.

### 4. Park state

- A job in `needs-human` always takes the whole-branch path, whose forced move to stage `review` already clears the old park fields (`set_stage`, `board.rs:416-419`). A park written later by `sync_with` (the checks gate, a risky diff, a conflict) is therefore a new record with its own kind and evidence, and `sync_requested` never clears park fields after `sync_with` returns.
- A non-risky `Decision::NeedsHuman` keeps its `ParkKind`, evidence and `next` list: `sync_with` returns the kind with the reason (`SyncOutcome::Parked` carries an optional `ParkKind`), and `sync_requested` parks with it.
- A pending `batch_snapshot` is refused like `approve` refuses it.

### 5. Small fixes

- The multi-role raw review files get the run suffix: `review-<role>-r<round>-rebase.raw.md` for a rebase review and `-sync<n>` for a sync review.
- The refusals of `sync_requested` return `CliError::Refused`, so the exit code and message form match `approve` and `rework`.
- The user guide heading `6.6a` is renumbered to fit the section order.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| Job in `approve`, `tip == head`, behind main | Merge main, floor `old head`, code reviewer on the sync diff, as an automatic sync |
| Job in `review`, `build` or `needs-human`, up to date, `tip == head` | `UpToDate`, nothing changes |
| Job in `review`, `build` or `needs-human`, behind main | Whole-branch review, code and security |
| Job in `build` behind main | Whole-branch review of the current work, a pending `batch_snapshot` refuses |
| Whole-branch review passes on a job that started in `needs-human` | Back to `needs-human`, reason `sync review passed, run factory approve <id>`, plain `approve` then passes |
| `tip != head` in any stage | Whole-branch review |
| A first sync stops after the reset | Next run: the stage is `review`, so the whole-branch path runs again |
| Sync conflict, or a checks gate failure, on a `needs-human` job | The forced move to `review` cleared the old park, so the new park keeps its own kind and evidence |
| `ChangesRemain` after a whole-branch review | Parks as `changes-remain` with the `approve --waive` hint |
| Open fix blockers and a requested sync | The whole-branch review is a new verdict and replaces `last_review` |

## Testing

All tests use `.expect("...")`, never the unwrap call. Reuse the fixtures of `crates/factory-engine/tests/sync_review.rs`.

- `a_rewrite_resets_the_review_states_and_keeps_last_review` (engine): after the reset and a park before the review, the reviews are pending, `last_review` is unchanged and `approve` refuses. A second case with `last_review` at `changes` shows that `approve --without-review` is still refused.
- `a_second_sync_after_a_stopped_first_one_still_reviews_the_whole_branch` (engine): stop after the reset, run again, the review covers `merge base..head`.
- `a_plain_sync_keeps_the_floor_only_in_approve_with_tip_equal_head` (engine): behind main with `tip == head`, the floor is kept in `approve`, and is none in `review`, `build` and `needs-human`.
- `an_up_to_date_job_with_tip_equal_head_changes_nothing_in_every_stage` (engine): `UpToDate` in `approve`, `review`, `build` and `needs-human`, with no stage change and no agent run.
- `a_passing_whole_branch_review_re_parks_a_needs_human_job` (engine): the job ends in `needs-human` with the exact reason, and plain `approve` then passes.
- `commits_added_on_top_of_head_get_the_whole_branch_review` (engine).
- `a_rebase_review_diff_starts_at_the_merge_base` (engine): after main moved, the review diff holds none of the main changes.
- `a_sync_conflict_on_a_needs_human_job_replaces_the_park_record` (engine): `park.json` holds the new conflict and a new ruling exists.
- `a_checks_gate_failure_during_a_sync_keeps_its_own_park_kind` (engine): on a `needs-human` job the gate park keeps its `NoChecks` kind and evidence in `park.json`.
- `a_changes_remain_decision_keeps_its_park_kind_after_a_sync` (engine).
- `sync_refuses_a_pending_batch_snapshot` (engine).
- `a_rebase_review_keeps_the_round_raw_files` (engine): both the round and the rebase raw files exist.
- `sync_refusals_are_cli_refused` (`factory-cli` test): a merged job gives `CliError::Refused`.
- `a_sync_runs_the_checks` (engine): closes the audit gap, a requested sync runs the checks before the review.
- `sync_refuses_a_job_with_no_worktree` (engine): closes the audit gap.

## Files changed

- `crates/factory-engine/src/sync.rs`, `review.rs`, `merge.rs`, `park.rs`.
- `crates/factory-cli/src/commands.rs` for the refusal mapping.
- `crates/factory-engine/tests/sync_review.rs` and `crates/factory-cli/tests/sync_cmd.rs`.
- `docs/guides/factory-user-guide.md` and `docs/reference/software-factory.md`.

## Constraints

- `-j4`, never `--workspace` or `--all`, never `CARGO_TARGET_DIR`.
- Never shell out to `git`. Do not read `/proc`. Paths reach the driver only through `Layout`.
- No board, ledger or events field is added. Board JSON stays compatible with `factory-server`.
- The driver never rewrites history.
- Code comments are short full sentences.

## Acceptance criteria

- A requested sync that is behind main outside `approve`, or with `tip != head`, resets the review states, keeps `last_review`, and runs a whole-branch code and security review before anything can merge.
- A requested sync on an up-to-date job with `tip == head` changes nothing in any stage.
- A whole-branch review that passes on a job that started in `needs-human` leaves it in `needs-human` until `factory approve`.
- A sync that stops after the reset can be run again and still reviews the whole branch, and `approve` refuses in between.
- The whole-branch review diff starts at the merge base, not the recorded base.
- A park after a requested sync replaces the old park record, and a non-risky decision keeps its kind and hints.
- The refusals exit as `CliError::Refused`, and the raw review files of earlier rounds are kept.
- The two audit test gaps are closed.

## Out of scope

- Everything in the Non-goals.

## Follow-ups

- Match a rebased branch to the reviewed commits by patch-id, so a rebase needs no whole-branch review.

## Decisions

- A requested sync keeps the reviewed floor only in `approve` with `tip == head`, and every other case gets a whole-branch review. Options: also keep it in `needs-human` after an approve, or refuse a sync outside `approve`. Chosen: only `approve` with `tip == head`. Decided by: user
- Any `tip != head` counts as a change outside the factory, not only a rewrite, and gets the whole-branch code and security review. Decided by: user
- An up-to-date job with `tip == head` stays `UpToDate` in every stage. Decided by: user
- A passing whole-branch review of a job that started in `needs-human` re-parks it, so a person runs `factory approve`. Decided by: user
- The reset clears `reviews` only and keeps `last_review`, so an interrupted sync cannot be approved and open blockers keep refusing `approve`. `--without-review` stays the user's explicit override. Decided by: user
- The whole-branch diff starts at the merge base with the base branch. Decided by: user
