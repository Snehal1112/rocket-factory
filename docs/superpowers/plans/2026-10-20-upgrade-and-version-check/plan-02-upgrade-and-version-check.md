# Upgrade and version check, Plan 2: factoryd and factory mismatch check

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** `factoryd` asks its `factory` binary for its version, warns once at `factoryd run` start (one `warning:` line on stderr and a `sup.warn` log entry) and shows the same warning in `factoryd status` right before the `factoryd <version>` line that plan 1 added. Work is never blocked.

**Architecture:** A new module `crates/factory-supervisor/src/version_check.rs` runs `<bin> --version` with a 5 second timeout and compares the last word of the first line with the own version. `ProcessSpawner::bin()` exposes the chosen path. `crates/factory-supervisor/src/bin/factoryd.rs` calls `mismatch` once in `run` after `ProcessSpawner::from_env`, and in `main` for `Status`, passing the result into `status`. No factory-engine change, so no port is involved. No board, ledger or events field is added; the log entry reuses the existing `sup.warn` event.

**Tech Stack:** Rust, `std::process::Command`, `tracing`, `factory-telemetry` (`capture` in tests), `tempfile`.

**Spec:** docs/superpowers/specs/2026-10-20-upgrade-and-version-check-design.md

## Global Constraints

- Always `-j4`; never `--workspace` or `--all`; never set `CARGO_TARGET_DIR`. Use per-crate commands: `cargo check -j4 -p factory-supervisor` and `cargo test -j4 -p factory-supervisor <filter>`.
- Paths reach the driver through `Home` and `Layout` only (`crates/factory-cli/tests/no_hardcoded_paths.rs`).
- `factory-core` stays free of I/O-heavy dependencies; `sha2` is pure. This plan adds no dependency.
- `factory presets refresh` output, and the existing `install` messages, keep their text.
- `upgrade` never overwrites an edited preset file.
- The mismatch check never blocks work: no error from it reaches `run` or `status` as an `Err`.
- Never read `/proc`; the check runs the binary with `Command`, arguments one by one, never through a shell.
- Each task lists the tests it adds. No task touches factory-engine. No task adds a board, ledger or events field; the log entry uses the existing `sup.warn` event name listed in `docs/software-factory.md`.
- Plan 1 is merged: `factory_core::version::FACTORY_VERSION` exists and `status` in `factoryd.rs` ends with the line `factoryd <FACTORY_VERSION>`, pinned by `status_ends_with_the_factoryd_version`. Use `FACTORY_VERSION` as the own version, never a second `env!("CARGO_PKG_VERSION")`.

## Review Focus

1. Output with no version word: a script printing `factory` (one word, no digit) gives `binary_version` an `Err`, and `mismatch` returns `cannot read the version of <path>: ...`. Pinned by `output_without_a_version_word_cannot_be_read` (Task 1).
2. A binary that never answers: a script running `sleep 30` ends after the timeout, is killed, and gives `Err` naming the timeout. Pinned by `a_binary_that_hangs_is_reported` (Task 1), which uses the private `version_within` with 300 ms so the test stays fast.
3. A binary that exits non-zero after printing a version: `echo "factory 0.1.2"; exit 3` gives `Err` with `exited with code 3`, never `Ok`. Pinned by `a_binary_that_fails_is_reported` (Task 1).
4. Several output lines: only the first line counts; `factory 0.1.2\nother 9.9.9` gives `Ok("0.1.2")`, and trailing `\r` or spaces are ignored. Pinned by `only_the_first_line_counts` (Task 1).
5. A pre-release binary: `factory 0.1.2-rc.1` against own `0.1.2` is a mismatch (exact text compare, unlike the marker rule of plan 1). Pinned by `a_pre_release_binary_is_reported` (Task 1).
6. Order in `factoryd status` with a budget and a mismatch: the text still starts with the budget line, and the `warning:` line is the second-to-last line, right before `factoryd <version>`. Pinned by `status_keeps_the_budget_first_with_a_mismatch` (Task 3).
7. The startup log entry: the `run` warning is logged with `event = "sup.warn"`, level `warn` and `msg` equal to the mismatch text. Pinned by `report_mismatch_logs_a_sup_warn` (Task 2).

