# Probe model, loud skips and live smoke Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-22-harness-ci-leftovers-design.md
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** A cross-harness probe sends each role the harness's default model, a missing fake-claude binary is built or fails its tests, a probe without agents names `--project`, and tests pin the live smoke scripts' modes and interactive step.

**Commit:** fix(probe): use the harness model and fail loudly

**Architecture:**
- `Driver::probe` in `crates/factory-engine/src/compat.rs` swaps the model fields when the harness differs.
- `Engine::shape_spec` in `ports.rs` takes the harness.
- The probe code sits behind the `AgentRunner` port, and tests use `FakeRunner`.
- The CLI test helper and the live smoke test are test code only.

**Tech Stack:** Rust, bash.

## Global Constraints

- Cargo commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- No task adds a dependency or edits any `Cargo.toml`, which is a risk path.
- The probe JSON gains one optional field, `model`, per role (`#[serde(default, skip_serializing_if = "Option::is_none")]`). No board, ledger or event field.
- The builder never runs git or `chmod`. The two script modes were set by hand on main before this job. `scripts/live-smoke.sh` is not edited.
- A same-harness probe is unchanged.

## Review Focus

1. A Codex project probed with `--harness codex`: a user expects the project's own models. Pinned by `a_same_harness_probe_keeps_the_project_models`.
2. A Codex project probed with `--harness claude`: a user expects the Claude defaults. Pinned as the second case of `a_cross_harness_probe_uses_the_harness_default_model`.
3. A production (non-probe) agent run: a user expects `shape_spec` to behave exactly as before. Pinned by the existing `codex_prep` and `agent_files` tests, which must pass unchanged.
4. A fresh worktree where fake-claude was never built, as in the factory's own checks: a user expects the tests to build it and run for real. Pinned by `fake_claude_builds_the_binary_when_it_is_missing`, which passes trivially when the binary already exists and runs the build path in the factory's fresh-worktree checks.
5. The fake tmux: a user expects no real tmux or claude to start. Pinned by `interactive_runs_with_a_fake_tmux`: tmux is replaced through `TMUX_BIN`, and `claude` through the shim on `PATH`.

## File Structure

- Modify: `crates/factory-engine/src/compat.rs` (`Driver::probe`), `crates/factory-engine/src/ports.rs` (`shape_spec`), `crates/factory-engine/src/probe.rs`
- Modify: `crates/factory-core/src/probe.rs` (`RoleProbe.model`)
- Modify: `crates/factory-cli/src/probe.rs` (`render_probe`)
- Modify: `crates/factory-engine/tests/probe.rs`, `crates/factory-cli/tests/probe_cmd.rs`, `crates/factory-cli/tests/common/mod.rs`, `crates/factory-cli/tests/plugin_install.rs`, `crates/factory-cli/tests/live_smoke_script.rs`

---

### Task 1: a cross-harness probe uses the harness model

**Files:**
- Modify: `crates/factory-engine/src/compat.rs`, `crates/factory-engine/src/ports.rs`, `crates/factory-engine/src/probe.rs`
- Modify: `crates/factory-core/src/probe.rs`, `crates/factory-cli/src/probe.rs`
- Modify: `crates/factory-engine/tests/probe.rs`, `crates/factory-cli/tests/probe_cmd.rs`

**Interfaces:**
- Port: behind `AgentRunner`. Tests use `FakeRunner` and the `fixture(harness)` helper of `crates/factory-engine/tests/probe.rs`, and `Fixture`, `ctx_with` and `probe` in `crates/factory-cli/tests/probe_cmd.rs`.
- Consumes: `factory_core::project::CODEX_DEFAULT_MODEL`, `Config::default()`, `Harness::from_config`.
- Produces:
  - `pub(crate) fn shape_spec(&self, spec: &mut AgentSpec, harness: Harness) -> Result<(), String>`;
  - `pub model: Option<String>` on `RoleProbe`, optional in serde;
  - `fn probe_config(config: &Config, harness: Harness) -> Config` in `compat.rs`, private.

**Tests added:**
- in factory-engine `tests/probe.rs`: `shape_spec_follows_the_probe_harness`;
- in factory-cli `tests/probe_cmd.rs`: `a_cross_harness_probe_uses_the_harness_default_model`, `a_same_harness_probe_keeps_the_project_models`, `the_probe_report_names_the_model`, and model asserts in `harness_flag_overrides_the_project_agent_cli`.

