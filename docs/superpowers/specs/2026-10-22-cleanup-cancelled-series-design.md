## Goal

Let cleanup remove a cancelled job card after its series has fully finished, while preserving cards needed by active work.

## Overview

Issue #13 reports that `factory cleanup <id>` keeps cancelled job cards whenever any sibling remains on the board. The current rule treats merged and cancelled siblings as live forever, so a finished series cannot be cleared. The issue also reports that `factory status` displays cancelled jobs as `needs-human`. The ticket asks that cancelled cards remain protected while series jobs are active, queued, or waiting on a lease, and asks that the dashboard display be checked.

The change makes cleanup determine whether the job's full series is finished. A series is finished only when every member on the board is merged or cancelled. A cancelled member's card can then be removed if there is no live per-job lease, project lease, or queued request for a series member. If a guard prevents removal, the card remains and a later cleanup attempt can remove it after the guard clears. The job folder remains available for ID allocation and usage accounting.

## Current state

- `crates/factory-cli/src/commands.rs:682-700` permits cleanup only for merged or cancelled jobs, removes checkout data, and marks the job cleaned.
- `crates/factory-cli/src/commands.rs:705-732` removes cancelled cards only after checking the project lease and the job lease. It then calls `Board::series_with_siblings`; any sibling still present causes the card to stay.
- `crates/factory-core/src/board.rs:371-381` defines a sibling as another board job with the same series ID. It does not inspect stage or cancellation state.
- `crates/factory-cli/src/status.rs:129-150` prints `job.stage` for every row. Cancellation currently stores `NeedsHuman`, so the table hides the cancellation state.
- `crates/factory-cli/tests/new_to_merged.rs:509-590` covers cancelled-card removal, merged-card retention, any-sibling retention, and live-lease retention. It does not cover a finished series or a queued request.
- `crates/factory-supervisor/src/request.rs:23-42` defines queued actions and their project and job fields. A run request may target a series; an approve request targets one job.
- `crates/factory-supervisor/src/queue.rs:109-142` lists valid requests from the factory home. Invalid queue entries are returned as `QueueItem::Bad` and do not prevent listing valid requests.
- `crates/factory-cli/src/main.rs:56-75` builds a project context with the factory home for `--project`; direct repository contexts do not carry a registered project name or factory home.
- `dashboard/src/specs.rs:34-56` already maps cancelled plan rows to a cancelled state. The cleanup change should be reflected by the cancelled card row disappearing after safe cleanup; no dashboard state-model redesign is required.

The gap is that board presence is being used as a proxy for active work. Finished siblings remain on the board by design, so that proxy blocks cleanup permanently. The CLI status row also needs to prefer the cancellation marker over its stored `needs-human` stage.

## Non-goals

- Do not remove merged job cards.
- Do not remove a job folder, ledger, events, or usage data when a cancelled card is removed.
- Do not cancel, delete, or otherwise mutate queued requests as part of cleanup.
- Do not automatically retry cleanup when a lease or queue request clears; the operator runs cleanup again.
- Do not change the dashboard's existing cancelled-row state representation or introduce a new aggregate series status.
- Do not change series assignment, job execution, cancellation, or merge behavior.

## Design

Flow sketch: `cleanup(cancelled job)` → remove checkout debris and mark cleaned → inspect the board series → retain the card if any member is not merged/cancelled → retain it if a series member has a live lease or matching queued request → otherwise remove the cancelled job card. Print the reason when retaining it. `factory status` renders a cancelled job as `cancelled` regardless of its stored stage.

### 1. Finished-series eligibility

