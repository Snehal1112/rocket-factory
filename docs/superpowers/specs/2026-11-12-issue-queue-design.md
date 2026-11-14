# Issue queue design

## Goal

Let the factory find GitHub issues that a trusted user labelled, draft a spec and plans for each without help, and fix them one at a time in priority order.

## Overview

Today every job starts in a `/factory` session: a person does the interview, the spec and the plans, then the driver builds, reviews and merges. Issues #58 to #61 and #65 went through that path by hand. The user wants the factory to take labelled issues on its own, highest priority first, one after another.

Four facts shape the design.

- The driver never writes a spec or a plan. `create_job_with_spec_accepting` (`runloop.rs:441`) only validates files that already exist, runs the spec review and adds the jobs.
- The issue body is untrusted text. It becomes the input of an agent that writes code and merges it, so the label is the trust gate: only a label set by a trusted user makes an issue eligible.
- `factoryd` runs queued requests (`request.rs:30` has `Action::Run` and `Action::Approve`) and starts one `factory` child per project (`scheduler.rs:34`). It has no timer that creates work.
- The `Tracker` port (`tracker.rs:12`) can read labels and state of one issue, create, comment on and close an issue. It cannot list or relabel issues.

## Current state

| Piece | Today | Gap |
|---|---|---|
| `Tracker` (`tracker.rs:12-24`) | `labels_of`, `is_closed`, `create`, `find_created`, `comment`, `close` | No list, no label events, no label edit |
| `GhTracker` args (`tracker.rs:50-110`) | `issue view`, `create`, `list` for `find_created`, `comment`, `close` | Same |
| Job creation | `create_job_with_spec_accepting(driver, slug, feature, plan, spec_path, model, accept_spec)` | Needs files that a person wrote |
| `Request` / `Action` | `Run`, `Approve`, both carry a job id | No request without a job |
| Scheduler `decide` | Starts a queued request when the project is idle and a slot is free | Nothing queues work by itself |
| Config (`config.rs:129`) | `tickets_on_merge`, `minor_tickets` and friends | No intake settings |

## Non-goals

- Pushing to origin. Merges stay local, as for every job.
- Reviewing the drafted design with a person. The spec review and the code review are the only gates.
- Issues that are not issues (pull requests) and issues in other repositories.
- Several projects' queues sharing one daily budget logic beyond what `factoryd` has.

## Design

```
factoryd timer (intake on) -> Intake request -> factory --project P intake --once
  list labelled issues -> drop untrusted/blocked -> sort by priority, age -> take the first
  claim: label "factory: running", remove the intake label
  drafter agent (Opus) writes spec.md + plan-NN-*.md in <state>/intake/<issue>/
  create_job_with_spec_accepting (plans required) -> comment with the job id -> run the series -> merge
  success: fixed comment, close the issue, label "factory: done"
  failure: label "factory: needs-human", fixed comment with a reason code, no agent text
```

### 1. Settings

Added to `Config` and to the project settings block (`settings_block.rs`), all optional:

| Key | Default | Rule |
|---|---|---|
| `intake` | `false` | Turns polling and `factory intake` on |
| `intake_label` | `"factory"` | Non-empty, no comma |
| `priority_labels` | `["priority: P0", "priority: P1", "priority: P2"]` | Highest first, no duplicates |
| `intake_poll_minutes` | `15` | At least 1 |
| `intake_trusted` | `[]` | Logins; empty means the `gh` user the factory runs as |
| `intake_model` | `"claude-opus-5-5"` | Non-empty |
| `intake_budget_weighted` | `400000` | Above zero; the drafter's cap per issue |

A bad value is a config error, which fails closed like every other setting.

### 2. Tracker additions

On the `Tracker` trait and `GhTracker` (with the `Arc<T>` forwarding impl):

