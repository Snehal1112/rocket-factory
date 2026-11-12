# Multi-review, CLI, dashboard and docs Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-12-multi-review-design.md
> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Report a missing security agent in `project show`, make the dashboard and `approve` agree about security, and update the docs. The security reviewer agent already ships in all five presets.

**Architecture:** The CLI reads the resolved agent files and the board, and changes no engine logic. The dashboard change is small and stays in its own workspace.

**Tech Stack:** Rust, JavaScript tests with `node --test`, markdown docs.

## Global Constraints

- Commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`. Never `--workspace` or `--all`.
- Board JSON stays readable by `dashboard/`. No `Cargo.toml` change. Tests use `.expect("...")` and `expect_err`, never the unwrap call.

## Review Focus

1. A project that lists `"security"` but resolves no agent file shows a problem naming the file. Pinned by `project_show_reports_a_missing_security_agent`.
2. A project without `"security"` shows no new problem. Pinned by `project_show_ignores_security_when_it_is_not_enabled`.
3. `approve` is refused while `security` has no verdict, and the message names the role. Pinned by `approve_is_refused_until_every_reviewer_has_a_verdict`.
4. The docs no longer claim there is no security agent. Pinned by `docs_name_the_security_reviewer_agent`.

## File Structure

| File | Responsibility |
|---|---|
| `crates/factory-cli/src/project.rs` | `project show` problem for a missing security agent |
| `dashboard/static/board.js`, `dashboard/src/state.rs`, `dashboard/src/plan.rs`, `dashboard/tests/board.test.js` | Approve command, gate chips, `reviewed` flag |
| `docs/reference/software-factory.md`, `README.md`, `docs/guides/factory-user-guide.md` | Describe the roles, gates and merged file |

### Task 1: CLI and dashboard checks for the new roles

**Files:**
- Modify: `crates/factory-cli/src/project.rs` (`show` problems at ~254-270, tests near ~517)
- Modify: `dashboard/static/board.js` (`ROLES` and `ROLE_LABEL` at ~12, `reviewMissing` at ~22), `dashboard/src/state.rs` (~299), `dashboard/src/plan.rs` (~171 and its caller)
- Test: `dashboard/tests/board.test.js`, the tests in `dashboard/src/plan.rs` and `state.rs`, `crates/factory-cli/src/status.rs` tests, `crates/factory-cli/tests/project_cmd.rs`, `crates/factory-cli/tests/e2e.rs` (or the file that holds the approve refusal tests)

**Interfaces:**
- Consumes: `Config.reviewers`, the resolved agent file names that `project show` already prints under `agent files:`, `Job::missing_reviews` and `approve_refusal`.
- Produces: one more entry in the `problems` list of `project show`: `reviewers lists "security" but factory-security-reviewer.md was not found; run factory presets refresh`, added only when `reviewers` contains `"security"` and no resolved agent file has that name. `status` and `approve` need no code change.

- [ ] **Step 1: Read the code this task touches.** Read `show` in `project.rs` with `codex_problems` and its tests, `reviews_text` and `review_word` in `status.rs`, and how an existing CLI test makes an approve refusal (search `approve_refusal` and `has no code review yet` in `crates/factory-cli/tests`).
- [ ] **Step 2: Write the failing tests.** In `project.rs` tests or `project_cmd.rs`:

```rust
#[test]
fn project_show_reports_a_missing_security_agent() {
    let out = show_with_reviewers(&["code", "security"], &["factory-code-reviewer.md"]);
    assert!(out.contains("problems:"), "{out}");
    assert!(out.contains("factory-security-reviewer.md was not found"), "{out}");
}
```

  Build the project the way the codex problem test does and give it the config line and the agent file list; write `show_with_reviewers` as a helper. Add:

| Test | Setup | Expected |
|---|---|---|
| `project_show_ignores_security_when_it_is_not_enabled` | `reviewers = ["code"]`, no security file | no `factory-security-reviewer.md` problem |
| `status_lists_every_review_and_gate` | a job with reviews `code` pass, `format` pass, `lint` pass, `security` pass | the status line holds `code:pass format:pass lint:pass security:pass` in that order |
| `approve_is_refused_until_every_reviewer_has_a_verdict` | a `needs-human` job with `code` pass and `security` pending | `approve` is refused with a message containing `has no security review yet` |
| `approve_command_waits_for_security_when_it_is_present` (`board.test.js`, using `makeJob`) | `reviews` `{code: "pass", security: "pending"}` | `board.approveCommand(job)` is `factory approve 001 --without-review` |
| `approve_command_ignores_security_when_it_is_absent` | `reviews` `{code: "pass"}` | `factory approve 001` |
| `gate_roles_render_as_chips` | `reviews` `{code: "pass", format: "pass", lint: "changes"}` | `board.reviewRoles(job)` lists `format`, `lint` and `code`, and `reviewChip("lint", "changes")` shows `lint changes` |
| `reviewed_ignores_gate_entries` (dashboard, near the `reviews` tests in `state.rs`) | a job in `needs-human` with `reviews` `{lint: "changes"}` only | the `reviewed` flag passed to `load_plan` is false; with `code: "pass"` added it is true |
| `tokens_and_costs_include_the_security_review_role` | a `usage.jsonl` line with role `security-review` next to a `reviewer` line, then `tokens <id>` and `costs` | the `tokens` per-role output names `security-review`, and the cost role split does not drop or fail on it (read `cost.rs` and `tokens.rs` for any role list that must learn the new name) |

- [ ] **Step 3: Run them and see them fail.** Run `cargo test -j4 -p factory-cli project_show_reports_a_missing_security_agent`. Expected: the output has no such problem.
- [ ] **Step 4: Implement the problem and the dashboard change.** In `show`, after the existing `validate_config` and codex checks, push the problem text above when the condition holds. In `board.js` make `reviewMissing` require `code` and also `security` when that key exists, add `format` and `lint` to `ROLES` and `ROLE_LABEL`, and export nothing new. In `state.rs` and `plan.rs` compute `reviewed` from the `reviews` entries whose key is not `format` or `lint`.
- [ ] **Step 5: Run the tests and see them pass.** Run `cargo test -j4 -p factory-cli`, `cargo clippy -j4 -p factory-cli`, `node --test dashboard/tests/board.test.js` and `cargo test -j4 --manifest-path dashboard/Cargo.toml`. Expected: all pass.

### Task 2: Documentation

**Files:**
- Modify: `docs/reference/software-factory.md` (`## Pipeline` at ~7-25, `## Agents` table and the sentence at ~63 that says there are no security agents, `## Checks` at ~194)
- Modify: `README.md` (`### Layers` at ~568-581, `### Agent files` at ~587, `### Rust preset checks` at ~556)
- Modify: `docs/guides/factory-user-guide.md` (the REVIEWS column text at ~412)
- Test: `crates/factory-cli/tests/agent_lint.rs`

