# Builder task resume design

## Goal

Let the builder of task N+1 resume the session of task N within one plan, guarded by an idle-gap check and a context-size cap, behind a project setting that is off by default, so a task no longer pays a full cold start when the prompt cache is still alive.

## Overview

Every task of a plan starts a fresh builder that loads about 23k tokens of fixed context (system prompt, tools, agent file, CLAUDE.md, user settings). A typical 3-task job pays that three times. A real-run measurement (builder launch shape, 1 hour cache) showed a resumed second task costing 5,913 weighted tokens against 13,474 for a fresh one. In the factory the gap between two task builders was 4 to 25 seconds in 6 of 7 known cases (jobs 018 to 021), so the cache is normally alive. The same measurement showed the read part growing with each resume (27k, 43k, 69k), and the project cache TTL is 5 minutes (`cache_ttl = "5m"`), so a resume after a long gap would rewrite the whole context at full price. The design therefore resumes only when the gap and the context size say it will pay.

## Current state

| Area | Today | Where |
|---|---|---|
| Per-task builder | `build_task` starts a fresh builder for each task, or resumes a saved crash session | `crates/factory-engine/src/build.rs:511-561` |
| Crash resume | `saved_session` returns the board's `task_session` id when task number and tip match | `build.rs:25-30`, `:528` |
| Session hook | `session_hook` saves `task_session` while a task builder runs | `build.rs:482-500`, wired at `:545` and `:575` |
| Failed resume | A `Failed` stop of a saved resume gets one fresh builder | `build.rs:562-583` |
| Advance | After a task the update sets `builder_id`, `task_index`, `batch_snapshot`, clears `task_session` | `build.rs:660-670` |
| Fix round | Resumes `builder_id` with the findings file | `build.rs:62-77`, `specs.rs:357` |
| Resume spec | `builder_task_resume_spec` builds the permissions, agent, tools and limits for a resume | `crates/factory-engine/src/specs.rs:339-354` |
| Board | `builder_id`, `task_session` are optional fields | `crates/factory-core/src/board.rs:63`, `:128-130` |
| Run usage | `StreamState` keeps `first_usage` and a per-message map, but no last usage | `crates/factory-agent/src/stream.rs:96-110`, `:184` |
| Usage line | `usage_line` writes `start_cc`, `start_cr`, `usage_v` 3 | `crates/factory-engine/src/ports.rs` |
| Settings | `Config` fields with defaults and validation, `deny_unknown_fields` | `crates/factory-core/src/config.rs:64`, `:100`, `:217-224` |

Gaps: no time or size of the previous builder run is stored, nothing decides to continue a session between tasks, and the builder agent file says `Later tasks belong to other agents`.

## Non-goals

- Changing fix rounds (they keep resuming `builder_id`).
- Editing the builder agent files or bumping `RULES_VERSION`.
- Stripping the reviewer's context.
- Carrying a session into task 1, the whole-plan builder, or after a park.
- A separate idle-limit setting.

## Design

Flow: a task advances and records `builder_last`. The next `build_task` asks `carry_decision`. On yes it runs a carried resume. Otherwise it runs a fresh builder as today. Both paths write the outcome to the ledger line of the task.

### 1. End-of-run context

- `StreamState` keeps `last_usage: Option<Usage>`: the merged usage (the `max_with` value kept per message id) of the last main-thread assistant message. A line counts as main thread when its `parent_tool_use_id` is null or absent; lines of helper agents (the builder has the `Agent` tool) carry an id and are ignored. `pub fn end_ctx(&self) -> u64` returns `input + cache_creation + cache_read` of it, 0 when there is none, saturating.
- `AgentRun` gets `end_ctx: u64` (0 in `AgentRun::done`), filled in `claude.rs` next to `start_ctx`.
- `usage_line` adds `"end_ctx"` and sets `usage_v` to 4. `UsageRecord` gets `end_ctx: Option<u64>`. The COLD STARTS table of `factory tokens --cache` gets an AVG-END column: the mean `end_ctx` of the role's runs that have it, `-` when none.