- `fn list_labelled(&self, label: &str) -> Result<Vec<IssueInfo>, String>` using `gh issue list --state open --label <label> --json number,title,body,labels,createdAt`. `IssueInfo { number: u32, title: String, body: String, labels: Vec<String>, created_at: String }`.
- `fn labelled_by(&self, issue: u32, label: &str) -> Result<Option<String>, String>` returning the login of the user who last added `label`, read with `gh api repos/{owner}/{repo}/issues/{n}/events` (the `labeled` events). `None` when no event is found.
- `fn edit_labels(&self, issue: u32, add: &[String], remove: &[String]) -> Result<(), String>` using `gh issue edit --add-label/--remove-label`. A missing label to add is created first with `gh label create <name> --force`.
- `fn current_user(&self) -> Result<String, String>` using `gh api user`.

### 3. Picking an issue

`pub fn pick_next(issues: &[IssueInfo], cfg: &Config, trusted_label_actor: impl Fn(u32) -> Option<String>, trusted: &[String]) -> Option<IssueInfo>` in a new `factory-engine/src/intake.rs`. An issue is eligible when it is open, has `intake_label`, has none of `factory: running`, `factory: needs-human`, `factory: done`, and `labelled_by` is in the trusted list. Eligible issues sort by the index of the first matching entry of `priority_labels` (none sorts last), then by `created_at`, then by number. The first one wins. An issue that fails the trust rule is skipped and logged, never labelled.

### 4. The drafter role

A new agent file `factory-intake-drafter.md` in the five presets (`crates/factory-core/presets/<preset>/agents/`), run headless through the existing `AgentRunner` port with `intake_model` and `intake_budget_weighted`. It may read only the repo root (`Read`, `Grep`, `Glob` rules built from the real repo path, with the run's cwd the repo root, so a factory home outside the repo and its key files are out of reach). The default layout keeps every factory folder inside the repo (`layout.rs:122-130`: state, jobs, worktrees, config file, commit context file), so the spec also sets `disallowed_tools` (`factory-agent/src/spec.rs:71`) with deny rules for `Read`, `Grep` and `Glob` on each of them. `Layout::protected_paths(&self) -> Vec<PathBuf>` returns those five paths, and the deny rules are built from it for the paths that lie under the repo root. A destructuring test lists every `Layout` field, so a field added later fails to compile until it is classified. The drafter's own draft folder is under the state folder and is not readable either, because the issue text and the repo are in the prompt or the working tree. The drafter writes only inside `<state>/intake/<issue>/` (a `Write` grant for that folder, built from the real path and checked like `kb_path`). Its prompt carries the issue title and body as data, marked as untrusted, plus the `plan_rules` and the spec quality bar of the `/factory` command. It writes `spec.md` and one to three plans named `plan-01-<slug>.md`, each with at most three tasks and a final `Next plan:` line, or a file `refused.md` holding one paragraph when the issue cannot be built unattended (too big, unclear, needs a decision). `agent_lint` covers the new file.

The drafts stay in the state folder, not in the repository. Nothing an agent wrote for an intake job is posted to GitHub: intake's comments hold fixed text only (see 5), and the post-merge steps cannot post for an intake job (see 6).

### 5. `factory intake`

`factory --project P intake [--once] [--dry-run]`. `--dry-run` prints the issue it would pick and why, and changes nothing. `--once` does one pass (the form `factoryd` runs). Without a flag it behaves like `--once`. A pass:

1. If `intake` is false, print `intake is off` and stop.
2. List, filter and pick. No issue: print `no issue` and stop.
3. Claim the issue: add `factory: running` and remove `intake_label`. No other label changes happen before this point, so a `gh` failure while listing or picking leaves every label alone.
4. Run the drafter. A `refused.md`, a budget stop or a missing `spec.md` is a failure with no job. The engine then validates and creates the jobs through `create_job_with_spec_accepting` with the drafted plans and without `--accept-spec`; the `plan = None` path (a job from the feature text alone) is never used. On a validation or spec review refusal it redrafts once with the findings, then fails.
5. Once the jobs exist, comment `factory picked this up as job <id>`, then run the series (`run_series`) and wait for the end.
6. Merged: swap `factory: running` for `factory: done`, comment `merged as job <ids>` and close the issue. Intake always closes it, because the ticket step does not run for intake jobs (section 6).
7. Any other end or any failure: swap `factory: running` for `factory: needs-human` and comment a fixed text: `factory: needs-human`, one reason code (`draft-refused`, `draft-invalid`, `spec-review`, `over-budget`, `build-failed`), the job id when one exists, and the local path of the draft folder. The agent's own text (`refused.md`, review findings) stays on the machine. Never retry the issue until a person relabels it.

An estimate over the job budget is a failure; `--accept-over-budget` is never passed.

### 6. Intake jobs cannot post through the tracker

`Job` gets `intake_issue: Option<u32>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`, optional, so the board stays compatible with `dashboard/`). `run_intake` sets it on every job of the series it creates.

