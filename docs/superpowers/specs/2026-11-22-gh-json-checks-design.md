# gh replies the tracker cannot trust become errors

**Commit:** fix(tracker): reject gh replies it cannot trust

## Goal

Make every `gh` reply the tracker reads either parse to a trusted value or fail with an error, so the ticket step and intake never act on a reply they misread.

## Overview

`crates/factory-engine/src/tracker.rs` is the only code that reads `gh` output. #62 and #135 fixed `parse_found` and `parse_issue_list` for replies that are not arrays. #136 reports that `parse_view` still reads a reply with no `state` as an open issue. A survey of the file found the same kind of silent read in three more places:

- `parse_actor` treats a stray object as a non-matching event.
- `parse_issue_list` reads a bad `labels` value as no labels.
- `repo_name` and `current_user` accept any non-empty text.

This job fixes all four places. Nothing is left for a follow-up ticket.

Effects today:
- The ticket step closes a parent only when `is_closed` says it is open. On a reply without `state` it tries to close the issue again.
- `labels_of` feeds the labels copied onto follow-up tickets. On a reply without labels, those tickets silently get none.
- Intake skips an issue whose label lookup returns `Ok(None)`. On a stray object it logs "no label event found", which is misleading. A matching event without a time gives `at = ""`, so `updated_at > at` is always true and intake logs "the issue changed after the intake label was set".
- `repo_name` builds the events API path `repos/{repo}/issues/{n}/events`, so a malformed name builds a wrong path.

## Current state

| Piece | Where | Today |
|---|---|---|
| `parse_view` | `crates/factory-engine/src/tracker.rs:306-322` | A missing `labels` gives `[]`, a label without `name` is dropped, and a missing or unknown `state` reads as open (`is_some_and(eq_ignore_ascii_case("closed"))`). |
| `labels_of`, `is_closed` | `crates/factory-engine/src/tracker.rs` `impl Tracker for GhTracker` | Both call `parse_view(&self.run(&view_args(issue))?)`. |
| Ticket step | `crates/factory-engine/src/tickets.rs:278` and `:450` | An `Err` from `labels_of` or `is_closed` is reported with `report.fail` and the step stops. |
| `parse_issue_list` labels | `crates/factory-engine/src/tracker.rs:363-368` | `issue["labels"].as_array().into_iter().flatten()` drops a bad label value without a sign. |
| `parse_actor` | `crates/factory-engine/src/tracker.rs:382-405` | Each top-level value is an array page or one event. A value that is not an event counts as a non-matching event, and a matching event without `created_at` gets `at: ""`. A null actor gives `None`. |
| Events filter | `crates/factory-engine/src/tracker.rs:274` `EVENTS_JQ` | Prints `{event, label: {name}, actor: {login}, created_at}` per labeled event. |
| Intake | `crates/factory-engine/src/intake.rs:171-193` `eligible` | An `Err` from `labelled_by` is a logged skip of that one issue. |
| `repo_name` | `crates/factory-engine/src/tracker.rs:487-504` | Rejects only empty output (`gh repo view printed no repository name`). |
| `current_user` | `crates/factory-engine/src/tracker.rs:553-563` | Rejects only empty output (`gh api user printed no login`). |
| `run` | `crates/factory-engine/src/tracker.rs:426-470` | Returns the trimmed stdout. On a missing key, gh's `--jq` prints an empty line, so the empty checks already catch it (checked by hand with `gh api user --jq .nosuchkey`). |

Existing tests that pin today's behaviour, all in `tracker.rs` `mod tests`:
- `view_json_gives_labels_and_state` (line 595)
- `parse_issue_list_*`: 2 read tests and 9 tests added for #135
- `parse_actor_*`: 6 tests, with the fixtures `EVENTS` (line 882) and `LABELED` (line 917)
- the fake-`gh` helper `fake_gh` (line 957)

Gaps:
- None of the four places has a test for a malformed reply.
- Some fixtures lack fields that real `gh` always prints. The `EVENTS` events have no `created_at`, and five of the #135 tests have entries with no `labels`.

