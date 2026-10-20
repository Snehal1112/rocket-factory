# Upgrade and version check, Plan 1: Version marker and version display

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Record the newest factory version that touched a home in `<home>/VERSION`, stop a binary that is older than that marker, write the marker from `factory project add` and `factoryd run`, and show the version in `factory project show` and `factoryd status`.

**Architecture:** A new pure module `factory-core/src/version.rs` holds `FACTORY_VERSION`, `read_marker`, `check` and `stamp`, with a private `major.minor.patch` parser and a new `CoreError::Version(String)` variant whose text is the bare message. `Home::version_file()` names the file. `factory-cli` gets two helpers, `check_home` and `open_home`, used by `main.rs` wherever a command opens the home; `project add` stamps after it succeeds; `project show` prints a `version:` line. `factoryd` checks in `main` for every command but `is-running`, stamps at the start of `run`, and ends `status` with `factoryd <version>`. No factory-engine code changes, so no port is involved. No board, ledger or events field is added.

**Tech Stack:** Rust, clap, thiserror, tempfile (tests), `CARGO_BIN_EXE_factory` and `CARGO_BIN_EXE_factoryd` binary tests.

**Spec:** docs/superpowers/specs/2026-10-20-upgrade-and-version-check-design.md

## Global Constraints

- Always `-j4`; never `--workspace` or `--all`; never set `CARGO_TARGET_DIR`. Check per crate with `cargo check -j4 -p <crate>` and test per crate with `cargo test -j4 -p <crate> <filter>`.
- Paths reach the driver through `Home` and `Layout` only (`crates/factory-cli/tests/no_hardcoded_paths.rs`). The file name `VERSION` lives as a constant in `crates/factory-core/src/home.rs`, like `CONFIG_FILE`.
- `factory-core` stays free of I/O-heavy dependencies. This plan adds no dependency; the version comparison uses no new crate.
- `factory presets refresh` output, and the existing `install` messages, keep their text.
- `upgrade` never overwrites an edited preset file (plan 4; nothing here touches presets).
- Each task lists the tests it adds. No task touches factory-engine, so no port is named. No board, ledger or events field is added.
- Error texts are exact: `this home was last used by factory <marker>, this binary is <current>: install a newer binary, or delete <home>/VERSION to ignore this`, where `<home>/VERSION` is `home.version_file().display()`, and `<path> does not hold a version (got "<text>"): delete it and run factory upgrade`, where `<text>` is the trimmed marker text.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.

## Review Focus

1. An empty or whitespace-only `VERSION` file is not a version: `check` fails with `does not hold a version (got "")`, and `stamp` replaces it. Pinned by `check_rejects_an_empty_marker` (Task 1).
2. A marker with a trailing newline or spaces (`"0.1.2\n"`, `"  0.1.2  \n"`) reads as `0.1.2`. Pinned by `read_marker_trims_the_line` (Task 1).
3. Text after `-` or `+` is ignored: marker `0.2.0-rc.1` against current `0.2.0` is equal, so `check` passes and `stamp` returns `false` and leaves the file byte-identical. Pinned by `suffix_after_dash_or_plus_is_ignored` (Task 1).
4. The comparison is numeric, not textual: marker `0.10.0` is newer than current `0.9.0`, so `check` fails and `stamp` returns `false`. Pinned by `comparison_is_numeric_not_text` (Task 1).
5. A marker that is not UTF-8 is "not a version", not an I/O error; `read_marker` reads it lossily. Pinned by `check_rejects_a_marker_that_is_not_utf8` (Task 1).
6. `VERSION` is a directory: `read_marker`, `check` and `stamp` return an `Io` error; `project add` still succeeds and appends a `note: version marker not written: ...` line. Pinned by `add_succeeds_with_a_note_when_the_marker_cannot_be_written` (Task 2).
7. Commands that do not open the home ignore even a newer marker: `factory --root <repo> status`, `factory install --dir <d>` and `factory --version` succeed, and the marker stays byte-identical. Pinned by `commands_without_the_home_ignore_a_newer_marker` (Task 2).
8. `factory presets refresh` checks but never stamps: with marker `0.0.1` it succeeds and the marker stays `0.0.1`. Pinned by `presets_refresh_checks_but_does_not_stamp` (Task 2).
9. `factory --project <name>` with an older marker passes the check and fails later on the unknown project with exit 2 and no version text. With a newer marker it exits 1 with the version text. Pinned by `project_flag_passes_an_older_marker_through` and `project_flag_stops_on_a_newer_marker` (Task 2).
10. `factoryd is-running` never reads the marker: with marker `99.0.0` and no running factoryd it exits 1 with empty stderr. Pinned by `is_running_ignores_a_newer_marker` (Task 3).
11. `factoryd run --once` with a newer marker exits 1 before stamping and leaves the marker byte-identical. Pinned by `run_once_refuses_a_newer_marker_and_leaves_it` (Task 3).

