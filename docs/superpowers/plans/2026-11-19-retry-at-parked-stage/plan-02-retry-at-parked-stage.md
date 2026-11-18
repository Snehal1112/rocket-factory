# Retry at the parked stage: the retry command (plan 2 of 3)

> **Spec:** docs/superpowers/specs/2026-11-19-retry-at-parked-stage-design.md
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** `factory retry <id>` continues a parked job from its recorded resume stage without a builder run, and `factory rework` refuses on such a park unless `--force` is given.

**Architecture:** A new `crates/factory-engine/src/retry.rs` holds `retry_job`, which checks the park, moves the job to the right stage (or runs a whole-branch review for `Rereview`) and then drives it with `run_series`, so a series continues. `sync_requested` gets a caller flag; only the `Retry` caller changes behaviour. The CLI command holds the leases like `run`.

**Tech Stack:** Rust, factory-engine, factory-core, factory-cli. No new dependency.

## Global Constraints

- Consumed from plan 1: `ResumeAt { Review, Rereview, Merge, Sync }` with `as_str()` words `review`, `rereview`, `merge`, `sync`; `Job.park_resume: Option<ResumeAt>`; `Job::stage_before_park() -> Option<Stage>`; `ParkKind::ReviewFailed`; `ParkRecord.resume`; test helpers in `crates/factory-engine/tests/common/mod.rs`: `RoleFailing::new(spec_role: &'static str, reason: StopReason, fails: u32, inner: Arc<ScriptedRunner>)` (fails only the first `fails` runs of that role, then delegates to `inner`) and `approved_job_dyn(runner: Arc<dyn AgentRunner>, scripted: Arc<ScriptedRunner>, with_context: bool, config: Config) -> MergeFixture`.
- A retry never starts by running the builder. After its stage runs, the normal rules apply: a Changes verdict or failed checks (first or final) start a fix round, exactly as in `run`.
- A retry refuses (`EngineError::Refused`) unless the job is in `needs-human`, not cancelled, has no pending `batch_snapshot`, and `park_resume` is set. The refusal for a park without a resume value names that park's next commands, read with `ParkRecord::read(&driver.job_dir(&job))` (`factory-core/src/park.rs:186`, returns `Option<ParkRecord>`); when park.json is missing, it uses `ParkKind::next` of the board's `park_kind`, parsed with `serde_json::from_value::<ParkKind>(serde_json::Value::String(..))`; a missing or unknown kind uses `ParkKind::Other`.
- Main-busy, merge-lock, tip-differs and max-syncs parks come from stage `approve`. A `merge failed` park raised during an automatic sync, and a `review-failed` Rereview park, come from `review`. For `Merge` and `Sync`, apply `approve_refusal(false, false)` whenever `stage_before_park() != Some(Stage::Approve)`; otherwise the earlier approval carries over, and `merge_stage` re-checks the tip and scope itself (`merge.rs:271`, `287-297`).
- The ruling `retry at <word>` goes to the ledger before any board write or sync (ledger first, as at `sync.rs:82-85`). Log event `operator.retry` with field `resume = <word>`. Stage moves use `factory_engine::resume_logged(job, to, now)` (`ports.rs:62`) so `stage.change` is logged.
- `EngineError::Cancelled` during a retry maps to `RunEnd::NeedsHuman(reason)` as `run_job` does (`runloop.rs:681-683`).
- CLI: only `EngineError::Refused` maps to `CliError::Refused`. `ProjectBusy` stays `CliError::Engine(EngineError::ProjectBusy(_))` so the process exits 75 and factoryd re-queues the request.
- Rework refusal (only when the job is not cancelled, `park_resume` is set, and `--force` is absent): `job <id> parked outside the builder (<job.reason, else park_summary>); run factory retry <id>, or factory rework <id> --force <note> to send it to the builder anyway`.
- Cargo: `-j4`, one crate at a time, never `--workspace`, `--all` or `CARGO_TARGET_DIR`. Tests use `.expect("...")`. Comments are short full sentences that end with a period.

