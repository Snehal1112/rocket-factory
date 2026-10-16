# factory-server

Local dashboard for the software factory in this repo. It is dev tooling and is not shipped.

## Run

```bash
cargo run -j4 --manifest-path dashboard/Cargo.toml -- --root <project checkout> --port 8765
```

Open http://127.0.0.1:8765/. `--root` is the main checkout (the folder that holds `.superpowers/`). Default port 8765.

## What it reads

- `.superpowers/factory/board.json`: job stages, written by the `factory` driver (the crates under `crates/`, binary in `crates/factory-cli`). Optional `stage_history`, `series`, `series.id`, `cancelled`, `tokens` and `budget_weighted` are read leniently, and a bad value is dropped.
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
| `/api/state` | JSON: `generated_at`, `board_error`, `skipped_lines`, `jobs[]` (with `stage_history`, `stage_durations`, `series`, `token_view`), `specs[]` (`series_id`, `title`, `goal`, `stage`, `plans[]`), `unassigned_agents[]` |
| `/healthz` | `ok` |

Only `GET` is accepted, only `127.0.0.1` is bound, and a `Host` header other than `127.0.0.1:<port>` or `localhost:<port>` gets 403. The CSP has `default-src 'self'`, `frame-ancestors 'none'`, `base-uri 'none'` and `form-action 'none'`.

## Tests

```bash
cargo test -j4 --manifest-path dashboard/Cargo.toml
node --test dashboard/tests/board.test.js dashboard/tests/patch.test.js
bash dashboard/tests/smoke.sh
```

The smoke test also covers the park endpoint (404 without a file, the evidence with one, no evidence text in /api/state).
The smoke test also checks that costs.json reaches /api/state as a group summary without any per-job list, that a job estimate passes through, and that a broken costs.json gives costs null.

`tests/seed-demo.sh <dir>` builds a demo root with a job in every stage and one five-plan series for a visual check.
Job 003 in the demo is parked with details, so its card shows the "See what went wrong" button.
The demo also has a costs.json and estimates on jobs 001, 004 and 005, so the cost panel and the estimate lines show.
