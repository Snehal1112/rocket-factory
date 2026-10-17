# API, Token, Event Stream and Multi-Project Dashboard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `factoryd` serves a token-protected JSON API on `127.0.0.1` (projects, jobs, create, approve, rework, cancel, usage, live events), and the dashboard shows every registered project, with its jobs, and creates jobs from existing spec and plan files.

**Architecture:** The API is a module of `factory-supervisor` (`src/api/`), started as a thread by `factoryd run`, so it calls the queue, records and registry functions directly and `factory-supervisor` does not depend on `factory-cli`. Commands that only `factory-cli` implements (`new`, `rework`, `cancel`) run as `factory --project <p> <cmd>` child processes through a `FactoryCli` trait (fake in tests, the real one reuses the `ProcessSpawner` binary lookup). `run` and `approve` go through the existing request queue. The dashboard crate stays a separate workspace and gets a `--home` mode: its own server reads the token file, calls the API on behalf of the browser, and reads each project's board and events files directly from paths the API returns, so the browser never sees the token.

**Tech Stack:** Rust 2021 (cargo 1.94), `tiny_http` 0.12 (already used by `dashboard/`; add it to the root workspace only if `cargo build --offline` finds it in the registry), serde, serde_json, vanilla JS in `dashboard/static`. Token bytes come from `/dev/urandom` through std, no new crate.

**Spec:** `docs/superpowers/specs/2026-10-16-standalone-software-factory-design.md` (Components: `factoryd`; API; Error handling; Acceptance criteria: API token, dashboard shows every project and creates a job from existing files).

## Global Constraints

- Work in `/home/numericlabs/data/rocket/rocket-factory`, on a branch `plan-06-api-and-dashboard` created from `main`. Fast-forward `main` at the end. No push (no remote).
- Every `cargo` command has `-j4`. Never `--workspace` or `--all`. Never set `CARGO_TARGET_DIR`. Test one crate at a time with `-p <crate>`; the dashboard is its own workspace, so run `cargo test -j4 --manifest-path dashboard/Cargo.toml`.
- Commits are conventional commits written through the `dev-workflow-skills:1-git-commit` skill. Stage explicit paths and commit with a pathspec, never `git add -A`, `--all` or `.`.
- No unwrap calls in production paths (tests may use `expect`). No shelling out to `git`.
- API: local JSON over HTTP on `127.0.0.1` only. Every call needs the token. A missing or wrong token returns 401 and does nothing. A refused call returns `{"error":{"code":"<stable_code>","message":"<text>"}}`.
- Stable error codes: `unauthorized` (401), `bad_request` (400), `not_found` (404), `method_not_allowed` (405), `project_disabled` (409), `conflict` (409), `failed` (500). Codes never change once released.
- Token file: `<home>/api.token`, mode `0600`, 64 hex chars, created by `factoryd run` when missing, never logged, never returned by the API. Compare in constant time. Home file names live in `factory-core/src/home.rs`.
- Request bodies are untrusted: project names use `project::validate_name`, job ids are 1 to 6 digits, rework notes follow `validate_rework_note` (one line, at most 200 chars), `model` matches `[A-Za-z0-9._-]{1,64}`, spec and plans paths must be absolute, exist, and are canonicalized before use. Body size is capped at 64 KiB.
- The guard `crates/factory-cli/tests/no_hardcoded_paths.rs` already scans `factory-supervisor/src`; never widen its allowlist. All repo paths come from `Layout` and `Home`.
- Plan 5 rules hold: `factoryd` never kills a child it did not start, never kills children on stop, never restarts a job after a crash. Cancel goes through `factory cancel` and its lease rules, never a signal sent by the API.
- Board JSON, `events.jsonl`, `usage.jsonl` and `costs.json` formats do not change. New fields are optional.
- Without `factoryd` the `factory` CLI behaves as before.

## Review Focus

