# Park evidence fixes

Follow-up to `2026-10-31-park-evidence-edges-design.md` (job 044). An audit of that job found three state bugs and a few test gaps. This spec fixes them. Findings come from the audit of 2026-10-31, each checked against the code named below.

## Goal

After a risky sync park, the board reason, the ledger and `park.json` agree with each other, a crash during the park never shows evidence of an earlier park, and the tests would fail if the write order or the unknown-risk text were broken again.

## Defects

| # | Defect | Where | Fix |
|---|---|---|---|
| 1 | `sync_stage` drops the outcome of `park_after_sync` and returns the generic `after sync: <why>` text. `merge_stage` then overwrites `job.reason` with it on a card that says `checks-failed`, writes a second ledger ruling and logs the park twice. | `sync.rs:84-90`, `merge.rs:47-74` | `park_after_sync` returns the reason of the park it wrote, `sync_stage` returns that as `Parked`, and `park_with_parts` writes no ruling and no log line when a park already exists. |
| 2 | Nothing removes `park.json` when a job leaves needs-human. With the new write order, a crash after the stage move and before the record write shows the record of an earlier park. | `park.rs:69-73`, `park.rs:174-186` | The first write of a park removes the old `park.json`. A crash after the stage move then leaves no record, as the earlier spec says. |
| 3 | An error from `park_after_sync` leaves `sync_stage` through `?`, and `merge_stage` parks the job as `merge failed: <error>` with kind `other`, although no merge ran. | `sync.rs:87`, `merge.rs:147-152` | `sync_stage` keeps the risky text, adds the error to it, logs the error and returns `Parked`. The park is still kind `other`, because the board was failing when the card was built. |

## Gaps to close

- `park_with` itself has no ordering test, only the `in_park_order` helper has.
- The unknown-risk text is tested only through `lead_parts_from`. `lead_parts_for_card` and `decision_parts` are not.
- `build.rs:208` still reads the diff with `unwrap_or_default()`, so a builder-blocked park cannot tell "not risky" from "could not check".
- The risky sync park test checks only a title prefix, and the checks test only that a `Checks` title exists. No test makes the final checks fail to run.
- A failed `park.json` write is silent.

## Non-goals

- No change to park kinds, `park.json` fields or board JSON fields.
- The park field update and the stage move stay two board saves. Folding them into one is a separate design.
- `park_with` and `park_with_parts` stay two functions.
- Checks that ran but whose `checks.md` write failed still report "could not run". That is a separate issue.
- No reader change in `factory status` or the dashboard.

## Decisions

- Should an unreadable job diff on a builder-blocked park show a `Risk` part saying so, which changes `crates/factory-engine/src/build.rs`? Options: change it; leave the gap open. Chosen: change it. Decided by: user
- Should the fallback park of defect 3 keep kind `other`? Options: keep `other`; add a `SyncOutcome` variant that carries the kind and the lead parts into `park_with_parts`. Chosen: keep `other`, because the variant costs a merge-path change for a case where the board is already failing. Decided by: user

## Acceptance criteria

- A risky sync park whose final checks fail shows `final checks failed after sync` as the board reason, the `MergeResult::Parked` text and the only ledger ruling, and the record kind is `checks-failed`.
- `park_with` removes the old `park.json` before the board update, so a stage move that fails leaves no record from an earlier park.
- An error from `park_after_sync` leaves `sync_stage` as `Parked` with the risky text and the error named, not as `merge failed`.
- A failed `park.json` write is logged as a warning.
- The builder-blocked park shows a `Risk` part `unknown: the job diff could not be read (<error>)` for an unreadable diff.
- Final checks that cannot run after a sync still park as `risky-diff` with a Checks part that says why.
- `park_with` has a test that fails when its steps are called in the wrong order.
- fmt and clippy `-D warnings` pass for `factory-engine`.
