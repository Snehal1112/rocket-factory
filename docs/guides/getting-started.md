# Getting started with the software factory

This guide takes you from a fresh machine to your first merged feature. Follow the parts in order. Each part ends with a check that tells you what you should see, so you always know whether to continue.

Every command here was run against a throwaway home and a throwaway repository before this guide was written.

## 1. What the factory is

You describe a feature to Claude Code with `/factory <feature>`. Claude interviews you and writes a spec and a plan. You approve them. After that a program called the driver does the rest without you:

1. It makes an isolated git worktree and branch.
2. It runs a builder agent (`claude -p`) per task.
3. It runs the checks for the files that changed, such as `cargo test`.
4. It runs a reviewer agent.
5. If everything is clean, it merges the branch into your local base branch.

If anything is unsure (a failing check, a risky file, a big diff, a budget stop), the job parks as `needs-human` and waits for you. You then approve, send a note back to the builder (rework) or cancel.

Three things to know up front:

- The factory never pushes and never rewrites history. A merge lands on your local base branch only. You push yourself.
- Your repository stays clean. Job worktrees, the board and the ledger live in the factory home (`~/.factory`), not in your repo.
- Orchestration costs no model tokens. Only the builder, reviewer and committer agents use tokens.

Words used below:

| Word | Meaning |
|---|---|
| home | The folder where the factory keeps its data. `~/.factory` unless you set `FACTORY_HOME`. |
| project | One of your git repositories, registered once with the factory. |
| preset | A starter set of rules for a language (checks, risky paths, agent prompts). `rust`, `go`, `ruby`, `csharp` and `node` ship. |
| job | One plan being built. Ids look like `001`, `002`. |
| series | The chain of jobs made from a multi-plan feature. Plan 2 starts after plan 1 merges. |
| `factory` | The command line tool. |
| `factoryd` | An optional background supervisor. It queues runs and lets you work on several projects at once. |

## 2. What you need first

The factory runs on Linux and macOS (see the note at the end). It is built from source with Rust.

| Need | Why | Check |
|---|---|---|
| Rust, stable 1.82 or newer | Builds the factory. The code uses `Option::is_none_or`, which needs 1.82. | `rustc --version` |
| A C compiler (`gcc` or `clang`) | The git library (libgit2) compiles C code. | `cc --version` |
| `git` | To clone this repo, and your repos are git repos. | `git --version` |
| Claude Code, logged in | The factory runs `claude -p` for every agent. | `claude --version` |
| The `superpowers` plugin for Claude Code, version 7.0.0 | `/factory` uses `superpowers:brainstorming`, `superpowers:writing-plans` and the builder uses `superpowers:subagent-driven-development`. | In Claude Code, run `/plugin` and look for it. |
| The `dev-workflow-skills` plugin | The committer agent writes commits with the `dev-workflow-skills:1-git-commit` skill. | In Claude Code, run `/plugin` and look for it. |
| `gh` (GitHub CLI), optional | Only for ticket hygiene (`tickets_on_merge`). Skip it if you do not use GitHub issues. | `gh --version` |
| `cargo` on the machine, only for Rust projects | The `rust` preset checks run `cargo check` and `cargo test` in your project. | `cargo --version` |
| `bundle` on the machine, only for Ruby projects | The `ruby` preset checks run `bundle exec rubocop` and `bundle exec rspec`. | `bundle --version` |
| `dotnet` on the machine, only for C# projects | The `csharp` preset checks run `dotnet build` and `dotnet test`. | `dotnet --version` |
| `bun` on the machine, only for Node and React projects | The `node` preset checks run the project's `typecheck`, `lint` and `test` scripts through Bun. | `bun --version` |

If you have no Rust yet, install it with `rustup` from https://rustup.rs and open a new terminal.