- No token, an empty `Bearer`, a token of the wrong length or one differing in the last byte: 401, nothing enqueued, no process started, and the 401 body is identical for every case.
- A `Host` header that is not `127.0.0.1:<port>` or `localhost:<port>` (DNS rebinding) is refused even with a valid token.
- A project with a missing repo or degraded settings is listed as disabled with its reason, its jobs endpoint still answers, creating or approving a job returns `project_disabled`, and other projects are unaffected.
- A broken `board.json` or `events.jsonl` in one project does not fail `GET /projects` or `GET /usage` for the others.
- `POST /projects/{p}/jobs` with a relative or missing spec path, a plans path that is a file, extra fields, or a 1 MiB body is refused before any `factory new` process starts.
- `GET /events` clients that disconnect do not leak threads; a truncated or rotated `events.jsonl` does not crash the stream.
- A dashboard `POST` from another web origin (CSRF) is refused, and the browser page never contains the token.

---

### Task 1: Token, server core and read endpoints

**Files:**
- Create: `crates/factory-supervisor/src/token.rs`, `src/api/mod.rs`, `src/api/http.rs`, `src/api/read.rs`, `src/api/usage.rs`, `crates/factory-supervisor/tests/api_read.rs`
- Modify: `crates/factory-core/src/home.rs` (`TOKEN_FILE`, `token_file()`), `crates/factory-core/src/homeconfig.rs` (`listen` key), `crates/factory-supervisor/Cargo.toml`, `src/lib.rs`, `src/bin/factoryd.rs`, root `Cargo.toml` and `Cargo.lock` if `tiny_http` is added, `README.md` (API section)
- Test: unit tests in each new file, plus `tests/api_read.rs`

**Interfaces:**
- Consumes: `Home`, `project::{list, get, layout}`, `Board::load`, `HomeConfig`, `queue::list`, `records::{read_children, read_results}`, `factory_core::budget::weighted_tokens`, `project_lease_status`.
- Produces, used by tasks 2 and 3:
  - `Home::token_file() -> PathBuf` (`<home>/api.token`). `HomeConfig.listen: String` (default `"127.0.0.1:8766"`; load fails unless it parses as a loopback `SocketAddr`, port `0` allowed for tests).
  - `token::{ensure(home) -> Result<String, TokenError>, read(home) -> Result<String, TokenError>, matches(expected: &str, given: &str) -> bool}`; `ensure` creates the file with mode `0600` only when absent and never rewrites an existing one.
  - `api::ApiServer::start(home: Home, listen: SocketAddr, deps: ApiDeps) -> Result<ApiHandle, ApiError>`; `ApiHandle { pub addr: SocketAddr }` with `stop(self)`. `ApiDeps { pub now_ms: Arc<dyn Fn() -> u64 + Send + Sync> }`; task 2 adds the `cli` field.
  - `api::http::{ApiResponse, error_response(status: u16, code: &str, message: &str) -> ApiResponse}` and the router `route(home, deps, method, path, query, headers, body) -> ApiResponse`, pure and unit-testable without a socket.
  - `GET /projects` returns `[{name, repo, preset, baseBranch, status: "ready"|"busy"|"disabled", reason: Option<String>, stateDir, jobsDir, activeJob: Option<String>}]`.
  - `GET /projects/{p}/jobs` returns `{jobs: [{id, slug, feature, stage, branch, tokens, series, cancelled}], queued: [...], lastResults: [...]}` from the board, the queue and the result records.
  - `GET /usage?days=N` (default 14, max 90) returns `{days, projects: [{name, total, perDay: [{day: "YYYY-MM-DD", tokens}]}]}`, UTC days from the `ts` field of each job's `usage.jsonl`.

- [ ] **Step 1: Failing tests for home names, config and token**

`home.rs`: `token_file()` is `<home>/api.token`. `homeconfig.rs`: default `listen` is `127.0.0.1:8766`; `127.0.0.1:0` parses; `0.0.0.0:8766`, `192.168.1.5:80` and `nonsense` are errors. `token.rs`: `ensure` creates 64 hex chars with mode `0600`, a second call returns the same value, `read` of a missing file is an error, `matches` is false for different length, empty and a one-byte difference, and true for equal.
Run: `cargo test -j4 -p factory-core home` and `cargo test -j4 -p factory-supervisor token` — Expected: FAIL to compile.

- [ ] **Step 2: Implement the names, `listen` and `token.rs`**

Randomness: read 32 bytes from `/dev/urandom` with std and hex-encode; create with `OpenOptions` mode `0o600` and `create_new`. `matches` compares in constant time over equal-length byte slices.
Run the same commands — Expected: PASS.

- [ ] **Step 3: Failing tests for the router (no socket)**

