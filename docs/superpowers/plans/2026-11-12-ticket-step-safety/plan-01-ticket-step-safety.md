# Ticket step safety, foundations Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-12-ticket-step-safety-design.md
> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Narrow the issue-ref parser, make follow-up titles unique, and add the `Tracker::find_created` port method, so the ticket step can recover and adopt an issue it already filed.

**Architecture:** Three independent building blocks, each with its own tests. The ref parser lives in `factory-core`. Unique titles live in `follow_ups`. The new port method sits on the existing `Tracker` trait, with the `gh` adapter and the test fakes. The step itself changes in the next plan.

**Tech Stack:** Rust, `serde_json`, the `gh` CLI behind the `Tracker` port.

## Global Constraints

- Commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`. Never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Only `git` may not be shelled out to. `gh` stays behind the `Tracker` port in `GhTracker`.
- No `Cargo.toml` change and no new crate dependency.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- Comments are short full sentences that end with a punctuation mark.
- `tickets.json` and board JSON stay backward compatible; this plan adds no field to either.

## Review Focus

1. A lone `#`, `#0`, and a `#` at the end of the text give no ref. Pinned by `hash_ref_alone_or_zero_is_not_a_ref`.
2. A ref in brackets or after punctuation, as in `(#12)` and `[#12]`, still counts. Pinned by `hash_ref_may_end_at_punctuation`.
3. A repeated title of exactly 100 characters keeps its suffixed twin within 100 characters. Pinned by `a_suffixed_title_stays_within_100_chars`.
4. A multi-byte title is cut on a char boundary and never panics. Pinned by `a_multibyte_title_is_cut_on_a_char_boundary`.
5. A `gh issue list` entry with `"body": null` is read as an empty body and never panics. Pinned by `parse_found_treats_a_null_body_as_empty`.

## File Structure

| File | Responsibility |
|---|---|
| `crates/factory-core/src/commitmsg.rs` | Issue-ref parsing: boundary rule and `owner/repo#N` shorthand |
| `crates/factory-engine/src/followups.rs` | Follow-up titles, now unique |
| `crates/factory-engine/src/tracker.rs` | `Tracker::find_created`, `find_args`, `parse_found`, the `gh` adapter |
| `crates/factory-engine/tests/common/mod.rs` | `FakeState.found`, `FakeTracker::with_found`, `FakeTracker::find_created` |
| `crates/factory-cli/tests/new_to_merged.rs`, `crates/factory-cli/tests/tickets.rs` | The other two `Tracker` fakes get the new method |

### Task 1: Narrow issue-ref parsing

**Files:**
- Modify: `crates/factory-core/src/commitmsg.rs` (`hash_refs` at ~86, `parse_issue_refs` at ~134, the `tests` module at ~345)

**Interfaces:**
- Consumes: the existing `IssueRef { number: u32, repo: Option<String> }`, `is_repo_name(&str) -> bool` and `GITHUB_PREFIX` in the same file.
- Produces: `parse_issue_refs(text: &str) -> Vec<IssueRef>` keeps its signature and now also returns shorthand refs; a new private `fn shorthand_refs(text: &str) -> Vec<IssueRef>`. `parse_issues` keeps its signature and behaviour.

- [ ] **Step 1: Read the code and docs this task touches.** Read `crates/factory-core/src/commitmsg.rs` lines 68 to 151 and the existing ref tests (`refs_read_hash_numbers_and_urls`, `pull_request_url_is_not_an_issue`, `refs_are_sorted_and_deduplicated`, `parse_issues_keeps_hash_behavior_and_adds_url_numbers`, `parse_issues_reads_hash_numbers_only`). Read the "Design, 1" section of the spec. Note where `parse_issue_refs` is called: `crates/factory-engine/src/tickets.rs` and, through `parse_issues`, `runloop.rs` and `one_job.rs`.
- [ ] **Step 2: Write the failing tests** in the `tests` module of `commitmsg.rs`. Build `IssueRef` values directly.

