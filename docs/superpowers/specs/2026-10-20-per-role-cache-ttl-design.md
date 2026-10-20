# Cache report for the per-role TTL decision

## Goal

Add `factory tokens --cache`, a project-wide report of prompt cache use per role and of resumed runs that rewrote the cache, so the per-role cache TTL question in ticket #3 can be decided from data.

## Overview

Ticket #3 parks a per-role `cache_ttl` until there is more resumed-run data. It names three revisit conditions: enough runs that log `cache_creation_1h`, resumed builders after more than 5 minutes that rewrite the cache more than once, and an explanation for the 49-minute miss in rocket job 023.

Checked on 2026-10-20 over `~/.factory/projects/*/state/jobs/*/usage.jsonl`: 124 rows carry `cache_creation_1h`, and all of them are 0, so the global `cache_ttl = "5m"` works. None of those 124 rows is a resumed run (builder, committer, reviewer, message and kb rows all have `resumed: false`), so the conditions cannot be judged yet. Nothing today answers "which resumed runs idled past 5 minutes, and did they rewrite the cache" without a hand-written script. This spec builds that answer. It does not build the per-role setting.

## Current state

| Area | Today | Reference |
|---|---|---|
| TTL setting | One `cache_ttl` per project, `"5m"` or `"1h"` | `crates/factory-core/src/config.rs:100` |
| TTL env var | `run_agent` pushes `FORCE_PROMPT_CACHING_5M` or `ENABLE_PROMPT_CACHING_1H` on every agent | `crates/factory-engine/src/ports.rs:167-169`, `cache_ttl_var` at `:236` |
| Usage line | Written once per agent run, has `cache_creation_1h` and `ts` (the end time), no duration | `usage_line` in `crates/factory-engine/src/ports.rs:245` |
| Run timing | `run_agent` holds `started: Instant` and logs `duration_ms` to tracing only | `crates/factory-engine/src/ports.rs:176-185` |
| Usage record | `UsageRecord` has no `cache_creation_1h` and no duration | `crates/factory-cli/src/tokens.rs:21-38` |
| Reader | `load_usage` reads one job's file and skips unreadable lines | `crates/factory-cli/src/tokens.rs:126-160` |
| Per-job report | `factory tokens <id>`, with run rows | `crates/factory-cli/src/commands.rs:824` |
| All-jobs walk | `gather_costs` loops over `board.jobs` and `driver.job_dir(job)` | `crates/factory-engine/src/costs.rs:13-33` |
| CLI arg | `Tokens { id: String }` | `crates/factory-cli/src/cli.rs:75` |

Gaps: no view across jobs, no 5m/1h split in any report, no idle time for a resumed run, no flag for a cache rewrite.

## Non-goals

- No `cache_ttl_by_role` setting and no change to `cache_ttl_var` or `run_agent`'s env handling. See Follow-ups.
- No change to `weighted_tokens` or to the board tally.
- No cross-project view. The report reads the `--project` given.
- No fix for the 49-minute miss of rocket job 023. Its row has no `secs`, so the report shows its gap as `?`.

## Design

Types: `cache_report(records_by_job: &[Vec<UsageRecord>], days: Option<u32>, now: u64) -> CacheReport` holds a `Vec<RoleCache>` and a `Vec<ResumedRun>`; `ResumedRun.gap` is `Option<u64>`. `render_cache(&CacheReport) -> String` prints it. Columns are left-aligned and padded like `render_detail`.

Flow: `run_agent` logs `secs` -> `usage.jsonl` -> `load_usage` per job -> `cache_report` joins jobs and pairs each resumed run with its predecessor -> `render_cache`.

### 1. `secs` in `usage.jsonl`

`usage_line` gets a `secs: u64` parameter and writes `"secs"` in every line. `run_agent` passes `started.elapsed().as_secs()` through `log_usage`. The field is additive and optional for readers, so old lines and the dashboard are unaffected.

The line shows the new key; real lines carry the other fields too.

```json
{"job":"007","role":"builder","resumed":true,"ts":1792457281,"secs":131,"cache_creation":3415,"cache_creation_1h":0,"cache_read":118160}
```

### 2. `UsageRecord` fields

`UsageRecord` gets `cache_creation_1h: Option<u64>` and `secs: Option<u64>`. `load_usage` sets them from the JSON keys, `None` when the key is absent. A line before 2026-10-19 has no `cache_creation_1h`, so it stays `None` and is never read as 0.

### 3. `factory tokens --cache [--days N]`

CLI shape: `Tokens { #[arg(required_unless_present = "cache", conflicts_with = "cache")] id: Option<String>, #[arg(long)] cache: bool, #[arg(long, requires = "cache", value_parser = clap::value_parser!(u32).range(1..))] days: Option<u32> }`. The dispatch at `commands.rs:329` matches `(id, cache)`: an id runs the existing `tokens` function, `--cache` runs `cache_tokens`. `factory tokens <id>` behaves as today. With `--days N`, only runs with `ts >= now - N * 86400` (inclusive) are listed and counted. `now` is a parameter of `cache_report`, and the command passes `now_secs()`. With no `--days`, every run counts.

