# Engine small fixes, plan 2: check results that survive a write error

> **Spec:** docs/superpowers/specs/2026-11-22-engine-small-fixes-design.md

> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Checks that ran keep their result when `checks.md` cannot be written (#50), and a rework after failing checks gets the failing output, not only the `FAIL` lines (#25).

**Commit:** fix(checks): keep results when checks.md fails

**Architecture:** A new helper in `checks_stage.rs` writes the check files. It removes the old `checks.md` and `checks-failures.md`, writes `checks.md` and its notes, writes `checks-failures.md` with `ChecksReport::failure_text()` when a check failed, and returns the write errors instead of raising them. `run_checks_now` logs those errors and goes on with the report. `open_findings` in `rework.rs` reads `checks-failures.md` first.

**Tech Stack:** Rust, `tempfile`, `tracing`.

## Global Constraints

- cargo commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`, never --workspace or --all.
- No new board, ledger or events field. `checks-failures.md` is a new job-folder file that only the driver writes.
- The `checks.md` format does not change.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.

## File Structure

| File | Change |
|---|---|
| `crates/factory-engine/src/checks_stage.rs` | `CHECKS_FAILURES_FILE`, `save_check_files`; `run_checks_now` (lines 84-104) uses it; unit tests. |
| `crates/factory-engine/src/lib.rs` | Re-export `CHECKS_FAILURES_FILE` next to the other job-file constants such as `MINORS_FILE`. |
| `crates/factory-engine/tests/park_evidence.rs` | `final_checks_that_cannot_run_after_a_sync_park_risky_and_say_why` (line 174) rewritten into two tests; one assertion added to `failing_final_checks_after_a_sync_park_as_checks_failed_without_a_fix_round`. |
| `crates/factory-engine/src/rework.rs` | `open_findings` (lines 30-47) reads `checks-failures.md` first; unit tests. |
| `docs/reference/software-factory.md:46` | Add `checks-failures.md` to the job directory list. |

## Review Focus

1. A job folder where `checks.md` is a directory: removal fails with something other than NotFound. A user expects that error logged and the run to go on, not a panic or an early return. Pinned by `a_failed_checks_md_write_after_a_sync_keeps_a_passing_result`.
2. A `checks-failures.md` that holds only whitespace: a user expects the rework to fall back to the `FAIL` lines. Pinned by `rework_findings_fall_back_to_fail_lines_without_the_failures_file` (its blank-file case).
3. A rework whose last review asked for changes and whose park is also `checks-failed`: the review text still wins, as today. Pinned by `rework_findings_prefer_the_review_over_the_failures_file`.
4. A run with zero mapped checks (`PASS none`): a user expects no `checks-failures.md`. Pinned by `save_check_files_writes_no_failures_for_an_empty_report`.
5. Checks that truly cannot run (no worktree): a user still expects "final checks could not run". This stays pinned by the unit test `final_checks_that_cannot_run_keep_the_risky_kind_and_say_why` in `park.rs` (about line 620), which must keep passing.

---

### Task 1: Keep the check report when a write fails

**Files:**
- Modify: `crates/factory-engine/src/checks_stage.rs:84-104`
- Modify: `crates/factory-engine/src/lib.rs`
- Test: `crates/factory-engine/src/checks_stage.rs` (`mod tests`; add one if the file has none), `crates/factory-engine/tests/park_evidence.rs`

**Interfaces:**
- Consumes: `factory_checks::{write_checks_md, append_notes, ChecksReport, CheckResult}`, `factory_core::write_atomic`.
- Produces: `pub const CHECKS_FAILURES_FILE: &str = "checks-failures.md";` and `pub(crate) fn save_check_files(job_dir: &Path, report: &ChecksReport, notes: &[String]) -> Vec<String>`. The return value is one line per write or removal error, each naming the file, for example `checks.md: <err>`.
- Port: none. The checks run through `factory_checks` directly, as today.

- [ ] **Step 1: Read the code.** Read `crates/factory-engine/src/checks_stage.rs:60-140`, `crates/factory-checks/src/run.rs:15-60` and `run.rs:226-256`, `crates/factory-engine/src/park.rs:127-150`, `crates/factory-engine/tests/park_evidence.rs:94-260` and `crates/factory-core/src/fsutil.rs:6-26`. The temp file of `write_atomic` is `.{name}.{pid}.tmp` in the same folder, and the tests below use that to make a write fail.

- [ ] **Step 2: Write the failing unit tests** in `checks_stage.rs`:

```rust
fn result(command: &str, passed: bool, tail: &str) -> factory_checks::CheckResult {
    factory_checks::CheckResult {
        command: command.to_string(),
        passed,
        summary: if passed { "ok".into() } else { "exit 1".into() },
        tail: tail.to_string(),
    }
}

/// Makes the next `write_atomic` of `name` in `dir` fail, by putting a folder at its temp path.
fn block_write(dir: &Path, name: &str) {
    std::fs::create_dir(dir.join(format!(".{name}.{}.tmp", std::process::id()))).expect("block");
}

#[test]
fn save_check_files_writes_failures_for_a_failing_report() {
    let dir = tempfile::tempdir().expect("tempdir");
    let report = ChecksReport { results: vec![result("cargo test", false, "error[E0425]: x")] };
    assert!(save_check_files(dir.path(), &report, &[]).is_empty());
    let text = std::fs::read_to_string(dir.path().join(CHECKS_FAILURES_FILE)).expect("failures");
    assert_eq!(text, report.failure_text());
    assert!(text.contains("error[E0425]: x"), "{text}");
}
```

Add these with the same shape:

| Test | Setup | Assertions |
|---|---|---|
| `save_check_files_removes_old_failures_on_a_pass` | write `checks-failures.md` with `old`; report with one passing result | no errors; `checks-failures.md` does not exist; `checks.md` starts with `# Checks` and has `PASS cargo test` |
| `save_check_files_writes_no_failures_for_an_empty_report` | `ChecksReport { results: vec![] }` | no errors; no `checks-failures.md`; `checks.md` has `PASS none` |
| `save_check_files_leaves_no_stale_pass_when_the_write_fails` | write `checks.md` with `# Checks\n\nPASS old (ok)\n`; `block_write(dir, "checks.md")`; failing report | one error that starts with `checks.md:`; `checks.md` does not exist; `checks-failures.md` holds `failure_text()` |

- [ ] **Step 3: Run them.** `cargo test -j4 -p factory-engine save_check_files`. Expected: compile error `cannot find function 'save_check_files' in this scope`.

- [ ] **Step 4: Implement** `save_check_files`. In order:
  1. Remove `checks.md` and `checks-failures.md`. A NotFound error is fine; any other error is pushed as a line.
  2. `write_checks_md`, then `append_notes` only when that write succeeded. Each error is pushed.
  3. When `!report.all_passed()`, `write_atomic` of `failure_text()` to `checks-failures.md`. An error is pushed.

  In `run_checks_now`, replace the two `?` lines with `let errors = save_check_files(&self.job_dir, &report, &plan.notes);`. For each error, emit `tracing::error!(event = "step.error", step = "checks_md", msg = %format!("checks.md could not be written: {e}"))` and `self.ruling(&format!("checks.md could not be written: {e}"))`. A ruling error is logged the same way and never raised. The rest of the function runs as today. Re-export `CHECKS_FAILURES_FILE` from `lib.rs`.

- [ ] **Step 5: Run them again.** `cargo test -j4 -p factory-engine save_check_files`. Expected: 4 passed.

- [ ] **Step 6: Rewrite the sync test.** In `tests/park_evidence.rs`, replace `final_checks_that_cannot_run_after_a_sync_park_risky_and_say_why` with two tests. Both keep its script, which makes `checks.md` a folder on the second run. Only the exit code of that second run differs.

```rust
#[test]
fn a_failed_checks_md_write_after_a_sync_keeps_a_passing_result() {
    // Same script as before: the second run turns checks.md into a folder and exits 0.
    // ... build `fx` and the script as the old test did ...
    let r = merge_stage(&fx.driver, &fx.id).expect("stage");
    assert!(matches!(r, MergeResult::Parked(_)), "{r:?}");
    let rec = merge_record(&fx);
    assert_eq!(rec.kind, ParkKind::RiskyDiff, "{rec:?}");
    assert!(
        !rec.parts.iter().any(|p| p.text.starts_with("final checks could not run")),
        "{rec:?}"
    );
    let job = fx.driver.job(&fx.id).expect("job");
    let ledger = std::fs::read_to_string(fx.driver.job_dir(&job).join("progress.md")).expect("ledger");
    assert!(ledger.contains("checks.md could not be written"), "{ledger}");
}
```

`a_failed_checks_md_write_after_sync_parks_as_checks_failed` is the same with `exit 1` on the second run. It asserts `rec.kind == ParkKind::ChecksFailed` and `rec.reason.contains("final checks failed after sync")`.

- [ ] **Step 7: Run them.** `cargo test -j4 -p factory-engine --test park_evidence`. Expected: all pass. Then `cargo test -j4 -p factory-engine final_checks_that_cannot_run_keep_the_risky_kind`. Expected: 1 passed.

### Task 2: A rework gets the failing output

**Files:**
- Modify: `crates/factory-engine/src/rework.rs:30-47`
- Modify: `docs/reference/software-factory.md:46`
- Test: `crates/factory-engine/src/rework.rs` (new `mod tests`), `crates/factory-engine/tests/park_evidence.rs`

**Interfaces:**
- Consumes: `CHECKS_FAILURES_FILE` and `save_check_files` from the keep-the-report task above; `Driver::rework_findings(&self, id: &str, note: &str) -> Result<String, EngineError>` (`rework.rs:57`).
- Produces: nothing new. `open_findings` keeps its signature.
- Port: none. The job folder is read directly.

- [ ] **Step 1: Read the code.** Read `crates/factory-engine/src/rework.rs` and `crates/factory-engine/src/park.rs` (the `with_unreadable_board` fixture, about line 645, as a model for an Engine in a unit test).

- [ ] **Step 2: Write the failing tests.** `open_findings` is private, so test it through a `mod tests` in `rework.rs`. Use an Engine built like `with_unreadable_board`, over a board that holds job `001` with `park_kind = Some("checks-failed".into())` and no `last_review`. If plan 1's `with_board_job` fixture in `park.rs` exists and is `pub(crate)` under `#[cfg(test)]`, reuse it.

```rust
#[test]
fn rework_findings_carry_the_failing_check_output() {
    // job 001 parked as checks-failed, job folder `job_dir`
    std::fs::write(job_dir.join("checks.md"), "# Checks\n\nFAIL cargo test (exit 1)\n").expect("checks");
    std::fs::write(job_dir.join(CHECKS_FAILURES_FILE), "## cargo test (exit 1)\nerror[E0425]: x\n\n").expect("failures");
    let text = engine.open_findings().expect("findings");
    assert!(text.contains("error[E0425]: x"), "{text}");
}
```

| Test | Setup | Assertion |
|---|---|---|
| `rework_findings_fall_back_to_fail_lines_without_the_failures_file` | `checks.md` as above; no failures file, then a failures file of `"  \n"` | both cases return exactly `FAIL cargo test (exit 1)` |
| `rework_findings_prefer_the_review_over_the_failures_file` | `last_review` with verdict `changes`, file `review-code-r1.md` holding `- [blocker] fix x`; failures file present | the text is the review file text |

Add one line to `failing_final_checks_after_a_sync_park_as_checks_failed_without_a_fix_round` in `tests/park_evidence.rs`: `assert!(job_dir.join(factory_engine::CHECKS_FAILURES_FILE).exists())`, with `job_dir` from `fx.driver.job_dir(&job)`.

- [ ] **Step 3: Run them.** `cargo test -j4 -p factory-engine rework_findings`. Expected: `rework_findings_carry_the_failing_check_output` fails, because the text is only the `FAIL` line.

- [ ] **Step 4: Implement.** In `open_findings`, in the `checks-failed` branch, first read `self.job_dir.join(CHECKS_FAILURES_FILE)`. When it reads and is not blank after `trim()`, return it. Otherwise keep the `FAIL`-line code. Add `checks-failures.md` (the failing check output, read by a rework) to the job directory list in `docs/reference/software-factory.md:46`.

- [ ] **Step 5: Run them again.** `cargo test -j4 -p factory-engine rework_findings` (expected: 3 passed), then `cargo test -j4 -p factory-engine --test park_evidence` (expected: all pass).

Next plan: docs/superpowers/plans/2026-11-22-engine-small-fixes/plan-03-engine-small-fixes.md
