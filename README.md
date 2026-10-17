# rocket-factory

A standalone software factory. A Rust driver builds, checks, reviews and merges one feature at a time in a separate git worktree, and a local dashboard shows what it is doing. It was extracted from Rocket's `tools/factory` and is being generalized to drive several projects at once.

- Design: `docs/superpowers/specs/2026-10-16-standalone-software-factory-design.md`
- Build plans: `docs/superpowers/plans/2026-10-16-standalone-software-factory/`
- Contributor rules and commands: `CLAUDE.md`

Status: extracted and tested. It still behaves like Rocket's in-repo factory. The standalone work is described in the plan index.

## Features ported from Rocket

Rocket's factory gained these after the extraction. They are part of this driver now:

- Batch committer: one committer run per job writes all the commits (`commit_batch`, default on), with recovery after a stop.
- Task excerpts: each task builder sees only its own task from the plan and spec (`builder_excerpt`, default on). A stale rework note stays out of fix rounds.
- Ticket hygiene: the `factory tickets` command and the `tickets_on_merge` key, see below.
- Live token bar: the dashboard card bar follows the running agent, so it moves while an agent works.

Presets already seeded under `~/.factory/presets/` are not updated: `preset::seed` writes only the files that are missing and never overwrites an existing one. A preset seeded before this port lacks the batch committer kind and the task excerpt rule. `seed` runs only in `factory project add`. To pick up the new text, delete the preset folder (or just the files you have not edited) and run `factory project add` for any project, which seeds the missing files again.

## Ticket hygiene

After the last plan of a job merges, the driver can file the leftover work as new GitHub tickets, comment on the
parent tickets and close them. The calls go through the `Tracker` port in `factory-engine`. The real tracker runs
the `gh` CLI in the main checkout (the repo from `Layout`); tests use a fake.

- `factory tickets <id> [--dry-run]` runs the step for a merged job. `--dry-run` makes only read calls and
  prints each action. It exits non-zero when a `gh` call failed.
- `tickets_on_merge` (default `false`, a normal setting in `config.toml` or in `[settings]` of `project.toml`)
  runs the same step at the end of the merge.
- The parent tickets come from the feature text given to `factory new`: `#N` and
  `https://github.com/<owner>/<repo>/issues/N` both count. A URL for another repository makes the step skip.
- The remaining work is the `## Follow-ups` section of the job's `spec.md`, one ticket per bullet. A spec with
  follow-ups gives the merge footer `Relates to: #N` instead of `Fixes: #N`.
- `tickets.json` in the job folder records every call, so a rerun never files a ticket twice.
- A failure (no tracker, no `gh`, a failed call) never changes the merge. The job stays `merged`, and the failure
  is written to the ledger, the job output and `events.jsonl`.

## Layout and base branch

The driver gets every path as a value. `Layout` holds them. `Layout::in_repo` gives the layout Rocket uses today.

- `repo`: the main checkout. Default is the repo path itself.
- `state_dir`: holds `board.json`, `events.jsonl`, `costs.json` and `kb-stopped`. Default is `.superpowers/factory`.
- `jobs_dir`: one folder per job. Default is `.superpowers/sdd`.
- `worktrees_dir`: one folder per job worktree. Default is `.claude/worktrees`.
- `config_file`: the factory config. Default is `.claude/factory/config.toml`.
- `commit_context_file`: the context the committer prompt reads. Default is `.claude/factory/commit-context.md`.

The config key `base_branch` is the branch jobs start from and merge into. The default is `main`. A name that git would refuse makes the whole config invalid. So does a name that starts with `factory/`. The driver then falls back to the fail-closed config.

## Projects and presets

The factory keeps its own files in a home folder: `$FACTORY_HOME`, or `~/.factory` when it is not set.

```
<home>/presets/<preset>/{config.toml,commit-context.md,agents/}   seeded from the built-in presets (rust, go)
<home>/projects/<name>/{project.toml,agents/,state/,worktrees/}    one folder per registered project
```

The `project` commands work on the home only, so they run outside a git repository:

```
factory project add /path/to/repo --preset rust [--base main] [--name demo]
factory project list
factory project show demo
factory project remove demo [--force]
factory project which [<path>]
```

