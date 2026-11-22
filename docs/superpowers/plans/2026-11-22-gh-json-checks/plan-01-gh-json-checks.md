# gh JSON parsers refuse untrusted replies Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-22-gh-json-checks-design.md
> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** `parse_view`, `parse_issue_list` and `parse_actor` return an error, or skip the one bad entry, for `gh` JSON whose fields they cannot trust, instead of reading it as a default.

**Commit:** fix(tracker): reject untrusted gh JSON replies

**Architecture:** Three parsers in `crates/factory-engine/src/tracker.rs` change. They sit behind the `Tracker` port: `GhTracker::labels_of` and `is_closed` call `parse_view`, `list_labelled` calls `parse_issue_list`, and `labelled_by` calls `parse_actor`. Their callers in `tickets.rs` and `intake.rs` already handle an `Err`, so no caller changes.

**Tech Stack:** Rust, `serde_json::Value`.

## Global Constraints

- Cargo commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- Only `crates/factory-engine/src/tracker.rs` changes. No change to the `Tracker` trait, `tickets.rs`, `intake.rs` or `crates/factory-engine/tests/`.
- Error texts, exactly:
  - `gh printed unexpected JSON: expected a labels array`
  - `gh printed unexpected JSON: a label has no name`
  - ``gh printed an unusable issue state: `<state>` ``, where `<state>` is the `serde_json::Value` `Display` of the `state` field, for example `null` or `"DRAFT"`
  - `gh printed unexpected JSON: expected label events`
  - ``gh printed a `<label>` label event without a time``
- A null or missing actor login on the last matching event still gives `None`.

## Review Focus

1. A `parse_view` reply that is an array or a scalar, not an object: a user expects the `expected a labels array` error, not a panic. Pinned by `parse_view_rejects_an_array_reply`.
2. A lowercase `state` such as `"closed"`: a user expects it read as closed, not rejected. Pinned by `parse_view_reads_a_lowercase_state`.
   A merged pull request (`"MERGED"`, which `gh issue view` prints for a PR number) also reads as closed. Pinned by `parse_view_reads_a_merged_state_as_closed`.
3. A labeled event for another label that has no `created_at`: a user expects it ignored, not an error. Pinned by `parse_actor_ignores_a_time_missing_on_another_label`.
4. Empty `parse_actor` input and `[]`: a user expects `Ok(None)` as today. Already pinned by `parse_actor_is_none_without_a_matching_event`; it must pass with its assertions unchanged.
5. The ticket step and intake on these errors: a user expects a reported failure and a logged skip, as today. Already pinned by `crates/factory-engine/tests/tickets.rs` and `tests/intake.rs`; run them unchanged.

## File Structure

- Modify: `crates/factory-engine/src/tracker.rs`
  - `parse_view` (around lines 306-322) and its doc comment.
  - `parse_issue_list` (around lines 350-379): the `labels` read and the doc comment.
  - `parse_actor` (around lines 382-405) and its doc comment.
  - `mod tests`: the new tests, plus the fixture changes listed in each task.

---

### Task 1: parse_view refuses a reply without labels or a known state

**Files:**
- Modify: `crates/factory-engine/src/tracker.rs` (`parse_view` and `mod tests`)

**Interfaces:**
- Port: behind `Tracker::labels_of` and `Tracker::is_closed` (`GhTracker` in `tracker.rs`).
- Produces: `pub(crate) fn parse_view(json: &str) -> Result<(Vec<String>, bool), String>`, the same signature as today.

**Tests added:** `parse_view_rejects_a_reply_without_labels`, `parse_view_rejects_a_label_without_a_name`, `parse_view_rejects_a_missing_state`, `parse_view_rejects_an_unknown_state`, `parse_view_rejects_an_array_reply`, `parse_view_reads_a_lowercase_state`, `parse_view_reads_a_merged_state_as_closed`.

- [ ] **Step 1: Read the code**