Runs come from `usage.jsonl` of every job on the board, in board order, using `load_usage`. A job with no file adds nothing.

### 4. Role table

One row per role, sorted by role name:

| Column | Meaning |
|---|---|
| RUNS | rows for the role |
| CREATION | sum of `cache_creation` |
| READ | sum of `cache_read` |
| CREATION% | `CREATION / (CREATION + READ)` as one decimal, `-` when both are 0 |
| 5M | sum of `cache_creation - cache_creation_1h` over rows that have the field, else `n/a` |
| 1H | sum of `cache_creation_1h` over rows that have the field, else `n/a` |

The 5M and 1H sums cover only rows with the field. A footnote line `split covers N of M runs` appears when `N < M`.

### 5. Resumed runs

A resumed run is a row with `resumed: true`. Its predecessor is the previous row in the same job file (any role), which is chronological because the driver appends one row at a time. Gap in seconds is `ts - secs - previous.ts`, floored at 0. The gap is `None` when the run has no `secs` or has no predecessor.

`SHARE = cache_creation / (cache_creation + cache_read)` of the resumed run. A run counts as a rewrite when `SHARE >= 0.25`. The threshold is a constant `REWRITE_SHARE` in `tokens.rs`. A run counts as idle when its gap is more than 300 seconds (`CACHE_5M_SECS`).

```
RESUMED RUNS
JOB  ROLE     ROUND  GAP     CREATION  READ     SHARE  REWROTE
023  builder  2      2916s   108782    215791   33.5%  yes
020  builder  2      244s    3415      118160   2.8%   no
```

Rows are in job and log order. The last line is a summary: `resumed runs: N, idle over 5m: A, of those rewrote: B, rewrote with gap unknown: C`. With no resumed runs the section prints `no resumed runs` and no summary.

### 6. Header

First line: `cache report (<N> runs, all time)` or `cache report (<N> runs, last <D> days)`. `<N>` counts the runs left after the `--days` filter, and the role table and the summary use the same filtered runs. With no runs the report prints the header and `no runs`. The header carries no project name, because `CliCtx` holds none.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| `--cache` with an id | clap error, exit 2 |
| No id and no `--cache` | clap error, exit 2 |
| `--days` without `--cache`, or `--days 0` | clap error, exit 2 |
| Job folder or `usage.jsonl` missing | job adds nothing |
| Unreadable JSON line | skipped, as `load_usage` does today |
| Old line without `cache_creation_1h` | split columns skip it, footnote shows the coverage |
| Old line without `secs` | gap `?`, counted under "gap unknown" when it rewrote |
| First row of a job is resumed | no predecessor, gap `?` |
| `ts - secs` below `previous.ts` | gap 0 |
| `cache_creation + cache_read` is 0 | share `-`, never a rewrite |
| `--days` window cuts off a predecessor | gap still computed from all rows, only listing is filtered |
| `cache_creation_1h` above `cache_creation` | 5M uses `saturating_sub`, no panic |

## Testing

`factory-engine`, `crates/factory-engine/src/ports.rs` unit tests:
- `usage_line_has_the_run_duration`: `usage_line` with `secs` 42 gives `line["secs"] == 42`. Extend `usage_line_has_the_counters_and_the_run_details`. Update the other `usage_line(...)` test callers in the same module (four more calls) for the new parameter.

`factory-engine`, `crates/factory-engine/tests/runloop.rs`:
- `usage_lines_carry_the_run_details` also asserts `v["secs"].is_u64()` on every line.

`factory-cli`, `crates/factory-cli/src/tokens.rs` unit tests:
- `load_usage_reads_the_split_and_the_duration_when_present`
- `load_usage_leaves_split_and_duration_none_on_old_lines`
- `cache_roles_sum_creation_read_and_split` (5M and 1H from rows that have the field)
- `cache_split_is_n_a_when_no_row_has_it`
- `cache_footnote_shows_split_coverage`
- `resumed_gap_is_ts_minus_secs_minus_previous_ts`
- `resumed_gap_is_unknown_without_secs_or_predecessor`
- `resumed_gap_floors_at_zero`
- `rewrite_threshold_is_25_percent_inclusive`: shares 24.9% and 25.0% give no and yes.
- `zero_creation_and_read_is_never_a_rewrite`
- `summary_counts_idle_rewrites_and_unknown_gaps`
- `days_filter_lists_recent_runs_but_keeps_the_predecessor_gap`
- `no_resumed_runs_prints_the_empty_line`
- `five_m_split_saturates`
- `days_cutoff_is_inclusive`: `ts == now - N * 86400` is listed, one second older is not.
- `header_names_all_time_or_the_day_window_and_empty_prints_no_runs`
- `creation_percent_is_one_decimal_and_dash_when_empty`
- `roles_are_sorted_by_name`