---

## File Structure

- `crates/factory-supervisor/src/version_check.rs` (new): `binary_version`, `mismatch`, private `version_within`, unit tests.
- `crates/factory-supervisor/src/lib.rs` (modify): `pub mod version_check;`.
- `crates/factory-supervisor/src/spawn.rs` (modify): `ProcessSpawner::bin()`, `TEXT_FILE_BUSY` made `pub(crate)`, the test module made `pub(crate) mod tests` so `write_script` is reachable, one unit test.
- `crates/factory-supervisor/src/bin/factoryd.rs` (modify): `report_mismatch`, the call in `run`, `status` gets a mismatch argument, `main` computes it, unit tests.

### Task 1: version_check module and ProcessSpawner::bin

**Files:**
- Create: `crates/factory-supervisor/src/version_check.rs`
- Modify: `crates/factory-supervisor/src/lib.rs`, `crates/factory-supervisor/src/spawn.rs`

**Interfaces:**
- Consumes: `crate::spawn::tests::write_script(dir: &Path, name: &str, body: &str) -> PathBuf` (tests only, `spawn.rs:200`); `ProcessSpawner::new(bin: PathBuf) -> ProcessSpawner`.
- Produces:
  - `pub fn binary_version(bin: &Path) -> Result<String, String>` (5 second timeout).
  - `pub fn mismatch(bin: &Path, own: &str) -> Option<String>`: `None` when equal; `Some(format!("factory binary {} is {v}, factoryd is {own}", bin.display()))` when they differ; `Some(format!("cannot read the version of {}: {why}", bin.display()))` on `Err(why)`.
  - `fn version_within(bin: &Path, timeout: Duration) -> Result<String, String>` (private, called by `binary_version` with `Duration::from_secs(5)`).
  - `impl ProcessSpawner { pub fn bin(&self) -> &Path }`. The public field `bin` stays, because `factory_cli.rs` reads it.

- [ ] **Step 1: Read** spec section 3 and the edge-case rows for `--version`; `crates/factory-supervisor/src/spawn.rs` (`ProcessSpawner`, `start` with its `TEXT_FILE_BUSY` retry, the test module with `write_script` and `setup`); `crates/factory-supervisor/src/factory_cli.rs` (the `try_wait` timeout loop and kill on timeout, to copy the shape); `crates/factory-supervisor/src/lib.rs`.
- [ ] **Step 2: Write the failing tests.** In `spawn.rs` change `mod tests` to `pub(crate) mod tests` and add:

```rust
#[test]
fn bin_returns_the_chosen_path() {
    let sp = ProcessSpawner::new(PathBuf::from("/opt/x/factory"));
    assert_eq!(sp.bin(), Path::new("/opt/x/factory"));
}
```

Create `version_check.rs` with `#[cfg(all(test, unix))] mod tests { use super::*; use crate::spawn::tests::write_script; ... }` and this full test:

```rust
#[test]
fn a_different_version_is_reported() {
    let t = tempfile::tempdir().expect("tempdir");
    let bin = write_script(t.path(), "factory", "echo \"factory 0.1.1\"");
    assert_eq!(binary_version(&bin).expect("version"), "0.1.1");
    let text = mismatch(&bin, "0.1.2").expect("a mismatch");
    assert_eq!(
        text,
        format!("factory binary {} is 0.1.1, factoryd is 0.1.2", bin.display())
    );
}
```

The other tests share that shape (script body, call, assertion):

