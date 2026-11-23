# Projects, leases and factoryd

Reference for the factory home, registered projects, presets, upgrades, leases and the `factoryd` supervisor. For the `factoryd` HTTP API see `factoryd-api.md`. For every `project.toml` key see `project-toml-reference.md`.

## Layout and base branch

The driver gets every path as a value. `Layout` holds them. `Layout::in_repo` gives the layout Rocket uses today.

- `repo`: the main checkout. Default is the repo path itself.
- `state_dir`: holds `board.json`, `events.jsonl`, `costs.json` and `kb-stopped`. Default is `.superpowers/factory`.
- `jobs_dir`: one folder per job. Default is `.superpowers/sdd`.
- `worktrees_dir`: one folder per job worktree. Default is `.claude/worktrees`.
- `config_file`: the factory config. Default is `.claude/factory/config.toml`.

The config key `base_branch` is the branch jobs start from and merge into. The default is `main`. A name that git would refuse makes the whole config invalid. So does a name that starts with `factory/`. The driver then falls back to the fail-closed config.

## Projects and presets

The factory keeps its own files in a home folder: `$FACTORY_HOME`, or `~/.factory` when it is not set.

```
<home>/presets/<preset>/{config.toml,agents/}   seeded from the built-in presets (rust, go, ruby, csharp, node)
<home>/projects/<name>/{project.toml,agents/,state/,worktrees/}    one folder per registered project
```

The `project` commands work on the home only, so they run outside a git repository:

```
factory project add /path/to/repo --preset rust [--base main] [--name demo]
factory project list
factory project show demo
factory project defaults demo   # or --all: add every missing setting as a commented line
factory project remove demo [--force]
factory project which [<path>]
```

`which` prints the name of the registered project whose folder is `<path>` (default: the current directory) or
contains it, and exits 0. When two registered folders nest, the inner one wins. A linked git worktree of a
registered repo resolves to that repo's project. It prints `no registered project contains <path>` to stderr
and exits 1 when nothing matches, for example at the root of a repo that holds only subfolder projects.

`add` takes the top folder of a git repository or any folder inside one, and stores the base branch it used
(`--base`, or the branch the repo is on). The name defaults to the lowercased folder name of `<path>`.

`list` shows a project whose repo is gone as `disabled`. `show` prints the resolved settings, the folders, the
number of risk paths, the agent file names and any problems; a project whose settings failed closed is marked
`degraded`. `remove` deletes only the project folder and refuses while a job is not merged or cancelled, unless
`--force` is given. A project with an unreadable `project.toml` is removed only with `--force`. `remove` also
prunes the git worktree registrations of the project's `worktrees/` folder (a stale one whose folder is gone too,
a valid one together with its working folder). It never deletes branches: the `factory/*` job branches stay in the
repo and are listed in the output.

### Monorepos

`factory project add /path/to/repo/apps/web --preset rust` registers a subfolder as its own project, with its own
queue, settings, lease and board. `project.toml` keeps `repo` as the git top folder and stores the folder in
`subpath` (`apps/web`). A repo holds either one top-folder project or any number of disjoint subfolder projects:
`add` refuses a folder that equals, contains or sits inside another project's folder, and names that project.
Projects of one repo run side by side, and each has its own job ids.

- Job ids, branches and worktrees. A top-folder project keeps `factory/<id>-<slug>`. A subfolder project uses
  `factory/<project>/<id>-<slug>` and a worktree called `factory-<project>-<id>-<slug>`, so two projects never
  collide. A project only sees its own branches.
- The agent starts in the project folder inside the job worktree (`<worktree>/apps/web`), for the builder, the
  fix builder and the reviewer, with both `claude` and `codex`. The KB refresher is unchanged (the main checkout, or the KB folder under Codex). Risk paths and check mappings are relative to the project folder, and check
  commands run there.
