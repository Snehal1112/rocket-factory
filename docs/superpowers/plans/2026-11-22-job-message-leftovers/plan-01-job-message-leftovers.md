# Job creation rollback and one-job create Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-22-job-message-leftovers-design.md
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** A failed `factory new` leaves no half-made job, the one-job count check and combine rules are tested, and factoryd and the dashboard can create a one-job.

**Commit:** fix(factory): roll back failed job creation

**Architecture:** Two helpers in `crates/factory-engine/src/runloop.rs` remove the jobs a failed creation made. The one-job count check becomes a pure function in `one_job.rs`. The supervisor's create route passes two new flags to `factory new` through the `FactoryCli` port, and the dashboard form sends one of them.

**Tech Stack:** Rust, plain JavaScript (`node --test`).

## Global Constraints

- Cargo commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- No new board, ledger or event field.
- The rollback never hides the original error, and never runs after `adopt_draft`.
- Refusal texts stay exactly as they are today.

## Review Focus

1. A rollback whose own board write fails: a user expects the first error, not the rollback's. Pinned in part by `a_failed_creation_removes_every_job_it_made`, which checks that the returned error is the one `f` gave. A failing rollback write is only logged, and no test forces it.
2. A series whose later job fails: a user expects the earlier jobs gone too. Pinned by `a_failed_creation_removes_every_job_it_made`, which creates two jobs.
3. The intake path, which also goes through `add_job_marked`: a user expects no leftover job. Pinned by the new assertion in `a_job_creation_io_error_fails_at_once_without_a_redraft`.
4. An old dashboard or client that sends no `one_job`: a user expects the body and the arguments unchanged. Pinned by the existing `buildCreateBody returns the body with run and series` and `create_runs_new_once_with_canonical_paths_then_queues_a_run`, which must pass unchanged.
5. `one_job: false` with `accept_over_budget: true`: a user expects a 400. Pinned as a case of `create_refuses_accept_over_budget_without_one_job`.

## File Structure

- Modify: `crates/factory-engine/src/runloop.rs`. Add `remove_new_jobs` and `rolled_back`, use them in `add_job_marked`, `create_job_with_spec_accepting` and `create_replacement`. Set `min_free_disk_mb: 0` in the `mod tests` helpers.
- Modify: `crates/factory-engine/src/one_job.rs`. Add `count_mismatch`, wrap the writes in `rolled_back`, add `mod tests`.
- Modify: `crates/factory-engine/src/combine.rs`. Add a sentence to the `split_plan` doc comment, and tests.
- Modify: `crates/factory-engine/src/lib.rs`. Add `NEEDS_CONSENT`.
- Modify: `crates/factory-engine/tests/one_job.rs`, `tests/intake.rs`, `tests/series.rs`.
- Modify: `crates/factory-supervisor/src/api/write.rs`.
- Modify: `dashboard/static/dashboard.html`, `dashboard/static/projects.js`, `dashboard/static/app.js`, `dashboard/tests/projects.test.js`, `README.md`.

---

### Task 1: a failed job creation removes what it made

**Files:**
- Modify: `crates/factory-engine/src/runloop.rs`, `crates/factory-engine/src/one_job.rs`
- Modify: `crates/factory-engine/tests/one_job.rs`, `crates/factory-engine/tests/intake.rs`

**Interfaces:**
- Port: job creation is driver code over `BoardStore` (`driver.store()`) and the job folders from `Driver::job_dir`. No trait is involved. Tests use `SeriesFixture` and the `driver(&tmp)` helper of `runloop.rs` `mod tests`.
- Produces: `pub(crate) fn remove_new_jobs(driver: &Driver, jobs: &[Job])` and `pub(crate) fn rolled_back<T>(driver: &Driver, f: impl FnOnce(&mut Vec<Job>) -> Result<T, EngineError>) -> Result<T, EngineError>`, both in `runloop.rs`.

