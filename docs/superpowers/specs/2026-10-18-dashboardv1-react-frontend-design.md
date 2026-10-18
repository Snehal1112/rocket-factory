# dashboardv1: React frontend for the factory dashboard

Status: draft for review. Nothing here is built. Dashboard work is low priority, so this spec and its plan exist to make a later build fast.

## Intent

The current dashboard frontend (`dashboard/static/`: `app.js`, `board.js`, `projects.js`, `patch.js`, `dashboard.css`, `dashboard.html`) renders screens as HTML strings from global scripts, with no types and no build step. The owner finds it not manageable or maintainable, and the planned draft cards (see `2026-10-18-factory-interview-cards-and-execution-design.md`) will add more to it.

`dashboardv1` is a new frontend in React and TypeScript with the same features, the same look and the same behaviour, split into typed parts that can be changed one at a time.

## Goals

- Feature parity with today's dashboard in both modes (`--root` and `--home`). See "Parity checklist".
- The same design system: reuse the CSS variables from `dashboard/static/dashboard.css` (oklch colour tokens, light and dark themes, `--radius`) and the same font stack. No redesign.
- Two ways to run: development with hot reload, and a static build that the Rust dashboard serves.
- Live updates pushed from `factoryd`, with polling as a fallback.
- Leave room for draft cards (`board.drafts[]`) without a rewrite.
- Types for every payload, so a contract change breaks the build instead of the page.

## Non-goals

- No redesign, no new screens, no new features in v1.
- No change to the board JSON, the ledger, `factoryd`'s API or the dashboard's error codes.
- No in-dashboard interview. The dashboard creates jobs only from existing spec and plan files, as today.
- No removal or edit of `dashboard/`. It keeps working until `dashboardv1` reaches parity and the owner decides to switch.
- No server-side rendering, no Node requirement at runtime.

## Anti-goals

- A new look that nobody asked for.
- A frontend that ever holds, logs or can read the API token.
- A dev setup that needs Rust's security checks loosened.
- A build that only works with Bun installed on the machine that runs the dashboard.

## Constraints (from the existing dashboard, all must hold)

1. The token never reaches the browser. The Rust dashboard injects it from `<home>/api.token`. The smoke script greps `/`, `/app.js`, `/projects.js` and `/api/projects` for it, and a `dashboardv1` check must grep every built asset.
2. CSP stays `default-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'`. No inline script, no external fonts, scripts or requests. Bar widths are set from script today. In React, setting `style` goes through the DOM style API, which the CSP allows; verify this in the first spike.
3. Writes are `POST` with `Content-Type: application/json`, an `Origin` equal to `http://<Host>`, a loopback Host, and a body of at most 64 KiB. Only the four write routes exist: create job, approve, rework, cancel. Writes can take up to 70 s, and the page never retries after `api_timeout`.
4. Mode is detected at runtime: `GET /api/projects` is 404 in root mode and 200 in home mode.
5. Data contracts stay as they are: `StateView`, job, spec card, `ParkView`, and errors as `{"error":{"code","message"}}`. A missing or empty file is an empty view. A park 404 is plain text.
6. Stage vocabulary: spec, build, review, approve, merged, needs-human. Unknown stages show in a stray column. Unknown or missing optional fields are tolerated.
7. Action rules: needs-human gives approve, rework and cancel. Approve gives approve and cancel. Merged and cancelled give none. Everything else gives cancel only. Approving a needs-human job whose code review is missing needs `without_review` (otherwise 409). Rework notes are one line, at most 200 characters. Cancel takes `reason` and `force`.
8. Accessibility stays: an aria-live announcer, a status pill with stale and error states, `dialog` elements for confirm and park, and a budget strip hidden when no limit is set.
9. The dashboard is loopback only and read-only against the board, ledger and events.

## What the owner decided

