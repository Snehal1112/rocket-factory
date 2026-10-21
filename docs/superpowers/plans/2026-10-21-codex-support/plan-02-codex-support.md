# Codex support, Plan 2: The setting and the engine wiring

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** A project with `agent_cli = "codex"` runs every agent through `CodexRunner`, with the agent text and preface in front of the prompt, the sandbox mode, network and writable roots chosen per role, saved Claude sessions cleared on a provider switch, and the provider written to every usage line.

**Architecture:** `Config` gets `agent_cli`, `Job` gets an optional `provider`. `factory project add --agent-cli codex` seeds the settings and `project show` reports problems. `Engine::run_agent` calls a new `prepare_for_codex` when the project is on Codex, and a provider check that clears session ids once. `CliCtx::runner_for` picks the runner. The Codex logic sits behind the existing `AgentRunner` port in `factory-agent`; the engine only prepares the `AgentSpec`.

**Tech Stack:** Rust, `git2` through `factory-git`, clap derive.

**Spec:** docs/superpowers/specs/2026-10-21-codex-support-design.md

## Global Constraints

- Always `-j4`; never `--workspace` or `--all`; never set `CARGO_TARGET_DIR`. Per crate: `cargo test -j4 -p <crate> <filter>`.
- Every new board and usage field is optional: `Job.provider: Option<String>` with `#[serde(default, skip_serializing_if = "Option::is_none")]`. An old board, an old `usage.jsonl` line and the dashboard keep loading.
- `agent_cli` allows only `"claude"` and `"codex"`; unset means `"claude"`. The error text is `agent_cli must be "claude" or "codex" when it is set`. A broken config still fails closed.
- `project add --agent-cli codex` seeds `agent_cli = "codex"` and `builder_model`, `reviewer_model`, `commit_model`, `kb_model` all `"gpt-5.6-luna"`; it writes no `cache_ttl` and no `reviewer_model_large`. Without the flag the file is exactly what it is today.
- Sandbox by role: `builder*` `workspace-write` with network on; `committer*` and `message*` `workspace-write`, network off, writable root the main repo's common git folder; `kb*` `workspace-write`, network off, `cwd` the KB folder, writable root the KB repo's `.git` only when `kb_auto_commit`; `reviewer*` and anything else `read-only`, network off.
- The Codex preface is: `You run under Codex. Do not load or invoke skills and do not start subagents. Where this text names a skill, follow the rules written in short in this text instead.` For roles `committer*`, `message*` and `kb*` this sentence follows it: `The skill your instructions name is not available; the analysis in this prompt replaces its first steps, and the agent text gives the rules.`
- Agent text comes from the engine's `agent_files` list (`AgentFile { name, content }`), never from the disk. A name that is not in the list fails the run before spawning with `agent file <name> not found`.
- For Codex runs `run_agent` pushes no `cache_ttl` variable, sets `spec.max_turns = None`, and carry is refused. Provider names are the strings `claude` and `codex`.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call. Comments are short full sentences ending with a punctuation mark.

## Review Focus

1. A job that ran on Claude and whose project then switches to Codex. Expected: every saved session id is cleared once, a fresh builder runs, nothing is sent to `codex exec resume`. Pinned by `a_provider_switch_clears_saved_session_ids_once` (Task 3).
2. An old job with no `provider`. Expected: it counts as `claude`, so a switch to Codex clears its sessions, and a Claude run changes nothing. Pinned by the same test and `an_old_job_counts_as_claude` (Task 3).
3. The KB refresher under Codex. Expected: it gets its agent text from the engine list and runs in the KB folder, not the main checkout. Pinned by `kb_runs_in_the_kb_folder_under_codex` (Task 2).
4. A committer run that Codex cannot finish because git is blocked. Expected: the existing fallback commit still lands the work. Pinned by `fallback_commit_runs_when_a_codex_committer_fails` (Task 3).
5. A Claude alias such as `sonnet` left in a model key of a Codex project. Expected: `project show` lists it as a problem. Pinned by `project_show_reports_a_missing_codex_and_a_claude_alias` (Task 1).

---

## File Structure

