# gh repo name and login shape checks Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-22-gh-json-checks-design.md
> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** `GhTracker::repo_name` and `GhTracker::current_user` reject output that is not a well-formed `owner/name` or login, so a bad reply never becomes an API path or a trusted login.

**Commit:** fix(tracker): check gh repo name and login shape

**Architecture:** Two pure allowlist helpers, `check_repo_name` and `check_login`, are added in `crates/factory-engine/src/tracker.rs`. `check_repo_name` reuses the existing `is_repo_name` rule from `crates/factory-core/src/commitmsg.rs`, made `pub` and re-exported for this. `GhTracker::repo_name` and `GhTracker::current_user` call them after their existing empty-output check. Both methods sit behind the `Tracker` port, and their callers already handle an `Err`.

**Tech Stack:** Rust.

## Global Constraints

- Cargo commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- Only `crates/factory-engine/src/tracker.rs` changes, plus the visibility of `is_repo_name` in `crates/factory-core/src/commitmsg.rs` and its re-export in `crates/factory-core/src/lib.rs`. `is_repo_name` keeps its behaviour.
- Error texts, exactly:
  - ``gh repo view printed an unusable repository name: `<name>` ``
  - ``gh api user printed an unusable login: `<login>` ``
- The existing empty-output texts stay as they are: `gh repo view printed no repository name` and `gh api user printed no login`.
- Both checks are allowlists. Never write them as a list of rejected characters.

## Review Focus

1. A bot login such as `github-actions[bot]`: a user expects it accepted. Pinned by `check_login_accepts_bot_logins`.
2. Two lines of output (`o/r\nx/y`), a path step (`../x`, `o/..`) or URL syntax (`o/r?x=1`, `o/r#a`, `o%2Fr/x`): a user expects the repository name rejected. Pinned as rows of `check_repo_name_rejects_malformed_names`.
3. A name with an empty side (`/r`, `o/`): a user expects it rejected. Pinned as rows of `check_repo_name_rejects_malformed_names`.
4. Empty output: a user expects the existing `printed no repository name` error, not the new one. Pinned by `repo_name_keeps_the_empty_output_error`.
5. The real `gh` path: a user expects the check in the method, not only in the helper. Pinned by `repo_name_rejects_a_malformed_gh_reply` and `current_user_rejects_a_malformed_gh_reply`.

## File Structure

- Modify: `crates/factory-core/src/commitmsg.rs`: `fn is_repo_name` becomes `pub fn is_repo_name`.
- Modify: `crates/factory-core/src/lib.rs`: add `is_repo_name` to the `pub use commitmsg::{...}` list.
- Modify: `crates/factory-engine/src/tracker.rs`
  - New `check_repo_name` and `check_login`, next to `parse_actor`.
  - `GhTracker::repo_name` (around line 487) and `GhTracker::current_user` (around line 553): call the helpers.
  - `mod tests`: the new tests, after `edit_labels_reports_another_label_create_error`.

---

### Task 1: repo name and login are checked for shape

**Files:**
- Modify: `crates/factory-core/src/commitmsg.rs`, `crates/factory-core/src/lib.rs`
- Modify: `crates/factory-engine/src/tracker.rs`

**Interfaces:**
- Port: behind `Tracker::repo_name` and `Tracker::current_user` (`GhTracker` in `tracker.rs`).
- Consumes: `pub fn is_repo_name(s: &str) -> bool` from `factory_core` (letters, digits, `-`, `_`, `.`, non-empty), made public in this task.
- Produces: `pub(crate) fn check_repo_name(name: &str) -> Result<(), String>` and `pub(crate) fn check_login(login: &str) -> Result<(), String>`.

**Tests added:** `check_repo_name_accepts_owner_and_name`, `check_repo_name_rejects_malformed_names`, `check_login_accepts_bot_logins`, `check_login_rejects_malformed_logins`, `repo_name_rejects_a_malformed_gh_reply`, `current_user_rejects_a_malformed_gh_reply`, `repo_name_keeps_the_empty_output_error`.

- [ ] **Step 1: Read the code**

Read `is_repo_name` and `is_repo_char` in `crates/factory-core/src/commitmsg.rs` and the `pub use commitmsg::{...}` list in `crates/factory-core/src/lib.rs`. Read `GhTracker::run`, `GhTracker::repo_name`, `GhTracker::current_user`, the helper `fake_gh` and `edit_labels_reports_another_label_create_error` in `crates/factory-engine/src/tracker.rs`. Read the spec's Design section 4.

