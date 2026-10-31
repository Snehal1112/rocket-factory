# Cost trend report, plan 1: core, engine and CLI

> **Spec:** docs/superpowers/specs/2026-10-31-cost-trend-report-design.md
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** `factory costs --trend` prints per size group whether the median weighted tokens per changed line fell, rose or stayed flat after each named marker, and `costs.json` carries the same report.

**Architecture:** Markers are a new `Config` list. `gather_costs` fills four new per-job figures. A new pure module `factory-core/src/trend.rs` turns `JobCost` records and markers into a `TrendReport`. `CostsFile` carries it as an optional field, and `factory-cli` renders it.

**Tech Stack:** Rust, serde, toml, clap. Tests with `cargo test -j4 -p <crate>`.

## Global Constraints

- Always `-j4` and `-p <crate>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Markers are written `[[settings.cost_markers]]` in `project.toml` (`[[cost_markers]]` in `config.toml`).
- `MIN_JOBS_PER_SIDE = 3` and `VERDICT_BAND_PCT = 10`, fixed in code, no setting.
- `costs.json` field `trend` is optional (`#[serde(default, skip_serializing_if = "Option::is_none")]`). Old files must still load.
- Plain `factory costs` output does not change.
- Rust follows the CI gate for every changed crate: `cargo fmt -p <crate>` leaves nothing to change and `cargo clippy -j4 -p <crate> --all-targets -- -D warnings` prints no warning. No `#[allow(...)]` to silence a lint, fix the code instead. Public items get a one-line doc comment.
- Never shell out to `git`. Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- Code comments are short full sentences that end with a period.

## Review Focus

1. Marker ids compare as numbers, not text: marker `"999"` against a board with job `1000` cuts at `1000`. Pinned by `marker_ids_compare_as_numbers` (Task 2).
2. An after median of 0 (jobs that spent no tokens) is a 100% drop, not a crash. Pinned by `an_after_median_of_zero_is_a_drop_of_one_hundred_percent` (Task 2).
3. Huge weighted values must not overflow the per-line ratio. Pinned by `huge_weighted_values_do_not_overflow_the_ratio` (Task 2).
4. The pseudo job `spec-review` (non-numeric id) is on the cost list and must never be a cut or crash the cut search. Pinned by `non_numeric_job_ids_are_ignored_for_cuts` (Task 2).
5. A marker block with no eligible job around its cut prints a plain sentence, not an empty table. Pinned by `render_trend_says_so_when_a_block_has_no_groups` (Task 3).

## File Structure

| File | Responsibility |
|---|---|
| `crates/factory-core/src/config.rs` | `CostMarker`, `Config.cost_markers`, marker validation |
| `crates/factory-core/src/settings_block.rs` | List `cost_markers`, treated like `checks` |
| `crates/factory-core/src/cost.rs` | New `UsageSummary` and `JobCost` figures, `CostsFile.trend`, `pub(crate)` `median_of` and `spread` |
| `crates/factory-core/src/trend.rs` (new) | `build_trend` and the report types |
| `crates/factory-core/src/lib.rs` | Exports |
| `crates/factory-engine/src/costs.rs` | `gather_costs` fills the new figures |
| `crates/factory-cli/src/cli.rs`, `commands.rs`, `costs.rs` | `--trend` flag and `render_trend` |
| `crates/factory-cli/tests/e2e.rs` | End to end test |

### Task 1: Markers and per-job figures

**Files:**
- Modify: `crates/factory-core/src/config.rs`, `crates/factory-core/src/settings_block.rs`, `crates/factory-core/src/cost.rs`, `crates/factory-core/src/lib.rs`
- Modify: `crates/factory-engine/src/costs.rs`
- Test: `crates/factory-core/src/config.rs` (tests module), `crates/factory-core/src/cost.rs` (tests module), `crates/factory-core/src/settings_block.rs` (tests module), `crates/factory-engine/tests/costs.rs`

