# A changed path counts as covered when its rule plans no command

Ticket: https://github.com/Snehal1112/rocket-factory/issues/23

## Goal

Make the no-checks gate count a changed code path as covered only when a check command was planned for it, and reject a check template that expands to a cross product of crates and dependents.

## Overview

The no-checks gate parks a job when a changed code path has no check. Today it asks only whether some `[[checks]]` rule matches the path glob. A rule can match a path and still plan no command:

- A `{crate}` template expands to nothing for a path that no crate owns, for example a root-level file or a folder under `crates/` that is not a workspace member.
- A `{dependents}` template expands to nothing when `dependents_cap` cuts every dependent.

Such a job merges with no check run on that path. A second problem is a template that holds both `{crate}` and `{dependents}`: it expands to crates × dependents, so a small diff can plan a large number of commands.

## Current state

| Piece | Today | Reference |
|---|---|---|
| Coverage test | a path is covered when any rule's glob matches it, or `unchecked_paths` exempts it | `factory-core/src/gate.rs:52-59`, `config.rs:37-41` |
| Gate text | `no check covers N changed code paths: a, b (+k more)`, kind `no-checks` | `checks_stage.rs:17-33` |
| Gate call sites | after the checks pass, and in the final gate | `checks_stage.rs:268-271`, `review.rs:279-282` |
| Gate method | `gate_checks(&self, paths)` is infallible and builds no plan | `checks_stage.rs:116-118` |
| Planner | `plan_checks_in` returns a flat, de-duplicated command list with no link to paths | `factory-checks/src/plan.rs:76-132` |
| Expansion | `{crate}` takes the owning crate, or the folder after `crates/` without a map; `{dependents}` is capped by `dependents_cap` | `plan.rs:57-70,90-105` |
| Cross product | one template with both placeholders expands to crates × dependents | `plan.rs:107-120` |
| Plan for a diff | `plan_for` loads the workspace map and falls back without it | `checks_stage.rs:170-188` |
| Config check | `validate_config` refuses a cargo command without `-j4` or with `--workspace` | `plan.rs:20-54`, run at `project.rs:256` and engine start (`factory-engine/src/lib.rs:52`) |
| Docs | the gate and `{crate}` behaviour | `docs/reference/software-factory.md:195`, `project-toml-reference.md:230-231` |

Both `rocket` and `rocket-factory` use `crates/*/**` with `{crate}` commands (`project.toml`). No config or preset in this repo uses `{dependents}`.

## Non-goals

- No change to `unchecked_paths`, `code_paths` or the no-tests gate.
- No change to how many commands run or to the `dependents_cap` behaviour.
- No new park kind and no change to the `park.json` shape.
- No per-path coverage report in `factory costs` or the dashboard.

## Design

Flow: the checks stage plans the commands for the diff and gets, with the plan, the list of paths a rule matched but planned nothing for. The gate adds those paths to the uncovered ones, and the park reason marks them.

### 1. The planner reports unplanned paths

`CheckPlan` gets a new field `pub unplanned: Vec<String>`. In `plan_checks_in`, a rule that matches some changed paths and expands, after `validate_command`, to at least one command covers all the paths it matched. A command that another rule planned first still counts for this rule. After all rules, `unplanned` holds, in input order and without repeats, the paths that some rule matched and no matching rule covered. A path that no rule matches is not in `unplanned`, because the existing check already finds it.

Example: with the rule `paths = ["**"]`, `commands = ["cargo test -j4 -p {crate}"]` and no workspace map, the path `Cargo.toml` has no `crates/` segment, so `commands` is empty and `unplanned` is `["Cargo.toml"]`.

### 2. The gate counts them as uncovered