### 2. Settings and board field

- `builder_carry_session: bool`, default `false`.
- `builder_carry_max_ctx: u64`, default `100000`, must be above 0 (`Config::parse` error otherwise).
- The idle limit is not a setting: `carry_idle_limit(cache_ttl: Option<&str>) -> u64` returns 3300 for `"1h"` and 240 for `"5m"` or unset.
- `Job.builder_last: Option<BuilderLast>` with `BuilderLast { task: u32, id: String, ended_at: u64, end_ctx: u64 }`. It is optional, absent when `None` (`skip_serializing_if`), and the dashboard ignores it.
- It is set in the advance update (`build.rs:664-670`) from the run that just finished: `task` is the finished task number, `id` is `run.session_id` (else the earlier id), `ended_at` is the clock read right after `run_builder_guarded` returns (before checks and the snapshot), `end_ctx` is `run.end_ctx`. It is not set on a park. `build_task` clears it when it dispatches a builder (in the same update that drops a stale `task_session`), so a park of task N+1 or a crash leaves none and a later `rework` starts fresh. `factory cancel` clears it.

### 3. The carry decision

`fn carry_decision(job: &JobView, config: &Config, task_n: u32, now: u64, rework_note: bool, saved: bool) -> Carry` is pure. `Carry` is `Yes { id: String }` or `No { reason: String }`. The first matching rule wins:

1. setting off: `No("off")`
2. `saved` (a crash session exists): `No("crash resume")`
3. `task_n == 1`, or no `builder_last`: `No("first task")`
4. `rework_note`: `No("rework note")`
5. `builder_last.task + 1 != task_n`: `No("previous session is not task N-1")`
6. `now - ended_at` above the idle limit: `No("idle 312s over 240s")`
7. `end_ctx` above `builder_carry_max_ctx`: `No("context 130000 over 100000")`
8. otherwise `Yes { id }`.

### 4. Running a carried task

- In `build_task`, after `saved_session` (`build.rs:528`), call `carry_decision`. The ledger line is `BASE <base>, <model>, <how>` with `how` one of `resumed` (crash), `continued`, `fresh`, or `fresh (<reason>)` when the setting is on. With the setting off the text stays exactly `fresh`.
- A carried run uses `builder_task_carry_spec`, built like `builder_task_resume_spec` (`--resume`, `--agent factory-builder`, `strict_mcp`, tools, `add_dirs` from `kb_dirs()`, `max_turns` 120, permissions through `with_builder_permissions`, limits through `with_limits`, the session hook for task N+1).
- Prompt of a carried run: `Task {prev} is done and committed as {sha7}. Base {base}, current tip {tip}, the worktree is clean. The driver now gives you Task {n} in this same session; this replaces the earlier line that later tasks belong to other agents. Do only Task {n}: {title}. Do not redo or rework earlier tasks.` followed by the task section from the plan. The header, spec excerpt, rules and KB line are already in the session. `{sha7}` is `short_sha(tip)` of the branch tip at the start of `build_task`, which is the snapshot or task commit of task N. The prompt does not claim the worktree is clean.
- The failed-resume fallback (`build.rs:562`) also runs when a carried run ends `Failed`: one fresh builder, and the ledger gets `fresh (carry failed)`.
- Stalls, limits, tampering and a blocked builder park without a second run, as for any task. The tamper guard and agent-file rewrite run per run as before.
- A fix round is unchanged: it resumes `builder_id`, which is the last task's session.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| Setting off | Fresh builders, ledger text `fresh`, no new fields read |
| Task 1, whole-plan builder | Never carried |
| Crash during a carried task | `task_session` holds the shared id; the crash resume wins (rule 2) |
| `--resume` returns a different session id | The new `run.session_id` is stored |
| Resume fails (missing transcript) | One fresh builder, ledger `fresh (carry failed)` |
| Rework note on disk | Fresh builder |
| Driver restarted between tasks | `ended_at` is on the board, so the gap check still works |
| Clock earlier than `ended_at` | The gap counts as 0 |
| `cache_ttl` unset | Limit 240 seconds |
| `end_ctx` is 0 (old binary) | Treated as 0, so under the cap |
| Fix round after a chain | Unchanged; it resumes the last task's session |
| Task N parked, then `rework` | `builder_last` was cleared at dispatch, so fresh |
| `--resume` of a missing session | `claude` exits 1 with `No conversation found with session ID` and an `error_during_execution` result; the runner maps it to `Failed`, so the fallback runs |
| Helper-agent lines in the stream | Ignored for `end_ctx` (`parent_tool_use_id` set) |
| The branch tip moved since the session last saw it | Not checked; the prompt names the current tip and base |

