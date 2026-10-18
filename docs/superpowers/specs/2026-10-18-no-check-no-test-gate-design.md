# No-checks and no-tests merge gate - Design Spec

Ticket: Snehal1112/rocket-factory#2.

## Goal

A job that changes code must not reach auto-merge on the strength of "no check ran". The driver parks such a job for the human, warns the reviewer when a code diff adds no test, and records which checks ran. Rocket's `src-tauri` check rule also runs tests.

## Overview

`VERDICT: APPROVE` is the last line of a reviewer that has only `Read`, `Grep` and `Glob`. It cannot run anything, so the only mechanical evidence behind a merge is `checks.md`, written by the driver. That evidence is weaker than it looks, in three ways:

1. `PASS none` counts as a pass.
2. Nothing requires or even notices a missing test.
3. Rocket's `src-tauri/**` rule runs `cargo check -j4` only.

This spec closes the first two in the engine, adds a small ledger line so the evidence is readable, and fixes the third in Rocket's registration fixtures.

## Current state

How a job reaches a merge today (all paths under `crates/`):

| Step | Where | What it does |
|---|---|---|
| Plan the checks | `factory-checks/src/plan.rs:62-89` | Each `[[checks]]` rule whose `paths` match any changed path contributes its commands. `{crate}` expands once per touched crate. A command that fails `validate_command` is dropped silently. |
| Run the checks | `factory-checks/src/run.rs:123-140` | One subprocess per command in the job worktree. |
| Write the evidence | `factory-checks/src/run.rs:186-197` | `checks.md` gets `PASS <cmd>` / `FAIL <cmd>` lines, or `PASS none (no check is mapped to the changed paths)` when `results` is empty. |
| Checks stage | `factory-engine/src/checks_stage.rs:53-63` | `report.all_passed()` is true for an empty report, so the stage advances to review. |
| Review stage | `factory-engine/src/review.rs:30-92` | Runs the reviewer, then `decide()`. `checks_ok` comes from `latest_checks_ok`, which accepts a `PASS none` line. |
| Decide | `factory-engine/src/decide.rs:24-58` | Risky diff, budget, size, checks, then the verdict. `Approve` gives `AutoMerge`. |
| Apply | `factory-engine/src/review.rs:105-128` | On `AutoMerge` it re-runs `final_checks()` on the final head. A failure starts a fix round. Only a green re-run sets `Stage::Approve`. |

Gaps this leaves:

- A job whose changed paths match no rule, or whose matching commands are all dropped, merges with zero checks run. Nothing in `decide()` or `review.rs` guards it.
- `docs/` and `.md` diffs reach the same state legitimately, so "no checks" cannot simply be a failure.
- The builder and reviewer prompts ask for tests (`factory-builder.md:19,29`, `factory-code-reviewer.md:27,34`). No code checks that a diff touches any test.
- `.claude/factory/config.toml` and Rocket's `project.toml` map `src-tauri/**` to `cargo check -j4`. The `src-tauri` package is named `rocket` (checked in Rocket's `src-tauri/Cargo.toml`).

## Non-goals

- Proving that each acceptance criterion has a test. That stays a reviewer judgment.
- Changing the reviewer's tools, the `VERDICT:` parser or `decide()`.
- Running tests from the reviewer.
- Gating the per-task checks that run during build (`build.rs:329-339`). They are informational today and stay so.
- Editing the live `~/.factory/projects/rocket/project.toml` (outside this repo).

## Design

### Flow after the change

```
build -> checks stage -> [no-checks gate] -> [no-tests gate/warn] -> review -> decide -> apply
                                                                                     |
                                              final checks on head -> [no-checks gate] -> checks ran: ... -> approve
```

The gates sit in the checks stage and in the final re-run, not in `decide()`. `decide()` runs after the reviewer, so a gate placed there would spend review tokens on a job that cannot merge. A risky diff is skipped by the gates, so it still stops as `risky-diff` in `decide()`.

### 1. Settings (`factory-core/src/config.rs`)

Four optional keys on `Config`, all with defaults, so an old config parses unchanged (`Config` keeps `deny_unknown_fields`).

