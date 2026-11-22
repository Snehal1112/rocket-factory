# Stronger tests and the reference docs Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-22-issue-132-wrap-up-design.md
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Six tests fail when the code they guard is broken, and the reference docs carry the live probe findings and the current facts.

**Commit:** test(factory): pin weak tests and refresh docs

**Architecture:** Test-only edits in factory-core, factory-engine and factory-cli, then Markdown edits. No production code changes in this plan.

**Tech Stack:** Rust, Markdown.

## Global Constraints

- Cargo commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- No task adds a dependency or edits any `Cargo.toml`, `.github/**`, `scripts/**` or `journeybook/**`.
- Every expected path in a test comes from `std::fs::canonicalize` or from the code under test, because macOS resolves `/var` to `/private/var`.

## Review Focus

1. The merge helper that leaves main's index stale: a user expects the later commit's tree to keep the job's files. Pinned by `a_scoped_job_merged_with_a_merge_commit_resumes`, which checks out main inside the helper.
2. The fake tmux must not pass when the script never types the command. Pinned by `interactive_runs_with_a_fake_tmux`, which now gates its output on the logged `send-keys`.
3. The KB `add_dirs` assert on macOS: a user expects CI on macOS to pass. Pinned by `a_reply_with_pages_is_refreshed` comparing canonical paths.
4. A docs line that names a test or function that does not exist: a user expects every name to be real. Each docs step names only items this job or the code already has.
5. `docs/README.md` must still index every doc after the edits. No doc is added or removed, so the index is unchanged.

## File Structure

- Modify: `crates/factory-core/src/resolve.rs` (tests only)
- Modify: `crates/factory-engine/tests/agent_files.rs`, `crates/factory-engine/tests/merge.rs`, `crates/factory-engine/tests/kb.rs`, `crates/factory-engine/tests/common/mod.rs`
- Modify: `crates/factory-cli/tests/live_smoke_script.rs`, `crates/factory-cli/tests/probe_cmd.rs`
- Modify: `docs/reference/software-factory.md`, `docs/reference/project-toml-reference.md`, `CLAUDE.md`

---

### Task 1: core and engine tests guard the retain, the cwd, the hand merge and the KB path

**Files:**
- Modify: `crates/factory-core/src/resolve.rs`
- Modify: `crates/factory-engine/tests/agent_files.rs`, `crates/factory-engine/tests/merge.rs`, `crates/factory-engine/tests/kb.rs`, `crates/factory-engine/tests/common/mod.rs`

**Interfaces:**
- Port: test code only. The engine tests run through `Fixture` and `MergeFixture` with the scripted `AgentRunner`; git is the concrete `factory_git::Repo` through `git2` in the helpers.
- Produces: `pub fn merge_by_hand_no_ff(fx: &MergeFixture) -> String` in `crates/factory-engine/tests/common/mod.rs`, returning the merge commit sha.

**Tests added:** `a_scoped_job_merged_with_a_merge_commit_resumes` (`merge.rs`); changed `a_retired_agent_is_dropped_from_every_layer_with_a_note`, `every_builder_and_the_reviewer_start_in_the_project_folder` (through `fix_builder_spec`) and `a_reply_with_pages_is_refreshed`.

- [ ] **Step 1: Read the code**

Read `a_retired_agent_is_dropped_from_every_layer_with_a_note`, `merged_agents` and `note_drift` in `crates/factory-core/src/resolve.rs` and `agent_drift` in `crates/factory-core/src/agent_drift.rs`. Read `fix_builder_spec`, `worktree` and `every_builder_and_the_reviewer_start_in_the_project_folder` in `crates/factory-engine/tests/agent_files.rs`. Read `merge_by_hand`, `commit_file`, `main_tip` and `approved_scoped_job` in `crates/factory-engine/tests/common/mod.rs`, `a_scoped_job_with_a_merged_base_resumes` in `merge.rs`, and the `BaseState::Merged` branches in `crates/factory-engine/src/merge.rs`. Read `with_kb` and `a_reply_with_pages_is_refreshed` in `crates/factory-engine/tests/kb.rs` and `kb_refresh_step` in `crates/factory-engine/src/kb.rs`.

- [ ] **Step 2: Write the changed and new tests**

- Retired agent: the preset copy text becomes `"preset committer\n## Report\nx\n"`; the project and tip copies stay one line. The existing `count() == 2` assert now fails when the preset-layer `retain` is removed, because a drift note names the file.
- Fix builder: `fix_builder_spec(idle_secs) -> (Fixture, factory_agent::AgentSpec)`. Both call sites become `let (fx, spec) = fix_builder_spec(0);` and `assert_eq!(spec.cwd, worktree(&fx).join("apps/web"), "resumed fix builder");`, and the same with `1_000` and `"fresh fix builder"`.
- Hand merge: `merge_by_hand_no_ff` opens `fx.main` with `git2::Repository::open`, builds a commit with the tree of `fx.head`, parents (main tip, head) and message `hand merge`, moves `refs/heads/main` to it, force-checks out as `merge_by_hand` does, and returns its sha.

