# rocket-factory

A standalone software factory. A Rust driver builds, checks, reviews and merges one feature at a time in a separate git worktree, and a local dashboard shows what it is doing. It was extracted from Rocket's `tools/factory` and is being generalized to drive several projects at once.

- Design: `docs/superpowers/specs/2026-10-16-standalone-software-factory-design.md`
- Build plans: `docs/superpowers/plans/2026-10-16-standalone-software-factory/`
- Contributor rules and commands: `CLAUDE.md`

Status: extracted and tested. It still behaves like Rocket's in-repo factory. The standalone work is described in the plan index.

## Workflow

How one job moves from the `/factory` command to a merge. Yellow steps spend LLM tokens, teal steps are the Rust driver, which spends none, and orange marks a stop for a human.

![One job, start to merge](docs/images/factory-lifecycle.svg)

The order of the gates a job passes, and what each one does when it fails:

![What can stop a merge, in order](docs/images/factory-gates.svg)

The processes behind a job:

![Who talks to whom](docs/images/factory-processes.svg)

The same three diagrams as one page: [Rocket Factory Workflow](https://claude.ai/code/artifact/64fc3640-5deb-4003-a67d-60b5fc29f80b) (a private Claude artifact, so only its owner can open it). The page source is `docs/factory-workflow.html` and the images are in `docs/images/`.

## Features ported from Rocket

Rocket's factory gained these after the extraction. They are part of this driver now:

- Batch committer: one committer run per job writes all the commits (`commit_batch`, default on), with recovery after a stop.
- Task excerpts: each task builder sees only its own task from the plan and spec (`builder_excerpt`, default on). A stale rework note stays out of fix rounds.
- Ticket hygiene: the `factory tickets` command and the `tickets_on_merge` key, see below.
- Live token bar: the dashboard card bar follows the running agent, so it moves while an agent works.

Presets already seeded under `~/.factory/presets/` are not updated by `factory project add`: `preset::seed` writes only the files that are missing and never overwrites one. To bring them up to date, run `factory presets refresh`, see below.

## No-check and no-test gates

The driver parks a job whose code diff ran no checks (`no-checks`), and can park one whose code diff adds no test
(`no-tests`). A diff of only `*.md`, `docs/**` or `.claude/**` paths is never code. A risky diff is left to the
merge decision and ends as `risky-diff`. These settings live in `config.toml` or in `[settings]` of
`project.toml`. A non-empty list replaces its default, and an empty list means the default.

| Setting | Default | Meaning |
| --- | --- | --- |
| `code_paths` | empty: every path that is not a doc path | Globs that count as code. |
| `test_paths` | `**/tests/**`, `**/__tests__/**`, `**/*.test.ts`, `**/*.test.tsx`, `**/*.spec.ts`, `**/*_test.rs`, `**/*_test.go` | Globs that count as a test file. |
| `test_markers` | `#[test]`, `#[tokio::test]`, `#[cfg(test)]`, `describe(`, `it(`, `test(`, `func Test` | Text on an added line that counts as a test. |
| `require_tests` | `false` | `true` parks a code diff with no test path and no marker. `false` only warns the reviewer. |

The ledger has one line `checks ran: <commands>` (or `none`) before the auto-merge line.

## Prompt cache lifetime

`cache_ttl` (`"5m"` or `"1h"`, a normal setting in `config.toml` or in `[settings]` of `project.toml`) sets the prompt
cache lifetime of every agent run. `factory project add` writes `cache_ttl = "5m"` into the new project's `[settings]`. Unset leaves the choice to `claude`, which gives a subscription
login the 1 hour cache. `"5m"` sets `FORCE_PROMPT_CACHING_5M=1` and `"1h"` sets `ENABLE_PROMPT_CACHING_1H=1` on each
`claude -p` child. A 1 hour write costs twice a plain input token and a 5 minute write 1.25 times, so `"5m"` suits
runs that rarely resume. `"1h"` does not override a `FORCE_PROMPT_CACHING_5M` exported in the driver's own
environment. Each `usage.jsonl` line has `cache_creation_1h`, the 1 hour part of `cache_creation`, to check the
effect.

## Logging

The driver writes a JSON log to `<state_dir>/log.jsonl` and `factoryd` to `<home>/factoryd.log.jsonl`. `factory log [--job <id>] [--level <lvl>] [--tail <n>]` reads it. `log_level` in `project.toml` and `FACTORY_LOG` set the level; see `docs/software-factory.md` (Logging).

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

## Draft cards

A draft card tracks a spec that is still being written, before any job exists. It lives in the board's `drafts` list.

- `factory spec start <slug> '<feature>'` creates a draft at the `interviewing` stage. The slug goes through the same slugify as job names. A slug with no letters or digits is refused, and so is a slug of only digits (`a draft slug cannot be only digits`), because `factory cancel 12` means job 012.
- `factory spec stage <slug> <interviewing|writing-spec|writing-plans>` moves the card to another stage.
- `factory spec resume <slug>` prints the `draft`, `stage` and `file <spec path> (exists|missing)` lines.
- `factory cancel <slug>` removes the card and keeps the spec file. A numeric argument still means a job.
- `factory status` lists the cards in a `drafts:` block.
- `factory new` adopts the draft with the same slug and removes it. When no list item of the spec's `## Decisions` section says `Decided by: user`, or the spec has no `## Decisions` section at all, it prints `warning: no decision in this spec was answered by the user` after `created job ...`. The job still runs. A single plan file with no spec text gets no warning.
- The dashboard shows each draft in the Spec column. A card not updated for 24 hours gets a stale marker (dashboard `--draft-stale-hours`, factoryd `draft_stale_hours`).
- An older `factory` binary that rewrites the board drops `drafts`.

## Plan lint

`factory new` lints every plan on the chain before it creates a job. A plan with `TBD`, `TODO`, `FIXME` or `???`
outside backticks and code fences is refused, the error names the plan file and line, and no job is made. Other
findings only warn: a header without Goal, Architecture, Tech Stack, Spec, Global Constraints or Review Focus, a
header that does not name `superpowers:subagent-driven-development` as the executor, a task without `**Files:**`
or `**Interfaces:**`, and a commit step. The warnings print as `warning: ...` lines after `created job ...`,
after the spec's own warning, and the job still runs.

## Stronger reviewer model

- `reviewer_model_large` (default unset, which turns the switch off) names a stronger model for the reviewer.
- `reviewer_large_over_lines` (default `800`, must be above zero) is the size threshold.
- The review uses the large model when the whole job diff is risky, or when this round's diff has more changed
  lines than the threshold. A diff exactly at the threshold uses `reviewer_model`. The choice is written to the
  ledger as `review model: <name> (<reason>)`.
- The weighted-token figure ignores the model, so a stronger model's extra cost does not show in `factory tokens`.

## One job per spec

By default a spec with several plans becomes one job per plan. `factory new ... --plans <dir> --one-job` makes
one job for the whole spec instead. The driver combines the plans. Plans are still written with at most 3 tasks
each, but the driver does not enforce that cap on this path. The 6 plan cap and the chain rules (`Next plan:` lines)
still apply. The flag needs `--plans`, and with a single plan it has no effect and
`new` prints a `note:` line.

- The estimate is `110K + 100K per task` in weighted tokens, compared with `budget_weighted`.
- Over budget, `new` creates nothing, prints a `needs consent:` line with the estimate and the budget on stderr and
  exits non-zero. Raise `budget_weighted`, or run the same command again with `--accept-over-budget` (it needs
  `--one-job`).
- At or under budget, `new` prints a `note:` line with the estimate and the budget. With `--accept-over-budget` the
  job is created, the over-budget estimate is stored as a job warning and `new` prints it as a `warning:` line.
- The `/factory` command passes `--one-job` when the feature starts or ends with that token, and asks the user
  before it adds `--accept-over-budget`.

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

### Refreshing seeded presets

`factory presets refresh [--force]` brings `<home>/presets/` up to the text built into the binary. It needs no
repository. Run it after you upgrade the factory, because a home seeded by an older version keeps the old
builder and reviewer agents, and the driver now expects the new builder reply format.

For each built-in preset file the command does one of four things and prints a line for it, then a count:

- `written`: the file was absent, so it is created.
- `unchanged`: the file already has the built-in text.
- `updated`: the file holds a known older shipped text of the builder or reviewer agent, so it is replaced.
- `refused`: the file differs from every known text, so it looks hand edited. It is listed and left as it is.

Refused files do not make the command fail. To overwrite them, run `factory presets refresh --force`. This
discards your edits, so copy them first.

The command only writes under `<home>/presets/`. It never touches a project override under
`<home>/projects/<name>/` or a repo override under `.factory/`.

Any home whose agent files differ from both the shipped text and the previous release text is refused until
`--force`. That includes homes seeded by a build between releases, such as one from before the task excerpt
change. The `refused` lines name each such file: compare it with the shipped text, which `factory presets refresh`
writes when `FACTORY_HOME` names an empty folder, keep your own edits, then run `--force`.

`refresh` does not update the global command. After an upgrade also run `factory install --force` to pick up
the new `/factory` command.

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

### Approving a parked job

`factory approve <id>` merges a job in `needs-human` or `approve`. It refuses a `needs-human` job whose code review
has no verdict yet (pending, running or absent), because the checks never passed or the reviewer never ran:
`job 020 has no code review yet (...): use rework, or approve --without-review to merge anyway`. A job parked
by the reviewer (risky diff, changes remain) has a verdict and approves as before. `factory approve <id>
--without-review` merges anyway and the ledger says `approved by the user without a review`. The park hints in
`park.json` and `factory status` offer that flag, not a plain approve, when the review has not run. When a fix round
that started from failed checks changes nothing, the driver runs the checks once more: if they pass the job goes
to the reviewer, if they still fail it parks as `checks-failed`. The board field `fix_source` (`checks` or
`review`) records what started the latest fix round.

### The project lease

At most one job is active per project. A running `run` or `approve` holds a lease file in the project's state
folder and renews it while it works. `factory status` starts with `project: free`, or with
`project: busy, job <id> (owner <who>, <n> s left)` while another process holds it. The busy message means that
process owns the project for about that many more seconds: wait for it, or stop it. A lease of a crashed process
expires on its own (after up to 30 minutes), and an expired or corrupt lease file is taken over. A lease whose
owner is a `driver-<pid>-<secs>` process that no longer runs on this machine counts as stale at once (Linux and macOS).
If the process that held it is gone and the lease is still shown as busy, `factory cancel <id> --force` releases
the lease of that job, or delete `<state folder>/project.lease`. `project remove` refuses while the lease is held,
`--force` included.

A running driver renews both the project lease and the job lease every 60 seconds from a background thread, so a
long agent step does not let them expire. If a renewal finds another owner holding the lease, the run stops at the
start of its next loop iteration with the busy error. `kb-refresh` takes the project lease like `run` does, so a
busy project refuses it with the same message.

The dead-owner check looks up the pid in this machine's process table, through one library (`sysinfo`) on both
Linux and macOS. A state folder shared through a container bind mount or a network filesystem can show a live
owner as gone, so run the drivers of one project from one machine.
A hung agent that keeps producing output keeps the project locked as long as its driver process lives. Only
`stall_minutes` covers a silent one.

### factoryd

`factoryd` is a small supervisor that runs `factory` for queued requests, so several projects can work in
parallel. It lives in the factory home (`$FACTORY_HOME`, else `~/.factory`):

- `queue/` holds one JSON file per waiting request.
- `results/` holds one JSON file per finished request (`Merged`, `NeedsHuman`, `Failed`, `Rejected` or `Lost`).
- `run/` holds the child records of running jobs, one per project.
- `logs/<project>/<request>.out` and `.err` hold the output of each child.
- `config.toml` may set `max_parallel` (how many projects run at once), `listen` (the API address),
  `daily_budget_weighted` (see below) and `draft_stale_hours` (hours without a change before an interview draft
  counts as stale, default 24, a whole number above 0). The file rejects unknown keys, so an older `factoryd`
  refuses a config that has `draft_stale_hours`.

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
than a minute) is treated as a stranger that reused the number, so the child counts as gone. The start time
comes from the same library, to the second. When it is unknown, the recorded pid is trusted.

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
unchanged. It is `budget_weighted` in the project config, default 1,000,000 weighted tokens per job: the agent run warns at 80 percent and stops at 100 percent.

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
| `GET /projects/{p}/jobs` | `{jobs, drafts, queued, lastResults}` from the board, the queue and the result records. `drafts` is `[{slug, stage, updatedAt, stale}]` for the interview drafts on the board (`stale` once a draft is older than `draft_stale_hours`) |
| `GET /usage?days=N` | `{days, projects: [{name, total, perDay: [{day, tokens}]}], dailyBudget?}`, weighted tokens per UTC day, `N` 1 to 90 (default 14). `dailyBudget` is `{limit, used, resetsAt}` (`resetsAt` is an RFC 3339 UTC time) and is present only when `daily_budget_weighted` is set |

