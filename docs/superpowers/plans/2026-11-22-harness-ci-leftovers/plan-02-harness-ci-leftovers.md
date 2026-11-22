# macOS CI, the /proc lint and the combine lint Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-22-harness-ci-leftovers-design.md
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Tests pin the macOS CI matrix and the crate lists, a test stops any `/proc` read, and the combine lint ignores `Task N` inside backticks.

**Commit:** fix(lint): pin CI lists, skip code in task refs

**Architecture:**
- The CI matrix, `factory-procgroup` in `PKGS` and `CRATES`, and the portable `live-smoke.sh` were done by hand on main before this job. This plan only adds tests that pin them, and does not edit `.github/workflows/ci.yml`, `scripts/release.sh` or `scripts/live-smoke.sh`.
- A source lint test sits next to `no_hardcoded_paths.rs`.
- No ungated Linux-only test is known today. A test that fails on the first macOS CI run is gated with `#[cfg(target_os = "linux")]` after the job, as the spec's Constraints say.
- `stale_mentions` in `crates/factory-engine/src/combine.rs` reuses `strip_spans` from `crates/factory-core/src/commitmsg.rs`. The combine code is pure plan text with no port.

**Tech Stack:** GitHub Actions YAML, bash, Rust.

## Global Constraints

- Cargo commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- No task adds a dependency or edits any `Cargo.toml`, which is a risk path.
- `PKGS` in `.github/workflows/ci.yml` and `CRATES` in `scripts/release.sh` list the same crates as the workspace `Cargo.toml` members.
- No board, ledger or event field.

## Review Focus

1. A workspace member added later and missing from `PKGS`: a user expects a test to say so. Pinned by `ci_and_release_list_every_workspace_crate`.
2. A `/proc` mention in a doc comment, such as `procinfo.rs`: a user expects it allowed. Pinned by `no_source_reads_proc` passing on today's tree.
3. A mention outside backticks on the same line as one inside: a user expects the outside one still to warn. Pinned by `a_mention_outside_the_code_span_still_warns`.
4. A lone backtick on a line: a user expects the rest of the line still checked. Pinned by `a_lone_backtick_does_not_hide_a_mention`.
5. The issue-ref parser, which shares `strip_spans`: a user expects it unchanged. Pinned by the existing `commitmsg` tests, which must pass unchanged.

## File Structure

- Modify: `crates/factory-cli/tests/no_hardcoded_paths.rs`
- Modify: `crates/factory-core/src/commitmsg.rs` (`strip_spans` becomes `pub`), `crates/factory-core/src/lib.rs` (export)
- Modify: `crates/factory-engine/src/combine.rs` (`stale_mentions`, tests)

---

### Task 1: tests pin the CI matrix and the crate lists

**Files:**
- Modify: `crates/factory-cli/tests/no_hardcoded_paths.rs`

**Interfaces:**
- Port: none. The workflow and the release script are text, and a Rust test reads them.
- Produces: no code items.

**Tests added:** `ci_and_release_list_every_workspace_crate` (in `crates/factory-cli/tests/no_hardcoded_paths.rs`), `ci_runs_on_linux_and_macos`.

- [ ] **Step 1: Read the code**

Read `.github/workflows/ci.yml` in full, the `CRATES` list and its CI gate in `scripts/release.sh`, the `[workspace] members` of the root `Cargo.toml`, and `crates/factory-cli/tests/no_hardcoded_paths.rs`. Then read `crates/factory-cli/tests/plugin_files.rs` `release_script_gate_checks_plugin_drift`, to see how a test reads `release.sh`.

- [ ] **Step 2: Write the tests, which pin today's text**

`ci_and_release_list_every_workspace_crate`:
- Read the root `Cargo.toml`, `.github/workflows/ci.yml` and `scripts/release.sh` from `CARGO_MANIFEST_DIR/../..`.
- Parse the `members = [ ... ]` block of the root `Cargo.toml` as text, one quoted `crates/<name>` per line. Do not add a TOML dependency.
- For each member path `crates/<name>`, assert that `ci.yml` contains `-p <name>`, and that `release.sh` names `<name>` in `CRATES`.
- Each assert message names the crate and the file.

`ci_runs_on_linux_and_macos`:
- `ci.yml` contains `os: [ubuntu-latest, macos-latest]`, `fail-fast: false` and `runs-on: ${{ matrix.os }}`.
- Each of the three appears twice, once per job.

- [ ] **Step 3: Run them**

Run: `cargo test -j4 -p factory-cli --test no_hardcoded_paths`, then `cargo test -j4 -p factory-cli --test plugin_files` and `cargo test -j4 -p factory-cli --test live_smoke_script`.
Expected: all pass, because the CI and release edits were made by hand before this job. If one fails, report it in the ledger and do not edit `ci.yml`, `release.sh` or `live-smoke.sh`.

### Task 2: no source reads `/proc`

**Files:**
- Modify: `crates/factory-cli/tests/no_hardcoded_paths.rs`

**Interfaces:**
- Port: none. It is a source lint test.
- Consumes: the helper `rust_files(dir, out)` and the `#[cfg(test)]` split and `//` skip of `no_source_names_a_state_path_or_the_main_branch`.

