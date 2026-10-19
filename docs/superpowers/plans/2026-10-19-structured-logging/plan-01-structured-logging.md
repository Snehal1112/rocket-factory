# Structured Logging, Plan 1: Settings and the telemetry crate

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Add the `log_level` setting, the two log paths, and the `factory-telemetry` crate that formats, redacts, rotates and reads the log.

**Architecture:** `factory-core` gets the setting and the paths. A new crate `factory-telemetry` holds one custom `tracing_subscriber::Layer` that writes JSON lines through a rotating file, a stderr layer, an `init`, a `capture` helper for tests and `read_log`. The crate depends on no other factory crate.

**Tech Stack:** Rust 2021, `tracing` 0.1, `tracing-subscriber` 0.3 (features `env-filter`, `registry`), `serde_json`, `fs2`, `tempfile` (dev).

**Spec:** docs/superpowers/specs/2026-10-19-structured-logging-design.md

## Global Constraints

- Always `-j4`; never `--workspace` or `--all`; never set `CARGO_TARGET_DIR`.
- Paths reach the driver only through `Layout` and `Home`; `factory-cli/tests/no_hardcoded_paths.rs` must still pass.
- `log_level` allowed values: `error`, `warn`, `info`, `debug`, `trace`; unset means `info`; error text `log_level must be one of error, warn, info, debug, trace when it is set`.
- File cap 10 MiB, rotated files `.1` to `.3`; rotation under an exclusive `fs2` lock on `<file>.lock` with a second size check; plain appends take no lock.
- Message cap 300 characters (the last 300, like `MAX_MESSAGE` in `crates/factory-agent/src/events.rs`).
- Redact any field whose name contains `token`, `key`, `secret` or `password` (case-insensitive substring) as `"[redacted]"`; the one exception is the exact name `weighted_tokens`.
- A log write failure never fails the caller.
- Timestamp: UTC RFC 3339 with milliseconds, for example `2026-10-19T09:12:03.412Z`.

## Review Focus

- A `Password` or `API_TOKEN` field in any case is redacted (Task 2 test `redaction_is_case_insensitive`).
- A message with multi-byte characters is cut on a character boundary (Task 2 test `message_cut_keeps_whole_chars`).
- A log directory that does not exist is created on the first write (Task 2 test `first_write_creates_the_folder`).
- A log path that is a directory or unwritable counts a failure and returns normally (Task 2 test `write_error_is_counted_not_returned`).
- `init` with a bad `FACTORY_LOG` value falls back and reports why instead of failing (Task 3 test `invalid_env_filter_falls_back_with_a_note`).
- A log that holds a corrupt line and a missing rotated file still reads (Task 3 tests `read_log_skips_corrupt_line`, `read_log_reads_rotated_files_oldest_first`).

---

## File Structure

- `Cargo.toml` (modify): member `crates/factory-telemetry`; workspace dependencies `tracing = "0.1"`, `tracing-subscriber = { version = "0.3", default-features = false, features = ["env-filter", "registry", "fmt", "std"] }`.
- `crates/factory-core/src/config.rs` (modify): `log_level`.
- `crates/factory-core/src/layout.rs` (modify): `Layout::log_file`.
- `crates/factory-core/src/home.rs` (modify): `Home::factoryd_log_file`.
- `crates/factory-telemetry/Cargo.toml`, `src/lib.rs` (new): re-exports, `TelemetryError`.
- `crates/factory-telemetry/src/redact.rs` (new): name and command redaction, message cap.
- `crates/factory-telemetry/src/time.rs` (new): `rfc3339_utc`.
- `crates/factory-telemetry/src/rotate.rs` (new): `RotatingFile`.
- `crates/factory-telemetry/src/layer.rs` (new): `JsonLayer`, `StderrLayer`.
- `crates/factory-telemetry/src/init.rs` (new): `init`, `Guard`, `capture`.
- `crates/factory-telemetry/src/read.rs` (new): `LogFilter`, `read_log`.

### Task 1: The setting and the paths

**Files:**
- Modify: `crates/factory-core/src/config.rs`, `crates/factory-core/src/layout.rs`, `crates/factory-core/src/home.rs`

**Interfaces:**
- Produces: `Config.log_level: Option<String>`; `Config::log_level_or_default(&self) -> &str` (returns `"info"` when unset); `Layout::log_file(&self) -> PathBuf` (`<state_dir>/log.jsonl`); `Home::factoryd_log_file(&self) -> PathBuf` (`<home>/factoryd.log.jsonl`).

- [ ] **Step 1: Write the failing tests**
  - `config.rs`: `log_level_unset_by_default_and_takes_only_known_values`: `Config::default().log_level == None`, `log_level_or_default() == "info"`; `Config::parse("log_level = \"debug\"")` is ok; `Config::parse("log_level = \"loud\"")` is `Err` and the message contains `log_level must be one of error, warn, info, debug, trace`.
  - `layout.rs`: `log_file_is_in_the_state_dir`: `Layout::in_repo(Path::new("/r")).log_file() == PathBuf::from("/r/.superpowers/factory/log.jsonl")`.
  - `home.rs`: `factoryd_log_file_is_in_the_home`: `Home::at(PathBuf::from("/h")).factoryd_log_file() == PathBuf::from("/h/factoryd.log.jsonl")`.
