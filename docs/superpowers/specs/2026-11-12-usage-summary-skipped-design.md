# Usage summary skipped lines design

## Goal

Make `factory costs` apply the same skip rule as `factory tokens` to bad usage lines and tell the user how many lines it skipped.

## Overview

GitHub issue #66 follows #61. `factory tokens` now counts usage lines it cannot read (`tokens.rs:565`). `usage_summary` (`factory-core/src/cost.rs:88`) feeds `factory costs` and parses the same files on its own, so on a bad line the two commands disagree. The ticket named `factory-engine/src/costs.rs` as the file; the parser is in `factory-core`, and the engine only calls it (`engine/costs.rs:25`).

- A non-JSON line is dropped without a count (`cost.rs:91-93`).
- A line with `"input":"12"` is kept and the field reads as 0 (`cost.rs:98`), so the line adds runs and turns but no tokens. `factory tokens` drops the whole line.
- The usage writer only writes non-negative integers (`engine/ports.rs:382-406`), so lines written today are never skipped. The change is defensive.

## Current state

| Piece | Today | Gap |
|---|---|---|
| `load_usage` (`tokens.rs:565`) | Skips a non-object line and a line with a bad `COUNT_FIELDS` value, counts both | The rule lives in factory-cli only |
| `COUNT_FIELDS` (`tokens.rs:551`) | Nine count fields | Not shared |
| `usage_summary` (`cost.rs:88`) | Drops non-JSON silently, reads bad numbers as 0 | No skip rule, no count |
| `UsageSummary`, `JobCost` | No skipped field | Nowhere to carry a count |
| `gather_costs` (`engine/costs.rs:15`) | Builds a `JobCost` per board job and one for the spec-review log | Does not copy a count |
| `render_costs` (`cli/costs.rs:19`) | Per-job rows, group tables, role split | Silent about skipped lines |

## Non-goals

- Moving the whole line parser. Both parsers stay; only the rule is shared.
- Any change to `costs.json`, the board JSON or the dashboard. The count is not written to `costs.json`.
- Changing the warning or notes of `factory tokens`.

## Design

```
factory-core:  has_bad_count(&Value) -> bool    (the shared rule)
load_usage     -> uses has_bad_count
usage_summary  -> uses has_bad_count, counts skipped
JobCost.skipped -> gather_costs -> render_costs prints one line when the sum is above zero
```

### 1. The shared rule

In `factory-core/src/cost.rs`: `pub const USAGE_COUNT_FIELDS: [&str; 9]` with the nine fields now in `COUNT_FIELDS` (`input`, `cache_creation`, `output`, `cache_read`, `usage_v`, `turns`, `round`, `start_ctx`, `ts`), and `pub fn has_bad_count(line: &serde_json::Value) -> bool`. It is true when any of those fields is present, not null and not a non-negative integer. Both are exported from the crate root. `tokens.rs` drops its own constant and calls `has_bad_count`. Its behaviour does not change.

### 2. `usage_summary`

`UsageSummary` gets `pub skipped: u32`, default 0. A line is skipped and counted, with `saturating_add`, when it is not valid JSON, is not a JSON object, or belongs to the job and `has_bad_count` is true. As in `load_usage`, a non-JSON line counts for the file being read and a bad-count line counts only for its own job. A skipped line adds nothing to the figures. The optional fields (`start_cc`, `end_ctx` and the like) are not read here and stay unchecked.

### 3. `JobCost` and `gather_costs`

`JobCost` gets `pub skipped: u32`, default 0, not serialised anywhere. `gather_costs` copies `usage.skipped` into it for each board job and for the spec-review record.

### 4. `render_costs`

When the sum of `skipped` over all records (the spec-review record included) is above zero, `render_costs` prints one line `N usage lines skipped` right after the per-job rows and before the blank line that precedes the group table. With no jobs the early `no jobs` return is unchanged and prints no skipped line. With a zero sum the output is unchanged.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| Non-JSON or non-object line in a job's `usage.jsonl` | Skipped and counted for that job |
| `"input":"12"`, `1.5e3`, `-1` on a line of the job | Skipped and counted, no tokens, no run |
| `"input":null` or no `input` | Kept, field 0 |
| Wrong-type optional field | Not read by `usage_summary`; line kept |
| Bad line of another job | Not counted for this job |
| Bad lines only in the spec-review log | Counted in the summary line |
| Missing usage file | `UsageSummary::default()`, no count |
| Count near `u32::MAX` | Saturates |

## Testing

- `crates/factory-core/src/cost.rs` unit tests: `has_bad_count_flags_wrong_type_count_fields` (`"12"`, `1.5e3`, `-1` true; `null`, absent, `5` false), `usage_summary_counts_and_drops_bad_lines` (a garbage line, a wrong-type line and one good line give `skipped == 2`, `runs == 1`), `usage_summary_does_not_count_other_jobs_bad_lines`.
- `crates/factory-cli/src/costs.rs` unit tests: `render_costs_prints_the_skipped_line_only_when_above_zero`.
- `crates/factory-engine/tests/costs.rs`: `gather_costs_carries_the_skipped_count` (a job whose `usage.jsonl` holds a bad line).
- Existing `usage_summary_sums_this_jobs_lines_and_weights_cache_reads` and the `tokens.rs` tests must keep passing.
- Commands: `cargo test -j4 -p factory-core`, `-p factory-engine`, `-p factory-cli`, then `cargo clippy -j4 -p factory-core -p factory-engine -p factory-cli`.

## Files changed

- `crates/factory-core/src/cost.rs`, `lib.rs`
- `crates/factory-engine/src/costs.rs`
- `crates/factory-cli/src/costs.rs`, `tokens.rs`
- Tests as listed above

## Constraints

- Output only grows: with no skipped line `factory costs` prints what it prints today.
- `costs.json` and board JSON do not change. No `Cargo.toml` change, no `git` shell-outs, no `/proc` reads.
- `cost.rs` is in `factory-core`, which has no I/O dependencies; the rule takes a `serde_json::Value` and does no I/O.
- Use `-j4`. Comments are short full sentences.

## Acceptance criteria

- `factory costs` prints `N usage lines skipped` after the job rows when a usage file holds a non-JSON line or a line with a wrong-type count field.
- `factory costs` output is unchanged when no line is skipped.
- A wrong-type line adds no tokens, runs or turns to a job in `factory costs`, as in `factory tokens`.
- `factory tokens` and `factory costs` use one rule function, `has_bad_count`.
- `costs.json` is unchanged.

## Out of scope

- A per-job marker on the rows, and a `skipped` field in `costs.json`.
- Sharing the whole line parser.

## Follow-ups

None.

## Decisions

- How should the two parsers agree on the skip rule? Options: one shared rule function, copy the rule into `usage_summary`, one shared parser. Chosen: one shared rule function. Decided by: user
- Where does `factory costs` show the count? Options: a summary line only when above zero, a marker on each job row, a summary line plus a `costs.json` field. Chosen: a summary line only when above zero. Decided by: user
- Does the summary line count the spec-review log? Options: yes, no. Chosen: yes, so a bad line there is not hidden. Decided by: call
