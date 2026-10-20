# Crash resume

## Goal

Make a job whose driver died resume at once: a dead driver's job lease no longer blocks, the builder's session is saved while it runs, and a re-run resumes that session instead of starting a fresh builder.

## Overview

Driver `driver-1200221-...` started job 012 outside `factoryd`, logged the builder's `agent.start` at 04:00:51 UTC and died about a minute later. Every re-run then failed with `job is leased by driver-1200221-... for another N seconds`, because the job lease is judged by expiry time only (30 minutes). The project lease already treats a dead `driver-<pid>` owner as stale at once. The builder's session id was also lost: it reaches the board only after the run returns. A re-run would have started a fresh builder on a half-built worktree.

## Current state

| Area | Today | Where |
|---|---|---|
| Job lease liveness | `lease_is_live` is `expires > now`; `check_free` refuses when the lease is live and the owner differs; neither looks at the owner pid | `crates/factory-engine/src/lease.rs:15-27` |
| Project lease liveness | `Held` only when not expired and the owner is not dead; takeover when expired or `owner_is_dead` | `crates/factory-engine/src/project_lease.rs:61-76`, `:111`, `:189-197` |
| Lease lifetime | `LEASE_TTL_SECS = 1800.0`, heartbeat 60 s, both leases renewed together | `crates/factory-engine/src/compat.rs:146-149`, `:181-195` |
| Job lease readers | `cancel` (`lease_is_live`), `remove_cancelled_card`, `tickets`, `kb_refresh`, the run loop's `renew_lease` | `crates/factory-cli/src/commands.rs:600`, `:673`, `:767`, `:809`; `crates/factory-engine/src/runloop.rs:510` |
| Session id on the stream | The `system/init` line sets `StreamState.session_id` and yields `StreamEvent::Init`; the read loop drops `Init` | `crates/factory-agent/src/stream.rs:79-87`, `crates/factory-agent/src/claude.rs:128-140` |
| Session id on the board | `Job.builder_id` is written after `run_builder_guarded` returns, and for a task after its commit | `crates/factory-engine/src/build.rs:82-85`, `:581-589`, `crates/factory-core/src/board.rs:62-63` |
| Re-run of a `Dispatched` task | `build_task` starts a fresh builder: `builder_task_spec` never sets `resume`; only fix rounds do | `crates/factory-engine/src/build.rs:473-601`, `crates/factory-engine/src/specs.rs:197`, `:280` |
| Adopted child | `classify` with `code == None` and a last line `merged` returns `Merged` | `crates/factory-supervisor/src/supervisor.rs:188-224` |

Gaps: the two leases disagree about a dead owner; a mid-run session id is never stored; a crash always costs a fresh builder.

## Non-goals

- Ignoring SIGHUP in the driver, or refusing foreground runs (the README only gets a note).
- Resuming reviewer, committer or message runs, and the legacy whole-plan builder path (`build_stage`).
- Crashes in the middle of a fix round: no `task_session` is saved there, and a fix round behaves as today.
- Restarting jobs automatically: a re-run is still asked for by the user or by `factoryd enqueue`.
- Telling a reused pid from the dead driver: the same limit as the project lease, so the 30 minute expiry still applies then.

## Design

Flow of a crash and its recovery:

```
driver dies -> job lease still has time left -> re-run: owner pid is gone -> lease not live
  -> run loop resumes at the board stage -> build_task finds task_session -> claude --resume <id>
  -> resume fails -> fresh builder (as today)
```

### 1. Dead-owner job lease

- `owner_is_dead` in `project_lease.rs` becomes `pub fn lease_owner_dead(owner: &str) -> bool` and is re-exported by `factory-engine`. It keeps its rule: `owner_pid(owner)` is `Some` and `process_gone(pid)`. A pid equal to the current process is alive.
- `lease_is_live(job, now)` becomes `expires > now && !lease_owner_dead(&l.owner)`. `check_free` uses the same test, so an expired or dead-owner lease can be taken over by any other owner, as the project lease already can.
- Every reader of the job lease follows through this function, so `run`, `approve`, `cancel` without `--force`, `tickets`, `kb-refresh` and the API's live-lease check all see a dead owner as free. Owners that are not `driver-<pid>-<secs>` are judged by expiry only.

Example: a job with lease `driver-4294967000-1` expiring in 25 minutes: `acquire_lease` for `driver-1111-2` succeeds.

### 2. Session capture while the builder runs

