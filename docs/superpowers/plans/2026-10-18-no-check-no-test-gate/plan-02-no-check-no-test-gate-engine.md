# Engine gates Implementation Plan

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** The driver parks a code diff that ran no checks, warns the reviewer (or parks, when asked) when a code diff adds no test, and records the commands that ran.

**Architecture:** Two pure functions in the engine decide the no-checks and no-tests cases from paths, the checks report and the config. `checks_stage` and the `AutoMerge` arm of `apply_decision` call them and park through the existing `park_with`. `review_stage` passes a no-tests flag to the reviewer context. A risky diff is skipped by the no-checks gate, so `decide()` still stops it as `risky-diff`. `decide()` itself does not change.

**Tech Stack:** Rust, cargo with `-j4` per crate, the engine fakes in `crates/factory-engine/tests/common`.

**Spec:** `docs/superpowers/specs/2026-10-18-no-check-no-test-gate-design.md`

## Global Constraints

- Every cargo call has `-j4` and `-p <crate>`. Never `--workspace`, `--all` or `CARGO_TARGET_DIR`. Run the filtered tests each step names, and the whole `factory-engine` suite only in the last step of Task 3.
- Follow each task's steps in order: write the failing tests exactly as given, run the stated command and confirm the stated failure, implement from the signatures, then confirm the stated pass.
- Use the `Config` fields and the `factory_core` helpers from plan 1: `code_paths_of`, `has_test_path`, `has_test_marker`, `ParkKind::NoChecks`, `ParkKind::NoTests`. Do not redefine them.
- Engine logic stays behind the ports. Tests use the fakes. Only the driver writes the ledger, the board and `park.json`.
- A parked job offers `approve` and `rework`. Because the review has not run, `approve` appears as `approve --without-review`.
- Tests use `.expect("...")`. Never write the literal text of a panicking unwrap call in a plan, doc or Rust file. No `git` CLI, no commit steps. Comments are short full sentences ending with a punctuation mark.

## Review Focus

1. **A docs-only diff with no mapped check.** It must not park, and the ledger must say `checks ran: none`. Pinned in Task 1 (`a_docs_only_diff_with_no_mapped_check_is_not_parked`) and Task 3 (`the_ledger_says_none_when_no_command_ran`).
2. **A risky code diff with no mapped check.** It must still reach `decide()` and end as `risky-diff`, not `no-checks`. Pinned in Task 1 (`a_risky_code_diff_with_no_mapped_check_is_left_to_decide`).
3. **The final re-run.** A fix round that leaves no mapped check must park `no-checks` even though the reviewer approved. Pinned in Task 1 (`the_final_checks_gate_parks_an_approved_job`).
4. **A reason with many paths.** It names five and a count. Pinned in Task 1 (`no_checks_reason_names_five_paths_and_a_count`).
5. **`require_tests`.** False never parks, true parks only a code diff with no test path and no marker. Pinned in Task 2 (`a_code_diff_with_no_test_warns_and_does_not_park`, `require_tests_parks_a_code_diff_with_no_test`, `require_tests_does_not_park_a_docs_only_diff`).
6. **An inline test.** A Rust source file that adds `#[test]` counts as a test. Pinned in Task 2 (`an_added_test_marker_counts_as_a_test`).
7. **An existing test that maps no check to its changed path.** `docs_only_diff_still_writes_a_pass_line` in `build_checks.rs` changes `a.txt` with checks on `src/**` only. After this plan that job parks. Update that test to change `a.md` so it keeps its meaning (Task 1, Step 3).

## File Structure

- Modify `crates/factory-engine/src/checks_stage.rs`: `no_checks_reason`, `no_tests_added`, `Engine::gate_checks`, the two calls, unit tests.
- Modify `crates/factory-engine/src/review.rs`: the `AutoMerge` arm calls the gate and writes the `checks ran` ledger line; `review_stage` sets the reviewer flag.
- Modify `crates/factory-engine/src/park.rs`: `decision_parts` for the two kinds.
- Modify `crates/factory-engine/src/specs.rs`: `ReviewerExtra.no_tests` and one prompt line.
- Create `crates/factory-engine/tests/gates.rs`: the integration tests of all three tasks.
- Modify `crates/factory-engine/tests/build_checks.rs` (one test), `README.md`, `.claude/software-factory.md`.

