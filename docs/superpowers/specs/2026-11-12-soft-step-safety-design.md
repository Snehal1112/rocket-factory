# Soft step safety design

## Goal

Make the post-merge soft steps write every failure to the ledger and survive a panic, so a merged job always ends as merged, its series continues and its lease is freed.

## Overview

GitHub issue #58 reports two defects in what the driver runs after a merge (`merge_stage` in `crates/factory-engine/src/merge.rs`). The project rule is that these steps never change the merge result and report failures to the ledger, stderr and events. A code read in this session confirms both defects and adds two small ones.

- A. An `Err` from `tickets_step`, `waiver_tickets_step` or `minor_tickets_step` reaches stderr and the events but not the ledger. The ledger line is written inside each step body, and the `?` exits skip it. A `report.failed` run does write one.
- B. A panic in a soft step unwinds out of `merge_stage` after the merge is done. The most likely source is a `Tracker` implementation, which the engine does not own. The panic skips the later steps and the costs refresh, ends `run_series` before the next plan and leaves the job lease held until its 1800 s expiry.
- C. `notify_failure` (`tickets.rs:391`) labels every event `tickets step:`, so a waiver or minor failure is mislabelled.
- D. `run_job` frees the job lease with plain code after the run (`runloop.rs:594`), so any unwind, from a soft step or elsewhere, keeps the lease.

Both soft steps for waivers and minors run on every merge, so A and B apply with `tickets_on_merge` off.

## Current state

| Piece | Today | Gap |
|---|---|---|
| `report_soft_step` | Logs `step.error` and calls `notify_failure` for `Ok(report)` with `failed` and for `Err` (`merge.rs:81-100`) | Writes no ledger line on `Err` |
| `merge_stage` | After `finish_merge` it calls `kb_refresh`, `tickets_step` (only with `tickets_on_merge`), `waiver_tickets_step`, `minor_tickets_step` and `refresh_costs` in a row (`merge.rs:102-146`) | No panic boundary: a panic skips the rest and unwinds |
| Ledger writes | `finish` (`tickets.rs:379-389`), `waivers.rs:182` and `minors.rs:79`, each one folded line `driver.ledger(&job).ruling(..)` | Reached only on the paths that finish normally |
| `notify_failure` | Emits a `Notice` event with the text `tickets step: {text}` (`tickets.rs:391-404`) | Hard-coded label |
| `run_job` | `hold_project`, run, then `driver.release_lease(id)` (`runloop.rs:589-596`) | Not a guard, so an unwind skips it |
| `run_series` | Calls `run_job(..)?` in a loop (`runloop.rs:664-`) | A panic ends the series at a merged plan |
| Locks | `lock_held` and `driver.beat` recover from a poisoned mutex (`compat.rs:131`, `:152`) | Nothing to change |
| Panic handling | No `catch_unwind` in the repo; no `panic = "abort"` profile | None |

The existing test `a_broken_marker_after_a_merge_is_reported_and_the_job_stays_merged` (`tests/tickets.rs:345`) asserts the events text only, so it passes today.

## Non-goals

- Changing `MergeResult`, the job stage or the merge for any soft step outcome.
- Changing any `Cargo.toml` or the build profile.
- Making the three steps panic-free. The boundary is `catch_unwind`, not a code audit.
- Fixing the minor ticket being filed before its marker is written (see Follow-ups).

## Design

Flow after a merge:

```
merge_stage -> finish_merge
  run_soft_step("kb_refresh", ..)      Err: stderr only          Panic: stderr + ledger + events
  run_soft_step("tickets", ..)         Err or Panic: stderr + ledger + events   (only with tickets_on_merge)
  run_soft_step("waivers", ..)         Err or Panic: stderr + ledger + events
  run_soft_step("minors", ..)          Err or Panic: stderr + ledger + events
  run_soft_step("costs_refresh", ..)   Err: stderr only          Panic: stderr + ledger + events
  -> Ok(MergeResult::Merged(sha))
```

### 1. The panic boundary (`merge.rs`)

```rust
/// What went wrong in a soft step.
enum StepFailure {
    Error(EngineError),
    Panic(String),
}

/// Runs one post-merge step and turns a panic into a `StepFailure::Panic` with the payload text.
fn run_soft_step<T>(f: impl FnOnce() -> Result<T, EngineError>) -> Result<T, StepFailure>
```

- It uses `std::panic::catch_unwind(AssertUnwindSafe(f))`. The closure borrows `&Driver` only, and the shared mutexes already recover from poison.
- The payload text is the `&str` or `String` payload, else `unknown panic`.
- Each of the five calls in `merge_stage` goes through it. A panic or error in one step never skips a later one.

### 2. Reporting (`merge.rs`)