- [ ] **Step 2: Run** `cargo test -j4 -p factory-core log_` and expect FAIL (missing items).
- [ ] **Step 3: Implement** the field next to `cache_ttl` in `Config` (default `None`, serde default like its neighbours), the check in `Config::parse` after the `cache_ttl` check, and the two path functions. Check `factory-core/tests/fixtures` and `rocket_parity.rs` still pass; a fixture needs no change because the key is optional.
- [ ] **Step 4: Run** `cargo test -j4 -p factory-core` and expect PASS.

### Task 2: Format, redaction, time and rotation

**Files:**
- Create: `crates/factory-telemetry/Cargo.toml`, `src/lib.rs`, `src/redact.rs`, `src/time.rs`, `src/rotate.rs`
- Modify: `Cargo.toml` (workspace member and dependencies)

**Interfaces:**
- Produces:
  - `pub fn redact_field(name: &str, value: serde_json::Value) -> serde_json::Value`: `"[redacted]"` when the name is sensitive and not `weighted_tokens`.
  - `pub fn redact_command(command: &str) -> String`: splits on whitespace, for each word `--name=value` with a sensitive `name` writes `--name=[redacted]`, joins with single spaces.
  - `pub fn cap_message(msg: &str) -> String`: the last 300 characters.
  - `pub fn rfc3339_utc(epoch_ms: u64) -> String`.
  - `pub struct RotatingFile`; `RotatingFile::new(path: impl Into<PathBuf>, cap_bytes: u64) -> RotatingFile`; `write_line(&self, line: &str)` (adds the newline, never returns an error); `failures(&self) -> u64`. The production cap is `pub const LOG_CAP_BYTES: u64 = 10 * 1024 * 1024`.
  - `pub enum TelemetryError { AlreadyInstalled, Io(String) }` implementing `Display`.

- [ ] **Step 1: Write the failing tests** (unit tests in each file, `tempfile` for folders)
  - `redact.rs`: `sensitive_field_names_are_redacted` (`api_token`, `secret_key`, `password`, `Authorization_Key` are redacted, `job_id` is not); `redaction_is_case_insensitive` (`Password`, `API_TOKEN`); `weighted_tokens_is_the_one_field_not_redacted`; `command_value_of_a_sensitive_flag_is_redacted` (`cargo run --token=abc --name=x` becomes `cargo run --token=[redacted] --name=x`); `bare_secret_word_in_a_command_is_kept` (`echo hunter2` unchanged); `message_is_cut_to_300_chars`; `message_cut_keeps_whole_chars` (400 `é` gives 300 `é`).
  - `time.rs`: `epoch_zero` (`rfc3339_utc(0) == "1970-01-01T00:00:00.000Z"`); `leap_day` (`rfc3339_utc(951_782_400_000) == "2000-02-29T00:00:00.000Z"`); `milliseconds` (`rfc3339_utc(1_234) == "1970-01-01T00:00:01.234Z"`).
  - `rotate.rs`: `first_write_creates_the_folder`; `write_appends_lines_with_newlines`; `rotation_moves_files_to_1_2_3` (cap 50 bytes, many writes: `.1`, `.2`, `.3` exist and the live file is under the cap plus one line); `oldest_file_is_dropped` (no `.4` ever); `concurrent_writers_lose_no_line` (8 threads write 200 lines each, `{"n":<unique int>}` padded to 40 bytes, cap 4096; afterwards every line of the live file and `.1` to `.3` parses as JSON, no `n` appears twice); `write_error_is_counted_not_returned` (the path is an existing directory, `write_line` returns, `failures() == 1`).
- [ ] **Step 2: Run** `cargo test -j4 -p factory-telemetry` and expect FAIL (crate missing). Create the crate manifest first so the failure is a test failure.
- [ ] **Step 3: Implement** the items above. `redact_*` use ASCII-lowercased substring checks. `rfc3339_utc` converts days to civil date with the days-from-civil algorithm, no extra crate. `RotatingFile::write_line` opens with `OpenOptions::new().create(true).append(true)`, makes the parent folder, does one `write_all` of `line + "\n"`, then if `metadata.len() > cap` takes `fs2` `lock_exclusive` on `<path>.lock`, re-reads the size, and if still over renames `.2→.3`, `.1→.2`, file→`.1`; every error adds 1 to an `AtomicU64` and returns.
- [ ] **Step 4: Run** `cargo test -j4 -p factory-telemetry` and expect PASS.

### Task 3: Layers, init, capture and read

