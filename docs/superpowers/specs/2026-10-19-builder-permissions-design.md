# Builder permissions

## Goal

Make the driver give every builder run its own permission mode, tool allowlist and job-folder access, so a job never blocks on a permission prompt and no repository needs a settings file.

## Overview

The builder runs headless as `claude -p`. Nothing can answer a permission prompt, so any Edit, Write or Bash call that no settings file allows is refused, and the builder replies `Status: BLOCKED`. This happened on the first job of a fresh project (edits and `cargo test` refused) and again in a fix round (the builder could not read `changes-r2.md` in the job folder). Both stopped a job in `needs-human` and cost tokens: job 001 spent 43K on the blocked retry alone. Today the fix is a hand-written `.claude/settings.json` in every repository and a copy into every existing worktree.

A probe on 2026-10-19 found a third cause: in headless mode `claude` ignores the `permissions.allow` entries of a repository `.claude/settings.json` ("this workspace has not been trusted") until the folder is trusted. Every new worktree is a new folder, so a settings file cannot be relied on at all. Flags on the command line do not depend on trust.

The driver already sends `--permission-mode`, `--allowedTools` and `--add-dir` for other roles. The builder gets none of them.

## Current state

| Area | Today | Reference |
|---|---|---|
| Builder specs | Three: first, per task, resume. Each sets `agent`, `max_turns`, `strict_mcp`, `tools = BUILDER_TOOLS`, `add_dirs = kb_dirs()` | `crates/factory-engine/src/specs.rs:98-105`, `:190-197`, `:224-229` |
| Builder tools | `Bash,Read,Edit,Write,Grep,Glob,Skill,Agent` | `crates/factory-engine/src/specs.rs:19` |
| Permission fields | `AgentSpec` has `permission_mode`, `allowed_tools`, `add_dirs`; no field for denied tools | `crates/factory-agent/src/spec.rs:21-49` |
| Flag building | `args()` emits `--permission-mode`, `--allowedTools`, `--tools`, one `--add-dir` per directory | `crates/factory-agent/src/spec.rs:82-117` |
| KB run | Only role with `permission_mode = "dontAsk"` and an allowlist | `crates/factory-engine/src/kb.rs:74-77` |
| Reviewer, message | Read-only allowlists | `crates/factory-engine/src/specs.rs:322`, `crates/factory-engine/src/message.rs:114` |
| Committer | `tools` only | `crates/factory-engine/src/commit.rs:158-169` |
| Check commands | `[[checks]]` rules hold `commands`; `check_hints` holds free text | `crates/factory-core/src/config.rs:32-35`, `:85` |
| Job folder | `Engine.job_dir` holds `spec.md`, `plan.md`, `progress.md`, `changes-rN.md` | `crates/factory-engine/src/specs.rs:55-58` |
| Spec test | `builder_specs_are_strict_and_name_their_tools` | `crates/factory-engine/tests/build_checks.rs:1319` |
| Arg echo | `fake-claude` mode `args` returns its argument list | `crates/factory-agent/src/bin/fake-claude.rs:43-46` |

Gaps: the builder has no permission mode or allowlist, cannot read the job folder, and the presets differ (`rust` uses cargo, `go` uses go), so a fixed command list would be wrong for some projects.

## Verified behaviour

Probes with the real `claude` 2.1.303 on haiku in a scratch folder, 2026-10-19:

| Question | Result |
|---|---|
| Compound command `cd sub && python3 ...` with only `Bash(python3:*)` allowed | Runs; `cd` is read-only and always allowed |
| The same with only `Bash(cd:*)` allowed | Denied: "The following part requires approval: python3 ..." (each part is checked) |
| `python3 ... \| tee out.txt` with only `python3` allowed | Denied on `tee` |
| `--add-dir` plus `acceptEdits`, no deny rule | An Edit of a file in the added folder succeeds |
| The same plus `--disallowedTools Edit(//dir/**),Write(//dir/**)` | Edit denied; Read still works |
| Deny rule plus an allow for the same path from `--settings` | The deny wins |
| Bash redirect `echo x > <denied folder>/plan.md` with `Bash(echo:*)` allowed | Denied |
| `--resume <session>` with the permission flags passed again | Works |
| `--resume <session>` without the flags | The command is refused: the flags must be passed on every run |

## Non-goals

- Changing the reviewer, committer, message or KB specs.
- A permission prompt relay or any interactive approval.
- Sandboxing beyond the allowlist and deny rules.
- Editing `.claude/settings.json` files in repositories.

