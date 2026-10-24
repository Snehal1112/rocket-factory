# Structured logging gaps

## Goal

Add task, commit and operator events and richer agent-run fields to the structured log, and let `factory log` filter by event, so a job's cost and path can be read from `log.jsonl` alone.

## Overview

The factory already writes one JSON line per event through `factory-telemetry`. The events cover the stage changes, parks, checks, decisions and merges, but a builder's tasks are invisible: three tasks look like one `agent.start`/`agent.end` pair. `agent.end` carries only the weighted token count, so the cache split, turns, resume flag, round and context size are readable only in `usage.jsonl`. Commits and the operator commands `approve`, `rework` and `cancel` leave no log line (the stage change they cause is logged, the reason and the actor are not). Cost questions and "why did this job park" questions needed raw files plus guesses.

Evidence: the Rocket project's whole log is 35 KB, so volume is not a concern. Job 035 parked in task 1; its log shows `agent.end` and `park`, with no line that says which task, which session mode or how much context the run started with.

## Current state

| Piece | Where | Today |
|---|---|---|
| Subscriber, JSON layer, redaction, rotation | `crates/factory-telemetry/src/{init,layer,redact,rotate}.rs` | Done. Not changed by this spec. |
| Job and stage spans | `factory-engine/src/runloop.rs:96` (`job_span`), `:513` (`stage` span) | Every event inside a job run carries `job_id` and `stage`. |
| `agent.start`, `agent.end` | `factory-engine/src/ports.rs:186-196` (`run_agent`) | `agent.end` has `role`, `model`, `stop_reason`, `weighted_tokens`, `duration_ms`. |
| Usage detail | `factory-engine/src/ports.rs:285-320` (`usage_line`), written by `log_usage` to `usage.jsonl` | Has input, cache, output, turns, resumed, round, start and end context. Not in the log. |
| Task loop | `factory-engine/src/build.rs:507-720` (`build_task`) | Ledger rows only (`Dispatched`, `ImplementerDone`), no log line. |
| Commit step | `factory-engine/src/commit.rs:288` (`commit_step`), returns `CommitOutcome` | No log line. Callers: `build.rs:136`, `:671`, the batch path. |
| Operator commands | `factory-cli/src/commands.rs:557` (`approve`), `:613` (`rework`), `:641` (`cancel`) | Each runs inside `job_span`; only `stage.change` is logged. |
| `factory log` | `factory-cli/src/log.rs`, `factory-telemetry/src/read.rs` (`LogFilter`) | Filters by `--job`, `--level`, `--tail`. |

Gaps: no task events, thin `agent.end`, no commit event, no operator event, no event filter.

## Non-goals

- Replacing the `println!`/`eprintln!` calls. They are the human output of the CLI and `factoryd`, not log gaps.
- Events for series advance, sync, budget warnings and worktree or branch changes.
- A correlation id across processes, a `--role` filter, OpenTelemetry or a new file layout.
- Changing the log line format, rotation, redaction rules or the log level setting.
- Changing `usage.jsonl`, `events.jsonl`, the board or the ledger.

## Design

Flow: the engine and the CLI emit `tracing` events with `event = "<noun.verb>"`; the existing JSON layer writes them; `factory log --event <name>` reads them back.

### 1. Richer `agent.end`

`run_agent` logs `agent.end` with the current fields plus: `input`, `cache_creation`, `cache_read`, `output` (u64), `turns` (u32), `resumed` (bool, `spec.resume.is_some()`), `round` (u32), `start_ctx`, `end_ctx` (u64), and `session_id` (string, empty when the run reported none). `round` is read from the job like `log_usage` does, with the same fallback of 1 on a board read error. The field names avoid `token`, `key`, `secret` and `password`, because `redact_field` would write them as `[redacted]`. `weighted_tokens` stays.

```json
{"event":"agent.end","role":"builder","model":"sonnet","stop_reason":"Done","weighted_tokens":56978,"input":10,"cache_creation":34419,"cache_read":151613,"output":1790,"turns":5,"resumed":false,"round":1,"start_ctx":30748,"end_ctx":42784,"session_id":"…","duration_ms":20587}
```

### 2. `task.start` and `task.end`