**Files:**
- Create: `crates/factory-telemetry/src/layer.rs`, `src/init.rs`, `src/read.rs`
- Modify: `crates/factory-telemetry/src/lib.rs`

**Interfaces:**
- Consumes: Task 2 items.
- Produces:
  - `pub enum Scope { Cli, Factoryd }` with `as_str()` (`"cli"`, `"factoryd"`).
  - `pub struct LogTarget { pub path: PathBuf, pub project: Option<String>, pub scope: Scope }`.
  - `pub fn init(target: LogTarget, level: &str, env: Option<&str>) -> Result<Guard, TelemetryError>`: file filter is the valid `env` value, else `level`; an invalid `env` value is ignored and `init` returns the reason in `Guard::note()`; installs the global subscriber (file layer plus stderr layer at `warn`, plus `info` for events whose `event` field is exactly `sup.notice`); a second call returns `Err(AlreadyInstalled)`.
  - `pub struct Guard`; `Guard::note(&self) -> Option<&str>`; `Guard::failures(&self) -> u64`.
  - `pub fn capture<R>(level: &str, f: impl FnOnce() -> R) -> (R, Vec<serde_json::Value>)`: runs `f` under a thread-local subscriber (`tracing::subscriber::with_default`) with the same JSON layer writing to memory; no stderr output.
  - `pub struct LogFilter { pub job: Option<String>, pub min_level: Option<String>, pub tail: usize }` (`tail 0` = all); `pub fn read_log(path: &Path, filter: &LogFilter) -> Result<Vec<String>, TelemetryError>`: reads `.3`, `.2`, `.1`, then the file, oldest first, skips unparsable and missing files, keeps lines with `job_id == job` and `level` at or above `min_level`, then the last `tail`.
- JSON line keys: `ts`, `level`, `scope`, optional `project`, optional `job_id`, optional `stage`, `event`, `msg` (capped), other fields redacted by `redact_field`. `project` and `scope` come from `LogTarget`. Span fields (`job_id`, `stage`, others) are merged into each line, the event's own fields win. The `event` field is required on an event; a plain `tracing::info!("text")` gets `event: "log"` and `msg: "text"`.
- Stderr line: `<level> <event>: <msg>`.

- [ ] **Step 1: Write the failing tests** (in `init.rs` and `read.rs`, using `capture` and `tempfile`)
  - `line_has_ts_level_scope_event`: `capture("info", || tracing::info!(event = "x.y", msg = "hi"))` gives one object with keys `ts`, `level == "info"`, `scope == "cli"`, `event == "x.y"`, `msg == "hi"`.
  - `span_fields_reach_the_line`: inside `tracing::info_span!("job", job_id = "003").in_scope(...)` the line has `job_id == "003"`.
  - `event_field_beats_span_field`: a span with `stage = "build"` and an event with `stage = "review"` gives `"review"`.
  - `level_filter_drops_debug_at_info`: a `debug!` is absent from `capture("info", ...)` and present from `capture("debug", ...)`.
  - `sensitive_event_field_is_redacted_in_the_line`.
  - `planted_token_never_reaches_the_file`: `init` into a temp file is global, so use a `JsonLayer` built over a `RotatingFile` in `with_default`; after logging a field `api_token = "sekret-123"` and a message that does not contain it, the file text has no `sekret-123`.
  - `init_twice_returns_already_installed`: run in its own test binary file `crates/factory-telemetry/tests/init_twice.rs` because `init` is global; the first call is `Ok`, the second `Err(TelemetryError::AlreadyInstalled)`.
  - `invalid_env_filter_falls_back_with_a_note`: a pure function `resolve_filter(level: &str, env: Option<&str>) -> (String, Option<String>)` returns the level and a note for `Some("not a filter!!")`; `env_filter_beats_log_level`: `Some("debug")` with level `info` gives `debug` and no note.
  - `read_log_filters_by_job_and_level`, `read_log_tail_and_missing_file` (missing file gives `Ok(vec![])`, `tail = 2` gives the last two), `read_log_skips_corrupt_line`, `read_log_reads_rotated_files_oldest_first`.
- [ ] **Step 2: Run** `cargo test -j4 -p factory-telemetry` and expect FAIL.
- [ ] **Step 3: Implement** `JsonLayer<W>` as a `tracing_subscriber::Layer` over `Registry` using `LookupSpan`: on span creation store its fields in span extensions, on event collect fields with a `Visit` (record_str, record_i64, record_u64, record_bool, record_debug), merge parent span fields root-first, call `redact_field`, build the object with `serde_json::Map`, write via `RotatingFile::write_line`. `StderrLayer` prints with `eprintln!`. Filtering uses `EnvFilter` on the file layer only (per-layer filter with `Layer::with_filter`). Export everything from `lib.rs`.
- [ ] **Step 4: Run** `cargo test -j4 -p factory-telemetry` and `cargo check -j4 -p factory-core` and expect PASS.

Next plan: docs/superpowers/plans/2026-10-19-structured-logging/plan-02-structured-logging.md
