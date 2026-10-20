# Cold-start savings, plan 2: the COLD STARTS report

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** `factory tokens --cache` prints a COLD STARTS section per role and per job, built from the first-turn fields that plan 1 logs.

**Architecture:** `UsageRecord` reads the two optional keys. `cache_report` fills two new lists while it walks the rows it already counts, and `render_cache` appends the section, also when there are no resumed runs.

**Tech Stack:** Rust, clap (unchanged), serde_json.

**Spec:** docs/superpowers/specs/2026-10-20-cold-start-savings-design.md

## Global Constraints

- Always `-j4`. Never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- No `unwrap` in production code. Tests use `.expect("...")` and `expect_err`.
- `start_cc` and `start_cr` are optional on every line. A line without them is counted as a run but never in the first-turn columns.
- The report only reads `usage.jsonl` files. It needs no network and changes no file.
- `--days N` limits the new lists exactly as it limits the existing ones: by the row's `ts`.

## Review Focus

1. Old `usage.jsonl` lines only (no start fields): the section says `no runs with start fields yet` and the rest of the report is unchanged (test `cold_section_says_when_no_start_fields_exist`, Task 3).
2. A job with no resumed runs: the section still prints (test `cold_section_prints_without_resumed_runs`, Task 3).
3. A role whose fresh runs have a creation total of 0: FIRST-CC% prints `-`, not `NaN` (test `cold_share_is_a_dash_without_creation`, Task 2).
4. A non-numeric `start_cc` value: treated as absent, no error (test `a_non_numeric_start_field_is_absent`, Task 1).
5. A resumed row that carries start fields: never added to FIRST-CC (test `cold_report_counts_fresh_runs_only`, Task 2).

## File Structure

- `crates/factory-cli/src/tokens.rs` (modify): `UsageRecord` fields, `load_usage`, `RoleCold`, `JobCold`, `CacheReport` fields, `cache_report`, `render_cache`, `render_cold`, unit tests.
- `crates/factory-cli/tests/tokens_cache.rs` (modify): one CLI test.
- `README.md` (modify): the "Cache report" paragraph.

### Task 1: UsageRecord reads the start fields

**Files:**
- Modify: `crates/factory-cli/src/tokens.rs`
- Test: `crates/factory-cli/src/tokens.rs` (unit tests)

**Interfaces:**
- Consumes: `UsageRecord`, `load_usage(log: &Path, job: &str) -> Result<Vec<UsageRecord>, std::io::Error>`.
- Produces: `UsageRecord.start_cc: Option<u64>`, `UsageRecord.start_cr: Option<u64>`. Both are new optional fields, `None` on old lines and on non-numeric values.

- [ ] **Step 1: Read** `UsageRecord`, `load_usage` and the existing load test (the one that writes a jsonl file and asserts `records[1].start_ctx == 77`) in `crates/factory-cli/src/tokens.rs`, and the `rec` helper of the unit tests. If `rec` builds a full struct literal, it needs `..Default::default()`.
- [ ] **Step 2: Write the failing tests** next to the existing load test, built the same way (a temp file, one JSON object per line, then `load_usage(&path, "001")`).

```rust
#[test]
fn load_usage_reads_the_start_fields() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("usage.jsonl");
    std::fs::write(
        &path,
        "{\"job\":\"001\",\"role\":\"builder\",\"start_cc\":300,\"start_cr\":50,\"usage_v\":3}\n",
    )
    .expect("write usage");
    let records = load_usage(&path, "001").expect("load");
    assert_eq!(records[0].start_cc, Some(300));
    assert_eq!(records[0].start_cr, Some(50));
}
```

  The other tests:

| Test | Line | Assertion |
|---|---|---|
| `old_lines_have_no_start_fields` | `{"job":"001","role":"builder","usage_v":2}` | both fields `None` |
| `a_non_numeric_start_field_is_absent` | `"start_cc":"many","start_cr":null` | both `None`, no error |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-cli --lib start_fields` and expect a compile failure: `no field 'start_cc' on type 'UsageRecord'`.
- [ ] **Step 4: Implement** the two `Option<u64>` fields with a doc comment (first turn cache creation and read, `None` on old lines) and fill them in `load_usage` with `v["start_cc"].as_u64()` and `v["start_cr"].as_u64()`.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-cli --lib tokens` and expect PASS.

### Task 2: cold_report data in CacheReport