---

### Task 1: Park a code diff that ran no checks

**Files:**
- Modify: `crates/factory-engine/src/checks_stage.rs`, `crates/factory-engine/src/review.rs`, `crates/factory-engine/src/park.rs`, `crates/factory-engine/tests/build_checks.rs`
- Create: `crates/factory-engine/tests/gates.rs`

**Interfaces:**
- Produces (in `checks_stage.rs`):

```rust
/// None unless a code diff ran no check. A risky diff is left to `decide()`.
pub(crate) fn no_checks_reason(paths: &[String], report_is_empty: bool, config: &Config) -> Option<String>;
```

  It returns `None` when `report_is_empty` is false, when `classify_paths(paths, config)` is `DiffClass::Risky`, or when `code_paths_of(paths, config)` is empty. Otherwise it returns `no check ran for {n} changed code path(s): {first five joined with ", "}` and ` (+{n-5} more)` when `n > 5`. Use `path` for `n == 1` and `paths` otherwise.
- Produces: `Engine::gate_checks(&self, paths: &[String], report: &ChecksReport) -> Option<(ParkKind, String)>` wrapping it with `ParkKind::NoChecks`.
- Consumes: `park_with(kind, reason, parts)`, `ParkPart::new`, `part_from_file(&self.job_dir, "checks.md", "Checks")` (already used by `decision_parts`).

- [ ] **Step 1: Write the failing tests.** Unit tests in a `#[cfg(test)]` module of `checks_stage.rs`:

```rust
#[cfg(test)]
mod gate_tests {
    use super::*;

    fn paths(p: &[&str]) -> Vec<String> {
        p.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn no_checks_reason_names_five_paths_and_a_count() {
        let all = paths(&["a.rs", "b.rs", "c.rs", "d.rs", "e.rs", "f.rs", "g.rs"]);
        let reason = no_checks_reason(&all, true, &Config::default()).expect("a reason");
        assert_eq!(reason, "no check ran for 7 changed code paths: a.rs, b.rs, c.rs, d.rs, e.rs (+2 more)");
    }

    #[test]
    fn one_path_reads_singular() {
        let reason = no_checks_reason(&paths(&["a.rs"]), true, &Config::default()).expect("a reason");
        assert_eq!(reason, "no check ran for 1 changed code path: a.rs");
    }

    #[test]
    fn a_report_with_results_is_never_gated() {
        assert!(no_checks_reason(&paths(&["a.rs"]), false, &Config::default()).is_none());
    }

    #[test]
    fn docs_only_and_empty_path_lists_are_not_gated() {
        assert!(no_checks_reason(&paths(&["README.md", "docs/a.md"]), true, &Config::default()).is_none());
        assert!(no_checks_reason(&[], true, &Config::default()).is_none());
    }

    #[test]
    fn a_risky_diff_is_left_to_decide() {
        let all = paths(&["src-tauri/src/lib.rs"]);
        assert!(no_checks_reason(&all, true, &Config::default()).is_none());
    }
}
```

  Integration tests in `crates/factory-engine/tests/gates.rs`:

```rust
mod common;

use common::{committing_fixture, scripted, scripted_auto, Effect, Fixture, ScriptedRunner};
use factory_core::{CheckRule, ParkKind, ParkRecord, Stage};
use factory_engine::{Decision, StageOutcome};

/// A builder that writes each file with the given body and leaves it uncommitted.
fn write_effect(files: Vec<(&'static str, &'static str)>) -> Effect {
    Box::new(move |spec| {
        for (name, body) in &files {
            if let Some(parent) = std::path::Path::new(name).parent() {
                std::fs::create_dir_all(spec.cwd.join(parent)).expect("mkdir");
            }
            std::fs::write(spec.cwd.join(name), body).expect("write file");
        }
        "done".to_string()
    })
}

/// Builds the job with one builder run that writes `files`.
fn built(fx: &Fixture, files: Vec<(&'static str, &'static str)>) -> ScriptedRunner {
    let runner = scripted_auto(vec![write_effect(files)]);
    fx.engine(&runner).build_stage().expect("build");
    runner
}

fn only_src_checks(fx: &mut Fixture) {
    fx.config.checks = vec![CheckRule { paths: vec!["src/**".into()], commands: vec!["false".into()] }];
}

fn reviewer(reply: &'static str) -> ScriptedRunner {
    scripted(vec![Box::new(move |_| reply.to_string())])
}

fn ledger(fx: &Fixture) -> String {
    std::fs::read_to_string(fx.job_dir.join("progress.md")).unwrap_or_default()
}

#[test]
fn a_code_diff_with_no_mapped_check_parks_as_no_checks_before_the_review() {
    let mut fx = committing_fixture();
    only_src_checks(&mut fx);
    let runner = built(&fx, vec![("a.txt", "x\n")]);
    let out = fx.engine(&runner).checks_stage().expect("checks");
    match out {
        StageOutcome::NeedsHuman(r) => assert!(r.contains("no check ran") && r.contains("a.txt"), "{r}"),
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(fx.job().stage, Stage::NeedsHuman);
    let rec = ParkRecord::read(&fx.job_dir).expect("park.json");
    assert_eq!(rec.kind, ParkKind::NoChecks);
    assert!(rec.next.iter().any(|n| n.contains("--without-review")), "{:?}", rec.next);
    assert!(rec.parts.iter().any(|p| p.text.contains("PASS none")), "{:?}", rec.parts);
}

#[test]
fn a_docs_only_diff_with_no_mapped_check_is_not_parked() {
    let mut fx = committing_fixture();
    only_src_checks(&mut fx);
    let runner = built(&fx, vec![("a.md", "x\n")]);
    let out = fx.engine(&runner).checks_stage().expect("checks");
    assert_eq!(out, StageOutcome::Advanced(Stage::Review));
}

#[test]
fn a_risky_code_diff_with_no_mapped_check_is_left_to_decide() {
    let mut fx = committing_fixture();
    only_src_checks(&mut fx);
    let runner = built(&fx, vec![("src-tauri/src/lib.rs", "x\n")]);
    let out = fx.engine(&runner).checks_stage().expect("checks");
    assert_eq!(out, StageOutcome::Advanced(Stage::Review));
    let rr = reviewer("fine\nVERDICT: APPROVE");
    let review = fx.engine(&rr).review_stage().expect("review");
    match review.decision {
        Decision::NeedsHuman { kind, .. } => assert_eq!(kind, ParkKind::RiskyDiff),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn the_final_checks_gate_parks_an_approved_job() {
    let mut fx = committing_fixture();
    let runner = built(&fx, vec![("a.txt", "x\n")]);
    fx.engine(&runner).checks_stage().expect("checks mapped at round 1");
    let rr = reviewer("fine\nVERDICT: APPROVE");
    let review = fx.engine(&rr).review_stage().expect("review");
    assert_eq!(review.decision, Decision::AutoMerge);
    // A fix round changed the diff so that no rule maps to it any more.
    only_src_checks(&mut fx);
    let out = fx.engine(&rr).apply_decision(&review).expect("apply");
    assert!(matches!(out, StageOutcome::NeedsHuman(_)), "{out:?}");
    assert_eq!(ParkRecord::read(&fx.job_dir).expect("park.json").kind, ParkKind::NoChecks);
    assert_ne!(fx.job().stage, Stage::Approve);
}
```

- [ ] **Step 2: Run to verify they fail:** `cargo test -j4 -p factory-engine --lib no_checks_reason` fails to compile (`no_checks_reason` not found). `cargo test -j4 -p factory-engine --test gates a_code_diff_with_no_mapped_check` fails: the stage returns `Advanced(Review)` instead of `NeedsHuman`.

