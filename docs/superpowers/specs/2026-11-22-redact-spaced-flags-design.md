# Redact the value after a bare sensitive flag

## Goal

Make `redact_command` also hide the value that follows a bare sensitive long flag, and widen the sensitive names, so `--token abc` and `--auth abc` are logged and shown as `[redacted]`.

## Overview

`redact_command` hides secrets in check commands before they reach two places: the `check.run` log event and the `format` and `lint` gate details in `gates.jsonl`, which the dashboard sends on every poll. It hides only the `--name=value` form. A secret written as `--token abc`, a form most command-line tools accept, passes through unchanged. The review of job 104 (`e52ba9e`) found this and filed it as issue #147.

The spec review of the first version of this design found a second gap: the sensitive-name check knows only `token`, `key`, `secret` and `password`, so `--auth abc`, `--bearer abc`, `--pat abc`, `--pass x`, `--passwd x` and `--credentials x` keep their values in either form.

## Current state

- `redact_command` (`crates/factory-telemetry/src/redact.rs:25-37`) splits the command on whitespace. A word `--<name>=<value>` whose name is sensitive becomes `--<name>=[redacted]`; every other word is kept. The words are joined with single spaces.
- `is_sensitive` (`redact.rs:6-14`) is true when the lowercased name contains `token`, `key`, `secret` or `password`, except the name `weighted_tokens`.
- `redact_field` (`redact.rs:16-23`) uses `is_sensitive` for log field names; the log layer calls it for every field (`crates/factory-telemetry/src/layer.rs:147`).
- Callers of `redact_command`: `log_check_results` (`crates/factory-engine/src/checks_stage.rs:78`) and `gate_checks_run` (`checks_stage.rs:128`).
- Tests: `sensitive_field_names_are_redacted`, `redaction_is_case_insensitive`, `weighted_tokens_is_the_one_field_not_redacted`, `command_value_of_a_sensitive_flag_is_redacted` and `bare_secret_word_in_a_command_is_kept` in `redact.rs`. The gate tests `a_format_detail_redacts_a_secret_flag`, `a_lint_detail_redacts_a_secret_flag` and `a_gate_detail_redacts_each_command` (`crates/factory-engine/tests/gate_record.rs:389-425`) use the `--name=value` form.
- Docs: `docs/reference/software-factory.md:202` (Redaction) and `:394` (Gate record) name only the four words and the `--name=value` form.

Gaps: `--token abc` keeps `abc`, and the common names above are not sensitive at all.

## Non-goals

- Short flags such as `-p value` or `-k value`. They have no name to test, and `cargo -p <crate>` would lose its crate name.
- `NAME=value` environment prefixes before the program.
- Hiding more than one word after a flag. A shell passes a quoted secret with spaces as one word.
- Any change to the dashboard or to `checks.md`.

## Design

```
is_sensitive(name):  lowercase; false for "weighted_tokens"
  true when the name contains token, key, secret, password, passwd, passphrase, credential, auth, bearer or cookie
  true when a whole part of the name (split on "-" and "_") is pat or pass
redact_command, for each word left to right:
  word is marked                         -> "[redacted]" (and, if the word is itself a bare
                                            sensitive flag, the next word is marked too)
  "--name=value", name sensitive         -> "--name=[redacted]"            (unchanged)
  "--name" (no "="), name sensitive      -> kept, the next word is marked
  anything else                          -> kept
words joined with single spaces                                            (unchanged)
```

### 1. Wider sensitive names

`is_sensitive` keeps its `weighted_tokens` exception and becomes true when either holds:

- The lowercased name contains `token`, `key`, `secret`, `password`, `passwd`, `passphrase`, `credential`, `auth`, `bearer` or `cookie`.
- One whole part of the name, split on `-` and `_`, is `pat` or `pass`. These two are whole parts only, because as substrings they would hide `path`, `manifest-path` and the `passed` field of the `check.run` event.

The rule covers both flag forms and log field names, since all three go through `is_sensitive`.

