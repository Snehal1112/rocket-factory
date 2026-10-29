# Factory command manual for developers and administrators

This manual covers the `factory` driver and the optional `factoryd` supervisor. It describes the commands in the current checkout. Use `factory <command> --help` or `factoryd <command> --help` to check the installed binary on another machine.

The examples use a registered project named `rocket-api`, job `001`, and a factory home at `~/.factory`. Replace those values with yours. A job normally represents one implementation plan; `new --one-job` can combine several plans. A series runs jobs in order. The driver merges locally and never pushes your repository. Relative plan and spec paths are resolved from your shell's current directory, even with `--project`.

## 1. Prepare the tools

Build from the repository root (Rust builds in this repository use `-j4`):

```sh
cargo build -j4 -p factory-cli -p factory-supervisor
```

The binaries are `target/debug/factory` and `target/debug/factoryd`. Put them on your `PATH`, or invoke those paths directly. Set `FACTORY_HOME` to use a different home; otherwise both commands use `~/.factory`. Register a Git repository before creating a draft or job. See [Getting started](../guides/getting-started.md) for installation and the first run, and [project.toml reference](project-toml-reference.md) for project settings, checks, agent provider, and model defaults.

## 2. Developer workflow

1. Register the repository with `factory project add`, and select it with `--project` for later commands.
2. Start a spec draft with `factory spec start`, complete its interview, and stage the approved spec and plans.
3. Create jobs with `factory new`, then run a job directly or enqueue it through `factoryd`.
4. Inspect `factory status` and `factory log`. Resolve a parked job with `factory rework`, `factory approve`, or `factory cancel`.
5. Inspect merged work and push the local base branch yourself.

```sh
factory project add /path/to/rocket-api --preset rust
cd /path/to/rocket-api
factory --project rocket-api spec start add-search 'Add search to the API'
factory --project rocket-api status
factory --project rocket-api new add-search 'Add search to the API' --plans docs/superpowers/plans/2026-10-22-add-search --spec docs/superpowers/specs/2026-10-22-add-search-design.md
factory --project rocket-api run 001 --series
factory --project rocket-api status
```

`--project` selects a registered project. A provider such as Codex is selected in the project's `agent_cli` setting or during registration with `--agent-cli codex`. `new --model` selects a model for the job; it does not change the provider.

## 3. Administrator workflow

1. Check registrations and configuration with `factory project list` and `factory project show`.
2. Configure `<home>/config.toml`, then start one `factoryd run` process for that home.
3. Enqueue work with `factoryd enqueue`, and monitor with `factoryd status` and `factory status`.
4. Stop scheduling with `factoryd stop`; let running jobs finish before replacing the binary or changing supervisor settings.
5. Preview upgrades with `factory upgrade --dry-run`, then run `factory upgrade` and restart the supervisor.

```sh
factory project list
factoryd is-running
factoryd enqueue --project rocket-api --job 001 --series
factoryd status
factoryd stop
factory upgrade --dry-run
```

## 4. `factory` command reference

Options shown in square brackets are optional. Every command below accepts `-h, --help`. The top level accepts `-V, --version` and `factory help [command ...]`, for example `factory help project add`. The `spec`, `project`, and `presets` groups require a subcommand.

### Selection flags and scope

| Flag | Meaning | Example |
|---|---|---|
| `--project <name>` | Select a registered project in the factory home. | `factory --project rocket-api status` |
| `--root <path>` | Select a Git repository directly and use its legacy repository-local state layout. Without either selection flag, the repository containing the current directory is used. | `factory --root /path/to/repo status` |

The two selection flags conflict. They can appear before or after a subcommand. `project`, `presets`, `install`, and `upgrade` are home-wide operations: their parser accepts these global flags, but their implementation does not use them. Leave the flags off those commands. Most job commands normalize IDs (`7` resolves to `007`); `log --job` filters exact stored strings, so use the padded ID there.

### Create and run jobs

| Command | Supported arguments and flags | Example and result |
|---|---|---|
| `new <slug> <feature>` | `--plans <file-or-directory>`; `--spec <file>`; `--model <name>`; `--one-job`; `--accept-over-budget` | `factory --project rocket-api new add-search 'Add search #42' --plans docs/superpowers/plans/2026-10-22-add-search --spec docs/superpowers/specs/2026-10-22-add-search-design.md` creates jobs from the plan chain. |
| `run <id>` | `--series` | `factory --project rocket-api run 001 --series` runs job `001` and continues to the next job after a successful merge. Without `--series`, it stops after this job. |
| `status` | No command-specific flags | `factory --project rocket-api status` lists jobs, estimates, the lease, and parked-job next steps. |
| `show <id>` | No command-specific flags | `factory --project rocket-api show 001` prints the job as formatted JSON. |

