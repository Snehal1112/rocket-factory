# Issue queue, part 3: factoryd polling and docs Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-12-issue-queue-design.md
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Let `factoryd` queue an intake pass for each project that turns `intake` on, stop the post-merge ticket steps from posting agent text for intake jobs, and document the feature.

**Architecture:** A new `Action::Intake` request goes through the existing queue and scheduler, and the spawned child is `factory --project <p> intake --once`. A timer in the supervisor loop enqueues at most one such request per project. No agent or tracker port is involved in `factory-supervisor`.

**Tech Stack:** Rust, the existing `factory-supervisor` request, queue and scheduler code.

## Global Constraints

- Commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`. Never `--workspace` or `--all`.
- No `Cargo.toml` change, no `git` shell-outs, no `/proc` reads (process checks go through `procinfo.rs`).
- Request files stay compatible: older files parse as before, the new action is additive, and `deny_unknown_fields` stays.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call. Comments are short full sentences.

## Review Focus

1. An old `Run` or `Approve` request file still parses and runs. Pinned by `old_request_files_still_parse`.
2. An `Intake` request with a series flag or approve flags is refused. Pinned by `an_intake_request_refuses_run_and_approve_flags`.
3. A project with intake off never gets a request. Pinned by `intake_polling_is_off_for_projects_without_the_setting`.
4. A second request is not queued while one is queued or running. Pinned by `an_intake_request_is_not_enqueued_twice`.
5. With the daily budget used up the `Intake` request waits, like `Run`. Pinned by `an_intake_request_waits_for_the_daily_budget`.
6. An intake job files no follow-up issue and no minor ticket, with `tickets_on_merge` on or off. Pinned by `the_ticket_step_skips_an_intake_job` and `the_minor_ticket_step_skips_an_intake_job`.
7. A job without the marker behaves as before. Pinned by the existing ticket and minor tests, which must keep passing.

## File Structure

| File | Responsibility |
|---|---|
| `crates/factory-supervisor/src/request.rs`, `scheduler.rs`, `spawn.rs` | The action, its validation, scheduling and child arguments |
| `crates/factory-supervisor/src/supervisor.rs` | The poll timer |
| `crates/factory-engine/src/tickets.rs`, `minors.rs` | Skip jobs that carry `intake_issue` |
| `docs/reference/software-factory.md`, `docs/guides/factory-user-guide.md` | The feature description |

### Task 1: The `Intake` action

**Files:**
- Modify: `crates/factory-supervisor/src/request.rs`, `crates/factory-supervisor/src/scheduler.rs`, `crates/factory-supervisor/src/spawn.rs`, `crates/factory-supervisor/src/supervisor.rs` (outcome classification)
- Test: unit tests in the same files

**Interfaces:**
- Consumes: `Action`, `Request::new`, `validate_job`, `decide`, the child argument builder in `spawn.rs` and `classify` in `supervisor.rs`.
- Produces: `Action::Intake` (serialised `intake`); `Request::new(project, Action::Intake, "000", false, ts)` is valid; the scheduler holds an `Intake` while `daily_exhausted`, like `Run`, by extending the existing rule at `scheduler.rs:52` and not adding a second one; the child argv for `Intake` is `--project <p> intake --once`; `classify` reads the last stdout line of an intake child (`merged #n ...`, `failed #n ...`, `no issue`, `intake is off`) as a finished outcome, not an error.

- [ ] **Step 1: Read `request.rs`, `scheduler.rs` (`decide` and its tests), the argument building in `spawn.rs`, and `classify` with its tests in `supervisor.rs`.**
- [ ] **Step 2: Write the failing tests.** `old_request_files_still_parse` (JSON of a `run` and an `approve` request written without the new action). `an_intake_request_refuses_run_and_approve_flags` (series true, `without_review`, `waive_reason` each give an error). `an_intake_request_waits_for_the_daily_budget` (a `decide` input with `daily_exhausted`, one `Intake`: no decision). `an_intake_request_starts_on_an_idle_project`. `intake_child_arguments` (argv is `--project p intake --once`). `classify_reads_an_intake_end` (the four stdout lines above each classify as a settled outcome).
- [ ] **Step 3: Run `cargo test -j4 -p factory-supervisor`.** Expected: `no variant named Intake found for enum Action`.
- [ ] **Step 4: Implement** the variant, the validation (no series, no approve flags, job `000`), the scheduler rule, the argument list and the classification.
- [ ] **Step 5: Run `cargo test -j4 -p factory-supervisor` then `cargo clippy -j4 -p factory-supervisor`.** Expected: all pass, no warnings.