## Non-goals

- No change to the `Tracker` trait, the fake trackers in `crates/factory-engine/tests/`, `tickets.rs` or `intake.rs`.
- No change to `parse_found` or `parse_created`.
- No change to `Marker::load` in `tickets.rs`: it reads the factory's own file, not `gh` output.
- A null actor on the last matching event still gives `None`. A deleted user shows as null, and that is not a malformed reply.

## Design

```
parse_view      reply object  -> labels array of {name: string}?  state OPEN|CLOSED?  else Err
parse_issue_list entry        -> labels array of {name: string}?  else skip entry (like a bad number)
parse_actor     each value    -> array of event objects | one event object, each with string `event`; else Err
                matching event -> string `created_at`?  else Err;  null actor -> None (unchanged)
repo_name       trimmed text  -> owner/name, one `/`, no whitespace?  else Err
current_user    trimmed text  -> one token, no whitespace, no `/`?  else Err
```

Every error goes to the existing callers, which already handle an `Err` (see Current state). All texts start with `gh printed` or name the `gh` command, as the existing errors do.

### 1. `parse_view`

The signature stays `pub(crate) fn parse_view(json: &str) -> Result<(Vec<String>, bool), String>`.

- The reply must be an object with a `labels` array. Otherwise return `Err("gh printed unexpected JSON: expected a labels array".to_string())`. This also covers a reply that is not an object, because indexing gives `Null`.
- Every element of `labels` must have a string `name`. Otherwise return `Err("gh printed unexpected JSON: a label has no name".to_string())`.
- `state` must be a string equal to `OPEN`, `CLOSED` or `MERGED`, ignoring case. Otherwise return ``Err(format!("gh printed an unusable issue state: `{state}`"))``, where `{state}` is the raw JSON value from `serde_json::Value`'s `Display`, for example `null` or `"DRAFT"`. `CLOSED` and `MERGED` give `true`, and `OPEN` gives `false`. `gh issue view` on a pull request prints `MERGED` with exit 0 (checked on #47), and a feature text can cite a PR number, so a merged PR reads as done.
- The labels check runs first, then the state check.

Example: `{"message":"Not Found"}` gives `Err("gh printed unexpected JSON: expected a labels array")`. `{"labels":[],"state":"DRAFT"}` gives ``Err("gh printed an unusable issue state: `\"DRAFT\"`")``, and `{"labels":[],"state":"MERGED"}` gives `Ok(([], true))`.

### 2. `parse_issue_list` labels

An entry is skipped, like an entry with a bad number, when any of these holds:
- `labels` is missing or is not an array;
- any element of `labels` lacks a string `name`.

`"labels":[]` is valid and gives an empty list. The rest of the entry is read as today. The doc comment names the new skip rule.

### 3. `parse_actor`

The signature stays `pub(crate) fn parse_actor(json: &str, label: &str) -> Result<Option<LabelEvent>, String>`.

- Each top-level value must be either an array whose elements are all objects with a string `event`, or a single such object. Otherwise return `Err("gh printed unexpected JSON: expected label events".to_string())`. That covers a stray object, a number, a string, `null`, and an array element that is not an event.
- Empty input still gives `Ok(None)`, and so does an empty array `[]`.
- A `labeled` event for `label` must have a string `created_at`. Otherwise return ``Err(format!("gh printed a `{label}` label event without a time"))``.
- The last matching event wins, and a null or missing actor login on it gives `None`, both as today.

Example: `{"message":"Not Found"}` gives `Err("gh printed unexpected JSON: expected label events")`.

### 4. `repo_name` and `current_user`

Two pure helpers in `tracker.rs`, called by the two `GhTracker` methods after the existing empty check:

```rust
pub(crate) fn check_repo_name(name: &str) -> Result<(), String>
pub(crate) fn check_login(login: &str) -> Result<(), String>
```