---

## File Structure

- `crates/factory-core/src/version.rs` (create): `FACTORY_VERSION`, `read_marker`, `check`, `stamp`, the private parser, unit tests.
- `crates/factory-core/src/home.rs` (modify): `VERSION_FILE` constant and `Home::version_file()`, plus its test line.
- `crates/factory-core/src/error.rs` (modify): `CoreError::Version(String)` with `#[error("{0}")]`.
- `crates/factory-core/src/lib.rs` (modify): `pub mod version;`.
- `crates/factory-cli/src/commands.rs` (modify): `check_home` and `open_home`.
- `crates/factory-cli/src/lib.rs` (modify): re-export `check_home` and `open_home`.
- `crates/factory-cli/src/main.rs` (modify): call the check for `project`, `presets` and `--project`.
- `crates/factory-cli/src/project.rs` (modify): stamp after `add`, `version:` line in `show`.
- `crates/factory-cli/tests/version_marker.rs` (create): binary tests of the gate.
- `crates/factory-cli/tests/project_cmd.rs` (modify): add-stamp and show-line tests.
- `crates/factory-supervisor/src/bin/factoryd.rs` (modify): check in `main`, stamp in `run`, last line of `status`, unit test.
- `crates/factory-supervisor/tests/version_marker.rs` (create): binary tests of the factoryd gate and stamp.

### Task 1: The version module and `Home::version_file`

**Files:**
- Create: `crates/factory-core/src/version.rs`
- Modify: `crates/factory-core/src/home.rs`, `crates/factory-core/src/error.rs`, `crates/factory-core/src/lib.rs`

**Interfaces:**
- Consumes: `factory_core::Home` (`Home::at`, `Home::dir`), `factory_core::write_atomic(path: &Path, bytes: &[u8]) -> Result<(), CoreError>`, `CoreError::io(path: &Path, source: std::io::Error) -> CoreError`.
- Produces:
  - `Home::version_file(&self) -> PathBuf` (returns `self.dir.join("VERSION")` through a `VERSION_FILE` constant).
  - `CoreError::Version(String)` with `#[error("{0}")]`.
  - `pub const FACTORY_VERSION: &str = env!("CARGO_PKG_VERSION");`
  - `pub fn read_marker(home: &Home) -> Result<Option<String>, CoreError>`: `Ok(None)` when the file is missing; otherwise the bytes read lossily as UTF-8 and trimmed; any other read error is `CoreError::io`.
  - `pub fn check(home: &Home, current: &str) -> Result<(), CoreError>`: `Ok` when missing, equal or older; `CoreError::Version` with the first spec text when newer, with the second spec text when the marker is not a version.
  - `pub fn stamp(home: &Home, current: &str) -> Result<bool, CoreError>`: writes `"{current}\n"` with `write_atomic` when the marker is missing, not a version or older, and returns `true`; returns `false` without writing when equal or newer.
  - private `fn parse(text: &str) -> Option<(u64, u64, u64)>`: cut at the first `-` or `+`, split on `.`, exactly three parts, each parsed as `u64`; anything else is `None`. Both `check` and `stamp` return `CoreError::Version(format!("this binary's version {current:?} is not a version"))` when `current` does not parse.

