# Redact commands in the format and lint gate details

## Goal

Run every command shown in a `format` or `lint` gate detail through `redact_command`, so the polled dashboard state never carries a secret flag value.

## Overview

With `gate_record` on, the driver writes one `format` and one `lint` line per checks run into the job's `gates.jsonl`. Their detail is the raw list of commands that ran for that gate, joined with `, `. The dashboard reads that file and sends every detail in `/api/state`, which the board polls every second.

The log path already hides secrets: `log_check_results` passes each command through `factory_telemetry::redact_command` before it logs a `check.run` event. The gate details skip that step, so a check command such as `cargo fmt --check --api-key=abc` would show `abc` on every card. Issue #143 asks for the same redaction on the gate details. It was filed from the review of the gate record (`77769b0`).

## Current state

- `gate_checks_run` (`crates/factory-engine/src/checks_stage.rs:108-140`) writes the `format` and `lint` lines. For each gate it collects the results whose command `plan.gate_of` maps to that gate (`:111-115`), then writes `commands.join(", ")` of the raw commands as the detail (`:125-126`). With no such command it writes `skipped` "no format command" or "no lint command" (`:116-118`).
- `log_check_results` (`:71-82`) logs each command as `factory_telemetry::redact_command(&r.command)`.
- `redact_command` (`crates/factory-telemetry/src/redact.rs:26-37`) splits the command on whitespace, replaces the value of every `--name=value` word whose name contains `token`, `key`, `secret` or `password` (any case, except `weighted_tokens`) with `[redacted]`, and joins the words with single spaces. factory-engine already depends on factory-telemetry.
- `CheckPlan::gate_of` (`crates/factory-checks/src/plan.rs:27-29`) looks a command up by its exact raw text.
- The `checks` line detail is a count ("N of M commands passed") and holds no command.
- `a_failing_format_command_writes_format_fail` (`crates/factory-engine/tests/gate_record.rs:377-386`) checks that a failing format detail contains `false`.

Gap: the `format` and `lint` details hold raw commands.

## Non-goals

- Widening `redact_command`, for example to `--token abc` or an `API_KEY=abc` prefix.
- Redacting `checks.md`, which the park dialog fetches only when a person opens a parked job.
- Any change to which commands run, to `gate_of`, or to the `checks` line.
- Any change to the dashboard UI: its layout, the gate strip, the gate list or the styles.

## Design

```
gate_checks_run
  results for the gate = report results whose raw command gate_of maps to it   (unchanged)
  detail = each result's redact_command(command), joined with ", "             (new)
```

### 1. Redacted detail

In `gate_checks_run`, the detail for `format` and `lint` is built from `factory_telemetry::redact_command(&r.command)` for each matching result, joined with `, `. The filter still passes the raw `r.command` to `plan.gate_of`, because the plan keys its gates by the raw text.

Examples, for a format rule:

| Commands | Detail |
|---|---|
| `true --api-key=abc` | `true --api-key=[redacted]` |
| `true --password=p`, `true` | `true --password=[redacted], true` |
| `cargo fmt --check` | `cargo fmt --check` |

`redact_command` joins words with single spaces, so a command written with runs of spaces shows them as one. The detail is still cut to 300 characters by the gate record, after redaction.

### 2. Comment

The comment above the detail line says that commands go through `redact_command`, as in the log.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| A format or lint command with a secret `--name=value` flag | The value shows as `[redacted]`. |
| A command with no sensitive flag | Unchanged, apart from runs of spaces becoming one. |
| Several commands for one gate | Each is redacted, then joined with `, `. |
| No command for the gate | `skipped` "no format command" or "no lint command", unchanged. |
| A secret in another form (`--token abc`, `API_KEY=abc cmd`) | Not redacted, the same as in the log. A non-goal. |
| `gate_record` off | No line is written, unchanged. |

## Testing

In `crates/factory-engine/tests/gate_record.rs`, built like `a_failing_format_command_writes_format_fail` with `with_rule` and `recording_fixture(true)`:

- `a_format_detail_redacts_a_secret_flag`: format command `true --api-key=abc`. The `format` line's detail is exactly `true --api-key=[redacted]` and does not contain `abc`.
- `a_lint_detail_redacts_a_secret_flag`: a rule with lint command `true --token=xyz`. `with_rule` has no lint argument, so set `fx.config.checks[0].lint` after calling it. The `lint` line's detail is exactly `true --token=[redacted]`.
- `a_gate_detail_redacts_each_command`: `with_rule(..., &[], &["true --password=p", "true"])`, with no plain commands, because the plan drops a later command whose raw text it has already planned. The detail is exactly `true --password=[redacted], true`.
- The existing `a_failing_format_command_writes_format_fail` and `no_format_command_writes_skipped` still pass.

Command: `cargo test -j4 -p factory-engine --test gate_record`, then `cargo test -j4 -p factory-engine`.

## Files changed

- `crates/factory-engine/src/checks_stage.rs`
- `crates/factory-engine/tests/gate_record.rs`
- `docs/reference/software-factory.md` (one sentence in the Gate record section)

## Constraints

- Reuse `factory_telemetry::redact_command` unchanged. No new dependency.
- Cargo runs with `-j4 -p factory-engine`, never `--workspace`, `--all` or `CARGO_TARGET_DIR`.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- Comments are short full sentences.
- Board JSON and the gate line format do not change.
- The UI does not change: no file under `dashboard/` is touched, so the card, the gate strip, the list and their styles stay exactly as they are.

## Acceptance criteria

- A `format` detail shows `[redacted]` in place of a sensitive `--name=value` value.
- A `lint` detail does the same.
- A gate with several commands redacts each one and joins them with `, `.
- `plan.gate_of` is still called with the raw command.
- The Gate record section of `docs/reference/software-factory.md` says that format and lint details go through `redact_command`.
- No file under `dashboard/` changes.
- `cargo test -j4 -p factory-engine` passes, with the three new tests.

## Out of scope

- A wider redaction rule.
- Redacting `checks.md` or the park dialog.

## Decisions

- How much should this job do? Options: reuse `redact_command` as is, also widen it, drop commands from the detail. Chosen: reuse it as is. Decided by: user
- Should `checks.md` be redacted too? Options: gate details only, also `checks.md`, file `checks.md` separately. Chosen: gate details only. Decided by: user
- Keep the UI unchanged: no file under `dashboard/` changes. Decided by: user
- The docs get one sentence about the redaction in the Gate record section. Decided by: call
