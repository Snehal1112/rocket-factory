# Builder Permissions 2, Plan 1: Flags, allowlist and builder specs

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Send `--disallowedTools`, build the builder allowlist and deny rules from the config, and apply a permission mode, the allowlist, the deny rules and the job folder to all three builder specs.

**Architecture:** `factory-agent` gets one new `AgentSpec` field and one flag. `factory-engine` gets a new `permissions.rs` with two pure functions (no port is needed: they read only the `Config` and a path, and `specs.rs` already reads the job folder directly). `Engine::with_builder_permissions` in `specs.rs` applies them, and the three builder specs call it before `with_limits`.

**Tech Stack:** Rust 2021, `factory-core` (`Config`, `NEVER_ALLOWED_PROGRAMS`, built in series 1), `factory-agent`, `factory-engine`.

**Spec:** docs/superpowers/specs/2026-10-19-builder-permissions-design.md

## Global Constraints

- Always `-j4`; never `--workspace` or `--all`; never set `CARGO_TARGET_DIR`.
- Only the three builder specs change. Every other role's argument list stays byte-identical, and a test pins that.
- `--disallowedTools a,b` goes right after `--allowedTools`, and only when the list is not empty. The flag order of `AgentSpec::args` is then `--permission-mode`, `--allowedTools`, `--disallowedTools`, `--tools`, `--add-dir`.
- With `builder_permissions = false` the three builder specs equal today's, field for field.
- Allowlist order is fixed: `Read`, `Grep`, `Glob`, `Skill`, `Agent`; then `Bash(<name>:*)` for `cd`, `ls`, `cat`, `head`, `tail`, `wc`, `grep`, `find`, `pwd`; then `Bash(git status:*)`, `Bash(git diff:*)`, `Bash(git log:*)`, `Bash(git show:*)`; then one `Bash(<program>:*)` per program of `config.checks[*].commands` and then `config.check_hints`, in order of first use, without duplicates; then `config.builder_allow` as written.
- A program is never derived when its basename is in `factory_core::NEVER_ALLOWED_PROGRAMS`, or when it holds anything but letters, digits and `_ - . /`. The program is the first word after any leading `NAME=value` words, and the rule keeps that word as written (`./scripts/test.sh` stays `./scripts/test.sh`).
- The deny rules are `Edit(//<job dir>/**)` and `Write(//<job dir>/**)` with the canonical job folder and no leading `/` after the `//`, as `kb_allowed_tools` writes it in `crates/factory-engine/src/kb.rs`.
- The allowlist never contains `commit`, `add`, `push`, `reset`, `rm`, `curl` or a shell.

## Review Focus

- A job folder path with `*`, `?`, `[` or `{` leaves the spec without any permission field (Task 3 test `glob_character_in_job_dir_leaves_the_spec_unchanged`).
- A symlinked job folder gets deny rules for the real path (Task 2 test `job_dir_is_canonicalized_in_the_deny_rules`).
- A check command `FOO=1 cargo test` derives `cargo`, and `git diff --check` derives nothing (Task 2 tests `leading_env_assignment_is_skipped`, `git_rm_rmdir_curl_sudo_sh_bash_env_xargs_are_never_derived`).
- A program with `*`, `(`, `)` or `:` is skipped (Task 2 test `wildcard_program_is_skipped`).
- A prose `check_hints` entry such as `run the tests` adds only `Bash(run:*)` (Task 2 test `prose_check_hint_adds_only_a_harmless_rule`).
- The reviewer, committer, message and KB arguments do not change (Task 3 test `reviewer_committer_message_and_kb_arguments_are_unchanged`).
- A resumed builder run carries the permission flags again, because every run is a new `claude` process (Task 3 test `builder_specs_carry_the_permission_flags` covers the resume spec).

---

## File Structure

- `crates/factory-agent/src/spec.rs` (modify): `disallowed_tools` field, default, flag.
- `crates/factory-agent/tests/stream.rs`, `crates/factory-agent/tests/runner.rs` (modify): the flag tests.
- `crates/factory-engine/src/permissions.rs` (new): `builder_allowed_tools`, `builder_denied_tools`, program derivation, unit tests.
- `crates/factory-engine/src/lib.rs` (modify): `mod permissions;` and the re-export of the two functions.
- `crates/factory-engine/src/specs.rs` (modify): `Engine::with_builder_permissions`, called by the three builder specs.
- `crates/factory-engine/tests/build_checks.rs`, `crates/factory-engine/tests/permission_pins.rs` (new): the spec tests and the pinned argument lists.