**Files:**
- Modify: `crates/factory-cli/src/tokens.rs`
- Test: `crates/factory-cli/src/tokens.rs` (unit tests)

**Interfaces:**
- Consumes: `UsageRecord.start_cc`, `UsageRecord.start_cr` (Task 1), `cache_report(jobs: &[Vec<UsageRecord>], days: Option<u32>, now: u64) -> CacheReport`.
- Produces:
  - `pub struct RoleCold { pub role: String, pub fresh: u64, pub resumed: u64, pub covered: u64, pub start_ctx: u64, pub first_cc: u64, pub first_cr: u64, pub creation: u64 }` with `#[derive(Debug, Clone, Default, PartialEq)]`. `fresh` and `resumed` count runs of the role. `covered` counts fresh runs that have start fields. `start_ctx`, `first_cc`, `first_cr` and `creation` (the rows' `cache_creation`) sum over covered rows only.
  - `impl RoleCold { pub fn avg_start(&self) -> Option<u64>; pub fn first_cc_share(&self) -> Option<f64> }`. `avg_start` is `start_ctx / covered`, `None` when `covered == 0`. `first_cc_share` is `first_cc / creation`, `None` when `creation == 0`.
  - `pub struct JobCold { pub job: String, pub cold: u64, pub resumed: u64, pub first_cc: u64 }`. `cold` counts all fresh runs of the job, `first_cc` sums covered fresh rows.
  - `CacheReport` gets `pub cold_roles: Vec<RoleCold>` (sorted by role), `pub cold_jobs: Vec<JobCold>` (sorted by job id), `pub fresh_runs: u64` and `pub fresh_covered: u64`.
  - The logic is a pure function of the loaded rows. It reads no file and sits behind no port.

- [ ] **Step 1: Read** `cache_report`, `CacheReport`, `RoleCache` and the existing `cache_report` unit tests in `tokens.rs` to see how rows, `days` and `now` are built.
- [ ] **Step 2: Write the failing tests** with one helper that builds a row. One full example:

```rust
fn cold_rec(job: &str, role: &str, resumed: bool, ts: u64, creation: u64, start: Option<(u64, u64)>) -> UsageRecord {
    UsageRecord {
        job: job.into(),
        role: role.into(),
        resumed,
        ts,
        cache_creation: creation,
        start_ctx: start.map_or(0, |(c, r)| c + r),
        start_cc: start.map(|(c, _)| c),
        start_cr: start.map(|(_, r)| r),
        ..Default::default()
    }
}

#[test]
fn cold_report_counts_fresh_runs_only() {
    let rows = vec![
        cold_rec("001", "builder", false, 10, 1000, Some((400, 100))),
        cold_rec("001", "builder", false, 20, 3000, Some((600, 0))),
        cold_rec("001", "builder", true, 30, 5000, Some((9000, 9000))),
    ];
    let r = cache_report(&[rows], None, 100);
    let b = r.cold_roles.iter().find(|c| c.role == "builder").expect("builder");
    assert_eq!((b.fresh, b.resumed, b.covered), (2, 1, 2));
    assert_eq!((b.first_cc, b.first_cr, b.creation), (1000, 100, 4000));
    assert_eq!(b.avg_start(), Some(550));
    assert_eq!(b.first_cc_share(), Some(0.25));
}
```

  The other tests:

| Test | Input | Assertion |
|---|---|---|
| `cold_report_counts_cold_and_resumed_per_job` | job "002": two fresh builder rows (one with start fields `(200, 0)`), one resumed; job "001": one fresh committer row | `cold_jobs` holds "001" then "002"; "002" has `cold == 2`, `resumed == 1`, `first_cc == 200` |
| `cold_report_respects_days` | rows at `ts` 10 and `ts` 90_000, `days = Some(1)`, `now = 100_000` | only the row at 90_000 is counted in `fresh_runs` and in the role row |
| `a_fresh_run_without_start_fields_counts_but_is_not_covered` | one fresh row with `None` start | `fresh == 1`, `covered == 0`, `avg_start() == None`, `fresh_runs == 1`, `fresh_covered == 0` |
| `cold_share_is_a_dash_without_creation` | covered fresh row with `creation == 0` | `first_cc_share() == None` |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-cli --lib cold` and expect a compile failure: `no field 'cold_roles' on type 'CacheReport'`.
- [ ] **Step 4: Implement** the types and fill the lists inside the existing `cache_report` loop, after the `cutoff` check, so the rows counted are the same as for the existing tables. Use a `BTreeMap` per role and per job so the output is sorted. `rewrote`, gap and the existing fields are unchanged.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-cli --lib tokens` and expect PASS, including every existing `cache_report` test unchanged.

### Task 3: render the section, CLI test and README

**Files:**
- Modify: `crates/factory-cli/src/tokens.rs`, `README.md`
- Test: `crates/factory-cli/src/tokens.rs` (unit tests), `crates/factory-cli/tests/tokens_cache.rs`

**Interfaces:**
- Consumes: `CacheReport.cold_roles`, `cold_jobs`, `fresh_runs`, `fresh_covered` (Task 2), `render_cache(r: &CacheReport) -> String`.
- Produces: `fn render_cold(r: &CacheReport) -> String`, called at the end of `render_cache` for every report with `runs > 0`, so the section also prints when there are no resumed runs. Output:

```
COLD STARTS
ROLE       FRESH  RESUMED  AVG-START  FIRST-CC  FIRST-CR  FIRST-CC%
builder    2      1        550        1000      100       25.0%
start fields cover 2 of 3 fresh runs

JOB   COLD  RESUMED  FIRST-CC
001   2     1        1000
```

  Columns are left aligned as in the existing tables. `-` replaces a missing average or share. The coverage line appears only when `fresh_covered < fresh_runs`. When `fresh_covered == 0` the section prints `COLD STARTS` then `no runs with start fields yet` and no job table.

- [ ] **Step 1: Read** `render_cache` in `tokens.rs`, the early returns (`no runs`, `no resumed runs`), the existing `render_cache` tests, `crates/factory-cli/tests/tokens_cache.rs` (the `USAGE` constant, `new_job`, `fx.run(&["tokens", "--cache"])`) and the "Cache report" paragraph in `README.md`.
- [ ] **Step 2: Write the failing tests.** Unit tests build a `CacheReport` through `cache_report` with the Task 2 helper `cold_rec`. One full example:

```rust
#[test]
fn cold_section_prints_without_resumed_runs() {
    let rows = vec![cold_rec("001", "builder", false, 10, 1000, Some((250, 0)))];
    let out = render_cache(&cache_report(&[rows], None, 100));
    assert!(out.contains("COLD STARTS"), "{out}");
    assert!(out.contains("no resumed runs"), "{out}");
    assert!(out.contains("25.0%"), "{out}");
}
```

  The other tests:

| Test | Input | Assertion |
|---|---|---|
| `cold_section_says_when_no_start_fields_exist` | one fresh row with `None` start | output contains `no runs with start fields yet` and no `FIRST-CC%` header |
| `cold_section_notes_runs_without_start_fields` | one covered and one uncovered fresh row | output contains `start fields cover 1 of 2 fresh runs` |
| `cold_section_has_no_coverage_note_when_all_covered` | two covered fresh rows | output has no `start fields cover` |
| `cold_section_lists_jobs_in_id_order` | rows for jobs "002" then "001" | `001` appears before `002` in the job table |

  In `crates/factory-cli/tests/tokens_cache.rs` add `tokens_cache_prints_the_cold_starts_section`: create a job with `new_job`, write a `usage.jsonl` into its folder with one fresh builder line that has `"start_cc":400,"start_cr":100,"usage_v":3` (copy the shape of `USAGE`), run `fx.run(&["tokens", "--cache"])` and assert the output contains `COLD STARTS` and `builder`. The existing test `cache_report_reads_every_job_of_the_project` must still pass unchanged.
- [ ] **Step 3: Run** `cargo test -j4 -p factory-cli --lib cold_section` and expect FAIL: the output has no `COLD STARTS`.
- [ ] **Step 4: Implement** `render_cold` and change `render_cache` so the `no resumed runs` branch no longer returns early: print that line, then the cold section. The `no runs` early return stays. Update the README "Cache report" paragraph: name the COLD STARTS section, its columns, that `start_cc` and `start_cr` come from runs logged since usage_v 3 and older runs show only in the coverage note, and that `--days N` applies to it.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-cli --lib tokens`, `cargo test -j4 -p factory-cli --test tokens_cache` and expect PASS. Then `cargo clippy -j4 -p factory-cli --all-targets -- -D warnings` and `cargo fmt -p factory-cli -- --check`, both clean.

Next plan: docs/superpowers/plans/2026-10-20-cold-start-savings/plan-03-cold-start-savings.md
