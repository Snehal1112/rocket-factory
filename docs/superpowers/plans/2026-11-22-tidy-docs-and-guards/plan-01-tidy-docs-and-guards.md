# Tidy docs and guards, plan 1: board fallback, paths and engine tidy

> **Spec:** docs/superpowers/specs/2026-11-22-tidy-docs-and-guards-design.md

> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** A board with an unknown review state still loads (#118), paths go through `Layout` constants and the lint catches `".factory` literals (#95), and the engine and dashboard tidies of #145 land without changing output.

**Commit:** refactor(core): unknown review state, path consts

**Architecture:** `ReviewState` gets a `#[serde(other)] Unknown` variant. New `pub const` path names in `layout.rs` replace literals in `resolve.rs`, `preset.rs`, both `project.rs` files and `Layout::board_file`, and the lint gains one needle. The engine tidy shares one whole-seconds helper, writes several gate lines with one board read, and renames the `risk_line` argument.

**Tech Stack:** Rust, serde, the dashboard crate (its own workspace).

## Global Constraints

- cargo commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`, never --workspace or --all. The dashboard is `cargo test -j4 --manifest-path dashboard/Cargo.toml <filter>`.
- No new board, ledger or events field. `Unknown` is a new value of an existing field, and the dashboard reads review states as plain strings.
- No change to risk patterns, protected paths, defaults or any output text.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- Line numbers are from 9a0755d. Find code by name when they have moved.

## File Structure

| File | Change |
|---|---|
| `crates/factory-core/src/board.rs` | `ReviewState::Unknown` (lines 10-20); tests. |
| `crates/factory-engine/src/sync.rs` | `has_no_verdict` (lines 161-168) lists `Unknown`; a test. |
| `crates/factory-cli/src/status.rs` | `review_word` (line 63) prints `unknown`. |
| `crates/factory-core/src/layout.rs` | Eight path constants; `board_file` uses `BOARD_FILE`. |
| `crates/factory-core/src/resolve.rs`, `preset.rs`, `project.rs` | Use the constants. |
| `crates/factory-cli/src/project.rs` | Lines 422-437 use the constants. |
| `crates/factory-cli/tests/no_hardcoded_paths.rs` | `".factory` needle, `home.rs` allowed, reasons updated, one test. |
| `crates/factory-engine/src/ports.rs`, `gate_record.rs`, `park.rs`, `merge.rs`, `checks_stage.rs`, `review.rs` | Engine tidy. |
| `dashboard/src/gates.rs` | `is_stale` (line 123) drops `"checks"`; a test. |

## Review Focus

1. A board written by this version with `"unknown"` and read again: a user expects it to stay `Unknown`, not fail. Pinned by `known_review_states_round_trip`, which includes `Unknown`.
2. A `".factory` literal inside a `#[cfg(test)]` module: a user expects no lint hit, because tests may name paths. Pinned by `path_hits_flag_a_factory_literal`, which has a test-module line that must not count.
3. Gate lines with `gate_record = false`: a user expects no `gates.jsonl` and no board read. Pinned by `gate_lines_write_nothing_when_off`.
4. A job in round 0 recorded through `record_gate`: the line still says round 1. Pinned by `record_gate_keeps_round_zero_as_one`.
5. A `waiting` line on a job in `review`: the dashboard calls it stale. Pinned by `waiting_in_review_is_stale_and_checks_is_no_stage`.

---

### Task 1: Unknown review states

**Files:**
- Modify: `crates/factory-core/src/board.rs:10-20`, `crates/factory-engine/src/sync.rs:161-168`, `crates/factory-cli/src/status.rs:63-71`
- Test: `crates/factory-core/src/board.rs` (`mod tests`), `crates/factory-engine/src/sync.rs` (add `mod tests` if none)

**Interfaces:**
- Produces: `ReviewState::Unknown`, serialized as `"unknown"`, and the target of any unknown string.
- Port: none. `has_no_verdict` is plain board logic in the engine.

- [ ] **Step 1: Read the code.** Read `crates/factory-core/src/board.rs:1-30` and `board.rs:430-480`, `crates/factory-engine/src/sync.rs:150-170` and `crates/factory-cli/src/status.rs:55-75`.

- [ ] **Step 2: Write the failing tests** in `board.rs` `mod tests`:

```rust
#[test]
fn an_unknown_review_state_loads_as_unknown() {
    let state: ReviewState = serde_json::from_str("\"reviewing\"").expect("parse");
    assert_eq!(state, ReviewState::Unknown);
}

#[test]
fn an_unknown_review_state_counts_as_missing() {
    let mut j = Job::new("001", "d", "s", "2026-11-22", "f", 1.0);
    j.set_review("code", ReviewState::Unknown);
    assert!(j.review_missing());
    assert_eq!(j.missing_reviews(), vec!["code"]);
}

#[test]
fn known_review_states_round_trip() {
    for (state, word) in [
        (ReviewState::Pending, "\"pending\""), (ReviewState::Running, "\"running\""),
        (ReviewState::Pass, "\"pass\""), (ReviewState::Changes, "\"changes\""),
        (ReviewState::Skipped, "\"skipped\""), (ReviewState::Failed, "\"failed\""),
        (ReviewState::Unknown, "\"unknown\""),
    ] {
        assert_eq!(serde_json::to_string(&state).expect("write"), word);
        assert_eq!(serde_json::from_str::<ReviewState>(word).expect("read"), state);
    }
}
```

In `sync.rs`, add `has_no_verdict_counts_unknown`. Build a `Job` with `Job::new` (as `review_missing_is_true_until_the_code_review_has_a_verdict` in `board.rs` does), set `code` to `ReviewState::Unknown`, and assert `has_no_verdict(&j)`.

- [ ] **Step 3: Run them.** `cargo test -j4 -p factory-core review_state`. Expected: compile error `no variant or associated item named 'Unknown'`.

- [ ] **Step 4: Implement.** Add `#[serde(other)] Unknown` as the last variant. Add `ReviewState::Unknown` to `has_no_verdict`, and add `ReviewState::Unknown => "unknown"` to `review_word`. Fix any other match the compiler flags in `cargo check -j4 -p factory-engine`, `-p factory-cli` and `-p factory-supervisor`, treating `Unknown` as "no verdict".

- [ ] **Step 5: Run them again.** `cargo test -j4 -p factory-core board` (all pass), `cargo test -j4 -p factory-engine has_no_verdict` (1 passed) and `cargo test -j4 -p factory-cli` (all pass).

### Task 2: Paths behind Layout constants

**Files:**
- Modify: `crates/factory-core/src/layout.rs`, `resolve.rs:13-16`, `preset.rs:446-451`, `project.rs:235,416-422`; `crates/factory-cli/src/project.rs:422-437`
- Test: `crates/factory-cli/tests/no_hardcoded_paths.rs`

**Interfaces:**
- Produces, in `factory_core::layout` (re-exported from `factory_core` as the existing layout items are): `FACTORY_OVERRIDE_REL_FILE = ".factory/override.toml"`, `FACTORY_AGENTS_REL_DIR = ".factory/agents"`, `FACTORY_RISK_GLOB = ".factory/**"`, `PROJECT_AGENTS_DIR = "agents"`, `PROJECT_STATE_DIR = "state"`, `PROJECT_WORKTREES_DIR = "worktrees"`, `STATE_JOBS_DIR = "jobs"` and `BOARD_FILE = "board.json"`, all `pub const &str`.
- Port: none.

- [ ] **Step 1: Read the code.** Read `crates/factory-core/src/layout.rs`, `crates/factory-cli/tests/no_hardcoded_paths.rs`, `crates/factory-core/src/resolve.rs:1-30`, `crates/factory-core/src/preset.rs:440-455`, `crates/factory-core/src/project.rs:225-240` and `project.rs:410-425`, and `crates/factory-cli/src/project.rs:415-440`.

- [ ] **Step 2: Write the failing test** in `no_hardcoded_paths.rs`, next to `the_paths_lint_catches_main_after_an_early_test_use`:

```rust
#[test]
fn path_hits_flag_a_factory_literal() {
    let src = "const A: &str = \".factory/agents\";\n#[cfg(test)]\nmod tests { const B: &str = \".factory/x\"; }\n";
    let hits = path_hits("factory-core/src/x.rs", src);
    assert_eq!(hits, vec!["factory-core/src/x.rs:1: const A: &str = \".factory/agents\";".to_string()]);
}
```

- [ ] **Step 3: Run it.** `cargo test -j4 -p factory-cli --test no_hardcoded_paths path_hits_flag`. Expected: fails, because the hits are empty.

- [ ] **Step 4: Implement.** Add `|| line.contains("\".factory")` to `path_hits`. Add the constants to `layout.rs`, and make `Layout::board_file` use `BOARD_FILE`. Use them in place of each literal listed in Files. In `resolve.rs`, the three private constants go away and their uses take the `layout` names. `preset.rs:451` formats its message with `{AGENTS_DIR}` or `FACTORY_AGENTS_REL_DIR`. In `ALLOWED`:
  - Add `"factory-core/src/home.rs"` with the comment `// The default home folder name, ~/.factory.`
  - Change the `config.rs` and `risk.rs` comments to say the patterns are risk policy, not paths the driver opens, and drop "plan 3 moves".
  - Give `factory-git/src/repo.rs` the comment `// factory-git does not depend on factory-core; the in-repo worktree folder mirrors Layout::in_repo.`

- [ ] **Step 5: Run them again.** `cargo test -j4 -p factory-cli --test no_hardcoded_paths` (all pass), then `cargo test -j4 -p factory-core` and `cargo test -j4 -p factory-cli --test project_cmd --test external_layout` (all pass).

### Task 3: Engine and dashboard tidy

**Files:**
- Modify: `crates/factory-engine/src/ports.rs:20`, `gate_record.rs:73-120`, `park.rs:1-10` and `park.rs:213`, `merge.rs:1-4`, `checks_stage.rs:1-12` and its `gate_checks_run`, `review.rs:497` (`risk_line`), `dashboard/src/gates.rs:129`
- Test: `crates/factory-engine/src/gate_record.rs` (`mod tests`, line 193), `crates/factory-engine/src/review.rs` (`mod tests`, line 608), `dashboard/src/gates.rs` (`mod tests`, line 189)

**Interfaces:**
- Produces: `pub(crate) fn now_whole_secs() -> u64` in `ports.rs`, and `pub(crate) fn gate_lines(&self, lines: &[(GateKey, GateResult, &str)])` on `Engine` in `gate_record.rs`.
- Port: none. The gate record is a job-folder file.

- [ ] **Step 1: Read the code.** Read `crates/factory-engine/src/gate_record.rs`, `crates/factory-core/src/gate_record.rs:140-170` (`GateLine::new`), `crates/factory-engine/src/checks_stage.rs:140-180`, `crates/factory-engine/src/review.rs:490-530` and `dashboard/src/gates.rs:115-135`, plus the `job_at` test helper at `gates.rs:196`.

- [ ] **Step 2: Write the failing tests.** In `review.rs` `mod tests`:

```rust
#[test]
fn risk_line_cleared_by_lock_says_so() {
    let (result, text) = risk_line(false, &[], Some("only lock bumps"));
    assert_eq!(result, GateResult::Pass);
    assert_eq!(text, "cleared by the Cargo lock check; only lock bumps");
    let (result, text) = risk_line(true, &["a.rs".to_string()], Some("x"));
    assert_eq!(result, GateResult::Note);
    assert_eq!(text, "risky: a.rs");
}
```

In `gate_record.rs` `mod tests`, add these. Build an Engine over a board with one job, the way plan 1 of engine-small-fixes did with `with_board_job` in `crates/factory-engine/src/park.rs`. Reuse it if it is visible, or else copy its shape.

| Test | Setup | Assertion |
|---|---|---|
| `gate_lines_write_every_line_once` | `gate_record = true`; `gate_lines(&[(Format, Pass, "a"), (Lint, Pass, "b")])` | `factory_core::gate_record::read(job_dir)` has 2 lines, gates `format` and `lint` in that order |
| `gate_lines_write_nothing_when_off` | `gate_record = false`; one line | no `gates.jsonl` in the job folder |
| `record_gate_keeps_round_zero_as_one` | job with `round = 0`; `record_gate(..)` through a `Driver`, or `GateLine::new` round 0 if no driver fixture fits | the line's `round == 1` |

In `dashboard/src/gates.rs` `mod tests`:

```rust
#[test]
fn waiting_in_review_is_stale_and_checks_is_no_stage() {
    assert!(is_stale("checks", "waiting", &job_at("review", 1)));
    assert!(!is_stale("series", "waiting", &job_at("review", 1)));
    assert!(!is_stale("checks", "waiting", &job_at("checks", 1)));
}
```

- [ ] **Step 3: Run them.** `cargo test -j4 -p factory-engine gate_lines risk_line` (expected: compile error `no method named 'gate_lines'`) and `cargo test -j4 --manifest-path dashboard/Cargo.toml waiting_in_review` (expected: the third assertion fails, because `"checks"` still counts as working).

- [ ] **Step 4: Implement.**
  - Add `now_whole_secs` to `ports.rs` as `now().max(0.0) as u64`, and use it in `gate_record.rs` (where it replaces `now_at`) and at `park.rs:213`.
  - Add `gate_lines`. It returns when `gate_record` is off, loads the job once, and writes each line. `gate` becomes `self.gate_lines(&[(key, result, detail)])`.
  - `gate_checks_run`, and the park's two gate calls in `park_with_target`, use one `gate_lines` call where they write more than one line.
  - `record_gate` passes `job.round`.
  - Rename the third `risk_line` parameter to `cleared_by_lock`, with a doc comment.
  - Merge the `factory_core` imports into one block in `park.rs`, `checks_stage.rs` and `merge.rs`.
  - Drop `"checks"` from `gates.rs:129`.

- [ ] **Step 5: Run them again.** `cargo test -j4 -p factory-engine` (all pass) and `cargo test -j4 --manifest-path dashboard/Cargo.toml` (all pass).

Next plan: docs/superpowers/plans/2026-11-22-tidy-docs-and-guards/plan-02-tidy-docs-and-guards.md