- `factory-agent`: `pub struct SessionHook(pub Arc<dyn Fn(&str) + Send + Sync>)` with a manual `Debug`; `AgentSpec.on_session: Option<SessionHook>`, default `None`.
- `ClaudeRunner` calls `on_session(session_id)` once, when `StreamEvent::Init` arrives, and emits `EventKind::Session` through its sink (`hook_name` `FactorySession`, the id in the message text).
- `factory-core`: `Job.task_session: Option<TaskSession>` with `TaskSession { task: u32, id: String, tip: String }`, where `tip` is the branch tip when the task started, `#[serde(default, skip_serializing_if = "Option::is_none")]`. It is optional, so old boards load and the dashboard (`factory-server`) is unaffected.
- `build_task` sets `spec.on_session` to a hook that writes `task_session = Some(TaskSession { task: n, id, tip })` to the board. The hook builds its own `BoardStore` from the board path (`store.rs:23`, a path and a lock wait), so the file lock makes the write safe from the stream thread. A failing board write is logged and never stops the run.
- `task_session` is cleared as soon as the builder run returns, before any park check, because from then on `builder_id` and the existing fix-round path carry the session. It is also cleared when the task advances and by `cancel`. Only a crash leaves it set, so `rework` never sees it.

### 3. Resume after a crash

- At the start of `build_task` for task `n`: when `job.task_session` is `Some` with `task == n` and the branch tip equals the recorded tip (no commit), the spec gets `resume = Some(id)` and the prompt is the constant `INTERRUPTED_PROMPT`: `You were interrupted. Run git status and git diff, check what is already done, continue task N, and do not redo finished work.` (with the task number).
- The ledger `Dispatched` line reads `BASE <base>, <model>, resumed` for a resume and `..., fresh` for a fresh start.
- The resume counts as failed when the run returns `StopReason::Failed`. The driver then clears `task_session`, builds `builder_task_spec` as today, runs once more, and logs `fresh`. Any other stop reason is handled as for a normal run.
- The resume runs through `run_builder_guarded`, so the tamper guard still applies.
- A fallback fresh builder runs on the worktree as it is, with the interrupted builder's uncommitted edits left in place, exactly as a re-run does today. The driver never resets or cleans it.
- A resumed run that stalls, hits a budget or turn limit, or parks is not retried fresh.
- A missing transcript makes `claude --resume` exit without a result, which the runner reports as `StopReason::Failed` (`claude.rs`, exit without a result).

Example ledger: `Task 1: dispatched (BASE ec10825, sonnet, resumed)`.

### 4. Adopted child

`classify` keeps its rule. A comment states why: a series and `approve` print `merged` only at the very end. A unit test pins it.

### 5. README

The job-lease paragraph (after the project lease section) says: both leases are renewed every 60 seconds; a dead `driver-<pid>` owner counts as stale at once for the job lease too; a crashed build resumes its session; use `factoryd enqueue`, or `nohup` or `tmux`, for runs that must survive a dropped terminal.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| Owner pid is gone, lease not expired | Not live; the next driver takes it over |
| Owner string is not `driver-<pid>-<secs>` | Expiry only |
| Lease owner is this process | Alive, as today |
| Pid reused by another process | Treated as alive until expiry |
| Session id arrives, board write fails | Logged at warn; the run continues; a crash then costs a fresh builder |
| `task_session` is for another task | Ignored and cleared; fresh builder |
| Tip moved since the session started | Fresh builder; `task_session` cleared |
| Fresh fallback after a failed resume | Runs on the worktree as it is; edits are never reset |
| Builder run returns normally | `task_session` is cleared before park checks |
| `claude --resume` fails or the transcript is missing | One fresh builder run; ledger says `fresh` |
| Resumed run parks (blocked, budget, tamper) | Handled as a normal run |
| Old board without `task_session` | Loads; behaves as today |
| Adopted child, last line `merged` | `Merged` |

## Testing

- `factory-engine` `lock_lease.rs`: `a_dead_owner_lease_is_not_live` (owner `driver-4294967000-1`, live expiry), `a_live_owner_lease_stays_live`, `a_non_driver_owner_is_judged_by_expiry_only`, `a_dead_owner_lease_is_taken_over`; `project_lease.rs`: `the_pid_helpers_are_public` extended for `lease_owner_dead`.
- `factory-cli` `project_lease_cli.rs`: `run_resumes_a_job_whose_driver_died_with_a_live_lease`, `cancel_without_force_accepts_a_dead_owner_lease`, `tickets_accepts_a_dead_owner_lease`; the fixture is `hold_lease` in `common/mod.rs`.
- `factory-agent` `tests/runner.rs`: `the_session_hook_is_called_once_with_the_init_id` and `a_session_event_is_emitted` (fake-claude `ok`), `the_session_hook_fires_before_a_stalled_run_is_killed` (fake-claude `silent` with `stall_after`).
- `factory-core` `board.rs`: `a_board_without_task_session_loads` and `task_session_round_trips` (with `tip`).
- `factory-engine` `build_checks.rs` (template `two_task_fixture()`, `ScriptedRunner`): `build_task_saves_the_session_while_the_builder_runs`, `a_rerun_resumes_the_saved_session`, `a_resume_prompt_names_the_task`, `a_failed_resume_falls_back_to_a_fresh_builder`, `a_session_of_another_task_is_ignored`, `a_moved_tip_starts_fresh`, `the_session_is_cleared_when_the_run_returns_even_if_it_parks`, `a_fresh_fallback_keeps_the_partial_edits`, `a_stalled_resume_is_not_retried`, `the_hook_writes_the_board_through_its_own_store`, `the_session_is_cleared_when_the_task_advances`, `the_ledger_says_resumed_or_fresh`.
- `factory-supervisor` `supervisor.rs`: `an_adopted_child_with_a_merged_line_is_merged`.
- `factory-cli` `agent_lint.rs`: `readme_documents_the_job_lease_and_resume`.
- Commands: `cargo test -j4 -p factory-engine`, `-p factory-agent`, `-p factory-core`, `-p factory-cli`, `-p factory-supervisor`, then clippy with `-D warnings` and `cargo fmt --check`.