| Name | Sensitive |
|---|---|
| `api-key`, `token`, `password` | yes, as today |
| `auth`, `authorization`, `bearer`, `credentials`, `passwd`, `passphrase`, `cookie` | yes, new |
| `pat`, `github_pat`, `pass`, `db-pass` | yes, new (whole part) |
| `path`, `manifest-path`, `passed`, `bypass`, `patch` | no |
| `session`, `session_id` | no; `session_id` is an agent session id the logs keep on purpose |
| `dbpass`, `mypat` | no, a known leak of the whole-part rule |
| `weighted_tokens` | no |

### 2. Spaced sensitive flags

`redact_command` walks the words in order. A word that starts with `--`, has no `=`, and whose name after `--` is sensitive marks the next word. A marked word always becomes `[redacted]`, whatever it starts with, so `--token -abc123` hides `-abc123`. If the marked word is itself a bare sensitive flag, the word after it is marked too, so a value after two such flags is still hidden. A sensitive flag that is the last word has nothing to redact. A bare `--` has an empty name, which is not sensitive.

| Command | Result |
|---|---|
| `cargo run --token abc --name x` | `cargo run --token [redacted] --name x` |
| `tool --api-key abc --token=xyz` | `tool --api-key [redacted] --token=[redacted]` |
| `tool --token -abc123` | `tool --token [redacted]` |
| `tool --token-stdin --check` | `tool --token-stdin [redacted]` (a harmless false mask) |
| `tool --token --api-key abc` | `tool --token [redacted] [redacted]` |
| `tool --auth abc --pat xyz` | `tool --auth [redacted] --pat [redacted]` |
| `tool --password` | `tool --password` |
| `tool --weighted_tokens 5` | `tool --weighted_tokens 5` |
| `cargo test -j4 -p factory-engine --manifest-path a/Cargo.toml` | unchanged |
| `tool -- --token abc` | `tool -- --token [redacted]` |

### 3. Doc comments and docs

- The doc comments on `is_sensitive` and `redact_command` state the name rule and both flag forms.
- `docs/reference/software-factory.md` Redaction (`:202`) lists the new names, the whole-part rule for `pat` and `pass`, and both flag forms. Gate record (`:394`) says both forms are redacted.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| Sensitive flag followed by a value | The value becomes `[redacted]`. |
| Value that starts with `-` | Hidden. |
| Sensitive boolean flag followed by another flag | The next flag is hidden too, a harmless false mask. |
| Two bare sensitive flags, then a value | Both following words are hidden. |
| Sensitive flag as the last word | Kept, nothing redacted. |
| A name that contains a marker, such as `--keyword foo` | Hidden, an accepted false mask. |
| `path`, `passed` and other names that only contain `pat` or `pass` | Not sensitive. |
| A bare `--` | Not sensitive; the words after it are checked as usual. |
| Empty command | Empty string, as today. |

## Testing

In `crates/factory-telemetry/src/redact.rs` (unit tests), each a single `assert_eq!` on the exact output:

- `spaced_value_of_a_sensitive_flag_is_redacted`: `cargo run --token abc --name x` gives `cargo run --token [redacted] --name x`.
- `both_flag_forms_are_redacted_together`: the second Design row.
- `a_value_starting_with_a_dash_is_redacted`: `tool --token -abc123` gives `tool --token [redacted]`.
- `a_flag_after_a_sensitive_flag_is_hidden`: `tool --token-stdin --check` gives `tool --token-stdin [redacted]`.
- `two_sensitive_flags_hide_the_value`: `tool --token --api-key abc` gives `tool --token [redacted] [redacted]`.
- `a_sensitive_flag_at_the_end_is_kept`: `tool --password` is unchanged.
- `short_flags_and_paths_are_never_redacted`: `cargo test -j4 -p factory-engine --manifest-path a/Cargo.toml` is unchanged.
- `a_bare_double_dash_is_not_sensitive`: `tool -- --token abc` gives `tool -- --token [redacted]`.
- `new_names_are_sensitive`: for each of `auth`, `authorization`, `bearer`, `credentials`, `passwd`, `passphrase`, `cookie`, `pat`, `github_pat`, `pass` and `db-pass`, `redact_command("tool --<name> v")` is `tool --<name> [redacted]`.
- `names_that_only_contain_pat_or_pass_are_not_sensitive`: `redact_field` keeps the value for `path`, `manifest_path`, `passed`, `bypass`, `patch` and `session_id`.
- `new_field_names_are_redacted`: `redact_field` gives `[redacted]` for `auth_header`, `bearer`, `db_pass` and `github_pat`.
- The existing five tests still pass, including `weighted_tokens_is_the_one_field_not_redacted`.