| Test | Script body | Call | Assertion |
|---|---|---|---|
| `a_matching_binary_has_no_mismatch` | `echo "factory 0.1.2"` | `mismatch(&bin, "0.1.2")` | `is_none()` |
| `a_binary_that_cannot_run_is_reported` | none; `bin = t.path().join("missing")` | `mismatch(&bin, "0.1.2")` | `expect("a mismatch")` starts with `format!("cannot read the version of {}: ", bin.display())` |
| `output_without_a_version_word_cannot_be_read` | `echo factory` | `binary_version(&bin)` and `mismatch` | `expect_err("no version")`; mismatch starts with `cannot read the version of ` |
| `a_binary_that_fails_is_reported` | `echo "factory 0.1.2"; exit 3` | `binary_version(&bin)` | `expect_err("non-zero exit")` contains `exited with code 3` |
| `a_binary_that_hangs_is_reported` | `sleep 30` | `version_within(&bin, Duration::from_millis(300))` | `expect_err("timeout")` contains `no answer within`; the call returns in under 5 seconds (measured with `Instant`) |
| `only_the_first_line_counts` | `printf 'factory 0.1.2 \r\nother 9.9.9\n'` | `binary_version(&bin)` | `expect("version") == "0.1.2"` |
| `a_pre_release_binary_is_reported` | `echo "factory 0.1.2-rc.1"` | `mismatch(&bin, "0.1.2")` | `Some` with `is 0.1.2-rc.1, factoryd is 0.1.2` |

Add `pub mod version_check;` to `lib.rs` so the module compiles.
- [ ] **Step 3: Run** `cargo test -j4 -p factory-supervisor --lib version_check` and `cargo test -j4 -p factory-supervisor --lib bin_returns_the_chosen_path`. Expect FAIL to compile: `cannot find function binary_version`, `mismatch`, `version_within`, and `no method named bin found` (the field exists, the method does not).
- [ ] **Step 4: Implement.** `ProcessSpawner::bin` returns `&self.bin`. Make `TEXT_FILE_BUSY` `pub(crate)`. `version_within`: build `Command::new(bin).arg("--version")` with `stdin` null, `stdout` piped, `stderr` null; spawn with the same `TEXT_FILE_BUSY` retry as `start` (20 tries, 10 ms) but without `process_group`; a spawn error gives `Err(e.to_string())`; poll `try_wait` every 20 ms until `timeout`, then `kill` and `wait` and return `Err(format!("no answer within {} ms", timeout.as_millis()))`; a non-zero or signal exit gives `Err("exited with code <n>")` (`none` for a signal); read stdout to a string after exit; take the first line, trim it, take the last whitespace-separated word; that word must start with an ASCII digit, else `Err(format!("no version in {:?}", first_line))`. `binary_version` calls it with 5 seconds. `mismatch` maps as in the Interfaces block. Comments are short full sentences ending with a period.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-supervisor --lib version_check`, `cargo test -j4 -p factory-supervisor --lib spawn` and `cargo check -j4 -p factory-supervisor`. Expect PASS: 8 `version_check` tests and every `spawn` test.

### Task 2: factoryd run warns once and logs it

**Files:**
- Modify: `crates/factory-supervisor/src/bin/factoryd.rs`

**Interfaces:**
- Consumes: `factory_supervisor::version_check::mismatch(bin: &Path, own: &str) -> Option<String>`; `ProcessSpawner::bin(&self) -> &Path`; `factory_core::version::FACTORY_VERSION`; `factory_telemetry::capture<R>(level: &str, f: impl FnOnce() -> R) -> (R, Vec<serde_json::Value>)`.
- Produces: `fn report_mismatch(text: &str)` in `factoryd.rs`: prints `warning: <text>` on stderr with `eprintln!` and logs `tracing::warn!(event = "sup.warn", msg = text)`.

- [ ] **Step 1: Read** spec section 3 (the `factoryd run` bullet); `run` in `factoryd.rs` (logging init with the `_log` guard, then `ProcessSpawner::from_env`); `log_report` in `crates/factory-supervisor/src/supervisor.rs` for the `sup.warn` shape; `StderrLayer` in `crates/factory-telemetry/src/layer.rs`; `crates/factory-supervisor/tests/log_events.rs` for the `capture` and `of` pattern.
- [ ] **Step 2: Write the failing test** in the `tests` module of `factoryd.rs`:

```rust
#[test]
fn report_mismatch_logs_a_sup_warn() {
    let text = "factory binary /x/factory is 0.1.1, factoryd is 0.1.2";
    let ((), lines) = factory_telemetry::capture("info", || report_mismatch(text));
    let warns: Vec<_> = lines.iter().filter(|l| l["event"] == "sup.warn").collect();
    assert_eq!(warns.len(), 1, "{lines:?}");
    assert_eq!(warns[0]["level"], "warn");
    assert_eq!(warns[0]["msg"], text);
}
```

- [ ] **Step 3: Run** `cargo test -j4 -p factory-supervisor --bin factoryd report_mismatch`. Expect FAIL to compile: `cannot find function report_mismatch`.
- [ ] **Step 4: Implement** `report_mismatch` from its signature. In `run`, right after `let spawner = ProcessSpawner::from_env()...?;` and before `Supervisor::new_locked`, add `if let Some(text) = version_check::mismatch(spawner.bin(), FACTORY_VERSION) { report_mismatch(&text); }`. It runs once, also with `--once`, and never returns an error. Plan 1's `stamp` call at the start of `run` stays where plan 1 put it.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-supervisor --bin factoryd` and `cargo check -j4 -p factory-supervisor`. Expect PASS, including the existing `version_flag_prints_the_workspace_version` and plan 1's tests.

