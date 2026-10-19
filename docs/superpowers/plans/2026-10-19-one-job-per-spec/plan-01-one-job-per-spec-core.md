# One job per spec, core pieces Implementation Plan

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** `factory-engine` can validate a plan chain without the 3 task cap and can turn the ordered plan texts of one spec into one combined plan text.

**Architecture:** `parse_series` keeps its behaviour and delegates to a new `parse_series_with` that takes an optional task cap. A new pure module `combine.rs` builds the combined plan text from `(path, text)` pairs with no I/O, reusing the helpers in `tasks.rs`. Nothing in this plan creates a job.

**Tech Stack:** Rust, cargo with `-j4` per crate.

**Spec:** `docs/superpowers/specs/2026-10-19-one-job-per-spec-design.md`

## Global Constraints

- Every cargo call has `-j4` and `-p <crate>`. Never `--workspace`, `--all` or `CARGO_TARGET_DIR`.
- Follow each task's steps in order: write the failing tests as given, run the stated command and confirm the failure, implement from the signatures, then confirm the pass.
- The series path stays as it is: `parse_series` still refuses a plan of 0 or more than 3 tasks, and every existing test passes unchanged.
- The combine function does no I/O and never edits a plan file on disk.
- Engine logic stays behind the ports in `ports.rs`. This plan adds only pure functions.
- Tests use `.expect("...")` and `expect_err`. Never write the literal text of a panicking unwrap call in a plan, doc or Rust file. No `git` CLI, no commit steps. Code comments are short full sentences ending with a punctuation mark.

## Review Focus

1. **A task number reused across plans.** Plan 1 and plan 2 both have `### Task 1:`. The combined text must hold every task, renumbered. Pinned in Task 2 (`repeated_task_numbers_across_plans_are_renumbered`).
2. **The older checkbox task form.** A plan that uses `- [ ] Task N: title` lines must combine too. Pinned in Task 2 (`checkbox_form_tasks_are_renumbered`).
3. **A task body that names another task by number.** The number is kept and a warning is returned. Pinned in Task 2 (`a_body_mention_of_another_task_warns`).
4. **A later plan's own constraint.** A bullet that only plan 2 has must survive in the combined Global Constraints, and a bullet both plans have must appear once. Pinned in Task 2 (`global_constraints_are_merged_without_duplicates`).
5. **The 3 task cap on the series path.** A 4 task plan is still refused by `parse_series`. Pinned in Task 1 (`parse_series_still_refuses_four_tasks`).

## File Structure

- Modify `crates/factory-engine/src/series.rs`: `parse_series_with`, and `parse_series` calls it with `Some(3)`.
- Modify `crates/factory-engine/src/lib.rs`: `mod combine;` and the `pub use` lines.
- Create `crates/factory-engine/src/combine.rs`: `combine_plans` and `CombinedPlan`, with unit tests.
- Modify `crates/factory-engine/tests/series.rs`: two tests.

---

### Task 1: Validate a plan chain without the task cap

**Files:**
- Modify: `crates/factory-engine/src/series.rs`, `crates/factory-engine/src/lib.rs`
- Test: `crates/factory-engine/tests/series.rs`

**Interfaces:**
- Produces:

```rust
/// `max_tasks` of `None` lifts the upper bound on tasks per plan. A plan still needs at least one task.
pub fn parse_series_with(root: &Path, start: &Path, max_tasks: Option<usize>) -> Result<Series, SeriesError>;
```

  `parse_series(root, start)` is unchanged and returns `parse_series_with(root, start, Some(3))`. `MAX_PLANS` still applies.
- Consumes: the existing `read_plan`, which takes the cap as a new parameter.