- [ ] **Step 1: Read** spec section 1 (Version marker), `crates/factory-core/src/home.rs` (constants and `supervisor_names_are_under_the_home`), `crates/factory-core/src/error.rs`, `crates/factory-core/src/fsutil.rs` (`write_atomic` creates the parent folder) and `crates/factory-core/src/lib.rs`.

- [ ] **Step 2: Write the failing tests.** In `home.rs`, add to `supervisor_names_are_under_the_home`:

```rust
assert_eq!(h.version_file(), PathBuf::from("/h/VERSION"));
```

In `version.rs`, a `#[cfg(test)] mod tests` with this helper and tests:

```rust
use super::*;

fn home_with(marker: Option<&[u8]>) -> (tempfile::TempDir, Home) {
    let t = tempfile::tempdir().expect("tempdir");
    let home = Home::at(t.path().join("home"));
    if let Some(bytes) = marker {
        std::fs::create_dir_all(&home.dir).expect("mkdir");
        std::fs::write(home.version_file(), bytes).expect("write marker");
    }
    (t, home)
}

#[test]
fn check_stops_a_binary_older_than_the_marker() {
    let (_t, home) = home_with(Some(b"0.2.0\n"));
    let err = check(&home, "0.1.2").expect_err("newer marker");
    assert!(matches!(err, CoreError::Version(_)), "{err}");
    assert_eq!(
        err.to_string(),
        format!(
            "this home was last used by factory 0.2.0, this binary is 0.1.2: install a newer binary, or delete {} to ignore this",
            home.version_file().display()
        )
    );
}
```

Other `check` cases, same shape:

| Test | Marker bytes | current | Expected |
|---|---|---|---|
| `check_accepts_a_missing_marker` | none (home folder absent) | `0.1.2` | `Ok(())` and `home.dir` still does not exist |
| `check_accepts_an_equal_or_older_marker` | `0.1.2\n`, then `0.1.1\n`, then `0.0.9` | `0.1.2` | `Ok(())` each |
| `check_rejects_a_marker_that_is_not_a_version` | `garbage\n` | `0.1.2` | `expect_err`; text equals `format!("{} does not hold a version (got \"garbage\"): delete it and run factory upgrade", home.version_file().display())` |
| `check_rejects_short_and_long_versions` | `0.1`, then `1.2.3.4`, then `1.x.3` | `0.1.2` | `expect_err` each; text contains `does not hold a version` |
| `check_rejects_an_empty_marker` | `""`, then `"  \n"` | `0.1.2` | `expect_err`; text contains `(got "")` |
| `check_rejects_a_marker_that_is_not_utf8` | `[0xff, 0xfe, b'\n']` | `0.1.2` | `expect_err`; `matches!(err, CoreError::Version(_))` |
| `check_rejects_an_unparseable_current` | `0.1.2` | `dev` | `expect_err`; text contains `"dev"` and `is not a version` |
| `comparison_is_numeric_not_text` | `0.10.0` | `0.9.0` | `check` is `expect_err` with `last used by factory 0.10.0`; `stamp` returns `false`; file still `0.10.0` |
| `suffix_after_dash_or_plus_is_ignored` | `0.2.0-rc.1`, then `0.2.0+build.5` | `0.2.0` | `check` is `Ok`; `stamp` returns `false`; file bytes unchanged |

`read_marker` tests:

| Test | Marker bytes | Expected |
|---|---|---|
| `read_marker_is_none_for_a_missing_file` | none | `Ok(None)` |
| `read_marker_trims_the_line` | `"  0.1.2  \n"` | `Ok(Some("0.1.2".to_string()))` |
| `read_marker_reports_a_directory_as_io` | create `home.version_file()` as a directory | `expect_err`; `matches!(err, CoreError::Io { .. })` |

`stamp` tests:

```rust
#[test]
fn stamp_writes_a_missing_marker_and_creates_the_home() {
    let (_t, home) = home_with(None);
    assert!(stamp(&home, "0.1.2").expect("stamp"));
    assert_eq!(
        std::fs::read_to_string(home.version_file()).expect("read"),
        "0.1.2\n"
    );
    assert!(!stamp(&home, "0.1.2").expect("second stamp"), "equal is not rewritten");
}
```