## Design

Flow:

```
builder_*_spec(...)                      (first, per task, resume)
  -> Engine::with_builder_permissions(spec)        [skipped when builder_permissions = false]
       permission_mode = acceptEdits
       allowed_tools   = base + derived from checks and check_hints + builder_allow
       disallowed_tools = Edit/Write on the job folder
       add_dirs        += job folder
  -> AgentSpec::args() -> claude -p --permission-mode ... --allowedTools ... --disallowedTools ... --add-dir ...
```

### 1. Settings

- `builder_permissions: bool`, default `true`. `false` restores today's builder argument list exactly.
- `builder_allow: Vec<String>`, default empty. Extra allow rules, such as `Bash(make:*)`. Each entry must be non-empty, one line, at most 200 characters, and must be `Bash(<command>:*)` or `Bash(<command>)`. The validator splits `<command>` on whitespace, drops `-C <path>` pairs, takes the basename of the first word, and rejects the entry when that program is `rm`, `rmdir`, `curl`, `sudo`, `sh`, `bash`, `env` or `xargs`, or when it is `git` and the next word is not one of `status`, `diff`, `log`, `show`, `rev-parse`. A bare `Bash(git:*)` is rejected. `shellcheck` and similar names pass, because the match is on whole words. `Config::parse` rejects a bad entry with `builder_allow: <why>`.
- Both keys are documented in `docs/project-toml-reference.md`. `Config::fail_closed()` keeps `builder_permissions` at its default and `builder_allow` empty.

### 2. `AgentSpec.disallowed_tools`

`pub disallowed_tools: Vec<String>`, default empty. `args()` pushes `--disallowedTools a,b` right after `--allowedTools` when the list is not empty, and nothing otherwise, so every other role's argument list stays byte-identical.

### 3. The allowlist

`pub fn builder_allowed_tools(config: &Config) -> Vec<String>` in a new `crates/factory-engine/src/permissions.rs`. Order is fixed:

1. `Read`, `Grep`, `Glob`, `Skill`, `Agent`.
2. The base commands, as `Bash(<name>:*)`: `cd`, `ls`, `cat`, `head`, `tail`, `wc`, `grep`, `find`, `pwd`.
3. Read-only git: `Bash(git status:*)`, `Bash(git diff:*)`, `Bash(git log:*)`, `Bash(git show:*)`.
4. One `Bash(<program>:*)` per program named by `config.checks[*].commands` and `config.check_hints`, in order of first use, without duplicates. The program is the first word after any leading `NAME=value` words.
5. The entries of `builder_allow`, as written.

A program is never derived when its basename is `git`, `rm`, `rmdir`, `curl`, `sudo`, `sh`, `bash`, `env` or `xargs`, or when it contains anything but letters, digits and `_ - . /`. An entry of `check_hints` counts only when its first word passes the same test, so prose such as `run the tests` adds the harmless rule `Bash(run:*)`. Broad programs a project lists itself (`make`, `yarn`, `python`) are accepted, because they are the project's own check commands. Edit and Write need no entry: `acceptEdits` covers files in the worktree.

Example: the `rust` preset gives `Bash(cargo:*)`.

### 4. The deny rules

`pub fn builder_denied_tools(job_dir: &Path) -> Vec<String>` canonicalizes `job_dir` (falling back to the path as given) and returns `Edit(//<job dir>/**)` and `Write(//<job dir>/**)` (`//` marks an absolute path in a rule, as in `kb.rs:28-30`). `--add-dir` makes the job folder writable under `acceptEdits`, and a deny rule wins over it and over any repository settings. The ledger (`progress.md`), the spec and the plan sit in the job folder and stay driver-only. The board lives in `state_dir`, outside the worktree and outside `add_dirs`, so it is not editable, as today.

### 5. Applying it

`Engine::with_builder_permissions(&self, spec: AgentSpec) -> AgentSpec` in `specs.rs`. When `config.builder_permissions` is true it sets `permission_mode = Some("acceptEdits")`, `allowed_tools`, `disallowed_tools`, and appends `job_dir` to `add_dirs`. When false it returns the spec unchanged. It also returns the spec unchanged when the canonical job folder path contains `*`, `?`, `[` or `{`, so a path that a rule could misread never widens access. The engine holds the loaded `Config` (`self.config`, as `with_limits` uses it), so the fail-closed config reaches this code like any other. Every run is a new `claude` process, so a resumed run passes the flags again. All three builder specs call it before `with_limits`.