- Scope rule. A job may change only files inside the project folder. After the builder and every fix round the
  driver compares the job's own changes (from the merge base with the base branch) with the folder. A path
  outside it, or a rename across the boundary, parks the job as `needs-human` with the paths named, and
  `factory approve` refuses it, also with `--without-review`. The driver never reverts a file. Use
  `factory rework <id> "<note>"` so the builder removes those files, then run the job again.
- Merge lock. Merges into the base branch take a per-repo lock in the git common dir, so only one project of a
  repo merges at a time. A sync runs in the job worktree, outside the lock. A project that finds the lock held
  waits up to 30 seconds. If it is still held, the job parks with
  `another project is merging into this repository, run factory retry <id>`.
  The OS releases the lock if its holder dies.
- Known limit. The KB refresh safety check fingerprints every ref of the repo, so another project's activity in
  the same repo during a long KB agent run can make a refresh report "tampered". Automatic refreshes for that
  project then stop until a manual `factory kb` succeeds.

## Upgrading

To move to a new factory version:

```
git pull
cargo build -j4 -p factory-cli -p factory-supervisor
factory upgrade --dry-run     # preview, changes nothing
factory upgrade
factoryd stop                 # then start `factoryd run` again
cargo build -j4 --manifest-path dashboard/Cargo.toml
```

Upgrade with no running jobs, because a running job keeps the agent files it started with. The dashboard is
not touched by `factory upgrade`, so rebuild it with the last command.

`factory upgrade` prints, in order:

- the command line: the `/factory` command is updated. A replaced file is saved to the first free name of `factory.md.bak`, `factory.md.bak.1`, `factory.md.bak.2`, and so on, so an existing backup is never overwritten.
- the Codex integration: `SKILL.md` under `~/.agents/skills/factory/` is refreshed only when it already exists; upgrade never creates a missing one, and `factory install` is what installs the Codex skill in the first place. A replaced `SKILL.md` is backed up the same way, as `SKILL.md.bak`, `SKILL.md.bak.1`, and so on.
- the preset lines, as in `factory presets refresh`, and a note that refused files stay as they are. To
  overwrite them, run `factory presets refresh --force`.
- `shadowed: <project> <project-layer|repo-layer> <file>` lines and `skipped: <project> (<reason>)` lines.
- the marker line.

If the Codex skill update fails, the Claude command, presets and version marker still finish updating;
`factory upgrade` then exits with a partial-upgrade error naming the Codex failure.

Project-layer and repo-layer agent files do not follow preset changes. A `shadowed:` line means that override
still holds the old text of an agent file the upgrade updated, so merge the change by hand.

The marker is `<home>/VERSION`. A binary older than the marker stops with a message naming both versions.
`--version` and `install` still work. `factory project show` prints `version: <version>`, and `factoryd status`
ends with `factoryd <version>` and a `warning:` line when it differs from the `factory` binary.

Boards written before this release have no `last_review`. A job parked there with a CHANGES review approves
as before, with no `--waive`. The first review after the upgrade writes the field.

### Check coverage

The rust preset now covers the root `Cargo.toml` and `Cargo.lock` with `cargo check -j4` and exempts generic non-code files such as `LICENSE` and `.gitignore`. Run `factory upgrade`: it refreshes an unedited rust preset.

A project that sets `[[settings.checks]]` or `unchecked_paths` in `project.toml` keeps its own values. It must add the root rule and the exempt paths itself:

```toml
[[settings.checks]]
paths = ["Cargo.toml", "Cargo.lock"]
commands = ["cargo check -j4"]

[settings]
unchecked_paths = ["LICENSE", "LICENSE-*", "LICENSE.txt", "COPYING", "COPYING.*", ".gitignore", ".gitattributes", ".editorconfig"]
```

A job that parks as `no-checks` shows in `factory --project <name> status`.

## Refreshing seeded presets

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

`refresh` does not update the global command. `factory upgrade` does both in one step, see Upgrading above.

## The global /factory command

The recommended install is the Claude Code plugin: `claude plugin marketplace add Snehal1112/rocket-factory`,
then `claude plugin install factory@rocket-factory`. Once the plugin is installed, `factory upgrade` stops
touching the old `~/.claude/commands/factory.md`. `factory install` is the fallback.