New in `gate.rs`: `pub struct Uncovered { pub path: String, pub unplanned: bool }` and `pub fn uncovered_paths(paths: &[String], config: &Config, unplanned: &[String]) -> Vec<Uncovered>`. It returns the code paths that no rule matches (`unplanned: false`) and the code paths in `unplanned`, minus every path `unchecked_paths` exempts. `uncovered_code_paths` stays and calls it with an empty `unplanned`. Both are exported from `factory-core`.

### 3. The engine builds the plan at the gate

`gate_checks(&self, paths)` becomes `Result<Option<(ParkKind, String)>, EngineError>`. It builds the plan with `plan_for(paths, &workdir)` for the job worktree, as `run_checks_now` does, and passes `plan.unplanned` to `no_checks_reason`. The two call sites propagate the error with `?`. An unplanned path parks the job as `no-checks` before the reviewer runs, like any uncovered path.

### 4. The park reason marks unplanned paths

`no_checks_reason(paths, unplanned, config)` keeps the text, the count and the five-name cap. A path in `unplanned` is written as `<path> (a rule matches but planned no command)`.

Example: `no check covers 2 changed code paths: Cargo.toml (a rule matches but planned no command), a.rs`.

### 5. A template with both placeholders is rejected

`validate_config` returns a new `CheckError::CrossProduct(String)` for a command that contains both `{crate}` and `{dependents}`. Its text is `a check command must not use both {crate} and {dependents}: <command>`. `factory project add` and `show` (`project.rs:256`) and the engine start-up show it like the other check errors.

### 6. Docs

`software-factory.md` (the `no-checks` bullet) says a path counts as covered only when a command was planned for it, and shows the new suffix. `project-toml-reference.md` says that a path whose `{crate}` or `{dependents}` rule plans no command is uncovered and can be exempted with `unchecked_paths`, and that one template must not use both placeholders.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| A `{crate}` rule matches a root-level code file | the path is `unplanned`, so the job parks `no-checks` with the suffix |
| A code file under `crates/<dir>/` that no workspace crate owns | parks `no-checks` with the suffix. The user exempts it with `unchecked_paths` |
| `dependents_cap = 0` and a rule with only `{dependents}` commands | its paths are `unplanned` |
| A rule with one `{dependents}` command and one plain command | the plain command covers the paths |
| A path matched by two rules, one plans a command | covered by the other rule |
| A rule plans only a command another rule already planned | still covered |
| A doc path with a rule that plans nothing | not code, never gated |
| An `unplanned` path that `unchecked_paths` exempts | not gated |
| Workspace map cannot be loaded | the planner uses the folder after `crates/`; paths it cannot resolve are `unplanned` |
| The worktree is missing at the gate | `gate_checks` returns the error, as `run_checks_now` does |
| A template with `{crate}` and `{dependents}` | refused at config validation, naming the command |

## Testing

`factory-checks` (`plan.rs` tests; the workspace fixture of `dependents_expand_and_cap` is reused): `a_crate_rule_for_a_root_file_plans_nothing_and_reports_the_path`, `dependents_over_the_cap_leave_the_paths_unplanned`, `a_path_with_a_planned_command_is_not_unplanned`, `a_second_rule_that_plans_a_command_covers_the_path`, `a_duplicate_command_still_counts_as_planned`, `a_path_no_rule_matches_is_not_unplanned`, `validate_config_rejects_crate_with_dependents`.

`factory-core` (`gate.rs` tests): `an_unplanned_path_is_uncovered_and_flagged`, `an_exempt_unplanned_path_is_not_uncovered`, `an_unplanned_doc_path_is_not_code`, `uncovered_code_paths_is_unchanged_without_a_plan`.

`factory-engine` (`checks_stage.rs` tests and `tests/gates.rs`): `no_checks_reason_marks_unplanned_paths`, `no_checks_reason_keeps_the_five_name_cap_with_marks`, `a_crate_rule_with_no_owner_parks_no_checks`, `a_path_covered_by_a_planned_command_is_not_gated`.

