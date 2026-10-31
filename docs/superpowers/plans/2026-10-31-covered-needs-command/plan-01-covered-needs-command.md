# Covered paths need a command Implementation Plan

> **Spec:** docs/superpowers/specs/2026-10-31-covered-needs-command-design.md
>
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Count a changed code path as covered only when a check command was planned for it, and refuse a check template that uses both `{crate}` and `{dependents}`.

**Architecture:** `factory-checks` decides per path whether a command was planned and reports the rest in `CheckPlan.unplanned`. `factory-core` gets a pure `uncovered_paths` that adds those paths to the uncovered ones. `factory-engine` builds the plan at the gate and marks the paths in the park reason.

**Tech Stack:** Rust, `cargo test -j4 -p <crate>`, the `Fixture` helpers in `crates/factory-engine/tests/common/mod.rs`.

## Global Constraints

- Always `-j4` and `-p <crate>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Each task ends with `cargo fmt -p <crate> -- --check` and `cargo clippy -j4 -p <crate> --all-targets -- -D warnings` for every crate it changed. No `#[allow(...)]` to silence a lint.
- No unwrap in production Rust. Tests use `.expect("...")` and `expect_err`.
- A missing or broken config still makes every path code and uncovered. Never default to permissive.
- No new board, ledger or events field. Park records keep their shape. Port: none. The gate calls `plan_for` and `factory_checks` directly, which CLAUDE.md says are called directly, not behind a trait.
- New public items carry a doc comment. Code comments are short full sentences that end with a period.
- Exact texts: ` (a rule matches but planned no command)` after the path, and `a check command must not use both {crate} and {dependents}: <command>`.

## Review Focus

1. One `{crate}` rule that matches a crate path and a root path must cover only the crate path. Pinned by `a_rule_that_plans_for_one_path_leaves_an_unowned_path_unplanned`.
2. A command that another rule planned first must still count as planned for the second rule. Pinned by `a_duplicate_command_still_counts_as_planned`.
3. `unchecked_paths` must exempt an unplanned path. Pinned by `an_exempt_unplanned_path_is_not_uncovered`.
4. The five-name cap and the count must hold when some names carry the mark. Pinned by `no_checks_reason_keeps_the_five_name_cap_with_marks`.
5. Separate `{crate}` and `{dependents}` commands in one rule must stay valid. Pinned by `validate_config_accepts_crate_and_dependents_in_separate_commands`.

## File Structure

| File | Responsibility |
|---|---|
| `crates/factory-checks/src/plan.rs` | `CheckPlan.unplanned`, the per-path coverage in `plan_checks_in`, the `validate_config` check |
| `crates/factory-checks/src/lib.rs` | `CheckError::CrossProduct` |
| `crates/factory-core/src/gate.rs` | `Uncovered`, `uncovered_paths`, the `uncovered_code_paths` wrapper |
| `crates/factory-core/src/lib.rs` | exports |
| `crates/factory-engine/src/checks_stage.rs` | `gate_checks` builds the plan, `no_checks_reason` marks paths |
| `crates/factory-engine/src/review.rs` | the final gate propagates the new error |
| `crates/factory-engine/tests/gates.rs` | integration tests |
| `docs/reference/software-factory.md`, `docs/reference/project-toml-reference.md` | the new rule |

### Task 1: The planner reports unplanned paths and refuses the cross product

**Files:**
- Modify: `crates/factory-checks/src/plan.rs`, `crates/factory-checks/src/lib.rs`
- Test: `crates/factory-checks/src/plan.rs` (tests module)

**Interfaces:**
- Produces: `CheckPlan.unplanned: Vec<String>` (in input order, no repeats); `CheckError::CrossProduct(String)` with the text `a check command must not use both {crate} and {dependents}: {0}`.

- [ ] **Step 1: Read the code this task touches**

Read `plan.rs` in full (`validate_command`, `validate_config`, `crate_of`, `plan_checks_in` at lines 76-132, and the tests module with `cfg`, `paths` and `ws`), and `CheckError` in `lib.rs`. Find every `CheckPlan { ... }` literal with `grep -rn "CheckPlan {" crates` so the new field can be added.

- [ ] **Step 2: Write the failing tests**

In the `plan.rs` tests module, reusing `cfg`, `paths` and `ws()`:

