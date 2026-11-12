# Soft step safety Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-12-soft-step-safety-design.md
> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Make every post-merge soft step write its failure to the ledger, survive a panic, and free the job lease on any unwind.

**Architecture:** `merge.rs` gains a `catch_unwind` wrapper and one reporting rule for the five post-merge calls. `notify_failure` takes a step name. `run_job` holds a Drop guard for the job lease. The logic sits behind the existing `Tracker` port for the tickets, waivers and minors steps; no new trait.

**Tech Stack:** Rust, `std::panic::catch_unwind`, `tracing`.

## Global Constraints

- Commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`. Never `--workspace` or `--all`.
- Do not change `MergeResult` or the job stage for any soft step outcome. Do not touch any `Cargo.toml`.
- No board, ledger or events field is added. A ledger line is one folded line of `Ruling:` text.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call. Comments are short full sentences.

## Review Focus

1. A waiver or minor failure shows its own step name in the events, not `tickets step:`. Pinned by `a_waiver_failure_is_labelled_waivers_in_the_events`.
2. A tracker failure still writes exactly one ledger line, with no `error` line next to it. Pinned by `a_tracker_failure_still_writes_one_ledger_line`.
3. A panic payload that is not a string gives the text `unknown panic`. Pinned by `run_soft_step_turns_every_payload_kind_into_text`.
4. An `Err` from `kb_refresh` or the costs refresh writes no ledger line, as today, while their panic does. Pinned by `a_kb_or_costs_error_gets_no_ledger_line` and `a_kb_or_costs_panic_gets_a_ledger_line`.
5. A panic in one soft step does not stop the later steps. Pinned by `a_panicking_tracker_does_not_skip_the_waiver_and_minor_steps`.
6. A panic in the middle of a series does not end it. Pinned by `a_series_continues_after_a_panicking_soft_step`.
7. No profile sets `panic = "abort"`, which would silently turn the boundary off. Pinned by `no_panic_abort_profile`.

## File Structure

| File | Responsibility |
|---|---|
| `crates/factory-engine/src/merge.rs` | `StepFailure`, `run_soft_step`, `ledger_line`, `report_soft_step`, the five wrapped calls, unit tests |
| `crates/factory-engine/src/tickets.rs` | `notify_failure` takes a step name |
| `crates/factory-engine/src/runloop.rs` | `LeaseGuard` in `run_job` |
| `crates/factory-engine/tests/common/mod.rs` | `PanicTracker`, `with_boxed_tracker` |
| `crates/factory-engine/tests/tickets.rs`, `waivers.rs`, `runloop.rs` | New integration tests |
| `crates/factory-cli/tests/no_panic_abort.rs` | Source test for the build profile |

### Task 1: The step name and the ledger line for an error

**Files:**
- Modify: `crates/factory-engine/src/merge.rs` (`report_soft_step` at ~81, the callers at ~118-140)
- Modify: `crates/factory-engine/src/tickets.rs` (`notify_failure` at ~391)
- Test: `crates/factory-engine/tests/tickets.rs`, `crates/factory-engine/tests/waivers.rs`

**Interfaces:**
- Consumes: `Driver::job(id)`, `Driver::ledger(&job).ruling(&str)`, the folding used by `finish` in `tickets.rs` (split on whitespace, join with one space).
- Produces: `pub(crate) fn notify_failure(driver: &Driver, id: &str, step: &str, text: &str)` emitting the note `{step} step: {text}`; a private `fn write_step_ledger(driver: &Driver, id: &str, line: &str)` in `merge.rs` that folds the line, writes it with `ruling`, skips silently when `driver.job(id)` fails, and logs a failed write as `step.error` with `step = "<step>_ledger"` without raising. The `Err` arm of `report_soft_step` writes `<step> error: <message>` through it. The `Ok(report)` with `failed` arm adds no ledger line.

- [ ] **Step 1: Read the code this task touches.** Read `report_soft_step` and `merge_stage` in `merge.rs`, `finish` and `notify_failure` in `tickets.rs`, the ledger writes at `waivers.rs:182` and `minors.rs:79`, and the tests `a_broken_marker_after_a_merge_is_reported_and_the_job_stays_merged` and `a_failing_tracker_never_changes_the_merge_result` in `tests/tickets.rs`, with the helpers `approved_with_hook`, `events_text` and `tracker`.
- [ ] **Step 2: Write the failing tests.** In `tests/tickets.rs`:

```rust
#[test]
fn a_broken_marker_after_a_merge_writes_a_ledger_line() {
    let t = tracker();
    let fx = approved_with_hook(true, Some(SPEC_WITH_TWO), &t);
    let job = fx.driver.job(&fx.id).expect("job");
    std::fs::write(fx.driver.job_dir(&job).join("tickets.json"), "{ not json").expect("marker");
    assert!(matches!(
        merge_stage(&fx.driver, &fx.id).expect("merge"),
        MergeResult::Merged(_)
    ));
    let ledger =
        std::fs::read_to_string(fx.driver.job_dir(&job).join("progress.md")).expect("ledger");
    assert!(ledger.contains("tickets error"), "{ledger}");
    assert!(ledger.contains("not valid JSON"), "{ledger}");
    assert_eq!(
        fx.driver.job(&fx.id).expect("job").stage,
        factory_core::Stage::Merged
    );
}
```

  Add `a_tracker_failure_still_writes_one_ledger_line` in the same file: set it up as `a_failing_tracker_never_changes_the_merge_result` does, then assert the ledger has exactly one line containing `tickets failed` and none containing `tickets error`. In `tests/waivers.rs` add `a_waiver_failure_is_labelled_waivers_in_the_events`: build the tracker with `FakeTracker::new("o/r")`, call `fail_on("create")`, merge a job with `two_blockers()` through the file's `merged` helper, read `fx.driver.events_path()`, and assert the text contains `waivers step:` and does not contain `tickets step:`.
- [ ] **Step 3: Run them and see them fail.** Run `cargo test -j4 -p factory-engine --test tickets a_broken_marker_after_a_merge_writes_a_ledger_line` and `cargo test -j4 -p factory-engine --test waivers a_waiver_failure_is_labelled`. Expected: the ledger has no `tickets error`, and the events hold `tickets step:`.
- [ ] **Step 4: Implement `notify_failure(driver, id, step, text)` and `write_step_ledger`.** Pass `"tickets"`, `"waivers"` or `"minors"` from the three callers, through `report_soft_step`'s existing `step` argument, in both its arms. Update the event text to `{step} step: {text}`.
- [ ] **Step 5: Run the tests and see them pass.** Run `cargo test -j4 -p factory-engine --test tickets --test waivers --test minors` and `cargo clippy -j4 -p factory-engine`. Expected: all pass, including the unchanged `a_broken_marker_after_a_merge_is_reported_and_the_job_stays_merged`.

### Task 2: The panic boundary

**Files:**
- Modify: `crates/factory-engine/src/merge.rs` (`merge_stage` at ~102, `report_soft_step`)
- Modify: `crates/factory-engine/tests/common/mod.rs` (`FakeTracker` at ~696, `with_tracker` at ~817)
- Test: `crates/factory-engine/src/merge.rs` unit tests, `crates/factory-engine/tests/tickets.rs`, `crates/factory-engine/tests/runloop.rs`

**Interfaces:**
- Consumes: `notify_failure` and `write_step_ledger` from the previous task; `Tracker` (exported as `factory_engine::Tracker`); the fixtures `approved_with_hook`, `merged` in `tests/waivers.rs`, and `SeriesFixture` in `tests/common/mod.rs`.
- Produces, in `merge.rs`: `enum StepFailure { Error(EngineError), Panic(String) }`; `fn run_soft_step<T>(f: impl FnOnce() -> Result<T, EngineError>) -> Result<T, StepFailure>` using `catch_unwind(AssertUnwindSafe(f))`, with the payload text taken from a `&str` or `String` payload and `unknown panic` otherwise; `fn ledger_line(step: &str, failure: &StepFailure) -> Option<String>`, which gives `<step> panicked: <text>` for any panic, `<step> error: <text>` for an error of `tickets`, `waivers` or `minors`, and `None` for an error of `kb_refresh` or `costs_refresh`; `report_soft_step(driver, id, step, result: Result<TicketsReport, StepFailure>)`. In `tests/common/mod.rs`: `pub struct PanicTracker { pub inner: FakeTracker, pub panic_on: Option<&'static str> }` implementing `Tracker`, which panics in the named method (or in every method when `None`) and forwards the rest to `inner`; `pub fn with_boxed_tracker(fx: MergeFixture, tracker: Box<dyn Tracker>) -> MergeFixture`.

- [ ] **Step 1: Read the code this task touches.** Read `merge_stage`, `report_soft_step` and the unit tests in `merge.rs`; the `Tracker` trait and the `Arc<T>` forwarding impl in `tracker.rs`; `FakeTracker`, `with_tracker`, `SeriesFixture::new` and `plan_file` in `tests/common/mod.rs`; `run_series_chains_plans_and_each_job_writes_its_own_job_id_into_events` in `tests/runloop.rs`; and `waiver_tickets_step`, to see which tracker calls it makes.
- [ ] **Step 2: Write the failing unit tests** in `merge.rs`:

```rust
#[test]
fn run_soft_step_turns_every_payload_kind_into_text() {
    let a = run_soft_step::<()>(|| panic!("boom"));
    assert!(matches!(a, Err(StepFailure::Panic(ref t)) if t == "boom"));
    let b = run_soft_step::<()>(|| panic!("{}", String::from("owned")));
    assert!(matches!(b, Err(StepFailure::Panic(ref t)) if t == "owned"));
    let c = run_soft_step::<()>(|| std::panic::panic_any(7_u32));
    assert!(matches!(c, Err(StepFailure::Panic(ref t)) if t == "unknown panic"));
    let d = run_soft_step(|| Ok::<_, EngineError>(5));
    assert!(matches!(d, Ok(5)));
    let e = run_soft_step::<()>(|| Err(EngineError::Msg("bad".into())));
    assert!(matches!(e, Err(StepFailure::Error(_))));
}
```

  Add `a_kb_or_costs_error_gets_no_ledger_line` (an `Error` for `kb_refresh` and for `costs_refresh` gives `None`), `a_kb_or_costs_panic_gets_a_ledger_line` (`Panic("boom")` for `kb_refresh` gives `Some("kb_refresh panicked: boom")`) and `a_ticket_step_error_gets_a_ledger_line` (an `Error` for `tickets` gives a line starting `tickets error: `).
- [ ] **Step 3: Write the failing integration tests.** In `tests/tickets.rs`, build the fixture as `a_failing_tracker_never_changes_the_merge_result` does but install the tracker with `with_boxed_tracker`:

```rust
#[test]
fn a_panicking_tracker_after_a_merge_returns_merged() {
    let fx = approved_with_hook(true, Some(SPEC_WITH_TWO), &tracker());
    let fx = common::with_boxed_tracker(
        fx,
        Box::new(common::PanicTracker { inner: tracker(), panic_on: Some("repo_name") }),
    );
    assert!(matches!(
        merge_stage(&fx.driver, &fx.id).expect("merge"),
        MergeResult::Merged(_)
    ));
    let job = fx.driver.job(&fx.id).expect("job");
    assert_eq!(job.stage, factory_core::Stage::Merged);
    let ledger =
        std::fs::read_to_string(fx.driver.job_dir(&job).join("progress.md")).expect("ledger");
    assert!(ledger.contains("tickets panicked"), "{ledger}");
    assert!(events_text(&fx).contains("tickets step:"), "{}", events_text(&fx));
}
```

  Add `a_panicking_tracker_does_not_skip_the_waiver_and_minor_steps` in `tests/waivers.rs`: record `two_blockers()` on a job before the merge, install a `PanicTracker` with `panic_on: Some("repo_name")` around a `FakeTracker` and `tickets_on_merge` on, merge, and assert the inner tracker's `calls()` contains a `create` (the waiver step ran after the tickets step panicked). In `tests/runloop.rs` add `a_series_continues_after_a_panicking_soft_step`: build the two-plan series as `run_series_chains_plans_and_each_job_writes_its_own_job_id_into_events` does, record a waiver in the first job's folder with `record_waiver`, install a `PanicTracker` with `panic_on: None` on the driver, and assert `run_series` returns `RunEnd::Merged` with both jobs in the Merged stage.
- [ ] **Step 4: Run them and see them fail.** Run `cargo test -j4 -p factory-engine --lib run_soft_step` and `cargo test -j4 -p factory-engine --test tickets a_panicking_tracker`. Expected: `cannot find function 'run_soft_step' in this scope`, and the integration tests unwind with the panic.
- [ ] **Step 5: Implement the boundary.** Add `StepFailure`, `run_soft_step` and `ledger_line`. Route each of the five calls in `merge_stage` through `run_soft_step`. For `kb_refresh`, keep the current handling of an `Ok` text that starts with `kb refresh failed` and of an `Err`, and report a `Panic` through the same arm as the ticket steps. For `refresh_costs`, log an `Error` as today and report a `Panic` the same way. In the report arm, write the ledger line when `ledger_line` gives one and call `notify_failure` only then. Add `PanicTracker` and `with_boxed_tracker` to `tests/common/mod.rs`; `with_boxed_tracker` is `with_tracker` taking a boxed trait object.
- [ ] **Step 6: Run the tests and see them pass.** Run `cargo test -j4 -p factory-engine --lib`, `cargo test -j4 -p factory-engine --test tickets --test waivers --test minors --test runloop` and `cargo clippy -j4 -p factory-engine`. Expected: all pass.

### Task 3: The job lease guard and the build profile test

**Files:**
- Modify: `crates/factory-engine/src/runloop.rs` (`run_job` at ~589)
- Create: `crates/factory-cli/tests/no_panic_abort.rs`
- Test: `crates/factory-engine/tests/runloop.rs`

**Interfaces:**
- Consumes: `Driver::release_lease(&self, id: &str)`, `Driver::hold_project`, `SeriesFixture::owned(owner: &str, hook: RunHook)` with `RunHook = Arc<dyn Fn(&AgentSpec) + Send + Sync>` in `tests/common/mod.rs`, and `Job::lease: Option<Lease>`.
- Produces: a private `struct LeaseGuard<'a> { driver: &'a Driver, id: &'a str }` in `runloop.rs` whose `Drop` calls `self.driver.release_lease(self.id)`. `run_job` creates it right after `hold_project` and no longer calls `release_lease` itself, so the job lease drops before the project guard.

- [ ] **Step 1: Read the code this task touches.** Read `run_job`, `run_job_inner` and `run_series` in `runloop.rs`, `release_lease` and `JobGuard` in `compat.rs`, `SeriesFixture::owned` and `HookRunner` in `tests/common/mod.rs`, and `tests/lease_heartbeat.rs` for a test that asserts a lease on the board.
- [ ] **Step 2: Write the failing tests.** In `tests/runloop.rs`:

```rust
#[test]
fn a_panic_in_a_run_frees_the_job_lease() {
    let hook: common::RunHook = Arc::new(|_spec| panic!("agent exploded"));
    let fx = common::SeriesFixture::owned("tester", hook);
    let a = fx.plan_file("plan-01-a.md", 1, "none");
    let job = create_job(&fx.driver, "lease", "one plan", Some(&a), None).expect("create");
    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_job(&fx.driver, &job.id)
    }));
    assert!(ran.is_err(), "the hook panics, so the run unwinds");
    let after = fx.driver.job(&job.id).expect("job");
    assert!(after.lease.is_none(), "the lease must be freed: {:?}", after.lease);
}
```

  Create `crates/factory-cli/tests/no_panic_abort.rs` with `no_panic_abort_profile`: it finds the repo root from `env!("CARGO_MANIFEST_DIR")`, reads the root `Cargo.toml` and every `crates/*/Cargo.toml` and `dashboard/Cargo.toml`, and asserts none contains `panic = "abort"`, naming the file in the failure message. Use `read_dir` and `.expect("...")`, matching `no_hardcoded_paths.rs`.
- [ ] **Step 3: Run them and see them fail.** Run `cargo test -j4 -p factory-engine --test runloop a_panic_in_a_run_frees_the_job_lease`. Expected: the assertion fails because the lease is still held after the unwind. Run `cargo test -j4 -p factory-cli --test no_panic_abort`. Expected: it passes on unchanged code, which pins the current state.
- [ ] **Step 4: Implement `LeaseGuard`.** Add the guard and use it in `run_job`, keeping the declaration order so the project guard outlives it.
- [ ] **Step 5: Run the tests and see them pass.** Run `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli --test no_panic_abort` and `cargo clippy -j4 -p factory-engine -p factory-cli`. Expected: all pass.

Next plan: none