| Key | Type | Default | Meaning |
|---|---|---|---|
| `code_paths` | list of globs | empty = built-in rule | What counts as code |
| `test_paths` | list of globs | empty = built-in list | What counts as a test file |
| `test_markers` | list of strings | empty = built-in list | Substrings that mark an added test line |
| `require_tests` | bool | `false` | Park instead of warn when a code diff adds no test |

Built-in rules:

- Code: empty `code_paths` means a path is code unless it is a docs path (`*.md`, `docs/**`, `.claude/**`), the same rule `risk.rs` already uses as `is_doc_path`. A non-empty list is a positive list of globs, matched with `glob_match` like `risk_paths`.
- Test paths: `**/tests/**`, `**/__tests__/**`, `**/*.test.ts`, `**/*.test.tsx`, `**/*.spec.ts`, `**/*_test.rs`, `**/*_test.go`.
- Test markers: `#[test]`, `#[tokio::test]`, `#[cfg(test)]`, `describe(`, `it(`, `test(`, `func Test`.
- A degraded config (`Config::fail_closed()`) sets `code_paths` to `["**"]`, so every path counts as code. `test_paths` and `test_markers` keep their defaults and `require_tests` stays false.

Example, a Go project that opts in:

```toml
[settings]
code_paths = ["**/*.go"]
test_paths = ["**/*_test.go"]
require_tests = true
```

### 2. Helpers (`factory-core/src/gate.rs`, new)

Pure functions, no I/O:

```rust
pub fn code_paths_of<S: AsRef<str>>(paths: &[S], config: &Config) -> Vec<String>;
pub fn has_test_path<S: AsRef<str>>(paths: &[S], config: &Config) -> bool;
pub fn has_test_marker(added_lines: &str, config: &Config) -> bool;
```

Paths are normalized like `classify_paths` (trim, strip a leading `./`) and compared lower case. `has_test_marker` takes the added lines only (lines starting with `+`, not `+++`) and returns true when any contains a marker. Both empty-list rules above live here, not in `Config`.

### 3. Park kinds (`factory-core/src/park.rs`)

`ParkKind::NoChecks` (`no-checks`) and `ParkKind::NoTests` (`no-tests`). Both offer `approve` and `rework`, like `risky-diff`. The review has not run when these park, so `next_for` already turns `approve` into `approve --without-review` with its explanation. `park.json` stays readable by older code because the kind is a string (`as_str`). `dashboard/static/board.js` gets a label for each in `PARK_LABEL`.

### 4. The no-checks gate (`factory-engine`)

New `Engine::gate_checks(paths, report) -> Option<(ParkKind, String)>`:

- `None` when `report.results` is not empty.
- `None` when the diff is risky (`classify_paths` is `Risky`). `decide()` stops it as `risky-diff`.
- `None` when `code_paths_of(paths)` is empty (docs-only, or an empty path list).
- Otherwise `Some((NoChecks, reason))`. The reason names the first 5 code paths and a count, for example `no check ran for 7 changed code paths: a.rs, b.rs, c.rs, d.rs, e.rs (+2 more)`.

"No checks ran" means an empty report, never a text match on `checks.md`.

Call sites:

- `checks_stage`, after `report.all_passed()` and before it returns `Advanced(Review)`. On `Some`, write a ruling and `park_with(kind, reason, parts)`. Evidence part: `checks.md` plus the reason.
- `apply_decision`, `Decision::AutoMerge` arm, after `final_checks()` passes and before `Stage::Approve`. The same helper and the same park path. This catches a fix round that changed the diff so that no rule maps to it any more.

### 5. The no-tests gate (`factory-engine`)

`Engine::no_tests_added(paths, diff_text) -> bool` is true when the diff has a code path, no changed path matches `test_paths`, and no added line contains a marker. The diff text is the whole job diff (`diff_to_files(base, head)`, renamed and deleted paths included in `paths`).

In `checks_stage`, after the no-checks gate:

- `no_tests_added` and `require_tests`: park `NoTests`, reason naming the code paths.
- `no_tests_added` and not `require_tests`: write one ruling `no test file or test marker in a code diff` and carry on. It is written each time the checks stage passes.
- Otherwise nothing.