`which` prints the name of the registered project whose repo is `<path>` (default: the current directory) or
contains it, and exits 0. When two registered repos nest, the inner one wins. A linked git worktree of a
registered repo resolves to that repo's project. It prints `no registered project contains <path>` to stderr
and exits 1 when nothing matches.

`add` needs the top folder of a git repository and stores the base branch it used (`--base`, or the branch the
repo is on). The name defaults to the lowercased repo folder name. `list` shows a project whose repo is gone as
`disabled`. `show` prints the resolved settings, the folders, the number of risk paths, the agent file names and
any problems; a project whose settings failed closed is marked `degraded`. `remove` deletes only the project
folder and refuses while a job is not merged or cancelled, unless `--force` is given. A project with an unreadable
`project.toml` is removed only with `--force`. `remove` also prunes the git worktree registrations of the
project's `worktrees/` folder (a stale one whose folder is gone too, a valid one together with its working
folder). It never deletes branches: the `factory/*` job branches stay in the repo and are listed in the output.

### The global /factory command

`factory install [--dir <path>] [--force]` writes the `/factory` slash command (embedded in the binary) as
`factory.md` into `<--dir>`, else into the Claude commands folder under `$HOME`; without `--dir` an unset or
empty `$HOME` is an error. The folder is created when absent and the write is atomic. A file with the same
content is left alone (`already installed`); a file that differs is an error naming it, unless `--force`.

In a repository, `/factory` runs `factory project which` to find the project, then passes `--project <name>`
on every `factory` call (`$FACTORY_BIN`, else `factory` on PATH). It runs the brainstorm interview and writes
the spec and the plans (at most 3 tasks each) with explicit approvals, creates the job with `factory new`, and
starts the run: `factoryd enqueue --project <name> --job <id> --series` when `factoryd is-running` exits 0,
else `factory --project <name> run <id> --series` in the background. It does not poll. The checks come from the
project's registry entry. `factoryd is-running` exits 0 while a `factoryd` holds the instance lock of this
home and exits 1 otherwise, with no output.

A project-local `.claude/commands/factory.md` (Rocket has one until plan 8) shadows the global command of the same
name inside that repository.

### Running against a project

`factory --project <name> <command>` runs a command against a registered project, from any folder. It cannot be
combined with `--root`. The settings come from `project.toml`, the preset and the repo override, and the layout
(state, worktrees) lives in the project folder. A project whose repo folder is missing or is not a git repo is
refused outright. A project is blocked when its settings failed closed (`factory project show` lists why), a
check command would be rejected, or `project.toml` has no valid `base_branch`. A blocked project refuses `run`,
`approve` and `kb-refresh`, the commands that start agents or merge.

### The project lease

At most one job is active per project. A running `run` or `approve` holds a lease file in the project's state
folder and renews it while it works. `factory status` starts with `project: free`, or with
`project: busy, job <id> (owner <who>, <n> s left)` while another process holds it. The busy message means that
process owns the project for about that many more seconds: wait for it, or stop it. A lease of a crashed process
expires on its own (after up to 30 minutes), and an expired or corrupt lease file is taken over. A lease whose
owner is a `driver-<pid>-<secs>` process that no longer runs on this machine counts as stale at once (Linux only).
If the process that held it is gone and the lease is still shown as busy, `factory cancel <id> --force` releases
the lease of that job, or delete `<state folder>/project.lease`. `project remove` refuses while the lease is held,
`--force` included.

A running driver renews both the project lease and the job lease every 60 seconds from a background thread, so a
long agent step does not let them expire. If a renewal finds another owner holding the lease, the run stops at the
start of its next loop iteration with the busy error. `kb-refresh` takes the project lease like `run` does, so a
busy project refuses it with the same message.

The dead-owner check reads `/proc` on this machine only. A state folder shared through a container bind mount or a
network filesystem can show a live owner as gone, so run the drivers of one project from one machine. On a system
without `/proc` (macOS) a recorded pid always looks alive, so an adopted child (see below) is never settled and
its project stays blocked until a person removes the child record under `run/children` in the factory home.
A hung agent that keeps producing output keeps the project locked as long as its driver process lives. Only
`stall_minutes` covers a silent one.

### factoryd

`factoryd` is a small supervisor that runs `factory` for queued requests, so several projects can work in
parallel. It lives in the factory home (`$FACTORY_HOME`, else `~/.factory`):

