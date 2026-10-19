# Structured logging

## Goal

Give the driver and factoryd one structured JSON log, so every stage change, agent run, check, merge decision, ticket step and error can be tracked and read back.

## Overview

The driver's own activity has no structured record. `events.jsonl` holds only agent events (start, tool use, stop, budget warning, driver notice), written by `EventSink` (`crates/factory-agent/src/events.rs:47`). The ledger and the board record outcomes, not a timeline. Everything else is a plain `eprintln!` line: about 30 sites outside tests, for example `merge.rs:54-73`, `runloop.rs:166-169`, `tickets.rs:248`, `supervisor.rs:568-578` and `factoryd.rs:68-71`. Those lines have no timestamp, no job id and no level, and they vanish with the terminal. No crate depends on `tracing` or `log`.

The user wants all activity trackable. The tool is the `tracing` crate with `tracing-subscriber`.

## Current state

| Area | Today | Reference |
|---|---|---|
| Agent events | JSONL, one `write_all` per line, never fails the caller | `crates/factory-agent/src/events.rs:47-100` |
| Events path | `Layout::events_file()` is `<state_dir>/events.jsonl` | `crates/factory-core/src/layout.rs:62` |
| Stage changes | One choke point per job: `Engine::set_stage` | `crates/factory-engine/src/ports.rs:76` |
| Agent runs | `Engine::run_agent`, then `usage.jsonl` per job | `crates/factory-engine/src/ports.rs:101-135` |
| Merge side steps | Failures go to `eprintln!`, ledger and events | `crates/factory-engine/src/merge.rs:50-75` |
| factoryd | Prints new tick lines with `eprintln!` | `crates/factory-supervisor/src/supervisor.rs:560-585`, `src/bin/factoryd.rs:65-72` |
| CLI | Errors with `eprintln!("factory: {e}")`, then exit 1 or 2 | `crates/factory-cli/src/main.rs:13-62` |
| Settings | `Config` fields, validated in `Config::parse`; `cache_ttl` is the newest | `crates/factory-core/src/config.rs:100,177` |
| Home | `Home` has `logs_dir(project)`, `run_dir()`, `token_file()` | `crates/factory-core/src/home.rs:60-72` |
| Stages | `Spec, Build, Review, Approve, Merged, NeedsHuman` | `crates/factory-core/src/stage.rs:8` |

Gaps: no timestamp, level, job id or stage on a driver message; no log that outlives the terminal; no way to see what the driver did between two ledger lines; no command to read it.

## Non-goals

- Replacing `events.jsonl`. Agent events stay there, and the dashboard keeps reading them.
- A dashboard view and a factoryd `GET /logs` route (see Follow-ups).
- Logging agent prompts, agent output, command output, environment variables or request headers.
- Changing the board JSON, the ledger format or `usage.jsonl`.
- Remote shipping, OpenTelemetry, metrics.

## Design

Flow:

```
factory / factoryd start
  -> read log_level (project.toml) , FACTORY_LOG overrides
  -> telemetry::init(file, level) installs one tracing subscriber
       file layer   : JSON lines, level from the filter, redacting
       stderr layer : short human line, warn and above
engine / supervisor code
  -> tracing::info!(event = "stage.change", job_id, stage, ...)
  -> file layer -> RotatingFile::write_line -> <state_dir>/log.jsonl
factory log [--job] [--level] [--tail]
  -> LogReader reads log.jsonl.3 .. log.jsonl, filters, prints
```

### 1. The `log_level` setting

`Config` gets `log_level: Option<String>`. Allowed values are `error`, `warn`, `info`, `debug`, `trace`. Unset means `info`. `Config::parse` rejects any other value with `log_level must be one of error, warn, info, debug, trace when it is set`, in the style of the `cache_ttl` check (`config.rs:177`). A broken config uses `fail_closed()` as today, and the log then runs at `info`. `factory project add` does not write the key. `docs/project-toml-reference.md` gets a `log_level` row.

`FACTORY_LOG` holds an `EnvFilter` string such as `debug` or `factory_engine=debug,info`. When it is set and valid, it replaces `log_level`. When it is set and invalid, the driver prints `note: FACTORY_LOG ignored: <why>` to stderr and uses `log_level`.

### 2. The `factory-telemetry` crate

A new workspace member `crates/factory-telemetry`, depending on `tracing`, `tracing-subscriber` (features `json`, `env-filter`), `serde_json`, `fs2`. It holds:

- `pub fn init(target: LogTarget, level: Level, env: Option<&str>) -> Result<Guard, TelemetryError>`: builds the subscriber and installs it as the global default. A second call returns `Err(AlreadyInstalled)` and changes nothing. The `Guard` flushes on drop.
- `pub struct LogTarget { path: PathBuf, project: Option<String>, scope: Scope }` with `Scope::{Cli, Factoryd}`.
- `RotatingFile` (section 3) and `Redactor` (section 4).
- `pub fn read_log(path: &Path, filter: &LogFilter) -> Result<Vec<String>, TelemetryError>` for `factory log`.

