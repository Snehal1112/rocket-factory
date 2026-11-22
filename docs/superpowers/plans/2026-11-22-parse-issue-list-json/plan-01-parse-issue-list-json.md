# parse_issue_list JSON checks Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-22-parse-issue-list-json-design.md
> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** `parse_issue_list` returns an error for `gh` JSON that is not an array, so intake reports a broken `gh` reply instead of reading it as an empty queue.

**Commit:** fix(tracker): reject non-array gh JSON in intake

**Architecture:** One function changes: `parse_issue_list` in `crates/factory-engine/src/tracker.rs`, the parser behind `GhTracker::list_labelled` (the `Tracker` port). Intake already maps a `list_labelled` error to `intake: cannot list the labelled issues: <e>` and leaves every label alone, so no caller changes.

**Tech Stack:** Rust, `serde_json::Value`.

## Global Constraints

- Cargo commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- Error text, exactly: `gh printed unexpected JSON: expected an array` (the same text `parse_found` returns).
- No change to the `Tracker` trait, `GhTracker::list_labelled`, `list_labelled_args` or `crates/factory-engine/src/intake.rs`.
- An entry without a usable number is still skipped, not an error.

## Review Focus

1. A JSON string reply (`"oops"`), not only an object or `null`: a user expects the same `expected an array` error. Pinned by `parse_issue_list_rejects_a_string_reply`.
2. A string number (`"7"`) next to a good entry: a user expects that entry skipped and the good one kept, not an error. Pinned by `parse_issue_list_skips_a_string_number`.
3. An array element that is not an object (`[1, {...#4...}]`): a user expects it skipped and only `#4` returned. Pinned by `parse_issue_list_skips_a_non_object_element`.
4. A number of exactly `u32::MAX`: a user expects it kept, not dropped as too large. Pinned by `parse_issue_list_keeps_u32_max`.
5. Intake on a parse error: a user expects the pass to fail with no label change. Already pinned by `a_tracker_failure_before_the_claim_leaves_the_labels_alone` in `crates/factory-engine/tests/intake.rs`; run it unchanged.

## File Structure

- Modify: `crates/factory-engine/src/tracker.rs`
  - `parse_issue_list` (around lines 350-378): the non-array check and its doc comment.
  - `mod tests`: new unit tests after `parse_issue_list_reads_labels_and_dates` (around line 786).

---

### Task 1: parse_issue_list refuses non-array JSON

**Files:**
- Modify: `crates/factory-engine/src/tracker.rs` (`parse_issue_list` and `mod tests`)
- Test: `crates/factory-engine/src/tracker.rs` `mod tests`

**Interfaces:**
- Port: the logic sits behind the `Tracker` port (`fn list_labelled(&self, label: &str) -> Result<Vec<IssueInfo>, String>`, implemented by `GhTracker` at `tracker.rs:529`).
- Consumes: `IssueInfo { number: u32, title: String, body: String, labels: Vec<String>, created_at: String, updated_at: String }` (`tracker.rs:41`), unchanged.
- Produces: `pub(crate) fn parse_issue_list(json: &str) -> Result<Vec<IssueInfo>, String>`, same signature as today.

**Tests added:** `parse_issue_list_rejects_an_object_reply`, `parse_issue_list_rejects_a_null_reply`, `parse_issue_list_rejects_a_string_reply`, `parse_issue_list_accepts_an_empty_array`, `parse_issue_list_skips_a_number_over_u32`, `parse_issue_list_skips_a_string_number`, `parse_issue_list_skips_an_entry_without_a_number`, `parse_issue_list_skips_a_non_object_element`, `parse_issue_list_keeps_u32_max`.

- [ ] **Step 1: Read the code this task touches**

Read `parse_issue_list`, `parse_found` (the sibling with the same check), `GhTracker::list_labelled`, `const NOT_AN_ARRAY`, `parse_issue_list_reads_labels_and_dates` and `parse_issue_list_reads_updated_at` in `crates/factory-engine/src/tracker.rs`, and `pick_issue` in `crates/factory-engine/src/intake.rs` (around line 201). Read the spec's Design and Error handling sections.

- [ ] **Step 2: Write the tests**