- Keep the existing public CLI shape: `cleanup(ctx: &CliCtx, id: &str, discard: bool)`.
- Replace the any-sibling guard in `remove_cancelled_card` with a series-state check under the board update lock. The target's series is the set of board jobs whose `series.id` matches the target's non-empty series ID. A series is finished only when every member has `stage == Stage::Merged || cancelled == true`.
- Implement the board predicate as `fn series_is_finished(board: &Board, target_id: &str) -> bool`. It returns true for a missing series ID or a series with one remaining board member; otherwise it checks every member sharing the target's series ID.
- A job without a series ID, a single-plan series, or a series with no other board member is treated as having no remaining sibling constraint. Existing single cancelled-job cleanup continues to remove the card.
- If any sibling is neither merged nor cancelled, retain the target card and report `series <id> still has active jobs`.
- A cancelled sibling's stored stage is not used to classify it as active. A merged sibling counts as finished even though its card remains on the board.
- The check and removal happen in the existing `BoardStore::update` closure so membership and eligibility are read from the same board snapshot used for deletion.
- Example: jobs 001 (cancelled), 002 (merged), and 003 (cancelled) in S1 allow cleanup of 001; jobs 001 (cancelled) and 002 (review) in S1 retain 001.

### 2. Lease and queue guards

- Keep the existing per-job lease and project lease protections. A live target-job lease, or a project lease held by the target, retains the card. A stale project lease owned by the target is released using the current behavior before removal checks continue.
- For registered project contexts, expose the `Home` and project name to cancelled-card cleanup so it can inspect supervisor requests through `factory_supervisor::queue::try_list`. Add the CLI dependency on `factory-supervisor`; the supervisor does not depend on the CLI.
- Add `home: Option<Home>` and `project_name: Option<String>` to `CliCtx`. `CliCtx::for_project` sets both values; `CliCtx::new`, `with_layout`, and `from_env` set both to `None`. This keeps existing test and repository-local context construction explicit.
- Add `queue::try_list(home: &Home) -> Result<Vec<QueueItem>, QueueError>`. A missing queue directory returns an empty list; other directory or entry-read failures return `QueueError::Core`. Keep `queue::list`'s existing best-effort behavior for its current supervisor and dashboard callers.
- A valid queued request blocks card removal when `request.project` matches the current registered project and `request.job` names any board member in the target's series. Both `Run` and `Approve` actions count. Requests for other projects or jobs do not count. A `series` run request is matched by its explicit job field; it does not protect unrelated series.
- If a matching request exists, retain the target card and report `series <id> has queued requests` (for a job without a series ID, report `job <id> has a queued request`). Do not remove queue files.
- When no registered project context is available, preserve current repository-local behavior and evaluate only board and lease guards; there is no reliable project name with which to filter a shared factory queue.
- If reading the queue directory fails for a registered project, `try_list` returns an error; fail closed by keeping the card and report that queued work could not be checked. A malformed request that the queue reader reports as `QueueItem::Bad` is ignored because it cannot be safely attributed to this project and job.
- After a lease or queued request clears, another explicit cleanup attempt repeats these checks and removes the card if the series is finished.

### 3. Cancelled CLI status

- Keep `Job.stage` unchanged in the board and ledger. In `render_jobs`, render the stage text as `cancelled` whenever `job.cancelled` is true; otherwise keep `stage_name(job.stage)`.
- This changes only the status table's displayed stage. Other stage-based behavior and dashboard serialization remain unchanged.
- Example: a cancelled job stored with `Stage::NeedsHuman` appears in `factory status` as `cancelled`.

### 4. Dashboard display

- Do not add a dashboard-specific cleanup path. The dashboard already maps cancelled rows to cancelled state, and the board update performed by cleanup removes the same job row consumed by the dashboard.
- Verify the existing UI shows a cancelled job before cleanup and no longer shows that card after cleanup removes it. When cleanup is blocked, the cancelled row remains visible.

## Error handling and edge cases