```rust
fn report_soft_step(driver: &Driver, id: &str, step: &str, result: Result<TicketsReport, StepFailure>)
```

- `Ok(report)` with `report.failed`: as today. The step already wrote its single ledger line, so this arm adds none.
- `Err(StepFailure::Error(e))` for the three ticket steps: log `step.error`, write one ledger line `<step> error: <one line>`, and call `notify_failure(driver, id, step, &text)`.
- `Err(StepFailure::Panic(p))` for any step: log `step.error`, write one ledger line `<step> panicked: <one line>`, and call `notify_failure`.
- For `kb_refresh` and `costs_refresh`, an `Err` stays stderr only, as today. A panic uses the panic arm above.
- The ledger line is folded to one line with the same whitespace folding as `finish`. It is written with `driver.job(id)` then `driver.ledger(&job).ruling(&line)`. If `driver.job(id)` fails, the ledger line is skipped. A failed ledger write is logged as `step.error` with `step = "<step>_ledger"` and never raised.

Example ledger lines: `tickets error: tickets.json is not valid JSON`, `waivers panicked: index out of bounds`.

### 3. The event label (`tickets.rs`)

`notify_failure(driver: &Driver, id: &str, step: &str, text: &str)` emits `{step} step: {text}`. Its two callers in `merge.rs` pass the step name. The tickets step keeps the label `tickets step:`, so the current test stays valid.

### 4. The job lease guard (`runloop.rs`)

A private `LeaseGuard<'a> { driver: &'a Driver, id: &'a str }` whose `Drop` calls `driver.release_lease(self.id)`. `run_job` creates it after `hold_project`, so it drops before the project guard, and the explicit `release_lease` call is removed. `release_lease` already does nothing when the driver has no lease owner. `JobGuard` in `compat.rs` is not reused, because it serves the CLI `factory tickets` run and does not clear the held lease or release the project lease.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| `tickets_step` returns `Err` (broken `tickets.json`, failed `driver.job`) | One ledger line `tickets error: ...`, one stderr event, one events notice; job stays merged |
| `tickets_step` returns `Ok` with `failed` | One ledger line from `finish`, none added by `report_soft_step` |
| A soft step panics | One ledger line `<step> panicked: ...`; later steps still run; `merge_stage` returns `Merged` |
| Panic payload is not a string | Text `unknown panic` |
| `driver.job(id)` fails while reporting | No ledger line; stderr and events still written |
| Ledger write fails | Logged as `step.error`, never raised |
| `kb_refresh` or `refresh_costs` returns `Err` | stderr only, as today |
| `kb_refresh` or `refresh_costs` panics | stderr, ledger and events |
| A panic leaves a mutex poisoned | Later locks recover; no change needed |
| A panic outside the soft steps during `run_job` | The lease guard frees the job lease while unwinding |
| `tickets_on_merge` is off | The tickets step is not called; waivers and minors are |
| `panic = "abort"` set in a profile | Would defeat the boundary; a test pins that none is set |

## Testing

`cargo test -j4 -p factory-engine`:

- `tests/tickets.rs`, with `approved_with_hook`, `events_text` and `FakeTracker`:
  - `a_broken_marker_after_a_merge_writes_a_ledger_line`: the broken-marker setup; the job's `progress.md` has a line with `tickets error` and `not valid JSON`; the job stays Merged.
  - `a_tracker_failure_still_writes_one_ledger_line`: `fail_on("repo_name")`; `progress.md` holds exactly one `tickets failed` line and no `tickets error` line.
  - `a_panicking_tracker_after_a_merge_returns_merged`: a `PanicTracker` that panics in `repo_name`; `merge_stage` returns `Merged`; the stage is Merged; the ledger and events contain `tickets panicked`.
  - `a_panicking_tracker_does_not_skip_the_waiver_and_minor_steps`: `PanicTracker` panics only in `repo_name` and forwards the rest to an inner `FakeTracker`; a waiver is on file; after the merge the inner tracker saw the waiver's `create`.
  - `a_waiver_failure_is_labelled_waivers_in_the_events`: a failing waiver step produces `waivers step:` in the events and not `tickets step:`.
- `merge.rs` unit tests (a panic inside `kb_refresh` or the costs refresh cannot be set off from an integration test, so the wrapper and the reporting rule are tested directly):
  - `run_soft_step_turns_every_payload_kind_into_text`: a `&str` payload, a `String` payload and a payload of another type (`std::panic::panic_any(7)`) give `Panic("msg")`, `Panic("msg")` and `Panic("unknown panic")`; an `Err` and an `Ok` pass through.
  - `a_kb_or_costs_error_gets_no_ledger_line` and `a_kb_or_costs_panic_gets_a_ledger_line`: `ledger_line` returns `None` for an `Error` of `kb_refresh` or `costs_refresh`, `Some("kb_refresh panicked: boom")` for their panic, and `Some("tickets error: ...")` for an error of a ticket step.
