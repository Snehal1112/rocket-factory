# Spec: A cancelled job never reaches review or merge

## Goal

When a job is cancelled, a driver that is still running must not move it to review, approve or merged, and must not create the merge commit. A job in needs-human may leave it only through rework, approve or sync.

## Overview

`factory cancel <id> --force` (also the API, `crates/factory-supervisor/src/api/write.rs:492`) parks the job in needs-human, sets `cancelled = true` and clears the leases (`crates/factory-cli/src/commands.rs:848-885`). A driver that is mid-stage does not look at the board again. When its stage ends it writes its own stage move, and the stage guard allows needs-human to move to every stage except spec (`crates/factory-core/src/stage.rs:57-59`). So the cancel is overwritten: build moves the job to review, and the merge path moves it to merged. The merge is worse: `try_merge` makes the git merge commit and moves `main` before `finish_merge` writes the stage, so main changes for a job that was cancelled.

Evidence: audit of 2026-11-15, issue #86. The stage writes and the missing check were re-read in code for this spec.

## Current state

| Place | What it does today | Gap |
|---|---|---|
| `cancel` (`commands.rs:848-885`) | Sets needs-human, `cancelled = true`, clears the job lease and, with `--force`, the project lease | Nothing makes a live driver notice |
| `Job.cancelled` (`crates/factory-core/src/board.rs:93-95`) | Read by `sync.rs:48`, `approve` (`commands.rs:685`, `api/write.rs:405`) | The engine stage code never reads it |
| `Job::set_stage` (`board.rs:398-420`) | Calls `Stage::check_move`, then updates stage, history and park fields | Ignores `cancelled` |
| `Stage::check_move` (`stage.rs:50-65`) | Needs-human may move to any stage except spec, without force | The driver can move a parked job on |
| Run loop (`crates/factory-engine/src/runloop.rs:699-701`) | Returns on needs-human at the top of each pass | Does not cover a stage already running |
| Stage writes of a running driver | `finish_build` `build.rs:911` (to review), `apply_decision` `review.rs:341` (to approve), `checks_stage.rs:288` (forced, to build), `finish_merge` `merge.rs:378` (to merged) | None checks `cancelled` |
| `try_merge` (`merge.rs:256-366`) | Runs the commit-message agent, re-checks the base, then `merge_no_ff` | No check that the job is still wanted |
| Rework (`commands.rs:796,822`) | Moves needs-human to build, then clears `cancelled` | The order must change, see Design 2 |
| Sync (`sync.rs:48-49,92,178`) | Refuses a cancelled job, forces review | Unchanged |

## Non-goals

