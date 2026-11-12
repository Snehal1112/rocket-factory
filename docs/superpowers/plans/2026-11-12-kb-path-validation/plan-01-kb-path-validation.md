# KB path validation Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-12-kb-path-validation-design.md
> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Refuse a bad `kb_path` with a clear message, scope the KB run's Edit and Write grants to the real folder, and stop `kb-refresh` taking the project lease for an unknown job.

**Architecture:** A pure resolver with a typed error lives in `factory-core` next to `kb_dir`. `Config::parse` runs its text rules. The KB step in `factory-engine` uses the checked form, canonicalises the folder and re-checks it. One CLI function reorders two calls. No new trait: the KB run goes through the existing `AgentRunner` port.

**Tech Stack:** Rust, `thiserror`, `std::fs::canonicalize`.

## Global Constraints

- Commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`. Never `--workspace` or `--all`.
- A bad `kb_path` gives the fail-closed config. No `Cargo.toml` change. No `git` shell-outs.
- No board, ledger or events field is added. Tests use `.expect("...")` and `expect_err`, never the unwrap call. Comments are short full sentences.
- `crates/factory-cli/tests/no_hardcoded_paths.rs` must keep passing.

## Review Focus

1. `kb/x`, `./x` and `~user/x` are refused as not absolute. Pinned by `relative_kb_path_is_rejected_at_load`.
2. A missing `HOME` never fails the config load. Pinned by `tilde_kb_path_loads_without_home`.
3. A `Config` built without `parse` and holding a bad path never yields a KB folder. Pinned by `kb_dir_hides_an_error_and_kb_dir_checked_shows_it`.
4. A symlink whose target holds a character outside the allowlist fails the run before any agent starts. Pinned by `a_symlink_to_a_path_with_bad_characters_fails_the_run`.
5. The KB prompt keeps the configured path while the grants use the real folder. Pinned by `kb_allow_rule_uses_the_canonical_folder`.
6. A refresh with a bad path reports the cause in the ledger, not "kb_path is not set". Pinned by `kb_refresh_with_a_bad_kb_path_reports_the_cause`.
7. An unknown job id is refused without the project lease. Pinned by `kb_refresh_for_an_unknown_job_is_not_blocked_by_the_project_lease`.

## File Structure

| File | Responsibility |
|---|---|
| `crates/factory-core/src/config.rs` | `KbPathError`, `check_kb_path`, `resolve_kb_path`, `kb_dir_checked`, the load check |
| `crates/factory-core/src/lib.rs` | Export the new items |
| `crates/factory-engine/src/kb.rs` | Typed errors, canonical folder, character re-check |
| `crates/factory-engine/src/compat.rs` | "not set" only for `None` |
| `crates/factory-cli/src/commands.rs` | Job and stage check before `hold_project` |
| `crates/factory-engine/tests/kb.rs`, `kb_home_unset.rs`, `crates/factory-cli/tests/project_lease_cli.rs` | New tests |
| `docs/reference/project-toml-reference.md`, `docs/reference/software-factory.md` | State the rules |

### Task 1: The resolver in factory-core

**Files:**
- Modify: `crates/factory-core/src/config.rs` (`kb_dir` at ~548, the load check at ~419, `expand_home` at ~557, tests near ~1437)
- Modify: `crates/factory-core/src/lib.rs` (the `pub use config::{..}` list at ~54)

**Interfaces:**
- Consumes: `expand_home(path: &str, home: Option<&str>) -> Option<PathBuf>`, `CoreError::Config(String)`.
- Produces: `pub enum KbPathError { HomeUnset { path: String }, NotAbsolute { path: String }, ParentDir { path: String }, InvalidChar { path: String, ch: char } }` deriving `Debug, Clone, PartialEq, Eq, thiserror::Error` with the messages `kb_path {path} starts with ~ but HOME is unset or empty`, `kb_path {path} must be absolute or start with ~/`, `kb_path {path} must not contain a .. component` and `kb_path {path} may only contain ASCII letters, digits and / . _ - ~, not {ch:?}`; `pub fn check_kb_path(raw: &str) -> Result<(), KbPathError>`; `pub fn resolve_kb_path(raw: &str, home: Option<&str>) -> Result<PathBuf, KbPathError>`; `Config::kb_dir_checked(&self) -> Result<Option<PathBuf>, KbPathError>` (`Ok(None)` when unset, `HOME` read from the environment); `Config::kb_dir(&self) -> Option<PathBuf>` kept as `self.kb_dir_checked().ok().flatten()`. All four items are exported from the crate root.

- [ ] **Step 1: Read the code this task touches.** Read `kb_dir`, `api_key_path`, `expand_home`, the load checks around `kb_path` in `Config::parse`, and the tests `kb_dir_is_none_when_unset`, `an_absolute_kb_path_is_kept` and `expand_home_handles_tilde_forms`, plus `crates/factory-core/tests/rocket_parity.rs`.
- [ ] **Step 2: Write the failing tests** in the `config.rs` tests module:

```rust
#[test]
fn relative_kb_path_is_rejected_at_load() {
    for raw in ["kb/x", "./x", "~user/x"] {
        let err = Config::parse(&format!("kb_path = \"{raw}\"\n")).expect_err("relative");
        assert!(err.to_string().contains("must be absolute"), "{raw}: {err}");
    }
}