## Review Focus

1. A rereview retry when the branch tip equals `head` and main has not moved, and a rereview retry whose first or final checks fail: the whole-branch review runs, and failing checks start a fix round instead of parking. Pinned by `a_rereview_retry_after_a_failed_sync_review_reviews_again` and `a_rereview_retry_with_failing_final_checks_starts_a_fix_round`.
2. Retry while the cause is still there (main still dirty): parks again with the same reason and resume, no loop. Pinned by `a_merge_retry_on_dirty_main_parks_again`.
3. A cancelled job that keeps `park_resume`: `rework` still revives it, `retry` refuses it. Pinned by `rework_revives_a_cancelled_job_with_resume` and `retry_refuses_a_cancelled_job`.
4. A park written before plan 1 (no resume): retry refuses and names that park's commands. Pinned by `retry_refuses_a_park_without_resume`.
5. A retry that merges plan 1 of a series: plan 2 starts. Pinned by `a_retry_that_merges_continues_the_series`.

## File Structure

- `crates/factory-engine/src/retry.rs` (new): `retry_job` and its refusal text.
- `crates/factory-engine/src/sync.rs`: `SyncCaller`, `sync_requested_by`.
- `crates/factory-engine/src/merge.rs`: tip-differs evidence text.
- `crates/factory-engine/src/lib.rs`: module and exports.
- `crates/factory-core/src/board.rs`: refusal texts that name `retry` on a resume park.
- `crates/factory-cli/src/cli.rs`, `commands.rs`: `Retry` command, `rework --force`.
- `crates/factory-core/assets/global-command.md` and the generated `plugins/factory/commands/factory.md`.
- Docs: `docs/reference/software-factory.md`, `README.md`, `docs/guides/factory-user-guide.md`, `docs/reference/factory-admin-manual.md`.
- Tests: `crates/factory-engine/tests/retry.rs` (new), `crates/factory-cli/tests/retry_cmd.rs` (new), `operator_events.rs`, `global_command.rs`, `agent_lint.rs`, `plugin_files.rs`, `new_to_merged.rs`, and the `board.rs` unit tests.

---

### Task 1: Retry at review and merge, and the refusal texts