How the factory logs in to Claude: it runs `claude` as you, with your normal Claude login (subscription). It removes `ANTHROPIC_API_KEY` and `ANTHROPIC_AUTH_TOKEN` from the agent environment, so a key in your shell does not silently bill the API. To allow API billing on purpose for every project, set `FACTORY_ALLOW_API_KEY=1`. To do it for one project, set `allow_api_key = true` in its `project.toml` (the key comes from `ANTHROPIC_API_KEY`), or set `api_key_file = "~/.factory/keys/<name>.key"` to give that project its own key. Make the file readable only by you with `chmod 600`. `allow_api_key = false` keeps a project on the login even when `FACTORY_ALLOW_API_KEY=1` is set.

Check that `claude` works on its own before you go on:

```bash
claude -p "say ok"
```

You should see a short reply. If it asks you to log in, run `claude` once and log in.

## 3. Build the binaries

Get the code and build. Always use `-j4`, and never `--workspace` or `--all`.

```bash
git clone <url-of-rocket-factory> ~/data/rocket/rocket-factory
cd ~/data/rocket/rocket-factory
cargo build -j4 -p factory-cli -p factory-supervisor
cargo build -j4 --manifest-path dashboard/Cargo.toml
```

The first build takes a few minutes. When it ends you have:

| Binary | Path |
|---|---|
| `factory` | `target/debug/factory` |
| `factoryd` | `target/debug/factoryd` |
| dashboard | `dashboard/target/debug/factory-server` |

Check:

```bash
ls target/debug/factory target/debug/factoryd dashboard/target/debug/factory-server
```

All three paths should be listed. The dashboard is optional, you can build it later.

## 4. Put the binaries on your PATH

`factory` and `factoryd` must be found by name, because the `/factory` command in Claude Code calls them. Linking them into a folder that is already on your PATH is the easiest way. A link keeps working after you rebuild.

```bash
mkdir -p ~/.local/bin
ln -sf ~/data/rocket/rocket-factory/target/debug/factory  ~/.local/bin/factory
ln -sf ~/data/rocket/rocket-factory/target/debug/factoryd ~/.local/bin/factoryd
```

If `~/.local/bin` is not on your PATH, add this line to `~/.zshrc` or `~/.bashrc` and open a new terminal:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

Check:

```bash
which factory factoryd
factory --help
```

You should see two paths and a list of commands. `factoryd` finds its `factory` child next to its own file, so keep both links pointing into the same `target/debug` folder. If you keep the binaries somewhere else, set `FACTORY_BIN` (see section 5).

## 5. Environment variables

You do not need to set any of these for a normal setup. They exist for special cases.

| Variable | Default | What it does |
|---|---|---|
| `FACTORY_HOME` | `~/.factory` | Moves the factory home. Set it to try the factory in a scratch folder. |
| `FACTORY_BIN` | the `factory` file next to `factoryd` | Tells `factoryd` and the `/factory` command where the `factory` binary is. |
| `FACTORY_ALLOW_API_KEY` | off | Set to `1` to let agents use `ANTHROPIC_API_KEY` (API billing). A project's `allow_api_key` and `api_key_file` settings override it. |
| `FACTORY_LOCK_WAIT` | 15 | Seconds a command waits for a busy lock before it gives up. |

Example, to try the factory without touching your real home:

```bash
export FACTORY_HOME=/tmp/factory-try
```

Unset it (or open a new terminal) to go back to `~/.factory`. The home is created on first use, so you do not make it yourself.

## 6. Install the `/factory` command

This puts the slash command in Claude Code, for every project:

```bash
factory install
```

It writes `~/.claude/commands/factory.md`. You should see `installed /home/<you>/.claude/commands/factory.md`. Running it again prints `already installed`.

Note: a `.claude/commands/factory.md` inside a repo shadows the global one.

## 7. Register your repository