**Interfaces:**
- Consumes: the finished behaviour of the previous plans.
- Produces: docs that describe the `reviewers` and `security_rules` settings, the `format` and `lint` lists, the merged review file with `## <role> review` headings and one final `VERDICT:` line, the sequential run, and the `security-review` usage role. The sentence "There are no spec-writer, security, ux, ui or approver agents." becomes a sentence that names `factory-security-reviewer` as the only extra reviewer and still says there are no spec-writer, ux, ui or approver agents.

- [ ] **Step 1: Read the code and docs this task touches.** Read the three docs at the lines named, the phrase tests at `agent_lint.rs` ~584-617 and 762, `check_coverage_docs.rs`, and `global_command.rs`; do not reword any text those tests pin.
- [ ] **Step 2: Write the failing test** in `agent_lint.rs`:

```rust
#[test]
fn docs_name_the_security_reviewer_agent() {
    let text = read_repo_file("docs/reference/software-factory.md");
    assert!(text.contains("factory-security-reviewer"), "agents table");
    assert!(!text.contains("There are no spec-writer, security, ux, ui or approver agents."));
    assert!(text.contains("reviewers"), "settings");
}
```

  Use the file helper of the neighbouring doc tests.

- [ ] **Step 3: Run it and see it fail.** Run `cargo test -j4 -p factory-cli --test agent_lint docs_name_the_security_reviewer_agent`. Expected: the first assertion fails.
- [ ] **Step 4: Update the docs.** Add the agent row and rewrite the sentence in `software-factory.md`; add the sequential review step, the merged file and the `format` and `lint` gates to Pipeline and Checks; update the README layers and agent file text and the rust preset checks (fmt and clippy now sit in `format` and `lint`); update the REVIEWS example in the user guide to `code:pass format:pass lint:pass security:pass`.
- [ ] **Step 5: Run the tests and see them pass.** Run `cargo test -j4 -p factory-cli --test agent_lint --test check_coverage_docs --test global_command`. Expected: all pass.

Next plan: none
