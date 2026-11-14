# Sync a job on request design

## Goal

Add `factory sync <id>`, so a person can bring a job up to date with the base branch at any time, and recover a job whose branch was rewritten outside the factory, without losing the code and security review.

## Overview

GitHub issue #77. Job 077 hit a dead end.

- The merge parked with `main busy` after the round-2 review approved.
- The user rebased the job worktree onto main by hand and ran `factory rework`. The builder had nothing to fix and the job parked as `nothing-changed`.
- A second rework made a real fix commit, and the driver parked it as `commit-failed: committer rewrote history at or below the floor`. The floor is the last reviewed head. The rebase rewrote every hash, so the floor is no longer on the branch.
- `approve` refuses (no review of the current state), and `rework` and `run` only hand the job back to a builder. Every way out drops the review, drops the work, or needs an artificial change.

The driver already has the right flow for a branch that is behind main: `sync_stage` (`crates/factory-engine/src/sync.rs:14`) merges main into the job branch, reruns the checks and reviews the sync diff. Only the merge step calls it. A feature that falls behind main while other developers merge is normal, so a person must be able to start it.

## Current state

| Piece | Today | Gap |
|---|---|---|
| `sync_stage` (`sync.rs:14-101`) | Refuses after `MAX_SYNCS` (2, `merge.rs:10`), merges main into the branch, sets `prev_head = old head`, `head = merge sha`, `syncs += 1`, forces stage `review`, runs checks, then `review_sync` | Only the merge step calls it (`merge.rs:259`) |
| Round diff (`review.rs:56`) | The reviewer sees `prev_head..head`, or `base..head` when `prev_head` is none. Risk and size judge the whole job diff | None: a reset floor gives a whole-branch review |
| Floor check (`commit.rs:378-381`) | Parks with `committer rewrote history at or below the floor` when the floor is not an ancestor of the tip | The text blames the committer when the branch was rewritten before the run |
| No-op fix round (`build.rs:195-207`) | Parks as `nothing-changed` with next commands `rework` and `cancel` | No hint that the branch was rewritten, no way to review the current head |
| CLI (`cli.rs`, `commands.rs`) | `run` takes the project lease, the job lease and a guard (`commands.rs:612-627`) | No command for a sync |

## Non-goals

- Rebasing the branch. The driver never rewrites history and merges only the reviewed sha, so a sync merges main in.
- Detecting a rebase by patch-id and moving the floor on its own.
- Changing the automatic sync at merge time or its `MAX_SYNCS` limit.
- A dashboard button or copy chip for `factory sync`.

## Design

```
factory sync <id>
  refuse: merged or cancelled job, no worktree, dirty worktree, live lease
  rewritten? (job.head is not an ancestor of the branch tip)
     yes: head = tip, prev_head = none, ruling "branch rewritten outside the factory"
  behind main? (base branch tip not an ancestor of the branch tip)
     yes: merge main into the branch (conflict parks, paths named)
  neither: print "job <id> is up to date with <base>", change nothing
  otherwise: stage review (forced, logged), checks, code and security review, then the usual decision
```

### 1. The command

- `Command::Sync { id: String }` in `crates/factory-cli/src/cli.rs`, with the doc comment "Merge the base branch into a job, reset a rewritten branch, then rerun the checks and reviews".
- `fn sync(ctx: &CliCtx, id: &str) -> Result<String, CliError>` in `commands.rs`. It follows `run`: `ctx.runnable()`, the driver, `hold_project`, `renew_lease` and a `LeaseGuard`, so a job with a live lease is refused the same way. It calls `factory_engine::sync_requested(&d, id)` and prints one line from the outcome, through `outcome_lines` like `run`.
- Allowed in the stages `build`, `review`, `approve` and `needs-human`. A `merged` or cancelled job is refused with `job <id> is <state>`. A worktree with uncommitted changes is refused with `the worktree of job <id> has uncommitted changes`.

### 2. The engine

- `pub fn sync_requested(driver: &Driver, id: &str) -> Result<SyncOutcome, EngineError>` in `sync.rs`. `sync_stage` becomes `sync_with(driver, id, SyncKind)` with `enum SyncKind { Auto, Requested }`, and `sync_stage` calls it with `Auto`. The merge, checks and review steps stay shared.
- `Requested` skips the `MAX_SYNCS` check and does not add to `job.syncs`. `Auto` is unchanged.
- Rewrite detection: `!repo.is_ancestor(job.head, tip(job.branch))`. When true, one `store().update` sets `head = tip`, `prev_head = None` and forces stage `review`, and the ledger gets the ruling `branch rewritten outside the factory, floor reset to base, head is <sha>`. The review then covers `base..head`, the whole branch.
- When the branch is behind the base branch, the existing merge path runs. `MergeOutcome::UpToDate` with no rewrite returns early with no stage change and no review.
- The review of a reset floor is saved as `review-code-r<round>-rebase.md`, so the earlier round reviews are kept. A new `ReviewRun::Rebase` variant carries the file name.
- Every other step (checks, the code and security reviewers, `Decision`, `park_after_sync`) is the code `sync_stage` runs today.

### 3. A clear park for a rewritten branch