`factory install [--dir <path>] [--force]` writes the `/factory` slash command for Claude and a `factory`
skill for Codex. Without `--dir`, these go to `~/.claude/commands/factory.md` and
`~/.agents/skills/factory/SKILL.md`; without `--dir` an unset or empty `$HOME` is an error. With `--dir`,
only the Claude `factory.md` is written to that folder. Missing folders are created and each write is atomic.
Identical files are left alone (`already installed`). A differing Claude file causes an error unless `--force`;
a differing Codex skill is kept with a note so Claude installation still succeeds. Other Codex write failures
return an error after the Claude command has been installed. Restart Codex after installation, then invoke
`$factory` or select it from `/skills`.

In a repository, `/factory` runs `factory project which` to find the project, then passes `--project <name>`
on every `factory` call (`factory` on PATH). It runs the brainstorm interview and writes
the spec and the plans (at most 3 tasks each) with explicit approvals, creates the job with `factory new`, and
starts the run: `factoryd enqueue --project <name> --job <id> --series` when `factoryd is-running` exits 0,
else `factory --project <name> run <id> --series` in the background. It does not poll. The checks come from the
project's registry entry. `factoryd is-running` exits 0 while a `factoryd` holds the instance lock of this
home and exits 1 otherwise, with no output.

A project-local `.claude/commands/factory.md` (Rocket has one until plan 8) shadows the global command of the same
name inside that repository.

## Running against a project

`factory --project <name> <command>` runs a command against a registered project, from any folder. It cannot be
combined with `--root`. The settings come from `project.toml`, the preset and the repo override, and the layout
(state, worktrees) lives in the project folder. A project whose repo folder is missing or is not a git repo is
refused outright. A project is blocked when its settings failed closed (`factory project show` lists why), a
check command would be rejected, or `project.toml` has no valid `base_branch`. A blocked project refuses `run`,
`approve` and `kb-refresh`, the commands that start agents or merge.

## Approving a parked job

`factory approve <id>` merges a job in `needs-human` or `approve`. It refuses a `needs-human` job whose code review
has no verdict yet (pending, running or absent), because the checks never passed or the reviewer never ran:
`job 020 has no code review yet (...): use rework, or approve --without-review to merge anyway`. A job parked
by the reviewer (risky diff, changes remain) has a verdict and approves as before. `factory approve <id>
--without-review` merges anyway and the ledger says `approved by the user without a review`. The park hints in
`park.json` and `factory status` offer that flag, not a plain approve, when the review has not run. When a fix round
that started from failed checks changes nothing, the driver runs the checks once more: if they pass the job goes
to the reviewer, if they still fail it parks as `checks-failed`. The board field `fix_source` (`checks` or
`review`) records what started the latest fix round.

`factory retry <id>` continues a job parked by the driver at the stage that parked it: a
failed review, a `no-checks` park, a changed branch tip, a busy main, a spent sync cap or a series plan
whose earlier plan has not merged (it continues at build). It then drives the job like
`run`, and continues a series. A park with no resume stage (builder parks, risky diff, changes remain) is refused
with its own next commands. `factory rework` refuses a park that `retry` can resume, unless you add `--force`.
Details are in `docs/reference/software-factory.md` (Retry).

## The project lease

At most one job is active per project. A running `run` or `approve` holds a lease file in the project's state
folder and renews it while it works. `factory status` starts with `project: free`, or with
`project: busy, job <id> (owner <who>, <n> s left)` while another process holds it. The busy message means that
process owns the project for about that many more seconds: wait for it, or stop it. A lease of a crashed process
expires on its own (after up to 30 minutes), and an expired lease file is taken over. A lease whose
owner is a `driver-<pid>-<secs>` process that runs on this machine, with the start time its name says, stays held
even after it expired. One whose process is gone, a zombie or reused by another process counts as stale at once
(Linux and macOS). An owner without a pid keeps the expiry rule.
A corrupt lease file is refused and left in place, and a failed run does not delete it. Delete
`<state folder>/project.lease` to clear it. `factory cancel <id> --force` also removes it, but it gives up the job it
names first (needs-human and cancelled).
If the process that held the lease is gone and the lease is still shown as busy, `factory cancel <id> --force`
releases the lease of that job, or delete `<state folder>/project.lease`. `project remove` refuses while the lease
is held, `--force` included.