- [ ] **Step 3: Implement.** Make `run_checks_now` also return the changed paths (or read them again in `checks_stage`). Write `no_checks_reason` and `Engine::gate_checks` to the signatures above. In `checks_stage`, after `report.all_passed()` and before `Advanced(Review)`: on `Some((kind, reason))` write a ruling and `park_with(kind, reason, parts)`, with one part from `part_from_file(&self.job_dir, "checks.md", "Checks")` and one `ParkPart::new("Why it stopped", "driver", &reason)`. Add `NoChecks | NoTests` to `decision_parts` in `park.rs` with the same two parts. In `apply_decision`, `Decision::AutoMerge` arm, call `gate_checks` on `final_checks()`'s report and the job's changed paths after the all-passed test and before `Stage::Approve`, and park the same way. In `build_checks.rs` change `docs_only_diff_still_writes_a_pass_line` to build `a.md` instead of `a.txt` (use a `change_effect("a.md", 3)` runner) and also assert `Advanced(Review)`.

- [ ] **Step 4: Run to verify they pass:** `cargo test -j4 -p factory-engine --lib gate_tests` (5 tests), `cargo test -j4 -p factory-engine --test gates` (4 tests) and `cargo test -j4 -p factory-engine --test build_checks docs_only`. Expected: pass. Then `cargo check -j4 -p factory-engine`.

### Task 2: Warn the reviewer, or park, when a code diff adds no test

**Files:**
- Modify: `crates/factory-engine/src/checks_stage.rs`, `crates/factory-engine/src/review.rs`, `crates/factory-engine/src/specs.rs`
- Test: `crates/factory-engine/tests/gates.rs`

**Interfaces:**
- Consumes: `has_test_path`, `has_test_marker`, `code_paths_of`, `Engine::gate_checks`, and `diff_to_files(base, head, job_dir)` whose `.diff` file is the unified diff.
- Produces (in `checks_stage.rs`):

```rust
/// True when the diff has a code path and neither a test path nor an added test marker.
/// `diff_text` is None when the diff file could not be read, which counts as no marker.
pub(crate) fn no_tests_added(paths: &[String], diff_text: Option<&str>, config: &Config) -> bool;
```

- Produces: `ReviewerExtra { no_tests: bool, .. }` (add the field, default false). When true, the reviewer context gets exactly the line `No test was added in this job. Judge whether the new behavior is covered, and treat missing tests as a blocker.`

- [ ] **Step 1: Write the failing tests.** Unit tests in the `gate_tests` module of `checks_stage.rs`:

```rust
    #[test]
    fn no_tests_added_is_true_for_a_code_diff_with_neither_path_nor_marker() {
        assert!(no_tests_added(&paths(&["src/a.rs"]), Some("+fn a() {}\n"), &Config::default()));
    }

    #[test]
    fn a_test_path_or_a_marker_or_docs_only_is_not_flagged() {
        let c = Config::default();
        assert!(!no_tests_added(&paths(&["src/a.rs", "tests/a.rs"]), Some("+x\n"), &c));
        assert!(!no_tests_added(&paths(&["src/a.rs"]), Some("+#[cfg(test)]\n"), &c));
        assert!(!no_tests_added(&paths(&["docs/a.md"]), Some("+x\n"), &c));
        assert!(!no_tests_added(&[], Some("+x\n"), &c));
    }

    #[test]
    fn an_unreadable_diff_counts_as_no_marker() {
        assert!(no_tests_added(&paths(&["src/a.rs"]), None, &Config::default()));
        assert!(!no_tests_added(&paths(&["tests/a.rs", "src/a.rs"]), None, &Config::default()));
    }
```

  Integration tests appended to `gates.rs`:

```rust
const NOTE: &str = "No test was added in this job.";
const LEDGER_NOTE: &str = "no test file or test marker in a code diff";

fn reviewer_prompt(fx: &Fixture) -> String {
    let rr = reviewer("fine\nVERDICT: APPROVE");
    fx.engine(&rr).review_stage().expect("review");
    rr.last_spec().expect("reviewer spec").prompt
}

#[test]
fn a_code_diff_with_no_test_warns_and_does_not_park() {
    let fx = committing_fixture();
    let runner = built(&fx, vec![("a.txt", "x\n")]);
    let out = fx.engine(&runner).checks_stage().expect("checks");
    assert_eq!(out, StageOutcome::Advanced(Stage::Review));
    assert!(ledger(&fx).contains(LEDGER_NOTE), "{}", ledger(&fx));
    assert!(reviewer_prompt(&fx).contains(NOTE));
}

#[test]
fn a_new_test_path_gives_no_note() {
    let fx = committing_fixture();
    let runner = built(&fx, vec![("a.txt", "x\n"), ("tests/a.txt", "x\n")]);
    fx.engine(&runner).checks_stage().expect("checks");
    assert!(!ledger(&fx).contains(LEDGER_NOTE));
    assert!(!reviewer_prompt(&fx).contains(NOTE));
}

#[test]
fn an_added_test_marker_counts_as_a_test() {
    let fx = committing_fixture();
    let runner = built(&fx, vec![("src/a.rs", "#[cfg(test)]\nmod t {}\n")]);
    fx.engine(&runner).checks_stage().expect("checks");
    assert!(!ledger(&fx).contains(LEDGER_NOTE));
    assert!(!reviewer_prompt(&fx).contains(NOTE));
}

#[test]
fn require_tests_parks_a_code_diff_with_no_test() {
    let mut fx = committing_fixture();
    fx.config.require_tests = true;
    let runner = built(&fx, vec![("a.txt", "x\n")]);
    let out = fx.engine(&runner).checks_stage().expect("checks");
    assert!(matches!(out, StageOutcome::NeedsHuman(_)), "{out:?}");
    let rec = ParkRecord::read(&fx.job_dir).expect("park.json");
    assert_eq!(rec.kind, ParkKind::NoTests);
    assert!(rec.next.iter().any(|n| n.contains("--without-review")), "{:?}", rec.next);
}

#[test]
fn require_tests_does_not_park_a_docs_only_diff() {
    let mut fx = committing_fixture();
    fx.config.require_tests = true;
    let runner = built(&fx, vec![("a.md", "x\n")]);
    let out = fx.engine(&runner).checks_stage().expect("checks");
    assert_eq!(out, StageOutcome::Advanced(Stage::Review));
}

#[test]
fn a_failing_check_still_wins_over_the_no_tests_rule() {
    let mut fx = committing_fixture();
    fx.config.require_tests = true;
    fx.config.checks = vec![CheckRule { paths: vec!["**".into()], commands: vec!["false".into()] }];
    let runner = built(&fx, vec![("a.txt", "x\n")]);
    let out = fx.engine(&runner).checks_stage().expect("checks");
    assert_eq!(out, StageOutcome::Advanced(Stage::Build), "a fix round, not a no-tests park");
}
```

- [ ] **Step 2: Run to verify they fail:** `cargo test -j4 -p factory-engine --lib no_tests_added` fails to compile. `cargo test -j4 -p factory-engine --test gates a_code_diff_with_no_test` fails: the ledger has no note and the prompt has no line.

- [ ] **Step 3: Implement.** Write `no_tests_added`. In `checks_stage`, after the no-checks gate and only when the checks passed, compute the whole job diff with `diff_to_files(base, head, &self.job_dir)`, read its `.diff` file (`ok()` for `Option<&str>`), and call `no_tests_added`. If true and `config.require_tests`: ruling, then `park_with(NoTests, reason, parts)` with the reason `no test was added in {n} changed code path(s): ...` built like the no-checks reason. If true and not `require_tests`: `self.ruling(LEDGER_NOTE)` and carry on. In `review_stage` compute the same flag from `paths` and the whole diff file the stage already writes (`whole.diff`), set `ReviewerExtra.no_tests`, and in `specs.rs` push the exact line above to the reviewer context only when it is set.

