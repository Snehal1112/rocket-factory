# Multi-review, settings and gates Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-12-multi-review-design.md
> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Add the `reviewers` and `security_rules` settings, the review roles on a job, the combined verdict, and the `format` and `lint` command lists of a check rule with their gate map.

**Architecture:** Pure rules in `factory-core` (settings, roles, verdict combine, board methods) and the check planner in `factory-checks`. Nothing here runs a reviewer; the engine plan uses these pieces.

**Tech Stack:** Rust, `serde`, `toml`.

## Global Constraints

- Commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`. Never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- A missing or broken config must make every diff risky; the new settings never default to permissive.
- New board, ledger and events fields are optional with defaults, and board JSON stays readable by `dashboard/`. This plan adds no board field: `reviews` keys stay strings.
- Only `git` may not be shelled out to. No `Cargo.toml` change and no new crate dependency.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- Comments are short full sentences that end with a punctuation mark.

## Review Focus

1. `reviewers = []` and `reviewers = ["security"]` are config errors, and the error names the allowed values. Pinned by `reviewers_parse_and_validate`.
2. A job created with `reviewers = ["code"]` has exactly the `code` entry and nothing else. Pinned by `set_reviewers_creates_one_pending_entry_per_role`.
3. `review_missing` ignores `format` and `lint` entries. Pinned by `review_missing_ignores_gate_keys`.
4. A rule whose only list is `lint` still covers its paths and expands `{crate}`. Pinned by `a_path_is_covered_by_a_lint_only_rule`.
5. An old check rule with only `paths` and `commands` still loads and plans as before. Pinned by `check_rule_without_gate_lists_still_loads`.

## File Structure

| File | Responsibility |
|---|---|
| `crates/factory-core/src/config.rs` | `reviewers`, `security_rules`, `CheckRule.format` and `.lint`, validation |
| `crates/factory-core/src/settings_block.rs`, `docs/reference/project-toml-reference.md` | Key list and docs for the new settings |
| `crates/factory-core/src/verdict.rs`, `lib.rs` | `REVIEW_ROLES`, `combine_verdicts`, re-exports |
| `crates/factory-core/src/board.rs` | Job roles, `review_missing`, `reset_reviews`, the missing-review message |
| `crates/factory-engine/src/runloop.rs` | `add_job` sets the roles from the config |
| `crates/factory-checks/src/plan.rs` | Gate lists in the plan and the gate map |
| `crates/factory-core/presets/{rust,ruby,node}/config.toml` | Move format and lint commands into the new lists |

### Task 1: Settings, roles and the combined verdict

**Files:**
- Modify: `crates/factory-core/src/config.rs` (`Config` at ~82 and `reviewer_rules` at ~145, `Default` at ~192 and ~230, the one-line list check at ~466-483, `Config::parse` end at ~497, tests from ~660)
- Modify: `crates/factory-core/src/settings_block.rs` (key list `GROUPS` near `reviewer_rules`), `crates/factory-core/src/verdict.rs`, `crates/factory-core/src/lib.rs:88`
- Modify: `docs/reference/project-toml-reference.md` (rows after `reviewer_rules` at ~106)

**Interfaces:**
- Consumes: the existing `Verdict { Approve, Changes, Missing }` and `CoreError::Config(String)`.
- Produces: `Config.reviewers: Vec<String>` (default `["code"]`), `Config.security_rules: Vec<String>` (default empty), `pub const REVIEW_ROLES: [&str; 2] = ["code", "security"]` and `pub fn combine_verdicts(verdicts: &[Verdict]) -> Verdict`, both re-exported from `factory_core`.

- [ ] **Step 1: Read the code and docs this task touches.** Read `Config`, `Default`, `Config::parse` and the tests `prompt_keys_default_to_empty_and_parse` and `prompt_lines_must_be_one_line_and_not_empty` in `config.rs`; all of `verdict.rs`; the `GROUPS` list and the test `every_config_key_is_listed_once` in `settings_block.rs`; the `reviewer_rules` row of `project-toml-reference.md`; and the spec "Design 1" and "Design 3".
- [ ] **Step 2: Write the failing tests.** In `config.rs` tests:

```rust
#[test]
fn reviewers_default_is_code_only() {
    assert_eq!(Config::default().reviewers, vec!["code".to_string()]);
    assert!(Config::default().security_rules.is_empty());
}

