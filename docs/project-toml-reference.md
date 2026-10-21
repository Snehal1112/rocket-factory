# project.toml reference

Everything you can set in a registered project's `project.toml`, with defaults, validation, where each key takes effect and examples. Verified against the code on 2026-10-19 (commit `cfd6c64`). Source files are named so you can re-check. For the pipeline itself, read `software-factory.md`.

## Where the file lives

`<factory home>/projects/<name>/project.toml`. The factory home is `$FACTORY_HOME`, else `~/.factory`. Create it with `factory project add <path> --preset <name>`, then edit it by hand.

A project is a registration, not a copy. The repo stays where it is. The state folder (board, events, jobs) and the worktrees sit next to `project.toml` in the project folder.

## File structure

```toml
repo   = "/abs/path/to/checkout"   # required, absolute
preset = "rust"                    # required, "rust", "go", "ruby" or "csharp"

subpath = "apps/web"               # optional, only for a project in a subfolder of the repo

[settings]                         # optional, every key below is optional
base_branch = "main"
```

Rules:

- `subpath` is the project folder relative to the git top folder `repo`, with `/` separators. Absent means the project is the repo top folder. `factory project add <repo>/apps/web` writes it. It must be a relative path without `..` and must name an existing folder, otherwise the project shows as disabled. A job may change only files inside this folder, the agents start there, and every path in `risk_paths` and `[[checks]]` (`paths`, and the working directory of the commands) is relative to it, not to the repo root. A project without `subpath` is unchanged.
- Only `repo`, `preset`, `subpath` and `[settings]` are allowed at the top level. Any other key makes the file unreadable (`factory-core/src/project.rs:37`).
- `[settings]` takes any `Config` key from the next sections. An unknown key or a wrong type makes the project fail closed (see "Fail closed").
- `factory project add` writes only `repo`, `preset`, `subpath` (for a subfolder) and `base_branch`. Everything else is added by you.

## How settings combine

Resolution order, last one wins (`factory-core/src/resolve.rs`):

1. Built-in defaults for every missing key.
2. The preset, `<home>/presets/<preset>/config.toml`.
3. `[settings]` in `project.toml`. A value here replaces the preset's value for that key, lists included. Nothing in `[settings]` is additive.
4. The repo file `.factory/override.toml`, read from the tip of the base branch. It can only add to `risk_paths` and `checks`.
5. `.factory/**` is always added to `risk_paths`.

The repo's `.claude/factory/config.toml` is not a layer for a registered project. It is only used by the old in-repo layout.

Because lists are replaced, copy the preset's entries into your list when you want to keep them. Put them first, then your own.

## Settings reference

Defaults come from `Config::default()` in `factory-core/src/config.rs`. A value of `0` where "above 0" is required makes the config invalid.

### Limits and rounds

| Key | Type | Default | Effect |
|---|---|---|---|
| `budget_weighted` | integer | 1000000 | Cap on weighted tokens for the whole job, summed over all runs. The runner kills the agent when the job reaches it and warns at 80%. Above 0. |
| `diff_cap_lines` | integer | 4000 | Cap on changed lines of the whole job diff. A bigger diff parks as `diff-too-large`. Above 0. |
| `fix_rounds` | integer | 2 | Fix rounds allowed after a failure. When used up, failing checks park as `checks-failed` and review changes park as `changes-remain`. `0` parks on the first failure. |
| `stall_minutes` | integer | 10 | Minutes without any stream event before the driver kills the agent. Applies to builder, reviewer, committer, KB and message agents. The check is approximate. |

Weighted tokens are `input + cache_creation + output + cache_read / 10` (`factory-core/src/budget.rs`). When a builder is killed on budget it parks as `builder-stopped`. `budget-stopped` is only used when a reviewed round finds the budget gone.

### Models