In `build_task`, right after the `Dispatched` ledger row, log `task.start` with `task` (the plan's own task number, `task.n`), `title` (folded to one line, then capped by `cap_message`, which keeps the last 300 characters), `how` (`resumed`, `continued`, `fresh`, or `fresh (<reason>)`, the string already built there), `model` (`config.builder_model`) and `of` (`tasks.len()`, the number of tasks in the plan). Right after the `ImplementerDone` ledger row, log `task.end` with `task`, `sha` (the full new tip, the same string `commit.done` logs), `of` and `duration_ms`. Both carry a short `msg`. Every entry into `build_task` logs one `task.start`, so a task re-entered after a fix round, a rework or a driver restart logs another one. `duration_ms` covers that one call, from its start to the `ImplementerDone` row, not the task's whole time. A task that parks logs `task.start` and the existing `park` line, and no `task.end`. The fresh retry after a failed resume, inside the same call, does not log a second `task.start`.

### 3. `commit.done`

One event per `Committed` outcome of `commit_step`, whether the agent or the fallback made it: `kind` (`CommitKind::as_str`), `sha` (the full tip string from `CommitOutcome::Committed`), `fallback` (bool). `commit_step` becomes a thin wrapper that calls the current body and logs on `Ok(CommitOutcome::Committed { .. })`. `NothingStaged` and `Park` log nothing here, because a park is logged by its caller.

The driver's own snapshot commit in `build_task` (`commit_batch` mode, `repo.commit_staged` with `snapshot_subject`) skips `commit_step`. It logs `commit.done` too, with `kind` `snapshot` (a plain string, `CommitKind` gets no new variant), the full sha it returns and `fallback` false, right after the commit succeeds.

### 4. Operator events

Right after the `update_job` call that changes the board succeeds (not after the later ledger or file writes), inside the existing job span. Only the CLI commands log these; the supervisor does not approve, rework or cancel a job:

- `operator.approve` in `approve`, after the stage moves to `approve` and before the merge: `without_review` (bool).
- `operator.rework` in `rework`, after the stage moves to `build`: `note` (capped by `cap_message`).
- `operator.cancel` in `cancel`, after the board update: `reason` (capped), `force` (bool).

A refused command (wrong stage, live lease, bad note) logs no `operator.*` line.

### 5. `factory log --event`

`LogFilter` gains `event: Option<String>`. A line matches when its `event` equals the value, or starts with the value followed by `.`. So `--event task` matches `task.start` and `task.end`, `--event agent.end` matches only that event, and `--event tas` matches nothing. The filter combines with `--job`, `--level` and `--tail` by AND, and `--tail` applies last. `factory log` gets `--event <name>`.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| Log write fails | Unchanged: never changes a stage, merge or command. |
| Board read fails inside `run_agent` | `round` falls back to 1, like `usage.jsonl`. |
| Run reported no session | `session_id` is the empty string. |
| Task title is long or has newlines | `title` is folded to one line, then capped by `cap_message`. |
| Task parks | `task.start` without `task.end`; the `park` line explains. |
| Commit falls back to the git2 commit | `commit.done` with `fallback: true`. |
| Commit is `NothingStaged` or `Park` | No `commit.done`. |
| Operator command refused | No `operator.*` line. |
| Rework note has a quote or backslash | Written as JSON string, escaped by the layer. |
| `--event ""` | Refused: `empty event, use a name such as task or agent.end`. |
| `--event` matches nothing | Prints nothing, exit 0 (the same as an empty `--job` match). |
| `--event` with a rotated log | Applies across `.3`, `.2`, `.1` and the file, as the other filters do. |

## Testing

`crates/factory-engine/tests/log_events.rs` (uses `factory_telemetry::capture`):
- `agent_end_carries_usage_fields`: a builder run with a fake runner reporting known usage; asserts every new field value and that none is `[redacted]`.
- `agent_end_resumed_is_true_for_a_resume_spec`.
- `task_start_and_end_are_logged_per_task`: a two-task plan; asserts two `task.start`, two `task.end`, the `of`, `how` and `sha` fields.
- `parked_task_has_no_task_end`: a blocked builder; one `task.start`, zero `task.end`, one `park`.
- In `tests/commit.rs`, which already calls `commit_step` directly: `commit_done_is_logged_for_an_agent_commit`, `commit_done_marks_the_fallback` and `no_commit_done_when_nothing_is_staged`; in `tests/log_events.rs`, `batch_snapshot_logs_commit_done_with_kind_snapshot` and `task_start_repeats_when_the_task_is_re_entered`. Each is wrapped in `factory_telemetry::capture`.

`crates/factory-telemetry/src/read.rs` unit tests: `event_filter_matches_exact_and_prefix`, `event_filter_ignores_a_partial_word`, `event_filter_combines_with_job_and_level`, `event_filter_then_tail`.

`crates/factory-cli`: `cli.rs` `log_parses_event_flag`; in `tests/log_cmd.rs`, which already drives `factory log`: `log_event_empty_is_refused` and `log_event_filters_by_prefix`; the operator events are tested in the same file or in `tests/e2e.rs`, whichever already runs the command with a log file (`approve_logs_operator_approve`, `rework_logs_the_note`, `cancel_logs_reason_and_force`, `refused_approve_logs_nothing`).

Commands: `cargo test -j4 -p factory-telemetry`, `cargo test -j4 -p factory-engine --test log_events`, `cargo test -j4 -p factory-cli`, `cargo clippy -j4 -p factory-telemetry -p factory-engine -p factory-cli --all-targets -- -D warnings`.

## Files changed

- `crates/factory-engine/src/ports.rs`: `agent.end` fields.
- `crates/factory-engine/src/build.rs`: `task.start`, `task.end`.
- `crates/factory-engine/src/commit.rs`: `commit_step` wrapper and `commit.done`.
- `crates/factory-cli/src/commands.rs`: `operator.*` events.
- `crates/factory-cli/src/cli.rs`, `crates/factory-cli/src/log.rs`: `--event`.
- `crates/factory-telemetry/src/read.rs`: `LogFilter.event`.
- `docs/software-factory.md` (Logging table and `factory log` line), `docs/factory-admin-manual.md` (`log` row).
- The tests above.

## Constraints

- All new events are `info` and use the `noun.verb` names above; existing event names and fields stay as they are, new fields are additive.
- New field names must not contain `token`, `key`, `secret` or `password`.
- Run cargo with `-j4`, per package, never `--workspace`, never set `CARGO_TARGET_DIR`.
- A log failure still never changes a stage, a merge or a command.

## Acceptance criteria

- `agent.end` carries `input`, `cache_creation`, `cache_read`, `output`, `turns`, `resumed`, `round`, `start_ctx`, `end_ctx`, `session_id`, and none is redacted.
- Each task logs `task.start` with `task`, `title`, `how`, `model`, `of`; a finished task logs `task.end` with `task`, `sha`, `of`, `duration_ms`; a parked task has no `task.end`.
- `commit.done` is logged once per commit the driver makes, with `kind`, `sha`, `fallback`: once per `Committed` outcome of `commit_step`, and once per batch snapshot commit (`kind` `snapshot`). It is never logged for `NothingStaged` or `Park`.
- `approve`, `rework` and `cancel` log `operator.approve`, `operator.rework` and `operator.cancel` with the fields above after success, and nothing when refused.
- `factory log --event <name>` matches the exact event or the dotted prefix, combines with `--job`, `--level` and `--tail`, and refuses an empty value.
- `docs/software-factory.md` and `docs/factory-admin-manual.md` list the new events and the flag.
- `cargo test -j4` for the three crates and clippy with `-D warnings` pass.

## Out of scope

Series, sync, budget and git events, a correlation id, a `--role` filter, OpenTelemetry, and any change to prints, rotation or redaction.

## Follow-ups

- Log events for series advance, sync and budget warnings (80 percent and stop).
- A correlation id across the driver, `factoryd` and agent children, with `factory log --run`.
- A `--role` filter for `factory log`.

## Decisions

- What is missing in the existing structured logging (the `factory-telemetry` crate)? Options: fill the gaps in the engine and agent runs with stage and agent-run events and no new infrastructure; add a correlation id and `factory log --run`; change the output with OpenTelemetry or a new file layout; something else. Chosen: fill the gaps in the engine and agent runs, no new infrastructure. Decided by: user
- Which gaps does this spec close? Options: tasks, rich agent runs, commits and operator actions; all gaps (also series, sync, budget, git); only a richer `agent.end`; something else. Chosen: tasks, rich agent runs, commits and operator actions. Series, sync, budget and git events stay out. Decided by: user
- What do the new events look like? Options: `noun.verb` names at `info` like the existing ones; `debug` for per-task and per-commit events; names outside the `noun.verb` style; something else. Chosen: `noun.verb` names at `info` (`task.start`, `task.end`, `commit.done`, `operator.approve`, `operator.rework`, `operator.cancel`, and a richer `agent.end`), redacted like today. Decided by: user
- Should `factory log` filter by event? Options: add `--event <name-or-prefix>`; no CLI change; add `--event` and `--role`; something else. Chosen: add `--event <name-or-prefix>` only. Decided by: user
- Does a parked task log `task.end`? Chosen: no, `task.start` plus the existing `park` line is enough. Decided by: call
- Where does `commit.done` fire? Chosen: once per `Committed` outcome, in a wrapper around `commit_step`, so the agent and fallback paths both log. Decided by: call
- When do the operator events fire? Chosen: after the state change succeeds, so a refused command logs nothing. Decided by: call
- What does `--event ""` do? Chosen: refuse it with an error. Decided by: call
- Does the batch snapshot commit log `commit.done`? Chosen: yes, with `kind` `snapshot`. Decided by: call
- Short or full sha? Chosen: full sha in `task.end` and `commit.done`, so the two join, like `merge.done`. Decided by: call
- What are `task` and `of`? Chosen: the plan's task number and the plan's task count. Decided by: call
- Is `task.end.duration_ms` the whole task time? Chosen: no, it covers one `build_task` call, and each entry logs its own `task.start`. Decided by: call
- Which write counts as "state change succeeded" for operator events? Chosen: the board `update_job` call. Decided by: call
- Do supervisor paths log operator events? Chosen: no, only the CLI commands do. Decided by: call