| Case | Behaviour |
| --- | --- |
| Cancelled job has no series or no other series member | Remove its card after existing lease checks and queue check. |
| Every series member is merged or cancelled | The series-state guard passes; continue to lease and queue guards. |
| A sibling is queued, running, in review, needs-human, or otherwise neither merged nor cancelled | Keep the cancelled card and explain that the series still has active jobs. |
| A sibling is cancelled but stores `needs-human` | Treat it as finished because `cancelled` is true. |
| A sibling is merged | Treat it as finished even though its card remains. |
| Target job has a live job lease | Keep the card with the existing live-lease explanation. |
| Project lease is held by the target job | Keep the card with the existing project-lease explanation. |
| Project lease for the target is stale | Release it as today, then continue eligibility checks. |
| Matching valid queue request exists | Keep the card and report the queued-request guard. |
| Queue request belongs to another project or unrelated job | Ignore it. |
| Queue directory cannot be read for a registered project | Keep the card and report that queued work could not be checked. |
| Queue contains malformed entries | Ignore malformed entries; continue using valid entries returned by the queue reader. |
| Cleanup is run again after active jobs and requests finish | Re-evaluate current state and remove the card if all guards pass. |
| Merged job is cleaned | Preserve its card, as today. |

## Testing

- `factory-cli` unit tests in `crates/factory-cli/src/status.rs`:
  - `status_shows_cancelled_instead_of_needs_human` asserts a cancelled job stored at `NeedsHuman` prints `cancelled` and not `needs-human`.
  - Existing `status_shows_stage_round_reviews_tokens_and_series_position` still asserts an ordinary job prints its actual stage and fields.
- `factory-cli` integration tests in `crates/factory-cli/tests/new_to_merged.rs`:
  - `cleanup_removes_cancelled_card_when_all_series_jobs_are_merged_or_cancelled` asserts a cancelled target card is removed when its siblings are merged or cancelled, while merged cards and job folders remain.
  - `cleanup_keeps_cancelled_card_when_series_has_active_sibling` asserts the cancelled card remains and output names the active-series reason when a sibling is queued, running, needs-human, or in review; exercise representative queued and review states.
  - `cleanup_keeps_cancelled_card_when_series_member_has_live_lease` asserts a live sibling lease preserves the card.
  - `cleanup_keeps_cancelled_card_when_series_member_has_queued_request` asserts a valid matching factoryd request preserves the card.
  - `cleanup_ignores_unrelated_project_and_job_queue_requests` asserts requests for a different project or unrelated job do not block removal.
  - `cleanup_fails_closed_when_registered_project_queue_is_unreadable` asserts the card remains with a queue-check explanation.
  - `cleanup_can_remove_card_when_retried_after_queue_request_finishes` removes the request, retries cleanup, and asserts the card is removed.
  - Retain and update `cleanup_keeps_the_card_of_a_cancelled_series_member_with_a_sibling` to assert that a live sibling blocks, rather than any sibling.
  - Keep `cleanup_of_a_cancelled_job_removes_its_card_and_keeps_the_job_folder`, `cleanup_of_a_merged_job_keeps_its_card`, and `cleanup_keeps_the_card_of_a_cancelled_job_with_a_live_lease` passing.
- `factory-supervisor` unit tests in `crates/factory-supervisor/src/queue.rs`:
  - `try_list_returns_empty_for_missing_queue_directory` asserts a new home with no queue directory yields `Ok(vec![])`.
  - `try_list_returns_bad_items_for_malformed_requests` asserts malformed JSON is represented as `QueueItem::Bad` while valid requests remain available.
  - `try_list_reports_queue_directory_read_errors` asserts a non-NotFound directory read error returns `Err(QueueError::Core(_))`.
- `factory-cli` queue tests use the existing `Request::new`, `queue::enqueue`, and `queue::remove` APIs; do not hand-write a different queue format.
- Dashboard verification: `dashboard/package.json` defines no test script. Inspect the existing dashboard row rendering and verify manually that a cancelled row remains when cleanup is blocked and disappears after cleanup succeeds. Do not change dashboard code unless verification demonstrates a regression.
- Commands: `cargo test -p factory-cli` and `cargo test -p factory-supervisor`.

