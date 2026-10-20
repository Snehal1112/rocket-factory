# Dashboard columns sorted by time

## Goal

Show every dashboard column as one list of cards ordered by time, newest first, with spec cards mixed in among the jobs, and show the date and time on the right of every card header.

## Overview

In the Merged column the spec cards always render above the job cards, and a spec card has no date on its right side, so the column is not in time order. The user wants the latest merge on top, spec cards included, and a date and time on every card.

## Current state

| Area | Today | Reference |
|---|---|---|
| Column build | Draft cards, then spec cards, then job cards, in that fixed order | `dashboard/static/board.js:616-640` |
| Merged cap | Specs `slice(-10)`, jobs sorted by `stage_since` descending and cut to 10, separately | `dashboard/static/board.js:37,622-629` |
| Count in the header | Total of jobs, specs and drafts before the cap | `dashboard/static/board.js:621` |
| Job card date | `job.created`, a date such as `2026-10-13` | `dashboard/static/board.js:589` |
| Spec card header | Id, stage label, plan count; no date | `dashboard/static/board.js:231-232` |
| Draft card date | `draft.created` | `dashboard/static/board.js:261` |
| Job time field | `stage_since` (epoch seconds) is in the job JSON | `dashboard/src/model.rs:148` |
| Draft time field | `updated_at` (epoch seconds) is in the draft JSON | `dashboard/src/model.rs:208-216` |
| Spec card data | `SpecCard { series_id, title, goal, stage, plans }`, no time | `dashboard/src/specs.rs:23-30` |
| Builder | `build_spec_cards` groups the plan jobs of a series | `dashboard/src/specs.rs:67-103` |
| Header style | `.job-date` is pushed to the right | `dashboard/static/dashboard.css:173` |

Gaps: spec cards have no time, so they cannot be ordered, and the headers show a date without a time.

## Non-goals

- No change to the driver, `board.json` or factoryd's API.
- No change to which column a card belongs to.
- No new sort controls or filters.
- No change to the 10-card cap value.

## Design

Flow: `build_spec_cards` adds `stage_since` to each spec card -> `renderBoard` builds one list per column and sorts it -> each header prints a stamp.

### 1. `stage_since` on the spec card

`SpecCard` gets `pub stage_since: f64`: the largest `stage_since` among all plan jobs of the series, cancelled ones included. The builder reads it with `fold(f64::MIN, f64::max)`. A series always has at least one plan job, so the value is always set. The field is additive in `/api/state`.

```json
{ "series_id": "S2", "stage": "merged", "stage_since": 1792490800.5, "plans": [ ... ] }
```

### 2. One sorted list per column

`renderBoard` makes, per column, a list of entries `{ since, html }` from the spec cards and the job cards of that column. `since` is `spec.stage_since` or `job.stage_since`. A value that is not a finite number counts as 0. The list is sorted by `since` descending. Equal values keep the reverse of their order in the server data, so later entries come first (specs are listed before jobs in that order).

The stray column for jobs with an unknown stage is not sorted and renders in server order. Draft cards are not in the list. In the Spec column they render first, in server order, as today.

### 3. Merged cap

Only the Merged column is capped. After sorting, the first `MERGED_LIMIT` (10) entries render, whatever their kind. The count in the column header stays the total before the cap, drafts included. Other columns render every entry.

### 4. Header stamp

New `formatStamp(ts)` in `board.js` returns the local time of `ts` (epoch seconds) as `YYYY-MM-DD HH:MM`, 24 hour clock, and an empty string when `ts` is not a finite number above 0. It is exported next to `formatAge`.

Job card: `<time class="job-date" title="Last stage change">` shows `formatStamp(job.stage_since)`, and falls back to `job.created` when the stamp is empty. Spec card: the same element with `formatStamp(spec.stage_since)`, omitted when the stamp is empty. Draft card: `formatStamp(draft.updated_at)`, falling back to `draft.created`, with the title `Last change`. The element keeps the class `job-date`, so the CSS is unchanged.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| Two cards with the same `since` | Reverse server order, later first |
| Job with `stage_since` missing or `NaN` | Counts as 0, sorts last; header shows `created` |
| Older `/api/state` without `spec.stage_since` | Spec sorts as 0; header shows no date |
| More than 10 cards in Merged | Newest 10 render; count shows the total |
| Spec column with drafts | Drafts first, then specs and jobs by time |
| Job `stage_since` is a string or `NaN` | Counts as 0 |
| Empty column | `Nothing here`, as today |
| Browser time zone | `formatStamp` uses the browser's local time |