**Tests added:** `a_failed_creation_removes_every_job_it_made`, `a_finished_creation_keeps_its_jobs`, `unit_test_drivers_skip_the_disk_check` (in `runloop.rs` `mod tests`), `a_failed_job_folder_leaves_no_job_on_the_board` (in `tests/one_job.rs`), and one more assertion in `a_job_creation_io_error_fails_at_once_without_a_redraft` (in `tests/intake.rs`).

- [ ] **Step 1: Read the code**

Read these in `crates/factory-engine/src/runloop.rs`:
- `add_job_marked`, and `create_job_with_spec_accepting` with its `copy_spec` closure;
- the inline cleanup in `create_replacement` that starts with the comment "The board may have changed while the spec was checked";
- in `mod tests`, the helpers `driver`, `driver_with` and `risky_config`, and the test `add_job_marked_writes_the_marker_with_the_job`.

Also read:
- `create_one_job_accepting` in `crates/factory-engine/src/one_job.rs`;
- `over_budget_without_consent_creates_nothing` and `four_plans` in `crates/factory-engine/tests/one_job.rs`;
- `a_job_creation_io_error_fails_at_once_without_a_redraft` in `crates/factory-engine/tests/intake.rs`;
- `test_config` in `crates/factory-engine/tests/common/mod.rs`;
- the spec's Design section 1.

- [ ] **Step 2: Write the failing tests**

In `runloop.rs` `mod tests`:

```rust
#[test]
fn a_failed_creation_removes_every_job_it_made() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let d = driver(&tmp);
    let err = rolled_back(&d, |made| -> Result<(), EngineError> {
        for slug in ["one", "two"] {
            let job = add_job(&d, slug, "f", &[], None, Stage::Build, None)?;
            made.push(job);
        }
        Err(EngineError::Msg("boom".to_string()))
    })
    .expect_err("fails");
    assert_eq!(err.to_string(), EngineError::Msg("boom".to_string()).to_string());
    assert!(d.store().load().expect("board").jobs.is_empty());
    let left = std::fs::read_dir(&d.layout().jobs_dir).map(|rd| rd.count()).unwrap_or(0);
    assert_eq!(left, 0);
}
```

