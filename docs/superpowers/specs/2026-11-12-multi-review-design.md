# Multi-review design

## Goal

Let a job record several independent review results next to `code`, namely a security review by a second reviewer agent and named `format` and `lint` results from the checks, so `factory status` shows for example `code:pass format:pass lint:pass security:pass`.

## Overview

Today a job has one combined review. `factory new` creates `reviews["code"]` only (`crates/factory-core/src/board.rs:241`), the review stage runs one reviewer and sets `code` (`crates/factory-engine/src/review.rs:123`, `:172`), `review_missing` looks at `code` alone (`board.rs:354`), and `decide` takes one `Verdict` (`crates/factory-engine/src/decide.rs:13`). The docs say there is no security agent (`docs/reference/software-factory.md`, Agents). Formatting and lint already run, but as plain checks: the rust preset has `cargo fmt -- --check` and `cargo clippy ... -D warnings` in `commands` (`crates/factory-core/presets/rust/config.toml:25`), and their result is not shown on its own.

The board already holds `reviews: BTreeMap<String, ReviewState>` (`board.rs:73`), and the dashboard reads it as a string map (`dashboard/src/state.rs:299`), so more keys need no schema change. Its code needs a small change, so that `format` and `lint` do not count as reviews and `reviewMissing` knows `security` (Design 6).

## Current state

| Piece | Today | Gap |
|---|---|---|
| `Job.reviews` | map role to `Pending`, `Running`, `Pass`, `Changes`, `Skipped`; only `code` is created | No second role, no gates |
| `review_stage_as` | builds one spec, runs one agent, writes `review-code-r<N>.md` (or `-sync<K>.md`), sets `code` (`review.rs:42-175`) | Role `code` is fixed in the code |
| Reviewer spec | `reviewer_spec_with_model` with agent `factory-code-reviewer` (`specs.rs:15`, `:551`) | No security spec |
| `decide` | one `Verdict` (`Approve`, `Changes`, `Missing`); `Missing` counts as `Changes` (`decide.rs:64`) | Must stay unchanged |
| Fix round | `apply_decision` reads `outcome.review_path` and feeds the text to the fix builder (`review.rs:239-247`) | Reads one file |
| Readers of the review file | `last_review.path_in`: park text, waivers, minor tickets, rework, merge, the API (`board.rs:217`, `park.rs`, `minors.rs`, `rework.rs`, `merge.rs`, `factory-supervisor/src/api/write.rs`) | All expect one file |
| Checks | `CheckRule { paths, commands }` with `deny_unknown_fields` (`config.rs:32`); results in `checks.md` as `PASS <cmd>` / `FAIL <cmd>` (`review.rs:215`) | No gate on a command |
| `factory status` | prints every `reviews` entry as `role:state`, sorted by role name (`status.rs:74`) | Needs no change for new keys |
| Agent files | five presets under `crates/factory-core/presets/<preset>/agents/`, each embedded with `include_str!` in `preset.rs`; `agent_lint.rs` lints them; many tests count them | No security file |
| Runner and engine | `AgentRunner::run(&self, &AgentSpec)` has no `Send` or `Sync` bound; `Engine` holds a `Cell` and a git handle, so it is not `Sync`; `run_agent` mixes board writes, `agents.json`, usage logging and the runner call (`ports.rs:229`) | Two agents cannot safely run on two threads |

## Non-goals

- Reviewer roles other than `code` and `security`, and a `security_model` setting. The design leaves room for both.
- Auto-fixing format or lint failures without a builder round.
- Running the security reviewer on sync reviews.
- Running the two reviewers on two threads. That needs a thread-safety bound on the runner trait and a split of `run_agent`; see Follow-ups.
- Any change to `decide.rs` or to a `Cargo.toml`, and any dashboard change beyond Design 6.

## Design

