# parse_found refuses JSON it cannot trust

## Goal

Make `parse_found` return an error, not "no match", when `gh` prints JSON that is not an array or a matching issue has an unusable number, so the ticket step never files a duplicate issue.

## Overview

The ticket step files follow-up issues after a merge. Before it files a title that was left pending by an earlier run, it asks the tracker whether that issue already exists (`find_created`). The `gh` tracker answers by parsing `gh issue list --json number,title,body` with `parse_found`.

`parse_found` treats any valid JSON that is not an array as an empty list, so it returns `Ok(None)`. It also drops a matching issue whose `number` is missing or does not fit in `u32`. In both cases the step concludes the issue does not exist and creates it again, which is the duplicate the pending marker is there to prevent. This was reported as finding 1 of the review of job 054 in issue #62. No test covers a non-array reply or a number too large for `u32`.

## Current state

| Piece | Where | Today |
|---|---|---|
| Parser | `crates/factory-engine/src/tracker.rs:326-338` | `value.as_array().into_iter().flatten()`: a non-array value yields nothing, so `Ok(None)`. |
| Number | `crates/factory-engine/src/tracker.rs:335` | `filter_map(... as_u64().and_then(u32::try_from(n).ok()))`: a missing or too-large number drops the entry silently. |
| Invalid JSON | `crates/factory-engine/src/tracker.rs:327-328` | Returns `Err("gh printed invalid JSON: ...")`. |
| Caller | `crates/factory-engine/src/tracker.rs:507-509` | `GhTracker::find_created` returns `parse_found(&self.run(&find_args())?, title, footer)`. |
| Step | `crates/factory-engine/src/tickets.rs:317-339` | `Ok(Some(n))` adopts `#n`, `Ok(None)` goes on to `create`, `Err(e)` reports `find \`<title>\`: <e>` and stops without creating. |
| Step test | `crates/factory-engine/tests/tickets.rs` `a_failed_find_stops_without_creating` | A `find_created` error marks the report failed, makes no `create` call and keeps the pending entry. |
| Parser tests | `crates/factory-engine/src/tracker.rs` `mod tests` (from line 555) | `parse_found_picks_the_lowest_number_with_title_and_footer`, `parse_found_ignores_a_title_without_the_footer`, `parse_found_treats_a_null_body_as_empty`, `parse_found_rejects_invalid_json`, with `const FOOTER: &str = "Filed by factory job 007 (slug)"`. |

Gaps: a non-array reply and a matching entry with a bad number both read as "not found", and neither case has a test.

## Non-goals

- `parse_issue_list` (`tracker.rs:343`), used by intake, has the same non-array gap. It is a follow-up, not part of this job.
- No change to the `Tracker` trait, `GhTracker::find_created`, `find_args` or the ticket step.
- No change to how unrelated issues in the list are read.

## Design

```
gh issue list JSON
  -> invalid JSON?                         Err("gh printed invalid JSON: ...")   (unchanged)
  -> not an array?                         Err("gh printed unexpected JSON: expected an array")
  -> for each entry with title == title and body contains footer:
       number missing or > u32::MAX?       Err("gh printed an unusable number for `<title>`")
       else                                keep the number
  -> lowest kept number                    Ok(Some(n)), or Ok(None) when none match
```

### 1. Non-array reply

Signature stays `pub(crate) fn parse_found(json: &str, title: &str, footer: &str) -> Result<Option<u32>, String>`.

When the parsed `serde_json::Value` is not an array, return exactly `Err("gh printed unexpected JSON: expected an array".to_string())`. An empty array `[]` is still valid and gives `Ok(None)`.

Example: `{"message":"Bad credentials"}` gives `Err("gh printed unexpected JSON: expected an array")`.

### 2. Matching entry with an unusable number

An entry matches when `issue["title"].as_str() == Some(title)` and its body (a missing or null body counts as empty) contains `footer`, the same test as today. For a matching entry, `number` must be a JSON integer that `as_u64` reads and that fits in `u32`. When it is missing, not an integer, or over `u32::MAX`, return ``Err(format!("gh printed an unusable number for `{title}`"))``.

The first such matching entry ends the parse with that error, even when another matching entry has a good number, because the bad one could be the lowest.

Example: `[{"number":4294967296,"title":"Fix x","body":"Filed by factory job 007 (slug)"}]` with title `Fix x` gives ``Err("gh printed an unusable number for `Fix x`")``.

### 3. Unchanged behaviour

- The lowest matching number wins.
- A missing or null body counts as empty.
- An entry whose title or footer does not match is skipped, whatever its number looks like, so one odd unrelated issue never blocks the step.
- An array element that is not an object has no title, so it does not match and is skipped.