In `api/http.rs` and `api/read.rs` tests, with a temp home holding two registered projects (one with a real temp git repo and board, one whose repo is missing):
- no `Authorization`, an empty `Bearer `, a wrong token and a token one byte short all return 401 with code `unauthorized` and identical bodies; a valid token returns 200.
- `Host: evil.example` with a valid token returns 400 `bad_request`; `Host: 127.0.0.1:<port>` and `localhost:<port>` pass.
- `GET /projects` lists both, the broken one with `status: "disabled"` and a `reason`; `GET /projects/ghost/jobs` is `not_found`; `GET /projects/Bad..Name/jobs` is `bad_request`; `DELETE /projects` is `method_not_allowed`.
- `GET /projects/{p}/jobs` returns the board's jobs with queued requests and last results; a corrupt `board.json` makes only that project's jobs call fail, not `GET /projects`.
- `GET /usage?days=2` sums weighted tokens per UTC day from fixture `usage.jsonl` lines (one with cache reads, weighted at 1/10), ignores bad lines; `days=0`, `days=91` and `days=x` are `bad_request`.
Run: `cargo test -j4 -p factory-supervisor api` — Expected: FAIL to compile.

- [ ] **Step 4: Implement the server core, router and read handlers**

`ApiServer::start` binds with `tiny_http`, runs an acceptor thread and short-lived handler threads (at most 16 at once; a 17th gets `conflict` 409 "busy"). The router checks Host first, then the token, then dispatches. Reuse `project::list` for the registry and the same disabled, degraded and busy rules `factory project list` uses. Per-day aggregation lives in `api/usage.rs` as `per_day(text: &str) -> BTreeMap<String, u64>`.
Run: `cargo test -j4 -p factory-supervisor api` — Expected: PASS.

- [ ] **Step 5: Wire `factoryd run` and add the socket test**

`factoryd run` calls `token::ensure`, starts the API on `HomeConfig.listen`, prints one line `api listening on <addr>` (never the token), and stops the API when `run_loop` ends. A bind failure is a clear error and `factoryd` exits non-zero before taking jobs. `tests/api_read.rs` starts the server on port 0 over a temp home and checks with a raw `TcpStream` client: 401 without token, 200 with token, bad Host refused, `GET /projects` JSON shape.
Run: `cargo test -j4 -p factory-supervisor` and `cargo clippy -j4 -p factory-supervisor -- -D warnings` — Expected: PASS, no warnings.

- [ ] **Step 6: README API section, then commit**

Document the token file, `listen`, the endpoints with their error codes and a `curl` example in `README.md`. Commit with the commit skill: `feat(supervisor): add the token-protected read API`.

---

### Task 2: Write endpoints and the event stream

**Files:**
- Create: `crates/factory-supervisor/src/api/write.rs`, `src/api/events.rs`, `src/factory_cli.rs`, `crates/factory-supervisor/tests/api_write.rs`
- Modify: `crates/factory-supervisor/src/api/mod.rs` and `http.rs` (routes, `ApiDeps.cli`), `src/lib.rs`, `src/bin/factoryd.rs`, `README.md`
- Test: unit tests in each new file, plus `tests/api_write.rs`

**Interfaces:**
- Consumes: Task 1 router and `ApiDeps`; `Request::new`, `queue::enqueue`; the `ProcessSpawner` binary lookup (`FACTORY_BIN`, else next to `factoryd`); `validate_rework_note` rules.
- Produces:
  - `trait FactoryCli: Send + Sync { fn run(&self, project: &str, args: &[String]) -> Result<CliOutput, CliSpawnError>; }`, `CliOutput { code: i32, stdout: String, stderr: String }`, `ProcessFactoryCli::from_env()` (passes `FACTORY_HOME` to the child, 60 second timeout). `ApiDeps` gains `pub cli: Arc<dyn FactoryCli>`.
  - `POST /projects/{p}/jobs` body `{"spec": "/abs/spec.md", "plans": "/abs/plans-folder", "model": "optional", "feature": "optional", "run": true}` runs `factory --project p new ...`, then enqueues `Action::Run` when `run` is true (the default). Response 201 `{"job": "<id>", "requestId": "<id or null>"}`.
  - `POST /projects/{p}/jobs/{id}/approve` enqueues `Action::Approve`, response 202 `{"requestId": "..."}`.
  - `POST /projects/{p}/jobs/{id}/rework` body `{"note": "..."}` and `POST .../cancel` body `{"reason": "...", "force": false}` run the matching `factory` command and return `{"output": "<stdout>"}`.
  - `GET /events?project=<p>`: `text/event-stream`. Each SSE `data:` line is an `events.jsonl` object with an added `"project"` field. The stream first replays the last 50 lines, then follows new ones; with no `project` parameter it streams every project. A `: keepalive` comment is sent every 15 seconds.