- [ ] **Step 2: Write the failing tests**

Add them to `mod tests`, after `edit_labels_reports_another_label_create_error`. Full examples:

```rust
#[test]
fn check_repo_name_rejects_malformed_names() {
    for name in [
        "o/r/x", "o r/x", "/r", "o/", "or", "o/r\nx/y", "../x", "o/..", "./r", "o/r?x=1", "o/r#a",
        "o%2Fr/x",
    ] {
        let err = check_repo_name(name).expect_err(name);
        assert_eq!(
            err,
            format!("gh repo view printed an unusable repository name: `{name}`")
        );
    }
}

#[test]
fn repo_name_rejects_a_malformed_gh_reply() {
    let dir = tempfile::tempdir().expect("tempdir");
    let gh = fake_gh(dir.path(), "echo 'o r/x'");
    let tracker = GhTracker::with_program(&gh, dir.path());
    let err = tracker.repo_name().expect_err("malformed");
    assert_eq!(err, "gh repo view printed an unusable repository name: `o r/x`");
}
```

| Test | Inputs | Assertion |
|---|---|---|
| `check_repo_name_accepts_owner_and_name` | `"o/r"`, `"Snehal1112/rocket-factory"` | each `.expect("valid")` |
| `check_login_accepts_bot_logins` | `"alice"`, `"a-b"`, `"github-actions[bot]"` | each `.expect("valid")` |
| `check_login_rejects_malformed_logins` | `"a b"`, `"a/b"`, `"a\nb"`, `"a:b"`, `"a?b"`, `"a_b"`, `"[bot]"` | each `expect_err` equals ``format!("gh api user printed an unusable login: `{login}`")`` |
| `repo_name_keeps_the_empty_output_error` | `fake_gh(dir.path(), "true")` (prints nothing), then `tracker.repo_name()` | `expect_err` equals `gh repo view printed no repository name` |
| `current_user_rejects_a_malformed_gh_reply` | `fake_gh(dir.path(), "echo 'a b'")`, then `tracker.current_user()` | `expect_err` equals ``gh api user printed an unusable login: `a b` `` (no trailing space) |

- [ ] **Step 3: Run the tests and see them fail**

Run: `cargo test -j4 -p factory-engine --lib tracker::tests::check_`
Expected: the build fails with ``cannot find function `check_repo_name` in this scope`` and ``cannot find function `check_login` in this scope``.

- [ ] **Step 4: Implement**

- In `crates/factory-core/src/commitmsg.rs`, change `fn is_repo_name` to `pub fn is_repo_name`, and add `is_repo_name` to the `pub use commitmsg::{...}` list in `crates/factory-core/src/lib.rs`. Leave its body alone.
- `check_repo_name`: split on `/`. Accept only exactly two parts, each passing `factory_core::is_repo_name` and not equal to `.` or `..`. Otherwise return the unusable-name error.
- `check_login`: strip one optional `[bot]` suffix with `strip_suffix`. Accept only a non-empty rest whose characters are all ASCII alphanumeric or `-`. Otherwise return the unusable-login error.
- In `GhTracker::repo_name` and `GhTracker::current_user`, call the helper after the existing empty check and before returning `Ok`, so empty output keeps its old error.
- Give both helpers a one-line doc comment.

- [ ] **Step 5: Run the tests and see them pass**

Run: `cargo test -j4 -p factory-engine --lib tracker::tests::check_`
Expected: 4 passed, 0 failed.

Run: `cargo test -j4 -p factory-engine --lib malformed_gh_reply`, then `cargo test -j4 -p factory-engine --lib repo_name_keeps_the_empty_output_error`
Expected: 2 passed, then 1 passed, 0 failed.

- [ ] **Step 6: Run the tracker module and the callers' tests**

Run: `cargo test -j4 -p factory-engine --lib tracker`, then `cargo check -j4 -p factory-core`, `cargo test -j4 -p factory-core commitmsg`, `cargo test -j4 -p factory-engine --test tickets`, `cargo test -j4 -p factory-engine --test intake` and `cargo test -j4 -p factory-engine --test log_events`.
Expected: all pass, with no change to `tests/`.

- [ ] **Step 7: Format and lint**

Run: `cargo fmt -p factory-engine`, `cargo fmt -p factory-core`, then `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings` and `cargo clippy -j4 -p factory-core --all-targets -- -D warnings`
Expected: clippy finishes with no warnings.

Next plan: none
