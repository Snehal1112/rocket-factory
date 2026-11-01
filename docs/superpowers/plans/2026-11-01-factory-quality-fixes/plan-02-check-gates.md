# Factory quality fixes, Plan 02: Check gates

> **Spec:** docs/superpowers/specs/2026-11-01-factory-quality-fixes-design.md (section Plan 2, Compatibility)
>
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** A rust-preset job is gated on `cargo fmt --check` and on the tests of the crates that directly depend on what it changed, without changing what any other preset plans.

**Architecture:** `validate_command` stops demanding `-j4` of `cargo fmt`, which takes no job count. The rust preset appends a fmt command and a `{dependents}` test command to its check rules. `plan_checks_in` already expands `{dependents}` per direct dependent under `dependents_cap`, so the engine needs no change.

**Tech Stack:** Rust, `cargo test -j4 -p factory-checks`, `cargo test -j4 -p factory-core`.

## Global Constraints

- Always `-j4` and `-p <crate>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Each task ends with `cargo fmt -p <crate> -- --check` and `cargo clippy -j4 -p <crate> --all-targets -- -D warnings` for every crate it touched. No `#[allow(...)]`.
- No unwrap in production Rust. Tests use `.expect("...")` and `expect_err`.
- Code comments are short full sentences that end with a period.
- Only `crates/factory-core/presets/rust/config.toml` changes among the presets. The go, ruby, csharp and node `config.toml` files stay as they are, and a plan without a Cargo workspace map plans no `{dependents}` command.
- The `{crate}` and `{dependents}` tokens never share one command, which `validate_config` rejects with `CheckError::CrossProduct`.
- Texts are exact: `cargo fmt -p {crate} -- --check`, `cargo fmt -- --check`, `cargo test -j4 -p {dependents}`.

## Review Focus

1. `cargo fmt --all -- --check` and `cargo fmt --workspace` must stay refused, and every other cargo command must still need `-j4`. Pinned by `cargo_fmt_still_refuses_all_and_workspace` and `other_cargo_commands_still_need_j4`.
2. A change in a crate with no dependents, or with no workspace map, must plan no `{dependents}` command and must not change what it covers. Pinned by `the_rust_preset_plans_no_dependents_test_without_dependents`.
3. A crate with more direct dependents than `dependents_cap` must still get the SKIP note and a capped list. Pinned by `the_rust_preset_dependents_test_respects_the_cap`.
4. A path under `crates/factory-core/presets/` must plan a test for `factory-cli`, where `agent_lint` lives. Pinned by `a_preset_file_change_plans_the_cli_tests`.
5. Config files of other presets must be untouched. Pinned by `only_the_rust_preset_plans_a_fmt_check`.

## File Structure

| File | Responsibility |
|---|---|
| `crates/factory-checks/src/plan.rs` | `validate_command` fmt exception, tests |
| `crates/factory-core/presets/rust/config.toml` | the fmt and dependents commands |
| `crates/factory-core/src/preset.rs` | the preset shape tests (tests module) |
| `docs/reference/project-toml-reference.md`, `README.md` | the rust preset's check rule text |

### Task 1: Let `cargo fmt` pass the command validator

**Files:**
- Modify: `crates/factory-checks/src/plan.rs`
- Test: `crates/factory-checks/src/plan.rs` (tests module)

**Interfaces:**
- Changes: `pub fn validate_command(cmd: &str) -> Result<(), CheckError>` returns `Ok(())` for a cargo command whose second token is `fmt` and that has no `--workspace` and no `--all`. The `-j4` rule applies to every other cargo command as before.

- [ ] **Step 1: Read the code this task touches**

Read `validate_command` and `validate_config` in `plan.rs` and the tests that call them (`CheckError::MissingJobs`, `CheckError::Forbidden`).

- [ ] **Step 2: Write the failing tests**

- `cargo_fmt_needs_no_job_count`: `validate_command("cargo fmt -p core -- --check")` and `validate_command("cargo fmt -- --check")` are `Ok`.
- `cargo_fmt_still_refuses_all_and_workspace`: `validate_command("cargo fmt --all -- --check")` is `Err(CheckError::Forbidden { flag: "--all", .. })`, and the same for `--workspace`.
- `other_cargo_commands_still_need_j4`: `validate_command("cargo check -p core")` is `Err(CheckError::MissingJobs(_))` and `validate_command("cargo clippy -p core")` too.