- `queue/` holds one JSON file per waiting request.
- `results/` holds one JSON file per finished request (`Merged`, `NeedsHuman`, `Failed`, `Rejected` or `Lost`).
- `run/` holds the child records of running jobs, one per project.
- `logs/<project>/<request>.out` and `.err` hold the output of each child.
- `config.toml` may set `max_parallel` (how many projects run at once), `listen` (the API address) and
  `daily_budget_weighted` (see below).

```
factoryd enqueue --project myproj --job 003        # add a request (--series, --approve)
factoryd run                                       # the loop (--once does one tick)
factoryd status                                    # queue, children, last results
factoryd stop                                      # ask the loop to end after its tick
```

At most one child runs per project, and the project lease still decides who may work. `FACTORY_BIN` names the
`factory` binary to start, else the one next to `factoryd` is used.

Stopping `factoryd` never stops jobs: children run in their own process group. A restarted `factoryd` adopts
them by their recorded pid and finishes them when they end. A pid that started far from the recorded time (more
than a minute) is treated as a stranger that reused the number, so the child counts as gone. This check reads
`/proc` and assumes 100 clock ticks per second, so it works on Linux only.

A request ends as `Lost` when `factoryd` cannot tell how it ended: the child died without a result line, or the
request was started before a crash but left no child record (its log files exist). A lost job is never restarted
by `factoryd`. Read the `.out` and `.err` logs and the job ledger, then run `factory run <job>` again by hand or
enqueue a new request. A project whose lease belongs to a live driver that is not a child of `factoryd` (for
example a manual `factory run`) is reported as busy and nothing starts for it until that process ends.

#### The daily budget

`daily_budget_weighted = 5000000` in `config.toml` limits all projects together for one UTC day. The unit is
weighted tokens (cache reads count one tenth), summed over the `usage.jsonl` of every job of every registered
project; the day is the UTC day, the same as `GET /usage`. Without the key there is no limit; `0`, a negative
number or a non-integer makes `config.toml` invalid, and `factoryd` then refuses to start. The per-job budget is
unchanged.

Once today's use reaches the limit, new runs wait: a `Run` request stays in the queue (nothing is rejected, lost
or reordered) and starts on the first tick after the budget is raised in `config.toml` or the UTC day changes.
Approvals are never held, running jobs are never stopped, and a waiting run never blocks a request behind it.
`factoryd` reads `config.toml` on every tick; if the file becomes invalid while it runs, it keeps the last valid
value and prints one line. `factoryd` also prints one line when the limit is reached and when runs may start again.
Without a running `factoryd`, the `/factory` command falls back to `factory run` in the background, and that path
does not check the daily budget.
`factoryd status` shows `daily budget: <used> of <limit> weighted tokens today (<pct>%), resets at <time>`, with
`warning` from 80% and `reached: new runs are waiting` from 100%.

#### The local API

`factoryd run` also serves a small JSON API for the dashboard and other local tools.

- The token is a random secret in `<home>/api.token` (mode `0600`). It is created when absent and never rewritten.
  Send it as `Authorization: Bearer <token>`. `factoryd` never prints it.
- `listen` in `config.toml` sets the address, default `127.0.0.1:8766`. Only loopback addresses are accepted
  (port `0` picks a free port). A bind failure stops `factoryd` before it takes any job.
- The `Host` header must name `127.0.0.1`, `localhost` or `[::1]`, else the answer is `400 bad_request`.
- At most 16 requests are served at once; one more gets `409 conflict` ("busy").

| Endpoint | Answer |
|---|---|
| `GET /projects` | `[{name, repo, preset, baseBranch, status, reason, stateDir, jobsDir, activeJob}]`, `status` is `ready`, `busy` or `disabled` |
| `GET /projects/{p}/jobs` | `{jobs, queued, lastResults}` from the board, the queue and the result records |
| `GET /usage?days=N` | `{days, projects: [{name, total, perDay: [{day, tokens}]}], dailyBudget?}`, weighted tokens per UTC day, `N` 1 to 90 (default 14). `dailyBudget` is `{limit, used, resetsAt}` (`resetsAt` is an RFC 3339 UTC time) and is present only when `daily_budget_weighted` is set |

Write endpoints. A body is JSON of at most 64 KiB, unknown fields are refused, and everything is checked before a
process starts or a request is queued. Commands that only `factory` implements run as `factory --project <p> <cmd>`
child processes (60 second limit, arguments passed one by one, never through a shell). The binary is found like
for the job children: `FACTORY_BIN`, else next to `factoryd`.