Registration tells the factory which repo to work on and which preset to use. Do it once per repo. The repo must be a git repo, and its base branch must exist locally and be the branch you merge into. Give the top folder of the repo, or, in a monorepo, a subfolder such as `apps/web`. A subfolder becomes its own project, and a job may change only files inside it. A repo holds either one top-folder project or several disjoint subfolder projects (see "Monorepos" in the README).

```bash
cd /path/to/your/repo
git branch --show-current          # for example: main
factory project add . --preset rust
```

Use `--preset go` for a Go repo, `--preset ruby` for a Ruby repo, `--preset csharp` for a C# repo and `--preset node` for a Node or React repo. Useful options:

- `--base <branch>`: the branch to merge into. It defaults to the branch you are on.
- `--name <name>`: the project name. It defaults to the folder name, lower case. Names use `a-z`, `0-9` and `-`, up to 40 characters.

You should see a line like `added project myrepo (rust, base main) at ~/.factory/projects/myrepo`.

Now check the registration. This is the most useful command when something looks wrong:

```bash
factory project show myrepo
```

The output lists every setting under `settings:` with its value and source, and must end with `problems: none`. It must not say `status: degraded`. A degraded project fails closed: every change counts as risky and nothing merges on its own.

Other project commands:

| Command | Use |
|---|---|
| `factory project list` | All projects, and any that are disabled and why. |
| `factory project which` | The project that owns the current folder. |
| `factory project defaults <name>` | Add every setting the file does not list yet, as a commented line with its default. `--all` does every project. |
| `factory project remove <name>` | Unregister a project. It never deletes your repo or your `factory/*` branches. |

There is no command to edit a project. To change a setting, edit `~/.factory/projects/<name>/project.toml` by hand (section 12).

## 8. Check the whole setup

Run these from your repo. Each line has the result you should see.

```bash
factory project which          # prints your project name
factory --project myrepo status   # prints "project: free" and "no jobs"
factoryd is-running; echo $?   # prints 1 now, because no supervisor runs yet
```

Then open Claude Code in your repo and type `/factory status`. You should get the same status output back. If it says the binaries are not on PATH, go back to section 4. If it says there is no registered project, go back to section 7.

## 9. Run your first feature

Pick a small, low-risk change for your first job, for example a helper function with a unit test. In Claude Code, inside your repo:

```
/factory add a --version flag to the cli
```

What happens, and what you do:

1. **Interview.** Claude explores your code, starts a draft card, and asks questions one at a time. Each question has a recommended answer and alternatives. Answer them, or accept the recommendation. Every answer is written into the spec as a decision. This takes a few minutes. Do not rush it, the quality of the spec decides the quality of the result.
2. **Approve the spec.** Claude shows the spec. It needs a goal line of at least 8 words and acceptance criteria. Read it and approve it. Claude commits it.
3. **Plans.** Claude writes one to six plans, each with at most three tasks, under `docs/superpowers/plans/<date>-<slug>/`. Read them and approve.
4. **Run.** Claude creates the jobs and starts the driver in the background, then tells you it is running and goes quiet. It does not poll. You can keep working or close the terminal tab, but do not stop the machine.
5. **Result.** Ask `/factory status` any time, or wait for the notification. A job ends as `merged` or `needs-human`.

The spec and plans are committed on the branch your session is on. They reach your base branch only when that branch merges. The job branches are named `factory/NNN-slug` and start from your base branch.

### When a job says `needs-human`

`/factory status` shows the reason. The common reasons:

| Reason | Meaning | What to do |
|---|---|---|
| `checks-failed` | A check kept failing after the fix rounds. | `/factory rework <id> <note>` with a hint, or cancel. |
| `changes-remain` | The reviewer still asks for changes after 2 fix rounds. | Read the review, then rework with a note or approve. |
| `risky-diff` | The change touches a risky path, such as `Cargo.toml` or a file with `auth` in its name. | Read the diff in the worktree. Approve if it is fine. |
| `diff-too-large` | The diff is over 4000 lines. | Approve after you review it, or cancel and split the work. |
| `budget-stopped` | The job used its token budget (default 1,000,000 weighted tokens). | Raise `budget_weighted`, then rework. |
| `builder-blocked` | The builder said it cannot continue. | Give it what it asks for with rework. |