- `check_repo_name` accepts only text with exactly one `/`, a non-empty part on each side, and no whitespace character (`char::is_whitespace`). Otherwise it returns ``Err(format!("gh repo view printed an unusable repository name: `{name}`"))``.
- `check_login` accepts only text with no whitespace character and no `/`. Otherwise it returns ``Err(format!("gh api user printed an unusable login: `{login}`"))``. Bot logins such as `github-actions[bot]` pass.
- The empty-output errors and their texts stay as they are.

Example: `o/r` passes, while `o/r/x`, `o r/x`, `/r` and `o/r\nx/y` each fail. `alice` passes, while `a b`, `a/b` and `a\nb` fail.

## Error handling and edge cases

| Case | Result |
|---|---|
| `parse_view` on `{"labels":[{"name":"a"}],"state":"OPEN"}` | `Ok((["a"], false))` |
| `parse_view` on `{"labels":[],"state":"closed"}` | `Ok(([], true))` |
| `parse_view` on `{"message":"Not Found"}` | `expected a labels array` error |
| `parse_view` on `[]` | `expected a labels array` error |
| `parse_view` on `{"labels":[{"id":1}],"state":"OPEN"}` | `a label has no name` error |
| `parse_view` on `{"labels":[]}` | ``unusable issue state: `null` `` |
| `parse_view` on `{"labels":[],"state":"MERGED"}` | `Ok(([], true))` |
| `parse_view` on `{"labels":[],"state":"DRAFT"}` | ``unusable issue state: `"DRAFT"` `` |
| `parse_view` on `not json` | `invalid JSON` error (unchanged) |
| issue-list entry without `labels`, plus a good `#4` | only `#4` |
| issue-list entry with `"labels":"x"`, plus a good `#4` | only `#4` |
| issue-list entry with `"labels":[{"id":1}]`, plus a good `#4` | only `#4` |
| `parse_actor` on `{"message":"Not Found"}` | `expected label events` error |
| `parse_actor` on `[1]` | `expected label events` error |
| `parse_actor` on `42` | `expected label events` error |
| `parse_actor` on empty input or `[]` | `Ok(None)` |
| a matching labeled event without `created_at` | `` `factory` label event without a time `` error |
| a non-matching event without `created_at` | ignored, as today |
| the last matching event with `"actor":null` | `Ok(None)`, as today |
| `repo_name` output `o/r` | `Ok("o/r")` |
| `repo_name` output `o/r/x` or `o r/x` | unusable repository name error |
| `current_user` output `a b` | unusable login error |
| empty output from either | the existing `printed no ...` error |

## Testing

All the new tests go in `crates/factory-engine/src/tracker.rs` `mod tests`.

`parse_view`:
- `parse_view_rejects_a_reply_without_labels`
- `parse_view_rejects_a_label_without_a_name`
- `parse_view_rejects_a_missing_state`
- `parse_view_rejects_an_unknown_state`
- `parse_view_rejects_an_array_reply`
- `parse_view_reads_a_lowercase_state`
- `parse_view_reads_a_merged_state_as_closed`

`parse_issue_list`:
- `parse_issue_list_skips_an_entry_without_labels`
- `parse_issue_list_skips_labels_that_are_not_an_array`
- `parse_issue_list_skips_a_label_without_a_name`

Existing-fixture changes: the inputs of `parse_issue_list_skips_a_number_over_u32`, `parse_issue_list_skips_a_string_number`, `parse_issue_list_skips_an_entry_without_a_number`, `parse_issue_list_skips_a_non_object_element` and `parse_issue_list_keeps_u32_max` gain `"labels":[]` on every entry that is meant to be kept. Real `gh` always prints `labels`.

`parse_actor`:
- `parse_actor_rejects_a_stray_object`
- `parse_actor_rejects_a_non_event_element`
- `parse_actor_rejects_a_scalar_value`
- `parse_actor_rejects_a_matching_event_without_a_time`
- `parse_actor_ignores_a_time_missing_on_another_label`

