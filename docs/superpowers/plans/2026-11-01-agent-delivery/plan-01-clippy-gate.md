# Agent delivery, Plan 01: Clippy gate for the rust preset

> **Spec:** docs/superpowers/specs/2026-11-01-agent-delivery-design.md (section Clippy gate for the rust preset)
>
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** A rust-preset job is gated on `cargo clippy -D warnings` for every crate it changes, and no other preset changes.

**Architecture:** Only `crates/factory-core/presets/rust/config.toml` and the tests and docs that pin its exact command lists change. `plan_checks_in` already expands `{crate}` per owning crate, and `validate_command` accepts a clippy command that has `-j4`.

**Tech Stack:** Rust, `cargo test -j4 -p factory-checks`, `cargo test -j4 -p factory-core`.

## Global Constraints

- Always `-j4` and `-p <crate>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Each task ends with `cargo fmt -p <crate> -- --check` and `cargo clippy -j4 -p <crate> --all-targets -- -D warnings` for every crate it touched. No `#[allow(...)]` to silence a lint.
- No unwrap in production Rust. Tests use `.expect("...")` and `expect_err`.
- Code comments are short full sentences that end with a period.
- Only the rust preset `config.toml` changes among the presets. The go, ruby, csharp and node `config.toml` files stay byte for byte as they are.
- Texts are exact: `cargo clippy -j4 -p {crate} --all-targets -- -D warnings` and `cargo clippy -j4 --all-targets -- -D warnings`. The crate rule order becomes check, test, fmt, clippy, then the dependents test. The root `src/**` rule order becomes check, test, fmt, clippy. The manifest rule stays `cargo check -j4`.

## Review Focus

1. A changed crate path must plan the clippy command for its own crate only, and a workspace without that crate must plan none for it. Pinned by `the_rust_preset_plans_clippy_for_the_changed_crate_only`.
2. A single-crate repo path such as `src/x.rs` must plan the unscoped clippy command. Pinned by `the_rust_preset_checks_a_single_crate_repo`, updated.
3. Both new commands must pass the validator, which needs `-j4` and forbids `--workspace` and `--all`. Pinned by `the_rust_preset_commands_all_validate`.
4. The other four presets must not gain a clippy command. Pinned by `only_the_rust_preset_plans_clippy`.
5. A root manifest change must still plan only `cargo check -j4`. Pinned by the existing `rust_preset_covers_the_root_manifest_and_lockfile`, which must stay green.

## File Structure

| File | Responsibility |
|---|---|
| `crates/factory-core/presets/rust/config.toml` | the two clippy commands |
| `crates/factory-checks/src/plan.rs` | the exact-list tests and the new tests (tests module) |
| `crates/factory-core/src/preset.rs` | the preset shape tests (tests module) |
| `docs/reference/project-toml-reference.md`, `README.md` | the sentences that describe the rust preset rules |

### Task 1: Add the clippy commands and pin them

**Files:**
- Modify: `crates/factory-core/presets/rust/config.toml`, `docs/reference/project-toml-reference.md`, `README.md`
- Test: `crates/factory-checks/src/plan.rs` (tests module), `crates/factory-core/src/preset.rs` (tests module)

**Interfaces:**
- Consumes: `plan_checks_in(changed_paths, cfg, ws)`, `validate_command(cmd)`, and the `ws()` and `rust_preset()` helpers of the `plan.rs` tests module.
- Produces: the rust preset rules described in Global Constraints.

- [ ] **Step 1: Read the code this task touches**

Read the rust preset rules, the `plan.rs` tests that assert the rust preset's exact command lists (`the_rust_preset_checks_a_single_crate_repo` and the tests that list `cargo fmt -p x -- --check`), the preset tests in `preset.rs` (`rust_preset_root_command_equals_the_src_rule_command`, which reads `commands[0]` of the src rule), and the README and reference lines that describe the rust preset's rules.

- [ ] **Step 2: Write the failing tests**

In the `plan.rs` tests module:
- `the_rust_preset_plans_clippy_for_the_changed_crate_only`: with `Some(&ws())`, a change to `crates/app/src/a.rs` plans `cargo clippy -j4 -p app --all-targets -- -D warnings` and no clippy command for any other crate.
- `the_rust_preset_commands_all_validate`: `validate_config(&rust_preset())` is `Ok`.
In `preset.rs` tests: `only_the_rust_preset_plans_clippy`: no `config.toml` of the go, ruby, csharp or node preset contains `clippy`.

- [ ] **Step 3: Run the tests to verify the first fails**

Run: `cargo test -j4 -p factory-checks -- the_rust_preset_plans_clippy_for_the_changed_crate_only`.
Expected: FAIL.

- [ ] **Step 4: Change the preset, the exact-list tests and the docs**

Edit the two rules as in Global Constraints. Update every `plan.rs` and `preset.rs` test that asserts an exact command list for the rust preset, so it includes the clippy commands in the order above. `commands[0]` of the src rule stays `cargo check -j4`. Update the README and reference text that describes the rust preset's rules.

- [ ] **Step 5: Run the tests, fmt and clippy**

Run: `cargo test -j4 -p factory-checks`, `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-engine`, then fmt and clippy for `factory-checks` and `factory-core`.
Expected: all pass, no warnings.

A project whose own `[[settings.checks]]` replaces the preset list, such as this repo's `project.toml`, gets clippy only by adding the commands by hand. After the merge run `factory presets refresh`.

Next plan: docs/superpowers/plans/2026-11-01-agent-delivery/plan-02-agents-file.md