### Task 1: `disallowed_tools` in the agent spec

**Files:**
- Modify: `crates/factory-agent/src/spec.rs`, `crates/factory-agent/tests/stream.rs`, `crates/factory-agent/tests/runner.rs`

**Interfaces:**
- Produces: `AgentSpec.disallowed_tools: Vec<String>` (empty from `AgentSpec::new`); `AgentSpec::args` pushes `--disallowedTools <list joined by ,>` right after the `--allowedTools` push, only when the list is not empty.

- [ ] **Step 1: Read** `AgentSpec::args` in `crates/factory-agent/src/spec.rs`, the `args_*` tests and the `caps(agent, max_turns, append)` helper in `crates/factory-agent/tests/stream.rs`, and `args_reach_the_child_and_prompt_goes_to_stdin` in `crates/factory-agent/tests/runner.rs`.
- [ ] **Step 2: Write the failing tests.** In `stream.rs`:
  - `args_of_a_default_spec_are_unchanged` (this one passes before the change and pins today's lists):
    ```rust
    #[test]
    fn args_of_a_default_spec_are_unchanged() {
        let spec = AgentSpec::new("001", "reviewer", "/w", "p");
        assert_eq!(
            spec.args(&caps(true, true, true)),
            vec!["-p", "--output-format", "stream-json", "--verbose"]
        );
        let mut full = AgentSpec::new("001", "builder", "/w", "p");
        full.strict_mcp = true;
        full.model = Some("sonnet".into());
        full.resume = Some("s1".into());
        full.permission_mode = Some("dontAsk".into());
        full.allowed_tools = vec!["Read".into(), "Bash(ls:*)".into()];
        full.tools = Some("Read,Bash".into());
        full.add_dirs = vec!["/kb".into()];
        full.agent = Some("factory-builder".into());
        full.max_turns = Some(40);
        assert_eq!(
            full.args(&caps(true, true, true)),
            vec![
                "-p", "--output-format", "stream-json", "--verbose", "--strict-mcp-config",
                "--model", "sonnet", "--resume", "s1", "--permission-mode", "dontAsk",
                "--allowedTools", "Read,Bash(ls:*)", "--tools", "Read,Bash",
                "--add-dir", "/kb", "--agent", "factory-builder", "--max-turns", "40",
            ]
        );
    }
    ```
  - `disallowed_tools_are_sent_after_allowed_tools`: a spec with `allowed_tools = ["Read"]` and `disallowed_tools = ["Edit(//j/**)", "Write(//j/**)"]`; find the index `i` of `--allowedTools` in `args`; assert `args[i + 2] == "--disallowedTools"` and `args[i + 3] == "Edit(//j/**),Write(//j/**)"`.
  - `empty_disallowed_tools_add_no_flag`: a default spec has no `--disallowedTools` in `args`.
  - In `runner.rs`, `builder_run_receives_permission_mode_allow_and_deny_flags`, modelled on `args_reach_the_child_and_prompt_goes_to_stdin` (mode `args`, the `fake-claude` binary): set `permission_mode = Some("acceptEdits")`, `allowed_tools = ["Read"]`, `disallowed_tools = ["Edit(//j/**)"]` and `add_dirs = [tmp path]`; assert `run.result_text` contains `--permission-mode acceptEdits --allowedTools Read --disallowedTools Edit(//j/**) --add-dir`.
- [ ] **Step 3: Run** `cargo test -j4 -p factory-agent` and expect a compile failure: `no field 'disallowed_tools' on type 'AgentSpec'`.
- [ ] **Step 4: Implement** the field (doc comment: tool rules the run may never use), its default in `AgentSpec::new`, and the flag push in `args`.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-agent` and expect PASS, including the unchanged pinned test.

### Task 2: The allowlist and the deny rules

**Files:**
- Create: `crates/factory-engine/src/permissions.rs`
- Modify: `crates/factory-engine/src/lib.rs`

**Interfaces:**
- Produces: `pub fn builder_allowed_tools(config: &Config) -> Vec<String>`; `pub fn builder_denied_tools(job_dir: &Path) -> Vec<String>` (canonicalizes `job_dir`, falling back to the path as given, and returns the `Edit` rule then the `Write` rule); private `fn derived_programs(config: &Config) -> Vec<String>` and `fn program_of(command: &str) -> Option<String>`.
- Produces: `pub use permissions::{builder_allowed_tools, builder_denied_tools};` in `lib.rs`.
- Consumes: `factory_core::{Config, CheckRule, NEVER_ALLOWED_PROGRAMS}`.

- [ ] **Step 1: Read** `kb_allowed_tools` in `crates/factory-engine/src/kb.rs` (the `//` rule style) and the `CheckRule` struct in `crates/factory-core/src/config.rs`.
- [ ] **Step 2: Write the failing tests** in a `#[cfg(test)] mod tests` of `permissions.rs`. Build configs with `Config { checks: vec![CheckRule { paths: vec!["**".into()], commands: vec![...] }], check_hints: vec![...], builder_allow: vec![...], ..Config::default() }`.
  - Full example:
    ```rust
    #[test]
    fn base_list_is_fixed_and_ordered() {
        let tools = builder_allowed_tools(&Config::default());
        let want: Vec<String> = [
            "Read", "Grep", "Glob", "Skill", "Agent",
            "Bash(cd:*)", "Bash(ls:*)", "Bash(cat:*)", "Bash(head:*)", "Bash(tail:*)",
            "Bash(wc:*)", "Bash(grep:*)", "Bash(find:*)", "Bash(pwd:*)",
            "Bash(git status:*)", "Bash(git diff:*)", "Bash(git log:*)", "Bash(git show:*)",
        ].iter().map(|s| s.to_string()).collect();
        assert_eq!(tools, want);
    }
    ```
  - The others, by name and inputs (expected values in the last column):

    | Test | Config input | Expected |
    |---|---|---|
    | `rust_preset_derives_cargo` | `checks` command `cargo test -j4 -p {crate}` | list ends with `Bash(cargo:*)`, once |
    | `go_style_commands_derive_go` | commands `go vet ./...`, `go test ./...` | one `Bash(go:*)` after the base |
    | `leading_env_assignment_is_skipped` | command `FOO=1 BAR=2 cargo test` | `Bash(cargo:*)` |
    | `git_rm_rmdir_curl_sudo_sh_bash_env_xargs_are_never_derived` | one command each: `git diff --check`, `rm -rf x`, `rmdir x`, `curl u`, `sudo x`, `sh -c x`, `bash -c x`, `env A=1 x`, `xargs x`, `/bin/rm x` | list equals the base list |
    | `prose_check_hint_adds_only_a_harmless_rule` | `check_hints = ["run the tests"]` | base plus `Bash(run:*)` only |
    | `wildcard_program_is_skipped` | commands `a*b x`, `a(b) x`, `a:b x` | base list only |
    | `duplicates_collapse_to_first_position` | commands `cargo a`, `make b`, `cargo c`, hint `make d` | base then `Bash(cargo:*)`, `Bash(make:*)` |
    | `builder_allow_comes_last` | command `cargo t`, `builder_allow = ["Bash(make:*)"]` | `Bash(cargo:*)` then `Bash(make:*)` last |
    | `script_path_program_keeps_its_path` | command `./scripts/test.sh x` | `Bash(./scripts/test.sh:*)` |
    | `allowlist_never_contains_commit_add_push_rm_curl` | for each preset name in `factory_core::preset::PRESET_NAMES`, `factory_core::preset::embedded(name)` parsed into a `Config` the way `both_presets_exist_and_parse_as_config` in `crates/factory-core/src/preset.rs` does | no entry contains `commit`, `add`, `push`, `reset`, `rm`, `curl`, `sudo`, `Bash(sh`, `Bash(bash` |
    | `denied_tools_cover_the_job_folder` | a `tempfile::tempdir()` as job folder | exactly two rules: `Edit(//<canonical path without the leading slash>/**)` then `Write(...)` |
    | `job_dir_is_canonicalized_in_the_deny_rules` | a symlink to a real folder (`std::os::unix::fs::symlink`) | the rules name the real path, not the link |
- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine permissions` and expect a compile failure: `cannot find function 'builder_allowed_tools' in this scope`.
- [ ] **Step 4: Implement** the two functions and the helpers from the signatures and the Global Constraints, add `mod permissions;` and the `pub use` to `lib.rs`.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine permissions` and expect PASS with 12 tests.

### Task 3: Apply it to the three builder specs

**Files:**
- Modify: `crates/factory-engine/src/specs.rs`
- Modify: `crates/factory-engine/tests/build_checks.rs`
- Create: `crates/factory-engine/tests/permission_pins.rs`

**Interfaces:**
- Produces: `pub(crate) fn with_builder_permissions(&self, spec: AgentSpec) -> AgentSpec` on `Engine`. With `config.builder_permissions` false it returns the spec unchanged. It also returns it unchanged when the canonical job folder text contains `*`, `?`, `[` or `{`. Otherwise it sets `permission_mode = Some("acceptEdits")`, `allowed_tools = builder_allowed_tools(&self.config)`, `disallowed_tools = builder_denied_tools(&job_dir)` and appends the canonical job folder to `add_dirs`.
- Consumes: `builder_first_spec`, `builder_task_spec` and `builder_resume_spec` call it right before `self.with_limits(...)`.

- [ ] **Step 1: Read** the three builder spec functions in `crates/factory-engine/src/specs.rs`, `builder_specs_are_strict_and_name_their_tools` and `assert_builder_spec_is_strict` in `crates/factory-engine/tests/build_checks.rs`, `crates/factory-engine/tests/common/mod.rs` (`committing_fixture`, `Fixture::engine`, `specs_for`, `last_spec_for`), and these tests that already produce the other roles' specs: `a_reply_with_pages_is_refreshed` in `tests/kb.rs`, `merge_uses_the_committer_message` in `tests/message.rs`, `reviewer_prompt_names_the_spec_and_the_earlier_findings_when_they_exist` in `tests/review_decide.rs`.
- [ ] **Step 2: Write the golden test first, before any production change.** In `permission_pins.rs`, `reviewer_committer_message_and_kb_arguments_are_unchanged` builds one spec per role the way those tests do (reviewer through `fx.engine(&rr).reviewer_spec(...)`, committer from `runner.specs_for("committer")` after `build_stage` in `committing_fixture`, message through `factory_engine::message_spec("001", &Config::default(), Path::new("/w"), "p")`, KB through `factory_engine::kb_spec("001", &Config::default(), Path::new("/repo"), Path::new("/kb"), "p")`), joins `role: <args joined by one space>` lines with `\n` using `spec.args(&Caps::default())`, and compares with `const GOLDEN: &str = "";`.
- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine --test permission_pins` and expect FAIL with the actual text in the assertion. Paste that text into `GOLDEN`, run again, and expect PASS on unchanged code.
- [ ] **Step 4: Write the failing tests** in `build_checks.rs`.
  - `builder_specs_carry_the_permission_flags`: for the first and resume specs (as `builder_specs_are_strict_and_name_their_tools` produces them) and the task spec (`two_task_fixture`), assert `permission_mode == Some("acceptEdits")`, `allowed_tools == factory_engine::builder_allowed_tools(&fx.config)`, `disallowed_tools == factory_engine::builder_denied_tools(&fx.job_dir)`, and `add_dirs` contains `std::fs::canonicalize(&fx.job_dir)`; also call `assert_builder_spec_is_strict` on each (test `builder_specs_still_pass_assert_builder_spec_is_strict` is this same assertion, kept as its own test over the three specs).
  - `builder_permissions_false_restores_the_old_argument_list`: `let mut fx = committing_fixture(); fx.config.builder_permissions = false;`, run `build_stage`, and assert on the first builder spec `permission_mode == None`, empty `allowed_tools` and `disallowed_tools`, and `spec.args(&Caps::default())` equals the pinned vector `["-p", "--output-format", "stream-json", "--verbose", "--strict-mcp-config", "--model", "sonnet", "--tools", "Bash,Read,Edit,Write,Grep,Glob,Skill,Agent", "--agent", "factory-builder", "--max-turns", "120"]`.
  - `glob_character_in_job_dir_leaves_the_spec_unchanged`: build the fixture with `job_dir` renamed to a folder containing `[` (create `tmp/job[1]` and point the fixture's `job_dir` at it, as the fixture fields are public), run `build_stage` and assert `permission_mode == None` and empty `allowed_tools`.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine --test build_checks builder_` and expect FAIL (`permission_mode` is `None` where `Some("acceptEdits")` is expected).
- [ ] **Step 6: Implement** `with_builder_permissions` and the three calls.
- [ ] **Step 7: Run** `cargo test -j4 -p factory-engine`, then `cargo test -j4 -p factory-agent`, then `cargo clippy -j4 -p factory-agent -p factory-engine --all-targets -- -D warnings` and `cargo fmt -p factory-agent -p factory-engine -- --check`. Expect PASS and no warnings.

Next plan: docs/superpowers/plans/2026-10-19-builder-permissions-2/plan-02-builder-permissions-2.md