- [ ] **Step 3: Run the tests to verify the first fails**

Run: `cargo test -j4 -p factory-checks -- cargo_fmt_needs_no_job_count`.
Expected: FAIL (`MissingJobs`).

- [ ] **Step 4: Implement the exception**

In `validate_command`, keep the `--workspace` and `--all` check first, then return `Ok(())` when `tokens.get(1) == Some(&"fmt")`, before the `-j4` check. Add a doc sentence saying `cargo fmt` takes no job count.

- [ ] **Step 5: Run the tests, fmt and clippy**

Run: `cargo test -j4 -p factory-checks`, then fmt and clippy for `factory-checks`.
Expected: all pass, no warnings.

### Task 2: Add the fmt and dependents commands to the rust preset

**Files:**
- Modify: `crates/factory-core/presets/rust/config.toml`, `docs/reference/project-toml-reference.md`, `README.md`
- Test: `crates/factory-checks/src/plan.rs` (tests module), `crates/factory-core/src/preset.rs` (tests module)

**Interfaces:**
- Consumes: `validate_command` from the previous task, `plan_checks_in(changed_paths, cfg, ws)` and the `ws()` and `rust_preset()` helpers of the `plan.rs` tests module.
- Produces: the rust preset rule `crates/*/**` with the commands `cargo check -j4 -p {crate}`, `cargo test -j4 -p {crate}`, `cargo fmt -p {crate} -- --check`, `cargo test -j4 -p {dependents}` in that order, and the root `src/**` rule with `cargo check -j4`, `cargo test -j4`, `cargo fmt -- --check`. The manifest rule stays `cargo check -j4`.

- [ ] **Step 1: Read the code this task touches**

Read the rust preset rules, the preset tests (`rust_preset_root_command_equals_the_src_rule_command`, which reads `commands[0]` of the src rule), the `plan.rs` tests that assert the rust preset's exact command lists (`the_rust_preset_checks_a_single_crate_repo` and the tests near it), the `ws()` helper (crates `core` and `app`, where `app` depends on `core`), and the README and reference lines that describe the rust preset's two rules.

- [ ] **Step 2: Write the failing tests**

In the `plan.rs` tests module, using `plan_checks_in` with `Some(&ws())`:
- `the_rust_preset_plans_no_dependents_test_without_dependents`: a change to `crates/app/src/a.rs` plans `cargo fmt -p app -- --check` and no command with `-p` of another crate.
- `the_rust_preset_dependents_test_respects_the_cap`: with `dependents_cap` set to 0 on the rust preset config, a change to `crates/core/src/a.rs` plans no `-p app` test and the plan notes start with `SKIP dependents over the cap: app`.
- `a_preset_file_change_plans_the_cli_tests`: with a workspace map where package `factory-cli` depends on `factory-core`, a change to `crates/factory-core/presets/rust/agents/factory-builder.md` plans `cargo test -j4 -p factory-cli`.
In `preset.rs` tests: `only_the_rust_preset_plans_a_fmt_check`: no `config.toml` of the go, ruby, csharp or node preset contains `fmt`.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -j4 -p factory-checks -- the_rust_preset_plans_no_dependents_test_without_dependents a_preset_file_change_plans_the_cli_tests`.
Expected: FAIL.

- [ ] **Step 4: Change the preset and update the exact-list tests**

Edit the two rules as in the Interfaces block. Update every `plan.rs` test and the one `preset.rs` test that asserts an exact command list for the rust preset, so they include the new commands in the same order. `commands[0]` of the src rule stays `cargo check -j4`. Update the README and reference text that describes the rust preset's rules.

- [ ] **Step 5: Run the tests, fmt and clippy**

Run: `cargo test -j4 -p factory-checks`, `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-engine`, then fmt and clippy for `factory-checks` and `factory-core`.
Expected: all pass, no warnings.

After the merge, run `factory presets refresh`. A project whose own `[[settings.checks]]` replaces the preset list, such as this repo's `project.toml`, needs the same three commands added by hand.

Next plan: docs/superpowers/plans/2026-11-01-factory-quality-fixes/plan-03-builder-context.md
