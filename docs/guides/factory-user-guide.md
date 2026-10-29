# Rocket Factory user guide

This guide is for people who are new to the factory, including people who do not write code every day. It explains what the factory does, how to set it up, and every `factory` and `factoryd` command with its options and examples.

You do not need to know Rust. You do need a terminal, git, and a project that lives in a git repository.

Other docs go deeper:

- `docs/guides/getting-started.md`: the full install walkthrough.
- `docs/reference/software-factory.md`: the pipeline and merge rules in detail.
- `docs/reference/project-toml-reference.md`: every project setting.
- `docs/reference/factory-admin-manual.md`: the operator reference.

Commands in this guide were checked against the built binaries (`factory --help`, `factoryd --help` and the help of every subcommand) and against the source. Where something is uncertain the guide says so.

## Contents

1. What the factory is
2. The ideas you need (glossary)
3. Before you start
4. Install and set up
5. Your first feature, start to finish
6. Command reference: `factory`
7. Command reference: `factoryd`
8. The dashboard
9. When a job stops: what to do
10. Recipes
11. Settings that matter most
12. Where files live
13. Troubleshooting
14. Quick reference card

---

## 1. What the factory is

The factory lets AI agents build a feature in your repository while you stay in control of the decisions.

You describe what you want. An interview turns it into a written spec and a set of plans. The factory then does the work in an isolated copy of your repository:

1. A **builder** agent writes the code, one task at a time.
2. The factory runs the **checks** for the files that changed, such as the test suite.
3. A **reviewer** agent reads the change and says APPROVE or CHANGES. If it asks for changes, the builder gets up to two fix rounds.
4. The factory **decides**:
   - If the review approved, the change is small, no sensitive file was touched and the token budget is fine, it **merges** into your local base branch.
   - Anything else stops and waits for you. This state is called **needs-human**.

Two guarantees to remember:

- **The factory never pushes.** It merges into your local branch only. You push when you are ready.
- **Your own checkout is not touched while a job builds.** The work happens in a separate worktree.

There are two programs:

- `factory`: the command you use for everything. It works on one project at a time.
- `factoryd`: an optional background supervisor. You need it only for several projects in parallel, queued runs, a daily token limit, or the multi-project dashboard.

There is also a `/factory` slash command for Claude Code. It runs the interview and planning, then drives `factory` for you. This guide covers the plain commands, which is what `/factory` calls underneath.

## 2. The ideas you need (glossary)

| Term | Meaning |
|---|---|
| Job | One unit of work the factory builds and merges. It has an id like `028`. |
| Spec | The approved design document: what to build and why. It must have a `## Goal` and `## Acceptance criteria`. |
| Plan | A markdown file with 1 to 3 tasks. A feature can have up to 6 plans in a chain. |
| Series | The chain of jobs made from a multi-plan feature. Plan 2 starts only after plan 1 is merged. Series ids look like `S1`. |
| Task | One checkbox line in a plan, written `- [ ] Task N: ...`. The builder does one task at a time. |
| Worktree | A second working copy of your repository on the job's own branch. It keeps your checkout clean. |
| Stage | Where a job is: `spec`, `build`, `review`, `approve`, `merged` or `needs-human`. |
| needs-human | The job stopped and wants you. The reason is printed, along with what you can do next. Also called "parked". |
| Risky diff | A change touching a sensitive path such as `Cargo.toml`, authentication or secrets. It always needs your approval. |
| Preset | A starter rule set for a language: `rust`, `go`, `ruby`, `csharp` or `node`. |
| Project | A repository (or a folder inside one) registered with the factory. |
| Factory home | The folder where the factory keeps its data: `$FACTORY_HOME`, else `~/.factory`. |
| Board | The list of jobs and their stages, kept in `board.json` and shown by the dashboard. |
| Ledger | `progress.md` in the job folder: a log of what the driver did. |
| Lease | A lock that says which process owns a project or job, so two drivers never work on the same project at once. |
| Weighted tokens | The factory's cost unit. Cache reads count one tenth. Each job has a budget, 1,000,000 by default. |
| Draft | A feature that is still being specified, before it has a job. |

Job ids are numbers. `1`, `01` and `001` all mean job `001`.

## 3. Before you start

You need:

- **Rust**, stable 1.82 or newer. Install it from https://rustup.rs.
- **A C compiler** (gcc or clang). The git library needs one.
- **git**.
- **Claude Code**, logged in. Check with `claude -p "say ok"`.
- The **superpowers** plugin (version 7.0.0) and the **dev-workflow-skills** plugin.
- **gh** (GitHub CLI), optional. Only `factory tickets` needs it.
- The build tool of your own project: `cargo` for Rust, `bundle` for Ruby, `dotnet` for C#, `go` for Go, `bun` for Node.js and React.

## 4. Install and set up

### 4.1 Build the programs

From the rocket-factory repository root:

```bash
cargo build -j4 -p factory-cli -p factory-supervisor
cargo build -j4 --manifest-path dashboard/Cargo.toml
```

Always use `-j4`. Do not use `--workspace` or `--all`.

You get three programs:

| Program | Path |
|---|---|
| `factory` | `target/debug/factory` |
| `factoryd` | `target/debug/factoryd` |
| dashboard (`factory-server`) | `dashboard/target/debug/factory-server` |

### 4.2 Put them on your PATH

```bash
mkdir -p ~/.local/bin
ln -sf "$PWD/target/debug/factory"  ~/.local/bin/factory
ln -sf "$PWD/target/debug/factoryd" ~/.local/bin/factoryd
which factory factoryd
```

If `which` finds nothing, add this line to `~/.zshrc` or `~/.bashrc` and open a new terminal:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

The two links point at `target/debug/factory` and `target/debug/factoryd` in your clone. The programs on your PATH change only when you build in that clone: a `git pull` or a merge alone does not update them. After either, build again (section 4.1) and run `factory upgrade` (section 6.18).

The factory home keeps a version marker, and a newer `factory` run from anywhere else raises it. Your older build on the PATH then refuses to run. Check with `factory --version` and `cat ~/.factory/VERSION`. If the marker is higher, see the `this home was last used by factory X` row in section 13.

### 4.3 Install the /factory command

```bash
factory install
```

This writes the Claude command to `~/.claude/commands/factory.md`. It also installs a Codex skill at `~/.agents/skills/factory/SKILL.md`. Running it again prints `already installed`.