| Test | Setup | Assertion |
|---|---|---|
| `a_finished_creation_keeps_its_jobs` | the same two `add_job` calls inside `rolled_back`, then `Ok(())` | `.expect("ok")`; the board has 2 jobs; both `d.job_dir(&job)` folders exist |
| `unit_test_drivers_skip_the_disk_check` | `driver(&tmp)`, and `driver_with(&tmp2, risky_config())` | `config().min_free_disk_mb == 0` for both (use the driver's config accessor) |
| `a_failed_job_folder_leaves_no_job_on_the_board` (`tests/one_job.rs`) | `fixture(10_000_000)` and `four_plans`. Then replace `fx.driver.layout().jobs_dir` with a file, as `a_job_creation_io_error_fails_at_once_without_a_redraft` does. Then call `create_one_job` | `expect_err`; `fx.driver.store().load().expect("board").jobs.is_empty()` |

In `a_job_creation_io_error_fails_at_once_without_a_redraft`, add after the existing assertions: `assert!(it.driver.store().load().expect("board").jobs.is_empty());`.

- [ ] **Step 3: Run them and see them fail**

Run: `cargo test -j4 -p factory-engine --lib runloop::tests`
Expected: the build fails with ``cannot find function `rolled_back` in this scope``.

- [ ] **Step 4: Implement**

- `remove_new_jobs`:
  - One `driver.store().update` drops every board job whose id is in `jobs`.
  - Then `std::fs::remove_dir_all(driver.job_dir(job))` runs for each job.
  - A folder that does not exist is skipped without a warning. Each other failure is logged with `tracing::warn!(event = "job.rollback", ...)` and dropped.
- `rolled_back`: start with `let mut made = Vec::new()`, call `f(&mut made)`, and on `Err` call `remove_new_jobs(driver, &made)` before returning the error.
- `add_job_marked`: run the `create_dir_all`, the ledger `create` and the series `plan.md` copy in one inner block. On its error call `remove_new_jobs(driver, std::slice::from_ref(&job))` and return the error. The estimate write stays as it is.
- `create_job_with_spec_accepting`:
  - The plan-less branch stays as it is, because `add_job` already rolls back its own job.
  - The series branch runs the first `add_job`, every later `add_job` and each `copy_spec` inside one `rolled_back`, pushing each job as it is made, and returns the first job.
  - `adopt_draft` and everything after it stay outside.
- `create_one_job_accepting`: run `add_job` and the `plan.md` and `spec.md` writes inside `rolled_back`.
- `create_replacement`: replace the inline cleanup with `remove_new_jobs(driver, std::slice::from_ref(&new))` and keep the comment.
- In `mod tests`, `driver` passes `Config { min_free_disk_mb: 0, ..Config::default() }`, and `risky_config` sets `min_free_disk_mb: 0`.
- Give both helpers a one-line doc comment.

- [ ] **Step 5: Run them and see them pass**

Run: `cargo test -j4 -p factory-engine --lib runloop::tests`, then `cargo test -j4 -p factory-engine --test one_job`, `cargo test -j4 -p factory-engine --test intake`, `cargo test -j4 -p factory-engine --test new_job`, `cargo test -j4 -p factory-engine --test runloop` and `cargo test -j4 -p factory-engine --test series_guard`.
Expected: all pass, 0 failed.

### Task 2: count check, combined marks and the task cap are pinned

**Files:**
- Modify: `crates/factory-engine/src/one_job.rs`, `crates/factory-engine/src/combine.rs`
- Modify: `crates/factory-engine/tests/series.rs`

**Interfaces:**
- Port: pure plan parsing, with no trait.
- Consumes: `crate::tasks::parse_checkbox_tasks`, `combine_plans(plans: &[(String, String)])`, and `parse_series_with(root, start, max_tasks)`.
- Produces: `fn count_mismatch(text: &str, expected: usize) -> Option<String>` in `one_job.rs`, private.

**Tests added:** `count_mismatch_names_both_counts`, `count_mismatch_is_none_when_the_counts_agree` (new `mod tests` in `one_job.rs`), `mixed_forms_write_checkbox_tasks_as_headings_with_a_warning`, `mixed_forms_drop_the_done_mark` (in `combine.rs` `mod tests`), `parse_series_with_a_cap_of_two_refuses_three_tasks` (in `tests/series.rs`).

- [ ] **Step 1: Read the code**

Read the count check in `create_one_job_accepting` (`one_job.rs`), `combine_plans`, `form_of`, `split_plan` and the test `checkbox_form_tasks_are_renumbered` in `combine.rs`, and the helper `plan` and `parse_series_with_none_accepts_five_tasks` in `crates/factory-engine/tests/series.rs`. Read the spec's Design sections 2 to 4.

- [ ] **Step 2: Write the tests**

```rust
#[test]
fn count_mismatch_names_both_counts() {
    let text = "### Task 1: a\n\n### Task 2: b\n";
    assert_eq!(
        count_mismatch(text, 3).as_deref(),
        Some("combining the plans failed: the combined plan has 2 tasks but 3 were expected")
    );
}
```

| Test | Input | Assertion |
|---|---|---|
| `count_mismatch_is_none_when_the_counts_agree` | the same text, `2` | `is_none()` |
| `mixed_forms_write_checkbox_tasks_as_headings_with_a_warning` | plan a = `"# A\n\n### Task 1: first\n\nNext plan: plan-02.md\n"`, plan b = `"# B\n\n- [ ] Task 1: second\n\nNext plan: none\n"`, through `combine_plans` | `tasks == 2`; the text contains `### Task 2: second`; one warning contains `its checkbox tasks are written as` and the path of b |
| `mixed_forms_drop_the_done_mark` | the same, with b's task written `- [x] Task 1: second` | the text contains `### Task 2: second` and no `[x]` |
| `parse_series_with_a_cap_of_two_refuses_three_tasks` | `plan(d.path(), "plan-01-x.md", 2, "none")`, then a second folder with 3 tasks | `parse_series_with(.., Some(2))` accepts 2 with `tasks == vec![1, 2]`; on 3 it matches `SeriesError::TaskCount { found: 3, .. }` |

- [ ] **Step 3: Run them**

Run: `cargo test -j4 -p factory-engine --lib count_mismatch`
Expected: the build fails with ``cannot find function `count_mismatch` in this scope``.

Run: `cargo test -j4 -p factory-engine --test series parse_series_with_a_cap_of_two`
Expected: it passes, because it pins today's behaviour. The `--lib` mixed-forms tests cannot build until `count_mismatch` exists, so they run in Step 5.

- [ ] **Step 4: Implement**

- Add `count_mismatch` with the exact text above, and call it from `create_one_job_accepting` in place of the inline check: `if let Some(why) = count_mismatch(&combined.text, combined.tasks) { return Err(EngineError::Msg(why)); }`.
- Add to the `split_plan` doc comment: "A checked `- [x]` mark is dropped. The ledger, not the mark, says which tasks are done."

- [ ] **Step 5: Run them**

Run: `cargo test -j4 -p factory-engine --lib count_mismatch`, `cargo test -j4 -p factory-engine --lib combine::tests::mixed_forms`, then `cargo test -j4 -p factory-engine --test one_job`.
Expected: all pass, 0 failed.

### Task 3: factoryd and the dashboard create a one-job

**Files:**
- Modify: `crates/factory-engine/src/lib.rs`, `crates/factory-supervisor/src/api/write.rs`
- Modify: `dashboard/static/dashboard.html`, `dashboard/static/projects.js`, `dashboard/static/app.js`, `dashboard/tests/projects.test.js`, `README.md`

**Interfaces:**
- Port: behind the `FactoryCli` trait (`crates/factory-supervisor/src/factory_cli.rs`). Tests use `FakeCli` from `src/api/testutil.rs`.
- Produces: `pub const NEEDS_CONSENT: &str = "needs consent:";` in `factory_engine`. `CreateBody` gains `one_job: Option<bool>` and `accept_over_budget: Option<bool>`. `buildCreateBody(form)` reads `form.oneJob`.

**Tests added:** `create_passes_one_job_and_accept_over_budget_before_the_separator`, `create_refuses_accept_over_budget_without_one_job`, `create_answers_needs_consent_with_a_conflict` (in `write.rs` `mod tests`), `needs_consent_text_starts_with_the_const` (in a `#[cfg(test)] mod tests` in `crates/factory-engine/src/lib.rs`, so `--lib needs_consent` finds it), and `buildCreateBody adds one_job only when checked` (in `dashboard/tests/projects.test.js`).

- [ ] **Step 1: Read the code**

Read `CreateBody`, `create_inner`, `finished` and the tests `create_runs_new_once_with_canonical_paths_then_queues_a_run`, `create_passes_accept_spec_joined_to_its_flag`, `create_refusal_is_a_conflict` and `create_other_failures_stay_failures` in `crates/factory-supervisor/src/api/write.rs`. Read `EngineError::NeedsConsent` and the `SPEC_OVERRIDE_HINT` export in `crates/factory-engine/src/lib.rs`. Read `buildCreateBody` in `dashboard/static/projects.js`, the `nj-*` form in `dashboard/static/dashboard.html`, the `nj-submit` handler in `dashboard/static/app.js` and the `buildCreateBody` tests. Read the README row for `POST /projects/{p}/jobs`. Read the spec's Design section 5.

- [ ] **Step 2: Write the failing tests**

```rust
#[test]
fn create_passes_one_job_and_accept_over_budget_before_the_separator() {
    let f = fx();
    f.cli.reply_stdout("created job 007 in 2026-10-17-007-work\n");
    let mut body = create_body(&f);
    body["one_job"] = serde_json::json!(true);
    body["accept_over_budget"] = serde_json::json!(true);
    let r = post(&f, "/projects/good/jobs", body.to_string().as_bytes());
    assert_eq!(code(&r).0, 201);
    let args = &f.cli.calls()[0].1;
    let sep = args.iter().position(|a| a == "--").expect("separator");
    let one = args.iter().position(|a| a == "--one-job").expect("one-job");
    let over = args.iter().position(|a| a == "--accept-over-budget").expect("over");
    assert!(one < sep && over < sep, "{args:?}");
}
```

Check `post`, `create_body`, `code` and the `FakeCli` method names against the file before using them.

| Test | Input | Assertion |
|---|---|---|
| `create_refuses_accept_over_budget_without_one_job` | `accept_over_budget: true` alone, then with `one_job: false` | each `(400, "bad_request")`, the message contains `accept_over_budget needs one_job`; `f.cli.calls()` is empty; `queued(&f) == 0` |
| `create_answers_needs_consent_with_a_conflict` | `f.cli.reply(1, "", "needs consent: this job is estimated at 2 weighted tokens and the budget is 1; raise budget_weighted in project.toml or pass --accept-over-budget\n")` with `one_job: true` | `(409, "conflict")`; the message contains `--accept-over-budget`; `queued(&f) == 0`. Then again with stderr `"note: x\nneeds consent: this job is estimated at 2 weighted tokens and the budget is 1; raise budget_weighted in project.toml or pass --accept-over-budget\n"`: also `(409, "conflict")` |
| `needs_consent_text_starts_with_the_const` | `EngineError::NeedsConsent { estimate: 2, budget: 1 }.to_string()` | `starts_with(NEEDS_CONSENT)` |
| `buildCreateBody adds one_job only when checked` | `{ ...FORM, oneJob: true }`, then `{ ...FORM, oneJob: false }` | the first deep-equals `{ spec: "/a/spec.md", plans: "/a/plans", run: true, series: true, one_job: true }`; the second has no `one_job` key |

- [ ] **Step 3: Run them and see them fail**

Run: `cargo test -j4 -p factory-supervisor --lib api::write::tests::create_`
Expected: `create_passes_one_job_and_accept_over_budget_before_the_separator` fails with 400, because `deny_unknown_fields` refuses `one_job`. The other new tests fail too.

Run: `node --test dashboard/tests/projects.test.js`
Expected: `buildCreateBody adds one_job only when checked` fails.

- [ ] **Step 4: Implement**

- Export `NEEDS_CONSENT` from `factory_engine`. Leave the `#[error]` text of `NeedsConsent` unchanged.
- In `create_inner`:
  - After the body is parsed, `accept_over_budget == Some(true)` with `one_job != Some(true)` returns `bad("accept_over_budget needs one_job")`, before anything runs.
  - Push `--one-job` and then `--accept-over-budget` after the `--accept-spec` argument and before `--`, each only when its field is `Some(true)`.
  - Treat an exit code 1 as a conflict when any line of stderr, or of stdout when stderr is blank, starts with `NEEDS_CONSENT` after `trim_start`, next to the `SPEC_OVERRIDE_HINT` check. Under `--project` the CLI can print `note:` lines before it.
- Dashboard:
  - Add `<label class="check"><input id="nj-onejob" type="checkbox"> One job</label>` after `nj-series`.
  - `app.js` passes `oneJob: byId("nj-onejob").checked`.
  - `buildCreateBody` adds `one_job: true` only when `form.oneJob === true`.
- README: in the `POST /projects/{p}/jobs` row, add `"one_job"?` and `"accept_over_budget"?` to the body. Say that they become `--one-job` and `--accept-over-budget`, that `accept_over_budget` needs `one_job`, and that a `needs consent:` refusal answers `409 conflict`.

- [ ] **Step 5: Run them and see them pass**

Run: `cargo test -j4 -p factory-supervisor`, `cargo test -j4 -p factory-engine --lib needs_consent`, `node --test dashboard/tests/*.test.js`.
Expected: all pass, 0 failed.

- [ ] **Step 6: Format and lint**

Run: `cargo fmt -p factory-engine`, `cargo fmt -p factory-supervisor`, `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`, `cargo clippy -j4 -p factory-supervisor --all-targets -- -D warnings`.
Expected: clippy finishes with no warnings.

Next plan: docs/superpowers/plans/2026-11-22-job-message-leftovers/plan-02-job-message-leftovers.md
