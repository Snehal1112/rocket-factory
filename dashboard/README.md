# factory-server

Local dashboard for the software factory. It is dev tooling and is not shipped.

## Run

```bash
cargo run -j4 --manifest-path dashboard/Cargo.toml -- --root <project checkout> --port 8765
```

Open http://127.0.0.1:8765/. `--root` is the main checkout (the folder that holds `.superpowers/`). Default port 8765.

### Home mode

```bash
cargo run -j4 --manifest-path dashboard/Cargo.toml -- --home ~/.factory --api 127.0.0.1:8766 --port 8765
```

`--home` shows every project of a factory home and needs `factoryd run` for the API. `--home` and `--root` cannot be
used together, and `--api` (default `127.0.0.1:8766`) must be a loopback `host:port`. The server reads
`<home>/api.token` for each call and sends it as `Authorization: Bearer ...`. The token never appears in a page, a
reply body, a header or a log line. The page title is "Rocket Factory" and there is a tab per project. A project
the API marks `disabled` shows its reason and an empty board. The board, events, costs and job folders come from the
API's `stateDir` and `jobsDir`; a missing file shows empty, never an error. The page can create a job and approve,
rework or cancel one. Errors show as a banner with the stable code (`unauthorized`, `bad_request`, `not_found`,
`method_not_allowed`, `project_disabled`, `conflict`, `failed`, `unavailable`), plus these from the dashboard itself:
`token_missing`, `token_rejected` (the API answered 401), `api_unreachable`, `api_timeout` (a write got no answer within 70 seconds and may still have completed, so check the job list before trying again; the page never retries by itself), `api_error`, `forbidden` (Origin),
`unsupported_media_type`.

`--draft-stale-hours N` (a whole number above 0, default 24) sets how long an interview draft may sit unchanged before the state marks it `stale`. It works in both modes. In home mode it is the dashboard's own setting and is separate from `draft_stale_hours` in the home `config.toml`, which only drives the `stale` flag of `GET /projects/{p}/jobs`. Do not add `draft_stale_hours` to a `config.toml` that an older `factoryd` reads, because it rejects unknown keys.

The header shows a daily budget strip in home mode when `daily_budget_weighted` is set in the factory home's `config.toml`: tokens used today against the limit and the UTC reset time. It turns amber from 80% and red at 100% with "New runs are waiting". The strip is global, so it is not tied to the selected project, and it is hidden when there is no limit or the usage call fails. Its text is built from numbers and a fixed phrase only.

## What it reads

- `.superpowers/factory/board.json`: job stages, written by the `factory` driver (the crates under `crates/`, binary in `crates/factory-cli`). Optional `stage_history`, `series`, `series.id`, `cancelled`, `tokens` and `budget_weighted` are read leniently, and a bad value is dropped. The card token bar shows the board total, or the running total from the agent events when that is higher, so it moves while an agent works.
- `.superpowers/sdd/<job dir>/plan.md`, `spec.md`, `progress.md`: the plan title, spec goal and task list on each card. A task is done when its box is ticked or the ledger says `Task N: complete`; a merged job shows every task done.
- `.superpowers/factory/events.jsonl`: subagent and tool events, appended by the same driver, with `job_id` and `tokens` on every event. There is no hook.

It never writes any of these files. If it is not running, the factory works the same.

- Events without a `job_id` show under "Agents without a job".
- At startup only the last 4 MiB of the log is read.
- Agents idle for a day are dropped (cap 500).
- The registry resets when the log is truncated, replaced or deleted. Rotate by moving the file.
- An agent silent for 10 minutes shows `stale`.

## Endpoints

| Path | Returns |
|---|---|
| `/` and `/dashboard.html` | the page |
| `/dashboard.css`, `/board.js`, `/patch.js`, `/app.js` | page assets |
| `/projects.js` | the project, form and action helpers |
| `/api/state` | JSON (only with `--root`): `generated_at`, `board_error`, `skipped_lines`, `jobs[]` (with `stage_history`, `stage_durations`, `series`, `provider` (a chip on the card, none when missing), `token_view`), `specs[]` (`series_id`, `title`, `goal`, `stage`, `stage_since` (the latest of its plans), `plans[]`; each column lists spec and job cards in one list, newest first by `stage_since`, and Merged shows the 10 newest), `drafts[]` (the board's interview drafts plus `seconds_since_update` and `stale`, shown first in the Spec column; the card carries copyable `factory spec resume` and `factory cancel` commands, no buttons), `spec_warning` on a job (shown on its card), `unassigned_agents[]` |
| `/healthz` | `ok` |
| `GET /api/version` | `{dashboard}` with `--root`. In home mode also `factoryd` and `factory` from factoryd `GET /version` (and `factoryError` when it gave one), or `apiError` with the reason when factoryd gave none. Always 200, and the token never appears. The header shows `v<factoryd version>` (home mode) or `v<dashboard version>` (root mode) next to the title, lists every version on hover, and turns amber when they differ. It is read at load and then once a minute |
| `GET /api/projects` | home mode only: `[{name, repo, preset, baseBranch, status, reason, activeJob}]` by name (404 with `--root`, which is how the page tells the modes apart) |
| `GET /api/projects/{p}/state` | home mode: the same state view for one project |
| `GET /api/projects/{p}/jobs/{id}/park` | home mode: park details of a job |
| `GET /api/usage` | home mode only (404 with `--root`): factoryd `GET /usage?days=14`, token added server-side, same error codes as the other routes. The page reads `dailyBudget` (`{limit, used, resetsAt}`) from it |
| `POST /api/projects/{p}/jobs` | home mode: create a job, forwarded to factoryd (body: `spec`, `plans`, `model?`, `slug?`, `run`, `series`) |
| `POST /api/projects/{p}/jobs/{id}/approve`, `rework`, `cancel` | home mode: forwarded to factoryd unchanged |

Reads wait 5 seconds for factoryd and writes 70 seconds, because factoryd allows `factory new|rework|cancel` 60 seconds. Without `--home` only `GET` is accepted. In home mode a write also needs an `Origin` header equal to `http://<Host>` and `Content-Type: application/json`; a refused write (403 or 415) never reaches factoryd, and a body over 64 KiB gets 413. Only `127.0.0.1` is bound, and a `Host` header other than `127.0.0.1:<port>` or `localhost:<port>` gets 403. The CSP has `default-src 'self'`, `frame-ancestors 'none'`, `base-uri 'none'` and `form-action 'none'`.

## Tests

```bash
cargo test -j4 --manifest-path dashboard/Cargo.toml
node --test dashboard/tests/*.test.js
bash dashboard/tests/smoke.sh
bash dashboard/tests/smoke-home.sh
```

`smoke-home.sh` starts a real `factoryd` with two temporary projects (and a stub for `factory`), then `factory-server --home`, and checks the project list, the refused writes and a valid create.

The smoke test also covers the park endpoint (404 without a file, the evidence with one, no evidence text in /api/state).
The smoke test also checks that costs.json reaches /api/state as a group summary without any per-job list, that a job estimate passes through, and that a broken costs.json gives costs null.

`tests/seed-demo.sh <dir>` builds a demo root with a job in every stage and one five-plan series for a visual check.
Job 003 in the demo is parked with details, so its card shows the "See what went wrong" button.
The demo also has a costs.json and estimates on jobs 001, 004 and 005, so the cost panel and the estimate lines show.