- `crates/factory-core/src/config.rs` (modify): `agent_cli`, validation, `agent_cli()` accessor.
- `crates/factory-core/src/board.rs` (modify): `Job.provider`.
- `crates/factory-core/src/project.rs`, `crates/factory-cli/src/cli.rs`, `crates/factory-cli/src/project.rs` (modify): `--agent-cli`, seeding, `project show`.
- `crates/factory-git/src/repo.rs` (modify): `Repo::common_git_dir`.
- `crates/factory-engine/src/codex_prep.rs` (create): sandbox plan, preface, agent text, `prepare_for_codex`.
- `crates/factory-engine/src/ports.rs`, `carry.rs`, `build.rs`, `lib.rs` (modify): wiring, provider reset, usage line, carry.
- `crates/factory-cli/src/commands.rs` (modify): `runner_for`.

### Task 1: The setting, `Job.provider`, `project add` and `project show`

**Files:**
- Modify: `crates/factory-core/src/config.rs:44-100,178-232`, `board.rs` (`Job`), `project.rs:84-125`, `crates/factory-cli/src/cli.rs:168-178`, `crates/factory-cli/src/project.rs:159-220`
- Test: the same files (unit tests), `crates/factory-cli/tests/project_cmd.rs`

**Interfaces:**
- Produces:
  - `Config.agent_cli: Option<String>` and `impl Config { pub fn agent_cli(&self) -> &str }` returning `"claude"` when unset.
  - `Job.provider: Option<String>`.
  - `project::add(..., agent_cli: Option<&str>)`: the extra last parameter; callers pass `None`.
  - `pub fn codex_problems(cfg: &Config, path_env: &str) -> Vec<String>` and `pub fn program_in_path(path_env: &str, name: &str) -> bool` in `crates/factory-cli/src/project.rs`.

- [ ] **Step 1: Read** `Config`, `Config::parse` and its `cache_ttl` check, `project::add`, the `Add` command, `show`, and the project tests in `tests/project_cmd.rs`.
- [ ] **Step 2: Write the failing tests.**
  - `config.rs`: `agent_cli_is_unset_by_default_and_takes_claude_or_codex` (default `None` and `agent_cli() == "claude"`; `Config::parse("agent_cli = \"codex\"")` gives `"codex"`) and `agent_cli_rejects_other_values` (`"gpt"` and `""` are errors whose text contains `agent_cli must be`).
  - `board.rs`: `job_provider_is_optional_and_old_boards_load` (a job JSON without `provider` loads with `None`; a job with `Some("codex")` round-trips; `None` is not written).
  - `project.rs`: `project_add_with_agent_cli_codex_seeds_models_and_no_cache_ttl` (the settings table holds `agent_cli = "codex"` and the four models, and has no `cache_ttl`) and `project_add_without_the_flag_is_unchanged` (only `cache_ttl` and `base_branch` as before).
  - `cli.rs`: `project_add_takes_agent_cli` parses `["factory","project","add","/r","--preset","rust","--agent-cli","codex"]`; a value `gpt` is a clap error.
  - `factory-cli/src/project.rs` full example:

```rust
#[test]
fn project_show_reports_a_missing_codex_and_a_claude_alias() {
    let mut cfg = Config::default();
    cfg.agent_cli = Some("codex".into());
    cfg.builder_model = "sonnet".into();
    cfg.reviewer_model_large = Some("opus".into());
    let problems = codex_problems(&cfg, "/nonexistent-dir");
    assert!(problems.iter().any(|p| p.contains("codex") && p.contains("PATH")), "{problems:?}");
    assert!(problems.iter().any(|p| p.contains("builder_model") && p.contains("sonnet")));
    assert!(problems.iter().any(|p| p.contains("reviewer_model_large")));
}
```

  Also `no_codex_problems_for_claude_projects` (`agent_cli` unset gives an empty list) and `program_in_path_finds_an_executable` (a temp folder with an executable file named `codex` in `path_env`).