- [ ] **Step 1: Write the failing tests** in `tests/series.rs`, using its `plan(dir, name, tasks, next)` helper: `parse_series_with_none_accepts_five_tasks` (a 5 task plan with `Next plan: none` parses, `plans[0].tasks == vec![1,2,3,4,5]`), `parse_series_with_none_still_refuses_zero_tasks` (`SeriesError::TaskCount`), and `parse_series_still_refuses_four_tasks` (`parse_series` on a 4 task plan is `SeriesError::TaskCount`).
- [ ] **Step 2: Run to verify it fails:** `cargo test -j4 -p factory-engine --test series`. Expected: the first two fail to compile, since `parse_series_with` does not exist.
- [ ] **Step 3: Implement** `parse_series_with` and thread `max_tasks: Option<usize>` into `read_plan` (replace the literal `3` at `series.rs:60` with the parameter and keep the empty check). Export it from `lib.rs`.
- [ ] **Step 4: Run to verify it passes:** `cargo test -j4 -p factory-engine --test series`, then `cargo check -j4 -p factory-engine`. Expected: pass.

### Task 2: Combine the plan texts of one spec

**Files:**
- Create: `crates/factory-engine/src/combine.rs`
- Modify: `crates/factory-engine/src/lib.rs`

**Interfaces:**
- Produces:

```rust
pub struct CombinedPlan {
    pub text: String,
    pub tasks: usize,
    pub warnings: Vec<String>,
}

/// `plans` holds `(path, text)` in chain order. Pure: no I/O.
pub fn combine_plans(plans: &[(String, String)]) -> CombinedPlan;
```

- Consumes from `tasks.rs`: `parse_tasks`, `parse_checkbox_tasks`, `plan_header`, `task_section`, `review_focus`, `plan_title` (reuse them, do not re-parse by hand where one fits).

- [ ] **Step 1: Write the failing tests** in `combine.rs` `#[cfg(test)]`, with a helper that builds a plan text from a title, a goal, constraint bullets, a Review Focus list and N tasks. Names and assertions:
  - `repeated_task_numbers_across_plans_are_renumbered`: two plans of 3 tasks each combine to `tasks == 6`, `parse_tasks(&text)` gives numbers `1..=6` in order, and the titles keep their plan order.
  - `checkbox_form_tasks_are_renumbered`: two plans of checkbox tasks give `parse_checkbox_tasks(&text)` numbers `1..=4`.
  - `the_text_has_one_header_and_no_next_plan_line`: the combined text has one `# ` title (the first plan's), the first non-empty line after it is `<!-- Combined from: <path1>, <path2> -->`, `plan_header(&text)` contains `Plan 2 goal:` followed by the second plan's goal, and no line starts with `Next plan:`.
  - `global_constraints_are_merged_without_duplicates`: a bullet in both plans appears once, a bullet only in plan 2 appears, order is plan 1 then plan 2.
  - `review_focus_sections_are_merged`: `review_focus(&text)` holds the lines of both plans under one `## Review Focus` heading.
  - `a_body_mention_of_another_task_warns`: a task body that contains `see Task 2` gives one warning that names the mention and the task it sits in, and the text keeps `see Task 2`.
  - `a_constraint_that_names_another_plan_warns`: a bullet containing `from plan 1` gives one warning.
  - `one_plan_combines_to_itself`: one plan gives the same tasks, no `Plan 2 goal:` line and no warnings.
- [ ] **Step 2: Run to verify it fails:** `cargo test -j4 -p factory-engine combine`. Expected: compile error, since `combine_plans` does not exist.
- [ ] **Step 3: Implement** `combine_plans` and register `mod combine;` with `pub use combine::{combine_plans, CombinedPlan};` in `lib.rs`. Approach: split every plan into header, task sections and Review Focus with the `tasks.rs` helpers, rewrite only the `### Task N:` heading or the `- [ ] Task N:` line to the running number, leave bodies untouched, and drop each plan's trailing `Next plan:` line. The combined file is title, the comment line, the first plan's header body, one `Plan N goal:` line for each later plan, the merged `## Global Constraints` (bullets in order, exact duplicates removed), the merged `## Review Focus`, then all tasks.
- [ ] **Step 4: Run to verify it passes:** `cargo test -j4 -p factory-engine combine`, then `cargo test -j4 -p factory-engine` and `cargo check -j4 -p factory-engine`. Expected: pass.

Next plan: docs/superpowers/plans/2026-10-19-one-job-per-spec/plan-02-one-job-per-spec-engine.md
