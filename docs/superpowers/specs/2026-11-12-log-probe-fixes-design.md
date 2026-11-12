# Log and probe fixes design

## Goal

Make `factory log --job` accept a short job id and make `factory probe` run each role once, leave no scratch file behind and never overwrite an earlier run.

## Overview

GitHub issue #60 reports four defects in `factory log` and `factory probe`. A code read in this session confirms all of them.

- A. `factory log --job 7` prints nothing for job `007`. `run_log` passes the raw string into `LogFilter.job` (`log.rs:40`) and `read_log` compares it with the logged `job_id` (`read.rs:47`). Logged ids are always padded by `normalize_id` (`ids.rs:4`). Other commands call `norm()` first (`commands.rs:452`); `log` does not.
- B. `--role builder --role builder` is kept as given (`probe.rs:53-57`). `run_probe` loops over every entry (`engine/probe.rs:255`), so a role runs twice, the second transcript overwrites the first and the report has two rows.
- C. `probe_scratch` (`engine/probe.rs:82`) is created in `run_probe` and never removed, so `probes/work/probe/<role>/agents.json` stays on disk. `utc_stamp()` (`clock.rs:43`) has one-second resolution, so two runs of one harness in the same second share `<stamp>-<harness>/`, the `<stamp>-<harness>.json` report and the scratch folder.
- D. Three small gaps: the `--delivery` help (`cli.rs:134`) and the `Delivery::Agent` doc (`engine/probe.rs:21`) describe an old mechanism; `--delivery` is ignored for codex without a word (`hide_codex_tools` takes no delivery, `engine/probe.rs:228`); `read_log` skips a rotated file that is not valid UTF-8 without a note (`read.rs:36-38`).

## Current state

| Piece | Today | Gap |
|---|---|---|
| `run_log` | Passes `job` as given | `7` never matches `007` |
| `run_probe_command` | `roles.to_vec()` as given, stamp from `utc_stamp()` | Duplicates run twice; stamp not unique |
| `run_probe` | Creates `<stamp>-<harness>/`, one scratch folder per role | Scratch never removed |
| `render_probe` | Table, skills, `Report:` and `Transcripts:` lines | Silent about ignored `--delivery` |
| `read_log` | `read_to_string` per file, `continue` on error | A non-UTF-8 file vanishes |

## Non-goals

- Making two probes that run at the same moment in two processes safe. The stamp check is an existence check, not a lock.
- A new field in the report JSON or the board.
- Changing the log line format or how `read_log` treats bad lines.

## Design

```
factory log --job 7  -> normalize_id("7") = "007" -> LogFilter.job = "007"
factory probe        -> dedupe roles -> unique stamp -> per role: run, write transcript, remove scratch
```

### 1. `factory log --job`

`run_log` maps a given job through `factory_core::normalize_id` before it builds the filter, with `?` as `norm()` does. A value that is not all digits is refused with the same error the other commands give. `7` and `007` return the same lines; `1234` stays `1234`.

### 2. Duplicate roles

`run_probe_command` removes repeated roles, keeping first-seen order, before it calls `d.probe`. The dedupe is silent. With no `--role` the list is every role as today.

### 3. Unique stamp

`pub fn unique_stamp(probes_dir: &Path, harness: Harness) -> String` in `factory-engine/src/probe.rs`, exported from the crate. It starts from `utc_stamp()`. A stamp is taken when `<probes_dir>/<stamp>-<harness>` or `<probes_dir>/<stamp>-<harness>.json` exists. When it is taken, the result is `<stamp>-2`, then `-3`, until one is free. Example: `20261112-101530-claude` taken gives `20261112-101530-2-claude`.

`run_probe_command` calls it once and passes the result to both the event sink folder and `d.probe`, so they agree. The printed `Report:` path and the saved file match the final stamp.

### 4. Scratch cleanup

`run_probe` removes `probe_scratch(role)` after each role, on success and on error, and ignores a missing folder. The transcript is written before the removal. Errors from the removal are ignored. The transcript and report folders are outside `work/`, so nothing the user is told about is removed.

### 5. Defect D