- Stopping the agent or check process of a live driver when a job is force-cancelled (a follow-up).
- Closing the window of milliseconds inside the git merge (accepted, see Decisions).
- Changing lease expiry or takeover (issue #87).
- Any new board field.

## Design

Flow: a cancel lands while a stage runs -> the stage tries to write its stage move -> the cancelled job refuses it -> the run ends as needs-human with the cancel reason. A merge checks the board once more right before `merge_no_ff`.

### 1. A cancelled job refuses every move except to needs-human

In `Job::set_stage` (`board.rs:398`), before `Stage::check_move`:

```rust
if self.cancelled && to != Stage::NeedsHuman {
    return Err(CoreError::Cancelled);
}
```

It applies with `force` as well, so the forced move at `checks_stage.rs:288` is covered. The job is left untouched on refusal, as for any refused move. New error variant `CoreError::Cancelled`, message `job is cancelled`.

### 2. Rework clears the flag first

`rework` (`commands.rs:796` and `:822`) moves needs-human to build and then sets `cancelled = false`. Both closures change to set `cancelled = false` first, then call the resume function of Design 4. Approve refuses a cancelled job already (`commands.rs:685`), and sync refuses it (`sync.rs:48`), so neither needs a change for the flag.

### 3. The run ends quietly as needs-human

New `EngineError::Cancelled` (`crates/factory-engine/src/lib.rs:127`, message `job is cancelled`). `From<CoreError> for EngineError` (`compat.rs:17`) maps `CoreError::Cancelled` to it and keeps `Core(String)` for every other variant. `run_job` (`runloop.rs:676`) catches it:

```rust
match span.in_scope(|| run_job_inner(driver, id, &guard)) {
    Err(EngineError::Cancelled) => Ok(RunEnd::NeedsHuman(cancel_reason(driver, id))),
    other => other,
}
```

`cancel_reason` re-reads the job and returns its `reason`, or `cancelled` when it has none. Nothing else is written. The supervisor reports `needs-human: cancelled: <reason>`. The build or review work already done stays in the worktree.

### 4. Needs-human is left only through a named resume

`Stage::check_move` (`stage.rs:50`) refuses every move out of needs-human, including the direct move to merged that it allows today, unless `force` is set. A new `Job::resume(to: Stage, now: f64) -> Result<(), CoreError>` is the only non-forced way out. It is valid only when the stage is needs-human and `to` is build, review or approve; it does the same bookkeeping as `set_stage` (stage, `stage_since`, history, reason and park fields cleared). Callers that switch to it: rework in both branches (`commands.rs:796,822`) and approve (`commands.rs:722`). Sync keeps `force = true` (`sync.rs:92,178`). `set_stage_logged` (`ports.rs:47`) gets a sibling `resume_logged` that logs `stage.change` the same way. Moving a parked job to spec stays refused, as today.

### 5. The merge checks the board right before the git merge

In `try_merge`, after the second `base_state` check and immediately before `merge_no_ff` (`merge.rs:~339`), re-read the job. If `cancelled` is set, return `Err(EngineError::Cancelled)`. The commit-message run before it is the long step; after this check only the git merge itself is left, which is milliseconds. `finish_merge` then refuses a cancelled job through Design 1 as a second guard.

### 6. A second cancel still works

`cancel` calls `set_stage_logged(NeedsHuman)` on an already cancelled job. That move is allowed, so the reason is updated and the command succeeds as today.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| Cancel lands during the builder run | `finish_build` is refused, the run ends needs-human with the cancel reason, no review runs |
| Cancel lands during review or checks | `apply_decision` or the forced move is refused, same ending |
| Cancel lands during the commit-message run | The check in Design 5 stops the job before `merge_no_ff`, main is unchanged |
| Cancel lands after the check, inside the git merge | The merge completes, `finish_merge` is refused, the job stays needs-human and cancelled while main has the merge. Window of milliseconds, documented |
| A stage parks the job after a cancel | The park is a move to needs-human, so it is allowed. It replaces the reason text and may add park evidence; `cancelled` stays true, so approve still refuses |
| Rework on a cancelled job | The flag is cleared first, then the job resumes at build, as today |
| Approve on a cancelled job | Refused as today (`job N was cancelled`) |
| Sync on a cancelled job | Refused as today |
| A job parked by the driver (not cancelled) | Resumes through rework, approve or sync only. Any other move out of needs-human is refused |
| Old board with `cancelled` absent | Deserialises as false (`board.rs:94-95`, serde default), nothing changes |
| `Job::resume` from a stage other than needs-human | `CoreError::StageMove` |
| `Job::resume` to spec or merged | `CoreError::StageMove` |

## Testing

`factory-core`:
- `board.rs`: `a_cancelled_job_refuses_every_move_except_needs_human` (all four live targets, with and without force), `a_refused_cancelled_move_leaves_the_job_untouched`, `a_cancelled_job_can_still_park_again`.
- `board.rs`: `resume_moves_needs_human_to_build_review_or_approve`, `resume_refuses_spec_merged_and_other_stages`.
- `stage.rs`: replace `needs_human_is_a_parking_stage` (lines 107-119, which asserts needs-human may move to build, review, approve and merged) with `needs_human_moves_out_only_by_force`; the move into needs-human and the refusal of spec stay asserted.

`factory-engine` (`crates/factory-engine/tests/`, fixtures `approved_job_with` and `SeriesFixture::owned` with its `RunHook` from `tests/common/mod.rs`):
- `a_cancel_during_the_builder_run_stops_the_job_at_needs_human`: the hook cancels the job through the store during the builder run; the job ends needs-human, never review, and the run returns `RunEnd::NeedsHuman` with the cancel reason.
- `a_cancel_during_review_does_not_reach_approve`.
- `a_cancelled_job_does_not_merge`: set the job cancelled in approve stage, call `merge_stage`; main's tip is unchanged and the result is `Cancelled`.
- `a_cancel_during_the_commit_message_run_leaves_main_unchanged`: the hook cancels during the message agent run.
- `engine_error_cancelled_maps_from_core_error`.
- `sync`, `rework` and `approve` tests that exist keep passing.

`factory-cli`:
- `crates/factory-cli/tests/operator_events.rs` or the rework test file: `rework_clears_cancelled_and_resumes_at_build` (both rework kinds), `approve_still_refuses_a_cancelled_job`, `a_second_cancel_updates_the_reason`.
- Any existing test that moves a job out of needs-human with a plain `set_stage` is changed to `resume` or `force` (grep `NeedsHuman` in the three crates' tests; the `status.rs` tests at lines 167-233 only move into needs-human and stay as they are).

Commands: `cargo test -j4 -p factory-core`, `-p factory-engine`, `-p factory-cli`, `-p factory-supervisor`; then `cargo clippy -j4 -p factory-core -p factory-engine -p factory-cli --all-targets -- -D warnings` and `cargo fmt -p factory-core -p factory-engine -p factory-cli -- --check`.

## Files changed

- `crates/factory-core/src/board.rs`: guard in `set_stage`, `Job::resume`, tests.
- `crates/factory-core/src/stage.rs`: `check_move` refuses needs-human exits without force, tests.
- `crates/factory-core/src/error.rs`: `CoreError::Cancelled`.
- `crates/factory-engine/src/lib.rs`, `compat.rs`: `EngineError::Cancelled` and the mapping.
- `crates/factory-engine/src/runloop.rs`: catch in `run_job`, `cancel_reason`.
- `crates/factory-engine/src/merge.rs`: check before `merge_no_ff`.
- `crates/factory-engine/src/ports.rs`: `resume_logged`, exported in `lib.rs` next to `set_stage_logged`.
- `crates/factory-cli/src/commands.rs`: rework and approve use resume, flag order.
- `docs/reference/software-factory.md` (the stage flow and the `/factory cancel` row), `docs/guides/factory-user-guide.md` (section 6.7 `factory cancel`): the new behaviour and the millisecond window.
- Tests listed above.

## Constraints

- Board JSON stays compatible with `dashboard/`: no new field, `cancelled` keeps its name and default.
- Never shell out to `git`. Use `factory-git` as today.
- Comments are short full sentences. Tests use `.expect("...")`, never the unwrap call.
- `-j4` on every cargo call, no `--workspace`.

## Acceptance criteria

- A cancelled job refuses every stage move except to needs-human, with and without force, and is left untouched when refused.
- A driver that is mid-stage when a job is cancelled ends the run as needs-human with the cancel reason, and the job never reaches review, approve or merged.
- `try_merge` re-reads the job right before `merge_no_ff`; for a cancelled job no merge commit exists and `main` is unchanged.
- Needs-human is left without force only through `Job::resume` to build, review or approve; every other non-forced move out of it is refused.
- Rework (both kinds) clears `cancelled` first and resumes the job at build; approve and sync still refuse a cancelled job; a second cancel succeeds and updates the reason.
- A driver-parked job (not cancelled) resumes through rework, approve and sync as before.
- `docs/reference/software-factory.md` and the user guide describe the behaviour and the millisecond window inside the git merge.
- The suites of `factory-core`, `factory-engine`, `factory-cli` and `factory-supervisor` pass, clippy has no warnings, fmt has no diff.

## Out of scope

- Signalling or killing the driver, its agent or its checks.
- A cancel callback inside `factory-git`, or holding the board lock across the merge.
- Lease expiry and takeover rules.
- The other safety issues of the audit (#87 to #93).

## Follow-ups

- Stop the agent and check processes of a live driver when a job is force-cancelled, so work ends at once and not at the next stage write.

## Decisions

- Where is a cancelled job stopped? Options: a guard in the stage write plus a check before the merge, checks at stage boundaries only, signal the live driver. Chosen: the guard plus the pre-merge check. Decided by: user
- How does a running driver end when its write is refused? Options: a quiet needs-human end, an error exit, ignore and keep looping. Chosen: a quiet needs-human end. Decided by: user
- Is `Stage::check_move` tightened for needs-human? Options: leave it, tighten it with named resume paths. Chosen: tighten it. Decided by: user
- How is the window inside the git merge treated? Options: accept and document, a cancel callback into `factory-git`, hold the board lock across the merge. Chosen: accept and document. Decided by: user
- Is the guard in `Job::set_stage` (core) or only in the engine? Options: core, engine. Chosen: core, because every writer, including the CLI, then obeys it and a test needs no engine fixture. Decided by: call
- Does the guard apply with `force`? Options: yes, no. Chosen: yes, because the forced move in `checks_stage.rs:288` is a driver write that must not revive a cancelled job. Decided by: call
- Is a new board field added? Options: reuse `cancelled`, add a field. Chosen: reuse `cancelled`, so the board JSON does not change. Decided by: call
- Does the pre-merge check go at the top of `try_merge` or just before `merge_no_ff`? Options: top of the loop, just before the merge. Chosen: just before the merge, because the commit-message agent run in between is the long step. Decided by: call