- In `verify_commit_run` (`commit.rs:378`), when the floor is not an ancestor of the tip, check the floor against the tip before the committer ran. If it was already not an ancestor then, park with `the branch no longer holds the reviewed head <sha7>; it was rewritten outside the factory: run factory sync <id>`. Otherwise keep the current text.
- In the `nothing-changed` park (`build.rs:207`) and its `park.json`, add `factory sync <id>` to `next` when the reviewed head is not an ancestor of the tip. `factory status` prints it from `park.json`.
- The `main busy` park text says that plain `factory approve <id>` retries the merge once main is clean, when the last review still matches the head.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| Job already up to date, branch not rewritten | Prints `job <id> is up to date with <base>`, no review, no cost |
| Merge conflict | Parks with the paths named, as `sync_stage` does |
| Job holds a live lease | Refused by `hold_project` and `renew_lease` |
| Dirty worktree | Refused before any change |
| Branch rewritten and also behind main | One run: reset the floor, merge main, then one whole-branch review |
| Tip rewritten into something that drops a reviewed commit | The whole-branch review sees the real diff, and risk and size judge it |
| `sync` on a job in a series | Only that job changes, later plan jobs are untouched |
| Requested sync after two automatic syncs | Runs, `syncs` stays at 2 |

## Testing

All tests use `.expect("...")`, never the unwrap call. Reuse the fixtures of `crates/factory-engine/tests/sync_review.rs` (a job behind main) and `tests/common`.

- `sync_requested_merges_main_into_a_job_that_is_behind` (engine): the job gets a merge commit, goes back to `review` and runs checks and review.
- `sync_requested_does_not_count_against_max_syncs` (engine): with `syncs = 2` it still runs and `syncs` stays 2.
- `sync_requested_resets_the_floor_after_a_rebase_and_reviews_the_whole_branch` (engine): rewrite the branch with `git2`, the job ends in `review` with `prev_head` none, and the review diff covers `base..head` and is saved as `review-code-r<round>-rebase.md`.
- `sync_requested_on_an_up_to_date_job_changes_nothing` (engine): no stage change, no agent run.
- `sync_requested_refuses_a_merged_job`, `..._a_dirty_worktree` (engine).
- `sync_requested_conflict_parks_and_names_the_paths` (engine).
- `a_branch_rewritten_before_the_committer_parks_with_the_sync_hint` (engine, `commit.rs` area): the text holds `factory sync` and not `committer rewrote`.
- `a_committer_that_rewrites_history_keeps_the_old_text` (engine): the existing case still gives `committer rewrote history at or below the floor`.
- `a_nothing_changed_park_after_a_rewrite_lists_sync` (engine): `park.json` `next` holds `factory sync <id>`.
- `sync_parses_one_id` (`cli.rs` unit) and `sync_refuses_a_job_with_a_live_lease` (`factory-cli` test).

## Files changed

- `crates/factory-engine/src/sync.rs`, `commit.rs`, `build.rs`, `review.rs`, `merge.rs` (the park text), `lib.rs` (export `sync_requested`).
- `crates/factory-cli/src/cli.rs`, `commands.rs`, `main.rs` if the command table needs a line.
- `crates/factory-engine/tests/sync_review.rs`, `crates/factory-cli/tests/` for the CLI cases.
- `docs/reference/software-factory.md` and `docs/guides/factory-user-guide.md`.

## Constraints

- `-j4`, never `--workspace` or `--all`, never `CARGO_TARGET_DIR`.
- Never shell out to `git`. Do not read `/proc`. Paths reach the driver only through `Layout`.
- No board, ledger or events field is added. Board JSON stays compatible with `factory-server`.
- The driver never rewrites history: a sync merges main in and never rebases.
- Code comments are short full sentences.

## Acceptance criteria

- `factory sync <id>` merges main into a job branch that is behind and runs checks and both reviews on the result.
- After a manual rebase, `factory sync <id>` resets the floor and the job gets a whole-branch code and security review before any merge.
- A requested sync is not limited by `MAX_SYNCS` and does not change `syncs`.
- A merged, cancelled, dirty or leased job is refused with a clear message.
- A rewritten branch parks with a message that names `factory sync <id>`, and the old committer text stays for a real committer rewrite.
- `factory status` lists `factory sync <id>` for a `nothing-changed` park after a rewrite.
- Both docs describe the command.

## Out of scope

- Rebasing the job branch.
- Automatic recovery without a command.

## Follow-ups

- A dashboard copy chip for `factory sync <id>`.
- Match a rebased branch to the reviewed commits by patch-id, so a rebase needs no whole-branch review.

## Decisions

- How to recover a rewritten branch: an explicit `factory sync` that resets the floor to the base for a whole-branch review, not a patch-id match and not a refusal. Decided by: user
- `factory sync` runs in any open stage with no live lease, not only in `needs-human`. Decided by: user
- A requested sync does not count against `MAX_SYNCS`, only automatic syncs do. Decided by: user
- A sync merges main into the branch and never rebases, so the driver's no-rewrite rule stays. Decided by: call
- The whole-branch review is saved under a new name, so earlier reviews are kept. Decided by: call
- The `main busy` text is improved in the same change, since it is the same dead end. Decided by: call