## Testing

- `factory-engine`, new file `tests/task_carry.rs` (a runner wrapper that gives each call a distinct session id, since `ScriptedRunner` always returns `sess-1`):
  - `the_setting_off_starts_every_task_fresh`
  - `task_two_continues_the_session_when_gap_and_context_allow`
  - `a_stale_gap_starts_fresh_and_names_the_reason`
  - `an_oversized_context_starts_fresh`
  - `a_rework_note_starts_fresh`
  - `task_one_and_the_whole_plan_builder_are_never_carried`
  - `a_crash_session_beats_a_carry`
  - `a_failed_carry_gets_one_fresh_builder`
  - `the_carried_spec_has_permissions_agent_tools_and_turns`
  - `the_carried_prompt_names_the_new_task_and_overrides_later_tasks`
  - `a_tampering_carried_builder_parks_without_a_second_run`
  - `a_fix_round_after_a_chain_resumes_the_last_session`
  - `builder_last_is_set_on_advance_and_not_on_a_park`
  - `builder_last_is_cleared_when_a_builder_is_dispatched`
  - gap tests backdate `ended_at` on the board; there is no clock port
- Unit tests: `carry_decision` rules 1 to 8, `carry_idle_limit`, config parse and defaults for the two settings.
- `factory-core`: `builder_last_round_trips_and_is_omitted_when_none`, `factory cancel` clears it (`factory-cli/tests`).
- `factory-agent`: `end_ctx_ignores_helper_agent_messages` (an assistant line with a non-null `parent_tool_use_id`), `end_ctx_uses_the_merged_usage_of_the_last_message`, `a_missing_session_resume_is_classified_as_failed` (a result line with `is_error` true and `error_during_execution`, exit code 1).
- `factory-agent` and `factory-engine`: `end_ctx_is_the_last_messages_input_creation_and_read`, `usage_line_carries_end_ctx_and_usage_v_4` (existing `usage_v` assertions move from 3 to 4).
- `factory-cli`: `cold_section_shows_the_average_end_context`, and `old_lines_have_no_end_ctx`.
- Commands: `cargo test -j4 -p factory-engine --test task_carry`, `-p factory-agent`, `-p factory-core`, `-p factory-cli`, `cargo clippy -j4 --all-targets -- -D warnings`, `cargo fmt --check`.

## Files changed

- `crates/factory-agent/src/stream.rs`, `run.rs`, `claude.rs`
- `crates/factory-engine/src/build.rs`, `specs.rs`, `ports.rs`, `tests/task_carry.rs`, `tests/runloop.rs`, `tests/common/mod.rs`
- `crates/factory-core/src/board.rs`, `config.rs`, `lib.rs`
- `crates/factory-cli/src/commands.rs` (cancel), `tokens.rs`, `tests/tokens_cache.rs`
- `README.md`, `docs/project-toml-reference.md`, `docs/software-factory.md`

## Constraints

- `builder_last`, `end_ctx` and the settings are optional or defaulted. Board JSON must stay compatible with `factory-server`.
- Always `-j4`; never `--workspace`, `--all` or `CARGO_TARGET_DIR`. No `unwrap` in production code. No git shell-outs.
- A carried run goes through the same builder permissions, tamper guard and agent-file restore as a fresh one.
- The builder agent files and `RULES_VERSION` do not change.
- With the setting off, behaviour and the ledger text of every existing job are unchanged.