The doc comment on `parse_found` is updated to name the two new errors.

## Error handling and edge cases

| Case | Result |
|---|---|
| Invalid JSON (`nope`) | `Err` containing `invalid JSON` (unchanged) |
| Object (`{"message":"Bad credentials"}`) | `Err("gh printed unexpected JSON: expected an array")` |
| `null`, a string, a number | Same `expected an array` error |
| Empty array `[]` | `Ok(None)` |
| Matching entry, number `4294967296` | ``Err("gh printed an unusable number for `Fix x`")`` |
| Matching entry, no `number` field | Same unusable-number error |
| Matching entry, number `"7"` (a string) | Same unusable-number error |
| Unrelated entry, number `4294967296`, plus a matching entry `#4` | `Ok(Some(4))` |
| Two matching entries, one with a bad number | The unusable-number error |
| Number `4294967295` (`u32::MAX`), matching | `Ok(Some(4294967295))` |

Any `Err` from `parse_found` reaches the ticket step through `find_created`, which reports it and stops without creating, as `a_failed_find_stops_without_creating` already shows.

## Testing

Unit tests in `crates/factory-engine/src/tracker.rs` `mod tests`, next to the existing `parse_found_*` tests and using the same `FOOTER` constant:

- `parse_found_rejects_an_object_reply`: `{"message":"Bad credentials"}` gives an error equal to `gh printed unexpected JSON: expected an array`.
- `parse_found_rejects_a_null_reply`: `null` gives the same error.
- `parse_found_accepts_an_empty_array`: `[]` gives `Ok(None)`.
- `parse_found_rejects_a_matching_number_over_u32`: a matching entry with number `4294967296` gives an error equal to ``gh printed an unusable number for `Fix x` ``.
- `parse_found_rejects_a_matching_entry_without_a_number`: a matching entry with no `number` gives the same error.
- `parse_found_skips_an_unrelated_number_over_u32`: an entry titled `Other` with number `4294967296`, plus a matching `#4`, gives `Ok(Some(4))`.
- `parse_found_keeps_u32_max`: a matching entry with number `4294967295` gives `Ok(Some(4294967295))`.

The four existing `parse_found_*` tests must keep passing unchanged.

Commands:

```bash
cargo test -j4 -p factory-engine parse_found
cargo test -j4 -p factory-engine --test tickets
```

## Files changed

- `crates/factory-engine/src/tracker.rs`: `parse_found` and its doc comment, plus the new unit tests.

## Constraints

- Cargo commands are per crate with `-j4`, never `--workspace` or `--all`.
- The logic sits behind the existing `Tracker` port; nothing outside `tracker.rs` changes.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.

## Acceptance criteria

- `parse_found` returns `Err("gh printed unexpected JSON: expected an array")` for valid JSON that is not an array.
- `parse_found` returns ``Err("gh printed an unusable number for `<title>`")`` for a matching entry whose number is missing, not an integer or over `u32::MAX`.
- An unrelated entry with a bad number is still skipped, and `[]` still gives `Ok(None)`.
- `u32::MAX` is accepted as a number.
- The seven new tests and the four existing `parse_found_*` tests pass.
- `cargo test -j4 -p factory-engine --test tickets` passes unchanged.

## Out of scope

- Fixing `parse_issue_list` or other `gh` JSON parsers.
- Raising the `--limit 100` of `find_args`.

## Follow-ups

- `parse_issue_list` (`crates/factory-engine/src/tracker.rs:343`) reads `gh` JSON that is not an array as an empty list, so intake silently picks no issue; return an error for it the same way `parse_found` now does, with a test.

## Decisions

- What happens to an entry whose `number` is missing or too large for u32? Options: error only when the entry matches, error on any bad number, keep skipping and add a test. Chosen: error only when the entry matches. Decided by: user
- Fix the same non-array gap in `parse_issue_list` in this job? Options: follow-up ticket, fix both here, ignore it. Chosen: follow-up ticket. Decided by: user
- How to build it? Options: edit `parse_found` in place, a typed serde struct, a shape check in `GhTracker::find_created`. Chosen: edit `parse_found` in place. Decided by: user
- When two entries match and one has a bad number, return the error, because the bad one could be the lowest. Decided by: call
- Exact error texts `gh printed unexpected JSON: expected an array` and ``gh printed an unusable number for `<title>` ``, following the existing `gh printed invalid JSON` wording. Decided by: call
- Only unit tests are added; the ticket step's handling of a find error is already pinned by `a_failed_find_stops_without_creating`. Decided by: call
