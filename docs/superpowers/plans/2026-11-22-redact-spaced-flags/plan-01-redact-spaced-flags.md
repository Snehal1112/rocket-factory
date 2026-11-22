# Redact spaced sensitive flags Implementation Plan

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Make `redact_command` also hide the value that follows a bare sensitive long flag, so `--token abc` is logged and shown as `--token [redacted]`.

**Commit:** fix(telemetry): redact spaced sensitive flags

**Architecture:** `redact_command` in factory-telemetry walks the command words in order. A bare `--name` whose name passes the existing `is_sensitive` check marks the next word as a value, and that word is redacted unless it starts with `-`. The `check.run` log and the format and lint gate details already call it, so nothing else changes in code. factory-engine is touched only by a test; no port is involved.

**Tech Stack:** Rust, factory-telemetry with no new dependency.

> **Spec:** docs/superpowers/specs/2026-11-22-redact-spaced-flags-design.md

## Global Constraints

- Only `redact_command` changes in code; `is_sensitive` and `redact_field` do not.
- The redaction text is `[redacted]` (the existing `REDACTED` constant). Words are joined with single spaces, as today.
- No new dependency, no `Cargo.toml` change, and no file under `dashboard/` changes.
- Cargo: `-j4 -p <crate>`, never `--workspace`, `--all` or `CARGO_TARGET_DIR`. Tests use `.expect("...")` and `expect_err`. Comments are short full sentences.

## File Structure

- `crates/factory-telemetry/src/redact.rs`: the spaced-flag rule in `redact_command`, its doc comment, and six unit tests.
- `crates/factory-engine/tests/gate_record.rs`: one gate test.
- `docs/reference/software-factory.md`: the Redaction and Gate record passages.

## Review Focus

1. Two sensitive flags in a row, then a value (`tool --token --api-key abc`): the first flag keeps the second, and the second redacts `abc`. Expected `tool --token --api-key [redacted]`. Pinned by `two_sensitive_flags_redact_only_the_value`.
2. A bare `--` before the flags (`tool -- --token abc`) must not count as sensitive. Expected `tool -- --token [redacted]`. Pinned by `a_bare_double_dash_is_not_sensitive`.
3. Short flags must never be touched, or every `cargo -p <crate>` loses its crate name. Pinned by `short_flags_are_never_redacted`.
4. The `--name=value` form must not change. Pinned by the existing `command_value_of_a_sensitive_flag_is_redacted` and the three gate tests.

---

### Task 1: Spaced sensitive flags in `redact_command`

**Files:**
- Modify: `crates/factory-telemetry/src/redact.rs:25-37` (`redact_command` and its doc comment)
- Modify: `docs/reference/software-factory.md` (Redaction near line 202, Gate record near line 394)
- Test: `crates/factory-telemetry/src/redact.rs` (`mod tests`), `crates/factory-engine/tests/gate_record.rs` (next to `a_format_detail_redacts_a_secret_flag` at `:389`)

**Interfaces:**
- Consumes: `is_sensitive(name: &str) -> bool` and `REDACTED` in `redact.rs`; the gate test helpers `with_rule`, `recording_fixture`, `lines_of`, `scripted` and `write_effect` in `tests/gate_record.rs`.
- Produces: no new item. `pub fn redact_command(command: &str) -> String` keeps its signature.

Tests added: `spaced_value_of_a_sensitive_flag_is_redacted`, `both_flag_forms_are_redacted_together`, `a_flag_after_a_sensitive_flag_is_kept`, `a_sensitive_flag_at_the_end_is_kept`, `short_flags_are_never_redacted`, `weighted_tokens_flag_keeps_its_value`, `two_sensitive_flags_redact_only_the_value`, `a_bare_double_dash_is_not_sensitive`, `a_format_detail_redacts_a_spaced_secret_flag`.

- [ ] **Step 1: Read** `crates/factory-telemetry/src/redact.rs` in full, `crates/factory-engine/src/checks_stage.rs:66-140`, `crates/factory-engine/tests/gate_record.rs:337-430`, `docs/reference/software-factory.md:198-204` and `:392-396`, and the spec's Design table and edge-case table.

- [ ] **Step 2: Write the failing tests.** In `redact.rs`'s `mod tests`:

```rust
#[test]
fn spaced_value_of_a_sensitive_flag_is_redacted() {
    assert_eq!(
        redact_command("cargo run --token abc --name x"),
        "cargo run --token [redacted] --name x"
    );
}
```

The others have the same one-assertion shape:

| Test | Input | Expected |
|---|---|---|
| `both_flag_forms_are_redacted_together` | `tool --api-key abc --token=xyz` | `tool --api-key [redacted] --token=[redacted]` |
| `a_flag_after_a_sensitive_flag_is_kept` | `tool --token-stdin --check` | `tool --token-stdin --check` |
| `a_sensitive_flag_at_the_end_is_kept` | `tool --password` | `tool --password` |
| `short_flags_are_never_redacted` | `cargo test -j4 -p factory-engine` | `cargo test -j4 -p factory-engine` |
| `weighted_tokens_flag_keeps_its_value` | `tool --weighted_tokens 5` | `tool --weighted_tokens 5` |
| `two_sensitive_flags_redact_only_the_value` | `tool --token --api-key abc` | `tool --token --api-key [redacted]` |
| `a_bare_double_dash_is_not_sensitive` | `tool -- --token abc` | `tool -- --token [redacted]` |

In `tests/gate_record.rs`, built exactly as `a_format_detail_redacts_a_secret_flag` is, with the format command `true --api-key abc`:

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

- [ ] **Step 3: Run** `cargo test -j4 -p factory-telemetry redact`. Expected: 4 failed (`spaced_value_of_a_sensitive_flag_is_redacted`, `both_flag_forms_are_redacted_together`, `two_sensitive_flags_redact_only_the_value`, `a_bare_double_dash_is_not_sensitive`), each with `left` still holding the raw value. The other four new tests pass already. Then run `cargo test -j4 -p factory-engine --test gate_record spaced`. Expected: 1 failed, the detail reading `true --api-key abc`.

- [ ] **Step 4: Implement** the rule in `redact_command`, as the spec's Design 1 says.
  - Walk the whitespace-split words with a flag that marks the next word as a value.
  - A word with the `--` prefix and no `=`, whose name passes `is_sensitive`, sets the mark.
  - A marked word that does not start with `-` becomes `REDACTED`. A marked word that starts with `-` is kept and checked as a word of its own.
  - The `--name=value` branch stays as it is.
  - Rewrite the doc comment so it names both the `--name=value` and the `--name value` form.

- [ ] **Step 5: Update the docs.** In `docs/reference/software-factory.md`, change the Redaction sentence (near line 202) and the last sentence of the Gate record fields paragraph (near line 394). Each should say that the value of a sensitive flag is redacted in both the `--name=value` and the `--name value` form.

- [ ] **Step 6: Run** `cargo test -j4 -p factory-telemetry`. Expected: all pass, the 7 existing tests and the 8 new ones. Then run `cargo test -j4 -p factory-engine --test gate_record`. Expected: 34 passed, 0 failed (33 existing and 1 new).

- [ ] **Step 7: Run** `cargo clippy -j4 -p factory-telemetry -p factory-engine --all-targets -- -D warnings` and `cargo fmt --check`. Expected: no warnings and no formatting changes.

Next plan: none