**Interfaces:**
- Produces, in `config.rs`: `pub struct CostMarker { pub name: String, pub job: String }` (derives `Debug, Clone, PartialEq, Eq, Serialize, Deserialize`, with `#[serde(deny_unknown_fields)]`) and `Config.cost_markers: Vec<CostMarker>`, default `Vec::new()`. Exported from `lib.rs`.
- Produces, in `cost.rs`: `UsageSummary.turns: u32`, `UsageSummary.resumed_runs: u32`; `JobCost` gains `turns: u32`, `resumed_runs: u32`, `round: u32`, `parked: bool` and derives `Default`.
- The engine change sits behind no port. `gather_costs` calls `driver.store()`, `driver.repo()` and the job folder directly, as it does today. No board, ledger or events field is added.

- [ ] **Step 1: Read the code this task touches.** Read `crates/factory-core/src/config.rs` (the `Config` struct, `impl Default`, `Config::parse` and its tests), `settings_block.rs` lines 136 to 225 and the test helpers at 308 to 360 (`checks` is the model to copy), `cost.rs` (`UsageSummary`, `usage_summary`, `JobCost`, and the `job()` test helper), `crates/factory-engine/src/costs.rs` and `crates/factory-engine/tests/costs.rs`.

- [ ] **Step 2: Write the failing config tests** in the `config.rs` tests module. One full example, the rest in the table:

```rust
#[test]
fn cost_markers_parse_name_and_job() {
    let cfg = Config::parse("[[cost_markers]]\nname = \"carry-cap-45k\"\njob = \"022\"\n").expect("parses");
    assert_eq!(
        cfg.cost_markers,
        vec![CostMarker { name: "carry-cap-45k".into(), job: "022".into() }]
    );
}

#[test]
fn a_bad_marker_is_refused_with_its_name() {
    let err = Config::parse("[[cost_markers]]\nname = \"m1\"\njob = \"2x\"\n").expect_err("bad job");
    assert!(err.to_string().contains("m1"), "{err}");
}
```

| Test | Input | Expectation |
|---|---|---|
| `cost_markers_default_to_empty` | `Config::default()` | `cost_markers.is_empty()` |
| `a_marker_with_a_bad_job_or_empty_name_is_refused` | name `""`; job `""`; job `"-1"` | each `expect_err`, the empty-name message says `cost marker #1` |
| `a_marker_name_over_40_characters_or_with_a_control_character_is_refused` | name of 41 `a`; name `"a\u0007b"` | each `expect_err` |
| `duplicate_marker_names_are_refused` | two markers named `m` | `expect_err` containing `m` |
| `an_unknown_marker_key_is_refused` | marker with `extra = 1` | `expect_err` |

- [ ] **Step 3: Write the failing usage and settings tests.** In `cost.rs` tests:

```rust
#[test]
fn usage_summary_counts_turns_and_resumed_runs() {
    let text = concat!(
        "{\"job\":\"001\",\"role\":\"builder\",\"turns\":7,\"resumed\":true,\"usage_v\":2}\n",
        "{\"job\":\"001\",\"role\":\"builder\",\"turns\":5,\"resumed\":false,\"usage_v\":2}\n",
        "{\"job\":\"001\",\"role\":\"reviewer\",\"turns\":\"x\",\"usage_v\":2}\n",
    );
    let s = usage_summary(text, "001");
    assert_eq!((s.turns, s.resumed_runs, s.runs), (12, 1, 3));
}

#[test]
fn usage_summary_treats_missing_turns_as_zero() {
    let s = usage_summary("{\"job\":\"001\",\"role\":\"builder\",\"usage_v\":2}\n", "001");
    assert_eq!((s.turns, s.resumed_runs, s.runs), (0, 0, 1));
}
```

