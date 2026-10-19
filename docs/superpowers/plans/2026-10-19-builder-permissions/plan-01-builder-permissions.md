# Builder Permissions, Plan 1: The two settings

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Add `builder_permissions` and `builder_allow` to `Config` in `factory-core`, with the `builder_allow` validator, so the later series can read them.

**Architecture:** Two new `Config` fields with serde defaults. `Config::parse` calls one new private validator per `builder_allow` entry. Two public constants hold the program lists, so the engine (series 2) and the validator share one source. Nothing outside `factory-core` changes in this plan.

**Tech Stack:** Rust 2021, `serde` and `toml` (already used in `crates/factory-core/src/config.rs`).

**Spec:** docs/superpowers/specs/2026-10-19-builder-permissions-design.md

## Global Constraints

- Always `-j4`; never `--workspace` or `--all`; never set `CARGO_TARGET_DIR`.
- `builder_permissions` is a bool with default `true`. `builder_allow` is a `Vec<String>` with default empty. A config file without either key parses as before.
- A bad `builder_allow` entry makes `Config::parse` return `CoreError::Config` with a message that starts `builder_allow: `, so `Config::load` falls back to the fail-closed config.
- `Config::fail_closed()` keeps `builder_permissions = true` and `builder_allow` empty (it already spreads `Config::default()`).
- The never-allowed programs are exactly `git`, `rm`, `rmdir`, `curl`, `sudo`, `sh`, `bash`, `env`, `xargs`. The read-only git words are exactly `status`, `diff`, `log`, `show`, `rev-parse`.
- No board, ledger or events format changes; this plan touches no JSON.

## Review Focus

- An entry with two spaces, `Bash(git  commit:*)`, is still rejected (Task 2 test `builder_allow_rejects_forbidden_and_malformed_entries`).
- `git -C x commit` is rejected because the `-C x` pair is dropped before the subcommand is read (same test).
- A path to a forbidden program, `Bash(/bin/rm:*)`, is rejected by its basename (same test).
- `Bash(env sh:*)` and `Bash(xargs rm:*)` are rejected on the first program (same test).
- A bare `Bash(git:*)` is rejected, and `Bash(git log:*)` is accepted (same test and `builder_allow_accepts_a_make_rule`).
- A program whose name only contains a forbidden word, such as `shellcheck`, is accepted (`builder_allow_accepts_a_make_rule`).
- An entry of 201 characters, an empty entry and a two-line entry are rejected (same rejection test).
- A broken `builder_allow` makes `Config::load` return the fail-closed config with the defaults (Task 2 test `a_bad_builder_allow_loads_as_fail_closed`).

---

## File Structure

- `crates/factory-core/src/config.rs` (modify): the two fields and their defaults, the two constants, `check_builder_allow`, the call in `Config::parse`, and the tests.
- `crates/factory-core/src/lib.rs` (modify): re-export the two constants next to `Config`.

### Task 1: The fields and their defaults

**Files:**
- Modify: `crates/factory-core/src/config.rs`

**Interfaces:**
- Produces: `Config.builder_permissions: bool` (default `true`); `Config.builder_allow: Vec<String>` (default empty). Both load from the file with `#[serde(default)]` through the struct-level attribute that `Config` already has.

- [ ] **Step 1: Read** `Config`, its `Default` impl, `fail_closed` and the `tests` module in `crates/factory-core/src/config.rs`, and `crates/factory-core/tests/rocket_parity.rs` (it compares named fields only, so it needs no change).
- [ ] **Step 2: Write the failing tests** in the existing `tests` module, using `Config::default()`, `Config::parse` and `Config::fail_closed()` as the neighbouring tests do.
  - `builder_permissions_defaults_to_true_and_allow_to_empty`:
    ```rust
    #[test]
    fn builder_permissions_defaults_to_true_and_allow_to_empty() {
        assert!(Config::default().builder_permissions);
        assert!(Config::default().builder_allow.is_empty());
        let cfg = Config::parse("diff_cap_lines = 100\n").expect("parses");
        assert!(cfg.builder_permissions);
        assert!(cfg.builder_allow.is_empty());
        let off = Config::parse("builder_permissions = false\n").expect("parses");
        assert!(!off.builder_permissions);
    }
    ```
  - `fail_closed_config_keeps_the_defaults`: `Config::fail_closed()` has `builder_permissions == true` and an empty `builder_allow`.