**Files:**
- Create: `crates/factory-engine/src/retry.rs`
- Modify: `crates/factory-engine/src/lib.rs`, `crates/factory-core/src/board.rs` (`changes_open_message` near 310, `approve_refusal` near 337, `review_missing_message` near 351), `crates/factory-engine/src/merge.rs` (tip-differs text at 291)
- Test: `crates/factory-engine/tests/retry.rs`, `board.rs` unit tests (`approve_refusal_follows_the_last_review` near 985 pins today's wording)

**Interfaces:**
- Consumes: the plan 1 items in Global Constraints; `run_series`, `RunEnd` (`runloop.rs:762`); `resume_logged`; `reset_reviews` (`board.rs:389`); `approve_refusal`; `driver_for` (`tests/common/mod.rs:309`).
- Produces:
  - `pub fn retry_job(driver: &Driver, id: &str) -> Result<RunEnd, EngineError>`. It takes no leases; the caller holds them, as for `run_series` (`run_series` re-takes `hold_project`, which is reentrant, `compat.rs:491-509`).
  - Refusal texts: where the three `board.rs` texts say `use rework`, they say `use factory retry <id>` when `park_resume` is `Review` or `Rereview`, and `use factory rework <id> --force <note>` when it is `Merge` or `Sync` (so retry's own refusal never tells the user to run retry); unchanged when `park_resume` is `None`.
  - The tip-differs evidence says `Run factory retry {id} to review the new tip, or reset the branch to the reviewed head.`

- [ ] **Step 1: Read** `run_job`, `run_job_inner` and `run_series` in `runloop.rs`, `approve` in `factory-cli/src/commands.rs:683-786`, the three `board.rs` texts and their tests, and the fixtures in `tests/common/mod.rs`, `tests/merge.rs` and `tests/review_decide.rs`.
- [ ] **Step 2: Write the failing tests**. In `tests/retry.rs` a test that needs a different runner or config after the park builds a second `Driver` with `common::driver_for(&fx.main, runner, config)` on the same repo.
  - `retry_after_a_failed_review_reviews_the_same_head`: park via `RoleFailing::new("reviewer", StopReason::Failed, 1, inner)` where `inner` scripts an approving reply for the retry, then `retry_job` on the same driver: `RunEnd::Merged`, round still 1, no `builder` spec among the runner calls, ledger has `retry at review`.
  - `a_review_retry_with_changes_starts_a_fix_round`: on retry the reviewer returns `VERDICT: CHANGES` with one blocker; script the fix builder and an approving second review; assert the ledger has the fix round 2 ruling and the job merges.
  - `retry_after_no_checks_runs_the_checks`: park as `no-checks` with a config that lacks the mapping; a second driver whose config maps the path; `retry_job` with it: checks run and the job merges.
  - `a_merge_retry_merges_once_main_is_clean` and `a_merge_retry_on_dirty_main_parks_again` (same reason, `park_resume == Some(Merge)`).
  - `a_merge_retry_keeps_an_earlier_without_review_approval` and `a_merge_retry_keeps_an_earlier_waive_approval` (the waiver stays not superseded and its tickets are filed after the merge).
  - `retry_refuses_a_park_without_resume` (park.json without `resume`: the refusal text contains its `next` commands); `retry_refuses_a_builder_park` (`checks-failed`); `retry_refuses_a_cancelled_job`; `retry_refuses_outside_needs_human`; `retry_refuses_a_pending_batch_commit`.
  - `board.rs`: `approve_refusal_names_retry_on_a_review_resume_park` and `approve_refusal_names_forced_rework_on_a_merge_resume_park`; update `approve_refusal_follows_the_last_review` only if its job has a resume value (it should not).
- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine --test retry` and `cargo test -j4 -p factory-core` and see the new tests fail.
- [ ] **Step 4: Implement** `retry_job` per Global Constraints for `Review` (ruling, then one board write: `reset_reviews` and `resume_logged(Stage::Review)`) and `Merge` (ruling, approval guard, `resume_logged(Stage::Approve)`), then emit `operator.retry` and return `run_series(driver, id)`, mapping `Cancelled` as stated. `Rereview` and `Sync` return `EngineError::Refused("retry at <word> is not supported yet")` until the next task. Change the texts in Produces.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-cli`, and clippy for the three crates. Expected: pass.
- [ ] **Step 6: Commit** `feat(engine): retry a parked job at review or merge`.

### Task 2: Retry at rereview and sync

**Files:**
- Modify: `crates/factory-engine/src/retry.rs`, `crates/factory-engine/src/sync.rs` (`sync_requested` at 46-107, `sync_with` at 112-232)
- Test: `crates/factory-engine/tests/retry.rs`

**Interfaces:**
- Consumes: `retry_job` from the previous task; `sync_requested`, `sync_with`, `MAX_SYNCS` (`merge.rs:10`); `Driver::apply` (`compat.rs:779`).
- Produces:
  - `pub enum SyncCaller { Command, Retry }` and `pub fn sync_requested_by(driver: &Driver, id: &str, by: SyncCaller) -> Result<SyncOutcome, EngineError>`. `sync_requested` calls it with `Command` and behaves exactly as today.
  - With `Retry`:
    - skip the `UpToDate` early return (`sync.rs:76-78`) and always set `whole_branch = true`, so the checks and the whole-branch review run even when the tip equals `head` and main has not moved (`sync_with(.., Requested, true)` then skips the merge and the message run and still runs `driver.checks` and the review);
    - checks that already started a fix round (`Advanced(Build)` near `sync.rs:185-193`) give `Synced`, not `Parked("checks failed after sync")`;
    - a review decision of `FixRound` calls `driver.apply(id, &outcome)` and gives `Synced`, not `Parked("sync review asked for changes")` (near `sync.rs:214-217`);
    - in the `Decision::AutoMerge` arm (`sync.rs:207-213`), `Advanced(Build)` from `driver.apply` (the final checks started a fix round) gives `Synced`; only a `NeedsHuman` result gives `Parked`;
    - skip the "sync review passed, run factory approve" re-park (`sync.rs:101-104`);
    - only a real `NeedsHuman` decision, a merge conflict or a refusal parks.
  - `retry_job` handles `Rereview` (ruling, `sync_requested_by(.., Retry)`; on `Parked(reason, _)` return `RunEnd::NeedsHuman(reason)`; else `run_series`) and `Sync` (ruling, approval guard, one board write with `j.syncs = j.syncs.min(MAX_SYNCS - 1)` and `resume_logged(Stage::Approve)`, then `run_series`).

- [ ] **Step 1: Read** `sync.rs` fully, the requested-sync tests in `tests/sync_review.rs` (the `with_security_agent` helper at 46, and the tests at 274, 455 and 628-645), and the series fixture in `tests/series.rs`.
- [ ] **Step 2: Write the failing tests** in `tests/retry.rs`. Copy `with_security_agent` from `tests/sync_review.rs:46` into it; every rereview test uses it, or the missing security agent parks the job as `review-failed`.
  - `a_rereview_retry_reviews_the_whole_branch_at_the_new_tip` (commit on the branch after review, tip-differs park, retry: `head` equals the tip, `prev_head` is `None`, the security role ran, the job merges).
  - `a_rereview_retry_after_a_failed_sync_review_reviews_again` (tip equals head, main not moved: the review runs and the job merges).
  - `a_rereview_retry_does_not_repark_after_a_passed_review`.
  - `a_rereview_retry_with_changes_starts_a_fix_round`.
  - `a_rereview_retry_with_failing_final_checks_starts_a_fix_round`.
  - `a_sync_retry_runs_one_more_sync_and_merges` (max-syncs park, main moved once more, retry merges after one sync review).
  - `a_retry_that_merges_continues_the_series` (two-plan series, plan 1 parked on main busy, retry: plan 1 merged and plan 2's job exists past stage spec).
  - Keep these existing tests green, unchanged: `a_passing_whole_branch_review_re_parks_a_needs_human_job` (`sync_review.rs:628`), `sync_requested_on_an_up_to_date_job_changes_nothing` (455), `an_up_to_date_job_with_tip_equal_head_changes_nothing_in_every_stage` (274).
- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine --test retry` and see the new ones fail.
- [ ] **Step 4: Implement** the Produces items, keeping every behaviour change behind `SyncCaller::Retry`.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine` and clippy for the crate. Expected: pass, including `sync_review`.
- [ ] **Step 6: Commit** `feat(engine): retry a parked job at rereview or sync`.

### Task 3: The retry command and the rework refusal

**Files:**
- Modify: `crates/factory-cli/src/cli.rs` (`Approve` at 79, `Rework` at 89), `crates/factory-cli/src/commands.rs` (dispatch, `norm` at 412, `rework` at 788), `crates/factory-core/assets/global-command.md` (hint at 3, shapes at 24-31, mappings at 40-43, step 6 near 94), `plugins/factory/commands/factory.md` (generated)
- Modify docs: `docs/reference/software-factory.md` (park kinds at 47, events at 162, no-checks at 213, Rework section at 238, max syncs at 258, commands table near 268), `README.md` (approve text 391-396, exit 75 at 487, commands), `docs/guides/factory-user-guide.md` and `docs/reference/factory-admin-manual.md` where they describe recovering a parked job
- Test: `crates/factory-cli/tests/retry_cmd.rs` (new), `operator_events.rs`, `global_command.rs` (argument hint at 428), `agent_lint.rs` (`command_is_thin_and_pins_the_shapes` at 437), `plugin_files.rs`, `new_to_merged.rs` (the main-busy rework near 1003 gets `--force`)

**Interfaces:**
- Consumes: `retry_job`, `RunEnd`, `Job.park_resume`.
- Produces:
  - `Command::Retry { id: String }`: `factory retry <id>`. It normalizes the id with `norm` and enters `factory_engine::job_span(id)` as `approve` and `sync` do (`commands.rs:650`, `693`), then takes the project lease and job lease exactly as `run` does (`commands.rs:624-645`); output via `outcome_lines::merged_line(None)` / `needs_human_line`.
  - `Command::Rework { id, note, #[arg(long)] force: bool }` with the refusal in Global Constraints. `rework --force -- <id> <note>` must parse.
  - `global-command.md`:
    - shapes: ``- `retry <digits>` (nothing after the digits)``, ``- `rework <digits> --force <note>` with a non-empty note``, and the plain rework shape becomes ``- `rework <digits> <note>` with a non-empty note that does not start with `--force` ``;
    - mappings: ``- `retry <id>`: `<factory> --project <name> retry <id>` `` and ``- `rework <id> --force <note>`: `<factory> --project <name> rework --force -- <id> '<note>'` ``;
    - the approve refusal line (40) offers `/factory retry <id>` when the refusal names it;
    - step 6 (near 94) offers `/factory retry <id>` when the status lists it;
    - the argument hint becomes `"<feature> [--opus] [--one-job] | status | approve <id> | retry <id> | rework <id> [--force] <note> | cancel <id|slug> | cleanup <id> | tickets <id>"`.

- [ ] **Step 1: Read** `cli.rs`, `run`, `approve`, `sync`, `rework` and `cancel` in `commands.rs`, `tests/sync_cmd.rs` and `tests/project_lease_cli.rs:44-55` for the CLI harness, `global-command.md`, and the pins in `global_command.rs:428`, `agent_lint.rs:437` and `plugin_files.rs:17`.
- [ ] **Step 2: Write the failing tests**:
  - `retry_cmd.rs`: `retry_merges_a_main_busy_job_once_main_is_clean`; `retry_refuses_a_builder_park_with_its_next_commands`; `rework_refuses_a_park_with_resume_and_names_retry` (exact text for job `001`, using `job.reason`); `rework_force_sends_it_to_the_builder` (back to build, round 2); `rework_still_works_on_a_no_tests_park`; `rework_revives_a_cancelled_job_with_resume`; `retry_is_busy_when_the_project_is_held` (asserts `CliError::Engine(EngineError::ProjectBusy(_))`, as in `project_lease_cli.rs:44-55`).
  - `operator_events.rs`: `retry_logs_operator_retry` (fields `resume` and `job_id == "001"`).
  - Update `global_command.rs:428` to the new argument hint.
- [ ] **Step 3: Run** `cargo test -j4 -p factory-cli --test retry_cmd --test operator_events` and see the new tests fail.
- [ ] **Step 4: Implement** the Produces items. Update `global-command.md`, then regenerate the plugin file with `cargo run -j4 -p factory-cli -- plugin sync` from the repo root. Update the docs in Files: `retry` in the commands table, which parks resume where (the spec's table), `rework` refusing them without `--force`, `review-failed` in the kinds list, `operator.retry` in the events list, `retry` exiting 75 like `run` and `approve`. Existing CLI tests that rework a no-checks, tip-differs or main-busy park get `--force`.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-cli` and `cargo clippy -j4 -p factory-cli --all-targets -- -D warnings`. Expected: pass, including `agent_lint`, `global_command` and `plugin_files`.
- [ ] **Step 6: Commit** `feat(cli): add factory retry and refuse rework on driver parks`.

Next plan: docs/superpowers/plans/2026-11-19-retry-at-parked-stage/plan-03-retry-at-parked-stage.md
