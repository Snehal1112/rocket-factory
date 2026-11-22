# parse_issue_list refuses JSON that is not a list

## Goal

Make `parse_issue_list` return an error, not an empty list, when `gh` prints valid JSON that is not an array, so intake reports the failure instead of silently picking no issue.

## Overview

Intake picks the next labelled issue to build. It asks the tracker for the labelled issues (`list_labelled`), and the `gh` tracker answers by parsing `gh issue list --json number,title,body,labels,createdAt,updatedAt` with `parse_issue_list`.

`parse_issue_list` treats any valid JSON that is not an array as an empty list, so it returns `Ok(vec![])`. An error reply such as `{"message":"Bad credentials"}` then reads as "no labelled issues", and intake ends the pass with `NoIssue` as if the queue were empty. Nobody learns that `gh` is broken. This was filed as #135, a follow-up of #62, which fixed the same gap in `parse_found`. No test covers a non-array reply.

## Current state

| Piece | Where | Today |
|---|---|---|
| Parser | `crates/factory-engine/src/tracker.rs:352-377` | `value.as_array().into_iter().flatten()`: a non-array value yields nothing, so `Ok(vec![])`. |
| Number | `crates/factory-engine/src/tracker.rs:359` | `u32::try_from(issue["number"].as_u64()?).ok()?`: a missing, non-integer or over-`u32` number skips the entry. |
| Invalid JSON | `crates/factory-engine/src/tracker.rs:353-354` | Returns `Err("gh printed invalid JSON: ...")`. |
| Sibling parser | `crates/factory-engine/src/tracker.rs:328-347` | `parse_found` already returns `Err("gh printed unexpected JSON: expected an array")` for a non-array reply. |
| Caller | `crates/factory-engine/src/tracker.rs:529-531` | `GhTracker::list_labelled` returns `parse_issue_list(&self.run(&list_labelled_args(label))?)`. |
| Intake | `crates/factory-engine/src/intake.rs:209-211` | `pick_issue` maps a `list_labelled` error to `intake: cannot list the labelled issues: <e>` and returns it. |
| Intake test | `crates/factory-engine/tests/intake.rs` `a_tracker_failure_before_the_claim_leaves_the_labels_alone` | A `list_labelled` error fails the pass, makes no `edit_labels` call and runs no drafter. |
| Parser test | `crates/factory-engine/src/tracker.rs` `parse_issue_list_reads_labels_and_dates` (line 786) | Two entries with labels and dates, plus `nope` giving an `invalid JSON` error. |

Gap: a non-array reply reads as an empty queue, and no test covers it. The skip of an entry with a bad number is also untested.

## Non-goals

- `parse_view` (`tracker.rs:306`) reads a reply with no `labels` and no `state` as "open, no labels". It is a follow-up, not part of this job.
- No change to the `Tracker` trait, `GhTracker::list_labelled`, `list_labelled_args` or `crates/factory-engine/src/intake.rs`.
- No change to how an entry with a bad number is handled: it is still skipped.

## Design

```
gh issue list JSON
  -> invalid JSON?               Err("gh printed invalid JSON: ...")   (unchanged)
  -> not an array?               Err("gh printed unexpected JSON: expected an array")
  -> for each entry:
       number missing, not an integer or > u32::MAX?   skip it (unchanged)
       else                                           IssueInfo { ... } (unchanged)
  -> Ok(list), Ok(vec![]) for []
```

### 1. Non-array reply

Signature stays `pub(crate) fn parse_issue_list(json: &str) -> Result<Vec<IssueInfo>, String>`.

When the parsed `serde_json::Value` is not an array, return exactly `Err("gh printed unexpected JSON: expected an array".to_string())`, the same text `parse_found` uses. An empty array `[]` is still valid and gives `Ok(vec![])`.

Example: `{"message":"Bad credentials"}` gives `Err("gh printed unexpected JSON: expected an array")`.

### 2. Unchanged behaviour, now pinned by tests

- An entry whose `number` is missing, a string, or over `u32::MAX` is skipped. The rest of the list is still returned.
- An array element that is not an object has no number, so it is skipped.
- `u32::MAX` (`4294967295`) is a usable number.
- A missing or null body, title or date reads as an empty string, and missing labels read as an empty list.

The doc comment on `parse_issue_list` is updated to say a non-array reply is an error and an entry without a usable number is skipped.

## Error handling and edge cases