In `crates/factory-engine/tests/gate_record.rs`:

- `a_format_detail_redacts_a_spaced_secret_flag`: format command `true --api-key abc`, built as `a_format_detail_redacts_a_secret_flag` is. The detail is exactly `true --api-key [redacted]`.

Commands:

- `cargo test -j4 -p factory-telemetry`
- `cargo test -j4 -p factory-engine`
- `cargo clippy -j4 -p factory-telemetry -p factory-engine --all-targets -- -D warnings`
- `cargo fmt --check`

## Files changed

- `crates/factory-telemetry/src/redact.rs`
- `crates/factory-engine/tests/gate_record.rs`
- `docs/reference/software-factory.md`

## Constraints

- The redaction text is `[redacted]`, the existing `REDACTED` constant.
- No new dependency, no `Cargo.toml` change, and no file under `dashboard/` changes.
- Cargo: `-j4 -p <crate>`, never `--workspace`, `--all` or `CARGO_TARGET_DIR`. Tests use `.expect("...")` and `expect_err`. Comments are short full sentences.

## Acceptance criteria

- `is_sensitive` is true for the ten contained markers and for `pat` or `pass` as a whole name part, and false for `weighted_tokens`, `path`, `passed` and `session_id`.
- The word after a bare sensitive long flag is always shown as `[redacted]`, including one that starts with `-`.
- A hidden word that is itself a bare sensitive flag also hides the word after it.
- A sensitive flag at the end of the command is kept, and a bare `--` is not sensitive.
- The `--name=value` form behaves as before for the old names and also covers the new ones.
- Short flags are never redacted.
- The format and lint gate details show the spaced form redacted.
- The doc comments and both docs passages state the name rule and both forms.
- Every test in Testing passes, with clippy and fmt clean.

## Out of scope

- Short flags and environment prefixes.
- A default-deny rule with a list of safe flags.
- `session`, `signature` and `cert`, which stay known risks.

## Decisions

- Which extra forms should `redact_command` hide? Options: spaced long flags, spaced flags plus an env prefix, also short flags. Chosen: spaced long flags (`--token abc`); short flags unchanged. Decided by: user
- Which name rule does the spaced form use? Options: the same rule for both forms, whole name parts only, stricter only for the spaced form. Chosen: one rule shared by both forms. Decided by: user
- Which sensitive names, after the spec review found `--auth`, `--bearer`, `--pat`, `--pass` and `--passwd` leaking? Options: a wider name list, default deny with a safe list, accept the leaks. Chosen: a wider name list (auth, bearer, pat, pass, passwd, credential added), for both flag forms and log fields. Decided by: user
- What happens to the word after a bare sensitive flag? Options: always hide the next word, keep words that start with `-` as flags, hide every word up to the next flag. Chosen: always hide the next word, one word only. Decided by: user
- Is a bare `--` sensitive? Options: not sensitive (the words after it are checked as usual), hide everything after it. Chosen: not sensitive. Decided by: user
- How do `pat` and `pass` match? Options: whole name parts plus a `passphrase` marker, plain substrings, whole parts only. Chosen: whole name parts, with `passphrase` added as a marker; `--dbpass` and `--mypat` stay a known leak. Decided by: user
- Is the value after two bare sensitive flags in a row hidden? Options: hide it too, hide one word only. Chosen: hide it too. Decided by: user
- Add more names? Options: cookie and session, all four (cookie, session, signature, cert), none. Chosen: cookie and session. Decided by: user
- How is `session` handled, given it would hide the `session_id` field of every `agent.end` event? Options: drop session and keep cookie, session for flags only, hide session everywhere. Chosen: drop session, keep cookie. Decided by: user
