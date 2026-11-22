# factoryd enqueue pads the job id and shares the API's admission checks

**Commit:** fix(supervisor): pad job ids in factoryd enqueue

## Goal

`factoryd enqueue` stores every job id padded to three digits and refuses the same requests the HTTP API refuses, so duplicate approves, retries and runs are caught from the command line.

## Overview

Issue #120. `factoryd enqueue --job 1 --approve` stores job `1`. A later `factoryd enqueue --job 1 --retry` pads to `001` and checks the queue for an approve or retry of job `001`. The strings differ, so both requests are queued and the child runs twice.

The same branch has more gaps around it, all in the same code:

- A CLI approve never checks the queue, so two CLI approves queue twice.
- A CLI approve skips the board checks the HTTP approve route makes (stage, cancelled, review refusals). The child refuses later, and nobody reads that refusal.
- Two CLI runs of one job both queue.
- A CLI run or approve for an unknown project is queued and only rejected by the scheduler.
- `--approve --series` is not refused by clap.
- `refuse_if_queued` compares the stored id exactly, so an entry queued before this fix (or written by hand) with an unpadded id is missed.
- Job id `0` pads to `000`, the intake placeholder, and is accepted.
- `factoryd status` lists queued requests without their action.

## Current state

| Path | What it does today |
|---|---|
| `crates/factory-supervisor/src/bin/factoryd.rs:46-59` | `Enqueue` flags. Only `--retry` conflicts with `--approve` and `--series`. |
| `crates/factory-supervisor/src/bin/factoryd.rs:63-83` | `enqueue` sends `--retry` to `admit::queue_retry`. Otherwise it builds `Request::new(project, action, job, series, now)` with `job` as typed and calls `queue::enqueue`. |
| `crates/factory-supervisor/src/bin/factoryd.rs:199-209` | `status` prints each queued request as `  {id} {project} job {job}`. |
| `crates/factory-supervisor/src/request.rs:27-39` | `Action` (`Run`, `Approve`, `Retry`, `Intake`, serde snake_case) and `INTAKE_JOB = "000"`. |
| `crates/factory-supervisor/src/request.rs:91-110` | `Request::new` checks the job with `validate_job` (1 to 6 ASCII digits) and stores it unchanged. |
| `crates/factory-supervisor/src/admit.rs:40` | `project_layout(home, name)`: `NotFound("no project named {name}")` or `Disabled`. |
| `crates/factory-supervisor/src/admit.rs:55-58` | `job_id(raw)` checks and pads the id with `normalize_id`. A bad id is `BadRequest`. It accepts `0`. |
| `crates/factory-supervisor/src/admit.rs:61-73` | `refuse_if_queued(home, name, id)`: `Conflict("job {id} already has an approve or retry queued")` when an approve or retry with `r.job == id` is queued. |
| `crates/factory-supervisor/src/admit.rs:85-129` | `queue_retry` pads, checks the board, calls `refuse_if_queued` and queues. |
| `crates/factory-supervisor/src/api/write.rs:375-441` | `approve_inner`: project, id, body, waive reason one line, then the board checks (job found, cancelled, stage needs-human or approve, `Job::approve_refusal`), `refuse_if_queued`, queue with `without_review` and `waive_reason`. |
| `crates/factory-supervisor/src/api/write.rs:77-86` | `refusal` maps `Refusal` kinds to 400, 404, 409 and 500. |
| `crates/factory-supervisor/src/api/mod.rs:7` | `testutil` (`fixture`, `job_json`, `write_board`) is `pub(crate)`; the `factoryd` binary's tests cannot use it. |
| `crates/factory-cli/src/commands.rs:475` | The child pads ids again, so a padded id changes nothing downstream. |

## Non-goals

- No `--without-review` or `--waive` flags on `factoryd enqueue`. A CLI approve is a plain approve.
- No change to the HTTP create route's run, which queues a freshly created job.
- No change to `Request::new` or the queue file format.
- No job-0 rule in the `factory` CLI (`norm`, `crates/factory-cli/src/commands.rs:474`). `factory approve 0` looks up job `000`, which no board has, and already refuses.

## Design

```
factoryd enqueue --job <raw>
  --retry    -> admit::queue_retry                         (unchanged)
  --approve  -> admit::queue_approve(home, p, raw, false, None, now)
  else       -> admit::queue_run(home, p, raw, series, now)
POST /projects/{p}/jobs/{id}/approve
  -> body parse and waive check, then admit::queue_approve(home, p, id, without_review, waive, now)
```

### 1. `job_id` refuses job 0

`admit::job_id` returns `BadRequest("job 000 is not a job, ids start at 001")` when the padded id equals `INTAKE_JOB`. This covers `0`, `00` and `000` on every route that uses `job_id` (approve, retry, rework, cancel and the CLI). Intake builds its request with `INTAKE_JOB` directly and is not affected.

### 2. The duplicate checks compare padded ids