In `settings_block.rs` tests add `cost_markers_gets_a_hint_line_and_a_count_like_checks`: render with `&|_| false` for the `rust` preset, assert the block contains `cost_markers: add [[settings.cost_markers]]` and no line starting `# cost_markers =`; build `Config` from `Config::parse("[[cost_markers]]\nname = \"a\"\njob = \"001\"\n")`, call `effective_lines(&cfg, &toml::Table::new(), rust_preset_config)` and assert one line equals `cost_markers = 1 markers (default)`. Use the preset fixture the neighbouring tests use (`preset::embedded("rust")`).

- [ ] **Step 4: Write the failing engine tests** in `crates/factory-engine/tests/costs.rs`. First split the helper: `add_finished_job_with(fx, id, plan_text, usage, tweak: impl FnOnce(&mut Job)) -> Job` holds the current body and calls `tweak(&mut job)` right after `Job::new`; `add_finished_job` calls it with `|_| {}`. Import `factory_core::StageEntry`. Then:

```rust
#[test]
fn gather_costs_fills_turns_resumed_round_and_parked() {
    let fx = common::SeriesFixture::new("x");
    let usage = concat!(
        "{\"job\":\"110\",\"role\":\"builder\",\"turns\":7,\"resumed\":true,\"usage_v\":2}\n",
        "{\"job\":\"110\",\"role\":\"builder\",\"turns\":5,\"resumed\":false,\"usage_v\":2}\n",
    );
    add_finished_job_with(&fx, "110", "# P\n", Some(usage), |j| {
        j.round = 2;
        j.stage_history.push(StageEntry { stage: Stage::NeedsHuman, at: 3.0 });
    });
    let jobs = gather_costs(&fx.driver).expect("gather");
    let j = jobs.iter().find(|j| j.id == "110").expect("job 110");
    assert_eq!((j.turns, j.resumed_runs, j.round, j.parked), (12, 1, 2, true));
}
```

