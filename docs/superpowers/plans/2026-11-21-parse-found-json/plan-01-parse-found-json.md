# parse_found JSON checks Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-21-parse-found-json-design.md
> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** `parse_found` returns an error for `gh` JSON that is not an array and for a matching issue with an unusable number, so the ticket step never files a duplicate.

**Commit:** fix(tracker): reject bad gh JSON in parse_found

**Architecture:** One function changes: `parse_found` in `crates/factory-engine/src/tracker.rs`, the parser behind `GhTracker::find_created` (the `Tracker` port). The ticket step already reports an `Err` from `find_created` and stops without creating, so no caller changes.

**Tech Stack:** Rust, `serde_json::Value`.

## Global Constraints

- Cargo commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- Error texts, exactly: `gh printed unexpected JSON: expected an array` and ``gh printed an unusable number for `<title>` `` (the title between backticks).
- No change to the `Tracker` trait, `GhTracker::find_created`, `find_args` or `crates/factory-engine/src/tickets.rs`.

## Review Focus

1. A JSON string or number reply (`"oops"`, `42`), not only an object or `null`: a user expects the same `expected an array` error. Pinned by `parse_found_rejects_a_string_reply`.
2. A matching entry whose number is a string (`"7"`): a user expects the unusable-number error, not a skip. Pinned by `parse_found_rejects_a_matching_string_number`.
3. Two matching entries, the lower one good and a later one with a bad number: a user expects the error, because the bad one could be the lowest. Pinned by `parse_found_rejects_when_any_match_has_a_bad_number`.
4. An array element that is not an object (`[1, {...matching #4...}]`): a user expects it skipped and `Ok(Some(4))`. Pinned by `parse_found_skips_a_non_object_element`.
5. The ticket step on a parse error: a user expects no `create` call. Already pinned by `a_failed_find_stops_without_creating` in `crates/factory-engine/tests/tickets.rs`; run it unchanged.

## File Structure

- Modify: `crates/factory-engine/src/tracker.rs`
  - `parse_found` (around lines 324-338): the two new checks and its doc comment.
  - `mod tests` (from line 555): new unit tests next to the existing `parse_found_*` tests, using the existing `const FOOTER: &str = "Filed by factory job 007 (slug)"`.

---

### Task 1: parse_found refuses non-array JSON and unusable matching numbers

**Files:**
- Modify: `crates/factory-engine/src/tracker.rs` (`parse_found` and `mod tests`)
- Test: `crates/factory-engine/src/tracker.rs` `mod tests`

**Interfaces:**
- Port: the logic sits behind the `Tracker` port (`fn find_created(&self, title: &str, footer: &str) -> Result<Option<u32>, String>`, implemented by `GhTracker` at `tracker.rs:507`).
- Consumes: nothing new.
- Produces: `pub(crate) fn parse_found(json: &str, title: &str, footer: &str) -> Result<Option<u32>, String>`, same signature as today.

**Tests added:** `parse_found_rejects_an_object_reply`, `parse_found_rejects_a_null_reply`, `parse_found_rejects_a_string_reply`, `parse_found_accepts_an_empty_array`, `parse_found_rejects_a_matching_number_over_u32`, `parse_found_rejects_a_matching_entry_without_a_number`, `parse_found_rejects_a_matching_string_number`, `parse_found_rejects_when_any_match_has_a_bad_number`, `parse_found_skips_an_unrelated_number_over_u32`, `parse_found_skips_a_non_object_element`, `parse_found_keeps_u32_max`.

- [ ] **Step 1: Read the code this task touches**

Read `parse_found`, `GhTracker::find_created` and the existing `parse_found_*` tests in `crates/factory-engine/src/tracker.rs`, and the `Err(e)` branch after `tracker.find_created` in `crates/factory-engine/src/tickets.rs` (around line 335). Read the spec's Design and Error handling sections.

- [ ] **Step 2: Write the failing tests**

Add to `mod tests` in `crates/factory-engine/src/tracker.rs`, after `parse_found_rejects_invalid_json`. Two full examples:

```rust
#[test]
fn parse_found_rejects_an_object_reply() {
    let err = parse_found(r#"{"message":"Bad credentials"}"#, "Fix x", FOOTER)
        .expect_err("an object is not a list");
    assert_eq!(err, "gh printed unexpected JSON: expected an array");
}

#[test]
fn parse_found_rejects_a_matching_number_over_u32() {
    let json = r#"[{"number":4294967296,"title":"Fix x","body":"Filed by factory job 007 (slug)"}]"#;
    let err = parse_found(json, "Fix x", FOOTER).expect_err("number over u32");
    assert_eq!(err, "gh printed an unusable number for `Fix x`");
}
```