| Key | Type | Default | Effect |
|---|---|---|---|
| `agent_cli` | `"claude"` or `"codex"` | unset, which means `"claude"` | The CLI that runs every agent of the project. `"codex"` runs them through `codex exec --json`, with the limits in "Using Codex" in `docs/software-factory.md`. Any other value makes the config invalid. `factory project add <path> --agent-cli codex` writes it with `gpt-5.6-luna` for the four model keys and no `cache_ttl`. |
| `builder_model` | string | `"sonnet"` | `--model` for every builder run. Not blank. |
| `reviewer_model` | string | `"sonnet"` | Reviewer model. Not blank. |
| `reviewer_model_large` | string | unset | A stronger reviewer. Used when the diff is risky, or the round's changed lines are strictly above `reviewer_large_over_lines`. Unset turns the switch off. Not blank when set. |
| `reviewer_large_over_lines` | integer | 800 | The threshold above. It counts the round diff, not the whole job diff. Above 0. |
| `commit_model` | string | `"sonnet"` | Committer and the merge and sync message agents. Not blank. |
| `kb_model` | string | `"sonnet"` | KB refresher. Not blank. |

The choice of reviewer model is written to the ledger as `review model: ...`. Weighted tokens ignore the model, so a stronger model's extra cost does not show in `factory tokens`.

### Commits

| Key | Type | Default | Effect |
|---|---|---|---|
| `commit_budget_weighted` | integer | 60000 | Cap on one committer run. The batch run gets double. Above 0. |
| `commit_subagents` | bool | false | Changes only the committer prompt: use the skill's subagents, or work inline. |
| `commit_batch` | bool | true | The driver makes a `wip:` snapshot commit per task, then one committer runs after the last task. `false` runs one committer per task. |

### Builder prompt

| Key | Type | Default | Effect |
|---|---|---|---|
| `builder_excerpt` | bool | true | A task builder gets the plan header, its own task and the spec goal and acceptance criteria. It falls back to the whole plan, with a notice event, when the plan has no `### Task N:` section for that task. |
| `builder_carry_session` | bool | false | Resume the previous task's builder session for the next task of a plan while the prompt cache is alive. |
| `builder_carry_max_ctx` | integer | 100000 | The largest end-of-task context that may be carried. Must be above 0. |
| `read_first` | string list | `[]` | Files or folders the builder is told to read first. They are plain text in the prompt, relative to the job worktree, and the driver does not check them. Used in the first builder prompt and the per-task builder prompt, not in resume or fix prompts and not for the reviewer. |
| `builder_rules` | string list | `[]` | Joined into one `Rules:` line in the first builder prompt and the per-task builder prompt. Not in the resume prompt. |
| `check_hints` | string list | `[]` | Example check commands, shown only in the resumed fix-builder prompt, as `Run the relevant checks (for example ...)`. They do not run anything. |
| `builder_permissions` | bool | true | Each builder run gets permission mode `acceptEdits`, an allowlist of tools and the job folder as an extra directory, so it never waits on a prompt. `false` restores the old builder arguments. |
| `builder_allow` | string list | `[]` | Extra allow rules such as `Bash(make:*)`, added after the derived ones. Rules for the programs in `git`, `rm`, `rmdir`, `curl`, `sudo`, `sh`, `bash`, `env` and `xargs` are refused, as are shell operators; with a bad entry the whole config fails closed. |
| `reviewer_rules` | string list | `[]` | Joined into one line in the reviewer prompt, before the review checks. The prompt only, nothing enforces them. |
| `plan_rules` | string list | `[]` | Rules for plans. Only the global `/factory` command reads them, through `factory project show`. The driver never puts them in a prompt, and the plan lint does not check them. |

For all five string lists, every entry must be one non-empty line, with no newline.

### Builder permissions

With `builder_permissions = true`, the builder's allowlist is built in this order:

- A fixed base: the tools `Read`, `Grep`, `Glob`, `Skill` and `Agent`, the programs `cd`, `ls`, `cat`, `head`, `tail`, `wc`, `grep`, `find` and `pwd`, and `git status`, `git diff`, `git log` and `git show`.
- The programs of the `[[checks]]` commands and of `check_hints`, each as `Bash(<program>:*)`. The `rust` preset gives `Bash(cargo:*)`. Programs on the never list below (`git`, `rm`, `rmdir`, `curl`, `sudo`, the shells and wrappers such as `env`, `xargs` and `timeout`) are never derived.
- The `builder_allow` entries, last.

