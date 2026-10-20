# Per-role cache TTL, Plan 1: Cache report

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** `factory tokens --cache [--days N]` prints a project-wide table of prompt cache use per role and a list of resumed runs with their idle gap and a cache-rewrite flag, from a new `secs` field in `usage.jsonl`.

**Architecture:** `run_agent` already holds `started: Instant`; it passes the elapsed seconds to `log_usage` and `usage_line`, which writes `secs`. `UsageRecord` in `crates/factory-cli/src/tokens.rs` gains two optional fields. A pure `cache_report` function and a `render_cache` function live in the same file. `cli.rs` and `commands.rs` add the flags and a `cache_tokens` command function that loads every board job's `usage.jsonl` and calls them.

**Tech Stack:** Rust, clap derive, `serde_json`, `tempfile` in tests.

**Spec:** docs/superpowers/specs/2026-10-20-per-role-cache-ttl-design.md

## Global Constraints

- Always `-j4`; never `--workspace` or `--all`; never set `CARGO_TARGET_DIR`. Use `cargo test -j4 -p <crate> <filter>` and `--test <file>` for integration files.
- `usage.jsonl` gains one optional key, `secs`; no key is renamed or removed. Board, ledger and events are untouched.
- The report only reads files. It never calls `claude`, `git` or `gh`, and paths come from `driver.job_dir`, never hardcoded (`no_hardcoded_paths.rs`).
- `REWRITE_SHARE` is `0.25` (inclusive) and `CACHE_5M_SECS` is `300` (idle means a gap strictly above it), both constants in `tokens.rs`.
- `factory tokens <id>` output stays byte for byte as today.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call. Comments are short full sentences ending with a punctuation mark.

## Review Focus

1. A job whose first row is a resumed run. Expected: gap `?`, never a panic or a negative number. Pinned by `resumed_gap_is_unknown_without_secs_or_predecessor` (Task 2).
2. A run where `ts - secs` is below the previous `ts` (clock skew). Expected: gap `0`. Pinned by `resumed_gap_floors_at_zero` (Task 2).
3. Rows from before 2026-10-19 with no `cache_creation_1h`. Expected: the split is skipped, not counted as 0 for 1h. Pinned by `cache_split_is_n_a_when_no_row_has_it` and `cache_footnote_shows_split_coverage` (Task 2).
4. `cache_creation_1h` above `cache_creation`. Expected: 5M is 0, no overflow. Pinned by `five_m_split_saturates` (Task 2).
5. A board job with no folder or no `usage.jsonl`. Expected: it adds nothing and the report still prints. Pinned by `cache_report_reads_every_job_of_the_project` (Task 3).

---

## File Structure

- `crates/factory-engine/src/ports.rs` (modify): `log_usage` and `usage_line` take and write `secs`.
- `crates/factory-engine/tests/runloop.rs` (modify): `usage_lines_carry_the_run_details` asserts `secs`.
- `crates/factory-cli/src/tokens.rs` (modify): record fields, loader keys, `cache_report`, `render_cache`, constants.
- `crates/factory-cli/src/cli.rs` (modify): `Tokens` arguments and parse tests.
- `crates/factory-cli/src/commands.rs` (modify): dispatch on `(id, cache)` and `cache_tokens`.
- `crates/factory-cli/tests/tokens_cache.rs` (create): CLI tests on `common::Fixture`.
- `README.md`, `docs/software-factory.md`, `docs/roadmap-2026-10-18.md` (modify): short notes.

### Task 1: Log the run duration

**Files:**
- Modify: `crates/factory-engine/src/ports.rs:176-221,245`, and the test callers at lines 305, 348, 355, 364, 371
- Test: `crates/factory-engine/src/ports.rs` (unit tests), `crates/factory-engine/tests/runloop.rs`

**Interfaces:**
- Produces: `pub(crate) fn usage_line(job_id: &str, spec: &AgentSpec, run: &AgentRun, round: u32, tasks_done: u32, ts: u64, secs: u64) -> serde_json::Value`. The line carries `"secs": secs` for every role.

- [ ] **Step 1: Read** `run_agent`, `log_usage` and `usage_line` in `ports.rs`, and the two usage tests in `runloop.rs` (`every_agent_run_appends_a_usage_line_with_its_role`, `usage_lines_carry_the_run_details`).
- [ ] **Step 2: Write the failing tests.** In `ports.rs`, add `usage_line_has_the_run_duration`: build the line as `usage_line_has_the_counters_and_the_run_details` does, pass `secs` 42, then `assert_eq!(line["secs"], 42);` and `assert_eq!(line["role"], "builder");`. Pass `0` as the new last argument in the five existing `usage_line(...)` test calls so they compile. In `runloop.rs`, add `assert!(v["secs"].is_u64(), "{v}");` to the loop in `usage_lines_carry_the_run_details`.
- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine usage_line_has_the_run_duration`. Expected: compile failure, `usage_line` takes 6 arguments but 7 were supplied.
- [ ] **Step 4: Implement.** Add the `secs: u64` parameter to `usage_line` and write it into the JSON object. Add `secs: u64` to `log_usage` and pass `started.elapsed().as_secs()` from `run_agent` at the existing `self.log_usage(&spec, &run)` call.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine`. Expected: all pass, including `usage_line_has_the_run_duration` and `usage_lines_carry_the_run_details`.