| Test | Marker bytes | current | Returns | File afterwards |
|---|---|---|---|---|
| `stamp_replaces_an_older_marker` | `0.1.1\n` | `0.1.2` | `true` | `0.1.2\n` |
| `stamp_replaces_an_unparseable_marker` | `garbage` | `0.1.2` | `true` | `0.1.2\n` |
| `stamp_never_writes_an_older_version_over_a_newer_one` | `0.2.0\n` | `0.1.2` | `false` | `0.2.0\n` |
| `stamp_reports_a_directory_marker_as_io` | `version_file()` is a directory | `0.1.2` | `expect_err`, `CoreError::Io { .. }` | still a directory |
| `factory_version_is_the_workspace_version` | none | none | `assert_eq!(FACTORY_VERSION, env!("CARGO_PKG_VERSION"))` and `parse(FACTORY_VERSION).is_some()` | none |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-core version` and `cargo test -j4 -p factory-core supervisor_names_are_under_the_home`. Expect compile errors: `no method named version_file found for struct Home` and `cannot find function check` (the module does not exist yet; add `pub mod version;` to `lib.rs` with an empty file first so the error is about the missing items).

- [ ] **Step 4: Implement** `VERSION_FILE` and `Home::version_file`, the `CoreError::Version(String)` variant with `#[error("{0}")]`, and in `version.rs` the items listed under Produces with a `//!` module comment. `read_marker` maps `ErrorKind::NotFound` to `Ok(None)`, reads with `std::fs::read`, and uses `String::from_utf8_lossy(..).trim()`. `check` and `stamp` share `parse`; `stamp` writes through `write_atomic(&home.version_file(), format!("{current}\n").as_bytes())`. Comments are short full sentences ending with a period.

- [ ] **Step 5: Run** `cargo test -j4 -p factory-core version`, `cargo test -j4 -p factory-core home` and `cargo check -j4 -p factory-core`. Expect PASS and no warnings.

### Task 2: The version gate and the stamp in `factory`

**Files:**
- Modify: `crates/factory-cli/src/commands.rs`, `crates/factory-cli/src/lib.rs`, `crates/factory-cli/src/main.rs`, `crates/factory-cli/src/project.rs`, `crates/factory-cli/tests/project_cmd.rs`
- Create: `crates/factory-cli/tests/version_marker.rs`

**Interfaces:**
- Consumes: `factory_core::version::{FACTORY_VERSION, check, stamp, read_marker}`, `Home::version_file()`, `CoreError::Version` (Task 1); `CliError::Core(#[from] factory_core::CoreError)`; `Home::from_env()`.
- Produces:
  - `pub fn check_home(home: &Home) -> Result<(), CliError>` in `commands.rs`: `version::check(home, FACTORY_VERSION)` mapped into `CliError::Core`.
  - `pub fn open_home() -> Result<Home, CliError>` in `commands.rs`: `Home::from_env()` then `check_home`. Plan 4 uses it for `factory upgrade`.
  - Both re-exported from `crates/factory-cli/src/lib.rs` next to `dispatch`.
  - `main.rs`: `Command::Project` and `Command::Presets` use `open_home()` (exit 1 on any error, as today). The `--project` branch keeps `Home::from_env()` with exit 2 on error, then calls `check_home` and exits 1 with `factory: <text>` on error, then builds `CliCtx::for_project` as today (exit 2). `Command::Install`, `--version` and the `CliCtx::from_env` branch never call the check.
  - `project::add` (private `fn add` in `crates/factory-cli/src/project.rs`): after `project::add` returns `Ok`, calls `version::stamp(home, FACTORY_VERSION)`; on `Err(e)` it appends the line `note: version marker not written: {e}` to the success text and still returns `Ok`.

- [ ] **Step 1: Read** spec section 1 (where `check` and `stamp` run), `crates/factory-cli/src/main.rs`, `crates/factory-cli/src/commands.rs` (`CliError`, `CliCtx::for_project`, `CliCtx::from_env`), `crates/factory-cli/src/lib.rs`, `fn add` in `crates/factory-cli/src/project.rs`, the helpers `env`, `make_repo`, `run`, `s` and `add_demo` in `crates/factory-cli/tests/project_cmd.rs`, and the binary-test pattern of `bad_factory_log_is_a_note_not_a_failure` in `crates/factory-cli/tests/log_cmd.rs`.

