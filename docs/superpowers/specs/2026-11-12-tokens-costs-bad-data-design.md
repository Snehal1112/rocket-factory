# Tokens and costs bad data design

## Goal

Make `factory tokens` count and show the usage lines it cannot read, warn about an unreadable usage file without hiding the other jobs, and stop its sums from overflowing.

## Overview

GitHub issue #61 audits `factory tokens` and `factory costs`. A code read in this session confirms the defects below. Today the figures look lower than reality with no hint.

- A. `load_usage` (`tokens.rs:527`) drops a line that is not JSON with `continue`, and reads every number with `v[k].as_u64().unwrap_or(0)`, so `"input":"12"` or `1.5e3` becomes 0. A row without `ts` gets `ts` 0, so `--days N` drops it (`tokens.rs:303`). `cache_tokens` (`commands.rs:1158`) uses `load_usage(...)?`, so one unreadable file (not NotFound) aborts the report for every job.
- B. `factory costs` writes `costs.json` only when the board loads (`gather_costs`, `engine/costs.rs:15`). The behaviour is right; the docs do not say it.
- C. `aggregate` (`tokens.rs:113`) adds with plain `+=`, while `raw()` and `weighted_tokens` saturate. `weighted * 100` in `tokens` (`commands.rs:1180`) overflows above `u64::MAX / 100`. A zero budget is not reachable, because `Config` rejects `budget_weighted == 0`. The message and `checked_div` stay as they are.
- D. The totals block uses board `job.tokens` and the body uses `usage.jsonl`. `weighted` floors `cache_read / 10` per record, so the two can differ by a few tokens. This is not a bug and needs a doc sentence.

## Current state

| Piece | Today | Gap |
|---|---|---|
| `load_usage` | `Result<Vec<UsageRecord>, io::Error>`; garbage lines skipped silently; wrong types read as 0 | No count |
| `aggregate` | `+=` on `raw`, `weighted`, `runs` | Overflow panics in debug, wraps in release |
| `render` | By-role rows, total, baseline percent | No skipped note |
| `cache_report` / `render_cache` | `--days` drops rows with `ts` 0 | Not stated |
| `cache_tokens` | `load_usage(...)?` per job and for the spec-review file | One bad file aborts all |
| `tokens` (one job) | `weighted * 100` | Overflow |
| `engine/costs.rs` `usage_summary` | Parses usage lines on its own | Not touched here |

## Non-goals

- Changing `usage_summary` in `factory-engine`. It parses separately, so `factory costs` and `factory tokens` can differ on bad lines. A follow-up covers it.
- Changing `costs.json`, the board JSON or the trend spec text.
- Auditing the usage writer in `factory-agent`. The change is defensive.
- The `0% of 0` message.

## Design

```
load_usage(path, job) -> Usage { records, skipped, no_ts }
tokens <id>      -> render adds "N lines skipped" when N > 0
tokens --cache   -> warnings first, then the report, with the skipped notes
```

### 1. `Usage` and `load_usage`

`pub struct Usage { pub records: Vec<UsageRecord>, pub skipped: u64, pub no_ts: u64 }` with `Default`. `load_usage` returns `Result<Usage, std::io::Error>`; a missing file gives `Usage::default()`.

A line is skipped, and counted in `skipped`, when it is not valid JSON, is not a JSON object, or holds a known number field whose value is present, not null and not a non-negative integer. The known number fields are the ones `load_usage` reads as numbers today: `input`, `cache_creation`, `output`, `cache_read`, `usage_v`, `turns`, `round`, `start_ctx`, `ts`, `tasks_done`, `cache_creation_1h`, `secs`, `start_cc`, `start_cr`, `end_ctx`. An absent or null field stays 0 or `None` as today. A skipped line adds nothing to `records`. A line for another job is not counted at all. A line that is garbage cannot name its job, so it counts for the file being read.

A record whose `ts` is absent or null is kept and counted in `no_ts`.

### 2. Job report

`TokenReport` gets `pub skipped: u64` (default 0). The `tokens` command sets it from `Usage.skipped` after `aggregate`. `render` ends with the line `  N lines skipped` when `skipped > 0` and prints nothing extra otherwise.

### 3. Cache report

`CacheReport` gets `pub skipped: u64` and `pub no_ts: u64`. `cache_tokens` sums them over all loaded files after `cache_report`. `render_cache` prints, right after its heading line and before any `no runs` line:

- `  N lines skipped` when `skipped > 0`;
- `  M rows without ts not counted by --days` when `days` is set and `no_ts > 0`.

With zero counts the output is unchanged.

### 4. Unreadable usage file in the cache report