**Tests added:** `no_source_reads_proc`, `the_proc_lint_catches_a_proc_literal`.

- [ ] **Step 1: Read the code**

Read `no_source_names_a_state_path_or_the_main_branch` and `rust_files` in `crates/factory-cli/tests/no_hardcoded_paths.rs`. Grep every crate's `src` for `/proc`, to see that only the doc comment in `crates/factory-engine/src/procinfo.rs` mentions it.

- [ ] **Step 2: Write the tests**

Factor the per-file check into `fn proc_lines(text: &str) -> Vec<String>`. It returns the non-comment lines before `#[cfg(test)]` that contain `"/proc`.

```rust
#[test]
fn the_proc_lint_catches_a_proc_literal() {
    let src = "//! Reads nothing from /proc.\nfn f() { let _ = std::fs::read(\"/proc/self/stat\"); }\n#[cfg(test)]\nmod tests { const P: &str = \"/proc/1\"; }\n";
    assert_eq!(proc_lines(src).len(), 1);
}
```

`no_source_reads_proc` collects the `src` folder of every member of the workspace `Cargo.toml`, `factory-procgroup` and `factory-telemetry` included, and asserts that `proc_lines` is empty for each file. A failure names the file and the line.

- [ ] **Step 3: Run them**

Run: `cargo test -j4 -p factory-cli --test no_hardcoded_paths proc`
Expected: both pass, because no source reads `/proc` today. If `no_source_reads_proc` fails, report the line in the ledger and stop. Do not change production code to make it pass.

### Task 3: the combine lint skips inline code

**Files:**
- Modify: `crates/factory-core/src/commitmsg.rs`, `crates/factory-core/src/lib.rs`
- Modify: `crates/factory-engine/src/combine.rs`

**Interfaces:**
- Port: pure plan text, with no port.
- Produces: `pub fn strip_spans(line: &str) -> String`, exported from `factory_core`, with the same body.
- Consumes: `lines_with_fence` in `crates/factory-engine/src/tasks.rs`.

**Tests added:** `an_inline_code_mention_is_left_alone`, `a_mention_outside_the_code_span_still_warns`, `a_lone_backtick_does_not_hide_a_mention` (in `combine.rs` `mod tests`).

- [ ] **Step 1: Read the code**

Read `strip_code` and `strip_spans` in `crates/factory-core/src/commitmsg.rs`, and the `pub use commitmsg::{...}` list in `crates/factory-core/src/lib.rs`. Read `stale_mentions` and its two callers in `crates/factory-engine/src/combine.rs`. Read the tests `fenced_lines_are_left_alone`, `a_body_mention_of_another_task_warns` and the helpers `plan` and `two` in `combine.rs` `mod tests`.

- [ ] **Step 2: Write the failing tests**

Build each case as `a_body_mention_of_another_task_warns` does, changing only plan b's task body to one of the bodies in the block below. They sit in a fence, because this plan is itself combined into the job and the lint skips fenced lines.

```text
body A: Write `### Task 1: first` into the fixture.
body B: Write `Task 1` and see Task 2.
body C: a ` b, see Task 2.
```

Each test uses the text after `body X: ` as a Rust string literal.

| Test | Plan b body | Assertion |
|---|---|---|
| `an_inline_code_mention_is_left_alone` | body A | `got.warnings.is_empty()` |
| `a_mention_outside_the_code_span_still_warns` | body B | exactly one warning, which names the mention of the second task |
| `a_lone_backtick_does_not_hide_a_mention` | body C | exactly one warning, which names the mention of the second task |

Every warning also names its own section, so assert on the `mentions "..."` part of the warning, built with `format!`, not on the bare number.

- [ ] **Step 3: Run them and see them fail**

Run: `cargo test -j4 -p factory-engine --lib combine::tests`
Expected: `an_inline_code_mention_is_left_alone` fails with one warning, and `a_mention_outside_the_code_span_still_warns` fails with two. `a_lone_backtick_does_not_hide_a_mention` passes.

- [ ] **Step 4: Implement**

- Make `strip_spans` `pub`, give it a doc comment saying it blanks inline code spans, and export it.
- In `stale_mentions`, search `factory_core::strip_spans(line)` in place of `line`.

- [ ] **Step 5: Run them and see them pass**

Run: `cargo test -j4 -p factory-engine --lib combine`, `cargo test -j4 -p factory-engine --test one_job`, `cargo test -j4 -p factory-core commitmsg`.
Expected: all pass, 0 failed.

- [ ] **Step 6: Format and lint**

Run: `cargo fmt -p factory-core`, `cargo fmt -p factory-engine`, `cargo fmt -p factory-cli`, then `cargo clippy -j4 -p factory-core --all-targets -- -D warnings`, `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`, `cargo clippy -j4 -p factory-cli --all-targets -- -D warnings`.
Expected: clippy finishes with no warnings.

Next plan: docs/superpowers/plans/2026-11-22-harness-ci-leftovers/plan-03-harness-ci-leftovers.md
