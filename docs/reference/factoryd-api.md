# factoryd API

`factoryd run` serves a small JSON API for the dashboard and other local tools. The supervisor itself is described in `projects-and-supervisor.md` (factoryd).

- The token is a random secret in `<home>/api.token` (mode `0600`). It is created when absent and never rewritten.
  Send it as `Authorization: Bearer <token>`. `factoryd` never prints it.
- `listen` in `config.toml` sets the address, default `127.0.0.1:8766`. Only loopback addresses are accepted
  (port `0` picks a free port). A bind failure stops `factoryd` before it takes any job.
- The `Host` header must name `127.0.0.1`, `localhost` or `[::1]`, else the answer is `400 bad_request`.
- At most 16 requests are served at once; one more gets `409 conflict` ("busy").

| Endpoint | Answer |
|---|---|
| `GET /projects` | `[{name, repo, preset, baseBranch, status, reason, stateDir, jobsDir, activeJob}]`, `status` is `ready`, `busy` or `disabled` |
| `GET /projects/{p}/jobs` | `{jobs, drafts, queued, lastResults}` from the board, the queue and the result records. `drafts` is `[{slug, stage, updatedAt, stale}]` for the interview drafts on the board (`stale` once a draft is older than `draft_stale_hours`). Each job has `parkResume`, the stage a retry continues from (`build`, `review`, `rereview`, `merge` or `sync`), else `null` |
| `GET /version` | `{factoryd, factory}`: the version of this factoryd and of the `factory` binary the jobs run, read with `factory --version` on each call. When that fails, `factory` is `null` and `factoryError` says why |
| `GET /usage?days=N` | `{days, projects: [{name, total, perDay: [{day, tokens}]}], dailyBudget?}`, weighted tokens per UTC day, `N` 1 to 90 (default 14). `dailyBudget` is `{limit, used, resetsAt}` (`resetsAt` is an RFC 3339 UTC time) and is present only when `daily_budget_weighted` is set |

Write endpoints. A body is JSON of at most 64 KiB, unknown fields are refused, and everything is checked before a
process starts or a request is queued. Commands that only `factory` implements run as `factory --project <p> <cmd>`
child processes (60 second limit, arguments passed one by one, never through a shell). The binary is found like
for the job children: `FACTORY_BIN`, else next to `factoryd`.