- [ ] **Step 1: Read the code**

Read these:
- `Driver::probe` in `crates/factory-engine/src/compat.rs`;
- `shape_spec` and its caller in `prepare_agent` in `ports.rs`;
- `probe_run`, `probe_base_spec` and `run_probe` in `crates/factory-engine/src/probe.rs`;
- `Harness`, `RoleProbe` and `ProbeReport` in `crates/factory-core/src/probe.rs`;
- `CODEX_DEFAULT_MODEL` and its use in `crates/factory-core/src/project.rs`;
- the codex branch of `Config::parse` in `crates/factory-core/src/config.rs`;
- `render_probe` in `crates/factory-cli/src/probe.rs`;
- `fixture` and the tests in `crates/factory-engine/tests/probe.rs`;
- `harness_flag_overrides_the_project_agent_cli` in `crates/factory-cli/tests/probe_cmd.rs`;
- the spec's Design section 1.

- [ ] **Step 2: Write the failing tests**

```rust
#[test]
fn a_cross_harness_probe_uses_the_harness_default_model() {
    let fx = Fixture::new(); // a Claude project, as `harness_flag_overrides_the_project_agent_cli` sets up
    let runner = Arc::new(FakeRunner::new(vec![AgentRun::done("MARKER: NONE")]));
    let ctx = ctx_with(&fx, runner.clone());
    // `MARKER: NONE` makes the probe report a failure, which this test does not need.
    let _ = probe(&ctx, &["--harness", "codex", "--role", "kb"]);
    let spec = runner.calls().pop().expect("one run");
    assert_eq!(spec.model.as_deref(), Some(factory_core::project::CODEX_DEFAULT_MODEL));
}
```

Put the test where its helpers live: `tests/probe_cmd.rs` has `Fixture`, `ctx_with` and `probe`. Check the spec's model field name against `AgentSpec`.