## Files changed

- `crates/factory-cli/Cargo.toml` — use the supervisor queue API for registered-project cleanup checks.
- `crates/factory-cli/src/commands.rs` — determine series completion and guard cancelled-card removal with queued requests.
- `crates/factory-cli/src/main.rs` and `crates/factory-cli/src/commands.rs` — carry registered project identity and factory home into `CliCtx` without changing repository-local contexts.
- `crates/factory-cli/src/status.rs` — render cancellation state in the status table.
- `crates/factory-cli/tests/new_to_merged.rs` — cover finished/active series, queue and lease guards, and cleanup retry.
- `crates/factory-supervisor/src/queue.rs` — add an error-reporting queue listing API for cleanup while preserving existing best-effort callers.
- `dashboard/src/specs.rs` or dashboard tests only if verification finds that successful cleanup leaves a cancelled row visible.

## Constraints

- Cleanup of a cancelled job never deletes the job directory, events, ledger, or usage history.
- Merged job cards remain on the board.
- A cancelled card is removable only when every series member is merged or cancelled, there is no live lease protecting the relevant work, and no matching queued request protects a series member.
- Queue matching is scoped by exact registered project name and exact job ID.
- A blocked cleanup attempt does not remove requests or automatically retry later.
- Existing repository-local CLI and test contexts remain usable without a registered project or factory home.
- A missing queue directory is an empty queue; other queue directory read errors block removal only for registered project contexts.
- Status output changes only for jobs whose `cancelled` flag is true.

## Acceptance criteria

- `factory cleanup <id>` removes a cancelled card in a multi-job series when all sibling jobs are merged or cancelled and all lease and queue guards pass.
- Cleanup retains a cancelled card while any sibling is not merged or cancelled, and its output identifies that the series still has active jobs.
- Cleanup retains a cancelled card while a member of its series has a live lease.
- Cleanup retains a cancelled card while a matching valid queued run or approve request exists for any member of the series.
- Queue requests for another project or an unrelated job do not prevent cleanup.
- A registered project's unreadable queue directory causes cleanup to retain the card and report that queued work could not be checked.
- A missing queue directory does not block cleanup, and malformed queue files do not block it unless a valid matching request is also present.
- After a guard clears, a later cleanup command removes the cancelled card without deleting the job folder.
- Merged job cleanup continues to keep the merged card.
- `factory status` prints `cancelled` for a cancelled job even when its stored stage is `needs-human`.
- Dashboard presentation shows cancelled state before cleanup, retains the row while cleanup is blocked, and drops it after successful cleanup.

## Out of scope

- Automatic cleanup after a job, lease, or supervisor request finishes.
- Removing merged cards or deleting historical job data.
- Changing status semantics for non-cancelled jobs.
- Redesigning dashboard card aggregation or queue management.

## Decisions

- When should cleanup remove a cancelled card if a sibling is still active? Options: require cleanup again after it finishes; remove the card immediately; remove it automatically when it finishes. Chosen: require cleanup again after it finishes. Decided by: user
- When is a series finished? Options: all jobs merged or cancelled; no live runs or leases; all jobs merged. Chosen: all jobs merged or cancelled. Decided by: user
- What should happen while a series member has a queued request? Options: keep the card until queued requests finish; treat cancelled jobs as done despite queued requests; remove queued requests during cleanup. Chosen: keep the card until queued requests finish. Decided by: user
- What should repository-local contexts do when there is no registered project identity for queue filtering? Options: preserve current board-and-lease behavior; refuse cleanup; infer a project from the repository path. Chosen: preserve current board-and-lease behavior. Decided by: call
- How should malformed queue entries affect cleanup? Options: ignore entries that cannot be attributed; block all cleanup; delete malformed entries. Chosen: ignore entries that cannot be attributed and consider only valid matching requests. Decided by: call