Each `builder_allow` entry is `Bash(<command>:*)` or `Bash(<command>)`, where the command may hold arguments, such as `Bash(git log:*)`. It is one line, at most 200 characters, with no shell operators (`&&`, `||`, `;`, `|`, `&`, `>`, `<`, a backtick or `$(`), no quotes, no backslashes and no wildcards (`*` or `?`). `git` is accepted only before `status`, `diff`, `log`, `show` or `rev-parse`.

The never list is `git`, `rm`, `rmdir`, `curl`, `sudo`, the shells (`sh`, `bash`, `zsh`, `dash`, `ksh`, `fish`, `busybox`), `source`, `.`, `eval`, `exec`, `command` and the wrappers `env`, `xargs`, `timeout`, `nice`, `nohup`, `time`, `stdbuf` and `setsid`. None of them is derived or accepted, because a wrapper can run any other command.

Derived programs hold only letters, digits and `_ - . /`. Programs and entries that repeat are dropped, so duplicates keep only their first position. A compound check command such as `cd web && bun test` derives only its first word, `cd`.

```toml
builder_allow = ["Bash(make:*)"]
```

The builder also gets deny rules for `Edit` and `Write` on the job folder, so it cannot change the plan, the spec or the ledger, and on the knowledge base folders.

### Merge and tickets

| Key | Type | Default | Effect |
|---|---|---|---|
| `base_branch` | string | `"main"` | Jobs branch from it and merge into it. Must be a valid git branch name and must not start with `factory/`. |
| `tickets_on_merge` | bool | false | After a merge, file follow-up tickets and close the parent tickets with `gh`. A failure never changes the merge. |
| `log_level` | string | `"info"` | Level of the JSON log file `log.jsonl`: `error`, `warn`, `info`, `debug` or `trace`. Any other value is a config error. The environment variable `FACTORY_LOG` (for example `debug` or `factory_engine=debug,info`) overrides it; an invalid value is ignored with a note. |
| `risk_paths` | string list | 17 built-in globs | Diffs touching these paths stop for a human after review. Replaces the preset list. See "Globs". |

### Checks and gates

| Key | Type | Default | Effect |
|---|---|---|---|
| `checks` | list of tables | `[]` | Maps changed paths to check commands. See "Checks". |
| `code_paths` | string list | `[]` | What counts as code. Empty means every path that is not a doc path. A non-empty list replaces the default. |
| `test_paths` | string list | `[]` | Paths that count as tests. Empty means the built-in list. |
| `test_markers` | string list | `[]` | Substrings of an added line that mark a test. Empty means the built-in list. |
| `require_tests` | bool | false | `true` parks a code diff that adds no test, as `no-tests`. `false` writes a ledger note and tells the reviewer to treat missing tests as a blocker. |

Built-in values when the lists are empty (`factory-core/src/gate.rs`, `risk.rs`):

- Test paths: `**/tests/**`, `**/__tests__/**`, `**/*.test.ts`, `**/*.test.tsx`, `**/*.spec.ts`, `**/*_test.rs`, `**/*_test.go`.
- Test markers: `#[test]`, `#[tokio::test]`, `#[cfg(test)]`, `describe(`, `it(`, `test(`, `func Test`. Only added lines (`+`) count.
- Doc paths, never code: a path that ends in `.md` or starts with `docs/` or `.claude/`.
- A code diff that gets no check at all parks as `no-checks`, unless the diff is risky or has no code paths.

### Knowledge base

| Key | Type | Default | Effect |
|---|---|---|---|
| `kb_path` | string | unset | The KB folder, absolute or starting with `~/`. Unset turns the KB off. When set, builder and reviewer prompts get a KB line and the folder is added as readable, and a refresh runs after each merge. The folder is not checked at load. Not blank when set. |
| `kb_budget_weighted` | integer | 150000 | Cap on one refresh run. Above 0. |
| `kb_auto_commit` | bool | false | Parsed only. Nothing reads it, and the refresher is told not to commit. |

Turn the KB on only after one manual `factory kb-refresh --job <id>` went well.