- [ ] **Step 1: Failing tests with a recording fake `FactoryCli`**

Router-level tests:
- create job: a valid body runs exactly one `new` with the canonical spec and plans paths, then enqueues one `Run` request; `run: false` enqueues nothing; a non-zero `new` exit returns `failed` with the stderr tail and enqueues nothing.
- refusals before anything runs (the fake records zero calls and the queue stays empty): relative spec path, missing spec file, plans path that is a file, a path containing `..`, extra field, invalid `model`, body over 64 KiB, invalid JSON, and a disabled project (`project_disabled`).
- approve: a stage that is not needs-human or approve returns `conflict`; otherwise one `Approve` request is queued; an unknown job id is `not_found`; job id `1/2` is `bad_request`.
- rework: a note with a newline or over 200 chars is `bad_request`; a valid note runs `rework <id> <note>` through the fake with the note as one argument (no shell).
- cancel: `force` must be a boolean; a live job lease without `force` surfaces the CLI refusal as `conflict`.
Run: `cargo test -j4 -p factory-supervisor api::write` — Expected: FAIL to compile.

- [ ] **Step 2: Implement `FactoryCli`, `ProcessFactoryCli` and the write handlers**

Handlers validate everything first, then call the trait. Arguments are passed as separate `Command` args. Read the real `new` arguments (`slug`, `feature`) from `crates/factory-cli/src/cli.rs` and derive them: slug from the spec's parent folder name, feature from the body or the slug. Do not duplicate stage rules: read the board stage only for the approve pre-check and let the CLI be the authority for everything else.
Run: `cargo test -j4 -p factory-supervisor api::write` — Expected: PASS.

- [ ] **Step 3: Failing tests for the event stream**

In `api/events.rs`: an `EventFollower` over a file path returns the last N lines on open, then only new complete lines (a partial last line is held back), restarts from the start after truncation or replacement, and tags lines with the project. A `Read` adapter produces the SSE bytes and ends when a stop flag is set. One socket test: connect, append a line to a fixture `events.jsonl`, read it back as `data: {...,"project":"p"}`, drop the connection, and check the handler thread ends.
Run: `cargo test -j4 -p factory-supervisor api::events` — Expected: FAIL to compile.

- [ ] **Step 4: Implement the event stream**

Poll every 500 ms with std only. Keep this code in `factory-supervisor` (the dashboard crate is a separate workspace). Open streams count against the 16-handler cap, with their own cap of 8.
Run: `cargo test -j4 -p factory-supervisor` and `cargo clippy -j4 -p factory-supervisor -- -D warnings` — Expected: PASS.

- [ ] **Step 5: End-to-end socket test, README, commit**

`tests/api_write.rs`: real server on port 0 with the fake CLI; create a job, approve it, check the queue files; two projects at once do not interfere. Document the endpoints and the SSE format in `README.md`. Commit: `feat(supervisor): add job write endpoints and the event stream`.

---

### Task 3: Multi-project dashboard

**Files:**
- Modify: `dashboard/src/main.rs`, `server.rs`, `state.rs`, `events.rs` (a per-project `Paths` value replaces the hardcoded `root/.superpowers/factory/...` joins), `dashboard/static/app.js`, `dashboard.html`, `dashboard.css`, `dashboard/README.md`, `dashboard/tests/smoke.sh`
- Create: `dashboard/src/home_mode.rs` (token read, small std HTTP client for the API, project list), `dashboard/static/projects.js`, `dashboard/tests/projects.test.js`
- Test: Rust unit tests in the new and changed modules, `node --test`, `tests/smoke.sh`