### Task 3: factoryd status shows the mismatch

**Files:**
- Modify: `crates/factory-supervisor/src/bin/factoryd.rs`

**Interfaces:**
- Consumes: `mismatch`, `ProcessSpawner::from_env() -> Result<ProcessSpawner, SpawnError>`, `ProcessSpawner::bin`, `FACTORY_VERSION`.
- Produces: `fn status(home: &Home, mismatch: Option<&str>) -> String` (was `status(home: &Home)`); `fn status_mismatch() -> Option<String>`: `ProcessSpawner::from_env().ok().and_then(|s| version_check::mismatch(s.bin(), FACTORY_VERSION))`. `main` prints `status(&home, status_mismatch().as_deref())`.

- [ ] **Step 1: Read** spec sections 2 and 3 (`factoryd status` bullets) and the acceptance criterion on the budget line; `status` in `factoryd.rs` with plan 1's final `factoryd <version>` line; the tests `status_shows_the_daily_budget_only_with_a_limit` and `status_ends_with_the_factoryd_version`.
- [ ] **Step 2: Write the failing tests.** Change the existing calls `status(&home)` in the tests to `status(&home, None)`; their assertions stay, and the budget test keeps `starts_with`. Add:

```rust
#[test]
fn status_warns_on_a_mismatch() {
    let t = tempfile::tempdir().expect("tempdir");
    let home = Home::at(t.path().to_path_buf());
    let text = status(&home, Some("factory binary /x/factory is 0.1.1, factoryd is 0.1.2"));
    let lines: Vec<&str> = text.lines().collect();
    let n = lines.len();
    assert!(n >= 2, "{text}");
    assert_eq!(
        lines[n - 2],
        "warning: factory binary /x/factory is 0.1.1, factoryd is 0.1.2"
    );
    assert_eq!(lines[n - 1], format!("factoryd {FACTORY_VERSION}"));
    assert!(!status(&home, None).contains("warning:"));
}
```

| Test | Input | Assertion |
|---|---|---|
| `status_keeps_the_budget_first_with_a_mismatch` | `daily_budget_weighted = 1000` in `home.config_file()`, mismatch `Some("cannot read the version of /x/factory: gone")` | text starts with `daily budget: 0 of 1000 weighted tokens today (0%), resets at `; second-to-last line is `warning: cannot read the version of /x/factory: gone`; last line is `factoryd <FACTORY_VERSION>` |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-supervisor --bin factoryd status`. Expect FAIL to compile: `this function takes 1 argument but 2 arguments were supplied`.
- [ ] **Step 4: Implement** the new `status` signature: when `mismatch` is `Some(m)`, push `warning: {m}\n` right before the `factoryd <version>` line, and nothing else changes. Add `status_mismatch` and use it in `main` for `Cmd::Status`. When `from_env` fails there is no warning line; a missing file at the chosen path gives the `cannot read the version` line, the same as `run`.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-supervisor --bin factoryd`, `cargo test -j4 -p factory-supervisor --lib version_check`, `cargo clippy -j4 -p factory-supervisor --all-targets -- -D warnings` and `cargo fmt -p factory-supervisor -- --check`. Expect PASS.

Next plan: docs/superpowers/plans/2026-10-20-upgrade-and-version-check/plan-03-upgrade-and-version-check.md