Flow of one job with `reviewers = ["code", "security"]`: checks run and set the `format` and `lint` entries; the code agent and then the security agent run, both every round; their verdicts are combined into one; `decide` chooses merge, fix round or needs-human as it does today.

### 1. Settings (`factory-core`)

- `reviewers`: string list, default `["code"]`. Allowed values `"code"` and `"security"`. It must contain `"code"`, hold no duplicate, and hold no other value; otherwise the config is invalid and the error names the allowed values. A broken config already makes every diff risky, so the check fails closed.
- `security_rules`: string list, default `[]`. Joined into one line in the security prompt, like `reviewer_rules` (`project-toml-reference.md:106`). The prompt only, nothing enforces them.
- Both are documented in `docs/reference/project-toml-reference.md`.

```toml
reviewers = ["code", "security"]
security_rules = ["Treat any new use of std::process::Command as a blocker unless the plan names it."]
```

### 2. Format and lint gates (`factory-core`, `factory-checks`)

- `CheckRule` gains `#[serde(default)] pub format: Vec<String>` and `#[serde(default)] pub lint: Vec<String>`. They are planned after `commands` with the same `{crate}` and `{dependents}` templating, and a path counts as covered when any of the three lists plans a command for it.
- The planner returns, next to `commands`, a map from each planned command string to its gate (`CheckPlan.gates`, only for `format` and `lint` commands). `CheckResult` and `ChecksReport` do not change; after a checks run the engine joins the results with the map by command string.
- `derived_programs` in `crates/factory-engine/src/permissions.rs:110` takes the builder's allowed programs from `commands`, `format` and `lint`, so moving a command into a gate list does not remove its program from the allow list.
- `validate_config` and the dependents scan in `plan.rs` cover the `format` and `lint` lists the same way as `commands`. `CheckRule` gets `Default`, and its struct literals in tests use `..CheckRule::default()`.
- `reviewers` and `security_rules` are added to the ordered key list in `settings_block.rs`, which a test compares with the `Config` keys.
- After every checks run (the checks stage and the final re-run), for each gate with at least one result the job gets `reviews[gate]` = `Pass` when every result of that gate passed, else `Changes`. A gate with no results has no entry. A new round clears both gate entries before the checks run.
- The rust preset moves `cargo fmt` into `format` and `cargo clippy` into `lint` in both its rules. The other presets move the equivalent commands they already run, and leave a preset alone when it has none.
- Failure behaviour does not change: a failed command is a failed check, a fix round starts, and no reviewer runs.

### 3. Roles and the combined verdict (`factory-core`)

```rust
pub const REVIEW_ROLES: [&str; 2] = ["code", "security"];
pub fn combine_verdicts(verdicts: &[Verdict]) -> Verdict;
```

`combine_verdicts` returns `Approve` when the list is not empty and every entry is `Approve`, `Changes` when any entry is `Changes`, and `Missing` otherwise (an empty list included).

- `factory new` creates one `Pending` entry per role in `reviewers`.
- `review_missing` is true when `code` is absent or is not `Pass`, `Changes` or `Skipped`, or when any other key of `reviews` that is in `REVIEW_ROLES` is not `Pass`, `Changes` or `Skipped`. `code` is always required, so a job with an empty or hand-edited `reviews` map still counts as unreviewed, as today. Gate keys never count, and `security` is required only when its key is present, which is when the job was created with it.
- `reset_reviews` sets the roles in `REVIEW_ROLES` back to `Pending` and removes the gate keys.

### 4. The review stage (`factory-engine`)