**Interfaces:**
- Consumes: the Task 1 and 2 API (`/projects` returns `repo`, `stateDir`, `jobsDir`, `status`, `reason`; the `POST` endpoints) and the token file `<home>/api.token`.
- Produces:
  - `factory-server --home <dir> --api <host:port> --port 8765`. `--root` keeps today's single-project behavior unchanged. `--home` and `--root` are mutually exclusive.
  - Dashboard routes in home mode: `GET /api/projects`, `GET /api/projects/{p}/state` (the existing `StateView` for that project), `GET /api/projects/{p}/jobs/{id}/park`, `POST /api/projects/{p}/jobs`, `POST /api/projects/{p}/jobs/{id}/approve|rework|cancel`. Writes are forwarded to the factoryd API with the token added by the dashboard server.
  - Write routes require an allowed `Host`, an `Origin` header equal to `http://<host header>` (missing or different is refused) and `Content-Type: application/json`. A refused write never reaches factoryd.

- [ ] **Step 1: Failing Rust tests for `Paths` and home mode**

`Paths::for_root(root)` equals today's joins (`root/.superpowers/factory/board.json` and the rest); `Paths::from_project(state_dir, jobs_dir)` uses the API-provided folders; all existing dashboard tests still pass with `--root`. Home-mode tests against a fake API (a `tiny_http` thread on port 0): `/api/projects` merges the API list; a project the API marks `disabled` shows its reason and its state view is empty; the token is sent as `Authorization: Bearer ...` and never appears in any response body or header; an API 401 shows a clear "token rejected" error to the page. Write routes: a wrong or missing `Origin`, a wrong `Content-Type` and a non-allowed `Host` are refused and the fake API records zero calls; a valid write is forwarded unchanged.
Run: `cargo test -j4 --manifest-path dashboard/Cargo.toml` — Expected: FAIL to compile.

- [ ] **Step 2: Implement `Paths` and the home mode**

Replace the literal joins in `server.rs`, `state.rs` and `events.rs` with one `Paths` value per project, and keep one tailer `Registry` per project keyed by name. `home_mode.rs` holds the minimal HTTP/1.1 client (`TcpStream`, `Connection: close`, 5 second timeouts, response body cap 4 MiB).
Run: `cargo test -j4 --manifest-path dashboard/Cargo.toml` — Expected: PASS.

- [ ] **Step 3: Failing JS tests for the project switcher and the new-job form**

`tests/projects.test.js` (node test runner, pure functions in `projects.js`): `renderProjectTabs(projects)` marks disabled projects with their reason and orders by name; `buildCreateBody(form)` returns `{spec, plans, model?, run}` and refuses a relative path or an empty field with a message; `isSafeId` accepts digits only; the selected project survives a poll refresh.
Run: `node --test dashboard/tests/projects.test.js` — Expected: FAIL.

- [ ] **Step 4: Implement the UI**

`app.js` gains a project tab strip (only when `GET /api/projects` answers 200, which means home mode), switches the polled URL to `/api/projects/{p}/state`, adds a "New job" panel (spec path, plans path, optional model, run checkbox) and Approve, Rework and Cancel buttons on jobs in the matching stages. Errors from the API show as a banner with the stable error code. The page never holds a token. In home mode the page title is "Software factory". The dashboard keeps polling; `GET /events` is for other clients.
Run: `node --test dashboard/tests/` — Expected: PASS.

- [ ] **Step 5: Smoke test with a real `factoryd`, docs, commit**

Add `dashboard/tests/smoke-home.sh`: temp `FACTORY_HOME`, `factory project add` for two temp repos, `factoryd run` with `listen = "127.0.0.1:0"` style config on a free port and `FACTORY_BIN` pointing at a stub, `factory-server --home`, then `curl` checks: `/api/projects` lists both, a write with a bad `Origin` is refused, a valid create reaches the queue. Update `dashboard/README.md`, the root `README.md` and `CLAUDE.md` (the dashboard crate is no longer untouched). Mark plan 6 done in the plan index. Commit: `feat(dashboard): show every project and create jobs through the API`.

Run before the final commit: `cargo test -j4 -p factory-supervisor`, `cargo test -j4 -p factory-cli --test no_hardcoded_paths`, `cargo test -j4 --manifest-path dashboard/Cargo.toml`, `node --test dashboard/tests/`, `bash dashboard/tests/smoke-home.sh` — Expected: all PASS.
