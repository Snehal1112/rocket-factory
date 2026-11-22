# Redact the value after a bare sensitive flag

## Goal

Make `redact_command` also hide the value that follows a bare sensitive long flag, so `--token abc` is logged and shown as `--token [redacted]`.

## Overview

`redact_command` hides secrets in check commands before they reach two places: the `check.run` log event and the `format` and `lint` gate details in `gates.jsonl`, which the dashboard sends on every poll. It hides only the `--name=value` form. A secret written as `--token abc`, a form most command-line tools accept, passes through unchanged.

The review of job 104 (`e52ba9e`) found this and filed it as issue #147.

## Current state

- `redact_command` (`crates/factory-telemetry/src/redact.rs:25-37`) splits the command on whitespace. A word `--<name>=<value>` whose name is sensitive becomes `--<name>=[redacted]`; every other word is kept. The words are joined with single spaces.
- `is_sensitive` (`redact.rs:6-14`) is true when the lowercased name contains `token`, `key`, `secret` or `password`, except the name `weighted_tokens`.
- Callers: `log_check_results` (`crates/factory-engine/src/checks_stage.rs:78`) and `gate_checks_run` (`checks_stage.rs:128`).
- Tests: `command_value_of_a_sensitive_flag_is_redacted` and `bare_secret_word_in_a_command_is_kept` in `redact.rs`. The gate tests `a_format_detail_redacts_a_secret_flag`, `a_lint_detail_redacts_a_secret_flag` and `a_gate_detail_redacts_each_command` (`crates/factory-engine/tests/gate_record.rs:389-430`) use the `--name=value` form.
- Docs: `docs/reference/software-factory.md:202` (Redaction) and `:394` (Gate record) say only `--name=value` is redacted.

Gap: `--token abc` keeps `abc`.

## Non-goals

- Short flags such as `-p value` or `-k value`. They have no name to test, and `cargo -p <crate>` would lose its crate name.
- `NAME=value` environment prefixes before the program.
- Any change to `is_sensitive` or to `redact_field`.
- Any change to the dashboard or to `checks.md`.

## Design

```
for each word, left to right:
  "--name=value" and name is sensitive  -> "--name=[redacted]"           (unchanged)
  "--name" (no "=") and name is sensitive
      and a next word exists that does not start with "-"
                                         -> keep "--name", next word -> "[redacted]"
  anything else                          -> kept
words joined with single spaces                                          (unchanged)
```

### 1. Spaced sensitive flags

`redact_command` walks the words in order. A word that starts with `--`, has no `=`, and whose name after `--` passes `is_sensitive` marks the next word as a value. That next word becomes `[redacted]` when it exists and does not start with `-`. A next word that starts with `-` is a flag of its own, so it is kept and checked on its own. A sensitive flag that is the last word has nothing to redact.

The name rule is the same `is_sensitive` check for both forms, so `--name=value` and `--name value` always agree.

| Command | Result |
|---|---|
| `cargo run --token abc --name x` | `cargo run --token [redacted] --name x` |
| `tool --api-key abc --token=xyz` | `tool --api-key [redacted] --token=[redacted]` |
| `tool --token-stdin --check` | `tool --token-stdin --check` |
| `tool --password` | `tool --password` |
| `tool --keyword foo` | `tool --keyword [redacted]` (accepted: `keyword` contains `key`) |
| `tool --weighted_tokens 5` | `tool --weighted_tokens 5` |
| `cargo test -j4 -p factory-engine` | unchanged |
| `tool -- --token abc` | `tool -- --token [redacted]` (a bare `--` has an empty name, which is not sensitive) |

### 2. Doc comment and docs