- `review_stage_as(ReviewRun::Round)` builds one spec per role in the job's `reviews` that is in `REVIEW_ROLES`. The code spec is built as today. The security spec uses a new `const SECURITY_AGENT: &str = "factory-security-reviewer"`, the same diff, plan, spec and notes, the model chosen by `reviewer_model_for`, and `security_rules`.
- Security is skipped (`ReviewState::Skipped`, no agent run) only when the round has at least one changed path, the diff is not risky, and every changed path is under `docs/**`. Nothing else is skippable, not even a `README.md` or another markdown file, because markdown can be an agent instruction or prompt (for example `crates/factory-core/assets/global-command.md`, the legacy preset builder prompts, `commit-context.md` and the tool maps). This test ignores `code_paths`, so workflows, Dockerfiles, manifests, scripts, settings, agent files and every markdown file outside `docs/**` get a security review. Any other round runs it.
- The roles run one after the other in the order `code`, `security`, and both always run, even when the first asks for changes, so one fix round gets both sets of findings. Each role is set to `Running` just before its run and to its result right after, and each reply is saved when its run ends. The "review model" ruling and the risky-paths file are written once, before the first role.
- The security spec's role string is `security-review`, so `usage.jsonl` records it under its own role and `factory tokens` and `factory costs` show it. Wherever the driver matches the role `"reviewer"` (`plugins_for` in `config.rs`, `skill_allow_for` and `sandbox_for` in `codex_prep.rs`), `"security-review"` is treated the same way.
- With more than one role that ran, the review file `review-code-r<N>.md` holds the merged text. Each role gets a level-2 heading `## code review` or `## security review`, then the reply without its own `VERDICT:` line, then a line `Role verdict: APPROVE` or `Role verdict: CHANGES`. The file ends with one blank line and the single combined line. For two roles, indented here so it is not read as a heading:

```
    ## code review
    <code reply without its VERDICT line>
    Role verdict: APPROVE
    ## security review
    <security reply without its VERDICT line>
    Role verdict: CHANGES

    VERDICT: CHANGES
```

  The final `VERDICT:` line comes from `combine_verdicts` and is left out when the result is `Missing`. The raw replies are kept as `review-code-r<N>.raw.md` and `review-security-r<N>.raw.md`. When only `code` ran, the file is the code reply exactly as today and no raw copy is written.
- `last_review` keeps its shape. `blockers` is the count of open blockers in the merged file, and `file` is `review-code-r<N>.md`. Every reader of the review file therefore sees all findings with no change.
- The combined verdict is passed to `decide` as it is. A fix round reads the merged file, so it gets the findings of every role that did not approve.
- A sync review (`ReviewRun::Sync`) runs the code role only and writes `review-code-r<N>-sync<K>.md` as today.
- A risky diff still goes to a human after all reviews approve.

### 5. The security agent (presets)

- A new `factory-security-reviewer.md` in each preset (`rust`, `go`, `ruby`, `csharp`, `node`). Frontmatter as the code reviewer: `tools: Read, Grep, Glob`, a `maxTurns`, the rules marker comment. Same output contract: at most 60 lines, findings tagged `[blocker]` or `[minor]`, last line `VERDICT: APPROVE` or `VERDICT: CHANGES`. Read-only.
- Scope: injection (SQL, shell, template), authentication and authorization gaps, secrets in code or logs, path traversal, command execution, unsafe deserialization, SSRF, crypto misuse, new dependencies, and changes at trust boundaries. It does not judge style, tests or correctness beyond security.
- The file is embedded as a sixth entry in the `agents` list of each of the five presets in `preset.rs`, and `factory-cli/tests/agent_lint.rs` lints it. It must carry the rules marker `<!-- factory-rules: 1 -->` and avoid the words the preset tests ban (`Rocket`, `rocket-`, `yarn`, `tools/factory`, `shadcn`, `src-tauri`).
- Tests that count agent files or preset files are updated: `agent_lint.rs` (the five-name list), `tool_map.rs` (`seen == 25`), `presets_refresh.rs`, `preset.rs`, `agents.rs`, `resolve.rs` (name lists) and `factory-engine/tests/agent_files.rs`.
- The file name matches none of the `risk_paths` globs (`**/*auth*`, `**/*secret*`, `**/*trust*`, `**/*csp*`).

### 6. CLI and docs