### Task 2: Cache report logic

**Files:**
- Modify: `crates/factory-cli/src/tokens.rs`
- Test: `crates/factory-cli/src/tokens.rs` (unit tests, next to the existing `rec` helper)

**Interfaces:**
- Consumes: `UsageRecord`, `load_usage` and the `rec(role, input, cc, out, cr)` test helper already in `tokens.rs`.
- Produces:
  - `UsageRecord { cache_creation_1h: Option<u64>, secs: Option<u64>, .. }`, set by `load_usage` from the keys of the same name, `None` when the key is absent.
  - `pub const REWRITE_SHARE: f64 = 0.25;` and `pub const CACHE_5M_SECS: u64 = 300;`
  - `pub struct RoleCache { role: String, runs: u64, creation: u64, read: u64, split_runs: u64, one_hour: u64, five_min: u64 }`
  - `pub struct ResumedRun { job: String, role: String, round: u64, gap: Option<u64>, creation: u64, read: u64 }` with `fn share(&self) -> Option<f64>` (`None` when creation plus read is 0), `fn rewrote(&self) -> bool` (`share >= REWRITE_SHARE`) and `fn idle(&self) -> bool` (`gap > CACHE_5M_SECS`).
  - `pub struct CacheReport { runs: u64, days: Option<u32>, roles: Vec<RoleCache>, resumed: Vec<ResumedRun> }`
  - `pub fn cache_report(jobs: &[Vec<UsageRecord>], days: Option<u32>, now: u64) -> CacheReport` and `pub fn render_cache(r: &CacheReport) -> String`.

- [ ] **Step 1: Read** `tokens.rs` in full, and spec sections 4 to 6 for the exact columns and the summary line.
- [ ] **Step 2: Write the failing tests** in the `tokens.rs` test module. A helper `run(job, role, resumed, ts, secs, cc, cc1h, cr)` builds a `UsageRecord`. Full example for the gap:

```rust
#[test]
fn resumed_gap_is_ts_minus_secs_minus_previous_ts() {
    let first = run("020", "builder", false, 1_000, Some(50), 100, Some(0), 900);
    let second = run("020", "builder", true, 1_400, Some(100), 3_415, Some(0), 118_160);
    let r = cache_report(&[vec![first, second]], None, 2_000);
    assert_eq!(r.resumed.len(), 1);
    assert_eq!(r.resumed[0].gap, Some(300));
}
```

  The other cases, each one test with its own inputs:

  | Test | Inputs | Assertion |
  |---|---|---|
  | `load_usage_reads_the_split_and_the_duration_when_present` | a line with `cache_creation_1h` 5 and `secs` 9 | `Some(5)` and `Some(9)` |
  | `load_usage_leaves_split_and_duration_none_on_old_lines` | a line with neither key | both `None` |
  | `cache_roles_sum_creation_read_and_split` | two builder rows, 1h 10 and 20 of creation 30 and 50 | creation 80, 1h 30, 5M 50 |
  | `cache_split_is_n_a_when_no_row_has_it` | rows with `cache_creation_1h: None` | `render_cache` role row shows `n/a` twice |
  | `cache_footnote_shows_split_coverage` | 3 rows, 2 with the field | output contains `split covers 2 of 3 runs` |
  | `resumed_gap_is_unknown_without_secs_or_predecessor` | resumed first row; resumed row with `secs: None` | gap `None` for both, rendered `?` |
  | `resumed_gap_floors_at_zero` | `ts - secs` below previous `ts` | gap `Some(0)` |
  | `rewrite_threshold_is_25_percent_inclusive` | creation/read of 249/751 and 250/750 | `rewrote()` false, then true |
  | `zero_creation_and_read_is_never_a_rewrite` | 0 and 0 | `share() == None`, `rewrote()` false, rendered `-` |
  | `summary_counts_idle_rewrites_and_unknown_gaps` | one idle rewrite, one idle non-rewrite, one unknown-gap rewrite | `resumed runs: 3, idle over 5m: 2, of those rewrote: 1, rewrote with gap unknown: 1` |
  | `days_filter_lists_recent_runs_but_keeps_the_predecessor_gap` | predecessor older than the window, resumed run inside | one resumed row, gap still `Some` |
  | `days_cutoff_is_inclusive` | `ts == now - 86400` and one second older, `days` 1 | first counted, second not |
  | `no_resumed_runs_prints_the_empty_line` | only fresh rows | output contains `no resumed runs` and no `resumed runs:` summary |
  | `header_names_all_time_or_the_day_window_and_empty_prints_no_runs` | `days` None, `Some(7)`, no rows | `cache report (N runs, all time)`, `cache report (N runs, last 7 days)`, `no runs` |
  | `creation_percent_is_one_decimal_and_dash_when_empty` | 1/3 split; 0/0 | `25.0%`-style one decimal; `-` |
  | `roles_are_sorted_by_name` | rows for `reviewer` then `builder` | `builder` row first |
  | `five_m_split_saturates` | `cache_creation_1h` 9 above `cache_creation` 5 | 5M 0 |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-cli tokens::`. Expected: compile failure, `cache_report` and the new `UsageRecord` fields not found.
- [ ] **Step 4: Implement** the fields, loader keys, constants, structs, `cache_report` (filter by `days` after the gaps are computed, pair each resumed row with the previous row of its own job) and `render_cache` as the spec shows. Percent columns use `{:.1}%`, widths pad like `render_detail`.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-cli tokens::`. Expected: every test above passes, plus the existing `tokens.rs` tests.

