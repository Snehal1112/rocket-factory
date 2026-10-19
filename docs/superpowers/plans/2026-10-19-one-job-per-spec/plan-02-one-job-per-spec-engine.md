# One job per spec, engine path Implementation Plan

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** The engine creates one job with no series from all the plans of a spec, and stops for the user's consent when the estimated cost passes `budget_weighted`.

**Architecture:** A new `one_job.rs` holds the estimate, the over-budget error and `create_one_job`. It reuses the validation of `create_job_with_spec` (chain, plan lint, spec check) through `pub(crate)` helpers in `runloop.rs`, then builds the combined text with `combine_plans` and writes it to the job's `plan.md` itself, because `add_job` copies a plan file only when `job.series` is set. The guard runs before any job or folder exists.

**Tech Stack:** Rust, cargo with `-j4` per crate, the engine fixtures in `crates/factory-engine/tests`.

**Spec:** `docs/superpowers/specs/2026-10-19-one-job-per-spec-design.md`

## Global Constraints

- Every cargo call has `-j4` and `-p <crate>`. Never `--workspace`, `--all` or `CARGO_TARGET_DIR`.
- Follow each task's steps in order: write the failing tests as given, run the stated command and confirm the failure, implement from the signatures, then confirm the pass.
- `create_job_with_spec` and the series path behave exactly as before. Existing tests pass unchanged. Board JSON stays compatible with `dashboard/`: add no field, the one job has `series: None`.
- The budget that is enforced is `config.budget_weighted`. The per-job `budget_weighted` field is not a limit and is not changed.
- Only the driver writes the board, ledger and events. The estimate constants are `110_000` and `100_000` per task, named constants in `one_job.rs`.
- Tests use `.expect("...")` and `expect_err`. Never write the literal text of a panicking unwrap call in a plan, doc or Rust file. No `git` CLI, no commit steps. Comments are short full sentences ending with a punctuation mark.

## Review Focus

1. **Over budget creates nothing.** At the default 1M budget a 12 task spec returns the consent error and leaves no job on the board and no new job folder. Pinned in Task 1 (`over_budget_without_consent_creates_nothing`).
2. **Consent goes ahead.** With consent the job is created, the warning is stored on the job and the run is not changed. Pinned in Task 1 (`over_budget_with_consent_creates_the_job_with_a_warning`).
3. **One job, not four.** A 4 plan, 12 task spec makes exactly one job with `series: None`, a 12 task `plan.md` and no jobs for plans 2 to 4. Pinned in Task 2 (`four_plans_become_one_job_with_a_combined_plan`).
4. **A single plan.** The call goes through the normal path, so a 4 task plan is still refused and a 2 task plan makes its usual job. Pinned in Task 2 (`one_plan_takes_the_normal_path`).
5. **The series path is untouched.** The same 4 plan folder without the flag still makes four jobs. Pinned in Task 2 (`without_the_flag_the_same_plans_still_make_four_jobs`).
6. **A bad plan.** A placeholder in the second plan refuses the call before any job exists. Pinned in Task 2 (`a_placeholder_in_a_later_plan_refuses_the_call`).

## File Structure

- Create `crates/factory-engine/src/one_job.rs`: the constants, `one_job_estimate`, `create_one_job`, `OneJob`.
- Modify `crates/factory-engine/src/lib.rs`: `mod one_job;`, the `pub use`, and the `NeedsConsent` variant of `EngineError`.
- Modify `crates/factory-engine/src/runloop.rs`: make `add_job`, `add_warning`, `adopt_draft`, `check_spec` and the spec-copy step `pub(crate)`, without changing them.
- Create `crates/factory-engine/tests/one_job.rs`: the integration tests of both tasks, with the fixture set up as in `tests/new_job.rs`.

---

### Task 1: The estimate and the consent guard

**Files:**
- Create: `crates/factory-engine/src/one_job.rs`, `crates/factory-engine/tests/one_job.rs`
- Modify: `crates/factory-engine/src/lib.rs`

**Interfaces:**
- Produces:

```rust
pub const ONE_JOB_OVERHEAD_WEIGHTED: u64 = 110_000;
pub const ONE_JOB_PER_TASK_WEIGHTED: u64 = 100_000;

/// The rough weighted cost of one job that holds `tasks` tasks.
pub fn one_job_estimate(tasks: usize) -> u64;

pub struct OneJob {
    pub job: Job,
    /// Plain lines for the CLI to print as `note:` lines.
    pub notes: Vec<String>,
}

pub fn create_one_job(
    driver: &Driver,
    slug: &str,
    feature: &str,
    plan: &Path,
    spec_path: Option<&Path>,
    model: Option<&str>,
    accept_over_budget: bool,
) -> Result<OneJob, EngineError>;
```

  `EngineError` gains `NeedsConsent { estimate: u64, budget: u64 }`, displayed as `needs consent: this job is estimated at {estimate} weighted tokens and the budget is {budget}; raise budget_weighted in project.toml or pass --accept-over-budget`.