| Endpoint | Body and answer |
|---|---|
| `POST /projects/{p}/jobs` | `{"spec": "/abs/spec.md", "plans": "/abs/plans-folder", "model"?, "feature"?, "run"?, "series"?, "slug"?, "accept_spec"?, "one_job"?, "accept_over_budget"?}`. Both paths must be absolute, exist and hold no `..`. `model` matches `[A-Za-z0-9._-]{1,64}`, `feature` is one line of at most 200 characters (default: the slug). The slug is the plans folder name without a leading `YYYY-MM-DD-` (`plans/2026-10-16-standalone-software-factory` gives `standalone-software-factory`); a `slug` in the body overrides it and must match `[a-z0-9][a-z0-9-]{0,63}`, and a name that gives no slug is `bad_request`. `accept_spec` is one line of at most 200 characters and is passed as `--accept-spec=<reason>`. `one_job` and `accept_over_budget` (booleans) are passed as `--one-job` and `--accept-over-budget`; `accept_over_budget` needs `one_job` and is `400 bad_request` without it, and a `needs consent:` refusal of the budget guard answers `409 conflict`. Runs `factory new`, which runs the spec reviewer, so this call waits up to the project's `stall_minutes` plus two minutes (other commands wait 60 seconds); a refusal answers `409 conflict` when it carries the `--accept-spec` override hint (a missing user decision, or a spec review that asked for changes or did not finish), and every other failure of `factory new` stays a 500. Then it queues a run unless `run` is `false`. `series` (boolean, default `true`) makes that run continue with the next plans of the series, like `factory run --series`; it only matters when `run` is true. The spec path may be any readable file and is canonicalized once (a swap between the check and the command is accepted, because a token is required). `201 {"job", "requestId"}` (`requestId` is `null` when nothing was queued) |
| `POST /projects/{p}/jobs/{id}/approve` | `{"without_review"?, "waive_reason"?}` (a boolean, an empty body is fine). Needs a job in `needs-human` or `approve` that is not cancelled. A job in either stage with open review changes is `409 conflict` unless `waive_reason` is sent, and stays `409 conflict` with any body while a required review (code, or security when the job has one) has no verdict. A `needs-human` job whose code review is pending or absent (the checks never passed or the reviewer never ran) is `409 conflict` with the message `job 020 has no code review yet ...`, unless `without_review` is `true`, which queues `factory approve --without-review`. It is `409` when an approve or retry request for the job is already queued or running. Queues an approve request. `202 {"requestId"}` |
| `POST /projects/{p}/jobs/{id}/retry` | An empty body or `{}`; any field is `400`. Continues a parked job at the stage that parked it. `409 conflict` when the job is cancelled, is not in `needs-human`, has a pending batch commit, or has no resume stage (`parkResume` is `null`) (for example `job 020 has no stage to retry at; use factory rework 020 <note>, factory cancel 020`). For a `merge` or `sync` resume without an earlier approval, it is also `409` with the refusal text of a plain approve (for example open review changes). It is `409` too when an approve or retry request for the job is already queued or running. Queues a retry request, which waits for the daily budget like a run. `202 {"requestId"}` |
| `POST /projects/{p}/jobs/{id}/rework` | `{"note": "one line, 200 characters at most", "force"?}` (`force` is a boolean). Runs `factory rework`, or `factory rework --force` when `force` is `true`, which is how a park with a resume stage is reworked. `200 {"output"}` |
| `POST /projects/{p}/jobs/{id}/cancel` | `{"reason"?, "force"?, "only"?}` (`force` and `only` are booleans, an empty body is fine). Runs `factory cancel`, never a signal. Without `only`, the cancel also cancels the later unmerged plans of the job's series; `only: true` passes `--only`. A refusal by the command, such as a live job lease without `force`, is `409 conflict`. `200 {"output"}` |
| `GET /events?project={p}` | Server-sent events, see below |

A project that cannot take work (broken `project.toml`, missing repo) answers `409 project_disabled`. A failed
`factory new` answers `500 failed` with the end of its stderr in the message.

`GET /events` answers `text/event-stream`. Each `data:` line is one object of that project's `events.jsonl` with an
added `"project"` field. The stream first replays the last 50 lines, then follows new complete lines (polled every
500 ms). Without `project` it streams every project, and projects added later join. A `: keepalive` comment is sent
every 15 seconds. At most 8 streams are open at once (they also use handler slots), more get `409 conflict`.

```
curl -N -H "Authorization: Bearer $TOKEN" "http://127.0.0.1:8766/events?project=rocket"
data: {"...the original event fields...": "...", "project": "rocket"}
```

Errors are `{"error": {"code", "message"}}` with these codes: `unauthorized` (401), `bad_request` (400),
`not_found` (404), `method_not_allowed` (405), `project_disabled` (409), `conflict` (409), `failed` (500) and `unavailable` (503, the daemon cannot start a thread right now; try again). A wrong, missing or short
token always gets the same 401 body.

```
TOKEN=$(cat ~/.factory/api.token)
curl -H "Authorization: Bearer $TOKEN" http://127.0.0.1:8766/projects
curl -H "Authorization: Bearer $TOKEN" "http://127.0.0.1:8766/usage?days=7"
```

## The dashboard across projects

`factory-server --home <factory home> [--api 127.0.0.1:8766] --port 8765` shows every registered project in one
page, with a tab per project. It reads the token from `<home>/api.token` on the server and calls the API for the
project list and every write, so the browser never sees the token. `--api` must be a loopback address and
defaults to `127.0.0.1:8766`. The board, the event log, the costs file and the job folders of a project come from
the `stateDir` and `jobsDir` the API reports. `--home` and `--root` cannot be combined, and `--root` keeps its
single-project behavior. The page can create a job (spec, plans folder, optional model and slug, "Run now" and
"Run as series") and approve, retry, rework or cancel a job. A write needs an allowed `Host`, an `Origin` equal to
`http://<Host>` and `Content-Type: application/json`, else the dashboard refuses it before factoryd is called.
Errors show as a banner with the API's error code (`token_rejected` when factoryd refuses the dashboard's token).