The agent and check children of a driver run in their own process group. The group ends when the driver drops it
and when the driver gets SIGTERM, SIGINT or SIGHUP. Each driver lists its children in
`<state folder>/children-<owner>.json`. After a SIGKILL of a driver, the next driver that takes over the lease, or
takes a free one, reads the files of dead owners and kills a registered group only when the pid runs, leads its
group and has the recorded start time. Anything else is left alone and reported as one line in the ledger, the
log and stderr.

A running driver renews both the project lease and the job lease every 60 seconds from a background thread, so a
long agent step does not let them expire. If a renewal finds another owner holding the lease, the run stops at the
start of its next loop iteration with the busy error. `kb-refresh` takes the project lease like `run` does, so a
busy project refuses it with the same message.

The dead-owner check looks up the pid in this machine's process table, through one library (`sysinfo`) on both
Linux and macOS. A state folder shared through a container bind mount or a network filesystem can show a live
owner as gone, so run the drivers of one project from one machine.
A hung agent that keeps producing output keeps the project locked as long as its driver process lives. Only
`stall_minutes` covers a silent one.

## The job lease

Each job also has a lease on the board, held by the driver that works on it. A running driver renews both the
project lease and the job lease every 60 seconds. A job lease whose owner is a `driver-<pid>-<secs>` process that
no longer runs on this machine counts as stale at once, for the job lease too, so `run`, `approve`, `cancel`
without `--force`, `tickets` and `kb-refresh` take it over on the first retry. Other owners keep it until it
expires (30 minutes).

While a task builder runs, the driver saves its session id and the branch tip on the board. After a crash the next
`run` resumes that session with a short "you were interrupted" prompt, as long as the branch tip has not moved. If
the resume fails, one fresh builder runs on the worktree as it is, with the interrupted edits kept. The ledger line
of the dispatched task ends in `resumed` or `fresh`.

A run started in a terminal stops when the terminal goes away. For runs that must survive a dropped terminal, use
`factoryd enqueue`, or start `factory run` under `nohup` or in `tmux`.

## factoryd

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
factoryd enqueue --project myproj --job 003        # add a request (--series, --approve, --retry)
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

`factory run`, `factory approve` and `factory retry` exit with code 75 when the project lease is already held at the start, and
nothing was done. `factoryd` then keeps the request queued and starts it again once the lease is free, up to 20
times, after which the request ends as `Failed`. The count is kept in memory, so a restart of `factoryd` begins
again at 0. Their last stdout line (`merged`, `needs-human: <reason>`, `parked: <reason>`) is always one line: a
newline in a reason is replaced by a space. `merged` counts only with exit code 0.

### The daily budget

`daily_budget_weighted = 5000000` in `config.toml` limits all projects together for one UTC day. The unit is
weighted tokens (cache reads count one tenth), summed over the `usage.jsonl` of every job of every registered
project; the day is the UTC day, the same as `GET /usage`. Without the key there is no limit; `0`, a negative
number or a non-integer makes `config.toml` invalid, and `factoryd` then refuses to start. The per-job budget is
unchanged. It is `budget_weighted` in the project config, default 1,000,000 weighted tokens per job: the agent run warns at 80 percent and stops at 100 percent.