- [ ] **Step 3: Run** `cargo test -j4 -p factory-core agent_cli`. Expected: compile failure, no field `agent_cli` on `Config`.
- [ ] **Step 4: Implement** the field, the validation and accessor, `Job.provider`, the `add` parameter and seeding, the clap option (`#[arg(long, value_parser = ["claude", "codex"])] agent_cli: Option<String>`), and in `show` the line `agent cli: <value>` after `base branch` plus the `codex_problems` entries (only when `agent_cli` is `codex`) appended to `problems`.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-core` and `cargo test -j4 -p factory-cli project`. Expected: all pass, and `cargo test -j4 -p factory-core --test rocket_parity` still passes because the key is optional.

### Task 2: Preparing a Codex spec in the engine

**Files:**
- Create: `crates/factory-engine/src/codex_prep.rs`
- Modify: `crates/factory-git/src/repo.rs`, `crates/factory-engine/src/lib.rs`, `crates/factory-engine/src/ports.rs:170-176`
- Test: `crates/factory-engine/src/codex_prep.rs` (unit tests), `crates/factory-engine/tests/codex_prep.rs`

**Interfaces:**
- Consumes: `Config::agent_cli()`, `AgentSpec.sandbox`, `writable_roots`, `network` (plan 1), `Engine.agent_files`.
- Produces:
  - `Repo::common_git_dir(&self) -> PathBuf` in `factory-git` (the repository's common git folder, from `git2`).
  - `pub(crate) fn strip_frontmatter(text: &str) -> &str` (removes a leading `---` block).
  - `pub(crate) fn codex_preface(role: &str) -> String`.
  - `pub(crate) struct SandboxPlan { pub mode: &'static str, pub network: bool }` and `pub(crate) fn sandbox_for(role: &str) -> SandboxPlan`.
  - `impl Engine<'_> { pub(crate) fn prepare_for_codex(&self, spec: &mut AgentSpec) -> Result<(), EngineError> }`, called from `run_agent` only when `self.config.agent_cli() == "codex"`; the `cache_ttl` variable push is skipped in that case.

- [ ] **Step 1: Read** `run_agent`, `kb_spec` and `kb.rs` (the KB folder and `kb_auto_commit`), the `AgentFile` type, and the engine test fixtures (`tests/common`).
- [ ] **Step 2: Write the failing tests.**
  - `factory-git`: `common_git_dir_is_the_main_dot_git_from_a_worktree` (create a repo and a worktree as the factory-git tests do; both give the main `.git` path).
  - `codex_prep.rs` unit tests: `strip_frontmatter_removes_only_a_leading_block`, `the_preface_adds_the_skill_sentence_for_committer_message_and_kb` (builder and reviewer get only the first sentence), and a table test `sandbox_follows_the_role`:

  | Role | Mode | Network |
  |---|---|---|
  | `builder` | `workspace-write` | true |
  | `committer` | `workspace-write` | false |
  | `message` | `workspace-write` | false |
  | `kb` | `workspace-write` | false |
  | `reviewer` | `read-only` | false |
  | `something-else` | `read-only` | false |

  - `tests/codex_prep.rs`, on the engine fixture with a `FakeRunner`; full example:

```rust
#[test]
fn run_agent_prepends_the_agent_file_and_the_codex_preface() {
    // Build the engine fixture as `usage_lines_carry_the_run_details` does, with agent_cli = "codex"
    // and one AgentFile named "factory-builder" whose content starts with a frontmatter block.
    let spec = run_one_builder_and_capture_the_spec();
    assert_eq!(spec.agent, None);
    let text = spec.agent_prompt.expect("agent text");
    assert!(text.starts_with("You run under Codex."), "{text}");
    assert!(!text.contains("name: factory-builder"), "frontmatter must be stripped");
    assert_eq!(spec.max_turns, None);
}
```

  | Test | Setup | Assertion |
  |---|---|---|
  | `committer_message_and_kb_get_the_skill_sentence` | roles `committer`, `message`, `kb` | each `agent_prompt` contains `The skill your instructions name is not available` |
  | `network_is_on_only_for_builder_roles` | roles builder and reviewer | `spec.network` true, then false |
  | `committer_gets_the_git_dir_as_a_writable_root` | role `committer` | `writable_roots == [repo.common_git_dir()]` |
  | `kb_runs_in_the_kb_folder_under_codex` | `kb_path` set, role `kb` | `spec.cwd == kb_dir`, agent text from the engine list |
  | `codex_runs_skip_the_cache_ttl_variables` | `cache_ttl = "5m"`, codex | `spec.env` has no `FORCE_PROMPT_CACHING_5M` |
  | `agent_file_name_missing_from_the_list_fails_before_spawn` | agent name `factory-nope` | the run is `Failed` with `agent file factory-nope not found`, and the runner was not called |
  | `claude_projects_are_untouched` | `agent_cli` unset | spec equals what `builder_task_spec` built |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine codex_prep`. Expected: compile failure, cannot find module `codex_prep`.
- [ ] **Step 4: Implement** the functions above. `prepare_for_codex` looks the agent up by `spec.agent`, sets `agent_prompt = preface + "\n\n" + stripped content`, clears `agent`, sets `max_turns = None`, `sandbox`, `network`, `writable_roots` (committer and message: the common git folder; kb: the KB repo's `.git` only when `kb_auto_commit`), and for role `kb*` sets `cwd` to the KB folder. It returns `EngineError::Msg("agent file <name> not found")` when the name is absent, and `run_agent` turns that into a failed run without calling the runner.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-git`, `cargo test -j4 -p factory-engine`. Expected: all pass.

### Task 3: Provider reset, usage provider, carry, and the runner choice

**Files:**
- Modify: `crates/factory-engine/src/ports.rs:170-260`, `carry.rs`, `build.rs`, `crates/factory-cli/src/commands.rs:202-211`
- Test: `crates/factory-engine/tests/codex_prep.rs`, `crates/factory-engine/src/ports.rs`, `carry.rs`, `crates/factory-cli/src/commands.rs`

**Interfaces:**
- Consumes: `Config::agent_cli()`, `Job.provider`, `prepare_for_codex`, `CodexRunner` (plan 1).
- Produces:
  - `usage_line(...)` gets a trailing `provider: &str` parameter and writes `"provider"` on every line.
  - `Engine::sync_provider(&self) -> Result<(), EngineError>` called at the start of `run_agent`: with `current = job.provider.as_deref().unwrap_or("claude")`, when it differs from `self.config.agent_cli()` it sets `task_session`, `builder_id` and `builder_last` to `None`; it then sets `job.provider = Some(agent_cli)` when that changed the stored value.
  - `carry_decision` refuses with the reason `codex` when the project is on Codex (add a `codex: bool` field to `CarryInput`).
  - `pub enum AgentCli { Claude, Codex }` and `pub fn runner_kind(cfg: Option<&Config>) -> AgentCli` in `commands.rs`; `runner_for` builds `CodexRunner::new("codex", sink)` for `Codex`.

- [ ] **Step 1: Read** the `usage_line` tests in `ports.rs`, `carry_decision` and `CarryInput`, the places in `build.rs` that read `builder_id`, `task_session` and `builder_last`, `runner_for`, and the committer fallback in `commit.rs`.
- [ ] **Step 2: Write the failing tests.**
  - `ports.rs`: `usage_line_carries_the_provider` (`claude` and `codex`; update every existing `usage_line(...)` test call with `"claude"`).
  - `carry.rs`: `carry_is_refused_under_codex` (`Carry::No { reason: "codex" }` even when every other condition allows a carry).
  - `tests/codex_prep.rs`: `a_provider_switch_clears_saved_session_ids_once` (a job on the board with `provider: Some("claude")`, a `task_session`, a `builder_id` and a `builder_last`, project on Codex: after one `run_agent` all three are `None` and `provider == Some("codex")`; a second call changes nothing and a counter shows the clear ran once), `an_old_job_counts_as_claude` (`provider: None` and a Claude project: nothing cleared, `provider` becomes `Some("claude")`), and `fallback_commit_runs_when_a_codex_committer_fails` (committer scripted to fail on a Codex project: the existing fallback commit path runs and the work is committed, as in the existing committer fallback test).
  - `commands.rs`: `runner_kind_follows_agent_cli` (`None` and an unset config give `Claude`, `"codex"` gives `Codex`).
- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine codex_prep` and `cargo test -j4 -p factory-cli runner_kind`. Expected: compile failures for the missing parameter and `runner_kind`.
- [ ] **Step 4: Implement** the items in Interfaces. The `build.rs` carry call passes `codex: self.config.agent_cli() == "codex"`.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`, and `cargo clippy -j4 -p factory-core -p factory-engine -p factory-cli -- -D warnings`. Expected: all pass, no warnings.

Next plan: docs/superpowers/plans/2026-10-21-codex-support/plan-03-codex-support.md