Write endpoints. A body is JSON of at most 64 KiB, unknown fields are refused, and everything is checked before a
process starts or a request is queued. Commands that only `factory` implements run as `factory --project <p> <cmd>`
child processes (60 second limit, arguments passed one by one, never through a shell). The binary is found like
for the job children: `FACTORY_BIN`, else next to `factoryd`.

| Endpoint | Body and answer |
|---|---|
| `POST /projects/{p}/jobs` | `{"spec": "/abs/spec.md", "plans": "/abs/plans-folder", "model"?, "feature"?, "run"?, "series"?, "slug"?}`. Both paths must be absolute, exist and hold no `..`. `model` matches `[A-Za-z0-9._-]{1,64}`, `feature` is one line of at most 200 characters (default: the slug). The slug is the plans folder name without a leading `YYYY-MM-DD-` (`plans/2026-10-16-standalone-software-factory` gives `standalone-software-factory`); a `slug` in the body overrides it and must match `[a-z0-9][a-z0-9-]{0,63}`, and a name that gives no slug is `bad_request`. Runs `factory new`, then queues a run unless `run` is `false`. `series` (boolean, default `true`) makes that run continue with the next plans of the series, like `factory run --series`; it only matters when `run` is true. The spec path may be any readable file and is canonicalized once (a swap between the check and the command is accepted, because a token is required). `201 {"job", "requestId"}` (`requestId` is `null` when nothing was queued) |
| `POST /projects/{p}/jobs/{id}/approve` | `{"without_review"?}` (a boolean, an empty body is fine). Needs a job in `needs-human` or `approve` that is not cancelled. A `needs-human` job whose code review is pending or absent (the checks never passed or the reviewer never ran) is `409 conflict` with the message `job 020 has no code review yet ...`, unless `without_review` is `true`, which queues `factory approve --without-review`. Queues an approve request. `202 {"requestId"}` |
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

`plan_rules` (optional, default empty) are rules for the plans the `/factory` command writes. The driver puts them in no
prompt. `factory project show <name>` prints them in a `plan rules:` block and the command applies each line to every
plan. A repo override cannot set them.

### Agent files

Agent files are merged by name from the preset `agents/` folder, the project `agents/` folder and the repo's
`.factory/agents/` folder on the base branch (later wins). The engine writes them to `.claude/agents/` in each new
job worktree. Only names like `factory-<name>.md` are written, they are git-excluded, and a file that is tracked
on the base branch is never overwritten.