Once today's use reaches the limit, new runs wait: a `Run` or `Retry` request stays in the queue (nothing is rejected, lost
or reordered) and starts on the first tick after the budget is raised in `config.toml` or the UTC day changes.
A retry waits like a run, even one that only merges, because it may also run reviewers, fix rounds and the next plans of a series.
Approvals are never held, running jobs are never stopped, and a waiting run never blocks a request behind it.
`factoryd` reads `config.toml` on every tick; if the file becomes invalid while it runs, it keeps the last valid
value and prints one line. `factoryd` also prints one line when the limit is reached and when runs may start again.
Without a running `factoryd`, the `/factory` command falls back to `factory run` in the background, and that path
does not check the daily budget.
`factoryd status` shows `daily budget: <used> of <limit> weighted tokens today (<pct>%), resets at <time>`, with
`warning` from 80% and `reached: new runs are waiting` from 100%.

## Rust preset checks

The rust preset has two check rules. `crates/*/**` runs `cargo check -j4 -p {crate}` and `cargo test -j4 -p {crate}`
for each changed workspace member, then `cargo fmt -p {crate} -- --check` (the `format` gate) and
`cargo clippy -j4 -p {crate} --all-targets -- -D warnings` (the `lint` gate), then `cargo test -j4 -p {dependents}` for
the crates that directly depend on it (limited by `dependents_cap`). A second rule covers a single-crate repo:
`src/**`, `tests/**`, `benches/**`, `examples/**` and `build.rs` at the repo root run `cargo check -j4`,
`cargo test -j4`, `cargo fmt -- --check` (`format`) and `cargo clippy -j4 --all-targets -- -D warnings` (`lint`). These paths are anchored
at the root, so a member's `crates/x/src/lib.rs` matches only the first rule. A virtual workspace (a root
`Cargo.toml` without a `[package]`) relies on the `crates/*/**` rule, because a bare `cargo check -j4` there would
check the whole workspace.

## Layers

Settings come from three layers, each on top of the one before:

1. the preset `config.toml`,
2. the `[settings]` table in the project's `project.toml`,
3. the repo override `.factory/override.toml`, read from the tip of the base branch (never the working tree).

A repo override may only add `risk_paths` and `checks`. Both are additive. Any other key, or an unreadable file,
makes the config fail closed. `.factory/**` is always a risk path, so a repo cannot change its own factory
settings without review.

The four prompt keys are `read_first` (files the builder reads first), `builder_rules`, `check_hints` and
`reviewer_rules`.

`reviewers` (default `["code"]`) lists the review roles. Allowed values are `code` and `security`, `code` is
required and a value may not repeat. With `"security"` the `factory-security-reviewer` agent also reviews every
round, after the code review, and a job merges only when both approve. `security_rules` (default empty) is
joined into one line of the security prompt and enforces nothing. A `[[checks]]` rule may also hold `format` and
`lint` command lists, planned after `commands`. Their results show as `format` and `lint` entries in
`factory status`.

`plan_rules` (optional, default empty) are rules for the plans the `/factory` command writes. The driver puts them in no
prompt. `factory project show <name>` prints them in a `plan rules:` block and the command applies each line to every
plan. A repo override cannot set them.

## Agent files

Agent files are merged by name from the preset `agents/` folder, the project `agents/` folder and the repo's
`.factory/agents/` folder on the base branch (later wins). On Claude the agents reach the CLI through
`--agents <job folder>/agents.json`, so nothing is written into a worktree or the main checkout. Only names like
`factory-<name>.md` are used, and the text of a `.claude/agents/<name>.md` tracked on the base branch still wins.
When `reviewers` lists `security`, the merged set must hold `factory-security-reviewer.md`. If it does not,
`factory project show` reports it under `problems:` and the job parks. Run `factory presets refresh` to restore it.

## Agent file rules marker

The builder and reviewer agent files carry the line `<!-- factory-rules: 1 -->` right after their frontmatter. When
the agent file in the job worktree has a current marker, the prompts leave out the rules that file already holds.
Otherwise the driver uses the full prompts and writes one ledger line per job and agent:
`agent file <name>.md has no current rules marker, prompts keep the full rules`.

`factory upgrade` refreshes agent files you have not edited. An edited file, or a repo file in `.factory/agents/` that
shadows the shipped one, keeps working with the full prompt until you merge the new marker and rules by hand. The
KB prompt is unchanged.