#[test]
fn kb_path_with_other_characters_is_rejected_at_load() {
    for ch in ["*", "?", "[", "]", "{", "}", "!", ",", "(", ")", "\\\\", " ", "\\t", "é"] {
        let err = Config::parse(&format!("kb_path = \"/kb/a{ch}b\"\n")).expect_err("bad char");
        assert!(err.to_string().contains("may only contain"), "{ch}: {err}");
    }
}

#[test]
fn kb_path_with_tilde_and_no_home_reports_home_unset() {
    for home in [None, Some("")] {
        let err = resolve_kb_path("~/kb", home).expect_err("no home");
        assert!(matches!(err, KbPathError::HomeUnset { .. }), "{err:?}");
    }
}
```

  Add these by name and inputs: `check_kb_path_accepts_the_documented_forms` (`/kb/rocketapi`, `~/kb`, `~` all `Ok`), `kb_path_with_tilde_expands_with_home` (`resolve_kb_path("~/kb", Some("/home/u"))` is `/home/u/kb`), `kb_path_with_parent_dir_is_rejected_at_load` (`/a/../b` and `~/../x` fail with `..`), `absolute_kb_path_still_loads`, `tilde_kb_path_loads_without_home` (`Config::parse("kb_path = \"~/kb\"")` is `Ok`), and `kb_dir_hides_an_error_and_kb_dir_checked_shows_it` (a `Config { kb_path: Some("kb/x".into()), ..Config::default() }` gives `kb_dir() == None` and a `NotAbsolute` error from `kb_dir_checked()`; an unset path gives `Ok(None)`).
- [ ] **Step 3: Run them and see them fail.** Run `cargo test -j4 -p factory-core kb_path`. Expected: `cannot find function 'resolve_kb_path' in this scope`.
- [ ] **Step 4: Implement the resolver.** The text rules run in this order: the value is `~`, starts with `~/`, or starts with `/`, else `NotAbsolute`; any `..` component when split on `/` gives `ParentDir`; the first character that is not an ASCII letter, a digit or one of `/ . _ - ~` gives `InvalidChar`. Add `kb_path_injection_is_rejected_at_load`: `Config::parse("kb_path = \"/kb),Bash(curl:x\"")` fails with `may only contain`. `resolve_kb_path` runs `check_kb_path`, then `expand_home`, and maps `None` to `HomeUnset`. `Config::parse` calls `check_kb_path` after the blank check and maps the error to `CoreError::Config(e.to_string())`. `/` and `~` alone stay valid.
- [ ] **Step 5: Run the tests and see them pass.** Run `cargo test -j4 -p factory-core`. Expected: all pass, including `rocket_parity` and the unchanged `an_absolute_kb_path_is_kept`.

### Task 2: The KB run uses the checked, canonical folder

**Files:**
- Modify: `crates/factory-engine/src/kb.rs` (`kb_refresh_step` at ~159)
- Modify: `crates/factory-engine/src/compat.rs` (`Driver::kb_refresh` at ~600)
- Test: `crates/factory-engine/tests/kb.rs`, new `crates/factory-engine/tests/kb_home_unset.rs`

**Interfaces:**
- Consumes: `Config::kb_dir_checked` and `check_kb_path` from the previous task; `KbOutcome::Failed(String)`; the `Fixture` helpers `scripted`, `reply` and `specs_for` in `tests/common/mod.rs`; `with_kb()` and the driver-level `fixture` helper in `tests/kb.rs`.
- Produces: `kb_refresh_step` matches `kb_dir_checked()` (`Ok(None)` gives `Failed("kb_path is not set")`, `Err(e)` gives `Failed(e.to_string())`), then after `is_dir()` calls `canonicalize()`, applies `check_kb_path` to the real path, and on a failure returns `Failed("the KB folder <configured> resolves to <real>: <reason>")` before any run. `kb_spec` receives the real folder; `kb_prompt` receives the configured one. `Driver::kb_refresh` returns "kb_path is not set" only for `Ok(None)` and continues for `Err(_)`.

- [ ] **Step 1: Read the code this task touches.** Read `kb_refresh_step`, `kb_spec` and `kb_allowed_tools` in `kb.rs`; `Driver::kb_refresh` in `compat.rs`; and in `tests/kb.rs` the helpers `with_kb` and `fixture` and the tests `a_reply_with_pages_is_refreshed` and `an_unset_kb_path_writes_nothing`.
- [ ] **Step 2: Write the failing tests.** In `tests/kb.rs` (Unix only):

```rust
#[cfg(unix)]
#[test]
fn kb_allow_rule_uses_the_canonical_folder() {
    let mut fx = with_kb();
    let real = fx.tmp.path().join("kb").join("rocketapi").canonicalize().expect("real");
    let link = fx.tmp.path().join("kb-link");
    std::os::unix::fs::symlink(&real, &link).expect("symlink");
    fx.config.kb_path = Some(link.to_string_lossy().into_owned());
    let runner = scripted(vec![reply("auth.md\n")]);
    fx.engine(&runner).kb_refresh_step(Some(("001", "demo"))).expect("step");
    let spec = &runner.specs_for("kb")[0];
    let rule = format!("Edit(//{}/**)", real.display().to_string().trim_start_matches('/'));
    assert!(spec.allowed_tools.contains(&rule), "{:?}", spec.allowed_tools);
    assert_eq!(spec.add_dirs, vec![real]);
    assert!(spec.prompt.contains(&link.to_string_lossy().into_owned()));
}
```

  Add `a_symlink_to_a_path_with_bad_characters_fails_the_run` (Unix only: create a real folder named `kb[1]` in the temp dir, link to it, set `kb_path` to the link; assert `KbOutcome::Failed(text)` with `text` containing `resolves to` and `may only contain`, and `runner.specs_for("kb")` empty). Add `kb_refresh_with_a_bad_kb_path_reports_the_cause` in the driver-level module of `tests/kb.rs`: extend its `fixture` helper to take the `kb_path` text, use the relative `kb/x`, call `fx.driver.kb_refresh(&fx.id, true)`, and assert the line contains `must be absolute` and not `kb_path is not set`. Create `tests/kb_home_unset.rs`:

```rust
mod common;
use common::{scripted, Fixture};
use factory_engine::KbOutcome;