#[test]
fn reviewers_parse_and_validate() {
    let cfg = Config::parse("reviewers = [\"code\", \"security\"]\n").expect("parse");
    assert_eq!(cfg.reviewers, vec!["code".to_string(), "security".to_string()]);
    for bad in [
        "reviewers = []\n",
        "reviewers = [\"security\"]\n",
        "reviewers = [\"code\", \"ux\"]\n",
        "reviewers = [\"code\", \"code\"]\n",
    ] {
        let err = Config::parse(bad).expect_err(bad).to_string();
        assert!(err.contains("code") && err.contains("security"), "{bad}: {err}");
    }
}
```

  Add `security_rules_must_be_one_line_and_not_empty`, shaped like `prompt_lines_must_be_one_line_and_not_empty`. In `verdict.rs` tests:

| Test | Input | Expected |
|---|---|---|
| `combine_all_approve_is_approve` | `[Approve, Approve]` | `Approve` |
| `combine_any_changes_is_changes` | `[Approve, Changes, Missing]` | `Changes` |
| `combine_missing_without_changes_is_missing` | `[Approve, Missing]` | `Missing` |
| `combine_empty_is_missing` | `[]` | `Missing` |

- [ ] **Step 3: Run them and see them fail.** Run `cargo test -j4 -p factory-core reviewers` and `cargo test -j4 -p factory-core combine_`. Expected: `no field 'reviewers' on type 'Config'` and `cannot find function 'combine_verdicts'`.
- [ ] **Step 4: Implement the settings.** Add both fields with doc comments, defaults in `Default`, and in `Config::parse` a check that `reviewers` is non-empty, holds only values from `REVIEW_ROLES`, contains `"code"` and has no duplicate, with an error such as `reviewers: must be a list of unique roles from code, security and must include code`. Add `security_rules` to the existing one-line list check. Add both keys to `GROUPS` next to `reviewer_rules`, and two rows to `project-toml-reference.md`.
- [ ] **Step 5: Implement `REVIEW_ROLES` and `combine_verdicts`** in `verdict.rs` and re-export them in `lib.rs`. `combine_verdicts` returns `Approve` for a non-empty all-approve list, `Changes` when any entry is `Changes`, else `Missing`.
- [ ] **Step 6: Run the tests and see them pass.** Run `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-cli --test agent_lint --test check_coverage_docs` and `cargo clippy -j4 -p factory-core`. Expected: all pass, including `every_config_key_is_listed_once`.

### Task 2: Review roles on a job

**Files:**
- Modify: `crates/factory-core/src/board.rs` (`Job::new` at ~239, `approve_refusal` at ~313, `review_missing_message` at ~344, `review_missing` at ~352, `reset_reviews` at ~363, tests from ~492)
- Modify: `crates/factory-engine/src/runloop.rs` (`add_job` at ~18-60)
- Test: `crates/factory-engine/tests/new_job.rs`

**Interfaces:**
- Consumes: `REVIEW_ROLES` and `Config.reviewers` from the settings deliverable of this plan, and `ReviewState`.
- Produces: `Job::set_reviewers(&mut self, roles: &[String])`, `Job::missing_reviews(&self) -> Vec<&'static str>` (`"code"` when it is absent or has no verdict, then every other role in `REVIEW_ROLES` that is present in `reviews` and has no verdict, in `REVIEW_ROLES` order; an absent `code` key is always missing, as `board.rs` does today), `review_missing()` true when that list is not empty, and `reset_reviews()` that sets the roles back to `Pending` and removes every key that is not in `REVIEW_ROLES`. The logic sits behind no port; `add_job` reads `driver.config()`.

- [ ] **Step 1: Read the code this task touches.** Read the `Job` methods listed above, the board tests `review_missing_is_true_until_the_code_review_has_a_verdict` and `the_review_missing_message_names_the_problem_and_the_options`, `add_job` in `runloop.rs`, and `tests/new_job.rs` for how a fixture sets a config.
- [ ] **Step 2: Write the failing tests.** In `board.rs` tests:

```rust
#[test]
fn review_missing_needs_every_reviewer_role() {
    let mut j = job();
    j.set_reviewers(&["code".to_string(), "security".to_string()]);
    j.set_review("code", ReviewState::Pass);
    assert!(j.review_missing());
    assert_eq!(j.missing_reviews(), vec!["security"]);
    j.set_review("security", ReviewState::Skipped);
    assert!(!j.review_missing());
}
```

  Use the helper that the neighbouring tests use to build a job in place of `job()`. Add the cases below. In `tests/new_job.rs` add `a_new_job_gets_a_pending_review_per_configured_role`: configure `reviewers = ["code", "security"]`, create a job, and assert the sorted keys of `job.reviews` are `["code", "security"]` and both are `Pending`.