In `cache_tokens`, when `load_usage` fails with an error other than NotFound for a file, the command adds no rows for it and puts the line `warning: could not read <path>: <err>` at the top of the output, then goes on. This applies to every job file and to the spec-review file. `tokens <id>` for one job still returns the error, because the user asked for that file.

### 5. Overflow

`aggregate` uses `saturating_add` for `raw`, `weighted` and `runs`. A function `fn budget_percent(weighted: u64, budget: u64) -> u64` in `commands.rs` returns `weighted.saturating_mul(100).checked_div(budget).unwrap_or(0)`, and `tokens` uses it.

### 6. Docs

- `docs/guides/factory-user-guide.md` (the `factory costs` entry near line 600): "If the board cannot be loaded the command fails and leaves the existing costs.json untouched."
- `docs/reference/software-factory.md` (the cost report paragraph near line 411): the same sentence.
- `docs/guides/factory-user-guide.md` section 6.10: one sentence that the totals come from the board and may differ by a few tokens from the per-role sum, and one that `N lines skipped` counts usage lines that could not be read.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| `"input":"12"`, `"input":1.5e3`, `"input":-1` | Line skipped and counted |
| `"input":null` or no `input` | 0, line kept |
| Line is `[]`, `3` or truncated JSON | Skipped and counted |
| Row without `ts`, no `--days` | Kept, not shown |
| Row without `ts`, `--days 7` | Not counted in the report, counted in the `no_ts` note |
| Usage file is not UTF-8 | Cache report: warning, other jobs reported. One job: error |
| Usage file missing | Empty, no warning |
| Totals near `u64::MAX` | Saturate, no panic |
| `budget_weighted` is 0 | Not reachable, `checked_div` kept |

## Testing

- `crates/factory-cli/src/tokens.rs` unit tests: `load_usage_counts_garbage_and_wrong_type_lines_as_skipped`, `load_usage_counts_rows_without_ts`, `aggregate_saturates_instead_of_overflowing`, `render_prints_the_skipped_line_count_only_when_above_zero`. The existing `load_reads_old_new_and_garbage_lines_together` is updated to the new return type.
- `crates/factory-cli/tests/tokens_cache.rs`: `tokens_cache_warns_and_continues_when_one_usage_file_is_not_utf8`, `tokens_cache_reports_skipped_lines`, `tokens_cache_states_rows_without_ts_under_days`, `tokens_for_one_job_still_errors_on_an_unreadable_usage_file`. `tokens_for_one_job_is_unchanged` must keep passing.
- `crates/factory-cli/src/commands.rs` tests: `tokens_percent_does_not_overflow_on_huge_totals`.
- Commands: `cargo test -j4 -p factory-cli`, then `cargo clippy -j4 -p factory-cli`.

## Files changed

- `crates/factory-cli/src/tokens.rs`, `commands.rs`
- Tests as listed above
- `docs/guides/factory-user-guide.md`, `docs/reference/software-factory.md`

## Constraints

- Add output only. With no skipped line, `factory tokens` and `factory costs` print what they print today.
- `costs.json` and board JSON do not change. Any new field is optional.
- No `/proc` reads, no `git` shell-outs, no `Cargo.toml` change. No file touched is in `risk_paths`.
- Use `-j4`. Comments are short full sentences.

## Acceptance criteria

- `factory tokens <id>` prints `N lines skipped` for garbage and wrong-type lines and nothing extra otherwise.
- `factory tokens --cache` warns about an unreadable usage file and still reports the other jobs.
- `factory tokens --cache --days N` states how many rows without `ts` were not counted.
- `factory tokens <id>` for an unreadable usage file still returns the error.
- `aggregate` and the percent calculation cannot overflow.
- The user guide and the reference say that `costs.json` is left untouched when the board fails to load, and the guide explains the totals difference.

## Out of scope

- A shared usage-line parser for `factory costs`.
- The trend spec wording.

## Follow-ups

- Make `usage_summary` in `factory-engine/src/costs.rs` count skipped lines the same way, so `factory costs` and `factory tokens` agree on bad usage lines.

## Decisions

- What happens to a line with a wrong-type number field? Options: skip the whole line and count it, keep it with the field as 0 and count it, parse the value. Chosen: skip the whole line and count it. Decided by: user
- Should `usage_summary` follow the new skip rule? Options: leave it and say so, share one parser, warn in costs only. Chosen: leave it and say so. Decided by: user
- Where do the skipped and no-ts notes print, and in what words? Options: one combined line, two lines. Chosen: two lines, the second only with `--days`. Decided by: call
- Does a wrong type in an optional number field (`secs`, `end_ctx` and the like) also skip the line? Options: yes, all known number fields; no, required fields only. Chosen: yes, all known number fields. Decided by: call
