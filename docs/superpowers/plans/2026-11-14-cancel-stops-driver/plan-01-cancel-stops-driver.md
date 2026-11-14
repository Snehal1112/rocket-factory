# Cancel Stops Driver, Plan 1: Guard, Merge Check and Resume Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-14-cancel-stops-driver-design.md
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** A cancelled job refuses every stage move except to needs-human, a running driver ends quietly as needs-human, the merge checks the board right before the git merge, and needs-human is left only through `Job::resume`.

**Architecture:** The guard sits in `Job::set_stage` in factory-core, so every writer obeys it. `EngineError::Cancelled` carries the refusal to `run_job`, which turns it into `RunEnd::NeedsHuman`. `try_merge` re-reads the job before `merge_no_ff`. Logic stays behind the existing ports (`Tracker`, `AgentRunner`); no new trait.

**Tech Stack:** Rust, `cargo test -j4 -p <crate>`.

## Global Constraints

- Board JSON stays compatible with `dashboard/`: no new board field. `cancelled` keeps its name and default (`board.rs:94-95`).
- Never shell out to `git`. Use `factory-git` as today.
- The guard applies with `force` too.
- Comments are short full sentences that end with a period.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- Use `-j4`, never `--workspace` or `--all`. No `Cargo.toml` change.

## Review Focus

1. A forced move on a cancelled job (the forced move to build at `checks_stage.rs:288`): it must be refused too. Pinned by `a_cancelled_job_refuses_every_move_except_needs_human` in the core task.
2. A cancelled job that parks again (an in-flight stage parks after the cancel): the move to needs-human stays allowed and `cancelled` stays true. Pinned by `a_cancelled_job_can_still_park_again` in the core task.
3. A job the driver parked itself (not cancelled): it resumes through `resume` to build, review or approve and is refused anywhere else. Pinned by `resume_moves_needs_human_to_build_review_or_approve` and `resume_refuses_spec_merged_and_other_stages` in the core task.
4. A cancel that lands during the commit-message run: no merge commit, main unchanged. Pinned by `a_cancel_during_the_commit_message_run_leaves_main_unchanged` in the engine task.
5. A cancel during the builder run: the run ends as needs-human, never review. Pinned by `a_cancel_during_the_builder_run_ends_the_run_at_needs_human` in the engine task.
6. Rework on a cancelled job (both kinds) and a second cancel keep working. Pinned by `rework_next_round_clears_cancelled_and_resumes_at_build` and `a_second_cancel_updates_the_reason` in the operator task.

## File Structure

- `crates/factory-core/src/error.rs`: `CoreError::Cancelled`.
- `crates/factory-core/src/board.rs`: guard in `Job::set_stage`, `Job::resume`, tests.
- `crates/factory-core/src/stage.rs`: `Stage::check_move` refuses exits from needs-human, tests.
- `crates/factory-engine/src/lib.rs`, `compat.rs`: `EngineError::Cancelled` and the `From<CoreError>` mapping, export of `resume_logged`.
- `crates/factory-engine/src/ports.rs`: `resume_logged`.
- `crates/factory-engine/src/runloop.rs`: `run_job` catch and `cancel_reason`.
- `crates/factory-engine/src/merge.rs`: the check before `merge_no_ff`.
- `crates/factory-engine/tests/cancel.rs` (new): engine tests.
- `crates/factory-cli/src/commands.rs`: rework and approve use `resume_logged`.
- `crates/factory-cli/tests/operator_events.rs`: operator tests.

### Task 1: Core guard, `Job::resume` and the tighter `check_move`

**Files:**
- Modify: `crates/factory-core/src/error.rs`
- Modify: `crates/factory-core/src/board.rs` (`set_stage` at 398-420, tests from about 1044)
- Modify: `crates/factory-core/src/stage.rs` (`check_move` at 50-65, tests at 85-119)

**Interfaces:**
- Consumes: `Job.cancelled: bool` (`board.rs:94-95`), `Stage::{Spec, Build, Review, Approve, Merged, NeedsHuman}`, `CoreError::StageMove { from, to }`.
- Produces: `CoreError::Cancelled` with the message `job is cancelled`. `Job::resume(&mut self, to: Stage, now: f64) -> Result<(), CoreError>`. `Job::set_stage` returns `CoreError::Cancelled` when `self.cancelled && to != Stage::NeedsHuman`, with or without `force`, and leaves the job untouched. `Stage::check_move(from, to, force)` refuses every move out of needs-human to another stage when `force` is false.

- [ ] **Step 1: Read the code**

