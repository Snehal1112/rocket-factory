# Cost trend report: design

Ticket: https://github.com/Snehal1112/rocket-factory/issues/48

## Goal

Add `factory costs --trend`, a report that shows per size group whether the median weighted tokens per changed line fell, rose or stayed flat after each named change point, and show the same report in the dashboard.

## Overview

Raw job cost is not comparable across jobs because job size varies a lot (job 026 cost 628K weighted for 822 lines, job 003 cost 181K for 148 lines). `factory costs` prints per-job cost and per-group medians of the raw cost, so nobody can tell from it whether the factory got cheaper after a change such as the 45k carry cap, the quality levers or the rust preset check coverage. A per-line figure plus a before/after cut at each change point answers that question.

All inputs already exist: `usage.jsonl` lines carry `turns`, `resumed` and `round`; `Job.stage_history` records every stage the job passed; `gather_costs` already computes changed lines per job. Only the cut and the comparison are missing.

## Current state

| Piece | Today | Reference |
|---|---|---|
| Per-job record | `JobCost` has weighted, raw, runs, lines, files, group, roles. No turns, resumed count, round or park flag. | `crates/factory-core/src/cost.rs:139` |
| Usage sum | `usage_summary` sums tokens per role and provider, and counts runs. It ignores `turns` and `resumed`. | `crates/factory-core/src/cost.rs:83` |
| Gather | `gather_costs` reads `plan.md`, `usage.jsonl` and `diff_counts(base, head)` per board job. | `crates/factory-engine/src/costs.rs:14` |
| Median | `median_of` is private to `cost.rs`. | `crates/factory-core/src/cost.rs:193` |
| Feeding jobs | `stats` uses only merged jobs with data. | `crates/factory-core/src/cost.rs:269` |
| File | `CostsFile` has `groups` and `roles`, never a job list. | `crates/factory-core/src/cost.rs:322` |
| Command | `Command::Costs` has no flags. `costs()` prints and writes `costs.json`. | `crates/factory-cli/src/cli.rs:117`, `crates/factory-cli/src/commands.rs:588` |
| Few-jobs rule | `FEW_JOBS = 3` only adds a note. | `crates/factory-cli/src/costs.rs:9` |
| Config | `Config::parse` reads `project.toml` with `deny_unknown_fields`. `checks: Vec<CheckRule>` is the precedent for an array of tables. | `crates/factory-core/src/config.rs:107`, `:275` |
| Settings block | Every `Config` key must appear once in the settings block. | `crates/factory-core/src/settings_block.rs:116`, test at `:331` |
| Dashboard | `CostsView` is a typed copy of `costs.json`. `renderCostPanel` draws the group table and the role bar. | `dashboard/src/costs.rs:37`, `dashboard/static/board.js:541` |

Gaps: no per-line figure, no way to name a change point, no before/after comparison, no verdict, nothing in `costs.json` or the dashboard.

## Non-goals

- No change to plain `factory costs` output or to the group and role figures in `costs.json`.
- No comparison across projects. Jobs of different projects are never a before/after pair. The report reads one project's board only.
- No setting for the minimum job count or the verdict band. Both are fixed in code.
- No automatic detection of change points. The user names them.
- No statistical test. The verdict is a median comparison with a dead band.

## Design

Flow:

```
project.toml [[cost_markers]] ──┐
board jobs ─ gather_costs ──────┼─> build_trend ─> TrendReport ─┬─> factory costs --trend (text)
usage.jsonl, stage_history ─────┘                               └─> costs.json "trend" ─> dashboard
```

### 1. Markers in `project.toml`

New `Config` field `cost_markers: Vec<CostMarker>`, default empty.