## Acceptance criteria

- With `builder_carry_session = true`, task 2 and later tasks of a plan resume the previous task's session when the previous builder ended under the idle limit ago and its `end_ctx` is at most `builder_carry_max_ctx`.
- Otherwise the task starts a fresh builder and, with the setting on, the ledger line says why.
- Task 1, the whole-plan builder, a task with a rework note, and a task with a saved crash session are never carried.
- A carried run keeps the builder permissions, the agent, the tools and the turn limit, and a failed carry gets one fresh builder.
- The idle limit follows `cache_ttl` (240 seconds for `5m` or unset, 3300 for `1h`).
- `usage.jsonl` lines carry `end_ctx` with `usage_v` 4, and `factory tokens --cache` shows AVG-END.
- Fix rounds, crash resume and the ledger text with the setting off are unchanged.
- The tests named in Testing pass, and clippy and fmt are clean.

## Out of scope

- The reviewer strip, the sync-message template, and any change to fix rounds.

## Follow-ups

- Guard the fix round (a fresh fix builder when the chain is large or the cache has expired). Revisit when fix rounds become common.
- Run the factory with `builder_carry_session = true` on this repo for a few jobs, then compare job cost and the COLD STARTS and AVG-END numbers before making it a default for new projects.
- Strip the reviewer's ambient context (about 1.8k tokens a run).
- Replace the sync commit message agent with a fixed template.

## Decisions

- What goes in this job? Options: builder resume only; builder resume plus the reviewer strip; builder resume plus a sync-message template. Chosen: builder resume only, the reviewer strip stays out. Decided by: user
- Should the resume be on or off by default? Options: off by default; on for new projects only; on for every project. Chosen: off by default, turned on per project in project.toml. Decided by: user
- Where does the idle-gap limit come from? Options: derive it from cache_ttl; a separate setting with default 240 seconds; derived with an optional override. Chosen: derive it from cache_ttl (5m gives 240 seconds, 1h gives 3300 seconds, unset counts as 5m), no separate setting. Decided by: user
- How does the driver know a task's context is too big to carry? Options: log the exact end-of-run context with a cap setting; estimate it from start_cr plus cache_creation; no cap. Chosen: log the exact end context (`end_ctx`, usage_v 4) and a setting `builder_carry_max_ctx` defaulting to 100000 tokens. Decided by: user
- What does a fix round do when the builder session has become a multi-task chain? Options: leave fix rounds unchanged and document the cost; guard them and start a fresh fix builder when over the limits; never carry into the last task. Chosen: leave fix rounds unchanged. Decided by: user
- How do we tell a continuing builder it now has the next task? Options: override in the carried prompt only; add a third resume case to the agent files; both. Chosen: override in the carried prompt only, the agent files are not touched. Decided by: user
- What do we do about the per-task checks that run between two tasks? Options: leave the order and carry only when the measured gap is under the limit, with a ledger line per task; run the checks before the snapshot commit; skip the carry when per-task checks exist. Chosen: leave the order and write the outcome to the ledger line of each task. Decided by: user
- The setting names are `builder_carry_session` and `builder_carry_max_ctx`, and the board field is `builder_last`. Decided by: call
- With the setting off the ledger text stays `fresh`, so existing tests and ledgers do not change. Decided by: call
- A task with a rework note, task 1 and the whole-plan builder never carry. Decided by: call
- The AVG-END column in the COLD STARTS table is added so the cap can be tuned from real numbers. Decided by: call
- `end_ctx` comes from the last main-thread assistant message, using the merged per-message usage. Decided by: call
- `builder_last` is cleared when a builder is dispatched, not only on advance. Decided by: call
- The branch tip is not compared before a carry; the prompt gives the new tip and does not say the worktree is clean. Decided by: call