- The `--delivery` help reads "agent (a job-folder agents file, passed with --agents), append or stdin". The `Delivery::Agent` doc reads "A job-folder agents file, passed with `--agents`."
- `run_probe_command` appends one line to the table text when the harness is codex and the delivery is not `Agent`: `Note: --delivery has no effect with codex.` The JSON output is unchanged.
- `read_log` reads each file as bytes and converts it with `String::from_utf8_lossy`. A file that cannot be read at all is still skipped.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| `--job abc`, `--job ""` | Refused by `normalize_id` |
| `--job 0` | `000`, matches nothing |
| `--job 1234` | Unchanged |
| `--role a --role b --role a` | Runs a then b |
| Stamp, report or folder already exists | `-2`, `-3` suffix |
| A role fails | Scratch removed, error returned as today |
| Scratch folder already gone | Ignored |
| Codex with default delivery | No note |
| Codex with `--delivery append` | Note in the table only |
| Rotated log file with invalid UTF-8 bytes | Valid lines are kept, bad bytes become U+FFFD |
| Unreadable log file | Skipped as today |

## Testing

- `crates/factory-cli/tests/log_cmd.rs`: `log_pads_a_short_job_id` (`Some("1")` finds the lines of `001`); `log_refuses_a_non_numeric_job`.
- `crates/factory-cli/tests/probe_cmd.rs`: `probe_dedupes_repeated_roles` (one row, one runner call); `probe_removes_the_scratch_agents_file` (no `agents.json` under `probes/work` after the run); `probe_runs_in_one_second_keep_both_reports` (two runs, two distinct report paths and transcript folders); `probe_notes_a_delivery_codex_ignores`.
- `crates/factory-engine/src/probe.rs` unit tests: `unique_stamp_adds_a_suffix_when_taken` and `unique_stamp_keeps_a_free_stamp`.
- `crates/factory-telemetry/src/read.rs` unit test: `read_log_keeps_lines_after_a_non_utf8_file`.
- Commands: `cargo test -j4 -p factory-cli`, `-p factory-engine`, `-p factory-telemetry`, then `cargo clippy -j4`.

## Files changed

- `crates/factory-cli/src/log.rs`, `probe.rs`, `cli.rs`
- `crates/factory-engine/src/probe.rs`, `lib.rs`
- `crates/factory-telemetry/src/read.rs`
- Tests as listed above.

## Constraints

- Run from the repo root with `cargo ... -j4`, no `--workspace` or `--all`.
- Do not shell out to git, do not read `/proc`, add no `Cargo.toml` dependency.
- Log lines keep their format. Report JSON and board JSON do not change.

## Acceptance criteria

- `factory log --job 7` and `--job 007` return the same lines.
- `factory log --job abc` is refused.
- `--role builder --role builder` runs one agent and prints one row.
- No `agents.json` is left under `probes/work` after a probe run, whether a role passes or fails.
- Two same-second runs write separate reports and transcript folders.
- The printed `Report:` path exists after the run.
- The `--delivery` help and the `Delivery::Agent` doc describe `--agents`.
- A codex probe with a non-default `--delivery` prints the note in the table.
- A non-UTF-8 rotated log file no longer drops its valid lines.

## Out of scope

- A lock between concurrent probe processes.
- Editing `probes/work` handling for anything other than the role scratch folder.

## Follow-ups

None.

## Decisions

- What should `factory log --job` do with a value that is not all digits? Options: refuse like other commands, pad digits and keep other text, pad digits and print a note. Chosen: refuse like other commands. Decided by: user
- How should the run stamp become unique? Options: suffix on collision, always add milliseconds, always add the process id. Chosen: suffix on collision. Decided by: user
- When is the scratch folder removed? Options: after each role including on error, after each role on success only, once at the end. Chosen: after each role including on error. Decided by: user
- Should the dedupe of repeated roles be silent? Options: silent, warn on stderr, refuse. Chosen: silent. Decided by: user
- Which defect D items are in scope? Options: all three, texts and non-UTF-8 only, texts only. Chosen: all three. Decided by: user
- Where does the codex delivery note go? Options: the report JSON, the table text only. Chosen: the table text only, so the report JSON does not change. Decided by: call