For `new`, `--spec` and `--one-job` require `--plans`; `--accept-over-budget` requires `--one-job`. `--plans` accepts one plan file or a directory of `plan-*.md` files. A directory needs `spec.md` or `--spec`. A single plan file without `--spec` is accepted without spec validation. A plan chain can contain at most six plans. Normal plans have one to three tasks each, must end with a `Next plan:` pointer (or `Next plan: none`), and cannot contain unresolved placeholders. Every plan file in a directory must belong to that chain. An accompanying spec needs a `## Goal` of at least eight words, an `## Acceptance criteria` section with a list item, and no unresolved placeholders. Normal creation makes one job per plan; later jobs wait for earlier ones. `--one-job` combines the chain into one job, lifts the per-plan task cap, and uses a larger estimate; with one plan it has no effect. It refuses an estimate above the configured budget unless `--accept-over-budget` is present. `--model` saves a job model override; omitting it uses the configured builder model.

`new` without `--plans` creates a build-stage job, but a `plan.md` must be placed in its printed job directory before `run`. The feature text can carry an issue reference such as `#42`, which the ticket workflow uses after merge. `run` starts agents, checks, commits, and possibly a local merge; it can instead park at `needs-human`.

For a multi-plan feature that must stay in one job, the full option form is:

```sh
factory --project rocket-api new add-search 'Add search #42' \
  --plans docs/superpowers/plans/2026-10-22-add-search \
  --spec docs/superpowers/specs/2026-10-22-add-search-design.md \
  --one-job --accept-over-budget --model gpt-5.6-luna
```

Use `--accept-over-budget` only after inspecting the estimate. Its presence permits job creation above the configured budget; it does not grant unlimited agent usage.

### Resolve and clean up jobs

| Command | Supported arguments and flags | Example and result |
|---|---|---|
| `approve <id>` | `--without-review`; `--waive <reason>` | `factory --project rocket-api approve 001` approves an eligible parked job. `--without-review` permits a missing review; `--waive` permits open review changes and records the reason; other merge guards still apply. |
| `rework <id> <note>` | No command-specific flags | `factory --project rocket-api rework 001 'Handle an empty query'` records a one-line builder note and returns the job to build. Run or enqueue it afterward. |
| `cancel <id-or-draft-slug>` | `--reason <text>`; `--force` | `factory --project rocket-api cancel 001 --reason 'Feature withdrawn'` cancels a job. `factory --project rocket-api cancel add-search` removes a draft card while retaining its spec file. |
| `cleanup <id>` | `--discard-commits` | `factory --project rocket-api cleanup 001` removes a merged or cancelled job's worktree and branch when safe. |

`rework` accepts a nonblank, single-line note of at most 200 characters; it does not start the builder. `cancel` defaults to reason `cancelled by user`. `cancel --force` can bypass a live lease, but it does not kill the running process. A merged job cannot be cancelled. Draft cancellation ignores `--reason` and `--force`.

`cleanup` keeps the job folder and usage history. Merged cards remain; cancelled cards are normally removed, except when series siblings or a live lease require retaining them. It refuses dirty worktrees or unmerged commits by default. `cleanup --discard-commits` discards both, so inspect the worktree and branch first. `approve --without-review` overrides only the missing-review guard; it cannot override a wrong stage, cancelled job, pending batch snapshot, or project-scope failure.

`approve --waive` takes a one-line reason of at most 200 characters and never combines with `--without-review`. It writes a ledger line, an `operator.approve` event with `waived = true` and an entry in the job's `waivers.json`, then files one ticket per waived blocker after the merge, whether or not `tickets_on_merge` is set. Over the API, send `{"waive_reason": "..."}` to `POST /projects/{p}/jobs/{id}/approve`; a missing reason answers 409 with the refusal, and the supervisor passes it to the child as `--waive=<reason>`.

```sh
factory --project rocket-api approve 001 --without-review
factory --project rocket-api approve 001 --waive 'known flake'
factory --project rocket-api cancel 001 --reason 'Feature withdrawn' --force
factory --project rocket-api cleanup 001 --discard-commits
```

### Spec drafts

| Command | Supported arguments and flags | Example and result |
|---|---|---|
| `spec start <slug> <feature>` | No command-specific flags | `factory --project rocket-api spec start add-search 'Add search'` creates an interviewing draft card and prints the expected spec path. It does not write the spec. |
| `spec stage <slug> <stage>` | Stage: `interviewing`, `writing-spec`, or `writing-plans` | `factory --project rocket-api spec stage add-search writing-spec` updates the draft stage. |
| `spec resume <slug>` | No command-specific flags | `factory --project rocket-api spec resume add-search` prints the current stage and spec location. |

Draft slugs must contain an ASCII letter or digit and cannot be all digits. `cancel <draft-slug>` removes the draft card when the interview is abandoned.