The post-merge steps that talk to GitHub (`tickets.rs`, `minors.rs`, `waivers.rs`) would post text that agents wrote from the untrusted issue body: follow-ups from the drafted `spec.md`, `[minor]` lines and waived blockers from reviews of code built from it. The rule is deny by default and sits on the tracker, not on a list of steps:

- `Driver::tracker()` (`compat.rs:295`) is renamed `Driver::raw_tracker()` and made crate-private. A new `Driver::tracker_for(&self, job: &Job) -> Box<dyn Tracker + '_>` returns the raw tracker for a job with no `intake_issue`, and a `GuardedTracker` for a job with one.
- `GuardedTracker` forwards the reading calls (`repo_name`, `labels_of`, `is_closed`, `find_created`, `list_labelled`, `labelled_by`, `current_user`) and refuses `create`, `comment`, `close` and `edit_labels` with the error `refused for an intake job`.
- The three steps get their tracker through `tracker_for(&job)`. A refusal is a soft failure like any other: it goes to the ledger and the merge result does not change. The ticket step and the minor ticket step also return at once for an intake job, so no refusal is logged on every merge.
- Only `intake.rs` uses `raw_tracker()` for its own fixed-text calls. A source test fails when any other file under `crates/factory-engine/src` names `raw_tracker`.

Intake therefore closes the issue itself. The follow-ups and minor findings stay in the job folder. Skipping the steps also means nothing copies the issue's `factory:` or priority labels onto new issues.

### 7. `factoryd`

`Action::Intake` is added with `job` the fixed string `"000"` (valid under `validate_job`), no series, no approve flags. The scheduler's existing rule (`scheduler.rs:52`, which holds `Action::Run` when the daily budget is used up) is extended to hold `Intake` too, in place and not as a second rule. The scheduler treats it like `Run`: it waits while the project is busy or the daily budget is used up. A timer in the supervisor loop, for each project whose settings have `intake = true`, enqueues an `Intake` request every `intake_poll_minutes` when no request of that project is queued or running. The spawned child is `factory --project <p> intake --once`.

### 8. Docs

`docs/reference/project-toml-reference.md` gets the settings table. `docs/guides/factory-user-guide.md` gets a section on labels, the trust rule, the states and how to turn it on.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| `intake = false` | No polling, `factory intake` prints `intake is off` |
| No eligible issue | Nothing changes |
| Label set by an untrusted user | Skipped, logged, not labelled |
| Two issues, `P1` older and `P0` newer | The `P0` one first |
| No priority label | Sorts after every priority label |
| Issue already carries a `factory:` state label | Not picked |
| `gh` fails while listing or picking | The pass ends with an error line; no label has changed |
| `gh` fails after the claim | The pass ends with an error line; the issue keeps `factory: running` and a person relabels it |
| Drafter writes `refused.md` | Failure path with reason code `draft-refused`; the text stays local |
| Drafted decisions in a risky area end `Decided by: call` | The spec review refuses them twice and the issue ends `factory: needs-human`. This is intended: a risky decision needs a person |
| Failure with no job | The comment has no job id, only the reason code and the draft path |
| Issue body tells the drafter to read or copy a secret | The read rules stop at the repo root and no agent text is posted |
| Spec review refuses twice | Failure path |
| Plans exceed three tasks or six plans | Validation refusal, one redraft, then failure |
| Job ends `needs-human` | Failure path; the issue is not retried |
| Drafter tries to write outside its folder | Refused by the permission grant; run fails |
| Daily budget used up | The `Intake` request waits, as `Run` does |

