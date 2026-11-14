# Intake first-run fixes design

## Goal

Make a real intake run either work or fail with a clear cause: the drafter can write its draft, a missing drafter agent file is caught before the issue is claimed, and a drafter that cannot run gets its own reason code and a local log of what it said.

## Overview

GitHub issues #73 and #74 come from the first two real intake runs, both on issue #72. Unit tests could not catch either, because they use fake runners and a fake tracker.

- Run 1 (#73): intake claimed #72 and 4 seconds later set `factory: needs-human` with reason `draft-invalid`. The drafter never ran. `claude` stopped with `--agent 'factory-intake-drafter' not found`, because the seeded preset folder `~/.factory/presets/<preset>/agents/` was made before the drafter file existed. `factory project show` printed `problems: none`.
- Run 2 (#74): the drafter ran 229 seconds and 27 turns (about 268K weighted Opus tokens), called Write on `draft/spec.md` twice, and both calls were refused. `draft/` stayed empty and the pass ended `draft-invalid` (`the draft has no spec.md or no plan`). The CLI prints the cause itself: a `Write(//...)` allow rule "is not matched by file permission checks, only Edit(path) rules are". A test with the real CLI under `--permission-mode dontAsk --tools Write` confirmed it: a `Write(...)` rule gives `Permission to use Write has been denied`, an `Edit(...)` rule creates the file.

The KB run is not affected: `kb_allowed_tools` grants `Edit(//<kb>/**)` as well as `Write(//<kb>/**)` (`kb.rs:30-31`). The drafter grants only `Write` (`intake.rs:112`).

## Current state

| Piece | Today | Gap |
|---|---|---|
| `run_intake` (`intake.rs:211`) | Codex check (`:221`), `pick_issue`, then the claim by `edit_labels` (`:237`), then `draft_jobs` (`:248`) | Nothing checks that the drafter agent file exists before the claim |
| `write_drafter_agents` (`intake.rs:352`) | Writes `agents.json` from the effective agent files, or returns `None` when the driver has none or runs Codex | A missing drafter file passes silently |
| `show` (`factory-cli/src/project.rs:234-283`) | Reports a missing `factory-security-reviewer.md` when `reviewers` holds `security` | No such check for `intake = true` |
| `draft_spec` (`intake.rs:102-113`) | Allows `Read`, `Grep`, `Glob` on the tree and `Write` on the draft folder | A `Write(path)` rule never matches, so the drafter cannot write |
| `draft_jobs` stop match (`intake.rs:414-419`) | `Done` continues, `BudgetStop` is `over-budget`, every other stop is `draft-invalid` | A drafter that could not run looks like a bad issue |
| Empty draft (`intake.rs:425`) | `invalid("the draft has no spec.md or no plan")` | The log does not say what the drafter said |
| `failure` (`intake.rs:50`) | Logs the cause locally; the comment holds only the code, the job and the draft path | None; the log is the place for the cause |
| Reason codes in docs | The guide says "a fixed comment holds a reason code" (`factory-user-guide.md`, intake section) | The codes are not listed |

## Non-goals

- Changing the claim order for a running pass. The claim still happens after the pick; only a new check moves before it.
- Retrying a failed drafter. A failed issue stays `factory: needs-human` until a person relabels it.
- A live-CLI test in CI. The real `claude` cannot run in unit tests; this spec adds a guard on the rule names instead.
- The ignored `Write(//<kb>/**)` rule of the KB run (see Follow-ups).

## Design

```
run_intake: off? -> codex? -> drafter agent file present? -> pick -> claim -> draft_jobs
draft_spec: Read/Grep/Glob on tree, Edit on draft
draft_jobs: stop Done -> check files; Failed/Stalled -> drafter-failed; TurnLimit -> draft-invalid
```

### 1. Check the drafter agent file before the claim (#73)

In `run_intake`, right after the Codex check and before `pick_issue`, call a new function `check_drafter_agent(driver: &Driver) -> Result<(), EngineError>`. It returns `Ok` when the driver has no agent files (the same case in which `write_drafter_agents` returns `None`), and `Ok` when one effective agent file is named `factory-intake-drafter` or `factory-intake-drafter.md`. Otherwise it returns `EngineError::Msg("intake: the drafter agent file factory-intake-drafter.md was not found; run factory presets refresh")`. The check runs for a dry run too, so `--dry-run` shows the setup problem. No label changes and no tracker call happen before it. A failing pass ends with that error line, and the next poll of `factoryd` checks again.

### 2. `factory project show` reports it (#73)

In `show`, next to the security reviewer check: when `intake` is true and no resolved agent is named `factory-intake-drafter.md`, push the problem `intake is on but factory-intake-drafter.md was not found; run factory presets refresh`. `problems: none` then no longer hides it.

### 3. A drafter that cannot run has its own reason code (#73)

In the stop match of `draft_jobs`: `StopReason::Failed` and `StopReason::Stalled` return `failure("drafter-failed", n, "the drafter stopped: <reason>")`. `StopReason::TurnLimit` stays `draft-invalid`, because a drafter that used all its turns points at an issue that is too big or unclear. `Done` and `BudgetStop` are unchanged. The fixed comment text is unchanged apart from the code: `reason: drafter-failed`.

### 4. The draft folder grant is an Edit rule (#74)

In `draft_spec`, the draft folder rule becomes `Edit(//<draft>/**)` and the `Write(...)` rule is removed. `spec.tools` stays `Read,Grep,Glob,Write`: the Write tool is covered by an Edit rule. The rules for the tree stay `Read`, `Grep` and `Glob`. A new guard test fails when any path-scoped allow rule of the drafter names a tool other than `Read`, `Grep`, `Glob` or `Edit`.

### 5. An empty draft leaves its cause in the local log (#74)

When the drafter stops `Done` but `spec.md` or the plan is missing, the cause passed to `failure` becomes `the draft has no spec.md or no plan; last message: <tail>`, where `<tail>` is the last 600 characters of `run.result_text` on one line (line breaks replaced by spaces). It goes to the log through `failure` only. The GitHub comment is built from the code, the job and the draft path, so it never holds the tail.

### 6. Docs

The intake section of `docs/guides/factory-user-guide.md` gets a table of the reason codes: `draft-refused` (the drafter wrote `refused.md`), `draft-invalid` (no usable draft, or a drafter that used all its turns), `drafter-failed` (the drafter could not run: check the log and `factory presets refresh`), `spec-review` (the drafted spec was refused twice), `over-budget` (the drafter hit `intake_budget_weighted`), `build-failed` (a job parked or failed). It also says that file write rules for an agent run are `Edit(path)` rules, and that after an upgrade `factory presets refresh` must run so the seeded presets get new agent files. `docs/reference/software-factory.md` (the intake paragraph) says that a pass refuses to start when the drafter agent file is missing.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| Drafter agent file missing, the driver has agent files | Pass ends with an error naming `factory presets refresh`; no label, no comment, no agent run |
| Drafter agent file missing, a dry run | The same error |
| Driver has no agent files | The check passes, as `write_drafter_agents` returns `None` |
| `agent_file` name with or without `.md` | Both match |
| `intake = false` | `Off` first, no check |
| `project show`, intake on, file missing | One extra problem line |
| `project show`, intake off | No intake check |
| Drafter stops `Failed` or `Stalled` | `drafter-failed`, label and fixed comment, local log has the cause |
| Drafter stops `TurnLimit` | `draft-invalid` as before |
| Drafter `Done` with no `spec.md` | `draft-invalid`; the log holds the last 600 characters of its last message |
| Last message empty | The log reads `last message: ` with nothing after it |
| Last message holds an issue-steered secret | Only the local log holds it; the comment holds the fixed text |
| Edit rule on the draft folder | The drafter writes `draft/spec.md` and the plans; reads of the tree stay read-only |

## Testing

- `crates/factory-engine/tests/intake.rs`:
  - `intake_refuses_when_the_drafter_agent_file_is_missing` (the fake driver has agent files without the drafter; the error names `factory presets refresh`; the tracker saw no `edit_labels` and no `comment`; the runner has no call), and the same with `dry_run = true`.
  - `intake_does_not_check_the_agent_file_when_the_driver_has_none`.
  - `a_drafter_that_cannot_start_fails_with_the_drafter_failed_code` (a `Failed` stop and a `Stalled` stop; no job on the board; the comment holds `reason: drafter-failed`).
  - `a_drafter_that_hits_the_turn_limit_is_draft_invalid`.
  - `an_empty_draft_logs_the_tail_of_the_last_message_locally` (reads the cause through the log or a `failure` hook the test can see, and checks that the tail is at most 600 characters on one line).
  - `the_failure_comment_never_holds_the_last_message` (a last message of `SECRET-MARKER`; no comment body contains it).
  - Update the stalled row of `a_drafter_that_stops_early_fails_the_issue_with_no_job` (`:434`) from `draft-invalid` to `drafter-failed`.
- `crates/factory-engine/src/intake.rs` unit tests: update `draft_spec_grants_only_the_draft_folder` to expect one `Edit(//state/intake/7/draft/**)` rule and no rule that starts with `Write(`; add `draft_spec_path_rules_use_only_matched_tools`.
- `crates/factory-cli/tests/project_cmd.rs` (next to `project_show_reports_a_missing_security_agent`, built the same way with `show_without_security_agent`): `project_show_reports_a_missing_drafter_agent_file_when_intake_is_on` and `project_show_has_no_intake_problem_when_intake_is_off`.
- Commands: `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`, then `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings` and the same for `factory-cli`.

## Files changed

- `crates/factory-engine/src/intake.rs`, `crates/factory-engine/tests/intake.rs`
- `crates/factory-cli/src/project.rs`, `crates/factory-cli/tests/project_cmd.rs`
- `docs/guides/factory-user-guide.md`, `docs/reference/software-factory.md`

## Constraints

- Only `AgentRunner` and `Tracker` are traits; the new check reads the driver's agent files and touches no tracker.
- The GitHub comment keeps its fixed text. No agent text, no log tail, no cause string reaches it.
- No new board, ledger or event field. No `Cargo.toml` change, no `git` shell-outs, no `/proc` reads.
- Use `-j4`. Comments are short full sentences.

## Acceptance criteria

- `factory intake` and `factory intake --dry-run` refuse to start, with an error that names `factory presets refresh`, when the drafter agent file is missing, and they change no label.
- `factory project show` lists a problem when `intake = true` and the drafter agent file is missing.
- A drafter that stops `Failed` or `Stalled` ends the issue with `reason: drafter-failed`, and no job exists.
- A drafter that stops `TurnLimit` still ends `draft-invalid`.
- The drafter's grant for the draft folder is an `Edit(//<draft>/**)` rule, with no `Write(...)` rule, and a test fails if a path rule names an unmatched tool.
- An empty draft logs the last 600 characters of the drafter's last message locally, and the GitHub comment never holds it.
- The user guide lists the six reason codes and says to run `factory presets refresh` after an upgrade.

## Out of scope

- A live `claude` run in CI, and a `factory probe` role for the drafter.
- Retrying an issue automatically.
- Moving the claim after the draft.

## Follow-ups

- `kb_allowed_tools` (`kb.rs:31`) grants a `Write(//<kb>/**)` rule that Claude Code ignores, and prints a warning for. Drop it and its test line (`kb.rs:288`), keeping the `Edit` rule.

## Decisions

- What does intake do when the drafter stops for a reason that is not about the issue (`failed`, `stalled`)? Options: a new reason code `drafter-failed`, release the issue and retry later, keep `draft-invalid`. Chosen: a new reason code `drafter-failed`. Decided by: user
- Does a drafter that used all its turns get `drafter-failed` too? Options: yes, no (it stays `draft-invalid` because the issue is probably too big). Chosen: no. Decided by: call
- Should the pre-claim check also run for a dry run? Options: yes, no. Chosen: yes, so a dry run shows the setup problem. Decided by: call
- Is the Write rule replaced by an Edit rule, or are both granted as the KB run does? Options: Edit only, both. Chosen: Edit only, because the CLI says a Write rule never matches and warns on every run. Decided by: call
- Where does the tail of an empty draft's last message go? Options: the local log only, a GitHub comment. Chosen: the local log only, so no agent text is posted. Decided by: call
- How is the KB run's ignored Write rule handled? Options: fix it here, file a follow-up. Chosen: a follow-up, because the KB run works. Decided by: call