```rust
#[test]
fn hash_ref_needs_a_boundary_after_the_digits() {
    assert!(parse_issue_refs("fix #1a2b3c now").is_empty());
    assert!(parse_issue_refs("see #123abc").is_empty());
    assert!(parse_issue_refs("see #12_x").is_empty());
}

#[test]
fn hash_ref_may_end_at_punctuation() {
    for text in ["#12.", "(#12)", "#12,", "[#12]", "done #12", "#12"] {
        let refs = parse_issue_refs(text);
        assert_eq!(refs, vec![IssueRef { number: 12, repo: None }], "{text}");
    }
}

#[test]
fn hash_ref_alone_or_zero_is_not_a_ref() {
    for text in ["#", "#0", "x #", "# 12"] {
        assert!(parse_issue_refs(text).is_empty(), "{text}");
    }
}

#[test]
fn owner_repo_shorthand_carries_the_repo() {
    let refs = parse_issue_refs("fix other/repo#5 later");
    assert_eq!(refs, vec![IssueRef { number: 5, repo: Some("other/repo".to_string()) }]);
    let refs = parse_issue_refs("Closes o/r#7.");
    assert_eq!(refs, vec![IssueRef { number: 7, repo: Some("o/r".to_string()) }]);
}

#[test]
fn shorthand_inside_a_longer_path_is_not_a_ref() {
    assert!(parse_issue_refs("x.com/a/b#12").is_empty());
    assert!(parse_issue_refs("/a/b#12").is_empty());
}
```

- [ ] **Step 3: Run them and see them fail.** Run `cargo test -j4 -p factory-core commitmsg`. Expected: `hash_ref_needs_a_boundary_after_the_digits` and `owner_repo_shorthand_carries_the_repo` fail; the others may already pass.
- [ ] **Step 4: Implement the boundary rule in `hash_refs`.** After reading the digits, the char at `i + 1 + digits.len()` must be absent, or neither alphanumeric nor `_`; otherwise skip this `#`.
- [ ] **Step 5: Implement `fn shorthand_refs(text: &str) -> Vec<IssueRef>`** and extend the list in `parse_issue_refs` with it before the sort and dedup. For each `#` that follows a repo-name char, scan back over repo-name chars to a `/`, then back over repo-name chars to the owner start. Accept only when both parts are non-empty and pass `is_repo_name`, and the char before the owner is absent or is neither a repo-name char nor `/`. Read the digits after the `#` with the same boundary rule as step 4 and the same `number >= 1` check. Build `IssueRef { number, repo: Some(format!("{owner}/{repo}")) }`. Scan on a `Vec<char>` like `hash_refs` does.
- [ ] **Step 6: Run the tests and see them pass.** Run `cargo test -j4 -p factory-core` and `cargo clippy -j4 -p factory-core`. Expected: all pass and clippy is clean. If an older test fails only because its input has digits followed by a letter, fix that input and keep the test's intent.

### Task 2: Unique follow-up titles

**Files:**
- Modify: `crates/factory-engine/src/followups.rs` (`follow_ups` at ~49, the `tests` module at ~69)

**Interfaces:**
- Consumes: the existing `FollowUp { title: String, body: String }`, `MAX_TITLE_CHARS` (100) and `title_of(&str) -> String` in the same file.
- Produces: `follow_ups(spec: &str) -> Vec<FollowUp>` keeps its signature, and the titles it returns are now pairwise different; a new private `fn unique_titles(items: Vec<FollowUp>) -> Vec<FollowUp>`. This is pure logic and sits behind no port.

- [ ] **Step 1: Read the code this task touches.** Read all of `crates/factory-engine/src/followups.rs` and the "Design, 3" section of the spec. Read how `tickets_step` uses `item.title` in `crates/factory-engine/src/tickets.rs` lines 159 to 221, so you know every later lookup relies on a unique title.
- [ ] **Step 2: Write the failing tests** in the `tests` module of `followups.rs`.