- [ ] **Step 2: Write the failing tests.**

In `crates/factory-cli/tests/project_cmd.rs` (reuse `env`, `add_demo`, `make_repo`, `run`, `s`):

```rust
#[test]
fn add_writes_the_version_marker() {
    let e = env();
    add_demo(&e);
    assert_eq!(
        factory_core::version::read_marker(&e.home).expect("read"),
        Some(factory_core::version::FACTORY_VERSION.to_string())
    );
}
```

| Test | Setup | Action | Expected |
|---|---|---|---|
| `a_refused_add_writes_no_marker` | `env()`, plain folder `e.tmp.path().join("plain")` | `run(&e, &["add", s(&plain), "--preset", "rust", "--name", "p"])` | `expect_err`; `e.home.version_file().exists()` is false |
| `add_keeps_a_newer_marker` | `std::fs::create_dir_all(&e.home.dir)`, write `99.0.0\n` to `e.home.version_file()` | `add_demo(&e)` | file still `99.0.0\n` |
| `add_succeeds_with_a_note_when_the_marker_cannot_be_written` | `std::fs::create_dir_all(e.home.version_file())` | `run(&e, &["add", s(&repo), "--preset", "rust", "--name", "demo", "--base", "main"])` with `repo = make_repo(e.tmp.path(), "r1", "main")` | `.expect("add")`; output contains `added project demo` and `note: version marker not written:`; `run(&e, &["list"])` starts with `demo rust main` |

In the new `crates/factory-cli/tests/version_marker.rs`:

```rust
//! The version gate of the `factory` binary.

use std::path::Path;
use std::process::Output;

use factory_core::version::FACTORY_VERSION;
use factory_core::Home;

fn factory(home: &Home, args: &[&str]) -> Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_factory"))
        .args(args)
        .env("FACTORY_HOME", &home.dir)
        .output()
        .expect("run binary")
}

fn home_with(t: &Path, marker: &str) -> Home {
    let home = Home::at(t.join("home"));
    std::fs::create_dir_all(&home.dir).expect("mkdir");
    std::fs::write(home.version_file(), marker).expect("marker");
    home
}

#[test]
fn project_list_stops_on_a_newer_marker() {
    let t = tempfile::tempdir().expect("tempdir");
    let home = home_with(t.path(), "99.0.0\n");
    let out = factory(&home, &["project", "list"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.starts_with(&format!(
            "factory: this home was last used by factory 99.0.0, this binary is {FACTORY_VERSION}: install a newer binary, or delete {} to ignore this",
            home.version_file().display()
        )),
        "{stderr}"
    );
}
```