### 6. Guard on the driver-only files

In `Engine::run_agent`, for a spec whose role is `builder`, the driver reads `plan.md`, `spec.md` and `progress.md` of the job folder before the run (missing files count as absent) and compares them after it. When any differs, the driver writes the original content back, then parks the job with the reason `builder changed <file>` through the existing park path. This holds even when `builder_permissions = false`.

### 7. Delivery

Three series run in order, each with its own plans and approval:

1. `builder-permissions`, model sonnet: section 1 in `factory-core` (`Config`, validation, defaults, tests).
2. `builder-permissions-2`, model opus: sections 2 to 6 in `factory-agent` and `factory-engine`, with the pinned regression and safety tests.
3. `builder-permissions-3`, model sonnet: the docs and the manual real-`claude` check record.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| `builder_permissions = false` | The three builder specs equal today's, field for field |
| No `checks` and no `check_hints` | Only the fixed base list |
| Check command `FOO=1 cargo test` | Program is `cargo` |
| Check command `git diff --check` | `git` is never derived; the fixed `git diff` rule already covers it |
| Check command `./scripts/test.sh` | `Bash(./scripts/test.sh:*)` |
| Program with `*`, `(`, `)` or `:` | Skipped |
| Duplicate programs across rules | One rule, first position |
| `builder_allow` entry `Bash(rm:*)` | `Config::parse` error, so the config fails closed |
| Broken config (fail-closed) | Base list only, never wider |
| Repository `.claude/settings.json` allows more | Honoured only in a trusted folder; deny rules on the job folder win either way |
| Repository settings deny a command | The deny wins; the job may block, which is the repository's choice |
| The installed `claude` rejects a permission flag | The run fails with its stop reason and the job parks; `builder_permissions = false` turns it off |
| Builder changes `plan.md`, `spec.md` or `progress.md` (for example through a tool that writes files) | The original content is restored and the job parks with `builder changed <file>` |
| The job folder lacks `spec.md` (a job without a spec) | Absent before and after counts as unchanged |
| Workspace not trusted | Command-line flags still apply; a repository settings file does not |
| Builder asks to edit `plan.md` | Denied |
| Job folder path with spaces | Rule written as is; the shell is not involved |
| Reviewer, committer, message, KB specs | Unchanged |

## Testing

- `factory-agent` (`src/spec.rs` tests): `disallowed_tools_are_sent_after_allowed_tools`, `empty_disallowed_tools_add_no_flag`, `args_of_a_default_spec_are_unchanged`.
- `factory-core` (`config.rs` tests): `builder_permissions_defaults_to_true_and_allow_to_empty`, `builder_allow_rejects_forbidden_and_malformed_entries`, `builder_allow_accepts_a_make_rule`, `fail_closed_config_keeps_the_defaults`.
- `factory-engine` (`permissions.rs` tests): `base_list_is_fixed_and_ordered`, `rust_preset_derives_cargo`, `go_style_commands_derive_go`, `leading_env_assignment_is_skipped`, `git_rm_rmdir_curl_sudo_sh_bash_env_xargs_are_never_derived`, `prose_check_hint_adds_only_a_harmless_rule`, `builder_allow_validator_is_token_aware` (`Bash(git  commit`, `Bash(git -C x commit`, `Bash(/bin/rm`, `Bash(env sh`, `Bash(xargs rm`, `Bash(git:*)` rejected; `Bash(shellcheck:*)`, `Bash(git log:*)` accepted), `glob_character_in_job_dir_leaves_the_spec_unchanged`, `builder_changing_plan_md_is_restored_and_parked`, `builder_changing_progress_md_is_restored_and_parked`, `unchanged_job_files_do_not_park`, `absent_job_file_stays_absent`, `guard_holds_with_builder_permissions_false`, `job_dir_is_canonicalized_in_the_deny_rules`, `wildcard_program_is_skipped`, `duplicates_collapse_to_first_position`, `builder_allow_comes_last`, `allowlist_never_contains_commit_add_push_rm_curl` (checked over all presets), `denied_tools_cover_the_job_folder`.
- `factory-engine` (`tests/build_checks.rs`): `builder_specs_carry_the_permission_flags` (first, task and resume), `builder_permissions_false_restores_the_old_argument_list` (compares with a pinned vector), `reviewer_committer_message_and_kb_arguments_are_unchanged` (pinned vectors for each role), `builder_specs_still_pass_assert_builder_spec_is_strict`.
- `factory-agent` (`tests/runner.rs`) with `fake-claude` mode `args`, part of series 2: `builder_run_receives_permission_mode_allow_and_deny_flags`.
- Manual check with the real `claude`, recorded in `docs/software-factory.md`: the probe table above (version, date, results), repeated when the minimum version changes. Probes already run on 2026-10-19 are in the table; series 3 only records them.
- Commands: `cargo test -j4 -p factory-agent`, `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`.