```rust
#[test]
fn repeated_titles_get_a_numbered_suffix() {
    let spec = "## Follow-ups\n- Same thing. First detail.\n- Same thing. Second detail.\n- Same thing\n";
    let titles: Vec<String> = follow_ups(spec).into_iter().map(|f| f.title).collect();
    assert_eq!(titles, vec!["Same thing", "Same thing (2)", "Same thing (3)"]);
}

#[test]
fn a_suffix_never_collides_with_another_title() {
    let spec = "## Follow-ups\n- Same\n- Same (2)\n- Same\n";
    let titles: Vec<String> = follow_ups(spec).into_iter().map(|f| f.title).collect();
    assert_eq!(titles, vec!["Same", "Same (2)", "Same (3)"]);
}

#[test]
fn a_suffixed_title_stays_within_100_chars() {
    let long = "x".repeat(100);
    let spec = format!("## Follow-ups\n- {long}\n- {long}\n");
    let titles: Vec<String> = follow_ups(&spec).into_iter().map(|f| f.title).collect();
    assert_eq!(titles[0], long);
    assert_eq!(titles[1].chars().count(), 100);
    assert!(titles[1].ends_with(" (2)"), "{}", titles[1]);
}

#[test]
fn a_multibyte_title_is_cut_on_a_char_boundary() {
    let long = "é".repeat(100);
    let spec = format!("## Follow-ups\n- {long}\n- {long}\n");
    let titles: Vec<String> = follow_ups(&spec).into_iter().map(|f| f.title).collect();
    assert_eq!(titles[1].chars().count(), 100);
    assert!(titles[1].starts_with(&"é".repeat(96)), "{}", titles[1]);
    assert!(titles[1].ends_with(" (2)"), "{}", titles[1]);
}

#[test]
fn the_body_of_a_suffixed_item_is_unchanged() {
    let spec = "## Follow-ups\n- Same. One.\n- Same. Two.\n";
    let items = follow_ups(spec);
    assert_eq!(items[1].body, "Same. Two.");
}
```

- [ ] **Step 3: Run them and see them fail.** Run `cargo test -j4 -p factory-engine followups`. Expected: the first four fail on the title assertions; the body test passes.
- [ ] **Step 4: Implement `fn unique_titles(items: Vec<FollowUp>) -> Vec<FollowUp>`** and call it as the last step of `follow_ups`. Walk the items in order and keep the set of titles used so far. A title not yet used stays. Otherwise try `N = 2, 3, ...` and take the first `"{base} ({N})"` not in the set, where `base` is the original title cut to `MAX_TITLE_CHARS - suffix_len` chars (count chars, not bytes) and trimmed at the end. Only the title changes, never the body.
- [ ] **Step 5: Run the tests and see them pass.** Run `cargo test -j4 -p factory-engine followups` and `cargo clippy -j4 -p factory-engine`. Expected: all `followups` tests pass and clippy is clean.

### Task 3: The `Tracker::find_created` port method

**Files:**
- Modify: `crates/factory-engine/src/tracker.rs` (trait at ~12, the `Arc<T>` impl at ~23, helpers near `create_args` at ~57, `GhTracker` impl at ~157, the `tests` module)
- Modify: `crates/factory-engine/tests/common/mod.rs` (`FakeState` at ~679, `FakeTracker` at ~693 and its `Tracker` impl at ~749)
- Modify: `crates/factory-cli/tests/new_to_merged.rs` (`CountCreates` at ~926)
- Modify: `crates/factory-cli/tests/tickets.rs` (`NoCalls` at ~8)

**Interfaces:**
- Consumes: the existing `GhTracker::run(&self, args: &[String]) -> Result<String, String>` and the `Tracker` trait.
- Produces: a trait method `fn find_created(&self, title: &str, footer: &str) -> Result<Option<u32>, String>` that reads only; `pub(crate) fn find_args() -> Vec<String>`; `pub(crate) fn parse_found(json: &str, title: &str, footer: &str) -> Result<Option<u32>, String>`; `FakeState.found: HashMap<String, u32>`; `FakeTracker::with_found(self, title: &str, number: u32) -> FakeTracker`. The logic sits behind the existing `Tracker` port. The next plan calls `find_created` from `tickets_step`.