Your three choices:

```
/factory approve <id>            # merge it now
/factory rework <id> <note>      # one-line note, up to 200 characters, back to the builder
/factory cancel <id>             # stop it
```

`approve` is refused when the code review never ran. Add `--without-review` only if you mean it. Cancelling one plan of a series leaves the later plans waiting, so cancel each of them. After a merge or a cancel, `factory cleanup <id>` removes the worktree and the branch.

After a merge, your base branch has a new merge commit. Check it, then push it yourself with `git push`.

### Using the command line without Claude Code

Everything `/factory` does can be done directly. You write the spec and plans yourself, commit them, then:

```bash
factory --project myrepo new my-slug "feature text" --plans docs/superpowers/plans/<dir> --spec docs/superpowers/specs/<file>
factory --project myrepo run 001 --series
```

Use a short kebab-case slug that is not only digits. The spec and plans must already be committed. Pass `--project <name>` on every command, as above.

## 10. Optional: the supervisor and the dashboard

You can skip this part until you want it. `factoryd` is needed when you want to run several projects in parallel, queue runs, set a daily token limit, or use the multi-project dashboard.

### Start the supervisor

```bash
nohup factoryd run >> ~/.factory/factoryd.log 2>&1 &
factoryd is-running; echo $?     # 0 means it is running
factoryd status
```

Once it is running, `/factory` queues work through it automatically. If it is not running, `/factory` starts the driver as a background process instead.

| Command | Use |
|---|---|
| `factoryd run` | Run the supervisor loop. It listens on `127.0.0.1:8766` (local only). |
| `factoryd status` | Daily budget line, the queue, running jobs, the last 10 results. |
| `factoryd stop` | Ask it to end. Running jobs keep running. |
| `factoryd is-running` | Exit code 0 if it runs, else 1. Prints nothing. |
| `factoryd enqueue --project <name> --job <id> [--series]` | Queue a run by hand. |

If `factoryd` stops or crashes, running jobs keep going. When you start it again it adopts them. A request whose outcome it cannot tell ends as `Lost`, and it never restarts those. Read the logs, then run `factory run <id>` or enqueue again.

`factoryd` does not start by itself after a reboot. Start it again when you need it.

### Limits

Create `~/.factory/config.toml` to change the supervisor. All keys are optional, and unknown keys are rejected so a typo shows up at once:

```toml
max_parallel = 2                  # projects run at the same time, 1 to 64
listen = "127.0.0.1:8766"         # must stay on loopback
daily_budget_weighted = 5000000   # all projects together, per UTC day
draft_stale_hours = 24
```

When the daily limit is reached, new runs wait in the queue until the next UTC midnight. `factoryd status` shows the used amount and the reset time. Jobs that are already running, and approvals, are not stopped by it.

### The dashboard

The dashboard is a local web page with a board of every job, its cards, review results, token bars and a cost panel.

All projects (needs `factoryd run`):

```bash
~/data/rocket/rocket-factory/dashboard/target/debug/factory-server --home ~/.factory --port 8765
```

One project, read only, no `factoryd` needed:

```bash
~/data/rocket/rocket-factory/dashboard/target/debug/factory-server --root /path/to/your/repo --port 8765
```

Open http://127.0.0.1:8765/ in your browser. It listens on your own machine only. In `--home` mode you can start a job and approve, rework or cancel from the page. The page never sees the API token. The server reads it from `~/.factory/api.token`, which `factoryd run` creates on first start with mode 0600. Do not paste that token anywhere.

## 11. Daily use