- `factory status` prints the new keys through the existing `reviews_text`, sorted by name, so the example reads `code:pass format:pass lint:pass security:pass`.
- `approve` without a verdict from every required role is refused with the existing message, whose text now says "no review yet from: security".
- `project show` lists under `problems:` a project with `"security"` in `reviewers` whose agent file resolves nowhere.
- Dashboard (`dashboard/`, its own workspace): `reviewMissing` in `static/board.js` requires `code` and also `security` when that key is present, so the approve command it shows matches the CLI. `ROLES` and `ROLE_LABEL` gain `format` and `lint`, so the gate results show as chips. The `reviewed` flag in `src/state.rs` (~299) and `src/plan.rs` (~171) ignores the `format` and `lint` keys, so a job parked in checks with `lint:changes` is not counted as reviewed.
- `docs/reference/software-factory.md` (Pipeline, Agents, Checks) and `README.md` describe the roles, the gates and the merged file.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| `reviewers` without `code`, with an unknown value, or with a duplicate | Config error naming the allowed values; fails closed |
| Security agent file is not among `effective_agent_files()` (a base branch copy only replaces the text of a resolved file and never adds one, so a file tracked only on the base branch does not count) | The job parks as needs-human with `ParkKind::Other` and the reason `factory-security-reviewer.md not found`, and `project show` reports it |
| One agent crashes or times out, the other approves | Combined verdict `Missing`; a fix round while rounds remain, then needs-human, as for one reviewer |
| Docs-only round (at least one path, every path under `docs/**`, none risky) | Security is `skipped`, counted as a pass; `code` runs as today |
| Only a workflow, Dockerfile, manifest, script, settings file, agent file (`.claude/agents/*.md` or a preset `agents/*.md`), `CLAUDE.md` or any other markdown file outside `docs/**` (a root `README.md`, `global-command.md`, a legacy builder prompt, `commit-context.md`) changed, even with `code_paths` set | Security runs |
| `reviews` map empty or hand-edited | `code` is still required, so `approve` is refused |
| `reviewers` changed while a job is in flight | The job keeps the roles it was created with |
| `rework` or a new round | Roles reset to `Pending`; gate entries removed, then rewritten by the next checks run |
| A format or lint command fails | Check failure, fix round, no reviewer runs; the gate entry shows `changes` until the next checks run |
| Both reviews have blockers | One fix round; the fix builder gets the merged file with both headings |
| `reviewers = ["code"]` | Behaviour and review file identical to today |
| Budget is stopped | `decide` parks before any review result is used, as today |
| Older board with only `code` | Loads; a job without a `security` key never requires it |

## Testing

`cargo test -j4 -p factory-core`:
- `config.rs`: `reviewers_default_is_code_only`, `reviewers_without_code_is_refused`, `unknown_reviewer_is_refused`, `duplicate_reviewer_is_refused`, `security_rules_default_is_empty`, `check_rule_without_gate_lists_still_loads`, `check_rule_gate_lists_load`.
- `board.rs`: `new_job_has_a_pending_review_per_configured_role`, `review_missing_needs_every_reviewer_role`, `review_missing_ignores_gate_keys`, `reset_reviews_clears_gate_keys`.
- `verdict.rs`: `combine_all_approve_is_approve`, `combine_any_changes_is_changes`, `combine_missing_without_changes_is_missing`, `combine_empty_is_missing`.
- `resolve.rs`: the resolved agent name list includes `factory-security-reviewer.md`.

`cargo test -j4 -p factory-checks`: `format_and_lint_commands_are_planned_after_commands`, `results_carry_their_gate`, `a_path_is_covered_by_a_format_only_rule`.