- [ ] **Step 1: Read the code this task touches.** Read `crates/factory-engine/src/tracker.rs` in full, the `FakeState` and `FakeTracker` code in `crates/factory-engine/tests/common/mod.rs` from line 679 to 790, and the "Design, 2" section of the spec.
- [ ] **Step 2: Write the failing unit tests** in the `tests` module of `tracker.rs`.

```rust
#[test]
fn find_args_list_all_states() {
    let expected: Vec<String> = [
        "issue", "list", "--state", "all", "--limit", "100", "--json", "number,title,body",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(find_args(), expected);
}

const FOOTER: &str = "Filed by factory job 007 (slug)";

#[test]
fn parse_found_picks_the_lowest_number_with_title_and_footer() {
    let json = r#"[
      {"number":9,"title":"Fix x","body":"b\n\nFiled by factory job 007 (slug) from the follow-ups of #1."},
      {"number":4,"title":"Fix x","body":"Filed by factory job 007 (slug) from the follow-ups of #1."},
      {"number":2,"title":"Other","body":"Filed by factory job 007 (slug)"}
    ]"#;
    assert_eq!(parse_found(json, "Fix x", FOOTER).expect("parse"), Some(4));
}

#[test]
fn parse_found_ignores_a_title_without_the_footer() {
    let json = r#"[{"number":3,"title":"Fix x","body":"unrelated"}]"#;
    assert_eq!(parse_found(json, "Fix x", FOOTER).expect("parse"), None);
}

#[test]
fn parse_found_treats_a_null_body_as_empty() {
    let json = r#"[{"number":3,"title":"Fix x","body":null}]"#;
    assert_eq!(parse_found(json, "Fix x", FOOTER).expect("parse"), None);
}

#[test]
fn parse_found_rejects_invalid_json() {
    let err = parse_found("nope", "Fix x", FOOTER).expect_err("invalid");
    assert!(err.contains("invalid JSON"), "{err}");
}
```

- [ ] **Step 3: Run them and see them fail.** Run `cargo test -j4 -p factory-engine tracker`. Expected: a compile error such as `cannot find function 'find_args' in this scope`.
- [ ] **Step 4: Implement the port method and the adapter.** Add `find_created` to the `Tracker` trait with a doc comment that says it reads only and returns the lowest matching number. Forward it in the `Arc<T>` impl. Add `find_args()` with the exact arguments in the test. Add `parse_found`: parse the JSON array (error text starts with `gh printed invalid JSON:` like `parse_view`), keep entries whose `title` equals `title` exactly and whose `body` (a missing or null body is an empty string) contains `footer`, and return the smallest `number` as `Some`, else `None`. Implement `GhTracker::find_created` as `parse_found(&self.run(&find_args())?, title, footer)`.
- [ ] **Step 5: Add the method to the three test fakes.** In `tests/common/mod.rs` add `pub found: HashMap<String, u32>` to `FakeState`, a builder `with_found` like `with_labels`, and `FakeTracker::find_created` that records the call `find_created {title}` through `enter(.., "find_created")` (so `fail_on("find_created")` works) and returns `found.get(title).copied()`. In `crates/factory-cli/tests/tickets.rs` make `NoCalls::find_created` panic with `find_created was called`, like its siblings. In `crates/factory-cli/tests/new_to_merged.rs` make `CountCreates::find_created` return `Ok(None)`.
- [ ] **Step 6: Run the tests and see them pass.** Run `cargo test -j4 -p factory-engine tracker`, then `cargo test -j4 -p factory-engine --test tickets` and `cargo test -j4 -p factory-cli --test tickets --test new_to_merged`. Expected: the five new unit tests pass and every existing test in those files still passes. Run `cargo clippy -j4 -p factory-engine -p factory-cli`.

Next plan: docs/superpowers/plans/2026-11-12-ticket-step-safety/plan-02-ticket-step-safety.md