| Test | Setup | Assertion |
|---|---|---|
| `a_cross_harness_probe_uses_the_harness_default_model`, second case | a project with `agent_cli = "codex"` and `builder_model = "gpt-x"`, then `--harness claude --role builder` | the spec model equals `Config::default().builder_model` |
| `a_same_harness_probe_keeps_the_project_models` | a Claude project with `reviewer_model = "claude-custom"`, `--harness claude --role reviewer` | the spec model is `claude-custom` |
| `harness_flag_overrides_the_project_agent_cli` | unchanged | also asserts the model is `CODEX_DEFAULT_MODEL` |
| `the_probe_report_names_the_model` | the run of the first test, rendered with `render_probe` (or the command's stdout) | contains `model: gpt-5.6-luna` |
| `shape_spec_follows_the_probe_harness` (`factory-engine/tests/probe.rs`) | `fixture(Harness::Claude)` (config `agent_cli` Claude), with `.with_agent_files(files())` as `spec_of` does, then `engine.probe_role_with(ProbeRole::Builder, Harness::Codex, ..)` | the recorded spec has the Codex shape: `agent == None`, and `agent_prompt` starts with `You run under Codex.` (`agent_prompt.is_some()` alone holds even before the fix) |

To set a project's models or `agent_cli`, write `.claude/factory/config.toml` in `fx.root` (keep `min_free_disk_mb = 0` in it), as `Fixture::with_risk_path` in `crates/factory-cli/tests/common/mod.rs` does. `probe` returns `Err(CliError::ProbeFailed(text))` for `MARKER: NONE`, so the report test takes the text from `Ok(t) | Err(CliError::ProbeFailed(t))`.

- [ ] **Step 3: Run them and see them fail**

Run: `cargo test -j4 -p factory-cli --test probe_cmd`, then `cargo test -j4 -p factory-engine --test probe shape_spec_follows`
Expected:
- `a_cross_harness_probe_uses_the_harness_default_model` and the new assert in `harness_flag_overrides_the_project_agent_cli` fail with `claude-sonnet-5-5`;
- `the_probe_report_names_the_model` fails because there is no `model:` line;
- `shape_spec_follows_the_probe_harness` fails because the spec keeps the Claude shape;
- `a_same_harness_probe_keeps_the_project_models` passes, because it pins today's behaviour.

- [ ] **Step 4: Implement**

- `probe_config`:
  - Clone the config and set `agent_cli` to the harness.
  - When `harness.as_str() != config.agent_cli()`:
    - for Codex, set `builder_model`, `reviewer_model` and `kb_model` to `CODEX_DEFAULT_MODEL`, and `reviewer_model_large` and `escalation_model` to `None`;
    - for Claude, set all five to the `Config::default()` values.
  - Check each field's type, `String` or `Option<String>`, in `Config`.
- `Driver::probe` uses `probe_config`.
- `shape_spec` takes `harness` and branches on `harness == Harness::Codex` in place of `self.config.agent_cli() == "codex"`. `prepare_agent` passes `Harness::from_config(&self.config)`, and `probe_run` passes its `harness`.
- `RoleProbe.model` is filled from the run spec's model in `probe_run`.
- `render_probe` prints `  model: <id>` on its own line after each role's row when the model is known.

- [ ] **Step 5: Run them and see them pass**

Run: `cargo test -j4 -p factory-engine --test probe`, `cargo test -j4 -p factory-engine --test codex_prep`, `cargo test -j4 -p factory-engine --test agent_files`, `cargo test -j4 -p factory-cli --test probe_cmd`, `cargo test -j4 -p factory-core probe`.
Expected: all pass, 0 failed.

### Task 2: fake-claude is built when missing, and a probe without agents names `--project`

**Files:**
- Modify: `crates/factory-cli/tests/common/mod.rs`, `crates/factory-cli/tests/probe_cmd.rs`, `crates/factory-cli/tests/plugin_install.rs`
- Modify: `crates/factory-engine/src/probe.rs`, `crates/factory-engine/tests/probe.rs`

**Interfaces:**
- Port: test helpers, plus the probe's error text behind `AgentRunner`. `prepare_for_codex` in `codex_prep.rs` is not changed, because it also runs in real jobs.
- Produces: `pub fn fake_claude() -> PathBuf` in `crates/factory-cli/tests/common/mod.rs`, which no longer returns an `Option`.

**Tests added:** `a_probe_without_agents_names_the_project_flag` (in `factory-engine/tests/probe.rs`) and `fake_claude_builds_the_binary_when_it_is_missing` (in `probe_cmd.rs`). The callers of `fake_claude` lose their early returns.

- [ ] **Step 1: Read the code**

Read these:
- `fake_claude` in `crates/factory-cli/tests/common/mod.rs`, and every caller: `probe_through_fake_claude_finds_the_marker` in `probe_cmd.rs`, and `Fixture::new` and `detects_nothing_from_bad_output` in `plugin_install.rs`;
- `hide_claude_tools` and `probe_run` in `crates/factory-engine/src/probe.rs`, and `prepare_for_codex` in `codex_prep.rs`;
- `missing_agent_file_is_an_error` in `crates/factory-engine/tests/probe.rs`.

- [ ] **Step 2: Write the failing tests**

`a_probe_without_agents_names_the_project_flag`:
- Copy the setup and call of `missing_agent_file_is_an_error`, then take the error with `.expect_err("no agent file")`.
- Assert its text contains `agent file factory-builder not found`, and ends with `; pass --project <name>, or run inside a registered project`.
- Add a second case with `Harness::Codex` and the Codex fixture of that file, with the same assertions.

`fake_claude_builds_the_binary_when_it_is_missing`: call `common::fake_claude()` and assert the returned path exists. In a worktree that never built the binary, this runs the build path.

- [ ] **Step 3: Run them and see them fail**

Run: `cargo test -j4 -p factory-engine --test probe a_probe_without_agents`
Expected: it fails on the `ends_with` assert.

- [ ] **Step 4: Implement**

- In `probe_run`, where the probe maps the `shape_spec` error, and in `hide_claude_tools`, append `; pass --project <name>, or run inside a registered project` to an `agent file {name} not found` error. Leave `prepare_for_codex` unchanged.
- `fake_claude()`:
  - When the binary is missing, run `std::process::Command::new(env!("CARGO"))` with `["build", "-j4", "-p", "factory-agent", "--bin", "fake-claude"]` in the workspace root, which is `CARGO_MANIFEST_DIR/../..`.
  - Guard the build with a `static BUILD: std::sync::Mutex<()>` in `common`, and check `is_file()` again under the lock, so parallel tests build once.
  - When that build fails, or the binary is still missing, panic with `build fake-claude with `cargo build -j4 -p factory-agent` first`.
  - Return the path.
- The callers take the path directly. `plugin_install.rs` `Fixture::new` returns `Fixture`, not `Option<Fixture>`, and the tests drop their early returns. In `Fixture::new` and `detects_nothing_from_bad_output`, call `common::fake_claude()` before `EnvGuard::set`. Do not take `ENV_LOCK` by hand: `EnvGuard::set` takes it, and a second lock on the same thread deadlocks. The rustup proxy sets `CARGO_HOME`, so the build does not depend on `HOME`.

- [ ] **Step 5: Run them**

Run: `cargo test -j4 -p factory-cli --test probe_cmd`, `cargo test -j4 -p factory-cli --test plugin_install`, `cargo test -j4 -p factory-engine --test probe`, `cargo test -j4 -p factory-engine --test codex_prep`.
Expected: all pass, 0 failed.

### Task 3: tests pin the live smoke script modes and its interactive step

**Files:**
- Modify: `crates/factory-cli/tests/live_smoke_script.rs`

**Interfaces:**
- Port: shell scripts, run from Rust tests with `std::process::Command`. There is no engine code, and the scripts do not change.
- Consumes: `run_script(args, envs)` in `live_smoke_script.rs`, whose `envs` are applied after its defaults and so win. `TMUX_BIN`, `--no-probe` and `interactive_step` come from `scripts/live-smoke.sh`.

**Tests added:** `the_scripts_are_executable`, `interactive_runs_with_a_fake_tmux`.

- [ ] **Step 1: Read the code**

Read `scripts/live-smoke.sh` in full: `interactive_step`, the argument loop (`--no-probe`, `--interactive`), `need`, the `$HOME/.claude/commands/factory.md` check and the `out` layout. Read every helper and test in `crates/factory-cli/tests/live_smoke_script.rs`, `fails_when_the_probe_fails` in particular, and `crates/factory-cli/tests/support/claude-shim.sh`.

- [ ] **Step 2: Write the tests**

`the_scripts_are_executable`, gated with `#[cfg(unix)]`:
- For `scripts/live-smoke.sh` and `crates/factory-cli/tests/support/claude-shim.sh`, read `std::fs::metadata(path).permissions().mode()`, using `std::os::unix::fs::PermissionsExt`.
- Assert `mode & 0o111 == 0o111` with the path in the message.
- Resolve the paths from `env!("CARGO_MANIFEST_DIR")`.

`interactive_runs_with_a_fake_tmux`, gated with `#[cfg(unix)]`:
- Make your own tempdir holding `home/.claude/commands/factory.md` and a `tmux` script, made executable with `std::fs::set_permissions(.., Permissions::from_mode(0o755))` as the file does for the shim.
- For `capture-pane` the fake prints a pane holding `? for shortcuts`, `merged` and `001`. For every other subcommand it exits 0 and prints nothing.
- Call `run_script(&["--no-probe", "--interactive"], &[("TMUX_BIN", <fake path>), ("HOME", <your home>)])`.
- Assert exit code 0, and that `out/interactive/pane.txt` under the tmp dir `run_script` returns contains `001`.

Shape the fake's `capture-pane` reply to match how `interactive_step` waits for its text.

- [ ] **Step 3: Run them**

Run: `cargo test -j4 -p factory-cli --test live_smoke_script`
Expected: both pass, because the modes were set by hand before this job. If `interactive_runs_with_a_fake_tmux` fails, fix the fake, not the script.

- [ ] **Step 4: Format and lint**

Run: `cargo fmt -p factory-engine`, `cargo fmt -p factory-cli`, `cargo fmt -p factory-core`, then `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`, `cargo clippy -j4 -p factory-cli --all-targets -- -D warnings`, `cargo clippy -j4 -p factory-core --all-targets -- -D warnings`.
Expected: clippy finishes with no warnings.

Next plan: docs/superpowers/plans/2026-11-22-harness-ci-leftovers/plan-02-harness-ci-leftovers.md