`cargo test -j4 -p factory-engine --test multi_review` (scripted runner, as `review_decide.rs` does):
- `both_reviewers_approve_and_the_job_merges`
- `security_changes_start_a_fix_round_holding_its_findings`
- `one_missing_verdict_is_a_fix_round`
- `the_review_file_has_one_section_per_role_and_one_verdict_line` (asserts `parse_verdict` of the merged file equals the combined verdict, so the `Role verdict:` lines are never read as the verdict)
- `a_code_only_config_writes_the_review_file_as_before`
- `security_is_skipped_for_a_docs_only_round`
- `security_runs_when_only_a_workflow_file_changes` (with `code_paths` set to a list that does not name it)
- `security_runs_for_a_claude_settings_file` (a `.claude/settings.json` change)
- `security_runs_for_an_agent_file` (a `.claude/agents/factory-builder.md` change, and a preset `agents/` file change)
- `security_runs_for_claude_md` (a root `CLAUDE.md` change)
- `security_runs_for_markdown_outside_docs` (a root `README.md` change, and a `crates/factory-core/assets/global-command.md` change)
- `a_sync_review_runs_the_code_reviewer_only`
- `security_usage_is_logged_under_its_own_role`
- `a_missing_security_agent_parks_with_a_clear_reason`
- `format_and_lint_results_land_on_the_board_after_the_checks`
- `a_new_round_clears_the_gate_entries`

Dashboard: `node --test dashboard/tests/board.test.js` with `approve_command_waits_for_security_when_it_is_present`, `approve_command_ignores_security_when_it_is_absent`, `gate_roles_render_as_chips`; `cargo test -j4 --manifest-path dashboard/Cargo.toml reviewed_ignores_gate_entries`.

`cargo test -j4 -p factory-cli`: `status_lists_every_review_and_gate`, `approve_is_refused_until_every_reviewer_has_a_verdict`, `project_show_reports_a_missing_security_agent`, and `cargo test -j4 -p factory-cli --test agent_lint` for the five new files.

Run `cargo clippy -j4 -p factory-core -p factory-checks -p factory-engine -p factory-cli`.

## Files changed

| File | Change |
|---|---|
| `crates/factory-core/src/config.rs` | `reviewers`, `security_rules`, `CheckRule.format` and `.lint` |
| `crates/factory-core/src/board.rs`, `verdict.rs` | Roles at creation, `review_missing`, `reset_reviews`, `REVIEW_ROLES`, `combine_verdicts` |
| `crates/factory-core/src/resolve.rs` | Drift list entry |
| `crates/factory-core/presets/*/config.toml` | Move fmt and lint commands into `format` and `lint` |
| `crates/factory-core/presets/*/agents/factory-security-reviewer.md` | New, five files |
| `crates/factory-checks/src/*.rs` | Gate on planned commands and results |
| `crates/factory-engine/src/review.rs`, `specs.rs`, `checks_stage.rs`, `park.rs` | Role loop, merged file, gate entries, missing-agent park |
| `crates/factory-cli/src/commands.rs`, `project.rs`, `status.rs` | Approve message, project problems |
| `crates/factory-engine/tests/multi_review.rs` and the crate tests above | New tests |
| `dashboard/static/board.js`, `dashboard/src/state.rs`, `dashboard/src/plan.rs`, `dashboard/tests/board.test.js` | Approve command, gate chips, `reviewed` flag |
| `README.md`, `docs/reference/software-factory.md`, `docs/reference/project-toml-reference.md` | Docs |

## Constraints

- `decide.rs` does not change; the verdict is combined before it is called.
- Board, ledger and events JSON stay readable by `dashboard/`: no new field, and `reviews` keys stay strings.
- A missing or broken config must make every diff risky; the new settings never default to permissive.
- Agents never write the board, ledger or events; only the driver does.
- Paths come from `Layout`; `no_hardcoded_paths.rs` must pass. Do not shell out to `git`.
- No `Cargo.toml` change. Commands use `-j4`, never `--workspace` or `--all`.
- Run after editing an agent file: `cargo test -j4 -p factory-cli --test agent_lint`.
- Build after job 055 of issue #57 has merged, because both change `README.md` and `docs/reference/software-factory.md`.