### Plan size groups

| Key | Type | Default | Effect |
|---|---|---|---|
| `size_small_max_bytes` | integer | 12288 | A plan under this many bytes is small. Above 0 and below the medium value. |
| `size_medium_max_bytes` | integer | 25600 | Up to this size is medium, bigger is complex. |

These only group jobs in `factory costs` and in the estimate that `factory new` prints.

### Prompt cache

| Key | Type | Default | Effect |
|---|---|---|---|
| `cache_ttl` | `"5m"` or `"1h"` | unset, but `factory project add` writes `"5m"` | Prompt cache lifetime of every agent run. `"5m"` sets `FORCE_PROMPT_CACHING_5M=1`. `"1h"` sets `ENABLE_PROMPT_CACHING_1H=1`. Unset lets `claude` choose, which is 1 hour on a subscription login. |

A 1 hour write costs twice a plain input token and a 5 minute write costs 1.25 times, so `"5m"` suits runs that rarely resume. `"1h"` does not beat a `FORCE_PROMPT_CACHING_5M` exported in the driver's own environment. Each `usage.jsonl` line has `cache_creation_1h` to check the effect.

## Checks

`checks` is an array of tables. Write it as `[[settings.checks]]` in `project.toml`.

```toml
[[settings.checks]]
paths    = ["crates/*/**"]
commands = ["cargo check -j4 -p {crate}", "cargo test -j4 -p {crate}"]
```

How it works (`factory-checks/src/plan.rs`):

- Every rule whose `paths` match a changed file applies. It is not first-match.
- `{crate}` is the folder after the first `crates` segment of a matching path. With several crates the command runs once per crate, in alphabetical order. A command that uses `{crate}` but has no matching crate runs nothing.
- Duplicate commands run once.
- Every command runs, even after a failure. The default timeout is 900 seconds.
- A failure sends the job back to the builder for a fix round, or parks it when `fix_rounds` is used up.
- Matching is case-sensitive here, unlike `risk_paths`.

Command validation (checked when the engine starts and by `factory project show`):

- An empty command is rejected.
- A `cargo` command must not use `--workspace` or `--all`, and must contain `-j4` or `-j 4`. `-j 3` and `--jobs 4` are rejected. `--all-targets` is fine.
- Other programs pass unchecked.

## Globs

Used by `risk_paths`, `checks`, `code_paths` and `test_paths` (`factory-core/src/risk.rs`).

- The pattern matches the whole repo-relative path, split on `/`.
- `*` and `?` stay inside one segment. `**` as a whole segment matches zero or more segments. A trailing `**` needs at least one segment, so `a/**` does not match `a`.
- Other characters are literal, including `[...]`.
- `risk_paths`, `code_paths` and `test_paths` ignore case. `checks` does not.

## The repo override file

`.factory/override.toml` in the repo can add risk paths and checks without touching `project.toml`. It must be committed on the base branch, because the driver reads it from the branch tip. A file on a feature branch or uncommitted is ignored.

```toml
risk_paths = ["db/migrations/**"]

[[checks]]
paths    = ["dashboard/**"]
commands = ["cargo check -j4 --manifest-path dashboard/Cargo.toml"]
```

- Only `risk_paths` and `checks` are allowed. Any other key, bad TOML or a wrong type makes the whole config fail closed.
- The lists are appended after the preset and project lists. Nothing can be removed.
- `.factory/**` is always risky.

## Fail closed

Any problem in the layers gives a config where every path is risky and `degraded` is true, with the base branch kept. A degraded project is blocked: commands that start agents or merge refuse to run. Causes include an unknown key, a wrong type, an invalid value, a bad preset name, a broken preset file and a bad override file. Run `factory project show <name>` and read `problems:` and `status:`.

## Other files in the project folder

- `agents/factory-*.md`: agent file overrides. Order is preset, then project, then the repo's `.factory/agents/`, later wins by file name. Names must be `factory-<a-z0-9->.md`.
- `commit-context.md`: used for the committer, else the preset's copy.
- `state/`, `worktrees/`: written by the driver. Do not edit.

