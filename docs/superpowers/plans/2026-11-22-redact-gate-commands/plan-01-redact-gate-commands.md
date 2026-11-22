# Redact gate commands Implementation Plan

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Run every command shown in a `format` or `lint` gate detail through `redact_command`, so the polled dashboard state never carries a secret flag value.

**Commit:** fix(gates): redact format and lint commands

**Architecture:** `gate_checks_run` in `checks_stage.rs` builds the `format` and `lint` detail from `factory_telemetry::redact_command(&r.command)` instead of the raw command. Matching a command to its gate still uses the raw text. The logic sits in the engine's checks stage, called directly, behind no port.

**Tech Stack:** Rust, factory-engine with its existing factory-telemetry dependency.

> **Spec:** docs/superpowers/specs/2026-11-22-redact-gate-commands-design.md

## Global Constraints

- Reuse `factory_telemetry::redact_command` unchanged. No new dependency.
- No file under `dashboard/` changes. The card, the gate strip, the gate list and their styles stay exactly as they are.
- Board JSON and the gate line format do not change. No field is added to the board, ledger or events.
- Cargo commands are per crate with `-j4`: `cargo test -j4 -p factory-engine --test gate_record`, `cargo test -j4 -p factory-engine`. Never `--workspace`, `--all` or `CARGO_TARGET_DIR`.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- Comments are short full sentences.

## File Structure

- `crates/factory-engine/src/checks_stage.rs`: build the `format` and `lint` detail from redacted commands in `gate_checks_run`.
- `crates/factory-engine/tests/gate_record.rs`: three new tests.
- `docs/reference/software-factory.md`: one sentence in the `## Gate record` section.

## Review Focus

1. The filter must keep passing the raw command to `plan.gate_of`, which looks commands up by exact raw text; a redacted lookup would find no gate and write `skipped`. Pinned by `a_format_detail_redacts_a_secret_flag`, which expects `pass`, not `skipped`.
2. A command with no sensitive flag must read the same as before. Pinned by the existing `a_failing_format_command_writes_format_fail`.
3. Several commands for one gate must each be redacted and keep their plan order. Pinned by `a_gate_detail_redacts_each_command`.
4. No `dashboard/` file may change. Checked by the final review against the diff stat.

---

### Task 1: Redacted format and lint details

**Files:**
- Modify: `crates/factory-engine/src/checks_stage.rs:108-140` (`gate_checks_run`)
- Modify: `docs/reference/software-factory.md` (the `## Gate record` section, near line 389)
- Test: `crates/factory-engine/tests/gate_record.rs` (next to `a_failing_format_command_writes_format_fail` at `:377`)

**Interfaces:**
- Consumes: `factory_telemetry::redact_command(command: &str) -> String`; the test helpers `with_rule(fx: Fixture, commands: &[&str], format: &[&str]) -> Fixture`, `recording_fixture(on: bool) -> Fixture`, `lines_of(fx: &Fixture, gate: GateKey) -> Vec<GateLine>`, `scripted` and `write_effect`, all in `tests/gate_record.rs`.
- Produces: no new public item. `gate_checks_run` keeps its signature.

Tests added: `a_format_detail_redacts_a_secret_flag`, `a_lint_detail_redacts_a_secret_flag`, `a_gate_detail_redacts_each_command`.

- [ ] **Step 1: Read** `crates/factory-engine/src/checks_stage.rs:66-140` (`log_check_results`, `run_checks_now`, `gate_checks_run`), `crates/factory-telemetry/src/redact.rs:1-37`, `crates/factory-checks/src/plan.rs:12-38`, and `crates/factory-engine/tests/gate_record.rs:1-40` and `:337-403`. Then read the spec's Design and edge-case table.

- [ ] **Step 2: Write the failing tests** next to `a_failing_format_command_writes_format_fail`, built the same way:

```rust
#[test]
fn a_format_detail_redacts_a_secret_flag() {
    let fx = with_rule(recording_fixture(true), &["true"], &["true --api-key=abc"]);
    let runner: ScriptedRunner = scripted(vec![write_effect(vec![("a.txt", "x\n")])]);
    fx.engine(&runner).build_stage().expect("build");
    fx.engine(&runner).checks_stage().expect("checks");
    let format = lines_of(&fx, GateKey::Format);
    assert_eq!(format.len(), 1, "{format:?}");
    assert_eq!(format[0].result, GateResult::Pass);
    assert_eq!(format[0].detail, "true --api-key=[redacted]");
    assert!(!format[0].detail.contains("abc"), "{:?}", format[0]);
}
```

The other two share that shape:

| Test | Setup | Gate | Expected result and detail |
|---|---|---|---|
| `a_lint_detail_redacts_a_secret_flag` | `with_rule(recording_fixture(true), &["true"], &[])`, then `fx.config.checks[0].lint = vec!["true --token=xyz".to_string()]` (bind `fx` as `mut`) | `GateKey::Lint` | `GateResult::Pass`, `"true --token=[redacted]"` |
| `a_gate_detail_redacts_each_command` | `with_rule(recording_fixture(true), &[], &["true --password=p", "true"])`, with no plain commands, because the plan drops a later command whose raw text it already planned | `GateKey::Format` | `GateResult::Pass`, `"true --password=[redacted], true"` |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine --test gate_record redacts`. Expected: 3 failed. Each fails on the detail assertion, for example `left: "true --api-key=abc"` against `right: "true --api-key=[redacted]"`.

- [ ] **Step 4: Implement** the change in `gate_checks_run`. Build the detail from `factory_telemetry::redact_command(&r.command)` for each matching result, joined with `", "`, in place of the raw `r.command`. Keep the filter's `plan.gate_of(&r.command)` on the raw command. Put a one-sentence comment above the detail line saying that commands go through `redact_command`, as in the log.

- [ ] **Step 5: Update the docs.** In `docs/reference/software-factory.md`, in the `## Gate record` section, add one sentence saying that the `format` and `lint` details list their commands through `redact_command`, the same rule the `check.run` log uses.

- [ ] **Step 6: Run** `cargo test -j4 -p factory-engine --test gate_record`. Expected: 32 passed, 0 failed (29 existing and 3 new). Then run `cargo test -j4 -p factory-engine`. Expected: all pass.

- [ ] **Step 7: Run** `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings` and `cargo fmt --check`. Expected: no warnings and no formatting changes.

Next plan: none