A private helper `fn is_queued(home: &Home, name: &str, id: &str, actions: &[Action]) -> bool` matches a queued `Ok` request when `r.project == name`, its action is in `actions`, and `normalize_id(&r.job)` is `Ok` and equals `id`. `refuse_if_queued` uses it with `[Approve, Retry]` and keeps its message. An old entry stored as `1` now blocks an approve or retry of `001`.

### 3. `admit::queue_run`

`pub fn queue_run(home: &Home, name: &str, raw_id: &str, series: bool, now_ms: u64) -> Result<String, Refusal>`:

1. `project_layout(home, name)?` (unknown project is `NotFound`, broken is `Disabled`).
2. `job_id(raw_id)?`.
3. When `is_queued(home, name, &id, &[Action::Run])`: `Conflict("job {id} already has a run queued")`. An approve or retry does not block a run, and a run does not block an approve or retry. The queue entry stays while the child runs, so a running run also blocks.
4. `Request::new(name, Action::Run, &id, series, now_ms)`, error mapped to `BadRequest(e.to_string())`, then `enqueue(home, &req)?`. Returns `req.id`.

It does not read the board. The child reports a run of a job that does not exist.

### 4. `admit::queue_approve`

`pub fn queue_approve(home: &Home, name: &str, raw_id: &str, without_review: bool, waive_reason: Option<&str>, now_ms: u64) -> Result<String, Refusal>` moves the checks of `approve_inner` into the library, with the same messages and kinds:

1. `project_layout`, `job_id`.
2. Board load failure: `Failed("the board of this project cannot be read: {e}")`.
3. No job: `NotFound("no job {id} in project {name}")`.
4. Cancelled: `Conflict("job {id} was cancelled")`.
5. Stage not needs-human or approve: `Conflict("job {id} is in stage {stage}, approve needs needs-human or approve")`.
6. `job.approve_refusal(without_review, waive_reason.is_some())`: `Conflict(why)`.
7. `refuse_if_queued`.
8. `Request::new(.., Action::Approve, ..)`, then `.without_review()` and `.waive(reason)` when given, each error as `BadRequest`; `enqueue`. Returns `req.id`.

`approve_inner` keeps its project and id checks first, then the body parse and `one_line` check of the waive reason, then calls `queue_approve(...).map_err(refusal)`. HTTP codes and messages do not change.

### 5. `factoryd enqueue` and `factoryd status`