The crate has no dependency on the other factory crates, so any of them can use it.

### 3. Log files, rotation and locking

Paths come only through `Layout` and `Home`:

- `Layout::log_file()` is `<state_dir>/log.jsonl` (next to `events.jsonl`).
- `Home::factoryd_log_file()` is `<home>/factoryd.log.jsonl`.

`RotatingFile::write_line` opens the file in append mode, writes one line with a single `write_all` and closes it, with no lock, so a writer never keeps a handle to a file another process renamed. There is no buffer, so the `Guard` only keeps the subscriber alive and flushes nothing. After the write it compares the file size to the cap (10 MiB). Over the cap, it takes an exclusive `fs2` lock on `<file>.lock`, checks the size again, and if it is still over, renames `.2` to `.3`, `.1` to `.2` and the file to `.1`, then releases the lock. The oldest `.3` is replaced. A failed write or rename is counted and never returned to the caller, the same rule as `EventSink::emit`. The lock is held only for the renames.

### 4. Fields and redaction

Every line is one JSON object with these keys:

| Key | Type | Note |
|---|---|---|
| `ts` | string | UTC, RFC 3339 with milliseconds |
| `level` | string | `error`, `warn`, `info`, `debug`, `trace` |
| `scope` | string | `cli` or `factoryd` |
| `project` | string | omitted when the checkout is not registered |
| `job_id` | string | omitted outside a job |
| `stage` | string | the job's stage, omitted outside a job |
| `event` | string | a dotted name, see below |
| `msg` | string | at most 300 characters (the `MAX_MESSAGE` cap in `events.rs`) |
| other | string/number/bool | event fields, see below |

Events the code emits (each is one `tracing` call at the named level):

| Event | Level | Where | Fields |
|---|---|---|---|
| `job.created` | info | `runloop.rs` `add_job` | `slug`, `series` |
| `stage.change` | info | `ports.rs` `set_stage`, plus the direct `Job::set_stage` call sites | `from`, `to`, `reason` |
| `agent.start`, `agent.end` | info | `ports.rs` `run_agent` | `role`, `model`, `stop_reason`, `weighted_tokens`, `duration_ms` |
| `check.run` | info | `checks_stage.rs` | `name`, `argv`, `exit_code`, `duration_ms` |
| `git.op` | debug | the engine, around its git port calls (`factory-git` gets no dependency) | `op`, `ok`, `duration_ms` |
| `decide` | info | `decide.rs` | `choice` (`merge`, `fix`, `needs-human`), `round` |
| `merge.done` | info | `merge.rs` | `sha` |
| `park` | warn | `merge.rs`, `ports.rs` | `reason` (one line, capped) |
| `step.error` | error | the 30 `eprintln!` sites | `step`, `msg` |
| `sup.start`, `sup.finish`, `sup.reject` | info | `supervisor.rs` | `request`, `project`, `job` |
| `sup.error` | error | `supervisor.rs`, `factoryd.rs` | `msg` |

A field whose name contains `token`, `key`, `secret` or `password` (case-insensitive substring) is written as `"[redacted]"`. Environment variables, prompts, command output and headers have no event and are never passed as fields. `argv` of a check is logged in full (it comes from the committed config), but the value of any `--name=value` argument whose `name` contains a sensitive word is written as `--name=[redacted]`. A check's environment is not logged. The one exception is the field `weighted_tokens`, a count that the redactor lets through by exact name. Example:

```json
{"ts":"2026-10-19T09:12:03.412Z","level":"info","scope":"cli","project":"free","job_id":"003","stage":"review","event":"stage.change","from":"build","to":"review","msg":"stage build -> review"}
```

### 5. Spans for context

`factory run` opens a `job` span with `job_id` and `project`; the stage loop opens a `stage` span with the job's stage at the start of each tick. The JSON layer copies span fields onto each line, so call sites do not pass `job_id` by hand. `stage.change` passes `stage` explicitly as the new stage, which wins over the span value. Code paths outside the run loop (`approve`, `rework`, `cancel`, `new`) open the `job` span only, and their lines carry the stage they pass. Tests that need no subscriber see no output, because `tracing` macros do nothing without one.

### 6. The stderr layer