| Test | Setup | Expected |
|---|---|---|
| `set_reviewers_creates_one_pending_entry_per_role` | `set_reviewers(&["code"])` | `reviews` has only `code`, `Pending` |
| `review_missing_ignores_gate_keys` | `code` Pass, `format` Changes | `review_missing()` is false |
| `an_absent_code_review_is_missing` | `reviews` cleared (the existing "an absent review is missing too" case), then `reviews` holding only `security` Pass | `review_missing()` is true and `missing_reviews()` is `["code"]` both times |
| `reset_reviews_clears_gate_keys` | roles Pass, `format` and `lint` set | roles `Pending`, gate keys gone |
| `the_missing_message_names_the_roles` | only `security` missing | contains `has no security review yet` and `approve --without-review` |

  The existing message test for `code` keeps passing: a missing `code` alone reads `has no code review yet`. Both missing reads `has no code and security review yet`.

- [ ] **Step 3: Run them and see them fail.** Run `cargo test -j4 -p factory-core board::` and `cargo test -j4 -p factory-engine --test new_job`. Expected: `no method named 'set_reviewers'`.
- [ ] **Step 4: Implement the board methods.** `set_reviewers` replaces the role entries (keys in `REVIEW_ROLES`) with `Pending` entries for the given roles and leaves other keys alone. Rewrite `review_missing_message` to name the missing roles from `missing_reviews`, keeping the text after the role names exactly as today. `approve_refusal` keeps calling `review_missing`.
- [ ] **Step 5: Use it in `add_job`.** After `Job::new`, call `job.set_reviewers(&driver.config().reviewers)` in the same place where the job model is set.
- [ ] **Step 6: Run the tests and see them pass.** Run `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-engine --test new_job --test review_decide`, `cargo test -j4 -p factory-cli` and `cargo clippy -j4 -p factory-core -p factory-engine`. Expected: all pass.

### Task 3: Format and lint lists in the check planner

**Files:**
- Modify: `crates/factory-core/src/config.rs` (`CheckRule` at ~31)
- Modify: `crates/factory-checks/src/plan.rs` (`CheckPlan` at ~14, `validate_config` at ~53, `plan_checks_in` at ~85-150, `uses_dependents` at ~112, tests from ~178)
- Modify: `crates/factory-engine/src/checks_stage.rs` (`plan_for` at ~191-195, which decides whether to load the workspace map by scanning `r.commands` only)
- Modify: `crates/factory-core/presets/rust/config.toml` (lines ~25-45), `crates/factory-core/presets/ruby/config.toml` (~27), `crates/factory-core/presets/node/config.toml` (~31)
- Modify, only to build the new struct: the `CheckRule` literals in `factory-checks/src/plan.rs:181`, `factory-engine/src/checks_stage.rs:332`, `permissions.rs:169`, `factory-core/src/gate.rs:221`, and in `factory-engine/tests/` the files `common/mod.rs:116`, `gates.rs`, `park_evidence.rs`, `review_decide.rs`, `fix_fresh.rs`, `escalation.rs`, `build_checks.rs`

**Interfaces:**
- Consumes: the planner (`plan_checks_in(changed_paths, cfg, ws) -> CheckPlan`) and the `{crate}` and `{dependents}` templating.
- Produces: `CheckRule { paths, commands, format, lint }` with `#[serde(default)]` on the two new lists and `#[derive(Default)]`; `CheckPlan.gates: BTreeMap<String, String>` mapping each planned command to `"format"` or `"lint"` (commands from `commands` have no entry) and `CheckPlan::gate_of(&self, command: &str) -> Option<&str>`. Within a rule the order is `commands`, then `format`, then `lint`. `CheckResult` and `ChecksReport` do not change.

- [ ] **Step 1: Read the code this task touches.** Read the whole planner in `plan.rs`, the tests `plan_is_ordered_and_deduplicated`, `a_second_rule_that_plans_a_command_covers_the_path` and the helpers `cfg`, `paths`, `rust_preset`; the three preset files; the test `rocket_parity.rs` and `rocketvault_registration.rs` in `factory-core/tests/`; and the spec "Design 2".
- [ ] **Step 2: Write the failing tests** in `plan.rs`, using the `cfg` helper extended to take the two lists (or a `CheckRule { .., ..CheckRule::default() }` literal):

