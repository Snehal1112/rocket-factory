# Harness Hardening, Plan 04: Ship /factory As A Claude Code Plugin

> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** A user installs `/factory` with `claude plugin install`, the plugin file is generated from the same text `factory install` writes and pinned by a drift test, `scripts/release.sh` bumps the plugin version, and `factory install` and `factory upgrade` detect the plugin and an old `~/.claude/commands/factory.md` and say how to switch.

**Architecture:** The repo root becomes a Claude Code marketplace (`.claude-plugin/marketplace.json`) with one plugin under `plugins/factory/`. `install::claude_command_text()` is the one function that yields the command text; `factory plugin sync` writes the committed plugin files from it and `factory plugin sync --check` fails on drift. Plugin detection asks `claude plugin list --json`, the same call `ClaudeRunner` already makes, never Claude Code's internal `installed_plugins.json`, whose format is not a public interface.

**Tech Stack:** Rust (clap, serde_json), bash (`scripts/release.sh`), Claude Code plugin manifests.

**Spec:** `docs/superpowers/specs/2026-10-30-harness-hardening-design.md` (GitHub issue #38, parent #40). Codex plugin packaging is out of scope: `factory install` keeps writing the Codex skill to `~/.agents/skills/factory/SKILL.md`, which is Codex's own discovery folder.

## Global Constraints

- Cargo per crate with `-j4`: `cargo test -j4 -p <crate> <filter>`; never `--workspace` or `--all`; never set `CARGO_TARGET_DIR`.
- Git only through `factory_git::Repo`; never shell out to git. `gh`, `claude`, `codex` may be spawned; tests spawn only the `fake-claude` binary, for `plugin list --json`.
- Paths reach the driver only through `factory_core::layout` helpers; `crates/factory-cli/tests/no_hardcoded_paths.rs` fails on hardcoded ones. Run it after Tasks 1 and 2.
- Board JSON stays compatible with the dashboard: this plan adds no board fields.
- Tests use `fake-claude`, never the real CLI; these tests need neither.
- After editing `crates/factory-core/assets/global-command.md` (this plan does not), run `cargo test -j4 -p factory-cli --test agent_lint`, `--test global_command` and `--test skill_drift`.
- Comments are short full sentences ending with a period. Docs go in `docs/`.
- The driver makes the commits (with the `dev-workflow-skills:1-git-commit` skill); no task has a commit step.
- Names fixed by this plan: marketplace `rocket-factory`, plugin `factory`, plugin id `factory@rocket-factory`, marketplace source `Snehal1112/rocket-factory`.
- The plugin ships only `commands/factory.md`, `.claude-plugin/plugin.json` and `README.md`. It ships no script, so the lost executable bit and undeclared-file gotchas do not apply; a test pins the file list so nobody adds an undeclared file.
- The command finds the binary exactly as today: `$FACTORY_BIN`, else `factory` on PATH. A plugin has no install-time substitution and the text needs none. When `factory` is not on PATH the command already says the factory binaries are not on PATH and stops; a test pins that sentence.

## Review Focus

1. The committed `plugins/factory/commands/factory.md` differs from the generated text (for example plan 03 changes the assembly): `plugin_files_match_generated_text` fails and names `factory plugin sync`.
2. The release bumps `Cargo.toml` but not `plugin.json`: the plugin keeps its old version and `claude plugin update` sees nothing (`plugin_json_version_matches_crate`, `release_script_syncs_and_adds_plugin_files`).
3. `claude` is missing, fails, or prints something that is not a JSON array (a fresh machine, an old Claude Code): detection says "not installed" and install and upgrade behave as before (`detects_nothing_from_bad_output`).
4. Plugin and old command both present: the user sees two `/factory` entries, so `factory upgrade` stops updating the old file and prints the `rm` line (`upgrade_with_plugin_and_old_command_says_remove`).
5. `--dir` installs and tests with a temp home must never read the real `~/.claude`: detection runs only for the standard home (`install_with_dir_ignores_plugin_state`).

## File Structure

- Modify `crates/factory-core/src/layout.rs`, `lib.rs`: `plugin_rel_files`.
- Modify `crates/factory-agent/src/claude.rs`, `lib.rs`: make `list_plugins` public as `installed_plugin_ids`. Modify `crates/factory-agent/src/bin/fake-claude.rs`: `FAKE_PLUGIN_LIST` overrides its `plugin list` answer.
- Create `crates/factory-cli/src/plugin.rs`: `plugin_manifest_text`, `sync`, `check`, `plugin_installed`, message helpers.
- Modify `crates/factory-cli/src/install.rs`: `claude_command_text`, detection in `run`, `home_of_standard_commands_dir`.
- Modify `crates/factory-cli/src/{cli.rs,main.rs,commands.rs,upgrade.rs,lib.rs}`: `plugin sync` subcommand and upgrade messages.
- Create `.claude-plugin/marketplace.json`, `plugins/factory/.claude-plugin/plugin.json` (generated), `plugins/factory/commands/factory.md` (generated), `plugins/factory/README.md`.
- Modify `scripts/release.sh`, `docs/operations/releasing.md`, `docs/guides/getting-started.md`, `README.md`.
- Tests: `crates/factory-cli/tests/plugin_files.rs` (new), `plugin_install.rs` (new), unit tests in `plugin.rs`.

## Tasks

### Task 1: Plugin files, generator and drift test

**Files:**
- Create: `crates/factory-cli/src/plugin.rs`, `.claude-plugin/marketplace.json`, `plugins/factory/README.md`, `crates/factory-cli/tests/plugin_files.rs`
- Modify: `crates/factory-core/src/layout.rs`, `crates/factory-core/src/lib.rs`, `crates/factory-cli/src/{install.rs,cli.rs,main.rs,commands.rs,lib.rs}`
- Generate: `plugins/factory/.claude-plugin/plugin.json`, `plugins/factory/commands/factory.md`

**Interfaces:**
- Produces in `factory_core::layout` (re-exported in `lib.rs`): `pub fn plugin_rel_files() -> [&'static str; 3]` returning `plugins/factory/.claude-plugin/plugin.json`, `plugins/factory/commands/factory.md`, `plugins/factory/README.md`.
- Produces `install::claude_command_text() -> String` (today `GLOBAL_COMMAND.to_string()`). `install::run` and `upgrade_into` use it instead of `GLOBAL_COMMAND`. Plan 03 changes only this function's body.
- Produces `plugin::plugin_manifest_text(version: &str) -> String`: pretty JSON with the real Claude Code fields `name` ("factory"), `description`, `version`, `author` (`{"name": "Snehal Dangroshiya"}`), ending in a newline.
- Produces `plugin::sync(repo: &Path) -> Result<Vec<String>, CliError>` (writes `plugin.json` and `commands/factory.md` atomically under `repo`, returns `wrote <path>` or `unchanged <path>` lines) and `plugin::check(repo: &Path) -> Result<(), CliError>` (error text `plugin files out of date: <rel path>; run factory plugin sync`).
- CLI: `factory plugin sync [--repo <dir>] [--check]`. `--repo` defaults to the current directory. Handled in `main.rs` before project lookup, like `install`; the `commands.rs` match gets a refusing arm.

Formats copied from the installed marketplaces (`~/.claude/plugins/marketplaces/*/.claude-plugin/marketplace.json`, `plugin.json`): marketplace has `name`, `owner.name`, `description`, `plugins[]` with `name`, `source` (relative path string such as `"./plugins/factory"`), `description`; a plugin has `.claude-plugin/plugin.json` (`name`, `description`, `version`, `author`) and `commands/*.md`.

- [ ] **Step 1: Write the failing tests** in `crates/factory-cli/tests/plugin_files.rs`. Repo root is `Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")`.
  - `plugin_files_match_generated_text`: `plugin::check(root)` is `Ok`; on failure the message must contain `factory plugin sync`.
  - `check_reports_drift_in_a_temp_copy`: copy the two generated files to a tempdir, append a line to the command, `plugin::check` errs naming the command's rel path; `plugin::sync` on that tempdir then makes `check` pass and a second `sync` returns only `unchanged` lines.
  - `plugin_json_version_matches_crate`: parse `plugin.json`; `version == FACTORY_VERSION`, `name == "factory"`.
  - `marketplace_names_the_plugin`: parse `.claude-plugin/marketplace.json`; `name == "rocket-factory"`, `plugins[0].name == "factory"`, `plugins[0].source == "./plugins/factory"`, and that source folder holds `.claude-plugin/plugin.json`.
  - `plugin_dir_holds_only_declared_files`: walk `plugins/factory` recursively; the sorted relative file set equals `plugin_rel_files()`.
  - `command_names_the_path_failure`: `install::claude_command_text()` contains `factory binaries are not on PATH` and `FACTORY_BIN`.
- [ ] **Step 2: Run** `cargo test -j4 -p factory-cli --test plugin_files`. Expected: compile FAIL (no `plugin` module).
- [ ] **Step 3: Implement** the layout helpers, `claude_command_text`, `plugin.rs`, the `Plugin { cmd: PluginCmd }` clap subcommand with `PluginCmd::Sync { repo: Option<PathBuf>, check: bool }`, and a cli.rs unit test `plugin_sync_parses_check`. `sync` prints its lines joined; `--check` prints `plugin files are current`.
- [ ] **Step 4: Create** `.claude-plugin/marketplace.json` (owner name `Snehal Dangroshiya`, description "Marketplace for the rocket-factory /factory command.") and `plugins/factory/README.md` (what the plugin is, that the `factory` binary must be on PATH and the plugin does not install it, the install commands, and that the command file is generated: run `factory plugin sync`). Then run `cargo run -j4 -p factory-cli -- plugin sync` to generate the other two files.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-cli --test plugin_files`, `cargo test -j4 -p factory-cli --test no_hardcoded_paths` and `cargo test -j4 -p factory-core layout`. Expected: PASS.

### Task 2: Detect the plugin and the old command in install and upgrade

**Files:**
- Modify: `crates/factory-cli/src/{plugin.rs,install.rs,upgrade.rs}`, `crates/factory-agent/src/{claude.rs,lib.rs}`, `crates/factory-agent/src/bin/fake-claude.rs`
- Test: `crates/factory-cli/tests/plugin_install.rs`

**Interfaces:**
- Consumes: `claude_commands_dir`.
- Produces `factory_agent::installed_plugin_ids(program: &OsStr) -> Vec<String>`: today's private `list_plugins`, renamed and made public; `ClaudeRunner` keeps calling it. Empty on any spawn or parse failure, as today.
- Produces in fake-claude: when `FAKE_PLUGIN_LIST` is set, `plugin list --json` prints its value instead of the built-in answer.
- Produces `plugin::PLUGIN_ID = "factory@rocket-factory"`, `plugin::MARKETPLACE_SOURCE = "Snehal1112/rocket-factory"`, `plugin::plugin_installed(ids: &[String]) -> bool` (pure: true when `PLUGIN_ID` is in `ids`), `plugin::install_hint() -> String`, `plugin::switch_note(old_command: &Path) -> String`. `install::run` and `upgrade::run` call `plugin_installed(&installed_plugin_ids(claude_program))`. The program is a parameter, not an env var: `install::run_with_claude(dir, force, claude_program: &OsStr)` and `upgrade::run_with_claude(.., claude_program: &OsStr)`, which `run` calls with `"claude"`, so tests pass the fake-claude path.
- Produces `install::home_of_standard_commands_dir(commands_dir: &Path) -> Option<&Path>` (the logic now inside `upgrade_codex_skill`, which is changed to call it).

Exact texts (tests assert them):
- `install_hint()`:
  ```
  note: the Claude Code plugin is the preferred install:
    claude plugin marketplace add Snehal1112/rocket-factory
    claude plugin install factory@rocket-factory
  ```
- `switch_note(old)` (plugin not installed, old command exists): `note: to switch to the plugin, run the two commands below, then remove <old>:` followed by the same two indented command lines.
- Plugin installed, old command exists: `note: the factory plugin is installed and <old> also exists, so /factory is listed twice; remove it with: rm <old>`
- Plugin installed, no old command: `plugin: factory@rocket-factory is installed; update it with: claude plugin update factory@rocket-factory`

Behaviour (install: only without `--dir`; upgrade: only when `commands_dir` is the standard `~/.claude/commands`):
- `install::run`, plugin installed and no `--force`: do not write the Claude command; line `skipped Claude command: the factory plugin is installed`, then the old-command note if the file exists. The Codex skill step is unchanged.
- `install::run`, plugin not installed: write as today, then append `install_hint()` to the Claude line.
- `upgrade::run`, plugin installed: skip `upgrade_into`, emit the plugin line or the remove note. Plugin not installed: unchanged behaviour, plus `switch_note` when the old file exists after the update, and `install_hint()` when it does not. Dry run prints the same notes and writes nothing.
- `--dir` installs never read plugin state.

- [ ] **Step 1: Write the failing tests** in `plugin_install.rs` (temp home folders via `claude_commands_dir(tmp)`; for `install::run_with_claude` use the `ENV_LOCK` and `EnvVarGuard` pattern from `global_command.rs` to set `HOME` and `FAKE_PLUGIN_LIST`; the program is `env!("CARGO_BIN_EXE_fake-claude")` if factory-cli can build it, else the path the existing fake-claude tests use). Installed state is `FAKE_PLUGIN_LIST=[{"id":"factory@rocket-factory","enabled":true}]`; not installed is `[]`.
  - `detects_the_plugin_by_id`: `plugin_installed` is true for `["factory@rocket-factory"]`, false for `["factory@other"]` and `[]`.
  - `detects_nothing_from_bad_output`: `FAKE_PLUGIN_LIST` set to `not json`, and a program path that does not exist, both give an empty `installed_plugin_ids`.
  - `install_prints_the_plugin_hint_when_not_installed`: output contains the exact `install_hint()` text and `installed <path>/factory.md`.
  - `install_skips_the_command_when_plugin_installed`: no `factory.md` written, output contains `skipped Claude command: the factory plugin is installed`; with `--force` it is written.
  - `install_with_dir_ignores_plugin_state`: plugin state says installed, `--dir` still writes `factory.md`.
  - `upgrade_with_plugin_and_old_command_says_remove`: the old file is byte-for-byte unchanged, no `.bak` appears, output has the `rm <old>` note.
  - `upgrade_with_plugin_only_prints_update_line`.
  - `upgrade_without_plugin_adds_the_switch_note`: old file updated as today and `switch_note` appears; a second upgrade still prints `already installed:` for the command.
  - `upgrade_dry_run_with_plugin_writes_nothing`.
- [ ] **Step 2: Run** `cargo test -j4 -p factory-cli --test plugin_install`. Expected: FAIL.
- [ ] **Step 3: Implement** as specified. `upgrade_into(folder, dry_run)` stays as it is; the plugin branching lives in `upgrade::run` and `install::run`.
- [ ] **Step 4: Run** `cargo test -j4 -p factory-cli --test plugin_install --test upgrade --test global_command --test no_hardcoded_paths`. Expected: PASS, with the existing install and upgrade tests unchanged.

### Task 3: Release script bumps the plugin version

**Files:**
- Modify: `scripts/release.sh`, `docs/operations/releasing.md`
- Test: `crates/factory-cli/tests/plugin_files.rs`

**Interfaces:**
- Consumes: `factory plugin sync` and `--check` (Task 1).

`release.sh` has no test today, so the pin is a text test plus a syntax check. In the real run, after the two `cargo check` lines, add `cargo run -j4 -q -p factory-cli -- plugin sync` (it rebuilds with the new `CARGO_PKG_VERSION`, so `plugin.json` gets the new version) and add `plugins/factory` to the `git add` line. In the CI gate, add `cargo run -j4 -q -p factory-cli -- plugin sync --check` after the factory tests, so drift blocks a release before anything changes. The dry-run text says the plugin files are updated too.

- [ ] **Step 1: Write the failing tests** in `plugin_files.rs`, reading `scripts/release.sh`:
  - `release_script_syncs_and_adds_plugin_files`: a `plugin sync` line (without `--check`) comes after `set_section_version "$DASH_MANIFEST" package`, and a `git add` line contains `plugins/factory`.
  - `release_script_gate_checks_plugin_drift`: `plugin sync --check` appears before the line containing `CI gate passed`.
  - `release_script_still_parses`: run `bash -n scripts/release.sh`, expect success (print a note and return if `bash` cannot be spawned).
- [ ] **Step 2: Run** `cargo test -j4 -p factory-cli --test plugin_files release_script`. Expected: FAIL.
- [ ] **Step 3: Edit** `scripts/release.sh` as described. In `docs/operations/releasing.md` extend step 2 (the gate also checks plugin drift) and step 7 (it also regenerates and commits `plugins/factory/.claude-plugin/plugin.json`), and add that users pick up a release with `claude plugin update factory@rocket-factory`.
- [ ] **Step 4: Run** the three tests. Expected: PASS. Run `./scripts/release.sh --dry-run` only on a clean tree with a signing key; otherwise skip it and say so.

#### Then: Docs and help text

**Files:**
- Modify: `docs/guides/getting-started.md` (sections 6 and 15), `README.md` (the "global /factory command" section), `crates/factory-cli/src/cli.rs` (`Install` doc comment), `plugins/factory/README.md`
- Test: `crates/factory-cli/tests/plugin_files.rs`

- [ ] **Step 1: Write the failing test** `docs_lead_with_the_plugin_commands`: `getting-started.md` contains `claude plugin marketplace add Snehal1112/rocket-factory` and `claude plugin install factory@rocket-factory`, the first of those appears before the first `factory install` inside section 6, and `README.md` contains `factory@rocket-factory`.
- [ ] **Step 2: Run** `cargo test -j4 -p factory-cli --test plugin_files docs_lead`. Expected: FAIL.
- [ ] **Step 3: Edit the docs.** Section 6 becomes: plugin path first (the two commands; `claude plugin list` shows `factory@rocket-factory`; `/factory` appears in a new session; the `factory` binary must be on PATH, the plugin does not install it); then "Fallback: `factory install`" for the file install and for Codex (`factory install` still writes the Codex skill). Add how to switch (install the plugin, then `rm ~/.claude/commands/factory.md`) and to uninstall (`claude plugin uninstall factory@rocket-factory`). Section 15 adds `claude plugin update factory@rocket-factory`. README states that `factory upgrade` stops touching the old command file once the plugin is installed. The `Install` help text says it is the fallback and prints the plugin commands.
- [ ] **Step 4: Run** `cargo test -j4 -p factory-cli --test plugin_files --test plugin_install --test global_command --test upgrade`, then `cargo clippy -j4 -p factory-cli -p factory-core --all-targets -- -D warnings` and `cargo fmt -p factory-cli -p factory-core -- --check`. Expected: PASS.

## Maintainer steps after merge

These need the real `claude` and network, so the driver does not run them.

1. Run `claude plugin validate .` and `claude plugin validate plugins/factory` from the repo root. Both must pass.
2. In a scratch session run `claude plugin marketplace add Snehal1112/rocket-factory`, then `claude plugin install factory@rocket-factory`, then `claude plugin list --json`. It must show `factory@rocket-factory` with the Cargo version.
3. Start a new Claude Code session and check which name works, `/factory` or `/factory:factory`. If only the namespaced form works, change the docs and the README examples to it and file a follow-up issue.
4. With the old `~/.claude/commands/factory.md` still present, run `factory install` and `factory upgrade --dry-run`. The output must match the texts in Task 2. Then remove the old file as the note says.
5. On the next release run `scripts/release.sh`. The bump commit must contain `plugins/factory/.claude-plugin/plugin.json` with the new version, and `claude plugin update factory@rocket-factory` must pick it up.

Next plan: docs/superpowers/plans/2026-10-30-harness-hardening/plan-05-live-smoke.md
