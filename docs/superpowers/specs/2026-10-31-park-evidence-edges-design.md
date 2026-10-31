# Park evidence edge cases

Ticket: https://github.com/Snehal1112/rocket-factory/issues/21

## Goal

Make the park card of a needs-human job complete and truthful when the park comes from the sync path, when the job diff cannot be read, when the ledger cannot be read, and when the driver stops between its two writes.

## Overview

A parked job shows a card built from `park.json` (`docs/reference/software-factory.md:43`). Four edge cases from the act-on-review-findings series (PR #17) leave that card thin, misleading or out of step with the board:

- **Sync path.** A risky diff that is parked after a sync gets only a "Why it stopped" part, with kind `other`, and the final checks never run.
- **Unreadable diff.** When the job diff cannot be read, the card shows no Risk part, so "not risky" and "could not check" look the same.
- **Unreadable ledger.** `park_unsettled` writes an empty `Ledger` part when `progress.md` cannot be read.
- **Write order.** `park_with` writes `park.json` before it moves the board, so a crash between the two leaves a record for a job that is not parked.

## Current state

| Piece | Today | Reference |
|---|---|---|
| Review park with evidence | `apply_decision` runs `final_gate` for a risky approve, then `decision_parts` and `park_with` | `review.rs:226-238` |
| Sync decision | `sync_stage` returns `SyncOutcome::Parked("after sync: <why>")` for a needs-human decision, with no checks and no evidence | `sync.rs:77-87` |
| Sync park write | `merge_stage` turns the outcome into `Step::plain`, a park of kind `Other` with no parts | `merge.rs:23-28,263-267` |
| Earlier park kept | `park_with_parts` keeps a park already written (stage needs-human and a kind set) | `merge.rs:46-56` |
| Final gate | runs the final checks, a failure calls `fix_or_park` (can start a fix round) | `review.rs:263-278` |
| Risky evidence | `lead_parts` gives Review then Risk, `is_risky` returns false on a diff read error | `park.rs:157-167` |
| Diff read in evidence | `decision_parts` uses `job_changed_paths().unwrap_or_default()` | `park.rs:113` |
| Unsettled park | reads the ledger with `unwrap_or_default` and always writes a `Ledger` part | `runloop.rs:564-579` |
| Empty evidence rule | `evidence_or_reason` adds `Why it stopped` only when the list is empty | `park.rs:58-66` |
| Write order | `park_with` calls `write_record`, then `update`, then `park` | `park.rs:100-107` |
| Stage move | `Engine::park` sets the stage to needs-human | `ports.rs:177-182` |

## Non-goals

- No change to park kinds, `park.json` fields or the board JSON.
- Other sync parks (a conflict, a refused sync, a failed check after the sync, a sync review that asked for changes) keep their current text and kind `other`.
- No repair step that rebuilds a missing or stale `park.json`.
- No change to the merge gating decisions. Only the evidence changes.

## Design

Flow: `merge_stage` finds main moved and calls `sync_stage`. The sync review decides needs-human for a risky diff. A new `park_after_sync` runs the final checks and parks with the full evidence. The `Parked` outcome returns to `merge_stage`, whose `park_with_parts` keeps that record.

### 1. A risky park after a sync

New `Engine::park_after_sync(&self, kind: ParkKind, reason: &str) -> Result<StageOutcome, EngineError>` in `park.rs`, and `Driver::park_after_sync(&self, id: &str, kind: ParkKind, reason: &str) -> Result<StageOutcome, EngineError>` in `compat.rs` beside `apply`. In `sync_stage`, the `Decision::NeedsHuman { kind, reason }` arm calls it when `kind == ParkKind::RiskyDiff`, with the reason `after sync: <why>`, and then returns the same `SyncOutcome::Parked` text as today. Every other kind keeps today's behaviour.

`park_after_sync` runs `final_checks()` and chooses:

| Final checks | Kind | Reason | Parts, in order |
|---|---|---|---|
| all passed | `risky-diff` | the reason it got | Review, Risk, Rounds, Changed files, Checks |
| a failure | `checks-failed` | `final checks failed after sync` | Review, Risk, Rounds, Changed files, Checks |
| could not run (an error) | `risky-diff` | the reason it got | Review, Risk, Rounds, Changed files, then `Checks` with `final checks could not run: <error>` |

It never starts a fix round and never returns an error for a failed check: the park must still happen. The Checks part is read from `checks.md` with `part_from_file`.

### 2. A truthful Risk part

`decision_parts` and `park_after_sync` read the diff paths through a new `Engine::lead_parts_for_card(&self) -> Vec<ParkPart>`. When `job_changed_paths()` succeeds it returns `lead_parts(&paths)`. When it fails it returns the Review part and a `Risk` part (source `driver`) with the text `unknown: the job diff could not be read (<error>)`. The `risky` field of `ParkRecord` stays unset in that case.

Example card text: `Risk: unknown: the job diff could not be read (job has no head)`.

### 3. The write order in `park_with`

`Engine::park` is split in `ports.rs`: a new `move_to_needs_human(&self, reason: &str) -> Result<(), EngineError>` sets the stage and logs the park, and `park` calls it and then writes the ledger ruling, so its behaviour is unchanged for other callers. `park_with` builds the record as today, then runs four steps in order: the board park fields update, `move_to_needs_human`, `write_record`, and the ledger ruling. A failure of the board update or of the stage move returns the error and writes no record. A failure of the ruling returns the error after the record is written. The order matches `park_with_parts` in `merge.rs:47-74`.

### 4. The unsettled park without a ledger

`park_unsettled` leaves the `Ledger` part out when the ledger cannot be read or its last 20 lines are all blank. `evidence_or_reason` then gives the `Why it stopped` part with the reason `run loop did not settle, see the ledger`.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| Risky sync park, final checks pass | `risky-diff` card with Review, Risk, Rounds, Changed files and Checks |
| Risky sync park, a final check fails | `checks-failed` card, same parts, no fix round, no builder run |
| Risky sync park, checks cannot run | `risky-diff` card, a Checks part that says why, the park still happens |
| Sync park of another kind (conflict, refused, checks failed after sync, review asked for changes) | unchanged, kind `other` |
| Job diff unreadable at a park | Risk part `unknown: ...`, `risky` unset, other parts as before |
| Diff unreadable and the job has no review | the Risk part alone leads, `evidence_or_reason` is not triggered |
| Ledger missing, unreadable or blank | no Ledger part, `Why it stopped` appears |
| Board update fails in `park_with` | error returned, no `park.json` written |
| Stage move fails in `park_with` | error returned, no `park.json` written |
| The ledger ruling fails after the stage moved | `park.json` is already written, the error is returned |
| Crash after the stage move, before the record | parked job with no `park.json`, a state the code already allows because the write is best effort |

## Testing

`factory-engine`, in `tests/park_evidence.rs` (reuse `merge_record` and the merge fixtures):
- `a_risky_park_after_a_sync_keeps_the_review_and_risk_evidence`
- `a_risky_park_after_a_sync_runs_the_final_checks`
- `failing_final_checks_after_a_sync_park_as_checks_failed_without_a_fix_round`
- `other_sync_parks_stay_kind_other`

`factory-engine` unit tests in `park.rs`, on small pure helpers (`in_park_order`, `lead_parts_from`, `sync_park_choice`): `park_steps_run_update_then_stage_then_record_then_ruling`, `a_failed_board_update_leaves_no_park_record`, `a_failed_stage_move_leaves_no_park_record`, `a_failed_ruling_leaves_the_park_record`, `an_unreadable_diff_gets_a_risk_unknown_part`, `a_readable_diff_keeps_the_lead_parts`, `passing_final_checks_keep_the_risky_kind`, `failing_final_checks_park_as_checks_failed`, `final_checks_that_cannot_run_keep_the_risky_kind_and_say_why`. In `runloop.rs`: `an_unreadable_ledger_leaves_out_the_ledger_part` and `a_blank_ledger_leaves_out_the_ledger_part`, built like `unsettled_park_keeps_the_last_twenty_ledger_lines`.

Commands: `cargo test -j4 -p factory-engine`, then `cargo fmt -p factory-engine -- --check` and `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`.

## Files changed

| File | Change |
|---|---|
| `crates/factory-engine/src/park.rs` | `park_after_sync`, `lead_parts_for_card`, the write order, tests |
| `crates/factory-engine/src/ports.rs` | `Engine::park` split into `move_to_needs_human` and the ruling |
| `crates/factory-engine/src/compat.rs` | `Driver::park_after_sync` |
| `crates/factory-engine/src/sync.rs` | the risky arm calls `park_after_sync` |
| `crates/factory-engine/src/runloop.rs` | `park_unsettled` leaves out an empty Ledger part, tests |
| `crates/factory-engine/tests/park_evidence.rs` | the integration tests above |
| `docs/reference/software-factory.md` | the park evidence paragraph names the sync park and the unknown Risk text |

## Constraints

- Always `-j4`, `-p <crate>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- The Rust change passes `cargo fmt -p factory-engine -- --check` and `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`. No `#[allow(...)]` to silence a lint.
- No unwrap in production Rust. Tests use `.expect("...")` and `expect_err`.
- No new board, ledger or events field and no new `park.json` field. Park records stay compatible with the dashboard.
- Paths reach the driver only through `Layout`. Git is the concrete `factory_git::Repo`, never shelled out to.
- New public items carry a doc comment. Code comments are short full sentences that end with a period.

## Acceptance criteria

- A risky review that parks after a sync runs the final checks and writes a card with Review, Risk, Rounds, Changed files and Checks.
- A failing final check on that path parks as `checks-failed` with `final checks failed after sync` and starts no fix round.
- Final checks that cannot run still park, with a Checks part that says so.
- Every other sync park keeps its text and kind `other`.
- An unreadable job diff gives a `Risk` part `unknown: the job diff could not be read (<error>)` and leaves `risky` unset.
- `park_with` writes `park.json` after the board update and the stage move and before the ledger ruling, writes none when either of the first two fails, and keeps the record when the ruling fails.
- `park_unsettled` writes no `Ledger` part for an unreadable or blank ledger, and the card shows `Why it stopped`.
- fmt and clippy `-D warnings` pass for `factory-engine`.

## Out of scope

- Repairing or rebuilding a stale or missing `park.json`.
- Giving the other sync park kinds their own evidence.
- Changing the final gate or the fix rounds of the normal review path.

## Decisions

- What should a risky park on the sync path do about the final checks? Options: handle it like the normal review path (run the final checks, then park with the same evidence and the real kind); add only the free evidence and note that the checks did not run; leave and document it. Chosen: handle it like the normal review path. Decided by: user
- In what order does `park_with` write the board and `park.json`? Options: board first then the record, like `merge.rs`; keep the order and repair on the next read; write the record under a temporary name and rename it after the board update; leave it. Chosen: board first, then the record. Decided by: user
- What does the card show when the job diff cannot be read? Options: a Risk part that says the risk is unknown, with the `risky` flag unset; the same part with `risky` set; leave it. Chosen: a Risk part that says the risk is unknown, and the `risky` flag stays unset. Decided by: user
- What does the card show when `park_unsettled` cannot read the ledger? Options: leave the Ledger part out so the "Why it stopped" part appears; keep the part with a fixed text; leave it. Chosen: leave the Ledger part out when the ledger cannot be read or is empty. Decided by: user
- What if the final checks fail on a risky sync park? Options: park directly with the Checks evidence and no fix round; reuse `final_gate` so a fix round may run; park as `other` with only the check text. Chosen: park directly as `checks-failed` with the Checks evidence and no fix round. Decided by: user
- Where is `park.json` written relative to the stage move and the ledger ruling? Options: split `Engine::park` and write the record before the ruling; keep one `park` call and accept no record when the ruling fails; write the record first as today. Chosen: split `Engine::park`, so the order is board update, stage move, record, ruling. Decided by: user
