# A retry through factoryd names the park's next commands

**Commit:** fix(supervisor): name park commands on retry

## Goal

A retry refused through factoryd or the HTTP API, because the park has no resume stage, names the park's next commands with the same text as `factory retry`.

## Overview

Issue #119. For a park with no resume stage, `factory retry` (`crates/factory-engine/src/retry.rs`) refuses with `job 001 has no stage to retry at; use factory rework 001 <note>, factory cancel 001`. The factoryd path, `factoryd enqueue --retry` and `POST /projects/{p}/jobs/{id}/retry`, both go through `admit::queue_retry`, which says only `job 001 has no resume stage, see its park for the next command`.

Since commit db43bec, `/factory retry` goes through factoryd whenever one runs, so users of `/factory retry` and `factoryd enqueue --retry` get the less useful text and have to look up the park themselves. The dashboard offers Retry only when `park_resume` is set (`dashboard/static/board.js:62`), so it meets this refusal only on a stale board.

## Current state

| Path | What it does today |
|---|---|
| `crates/factory-engine/src/retry.rs:8-21` | `park_commands(driver, job)`: the `next` list of `park.json` (`ParkRecord::read(&driver.job_dir(job))`) when it is not empty, else `ParkKind::next(&job.id)` of the board's `park_kind`, with an unknown or missing kind read as `ParkKind::Other`. Joined with `", "`. |
| `crates/factory-engine/src/retry.rs:48-53` | No `park_resume`: `Refused("job {id} has no stage to retry at; use {commands}")`. |
| `crates/factory-supervisor/src/admit.rs:193-197` | `queue_retry`, no `park_resume`: `Conflict("job {id} has no resume stage, see its park for the next command")`. |
| `crates/factory-supervisor/src/api/write.rs` `retry_inner` | Calls `admit::queue_retry` and maps a `Conflict` to `409 conflict`. |
| `crates/factory-core/src/park.rs:191-217` | `ParkKind::next(id)` per kind. `Other` gives approve, rework and cancel. |
| `crates/factory-core/src/park.rs:116-138` | `ParkKind::from_name(name) -> Option<ParkKind>` reads a kebab kind name. |
| `crates/factory-core/src/park.rs:250-304` | `ParkRecord` with `next: Vec<String>`; `ParkRecord::read(dir)` gives `None` for a missing or unreadable `park.json`. |
| `crates/factory-core/src/layout.rs:192` | `Layout::job_dir(dir)`; the engine's `Driver::job_dir(job)` is `layout.job_dir(&job.dir)` (`factory-engine/src/compat.rs:602`). |
| Pinned text | `admit.rs` test `queue_retry_refuses_a_park_without_resume`, `api/write.rs` test `retry_without_resume_is_409_and_queues_nothing`, README.md:549. |
| Engine tests | `factory-engine/tests/retry.rs:283` and `factory-cli/tests/retry_cmd.rs:70-87` check `no stage to retry at` and the commands. |

Gap: the supervisor has the job and the project layout in hand but does not read the park, and the logic to pick the commands lives only in the engine.

## Non-goals

- No change to which commands a park offers (`ParkKind::next`, `park.json` content).
- No dashboard change. It already shows the 409 message.
- No change to the other retry refusals (cancelled, stage, batch commit, approval), whose text already matches the engine.

## Design

```
queue_retry / retry_inner, job without park_resume
  -> factory_core::park::no_resume_refusal(&layout.job_dir(&job.dir), &job.id, job.park_kind.as_deref())
  -> Conflict("job 001 has no stage to retry at; use factory rework 001 <note>, factory cancel 001")
factory retry (engine), same case
  -> the same function with driver.job_dir(&job)
```

### 1. Shared functions in `factory-core/src/park.rs`

- `pub fn park_commands(dir: &Path, id: &str, park_kind: Option<&str>) -> Vec<String>`: the body of today's engine `park_commands`, without the join. The `next` list of `ParkRecord::read(dir)` when the file reads and the list is not empty; else `ParkKind::next(id)` of `park_kind.and_then(ParkKind::from_name)`, with a missing or unknown kind read as `ParkKind::Other`.
- `pub fn no_resume_refusal(dir: &Path, id: &str, park_kind: Option<&str>) -> String`: `format!("job {id} has no stage to retry at; use {}", park_commands(dir, id, park_kind).join(", "))`.

Both are exported from the crate root next to `ParkRecord`.

Example: a job `001` with `park_kind` `"nothing-changed"` and no `park.json` gives `job 001 has no stage to retry at; use factory rework 001 <note>, factory cancel 001`.

### 2. The engine uses them

`retry.rs` drops its private `park_commands` and returns `Refused(no_resume_refusal(&driver.job_dir(&job), &job.id, job.park_kind.as_deref()))`. The text does not change.

### 3. The supervisor uses them

`queue_retry` returns `Conflict(no_resume_refusal(&layout.job_dir(&job.dir), &id, job.park_kind.as_deref()))`. `factoryd enqueue --retry` prints it and exits 1, and the HTTP retry route answers `409 conflict` with it. No other check moves.