Commands: `cargo test -j4 -p factory-checks`, `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-engine`, then `cargo fmt -p <crate> -- --check` and `cargo clippy -j4 -p <crate> --all-targets -- -D warnings` for each of the three crates.

## Files changed

| File | Change |
|---|---|
| `crates/factory-checks/src/plan.rs` | `CheckPlan.unplanned`, the planner change, `validate_config` check, tests |
| `crates/factory-checks/src/lib.rs` | `CheckError::CrossProduct` |
| `crates/factory-core/src/gate.rs` | `Uncovered`, `uncovered_paths`, tests |
| `crates/factory-core/src/lib.rs` | exports |
| `crates/factory-engine/src/checks_stage.rs` | `gate_checks` builds the plan, `no_checks_reason` marks paths, tests |
| `crates/factory-engine/src/review.rs` | the final gate propagates the new error |
| `crates/factory-engine/tests/gates.rs` | the integration tests |
| `docs/reference/software-factory.md`, `docs/reference/project-toml-reference.md` | the new rule and the suffix |

## Constraints

- Always `-j4`, `-p <crate>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Rust changes pass `cargo fmt -p <crate> -- --check` and `cargo clippy -j4 -p <crate> --all-targets -- -D warnings` for each changed crate. No `#[allow(...)]` to silence a lint.
- No unwrap in production Rust. Tests use `.expect("...")` and `expect_err`.
- A missing or broken config still makes every path code and uncovered. Never default to permissive.
- No new board, ledger or events field. Park records keep their shape.
- New public items carry a doc comment. Code comments are short full sentences that end with a period.

## Acceptance criteria

- `plan_checks_in` reports, in `CheckPlan.unplanned`, each path that a rule matched and no matching rule planned a command for.
- A path covered by a rule that planned at least one command (also a duplicate) is not in `unplanned`.
- The gate treats an `unplanned` code path as uncovered unless `unchecked_paths` exempts it, and parks the job as `no-checks`.
- The park reason marks each such path with `(a rule matches but planned no command)` and keeps the count and the five-name cap.
- `uncovered_code_paths` returns the same result as before when given no plan.
- A template with both `{crate}` and `{dependents}` is refused by `validate_config` with `CheckError::CrossProduct` naming the command.
- Both reference docs describe the new rule.
- fmt and clippy `-D warnings` pass for `factory-checks`, `factory-core` and `factory-engine`.

## Out of scope

- Reporting coverage per path in `factory costs` or the dashboard.
- Changing how `{crate}` finds the owning crate.
- Changing the no-tests gate.

## Decisions

- How does the gate know that a command was planned for a path? Options: the planner reports the paths no matching rule planned a command for; guess in the gate from the `{crate}` placeholder and the path; fix only the root-file `{crate}` case. Chosen: the planner reports it, in a new `CheckPlan` field. Decided by: user
- What happens to a check template that uses both `{crate}` and `{dependents}`? Options: reject it at start-up with a new error; allow it and document the cross product; leave it. Chosen: reject it in `validate_config` with a new `CheckError::CrossProduct` that names the command. Decided by: user
- What does the park reason say about a path whose rule planned no command? Options: keep the kind and sentence and mark those paths with a suffix; keep the text unchanged; add a second sentence. Chosen: keep kind `no-checks` and the sentence, and mark each such path with `(a rule matches but planned no command)`. Decided by: user
- What about a code path that no workspace crate owns? Options: park it and let the user exempt it; let it pass when the workspace map loaded; park it only without a map. Chosen: park it, and the user exempts it with `unchecked_paths`. Decided by: user
- How does the gate get the plan? Options: build it again with `plan_for` in `gate_checks`; thread the plan from the checks run. Chosen: build it again in `gate_checks`, because the two call sites run at different times and `plan_for` only reads the workspace files. Decided by: call
- What happens to `uncovered_code_paths`? Options: keep it as a wrapper over the new function; change its signature. Chosen: keep it as a wrapper, so existing callers and tests do not change. Decided by: call