| I want to | Do this |
|---|---|
| Build something | `/factory <feature>` in Claude Code, in the repo. |
| Use the stronger model for one job | `/factory --opus <feature>` (the flag goes first or last). |
| See what is happening | `/factory status` or `factory --project <name> status`. |
| See what a job spent | `factory --project <name> tokens <id>` and `factory --project <name> costs`. |
| Watch it live | The dashboard. |
| Finish a parked job | `/factory approve <id>` or `/factory rework <id> <note>`. |
| Clean up | `/factory cleanup <id>` after merge or cancel. |
| Stop a stuck job | `factory --project <name> cancel <id>`. If it says the lease is live, add `--force`. |

Safety rules the driver follows, so you can trust it with your repo:

- It auto-merges only when no check failed, the reviewer's verdict is exactly `VERDICT: APPROVE`, no risky path is touched, the diff is within 4000 lines and the budget is not spent.
- It only merges when your main checkout is on the base branch, has no tracked changes and is not in the middle of a merge. Untracked files are fine.
- It merges only the commit the reviewer saw. If your base branch moved, it merges the base into the job branch, re-runs the checks and the review, and sends conflicts to you.
- It never pushes and never force-pushes.

## 12. Settings

Settings come in three layers. A later layer overrides an earlier one.

1. **Preset**: `~/.factory/presets/<preset>/config.toml`. The defaults for `rust`, `go`, `ruby`, `csharp` or `node`. Do not edit these, a refresh would then refuse to update them.
2. **Project**: the `[settings]` table in `~/.factory/projects/<name>/project.toml`. A key here replaces the whole key from the preset.
3. **Repo**: `.factory/override.toml` in your repo, read from the tip of the base branch (so it must be committed and merged). It may only set `risk_paths` and `checks`, and both add to the lower layers.

Example `project.toml`:

```toml
repo = "/home/me/code/myrepo"
preset = "rust"

[settings]
base_branch = "main"
budget_weighted = 1500000
fix_rounds = 2
builder_model = "sonnet"
reviewer_model = "sonnet"
tickets_on_merge = false
```

Keys you may want, with defaults:

| Key | Default | Meaning |
|---|---|---|
| `base_branch` | `main` | Branch the jobs start from and merge into. |
| `budget_weighted` | 1,000,000 | Token budget per job. It warns at 80 percent and stops at 100 percent. If jobs park on budget, raise it to 1,500,000 first. |
| `fix_rounds` | 2 | Fix rounds after a failed check or review. |
| `diff_cap_lines` | 4000 | Bigger diffs park for you. |
| `stall_minutes` | 10 | A silent agent counts as stalled after this long. |
| `builder_model`, `reviewer_model`, `commit_model` | `sonnet` | Models for each role. |
| `reviewer_model_large` | `opus` | A stronger reviewer for risky or big diffs. Set it to the same name as `reviewer_model` to turn it off. |
| `risk_paths` | from the preset | Globs that make a diff risky and force your review. |
| `read_first` | `CLAUDE.md` | Files the builder reads first. |
| `tickets_on_merge` | false | File follow-up GitHub issues and close the parent issue after a merge (needs `gh`). |
| `plan_rules` | empty | Extra rules that `/factory` applies to every plan it writes. |

The full key list is in `README.md`.

Fail-closed rule: if any layer is broken or has an unknown key, the project is treated as degraded. Every diff is risky, nothing merges by itself, and `run`, `approve` and `kb-refresh` are refused until you fix it. `factory project show <name>` names the problem.

## 13. Files and folders

```
~/.factory/                      the home ($FACTORY_HOME)
  config.toml                    supervisor limits (you create it, optional)
  api.token                      API token, created by factoryd, mode 0600
  presets/<rust|go|ruby|csharp|node>/             seeded defaults and agent prompts
  projects/<name>/
    project.toml                 repo path, preset, your settings
    agents/                      per-project agent overrides
    worktrees/                   one git worktree per running job
    state/
      board.json, events.jsonl, costs.json, project.lease
      jobs/<date>-<id>-<slug>/   progress.md (ledger), checks.md, park.json, review files, usage.jsonl
  queue/  results/  run/         supervisor request files, results and locks
  logs/<name>/<request>.out|.err supervisor run output
```