In `review_stage`, `ReviewerExtra` gets `no_tests: bool`. When set, `specs.rs` adds one line to the reviewer context: `No test was added in this job. Judge whether the new behavior is covered, and treat missing tests as a blocker.` When unset the line is absent. The no-tests rule is not repeated in the final re-run.

### 6. Ledger line

In the `AutoMerge` arm, after the gates pass and before `auto-merge approved, ...`, the driver writes `checks ran: <cmd1>, <cmd2>` from the final report, or `checks ran: none` when it is empty (a docs-only job). Only the driver writes the ledger.

### 7. Rocket's `src-tauri` rule

In `crates/factory-core/tests/fixtures/rocket-config.toml` and `rocket-project.toml` the `src-tauri/**` rule becomes:

```toml
commands = ["cargo check -j4 -p rocket", "cargo test -j4 -p rocket"]
```

`validate_command` accepts it (`-j4` present, no `--workspace` or `--all`). The live `~/.factory/projects/rocket/project.toml` is edited by hand after the merge.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| Every matching command dropped by `validate_command` | Empty report, so the no-checks gate applies to a code diff. |
| Docs-only diff, `PASS none` | Not parked. The ledger says `checks ran: none`. |
| Risky code diff, `PASS none` | The gate skips it, `decide()` parks `risky-diff`. |
| Empty changed-path list | Not code, not parked, no warning. |
| Renamed or deleted test file only | The path counts as a changed path. A delete-only diff of code with no test path and no marker warns as any other. |
| Config parse fails (degraded) | `code_paths = ["**"]`, so every path is code and the gate is strict. |
| `code_paths = []` or `test_paths = []` | The built-in rule or list, never "nothing". |
| Diff file cannot be read | The marker check fails open for `require_tests = false` (warn, do not park) and parks `NoTests` only when `require_tests = true`. The ledger notes the read failure. |
| Fix round after a `no-checks` park | Only a human `approve` or `rework` moves it. The builder cannot add a check rule, so the driver does not start a fix round. |
| Per-task checks during build | Unchanged. Not gated. |

## Testing

`factory-core`:
- `gate.rs`: docs-only paths are not code; a source file is; `code_paths` replaces the default; an empty list means the default; a degraded config counts every path as code; each default test path layout matches and `lib.rs` does not; case and a leading `./` are ignored; `has_test_marker` sees `+#[test]` and ignores a removed `-#[test]` line and `+++` headers.
- `config.rs`: a config without the new keys parses with defaults; the new keys parse; `fail_closed` sets `code_paths`.
- `park.rs`: stable names `no-checks` and `no-tests`; `next` offers approve and rework; a `park.json` with an old kind still parses.
- `rocket_parity.rs`: the `src-tauri/src/lib.rs` rule yields the two `-p rocket` commands and not a bare `cargo check -j4`.

`factory-engine` (fakes, in the style of `build_checks.rs` and `review_decide.rs`):
- A code diff with no mapped check parks `no-checks` and the reviewer is not run.
- A docs-only diff with no mapped check is not parked.
- A risky code diff with no mapped check ends as `risky-diff`.
- The final re-run parks `no-checks` when a fix round leaves no mapped check.
- The reason names five paths and a count.
- A code diff with no test path or marker writes the ledger line and the reviewer note and does not park. With a test path, or an added `#[test]`, there is no note.
- `require_tests = true` parks `no-tests` for a code diff with no test, and not for a docs-only diff. `require_tests = false` never parks.
- `checks ran: ...` and `checks ran: none` appear before the auto-merge ledger line.
- A park from `no-checks` offers `approve --without-review` because the review has not run.

Commands: `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-checks`, `cargo test -j4 -p factory-engine`, `cargo check -j4`.

## Files changed

- Modify `crates/factory-core/src/park.rs`, `config.rs`, `risk.rs` (`is_doc_path` becomes `pub(crate)`), `lib.rs`.
- Create `crates/factory-core/src/gate.rs`.
- Modify `crates/factory-engine/src/checks_stage.rs`, `review.rs`, `park.rs`, `specs.rs`.
- Modify `dashboard/static/board.js`.
- Modify `crates/factory-core/tests/fixtures/rocket-config.toml`, `rocket-project.toml`, `crates/factory-core/tests/rocket_parity.rs`.
- Tests in `crates/factory-engine/tests/`.
- Update `README.md` (settings table) and `.claude/software-factory.md` (the park kinds and the gates).
- No new crate, no new dependency, no change to the board JSON beyond the two park kind strings.