```rust
#[test]
fn a_scoped_job_merged_with_a_merge_commit_resumes() {
    let fx = common::approved_scoped_job(&["apps/web/a.txt"]);
    let merge = common::merge_by_hand_no_ff(&fx);
    let later = common::commit_file(&fx.main, "apps/api/later.txt", "x");
    assert_ne!(later, merge);
    let r = merge_stage(&fx.driver, &fx.id).expect("stage");
    assert_eq!(r, MergeResult::Merged(later.clone()));
    assert_eq!(common::main_tip(&fx.main), later);
}
```

If `commit_file` returns something other than the sha string, adapt the two compares to what it returns.

- KB path: in `a_reply_with_pages_is_refreshed`, the `add_dirs` assert compares with `vec![fx.config.kb_dir().expect("kb dir").canonicalize().expect("canon")]`.

- [ ] **Step 3: Run them and check what each proves**

Run: `cargo test -j4 -p factory-core resolve`, `cargo test -j4 -p factory-engine --test agent_files`, `cargo test -j4 -p factory-engine --test merge`, `cargo test -j4 -p factory-engine --test kb`
Expected: all pass on today's code, 0 failed. These tests pin code that already works. To check that the retired-agent test now bites, delete the preset-layer `by_name.retain(..)` inside the `validate_name` block of `merged_agents`, run `cargo test -j4 -p factory-core retired`, see `a_retired_agent_is_dropped_from_every_layer_with_a_note` fail, and restore the line.

### Task 2: the fake tmux checks the typed command, and the probe asserts use the constant

**Files:**
- Modify: `crates/factory-cli/tests/live_smoke_script.rs`, `crates/factory-cli/tests/probe_cmd.rs`

**Interfaces:**
- Port: test code only.
- Consumes: `run_script(args, envs)` in `live_smoke_script.rs`, `factory_core::project::CODEX_DEFAULT_MODEL`.

**Tests added:** changed `interactive_runs_with_a_fake_tmux`, `a_codex_probe_sends_the_codex_model_to_every_role` and `the_probe_report_names_the_model` in `probe_cmd.rs`.

- [ ] **Step 1: Read the code**

Read `run_script`, `interactive_runs_with_a_fake_tmux` in `crates/factory-cli/tests/live_smoke_script.rs` and `interactive_step` in `scripts/live-smoke.sh` (it now sets `PATH=<folder of FACTORY_BIN>:` for the claude session). Read the two asserts in `crates/factory-cli/tests/probe_cmd.rs` that spell `model: gpt-5.6-luna`.

- [ ] **Step 2: Write the changed tests**

The fake tmux, written to `tmp.path().join("tmux")` with mode 0o755 as today, logs every call and shows the merged job only after the command was typed:

```text
#!/bin/sh
log=<tempdir>/tmux.log
echo "$*" >> "$log"
if [ "$1" = capture-pane ]; then
  printf '? for shortcuts\n'
  if grep -q 'send-keys .*/factory status' "$log"; then printf 'merged\n001\n'; fi
fi
exit 0
```

Build the script text with `format!` so `<tempdir>` is the real log path. After the run, read `tmux.log` and assert:
- it has a line containing `send-keys` and `/factory status`;
- it has a line starting with `kill-session`;
- it has a `new-session` line containing `format!("PATH={}:", Path::new(env!("CARGO_BIN_EXE_factory")).parent().expect("bin dir").display())`.
Keep the exit code and `pane.txt` asserts.

In `probe_cmd.rs`, both asserts build the needle as `format!("model: {}", factory_core::project::CODEX_DEFAULT_MODEL)`.

- [ ] **Step 3: Run them and check what each proves**

Run: `cargo test -j4 -p factory-cli --test live_smoke_script`, `cargo test -j4 -p factory-cli --test probe_cmd`
Expected: all pass, 0 failed. To check that the tmux test now bites, change the fake's grep pattern to `send-keys .*/factory nothing`, see the test fail because `merged` never shows, and restore the pattern. Do not edit `scripts/live-smoke.sh`.

### Task 3: the reference docs

**Files:**
- Modify: `docs/reference/software-factory.md`, `docs/reference/project-toml-reference.md`, `CLAUDE.md`

**Interfaces:**
- Port: none. Docs only.
- Consumes: the names this job adds: `codex_runtime_tool_map`, `evaluate_with_absent`, the `Not installed (<role>)` line, `production_lines`.

**Tests added:** none; `agent_lint` and `check_coverage_docs` must still pass.

- [ ] **Step 1: Read the docs and the code they describe**

Read in `docs/reference/software-factory.md`: the title and status lines, "Using Codex", "Checking what agents see" and "Harness text". Read the top stamp, the line citing `project.rs:37` and the "Checks" section of `docs/reference/project-toml-reference.md`. Read `CLAUDE.md` "Crate layering" and "Carried over from Rocket". Read the spec's Overview and Design section 8 for the facts to write.

- [ ] **Step 2: Edit `software-factory.md`**