### Projects and presets

| Command | Supported arguments and flags | Example and result |
|---|---|---|
| `project add <path>` | Required `--preset <name>`; optional `--base <branch>`, `--name <name>`, `--agent-cli <claude\|codex>` | `factory project add /path/to/rocket-api --preset rust --name rocket-api --base main --agent-cli codex` registers a repo or subfolder. |
| `project list` | No command-specific flags | `factory project list` lists registrations and disabled reasons. |
| `project which [path]` | Optional path; defaults to current directory | `factory project which /path/to/rocket-api/src` prints the containing registered project. |
| `project show <name>` | No command-specific flags | `factory project show rocket-api` prints resolved configuration, paths, rules, and problems. |
| `project remove <name>` | `--force` | `factory project remove rocket-api` deletes the project's factory-home directory after safety checks. |
| `presets refresh` | `--force` | `factory presets refresh` refreshes unmodified seeded presets. `--force` overwrites hand-edited seeded presets. |

`project add` defaults the base to the current branch and the name to the lowercased final folder name of the supplied path. The project name must be 1–40 lowercase letters, digits, or hyphens and start with a letter or digit. An omitted `--agent-cli` uses Claude. `--agent-cli codex` seeds Codex model settings; see the [provider configuration](project-toml-reference.md) before changing models later.

`project remove` deletes the project's factory state, jobs, and worktrees; it leaves the source repository, presets, and job branches. `--force` bypasses unfinished jobs or broken configuration, but cannot bypass a live project lease. Dirty or unmerged work in the removed worktrees can be lost. `presets refresh` leaves hand edits alone unless forced; refused files are reported without failing the command.

```sh
factory project remove rocket-api --force
factory presets refresh --force
```

### Installation and upgrade

| Command | Supported arguments and flags | Example and result |
|---|---|---|
| `install` | `--dir <directory>`; `--force` | `factory install` installs the Claude command and Codex skill. `factory install --dir /tmp/factory-commands` installs only `factory.md` in that directory. `--force` overwrites differing files. |
| `upgrade` | `--dry-run` | `factory upgrade --dry-run` previews changes without writing. `factory upgrade` updates the command and skill, refreshes eligible presets, reports shadowing, and stamps the home version. |

`install` leaves identical files alone. A differing Claude command is refused without `--force`; a differing Codex skill is kept with a note so the Claude command can still install. Other Codex installation errors return a failure after the Claude command is installed. Forced replacement has no backup. `factory install` is what installs the Codex skill in the first place. `upgrade` refreshes the Codex skill only when `SKILL.md` already exists and never creates a missing one. It saves each replaced command or skill file to the first free of `<file>.bak`, `<file>.bak.1`, `<file>.bak.2`, and so on, so an existing backup is never overwritten. If the Codex skill update fails, the Claude upgrade can still complete, but the command returns an explicit partial-upgrade error. Upgrade the supervisor binary and restart `factoryd` when active child runs have finished.

```sh
factory install --dir /tmp/factory-commands --force
factory upgrade --dry-run
factory upgrade
```

### Usage, logs, tickets, and knowledge base

| Command | Supported arguments and flags | Example and result |
|---|---|---|
| `tokens <id>` | No command-specific flags | `factory --project rocket-api tokens 001` prints token usage by agent role for that job. |
| `tokens --cache` | `--days <positive-integer>` | `factory --project rocket-api tokens --cache --days 7` prints project cache, resumed-run, and cold-start figures for seven days. `--days` requires `--cache`; ID and `--cache` are exclusive. |
| `costs` | No command-specific flags | `factory --project rocket-api costs` prints job and size-group figures and writes `costs.json`. |
| `log` | `--job <id>`; `--level <error\|warn\|info\|debug\|trace>`; `--event <name-or-prefix>`; `--tail <unsigned-integer>` | `factory --project rocket-api log --job 001 --level warn --tail 50` reads structured logs. `factory --project rocket-api log --event task` keeps `task.start` and `task.end`: an event matches when it equals the value or starts with it plus a dot, and an empty value is refused. The filters combine, and `--tail` applies last. Level is case insensitive. The default tail is 100; `--tail 0` reads all lines. |
| `tickets <id>` | `--dry-run` | `factory --project rocket-api tickets 001 --dry-run` previews GitHub follow-up issues and parent updates. Without the flag, it makes those changes using `gh` for an eligible merged last-plan job. |
| `kb-refresh` | Required `--job <id>` | `factory --project rocket-api kb-refresh --job 001` manually refreshes the knowledge base for a merged job. |

`tickets` succeeds without changes when the job is not an eligible merged last plan or has no parent issue reference. `kb-refresh` takes the project lease and starts an agent to update the configured external KB; it skips when `kb_path` is unset. It guards the main checkout against changes and records tampering if that checkout changes during refresh. A manual refresh ignores the KB stop marker and clears it only after a successful refresh or up-to-date result. Do not run it alongside another driver run. A failed `costs.json` write is reported as a warning after the printed report.

