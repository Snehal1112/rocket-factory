# Structured Logging, Plan 3: CLI, factoryd and docs

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Install the log in the `factory` binary and in `factoryd`, add `factory log`, and document the feature.

**Architecture:** `factory`'s `main` installs the subscriber once the project context exists, using `log_level` and `FACTORY_LOG`. `factoryd` installs it with scope `factoryd` and turns the tick report into `sup.*` events. `factory log` reads the files through `factory_telemetry::read_log`.

**Tech Stack:** Rust, clap, `factory-telemetry` from plan 1, the engine events from plan 2.

**Spec:** docs/superpowers/specs/2026-10-19-structured-logging-design.md

## Global Constraints

- Always `-j4`; never `--workspace` or `--all`; never set `CARGO_TARGET_DIR`.
- Paths come only through `Layout::log_file` and `Home::factoryd_log_file`.
- `init` runs after the config load; a config-load error prints through plain `eprintln!` as today. `project`, `presets` and `install` commands install no log.
- The CLI's own `eprintln!("factory: {e}")` exit messages and `note:` lines stay as they are.
- `factory log [--job <id>] [--level <lvl>] [--tail <n>]`: `--tail` defaults to 100 and 0 means all; a missing file prints `no log yet` and exits 0; `--project <name>` reads that project's file.
- stderr shows `warn` and above, plus `sup.notice` at `info`; `sup.start`, `sup.finish` and `sup.reject` go to the file only.
- Logging never changes a result; a log failure prints one `note: log disabled: <why>` at startup at most.
- `FACTORY_LOG` set and invalid prints `note: FACTORY_LOG ignored: <why>`.

## Review Focus

- `factory log --job` with an id that has no lines prints nothing and exits 0 (Task 1 test `log_unknown_job_prints_nothing`).
- `--level warn` shows `warn` and `error` lines only (Task 1 test `log_level_filter`).
- `factory log` when the state folder does not exist prints `no log yet` (Task 1 test `no_log_yet_exits_zero`).
- A repeated factoryd error is logged once per `new_lines` rules (Task 2 test `repeated_error_is_logged_once`).
- A planted token in the environment of a run never reaches `log.jsonl` (Task 3 test `run_log_has_no_secret`).
- `FACTORY_LOG=bogus!!` does not stop a command (Task 1 test `bad_factory_log_is_a_note_not_a_failure`).

---

## File Structure

- `crates/factory-cli/Cargo.toml`, `crates/factory-supervisor/Cargo.toml` (modify): `tracing`, `factory-telemetry`.
- `crates/factory-cli/src/cli.rs` (modify): `Command::Log`.
- `crates/factory-cli/src/log.rs` (new): `run_log`.
- `crates/factory-cli/src/commands.rs` (modify): `CliCtx::log_level`, `CliCtx::log_target`, job spans, `set_stage_logged` at line 388, dispatch of `Log`.
- `crates/factory-cli/src/main.rs`, `src/lib.rs` (modify): init and export.
- `crates/factory-supervisor/src/supervisor.rs`, `src/bin/factoryd.rs` (modify).
- `crates/factory-cli/tests/log_cmd.rs` (new), `crates/factory-cli/tests/e2e.rs` (modify), `crates/factory-supervisor/tests/log_events.rs` (new).
- `docs/project-toml-reference.md`, `docs/software-factory.md`, `README.md` (modify).

### Task 1: `factory log` and the CLI install

**Files:**
- Create: `crates/factory-cli/src/log.rs`, `crates/factory-cli/tests/log_cmd.rs`
- Modify: `crates/factory-cli/Cargo.toml`, `src/cli.rs`, `src/commands.rs`, `src/main.rs`, `src/lib.rs`

**Interfaces:**
- Consumes: `Layout::log_file`, `Config::log_level_or_default`, `factory_telemetry::{init, LogTarget, Scope, LogFilter, read_log}`, `factory_engine::{job_span, set_stage_logged}`.
- Produces:
  - `Command::Log { job: Option<String>, level: Option<String>, tail: usize }` with `--tail` default 100 (clap `default_value_t = 100`).
  - `pub fn run_log(ctx: &CliCtx, job: Option<&str>, level: Option<&str>, tail: usize) -> Result<String, CliError>`: lines joined with `\n`, or `no log yet` when the file and its rotated files are all missing; `level` must be one of the five names or `CliError::Refused`.
  - `CliCtx::log_level(&self) -> String`: `ctx.config`'s level when `config` is `Some`, else the layout config file parsed with `Config::parse` (any error gives `"info"`).
  - `CliCtx::log_target(&self, project: Option<&str>) -> factory_telemetry::LogTarget` (path `layout.log_file()`, scope `Cli`).
  - In `main.rs`, after the context is built and for every command except `Log`: `factory_telemetry::init(ctx.log_target(cli.project.as_deref()), &ctx.log_level(), std::env::var("FACTORY_LOG").ok().as_deref())`; the `Guard` is kept until `main` ends; its `note()` prints as `note: FACTORY_LOG ignored: <why>`; an `Err` prints `note: log disabled: <why>` and the command goes on.
  - The approve, rework and cancel commands enter `factory_engine::job_span(id)`; `commands.rs:388` uses `set_stage_logged`.