| Endpoint | Body and answer |
|---|---|
| `POST /projects/{p}/jobs` | `{"spec": "/abs/spec.md", "plans": "/abs/plans-folder", "model"?, "feature"?, "run"?, "series"?, "slug"?}`. Both paths must be absolute, exist and hold no `..`. `model` matches `[A-Za-z0-9._-]{1,64}`, `feature` is one line of at most 200 characters (default: the slug). The slug is the plans folder name without a leading `YYYY-MM-DD-` (`plans/2026-10-16-standalone-software-factory` gives `standalone-software-factory`); a `slug` in the body overrides it and must match `[a-z0-9][a-z0-9-]{0,63}`, and a name that gives no slug is `bad_request`. Runs `factory new`, then queues a run unless `run` is `false`. `series` (boolean, default `true`) makes that run continue with the next plans of the series, like `factory run --series`; it only matters when `run` is true. The spec path may be any readable file and is canonicalized once (a swap between the check and the command is accepted, because a token is required). `201 {"job", "requestId"}` (`requestId` is `null` when nothing was queued) |
| `POST /projects/{p}/jobs/{id}/approve` | No body. Needs a job in `needs-human` or `approve` that is not cancelled. Queues an approve request. `202 {"requestId"}` |
| `POST /projects/{p}/jobs/{id}/rework` | `{"note": "one line, 200 characters at most"}`. Runs `factory rework`. `200 {"output"}` |
| `POST /projects/{p}/jobs/{id}/cancel` | `{"reason"?, "force"?}` (`force` is a boolean, an empty body is fine). Runs `factory cancel`, never a signal. A refusal by the command, such as a live job lease without `force`, is `409 conflict`. `200 {"output"}` |
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

#### The dashboard across projects

`factory-server --home <factory home> [--api 127.0.0.1:8766] --port 8765` shows every registered project in one
page, with a tab per project. It reads the token from `<home>/api.token` on the server and calls the API for the
project list and every write, so the browser never sees the token. `--api` must be a loopback address and
defaults to `127.0.0.1:8766`. The board, the event log, the costs file and the job folders of a project come from
the `stateDir` and `jobsDir` the API reports. `--home` and `--root` cannot be combined, and `--root` keeps its
single-project behavior. The page can create a job (spec, plans folder, optional model and slug, "Run now" and
"Run as series") and approve, rework or cancel a job. A write needs an allowed `Host`, an `Origin` equal to
`http://<Host>` and `Content-Type: application/json`, else the dashboard refuses it before factoryd is called.
Errors show as a banner with the API's error code (`token_rejected` when factoryd refuses the dashboard's token).

### Rust preset checks

The rust preset has two check rules. `crates/*/**` runs `cargo check -j4 -p {crate}` and `cargo test -j4 -p {crate}`
for each changed workspace member. A second rule covers a single-crate repo: `src/**`, `tests/**`, `benches/**`,
`examples/**` and `build.rs` at the repo root run `cargo check -j4` and `cargo test -j4`. These paths are anchored
at the root, so a member's `crates/x/src/lib.rs` matches only the first rule. A virtual workspace (a root
`Cargo.toml` without a `[package]`) relies on the `crates/*/**` rule, because a bare `cargo check -j4` there would
check the whole workspace.

### Layers

Settings come from three layers, each on top of the one before:

1. the preset `config.toml`,
2. the `[settings]` table in the project's `project.toml`,
3. the repo override `.factory/override.toml`, read from the tip of the base branch (never the working tree).

A repo override may only add `risk_paths` and `checks`. Both are additive. Any other key, or an unreadable file,
makes the config fail closed. `.factory/**` is always a risk path, so a repo cannot change its own factory
settings without review.

The four prompt keys are `read_first` (files the builder reads first), `builder_rules`, `check_hints` and
`reviewer_rules`.

### Agent files

Agent files are merged by name from the preset `agents/` folder, the project `agents/` folder and the repo's
`.factory/agents/` folder on the base branch (later wins). The engine writes them to `.claude/agents/` in each new
job worktree. Only names like `factory-<name>.md` are written, they are git-excluded, and a file that is tracked
on the base branch is never overwritten.