| Test | Marker | Args | Expected |
|---|---|---|---|
| `presets_refresh_stops_on_a_newer_marker` | `99.0.0\n` | `["presets", "refresh"]` | exit code 1; stderr contains `last used by factory 99.0.0`; `home.presets_dir()` does not exist |
| `a_marker_that_is_not_a_version_stops_project_commands` | `garbage\n` | `["project", "list"]` | exit code 1; stderr contains `does not hold a version (got "garbage")` |
| `project_flag_stops_on_a_newer_marker` | `99.0.0\n` | `["--project", "demo", "status"]` | exit code 1; stderr contains `last used by factory 99.0.0` |
| `project_flag_passes_an_older_marker_through` | `0.0.1\n` | `["--project", "demo", "status"]` | exit code 2; stderr does not contain `last used by`; marker still `0.0.1\n` |
| `presets_refresh_checks_but_does_not_stamp` | `0.0.1\n` | `["presets", "refresh"]` | success; marker still `0.0.1\n` |
| `project_list_accepts_an_older_marker_and_does_not_stamp` | `0.0.1\n` | `["project", "list"]` | success; stdout is `no projects\n`; marker still `0.0.1\n` |
| `commands_without_the_home_ignore_a_newer_marker` | `99.0.0\n` | three runs: `["--version"]`; `["install", "--dir", <t>/cmds]`; `["--root", <repo>, "status"]` with `<repo>` a `git2::Repository::init` folder | each succeeds; marker still `99.0.0\n` |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-cli --test version_marker` and `cargo test -j4 -p factory-cli --test project_cmd marker`. Expect FAIL: `project_list_stops_on_a_newer_marker`, `presets_refresh_stops_on_a_newer_marker`, `a_marker_that_is_not_a_version_stops_project_commands` and `project_flag_stops_on_a_newer_marker` exit 0 or 2 instead of 1; `add_writes_the_version_marker` gets `None`; `add_succeeds_with_a_note_when_the_marker_cannot_be_written` lacks the note.

- [ ] **Step 4: Implement** `check_home` and `open_home` in `commands.rs` with short doc comments, the re-exports in `lib.rs`, the three call sites in `main.rs` as Produces says, and the stamp in `fn add` of `project.rs`. Keep the success text of `add` unchanged when the stamp works.

- [ ] **Step 5: Run** `cargo test -j4 -p factory-cli --test version_marker`, `cargo test -j4 -p factory-cli --test project_cmd`, `cargo test -j4 -p factory-cli --test global_command`, `cargo test -j4 -p factory-cli --test presets_refresh`, `cargo test -j4 -p factory-cli --test no_hardcoded_paths` and `cargo check -j4 -p factory-cli`. Expect PASS.

### Task 3: The `project show` line and the factoryd gate, stamp and status line

**Files:**
- Modify: `crates/factory-cli/src/project.rs`, `crates/factory-cli/tests/project_cmd.rs`, `crates/factory-supervisor/src/bin/factoryd.rs`
- Create: `crates/factory-supervisor/tests/version_marker.rs`

**Interfaces:**
- Consumes: `factory_core::version::{FACTORY_VERSION, check, stamp}`, `Home::version_file()` (Task 1).
- Produces:
  - `fn show` in `crates/factory-cli/src/project.rs`: the line `version: {FACTORY_VERSION}` right after `project: {name}`, in the disabled and the normal variant.
  - `factoryd.rs` `main`: after `Cmd::IsRunning` returns early and `Home::from_env()` succeeds, `version::check(&home, FACTORY_VERSION)` runs for `run`, `status`, `stop` and `enqueue`; an error prints `factoryd: <text>` and exits 1.
  - `factoryd.rs` `fn run(home: Home, interval_ms: u64, once: bool) -> Result<(), String>`: its first statement is `version::stamp(&home, FACTORY_VERSION)`; on `Err(e)` it prints `note: version marker not written: {e}` to stderr and continues.
  - `factoryd.rs` `fn status(home: &Home) -> String`: keeps its signature; its last line is `factoryd {FACTORY_VERSION}` followed by `\n`. Plan 2 inserts its `warning:` line right before it.

- [ ] **Step 1: Read** spec sections 1 and 2, `fn show` in `crates/factory-cli/src/project.rs`, the tests `show_prints_the_resolved_project` and `show_of_a_project_with_a_missing_repo_is_disabled_like_list` in `crates/factory-cli/tests/project_cmd.rs`, all of `crates/factory-supervisor/src/bin/factoryd.rs` with its tests `status_shows_the_daily_budget_only_with_a_limit` and `version_flag_prints_the_workspace_version`, and `HomeConfig::load` in `crates/factory-core/src/homeconfig.rs` (a missing config gives the default).

- [ ] **Step 2: Write the failing tests.**

In `crates/factory-cli/tests/project_cmd.rs` (reuse `env`, `add_demo`, `run`):

```rust
#[test]
fn show_prints_the_version_after_the_project_line() {
    let e = env();
    add_demo(&e);
    let out = run(&e, &["show", "demo"]).expect("show");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "project: demo", "{out}");
    assert_eq!(
        lines[1],
        format!("version: {}", factory_core::version::FACTORY_VERSION),
        "{out}"
    );
}
```

| Test | Setup | Expected |
|---|---|---|
| `disabled_show_prints_the_version_after_the_project_line` | `let repo = add_demo(&e); std::fs::remove_dir_all(&repo)` | line 0 `project: demo`, line 1 `version: <FACTORY_VERSION>`, output still contains `disabled: repo path is missing` |

In the `tests` module of `factoryd.rs`:

```rust
#[test]
fn status_ends_with_the_factoryd_version_line() {
    let t = tempfile::tempdir().expect("tempdir");
    let home = Home::at(t.path().to_path_buf());
    let text = status(&home);
    assert_eq!(
        text.lines().last(),
        Some(format!("factoryd {}", env!("CARGO_PKG_VERSION")).as_str()),
        "{text}"
    );
    assert!(text.starts_with("Queue:\n"), "{text}");
}
```

Extend `status_shows_the_daily_budget_only_with_a_limit`: after the budget `starts_with` assertion, add `assert!(text.ends_with(&format!("factoryd {}\n", env!("CARGO_PKG_VERSION"))), "{text}");`.

In the new `crates/factory-supervisor/tests/version_marker.rs`, a helper `fn factoryd(home: &Home, args: &[&str]) -> Output` that runs `env!("CARGO_BIN_EXE_factoryd")` with `FACTORY_HOME` set to `home.dir` and `FACTORY_BIN` set to `/nonexistent/factory`, and a helper `fn home_with(t: &Path, marker: Option<&str>) -> Home` that creates `home.dir` and writes the marker when given.

```rust
#[test]
fn run_once_stamps_a_missing_marker() {
    let t = tempfile::tempdir().expect("tempdir");
    let home = home_with(t.path(), None);
    let out = factoryd(&home, &["run", "--once"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        std::fs::read_to_string(home.version_file()).expect("marker"),
        format!("{}\n", env!("CARGO_PKG_VERSION"))
    );
}
```

| Test | Marker | Args | Expected |
|---|---|---|---|
| `run_once_stamps_over_an_older_marker` | `0.0.1\n` | `["run", "--once"]` | success; marker `<CARGO_PKG_VERSION>\n` |
| `run_once_refuses_a_newer_marker_and_leaves_it` | `99.0.0\n` | `["run", "--once"]` | exit code 1; stderr starts with `factoryd: this home was last used by factory 99.0.0, this binary is <CARGO_PKG_VERSION>`; marker still `99.0.0\n`; `home.run_dir()` does not exist |
| `status_stop_and_enqueue_refuse_a_newer_marker` | `99.0.0\n` | `["status"]`, `["stop"]`, `["enqueue", "--project", "p", "--job", "001"]` | exit code 1 each; stderr contains `last used by factory 99.0.0`; `home.queue_dir()` does not exist |
| `status_accepts_an_older_marker_and_does_not_stamp` | `0.0.1\n` | `["status"]` | success; stdout ends with `factoryd <CARGO_PKG_VERSION>\n`; marker still `0.0.1\n` |
| `is_running_ignores_a_newer_marker` | `99.0.0\n` | `["is-running"]` | exit code 1; stderr is empty |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-cli --test project_cmd show`, `cargo test -j4 -p factory-supervisor --bin factoryd status` and `cargo test -j4 -p factory-supervisor --test version_marker`. Expect FAIL: line 1 of `show` is `repo: ...`; the status text ends with the Results block; `run --once` writes no marker; the newer-marker cases exit 0.

- [ ] **Step 4: Implement** the `version:` line in both branches of `fn show`, the check in `factoryd` `main` after the `Cmd::IsRunning` early return, the stamp at the top of `fn run`, and the final `factoryd <version>` line in `fn status`, as Produces says.

- [ ] **Step 5: Run** `cargo test -j4 -p factory-cli --test project_cmd`, `cargo test -j4 -p factory-supervisor --bin factoryd`, `cargo test -j4 -p factory-supervisor --test version_marker`, `cargo test -j4 -p factory-supervisor --test supervisor`, `cargo check -j4 -p factory-cli` and `cargo check -j4 -p factory-supervisor`. Expect PASS.

Next plan: docs/superpowers/plans/2026-10-20-upgrade-and-version-check/plan-02-upgrade-and-version-check.md