Read `parse_view`, `view_json_gives_labels_and_state` and the `labels_of` and `is_closed` methods of `GhTracker` in `crates/factory-engine/src/tracker.rs`. Read the spec's Design section 1.

- [ ] **Step 2: Write the failing tests**

Add them to `mod tests`, after `view_json_gives_labels_and_state`. Two full examples:

```rust
#[test]
fn parse_view_rejects_a_reply_without_labels() {
    let err = parse_view(r#"{"message":"Not Found"}"#).expect_err("no labels");
    assert_eq!(err, "gh printed unexpected JSON: expected a labels array");
}

#[test]
fn parse_view_rejects_an_unknown_state() {
    let err = parse_view(r#"{"labels":[],"state":"DRAFT"}"#).expect_err("unknown state");
    assert_eq!(err, "gh printed an unusable issue state: `\"DRAFT\"`");
}
```

The others follow the same shapes:

| Test | Input (Rust literal) | Assertion |
|---|---|---|
| `parse_view_rejects_a_label_without_a_name` | `r#"{"labels":[{"id":1}],"state":"OPEN"}"#` | `expect_err` equals `gh printed unexpected JSON: a label has no name` |
| `parse_view_rejects_a_missing_state` | `r#"{"labels":[]}"#` | `expect_err` equals ``gh printed an unusable issue state: `null` `` (no trailing space) |
| `parse_view_rejects_an_array_reply` | `"[]"` | `expect_err` equals `gh printed unexpected JSON: expected a labels array` |
| `parse_view_reads_a_merged_state_as_closed` | `r#"{"labels":[],"state":"MERGED"}"#` | `.expect("parse")` equals `(vec![], true)` |
| `parse_view_reads_a_lowercase_state` | `r#"{"labels":[{"name":"a"}],"state":"closed"}"#` | `.expect("parse")` equals `(vec!["a".to_string()], true)` |

- [ ] **Step 3: Run the tests and see them fail**

Run: `cargo test -j4 -p factory-engine --lib parse_view`
Expected: the build succeeds. The five `rejects` tests fail, because today they return `Ok`, so `expect_err` panics. `parse_view_reads_a_lowercase_state` already passes. `parse_view_reads_a_merged_state_as_closed` also fails, because today `MERGED` reads as open (`false`). 7 tests: 1 passed, 6 failed.

- [ ] **Step 4: Implement**

Keep the signature and the invalid-JSON error.
- When `value["labels"].as_array()` is `None`, return the `expected a labels array` error.
- Map each label to its `name` as a `str`, and return the `a label has no name` error when one lacks it.
- Read `state`: `"CLOSED"` or `"MERGED"` (any case) gives `true`, `"OPEN"` (any case) gives `false`, and any other value returns ``format!("gh printed an unusable issue state: `{}`", value["state"])``.
- Check labels first, then state.
- Update the doc comment to name the three errors.

- [ ] **Step 5: Run the tests and see them pass**

Run: `cargo test -j4 -p factory-engine --lib parse_view`
Expected: 7 passed, 0 failed. Then run `cargo test -j4 -p factory-engine --lib view_json_gives_labels_and_state`, which is expected to pass unchanged.

### Task 2: parse_issue_list skips an entry with unusable labels

**Files:**
- Modify: `crates/factory-engine/src/tracker.rs` (`parse_issue_list` and `mod tests`)

**Interfaces:**
- Port: behind `Tracker::list_labelled` (`GhTracker` in `tracker.rs`).
- Produces: `pub(crate) fn parse_issue_list(json: &str) -> Result<Vec<IssueInfo>, String>`, the same signature as today.

**Tests added:** `parse_issue_list_skips_an_entry_without_labels`, `parse_issue_list_skips_labels_that_are_not_an_array`, `parse_issue_list_skips_a_label_without_a_name`.

- [ ] **Step 1: Read the code**

Read `parse_issue_list`, the helper `issue_numbers`, and every `parse_issue_list_*` test in `crates/factory-engine/src/tracker.rs`. Read the spec's Design section 2.