#[test]
fn an_unset_home_with_tilde_kb_path_does_not_say_not_set() {
    std::env::remove_var("HOME");
    let mut fx = Fixture::new();
    fx.config.kb_path = Some("~/kb/rocketapi".into());
    let runner = scripted(vec![]);
    let out = fx.engine(&runner).kb_refresh_step(None).expect("step");
    match out {
        KbOutcome::Failed(text) => {
            assert!(text.contains("HOME"), "{text}");
            assert!(!text.contains("not set"), "{text}");
        }
        other => panic!("unexpected outcome {other:?}"),
    }
    assert!(runner.specs_for("kb").is_empty());
}
```

  The file holds only this test, so no other test reads `HOME` while it is cleared.
- [ ] **Step 3: Run them and see them fail.** Run `cargo test -j4 -p factory-engine --test kb --test kb_home_unset`. Expected: the HOME test reports `kb_path is not set`, and the canonical test sees the link path in the rule.
- [ ] **Step 4: Implement the typed errors and the canonical folder** in `kb_refresh_step`, and the `Err(_)` fall-through in `Driver::kb_refresh`. Keep the existing `does not exist` message for a missing folder.
- [ ] **Step 5: Run the tests and see them pass.** Run `cargo test -j4 -p factory-engine` and `cargo clippy -j4 -p factory-core -p factory-engine`. Expected: all pass, including the unchanged `an_unset_kb_path_writes_nothing`.

### Task 3: The lease order and the docs

**Files:**
- Modify: `crates/factory-cli/src/commands.rs` (`kb_refresh` at ~1130)
- Modify: `docs/reference/project-toml-reference.md` (the `kb_path` row at ~166), `docs/reference/software-factory.md` (~94)
- Test: `crates/factory-cli/tests/project_lease_cli.rs`

**Interfaces:**
- Consumes: `Driver::job(id)`, `Driver::hold_project(id)`, `Stage::Merged`, `CliError::Refused`; the helpers `hold_project(fx, owner, now)` and `Fixture::run` in `project_lease_cli.rs`.
- Produces: in `kb_refresh`, the `d.job(id)?` call and the merged-stage check come before `d.hold_project(id)?`; the foreign job lease check stays after it. No signature changes.

- [ ] **Step 1: Read the code this task touches.** Read `kb_refresh` in `commands.rs`, the tests `run_is_refused_while_another_driver_holds_the_project` and its neighbours in `project_lease_cli.rs`, and the `kb_path` text in both reference docs.
- [ ] **Step 2: Write the failing test** in `project_lease_cli.rs`:

```rust
#[test]
fn kb_refresh_for_an_unknown_job_is_not_blocked_by_the_project_lease() {
    let fx = Fixture::new();
    new_job(&fx);
    hold_project(&fx, "someone-else", now_secs());
    let e = fx.run(&["kb-refresh", "999"]).expect_err("unknown job");
    assert!(!e.to_string().contains("project is busy"), "{e}");
}
```

- [ ] **Step 3: Run it and see it fail.** Run `cargo test -j4 -p factory-cli --test project_lease_cli kb_refresh_for_an_unknown_job`. Expected: the error says the project is busy.
- [ ] **Step 4: Implement the reorder and update the docs.** Move the two checks above `hold_project`. In both docs state that `kb_path` must be absolute or start with `~/`, that relative paths, `..` and any character other than ASCII letters, digits and `/ . _ - ~` (so no spaces, commas, parentheses, backslashes or glob characters) are rejected when the config loads, that a `~/` path needs `HOME` at the time of the refresh, and that a symlinked folder is resolved to its real path.
- [ ] **Step 5: Run the tests and see them pass.** Run `cargo test -j4 -p factory-cli --test project_lease_cli --test no_hardcoded_paths --test check_coverage_docs`, `cargo test -j4 -p factory-cli --test agent_lint` and `cargo clippy -j4 -p factory-cli`. Expected: all pass.

Next plan: none