- Consumes: `Driver::config().budget_weighted`.

- [ ] **Step 1: Write the failing tests** in `tests/one_job.rs`: `estimate_is_overhead_plus_per_task` (`one_job_estimate(12) == 1_310_000`, `one_job_estimate(3) == 410_000`), `over_budget_without_consent_creates_nothing` (a fixture with `budget_weighted` 1_000_000, four plans of 3 tasks and a spec: the error is `NeedsConsent { estimate: 1_310_000, budget: 1_000_000 }`, the board has no jobs and the jobs folder gains no entry), `over_budget_with_consent_creates_the_job_with_a_warning` (same input with `accept_over_budget` true: the job exists and its `spec_warning` mentions `over the budget`), and `under_budget_creates_the_job_with_a_note` (budget 1_500_000: no warning, `notes` has one line that contains `1310000`). Put the shared fixture helper `four_plans(dir) -> PathBuf` at the top.
- [ ] **Step 2: Run to verify it fails:** `cargo test -j4 -p factory-engine --test one_job`. Expected: compile error, since `create_one_job` does not exist.
- [ ] **Step 3: Implement** the constants, `one_job_estimate`, the `NeedsConsent` variant and the guard part of `create_one_job`: validate with `parse_series_with(root, start, None)`, lint every plan and check the spec as `create_job_with_spec` does (reuse its logic through `pub(crate)` helpers rather than copying it), count tasks, compare the estimate with `config.budget_weighted`, return `NeedsConsent` before anything is created. Task 2 adds the creation.
- [ ] **Step 4: Run to verify it passes:** `cargo test -j4 -p factory-engine --test one_job` for the first three tests, then `cargo check -j4 -p factory-engine`. Expected: the guard tests pass. The creating tests may stay red until Task 2, so mark them `#[ignore]` with a comment and remove the marker in Task 2.

### Task 2: Create the one job

**Files:**
- Modify: `crates/factory-engine/src/one_job.rs`, `crates/factory-engine/src/runloop.rs`, `crates/factory-engine/tests/one_job.rs`

**Interfaces:**
- Consumes: `combine_plans(&[(String, String)]) -> CombinedPlan` and `parse_series_with` from plan 1, and the Task 1 guard.
- Produces: the creation half of `create_one_job`. With one plan in the chain it returns `create_job_with_spec(driver, slug, feature, Some(plan), spec_path, model)` wrapped in `OneJob` with a note `--one-job had no effect: the chain has one plan`.

- [ ] **Step 1: Write the failing tests** in `tests/one_job.rs` and remove the `#[ignore]` markers from Task 1: `four_plans_become_one_job_with_a_combined_plan` (exactly one new job, `job.series` is `None`, the job's `plan.md` has `parse_tasks` numbers `1..=12`, no line starts with `Next plan:`, the stage is `build`, and `spec.md` is in the job folder), `one_plan_takes_the_normal_path` (a 2 task plan makes a job whose `series` is `Some` and the notes hold the no effect line, and a 4 task plan still fails with `SeriesError::TaskCount` as an `EngineError`), `without_the_flag_the_same_plans_still_make_four_jobs` (the four plan fixture through `create_job_with_spec` gives 4 jobs and the series cap of 3 still refuses a 4 task plan), `a_placeholder_in_a_later_plan_refuses_the_call` (plan 2 holds `TBD`, the call fails and no job exists).
- [ ] **Step 2: Run to verify it fails:** `cargo test -j4 -p factory-engine --test one_job`. Expected: the new tests fail because `create_one_job` does not create a job yet.
- [ ] **Step 3: Implement** the creation: read the plan texts in chain order, call `combine_plans`, stage `Build`, `add_job(driver, slug, feature, &issues, None, Stage::Build, model)`, write `CombinedPlan.text` to `<job dir>/plan.md`, copy the spec as `create_job_with_spec` does, adopt the draft, store the combined warnings and (when consent was given) the over budget warning with `add_warning`, and compute the estimate from the combined plan size best effort as `add_job` does for series jobs.
- [ ] **Step 4: Run to verify it passes:** `cargo test -j4 -p factory-engine --test one_job`, then `cargo test -j4 -p factory-engine` and `cargo check -j4 -p factory-engine`. Expected: pass, including every existing engine test.

Next plan: docs/superpowers/plans/2026-10-19-one-job-per-spec/plan-03-one-job-per-spec-cli-command.md