## Testing

- `crates/factory-core`: `job_intake_issue_is_optional_and_absent_from_old_boards` (an old board file parses, a job without the field serialises without it), `intake_settings_have_the_documented_defaults`, `bad_intake_settings_are_config_errors` (empty label, comma, duplicate priority, zero poll, zero budget).
- `crates/factory-engine/src/intake.rs` unit tests: `pick_next_prefers_the_higher_priority_then_the_older_issue`, `pick_next_sorts_unprioritised_issues_last`, `pick_next_skips_state_labelled_issues`, `pick_next_skips_untrusted_labellers`.
- `crates/factory-engine/tests/intake.rs` (with the test `FakeTracker` and `FakeRunner`): `a_pass_with_no_eligible_issue_changes_nothing`, `a_merged_issue_is_commented_closed_and_labelled_done`, `a_refused_draft_labels_the_issue_needs_human` (the comment holds the reason code and the path, not the refused text), `a_failure_comment_never_contains_agent_text`, `intake_marks_every_job_of_the_series`, `draft_spec_reads_only_the_repo_root`, `draft_spec_denies_every_layout_path_under_the_repo`, `layout_protected_paths_cover_every_field`, `intake_never_creates_a_job_without_plans`, `a_spec_review_refusal_redrafts_once_then_fails`, `a_dry_run_changes_no_label`.
- `crates/factory-engine`: `guarded_tracker_refuses_every_write_and_forwards_reads`, `tracker_for_returns_the_raw_tracker_without_a_marker`, `the_ticket_step_skips_an_intake_job` (no `create`, `comment` or `close` call with `tickets_on_merge` on), `the_minor_ticket_step_skips_an_intake_job`, `a_waived_approve_of_an_intake_job_posts_nothing`, and the source test `only_intake_uses_the_raw_tracker`; the existing tests for other jobs keep passing.
- `crates/factory-engine/tests/tracker_gh.rs` style tests for the new `gh` argument builders: `list_labelled_args`, `edit_labels_args`, `events_args`.
- `crates/factory-supervisor`: `intake_requests_wait_while_the_project_is_busy`, `an_intake_request_is_not_enqueued_twice`, `intake_polling_is_off_for_projects_without_the_setting`.
- `crates/factory-cli/tests/agent_lint.rs` passes for the new agent file in all five presets.
- Commands: `cargo test -j4` per crate: `factory-core`, `factory-engine`, `factory-supervisor`, `factory-cli`; `cargo clippy -j4` for the same crates.

## Files changed

- `crates/factory-core/src/board.rs` (the optional `intake_issue` field)
- `crates/factory-engine/src/compat.rs`, `tracker.rs`, `tickets.rs`, `minors.rs`, `waivers.rs`
- `crates/factory-core/src/config.rs`, `settings_block.rs`, presets `<preset>/agents/factory-intake-drafter.md`
- `crates/factory-engine/src/tracker.rs`, `intake.rs`, `lib.rs`
- `crates/factory-cli/src/cli.rs`, `commands.rs`, `intake.rs`
- `crates/factory-supervisor/src/request.rs`, `scheduler.rs`, `supervisor.rs`
- Tests as listed, the two docs

## Constraints

- Only `AgentRunner` and `Tracker` are traits; the new engine logic goes behind them. `gh` is allowed, behind the port; no shell-out to `git`.
- Board JSON stays compatible with `dashboard/`; any new board field is optional. The request file format keeps working with older files: the new action is additive.
- No `Cargo.toml` change. Use `-j4`. Comments are short full sentences.
- The drafter never gets Bash, network or write access outside its draft folder.

## Acceptance criteria