- [ ] **Step 2: Update the fixtures that lack `labels`**

Real `gh` always prints `labels`, so these existing tests gain `"labels":[]` on every entry they expect to keep. Leave their assertions unchanged.

| Test | New input |
|---|---|
| `parse_issue_list_skips_a_number_over_u32` | `r#"[{"number":4294967296,"title":"Big","labels":[]},{"number":4,"title":"Fix x","labels":[]}]"#` |
| `parse_issue_list_skips_a_string_number` | `r#"[{"number":"7","title":"Str","labels":[]},{"number":4,"title":"Fix x","labels":[]}]"#` |
| `parse_issue_list_skips_an_entry_without_a_number` | `r#"[{"title":"None","labels":[]},{"number":4,"title":"Fix x","labels":[]}]"#` |
| `parse_issue_list_skips_a_non_object_element` | `r#"[1,{"number":4,"title":"Fix x","labels":[]}]"#` |
| `parse_issue_list_keeps_u32_max` | `r#"[{"number":4294967295,"title":"Max","labels":[]}]"#` |

- [ ] **Step 3: Write the failing tests**

Add them after `parse_issue_list_keeps_u32_max`. One full example:

```rust
#[test]
fn parse_issue_list_skips_an_entry_without_labels() {
    let json = r#"[{"number":3,"title":"No labels"},{"number":4,"title":"Fix x","labels":[]}]"#;
    assert_eq!(issue_numbers(json), vec![4]);
}
```

| Test | Input (Rust literal) | Assertion |
|---|---|---|
| `parse_issue_list_skips_labels_that_are_not_an_array` | `r#"[{"number":3,"title":"Str","labels":"x"},{"number":4,"title":"Fix x","labels":[]}]"#` | `issue_numbers(json)` equals `vec![4]` |
| `parse_issue_list_skips_a_label_without_a_name` | `r#"[{"number":3,"title":"Id","labels":[{"id":1}]},{"number":4,"title":"Fix x","labels":[]}]"#` | `issue_numbers(json)` equals `vec![4]` |

- [ ] **Step 4: Run the tests and see them fail**

Run: `cargo test -j4 -p factory-engine --lib parse_issue_list`
Expected: the build succeeds. The three new tests fail; each assertion shows `left: [3, 4]` and `right: [4]`. The 11 existing `parse_issue_list_*` tests pass (14 tests, 11 passed, 3 failed).

- [ ] **Step 5: Implement**

In the `filter_map` of `parse_issue_list`, read `labels` with `issue["labels"].as_array()?`. Map each element to its `name` as a `str`, collected as an `Option<Vec<String>>`, and return `None` for the entry when any element lacks a string `name`. Leave the other fields as they are. Update the doc comment to say an entry whose `labels` is missing, not an array, or holds a label without a name is skipped.

- [ ] **Step 6: Run the tests and see them pass**

Run: `cargo test -j4 -p factory-engine --lib parse_issue_list`
Expected: 14 passed, 0 failed.

### Task 3: parse_actor refuses values that are not events and events without a time

**Files:**
- Modify: `crates/factory-engine/src/tracker.rs` (`parse_actor` and `mod tests`)

**Interfaces:**
- Port: behind `Tracker::labelled_by` (`GhTracker` in `tracker.rs`).
- Produces: `pub(crate) fn parse_actor(json: &str, label: &str) -> Result<Option<LabelEvent>, String>`, the same signature as today.

**Tests added:** `parse_actor_rejects_a_stray_object`, `parse_actor_rejects_a_non_event_element`, `parse_actor_rejects_a_scalar_value`, `parse_actor_rejects_a_matching_event_without_a_time`, `parse_actor_ignores_a_time_missing_on_another_label`.

- [ ] **Step 1: Read the code**

Read `parse_actor`, `EVENTS_JQ`, the fixtures `EVENTS` and `LABELED`, the helper `login_of` and every `parse_actor_*` test in `crates/factory-engine/src/tracker.rs`, and `eligible` in `crates/factory-engine/src/intake.rs`. Read the spec's Design section 3.