- [ ] **Step 3: Run** `cargo test -j4 -p factory-core builder_permissions` and expect a compile failure: `no field 'builder_permissions' on type 'Config'`.
- [ ] **Step 4: Implement** the two fields at the end of `Config`, before `degraded`, each with a one-line doc comment. `builder_permissions: bool` defaults to `true` in `Default for Config`; `builder_allow: Vec<String>` defaults to `Vec::new()`.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-core builder_` and expect PASS (2 tests), then `cargo test -j4 -p factory-core --test rocket_parity` and expect PASS.

### Task 2: The `builder_allow` validator

**Files:**
- Modify: `crates/factory-core/src/config.rs`, `crates/factory-core/src/lib.rs`

**Interfaces:**
- Produces: `pub const NEVER_ALLOWED_PROGRAMS: [&str; 9]` (`git`, `rm`, `rmdir`, `curl`, `sudo`, `sh`, `bash`, `env`, `xargs`, in that order); `pub const READ_ONLY_GIT: [&str; 5]` (`status`, `diff`, `log`, `show`, `rev-parse`); both re-exported from `factory_core`.
- Produces (private): `fn check_builder_allow(entry: &str) -> Result<(), String>`.
  - Rejects an entry that is blank (`must not be empty`), holds `\n` or `\r` (`must be one line`), or has more than 200 characters (`must be at most 200 characters`).
  - Requires the form `Bash(<command>:*)` or `Bash(<command>)`, otherwise `must be Bash(<command>:*) or Bash(<command>)`.
  - Takes `<command>` without one trailing `:*`, splits it on whitespace, and drops every `-C` token together with the token after it. No tokens left is `needs a command`.
  - The program is the part of the first token after the last `/`. When it is in `NEVER_ALLOWED_PROGRAMS` and is not `git`, the error names it: `<program> is never allowed`.
  - When the program is `git`, the next token must be in `READ_ONLY_GIT`; otherwise `git is allowed only for status, diff, log, show and rev-parse`.
- Consumes: `Config::parse` calls it for each `builder_allow` entry after the `base_branch` check and returns `CoreError::Config(format!("builder_allow: {why}"))` for the first failure.

- [ ] **Step 1: Read** `check_branch_name` and the `base_branch` block in `Config::parse`; copy that error style.
- [ ] **Step 2: Write the failing tests** in the `tests` module.
  - Full example:
    ```rust
    #[test]
    fn builder_allow_rejects_forbidden_and_malformed_entries() {
        for entry in [
            "Bash(git:*)", "Bash(git  commit:*)", "Bash(git -C x commit:*)",
            "Bash(/bin/rm:*)", "Bash(env sh:*)", "Bash(xargs rm:*)",
            "Bash(rm:*)", "Bash(rmdir x)", "Bash(curl:*)", "Bash(sudo:*)",
            "Bash(sh:*)", "Bash(bash -c x)", "Bash()", "Bash(:*)", "", "  ",
            "make", "Bash(make:*", "Bash(make:*)\nBash(ls:*)",
        ] {
            let body = format!("builder_allow = [{entry:?}]\n");
            let err = Config::parse(&body).expect_err(entry);
            assert!(err.to_string().contains("builder_allow:"), "{entry}: {err}");
        }
        let long = format!("Bash({}:*)", "a".repeat(200));
        let body = format!("builder_allow = [{long:?}]\n");
        Config::parse(&body).expect_err("201 or more characters");
    }
    ```
  - `builder_allow_accepts_a_make_rule`: `Config::parse` with `builder_allow = ["Bash(make:*)", "Bash(shellcheck:*)", "Bash(git log:*)", "Bash(git -C x log)", "Bash(./scripts/test.sh:*)"]` is `Ok` and `cfg.builder_allow` equals those five strings in order.
  - `a_bad_builder_allow_loads_as_fail_closed`: write `builder_allow = ["Bash(rm:*)"]` to a file in a `tempfile::tempdir()` (as the `load_fails_closed_for_missing_and_invalid_files` test does), call `Config::load(&path)`, and assert `cfg.degraded`, `cfg.builder_allow.is_empty()` and `cfg.builder_permissions`.
  - `never_allowed_and_read_only_lists_are_exact`: `NEVER_ALLOWED_PROGRAMS` equals `["git", "rm", "rmdir", "curl", "sudo", "sh", "bash", "env", "xargs"]` and `READ_ONLY_GIT` equals `["status", "diff", "log", "show", "rev-parse"]`.
- [ ] **Step 3: Run** `cargo test -j4 -p factory-core builder_allow` and expect FAIL (the rejection test fails because `Config::parse` accepts the entries; the constants test fails to compile with `cannot find value 'NEVER_ALLOWED_PROGRAMS'`).
- [ ] **Step 4: Implement** the constants, `check_builder_allow` and the call in `Config::parse`, and add `NEVER_ALLOWED_PROGRAMS` and `READ_ONLY_GIT` to the `pub use config::{...}` line in `crates/factory-core/src/lib.rs`.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-core` and expect PASS with 4 new tests and no failures, then `cargo clippy -j4 -p factory-core --all-targets -- -D warnings` and `cargo fmt -p factory-core -- --check`, both clean.

Next plan: none
