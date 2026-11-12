# Multi-review, agents, CLI and docs Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-12-multi-review-design.md
> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Ship the security reviewer agent in all five presets, report a missing security agent in `project show`, and update the docs.

**Architecture:** The agent files are embedded in `preset.rs` like the existing five. The CLI reads the resolved agent files and the board, and changes no engine logic.

**Tech Stack:** Rust, markdown agent files, `include_str!`.

## Global Constraints

- Commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`. Never `--workspace` or `--all`.
- A task that edits `.claude/agents/factory-*.md` or a preset agent file ends with `cargo test -j4 -p factory-cli --test agent_lint`.
- The agent text must not contain the words the preset tests ban: `Rocket`, `rocket-`, `yarn`, `tools/factory`, `shadcn`, `src-tauri`. The file name must not match a risk path glob (`**/*auth*`, `**/*secret*`, `**/*trust*`, `**/*csp*`).
- The agent is read-only: `tools: Read, Grep, Glob`. Its text never contains `Bash`, `Write` or `Edit`.
- Board JSON stays readable by `dashboard/`. No `Cargo.toml` change. Tests use `.expect("...")` and `expect_err`, never the unwrap call.

## Review Focus

1. The security agent file passes the same contract as the code reviewer in every preset. Pinned by `security_reviewer_files_follow_the_reviewer_contract`.
2. A project that lists `"security"` but resolves no agent file shows a problem naming the file. Pinned by `project_show_reports_a_missing_security_agent`.
3. A project without `"security"` shows no new problem. Pinned by `project_show_ignores_security_when_it_is_not_enabled`.
4. `approve` is refused while `security` has no verdict, and the message names the role. Pinned by `approve_is_refused_until_every_reviewer_has_a_verdict`.
5. The docs no longer claim there is no security agent. Pinned by `docs_name_the_security_reviewer_agent`.

## File Structure

| File | Responsibility |
|---|---|
| `crates/factory-core/presets/{rust,go,ruby,csharp,node}/agents/factory-security-reviewer.md` | The security reviewer instructions |
| `crates/factory-core/src/preset.rs` | Embed the file in each preset, update counts |
| `crates/factory-cli/tests/agent_lint.rs`, `tool_map.rs`, `presets_refresh.rs` | Lint the new files, update counts |
| `crates/factory-core/src/agents.rs`, `resolve.rs`, `crates/factory-engine/tests/agent_files.rs` | Update counts and name lists |
| `crates/factory-cli/src/project.rs` | `project show` problem for a missing security agent |
| `docs/reference/software-factory.md`, `README.md`, `docs/guides/factory-user-guide.md` | Describe the roles, gates and merged file |

### Task 1: The security reviewer agent in every preset

**Files:**
- Create: `crates/factory-core/presets/{rust,go,ruby,csharp,node}/agents/factory-security-reviewer.md`
- Modify: `crates/factory-core/src/preset.rs` (the `agents` list of each preset, `rust` at ~26-46; the counts near ~530-545, 740, 750, 813, 925, 939, 969, 1196)
- Modify: `crates/factory-cli/tests/agent_lint.rs` (`every_preset_ships_the_five_factory_agents` at ~121, the max-turns test at ~778), `tool_map.rs` (`seen == 25` at ~103), `presets_refresh.rs` (~60-90)
- Modify: `crates/factory-core/src/agents.rs` (~114), `crates/factory-core/src/resolve.rs` (~402-406 and ~733), `crates/factory-engine/tests/agent_files.rs` (~362)

**Interfaces:**
- Consumes: the shape of `factory-code-reviewer.md`: frontmatter `name`, `description`, `tools: Read, Grep, Glob`, `maxTurns: 40`; the line `<!-- factory-rules: 1 -->`; the sections `## Stack`, `## How to read`, `## Check`, `## Judging`, `## Report`.
- Produces: an agent named `factory-security-reviewer` in each preset, so each preset ships six agent files and the embedded lists grow by one. The reviewer contract in the file: a reply of at most 60 lines, findings tagged `[blocker]` or `[minor]`, a last line `VERDICT: APPROVE` or `VERDICT: CHANGES`, and the paging rule "1500 lines per call, starting at line 1".

- [ ] **Step 1: Read the code and docs this task touches.** Read the rust `factory-code-reviewer.md` in full and the differences of the four other presets (the `## Stack` paragraph and one bullet of `## Check`); `agent_lint.rs` lines 60-130, 240-300, 770-850; `tool_map.rs` lines 80-105; the `agents` lists and counts in `preset.rs`; and the spec "Design 5".
- [ ] **Step 2: Write the failing tests.** In `agent_lint.rs` add:

```rust
#[test]
fn security_reviewer_files_follow_the_reviewer_contract() {
    for preset in PRESETS {
        let text = read_agent(preset, "factory-security-reviewer.md");
        assert!(text.contains("<!-- factory-rules: 1 -->"), "{preset}");
        assert!(text.contains("tools: Read, Grep, Glob"), "{preset}");
        for phrase in ["at most 60 lines", "[blocker]", "[minor]", "1500 lines per call", "starting at line 1"] {
            assert!(text.contains(phrase), "{preset}: {phrase}");
        }
        assert!(text.lines().any(|l| l == "VERDICT: APPROVE"), "{preset}");
        assert!(text.lines().any(|l| l == "VERDICT: CHANGES"), "{preset}");
        for banned in ["Bash", "Write", "Edit", "Rocket", "rocket-", "yarn", "shadcn", "src-tauri"] {
            assert!(!text.contains(banned), "{preset}: {banned}");
        }
    }
}
```

  Use the file-reading helper the neighbouring tests use in place of `read_agent`. Change the five-name list test to six names (rename it `every_preset_ships_the_six_factory_agents`), pair the new file with `REVIEWER_MAX_TURNS` in the max-turns test, and update `tool_map.rs`, `presets_refresh.rs`, `preset.rs`, `agents.rs`, `resolve.rs` and `agent_files.rs` counts and name lists by one file per preset.

- [ ] **Step 3: Run them and see them fail.** Run `cargo test -j4 -p factory-cli --test agent_lint`. Expected: the new test fails with a missing file.
- [ ] **Step 4: Write the five agent files.** Copy the structure of that preset's code reviewer and replace the review content. Scope: injection (SQL, shell, template), authentication and authorization gaps, secrets in code or logs, path traversal, command execution, unsafe deserialization, SSRF, crypto misuse, new dependencies and changes at trust boundaries. State that style, tests and general correctness are out of scope, and keep the `## Stack` paragraph specific to the preset's language. `description` has no `": "`.
- [ ] **Step 5: Embed and fix the counts.** Add the sixth entry to each preset's `agents` list in `preset.rs` with `include_str!`, then update every count and name list named in Files by one file per preset.
- [ ] **Step 6: Run the tests and see them pass.** Run `cargo test -j4 -p factory-cli --test agent_lint --test tool_map --test presets_refresh`, `cargo test -j4 -p factory-core` and `cargo test -j4 -p factory-engine --test agent_files`. Expected: all pass.

### Task 2: CLI and dashboard checks for the new roles

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

### Task 3: Documentation

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