Read `error.rs`, `board.rs` 396-425 and its tests from 1040, `stage.rs` 40-120, and `docs/superpowers/specs/2026-11-14-cancel-stops-driver-design.md` Design 1 and 4.

- [ ] **Step 2: Write the failing tests**

In `board.rs` tests, build the job as `set_stage_updates_since_only_on_change_and_manages_reason` does (`job()`), and add:

```rust
#[test]
fn a_cancelled_job_refuses_every_move_except_needs_human() {
    for force in [false, true] {
        for to in [Stage::Spec, Stage::Build, Stage::Review, Stage::Approve, Stage::Merged] {
            let mut j = job();
            j.set_stage(Stage::NeedsHuman, Some("cancelled: x"), false, 1.0).expect("park");
            j.cancelled = true;
            let before = j.clone();
            let err = j.set_stage(to, None, force, 2.0).expect_err("refused");
            assert!(matches!(err, CoreError::Cancelled), "{to} force={force}: {err}");
            assert_eq!(j, before, "refused move leaves the job untouched");
        }
    }
}
```

| Test | Setup | Assertion |
|---|---|---|
| `a_cancelled_job_can_still_park_again` | job in build, `cancelled = true`, then `set_stage(NeedsHuman, Some("again"), false, 2.0)` | `Ok`, `reason == Some("again")`, `cancelled` still true |
| `resume_moves_needs_human_to_build_review_or_approve` | park a job, then `resume(to, 3.0)` for each of Build, Review, Approve | `Ok`, `stage == to`, `reason`, `park_kind`, `park_at`, `park_summary` are `None`, `stage_since == 3.0`, one more `stage_history` entry |
| `resume_refuses_spec_merged_and_other_stages` | park a job and `resume(Spec)` and `resume(Merged)`; a job in review and `resume(Build)` | each is `CoreError::StageMove`, job untouched |
| `resume_refuses_a_cancelled_job` | parked job with `cancelled = true`, `resume(Build)` | `CoreError::Cancelled` |

In `board.rs` change `set_stage_updates_since_only_on_change_and_manages_reason` so its last move out of needs-human uses `j.resume(Stage::Review, 500.0)`. In `stage.rs` replace `needs_human_is_a_parking_stage` (lines 107-119) with `needs_human_moves_out_only_by_force`: moves from spec, build, review and approve into needs-human are `Ok`; from needs-human to build, review, approve, merged and spec are `Err` without force and `Ok` with force.

- [ ] **Step 3: Run the tests and see them fail**

Run: `cargo test -j4 -p factory-core`
Expected: compile errors `no variant named Cancelled` and `no method named resume`.

- [ ] **Step 4: Implement**

Add `CoreError::Cancelled` (`#[error("job is cancelled")]`). In `board.rs` move the body of `set_stage` after the checks into a private `apply_move(&mut self, to, reason, now)` that updates stage, `stage_since`, history, reason and the park fields exactly as today. `set_stage` first returns `CoreError::Cancelled` when `self.cancelled && to != Stage::NeedsHuman`, then runs `Stage::check_move`, then `apply_move`. `resume` returns `Cancelled` for a cancelled job, `StageMove` unless `self.stage == NeedsHuman` and `to` is Build, Review or Approve, then calls `apply_move` with no reason. In `stage.rs` change the `(Stage::NeedsHuman, Stage::Spec) => false` and `(Stage::NeedsHuman, _) => true` arms so a move out of needs-human to a different stage is `false`, and update the doc comment: needs-human is left only by force or `Job::resume`. Keep the arm order so the move into needs-human stays allowed.

- [ ] **Step 5: Run the tests and see them pass**

Run: `cargo test -j4 -p factory-core`
Expected: all pass. A core test that fails only because it moves out of needs-human with a plain `set_stage` switches to `resume` or `force`.

- [ ] **Step 6: Lint and format**

Run: `cargo clippy -j4 -p factory-core --all-targets -- -D warnings` then `cargo fmt -p factory-core -- --check`
Expected: no warnings, no diff.

### Task 2: Engine error, run-loop catch, merge check and `resume_logged`

**Files:**
- Modify: `crates/factory-engine/src/lib.rs` (`EngineError` at 127, exports at 89)
- Modify: `crates/factory-engine/src/compat.rs` (`From<CoreError>` at 17)
- Modify: `crates/factory-engine/src/ports.rs` (`set_stage_logged` at 47)
- Modify: `crates/factory-engine/src/runloop.rs` (`run_job` at 676)
- Modify: `crates/factory-engine/src/merge.rs` (`try_merge` at 256, the `merge_no_ff` call at about 339)
- Create: `crates/factory-engine/tests/cancel.rs`

