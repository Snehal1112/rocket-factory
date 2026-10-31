# Cost trend report, plan 2: dashboard and docs

> **Spec:** docs/superpowers/specs/2026-10-31-cost-trend-report-design.md
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** The dashboard shows the `trend` block of `costs.json` as a table and an SVG chart per marker, and the docs describe markers, `factory costs --trend` and the release note step.

**Architecture:** `dashboard/src/costs.rs` gets typed copies of the trend types, all optional. `board.js` gets `renderTrendPanel`, which `renderCostPanel` appends. The docs follow the code of plan 1.

**Tech Stack:** Rust (dashboard workspace), plain JavaScript with inline SVG, `bun test` for the JS tests.

## Global Constraints

- The dashboard is its own workspace: `cargo test -j4 --manifest-path dashboard/Cargo.toml`. Always `-j4`.
- `trend` is optional everywhere. A `costs.json` without it must render exactly as before.
- No chart library, no inline `style` attributes. Bar widths are SVG attributes. All text from data goes through `escapeHtml`.
- Verdict strings are exactly `fell`, `rose`, `flat`, `not-enough-data`.
- Rust in the dashboard follows the CI gate: `cargo fmt --manifest-path dashboard/Cargo.toml` leaves nothing to change and `cargo clippy -j4 --manifest-path dashboard/Cargo.toml --all-targets -- -D warnings` prints no warning. No `#[allow(...)]` to silence a lint. Tests use `.expect("...")`, never the unwrap call.
- Code comments are short full sentences that end with a period.

## Review Focus

1. A marker name with markup (`<b>x</b>`) must be escaped in the heading, the table and the SVG labels. Pinned by `renderTrendPanel escapes marker names` (Task 2).
2. A `trend` with `markers: []` and only warnings must not draw an empty table. Pinned by `renderTrendPanel shows only the warnings when there are no markers` (Task 2).
3. Bars must scale to the larger of the two medians, so a median of 0 on both sides draws no bar and no division by zero. Pinned by `renderTrendPanel draws no bars when both medians are zero` (Task 2).
4. A block with no groups says so in words. Pinned by `renderTrendPanel says so when a marker has no groups` (Task 2).
5. A `trend` of the wrong shape (a string, or markers as an object) must not throw. Pinned by `renderTrendPanel survives a malformed trend` (Task 2).

## File Structure

| File | Responsibility |
|---|---|
| `dashboard/src/costs.rs` | Typed `TrendView` and friends, `CostsView.trend` |
| `dashboard/static/board.js` | `renderTrendPanel`, called from `renderCostPanel` |
| `dashboard/static/dashboard.css` | Styles for the trend block and bars |
| `dashboard/tests/board.test.js` | JS tests |
| `docs/reference/project-toml-reference.md`, `docs/reference/software-factory.md`, `README.md`, `dashboard/README.md`, `docs/operations/releasing.md` | Docs |

### Task 1: Dashboard view types

**Files:**
- Modify: `dashboard/src/costs.rs`
- Test: `dashboard/src/costs.rs` (tests module)

**Interfaces:**
- Produces (all `#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]` with `#[serde(default)]`): `TrendView { markers: Vec<MarkerView>, warnings: Vec<String> }`, `MarkerView { names: Vec<String>, from: Vec<String>, cut: String, groups: BTreeMap<String, GroupTrendView> }`, `GroupTrendView { before: SideView, after: SideView, change_pct: Option<i64>, verdict: String }`, `SideView { n: u32, n_approx: u32, per_line: Option<SpreadView>, median_turns: Option<u64>, resumed_pct: Option<u32>, rework_pct: Option<u32>, park_pct: Option<u32> }`, and `CostsView.trend: Option<TrendView>`. The field names match `costs.json` written by plan 1. `trend` is optional and omitted from the output when `None`.
- No board, ledger or events field is added.

- [ ] **Step 1: Read the code this task touches.** Read `dashboard/src/costs.rs` (the existing views and their tests) and `dashboard/src/state.rs` around lines 65 and 339 and its test near line 967.

- [ ] **Step 2: Write the failing tests** in the `costs.rs` tests module, using the `write(root, text)` helper:

```rust
#[test]
fn trend_loads_from_costs_json() {
    let root = tempfile::tempdir().expect("tempdir");
    write(
        root.path(),
        r#"{"groups":{},"roles":{},"trend":{"warnings":["w"],"markers":[{"names":["m"],"from":["004"],"cut":"005",
            "groups":{"small":{"verdict":"fell","change_pct":-38,
              "before":{"n":3,"n_approx":0,"per_line":{"n":3,"median":1223,"low":1000,"high":1300},"median_turns":21,"resumed_pct":40,"rework_pct":17,"park_pct":33},
              "after":{"n":3,"n_approx":1,"per_line":{"n":3,"median":760,"low":700,"high":800},"median_turns":14,"resumed_pct":55,"rework_pct":0,"park_pct":20}}}}]}}"#,
    );
    let v = load_costs(&Paths::for_root(root.path())).expect("costs");
    let t = v.trend.expect("trend");
    assert_eq!(t.warnings, vec!["w".to_string()]);
    let g = &t.markers[0].groups["small"];
    assert_eq!((g.change_pct, g.verdict.as_str()), (Some(-38), "fell"));
    assert_eq!(g.after.per_line.as_ref().map(|s| s.median), Some(760));
    assert_eq!(g.after.n_approx, 1);
}

#[test]
fn a_costs_file_without_trend_has_none() {
    let root = tempfile::tempdir().expect("tempdir");
    write(root.path(), r#"{"groups":{},"roles":{}}"#);
    let v = load_costs(&Paths::for_root(root.path())).expect("costs");
    assert!(v.trend.is_none());
    assert!(!serde_json::to_string(&v).expect("json").contains("trend"));
}
```