- React and modern JavaScript on the 2026 ecosystem, with TypeScript.
- **Bun** instead of Yarn, for this project.
- A new folder next to `dashboard/`, named `dashboardv1/`.
- Hot reload in development, with the frontend and the Rust dashboard running separately. **Caddy** is the dev proxy.
- A static build that the Rust dashboard serves.
- Same design system and same fonts. No redesign.
- Live push updates.
- Version 1 matches today's dashboard feature for feature. Draft cards come later, when the interview-cards work lands.

## How it runs

### Development

```
browser -> Caddy :5173? --> /api/*  -> Rust dashboard (127.0.0.1:8765)
                        \-> everything else -> Vite dev server (hot reload)
```

- Caddy forwards `/api/*` to the Rust dashboard and everything else to the dev server. It rewrites `Host` and `Origin` for the upstream so the Rust checks stay unchanged. This is the planned approach and is the owner's call to proxy with Caddy. The exact Caddyfile is for the builder.
- The event stream route must not be buffered (`flush_interval -1` or equivalent).
- The Caddyfile and a one-line start script live in `dashboardv1/`.

### Static build

- `bun run build` writes plain files to `dashboardv1/dist/`. Nothing in them needs Bun or Node.
- The Rust dashboard serves them. Today it embeds five named files with `include_str!` (`dashboard/src/server.rs:13-18`) and has no folder serving. The builder chooses between embedding the folder at compile time (for example with a crate such as `include_dir` or `rust-embed`) and serving a directory at run time. Embedding keeps the single-binary property and is the recommended default (my call).
- Hashed asset names need the route table to be built from the folder listing, with `index.html` as the fallback for `/`.
- Old and new frontends coexist. Which one the server serves is chosen by a flag or by build feature (builder's call). The default stays on the old frontend until the owner switches.

### Live updates

- Add one Rust route that proxies `factoryd`'s `GET /events` stream to the browser, adding the Bearer token on the server. `factoryd` replays 50 lines per project, tags each line with its project, allows 8 streams, and carries events only, not the board.
- The browser opens one stream. Each event is a signal to refetch the state for the affected project. Events are coalesced so a burst causes one refetch.
- Polling stays as a fallback: when the stream is down, in root mode (no `factoryd`), or when the stream limit is reached. A slow safety poll runs even while the stream is healthy (suggested 15 s, my guess; builder may tune).
- The status pill reflects stream state (live, reconnecting, polling).
- Stale replies after a tab switch, and out-of-order budget replies, must be dropped, as the current code does with `acceptReply`.

## The Rust changes

These are the only changes outside `dashboardv1/`, and they are small and separate:
1. Serve the built folder (embedded or from disk), with the same security headers on every reply.
2. The event stream proxy route, home mode only. It must keep the token off every reply and header, cap the lifetime of a connection sensibly, and fail with the existing error shape.
3. A way to select which frontend is served.

Everything else about the Rust side stays. Its existing unit tests keep passing, and `smoke-home.sh` gets new checks for the stream route and the built assets.

## Proposed stack (my calls, for the builder to confirm or replace)

| Area | Choice | Why |
|---|---|---|
| Package manager, runner | Bun | Owner's choice. |
| Build and dev server | Vite | Standard for React, hot reload, works with Bun. |
| UI | React 19 and TypeScript, strict mode | Owner's choice. |
| Server state | TanStack Query | Handles refetch, caching, and invalidation from stream events. |
| Styling | Plain CSS carried over from `dashboard.css`, split into modules per component | Keeps the design system untouched. A utility CSS framework would risk drifting from it. |
| Validation of payloads | A small schema layer such as Zod at the API boundary | Keeps lenient parsing of unknown and missing fields in one place. |
| Unit and component tests | Vitest and Testing Library | Replaces the `node --test` render tests. |
| End-to-end | Playwright against the Rust dashboard with the demo seed | Replaces the visual checks done with `seed-demo.sh`. |
| Lint and format | Biome | One tool, fast. |

Versions are chosen when building. Use releases at least two weeks old, pinned exactly.

## Structure (suggested)

```
dashboardv1/
  package.json, bun.lock, tsconfig.json, vite.config.ts
  Caddyfile
  index.html
  src/
    api/        typed client, error mapping, schemas, mode detection
    live/       event stream hook, fallback polling, status
    features/   board, job-card, spec-card, costs, budget, park-dialog,
                action-dialog, new-job-form, project-tabs, commands, agents
    ui/         shared parts (dialog, toast, banner, status pill)
    styles/     tokens.css (copied variables), base.css
    main.tsx
  tests/        unit, component, e2e
```

Rule from the old code to keep: pure functions for rendering decisions (action buttons, review state, stage grouping) live apart from components so they stay easy to test.

## Parity checklist

- Header with status pill and the aria-live announcer.
- Budget strip (home mode): tokens against the limit, UTC reset time, amber at 80%, red at 100%, hidden with no limit or on error.
- Error banner with Dismiss, showing the error code.
- Project tabs and project note with status and reason (home mode). Selected tab kept in `sessionStorage`.
- New job form: spec path, plans folder, model, slug, "Run now", "Run as series".
- Summary, then the six-column board.
- Job card: plan title, goal, task list with done and in-review state, review chips (security, ux, ui, code; pending, running, pass, changes, skipped), round against `MAX_ROUNDS`, token bar, estimate line, stage durations, agents (running, stale, done), "See what went wrong" when `has_park`, action buttons.
- Spec cards for series, with per-plan states (waiting, building, review, needs-human, merged, cancelled).
- Cost panel, "Agents without a job", and the log notice.
- Commands panel with click-to-copy.
- Toast, confirm dialog (note field, force checkbox) and park dialog.
- Light and dark themes through the existing tokens.
- Disabled projects show their reason and an empty board.

## Testing

- Port the cases in `dashboard/tests/board.test.js`, `projects.test.js` and `patch.test.js` to Vitest, so behaviour that is covered today stays covered.
- Contract tests for the schemas using recorded payloads, including old, partial and unknown-field boards.
- A check that no built asset contains the token.
- A check that the CSP produces no violations in a browser run (Playwright, listening for CSP errors).
- Extend `smoke-home.sh` for the stream route and built-asset serving, and update its asset-path checks when the new frontend is selected.

## Settled while reviewing the draft

- **Stream format (answered by the code).** `factoryd`'s `GET /events` is server-sent events with no end. Each message is one line of a project's `events.jsonl`, a JSON object with `"project"` added (`crates/factory-supervisor/src/api/events.rs:1-60`). It is the contract, and `factoryd` does not change. The builder reads `events.rs` and `Timing` for heartbeat and lifetime behaviour before writing the parser.
- **Root mode (answered).** The browser uses the same mode probe as today (`GET /api/projects`). It opens the stream only in home mode. In root mode it polls, and no stream route is called.
- **No CI exists (answered).** The repo has no `.github/` or other CI. The plan covers local checks only, and Playwright runs locally. A CI pipeline is the separate release work in GitHub issue #1.
- **Caddy (answered).** Caddy is the documented dev proxy. Vite's own proxy is not part of v1.
- **Stream sharing (my call).** v1 opens one upstream stream per browser connection and does not share across tabs. Tabs beyond the 8-stream limit fall back to polling. Sharing is a later option.
- **Frontend selection (my call).** Selected at run time by a flag, defaulting to the old frontend. The built `dist/` is embedded at compile time only when it exists, and a Rust build without `dist/` still compiles and serves the old frontend only. `dist/` is not committed. The builder confirms the mechanism.

## Open for the builder

- Embed or serve from disk, and the exact crate if embedding.
- How the frontend is selected (flag or feature).
- The Caddyfile details, and whether Caddy is also documented as optional in favour of Vite's own proxy for people without it.
- Visual parity check: see the owner question below. Pending.
- Final library versions.

## Risks

- The `Origin` and `Host` rewrite in Caddy must match exactly what Rust checks, or writes fail in development only.
- The stream is limited to 8 connections in `factoryd`. Several open tabs can exhaust it, which is why fallback polling is required.
- `factoryd`'s stream carries events only. If a future change needs board data pushed, that is a contract change and needs its own spec.
- The Rust dashboard is tested on Linux only for lease and process checks. This spec does not change that.