Existing-fixture changes: every `labeled` event in `EVENTS`, and the extra page in `parse_actor_reads_paginated_pages`, gains a `created_at` value.

Repo name and login:
- `check_repo_name_accepts_owner_and_name`
- `check_repo_name_rejects_malformed_names`
- `check_login_accepts_bot_logins`
- `check_login_rejects_malformed_logins`
- `repo_name_rejects_a_malformed_gh_reply`: fake gh that prints `o r/x`
- `current_user_rejects_a_malformed_gh_reply`: fake gh that prints `a b`
- `repo_name_keeps_the_empty_output_error`: fake gh that prints nothing, so the error is still `gh repo view printed no repository name`

All three fake-gh tests are built with `fake_gh`, as `edit_labels_reports_another_label_create_error` does.

Commands:

```bash
cargo test -j4 -p factory-engine --lib tracker
cargo test -j4 -p factory-engine --test tickets
cargo test -j4 -p factory-engine --test intake
cargo test -j4 -p factory-engine --test log_events
```

## Files changed

- `crates/factory-engine/src/tracker.rs`: `parse_view`, `parse_issue_list`, `parse_actor`, the new `check_repo_name` and `check_login`, the `repo_name` and `current_user` methods, the doc comments and the tests.

## Constraints

- Cargo commands are per crate with `-j4`, never `--workspace` or `--all`.
- The logic sits behind the existing `Tracker` port; nothing outside `tracker.rs` changes.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- Error texts are exactly the ones in Design.

## Acceptance criteria

- `parse_view` returns the `expected a labels array`, `a label has no name` or `unusable issue state` error for the cases in the table, accepts `OPEN`, `CLOSED` or `MERGED` in any case, and reads `MERGED` as closed.
- `parse_issue_list` skips an entry whose `labels` is missing, is not an array, or holds an element without a string `name`, and keeps the others.
- `parse_actor` returns `expected label events` for any top-level value that is not an event or an array of events, and returns the without-a-time error for a matching labeled event that has no `created_at`.
- `parse_actor` still gives `Ok(None)` for empty input, for `[]` and for a null actor on the last matching event.
- `repo_name` and `current_user` reject malformed output with the unusable-name and unusable-login errors, and keep the empty-output errors.
- All new tests and all existing `tracker` tests pass, with only the fixture changes listed in Testing.
- `cargo test -j4 -p factory-engine --test tickets`, `--test intake` and `--test log_events` pass unchanged.

## Out of scope

- Raising `--limit 100` on the list calls.
- Retrying a `gh` call after a malformed reply.

## Decisions

- Which gh JSON gaps does this job fix? Options: parse_view, parse_actor, parse_issue_list labels, repo_name and current_user. Chosen: all four, with no follow-up tickets. Decided by: user
- How strict is parse_view? Options: strict on labels and state, strict on state only, missing state only. Chosen: strict on both. Decided by: user
- How is `MERGED` read, which `gh issue view` prints for a merged PR? Options: as closed, rejected, as open. Chosen: as closed. Decided by: user
- What changes in parse_actor? Options: error on non-event values and on a matching event without a time, only non-event values, only a missing time. Chosen: error on both. Decided by: user
- What does a bad `labels` value do to an issue-list entry? Options: skip that entry, fail the whole list, keep reading it as empty. Chosen: skip that entry. Decided by: user
- Should the repo name and login shape be checked? Options: check the shape, tests only, drop it. Chosen: check the shape. Decided by: user
- How is the work split? Options: 2 plans in one job, 2 plans in 2 jobs, 1 plan. Chosen: 2 plans in one job (`--one-job`). Decided by: user
- A null actor still gives `None`, because GitHub shows a deleted user as null. Decided by: call
- The shape checks are pure helpers next to the parsers, so they can be tested without a fake `gh`; each also gets one fake-gh test. Decided by: call
- The existing fixtures gain the fields real `gh` always prints (`labels`, `created_at`) instead of the code tolerating their absence. Decided by: call