```toml
[[cost_markers]]
name = "carry-cap-45k"
job = "022"
```

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CostMarker { pub name: String, pub job: String }
```

`Config::parse` rejects a marker whose `name` is empty, longer than 40 characters or has a control character, whose `job` is empty or not all ASCII digits, or whose `name` repeats another marker's name. The error names the marker. `job` is the first job id that ran with the change.

### 2. Per-job figures

`UsageSummary` gains `turns: u32` (sum of `turns` over the job's lines) and `resumed_runs: u32` (lines with `resumed` true). A missing or non-number `turns` counts as 0, a missing `resumed` as false. `JobCost` gains `turns: u32`, `resumed_runs: u32`, `round: u32` (from `Job.round`) and `parked: bool` (true when `Job.stage_history` has a `needs-human` entry). `gather_costs` fills them. The spec-review record sets `turns`, `resumed_runs` and `round` from the usage summary or 0, and `parked` false.

### 3. The trend model

New module `crates/factory-core/src/trend.rs`, exported from `lib.rs`. `median_of` in `cost.rs` becomes `pub(crate)`.

```rust
pub const MIN_JOBS_PER_SIDE: u32 = 3;
pub const VERDICT_BAND_PCT: i64 = 10;

pub fn build_trend(jobs: &[JobCost], markers: &[CostMarker]) -> Option<TrendReport>;

pub struct TrendReport { pub markers: Vec<MarkerTrend>, pub warnings: Vec<String> }
pub struct MarkerTrend {
    pub names: Vec<String>,   // more than one when markers collapse onto one cut
    pub cut: String,          // resolved job id
    pub from: Vec<String>,    // the configured job ids, same order as names
    pub groups: BTreeMap<String, GroupTrend>,  // small, medium, complex
}
pub struct GroupTrend {
    pub before: Side, pub after: Side,
    pub change_pct: Option<i64>,
    pub verdict: String,      // fell | rose | flat | not-enough-data
}
pub struct Side {
    pub n: u32, pub n_approx: u32,
    pub per_line: Option<Spread>,       // tokens per changed line: median, low, high
    pub median_turns: Option<u64>,
    pub resumed_pct: Option<u32>,
    pub rework_pct: Option<u32>,
    pub park_pct: Option<u32>,
}
```

It returns `None` when `markers` is empty. Rules:

1. Eligible jobs: stage `merged`, `has_data()`, `lines` is `Some(n)` with n above 0. A job in none of the three size groups is not eligible.
2. Resolve each marker: the first job on the board (any stage, numeric id) whose id, read as a number, is at or above the marker's `job`. No such job: skip the marker and add `warning: marker <name> (job <id>) is past the latest job`. Markers that resolve to the same job form one cut with several names. Cuts are sorted by resolved number.
3. For one cut, before is every eligible job below the cut, after is every eligible job from the cut up to the next cut (or the latest job for the last cut). Jobs are placed per size group.
4. Per job ratio: `(weighted + lines / 2) / lines` (integer, rounded). `per_line` is `spread` of the ratios.
5. If either side of a group has fewer than `MIN_JOBS_PER_SIDE` jobs, every figure of both sides is `None`, `change_pct` is `None` and the verdict is `not-enough-data`. `n` and `n_approx` are still filled. A group with no eligible job on both sides is left out.
6. `median_turns` is the median over the side's jobs of `turns`. `resumed_pct` is `resumed_runs` over `runs` summed over the side (0 when no runs), rounded to a whole percent. `rework_pct` is the share of jobs with `round >= 2`. `park_pct` is the share of jobs with `parked`. Shares round half up to a whole percent.
7. `change_pct = round((after - before) * 100 / before)` on the two medians. When the before median is 0 the group is treated as `not-enough-data`. Verdict: `fell` at or below -10, `rose` at or above +10, else `flat`.

### 4. `costs.json`

`CostsFile` gains `trend: Option<TrendReport>` with `#[serde(default, skip_serializing_if = "Option::is_none")]`. `CostsFile::build` sets it from `build_trend(jobs, &cfg.cost_markers)`. All types derive `Serialize`, `Deserialize` and `Clone`. An old file without `trend` still loads.

### 5. `factory costs --trend`

`Command::Costs { trend: bool }` with `#[arg(long)]`. The command always gathers and writes `costs.json` as today. With the flag it prints the trend text instead of the job tables, and the `warning: could not write costs.json` line still follows on a write error. Without the flag the output is unchanged. New `render_trend(report: Option<&TrendReport>) -> String` in `crates/factory-cli/src/costs.rs`:

