# Intake and preset gates, plan 2: gate intake on real edit times

> **Spec:** docs/superpowers/specs/2026-11-23-intake-and-preset-gates-design.md

> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** A freshly labelled issue is no longer skipped for its own label write (#75). The edit gate reads the body's `lastEditedAt` and the last title rename instead of `updatedAt`. A tie with the label time, a failed lookup or an odd time form skips the issue.

**Commit:** fix(intake): gate on real edit times

**Architecture:** A new `Tracker::edited_at` method, with no default body, gets the `gh api graphql` call in `GhTracker` and a pure parser. Every other tracker states its own body. `pick_next` takes an `edited_at` closure, and `eligible` checks it after the trust check.

**Tech Stack:** Rust, `serde_json`, `gh` behind the `Tracker` port.

## Global Constraints

- cargo commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`, never --workspace or --all.
- `gh` only through the `Tracker` port. Never shell out to `git`.
- Deny by default: every failed or odd lookup skips the issue and logs `intake.skip`.
- No new board, ledger or events field.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.

## File Structure

| File | Change |
|---|---|
| `crates/factory-engine/src/tracker.rs` | `edited_at` on the trait (lines 26-37) and in each impl (`:57`, `:93`, `:143`, `GhTracker` `:529`, `Recording` in tests); `edited_at_args`, `parse_edited_at`, `is_github_time`; tests. |
| `crates/factory-engine/src/intake.rs` | `pick_next` (line 133) takes `edited_at`, `eligible` (lines 166-190) uses it, `pick_issue` (lines 201-214) passes it; tests. |
| `crates/factory-engine/tests/common/mod.rs` | `FakeState.edited: HashMap<u32, String>`; `FakeTracker` and `PanicTracker` implement `edited_at`. |
| `crates/factory-cli/tests/tickets.rs`, `intake_cmd.rs`, `new_to_merged.rs` | `edited_at` returning `Ok(None)` in `NoCalls`, `OneIssue` and `CountCreates`. |
| `docs/reference/software-factory.md` | The intake edit-gate sentence names the real edit times. |

## Review Focus

1. The label event's own time in another form (fractional seconds): the user expects a skip, not a wrong compare. Pinned by `pick_next_skips_a_label_time_in_another_form`.
2. A GraphQL reply with `issue: null` (an issue moved or deleted): the user expects an error and a skip. Pinned by `parse_edited_at_rejects_other_json`, which includes that reply.
3. An issue with no body edit and no rename: `edited_at` is `None`, and the issue is picked. Pinned by `parse_edited_at_without_edits_is_none` and `pick_next_picks_an_issue_whose_label_bumped_updated_at`.
4. The edit lookup must not run for an issue whose label actor is untrusted, so no extra `gh` call is made. Pinned by `pick_next_asks_edited_at_only_after_the_trust_check`.

---

### Task 1: Tracker::edited_at

**Files:**
- Modify: `crates/factory-engine/src/tracker.rs`, `crates/factory-engine/tests/common/mod.rs`, `crates/factory-cli/tests/tickets.rs`, `intake_cmd.rs`, `new_to_merged.rs`
- Test: `crates/factory-engine/src/tracker.rs` (`mod tests`)

**Interfaces:**
- Produces:
  - `fn edited_at(&self, issue: u32) -> Result<Option<String>, String>` on `Tracker`, with no default body.
  - `pub(crate) fn edited_at_args(owner: &str, name: &str, issue: u32) -> Vec<String>`.
  - `pub(crate) fn parse_edited_at(json: &str) -> Result<Option<String>, String>`.
  - `pub(crate) fn is_github_time(s: &str) -> bool`.
- Port: `Tracker` (`tracker.rs`), faked by `FakeTracker`.

- [ ] **Step 1: Read.** Read `crates/factory-engine/src/tracker.rs:1-200`, `:268-290`, `:390-440` and `:529-600`, its tests `events_args` (`:864`) and `parse_actor` (about `:1062`), and `crates/factory-engine/tests/common/mod.rs:900-1180`.

- [ ] **Step 2: Write the failing tests:**

```rust
const EDITED: &str = r#"{"data":{"repository":{"issue":{"lastEditedAt":"2026-11-05T10:00:03Z","timelineItems":{"nodes":[{"createdAt":"2026-11-05T10:00:05Z"}]}}}}}"#;

#[test]
fn parse_edited_at_takes_the_later_time() {
    assert_eq!(parse_edited_at(EDITED).expect("parse"), Some("2026-11-05T10:00:05Z".to_string()));
}

#[test]
fn edited_at_args_query_body_edits_and_renames() {
    let args = edited_at_args("o", "r", 7);
    assert_eq!(&args[..2], &strings(&["api", "graphql"])[..]);
    let q = args.iter().find(|a| a.starts_with("query=")).expect("query");
    assert!(q.contains("lastEditedAt") && q.contains("RENAMED_TITLE_EVENT"), "{q}");
    for f in ["owner=o", "name=r", "number=7"] {
        assert!(args.iter().any(|a| a == f), "{f}");
    }
}
```

| Test | Input | Expected |
|---|---|---|
| `parse_edited_at_without_edits_is_none` | `lastEditedAt: null`, `nodes: []` | `Ok(None)` |
| `parse_edited_at_rejects_other_json` | `{"data":{"repository":{"issue":null}}}`, `not json`, `[]` | `Err` each, naming what was expected |
| `parse_edited_at_rejects_a_time_in_another_form` | `lastEditedAt: "2026-11-05T10:00:03.5Z"` | `Err` |
| `is_github_time_takes_only_the_plain_form` | `2026-11-05T10:00:03Z` true; `2026-11-05T10:00:03+00:00`, `2026-11-05 10:00:03Z` and empty false | as listed |

These tests use the existing `strings` helper of the tracker tests. If the form `gh api graphql` uses (`-f query=...`, `-F owner=...`) needs separate flag tokens, assert the flag and the value as neighbours instead.

- [ ] **Step 3: Run them.** `cargo test -j4 -p factory-engine edited_at is_github_time`. Expected: compile error `cannot find function 'parse_edited_at'`.

- [ ] **Step 4: Implement.**
  - `edited_at_args` builds `api graphql -f query=<Q> -F owner=<o> -F name=<r> -F number=<n>`, with `Q` as in the spec, Design 3.
  - `parse_edited_at` reads `data.repository.issue`. A missing or null value at any level is an error. It reads `lastEditedAt` (a string or null) and `timelineItems.nodes[*].createdAt`. Each time must pass `is_github_time`, or the result is an error. It returns the largest one, or `None`.
  - `GhTracker::edited_at` splits `repo_name()?` at `/`, runs the args and parses the output.
  - `Arc<T>`, `&T` and `GuardedTracker` forward the call. `GuardedTracker` handles it like `labelled_by`.
  - `FakeTracker` records `edited_at {issue}`, honours `fail`, and returns `edited.get(&issue).cloned()`. `PanicTracker` panics like its other methods.
  - `Recording`, `NoCalls`, `OneIssue` and `CountCreates` return `Ok(None)`.

- [ ] **Step 5: Run them again.** `cargo test -j4 -p factory-engine tracker` (all pass), then `cargo check -j4 -p factory-cli --tests` (compiles).

### Task 2: Gate pick_next on edited_at

**Files:**
- Modify: `crates/factory-engine/src/intake.rs:133-214`, `docs/reference/software-factory.md`
- Test: `crates/factory-engine/src/intake.rs` (`mod tests`, from line ~580)

**Interfaces:**
- Consumes: `Tracker::edited_at`, `is_github_time`.
- Produces: `pub fn pick_next(issues: &[IssueInfo], cfg: &Config, labelled_by: &dyn Fn(u32) -> Result<Option<LabelEvent>, String>, edited_at: &dyn Fn(u32) -> Result<Option<String>, String>, trusted: &[String]) -> Option<IssueInfo>`.
- Port: `Tracker`, through the closures `pick_issue` builds.

- [ ] **Step 1: Read.** Read `crates/factory-engine/src/intake.rs:125-215` and the tests from `const LABELLED_AT` (line ~585) through `pick_next_stops_asking_after_the_first_eligible_issue`.

- [ ] **Step 2: Write the failing tests.** Add `fn never_edited(_: u32) -> Result<Option<String>, String> { Ok(None) }` and use it in `pick(...)` and in every existing `pick_next` call. Then:

```rust
#[test]
fn pick_next_picks_an_issue_whose_label_bumped_updated_at() {
    let mut i = issue(3, &["factory"], "2026-11-01");
    i.updated_at = "2026-11-05T10:00:01Z".to_string();
    assert_eq!(pick_next(&[i], &Config::default(), &alice, &never_edited, &trusted()).map(|i| i.number), Some(3));
}
```

| Test | `edited_at` returns | Expected |
|---|---|---|
| `pick_next_skips_an_issue_edited_after_labelling` (rewritten) | `Ok(Some("2026-11-05T10:00:01Z"))` | `None` |
| `pick_next_skips_an_edit_in_the_label_second` | `Ok(Some(LABELLED_AT))` | `None` |
| `pick_next_picks_an_issue_edited_before_labelling` | `Ok(Some("2026-11-05T09:59:59Z"))` | `Some(3)` |
| `pick_next_skips_when_the_edit_lookup_fails` | `Err("graphql down")` | `None` |
| `pick_next_ignores_a_comment_after_the_label` | `Ok(None)`, with `updated_at` later than the label | `Some(3)` |
| `pick_next_skips_a_label_time_in_another_form` | `Ok(None)`, with the label event `at = "2026-11-05T10:00:00.5Z"` | `None` |

`pick_next_asks_edited_at_only_after_the_trust_check`: the label actor is `mallory` (untrusted), and `edited_at` is a closure that panics. Assert the result is `None` and that nothing panicked.

- [ ] **Step 3: Run them.** `cargo test -j4 -p factory-engine pick_next`. Expected: compile error `this function takes 4 arguments but 5 arguments were supplied`.

- [ ] **Step 4: Implement.** Add the parameter and pass it to `eligible`. After the trust check:
  - an event time where `!is_github_time(&event.at)` skips with "the label event time has an unexpected form";
  - `Err(e)` from `edited_at` skips with "the edit lookup failed: {e}";
  - `Ok(Some(t))` with `t.as_str() >= event.at.as_str()` skips with "the issue changed after the intake label was set";
  - anything else is eligible.

  Remove the `updated_at` comparison. `pick_issue` passes `&|n| tracker.edited_at(n)`. In `software-factory.md`, change the intake edit-gate sentence: the gate reads the body's last edit and the last title rename, a tie with the label time skips, and comments and label changes do not count.

- [ ] **Step 5: Run them again.** `cargo test -j4 -p factory-engine intake` and `cargo test -j4 -p factory-engine --test intake` (all pass), then `cargo test -j4 -p factory-cli --test intake_cmd --test tickets --test new_to_merged` (all pass).

Next plan: docs/superpowers/plans/2026-11-23-intake-and-preset-gates/plan-03-intake-and-preset-gates.md