Also `a_merged_job_that_never_parked_has_parked_false` (same usage, plain `add_finished_job`, id `111`, assert `!j.parked`) and `the_spec_review_record_has_zero_round_and_is_not_parked` (write `fx.driver.layout().spec_review_usage_file()` with one line for `SPEC_REVIEW_JOB` as the existing spec-review cost test in `crates/factory-engine/tests/spec_review.rs` does; assert `round == 0`, `!parked`, `turns` equals the line's turns).

- [ ] **Step 5: Run the tests and confirm they fail.** `cargo test -j4 -p factory-core cost_markers`, `cargo test -j4 -p factory-core usage_summary` and `cargo test -j4 -p factory-engine --test costs`. Expected: compile errors such as `cannot find type 'CostMarker' in this scope` and `no field 'turns' on type 'UsageSummary'`.

- [ ] **Step 6: Implement.**
  - `config.rs`: add `CostMarker` and the field (with `cost_markers: Vec::new()` in `impl Default`). Add `fn validate_cost_markers(markers: &[CostMarker]) -> Result<(), CoreError>` called from `Config::parse`: name not blank, at most 40 chars, no control character (`char::is_control`); job non-empty and all ASCII digits; names unique. Error text is `cost marker "<name>": <reason>`, and `cost marker #<n>: name must not be empty` for a blank name (n starts at 1).
  - `cost.rs`: add the two `UsageSummary` fields (sum `v["turns"].as_u64()` clamped to `u32`, count `v["resumed"].as_bool() == Some(true)`), add the four `JobCost` fields and `#[derive(Default)]`, and add `turns: 0, resumed_runs: 0, round: 0, parked: false` to every existing `JobCost` literal (the `job()` helper in `cost.rs` tests and the `cost()` helper in `crates/factory-cli/src/costs.rs` tests).
  - `settings_block.rs`: add group `("Cost markers.", &[("cost_markers", None)])` after the plan size group. In `render`, handle `cost_markers` like `checks` with the hint line `# cost_markers: add [[settings.cost_markers]] tables with a name and the first job id that ran with a change. See docs/reference/project-toml-reference.md.`. In `effective_lines` show `{n} markers` for it. In the test helper `uncommented`, skip `cost_markers` as `checks` is skipped.
  - `engine/costs.rs`: fill `turns`, `resumed_runs` from `usage`, `round: job.round`, `parked: job.stage_history.iter().any(|e| e.stage == Stage::NeedsHuman)`. The spec-review record uses `usage.turns`, `usage.resumed_runs`, `round: 0`, `parked: false`.
  - `lib.rs`: export `CostMarker`.

- [ ] **Step 7: Run the tests and confirm they pass.** `cargo test -j4 -p factory-core` (all pass, including `every_config_key_is_listed_once`), `cargo test -j4 -p factory-engine --test costs` (all pass, the three new ones included) and `cargo test -j4 -p factory-cli`. The new `cost_markers` line changes the output of `factory project show` and the text that `factory project defaults` adds, so update every existing expectation in `factory-cli` tests that lists the settings (the `project show` tests changed in commit `e23a24f` are the model) and any golden settings text in `factory-core/tests`. Change only expectations about the new `cost_markers` line, nothing else.

- [ ] **Step 8: Format and lint.** Run `cargo fmt -p factory-core -p factory-engine` and then `cargo clippy -j4 -p factory-core -p factory-engine --all-targets -- -D warnings`. Expected: no change from fmt on a second run and no warning from clippy. Fix every finding in the code.

### Task 2: The trend model and `costs.json`

**Files:**
- Create: `crates/factory-core/src/trend.rs`
- Modify: `crates/factory-core/src/cost.rs`, `crates/factory-core/src/lib.rs`
- Test: `crates/factory-core/src/trend.rs` (tests module), `crates/factory-core/src/cost.rs` (tests module)

**Interfaces:**
- Consumes: `JobCost` (with the Task 1 fields and `Default`), `CostMarker`, `SizeGroup`, `Spread`, `median_of`, `spread`.
- Produces in `trend.rs` (all derive `Debug, Clone, PartialEq, Serialize, Deserialize`; all new `costs.json` fields are optional or additive):

```rust
pub const MIN_JOBS_PER_SIDE: u32 = 3;
pub const VERDICT_BAND_PCT: i64 = 10;
pub fn build_trend(jobs: &[JobCost], markers: &[CostMarker]) -> Option<TrendReport>;
pub struct TrendReport { pub markers: Vec<MarkerTrend>, pub warnings: Vec<String> }
pub struct MarkerTrend { pub names: Vec<String>, pub from: Vec<String>, pub cut: String, pub groups: BTreeMap<String, GroupTrend> }
pub struct GroupTrend { pub before: Side, pub after: Side, pub change_pct: Option<i64>, pub verdict: String }
pub struct Side {
    pub n: u32, pub n_approx: u32, pub per_line: Option<Spread>, pub median_turns: Option<u64>,
    pub resumed_pct: Option<u32>, pub rework_pct: Option<u32>, pub park_pct: Option<u32>,
}
```

- Produces in `cost.rs`: `CostsFile.trend: Option<TrendReport>`, `median_of` and `spread` become `pub(crate)`.
- Exports from `lib.rs`: `build_trend, TrendReport, MarkerTrend, GroupTrend, Side, MIN_JOBS_PER_SIDE, VERDICT_BAND_PCT`.

- [ ] **Step 1: Read the code this task touches.** Read `cost.rs` (`median_of`, `spread`, `Spread`, `CostsFile::build`, the old-file test `an_old_costs_file_without_providers_still_loads`) and the "Design" section 3 of the spec.

- [ ] **Step 2: Write the failing trend tests** in `trend.rs`. Helpers and one full example:

```rust
fn jc(id: &str, group: SizeGroup, weighted: u64, lines: u64) -> JobCost {
    JobCost {
        id: id.into(), stage: "merged".into(), group: Some(group), lines: Some(lines),
        weighted, runs: 2, turns: 10, resumed_runs: 1, ..JobCost::default()
    }
}
fn marker(name: &str, job: &str) -> CostMarker {
    CostMarker { name: name.into(), job: job.into() }
}
/// Jobs 001 to 006 in the small group, lines 1, so weighted is the ratio: three at `a`, three at `b`.
fn six(a: u64, b: u64) -> Vec<JobCost> {
    (1..=6).map(|i| jc(&format!("{i:03}"), SizeGroup::Small, if i <= 3 { a } else { b }, 1)).collect()
}

#[test]
fn before_is_everything_below_the_cut_and_after_stops_at_the_next_cut() {
    let jobs: Vec<JobCost> = (1..=9)
        .map(|i| {
            let w = match i { 1..=3 => 100_000, 4..=6 => 50_000, _ => 25_000 };
            jc(&format!("{i:03}"), SizeGroup::Small, w, 100)
        })
        .collect();
    let report = build_trend(&jobs, &[marker("m2", "007"), marker("m1", "004")]).expect("report");
    let cuts: Vec<&str> = report.markers.iter().map(|m| m.cut.as_str()).collect();
    assert_eq!(cuts, vec!["004", "007"]);
    let g = &report.markers[0].groups["small"];
    assert_eq!((g.before.n, g.after.n), (3, 3));
    assert_eq!(g.before.per_line.as_ref().expect("before").median, 1000);
    assert_eq!(g.after.per_line.as_ref().expect("after").median, 500);
    assert_eq!((g.change_pct, g.verdict.as_str()), (Some(-50), "fell"));
    let g = &report.markers[1].groups["small"];
    assert_eq!((g.before.n, g.after.n), (6, 3));
    assert_eq!(g.before.per_line.as_ref().expect("before").median, 750);
    assert_eq!(g.after.per_line.as_ref().expect("after").median, 250);
    assert_eq!(g.change_pct, Some(-67));
}
```

The other tests, each built with `six(..)` or small variations of it:

| Test | Setup | Assertions |
|---|---|---|
| `no_markers_gives_no_report` | `six(100, 50)`, no markers | `build_trend(..).is_none()` |
| `a_marker_on_a_missing_job_cuts_at_the_next_board_job` | jobs `001 002 003 005 006 007` (ratios 100 ×3, 50 ×3), marker job `"004"` | `cut == "005"`, `from == ["004"]` |
| `marker_ids_compare_as_numbers` | jobs `001 002 003 1000 1001 1002` (ratios 100 ×3, 50 ×3), marker job `"999"` | `cut == "1000"`, `before.n == 3`, `after.n == 3` |
| `markers_are_sorted_by_resolved_id` | `six(100, 50)` plus jobs `007 008 009` (ratio 25), markers given as `[marker("late", "007"), marker("early", "004")]` | `markers[0].names == ["early"]`, `markers[1].names == ["late"]` |
| `a_marker_past_the_latest_job_warns_and_is_skipped` | `six(100, 50)`, marker job `"099"` | `markers.is_empty()`, one warning containing `past the latest job` and `m` |
| `two_markers_on_one_job_share_a_block` | `six(100, 50)`, markers `a` and `b` both job `"004"` | one block, `names == ["a","b"]`, `from == ["004","004"]` |
| `non_numeric_job_ids_are_ignored_for_cuts` | `six(100, 50)` plus a job with id `"spec-review"` and stage `"spec-review"`, marker `"004"` | `cut == "004"`, no panic |
| `per_line_is_the_median_of_job_ratios` | before ratios 100 100 100, after four jobs with ratios 100 200 300 400 | after median 250, low 100, high 400 |
| `the_job_ratio_rounds_half_up` | all six jobs weighted 3, lines 2 | both medians 2 |
| `huge_weighted_values_do_not_overflow_the_ratio` | after jobs weighted `u64::MAX`, lines 1 | no panic, after median `u64::MAX` |
| `a_side_with_two_jobs_is_not_enough_data` | jobs `001 002` before, `003 004 005` after, marker `"003"` | verdict `not-enough-data`, `per_line` is `None` on both sides, `change_pct` is `None`, `before.n == 2` |
| `a_group_with_no_eligible_job_on_either_side_is_left_out` | only small jobs | `!groups.contains_key("medium")` |
| `a_group_with_jobs_on_one_side_only_is_not_enough_data` | `six(..)` small plus three medium jobs `004 005 006` | `groups["medium"].verdict == "not-enough-data"`, `before.n == 0`, `after.n == 3` |
| `jobs_without_lines_or_data_are_left_out` | `six(..)` plus a job with `lines: None`, one with `lines: Some(0)`, one with stage `needs-human`, one with `runs: 0`, all in the after span | `after.n == 3` |
| `a_job_without_a_size_group_is_left_out` | `six(..)` plus a job in the after span with `group: None` (no plan) | `after.n == 3` |
| `change_pct_and_verdict_use_the_ten_percent_band` | `six(100, x)` with x = 90, 91, 109, 110, marker `"004"` | `(change_pct, verdict)` = `(-10, fell)`, `(-9, flat)`, `(9, flat)`, `(10, rose)` |
| `an_after_median_of_zero_is_a_drop_of_one_hundred_percent` | `six(100, 0)` | `change_pct == Some(-100)`, verdict `fell` |
| `a_zero_before_median_is_not_enough_data` | `six(0, 50)` | verdict `not-enough-data`, `change_pct` is `None` |
| `turns_resumed_rework_and_park_shares` | before: 8 small jobs `001..008`, job `001` has `round: 2`; after: jobs `009 010 011` with job `009` `parked: true`; marker `"009"`; every job runs 2, resumed_runs 1, turns 10 | `before.rework_pct == Some(13)`, `after.park_pct == Some(33)`, `before.resumed_pct == Some(50)`, `before.median_turns == Some(10)` |
| `a_side_with_no_runs_has_a_zero_resumed_share` | call the private `pct(0, 0)` | equals `0` |
| `approximate_jobs_are_counted_per_side` | `six(100, 50)` with job `005` `approx: true` | `before.n_approx == 0`, `after.n_approx == 1` |

Add to the `cost.rs` tests:

```rust
#[test]
fn the_costs_file_carries_the_trend_and_omits_it_without_markers() {
    let jobs: Vec<JobCost> = Vec::new();
    let with = Config::parse("[[cost_markers]]\nname = \"a\"\njob = \"001\"\n").expect("parses");
    let file = CostsFile::build(&jobs, &with, 1.0);
    assert!(file.trend.is_some());
    let json = serde_json::to_string(&file).expect("json");
    assert!(json.contains("\"trend\""), "{json}");
    let without = CostsFile::build(&jobs, &Config::default(), 1.0);
    assert!(!serde_json::to_string(&without).expect("json").contains("trend"));
}
```

and extend the old-file test so `an_old_costs_file_without_trend_still_loads` asserts `file.trend.is_none()` for the same JSON string.

- [ ] **Step 3: Run the tests and confirm they fail.** `cargo test -j4 -p factory-core trend` and `cargo test -j4 -p factory-core costs_file`. Expected: `cannot find function 'build_trend' in this scope` and `no field 'trend' on type 'CostsFile'`.

- [ ] **Step 4: Implement `build_trend` in `trend.rs`** from the signatures above and the spec's rules 1 to 7. Notes that the tests pin: parse ids with `str::parse::<u64>()` and ignore ids that fail; the cut search runs over all `jobs` with a numeric id; the ratio is `((u128::from(weighted) + u128::from(lines) / 2) / u128::from(lines))` clamped to `u64::MAX`; shares use `fn pct(num: u64, den: u64) -> u32` returning 0 when `den == 0` and `(num * 100 + den / 2) / den` otherwise; `change_pct` is `((after - before) as f64 * 100.0 / before as f64).round() as i64`; the warning text is `marker <name> (job <id>) is past the latest job`. Return `Some(TrendReport { markers: vec![], warnings })` when markers exist but none resolve, and `None` when `markers` is empty.

- [ ] **Step 5: Implement the `CostsFile` change.** Add the `trend` field with the serde attributes from Global Constraints, set it in `CostsFile::build` with `build_trend(jobs, &cfg.cost_markers)`, make `median_of` and `spread` `pub(crate)`, export the new names in `lib.rs`.

- [ ] **Step 6: Run the tests and confirm they pass.** `cargo test -j4 -p factory-core` (all pass) and `cargo check -j4 -p factory-cli`.

- [ ] **Step 7: Format and lint.** Run `cargo fmt -p factory-core` and then `cargo clippy -j4 -p factory-core --all-targets -- -D warnings`. Expected: no change from fmt on a second run and no warning from clippy.

### Task 3: `factory costs --trend`

**Files:**
- Modify: `crates/factory-cli/src/cli.rs`, `crates/factory-cli/src/commands.rs`, `crates/factory-cli/src/costs.rs`
- Test: `crates/factory-cli/src/costs.rs` (tests module), `crates/factory-cli/tests/e2e.rs`

**Interfaces:**
- Consumes: `factory_core::{TrendReport, MarkerTrend, GroupTrend, Side, build_trend, CostMarker, JobCost, SizeGroup}`, `CostsFile.trend`, `crate::status::human` is not needed (the figures are plain integers).
- Produces: `Command::Costs { trend: bool }` (flag `--trend`, doc comment `Print the cost trend per marker instead of the job tables.`) and `pub fn render_trend(report: Option<&TrendReport>) -> String` in `costs.rs`.
- No board, ledger or events field is added, and no engine code changes.

- [ ] **Step 1: Read the code this task touches.** Read `crates/factory-cli/src/cli.rs` around line 117, `commands.rs` around line 413 and 588, `costs.rs` (the `cost()` test helper), and the test `factory_costs_lists_jobs_writes_costs_json_and_new_prints_estimates` in `tests/e2e.rs` with `Scenario::new` in `tests/support/mod.rs` (its first argument is appended to `config.toml`).

- [ ] **Step 2: Write the failing render tests** in `costs.rs`. One full example, built with `build_trend` on real records:

```rust
fn tjob(id: &str, group: SizeGroup, weighted: u64) -> JobCost {
    JobCost {
        id: id.into(), stage: "merged".into(), group: Some(group), lines: Some(100),
        weighted, runs: 2, turns: 10, resumed_runs: 1, ..JobCost::default()
    }
}

#[test]
fn render_trend_prints_a_block_per_marker() {
    let jobs: Vec<JobCost> = (1..=6)
        .map(|i| tjob(&format!("{i:03}"), SizeGroup::Small, if i <= 3 { 122_300 } else { 76_000 }))
        .collect();
    let markers = [CostMarker { name: "carry-cap-45k".into(), job: "004".into() }];
    let report = build_trend(&jobs, &markers).expect("report");
    let text = render_trend(Some(&report));
    assert!(text.contains("MARKER carry-cap-45k (job 004, cut at 004)"), "{text}");
    let row = text.lines().find(|l| l.starts_with("small")).expect("small row");
    assert!(row.contains("3/3") && row.contains("1223 -> 760"), "{row}");
    assert!(row.contains("-38%") && row.contains("fell"), "{row}");
}
```

| Test | Setup | Expectation |
|---|---|---|
| `render_trend_labels_a_group_without_enough_data` | two medium jobs before, three after | a line `medium   2/3    not enough data (2 before, 3 after)` style: contains `not enough data (2 before, 3 after)` and no `->` on that line |
| `render_trend_without_markers_prints_the_hint` | `render_trend(None)` | equals `no cost_markers in project.toml. Add one with [[settings.cost_markers]], name = "...", job = "NNN".\n` |
| `render_trend_prints_warnings_and_approx_notes` | a report with one warning and an approximate job on the after side | text has the warning on its own line and `1 approx after` in the row |
| `render_trend_joins_the_names_of_collapsed_markers` | markers `a`, `b` on the same job | text contains `MARKER a, b (job 004, 004, cut at 004)` |
| `render_trend_says_so_when_a_block_has_no_groups` | markers on jobs with no eligible job at all | block heading followed by `  no jobs with data around this cut` |

Row format to implement: `format!("{:<8} {:<6} {:<19} {:<7} {:<8} {:<10} {:<11} {:<11} {:<11} {}", group, jobs, tok_line, change, verdict, turns, resumed, rework, parked, notes)` under a header line `GROUP    JOBS   TOK/LINE            CHANGE  VERDICT  TURNS      RESUMED     REWORK      PARKED      NOTES`. A figure pair prints `before -> after`. A heading is `MARKER <names joined by ", "> (job <from joined by ", ">, cut at <cut>)`. A not-enough-data row is `format!("{:<8} {:<6} not enough data ({} before, {} after)", ..)`.

- [ ] **Step 3: Write the failing e2e tests** in `tests/e2e.rs`, using the scenario pattern of the costs test above:

```rust
#[test]
fn factory_costs_trend_prints_the_report_and_writes_trend_to_costs_json() -> TestResult {
    let sc = Scenario::new(
        "[[cost_markers]]\nname = \"m\"\njob = \"001\"\n",
        vec![build(vec![("a.txt", "a\n")], 1_000), review(APPROVE)],
    )?;
    let plan = sc.plan_a.to_string_lossy().to_string();
    sc.run(&["new", "hello", "say hello", "--plans", &plan])?;
    assert_eq!(sc.run(&["run", "1"])?, "merged");
    let out = sc.run(&["costs", "--trend"])?;
    assert!(out.contains("MARKER m (job 001, cut at 001)"), "{out}");
    assert!(out.contains("not enough data (0 before, 1 after)"), "{out}");
    let json = std::fs::read_to_string(sc.root.join(".superpowers/factory/costs.json"))?;
    assert!(json.contains("\"trend\""), "{json}");
    Ok(())
}
```

`factory_costs_without_the_flag_is_unchanged_with_markers_set`: same scenario, `costs` output contains `ROLES` and does not contain `MARKER`. `trend_still_prints_the_costs_json_write_warning`: after the run, replace `.superpowers/factory/costs.json` with a directory of the same name, run `costs --trend`, assert the output contains `warning: could not write costs.json` and still contains `MARKER`.

- [ ] **Step 4: Run the tests and confirm they fail.** `cargo test -j4 -p factory-cli render_trend` and `cargo test -j4 -p factory-cli --test e2e factory_costs`. Expected: `cannot find function 'render_trend' in this scope` and `unexpected argument '--trend'`.

- [ ] **Step 5: Implement.** `cli.rs`: `Costs { #[arg(long)] trend: bool }`. `commands.rs`: `Command::Costs { trend } => costs(ctx, trend)`; in `costs`, build the file as today, then `let mut out = if trend { render_trend(file.trend.as_ref()) } else { render_costs(&jobs, &file.groups) };` and keep the write and the warning unchanged. `costs.rs`: `render_trend` with the formats above, a warning per line after the blocks, and the notes text `N approx before` and `N approx after` joined by `, `.

- [ ] **Step 6: Run the tests and confirm they pass.** `cargo test -j4 -p factory-cli` (all pass, including the existing `factory_costs_lists_jobs_writes_costs_json_and_new_prints_estimates`), `cargo check -j4`.

- [ ] **Step 7: Format and lint.** Run `cargo fmt -p factory-cli` and then `cargo clippy -j4 -p factory-cli --all-targets -- -D warnings`. Expected: no change from fmt on a second run and no warning from clippy.

Next plan: docs/superpowers/plans/2026-10-31-cost-trend-report/plan-02-cost-trend-report.md