See [`factory install`](#factory-install) for the options.

### 4.4 Register your project

Run this from anywhere. Point it at your repository:

```bash
factory project add ~/code/my-app --preset rust
factory project show my-app
```

`project show` should end with `problems: none`. If it lists problems, fix them before you continue.

From now on you can run commands for this project from any folder with `--project my-app`:

```bash
factory --project my-app status
```

If you are inside the repository folder you can leave `--project` out.

## 5. Your first feature, start to finish

This is the whole path in order. Each command is explained in section 6.

**Step 1. Check the board.**

```bash
factory --project my-app status
```

You should see `project: free` and `no jobs`.

**Step 2. Write the spec and plans.** The easy way is the `/factory` command in Claude Code, which interviews you and writes both. Or write them by hand. The expected layout is:

```
docs/superpowers/specs/2026-10-25-login-fix-design.md
docs/superpowers/plans/2026-10-25-login-fix/plan-01-login-fix.md
docs/superpowers/plans/2026-10-25-login-fix/plan-02-login-fix.md
```

Rules the factory checks before it accepts them (section 6.1 lists the exact errors):

- The spec has a `## Goal` of at least 8 words and a `## Acceptance criteria` section with at least one bullet.
- No `TBD`, `TODO`, `FIXME` or `???` anywhere outside code blocks, in the spec or in a plan.
- Each plan has 1 to 3 tasks, written as `- [ ] Task 1: ...`.
- The last line of each plan is `Next plan: <path>` or `Next plan: none`.
- At most 6 plans in a chain.

Commit the spec and plans.

**Step 3. Create the job or jobs.**

```bash
factory --project my-app new login-fix "Fix the login redirect" \
  --spec docs/superpowers/specs/2026-10-25-login-fix-design.md \
  --plans docs/superpowers/plans/2026-10-25-login-fix/
```

The first line printed is `created job 028 in <job folder>`. It then prints a token estimate per job.

**Step 4. Run it.**

```bash
factory --project my-app run 028 --series
```

This can take a long time and use many tokens. The last line it prints is either `merged` or `needs-human: <reason>`.

**Step 5. If it says `needs-human`, decide.**

```bash
factory --project my-app status        # shows the reason and the next commands
factory --project my-app approve 028   # you accept the change and it merges
factory --project my-app rework 028 "Handle the empty list case"
factory --project my-app run 028       # run again after a rework
factory --project my-app cancel 028    # give up
```

**Step 6. Clean up after a merge.**

```bash
factory --project my-app cleanup 028
```

**Step 7. Push when you are ready.** The factory merged into your local branch only:

```bash
git push
```

## 6. Command reference: `factory`

General form:

```
factory [--root <path> | --project <name>] <command> [arguments] [options]
```

### 6.0 Global options

These work on every command.

| Option | Meaning |
|---|---|
| `--project <name>` | Work on a registered project. Works from any folder. |
| `--root <path>` | Work on the repository whose main checkout is at `<path>`. |
| `-h`, `--help` | Show help. |
| `-V`, `--version` | Show the version (top level only). |

- `--project` and `--root` cannot be used together.
- With neither, the factory uses the repository that contains your current folder.
- `--root` does nothing useful for the commands that only use the factory home: `project`, `presets`, `install` and `upgrade`.
- Errors print as `factory: <message>` and exit with code 1. A project that is busy exits with code 75.

```bash
factory --project my-app status
factory --root ~/code/my-app status
```

### 6.1 `factory new`: create a job

```
factory new [options] <SLUG> <FEATURE>
```

Creates a job on the board. Nothing is built yet.

| Argument or option | Meaning |
|---|---|
| `<SLUG>` | A short name. It becomes part of the branch and folder name. Lowercased, joined with dashes, at most 40 characters. |
| `<FEATURE>` | One sentence on what you want. If it contains issue references like `#76`, they are recorded on the job. |
| `--plans <path>` | A plan file, or a folder of `plan-NN` files. |
| `--spec <file>` | The spec file. Needs `--plans`. |
| `--model <name>` | Recorded on the job card. See the note below. |
| `--one-job` | Combine the whole plan chain into one job. Needs `--plans`. |
| `--accept-over-budget` | Allow a `--one-job` job whose estimate is over the budget. Needs `--one-job`. |

**What each form does:**

```bash
# 1. No plans yet. A job is made at stage build, you add plan.md by hand.
factory new quick-fix "Fix typo in the footer"
# It prints: no plan yet: put plan.md at <job folder>/plan.md before running it

# 2. One plan file.
factory new login-fix "Fix login redirect" \
  --plans docs/superpowers/plans/2026-10-25-login-fix/plan-01-login-fix.md

# 3. A spec and a folder of plans (the normal case).
factory new login-fix "Fix login redirect" \
  --spec docs/superpowers/specs/2026-10-25-login-fix-design.md \
  --plans docs/superpowers/plans/2026-10-25-login-fix/

# 4. The whole chain as a single job.
factory new login-fix "Fix login redirect" \
  --spec docs/superpowers/specs/2026-10-25-login-fix-design.md \
  --plans docs/superpowers/plans/2026-10-25-login-fix/ \
  --one-job

# 5. One job, even though the estimate is over the budget.
factory new login-fix "Fix login redirect" \
  --spec docs/superpowers/specs/2026-10-25-login-fix-design.md \
  --plans docs/superpowers/plans/2026-10-25-login-fix/ \
  --one-job --accept-over-budget
```

**With `--plans` and several plans**, plan 1 becomes a job at stage `build`. Every later plan gets its own job, waiting at stage `spec`. They start in order when you run with `--series`. The jobs share a series id such as `S1`.

**How the plans folder is read:**

- Only files named `plan-*.md` count. They are sorted by name.
- Each plan must end with `Next plan: <path>` or `Next plan: none`. The path has no spaces.
- A `plan-*.md` file that the chain does not reach is refused.
- A loop in the chain is refused. More than 6 plans is refused.
- Each plan needs 1 to 3 tasks. With `--one-job` the upper limit is lifted, but each plan still needs at least 1.

**Spec rules.**

- With `--spec`, that file is used.
- Without `--spec`, a plans folder must contain `spec.md`.
- A single plan file given without `--spec` gets no spec check.
- A copy of the spec is stored in each job folder as `spec.md`. A copy of each plan is stored too. Edit the copy, not the committed file.
- If no item under `## Decisions` says `Decided by: user`, the job gets a warning. It is not an error.

**The `--one-job` estimate.** The estimate is 110,000 plus 100,000 per task, in weighted tokens. If it is over `budget_weighted` the command stops with:

```
needs consent: this job is estimated at E weighted tokens and the budget is B; raise budget_weighted in project.toml or pass --accept-over-budget
```

With a chain of only one plan, `--one-job` has no effect and prints `note: --one-job had no effect: the chain has one plan`.

**Note on `--model`.** In the current code, the value is stored on the job card and carried to the later jobs of a series. I found nothing that makes the builder or reviewer use it. The builder and reviewer models come from `builder_model` and `reviewer_model` in `project.toml`. Do not rely on `--model` to change which model runs.

**Common errors and what they mean:**

| Message | What to do |
|---|---|
| `--spec needs --plans` | Add `--plans`. |
| `the plans folder has no spec.md` | Pass `--spec`, or put `spec.md` in the folder. |
| `plan file not found: <dir>/plan-01-*.md` | The folder has no `plan-*.md` files. |
| `<file>: the last line must be 'Next plan: <path\|none>'` | Fix the last line of that plan. |
| `plan file is not in the chain` | Link that plan from another plan's `Next plan:` line, or remove it. |
| `a plan needs 1 to 3 tasks, found N` | Split or merge tasks. Or use `--one-job`. |
| `<name> line N still has `TBD`` | Finish or remove that text. |
| `<path> has no `## Goal` section` | Add a goal of at least 8 words. |
| `... has no list item; list each check as a bullet` | Add acceptance criteria as bullets. |

If you started a draft with `factory spec start` using the same slug, the draft card is removed when the job is created.

### 6.2 `factory run`: build and merge a job

```
factory run [options] <ID>
```

Drives one job through build, checks, review and merge until it is `merged` or `needs-human`.

| Option | Meaning |
|---|---|
| `--series` | After each merge, continue with the next plan of the series. Stops at the first `needs-human`. |

```bash
factory run 028                 # just this job
factory run 028 --series        # this job, then plan 2, plan 3 ...
factory --project my-app run 028 --series
```

What to know:

- The last line printed is `merged` or `needs-human: <reason>`.
- It never asks questions. It can run for a long time and use many tokens.
- It is resumable. The stage on the board is the only state, so you can run again after a stop.
- Running a job that is already `needs-human` prints the old reason straight away. Use `approve` or `rework` first.
- Running a job that is already `merged` prints `merged`.
- It takes the project lease. If another run holds the project, you get exit code 75 and a message like `project is busy: job N is running ...`. Nothing is done.
- A terminal run ends if you close the terminal. For runs that survive, see `factoryd enqueue` (section 7) or run it under `nohup` or `tmux`.
- If the project settings are broken, it stops with `project is blocked: <reason>`.

### 6.3 `factory status`: see the board

```
factory status
```

It has no options other than the global ones. It never changes anything. Run it whenever you are unsure.

It prints, in order:

1. The lease line: `project: free`, `project: busy, job N (owner O, S s left)` or `project: stale lease from O (job N), parked`.
2. A table, or the text `no jobs`:

| Column | Meaning |
|---|---|
| `ID` | The job id. |
| `STAGE` | spec, build, review, approve, merged or needs-human. |
| `ROUND` | The fix round the builder is on. |
| `REVIEWS` | For example `code:pass`. Words used: pending, running, pass, changes, skipped. |
| `TOKENS` | Weighted tokens used and the share of the budget, for example `12.3K (4%)`. |
| `SERIES` | `index/total`, or `-` for a single job. |
| `SLUG` | The job name. |

3. A `drafts:` list, if there are drafts.
4. For each job waiting on you: `<id> needs a human: <reason>`, a short piece of evidence, and the `next:` commands you can run.

A cancelled job shows in the table as `needs-human` but gets no waiting block.

### 6.4 `factory show`: the full job record

```
factory show <ID>
```

Prints the job as raw JSON: id, slug, stage, round, reason, reviews, series, branch, worktree, tokens, estimate and more. It is meant for debugging. Use `status` for a readable view.

```bash
factory show 028
```

### 6.5 `factory approve`: merge a job that is waiting on you

```
factory approve [options] <ID>
```

Use it after you looked at the change and accept it.

| Option | Meaning |
|---|---|
| `--without-review` | Merge a `needs-human` job even though the code review never ran. |
| `--waive "<reason>"` | Merge a `needs-human` job whose last review asked for changes. The reason is one line, not empty, at most 200 characters. It cannot be combined with `--without-review`. |

```bash
factory approve 028
factory approve 028 --without-review
factory approve 028 --waive "known flake, fixing in a follow-up"
```

A plain approve of a job whose last review asked for changes is refused. `--waive` is the way to merge it anyway. It writes a ledger line, an `operator.approve` event and an entry in `waivers.json` in the job folder, and after the merge it files one ticket for each waived blocker, whether or not `tickets_on_merge` is set. A ticket that cannot be filed never changes the merge result; `factory tickets <id>` retries it. The dashboard asks for the reason in a dialog when you press Approve on such a job.

The job must be in `needs-human` or `approve`. The merge is the same as an automatic one: it merges the reviewed change with a merge commit into your base branch. It never pushes.

Refusals, each exiting with code 1:

| Message | Meaning |
|---|---|
| `job N was cancelled` | Use `rework` to revive it. |
| `job N is in stage X, approve needs needs-human or approve` | The job is not waiting for you. |
| `job N has no code review yet ...` | Use `rework`, or `approve --without-review` to merge anyway. |
| `the review asked for changes ...` | Use `rework`, `cancel`, or `approve --waive "<reason>"` to merge anyway. |
| `job N cannot be approved, the job changed files outside <sub>` | Monorepo projects only. Use `rework` so the builder removes those changes. |
| `job N has a pending batch commit, use rework to finish it` | Use `rework`. |
| `project is blocked: <reason>` | Fix the project settings. |

What it prints: `merged <sha>`, or `parked: <reason>`. A `parked:` result leaves the job in `needs-human` with the new reason, for example `main busy: ...`. It is safe to fix the cause and run `approve` again.

### 6.6 `factory rework`: send the builder a note

```
factory rework <ID> <NOTE>
```

Sends one line to the builder and sends the job back to the build stage. A job parked at a task boundary starts the next round: the round number goes up, the builder gets a fresh set of fix rounds, and it reads `changes-r<N>.md` with your note first and then the open review findings. Earlier review and builder files are kept. A job parked in the middle of a task keeps its round and reads your note from `rework-note.md`.

```bash
factory rework 028 "Handle the empty list case and add a test for it"
```

- The note must not be empty, must be one line, and must be at most 200 characters.
- The job must be in `needs-human`.
- It also revives a cancelled job.
- It does not run the job. It prints `job N back to build, round <N>` for the next round, or `job N back to build, same task` for a job parked mid-task. You must run `factory run 028` afterwards.

### 6.7 `factory cancel`: give up on a job

```
factory cancel [options] <ID-or-draft-name>
```

| Option | Meaning |
|---|---|
| `--reason <text>` | Why. Default: `cancelled by user`. Stored as `cancelled: <reason>`. |
| `--force` | Cancel even though the job holds a live lease. Also clears the project lease held for this job. |

```bash
factory cancel 028
factory cancel 028 --reason "superseded by job 031"
factory cancel 028 --force            # only if the process that owned it is gone
factory cancel login-fix              # a draft name removes the draft card
```

- Cancelling does not delete anything. The branch and worktree stay until you run `cleanup`.
- A cancelled job is stored as `needs-human` with a cancelled flag. `rework` revives it.
- A merged job cannot be cancelled: `job N is already merged`.
- Without `--force`, a job with a live lease is refused: `job has a live lease, use --force to cancel anyway`.
- With a draft name instead of a number, the draft card is removed and its spec file is kept.

### 6.8 `factory cleanup`: remove the worktree and branch

```
factory cleanup [options] <ID>
```

| Option | Meaning |
|---|---|
| `--discard-commits` | Remove the branch even though it has commits that are not in the base branch. |

```bash
factory cleanup 028
factory cleanup 031 --discard-commits   # a cancelled job whose work you do not want
```

- It only works on a merged or cancelled job.
- It removes the git worktree and the job branch. The job folder (ledger, logs, usage figures) always stays.
- A merged job keeps its card on the board.
- A cancelled job also loses its card, unless it belongs to a series with other jobs, holds a live lease, or the project lease is held by it. The output says `card kept on the board: <why>`.
- **`--discard-commits` deletes real work for good.** Use it only on jobs you are throwing away.
- Output is one of: `removed worktree and branch of job N`, `removed leftover folder for job N; ...`, or `nothing to remove for job N, ...`.

### 6.9 `factory spec`: track a feature before it has a job

A draft is a card on the board for a feature you are still specifying. These commands only keep records. They run no agent and write no spec file.

```
factory spec start <SLUG> <FEATURE>
factory spec stage <SLUG> <STAGE>
factory spec resume <SLUG>
```

| Command | Meaning |
|---|---|
| `spec start` | Add a draft at stage `interviewing`. Prints the draft name, the stage, and the spec file path (`exists` or `missing`). |
| `spec stage` | Move a draft. `<STAGE>` is `interviewing`, `writing-spec` or `writing-plans`. Any direction is allowed. |
| `spec resume` | Print the stage and the spec file of a draft. |

```bash
factory spec start login-fix "Fix the login redirect"
factory spec stage login-fix writing-spec
factory spec resume login-fix
```

- The slug must contain a letter or digit and must not be only digits. Digits alone would look like a job id.
- Starting a draft whose name is taken fails: `a draft named X already exists`.
- An unknown name fails: `no draft named X`.
- A draft ends when you run `factory new` with the same slug, or when you run `factory cancel <draft name>`.

### 6.10 `factory tokens`: token use

```
factory tokens <ID>
factory tokens --cache [--days <N>]
```

| Option | Meaning |
|---|---|
| `<ID>` | Show one job. Not allowed together with `--cache`. |
| `--cache` | Show prompt-cache use for the whole project. |
| `--days <N>` | With `--cache`, list only resumed runs of the last N days. N is at least 1. |

```bash
factory tokens 028
factory tokens --cache
factory tokens --cache --days 7
```

**One job.** It prints:

- **totals:** `input`, `cache_creation`, `output`, `cache_read` and `weighted <n> (<pct>% of <budget>)`.
- **by role:** builder, reviewer, committer, message and kb. Each has `runs`, `raw` and `weighted`.
- **runs:** one row per agent run, with the role, provider, round, turns and weighted tokens.

`weighted = input + cache_creation + output + cache_read / 10`.

The line comparing against a "job 001 baseline" uses a fixed figure from another project. A beginner can ignore it.

**`--cache`.** Useful for tuning cost. It shows per-role cache use and a list of resumed runs where the cache had expired. You can skip it at first.

### 6.11 `factory costs`: cost of every job

```
factory costs
```

No options. Prints the weighted-token cost of every job, then statistics per size group, and writes `costs.json` into the project state folder.

Columns of the job table: `JOB`, `STAGE`, `GROUP`, `TASKS`, `LINES`, `FILES`, `WEIGHTED`, `RAW`, `RUNS`.

- `GROUP` is by plan size: `small` under 12 KB, `medium` up to 25 KB, `complex` above that. `-` means no plan.
- `~` marks an approximate figure from old usage lines. `no data` means the job has no runs yet.

The group table shows the median and range for merged jobs. The same history is behind the estimate printed by `factory new`.

### 6.12 `factory log`: read the log

```
factory log [options]
```

Prints the project's structured log, oldest first, one JSON object per line. It only reads.

| Option | Meaning |
|---|---|
| `--job <ID>` | Only lines of this job. Use the full id, for example `028`, not `28`. |
| `--level <LEVEL>` | This level or above: `error`, `warn`, `info`, `debug`, `trace`. |
| `--event <NAME>` | Only events with this name, or that start with it plus a dot. `task` matches `task.start`. |
| `--tail <N>` | Keep the last N lines. Default 100. `0` means all. |

All filters combine (AND). The tail is applied last.

```bash
factory --project my-app log --job 028 --level warn --tail 50
factory --project my-app log --event agent.end
factory --project my-app log --event task --tail 0
```

Fields on each line: `ts` (UTC time), `level`, `scope`, `project`, `event`, `msg`, plus event-specific fields such as `job_id`, `stage`, `reason`, `sha` or `duration_ms`. Secrets are redacted. If nothing has been logged yet it prints `no log yet`.

The default level is `info`. To see debug lines, set `log_level` in `project.toml` or set `FACTORY_LOG=debug`.

Handy filters:

| You want to know | Command |
|---|---|
| Why did job 028 stop? | `factory log --job 028 --event park` |
| Everything that went wrong | `factory log --level warn` |
| Which tasks ran | `factory log --job 028 --event task` |
| What merged | `factory log --event merge` |

### 6.13 `factory tickets`: file follow-up GitHub issues

```
factory tickets [options] <ID>
```

| Option | Meaning |
|---|---|
| `--dry-run` | Print what would happen and change nothing. |

After a job is merged, this files the follow-ups listed in the spec as new GitHub issues. It also comments on and closes the parent issues named in the feature text (for example `#76`). It uses the `gh` CLI, so `gh` must be installed and logged in.

```bash
factory tickets 028 --dry-run
factory tickets 028
```

- It skips, with exit code 0, when the job is not merged, is not the last plan of a series, or the feature text has no issue references.
- It is safe to run twice. A marker file in the job folder stops any call from repeating.
- It never changes the merge result.
- Set `tickets_on_merge = true` in the project settings to run it automatically after every merge. The default is off.

Open `[minor]` findings of the last review become one ticket after the merge, a `- [ ]` checklist titled `Minor review findings from job <id>: <slug>`. `minors.json` in the job folder records the number, so a re-run files nothing twice, and `factory tickets <id>` retries a failed one. This runs whether or not `tickets_on_merge` is set, for every plan of a series. Set `minor_tickets = false` to turn it off.

### 6.14 `factory kb-refresh`: refresh the knowledge base

```
factory kb-refresh --job <ID>
```

| Option | Meaning |
|---|---|
| `--job <ID>` | Required. The merged job whose ledger gets the result. |

Runs the knowledge-base refresh now and ignores the stop marker. It needs `kb_path` set in the project settings. The job must be merged, and the project must not be busy.

```bash
factory --project my-app kb-refresh --job 028
```

### 6.15 `factory project`: manage registered projects

These commands use only the factory home and need no repository.

#### `factory project add`

```
factory project add --preset <PRESET> [options] <PATH>
```

| Option | Meaning |
|---|---|
| `<PATH>` | The repository top folder, or a subfolder of it for a monorepo project. |
| `--preset <PRESET>` | Required. `rust`, `go`, `ruby`, `csharp` or `node`. |
| `--name <NAME>` | The project name. Default: the lowercased folder name. 1 to 40 characters of `a-z`, `0-9` and `-`. |
| `--base <BRANCH>` | The branch jobs start from and merge into. Default: the branch the repository is on now. It must exist locally. |
| `--agent-cli <claude\|codex>` | Which agent program to use. `codex` seeds Codex model names. |

```bash
factory project add ~/code/my-app --preset rust
factory project add ~/code/my-app --preset rust --name my-app --base main
factory project add ~/code/monorepo/services/api --preset go --name api
factory project add ~/code/my-app --preset rust --agent-cli codex
```

- It writes only into the factory home: `~/.factory/projects/<name>/project.toml`, plus empty `agents/`, `state/` and `worktrees/` folders. It seeds the presets if they are missing. **It does not change your repository.**
- Prints `added project <name> (<preset>, base <branch>) at <folder>`.
- A repository in a detached HEAD state needs `--base`.
- Two projects on the same repository may not overlap.
- Errors: `project <name> already exists`, `not a git repository: ...`, `unknown preset "x", choose one of rust, go, ruby, csharp, node`, `base branch X does not exist in the repository`.

What each preset sets up:

| Preset | Sensitive paths (always need you) | Checks it runs |
|---|---|---|
| `rust` | `Cargo.toml`, `Cargo.lock`, `build.rs`, auth, secret, credential files, `.github/` | `cargo check -j4` and `cargo test -j4`, per crate |
| `go` | `go.mod`, `go.sum`, auth, secret, credential files, `Dockerfile`, `.github/` | `go vet ./...` and `go test ./...` |
| `ruby` | `Gemfile`, lock, gemspec, credentials, `master.key`, `db/migrate` | `bundle exec rubocop` and `bundle exec rspec` |
| `csharp` | project and solution files, `Directory.*.props`, `nuget.config`, `appsettings`, migrations | `dotnet build --nologo` and `dotnet test --nologo` |
| `node` | `package.json`, `tsconfig*.json`, `vite.config.*`, `next.config.*`, `.env*`, auth, secret, credential files, `Dockerfile`, `.github/` | `bun run typecheck`, `bun run lint` and `bun test`, for changed `.ts`, `.tsx`, `.js` and `.jsx` files |

The `node` preset covers Node.js and React projects and uses Bun only. It runs your project's own `typecheck`, `lint` and `test` scripts from `package.json`, so the project needs all three. Lockfiles (`bun.lock`, `bun.lockb`) are not sensitive paths.

#### `factory project list`

```bash
factory project list
```

Prints `no projects`, or one line per project: `<name> <preset> <base-branch> <folder>`. A project that cannot be used shows `<name>  disabled: <why>`.

#### `factory project which`

```
factory project which [PATH]
```

Prints the name of the registered project that contains a folder. The path defaults to your current folder. Errors with `no registered project contains <path>` if there is none.

```bash
factory project which
factory project which ~/code/my-app/src
```

#### `factory project show`

```
factory project show <NAME>
```

Prints the resolved settings, one `key: value` per line, and ends with `problems: none` or a list of problems. This is the best check that a project is healthy.

```bash
factory project show my-app
```

#### `factory project remove`

```
factory project remove [--force] <NAME>
```

| Option | Meaning |
|---|---|
| `--force` | Remove even though some jobs are not merged or cancelled, or the project file is broken. |

```bash
factory project remove my-app
factory project remove my-app --force
```

- It deletes only `~/.factory/projects/<name>/`. **Your repository and the presets stay.**
- It refuses while a driver holds the project lease. `--force` does not override this. Use `factory --project <name> cancel <job> --force`, or delete the lease file.
- It never deletes job branches. It lists any it leaves in place.

### 6.16 `factory presets refresh`

```
factory presets refresh [--force]
```

| Option | Meaning |
|---|---|
| `--force` | Overwrite files you edited by hand. |

Brings the seeded presets in `~/.factory/presets/` up to the text built into the binary. Each file ends in one of four states:

| State | Meaning |
|---|---|
| `written` | The file was missing and is now created. |
| `updated` | You had not edited it, so it was replaced with the new text. |
| `unchanged` | It already matches. |
| `refused` | You edited it, so it was kept. `--force` overwrites it. |

```bash
factory presets refresh
factory presets refresh --force      # replaces your hand edits
```

There is no dry-run option here. `factory upgrade --dry-run` shows the preview.

### 6.17 `factory install`

```
factory install [options]
```

| Option | Meaning |
|---|---|
| `--dir <DIR>` | Write only `factory.md` into this folder. No Codex skill is installed. |
| `--force` | Replace an existing file that differs. |

```bash
factory install                              # Claude command and Codex skill under $HOME
factory install --dir ~/.claude/commands     # Claude command only, in a folder you choose
factory install --force                      # replace changed copies
```

- Default targets: `~/.claude/commands/factory.md` and `~/.agents/skills/factory/SKILL.md`.
- Output: `installed <path>` or `already installed: <path>`.
- A differing `factory.md` stops with `<path> exists and differs from the shipped command, use --force to replace it`. For the Codex skill it is only a note, and the command still succeeds.

### 6.18 `factory upgrade`

```
factory upgrade [--dry-run]
```

Run it after every time you install a new `factory` binary.

| Option | Meaning |
|---|---|
| `--dry-run` | Show what would change and write nothing. Changes are shown as `would ...`; files that need nothing show as `unchanged`. |

```bash
factory upgrade --dry-run
factory upgrade
```

In order, it:

1. Checks that the home was not last used by a newer binary. If it was, it refuses and tells you to install a newer binary.
2. Updates `~/.claude/commands/factory.md`. A changed old copy is saved as `factory.md.bak`.
3. Updates the Codex skill, only if it is already installed.
4. Refreshes the presets you have not edited (the same as `presets refresh` without `--force`).
5. Reports `shadowed:` lines for project or repository files that hide the new preset text.
6. Writes the version marker in the factory home.

---

## 7. Command reference: `factoryd`

`factoryd` is a background supervisor. Think of it as a small queue and scheduler that runs `factory` for you.

**Use plain `factory run` when** you work on one project at a time. **Use `factoryd` when** you want:

- several projects to build at once,
- runs queued up and started for you,
- a daily token limit,
- the multi-project dashboard.

How it behaves:

- It runs `factory --project <name> run <job>` as a child process for each request.
- Only one child runs per project, and at most `max_parallel` projects run at once (default 2).
- **Stopping `factoryd` never stops running jobs.** The next `factoryd` adopts them.
- Only one `factoryd` can run per factory home.
- It does not start by itself after a reboot. Start it again when you need it.
- It finds the `factory` program next to itself, or from the `FACTORY_BIN` variable. Link both onto your PATH (section 4.2).

General form: `factoryd <COMMAND>`. It also accepts `-h`/`--help` and `-V`/`--version`.

### 7.1 `factoryd run`: start the supervisor

```
factoryd run [--interval-ms <N>] [--once]
```

| Option | Meaning |
|---|---|
| `--interval-ms <N>` | How often it checks the queue, in milliseconds. Default 2000. Lowest accepted: 50. |
| `--once` | Do one pass and exit. No API and no loop. Prints `started N, finished N, rejected N`. |

```bash
# Start in the background and keep it running after you close the terminal.
nohup factoryd run >> ~/.factory/factoryd.log 2>&1 &

# One pass only, for testing.
factoryd run --once
```

- In normal mode it prints `api listening on <address>`, then loops until you run `factoryd stop`.
- If another `factoryd` already runs for this home, it stops with `factoryd is already running (pid N)`.
- It stops with an error if the factory home was last used by a newer version.
- The factoryd log is `~/.factory/factoryd.log.jsonl`.

### 7.2 `factoryd status`: what is queued and running

```bash
factoryd status
```

No options. It only reads and works whether or not `factoryd` runs. It prints:

1. `daily budget: <used> of <limit> weighted tokens today (<pct>%), resets at <time>`. Only when a daily budget is set.
2. `Queue:` waiting requests, or `(empty)`.
3. `Running:` for each project, `<project> job <job> pid <pid> age <N>s`, or `(none)`.
4. `Results:` the last 10 results, newest first.
5. The factoryd version.

Result outcomes you will see:

| Outcome | Meaning |
|---|---|
| `merged` | The job merged. |
| `needs-human: <reason>` | The job stopped and wants you. |
| `failed (code N) <last error line>` | The child exited with an error. |
| `lost: ...` | The child died without reporting a result. Read the logs and the ledger, then run it again by hand or enqueue again. `factoryd` never restarts a lost job. |
| `rejected: ...` | The request was refused, for example `unknown project`. |

### 7.3 `factoryd stop`

```bash
factoryd stop
```

Writes a stop flag. The supervisor ends after its current pass. **Running jobs keep running.**

- If one is running it prints `stop requested, factoryd pid N will end after its current tick; running jobs keep running`.
- If none is running it prints `no factoryd is running; the stop file is removed when the next one starts`.

### 7.4 `factoryd is-running`

```bash
factoryd is-running; echo $?
```

Prints nothing. Exits 0 when a `factoryd` runs for this home, and 1 when not. It is meant for scripts. The `/factory` command uses it to decide whether to queue the work or run it directly.

### 7.5 `factoryd enqueue`: add a request to the queue

```
factoryd enqueue --project <name> --job <id> [--series] [--approve]
```

| Option | Meaning |
|---|---|
| `--project <name>` | Required. The registered project. |
| `--job <id>` | Required. 1 to 6 digits. |
| `--series` | Run request only: continue with the next plan after each merge. |
| `--approve` | Make it an approve request, not a run request. |

```bash
factoryd enqueue --project my-app --job 028
factoryd enqueue --project my-app --job 028 --series
factoryd enqueue --project my-app --job 028 --approve
```

- It prints the new request id, for example `1761379200000-my-app-028`.
- A request starts `factory --project <name> run <job> [--series]`, or `factory --project <name> approve <job>`.
- `enqueue` does not check that the project exists and does not need `factoryd` to be running. An unknown project is rejected later with the reason `unknown project`. If no `factoryd` runs, the request waits in the queue until one starts.
- `--series` together with `--approve` is refused: `series is only valid for a run request`.
- There is no `--without-review` option here. You can pass it only through the API or by running `factory approve --without-review` yourself.
- If a project is busy, the request waits. A busy project that was started by hand with `factory run` also holds the lease, so the request waits until that run ends. A request that keeps meeting a held lease is retried up to 20 times, then marked failed.

### 7.6 The daily budget

Set `daily_budget_weighted` in `~/.factory/config.toml`. When the day's tokens are used up, new run requests wait. Approvals still start. Running jobs are never stopped. `factoryd status` shows the figure. The `/factory` fallback that runs `factory` directly does not check this budget.

### 7.7 The local API

The API listens on `127.0.0.1:8766` by default (loopback only). It is protected by a secret token in `~/.factory/api.token`. The dashboard reads that file on its own server side.

**Keep the token private. Do not paste it into chat, tickets or screenshots.**

You normally do not call the API yourself. The dashboard does.

### 7.8 Factory home settings: `~/.factory/config.toml`

An optional file. Unknown keys are rejected.

| Key | Meaning | Default |
|---|---|---|
| `max_parallel` | Projects that may run at once. 1 to 64. | 2 |
| `listen` | API address. Loopback only. | `127.0.0.1:8766` |
| `daily_budget_weighted` | Daily token limit for all projects. | unset (no limit) |
| `draft_stale_hours` | Hours before an idle draft is flagged stale. An older `factoryd` rejects this key. | 24 |

---

## 8. The dashboard

A local web page that shows the board. Build it once (section 4.1).

**All projects (recommended).** Needs `factoryd` running.

```bash
nohup factoryd run >> ~/.factory/factoryd.log 2>&1 &
dashboard/target/debug/factory-server --home ~/.factory --port 8765
```

Open http://127.0.0.1:8765/. You see a tab for each project. From the page you can create a job and approve, rework or cancel one.

**Old single-folder mode.**

```bash
dashboard/target/debug/factory-server --root /path/to/repo --port 8765
```

This mode is read-only and reads the old in-repository `.superpowers/` folder. **For a project you registered with `factory project add`, the board lives in the factory home, so this mode will look empty. Use `--home`.**

| Option | Meaning |
|---|---|
| `--home <dir>` | Show every project of a factory home. Needs `factoryd`. Cannot be combined with `--root`. |
| `--root <dir>` | Show one repository, read-only, old layout. |
| `--api <host:port>` | Where `factoryd`'s API is. Default `127.0.0.1:8766`. Loopback only. |
| `--port <N>` | The port for this page. Default 8765. |
| `--draft-stale-hours <N>` | Hours before an idle draft shows as stale. Default 24. |

Each job card shows: the job id, a `plan k of N` chip for a series, the feature, the branch, the builder round, review chips, time in the stage, tokens against the budget (amber at 80%, red at 100%), and ready-to-copy approve, rework and cancel commands. A job waiting on you has a button that shows what went wrong.

If you see `token_rejected`, `factoryd` refused the token. If you see `api_unreachable`, `factoryd` is not running.

---

## 9. When a job stops: what to do

When a job says `needs-human`, run:

```bash
factory --project my-app status
```

It shows the reason, some evidence and the `next:` commands that are valid for that reason. This table covers the reasons you will meet.

| Reason shown | Plain meaning | What you can do |
|---|---|---|
| `risky diff: risky paths (K): ...` | The change touched a sensitive file. This always needs you. | Read the diff. Then `approve`, or `rework` with a note. |
| `checks failed after N fix rounds` | The tests still fail after the fix rounds. | `rework` with a hint. |
| `review asked for changes after N fix rounds` | The reviewer still wants changes. | `approve` if you disagree, or `rework`. |
| `diff has N changed lines, the cap is C` | The change is too big to auto-merge. | `approve` after reading it, or `rework` to split it up. |
| `budget is stopped` | The job used up its token budget. | `approve` or `cancel`. Raise `budget_weighted` if the job is legitimately large. |
| `builder stopped: ...` | The builder ended early. | `rework`. |
| `builder blocked/needs context ...` | The builder needs information. | `rework` and give it in the note. |
| `builder made no changes` or `task N changed nothing` | Nothing was produced. | `rework`, or `cancel`. |
| `builder committed on its own ...` | The builder broke the "no commits" rule. | `rework`. |
| `builder changed <file>` | The builder touched a protected file. | `rework`, or `cancel`. |
| `no check ran for N changed code path(s)` | No check covers the files that changed. | `approve` if you accept that, or `rework`. |
| `no test was added in ...` | `require_tests` is on and the change has no test. | `approve` or `rework`. |
| `main busy: the main checkout has tracked changes` | Your own checkout has uncommitted edits to tracked files, so the merge cannot happen. | Commit or stash those changes, then `approve` again. Untracked files are fine. |
| `main busy: the main checkout is not on <base>` | Your checkout is on another branch. | Switch to the base branch, then `approve`. |
| `another project is merging into this repository, approve again to retry` | Two projects share one repository. | Run `approve` again. |
| `merge conflict in <paths>` or `sync conflict in <paths>` | The base branch moved and the change conflicts. | Look at the job's worktree, or `rework` with a note. |
| `main moved again after 2 syncs` | The base branch kept changing during the merge. | Run `approve` again. |
| `the job changed files outside <subfolder>` | Monorepo only. The change left its folder. | `rework`, so the builder removes those changes. |
| `no plan.md in the job folder` | The job has no plan. | Put `plan.md` in the job folder, then `run` again. |
| `run loop did not settle, see the ledger` | The job looped too many times. | Read the ledger (`progress.md` in the job folder), then `rework` or `cancel`. |

After you pick a path:

- `approve` merges the job straight away.
- `rework` sends the job back to build. Then run `factory run <id>`.
- `cancel` stops the job. Then run `factory cleanup <id>` when you are ready to remove its files.

---

## 10. Recipes

**Run a multi-plan feature and walk away.** Start `factoryd` once, then queue the work:

```bash
nohup factoryd run >> ~/.factory/factoryd.log 2>&1 &
factory --project my-app new login-fix "Fix login redirect" \
  --spec docs/superpowers/specs/2026-10-25-login-fix-design.md \
  --plans docs/superpowers/plans/2026-10-25-login-fix/
factoryd enqueue --project my-app --job 028 --series
factoryd status
```

**Check on a running factory.**

```bash
factoryd is-running; echo $?
factoryd status
factory --project my-app status
```

**Understand why a job stopped.**

```bash
factory --project my-app status
factory --project my-app log --job 028 --level warn --tail 50
```

**Retry after fixing your checkout.** The job parked with `main busy`:

```bash
git -C ~/code/my-app stash
factory --project my-app approve 028
git -C ~/code/my-app stash pop
```

**Throw a job away completely.**

```bash
factory --project my-app cancel 031 --reason "wrong approach"
factory --project my-app cleanup 031 --discard-commits
```

**See what the work cost.**

```bash
factory --project my-app tokens 028
factory --project my-app costs
```

**Register a monorepo service.**

```bash
factory project add ~/code/monorepo/services/api --preset go --name api
factory project show api
```

**Update the factory itself.**

```bash
cd ~/data/rocket/rocket-factory
git pull
cargo build -j4 -p factory-cli -p factory-supervisor
factoryd stop                    # wait for running jobs to finish
factory upgrade --dry-run
factory upgrade
nohup factoryd run >> ~/.factory/factoryd.log 2>&1 &
```

**Preview the follow-up tickets of a merged job.**

```bash
factory --project my-app tickets 028 --dry-run
```

---

## 11. Settings that matter most

Project settings live in `~/.factory/projects/<name>/project.toml` under `[settings]`. The full list, with defaults and examples, is in `docs/reference/project-toml-reference.md`. These are the ones a new user meets first.

| Setting | Meaning | Default |
|---|---|---|
| `base_branch` | The branch jobs start from and merge into. | set by `project add` |
| `budget_weighted` | Token budget per job. The agent is stopped at the limit and a warning shows at 80%. | 1,000,000 |
| `fix_rounds` | Fix rounds the builder gets after a failed check or review. | 2 |
| `diff_cap_lines` | A change above this many lines never auto-merges. | 4000 |
| `builder_model`, `reviewer_model`, `commit_model`, `kb_model` | The model each role uses. | `sonnet` |
| `risk_paths` | File patterns that always need your approval. | from the preset |
| `checks` | Which commands run for which changed files. | from the preset |
| `require_tests` | Park a change that adds no test. | false |
| `tickets_on_merge` | File follow-up tickets after each merge. | false |
| `minor_tickets` | File one ticket for the open minor review findings after each merge. | true |
| `kb_path` | A knowledge base folder. Unset turns the knowledge base off. | unset |
| `log_level` | How much the log records. | `info` |

If the project settings are missing or broken, the factory treats every change as risky. It never falls back to a permissive setting.

Check your project after any edit with `factory project show <name>`.

## 12. Where files live

```
~/.factory/                              the factory home ($FACTORY_HOME overrides it)
  config.toml                            optional factoryd settings
  VERSION                                version marker
  api.token                              the API secret, keep private
  factoryd.log.jsonl                     the supervisor log
  presets/<preset>/                      seeded rules and agent files
  projects/<name>/
    project.toml                         the project's settings
    agents/                              optional agent overrides
    worktrees/                           job worktrees
    state/
      board.json                         the board
      events.jsonl                       agent and tool events
      log.jsonl                          the structured log (what `factory log` reads)
      costs.json                         written by `factory costs`
      project.lease                      who owns the project right now
      jobs/<date>-<id>-<slug>/           one folder per job
        spec.md, plan.md                 copies made by `factory new`
        progress.md                      the ledger
        park.json                        why the job stopped
        usage.jsonl                      token use, one line per agent run
  queue/<request>.json                   waiting factoryd requests
  results/<request>.json                 finished factoryd requests
  run/                                   factoryd lock, stop flag and child records
  logs/<project>/<request>.out|.err      output of factoryd's child runs
```

In your repository, the factory also reads:

- `docs/superpowers/specs/` and `docs/superpowers/plans/` (where specs and plans go).
- `.factory/agents/` on the base branch (optional agent file overrides).

Environment variables:

| Variable | Meaning |
|---|---|
| `FACTORY_HOME` | The factory home. Default `~/.factory`. |
| `FACTORY_BIN` | Path to the `factory` program that `factoryd` starts. |
| `FACTORY_LOG` | Log level override, for example `debug`. |
| `FACTORY_LOCK_WAIT` | Seconds to wait for a lease. Default 15. |
| `FACTORY_ALLOW_API_KEY` | Set to `1` to let agents bill an `ANTHROPIC_API_KEY` instead of your normal Claude login. By default the factory removes API keys from the agent environment. |

## 13. Troubleshooting

| Symptom | Cause and fix |
|---|---|
| `factory: command not found` | The program is not on your PATH. See section 4.2. |
| `factory: neither FACTORY_HOME nor HOME is set` | Set `HOME` or `FACTORY_HOME`. |
| `no registered project contains <path>` | Run `factory project add` for it, or pass `--root`. |
| `project is busy: job N is running ...` (exit 75) | Another run holds the project. Wait. If the process is gone, run `factory cancel N --force`, or delete `project.lease` in the state folder. |
| `project is blocked: <reason>` | The settings are broken. Run `factory project show <name>` and read the `problems:` block. |
| `this home was last used by factory X, this binary is Y` | Install a newer binary, or delete the `VERSION` file to ignore the check. The usual cause is that the `factory` on your PATH was not rebuilt while a newer one raised the marker (section 4.2). Build again in your clone and run `factory upgrade`. |
| `factoryd is already running (pid N)` | One `factoryd` per home. Use `factoryd status`, or `factoryd stop` first. |
| `job N was cancelled` when approving | Run `factory rework N "<note>"` to revive it. |
| `job N is in stage X, rework needs needs-human` | `rework` only works on a stopped job. |
| `no log yet` | Nothing has been logged for the project. |
| The dashboard board looks empty | Use `--home`, not `--root`, for registered projects. See section 8. |
| Dashboard shows `token_rejected` | `factoryd` does not accept the token. Make sure `factoryd` uses the same home. |
| A `factoryd` request ends `lost: ...` | The child died silently. Read `~/.factory/logs/<project>/<request>.err` and the job's `progress.md`, then run again. |
| `refused` files after `presets refresh` | You edited them. Use `--force` only if you want to lose your edits. |

For help on any command, add `--help`:

```bash
factory --help
factory new --help
factory project add --help
factoryd enqueue --help
```

## 14. Quick reference card

```bash
# Setup (once)
factory install
factory project add <repo> --preset rust|go|ruby|csharp|node [--name N] [--base B] [--agent-cli claude|codex]
factory project show <name>

# Draft a feature
factory spec start <slug> "<feature>"
factory spec stage <slug> interviewing|writing-spec|writing-plans
factory spec resume <slug>

# Build a feature
factory new <slug> "<feature>" [--spec FILE] [--plans FILE|DIR] [--one-job [--accept-over-budget]] [--model M]
factory run <id> [--series]
factory status
factory show <id>

# When it stops
factory approve <id> [--without-review]
factory rework <id> "<one line note>"
factory cancel <id> [--reason TEXT] [--force]
factory cleanup <id> [--discard-commits]

# Look back
factory log [--job ID] [--level L] [--event E] [--tail N]
factory tokens <id>        factory tokens --cache [--days N]
factory costs
factory tickets <id> [--dry-run]
factory kb-refresh --job <id>

# Maintenance
factory project list | which [PATH] | remove <name> [--force]
factory presets refresh [--force]
factory upgrade [--dry-run]

# Background supervisor
factoryd run [--interval-ms N] [--once]
factoryd status
factoryd stop
factoryd is-running
factoryd enqueue --project <name> --job <id> [--series | --approve]

# Any command, any folder
factory --project <name> <command>
factory --root <path> <command>
```
