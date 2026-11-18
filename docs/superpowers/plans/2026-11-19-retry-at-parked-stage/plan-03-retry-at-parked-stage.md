# Retry at the parked stage: API and dashboard (plan 3 of 3)

> **Spec:** docs/superpowers/specs/2026-11-19-retry-at-parked-stage-design.md
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** factoryd queues a retry, the rework endpoint passes `force`, and the dashboard shows a working Retry button first on a parked card that has a resume stage.

**Architecture:** A new `Action::Retry` in `factory-supervisor/src/request.rs` maps to the child command `retry <id>` in `spawn.rs`. `POST /projects/{p}/jobs/{id}/retry` checks the board, refuses what the child would refuse, and enqueues. The dashboard (its own workspace) reads the optional `park_resume` field, forwards `retry` through its `--home` proxy, renders the button and posts it.

**Tech Stack:** Rust (factory-supervisor, dashboard), plain JavaScript with `node --test`. No new dependency.

## Global Constraints

- Consumed from plans 1 and 2: board key `park_resume` with the words `review`, `rereview`, `merge`, `sync`; board key `park_kind` value `review-failed`; CLI `factory retry <id>`; CLI `factory rework --force -- <id> <note>`; `Job::stage_before_park()` and `Job::approve_refusal` in factory-core.
- `POST /projects/{p}/jobs/{id}/retry`:
  - Body: empty or `{}` (a `RetryBody` with `deny_unknown_fields` and no fields); any field is `400 bad_request`.
  - `202 {"requestId"}` on success.
  - `404 not_found` for an unknown job.
  - `409 conflict` when the job is cancelled, not in `needs-human`, or has no `park_resume` (message `job <id> has no resume stage, see its park for the next command`).
  - `409 conflict` with the text of `job.approve_refusal(false, false)` when `park_resume` is `merge` or `sync`, `job.stage_before_park() != Some(Stage::Approve)` and that refusal is `Some` (the child would refuse it, and nobody reads a child's refusal, see `write.rs:422`).
  - `409 conflict` when the job has a pending `batch_snapshot` (the child refuses it).
  - `409 conflict` when the queue already holds an `Approve` or `Retry` request for the same project and job (a double click). Check with `crate::queue::list(home)` (`queue.rs:111`, returns `Vec<QueueItem>`; skip `QueueItem::Bad`): any `QueueItem::Ok(r)` with `r.project == name`, `r.job == <normalized id>` and `r.action` `Approve` or `Retry`. The queue entry is removed only when the child finishes (`supervisor.rs:438`), so this also covers a running request.
- `POST .../rework` body gains `"force"?: boolean`. With `true` the child args are `["rework", "--force", "--", id, note]`; without it, exactly today's args. A non-boolean `force` is `400`.
- A `Retry` request carries no `series`, `without_review` or `waive_reason`; `Request::new` already refuses those for non-Run/non-Approve actions (`request.rs:130-142`), so no new check is needed.
- Per spec: the daily budget gate treats `Retry` like `Run`, held back when the day's budget is used up, because a retry can run reviewers, fix rounds and the next plans of a series. Document that a merge-only retry waits too.
- `GET /projects/{p}/jobs` adds `"parkResume"` (the word or `null`) to each job.
- Out of scope: a `factoryd enqueue --retry` flag; a forced rework button on the dashboard (forced rework stays CLI and API only, so the dashboard drops Rework on a card with `park_resume`).
- Dashboard model fields use the existing park-field attributes `#[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]` (`dashboard/src/model.rs:165-185`).
- Keep the API token out of every reply. Cargo: `-j4`, never `--workspace`; the dashboard builds with `--manifest-path dashboard/Cargo.toml`. JS tests: `node --test dashboard/tests/*.test.js` from the repo root.

## Review Focus

1. The allow table (`http.rs:102`) is the first gate. Adding `retry` there without its own dispatch arm would send it to the cancel catch-all at `http.rs:121`. Pinned by `retry_route_never_calls_cancel`.
2. Retry on a job the child would refuse (merge retry with open changes and no earlier approval): 409 before queueing. Pinned by `a_merge_retry_without_an_earlier_approval_and_open_changes_is_409`.
3. Double click: a second retry while one is queued is 409. Pinned by `a_second_retry_while_one_is_queued_is_409`.
4. A retry request file with an unknown action word from another version: rejected as bad, no panic. Pinned by `an_unknown_action_word_is_rejected`.
5. A card in needs-human without `park_resume`: no Retry button, Rework stays, the existing rework chips stay. Pinned by the `jobActions` cases and the unchanged `board.test.js` rework assertions.

## File Structure

- `crates/factory-supervisor/src/request.rs`, `spawn.rs`, `scheduler.rs`: `Action::Retry`, its child args, the budget gate and its doc comments (`scheduler.rs:20`, `31-33`).
- `crates/factory-supervisor/src/api/http.rs`, `api/write.rs`, `api/read.rs`: the route, `retry`, `ReworkBody.force`, `parkResume`.
- `dashboard/src/model.rs` (`park_resume`), `dashboard/src/state.rs` (the `Job` literal at 368), `dashboard/src/home_mode.rs` (job-action whitelist at 508).
- `dashboard/static/projects.js` (`jobActions`, `jobPathFor`), `dashboard/static/board.js` (`ACTION_LABELS` at 700, card hint at 742, `summaryJob` at 861, `PARK_LABEL` at 395-411), `dashboard/static/app.js` (`startAction` at 468, its guard at 477).
- Docs: `README.md` (budget at 501-503, API table 525 and 537-538, line 573), `docs/reference/factory-admin-manual.md:195`, `dashboard/README.md:25` and `:63`, `docs/guides/getting-started.md:322`, `docs/guides/factory-user-guide.md:1176` and `:1194`.

---

### Task 1: factoryd retry action and endpoint

**Files:**
- Modify: `crates/factory-supervisor/src/request.rs` (`Action` at 28-33), `spawn.rs` (`args_of` at 81-108), `scheduler.rs` (gate at 52-53, comments at 20 and 31-33), `api/http.rs` (allowed methods at 102, dispatch at 117-121), `api/write.rs` (`approve_inner` at 376 as the pattern, `rework_inner` at 450), `api/read.rs` (job objects at 134-143)
- Modify docs: `README.md` (budget paragraph 501-503, API table 525 and 537-538, line 573), `docs/reference/factory-admin-manual.md:195`, `docs/guides/factory-user-guide.md:1142` (retries wait like runs)
- Test: unit tests in those source files

**Interfaces:**
- Produces: `Action::Retry` (serde word `"retry"`); child args `["--project", p, "retry", id]`; `pub fn retry(home: &Home, deps: &ApiDeps, name: &str, id: &str, body: &[u8]) -> ApiResponse`; `ReworkBody.force: Option<bool>`; `parkResume` in the jobs read.

- [ ] **Step 1: Read** `request.rs`, `args_of`, the scheduler gate, the router in `http.rs` (note the catch-all at 121), `approve_inner`, `rework_inner`, the `board_with` and `job_json` helpers in the `write.rs` tests (792-806), and the jobs read at `read.rs:134-143`.
- [ ] **Step 2: Write the failing tests**:
  - `request.rs`: `retry_request_round_trips`; `retry_refuses_series_and_approve_flags`; `an_unknown_action_word_is_rejected` (`serde_json::from_str::<Request>` of a good request JSON with `"run"` replaced by `"explode"` is `Err`).
  - `spawn.rs`: `retry_child_args` with the `req()` helper (job `"7"`): `["--project", "p", "retry", "7"]`.
  - `scheduler.rs`: `retry_is_held_by_the_daily_budget`.
  - `write.rs` (board via `board_with`, adding `"park_resume":"merge","park_kind":"other","stage_history":[{"stage":"approve","at":1.0},{"stage":"needs-human","at":2.0}]` to `job_json` with `.replace`): `retry_queues_a_request` (202, a queued `Retry`); `retry_without_resume_is_409_and_queues_nothing`; `retry_of_a_cancelled_job_is_409`; `retry_outside_needs_human_is_409` (stage `build` with `park_resume`); `retry_of_an_unknown_job_is_404`; `retry_refuses_unknown_body_fields` (`{"x":1}` is 400, `{}` is 202); `retry_with_a_pending_batch_commit_is_409`; `a_merge_retry_without_an_earlier_approval_and_open_changes_is_409` (add `"last_review":{"round":1,"verdict":"changes","blockers":1,"file":"review-code-r1.md","source":"review"}` and no `stage_history`); `a_second_retry_while_an_approve_is_queued_is_409` (first `crate::queue::enqueue` an `Approve` for job `001` with a different ts, e.g. `Request::new("good", Action::Approve, "001", false, 1)`, then POST retry: 409 and the queue still holds one request) and `a_second_retry_while_a_retry_is_queued_is_409` (same with a queued `Retry`); `retry_route_never_calls_cancel` (FakeCli records no `cancel` call); `an_unknown_job_action_is_not_found` (`POST .../explode` is 404).
  - `write.rs` rework: `forced_rework_passes_force_before_the_separator` (args `["rework", "--force", "--", "001", "note"]`); `unforced_rework_args_are_unchanged`; `forced_rework_keeps_the_note_after_the_separator` (note `-x`); `a_non_boolean_force_is_400` (`{"note":"n","force":"yes"}`).
  - `read.rs`: `jobs_read_includes_park_resume`.
- [ ] **Step 3: Run** `cargo test -j4 -p factory-supervisor` and see the new tests fail. These regression tests already pass before any change and must keep passing: `an_unknown_job_action_is_not_found`, `an_unknown_action_word_is_rejected`, `unforced_rework_args_are_unchanged`.
- [ ] **Step 4: Implement** the Produces items. In `http.rs`, add `"retry"` to the POST allow list, add the `retry` arm before the catch-all, and change the catch-all at 121 to match `"cancel"` explicitly with anything else `not_found`. Update the docs listed in Files and the scheduler comments: retries wait for the daily budget like runs, the new endpoint and its 409 cases, `force` on rework, `parkResume`, and "approve, retry, rework or cancel a job".
- [ ] **Step 5: Run** `cargo test -j4 -p factory-supervisor` and `cargo clippy -j4 -p factory-supervisor --all-targets -- -D warnings`. Expected: pass, including `tests/daily_budget.rs`.
- [ ] **Step 6: Commit** `feat(supervisor): queue a retry request and pass rework force`.

### Task 2: Dashboard retry button

**Files:**
- Modify: `dashboard/src/model.rs` (park fields at 165-185), `dashboard/src/state.rs` (`Job` literal at 368 gains `park_resume: None`), `dashboard/src/home_mode.rs` (whitelist at 508), `dashboard/static/projects.js` (`jobActions` at 105, `jobPathFor` at 125), `dashboard/static/board.js` (`PARK_LABEL` 395-411, `ACTION_LABELS` 700, card hint 742, `summaryJob` 861), `dashboard/static/app.js` (`startAction` at 468, its guard at 477)
- Modify docs: `dashboard/README.md` (action list at 25, forwarded routes at 63), `docs/guides/getting-started.md:322`, `docs/guides/factory-user-guide.md:1176` and `:1194`
- Test: `dashboard/src/model.rs` test module (`job_with` at 314), `dashboard/src/home_mode.rs` tests (loop at 997), `dashboard/tests/projects.test.js`, `dashboard/tests/board.test.js`

**Interfaces:**
- Consumes: board field `park_resume`, park kind `review-failed`, `POST .../retry` from the factoryd task.
- Produces:
  - `park_resume: Option<String>` on the dashboard job model.
  - `home_mode.rs` forwards `/api/projects/<p>/jobs/<id>/retry` to `/projects/<p>/jobs/<id>/retry`.
  - `jobActions(job)`: `["retry", "approve", "cancel"]` for a needs-human job with a non-empty string `park_resume`; today's list otherwise. `jobPathFor` accepts `"retry"`.
  - `ACTION_LABELS.retry = "Retry"`; `startAction` handles `kind === "retry"` like a plain approve: `runWrite(button, () => postApi(path), () => \`Queued a retry of job ${id}\`)`, no dialog, no body.
  - With a non-empty `park_resume` and `cancelled !== true`, the card hint and `summaryJob` (which keeps its cancelled branch first) show `factory retry <id>` first, then the approve command, then (card only) `factory cancel <id>`, and no `factory rework` chip. Otherwise unchanged.
  - `PARK_LABEL["review-failed"] = "Review failed"` (amber, not in `PARK_RED`).

- [ ] **Step 1: Read** the files above and their existing tests, including the rework assertions at `board.test.js:141` and `230`.
- [ ] **Step 2: Write the failing tests**:
  - `model.rs`: `park_resume_is_optional_and_round_trips`; `a_non_string_park_resume_loads_as_none` (`5` gives `None`).
  - `home_mode.rs`: add `"retry"` to the action loop at 997 and assert the forward path.
  - `projects.test.js`: `jobActions` with and without `park_resume`; `jobPathFor("p", "001", "retry")` is `/api/projects/p/jobs/001/retry`.
  - `board.test.js`: a needs-human card with `actions: ["retry","approve","cancel"]`, `project: "p"` has `data-act="retry"` as its first button; its hint shows `factory retry 001` and no `factory rework`; `renderSummary` with `park_resume` shows `factory retry 001`; a `review-failed` chip reads "Review failed"; a cancelled card with `park_resume` shows no `factory retry`; the existing rework assertions still pass.
- [ ] **Step 3: Run** `node --test dashboard/tests/*.test.js` and `cargo test -j4 --manifest-path dashboard/Cargo.toml` and see the new tests fail.
- [ ] **Step 4: Implement** the Produces items and update the dashboard README and the two guides (the action list, the forwarded routes, Retry on parked cards).
- [ ] **Step 5: Run** both test commands and `cargo clippy -j4 --manifest-path dashboard/Cargo.toml --all-targets -- -D warnings`. Expected: pass.
- [ ] **Step 6: Commit** `feat(dashboard): offer retry on parked jobs`.

Next plan: none
