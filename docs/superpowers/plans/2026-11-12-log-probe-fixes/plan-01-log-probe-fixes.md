# Log and probe fixes Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-12-log-probe-fixes-design.md
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Make `factory log --job` accept a short job id and make `factory probe` run each role once, leave no scratch file and never overwrite an earlier run.

**Architecture:** Three small changes in three crates. `factory-telemetry` reads log files as lossy text. `factory-engine` gets a stamp helper and removes the role scratch folder; its agent calls still go through the existing `AgentRunner` port. `factory-cli` pads the log job id, dedupes roles, uses the unique stamp and prints the codex note.

**Tech Stack:** Rust, `std::fs`, `String::from_utf8_lossy`.

## Global Constraints

- Commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`. Never `--workspace` or `--all`.
- No `Cargo.toml` change, no `git` shell-outs, no `/proc` reads.
- No board, ledger, events or report field is added. Log lines keep their format.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call. Comments are short full sentences.

## Review Focus

1. `--job 1234` stays `1234` and `--job 0` becomes `000`. Pinned by `log_pads_a_short_job_id`.
2. A stamp whose folder exists but whose `.json` does not is still taken. Pinned by `unique_stamp_adds_a_suffix_when_taken`.
3. Roles given as `a b a` run as `a b` in that order. Pinned by `probe_dedupes_repeated_roles`.
4. Codex with the default delivery prints no note. Pinned by `probe_notes_a_delivery_codex_ignores`.
5. A non-UTF-8 file keeps its valid lines. Pinned by `read_log_keeps_lines_after_a_non_utf8_file`.

## File Structure

| File | Responsibility |
|---|---|
| `crates/factory-telemetry/src/read.rs` | Lossy read of each log file |
| `crates/factory-cli/src/log.rs` | Pad the job id |
| `crates/factory-engine/src/probe.rs`, `lib.rs` | `unique_stamp`, scratch removal, `Delivery::Agent` doc |
| `crates/factory-cli/src/probe.rs`, `cli.rs` | Dedupe, final stamp, codex note, help text |
| `crates/factory-cli/tests/log_cmd.rs`, `probe_cmd.rs` | CLI tests |

### Task 1: Log job padding and lossy log reads

**Files:**
- Modify: `crates/factory-cli/src/log.rs`, `crates/factory-telemetry/src/read.rs`
- Test: `crates/factory-cli/tests/log_cmd.rs`, `crates/factory-telemetry/src/read.rs`

**Interfaces:**
- Consumes: `factory_core::normalize_id(&str) -> Result<String, CoreError>`; `CliError` converts from `CoreError` as `norm()` in `commands.rs` relies on.
- Produces: `run_log` signature unchanged; `read_log` signature unchanged.

- [ ] **Step 1: Read `crates/factory-cli/src/log.rs`, `crates/factory-telemetry/src/read.rs` and the existing tests in `crates/factory-cli/tests/log_cmd.rs`.**
- [ ] **Step 2: Write the failing tests.** In `log_cmd.rs`, `log_pads_a_short_job_id`: build the log with `ctx_with_log` and `line("info", "001", 1)` plus a line for `002`, call `run_log(&ctx, Some("1"), None, None, 100)` and assert the output equals the `001` line. `log_refuses_a_non_numeric_job`: `run_log(&ctx, Some("abc"), ...)` with `expect_err`, assert it is `Err`. In `read.rs`, `read_log_keeps_lines_after_a_non_utf8_file`: write `log.jsonl.1` with bytes of one valid line, then `0xFF`, then a second valid line (valid lines on separate lines), read with an empty `LogFilter`, assert both valid lines are returned.
- [ ] **Step 3: Run `cargo test -j4 -p factory-cli --test log_cmd` and `cargo test -j4 -p factory-telemetry read_log_keeps`.** Expected: the padding test finds no lines, the refusal test fails because `abc` is accepted, and the telemetry test returns no lines.
- [ ] **Step 4: Implement.** In `run_log`, map `job` through `factory_core::normalize_id` with `?` before the filter is built. In `read_log`, replace `read_to_string` by `std::fs::read` plus `String::from_utf8_lossy`, still skipping a file that cannot be read.
- [ ] **Step 5: Run `cargo test -j4 -p factory-cli --test log_cmd` and `cargo test -j4 -p factory-telemetry`.** Expected: all pass.

### Task 2: Unique stamp and scratch cleanup in the engine

**Files:**
- Modify: `crates/factory-engine/src/probe.rs`, `crates/factory-engine/src/lib.rs`
- Test: unit tests in `crates/factory-engine/src/probe.rs`

**Interfaces:**
- Consumes: `utc_stamp() -> String`, `Harness::as_str`, `Engine::probe_scratch`.
- Produces: `pub fn unique_stamp(probes_dir: &Path, harness: Harness) -> String`, exported from the crate root; the probe command calls it. The agent calls of `run_probe` stay behind the `AgentRunner` port.

- [ ] **Step 1: Read `crates/factory-engine/src/probe.rs` (`probe_scratch`, `run_probe`) and `clock.rs`.**
- [ ] **Step 2: Write the failing tests** against a private `free_stamp(stamp: &str, probes_dir: &Path, harness: Harness) -> String`. `unique_stamp_keeps_a_free_stamp`: empty temp dir, `free_stamp("20261112-101530", dir, Harness::Claude)` equals `20261112-101530`. `unique_stamp_adds_a_suffix_when_taken`: create the folder `20261112-101530-claude`, expect `20261112-101530-2`; also create `20261112-101530-2-claude.json`, expect `20261112-101530-3`.
- [ ] **Step 3: Run `cargo test -j4 -p factory-engine unique_stamp`.** Expected: `cannot find function free_stamp in this scope`.
- [ ] **Step 4: Implement.** `free_stamp` returns the stamp when neither `<probes_dir>/<stamp>-<harness>` nor `<probes_dir>/<stamp>-<harness>.json` exists, else tries `<stamp>-2`, `<stamp>-3` and so on. `unique_stamp` calls it with `utc_stamp()`. Export it in `lib.rs` next to `utc_stamp`. In `run_probe`, remove `probe_scratch(role)` with `remove_dir_all` after the transcript is written and also when `probe_run` or the transcript write returns an error, ignoring the removal result. Change the `Delivery::Agent` doc to "A job-folder agents file, passed with `--agents`."
- [ ] **Step 5: Run `cargo test -j4 -p factory-engine unique_stamp` then `cargo check -j4 -p factory-engine`.** Expected: 2 passed, check clean.

### Task 3: Probe command changes

**Files:**
- Modify: `crates/factory-cli/src/probe.rs`, `crates/factory-cli/src/cli.rs`
- Test: `crates/factory-cli/tests/probe_cmd.rs`

**Interfaces:**
- Consumes: `factory_engine::unique_stamp(&Path, Harness) -> String`; `d.layout().probes_dir()`.
- Produces: no new public items.

- [ ] **Step 1: Read `crates/factory-cli/src/probe.rs`, the `--delivery` help in `cli.rs` and the fixtures at the top of `crates/factory-cli/tests/probe_cmd.rs` (`scripted`, `probe`, `ProbeRunner`).**
- [ ] **Step 2: Write the failing tests**, using `scripted(&fx)` and `probe(&ctx, args)` as `probe_saves_the_report_under_probes_dir` does. `probe_dedupes_repeated_roles`: args `--role kb --role kb --json`, parse the report, assert `roles.len() == 1`. `probe_removes_the_scratch_agents_file`: run `--role builder`, walk `Layout::in_repo(&fx.root).probes_dir().join("work")` and assert no file named `agents.json`. `probe_runs_in_one_second_keep_both_reports`: run `--role kb` twice with the same ctx, assert two `.json` files in the probes dir and that each report's `transcript_dir` has `kb.txt`. `probe_notes_a_delivery_codex_ignores`: `--harness codex --delivery append --role kb` output contains `Note: --delivery has no effect with codex.`; the same with default delivery does not contain `Note:`.
- [ ] **Step 3: Run `cargo test -j4 -p factory-cli --test probe_cmd`.** Expected: the dedupe, scratch and note tests fail; the two-run test may pass or fail by timing.
- [ ] **Step 4: Implement.** Dedupe `roles` in `run_probe_command` keeping first-seen order. Replace `utc_stamp()` by `unique_stamp(&d.layout().probes_dir(), harness)` and use that one value for the sink folder and `d.probe`. In table mode, when the harness is codex and delivery is not `Delivery::Agent`, append the line `Note: --delivery has no effect with codex.` before returning; JSON output is unchanged. Change the `--delivery` help to "agent (a job-folder agents file, passed with --agents), append or stdin."
- [ ] **Step 5: Run `cargo test -j4 -p factory-cli`, `cargo test -j4 -p factory-telemetry`, then `cargo clippy -j4 -p factory-cli -p factory-engine -p factory-telemetry`.** Expected: all pass, no warnings.

Next plan: none