```rust
#[test]
fn a_crate_rule_for_a_root_file_plans_nothing_and_reports_the_path() {
    let c = cfg(vec![(&["**"], &["cargo test -j4 -p {crate}"])]);
    let plan = plan_checks_in(&paths(&["Cargo.toml"]), &c, None);
    assert!(plan.commands.is_empty());
    assert_eq!(plan.unplanned, paths(&["Cargo.toml"]));
}

#[test]
fn dependents_over_the_cap_leave_the_paths_unplanned() {
    let mut c = cfg(vec![(&["crates/**"], &["cargo test -j4 -p {dependents}"])]);
    c.dependents_cap = 0;
    let plan = plan_checks_in(&paths(&["crates/core/src/a.rs"]), &c, Some(&ws()));
    assert!(plan.commands.is_empty());
    assert_eq!(plan.unplanned, paths(&["crates/core/src/a.rs"]));
}

#[test]
fn a_rule_that_plans_for_one_path_leaves_an_unowned_path_unplanned() {
    let c = cfg(vec![(&["**"], &["cargo test -j4 -p {crate}"])]);
    let plan = plan_checks_in(&paths(&["crates/core/src/a.rs", "Cargo.toml"]), &c, None);
    assert_eq!(plan.commands, ["cargo test -j4 -p core"]);
    assert_eq!(plan.unplanned, paths(&["Cargo.toml"]));
}

#[test]
fn validate_config_rejects_crate_with_dependents() {
    let c = cfg(vec![(&["crates/**"], &["cargo test -j4 -p {crate} -p {dependents}"])]);
    let err = validate_config(&c).expect_err("cross product");
    assert!(matches!(err, crate::CheckError::CrossProduct(_)), "{err:?}");
    assert!(err.to_string().contains("{crate} and {dependents}"), "{err}");
    assert!(err.to_string().contains("cargo test -j4 -p {crate} -p {dependents}"), "{err}");
}
```

| Test | Setup | Assertions |
|---|---|---|
| `a_path_with_a_planned_command_is_not_unplanned` | rule `crates/**` with `cargo test -j4 -p {crate}`, path `crates/core/src/a.rs`, `Some(&ws())` | `unplanned` is empty and the command is `cargo test -j4 -p core` |
| `a_second_rule_that_plans_a_command_covers_the_path` | rules `**` with the `{crate}` command and `Cargo.toml` with `cargo check -j4`, path `Cargo.toml`, `None` | `commands == ["cargo check -j4"]` and `unplanned` is empty |
| `a_duplicate_command_still_counts_as_planned` | rules `crates/**` with `cargo check -j4` and `crates/core/**` with `cargo check -j4`, path `crates/core/a.rs` | one command, `unplanned` empty |
| `a_path_no_rule_matches_is_not_unplanned` | rule `docs/**` with `yarn check`, path `src/a.ts` | no commands, `unplanned` empty |
| `validate_config_accepts_crate_and_dependents_in_separate_commands` | rule `crates/**` with `cargo test -j4 -p {crate}` and `cargo test -j4 -p {dependents}` | `validate_config` is `Ok` |

- [ ] **Step 3: Run the tests to see them fail**

Run: `cargo test -j4 -p factory-checks plan::tests`
Expected: compile errors `no field 'unplanned' on type 'CheckPlan'` and `no variant named 'CrossProduct'`.

- [ ] **Step 4: Implement**

Add `pub unplanned: Vec<String>` with a doc comment to `CheckPlan` and fill it in `plan_checks_in`. For each rule with matched paths, a matched path is covered when the rule plans a command for it: a template without placeholders that passes `validate_command` covers every matched path; a `{crate}` template covers a matched path that has an owning crate (`ws.owner(path)` with a map, else `crate_of(path)`) and whose expansion passes `validate_command`; a `{dependents}` template covers the matched paths when its expansion is non-empty after the cap. A command already in `seen` still counts. Collect covered paths in a set; `unplanned` is the input paths that some rule matched and no rule covered, in input order without repeats. Add `CheckError::CrossProduct(String)` to `lib.rs` (`#[error("a check command must not use both {{crate}} and {{dependents}}: {0}")]`), and make `validate_config` return it for a command template that contains both placeholders, before the existing checks.

- [ ] **Step 5: Run the tests to see them pass**

Run: `cargo test -j4 -p factory-checks`
Expected: all pass, the nine new tests included, and the existing plan tests unchanged.

- [ ] **Step 6: Format and lint**