## Files changed

- `crates/factory-engine/src/lease.rs`, `project_lease.rs`, `lib.rs`, `build.rs`, `specs.rs`.
- `crates/factory-agent/src/spec.rs`, `claude.rs`, `events.rs`, `lib.rs`.
- `crates/factory-core/src/board.rs`.
- `crates/factory-supervisor/src/supervisor.rs`.
- `README.md`; tests as listed.

## Constraints

- Always `-j4`; never `--workspace` or `--all`; never set `CARGO_TARGET_DIR`.
- Board JSON stays compatible with `factory-server`: `task_session` is optional.
- Board, ledger and events are written only by the driver; the hook runs inside the driver process.
- Engine logic stays behind the ports in `ports.rs`; tests use fakes.
- Never read `/proc` directly; process checks go through `procinfo.rs`.

## Acceptance criteria

- A job lease whose owner is a dead `driver-<pid>` is not live, and `run` or `approve` takes it over on the first retry.
- `cancel` without `--force`, `tickets` and `kb-refresh` accept a dead-owner job lease.
- The builder's session id is on the board while the builder still runs, and an event is emitted for it.
- A re-run of a `Dispatched` task with a saved session and an unchanged tip resumes that session once, and falls back to a fresh builder when the resume fails.
- The ledger line says `resumed` or `fresh`.
- Old boards load, and `task_session` is absent when no session is saved.
- A unit test pins that an adopted child with a `merged` last line is `Merged`.
- The README explains the job lease, the resume and the advice for runs that must survive a dropped terminal.

## Out of scope

- SIGHUP handling, resume of non-builder roles, the legacy builder path, automatic restart of jobs.

## Decisions

- How does the driver learn the builder session id while the builder still runs? Options: a callback on the run request, an event on the event sink, a scan of the transcript folder. Chosen: the hybrid. An optional callback on the run request saves the id on the board the moment the stream reports it, and the runner also emits a `Session` event so the id shows in the event log. The board stays the only recovery state. Decided by: user
- What does a re-run do with a saved session id for a task that is `Dispatched` with no commit? Options: resume automatically with a fallback to a fresh builder, resume only on a clean worktree, save the id but never resume by itself. Chosen: resume automatically once with a short "you were interrupted" prompt, fall back to a fresh builder when the resume fails, and write `resumed` or `fresh` in the ledger line. Decided by: user
- Where does the dead-owner rule apply to the job lease? Options: everywhere the job lease is read, only on run and approve takeover, a new `--take-over` flag. Chosen: everywhere the job lease is read, with the same rule as the project lease; owners that are not `driver-<pid>-<secs>` are judged by expiry only. Decided by: user
- What does `factoryd` do with an adopted child that printed `merged`? Options: pin the behavior with a test and a comment, write a result marker file, treat it as `Lost`. Chosen: change no logic; add a unit test and a comment, because a series and `approve` print `merged` only at the very end. Decided by: user
- Does this work also protect a manually started driver from a dropped terminal? Options: document only, ignore the hangup signal, refuse foreground runs without factoryd. Chosen: document only, in the README job-lease paragraph; the dead-owner rule makes a dropped session cheap to recover from. Decided by: user
- Delivery? Chosen: one series of three plans: the dead-owner lease with the adopted-child test, session capture, resume with the README. Decided by: call
- Names? Chosen: `lease_owner_dead`, `SessionHook`, `EventKind::Session`, `Job.task_session` and `INTERRUPTED_PROMPT`. Decided by: call
- When does a resume count as failed? Chosen: when the run returns `StopReason::Failed`; any other stop reason is treated as a normal run. Decided by: call
- Where is the recorded tip stored? Chosen: in `TaskSession.tip`, written by the hook with the tip computed at the start of `build_task`. Decided by: call
- What happens to the worktree when a resume fails? Chosen: nothing; the fresh builder sees the partial edits, as a re-run does today. Decided by: call
- When is `task_session` cleared? Chosen: when the builder run returns, before the park checks, so `rework` and fix rounds keep using `builder_id`. Decided by: call
- Is a zombie owner dead? Chosen: the same rule as the project lease (`pid_exists`), with no new case. Decided by: call