Add to `mod tests` in `crates/factory-engine/src/tracker.rs`, after `parse_issue_list_reads_labels_and_dates`. Two full examples:

```rust
#[test]
fn parse_issue_list_rejects_an_object_reply() {
    let err = parse_issue_list(r#"{"message":"Bad credentials"}"#)
        .expect_err("an object is not a list");
    assert_eq!(err, NOT_AN_ARRAY);
}

#[test]
fn parse_issue_list_skips_a_number_over_u32() {
    let json = r#"[{"number":4294967296,"title":"Big"},{"number":4,"title":"Fix x"}]"#;
    let numbers: Vec<u32> = parse_issue_list(json)
        .expect("parse")
        .iter()
        .map(|i| i.number)
        .collect();
    assert_eq!(numbers, vec![4]);
}
```

The other tests follow one of those two shapes. `NOT_AN_ARRAY` is the existing `const` in `mod tests` (around line 668); reuse it, do not repeat the literal. The JSON inputs below are written as the Rust literals to pass:

| Test | JSON input | Assertion |
|---|---|---|
| `parse_issue_list_rejects_a_null_reply` | `"null"` | `expect_err`, equals `NOT_AN_ARRAY` |
| `parse_issue_list_rejects_a_string_reply` | `r#""oops""#` (the JSON string `"oops"` with its quotes, as `parse_found_rejects_a_string_reply` does) | `expect_err`, equals `NOT_AN_ARRAY` |
| `parse_issue_list_accepts_an_empty_array` | `"[]"` | `.expect("parse")` equals `vec![]` (an empty `Vec<IssueInfo>`) |
| `parse_issue_list_skips_a_string_number` | `r#"[{"number":"7","title":"Str"},{"number":4,"title":"Fix x"}]"#` | numbers equal `vec![4]` |
| `parse_issue_list_skips_an_entry_without_a_number` | `r#"[{"title":"None"},{"number":4,"title":"Fix x"}]"#` | numbers equal `vec![4]` |
| `parse_issue_list_skips_a_non_object_element` | `r#"[1,{"number":4,"title":"Fix x"}]"#` | numbers equal `vec![4]` |
| `parse_issue_list_keeps_u32_max` | `r#"[{"number":4294967295,"title":"Max"}]"#` | numbers equal `vec![u32::MAX]` |

- [ ] **Step 3: Run the tests and see the three reject tests fail**

Run: `cargo test -j4 -p factory-engine parse_issue_list`
Expected: the build succeeds and `parse_issue_list_rejects_an_object_reply`, `parse_issue_list_rejects_a_null_reply` and `parse_issue_list_rejects_a_string_reply` fail, because today they return `Ok([])` and `expect_err` panics. The other six new tests, `parse_issue_list_reads_labels_and_dates` and `parse_issue_list_reads_updated_at` pass, since they pin current behaviour (8 passed, 3 failed).

- [ ] **Step 4: Implement the check in `parse_issue_list`**

Keep the signature `pub(crate) fn parse_issue_list(json: &str) -> Result<Vec<IssueInfo>, String>` and the invalid-JSON error as it is.

- When the parsed value's `as_array()` is `None`, return `Err("gh printed unexpected JSON: expected an array".to_string())`, written the way `parse_found` does it (`.ok_or_else(...)?`).
- Map the array's entries to `IssueInfo` exactly as today, skipping an entry whose number is missing, not an integer or over `u32::MAX`.
- Update the doc comment to say a reply that is not an array is an error and an entry without a usable number is skipped.

- [ ] **Step 5: Run the tests and see them pass**

Run: `cargo test -j4 -p factory-engine parse_issue_list`
Expected: 11 tests pass (`parse_issue_list_reads_labels_and_dates`, `parse_issue_list_reads_updated_at` and the 9 new ones), 0 failed.

- [ ] **Step 6: Run the intake tests**

Run: `cargo test -j4 -p factory-engine --test intake`
Expected: all pass, including `a_tracker_failure_before_the_claim_leaves_the_labels_alone`, with no change to that file.

- [ ] **Step 7: Format and lint**

Run: `cargo fmt -p factory-engine` then `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`
Expected: clippy finishes with no warnings.

Next plan: none