**Interfaces:**
- Consumes: `CoreError::Cancelled`, `Job::resume` from the core task, `Driver::store()` and `Driver::job()`, `RunEnd::NeedsHuman(String)`, `merge_stage(&Driver, &str) -> Result<MergeResult, EngineError>`.
- Produces: `EngineError::Cancelled` (message `job is cancelled`). `pub fn resume_logged(job: &mut Job, to: Stage, now: f64) -> Result<(), CoreError>` in `ports.rs`, exported from `lib.rs` next to `set_stage_logged`; it logs `stage.change` as `set_stage_logged` does. `run_job` returns `Ok(RunEnd::NeedsHuman(reason))` when the stages return `EngineError::Cancelled`, where `reason` is the job's `reason` or `cancelled`. `try_merge` returns `Err(EngineError::Cancelled)` when the job is cancelled, before `merge_no_ff`.

- [ ] **Step 1: Read the code**

Read `lib.rs` 120-160, `compat.rs` 15-30, `ports.rs` 40-75, `runloop.rs` 674-685, `merge.rs` 256-366, and the fixtures `approved_job`, `approved_job_with`, `scripted`, `reply`, `SeriesFixture::owned` and `RunHook` in `tests/common/mod.rs`, plus `tests/runloop.rs` 1-50.

- [ ] **Step 2: Write the failing tests in `tests/cancel.rs`**

Start the file with `mod common;`. A helper `cancel_job(fx_main: &Path, id: &str)` opens `factory_engine::BoardStore::new(&Layout::in_repo(fx_main).board_file(), Duration::from_secs(5))` and in one `update_job` parks the job with `Job::set_stage(NeedsHuman, Some("cancelled: test"), false, 1.0)` and sets `cancelled = true`, as `cancel` in `factory-cli/src/commands.rs:848-885` does. Tests:

```rust
#[test]
fn a_cancelled_job_does_not_merge() {
    let fx = common::approved_job();
    let before = common::main_tip(&fx.main);
    cancel_job(&fx.main, &fx.id);
    let err = factory_engine::merge_stage(&fx.driver, &fx.id).expect_err("cancelled");
    assert!(matches!(err, factory_engine::EngineError::Cancelled), "{err}");
    assert_eq!(common::main_tip(&fx.main), before, "main is unchanged");
}
```

| Test | Fixture and trigger | Assertions |
|---|---|---|
| `a_cancel_during_the_commit_message_run_leaves_main_unchanged` | `approved_job_with(Arc::new(scripted(vec![effect])), true)` where `effect` is a custom `Effect` closure that calls `cancel_job` (the main path and id reach the closure through an `Arc<Mutex<Option<(PathBuf, String)>>>` filled after the fixture exists) and returns a valid commit message such as the `GOOD` text of `tests/message.rs`; then `merge_stage` | `Err(EngineError::Cancelled)`, `main_tip` unchanged, job `stage == NeedsHuman` and `cancelled` |
| `a_cancel_during_the_builder_run_ends_the_run_at_needs_human` | `SeriesFixture::owned("tester", hook)` as `a_panic_in_a_run_frees_the_job_lease` does, a hook that cancels the job on the first agent run (same shared cell), `create_job` with a one-task plan, `run_job` | `Ok(RunEnd::NeedsHuman(reason))` with `reason` containing `cancelled: test`, job stage `NeedsHuman`, never `Review`, `cancelled` true |
| `engine_error_cancelled_comes_from_core_error` | `EngineError::from(CoreError::Cancelled)` | `matches!(.., EngineError::Cancelled)`; and `EngineError::from(CoreError::LedgerLine)` is still `Core(_)` |
| `resume_logged_logs_a_stage_change` | a parked job, call `resume_logged(&mut job, Stage::Build, 5.0)` | `Ok`, stage is build |

- [ ] **Step 3: Run the tests and see them fail**

Run: `cargo test -j4 -p factory-engine --test cancel`
Expected: compile error `no variant named Cancelled found for enum EngineError`.

- [ ] **Step 4: Implement**