stderr keeps human output at `warn` and above, plus the factoryd tick lines (`sup.start`, `sup.finish`, `sup.reject`) at `info`, so a user watching factoryd sees what they see today. The `new_lines` de-duplication (`supervisor.rs:560-585`) runs before the log call, so a repeated line is neither printed nor logged twice. Each site keeps its level: a failure is `error`, a progress line is `info`. The form is `<level> <event>: <msg>` (for example `error step.error: kb refresh error for job 003: ...`). The 30 `eprintln!` sites in the driver and factoryd become `tracing` calls, so each failure is both on stderr and in the file. The CLI's own `eprintln!("factory: {e}")` exit messages and the `note:` lines stay as plain `eprintln!`, because they are the command's result. `factory-shots` and `fake-claude` binaries are not instrumented.

### 7. `factory log`

`factory log [--job <id>] [--level <lvl>] [--tail <n>]` prints the matching lines of `Layout::log_file()` and its rotated files, oldest first. `--job` keeps lines whose `job_id` equals the id. `--level` keeps that level and above. `--tail` keeps the last n matching lines (default 100, 0 means all). With `--project <name>` it reads that project's file. A line that is not valid JSON is skipped. A missing file prints `no log yet` and exits 0.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| Log file cannot be created or written | The failure is counted, the run continues, and one `note: log disabled: <why>` goes to stderr at startup |
| `init` called twice (tests, nested calls) | Second call returns `AlreadyInstalled`, the first subscriber stays |
| `log_level` invalid in `project.toml` | `Config::parse` fails as for other bad keys, so the config falls back to `fail_closed()` |
| `FACTORY_LOG` invalid | `note:` on stderr, falls back to `log_level` |
| Two processes pass the cap together | The lock plus the second size check give one rotation, no lost or cut line |
| Rotation rename fails | Counted and ignored, the file keeps growing until the next try |
| Message longer than 300 characters | Cut to the last 300 characters, as `EventSink` does |
| Field named `api_token` or `Password` | Written as `[redacted]` |
| Argv entry `--token=abc` | Logged as `--token=[redacted]`; a secret in a bare positional argument is not caught |
| Value is a secret under a harmless field name | Not caught, which is why no event passes environment or output |
| `factory log --level warn` | Compares the `level` field of each line, not the filter string |
| `factory log` with a corrupt line | Skipped, the other lines print |
| `factory log --job` for an unknown id | Prints nothing and exits 0 |
| Commands that run before a layout exists (`project`, `presets`, `install`) | No file log, plain stderr as today |
| Dashboard reads the state dir | `log.jsonl` is a new file beside `board.json` and `events.jsonl`; the dashboard ignores unknown files |

## Testing

- `factory-telemetry` unit tests: `line_has_ts_level_scope_event` , `span_fields_reach_the_line`, `message_is_cut_to_300_chars`, `sensitive_field_names_are_redacted`, `weighted_tokens_is_the_one_field_not_redacted`, `planted_token_never_reaches_the_file`, `argv_value_of_a_sensitive_flag_is_redacted`, `rotation_moves_files_to_1_2_3`, `oldest_file_is_dropped`, `concurrent_writers_lose_no_line` (threads past the cap), `init_twice_returns_already_installed`, `write_error_is_counted_not_returned`, `env_filter_beats_log_level`, `invalid_env_filter_is_ignored`, `read_log_filters_by_job_and_level`, `read_log_tail_and_missing_file`, `read_log_skips_corrupt_line`.
- `factory-core`: `log_level_unset_by_default_and_takes_only_known_values` in `config.rs`; `log_file_is_in_the_state_dir` in `layout.rs`; `factoryd_log_file_is_in_the_home` in `home.rs`.
- `factory-engine` (`tests/runloop.rs`, `tests/merge.rs`): a capturing layer under `tracing::subscriber::with_default` asserts `stage_change_is_logged_with_from_and_to`, `agent_run_logs_start_and_end`, `merge_logs_sha`, `park_logs_a_warning`, `kb_failure_logs_step_error_and_merge_result_is_unchanged`.
- `factory-supervisor`: `supervisor_logs_start_and_finish` in `tests/recovery.rs` style.
- `factory-cli`: `tests/log_cmd.rs` with `log_prints_a_jobs_lines`, `log_level_filter`, `no_log_yet_exits_zero`; `tests/no_hardcoded_paths.rs` must still pass; `tests/e2e.rs` asserts a run writes `log.jsonl` with a `merge.done` line and no secret.
- Commands: `cargo test -j4 -p factory-telemetry`, `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-supervisor`, `cargo test -j4 -p factory-cli`.

## Files changed

- `Cargo.toml` (member, workspace dependencies), `Cargo.lock`
- `crates/factory-telemetry/**` (new)
- `crates/factory-core/src/config.rs`, `layout.rs`, `home.rs`
- `crates/factory-engine/src/ports.rs`, `runloop.rs`, `merge.rs`, `tickets.rs`, `checks_stage.rs`, `decide.rs`, `build.rs`, `review.rs`, `sync.rs`
- `crates/factory-supervisor/src/supervisor.rs`, `src/bin/factoryd.rs`
- `crates/factory-cli/src/cli.rs`, `commands.rs`, `main.rs`, new `log.rs`
- `docs/project-toml-reference.md`, `docs/software-factory.md`, `README.md`