## Testing

`dashboard/src/specs.rs` unit tests:
- `a_spec_card_takes_the_latest_stage_since_of_its_plans`: plans at 5.0, 9.0 and 7.0 give 9.0.
- `a_spec_card_serializes_stage_since`: `json["stage_since"]` equals the value.

`dashboard/src/state.rs`: extend the test around line 453 so `json["specs"][0]["stage_since"]` is a number.

`dashboard/tests/order.test.js` (new, sets `process.env.TZ = "UTC"` first):
- `formatStamp prints the local date and time and is empty for bad input`
- `a merged spec card sorts among the merged jobs by time`
- `the merged column keeps the 10 newest cards of any kind and counts all`
- `equal times keep the later entry first`
- `a job with no stage_since sorts last and shows its created date`
- `every column sorts specs and jobs together`
- `drafts stay first in the spec column, show updated_at, fall back to created, and are counted`
- `columns other than merged are not capped`
- `a non-numeric stage_since counts as 0`
- `a spec card header shows its stamp, and none without stage_since`

`dashboard/tests/board.test.js`: `renderCard shows id, slug, date, branch, round and age` (line 97) asserts the `created` date, so change it to assert the stamp of its `stage_since` and add a case with no `stage_since` that still shows `created`. Update `the merged column keeps only the last spec cards but counts all` to give the specs a `stage_since`. `renderBoard shows only the 10 newest merged jobs` and the draft tests must keep passing.

Commands: `cargo test -j4 --manifest-path dashboard/Cargo.toml` and `node --test dashboard/tests/*.test.js`.

## Files changed

- `dashboard/src/specs.rs`: `SpecCard.stage_since` and its tests.
- `dashboard/src/state.rs`: one assertion.
- `dashboard/static/board.js`: `formatStamp`, sorted column lists, headers.
- `dashboard/tests/order.test.js`: new. `dashboard/tests/board.test.js`: one test updated.
- `dashboard/README.md` (line 55): `stage_since` on `specs[]` and the column order.

## Constraints

- Cargo with `-j4`, no `--workspace`, no `CARGO_TARGET_DIR`.
- The dashboard is its own workspace; `board.json` stays compatible, with no new field written by the driver.
- All text put in HTML goes through `escapeHtml`.
- Comments are short full sentences.

## Acceptance criteria

- Each spec card in `/api/state` carries `stage_since`, the latest of its plans.
- Every column lists its spec and job cards in one list, newest first by `stage_since`.
- Draft cards stay first in the Spec column.
- Equal times show the later entry first.
- Merged shows the 10 newest cards of any kind, and its count is the total.
- Job, spec and draft headers show `YYYY-MM-DD HH:MM` in local time on the right.
- A job without a usable `stage_since` shows its `created` date and sorts last.
- A spec card without `stage_since` shows no date and sorts last.
- Both test suites pass.

## Out of scope

- Server-side sorting and any change to factoryd.
- Relative times such as `3h ago` in the headers.

## Follow-ups

None.

## Decisions

- Which columns are sorted, and how? Options: Merged only with specs and jobs mixed, every column with specs and jobs mixed, Merged only with jobs first. Chosen: every column, one list each. Decided by: user
- What do the headers show? Options: the date of the last plan merge only, the creation date, date and time on every card. Chosen: date and time on every card. Decided by: user
- How is the Merged cap applied after the mix? Options: 10 cards in total, 10 jobs plus 10 specs, 20 in total. Chosen: 10 cards in total, newest first. Decided by: user
- A job's `created` date is shown only when its `stage_since` is unusable. Decided by: call
- The time of a card is the last stage change (`stage_since`), and a spec card uses the latest of its plans. Decided by: call
- Drafts stay first in the Spec column and show `updated_at`. Decided by: call
- Ties keep the later server entry first, so the existing tests without times still pass. Decided by: call
- Times print in the browser's local time zone. Decided by: call