The other tests follow one of those two shapes, with title `Fix x` and `FOOTER`:

| Test | JSON input | Assertion |
|---|---|---|
| `parse_found_rejects_a_null_reply` | `null` | `expect_err`, equals `gh printed unexpected JSON: expected an array` |
| `parse_found_rejects_a_string_reply` | `"oops"` | `expect_err`, equals `gh printed unexpected JSON: expected an array` |
| `parse_found_accepts_an_empty_array` | `[]` | `.expect("parse")` equals `None` |
| `parse_found_rejects_a_matching_entry_without_a_number` | `[{"title":"Fix x","body":"Filed by factory job 007 (slug)"}]` | `expect_err`, equals ``gh printed an unusable number for `Fix x` `` |
| `parse_found_rejects_a_matching_string_number` | `[{"number":"7","title":"Fix x","body":"Filed by factory job 007 (slug)"}]` | `expect_err`, equals ``gh printed an unusable number for `Fix x` `` |
| `parse_found_rejects_when_any_match_has_a_bad_number` | `[{"number":4,"title":"Fix x","body":"Filed by factory job 007 (slug)"},{"number":4294967296,"title":"Fix x","body":"Filed by factory job 007 (slug)"}]` | `expect_err`, equals ``gh printed an unusable number for `Fix x` `` |
| `parse_found_skips_an_unrelated_number_over_u32` | `[{"number":4294967296,"title":"Other","body":"Filed by factory job 007 (slug)"},{"number":4,"title":"Fix x","body":"Filed by factory job 007 (slug)"}]` | `.expect("parse")` equals `Some(4)` |
| `parse_found_skips_a_non_object_element` | `[1,{"number":4,"title":"Fix x","body":"Filed by factory job 007 (slug)"}]` | `.expect("parse")` equals `Some(4)` |
| `parse_found_keeps_u32_max` | `[{"number":4294967295,"title":"Fix x","body":"Filed by factory job 007 (slug)"}]` | `.expect("parse")` equals `Some(4294967295)` |

- [ ] **Step 3: Run the tests and see them fail**

Run: `cargo test -j4 -p factory-engine parse_found`
Expected: the build succeeds and these fail, because today they return `Ok(None)`: `parse_found_rejects_an_object_reply`, `parse_found_rejects_a_null_reply`, `parse_found_rejects_a_string_reply`, `parse_found_rejects_a_matching_number_over_u32`, `parse_found_rejects_a_matching_entry_without_a_number`, `parse_found_rejects_a_matching_string_number`, `parse_found_rejects_when_any_match_has_a_bad_number` (`expect_err` panics with `Ok(None)` or `Ok(Some(4))`). The four existing tests and the four new `accepts`/`skips`/`keeps` tests pass.

- [ ] **Step 4: Implement the checks in `parse_found`**

Keep the signature `pub(crate) fn parse_found(json: &str, title: &str, footer: &str) -> Result<Option<u32>, String>` and the invalid-JSON error as it is.

- When the parsed value's `as_array()` is `None`, return `Err("gh printed unexpected JSON: expected an array".to_string())`.
- Walk the array. An entry matches when `issue["title"].as_str() == Some(title)` and `issue["body"].as_str().unwrap_or("")` contains `footer`, as today. Skip entries that do not match.
- For a matching entry, read the number with `issue["number"].as_u64().and_then(|n| u32::try_from(n).ok())`. When that is `None`, return ``Err(format!("gh printed an unusable number for `{title}`"))`` at once.
- Return the lowest number of the matching entries, or `Ok(None)` when none match.
- Update the doc comment to say a non-array reply and a matching entry with a missing, non-integer or over-`u32` number are errors.

- [ ] **Step 5: Run the tests and see them pass**

Run: `cargo test -j4 -p factory-engine parse_found`
Expected: 15 tests pass (the 4 existing `parse_found_*` tests and the 11 new ones), 0 failed.

- [ ] **Step 6: Run the ticket step tests**

Run: `cargo test -j4 -p factory-engine --test tickets`
Expected: all pass, including `a_failed_find_stops_without_creating`, with no change to that file.

- [ ] **Step 7: Format and lint**

Run: `cargo fmt -p factory-engine` then `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`
Expected: clippy finishes with no warnings.

Next plan: none
