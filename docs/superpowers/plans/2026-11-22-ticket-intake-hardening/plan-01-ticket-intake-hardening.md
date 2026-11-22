# Issue refs skip code, intake follow-ups cite the intake issue Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-22-ticket-intake-hardening-design.md
> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Issue refs inside code never count. When intake files an intake job's follow-ups, they take their labels and parent only from the intake issue.

**Commit:** fix(tickets): skip code refs, cite intake issue

**Architecture:** Two changes:
- `strip_code` in `crates/factory-core/src/commitmsg.rs` blanks code before every ref parser.
- `stage_follow_ups` in `crates/factory-engine/src/tickets.rs` reads an intake job's parent from `job.intake_issue`.

The ticket logic sits behind the `Tracker` port, and the tests use `FakeTracker`.

**Tech Stack:** Rust.

## Global Constraints

- Cargo commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- No change to the `Tracker` trait, `GhTracker`, `tickets.json`, the board JSON or the dashboard.
- The ticket step never changes a merge result.

## Review Focus

1. A lone backtick in a feature text (``a ` b #7``): a user expects `#7` still read. Pinned by `a_lone_backtick_does_not_hide_a_ref`.
2. An unclosed fence: a user expects everything after it treated as code, as `unfenced_lines` does. Pinned by `refs_skip_an_unclosed_fence`.
3. Commit footers: a user expects `parse_issues` to skip code too, since it shares the parser. Pinned by `parse_issues_skips_code_too`.
4. `factory tickets` on an intake job: a user expects it still skipped. Already pinned by `the_ticket_step_skips_an_intake_job` in `crates/factory-engine/tests/tickets.rs`, which must pass unchanged.
5. A non-intake job: a user expects parents from the feature text, as today. Already pinned by `files_one_ticket_per_follow_up_comments_once_and_closes`, which must pass unchanged.

## File Structure

- Modify: `crates/factory-core/src/commitmsg.rs`. Add `strip_code` and use it in `plain_refs` and `shorthand_refs`, with tests in `mod tests`.
- Modify: `crates/factory-engine/src/tickets.rs`. Add the intake parent rule in `stage_follow_ups`.
- Modify: `crates/factory-engine/tests/tickets.rs`. Add two tests.

---

### Task 1: issue refs inside code are not refs

**Files:**
- Modify: `crates/factory-core/src/commitmsg.rs`

**Interfaces:**
- Port: pure parsing in factory-core. Its callers are the ticket step (`parse_issue_refs`) and the commit footers (`parse_issues`).
- Produces: `fn strip_code(text: &str) -> String`, private. `pub fn parse_issue_refs(text: &str) -> Vec<IssueRef>` and `pub fn parse_issues(text: &str) -> Vec<u32>` keep their signatures.

**Tests added:** `refs_skip_inline_code_spans`, `refs_skip_fenced_blocks`, `refs_skip_an_unclosed_fence`, `a_lone_backtick_does_not_hide_a_ref`, `urls_and_shorthand_in_code_are_not_refs`, `parse_issues_skips_code_too`.

- [ ] **Step 1: Read the code**

Read `hash_refs`, `shorthand_refs`, `url_refs`, `plain_refs`, `parse_issue_refs`, `parse_issues` and `unfenced_lines` in `crates/factory-core/src/commitmsg.rs`, and its `mod tests`. Read the spec's Design section 1.

- [ ] **Step 2: Write the failing tests**

Add them to `mod tests` in `commitmsg.rs`. One full example:

```rust
#[test]
fn refs_skip_inline_code_spans() {
    let refs = parse_issue_refs("fix `#42` and #7");
    assert_eq!(refs, vec![IssueRef { number: 7, repo: None }]);
}
```

| Test | Input (Rust literal) | Assertion |
|---|---|---|
| `refs_skip_fenced_blocks` | `"text #7\n```\n#42\nhttps://github.com/o/r/issues/5\n```\n"` | `parse_issue_refs` equals `vec![IssueRef { number: 7, repo: None }]` |
| `refs_skip_an_unclosed_fence` | `"#7\n```\n#42\n"` | `parse_issue_refs` equals `vec![IssueRef { number: 7, repo: None }]` |
| `a_lone_backtick_does_not_hide_a_ref` | ``"a ` b #7"`` | `parse_issue_refs` equals `vec![IssueRef { number: 7, repo: None }]` |
| `urls_and_shorthand_in_code_are_not_refs` | ``"`o/r#5` and `https://github.com/o/r/issues/6`"`` | `parse_issue_refs` is empty |
| `parse_issues_skips_code_too` | ``"Closes `#42`, #7"`` | `parse_issues` equals `vec![7]` |

- [ ] **Step 3: Run them and see them fail**

Run: `cargo test -j4 -p factory-core commitmsg`
Expected: the build succeeds. Five tests fail, because today the code refs are returned: `refs_skip_inline_code_spans`, `refs_skip_fenced_blocks`, `refs_skip_an_unclosed_fence`, `urls_and_shorthand_in_code_are_not_refs` and `parse_issues_skips_code_too`. `a_lone_backtick_does_not_hide_a_ref` passes, and every existing `commitmsg` test passes.

- [ ] **Step 4: Implement `strip_code`**