```rust
#[test]
fn format_and_lint_commands_are_planned_after_commands() {
    let rule = CheckRule {
        paths: vec!["crates/*/**".into()],
        commands: vec!["cargo check -j4 -p {crate}".into()],
        format: vec!["cargo fmt -p {crate} -- --check".into()],
        lint: vec!["cargo clippy -j4 -p {crate} --all-targets -- -D warnings".into()],
    };
    let plan = plan_for_rule_and_paths(rule, &["crates/foo/src/lib.rs"]);
    assert_eq!(plan.commands, vec![
        "cargo check -j4 -p foo".to_string(),
        "cargo fmt -p foo -- --check".to_string(),
        "cargo clippy -j4 -p foo --all-targets -- -D warnings".to_string(),
    ]);
    assert_eq!(plan.gate_of("cargo fmt -p foo -- --check"), Some("format"));
    assert_eq!(plan.gate_of("cargo check -j4 -p foo"), None);
}
```

  Use the existing helpers to build the workspace and config in place of `plan_for_rule_and_paths`. Add:

| Test | Input | Expected |
|---|---|---|
| `a_path_is_covered_by_a_lint_only_rule` | rule with only `lint = ["cargo clippy -j4 -p {crate}"]` | the path is covered and the command is planned |
| `gate_lists_expand_dependents` | `lint = ["cargo test -j4 -p {dependents}"]` | planned once per capped dependent, as in `commands` |
| `an_invalid_cargo_command_in_a_gate_list_is_refused` | `format = ["cargo fmt --workspace"]` | `validate_config` returns an error |
| `rust_preset_marks_fmt_and_clippy_as_gates` | `rust_preset()` and a changed crate file | `gate_of` is `format` for fmt and `lint` for clippy |

  In `config.rs` tests add `check_rule_without_gate_lists_still_loads` (a rule with only `paths` and `commands`, both new lists empty) and `check_rule_gate_lists_load`.

- [ ] **Step 3: Run them and see them fail.** Run `cargo test -j4 -p factory-checks` and `cargo test -j4 -p factory-core check_rule`. Expected: `struct 'CheckRule' has no field named 'format'`.
- [ ] **Step 4: Implement `CheckRule`.** Add the two defaulted lists and `Default`. Fix every literal named in Files with `..CheckRule::default()`. Keep `deny_unknown_fields`.
- [ ] **Step 5: Implement the planner changes.** `validate_config` and the `{dependents}` scan include the two lists. `plan_checks_in` iterates `commands`, `format`, `lint` of each rule with the same expansion, filtering and de-duplication, and records the gate of each planned command in `CheckPlan.gates` (the first rule that plans a command decides). A path counts as covered when any of the three lists plans a command for it. In `plan_for` of `checks_stage.rs`, scan `commands`, `format` and `lint` of each rule when deciding whether to load the workspace map, so a rule that uses `{crate}` only in `lint` still expands per crate in a real run. Add the engine test `a_crate_template_only_in_lint_expands_in_a_real_run` to `crates/factory-engine/tests/build_checks.rs`: a rule with `lint = ["cargo clippy -j4 -p {crate}"]` and a changed file of a workspace crate plans `cargo clippy -j4 -p <crate>` and not the folder fallback.
- [ ] **Step 6: Move the preset commands.** In `rust`, move both `cargo fmt` lines into `format` and both `cargo clippy` lines into `lint`, in both rules. In `ruby` move `bundle exec rubocop` into `lint`. In `node` move `bun run lint` into `lint`. Leave `go` and `csharp` as they are, since neither runs a formatter or linter today. Run `cargo test -j4 -p factory-core --test rocket_parity`; if it fails only because the preset's `commands` no longer hold the moved lines, make it compare `commands`, `format` and `lint` joined in that order.
- [ ] **Step 7: Run the tests and see them pass.** Run `cargo test -j4 -p factory-checks`, `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-engine` and `cargo clippy -j4 -p factory-core -p factory-checks -p factory-engine`. Expected: all pass.

Next plan: docs/superpowers/plans/2026-11-12-multi-review/plan-02-multi-review.md