```sh
factory --project rocket-api tokens --cache
factory --project rocket-api log --tail 0
factory --project rocket-api tickets 001
factory --project rocket-api kb-refresh --job 001
```

### Exit behavior

Usage and context errors commonly exit `2`; operational errors commonly exit `1`; a busy project exits `75`. A job parking at `needs-human` is a normal run outcome, so inspect `factory status` rather than interpreting the exit code alone. `factory --help`, `factory --version`, and `factory help <command>` are the quickest checks when binaries and documentation differ.

## 5. `factoryd` command reference

`factoryd` supervises queued runs. Its global options are `-h, --help` and `-V, --version`; each subcommand also accepts `-h, --help`. It reads `FACTORY_HOME` (default `~/.factory`) and runs one instance per home. It starts the `factory` executable named by `FACTORY_BIN`, or the `factory` file beside the running `factoryd` binary.

| Command | Supported arguments and flags | Action and example |
|---|---|---|
| `factoryd run` | `--interval-ms <milliseconds>` (default `2000`, minimum effective value `50`); `--once` | Schedule queued work in a loop. `factoryd run --interval-ms 1000` starts the supervisor. `factoryd run --once` performs one scheduling tick, prints counts, and exits without starting its local API. |
| `factoryd status` | None | Print daily budget if configured, queued requests, running children, the ten latest results, and the supervisor version. Example: `factoryd status`. |
| `factoryd stop` | None | Write a stop marker. The supervisor exits after its current tick; child jobs keep running. Example: `factoryd stop`. |
| `factoryd is-running` | None | Exit `0` when a supervisor holds this home's lock; otherwise exit `1`. It prints nothing. Example: `factoryd is-running`. |
| `factoryd enqueue` | Required `--project <name>` and `--job <id>`; optional `--series`, `--approve` | Queue a run: `factoryd enqueue --project rocket-api --job 001 --series`. Queue approval instead: `factoryd enqueue --project rocket-api --job 001 --approve`. `--series` and `--approve` cannot be combined. |
| `factoryd help [command]` | Optional command name | Show help, for example `factoryd help enqueue`. |

`--project` is a registered project name, not a repository path. Project names use lowercase letters, digits, and hyphens, start with a letter or digit, and have at most 40 characters. `--job` takes a one to six digit ID. `enqueue` prints a request ID on success. A queued request can wait if that project is already running or all parallel slots are occupied. Reaching the daily budget holds new run requests in the queue, but approval requests can still proceed.

`factoryd run` uses `<home>/config.toml`. Its defaults include `max_parallel = 2` and a local API listener at `127.0.0.1:8766`; the listener must be loopback. `run --once` performs real scheduling, can start queued jobs, and exits without waiting for those children. A second long-running instance for the same home fails because the first holds the home lock. `status` is also useful when the supervisor is stopped because it reads saved queue and result records.

### Supervisor operations and safety

```sh
factoryd run --once
factoryd enqueue --project rocket-api --job 001
factoryd status
factoryd stop
```

`factoryd stop` stops scheduling; it does not cancel a child job. To check whether the supervisor is active in a script, use its exit status:

```sh
if factoryd is-running; then
  factoryd status
fi
```

Do not run two supervisors against one home. A changed daily budget is read on the next tick; changed concurrency or listener settings need a supervisor restart. On restart, the supervisor adopts recorded children that are still running. A stopped supervisor leaves active children running. For configuration keys and validation, see [project.toml reference](project-toml-reference.md#not-in-projecttoml).

## 6. Environment and related files

| Setting | Use |
|---|---|
| `FACTORY_HOME` | Factory data home; default `~/.factory`. Use the same value for `factory` and `factoryd`. |
| `FACTORY_BIN` | Explicit `factory` binary for child runs started by `factoryd`. |
| `FACTORY_LOG` | Override the configured log level. |
| `FACTORY_LOCK_WAIT` | Seconds to wait for a project lock; default `15`. |
| `FACTORY_ALLOW_API_KEY=1` | Allow agent API-key billing instead of requiring the normal logged-in session. A project's `allow_api_key` and `api_key_file` settings override it. |
| `<home>/config.toml` | Supervisor concurrency, listener, and daily budget. |
| `<home>/projects/<name>/project.toml` | Repository registration, agent provider, models, checks, and project settings. |
| `<home>/projects/<name>/state/` | Board, jobs, events, and job ledgers. |
| `<home>/logs/<name>/` | Output from runs started by the supervisor. |

The driver owns its board, ledger, and event files. Inspect them for diagnosis; use the commands in this manual to change job state.