```
MARKER carry-cap-45k (job 030, cut at 035)
GROUP    JOBS   TOK/LINE            CHANGE  VERDICT  TURNS     RESUMED   REWORK    PARKED    NOTES
small    6/5    1223 -> 760         -38%    fell     21 -> 14  40% -> 55% 17% -> 0%  33% -> 20%  1 approx before
medium   2/4    not enough data (2 before, 4 after)
```

JOBS is before/after counts. TOK/LINE is the two medians. TURNS, RESUMED, REWORK and PARKED print `before -> after`. NOTES lists approximate counts per side (`2 approx before, 1 approx after`), empty when none. Names show joined by `, ` when several markers share a cut. Each warning prints as its own line after the blocks. With no `cost_markers` the text is `no cost_markers in project.toml. Add one with [[cost_markers]], name = "...", job = "NNN".` followed by a newline.

### 6. Dashboard

`dashboard/src/costs.rs` gets `TrendView`, `MarkerView`, `GroupTrendView` and `SideView`, all with `#[serde(default)]`, and `CostsView.trend: Option<TrendView>`. `renderCostPanel` in `board.js` appends `renderTrendPanel(costs.trend)` after the role bar. When `trend` is missing or has no markers it adds nothing. Per marker it draws:

- a heading with the names and the cut job,
- a table with a row per group: before median, after median, change, verdict, turns, resumed, rework, parked, with `not enough data` spanning the figures when the verdict says so,
- an inline SVG with two bars per group (before, after) on one scale for that marker, and the verdict as text beside them. A `not-enough-data` group draws no bars. The SVG has a `<title>` and each bar an `aria-label` with its value. All text goes through `escapeHtml`.

### 7. Docs

- `docs/reference/project-toml-reference.md`: a "Cost markers" section with the table of `name` and `job`, the resolve rule and an example.
- `crates/factory-core/src/settings_block.rs`: list `cost_markers` once, in a "Cost markers." group, so the settings-block test passes.
- `README.md` and `dashboard/README.md`: one short paragraph each on `factory costs --trend` and the dashboard block.
- `docs/operations/releasing.md`: under "Cutting a release", a step: run `factory costs --trend` and paste the table into the release notes.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| No `cost_markers` | `--trend` prints the hint line. `costs.json` has no `trend`. Dashboard shows nothing extra. |
| Marker job id not on the board | Cut at the first board job at or above the id. Printed as `job 030, cut at 035`. |
| Marker past the latest job | Skipped, one `warning:` line. |
| Two markers resolve to one job | One block, both names. |
| Markers in any order in the file | Sorted by resolved id. |
| Side has fewer than 3 jobs | `not-enough-data`, no figures for the group. |
| Job without `base` or `head`, or zero lines | Left out of the figures. |
| Approximate jobs (`usage_v` below 2) | Kept. Counted per side in NOTES. |
| Before median is 0 | Group is `not-enough-data`. |
| Duplicate marker name, bad `job`, empty name | `Config::parse` error naming the marker. |
| `turns` or `resumed` missing in a usage line | Counts as 0 or false. |
| Old `costs.json` without `trend` | Loads. Dashboard shows no trend block. |
| A size group with jobs on only one side | The group shows `not-enough-data`. A group with no eligible job on either side is left out of that marker. |

## Testing

