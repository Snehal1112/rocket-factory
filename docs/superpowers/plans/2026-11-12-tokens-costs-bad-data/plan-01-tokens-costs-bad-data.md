# Tokens and costs bad data Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-12-tokens-costs-bad-data-design.md
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Count and show the usage lines `factory tokens` cannot read, warn about an unreadable usage file without hiding other jobs, and stop the sums from overflowing.

**Architecture:** `load_usage` in `factory-cli/src/tokens.rs` returns a small `Usage` struct with a skipped count and a no-`ts` count. The report structs carry the counts and the renderers print them only when above zero. The two commands in `commands.rs` wire them in. No engine code changes and no trait is involved.

**Tech Stack:** Rust, `serde_json`.

## Global Constraints

- Commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`. Never `--workspace` or `--all`.
- No `Cargo.toml` change, no `git` shell-outs, no `/proc` reads.
- Output only grows: with zero counts every line of `factory tokens` and `factory costs` is unchanged (`tokens_for_one_job_is_unchanged` keeps passing). No board, ledger, events or `costs.json` field is added.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call. Comments are short full sentences.

## Review Focus

1. A line with `"input":-1` or `"input":"12"` is skipped, but `"input":null` and a missing field are kept as 0. Pinned by `load_usage_counts_garbage_and_wrong_type_lines_as_skipped`.
2. A line for another job is not counted as skipped. Pinned by `load_usage_does_not_count_other_jobs_lines`.
3. A rows-without-`ts` note appears only with `--days`. Pinned by `tokens_cache_states_rows_without_ts_under_days`.
4. A missing usage file gives no warning. Pinned by `tokens_cache_reports_skipped_lines`, which also asserts the output has no `warning:` text while other jobs have no usage file.
5. Totals near `u64::MAX` do not panic. Pinned by `aggregate_saturates_instead_of_overflowing` and `tokens_percent_does_not_overflow_on_huge_totals`.

## File Structure

| File | Responsibility |
|---|---|
| `crates/factory-cli/src/tokens.rs` | `Usage`, `load_usage`, `aggregate`, report fields, both renderers |
| `crates/factory-cli/src/commands.rs` | `tokens`, `cache_tokens`, `budget_percent` |
| `crates/factory-cli/tests/tokens_cache.rs` | CLI tests |
| `docs/guides/factory-user-guide.md`, `docs/reference/software-factory.md` | Doc sentences |

### Task 1: Usage parsing and the report structs

**Files:**
- Modify: `crates/factory-cli/src/tokens.rs`
- Test: unit tests in `crates/factory-cli/src/tokens.rs`

**Interfaces:**
- Consumes: `UsageRecord`, `TokenReport`, `CacheReport`, `Totals` as they are.
- Produces: `pub struct Usage { pub records: Vec<UsageRecord>, pub skipped: u64, pub no_ts: u64 }` (derive `Debug, Default`); `pub fn load_usage(log: &Path, job: &str) -> Result<Usage, std::io::Error>`; `TokenReport.skipped: u64`; `CacheReport.skipped: u64` and `CacheReport.no_ts: u64`; `render` and `render_cache` print the counts. The commands in the next task use all of these.

- [ ] **Step 1: Read `load_usage`, `aggregate`, `render`, `render_cache`, `TokenReport`, `CacheReport` and the existing test `load_reads_old_new_and_garbage_lines_together` in `crates/factory-cli/src/tokens.rs`.**
- [ ] **Step 2: Write the failing tests.** Reuse the temp-file style of `load_reads_old_new_and_garbage_lines_together`.
  - `load_usage_counts_garbage_and_wrong_type_lines_as_skipped`: lines for job `001` that are truncated JSON, `[]`, `{"job":"001","input":"12"}`, `{"job":"001","input":1.5e3}`, `{"job":"001","input":-1}`, plus a good line and a line with `"input":null`; assert `skipped == 5` and `records.len() == 2`.
  - `load_usage_does_not_count_other_jobs_lines`: a valid line for job `002` only; asking for `001` gives `skipped == 0` and no records.
  - `load_usage_counts_rows_without_ts`: one line with `"ts":5` and one without `ts`; assert `no_ts == 1` and both records kept.
  - `aggregate_saturates_instead_of_overflowing`: two records with `input` `u64::MAX`; `aggregate` does not panic and `total.raw == u64::MAX` and `total.weighted == u64::MAX`.
  - `render_prints_the_skipped_line_count_only_when_above_zero`: `render("001", &report)` has no `lines skipped` text for a default report, and ends with `  3 lines skipped\n` when `report.skipped = 3`. A similar assertion for `render_cache` with `skipped = 2` (line `  2 lines skipped` right after the heading) and with `days = Some(7)`, `no_ts = 4` (line `  4 rows without ts not counted by --days`); the `no_ts` line is absent when `days` is `None`.
- [ ] **Step 3: Run `cargo test -j4 -p factory-cli --lib tokens`.** Expected: compile failure, `no field skipped on type TokenReport` and `Usage` not found.
- [ ] **Step 4: Implement.** `load_usage` returns `Usage`; a missing file gives `Usage::default()`, other read errors stay errors. Skip rules and the list of known number fields are in the spec, section 1: skip when the line is not a JSON object, or a known number field is present, not null and not a non-negative integer (`as_u64()` is `None`). Count `no_ts` for kept records whose `ts` is absent or null. Lines of other jobs are not counted anywhere. `aggregate` uses `saturating_add`. `render` appends `  N lines skipped\n` last when `skipped > 0`. `render_cache` prints the skipped line and then, only when `days` is set and `no_ts > 0`, the no-`ts` line, directly after the heading and before any `no runs` line. Update the existing test and any other caller in this file to read `.records`.
- [ ] **Step 5: Run `cargo test -j4 -p factory-cli --lib tokens`.** Expected: all pass.

### Task 2: Wire the commands

**Files:**
- Modify: `crates/factory-cli/src/commands.rs`
- Test: `crates/factory-cli/tests/tokens_cache.rs`, unit test in `crates/factory-cli/src/commands.rs`

**Interfaces:**
- Consumes: `Usage`, `load_usage`, `TokenReport.skipped`, `CacheReport.skipped`, `CacheReport.no_ts` from the previous task.
- Produces: `fn budget_percent(weighted: u64, budget: u64) -> u64` (private, in `commands.rs`).

- [ ] **Step 1: Read `cache_tokens` and `tokens` in `crates/factory-cli/src/commands.rs`, and the fixtures `new_job`, `job_folder` and the tests `cache_report_reads_every_job_of_the_project` and `tokens_for_one_job_is_unchanged` in `crates/factory-cli/tests/tokens_cache.rs`.**
- [ ] **Step 2: Write the failing tests.**
  - `tokens_percent_does_not_overflow_on_huge_totals` in `commands.rs`: assert `budget_percent(u64::MAX, 100) == u64::MAX / 100` and `budget_percent(5, 0) == 0`.
  - `tokens_cache_reports_skipped_lines`: create a job, write a usage file with a garbage line and a good line, run `factory tokens --cache` as `cache_report_reads_every_job_of_the_project` does, assert the output contains `1 lines skipped`.
  - `tokens_cache_states_rows_without_ts_under_days`: a usage line without `ts`, run with `--cache --days 7`, assert the output contains `1 rows without ts not counted by --days`; the same run without `--days` does not contain `rows without ts`.
  - `tokens_cache_warns_and_continues_when_one_usage_file_is_not_utf8`: two jobs, the usage file of the first holds the bytes `0xFF 0xFE`, the second a good line; assert the output starts with `warning: could not read` and still lists the second job's role row.
  - `tokens_for_one_job_still_errors_on_an_unreadable_usage_file`: the same non-UTF-8 file, `factory tokens <id>` returns `Err`.
- [ ] **Step 3: Run `cargo test -j4 -p factory-cli --test tokens_cache` and `cargo test -j4 -p factory-cli --lib budget_percent`.** Expected: the new tests fail (no warning, no counts, `budget_percent` not found).
- [ ] **Step 4: Implement.** `budget_percent` returns `weighted.saturating_mul(100).checked_div(budget).unwrap_or(0)` and `tokens` uses it. In `tokens`, set `report.skipped` from the loaded `Usage` before `render`. In `cache_tokens`, a read error other than NotFound for a file adds no rows and a line `warning: could not read <path>: <err>` at the top of the output (every job file and the spec-review file); sum `skipped` and `no_ts` over all files into the `CacheReport` before `render_cache`. `tokens <id>` keeps returning the read error.
- [ ] **Step 5: Run `cargo test -j4 -p factory-cli` then `cargo clippy -j4 -p factory-cli`.** Expected: all pass, no warnings.

### Task 3: Docs

**Files:**
- Modify: `docs/guides/factory-user-guide.md`, `docs/reference/software-factory.md`

**Interfaces:**
- Consumes: the behaviour built in the first two tasks.
- Produces: the doc sentences only.

- [ ] **Step 1: Read the `factory costs` entry near line 600 and section 6.10 `factory tokens` in `docs/guides/factory-user-guide.md`, and the cost report paragraph near line 411 of `docs/reference/software-factory.md`.**
- [ ] **Step 2: Add the sentences.** User guide `factory costs`: "If the board cannot be loaded the command fails and leaves the existing costs.json untouched." Reference cost report paragraph: the same sentence. User guide 6.10: one sentence that the totals come from the board and may differ by a few tokens from the per-role sum, and one that `N lines skipped` counts usage lines that could not be read, and that `--cache` names an unreadable usage file in a `warning:` line and goes on.
- [ ] **Step 3: Run `cargo test -j4 -p factory-cli --test no_hardcoded_paths`.** Expected: PASS (nothing else reads these docs).

Next plan: none