## Files changed

- `crates/factory-core/src/config.rs`
- `crates/factory-agent/src/spec.rs`
- `crates/factory-engine/src/permissions.rs` (new), `src/specs.rs`, `src/lib.rs`
- `crates/factory-engine/tests/build_checks.rs`
- `crates/factory-agent/tests/runner.rs`
- `docs/project-toml-reference.md`, `docs/software-factory.md`, `README.md`

## Constraints

- Always `-j4`; never `--workspace` or `--all`; never set `CARGO_TARGET_DIR`.
- Only the three builder specs change. Every other role's arguments stay byte-identical.
- With `builder_permissions = false` the builder's arguments equal today's.
- Board JSON, the ledger and the events format do not change.
- The allowlist never grants commit, add, push, reset, rm, curl, sudo or a shell.
- A missing or broken config never widens permissions.
- Only `git` may not be shelled out to; this change shells out to nothing.

## Acceptance criteria

- Builder runs get `--permission-mode acceptEdits`, an allowlist and a deny list for Edit and Write on the job folder, and the job folder as an extra directory.
- The allowlist is the fixed base plus the programs of `checks` and `check_hints`, plus `builder_allow`.
- The `rust` preset derives `Bash(cargo:*)` and a Go-style config derives `go`.
- `git`, `rm`, `curl`, `sudo`, `sh` and `bash` are never derived, and `builder_allow` rejects them.
- A builder cannot edit the plan, the spec or the ledger, even when a repository setting allows more.
- `builder_permissions = false` gives today's builder arguments exactly.
- The reviewer, committer, message and KB arguments are pinned and unchanged.
- A builder run that changes `plan.md`, `spec.md` or `progress.md` has them restored and the job parked with the file name.
- A fail-closed config gives only the base list.
- A `fake-claude` run in `factory-agent` shows the new flags reaching the child process.
- The docs describe both keys, the allowlist rules and the manual check.

## Out of scope

- A sandbox, network rules or per-tool prompts.
- Changing agent files in `.claude/agents/`.

## Follow-ups

- Apply the same derived allowlist idea to the reviewer's Bash needs if a reviewer ever needs commands.

## Decisions

- How does the builder get its permissions? Options: the driver passes mode, allowlist and job folder on every run; only `add_dirs` for the job folder; `dontAsk` with a tight allowlist. Chosen: the driver passes mode, allowlist and job folder on every run. Decided by: user
- Where does the Bash allowlist come from? Options: derived from `checks` and `check_hints` plus a fixed base and an optional `builder_allow` key; a fixed cargo list; only a `builder_allow` key. Chosen: derived from the config with a fixed base and `builder_allow`. Decided by: user
- How is regression avoided? Options: pinned tests, a safety test and a `builder_permissions` kill switch on by default; the same without a switch; shipped off by default. Chosen: pinned tests, safety test and kill switch, on by default. Decided by: user
- How is the job folder protected once it is writable? Options: driver-added deny rules for Edit and Write; a Read rule instead of `add_dirs`; rely on the prompt. Chosen: driver-added deny rules through a new `disallowed_tools` field. Decided by: user
- How is the work split across models? Options: three series (sonnet, opus, sonnet); one series on sonnet; one series on opus. Chosen: three series, sonnet then opus then sonnet. Decided by: user
- Should the driver probe the real `claude` before planning? Options: about six haiku calls in a scratch folder; none; a broader sonnet probe. Chosen: about six haiku calls. Decided by: user
- How are driver-only files protected from Bash writes? Options: before/after content check with restore and park; accept the risk; Bash deny patterns. Chosen: the content check with restore and park. Decided by: user
- Which commands are in the fixed base? Options: cd, ls, cat, head, tail, wc, grep, find, pwd and read-only git; a shorter list. Chosen: the longer list, because the builder used compound `cd … && …` commands. Decided by: call
- Which programs are never derived? Options: git, rm, curl, sudo, sh, bash; none. Chosen: those six. Decided by: call
- Where does the code live? Options: a new `permissions.rs` in the engine; inside `specs.rs`. Chosen: a new file, to keep `specs.rs` focused. Decided by: call