## Not in project.toml

- Home settings in `<home>/config.toml`, read by `factoryd`. Missing file means defaults, unknown keys are errors.

  | Key | Default | Validation |
  |---|---|---|
  | `max_parallel` | 2 | 1 to 64 |
  | `listen` | `127.0.0.1:8766` | loopback address, port 0 picks a free one |
  | `daily_budget_weighted` | none | above 0, a UTC-day total over all projects |
  | `draft_stale_hours` | 24 | above 0 |

- Environment variables: `FACTORY_HOME`, `FACTORY_LOCK_WAIT` (seconds, default 15), `FACTORY_BIN` (the `factory` binary `factoryd` starts), `FACTORY_ALLOW_API_KEY=1` (let agents bill an API key instead of the login).
- Fixed in code, not configurable: turn limits (builder 120, reviewer 40, committer 40, KB 80), the tool allowlists, the series cap of 6 plans, the lease timings (30 minute TTL, 60 second heartbeat).

## Examples

### Minimal

```toml
repo   = "/home/me/code/app"
preset = "rust"

[settings]
base_branch = "main"
```

### Cheaper runs, 5 minute cache

```toml
repo   = "/home/me/code/app"
preset = "rust"

[settings]
base_branch = "main"
cache_ttl = "5m"
fix_rounds = 1
commit_model = "haiku"
```

### Stronger review for big or risky diffs

```toml
[settings]
reviewer_model_large = "opus"
reviewer_large_over_lines = 500
```

### Replace a list and keep the preset entries

The rust preset's `risk_paths` are `**/Cargo.toml`, `**/Cargo.lock`, `**/build.rs`, `**/*auth*`, `**/*secret*`, `**/*credential*` and `.github/**`. Setting the key replaces them, so list them again.

```toml
[settings]
risk_paths = [
  "**/Cargo.toml",
  "**/Cargo.lock",
  "**/build.rs",
  "**/*auth*",
  "**/*secret*",
  "**/*credential*",
  ".github/**",
  "src/billing/**",
]
```

### Custom checks, replacing the preset's

```toml
[[settings.checks]]
paths    = ["crates/*/**"]
commands = ["cargo check -j4 -p {crate}", "cargo test -j4 -p {crate}"]

[[settings.checks]]
paths    = ["src/**"]
# Half the workers: the full suite saturates the CPU and heavy dialog tests then time out.
commands = ["yarn tsc --noEmit", "yarn test --run --maxWorkers=50%"]
```

### Rules for builder, reviewer and plans

```toml
[settings]
read_first    = ["CLAUDE.md", "docs/architecture.md"]
builder_rules = [
  "Run `cargo check -j4` and `cargo test -j4 -p <crate> <test_name>`; never use --workspace or --all.",
  "No unwrap in production Rust.",
  "Use the Layout type for every path.",
]
check_hints    = ["cargo check -j4", "cargo test -j4 -p <crate> <test_name>"]
reviewer_rules = ["Judge correctness, error handling and tests against the rules in CLAUDE.md."]
plan_rules     = ["Every plan names a Review Focus section.", "Each task lists the tests it adds."]
```

`read_first` entries are added to the builder prompt of every task, so keep the list short. Do not copy `CLAUDE.md` text into `builder_rules` when `read_first` already names it.

### Strict test gate

```toml
[settings]
require_tests = true
test_paths    = ["**/tests/**", "**/*_test.rs"]
```

### Knowledge base on

```toml
[settings]
kb_path = "~/data/rocket/Nl-knowledge-base/rocketapi"
kb_budget_weighted = 150000
```

### Tickets closed on merge

```toml
[settings]
tickets_on_merge = true
```

Put `Fixes` style issue numbers in the `factory new` feature text. A job that only helps an issue should be created without the number, so the merge does not close it.

## Checking your edit

```bash
factory project show <name>     # resolved settings, plan rules, "problems:" and "status:"
factory project list            # a broken project shows as "disabled: <why>"
```

A new key needs a `factory` and `factoryd` build that knows it. After a rebuild, restart `factoryd` when no job is running, because the running copy keeps the old key list.