## Acceptance criteria

- `reviewers = ["code", "security"]` makes `factory status` show both roles, and a job merges only when both approve.
- A security `[blocker]` starts a fix round, and the fix builder's findings include the security section.
- The default config (`reviewers = ["code"]`) produces the same review file, board and status as before.
- `format` and `lint` appear for a job whose checks ran commands from those lists, and are absent otherwise.
- A docs-only round shows `security:skipped` and merges on the code review alone.
- `approve` is refused while a required role has no verdict.
- An invalid `reviewers` value is a config error that names the allowed values.
- A missing security agent file parks the job with a clear reason and shows under `problems:`.
- The dashboard shows `factory approve <id> --without-review` for a job with `code` passed and `security` pending, shows `format` and `lint` chips, and does not count a job with only gate entries as reviewed.
- `factory tokens` and `factory costs` include the security run.
- `cargo test -j4` for the four crates passes, `agent_lint` passes, and clippy is clean.

## Out of scope

- Roles beyond `code` and `security`, a `security_model` setting, and a reviewer for tests or docs.
- Running format or lint auto-fixers.
- Security review of sync merges.

## Follow-ups

- Add a `security_model` setting so the security review can use another model than the code review.
- Let a failed format gate run its fixer command before it starts a builder round.
- Run the code and security reviewers on two threads: add `Sync` to the runner trait, split `run_agent` into prepare, run and record parts, and give each run its own token snapshot.

## Decisions

- Which entries become their own results? Options: security reviewer plus fmt and lint gates; configurable LLM reviewers only; mechanical gates only. Chosen: a security reviewer plus format and lint gates. Decided by: user
- How does a security result affect the job? Options: runs after code APPROVE and blocks like code; both reviews in parallel each round; advisory only. Chosen: both reviews run every round and both block like code, so one fix round gets both sets of findings. Decided by: user
- Run the two reviews on two threads or one after the other? Options: one after the other, both every round; true parallel now; security only after code approves. Chosen: one after the other, both every round, because the engine and runner are not thread-safe and parallel running needs a larger refactor; parallel is a follow-up. Decided by: user
- How do format and lint get their own results? Options: tag existing check commands; a separate gates config; infer from command text. Chosen: tag commands in the check rules, built as optional `format` and `lint` lists next to `commands`. Decided by: user
- Is the security review on by default? Options: opt-in per project; on for code diffs; on for risky diffs only. Chosen: opt-in per project, `reviewers` defaults to `["code"]`. Decided by: user
- Which structure? Options: generalize `review.rs` into a reviewer set; copy it into `security.rs`; a post-review hook. Chosen: generalize `review.rs`. Decided by: user
- How are verdicts combined and files kept? Chosen: combine to one verdict before `decide`, and merge the replies into the existing review file. Decided by: user
- Does a sync review include security? Chosen: no, code only. Decided by: user
- Model for the security review. Chosen: the same model choice as the code review. Decided by: user
- A failed gate shows which state? Chosen: `changes`, so `ReviewState` gets no new variant. Decided by: call
- Which rounds skip security? Options: default-deny, skip only pure docs; never skip; reuse the no-checks predicate. Chosen: default-deny. Skip only when the round has a changed path, every changed path is under `docs/**`, and none is risky, ignoring `code_paths`. Any markdown outside `docs/**` runs security, because instruction and prompt files are markdown. Decided by: user
- `review_missing` and the empty `reviews` map. Chosen: `code` is always required, present or absent, as in the existing code and its test; other roles count only when their key is present. Decided by: user
- Missing agent file. Chosen: park with `ParkKind::Other` and a clear reason, and report it under `problems:`. Decided by: user
- Dashboard gap found by the spec review. Options: small dashboard change in this series; follow-up ticket; fix only the approve command. Chosen: a small dashboard change in this series (`reviewMissing`, gate chips, `reviewed` flag). Decided by: user