- Title `# Software Factory`. Status line: "Status: the standalone Rust driver. Design: `docs/superpowers/specs/2026-10-16-standalone-software-factory-design.md`." Keep the v1 and v2 superseded note.
- "Checking what agents see": change "the roles to all five" to "the roles to all four". Replace the line starting `Findings:` with a `Findings (2026-11-22):` list:
  - Claude 2.1.330: agent text reaches every role; the builder gets the superpowers bootstrap and skills; the reviewer and spec reviewer list no skills, so `--disable-slash-commands` blocks skills; the builder and kb also list 21 user and plugin skills, which the Claude probe does not check.
  - Codex 0.169.0 on `gpt-5.6-luna`: agent text reaches every role; the builder lists exactly the 14 superpowers skills, so the allowlist works with plugins and apps off; the other roles named a `factory` skill taken from the command-only `### Model option` section of the map, which run prompts now leave out; their catalog is empty.
  - Headless manual runs of `/factory status` and `$factory status` both worked; Claude refused two commands that held a shell variable (`B=${FACTORY_BIN:-factory}; ...`), which is why the command now calls `factory` literally.
  - Under Codex a role's named skill that is not installed (kb's `kb-refresh` lives in `~/.claude/skills` only) shows `Not installed (kb): kb-refresh, short rules used` and does not fail the probe.
- "Using Codex": in the bullet that starts "The agent file text and a short preface go into the prompt", replace "an empty list, or one that matches no installed skill, gives `skills.include_instructions=false`" with "an empty list, or one that matches no installed skill, gives `skills.include_instructions=false`, `skills.bundled.enabled=false` and every found skill disabled by path". Then add a sentence that the role plugin lists are the skill allowlist that replaced the planned `codex_builder_skills` setting.
- "Harness text": add that the shared command body must end with the exact Claude map, that `factory install` refuses otherwise so no Claude tool name reaches the Codex skill, and that Codex run prompts get the map without its `### Model option` section (`codex_runtime_tool_map`).

- [ ] **Step 3: Edit `project-toml-reference.md` and `CLAUDE.md`**

- Stamp: "Verified against the code on 2026-11-22 (commit `<short sha of this job's base>`)". The worktree HEAD holds the driver's commits for earlier tasks, so read the base with `git merge-base HEAD main` and then `git log -1 --format=%h <that sha>`.
- Replace `(`factory-core/src/project.rs:37`)` with "(`#[serde(deny_unknown_fields)]` on `ProjectFile` in `factory-core/src/project.rs`)".
- In "Checks", after the `commands` example, add `format` and `lint` lines to the TOML example and two bullets: `format` lists commands planned after `commands` whose results show as the `format` gate; `lint` lists commands planned after `format` whose results show as the `lint` gate. Then reword the rust preset paragraph after the bullets: for the `crates/*/**` rule, `commands` holds `cargo check -j4 -p {crate}`, `cargo test -j4 -p {crate}` and `cargo test -j4 -p {dependents}`, `format` holds `cargo fmt -p {crate} -- --check` and `lint` holds the clippy command; for the root `src/**` rule, `commands` holds `cargo check -j4` and `cargo test -j4`, `format` holds `cargo fmt -- --check` and `lint` holds `cargo clippy -j4 --all-targets -- -D warnings`. Copy the commands from `crates/factory-core/presets/rust/config.toml`.
- `CLAUDE.md` crate layering: factory-core reads "domain rules plus home, project and preset loading: board, ledger, config, risk, verdict, budget (no I/O crate deps)".
- `CLAUDE.md` traits bullet: "The workspace has seven traits: `AgentRunner` and `AgentStream` (factory-agent), `Tracker` (factory-engine), `BaseTipFiles` (factory-core), `FactoryCli` and `Spawner` (factory-supervisor), `LineSink` (factory-telemetry). `AgentRunner`, `Tracker`, `FactoryCli`, `Spawner` and `BaseTipFiles` have fakes in tests; `LineSink` has the in-memory `Memory` sink behind `factory_telemetry::capture`, which tests use; `AgentStream` has only real implementations. Of the engine's own ports only `AgentRunner` and `Tracker` sit behind traits; `tipfiles.rs` implements `BaseTipFiles` for the resolver." Keep the rest of the bullet (git, checks and `procinfo.rs` are concrete; `ports.rs` holds no traits).
- `CLAUDE.md` "Carried over from Rocket": drop "(plan 3)" and "(plan 7)". Say that `.claude/factory/config.toml` is Rocket's old config kept for the tests that read it, that the presets in `crates/factory-core/presets/` replaced it for registered projects, and that `factory install` writes the command from `factory-core/assets/global-command.md`. Keep the sentence on the prose docs.

- [ ] **Step 4: Run the docs checks**

Run: `cargo test -j4 -p factory-cli --test agent_lint`, then `cargo test -j4 -p factory-cli --test check_coverage_docs`.
Expected: all pass, 0 failed.

- [ ] **Step 5: Format and lint**

Run: `cargo fmt -p factory-core`, `cargo fmt -p factory-engine`, `cargo fmt -p factory-cli`, then `cargo clippy -j4 -p factory-core --all-targets -- -D warnings`, `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`, `cargo clippy -j4 -p factory-cli --all-targets -- -D warnings`.
Expected: clippy finishes with no warnings.

Next plan: none