- `--approve` gets `conflicts_with = "series"`.
- The run and approve branches call `queue_run` and `queue_approve(home, project, job, false, None, now)`. A refusal prints `factoryd: <message>` and exits 1, as a retry refusal does.
- `Action::name(self) -> &'static str` returns `run`, `approve`, `retry` or `intake`. `status` prints `  {id} {project} {action} job {job}`, for example `  1795-p-001 p approve job 001`.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| `--job 1` or `--job 0001` | Stored as `001`. |
| `--job 0`, `00`, `000` (CLI or HTTP) | `BadRequest("job 000 is not a job, ids start at 001")`; nothing queued. |
| `--job abc`, `""`, `1234567` | `BadRequest` with the `validate_job` message; nothing queued. |
| Unknown project, run or approve | `NotFound("no project named p")`; nothing queued. |
| Approve, then approve | Second refused with the approve-or-retry `Conflict`. |
| Approve `1`, then retry `1` (#120) | Retry refused with the same `Conflict`. |
| Retry, then approve | Approve refused. |
| Run, then run | Second refused with `job 001 already has a run queued`. |
| Run, then approve; approve, then run | Both queued. |
| Old queue entry stored as `1` | Matched as `001` by both checks. |
| A queue entry whose job is not digits | Never matches; it stays a bad entry for the scheduler. |
| CLI approve of a job in build, cancelled, or with a refused review | Refused with the HTTP route's message. |
| `--approve --series` | clap `ArgumentConflict`. |
| Several problems at once | The first check in the order above wins: project, id, board, queue. |

## Testing

`crates/factory-supervisor/src/admit.rs` tests, with `fixture`, `job_json`, `write_board` and `parked` as `queue_retry_queues_one_retry_and_refuses_a_second` uses them:

- `job_id_refuses_job_zero`: `"0"`, `"00"`, `"000"` give the job-0 `BadRequest`; `"1"` gives `"001"`.
- `refuse_if_queued_matches_an_unpadded_entry`: an approve of job `"1"` written with `Request::new` and `queue::enqueue`, then `queue_retry("1")` gives the `Conflict`.
- `queue_run_pads_the_job_id`: a run of `"0001"` stores `"001"`.
- `queue_run_refuses_a_second_run`: run `"1"` then run `"001"` gives `job 001 already has a run queued`; queue length 1.
- `queue_run_does_not_block_an_approve_or_retry`: a run, then a retry of a parked job; both queue.
- `queue_run_and_approve_do_not_block_each_other`: board in stage approve; a run then an approve, and in a second home an approve then a run; each pair queues 2.
- `queue_run_refuses_an_unknown_project_and_a_bad_id`: `"nope"` gives `NotFound`; `"abc"`, `""`, `"1234567"` give the `validate_job` message; queue empty.
- `queue_approve_pads_and_refuses_a_second_approve`: board in stage approve; approve `"1"` stores `"001"`; approve `"001"` gives the approve-or-retry `Conflict`.
- `queue_retry_refuses_after_an_unpadded_approve`: the #120 pair at library level.
- `queue_approve_refuses_an_approve_after_a_retry`.
- `queue_approve_checks_the_board`: job in build, cancelled, unknown job `"2"`, and a needs-human job without a review give the HTTP route's messages.
- `queue_approve_passes_the_flags`: `without_review` reaches the queued request.

`crates/factory-supervisor/src/api/write.rs` tests: every existing approve route test passes unchanged; `approve_checks_the_job_id` adds `"0"` and `"000"` as `bad_request`.

`crates/factory-supervisor/src/bin/factoryd.rs` tests (bare temp home, no projects):

- `enqueue_refuses_an_unknown_project_for_run_and_approve`: both give `no project named nope`; queue empty.
- `enqueue_approve_conflicts_with_series`: clap `ArgumentConflict`.
- `status_shows_the_action_of_a_queued_request`: an approve written with `queue::enqueue` shows `approve job 001`.

`crates/factory-supervisor/src/request.rs` tests: `action_name_matches_the_serde_name`.

The #120 pair is tested at library level, because a bin test on a bare home has no project for a retry.

Commands: `cargo test -j4 -p factory-supervisor`, `cargo clippy -j4 -p factory-supervisor --all-targets -- -D warnings`.

## Files changed

- `crates/factory-supervisor/src/admit.rs`: `job_id` job-0 rule, `is_queued`, `queue_run`, `queue_approve`, tests.
- `crates/factory-supervisor/src/request.rs`: `Action::name`.
- `crates/factory-supervisor/src/api/write.rs`: `approve_inner` calls `queue_approve`; one test case.
- `crates/factory-supervisor/src/bin/factoryd.rs`: clap conflict, the two branches, the status line, tests.
- `docs/reference/factory-admin-manual.md:193` and `:196`: the padded id, job 0, and the refused duplicates.
- `docs/guides/factory-user-guide.md:1099`, `:1142`, `:1157-1158`: the status line, the padded id and job 0, the project check, the clap conflict and the duplicate rules.

## Constraints

- Cargo with `-j4`, never `--workspace` or `--all`.
- Tests use `.expect("...")` and `expect_err`.
- Code comments are short full sentences.
- HTTP codes and messages of the approve route stay the same.

## Acceptance criteria

- `factoryd enqueue` stores run and approve job ids padded to three digits.
- Job id 0 is refused by `factoryd enqueue` and the HTTP routes.
- An approve or retry is refused while an approve or retry of the same job is queued, also when the queued entry has an unpadded id.
- A second run of a job is refused while a run of it is queued; runs and approves do not block each other.
- A CLI approve is refused for the same board states as the HTTP route, with the same messages.
- A CLI run or approve of an unknown project is refused and nothing is queued.
- `--approve --series` is refused by clap.
- `factoryd status` shows each queued request's action.
- The admin manual and the user guide describe the padding, job 0, the project check, the `--approve --series` conflict and the duplicate rules; the user guide no longer says enqueue skips the project check.
- `cargo test -j4 -p factory-supervisor` and clippy pass.

## Out of scope

- `--without-review` and `--waive` on `factoryd enqueue`.

## Decisions

- How much should the fix cover? Options: pad and add the duplicate check to CLI approve, pad only, full API parity for approve. Chosen: pad and add the duplicate check. Decided by: user
- Where does the padding happen? Options: in factoryd enqueue, in Request::new. Chosen: in factoryd enqueue. Decided by: user
- Should a CLI approve run the HTTP route's board checks? Options: share them through `admit::queue_approve`, file a ticket. Chosen: share them. Decided by: user
- Should a run be refused while a request for the job is queued? Options: a run blocks only a run, any queued request blocks a run, leave runs as they are. Chosen: a run blocks only a run. Decided by: user
- Should job id 0 be refused? Options: refuse it everywhere, file a ticket. Chosen: refuse it everywhere. Decided by: user
- Small follow-ups from the adviser review (padded compare in the duplicate check, project check for CLI run and approve, clap conflict, action in status, manual lines). Options: include, file. Chosen: include, since the user asked for all follow-ups in this job. Decided by: user
- Where does the logic live? Options: library functions in `admit`, inline in the binary. Chosen: the library, so tests can use the `pub(crate)` fixture and the HTTP route can share it. Decided by: call
- No `--without-review` or `--waive` on `factoryd enqueue`. `/factory approve` calls `factory approve` directly, so nobody needs them there. Decided by: call