`factory-cli`, `crates/factory-cli/src/cli.rs` unit tests, in the style of `project_conflicts_with_root`, using `Cli::try_parse_from`:
- `tokens_cache_with_an_id_is_refused`
- `tokens_without_id_or_cache_is_refused`
- `tokens_days_needs_cache_and_at_least_one`
- `tokens_cache_with_days_parses`

`factory-cli`, new `crates/factory-cli/tests/tokens_cache.rs`, built on `common::Fixture`:
- `cache_report_reads_every_job_of_the_project`: two jobs, one without `usage.jsonl`.
- `cache_report_writes_no_file`: the state folder listing is the same before and after.
- `tokens_for_one_job_is_unchanged`: `factory tokens 1` on a fixed `usage.jsonl` equals a pinned expected string.

Commands: `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`, `cargo clippy -j4`, `cargo fmt --check`.

## Files changed

- `crates/factory-engine/src/ports.rs`: `usage_line` and `log_usage` take and write `secs`.
- `crates/factory-cli/src/tokens.rs`: record fields, `cache_report`, `render_cache`, constants.
- `crates/factory-cli/src/cli.rs`: `Tokens` args and parse tests.
- `crates/factory-cli/src/commands.rs`: dispatch and a `cache_tokens` function.
- `crates/factory-cli/tests/tokens_cache.rs`: new.
- `crates/factory-engine/tests/runloop.rs`: extended assertion.
- `README.md`, `docs/software-factory.md`: one paragraph on `--cache`.
- `docs/roadmap-2026-10-18.md`: Item 4 line pointing at the report and the three revisit conditions.

## Constraints

- Run cargo with `-j4`, no `--workspace`, no `CARGO_TARGET_DIR`.
- Board and ledger are untouched. `usage.jsonl` gains one optional key and no key is renamed.
- The report only reads files. It writes nothing and never calls `claude`, `git` or `gh`.
- Paths come from `Layout` and `driver.job_dir`, no hardcoded ones (`no_hardcoded_paths.rs`).
- Comments are short full sentences.

## Acceptance criteria

- `usage.jsonl` lines written after this change carry `secs`, and lines without it still load.
- `factory tokens --cache` prints a role table with runs, creation, read, creation percent, 5M and 1H.
- The 5M and 1H columns show `n/a` when no row has the split, and a coverage footnote when only some do.
- Each resumed run is listed with its gap, creation, read, share and rewrite flag.
- A resumed run with share at or above 25% counts as a rewrite, below it does not.
- A gap is `ts - secs - previous.ts` within the same job, `?` when unknown, never negative.
- The summary line counts idle resumes over 5 minutes, those that rewrote, and rewrites with an unknown gap.
- `--days N` limits the listing to the last `N` days.
- `--cache` with an id, no id without `--cache`, `--days` alone and `--days 0` are refused by clap.
- The header and the tables count only the runs left after `--days`.
- `factory tokens <id>` output is unchanged.
- The report writes nothing and needs no network.

## Out of scope

- A per-role TTL, any change to env vars set for agents, and any change to defaults.
- A dashboard view of the report.
- Reports across registered projects.

## Follow-ups

- Build a per-role cache TTL (`cache_ttl_by_role` or `CLAUDE_CODE_PROMPT_CACHE_TTL` per run) only when `factory tokens --cache` shows, over enough runs, that resumed builders idle more than 5 minutes and rewrite the cache repeatedly, and rocket job 023's 49-minute miss is explained. Notes from ticket #3: `FORCE_PROMPT_CACHING_5M` beats every other setting and `ENABLE_PROMPT_CACHING_1H` sits low in the precedence, so a per-role setting should use `CLAUDE_CODE_PROMPT_CACHE_TTL`.

## Decisions

- What should this spec deliver? Options: a measure-only cache report, build `cache_ttl_by_role` now, docs and config only. Chosen: the measure-only report. Decided by: user
- Where does the report live? Options: `factory tokens --cache` over all jobs, extra columns in the per-job `factory tokens <id>`, a section in `factory costs`. Chosen: `factory tokens --cache` over all jobs. Decided by: user
- How is the idle gap measured? Options: log `secs` in `usage.jsonl`, use `ts` differences only, read hook events from `events.jsonl`. Chosen: log `secs`. Decided by: user
- When is a resumed run a cache rewrite? Options: creation share at least 25%, above 20%, at least 50%. Chosen: at least 25%, because job 023 was 33% and the other three known resumes were 1% to 5%. Decided by: user
- What happens to ticket #3? Options: list the per-role TTL under Follow-ups so the factory files a new ticket and closes #3, or keep #3 open. Chosen: Follow-ups. Decided by: user
- Which jobs does the report cover? Options: the `--project` with optional `--days N`, every registered project. Chosen: the `--project` with `--days N`. Decided by: user
- The predecessor of a resumed run is the previous row of the same job, any role. Decided by: call
- The idle limit is 300 seconds, the 5 minute TTL. Decided by: call
- `--days` filters the listing but not the gap computation. Decided by: call
- The header carries no project name, because `CliCtx` holds none. Decided by: call
- Refusals are tested with `Cli::try_parse_from`, because `Fixture::run` exits the process on a clap error. Decided by: call