`factory-core`, `cargo test -j4 -p factory-core`:
- `config.rs`: `cost_markers_default_to_empty`, `cost_markers_parse_name_and_job`, `a_marker_with_a_bad_job_or_empty_name_is_refused`, `a_marker_name_over_40_characters_or_with_a_control_character_is_refused`, `duplicate_marker_names_are_refused`, `an_unknown_marker_key_is_refused`.
- `cost.rs`: `usage_summary_counts_turns_and_resumed_runs`, `usage_summary_treats_missing_turns_as_zero`, `an_old_costs_file_without_trend_still_loads`, `the_costs_file_carries_the_trend_and_omits_it_without_markers`.
- `trend.rs`: `no_markers_gives_no_report`, `before_is_everything_below_the_cut_and_after_stops_at_the_next_cut`, `a_marker_on_a_missing_job_cuts_at_the_next_board_job`, `a_marker_past_the_latest_job_warns_and_is_skipped`, `two_markers_on_one_job_share_a_block`, `markers_are_sorted_by_resolved_id`, `per_line_is_the_median_of_job_ratios` (an odd and an even count), `the_job_ratio_rounds_half_up` (for example 3 weighted over 2 lines is 2), `a_side_with_two_jobs_is_not_enough_data`, `a_group_with_no_eligible_job_on_either_side_is_left_out`, `a_group_with_jobs_on_one_side_only_is_not_enough_data`, `jobs_without_lines_or_data_are_left_out`, `change_pct_and_verdict_use_the_ten_percent_band` (cases -10, -9, 9, 10), `a_zero_before_median_is_not_enough_data`, `turns_resumed_rework_and_park_shares` (including 1 of 8 jobs rounding to 13%, half up), `a_side_with_no_runs_has_a_zero_resumed_share`, `approximate_jobs_are_counted_per_side`.
- `settings_block.rs`: the existing `every_config_key_is_listed_once` covers `cost_markers`.

`factory-engine`, `cargo test -j4 -p factory-engine --test costs`:
- `gather_costs_fills_turns_resumed_round_and_parked` (a merged job with a `needs-human` history entry and a usage file with `turns` and `resumed`).
- `a_merged_job_that_never_parked_has_parked_false`.
- `the_spec_review_record_has_zero_round_and_is_not_parked`.

`factory-cli`, `cargo test -j4 -p factory-cli`:
- `costs.rs`: `render_trend_prints_a_block_per_marker`, `render_trend_labels_a_group_without_enough_data`, `render_trend_without_markers_prints_the_hint`, `render_trend_prints_warnings_and_approx_notes`, `render_trend_joins_the_names_of_collapsed_markers`.
- `commands.rs` (unit or e2e): `trend_still_prints_the_costs_json_write_warning` (state dir made read-only).
- `e2e.rs`: `factory_costs_trend_prints_the_report_and_writes_trend_to_costs_json`, `factory_costs_without_the_flag_is_unchanged_with_markers_set`.