| Case | Result |
|---|---|
| Invalid JSON (`nope`) | `Err` containing `invalid JSON` (unchanged) |
| Object (`{"message":"Bad credentials"}`) | `Err("gh printed unexpected JSON: expected an array")` |
| `null` | Same `expected an array` error |
| A string (`"oops"`) | Same `expected an array` error |
| Empty array `[]` | `Ok(vec![])` |
| Entry with number `4294967296`, plus entry `#4` | `Ok` with only `#4` |
| Entry with number `"7"` (a string), plus entry `#4` | `Ok` with only `#4` |
| Entry with no `number`, plus entry `#4` | `Ok` with only `#4` |
| `[1, {...#4...}]` | `Ok` with only `#4` |
| Number `4294967295` | `Ok` with that number |

Any `Err` from `parse_issue_list` reaches intake through `list_labelled`, which fails the pass and leaves every label alone, as `a_tracker_failure_before_the_claim_leaves_the_labels_alone` already shows.

## Testing

Unit tests in `crates/factory-engine/src/tracker.rs` `mod tests`, next to `parse_issue_list_reads_labels_and_dates`:

- `parse_issue_list_rejects_an_object_reply`: `{"message":"Bad credentials"}` gives an error equal to `gh printed unexpected JSON: expected an array`.
- `parse_issue_list_rejects_a_null_reply`: `null` gives the same error.
- `parse_issue_list_rejects_a_string_reply`: `"oops"` gives the same error.
- `parse_issue_list_accepts_an_empty_array`: `[]` gives `Ok(vec![])`.
- `parse_issue_list_skips_a_number_over_u32`: an entry with number `4294967296` and an entry `#4` give a list holding only `#4`.
- `parse_issue_list_skips_a_string_number`: an entry with number `"7"` and an entry `#4` give a list holding only `#4`.
- `parse_issue_list_skips_an_entry_without_a_number`: an entry with no `number` and an entry `#4` give a list holding only `#4`.
- `parse_issue_list_skips_a_non_object_element`: `[1, {...#4...}]` gives a list holding only `#4`.
- `parse_issue_list_keeps_u32_max`: an entry with number `4294967295` gives a list holding that number.

`parse_issue_list_reads_labels_and_dates` must keep passing unchanged.

Commands:

```bash
cargo test -j4 -p factory-engine parse_issue_list
cargo test -j4 -p factory-engine --test intake
```

## Files changed

- `crates/factory-engine/src/tracker.rs`: `parse_issue_list` and its doc comment, plus the new unit tests.

## Constraints

- Cargo commands are per crate with `-j4`, never `--workspace` or `--all`.
- The logic sits behind the existing `Tracker` port; nothing outside `tracker.rs` changes.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- The error text is exactly `gh printed unexpected JSON: expected an array`.

## Acceptance criteria

- `parse_issue_list` returns `Err("gh printed unexpected JSON: expected an array")` for valid JSON that is not an array (object, `null`, string).
- `[]` still gives `Ok(vec![])`.
- An entry whose number is missing, a string or over `u32::MAX` is still skipped, and the other entries are returned.
- A non-object array element is skipped.
- `u32::MAX` is accepted as a number.
- The nine new tests and `parse_issue_list_reads_labels_and_dates` pass.
- `cargo test -j4 -p factory-engine --test intake` passes unchanged.

## Out of scope

- Fixing `parse_view`, `parse_actor` or other `gh` JSON parsers.
- Warning about or reporting skipped entries.

## Follow-ups

- `parse_view` (`crates/factory-engine/src/tracker.rs:306`) reads a `gh issue view` reply with no `labels` array and no `state` (for example `{"message":"Not Found"}`) as an open issue with no labels; return an error when `state` is missing, with a test.

## Decisions

- What happens to an entry whose `number` is missing, a string or over u32? Options: keep skipping and pin it with tests, error on any bad number, skip and warn. Chosen: keep skipping and pin it with tests. Decided by: user
- Fix the similar gap in `parse_view` in this job? Options: follow-up ticket, fix both here, ignore it. Chosen: follow-up ticket. Decided by: user
- Edit `parse_issue_list` in place, the same way `parse_found` was fixed for #62, rather than a typed serde struct. Decided by: call
- Reuse the exact error text `gh printed unexpected JSON: expected an array` from `parse_found`. Decided by: call
- Only unit tests are added; intake's handling of a `list_labelled` error is already pinned by `a_tracker_failure_before_the_claim_leaves_the_labels_alone`. Decided by: call