### Task 2: Intake jobs skip the ticket steps

**Files:**
- Modify: `crates/factory-engine/src/tickets.rs`, `crates/factory-engine/src/minors.rs`
- Test: the existing ticket and minor test files in `crates/factory-engine/tests/` (find them with the names `tickets` and `minors`)

**Interfaces:**
- Consumes: `Job.intake_issue` from the first plan; the entry functions of the two steps (`tickets_step` style entry in `tickets.rs` and `minor_tickets_step` in `minors.rs`) and the `FakeTracker` call log.
- Produces: both steps return their existing "nothing to do" result at once for a job whose `intake_issue` is `Some`. The engine logic stays behind the `Tracker` port.

- [ ] **Step 1: Read the entry functions of `tickets.rs` and `minors.rs` and the tests that drive them with `FakeTracker` and `with_tracker`.**
- [ ] **Step 2: Write the failing tests.** `the_ticket_step_skips_an_intake_job`: a merged job with `intake_issue = Some(7)`, a spec with a `## Follow-ups` bullet, issue `#7` in its issue list and `tickets_on_merge = true`; after the step the tracker has no `create`, `comment` or `close` call and no ticket marker file was written. `the_minor_ticket_step_skips_an_intake_job`: a job with `intake_issue = Some(7)` and a review file with a `[minor]` line; no `create` call. Reuse the fixture of the neighbouring non-intake test for each.
- [ ] **Step 3: Run `cargo test -j4 -p factory-engine skips_an_intake_job`.** Expected: both fail, the tracker saw a `create` call.
- [ ] **Step 4: Implement the early return at the top of both entry functions.**
- [ ] **Step 5: Run `cargo test -j4 -p factory-engine` then `cargo clippy -j4 -p factory-engine`.** Expected: all pass, no warnings.

### Task 3: The poll timer and the docs

**Files:**
- Modify: `crates/factory-supervisor/src/supervisor.rs`, `docs/reference/software-factory.md`, `docs/guides/factory-user-guide.md`
- Test: unit tests in `crates/factory-supervisor/src/supervisor.rs` or the supervisor test file the existing tick tests use

**Interfaces:**
- Consumes: the supervisor `tick`, the way it loads each project's settings (`project_states`) and the queue enqueue call used by `factoryd enqueue`; `Config::intake`, `intake_poll_minutes` from the first plan; `Action::Intake` from the previous task.
- Produces: in `tick`, for each project whose config has `intake = true`, one `Intake` request is enqueued when the last enqueue for that project is older than `intake_poll_minutes` and no request of that project is queued or running; the last time is kept in memory and starts at zero so the first tick enqueues.

- [ ] **Step 1: Read `tick`, `project_states`, the queue module's enqueue and list calls, and the tests that drive `tick` with a fake spawner and clock.**
- [ ] **Step 2: Write the failing tests.** `intake_polling_is_off_for_projects_without_the_setting`; `an_intake_request_is_enqueued_when_intake_is_on` (one request after the first tick); `an_intake_request_is_not_enqueued_twice` (a second tick before the interval, and a tick while the first request is queued); `the_next_request_waits_for_the_poll_interval` (advance the fake clock by `intake_poll_minutes` and see one more).
- [ ] **Step 3: Run `cargo test -j4 -p factory-supervisor intake`.** Expected: the tests fail, no request is enqueued.
- [ ] **Step 4: Implement the timer in `tick`, then write the docs.** The reference gets one paragraph on the intake flow, the labels, the trust rule, the states and the skipped ticket steps; the user guide gets a section on turning it on, labelling an issue, what each `factory:` state means and how to relabel a failed issue to retry.
- [ ] **Step 5: Run `cargo test -j4 -p factory-supervisor` then `cargo clippy -j4 -p factory-supervisor`.** Expected: all pass, no warnings.

Next plan: none
