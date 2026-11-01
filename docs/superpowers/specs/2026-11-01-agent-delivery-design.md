# Agent delivery without the repo, and a clippy gate

This is spec 1 of 4 for the follow-up to `2026-11-01-factory-quality-fixes-design.md`. It fixes issue #53 (a job that deletes or edits a tracked factory agent file breaks its own committer and reviewer) and issue #54 (the rust preset runs no clippy). The other three specs (builder loop, reviewer tools, human steps) come after it.

## Goal

A factory job never depends on agent files inside its worktree, so a job may change any file of the repo, including the tracked agent files, without breaking its own agents. The rust preset gets a lint gate. Codex and the other presets are unchanged.

## Evidence

| Finding | Where |
|---|---|
| `write_agent_files` skips a resolved agent file when the base branch tracks it. Job 047 deleted the five tracked files in its worktree, `main` still tracked them, so nothing wrote them back. The committer failed with "--agent 'factory-committer' not found", the reviewer left a 0 byte file with verdict Missing, and the builder parked on an empty findings file. | `crates/factory-engine/src/build.rs:348-411`, job 047 ledger, issue #53 |
| A spike on claude 2.1.316 showed `claude -p --agent X --agents <file>` runs an agent defined only in the file. The marker, the tool list (`Read` only) and the model (`haiku`) came through. | spike, 2026-11-01 |
| The same spike showed that when a repo also holds `.claude/agents/X.md`, the `--agents` file wins. | spike, 2026-11-01 |
| Codex agent text comes from the in-memory agent list, so Codex never reads agent files from the worktree. | `crates/factory-engine/src/codex_prep.rs:101` |
| Max turns, model, permission mode and tool rules already reach the CLI as flags. The agent files carry only `name`, `description`, `maxTurns` and, for read-only roles, `tools`. | `crates/factory-agent/src/spec.rs:160-185` |
| The rust preset checks run `cargo check`, `cargo test`, `cargo fmt --check` and dependents tests, but no clippy. | `crates/factory-core/presets/rust/config.toml`, issue #54 |

## Design

### Delivery of agents on Claude

- `Caps` gains `agents_file: bool`, true for Claude. `AgentSpec` gains `agents_file: Option<PathBuf>`. When both are set, `args` pushes `--agents <path>` next to the existing `--agent <name>`.
- For every job the driver writes `agents.json` in the job folder, from the resolved agent list. Each agent becomes `{"<name>": {"description", "prompt", "tools"}}`. `description` and `tools` come from the file's frontmatter, `tools` as a list split on commas (absent when the frontmatter has none). `prompt` is the body after the frontmatter. Names are the file names without `.md`.
- The file is written before every agent run, the way `write_agent_files` was, so a crash or an edit repairs itself. It is rewritten only when its text differs.
- The file lives in the job folder, outside the worktree and outside the git tree. A job cannot remove it by changing the repo.

### Precedence of a repo's own agent file

- `--agents` beats a repo file, so the old rule "a file tracked in `.claude/agents` is never overwritten" is kept in the driver. When the base branch tracks `.claude/agents/<name>.md`, the driver reads that file from the base branch (`Repo::file_at_branch`) and uses its text for `agents.json`, in place of the resolved text.
- Because the text comes from the branch and not from the worktree, a job that edits or deletes the tracked file does not change what its agents run on. That fixes #53 and keeps the rule.
- The resolved layers (preset, the project `agents` folder, the repo `.factory/agents` on the base branch) are unchanged and decide the text when no file is tracked.

### What goes away

- On Claude the driver stops writing agent files into worktrees and into the main checkout: `write_agent_files`, `restore_agent_file`, `check_agent_names` (kept as a check on the names that become keys), the git exclude of `AGENT_EXCLUDE_PATTERN`, the symlink guards, and the "repo agent files shadow the resolved ones" ruling. The spec reviewer writer in `spec_check.rs` goes through the same `agents.json`.
- Excluded copies in old worktrees stay where they are and are harmless.
- `Layout` keeps no agent file helpers that nothing calls. `no_hardcoded_paths` must still pass.

### Test double and probe

- `fake-claude` reads `--agents <file>` first and falls back to `.claude/agents/<name>.md`, so the probe marker test and the engine tests keep working.
- `factory probe` delivery `agent` uses `--agents`. The text in `software-factory.md` that describes it is updated.

### Clippy gate for the rust preset

- The crate rule gets `cargo clippy -j4 -p {crate} --all-targets -- -D warnings` after the test command. The root `src/**` rule gets `cargo clippy -j4 --all-targets -- -D warnings`. The manifest rule stays `cargo check -j4`.
- The exact-list tests in `plan.rs` and `preset.rs`, the README and the reference docs that describe the rust rules are updated.
- No other preset changes.

## Compatibility

- **Codex:** untouched. It never read agent files from the worktree, and `agents_file` is false for it, so no `--agents` flag is sent.
- **Other presets and languages:** the resolved agent list and its layers are the same. Only the rust preset config changes, for clippy.
- **Projects with tracked agent files:** their text still wins, now through the branch read.
- **Claude version:** the factory needs a `claude` that accepts `--agents` with `--agent`. 2.1.316 does, and that is the tested floor. There is no fallback to writing files, because two paths would keep the bug. The version note goes into `docs/reference/software-factory.md` and the release notes.
- **Existing checks lists:** a project whose own `[[settings.checks]]` replaces the preset list, such as this repo, gets clippy only by adding it by hand.

## Non-goals

- Making per-task checks blocking, or changing what a fresh task-2+ builder sees (spec 2).
- Giving the reviewer tools to run tests (spec 3).
- Counting why jobs stop for a human (spec 4).
- Changing the content of any agent file.

## Decisions

- Pass agents with `--agents` and stop writing files into the repo, not park such jobs and not write files back. Decided by: user
- Keep the tracked-file rule by reading the base branch text. Decided by: call
- No fallback to file writing for a `claude` without `--agents`. Decided by: call
- Add clippy to the rust preset rules. Decided by: user

## Acceptance criteria

- A Claude run sends `--agents <job folder>/agents.json` with `--agent <name>`. A Codex run sends no `--agents`.
- `agents.json` holds all five factory agents with description, prompt and, for read-only roles, tools.
- A job that deletes a tracked `.claude/agents/factory-*.md` still runs its committer and reviewer.
- A repo-tracked agent text still wins over the preset text in `agents.json`, and an edit of that file inside the worktree does not change it.
- The driver writes no factory agent file into a worktree or the main checkout, and the shadow ruling is gone.
- `fake-claude` and `factory probe` work through `--agents`.
- The rust preset plans a clippy command for a changed crate path and for a root path, and the validator accepts both.
- The go, ruby, csharp and node preset configs are unchanged.
- fmt and clippy `-D warnings` pass for every crate touched, and the factory-engine, factory-cli, factory-agent and factory-checks tests pass.