- `tests/common/mod.rs`: `PanicTracker { inner: FakeTracker, panic_on: Option<&'static str> }` implementing `Tracker` (it panics in the named method, or in every method when `None`, and forwards the rest to `inner`), and `with_boxed_tracker(fx, Box<dyn Tracker>)` next to `with_tracker`.
- `tests/runloop.rs`:
  - `a_series_continues_after_a_panicking_soft_step`: a two-plan series whose first job has a waiver on file and a `PanicTracker` that panics in every method; `run_series` reaches and merges the second plan.
  - `a_panic_in_a_run_frees_the_job_lease`: a `SeriesFixture::owned` driver whose run hook panics; `run_job` inside `catch_unwind` unwinds; afterwards the job has `lease` of `None`.
- `crates/factory-cli/tests/no_panic_abort.rs`: `no_panic_abort_profile` reads the root `Cargo.toml` and every crate `Cargo.toml` and fails when any contains `panic = "abort"`.

Run `cargo clippy -j4 -p factory-engine -p factory-cli`.

## Files changed

| File | Change |
|---|---|
| `crates/factory-engine/src/merge.rs` | `StepFailure`, `run_soft_step`, `ledger_line`, the new `report_soft_step`, the five wrapped calls, unit tests |
| `crates/factory-engine/src/tickets.rs` | `notify_failure` takes a step name |
| `crates/factory-engine/src/runloop.rs` | `LeaseGuard` in `run_job` |
| `crates/factory-engine/tests/tickets.rs`, `runloop.rs`, `common/mod.rs` | New tests, `PanicTracker`, `with_boxed_tracker` |
| `crates/factory-cli/tests/no_panic_abort.rs` | New source test |

## Constraints

- Do not change `MergeResult` or the job stage for any soft step outcome.
- Board JSON stays compatible with `dashboard/`. Ledger lines are single-line `Ruling:` text.
- No `git` shell-outs; `gh` stays behind the `Tracker` port.
- Do not touch any `Cargo.toml` (a risk path), and do not set `panic = "abort"`.
- Comments are short full sentences. Commands use `-j4`, never `--workspace` or `--all`.
- `merge.rs`, `tickets.rs`, `runloop.rs` and `tests/` are not risky paths.

## Acceptance criteria

- An `Err` from `tickets_step`, `waiver_tickets_step` or `minor_tickets_step` writes one ledger line, one stderr event and one events notice.
- A run with `report.failed` still writes exactly one ledger line.
- A panic in any of the five soft calls leaves `merge_stage` returning `Merged`, with the panic text in the ledger, stderr and events.
- A panic in one soft step does not stop the later steps from running.
- `run_series` proceeds to the next plan after such a panic.
- A waiver or minor failure is labelled with its own step name in the events.
- A panic during `run_job` frees the job lease.
- No `Cargo.toml` sets `panic = "abort"`.

## Out of scope

- Any change to how the tickets, waivers or minors steps decide what to file.
- A panic before the merge completes; that path parks the job as today.

## Follow-ups

- The minor ticket is created before `minors.json` is written (`minors.rs:65-68`), so a failed marker write leaves a filed ticket with no marker and a rerun files a duplicate. It needs a pending marker with recovery, as the ticket step got in #57.

## Decisions

- Should the job lease be freed by a Drop guard? Options: include a guard in `run_job`; keep it out of scope; guard without catch_unwind. Chosen: a guard in `run_job` that calls `release_lease`. Decided by: user
- Which calls does the boundary wrap, and which write a ledger line? Options: all five, ledger for the three ticket steps; all five, ledger for every failure; only the three ticket steps. Chosen: all five wrapped; an `Err` or panic in the three ticket steps writes the ledger; `kb_refresh` and `refresh_costs` keep stderr only on `Err` and add ledger and events on a panic. Decided by: user
- What do the ledger line and the event note say? Options: name the step everywhere; ledger names the step but the event label stays; ledger for `Err` only. Chosen: name the step everywhere (`<step> error:`, `<step> panicked:`, `<step> step:`). Decided by: user
- The minor ticket marker is written after the ticket is created. Options: follow-up ticket; fix here with a pending marker; ignore. Chosen: follow-up ticket, not in this spec. Decided by: user
- The issue's line numbers moved after #57, so every reference here was re-read against the source in this session. Decided by: call
- A source test pins that no profile sets `panic = "abort"`, because it would silently disable the boundary. Decided by: call
- The test tracker is a `PanicTracker` that wraps a `FakeTracker`, because a panic while the `FakeTracker` mutex is held would poison it. Decided by: call