### 4. Docs

README.md:549 ends its retry row's 409 list with (for example `job 020 has no stage to retry at; use factory rework 020 <note>, factory cancel 020`). `docs/guides/factory-user-guide.md:1156`, `docs/reference/software-factory.md:346` and `docs/reference/factory-admin-manual.md:193` add that the refusal names the park's next commands.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| `park.json` with a non-empty `next` | Its commands, in its order. |
| `park.json` with an empty `next` | The kind's commands. |
| No `park.json`, or it does not parse | The board kind's commands. |
| No `park_kind` on the board, or an unknown one | `ParkKind::Other`: approve, rework, cancel. |
| Job dir missing | Same as no `park.json`. |
| A job with `park_resume` | Not affected; the retry is queued or refused by the later checks as today. |

## Testing

`crates/factory-core/src/park.rs` tests (temp dir):

- `park_commands_prefer_the_park_file`: a `ParkRecord` with `next` `["factory rework 001 <note>"]` written to the dir and kind `other` gives that list.
- `park_commands_fall_back_to_the_kind`: no file, a file with an empty `next`, and a `park.json` that is not JSON, each with kind `nothing-changed`, give `ParkKind::NothingChanged.next("001")`.
- `park_commands_read_an_unknown_kind_as_other`: no file and kind `None` or `"nonsense"` gives `ParkKind::Other.next("001")`.
- `no_resume_refusal_joins_the_commands`: the text for `nothing-changed` with no file equals `job 001 has no stage to retry at; use factory rework 001 <note>, factory cancel 001`.

`crates/factory-supervisor/src/admit.rs` tests (`fixture`, `parked`):

- `queue_retry_refuses_a_park_without_resume`: updated; kind `other`, no park file, gives `Conflict(no_resume_refusal(...))`, which equals `job 001 has no stage to retry at; use factory approve 001, factory rework 001 <note>, factory cancel 001`; queue empty.
- `queue_retry_names_the_commands_of_the_park_file`: a park file in the job dir with `next` `["factory rework 001 <note>"]` gives `job 001 has no stage to retry at; use factory rework 001 <note>`.

`crates/factory-supervisor/src/api/write.rs`: `retry_without_resume_is_409_and_queues_nothing` expects the new text.

Engine and CLI: the existing tests in `factory-engine/tests/retry.rs` and `factory-cli/tests/retry_cmd.rs` pass unchanged.

Commands: `cargo test -j4 -p factory-core park`, `cargo test -j4 -p factory-engine --test retry`, `cargo test -j4 -p factory-cli --test retry_cmd`, `cargo test -j4 -p factory-supervisor`, `cargo clippy -j4 -p factory-core -p factory-engine -p factory-supervisor --all-targets -- -D warnings`.

## Files changed

- `crates/factory-core/src/park.rs`, `crates/factory-core/src/lib.rs`: the two functions, the export, tests.
- `crates/factory-engine/src/retry.rs`: uses `no_resume_refusal`.
- `crates/factory-supervisor/src/admit.rs`, `crates/factory-supervisor/src/api/write.rs`: the refusal and tests.
- `README.md:549`, `docs/guides/factory-user-guide.md:1156`, `docs/reference/software-factory.md:346`, `docs/reference/factory-admin-manual.md:193`: the new text.

## Constraints

- Cargo with `-j4`, never `--workspace` or `--all`.
- Tests use `.expect("...")` and `expect_err`.
- `factory-core` keeps no git or process dependencies; reading `park.json` is already there.

## Acceptance criteria

- `factoryd enqueue --retry` and `POST .../retry` refuse a park without a resume stage with `job <id> has no stage to retry at; use <commands>`.
- The commands come from `park.json` when it lists any, else from the board's park kind, else from `Other`.
- `factory retry` prints the same text as before, from the same shared function.
- README.md, the user guide and the software-factory reference describe the new text.
- The listed tests and clippy pass.

## Follow-ups

- When `park.json` is missing, the fallback uses the plain `ParkKind::next`, not the review-aware `next_for`, so an `Other` park whose review has not run is offered `factory approve <id>`, which approve then refuses. Fixing it changes the engine's text too.
- `/factory retry` reports a factoryd refusal as it is (`crates/factory-core/assets/global-command.md:45`); it could map the named `factory ...` commands to their `/factory ...` forms. `agent_lint.rs` pins that file.

## Decisions

- What should the factoryd refusal say? Options: the same text as factory retry, the old start with commands added. Chosen: the same text as factory retry. Decided by: user
- Should the HTTP 409 text change too? Options: change both together, keep the HTTP text. Chosen: change both together. Decided by: user
- Where does the command lookup live? Options: shared in factory-core, a copy in the supervisor. Chosen: shared in factory-core. Decided by: user
- The function takes the job dir, id and park kind instead of a `Job`, so callers with a layout or a driver both pass what they have. Decided by: call
- The review-aware fallback and the `/factory` command mapping go to follow-up tickets, because the first changes the engine's text and the second edits a pinned skill file. Decided by: call