`dashboard`, `cargo test -j4 --manifest-path dashboard/Cargo.toml` and `bun test dashboard/tests/board.test.js` (the file's runner):
- `costs.rs`: `trend_loads_from_costs_json`, `a_costs_file_without_trend_has_none`.
- `board.test.js`: `renderTrendPanel draws a block, a table and bars per marker`, `renderTrendPanel shows not enough data and draws no bars`, `renderTrendPanel escapes marker names`, `renderTrendPanel gives the chart a title and each bar an aria-label`, `renderCostPanel without trend is unchanged`.

## Files changed

- `crates/factory-core/src/config.rs`, `src/cost.rs`, `src/trend.rs` (new), `src/lib.rs`, `src/settings_block.rs`
- `crates/factory-engine/src/costs.rs`, `tests/costs.rs`
- `crates/factory-cli/src/cli.rs`, `src/commands.rs`, `src/costs.rs`, `tests/e2e.rs`
- `dashboard/src/costs.rs`, `dashboard/static/board.js`, `dashboard/static/dashboard.css`, `dashboard/tests/board.test.js`, `dashboard/README.md`
- `docs/reference/project-toml-reference.md`, `docs/operations/releasing.md`, `README.md`

## Constraints

- Always `-j4`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- New `JobCost` fields mean every test that builds a `JobCost` literal must be updated.
- `costs.json` stays compatible with the dashboard: `trend` is optional and the dashboard types use `#[serde(default)]`.
- Paths go through `Layout`. Never shell out to `git`.
- Tests use `.expect("...")`, never the unwrap call.

## Acceptance criteria

- `factory costs --trend` prints, per marker and size group, the before and after median weighted tokens per changed line, the change in percent and a verdict of `fell`, `rose` or `flat`.
- A group with fewer than 3 eligible jobs on either side prints `not enough data` and no number.
- Before is every eligible job below the cut. After runs from the cut to the next cut or the latest job.
- A marker on a job id missing from the board cuts at the next board job and prints the resolved id.
- A marker past the latest job is skipped with a `warning:` line.
- The report shows median turns, resumed-run share, rework rate (`round >= 2`) and park rate (`needs-human` in `stage_history`) per side.
- The verdict uses a 10% dead band: `fell` at -10% or lower, `rose` at +10% or higher.
- Markers come from `[[cost_markers]]` in `project.toml`, and a bad or duplicate marker is refused with the marker named.
- `costs.json` gets an optional `trend` field, absent without markers, and old files still load.
- Plain `factory costs` prints exactly what it printed before.
- The dashboard shows a table and an SVG chart per marker, shows `not enough data` for thin groups, and shows nothing when `trend` is missing.
- `docs/operations/releasing.md` lists `factory costs --trend` as a release note step.

## Out of scope

- Filling in the real markers for rocket-factory's own history (a `project.toml` edit by the maintainer).
- Settings for the minimum job count or the verdict band.
- Comparing projects with each other.

## Decisions

- Where do the change-point markers come from? Options: a `[[cost_markers]]` list in `project.toml` (name plus first job id); a `markers.toml` in the state dir; markers by timestamp. Chosen: a `[[cost_markers]]` list in `project.toml`. Decided by: user
- How are before and after cut with several markers? Options: segments between consecutive markers; everything before versus everything after; a fixed window of N jobs; everything before versus the span up to the next marker. Chosen: everything before the marker versus the span from the marker to the next marker (or the latest job for the last one). Decided by: user
- How many jobs does a side need before a number shows? Options: 3 per side per group fixed in code; 5 per side; a `project.toml` setting defaulting to 3. Chosen: at least 3 jobs on each side per size group, fixed in code, else "not enough data". Jobs without a line count or with zero lines are left out of the figures. Decided by: user
- How is tokens per line computed for a side? Options: median of each job's own ratio; total weighted over total lines; both. Chosen: the median of each job's own `weighted / lines`, with the low and high of the ratios. Decided by: user
- What about approximate jobs? Options: keep them and show the approximate count per side; drop them; show both. Chosen: keep them in the figures and show the approximate count per side. Decided by: user
- How are the four extra figures defined? Options: turns summed per job then median, resumed runs over all runs, rework as share with `round >= 2`, park as any `needs-human` entry in `stage_history`; or turns per run and park from `park_kind`. Chosen: the first set. Decided by: user
- Where does the report live? Options: a `factory costs --trend` flag with an optional `trend` field in `costs.json`; always append to `factory costs`; a separate `factory trend` command. Chosen: the `--trend` flag, plain `factory costs` unchanged, `costs.json` gets an optional `trend` field, the dashboard shows it. Decided by: user
- How does the report say whether the median fell? Options: percent change plus a verdict word with a 10% dead band; percent only; verdict word with no band. Chosen: percent change plus `fell` at -10% or lower, `rose` at +10% or higher, else `flat`, fixed in code. Decided by: user
- How does the dashboard show the trend? Options: a table under the cost panel; a small chart per marker; only a verdict column. Chosen: the table (one block per marker, a row per size group) plus a small chart per marker. Decided by: user
- What form does the chart take? Chosen: inline SVG without a library, per marker a pair of bars (before median, after median) per size group on one shared scale, with the verdict as text beside them; a group with too few jobs draws no bars. Decided by: call
- What if a marker's job id is not on the board? Options: first board job at or above the id, skipped with a warning when none; exact match only; fail the report. Chosen: first board job whose id is at or above the marker id, resolved id printed, markers sorted by resolved id; past the latest job the marker is skipped with a warning; two markers on one job collapse into one cut and show both names. Decided by: user
- What does `--trend` print when no markers exist? Chosen: one hint line with a `[[cost_markers]]` example, exit 0. Decided by: call
