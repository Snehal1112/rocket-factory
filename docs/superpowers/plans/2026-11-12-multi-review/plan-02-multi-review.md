# Multi-review, engine review stage Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-12-multi-review-design.md
> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Record format and lint results on the board, build the security reviewer spec, and make the review stage run every configured role one after the other and merge the replies into one review file.

**Architecture:** The review logic stays in `Engine` behind the existing `AgentRunner` port. The planner's gate map from the previous plan feeds the board entries. The combined verdict is passed to `decide`, which does not change.

**Tech Stack:** Rust, the `AgentRunner` port with `ScriptedRunner` in tests.

## Global Constraints

- Commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`. Never `--workspace` or `--all`.
- `decide.rs` does not change. Agents never write the board, ledger or events; only the driver does.
- Board JSON stays readable by `dashboard/`: `reviews` keys stay strings and no board field is added. A new ledger or events line is plain text.
- A task that touches `factory-engine` names the port its logic sits behind; here that is `AgentRunner` for agent runs and `Engine::update` for the board.
- With `reviewers = ["code"]` the review file, the board and the status are exactly what they are today.
- No `Cargo.toml` change. Tests use `.expect("...")` and `expect_err`, never the unwrap call.

## Review Focus

1. A security `[blocker]` with a code approve starts a fix round whose findings contain the security section. Pinned by `security_changes_start_a_fix_round_holding_its_findings`.
2. Both roles run even when the first asks for changes. Pinned by `both_roles_run_when_the_first_asks_for_changes`.
3. A code-only config writes the review file byte for byte as the reply. Pinned by `a_code_only_config_writes_the_review_file_as_before`.
4. The `Role verdict:` lines are never read as the verdict. Pinned by `the_review_file_has_one_section_per_role_and_one_verdict_line`.
5. A missing security agent file parks with a clear reason and runs no agent. Pinned by `a_missing_security_agent_parks_with_a_clear_reason`.

## File Structure

| File | Responsibility |
|---|---|
| `crates/factory-engine/src/checks_stage.rs` | `record_gates`, called from `run_checks_now` |
| `crates/factory-engine/src/specs.rs` | Shared review spec builder, `SECURITY_AGENT`, the security spec |
| `crates/factory-core/src/config.rs`, `crates/factory-engine/src/codex_prep.rs` | `security-review` handled like `reviewer` |
| `crates/factory-engine/src/review.rs` | Role loop, merged file, combined verdict, missing-agent decision |
| `crates/factory-engine/tests/multi_review.rs` | New tests for all three deliverables |

### Task 1: Gate results on the board

**Files:**
- Modify: `crates/factory-engine/src/checks_stage.rs` (`run_checks_now` at ~81-92)
- Create: `crates/factory-engine/tests/multi_review.rs`

**Interfaces:**
- Consumes: `CheckPlan::gate_of(&self, command: &str) -> Option<&str>`, `ChecksReport.results` (`CheckResult { command, passed, .. }`) and `Job::set_review`.
- Produces: `fn record_gates(&self, plan: &CheckPlan, report: &ChecksReport) -> Result<(), EngineError>` on `Engine`, called at the end of `run_checks_now`, so the checks stage, the final re-run and the nothing-changed re-run all record. For each gate (`format`, `lint`) with at least one result in the report, `reviews[gate]` is `Pass` when every result of that gate passed and `Changes` otherwise; a gate with no result has its entry removed. The board write goes through `Engine::update`.

- [ ] **Step 1: Read the code this task touches.** Read `run_checks_now`, `plan_for` and `checks_stage` in `checks_stage.rs`, the `Fixture` and its `engine(&runner)` in `tests/common/mod.rs`, and how `tests/build_checks.rs` gives the fixture a config with check rules.
- [ ] **Step 2: Write the failing tests** in the new `tests/multi_review.rs`. Build a fixture whose only rule is `CheckRule { paths: vec!["**".into()], commands: vec![], format: vec!["true".into()], lint: vec!["false".into()], }` and run the checks stage over a changed file as `build_checks.rs` does.

```rust
#[test]
fn format_and_lint_results_land_on_the_board_after_the_checks() {
    let fx = fixture_with_gates(&["true"], &["false"]);
    let job = run_checks_over_one_file(&fx);
    assert_eq!(job.reviews.get("format"), Some(&ReviewState::Pass));
    assert_eq!(job.reviews.get("lint"), Some(&ReviewState::Changes));
}
```

  Write `fixture_with_gates` and `run_checks_over_one_file` as small helpers in the test file. Add:

| Test | Setup | Expected |
|---|---|---|
| `a_gate_with_no_commands_has_no_entry` | rule with only `commands` | no `format` or `lint` key |
| `a_new_round_clears_the_gate_entries` | gates recorded, then `fix_or_park` starts a fix round | gate keys gone, roles `Pending` |
| `the_final_checks_record_the_gates_again` | call `final_checks()` after a pass | entries still `Pass` |

- [ ] **Step 3: Run them and see them fail.** Run `cargo test -j4 -p factory-engine --test multi_review`. Expected: the first test fails with `left: None`.
- [ ] **Step 4: Implement `record_gates`** and call it at the end of `run_checks_now` after the report is written. A board write failure is an `EngineError`, as for the other `update` calls.
- [ ] **Step 5: Run the tests and see them pass.** Run `cargo test -j4 -p factory-engine --test multi_review --test build_checks --test gates` and `cargo clippy -j4 -p factory-engine`. Expected: all pass.

### Task 2: The security reviewer spec and its role

**Files:**
- Modify: `crates/factory-engine/src/specs.rs` (`REVIEWER_AGENT` at ~15, `reviewer_spec_with_model` at ~551-679, `reviewer_spec` at ~518)
- Modify: `crates/factory-core/src/config.rs` (`plugins_for`, the `"reviewer"` arm at ~271), `crates/factory-engine/src/codex_prep.rs` (`skill_allow_for` and `sandbox_for` at ~85-102)
- Test: `crates/factory-engine/tests/multi_review.rs`

**Interfaces:**
- Consumes: `Config.security_rules`, `ReviewerExtra`, the existing prompt pieces and `with_limits`.
- Produces: `pub(crate) const SECURITY_AGENT: &str = "factory-security-reviewer"`; a private `fn review_spec(&self, role: &str, agent: &str, cwd: &Path, diff: &Path, names: &Path, round: u32, rules: &[String], extra: &ReviewerExtra, model: &str) -> AgentSpec` that holds today's body; `reviewer_spec_with_model` keeps its signature and calls it with role `"reviewer"` and `REVIEWER_AGENT`; `pub(crate) fn security_spec_with_model(&self, cwd: &Path, diff: &Path, names: &Path, round: u32, extra: &ReviewerExtra, model: &str) -> AgentSpec` calls it with role `"security-review"`, `SECURITY_AGENT` and `self.config.security_rules`. The port is `AgentRunner`: the spec is data and no agent runs here.

- [ ] **Step 1: Read the code this task touches.** Read `reviewer_spec_with_model` in full, `slim_prompt`, `agent_rules_current`, the role matches in `plugins_for`, `skill_allow_for` and `sandbox_for`, and the tests `reviewer_spec_is_read_only_and_tells_it_to_page_the_diff` (review_decide.rs) and `slim_prompts.rs` that build a reviewer spec.
- [ ] **Step 2: Write the failing tests** in `tests/multi_review.rs`:

```rust
#[test]
fn the_security_spec_uses_its_own_agent_role_and_rules() {
    let (fx, extra, diff) = review_inputs_with_security_rules(&["No shell calls."]);
    let spec = fx.engine(&runner()).security_spec_with_model(
        &fx.root, &diff.diff, &diff.names, 1, &extra, "claude-sonnet-5-5",
    );
    assert_eq!(spec.agent.as_deref(), Some("factory-security-reviewer"));
    assert_eq!(spec.role, "security-review");
    assert_eq!(spec.tools.as_deref(), Some("Read,Grep,Glob"));
    assert!(spec.prompt.contains("No shell calls."));
}
```

  Build the inputs the way `reviewer_spec_is_read_only_and_tells_it_to_page_the_diff` does. Adjust the field types to the real `AgentSpec`. Add:

| Test | Assertion |
|---|---|
| `a_code_spec_is_unchanged` | the existing reviewer spec tests still pass with role `"reviewer"` and agent `factory-code-reviewer` |
| `security_review_gets_the_reviewer_plugins_and_sandbox` | `config.plugins_for("security-review") == config.plugins_for("reviewer")`; the codex `skill_allow_for` and `sandbox_for` answers match for the two roles |

- [ ] **Step 3: Run them and see them fail.** Run `cargo test -j4 -p factory-engine --test multi_review security_`. Expected: `no method named 'security_spec_with_model'`.
- [ ] **Step 4: Refactor and add the spec.** Move the body of `reviewer_spec_with_model` into `review_spec` with the role and agent as parameters (`AgentSpec::new(&self.job_id, role, ..)` and `spec.agent = Some(agent)`, and `self.slim_prompt(agent)`), keep the `reviewer_spec_with_model` signature, and add `security_spec_with_model`. Match `"security-review"` wherever the driver matches `"reviewer"` in the three places named in Files.
- [ ] **Step 5: Run the tests and see them pass.** Run `cargo test -j4 -p factory-engine` and `cargo test -j4 -p factory-core` and `cargo clippy -j4 -p factory-core -p factory-engine`. Expected: all pass, including the older reviewer spec tests.

### Task 3: The role loop, the merged file and the fix round

**Files:**
- Modify: `crates/factory-engine/src/review.rs` (`review_stage_as` at ~45-212, `apply_decision` at ~227)
- Test: `crates/factory-engine/tests/multi_review.rs`

**Interfaces:**
- Consumes: `security_spec_with_model`, `reviewer_spec_with_model`, `combine_verdicts`, `REVIEW_ROLES`, `Job.reviews`, `parse_verdict`, `open_blockers`, `Engine::run_agent`, `Engine::update` and the gate and role entries from the previous deliverables.
- Produces: `review_stage_as` runs, in `REVIEW_ROLES` order, every role found in `job.reviews` (only `code` for `ReviewRun::Sync`), each role set to `Running` before its run and to `Pass` or `Changes` after it; the `security` role is set to `Skipped` without a run when no changed path of the round is a code path (use the predicate the no-checks gate in `gate_checks` uses to exempt non-code paths). A pure `fn merge_replies(parts: &[RolePart], combined: Verdict) -> String` where `RolePart { role: &str, text: String }`. `ReviewOutcome.verdict` is the combined verdict; `last_review.blockers` counts open blockers in the file text; `last_review.file` stays `review-code-r<N>.md`. The agent runs go through the `AgentRunner` port via `run_agent`, one after the other.

- [ ] **Step 1: Read the code this task touches.** Read `review_stage_as` and `apply_decision` in full, `open_blockers` and `parse_verdict` in `verdict.rs`, and the sync review test `a_sync_review_keeps_the_round_review` in `tests/sync_review.rs`. Note the values computed once before the spec is built and the ones computed per role.
- [ ] **Step 2: Write the failing tests.** Use the `ScriptedRunner` effects by call order, which is now deterministic (code first, then security):

```rust
#[test]
fn security_changes_start_a_fix_round_holding_its_findings() {
    let fx = reviewed_fixture(&["code", "security"]);
    let runner = scripted(vec![
        reply("- [minor] nit\nVERDICT: APPROVE"),
        reply("- [blocker] SQL built from input at db.rs:10\nVERDICT: CHANGES"),
    ]);
    let outcome = fx.engine(&runner).review_stage().expect("review");
    assert_eq!(outcome.verdict, Verdict::Changes);
    assert!(matches!(outcome.decision, Decision::FixRound));
    let file = std::fs::read_to_string(&outcome.review_path).expect("file");
    assert!(file.contains("## security review") && file.contains("SQL built from input"));
    assert_eq!(parse_verdict(&file), Verdict::Changes);
    let job = fx.job();
    assert_eq!(job.reviews.get("code"), Some(&ReviewState::Pass));
    assert_eq!(job.reviews.get("security"), Some(&ReviewState::Changes));
}
```

  Write `reviewed_fixture` as a helper that builds and passes the checks like `build_and_pass_checks` in `review_decide.rs`, with the given `reviewers`. Add:

| Test | Setup | Expected |
|---|---|---|
| `both_reviewers_approve_and_the_job_merges` | two approving replies | verdict `Approve`, decision `AutoMerge`, both roles `Pass` |
| `both_roles_run_when_the_first_asks_for_changes` | code `CHANGES`, security `APPROVE` | `runner.calls` holds two specs, roles `reviewer` and `security-review` |
| `one_missing_verdict_is_a_fix_round` | security reply without a `VERDICT:` line | verdict `Missing`, decision `FixRound` |
| `the_review_file_has_one_section_per_role_and_one_verdict_line` | two replies | one line starting `VERDICT:`, last in the file; `Role verdict:` lines present; `parse_verdict(file)` equals the combined verdict; raw files `review-code-r1.raw.md` and `review-security-r1.raw.md` exist |
| `a_code_only_config_writes_the_review_file_as_before` | `reviewers = ["code"]` | file text equals the code reply exactly, no `.raw.md` file |
| `security_is_skipped_for_a_docs_only_round` | changed file `docs/a.md` only | security `Skipped`, one agent call, file equals the code reply |
| `a_sync_review_runs_the_code_reviewer_only` | `review_stage_as(ReviewRun::Sync)` with both roles | one agent call, file `review-code-r1-sync1.md` |
| `security_usage_is_logged_under_its_own_role` | two replies | `usage.jsonl` holds a line with role `security-review` |
| `a_missing_security_agent_parks_with_a_clear_reason` | no `factory-security-reviewer.md` among the agent files | no agent call, decision `NeedsHuman` with `ParkKind::Other` and a reason containing `factory-security-reviewer.md not found` |
| `a_risky_diff_still_goes_to_a_human_after_both_approve` | risky path, two approvals | decision `NeedsHuman` with `ParkKind::RiskyDiff` |

- [ ] **Step 3: Run them and see them fail.** Run `cargo test -j4 -p factory-engine --test multi_review`. Expected: the new review tests fail because only one agent runs.
- [ ] **Step 4: Implement the role loop.** Keep the values computed once (the diffs, `paths`, the model ruling, the risky-paths file) before the loop. Before any run, if `security` is a role and its agent file is absent (not in `self.agent_files` and not tracked on the base branch), return a `ReviewOutcome` with `Decision::NeedsHuman { kind: ParkKind::Other, reason: "factory-security-reviewer.md not found".into() }` and verdict `Missing`, and run no agent. Per role: set `Running`, build the spec, `run_agent`, take the verdict (`Missing` unless the stop reason is `Done`), and set the role state right after.
- [ ] **Step 5: Implement the file merge and the verdict.** Combine the verdicts with `combine_verdicts`. With one role that ran, write that reply unchanged. With more, write `merge_replies`: per role a `## <role> review` heading, the reply without its final `VERDICT:` line, and `Role verdict: APPROVE` or `Role verdict: CHANGES`; then a blank line and one `VERDICT: APPROVE` or `VERDICT: CHANGES` line, left out when the combined verdict is `Missing`. Write each raw reply as `review-<role>-r<N>.raw.md`. Compute `blockers` from the final file text. Build `DecideInput` with the combined verdict; `decide` is untouched.
- [ ] **Step 6: Run the tests and see them pass.** Run `cargo test -j4 -p factory-engine` and `cargo clippy -j4 -p factory-engine`. Expected: all pass, including `review_decide.rs`, `sync_review.rs`, `gates.rs` and `minors.rs`.

Next plan: docs/superpowers/plans/2026-11-12-multi-review/plan-03-multi-review.md