Your repository only gets the committed spec and plans, the `factory/NNN-slug` branches, the merge commits, and one line in `.git/info/exclude` for the agent files. You do not need to change `.gitignore`.

## 14. When something goes wrong

| Symptom | Likely cause and fix |
|---|---|
| `/factory` says the binaries are not on PATH | Section 4. Open a new terminal, or run `hash -r`. |
| `no registered project contains <path>` | Run `factory project add . --preset rust` in the repo (section 7). |
| `ambiguous match` | Two project names point at one repo. Remove one with `factory project remove`. |
| `HEAD is not on a branch, pass --base` | You are on a detached HEAD. Check out your branch or pass `--base main`. |
| `project show` says `degraded` or lists problems | Fix the setting it names (section 12). Nothing merges until then. |
| `project: busy, job 003` | One job runs per project at a time. Wait, or cancel that job. |
| A command waits then fails on a lock | Another factory command holds it. Wait, or raise `FACTORY_LOCK_WAIT`. |
| `cancel` is refused because the lease is live | The driver is still running. Stop that process, or use `cancel <id> --force`. |
| A job sits in a stalled state with no output | The driver or agent is hung. Find the `factory` process with `pgrep -a factory`, stop it, then `factory cancel <id> --force` and run it again. |
| Run was requested but nothing starts | `factoryd status` may say the daily budget is reached. It resets at UTC midnight. Or `factoryd` is not running and the fallback driver failed, so look at the logs below. |
| The agent fails to start | Run `claude -p "say ok"`. If it cannot log in, run `claude` and log in. |
| Agents say a skill is missing | Install the `superpowers` and `dev-workflow-skills` plugins (section 2). |
| Commit step fails with `commit-failed` | Check `park.json` in the job folder. If the builder committed by itself, run `git reset --soft <sha>` in the worktree, then rework. |

Where to look:

- `factory --project <name> status` first. It prints the park reason and the next commands.
- `~/.factory/projects/<name>/state/jobs/<job>/progress.md` is the ledger, and `park.json` holds the park details.
- `~/.factory/projects/<name>/state/events.jsonl` is the event log.
- `~/.factory/logs/<name>/` holds the output of runs started through `factoryd`.

## 15. After you update the factory

Pull the new code, build, and run one command. A new binary does not update the seeded presets or the installed command by itself, `factory upgrade` does.

```bash
cd ~/data/rocket/rocket-factory
git pull
cargo build -j4 -p factory-cli -p factory-supervisor
cargo build -j4 --manifest-path dashboard/Cargo.toml
factory upgrade --dry-run        # optional preview
factory upgrade                  # command, presets, shadow report, version marker
factoryd stop                    # wait for running jobs first, then start it again
nohup factoryd run >> ~/.factory/factoryd.log 2>&1 &
```

`factory upgrade` never touches your project settings or your repo. A preset file you edited by hand is listed as `refused` and left alone, and `factory presets refresh --force` overwrites it.

## 16. Platform notes

- Linux and macOS are supported.
- The dead-owner checks for locks and the adoption of running jobs after a `factoryd` restart use one process library on both systems. They only see processes on this machine.
- Windows is not supported for `factoryd`.

## 17. Where to read more

- `docs/reference/factory-admin-manual.md`: developer and administrator command examples, supported flags, and supervisor operations.
- `README.md`: every command, setting and file in detail.
- `docs/reference/software-factory.md`: the pipeline, job model, merge rules and dashboard.
- `dashboard/README.md`: dashboard modes and security.
- `docs/superpowers/specs/2026-10-16-standalone-software-factory-design.md`: why it is built this way.