- [ ] **Step 1: Write the failing tests** in `tests/log_cmd.rs`, using a temp layout and a hand-written `log.jsonl`:
  - `log_prints_a_jobs_lines`: three lines for jobs `001` and `002`; `--job 001` prints only the `001` lines, in file order.
  - `log_level_filter`: lines at info, warn, error; `--level warn` prints two.
  - `log_tail_keeps_the_last_n`; `log_unknown_job_prints_nothing`; `no_log_yet_exits_zero` (output is exactly `no log yet`); `log_rejects_an_unknown_level`.
  - `bad_factory_log_is_a_note_not_a_failure`: run the built binary `factory status` with `FACTORY_LOG=bogus!!` against a temp repo; exit code 0 and stderr contains `FACTORY_LOG ignored`.
  - In `cli.rs` unit tests: `log_parses_flags` (`factory log --job 003 --level warn --tail 5`) and `log_tail_defaults_to_100`.
- [ ] **Step 2: Run** `cargo test -j4 -p factory-cli log` and expect FAIL.
- [ ] **Step 3: Implement** the items above; dispatch `Command::Log` to `run_log`. `no_hardcoded_paths` must still pass.
- [ ] **Step 4: Run** `cargo test -j4 -p factory-cli` and expect PASS.

### Task 2: factoryd events

**Files:**
- Create: `crates/factory-supervisor/tests/log_events.rs`
- Modify: `crates/factory-supervisor/Cargo.toml`, `src/supervisor.rs`, `src/bin/factoryd.rs`

**Interfaces:**
- Consumes: `Home::factoryd_log_file`, `factory_telemetry::{init, LogTarget, Scope}`.
- Produces:
  - `pub fn log_report(report: &TickReport, printed: &mut HashSet<String>)` in `supervisor.rs`: emits `sup.start` per `started` id, `sup.finish` per `finished` (`request`, `outcome` as `Debug` text), `sup.reject` per `rejected` (`request`, `reason`), and for the lines `new_lines(printed, report)` returns: `sup.error` (error) for `errors`, `sup.warn` (warn) for `bad_children` and `orphans`, `sup.notice` (info) for `report.notices`. Only the not-yet-seen lines are logged.
  - `run_loop` and `factoryd run --once` call `log_report` instead of their `eprintln!` loops; the `msg` of each event is the old line, so the user sees the same words on stderr.
  - `factoryd` `main` installs the log for the `run` command only: `init(LogTarget { path: home.factoryd_log_file(), project: None, scope: Scope::Factoryd }, level "info", FACTORY_LOG)`; the `eprintln!("factoryd: {e}")` exit message stays.

- [ ] **Step 1: Write the failing tests** in `tests/log_events.rs` using `factory_telemetry::capture`:
  - `supervisor_logs_start_and_finish_without_printing_them`: a report with `started = ["r1"]` and `finished = [("r1", Outcome::..)]` gives a `sup.start` and a `sup.finish` object with `request == "r1"`, and `new_lines` returns no stderr line for them.
  - `repeated_error_is_logged_once`: `log_report` twice with the same `errors` entry gives one `sup.error` object.
  - `returning_error_is_logged_again`: error, empty report, error gives two.
  - `bad_child_and_orphan_log_warn`; `notice_logs_info_sup_notice`.
- [ ] **Step 2: Run** `cargo test -j4 -p factory-supervisor log_` and expect FAIL.
- [ ] **Step 3: Implement** as described. Check that the existing `new_lines` tests in `supervisor.rs` still pass unchanged.
- [ ] **Step 4: Run** `cargo test -j4 -p factory-supervisor` and expect PASS.

### Task 3: End to end and docs

**Files:**
- Modify: `crates/factory-cli/tests/e2e.rs`, `docs/project-toml-reference.md`, `docs/software-factory.md`, `README.md`

**Interfaces:**
- Consumes: everything above.

- [ ] **Step 1: Write the failing tests** in `tests/e2e.rs`:
  - `run_writes_a_log_with_merge_done`: the existing end-to-end run (fake claude) leaves `<state_dir>/log.jsonl` whose lines all parse as JSON, contain `stage.change` lines in board-history order, one `merge.done` whose `sha` equals the merged sha, and every line has `ts`, `level`, `scope == "cli"`, `event`.
  - `run_log_has_no_secret`: the same run with `ANTHROPIC_API_KEY=planted-secret-999` set in the driver environment leaves no `planted-secret-999` in `log.jsonl`.
  - `factory_log_command_shows_the_run`: `factory log --job <id> --level info` prints the `merge.done` line.
- [ ] **Step 2: Run** `cargo test -j4 -p factory-cli --test e2e` and expect FAIL.
- [ ] **Step 3: Fix** whatever the test shows is missing. Then write the docs: a `log_level` row (values, default `info`, `FACTORY_LOG` override) in `docs/project-toml-reference.md`; a "Logging" section in `docs/software-factory.md` (files, fields table of spec section 4, rotation, redaction rule, `factory log`); a README line for `factory log` and the two files.
- [ ] **Step 4: Run** `cargo test -j4 -p factory-cli`, `cargo test -j4 -p factory-telemetry`, `cargo test -j4 -p factory-engine` and `cargo test -j4 -p factory-supervisor` and expect PASS.

Next plan: none