- [ ] **Step 4: Run to verify they pass:** `cargo test -j4 -p factory-engine --lib gate_tests` (8 tests) and `cargo test -j4 -p factory-engine --test gates` (10 tests). Expected: pass. Then `cargo check -j4 -p factory-engine`.

### Task 3: Record which checks ran, and document the gates

**Files:**
- Modify: `crates/factory-engine/src/review.rs`, `README.md`, `.claude/software-factory.md`
- Test: `crates/factory-engine/tests/gates.rs`

**Interfaces:**
- Consumes: `ChecksReport.results` (each has `.command`).
- Produces: in the `Decision::AutoMerge` arm, after the gates pass and before the `auto-merge approved, checks green, reviewer approved` ruling, one ruling `checks ran: <cmd1>, <cmd2>` or `checks ran: none`.

- [ ] **Step 1: Write the failing tests** appended to `gates.rs`:

```rust
fn approve(fx: &Fixture) {
    let rr = reviewer("fine\nVERDICT: APPROVE");
    let e = fx.engine(&rr);
    let review = e.review_stage().expect("review");
    assert_eq!(review.decision, Decision::AutoMerge);
    assert_eq!(e.apply_decision(&review).expect("apply"), StageOutcome::Advanced(Stage::Approve));
}

#[test]
fn the_ledger_names_the_commands_that_ran_before_the_approval_line() {
    let fx = committing_fixture();
    let runner = built(&fx, vec![("tests/a.txt", "x\n")]);
    fx.engine(&runner).checks_stage().expect("checks");
    approve(&fx);
    let text = ledger(&fx);
    let ran = text.find("checks ran: true").expect("a checks ran line");
    let approved = text.find("auto-merge approved").expect("the approval line");
    assert!(ran < approved, "{text}");
}

#[test]
fn the_ledger_says_none_when_no_command_ran() {
    let mut fx = committing_fixture();
    only_src_checks(&mut fx);
    let runner = built(&fx, vec![("a.md", "x\n")]);
    fx.engine(&runner).checks_stage().expect("checks");
    approve(&fx);
    assert!(ledger(&fx).contains("checks ran: none"), "{}", ledger(&fx));
}

#[test]
fn a_docs_only_job_is_not_stopped_by_the_gates_at_the_end() {
    let mut fx = committing_fixture();
    only_src_checks(&mut fx);
    fx.config.require_tests = true;
    let runner = built(&fx, vec![("docs/a.md", "x\n")]);
    fx.engine(&runner).checks_stage().expect("checks");
    approve(&fx);
    assert_eq!(fx.job().stage, Stage::Approve);
}
```

- [ ] **Step 2: Run to verify they fail:** `cargo test -j4 -p factory-engine --test gates the_ledger`. Expected: FAIL, `a checks ran line` is missing.

- [ ] **Step 3: Implement.** In the `AutoMerge` arm of `apply_decision`, after the no-checks gate, build the list from the final report (`report.results` commands joined with `, `, or `none`) and call `self.ruling(&format!("checks ran: {list}"))` before `set_stage(Stage::Approve, ...)` and the existing approval ruling. In `README.md` add the four settings (`code_paths`, `test_paths`, `test_markers`, `require_tests`) with their defaults to the settings table. In `.claude/software-factory.md` describe the `no-checks` and `no-tests` parks, where the gates run (checks stage and the final re-run), that a risky diff is left to `decide()`, and the `checks ran:` ledger line.

- [ ] **Step 4: Run to verify they pass:** `cargo test -j4 -p factory-engine --test gates` (14 tests). Then the whole affected crates once: `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-checks`, `cargo test -j4 -p factory-engine`, and `cargo check -j4`. Expected: pass. `cargo test -j4 -p factory-cli --test agent_lint` also passes, since `.claude/software-factory.md` changed.

Next plan: none
