# Redact spaced sensitive flags Implementation Plan

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Make `redact_command` also hide the value that follows a bare sensitive long flag, and widen the sensitive names, so `--token abc` and `--auth abc` are logged and shown as `[redacted]`.

**Commit:** fix(telemetry): redact spaced sensitive flags

**Architecture:** `is_sensitive` in factory-telemetry gains six contained markers and a whole-part rule for `pat` and `pass`. `redact_command` walks the command words in order, and a bare sensitive `--name` hides the next word. The `check.run` log, the log fields and the format and lint gate details already call these, so nothing else changes in code. factory-engine is touched only by a test; no port is involved.

**Tech Stack:** Rust, factory-telemetry with no new dependency.

> **Spec:** docs/superpowers/specs/2026-11-22-redact-spaced-flags-design.md

## Global Constraints

- Contained markers: `token`, `key`, `secret`, `password`, `passwd`, `passphrase`, `credential`, `auth`, `bearer`, `cookie`. Whole name parts (split on `-` and `_`): `pat`, `pass`. Exception: `weighted_tokens`. Names are lowercased first.
- The redaction text is `[redacted]` (the existing `REDACTED` constant). Words are joined with single spaces, as today.
- No new dependency, no `Cargo.toml` change, and no file under `dashboard/` changes.
- Cargo: `-j4 -p <crate>`, never `--workspace`, `--all` or `CARGO_TARGET_DIR`. Tests use `.expect("...")` and `expect_err`. Comments are short full sentences.

## File Structure

- `crates/factory-telemetry/src/redact.rs`: the wider `is_sensitive`, the spaced-flag rule in `redact_command`, their doc comments, and eleven unit tests.
- `crates/factory-engine/tests/gate_record.rs`: one gate test.
- `docs/reference/software-factory.md`: the Redaction and Gate record passages.

## Review Focus

1. `pat` and `pass` as substrings would hide `path`, `--manifest-path` and the `passed` field of every `check.run` event. Expected: those stay readable. Pinned by `names_that_only_contain_pat_or_pass_are_not_sensitive` and `short_flags_and_paths_are_never_redacted`.
2. A value after two bare sensitive flags (`tool --token --api-key abc`) must stay hidden. Expected `tool --token [redacted] [redacted]`. Pinned by `two_sensitive_flags_hide_the_value`.
3. A bare `--` must not count as sensitive. Expected `tool -- --token [redacted]`. Pinned by `a_bare_double_dash_is_not_sensitive`.
4. The `--name=value` form and `weighted_tokens` must not change. Pinned by the existing `command_value_of_a_sensitive_flag_is_redacted`, `weighted_tokens_is_the_one_field_not_redacted` and the three gate tests.

---

### Task 1: Wider names and spaced flags in redaction

**Files:**
- Modify: `crates/factory-telemetry/src/redact.rs:6-37` (`is_sensitive`, `redact_command` and their doc comments)
- Modify: `docs/reference/software-factory.md` (Redaction near line 202, Gate record near line 394)
- Test: `crates/factory-telemetry/src/redact.rs` (`mod tests`), `crates/factory-engine/tests/gate_record.rs` (next to `a_format_detail_redacts_a_secret_flag` at `:389`)

**Interfaces:**
- Consumes: `REDACTED` in `redact.rs`; the gate test helpers `with_rule`, `recording_fixture`, `lines_of`, `scripted` and `write_effect` in `tests/gate_record.rs`.
- Produces: no new item. `fn is_sensitive(name: &str) -> bool`, `pub fn redact_field(name: &str, value: Value) -> Value` and `pub fn redact_command(command: &str) -> String` keep their signatures.

Tests added: `spaced_value_of_a_sensitive_flag_is_redacted`, `both_flag_forms_are_redacted_together`, `a_value_starting_with_a_dash_is_redacted`, `a_flag_after_a_sensitive_flag_is_hidden`, `two_sensitive_flags_hide_the_value`, `a_sensitive_flag_at_the_end_is_kept`, `short_flags_and_paths_are_never_redacted`, `a_bare_double_dash_is_not_sensitive`, `new_names_are_sensitive`, `names_that_only_contain_pat_or_pass_are_not_sensitive`, `new_field_names_are_redacted`, `a_format_detail_redacts_a_spaced_secret_flag`.

- [ ] **Step 1: Read** `crates/factory-telemetry/src/redact.rs` in full, `crates/factory-telemetry/src/layer.rs:140-150`, `crates/factory-engine/src/checks_stage.rs:66-140`, `crates/factory-engine/tests/gate_record.rs:337-430`, `docs/reference/software-factory.md:198-204` and `:392-396`, and the spec's Design tables and edge-case table.

- [ ] **Step 2: Write the failing tests.** In `redact.rs`'s `mod tests`:

```rust
#[test]
fn spaced_value_of_a_sensitive_flag_is_redacted() {
    assert_eq!(
        redact_command("cargo run --token abc --name x"),
        "cargo run --token [redacted] --name x"
    );
}

#[test]
fn new_names_are_sensitive() {
    for name in ["auth", "authorization", "bearer", "credentials", "passwd", "passphrase", "cookie", "pat", "github_pat", "pass", "db-pass"] {
        assert_eq!(
            redact_command(&format!("tool --{name} v")),
            format!("tool --{name} [redacted]"),
            "{name}"
        );
    }
}

#[test]
fn names_that_only_contain_pat_or_pass_are_not_sensitive() {
    for name in ["path", "manifest_path", "passed", "bypass", "patch", "session_id"] {
        assert_eq!(redact_field(name, json!("v")), json!("v"), "{name}");
    }
}
```

The other command tests have the one-assertion shape of the first:

| Test | Input | Expected |
|---|---|---|
| `both_flag_forms_are_redacted_together` | `tool --api-key abc --token=xyz` | `tool --api-key [redacted] --token=[redacted]` |
| `a_value_starting_with_a_dash_is_redacted` | `tool --token -abc123` | `tool --token [redacted]` |
| `a_flag_after_a_sensitive_flag_is_hidden` | `tool --token-stdin --check` | `tool --token-stdin [redacted]` |
| `two_sensitive_flags_hide_the_value` | `tool --token --api-key abc` | `tool --token [redacted] [redacted]` |
| `a_sensitive_flag_at_the_end_is_kept` | `tool --password` | `tool --password` |
| `short_flags_and_paths_are_never_redacted` | `cargo test -j4 -p factory-engine --manifest-path a/Cargo.toml` | the same text |
| `a_bare_double_dash_is_not_sensitive` | `tool -- --token abc` | `tool -- --token [redacted]` |

`new_field_names_are_redacted` has the loop shape of `names_that_only_contain_pat_or_pass_are_not_sensitive`, over `auth_header`, `bearer`, `db_pass` and `github_pat`, expecting `json!("[redacted]")`.

In `tests/gate_record.rs`, built exactly as `a_format_detail_redacts_a_secret_flag` is:

```rust
#[test]
fn a_format_detail_redacts_a_spaced_secret_flag() {
    let fx = with_rule(recording_fixture(true), &["true"], &["true --api-key abc"]);
    let runner: ScriptedRunner = scripted(vec![write_effect(vec![("a.txt", "x\n")])]);
    fx.engine(&runner).build_stage().expect("build");
    fx.engine(&runner).checks_stage().expect("checks");
    let format = lines_of(&fx, GateKey::Format);
    assert_eq!(format.len(), 1, "{format:?}");
    assert_eq!(format[0].result, GateResult::Pass);
    assert_eq!(format[0].detail, "true --api-key [redacted]");
}
```

- [ ] **Step 3: Run** `cargo test -j4 -p factory-telemetry redact`. Expected: these fail, each with `left` still holding the raw value:
  - `spaced_value_of_a_sensitive_flag_is_redacted`
  - `both_flag_forms_are_redacted_together`
  - `a_value_starting_with_a_dash_is_redacted`
  - `a_flag_after_a_sensitive_flag_is_hidden`
  - `two_sensitive_flags_hide_the_value`
  - `a_bare_double_dash_is_not_sensitive`
  - `new_names_are_sensitive`
  - `new_field_names_are_redacted`

  `a_sensitive_flag_at_the_end_is_kept`, `short_flags_and_paths_are_never_redacted` and `names_that_only_contain_pat_or_pass_are_not_sensitive` pass already.

  Then run `cargo test -j4 -p factory-engine --test gate_record spaced`. Expected: 1 failed, the detail reading `true --api-key abc`.

- [ ] **Step 4: Implement** as the spec's Design 1 and 2 say.
  - `is_sensitive` lowercases the name and returns false for `weighted_tokens`. Otherwise it is true when the name contains a marker, or when a part of `name.split(['-', '_'])` equals `pat` or `pass`.
  - `redact_command` walks the words with a flag that marks the next word.
    - A marked word becomes `REDACTED`. If it is itself a bare sensitive flag, it marks the next word too.
    - Otherwise a word with the `--` prefix and no `=`, whose name is sensitive, is kept and sets the mark.
    - The `--name=value` branch stays as it is.
  - Rewrite both doc comments to state the name rule and both flag forms.

- [ ] **Step 5: Update the docs.** In `docs/reference/software-factory.md`, rewrite the Redaction sentence (near line 202) to list the ten markers, the whole-part rule for `pat` and `pass`, and both flag forms. Change the last sentence of the Gate record fields paragraph (near line 394) to say that both the `--name=value` and the `--name value` form are redacted.

- [ ] **Step 6: Run** `cargo test -j4 -p factory-telemetry`. Expected: all pass, the 7 existing tests and the 11 new ones. Then run `cargo test -j4 -p factory-engine`. Expected: all pass, `gate_record` with 34 tests (33 existing and 1 new).

- [ ] **Step 7: Run** `cargo clippy -j4 -p factory-telemetry -p factory-engine --all-targets -- -D warnings` and `cargo fmt --check`. Expected: no warnings and no formatting changes.

Next plan: none