- Walk the lines. A line whose trimmed start begins with three backticks flips the fence state and becomes an empty line, and so does every line inside a fence. An unclosed fence runs to the end.
- On a line outside a fence, replace each span from a backtick to the next backtick on the same line with one space. A backtick with no closing partner on that line stays as text.
- Join the lines with `\n`.
- In `plain_refs` and `shorthand_refs`, parse `strip_code(text)` instead of `text`. Leave `hash_refs`, `url_refs` and `number_after_hash` unchanged.
- Give `strip_code` a one-line doc comment, and update the doc comments of `parse_issue_refs` and `parse_issues` to say that refs in code do not count.

- [ ] **Step 5: Run them and see them pass**

Run: `cargo test -j4 -p factory-core commitmsg`
Expected: all pass, 0 failed.

### Task 2: intake follow-ups take only the intake issue as parent

**Files:**
- Modify: `crates/factory-engine/src/tickets.rs`
- Modify: `crates/factory-engine/tests/tickets.rs`

**Interfaces:**
- Port: behind `Tracker` (`labels_of`, `create`). Tests use `FakeTracker` from `crates/factory-engine/tests/common/mod.rs`.
- Consumes: `Job.intake_issue: Option<u32>`, and `parse_issue_refs`, which now skips code.
- Produces: no new public items. `pub fn file_follow_ups(driver, id, tracker, dry_run) -> Result<FiledFollowUps, EngineError>` and `pub fn tickets_step` keep their signatures.

**Tests added:** `a_ref_inside_inline_code_is_not_closed`, `intake_follow_ups_take_only_the_intake_issue_as_parent`.

- [ ] **Step 1: Read the code**

Read these in `crates/factory-engine/src/tickets.rs`:
- `stage_follow_ups`: the `skip_intake` check and the parent selection, around lines 216-262;
- the `tickets_step` and `file_follow_ups` entry points, and which `skip_intake` each passes.

Read `issue_feature` and the `file_follow_ups` call in `crates/factory-engine/src/intake.rs`. In `crates/factory-engine/tests/tickets.rs`, read the helpers `merged`, `tracker` and `set_intake_issue`, and the tests `file_follow_ups_files_and_does_not_comment_or_close` and `files_one_ticket_per_follow_up_comments_once_and_closes`. Read `set_feature` and the `FakeState` fields in `crates/factory-engine/tests/common/mod.rs`. Read the spec's Design section 3.

- [ ] **Step 2: Write the tests**

```rust
#[test]
fn intake_follow_ups_take_only_the_intake_issue_as_parent() {
    let t = tracker();
    let fx = merged(Some(SPEC_WITH_TWO), &t);
    set_feature(&fx, "Fix https://github.com/o/r/issues/5 crash, #76", None);
    set_intake_issue(&fx, 76);
    let out = file_follow_ups(&fx.driver, &fx.id, &t, false).expect("file");
    assert!(!out.report.failed && !out.report.skipped, "{:?}", out.report);
    assert_eq!(t.count("labels_of 5"), 0);
    assert_eq!(t.count("labels_of 76"), 1);
    let s = t.state.lock().expect("lock");
    assert!(
        s.created.iter().all(|c| c.1.contains("#76") && !c.1.contains("#5")),
        "{:?}",
        s.created
    );
}
```

| Test | Setup | Assertion |
|---|---|---|
| `a_ref_inside_inline_code_is_not_closed` | `merged(Some(SPEC_NONE), &t)`, then `set_feature(&fx, "Fix the parser for `#42`, #76", None)` (the backticks are part of the Rust string), then `tickets_step(&fx.driver, &fx.id, false).expect("step")` | report not failed; `t.count("comment 42") == 0`; `t.count("close 42") == 0`; `t.count("close 76") == 1` |

Before using them, check the `FakeTracker` call names `labels_of {n}`, `comment {n}` and `close {n}`. Also check that `FakeState.created` entries are `(title, body, labels)`. Both are in `crates/factory-engine/tests/common/mod.rs`.

- [ ] **Step 3: Run them**

Run: `cargo test -j4 -p factory-engine --test tickets -- a_ref_inside_inline_code_is_not_closed intake_follow_ups_take_only_the_intake_issue_as_parent`
Expected:
- `a_ref_inside_inline_code_is_not_closed` passes, because the parser now skips code.
- `intake_follow_ups_take_only_the_intake_issue_as_parent` fails on `labels_of 5`: today #5 is a parent.

- [ ] **Step 4: Implement**

In `stage_follow_ups`, after the `skip_intake` check:
- When `job.intake_issue` is `Some(n)`, the refs are `vec![IssueRef { number: n, repo: None }]`.
- Otherwise they come from `parse_issue_refs(&job.feature)`, as today.
- Import `IssueRef` from `factory_core`.
- Add a comment saying why: an intake title can carry a URL to another issue.

- [ ] **Step 5: Run the files**

Run: `cargo test -j4 -p factory-engine --test tickets`, then `cargo test -j4 -p factory-engine --test intake`.
Expected: all pass, 0 failed.

- [ ] **Step 6: Format and lint**

Run: `cargo fmt -p factory-core` and `cargo fmt -p factory-engine`, then `cargo clippy -j4 -p factory-core --all-targets -- -D warnings` and `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`.
Expected: clippy finishes with no warnings.

Next plan: docs/superpowers/plans/2026-11-22-ticket-intake-hardening/plan-02-ticket-intake-hardening.md
