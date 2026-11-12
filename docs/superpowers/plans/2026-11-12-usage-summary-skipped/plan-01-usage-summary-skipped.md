# Usage summary skipped lines Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-12-usage-summary-skipped-design.md
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Make `factory costs` use the same skip rule as `factory tokens` for bad usage lines and print how many it skipped.

**Architecture:** The rule becomes one pure function in `factory-core`. `usage_summary` counts skipped lines in a new `UsageSummary.skipped`. The engine copies the count into `JobCost.skipped`, and the CLI prints one line from it. `factory-engine` only passes the value on: `gather_costs` has no trait behind it, and the agent and tracker ports are not touched.

**Tech Stack:** Rust, `serde_json`.

## Global Constraints

- Commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`. Never `--workspace` or `--all`.
- No `Cargo.toml` change, no `git` shell-outs, no `/proc` reads.
- Output only grows: with no skipped line `factory costs` prints what it prints today. `costs.json` and the board JSON do not change, and no field is added to the board, ledger or events.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call. Comments are short full sentences.

## Review Focus

1. A bad line of another job does not count for this job. Pinned by `usage_summary_does_not_count_other_jobs_bad_lines`.
2. A wrong-type optional field such as `start_cc` does not skip a line in `usage_summary`. Pinned by `usage_summary_keeps_a_line_with_a_bad_optional_field`.
3. Bad lines only in the spec-review log still show in the summary line. Pinned by `render_costs_counts_the_spec_review_record`.
4. `factory tokens` skips exactly the lines it skipped before the rule moved. The existing `load_usage_counts_garbage_and_wrong_type_lines_as_skipped` keeps passing.
5. A project with no jobs still prints `no jobs` and no skipped line. Pinned by `render_costs_prints_the_skipped_line_only_when_above_zero`.

## File Structure

| File | Responsibility |
|---|---|
| `crates/factory-core/src/cost.rs`, `lib.rs` | `USAGE_COUNT_FIELDS`, `has_bad_count`, `UsageSummary.skipped`, `JobCost.skipped`, exports |
| `crates/factory-engine/src/costs.rs` | Copy the count into each `JobCost` |
| `crates/factory-cli/src/costs.rs` | The summary line |
| `crates/factory-cli/src/tokens.rs` | Use the shared rule |
| `crates/factory-engine/tests/costs.rs` | Engine test |

### Task 1: Shared rule and usage_summary

**Files:**
- Modify: `crates/factory-core/src/cost.rs`, `crates/factory-core/src/lib.rs`
- Test: unit tests in `crates/factory-core/src/cost.rs`

**Interfaces:**
- Consumes: `usage_summary(text: &str, job: &str) -> UsageSummary` as it is.
- Produces: `pub const USAGE_COUNT_FIELDS: [&str; 9]` (`input`, `cache_creation`, `output`, `cache_read`, `usage_v`, `turns`, `round`, `start_ctx`, `ts`); `pub fn has_bad_count(line: &serde_json::Value) -> bool`; `UsageSummary.skipped: u32` and `JobCost.skipped: u32`, both default 0; all four exported from the crate root. The next two tasks use them.

- [ ] **Step 1: Read `usage_summary`, `UsageSummary`, `JobCost` and the existing `usage_summary_*` tests in `crates/factory-core/src/cost.rs`, and `COUNT_FIELDS` and `load_usage` in `crates/factory-cli/src/tokens.rs`.**
- [ ] **Step 2: Write the failing tests.**
  - `has_bad_count_flags_wrong_type_count_fields`: build values with `serde_json::json!`; `{"input":"12"}`, `{"input":1.5e3}`, `{"input":-1}` and `{"ts":"x"}` are true; `{"input":null}`, `{}` and `{"input":5}` are false.
  - `usage_summary_counts_and_drops_bad_lines`: text with a garbage line, a `{"job":"001","input":"12"}` line, a `[]` line and one good line for `001`; assert `skipped == 3`, `runs == 1`.
  - `usage_summary_does_not_count_other_jobs_bad_lines`: only `{"job":"002","input":"12"}`; asking for `001` gives `skipped == 0` and `runs == 0`.
  - `usage_summary_keeps_a_line_with_a_bad_optional_field`: `{"job":"001","role":"builder","input":5,"start_cc":"many"}` gives `runs == 1`, `skipped == 0`.
- [ ] **Step 3: Run `cargo test -j4 -p factory-core cost`.** Expected: `cannot find function has_bad_count in this scope`.
- [ ] **Step 4: Implement.** Add the constant and `has_bad_count` (true when a listed field is present, not null and `as_u64()` is `None`). In `usage_summary`, a line that is not valid JSON or not an object adds 1 to `skipped` with `saturating_add`; a line of the job where `has_bad_count` is true adds 1 and is dropped before it feeds any figure. Add the two fields and export the items in `lib.rs`. Every `JobCost` literal in `factory-core` that lists all fields (the tests in `trend.rs` and `cost.rs`) gets `skipped: 0`.
- [ ] **Step 5: Run `cargo test -j4 -p factory-core cost`.** Expected: all pass.

### Task 2: Carry the count through the engine

**Files:**
- Modify: `crates/factory-engine/src/costs.rs`
- Test: `crates/factory-engine/tests/costs.rs`

**Interfaces:**
- Consumes: `UsageSummary.skipped`, `JobCost.skipped` from the previous task.
- Produces: `gather_costs` sets `JobCost.skipped` for every board job and for the spec-review record.

- [ ] **Step 1: Read `gather_costs` in `crates/factory-engine/src/costs.rs` and the fixtures of the existing tests in `crates/factory-engine/tests/costs.rs`.**
- [ ] **Step 2: Write the failing test** `gather_costs_carries_the_skipped_count`: build a job as the existing tests there do, write a `usage.jsonl` with one good line and one `{"job":"<id>","input":"12"}` line, call `gather_costs`, assert the job's `skipped == 1` and `runs == 1`.
- [ ] **Step 3: Run `cargo test -j4 -p factory-engine --test costs gather_costs_carries`.** Expected: the assertion fails, `skipped` is 0 (the struct literal in `gather_costs` also needs the new field).
- [ ] **Step 4: Implement.** Set `skipped: usage.skipped` in both `JobCost` literals in `gather_costs`, and add `skipped: 0` to any other full `JobCost` literal in `factory-engine`.
- [ ] **Step 5: Run `cargo test -j4 -p factory-engine --test costs`.** Expected: all pass.

### Task 3: The summary line and the shared rule in tokens

**Files:**
- Modify: `crates/factory-cli/src/costs.rs`, `crates/factory-cli/src/tokens.rs`
- Test: unit tests in `crates/factory-cli/src/costs.rs`

**Interfaces:**
- Consumes: `JobCost.skipped`, `has_bad_count`, `USAGE_COUNT_FIELDS` from the first task.
- Produces: no new public items.

- [ ] **Step 1: Read `render_costs` in `crates/factory-cli/src/costs.rs` and its existing unit tests, and the `COUNT_FIELDS` use in `load_usage` in `crates/factory-cli/src/tokens.rs`.**
- [ ] **Step 2: Write the failing tests.**
  - `render_costs_prints_the_skipped_line_only_when_above_zero`: build `JobCost` values as the existing tests do; with `skipped` 0 the output has no `skipped`; with one job at `skipped: 2` the output contains `2 usage lines skipped` after the job row and before the group table; with an empty job list the output is still `no jobs` text without it.
  - `render_costs_counts_the_spec_review_record`: one real job with `skipped: 0` and a record with id `spec-review` and `skipped: 3`; the output contains `3 usage lines skipped`.
- [ ] **Step 3: Run `cargo test -j4 -p factory-cli --lib costs`.** Expected: both fail, no `usage lines skipped` text.
- [ ] **Step 4: Implement.** In `render_costs`, after the per-job rows and before the blank line, sum `skipped` over `all` (saturating) and push `N usage lines skipped\n` when the sum is above zero. Add `skipped: 0` to the full `JobCost` literals in this file's tests. In `tokens.rs`, delete `COUNT_FIELDS` and make `load_usage` call `factory_core::has_bad_count`.
- [ ] **Step 5: Run `cargo test -j4 -p factory-cli`, then `cargo clippy -j4 -p factory-core -p factory-engine -p factory-cli`.** Expected: all pass, no warnings.

Next plan: none