- [ ] **Step 3: Run the tests and confirm they fail.** `cargo test -j4 --manifest-path dashboard/Cargo.toml trend`. Expected: `no field 'trend' on type 'CostsView'`.

- [ ] **Step 4: Implement the types** listed in Interfaces. `SpreadView` already exists, reuse it for `per_line`. Add `#[serde(skip_serializing_if = "Option::is_none")]` on `CostsView.trend`.

- [ ] **Step 5: Run the tests and confirm they pass.** `cargo test -j4 --manifest-path dashboard/Cargo.toml` (all pass, including the existing costs tests and the state tests).

- [ ] **Step 6: Format and lint.** Run `cargo fmt --manifest-path dashboard/Cargo.toml` and then `cargo clippy -j4 --manifest-path dashboard/Cargo.toml --all-targets -- -D warnings`. Expected: no change from fmt on a second run and no warning from clippy.

### Task 2: The trend panel in `board.js`

**Files:**
- Modify: `dashboard/static/board.js`, `dashboard/static/dashboard.css`
- Test: `dashboard/tests/board.test.js`

**Interfaces:**
- Consumes: `costs.trend` as in Task 1, and the helpers `escapeHtml`, `formatTokens` and the `costs` fixture in `board.test.js`.
- Produces: `renderTrendPanel(trend) -> string` exported on the module `api` object (empty string when `trend` is missing, malformed or has no markers and no warnings). `renderCostPanel` appends its result after the role bar.

- [ ] **Step 1: Read the code this task touches.** Read `renderCostPanel`, `costRow` and `roleBar` in `board.js` (lines 490 to 570), the exports block at the end, the cost tests in `board.test.js` from line 802, and the cost styles in `dashboard.css` from line 311.

- [ ] **Step 2: Write the failing JS tests** at the end of the cost tests in `board.test.js`. Fixture and one full example:

```js
const trend = {
  warnings: ["marker late (job 099) is past the latest job"],
  markers: [
    {
      names: ["carry-cap-45k"], from: ["004"], cut: "005",
      groups: {
        small: {
          verdict: "fell", change_pct: -38,
          before: { n: 6, n_approx: 1, per_line: { n: 6, median: 1223, low: 900, high: 1500 }, median_turns: 21, resumed_pct: 40, rework_pct: 17, park_pct: 33 },
          after: { n: 5, n_approx: 0, per_line: { n: 5, median: 760, low: 700, high: 800 }, median_turns: 14, resumed_pct: 55, rework_pct: 0, park_pct: 20 },
        },
        medium: {
          verdict: "not-enough-data", change_pct: null,
          before: { n: 2, n_approx: 0, per_line: null, median_turns: null, resumed_pct: null, rework_pct: null, park_pct: null },
          after: { n: 4, n_approx: 0, per_line: null, median_turns: null, resumed_pct: null, rework_pct: null, park_pct: null },
        },
      },
    },
  ],
};

test("renderTrendPanel draws a block, a table and bars per marker", () => {
  const html = board.renderTrendPanel(trend);
  assert.match(html, /carry-cap-45k/);
  assert.match(html, /cut at 005/);
  assert.match(html, /<th scope="row">small<\/th>/);
  assert.match(html, /1223 tokens per line/);
  assert.match(html, /760 tokens per line/);
  assert.match(html, /-38%/);
  assert.match(html, /fell/);
  assert.equal((html.match(/<rect /g) || []).length, 2, "two bars for the one group with data");
  assert.match(html, /past the latest job/);
});
```

| Test | Input | Assertions |
|---|---|---|
| `renderTrendPanel shows not enough data and draws no bars for that group` | the `medium` group alone as the only group | html matches `not enough data`, no `<rect ` |
| `renderTrendPanel escapes marker names` | `names: ["<b>x</b>"]` | html has `&lt;b&gt;x&lt;/b&gt;`, no `<b>x</b>` |
| `renderTrendPanel gives the chart a title and each bar an aria-label` | the full fixture | html matches `<title>` inside an `<svg`, and `aria-label="small before: 1223 tokens per line"` and `aria-label="small after: 760 tokens per line"` |
| `renderTrendPanel shows only the warnings when there are no markers` | `{ markers: [], warnings: ["w"] }` | html has `w`, no `<table` |
| `renderTrendPanel draws no bars when both medians are zero` | a group with both medians 0 and verdict `flat` | no `<rect `, no `NaN` |
| `renderTrendPanel says so when a marker has no groups` | one marker with `groups: {}` | html matches `No jobs with data around this cut` |
| `renderTrendPanel survives a malformed trend` | `"x"`, `{ markers: {} }`, `null`, `undefined` | each returns a string without throwing |
| `renderCostPanel without trend is unchanged` | the existing `costs` fixture | html contains no `cost-trend` |
| `renderCostPanel appends the trend after the role bar` | `Object.assign({}, costs, { trend })` | the index of `cost-trend` is greater than the index of `role-bar` |