## Constraints

- New settings and park kinds are optional with defaults, so board JSON and old configs stay compatible with `dashboard/`.
- A missing or broken config stays restrictive.
- Engine logic stays behind the ports in `ports.rs`, so the tests use the fakes.
- Only the driver writes the board, the ledger and `park.json`.
- No shelling out to `git`. Every cargo call uses `-j4`, per crate.

## Acceptance criteria

- A diff with a code path and no check results parks with kind `no-checks`, and the reviewer is not run.
- A diff of only `*.md`, `docs/**` or `.claude/**` paths and no check results is not parked.
- A risky diff with no check results still ends as `risky-diff`.
- The final re-run on the merge head applies the no-checks gate and parks through the same path.
- With no config, or a config that fails to parse, every changed path counts as code.
- A non-empty `code_paths` or `test_paths` replaces its default, and an empty list means the default.
- A code diff with no test path and no test marker writes a ledger line and a reviewer note, and does not park.
- A new test path, or an added `#[test]` line, in a code diff gives no note.
- With `require_tests = true`, a code diff with no test path and no marker parks as `no-tests`.
- The ledger has one line `checks ran: <commands>` or `checks ran: none` before the auto-merge line.
- The kinds have the names `no-checks` and `no-tests`, offer approve and rework, and the dashboard labels them.
- Rocket's fixtures have the new `src-tauri` rule and the fixture parity tests pass.
- `README.md` and `.claude/software-factory.md` describe the settings and the gates.
- `cargo check -j4` and the targeted tests for `factory-core`, `factory-checks` and `factory-engine` pass.

## Out of scope

- A test-coverage measurement or running only the new tests.
- Gating the per-task checks in build.
- Teaching the reviewer to run commands.
- Parsing test frameworks beyond the marker list.

## Follow-ups

- Edit the live Rocket registration `~/.factory/projects/rocket/project.toml` to match the fixture. It is outside the repo, so a human does it.
- Check whether a `cargo test` filter that matches zero tests should count as a pass. Today it does.
- Consider reporting commands that `validate_command` dropped, so the human sees why a rule did not run.

## Decisions

- What happens on `PASS none` for a code diff? Options: park as needs-human, fail the checks for a fix round, warn only. Chosen: park as needs-human with a new `no-checks` kind. Decided by: user
- How is a code diff told from a docs-only diff? Options: `code_paths` config with a default, reuse the `[[checks]]` globs, a hardcoded docs list. Chosen: `code_paths` config with a default of everything except docs. Decided by: user
- What happens when a code diff adds no test? Options: warn and tell the reviewer, always park, reviewer prompt only. Chosen: warn and tell the reviewer, with `require_tests` to park. Decided by: user
- How is a test file recognised? Options: `test_paths` config with a default, a fixed default list. Chosen: `test_paths` config with a default. Decided by: user
- Scope for `src-tauri` and the ledger report? Options: ledger line only with src-tauri as a follow-up, include Rocket's config fix, skip the ledger line. Chosen: include Rocket's config fix. Decided by: user
- How is "the diff added tests" decided when most Rust tests are inline? Options: test path or added test marker, test path only, marker only. Chosen: test path or added test marker, with a `test_markers` setting. Decided by: user
- `no-checks` is gated in the checks stage and the final re-run, not in `decide()`, so the reviewer is not run. Risky diffs are skipped so `risky-diff` still wins. Decided by: call
- "No checks ran" is an empty checks report, not a text match on `checks.md`. Decided by: call
- A `no-checks` park offers `approve` and `rework` and starts no fix round, because the builder cannot add a check rule. Decided by: call
- The `src-tauri` crate name is `rocket`, checked in Rocket's `src-tauri/Cargo.toml`. Decided by: call
- The live `project.toml` is a follow-up, not part of the job, because it is outside the repo. Decided by: call