Run `cargo fmt -p factory-checks -- --check` and `cargo clippy -j4 -p factory-checks --all-targets -- -D warnings`. Fix any finding, then run the same command once to confirm.

### Task 2: The pure gate adds the unplanned paths

**Files:**
- Modify: `crates/factory-core/src/gate.rs`, `crates/factory-core/src/lib.rs`
- Test: `crates/factory-core/src/gate.rs` (tests module)

**Interfaces:**
- Produces: `pub struct Uncovered { pub path: String, pub unplanned: bool }` (derives `Debug`, `Clone`, `PartialEq`, `Eq`) and `pub fn uncovered_paths(paths: &[String], config: &Config, unplanned: &[String]) -> Vec<Uncovered>`. Both exported from `factory-core`. `uncovered_code_paths(paths, config)` keeps its signature and calls `uncovered_paths` with an empty `unplanned`.

- [ ] **Step 1: Read the code this task touches**

Read `gate.rs` in full (`code_paths_of`, `matches_any`, `uncovered_code_paths` and the tests with the `rule` and `strings` helpers) and the `pub use gate::{...}` line in `lib.rs:62`.

- [ ] **Step 2: Write the failing tests**

In the `gate.rs` tests module, reusing `cfg`, `rule` and `strings`:

```rust
#[test]
fn an_unplanned_path_is_uncovered_and_flagged() {
    let mut c = cfg();
    c.checks = vec![rule("**")];
    let got = uncovered_paths(
        &strings(&["Cargo.toml", "src/a.rs"]),
        &c,
        &strings(&["Cargo.toml"]),
    );
    assert_eq!(
        got,
        vec![Uncovered { path: "Cargo.toml".into(), unplanned: true }]
    );
}

#[test]
fn an_exempt_unplanned_path_is_not_uncovered() {
    let mut c = cfg();
    c.checks = vec![rule("**")];
    c.unchecked_paths = vec!["Cargo.toml".into()];
    let got = uncovered_paths(&strings(&["Cargo.toml"]), &c, &strings(&["Cargo.toml"]));
    assert!(got.is_empty(), "{got:?}");
}
```

| Test | Setup | Assertions |
|---|---|---|
| `an_unplanned_doc_path_is_not_code` | `uncovered_paths(&strings(&["README.md"]), &cfg(), &strings(&["README.md"]))` | the result is empty |
| `uncovered_paths_keep_input_order_and_both_kinds` | no rules; paths `a.rs` and `Cargo.toml`; unplanned `["Cargo.toml"]` | `[Uncovered { path: "a.rs", unplanned: false }, Uncovered { path: "Cargo.toml", unplanned: true }]` |
| `uncovered_code_paths_is_unchanged_without_a_plan` | the setup of `uncovered_paths_skip_rule_matches` | `uncovered_code_paths` gives `["Cargo.lock"]` as before |

- [ ] **Step 3: Run the tests to see them fail**

Run: `cargo test -j4 -p factory-core gate::tests`
Expected: compile errors `cannot find function 'uncovered_paths' in this scope` and `cannot find struct 'Uncovered'`.

- [ ] **Step 4: Implement**

`uncovered_paths` walks `code_paths_of(paths, config)` in order, drops every path that `unchecked_paths` exempts, returns `Uncovered { path, unplanned: true }` for a path found in `unplanned`, `Uncovered { path, unplanned: false }` for a path no rule matches, and nothing otherwise. `uncovered_code_paths` maps its result to the path strings. Export `Uncovered` and `uncovered_paths` in `lib.rs`.

- [ ] **Step 5: Run the tests to see them pass**

Run: `cargo test -j4 -p factory-core`
Expected: all pass, the four new tests included.

- [ ] **Step 6: Format and lint**

Run `cargo fmt -p factory-core -- --check` and `cargo clippy -j4 -p factory-core --all-targets -- -D warnings`. Fix any finding, then run the same command once to confirm.

### Task 3: The engine gate uses the plan, and the docs

**Files:**
- Modify: `crates/factory-engine/src/checks_stage.rs`, `crates/factory-engine/src/review.rs`, `docs/reference/software-factory.md`, `docs/reference/project-toml-reference.md`
- Test: `crates/factory-engine/src/checks_stage.rs` (tests module), `crates/factory-engine/tests/gates.rs`