- With `intake = true`, a trusted user's `factory` label makes an open issue eligible; the same label from anyone else does not.
- Eligible issues run one at a time, `priority: P0` before `P1` before `P2`, then oldest first.
- A picked issue gets `factory: running`, a comment with the job id, and on merge a closing comment and `factory: done`.
- A failed or parked issue gets `factory: needs-human` and a fixed comment with a reason code, and the queue moves on. No agent-written text is posted.
- The drafter cannot read outside the repo root or write outside its draft folder.
- A job is never created without drafted plans.
- Intake closes a merged issue itself. No other step can create, comment on, close or relabel a GitHub issue for an intake job, including after `approve --waive`, and a step added later is covered the same way.
- The drafter cannot read any `Layout` path under the repo root (state, jobs, worktrees, config), and a new `Layout` field cannot be added without being classified.
- A board without `intake_issue` parses as before and the dashboard is unaffected.
- `factory intake --dry-run` names the issue it would pick and changes nothing.
- `factoryd` enqueues at most one `Intake` request per project at a time, and only when `intake` is on.
- Nothing is pushed.
- Existing `Run` and `Approve` requests and old request files behave as before.

## Out of scope

- Pushing, opening pull requests, and cross-repository issues.
- Learning from past failures to change the drafter prompt.

## Follow-ups

- A dashboard panel for the intake queue and the labelled issues.

## Decisions

- Who writes the spec and plans for a labelled issue? Options: unattended draft with a human plan approval, fully unattended, semi-automatic hand-over. Chosen: fully unattended. Decided by: user
- Which labels? Options: configurable with defaults `factory` and `priority: P0/P1/P2`, fixed names, intake label only. Chosen: configurable with those defaults. Decided by: user
- When does the factory look at GitHub? Options: `factoryd` polls and is off by default, manual command only, always poll. Chosen: `factoryd` polls, off by default. Decided by: user
- What happens when a picked issue ends `needs-human`? Options: label, comment and move on; stop intake; retry once. Chosen: label, comment and move on. Decided by: user
- Who may make an issue eligible? Options: only labels set by trusted users, trust any label, trusted author and labeller. Chosen: only labels set by trusted users. Decided by: user
- Which model drafts? Options: Opus capped, Sonnet capped, Sonnet escalating to Opus. Chosen: Opus with `intake_budget_weighted = 400000`. Decided by: user
- Where do the drafts live? Options: the state folder, committed docs. Chosen: the state folder, so an unattended run does not commit to main. Replaced in part by the next two decisions about what the drafter may read. Decided by: user
- Is an over-budget estimate accepted? Options: never, ask. Chosen: never; it is a failure. Decided by: call
- What does a failure comment contain? Options: fixed reason code and local path, agent text redacted and capped, no comment. Chosen: fixed reason code and local path. Decided by: user
- Who closes the issue on merge when both intake and the ticket step are on? Options: the ticket step if on and else intake, intake always, never close. Chosen first: the ticket step if on, else intake. Replaced by the next decision, which skips the ticket step for intake jobs, so intake always closes. Decided by: user
- What do the post-merge ticket steps do for intake jobs? Options: skip both the follow-up filing and the minor ticket, accept that they post agent text, skip follow-ups only. Chosen: skip both. Decided by: user
- How should intake jobs be protected from posting agent text? Options: a default-deny gate in the tracker, add the waiver step to a skip list, refuse `approve --waive` on intake jobs. Chosen: a default-deny gate in the tracker. Decided by: user
- How are intake jobs recognised? Options: an optional `Job.intake_issue` board field, the `issue-<n>` slug. Chosen: the optional field. Decided by: user
- Where do the drafts live and what is the drafter denied? Options: the state folder with a deny on reads of it, a temp folder outside the repo, refuse a repo-local state folder. Chosen: the state folder with an explicit deny on reads of it. Decided by: user
- How is the drafter's read access limited? Options: deny every `Layout` path inside the repo, run on a clean copy of the base branch. Chosen: deny every `Layout` path inside the repo, built from `Layout::protected_paths`, with a test that covers every field. Decided by: user
- Where does the claim happen? Options: before the draft, after the job exists. Chosen: before the draft, because a job needs the drafted plans first. Decided by: call
