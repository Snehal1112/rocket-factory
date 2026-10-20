# Merged column sort, Plan 1: Time-ordered dashboard columns

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Every dashboard column lists its spec and job cards in one list, newest first by last stage change, Merged shows the 10 newest cards of any kind, and every card header shows `YYYY-MM-DD HH:MM` on the right.

**Architecture:** `build_spec_cards` in `dashboard/src/specs.rs` adds `stage_since` (the latest of the series' plan jobs) to each `SpecCard`. In `dashboard/static/board.js` a new `formatStamp` prints the header time, and `renderBoard` builds a sorted entry list per column instead of the fixed drafts, specs, jobs order.

**Tech Stack:** Rust (the dashboard is its own workspace), vanilla JS, `node --test`.

**Spec:** docs/superpowers/specs/2026-10-20-merged-column-sort-design.md

## Global Constraints

- Rust: `cargo test -j4 --manifest-path dashboard/Cargo.toml`, never `--workspace` or `--all`, never `CARGO_TARGET_DIR`. JS: `node --test dashboard/tests/*.test.js`.
- The dashboard changes only. No driver, `board.json` or factoryd change. `stage_since` on the spec card is a new, additive field of `/api/state`.
- Everything put in HTML goes through `escapeHtml`.
- `MERGED_LIMIT` stays 10 and applies to the Merged column only. The count in a column header is the total before the cap.
- A `stage_since` that is missing, `NaN` or not a number counts as 0. `formatStamp` returns an empty string unless the value is a finite number above 0.
- Tests use `.expect("...")` and `expect_err` in Rust, never the unwrap call. Comments are short full sentences ending with a punctuation mark.

## Review Focus

1. A series whose plans include a cancelled job. Expected: its `stage_since` counts too. Pinned by `a_spec_card_takes_the_latest_stage_since_of_its_plans` (Task 1).
2. Cards with equal or missing times. Expected: the later server entry first, nothing missing sorted above a dated card. Pinned by `equal times keep the later entry first` and `a job with no stage_since sorts last and shows its created date` (Task 3).
3. A job whose `stage_since` is a string. Expected: counts as 0, no exception. Pinned by `a non-numeric stage_since counts as 0` (Task 3).
4. A column with drafts. Expected: drafts first and counted, then the sorted list. Pinned by `drafts stay first in the spec column, show updated_at, fall back to created, and are counted` (Task 3).
5. Time zone. Expected: stamps in the browser's local time; tests fix `TZ=UTC`. Pinned by `formatStamp prints the local date and time and is empty for bad input` (Task 2).

---

## File Structure

- `dashboard/src/specs.rs` (modify): `SpecCard.stage_since` and its tests.
- `dashboard/src/state.rs` (modify): one JSON assertion in `the_state_has_one_spec_card_per_series_and_none_for_old_jobs`.
- `dashboard/static/board.js` (modify): `formatStamp`, header stamps, sorted column lists.
- `dashboard/tests/order.test.js` (create): new tests, `process.env.TZ = "UTC"` first.
- `dashboard/tests/board.test.js` (modify): two existing tests.
- `dashboard/README.md` (modify): line 55, the `specs[]` description.

### Task 1: `stage_since` on the spec card

**Files:**
- Modify: `dashboard/src/specs.rs:23-30,67-103`, `dashboard/src/state.rs` (the test near line 453)
- Test: `dashboard/src/specs.rs` (unit tests), `dashboard/src/state.rs`

**Interfaces:**
- Produces: `SpecCard { ..., pub stage_since: f64 }`, serialized as `"stage_since"`. Its value is the largest `Job.stage_since` (`model.rs:148`, an `f64`) among all members of the series, cancelled ones included, read with `fold(f64::MIN, f64::max)`.

- [ ] **Step 1: Read** `specs.rs` (`SpecCard`, `build_spec_cards`, the `job_json` and `job` test helpers) and the state test that builds a one-series board.
- [ ] **Step 2: Write the failing tests.** In `specs.rs`: `a_spec_card_takes_the_latest_stage_since_of_its_plans` builds three plan jobs of one series with `stage_since` 5.0, 9.0 and 7.0, the 7.0 one with `"cancelled": true`, builds `JobView`s and `Paths` as the state test does, calls `build_spec_cards`, and asserts `cards[0].stage_since == 9.0`. `a_spec_card_serializes_stage_since` asserts `serde_json::to_value(&card)` has `json["stage_since"] == 9.0`. In `state.rs`, add `assert!(json["specs"][0]["stage_since"].is_number());` after the existing `json["specs"][0]["plans"][1]["job_id"]` assertion.
- [ ] **Step 3: Run** `cargo test -j4 --manifest-path dashboard/Cargo.toml specs::`. Expected: compile failure, no field `stage_since` on `SpecCard`.
- [ ] **Step 4: Implement** the field with a doc comment and fill it in the `SpecCard { .. }` literal in `build_spec_cards`.
- [ ] **Step 5: Run** `cargo test -j4 --manifest-path dashboard/Cargo.toml`. Expected: all pass.

### Task 2: Header stamps

**Files:**
- Modify: `dashboard/static/board.js` (new function near `formatAge`, `renderCard` at the `job-date` line, `renderSpecCard` header, `renderDraftCard`, the `api` export list), `dashboard/tests/board.test.js:97`
- Create: `dashboard/tests/order.test.js`
- Test: `dashboard/tests/order.test.js`, `dashboard/tests/board.test.js`

**Interfaces:**
- Produces: `formatStamp(ts: number): string`, exported on the board API. Local time as `YYYY-MM-DD HH:MM` (24 hour, zero padded), `""` unless `ts` is a finite number above 0.
- Header rules: job card `<time class="job-date" title="Last stage change">` shows `formatStamp(job.stage_since)` and falls back to `job.created`. Spec card shows the same element with `formatStamp(spec.stage_since)`, omitted when empty. Draft card shows `formatStamp(draft.updated_at)` with title `Last change`, falling back to `draft.created`.

- [ ] **Step 1: Read** the three header sites in `board.js` and `makeJob`, `makeSpec`, `makeDraft` in `board.test.js`.
- [ ] **Step 2: Write the failing tests.** `order.test.js` starts with `"use strict"; process.env.TZ = "UTC";`, then requires `node:test`, `node:assert/strict` and `../static/board.js`, and copies the `makeJob`, `makeSpec`, `makeDraft`, `makeState` helpers it needs. Full example:

```js
test("formatStamp prints the local date and time and is empty for bad input", () => {
  assert.equal(board.formatStamp(0), "");
  assert.equal(board.formatStamp(1792490800), "2026-10-20 10:06");
  assert.equal(board.formatStamp(NaN), "");
  assert.equal(board.formatStamp("5"), "");
  assert.equal(board.formatStamp(-1), "");
});
```

  Header tests, each with its own inputs:

  | Test | Inputs | Assertion |
  |---|---|---|
  | `a job header shows the stamp of stage_since` | `makeJob({ stage_since: 1792490800 })` | html contains `2026-10-20 10:06` and `title="Last stage change"` |
  | `a job without a usable stage_since shows its created date` | `stage_since: undefined`, `created: "2026-10-13"` | html contains `2026-10-13` |
  | `a spec card header shows its stamp, and none without stage_since` | `makeSpec({ stage_since: 1792490800 })` then `makeSpec({ stage_since: undefined })` | first has the stamp in a `job-date` element, second has no `job-date` |
  | `a draft header shows updated_at and falls back to created` | `makeDraft({ updated_at: 1792490800 })`, then `updated_at: undefined` with `created: "2026-10-18"` | stamp, then `2026-10-18` |

  In `board.test.js`, `board.test.js` does not fix the time zone, so change `renderCard shows id, slug, date, branch, round and age` to build its job with `stage_since: undefined` and keep asserting `2026-10-13`, which now comes from the `created` fallback.
- [ ] **Step 3: Run** `node --test dashboard/tests/order.test.js`. Expected: failure, `board.formatStamp is not a function`.
- [ ] **Step 4: Implement** `formatStamp` with `getFullYear`, `getMonth`, `getDate`, `getHours`, `getMinutes` and a two-digit pad; apply the header rules at the three sites; export it.
- [ ] **Step 5: Run** `node --test dashboard/tests/*.test.js`. Expected: all pass.

### Task 3: Sorted column lists

**Files:**
- Modify: `dashboard/static/board.js:610-640` (`renderBoard`), `dashboard/tests/board.test.js` (`the merged column keeps only the last spec cards but counts all`), `dashboard/README.md:55`
- Test: `dashboard/tests/order.test.js`

**Interfaces:**
- Consumes: `formatStamp` and the header rules from Task 2; `SpecCard.stage_since` from Task 1.
- Produces: inside `renderBoard`, per column, `entries = [{ since, html }]` built from that column's spec cards and job cards in server order (specs first), reversed, then stable sorted by `since` descending, so ties give the later entry first. `since` is `spec.stage_since` or `job.stage_since` when finite, else 0. Draft cards render before the entries. For `merged` only, `entries.slice(0, MERGED_LIMIT)`. The count stays `drafts + specs + jobs` of the column. The unknown-stage column is not sorted.

- [ ] **Step 1: Read** `renderBoard`, `MERGED_LIMIT`, and the tests `renderBoard shows only the 10 newest merged jobs`, `the merged column keeps only the last spec cards but counts all`, `drafts come first in the spec column and are counted`.
- [ ] **Step 2: Write the failing tests** in `order.test.js`. Full example:

```js
test("a merged spec card sorts among the merged jobs by time", () => {
  const jobs = [
    makeJob({ id: "001", stage: "merged", stage_since: 100 }),
    makeJob({ id: "002", stage: "merged", stage_since: 300 }),
  ];
  const specs = [makeSpec({ series_id: "S9", stage: "merged", stage_since: 200 })];
  const html = board.renderBoard(makeState(jobs, { specs }));
  const at = (needle) => html.indexOf(needle);
  assert.ok(at('data-job="002"') < at('data-spec="S9"'));
  assert.ok(at('data-spec="S9"') < at('data-job="001"'));
});
```

  The others, each with its own inputs:

  | Test | Inputs | Assertion |
  |---|---|---|
  | `the merged column keeps the 10 newest cards of any kind and counts all` | 8 merged jobs at times 1..8, 5 merged specs at times 9..13 | 10 `<article class="card` in the column, newest 13 first, count `13` |
  | `columns other than merged are not capped` | 12 jobs in `build` | 12 cards |
  | `equal times keep the later entry first` | jobs `001` and `002` at the same time | `002` before `001` |
  | `a job with no stage_since sorts last and shows its created date` | one job with no time, one at 50 | dated job first |
  | `a non-numeric stage_since counts as 0` | `stage_since: "99"` and one at 5 | no exception, the 5 job first |
  | `every column sorts specs and jobs together` | a `needs-human` spec at 20 and a `needs-human` job at 10 | spec first in that column |
  | `drafts stay first in the spec column, show updated_at, fall back to created, and are counted` | a draft, a spec-stage spec at 50, a job at 40 | draft first, then spec, count `3` |

  In `board.test.js`, give the 13 specs of `the merged column keeps only the last spec cards but counts all` a `stage_since` equal to their number, so the assertions (`S13` present, `S3` absent) hold by time.
- [ ] **Step 3: Run** `node --test dashboard/tests/order.test.js`. Expected: failures on the ordering assertions, since specs still render before jobs.
- [ ] **Step 4: Implement** the entry lists and the Merged cap in `renderBoard`, and update `dashboard/README.md` line 55 so `specs[]` lists `stage_since` and the text says columns are ordered newest first.
- [ ] **Step 5: Run** `node --test dashboard/tests/*.test.js` and `cargo test -j4 --manifest-path dashboard/Cargo.toml`. Expected: all pass.

Next plan: none