**Interfaces:**
- Consumes: `CheckPlan.unplanned` and `uncovered_paths` from the earlier tasks.
- Produces: `fn no_checks_reason(paths: &[String], unplanned: &[String], config: &Config) -> Option<String>`; `fn gate_checks(&self, paths: &[String]) -> Result<Option<(ParkKind, String)>, EngineError>`.

- [ ] **Step 1: Read the code this task touches**

Read `no_checks_reason` and `code_path_list` (`checks_stage.rs:17-33`), `gate_checks` and `park_gate` (`:116-137`), `plan_for` (`:170-188`), the gate call at `:268-271` and in `review.rs:279-282`, the existing `no_checks_reason` tests in the `checks_stage.rs` tests module, and `a_code_diff_with_no_mapped_check_parks_as_no_checks_before_the_review` in `tests/gates.rs` (with `built`, `committing_fixture` and `CheckRule`).

- [ ] **Step 2: Write the failing tests**

In the `checks_stage.rs` tests module (existing calls `no_checks_reason(&paths, &config)` must be updated to pass `&[]` as the second argument):

```rust
#[test]
fn no_checks_reason_marks_unplanned_paths() {
    let reason = no_checks_reason(
        &paths(&["a.rs", "Cargo.toml"]),
        &paths(&["Cargo.toml"]),
        &Config::default(),
    )
    .expect("a reason");
    assert_eq!(
        reason,
        "no check covers 2 changed code paths: a.rs, Cargo.toml (a rule matches but planned no command)"
    );
}

#[test]
fn no_checks_reason_keeps_the_five_name_cap_with_marks() {
    let all = paths(&["a.rs", "b.rs", "c.rs", "d.rs", "e.rs", "f.rs", "g.rs"]);
    let reason = no_checks_reason(&all, &paths(&["b.rs"]), &Config::default()).expect("a reason");
    assert!(reason.starts_with("no check covers 7 changed code paths: "), "{reason}");
    assert!(reason.contains("b.rs (a rule matches but planned no command)"), "{reason}");
    assert!(reason.ends_with("(+2 more)"), "{reason}");
}
```

In `tests/gates.rs`, built like `a_code_diff_with_no_mapped_check_parks_as_no_checks_before_the_review`:

| Test | Setup | Assertions |
|---|---|---|
| `a_crate_rule_with_no_owner_parks_no_checks` | `fx.config.checks = vec![CheckRule { paths: vec!["**".into()], commands: vec!["cargo check -j4 -p {crate}".into()] }]`; `built(&fx, vec![("a.rs", "x\n")])`; `checks_stage()` | `NeedsHuman` whose reason contains `a.rs (a rule matches but planned no command)`; `ParkRecord::read` kind is `NoChecks` |
| `a_path_covered_by_a_planned_command_is_not_gated` | rule `paths: ["**"]`, `commands: ["true"]`; same build | `checks_stage()` returns `Advanced(Stage::Review)` |

- [ ] **Step 3: Run the tests to see them fail**

Run: `cargo test -j4 -p factory-engine no_checks_reason` and `cargo test -j4 -p factory-engine --test gates covered`
Expected: compile errors about the wrong number of arguments to `no_checks_reason`; the integration tests fail because the job still advances to review.

- [ ] **Step 4: Implement**

`no_checks_reason` builds its list from `uncovered_paths(paths, config, unplanned)`; the text, the count and the five-name cap stay, and an `unplanned` path is written as `<path> (a rule matches but planned no command)`. `gate_checks` becomes fallible: it builds `let workdir = self.repo.workdir_in(&self.worktree()?);`, then `let plan = self.plan_for(paths, &workdir);` and passes `&plan.unplanned`. The two callers (`checks_stage.rs` near line 269 and `review.rs` near line 280) propagate the error with `?`. In `software-factory.md` (the `no-checks` bullet near line 195) say that a path counts as covered only when a command was planned for it and show the suffix; in `project-toml-reference.md` (near lines 230-231) say that a path whose `{crate}` or `{dependents}` rule plans no command is uncovered and can be exempted with `unchecked_paths`, and that one command must not use both placeholders.

- [ ] **Step 5: Run the tests to see them pass**

Run: `cargo test -j4 -p factory-engine`
Expected: all pass, the four new tests included, and the existing gate tests (`an_exempt_path_does_not_park`, `the_final_checks_gate_parks_an_approved_job`) unchanged.

- [ ] **Step 6: Format and lint**

Run `cargo fmt -p factory-engine -- --check` and `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`. Fix any finding, then run the same command once to confirm.

Next plan: none
