# factory-server

Local dashboard for Rocket's software factory. It is dev tooling: it is not part of the Rocket app and is not built with it.

## Run

```bash
cargo run -j4 --manifest-path tools/factory-server/Cargo.toml -- --root "$(python3 .claude/factory/factory-ctl root)" --port 8765
```

Open http://127.0.0.1:8765/. `--root` is the main checkout (the folder that holds `.superpowers/`), which the `root` command prints even from a worktree. Default port 8765.

## What it reads

- `.superpowers/factory/board.json`: job stages, written by `.claude/factory/factory-ctl`.
- `.superpowers/sdd/<job dir>/plan.md`, `spec.md`, `progress.md`: the plan title, spec goal and task list on each card. A task is done when its box is ticked or the ledger says `Task N: complete`; a merged job shows every task done.
- `.superpowers/factory/events.jsonl`: subagent and tool events, appended by `.claude/factory/hooks/log-event`.

It never writes either file. If it is not running, the factory works the same.

## Endpoints

| Path | Returns |
|---|---|
| `/` and `/dashboard.html` | the page |
| `/dashboard.css`, `/board.js`, `/app.js` | page assets |
| `/api/state` | JSON: `generated_at`, `board_error`, `jobs[]`, `unassigned_agents[]`, `recent[]` |
| `/healthz` | `ok` |

Only `GET` is accepted, only `127.0.0.1` is bound, and a `Host` header other than `127.0.0.1:<port>` or `localhost:<port>` gets 403.

## Tests

```bash
cargo test -j4 --manifest-path tools/factory-server/Cargo.toml
node --test tools/factory-server/tests/board.test.js
bash tools/factory-server/tests/smoke.sh
```

`tests/seed-demo.sh <dir>` builds a demo root with a job in every stage for a visual check.