### Task 3: The `--cache` command and docs

**Files:**
- Modify: `crates/factory-cli/src/cli.rs:75`, `crates/factory-cli/src/commands.rs:329,824`, `README.md`, `docs/software-factory.md`, `docs/roadmap-2026-10-18.md`
- Create: `crates/factory-cli/tests/tokens_cache.rs`
- Test: `crates/factory-cli/src/cli.rs` (unit tests), `crates/factory-cli/tests/tokens_cache.rs`

**Interfaces:**
- Consumes: `cache_report`, `render_cache`, `load_usage` from Task 2, `Driver::job_dir`, `now_secs`.
- Produces: `Command::Tokens { id: Option<String>, cache: bool, days: Option<u32> }` and `fn cache_tokens(ctx: &CliCtx, days: Option<u32>) -> Result<String, CliError>` in `commands.rs`.

- [ ] **Step 1: Read** the `Tokens` arm and `tokens` function in `commands.rs`, `project_conflicts_with_root` in `cli.rs`, `common::Fixture` and `project_lease_cli.rs` (it builds `Layout::in_repo(&fx.root).state_dir`).
- [ ] **Step 2: Write the failing tests.** In `cli.rs`, in the style of `project_conflicts_with_root`, using `Cli::try_parse_from`: `tokens_cache_with_an_id_is_refused` (`["factory","tokens","1","--cache"]` is `ErrorKind::ArgumentConflict`), `tokens_without_id_or_cache_is_refused` (`["factory","tokens"]` is an error), `tokens_days_needs_cache_and_at_least_one` (`--days 3` alone errors, `--cache --days 0` errors), `tokens_cache_with_days_parses` (`["factory","tokens","--cache","--days","7"]` gives `cache == true` and `days == Some(7)`). In `tests/tokens_cache.rs` on `common::Fixture`:

```rust
#[test]
fn cache_report_reads_every_job_of_the_project() {
    let fx = Fixture::new();
    let plans = fx.write_plan("plan-01-a.md", 1, "none");
    let plans = plans.to_str().expect("utf8");
    fx.run(&["new", "a", "first", "--plans", plans]).expect("new a");
    fx.run(&["new", "b", "second", "--plans", plans]).expect("new b");
    // Write usage.jsonl into the first job folder only, the second has none.
    // The folders are under Layout::in_repo(&fx.root).state_dir.join("jobs").
    let out = fx.run(&["tokens", "--cache"]).expect("cache report");
    assert!(out.contains("builder"), "{out}");
    assert!(out.contains("resumed runs: 1"), "{out}");
}
```

  Also `cache_report_writes_no_file` (the file list of the state folder is equal before and after `tokens --cache`) and `tokens_for_one_job_is_unchanged` (`tokens 1` on a fixed two-line `usage.jsonl` equals a pinned expected string; run it once on unchanged code and paste the actual output into the test).
- [ ] **Step 3: Run** `cargo test -j4 -p factory-cli --test tokens_cache` and `cargo test -j4 -p factory-cli cli::`. Expected: failure, unknown argument `--cache`.
- [ ] **Step 4: Implement.** `Tokens { #[arg(required_unless_present = "cache", conflicts_with = "cache")] id: Option<String>, #[arg(long)] cache: bool, #[arg(long, requires = "cache", value_parser = clap::value_parser!(u32).range(1..))] days: Option<u32> }`. The dispatch matches `(id, cache)`: an id calls the existing `tokens`, `--cache` calls `cache_tokens`, which loops over `d.store().load()?.jobs`, calls `load_usage(&d.job_dir(job).join("usage.jsonl"), &job.id)`, and returns `render_cache(&cache_report(&jobs, days, now_secs() as u64))`. Add one paragraph on `--cache` to `README.md` and `docs/software-factory.md`, and a line under Item 4 of the roadmap that names the report and the three revisit conditions of ticket #3.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-cli`, `cargo clippy -j4 -p factory-cli -p factory-engine` and `cargo fmt --check`. Expected: all pass with no warnings.

Next plan: none