Add the `EngineError::Cancelled` variant with its message. In `compat.rs` map `CoreError::Cancelled` to it and keep `Core(e.to_string())` for the other variants. Add `resume_logged` to `ports.rs` (call `job.resume`, log `stage.change` with the same fields as `set_stage_logged` when the stage changed) and export it in `lib.rs`. In `run_job`, match the result of `span.in_scope(|| run_job_inner(...))`: `Err(EngineError::Cancelled)` becomes `Ok(RunEnd::NeedsHuman(cancel_reason(driver, id)))` and everything else passes through; `cancel_reason` re-reads the job and returns its `reason` or `"cancelled"`. In `try_merge`, after the second `base_state` match and immediately before `merge_no_ff`, re-read `driver.job(id)?` and return `Err(EngineError::Cancelled)` when `job.cancelled`.

- [ ] **Step 5: Run the tests and see them pass**

Run: `cargo test -j4 -p factory-engine --test cancel`, then `cargo test -j4 -p factory-engine`
Expected: the new tests pass and the suite is green. An engine test that fails only because it moves a job out of needs-human with a plain `set_stage` switches to `resume_logged` or `force`.

- [ ] **Step 6: Lint and format**

Run: `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings` then `cargo fmt -p factory-engine -- --check`
Expected: no warnings, no diff.

### Task 3: Rework and approve use `resume_logged`

**Files:**
- Modify: `crates/factory-cli/src/commands.rs` (approve at 722, rework at 796 and 822)
- Test: `crates/factory-cli/tests/operator_events.rs`

**Interfaces:**
- Consumes: `factory_engine::resume_logged(job: &mut Job, to: Stage, now: f64) -> Result<(), CoreError>`, `Job::cancelled`.
- Produces: rework (both kinds) sets `cancelled = false` before `resume_logged(.., Stage::Build, ..)`; approve calls `resume_logged(.., Stage::Approve, ..)` when the stage is needs-human and `set_stage_logged` otherwise.

- [ ] **Step 1: Read the code**

Read `commands.rs` 668-735 (approve) and 769-836 (rework), `tests/operator_events.rs` 1-60 (the `parked()` scenario with `Scenario::run`, `sc.job`, `sc.stage`), and `new_to_merged.rs` 89-110 (`rework_revives_a_cancelled_job_so_approve_can_merge_it`).

- [ ] **Step 2: Write the failing tests in `operator_events.rs`**

Build each scenario with `parked()`.

```rust
#[test]
fn a_second_cancel_updates_the_reason() -> TestResult {
    let sc = parked()?;
    sc.run(&["cancel", "1", "--reason", "first"])?;
    sc.run(&["cancel", "1", "--reason", "second"])?;
    let job = sc.job("001")?;
    assert!(job.cancelled);
    assert_eq!(job.reason.as_deref(), Some("cancelled: second"));
    Ok(())
}
```

| Test | Steps | Assertions |
|---|---|---|
| `rework_next_round_clears_cancelled_and_resumes_at_build` | `parked()`, `cancel 1`, then `rework 1 'fix it'` | stage `build`, `cancelled == false` |
| `approve_still_refuses_a_cancelled_job` | `parked()`, `cancel 1`, then `approve 1` | `expect_err` text contains `was cancelled`, stage `needs-human` |
| `approve_resumes_a_driver_parked_job_through_resume` | `parked()` then `approve 1` as `approve_logs_operator_approve` does | job reaches `merged` or the approve stage as before, no refusal |

The existing `rework_revives_a_cancelled_job_so_approve_can_merge_it` must keep passing.

- [ ] **Step 3: Run the tests and see them fail**

Run: `cargo test -j4 -p factory-cli --test operator_events`
Expected: `rework_next_round_clears_cancelled_and_resumes_at_build` fails with `job is cancelled` or `illegal stage move needs-human -> build`, because the flag is cleared after the move and `set_stage_logged` no longer leaves needs-human.

- [ ] **Step 4: Implement**

In both rework closures set `j.cancelled = false` first, then call `factory_engine::resume_logged(j, Stage::Build, now)?`; drop the old comment that says the guard allows it, and keep the rest of each closure. In approve, replace the `set_stage_logged(j, Stage::Approve, ..)` call with `resume_logged` when `j.stage == Stage::NeedsHuman`, else the existing call.

- [ ] **Step 5: Run the crate suites**

Run: `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`, `cargo test -j4 -p factory-supervisor`
Expected: all pass.

- [ ] **Step 6: Lint and format**

Run: `cargo clippy -j4 -p factory-core -p factory-engine -p factory-cli --all-targets -- -D warnings`, then `cargo fmt -p factory-core -p factory-engine -p factory-cli -- --check`
Expected: no warnings, no diff.

Next plan: docs/superpowers/plans/2026-11-14-cancel-stops-driver/plan-02-cancel-stops-driver.md