- [ ] **Step 2: Update the fixtures that lack `created_at`**

Real events always carry `created_at`.
- In `EVENTS`, add a distinct `"created_at"` to each of the three `labeled` events, in time order: `"2026-11-01T10:00:00Z"` for mallory, `"2026-11-01T11:00:00Z"` for bob and `"2026-11-01T13:00:00Z"` for alice.
- In `parse_actor_reads_paginated_pages`, add `"created_at":"2026-11-02T10:00:00Z"` to the erin event.

Leave the assertions unchanged.

- [ ] **Step 3: Write the failing tests**

Add them after `parse_actor_reads_compact_json_lines`. Two full examples:

```rust
#[test]
fn parse_actor_rejects_a_stray_object() {
    let err = parse_actor(r#"{"message":"Not Found"}"#, "factory").expect_err("not an event");
    assert_eq!(err, "gh printed unexpected JSON: expected label events");
}

#[test]
fn parse_actor_rejects_a_matching_event_without_a_time() {
    let json = r#"{"event":"labeled","label":{"name":"factory"},"actor":{"login":"alice"}}"#;
    let err = parse_actor(json, "factory").expect_err("no time");
    assert_eq!(err, "gh printed a `factory` label event without a time");
}
```

| Test | Input (Rust literal), label | Assertion |
|---|---|---|
| `parse_actor_rejects_a_non_event_element` | `"[1]"`, `"factory"` | `expect_err` equals `gh printed unexpected JSON: expected label events` |
| `parse_actor_rejects_a_scalar_value` | `"42"`, `"factory"` | `expect_err` equals `gh printed unexpected JSON: expected label events` |
| `parse_actor_ignores_a_time_missing_on_another_label` | `format!("{LABELED}\n{}", r#"{"event":"labeled","label":{"name":"bug"},"actor":{"login":"bob"}}"#)`, `"factory"` | `.expect("parse")` equals `Some(LabelEvent { actor: "alice".to_string(), at: "2026-11-01T10:00:00Z".to_string() })` |

- [ ] **Step 4: Run the tests and see them fail**

Run: `cargo test -j4 -p factory-engine --lib parse_actor`
Expected: the build succeeds. The four `rejects` tests fail, because today they return `Ok(None)` or `Ok(Some(..))`, so `expect_err` panics. `parse_actor_ignores_a_time_missing_on_another_label` and the 6 existing tests pass (11 tests, 7 passed, 4 failed).

- [ ] **Step 5: Implement**

Keep the signature, the streaming `Deserializer` loop and the invalid-JSON error.
- For each top-level value, the events are the array's elements when it is an array, or the value itself when it is an object. Any other value returns the `expected label events` error.
- Each event must be an object with a string `event`. Otherwise return the same error.
- For a `labeled` event whose `label.name` equals `label`, read `created_at` as a `str`. When it is missing, return ``format!("gh printed a `{label}` label event without a time")``.
- Keep `last` as today: a null or missing actor login on it gives `None`.
- Update the doc comment to name both errors.

- [ ] **Step 6: Run the tests and see them pass**

Run: `cargo test -j4 -p factory-engine --lib parse_actor`
Expected: 11 passed, 0 failed.

- [ ] **Step 7: Run the whole tracker module and the callers' tests**

Run: `cargo test -j4 -p factory-engine --lib tracker`, then `cargo test -j4 -p factory-engine --test tickets`, `cargo test -j4 -p factory-engine --test intake` and `cargo test -j4 -p factory-engine --test log_events`.
Expected: all pass, with no change to `tests/`.

- [ ] **Step 8: Format and lint**

Run: `cargo fmt -p factory-engine` then `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`
Expected: clippy finishes with no warnings.

Next plan: docs/superpowers/plans/2026-11-22-gh-json-checks/plan-02-gh-json-checks.md