- The doc comment on `redact_command` names both forms.
- `docs/reference/software-factory.md` Redaction (`:202`) and Gate record (`:394`) say that the value of a sensitive flag is redacted in both the `--name=value` and the `--name value` form.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| Sensitive flag followed by a value | The value becomes `[redacted]`. |
| Sensitive flag followed by another flag | Both are kept, and the second is checked on its own. |
| Sensitive flag as the last word | Kept, nothing redacted. |
| Two sensitive flags in a row, then a value | The first keeps the second (it starts with `-`); the second redacts the value. |
| A non-sensitive name that contains a marker, such as `--keyword foo` | Redacted, an accepted false mask; it never hides less than today. |
| `weighted_tokens` | Not sensitive, so its value is kept, as for fields. |
| Empty command | Empty string, as today. |
| A secret value that itself starts with `-` | Kept, because it reads as a flag. An accepted limit. |
| A value of several words, such as `--key a b` | Only the first word is redacted. An accepted limit. |

## Testing

In `crates/factory-telemetry/src/redact.rs` (unit tests):

- `spaced_value_of_a_sensitive_flag_is_redacted`: `cargo run --token abc --name x` gives `cargo run --token [redacted] --name x`.
- `both_flag_forms_are_redacted_together`: `tool --api-key abc --token=xyz` gives `tool --api-key [redacted] --token=[redacted]`.
- `a_flag_after_a_sensitive_flag_is_kept`: `tool --token-stdin --check` is unchanged.
- `a_sensitive_flag_at_the_end_is_kept`: `tool --password` is unchanged.
- `short_flags_are_never_redacted`: `cargo test -j4 -p factory-engine` is unchanged.
- `weighted_tokens_flag_keeps_its_value`: `tool --weighted_tokens 5` is unchanged.
- The existing `command_value_of_a_sensitive_flag_is_redacted` and `bare_secret_word_in_a_command_is_kept` still pass.

In `crates/factory-engine/tests/gate_record.rs`:

- `a_format_detail_redacts_a_spaced_secret_flag`: format command `true --api-key abc`, built as `a_format_detail_redacts_a_secret_flag` is. The detail is exactly `true --api-key [redacted]`.

Commands:

- `cargo test -j4 -p factory-telemetry`
- `cargo test -j4 -p factory-engine --test gate_record`
- `cargo clippy -j4 -p factory-telemetry --all-targets -- -D warnings`
- `cargo fmt --check`

## Files changed

- `crates/factory-telemetry/src/redact.rs`
- `crates/factory-engine/tests/gate_record.rs`
- `docs/reference/software-factory.md`

## Constraints

- Only `redact_command` changes; `is_sensitive` and `redact_field` do not.
- No new dependency. No `Cargo.toml` change.
- No file under `dashboard/` changes.
- Cargo: `-j4 -p <crate>`, never `--workspace`, `--all` or `CARGO_TARGET_DIR`. Tests use `.expect("...")` and `expect_err`. Comments are short full sentences.

## Acceptance criteria

- A bare sensitive long flag followed by a word that does not start with `-` has that word shown as `[redacted]`.
- A next word that starts with `-` is kept.
- A sensitive flag at the end of the command is kept.
- The `--name=value` form behaves as before.
- Short flags are never redacted.
- The format and lint gate details show the spaced form redacted.
- The doc comment and both docs passages name both forms.
- Every test in Testing passes, with clippy and fmt clean.

## Out of scope

- Short flags and environment prefixes.
- A stricter name rule.

## Decisions

- Which extra forms should `redact_command` hide? Options: spaced long flags, spaced flags plus an env prefix, also short flags. Chosen: spaced long flags (`--token abc`), keeping a next word that starts with `-`; short flags unchanged. Decided by: user
- Which name rule does the spaced form use? Options: the same contains-rule as today, whole name parts only, stricter only for the spaced form. Chosen: the same rule as today. Decided by: user
- A secret that starts with `-`, and the words after the first value word, are not redacted; both are accepted limits. Decided by: call
- A bare `--` has an empty name and is not sensitive, so the words after it are checked as usual. Decided by: call