- [ ] **Step 3: Run the tests and confirm they fail.** `bun test dashboard/tests/board.test.js`. Expected: `board.renderTrendPanel is not a function`.

- [ ] **Step 4: Implement `renderTrendPanel(trend)` in `board.js`.** Layout: a wrapper `<div class="cost-trend">` with `<h3 class="cost-sub">Cost trend</h3>`; per marker an `<h4>` reading `<names joined by ", "> (cut at <cut>)`, a `cost-scroll` table (`cost-table`) with columns size, before, after, change, verdict, turns, resumed, rework, parked, and a row per group in the order small, medium, complex. A group with verdict `not-enough-data` has one cell spanning the figure columns that reads `not enough data (<before.n> before, <after.n> after)`. Figures cells show `before -> after` for turns and the three shares, and `<median> tokens per line` text for the two medians. The chart is one `<svg class="trend-chart" role="img" viewBox="0 0 320 H">` with a `<title>` (`Tokens per changed line before and after <names>`), and per group with figures a label and two `<rect>` bars (class `trend-bar trend-before` and `trend-bar trend-after`) whose `width` attribute is `Math.round(median / max * 200)` with `max` the larger of the two medians in that marker (no bars when `max` is 0), each with `aria-label="<group> before: <n> tokens per line"` and the verdict as `<text>`. Warnings print as `<p class="cost-note">` lines. Guard every access with `Array.isArray` and `typeof` checks so a malformed `trend` returns `""`. Call it from `renderCostPanel` after `roleBar`, and export it in the `api` object.

- [ ] **Step 5: Add the styles** to `dashboard.css` next to the cost styles: `.cost-trend`, `.trend-chart { max-width: 100%; height: auto; }`, `.trend-bar { rx: 2; }`, `.trend-before { fill: var(--muted-foreground); }`, `.trend-after { fill: var(--primary, currentColor); }`, and `.trend-verdict-fell`, `.trend-verdict-rose` text classes using existing colour variables of the file.

- [ ] **Step 6: Run the tests and confirm they pass.** `bun test dashboard/tests/*.test.js` (all pass, the nine new tests included).

- [ ] **Step 7: Run the project's own runner and check the style.** Run `node --test dashboard/tests/*.test.js` (the runner the dashboard checks use). Expected: all pass. New JavaScript follows the style of `board.js`: two spaces, double quotes, semicolons, `"use strict"` kept at the top of the file.

### Task 3: Docs

**Files:**
- Modify: `docs/reference/project-toml-reference.md`, `docs/reference/software-factory.md`, `README.md`, `dashboard/README.md`, `docs/operations/releasing.md`

**Interfaces:**
- Consumes: the behaviour built in plan 1 and Task 2. Produces no code. It adds no board, ledger or events field.

- [ ] **Step 1: Read the docs this task touches.** Read the "Plan size groups" section of `docs/reference/project-toml-reference.md` (near line 170), line 400 of `docs/reference/software-factory.md`, the cost mentions in `README.md` and `dashboard/README.md`, and the "Cutting a release" section of `docs/operations/releasing.md`.

- [ ] **Step 2: Write the docs.**
  - `project-toml-reference.md`: after "Plan size groups" add a section `### Cost markers` with a table (`name`: string, at most 40 characters, one line, unique; `job`: string of digits, the first job id that ran with the change), the rule that a missing id cuts at the first board job at or above it, that a marker past the latest job is skipped with a warning, that tables are written `[[settings.cost_markers]]`, and this example:

    ```toml
    [[settings.cost_markers]]
    name = "carry-cap-45k"
    job = "022"
    ```

  - `software-factory.md`: after the cost report paragraph add a paragraph on `factory costs --trend`: before is every eligible job below the cut, after runs to the next cut, a side needs 3 jobs per size group, the figures (median tokens per changed line, median turns, resumed share, rework `round >= 2`, park from `stage_history`), the 10% verdict band, and the optional `trend` field in `costs.json`.
  - `README.md` and `dashboard/README.md`: one short paragraph each: the command and the settings key in `README.md`, the trend block under the cost panel in `dashboard/README.md`.
  - `releasing.md`: in "Cutting a release", after the probe paragraph, add: `Run \`factory costs --trend\` and paste the table into the release notes, so each release says whether the cost per changed line fell after the markers.`

- [ ] **Step 3: Verify.** `grep -c "cost_markers" docs/reference/project-toml-reference.md` prints a number above 0, `grep -n "factory costs --trend" docs/operations/releasing.md` finds the new line, and `cargo test -j4 -p factory-core settings_block` passes unchanged.

Next plan: none
