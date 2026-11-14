# Sync hardening design

## Goal

Make `factory sync <id>` safe in every stage a job can be in: no code reaches a merge without a review of its current head, a stopped sync can be run again without losing what it knew, and the whole-branch review sees only the job's own work.

## Overview

Job 080 added `factory sync <id>` (`crates/factory-engine/src/sync.rs:44`), see `docs/superpowers/specs/2026-11-14-sync-after-rebase-design.md`. A spec audit found it complete against that spec. A separate code and safety review of the merged change found holes in the design itself. All tests start the job in `approve`, so none reach them. The three that matter most were read in the code:

- A rewrite sets `head` and `prev_head` and forces stage `review`, but leaves `reviews` and `last_review` as they were (`sync.rs:68-74`). A park before the rebase review leaves a `needs-human` job whose unreviewed tip is `head` and whose reviews still read Pass, so `approve` can merge it.
- "Rewritten" lives only in a local variable (`sync.rs:66-79`). If the first run stops after the reset, the next run sees `head == tip` and treats it as a plain sync.
- A plain requested sync always sets `prev_head = old head` (`sync.rs:146-148`) as if that head had been reviewed and approved. That is true after an automatic sync, which runs only after approval, but a requested sync runs in `build`, `review` and `needs-human` too.

The rest are smaller: a wrong diff base, stale park evidence, a lost park kind, an overwritten raw review file, a crash window between board and ledger, and a refusal that exits as an engine error.

## Current state

| Piece | Today | Gap |
|---|---|---|
| Rewrite test (`sync.rs:66-67`) | `!is_ancestor(head, tip)` | Commits added by hand on top of `head` are not a rewrite, so they are folded into the sync diff with the code reviewer only |
| Reset (`sync.rs:68-77`) | Sets `head`, `prev_head = None`, stage `review` | Reviews and `last_review` stay, and the ledger is a second write |
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

A requested sync keeps the reviewed floor (`prev_head = old head`) only when the job is in stage `approve` and the branch tip equals `head`. In every other case the head is not known to be reviewed, so the sync is a whole-branch review: `prev_head = None`, `ReviewRun::Rebase`, code and security. The rule applies in `build`, `review` and `needs-human`.

A branch is changed outside the factory when `tip != head`, not only when `head` is not an ancestor of `tip`. Commits added by hand on top of `head` count, so they get the whole-branch review too.

### 2. A durable reset

When the sync takes the whole-branch path, one `store().update_job` does all of: `head = tip`, `prev_head = None`, `reset_reviews()`, `last_review = None`, and the forced stage `review`. A run that stops afterwards leaves the job in `review` with pending reviews, so `approve` refuses it, and the next `factory sync` takes the whole-branch path again because the stage is not `approve`.

The ledger ruling is written first, then the board update, so a crash between them leaves a ruling without a change, not a change without a ruling.

### 3. The whole-branch diff

For a review with no `prev_head`, `review_stage_as` uses `range_start(base, head)`, the merge base with the base branch, instead of `job.base`. The first-round review of a plain job is unchanged: its merge base is `job.base`.

### 4. Park state

- A requested sync that parks a `needs-human` job clears `park_kind`, `park_summary` and `park_at` first, so `merge::park_with_parts` writes a new record, ruling and log line. The `park.json` is replaced.
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
| Job in `review`, `build` or `needs-human` | Whole-branch review, code and security |
| `tip != head` in any stage | Whole-branch review |
| A first sync stops after the reset | Next run: the stage is `review`, so the whole-branch path runs again |
| Sync conflict on a `needs-human` job | New park record, old evidence replaced |
| `ChangesRemain` after a whole-branch review | Parks as `changes-remain` with the `approve --waive` hint |
| Open fix blockers and a requested sync | The whole-branch review is a new verdict and replaces `last_review` |

## Testing

All tests use `.expect("...")`, never the unwrap call. Reuse the fixtures of `crates/factory-engine/tests/sync_review.rs`.

- `a_rewrite_resets_reviews_and_last_review` (engine): after the reset and a park before the review, the reviews are pending, `last_review` is none and `approve` refuses.
- `a_second_sync_after_a_stopped_first_one_still_reviews_the_whole_branch` (engine): stop after the reset, run again, the review covers `merge base..head`.
- `a_plain_sync_keeps_the_floor_only_in_approve_with_tip_equal_head` (engine): the floor is kept there, and is none in `review`, `build` and `needs-human`.
- `commits_added_on_top_of_head_get_the_whole_branch_review` (engine).
- `a_rebase_review_diff_starts_at_the_merge_base` (engine): after main moved, the review diff holds none of the main changes.
- `a_sync_conflict_on_a_needs_human_job_replaces_the_park_record` (engine): `park.json` holds the new conflict and a new ruling exists.
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

- A requested sync outside `approve`, or with `tip != head`, resets the reviews and runs a whole-branch code and security review before anything can merge.
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
- Any `tip != head` counts as a change outside the factory, not only a rewrite. Decided by: call
- The reset also clears `reviews` and `last_review`, so an interrupted sync cannot be approved. Decided by: call
- The whole-branch diff starts at the merge base with the base branch. Decided by: call