## Constraints

- Always `-j4`, never `--workspace` or `--all`, never `CARGO_TARGET_DIR`.
- Paths reach the driver only through `Layout` and `Home`; no literal path elsewhere.
- Logging never changes a result: a log failure must not fail a stage, a merge or a command.
- Board JSON stays unchanged. No new required field.
- A missing or broken config stays fail-closed.
- The API token, `ANTHROPIC_API_KEY` and `ANTHROPIC_AUTH_TOKEN` never appear in the log.
- The lease heartbeat and `/proc` checks stay Linux only; the log works on every OS.
- Only `git` may not be shelled out to.

## Acceptance criteria

- A job run writes `<state_dir>/log.jsonl` with one JSON line per event in the table, each with `ts`, `level`, `scope`, `event` and, inside a job, `job_id` and `stage`.
- Every stage change, agent run, check, decide, merge and park of a job appears in the log.
- factoryd writes `<home>/factoryd.log.jsonl` with start, finish, reject and error events.
- The former `eprintln!` failure sites log an `error` line and still print a human line to stderr at `warn` and above.
- `log_level` in `project.toml` sets the file level, defaults to `info`, and an unknown value is a config error.
- `FACTORY_LOG` overrides `log_level`; an invalid value is ignored with a note.
- The file rotates at 10 MiB into `.1` to `.3`, and concurrent writers lose no line.
- Fields named `token`, `key`, `secret` or `password` are redacted, check argv values of such flags are redacted, and a planted token never reaches the file.
- A log write failure never fails a stage, a merge or a command.
- `factory log` filters by `--job`, `--level` and `--tail` across the rotated files, and a missing file prints `no log yet`.
- `events.jsonl`, the board JSON and the ledger are unchanged, and the dashboard still works.
- The docs describe the setting, the files and the command.

## Out of scope

- Any change to the dashboard workspace.
- Log shipping, metrics, tracing export.
- Logging inside the `claude` child process.

## Follow-ups

- Dashboard view of `log.jsonl` per job.
- A `GET /logs` route in factoryd for all projects.

## Decisions

- Which activity does the log cover? Options: the whole driver plus factoryd; job lifecycle only; lifecycle plus agent events replacing `events.jsonl`. Chosen: the whole driver plus factoryd. Decided by: user
- How is the log produced? Options: in-house JSONL logger behind a port; the `tracing` crate with a JSON layer; the `log` facade with a JSON backend. Chosen: the `tracing` crate with `tracing-subscriber`. Decided by: user
- Where is the log written and how is it bounded? Options: one file per scope rotated by size; daily files; no rotation. Chosen: one file per scope, rotated at 10 MiB keeping `.1` to `.3`. Decided by: user
- How is the level controlled? Options: `FACTORY_LOG` only; a `log_level` key only; both. Chosen: both, with `FACTORY_LOG` overriding the `log_level` key in `project.toml`. Decided by: user
- How are secrets and bulk output kept out? Options: a fixed field set with name-based redaction; log anything then scrub; names and ids only. Chosen: a fixed field set with name-based redaction. Decided by: user
- How is the log read? Options: a `factory log` command; file only; command plus dashboard and a factoryd route. Chosen: the `factory log` command, with the dashboard and the route as follow-ups. Decided by: user
- How do several processes rotate one file? Options: rotate under an `fs2` lock; only long-lived owners rotate; no lock. Chosen: rotate under an `fs2` lock with a second size check. Decided by: user
- Does `check.run` log the full argv? Options: full argv with sensitive `--name=value` values redacted; program name and argument count only; full argv unchanged. Chosen: full argv with sensitive values redacted. Decided by: user
- Does the engine need a port for logging, given `ports.rs`? Options: call `tracing` macros directly; wrap them in a port. Chosen: call the macros directly, because `tracing` is a no-op without a subscriber and tests capture with `with_default`. Decided by: call
- Where does the code live? Options: a new `factory-telemetry` crate; inside `factory-core`. Chosen: a new crate, because `factory-core` has no I/O dependencies. Decided by: call
- Which process writes which file? Options: each process writes its own scope's file; factoryd also writes into project files. Chosen: the child `factory` writes the project `log.jsonl`, factoryd writes `factoryd.log.jsonl`, and `init` runs after the config load, so a config-load error goes to plain stderr. Decided by: call
- Which timestamp format? Options: RFC 3339 UTC string; epoch seconds like `events.jsonl`. Chosen: RFC 3339 UTC string, for readable lines. Decided by: call
