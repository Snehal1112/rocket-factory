# Check coverage defaults Implementation Plan

> **Spec:** docs/superpowers/specs/2026-10-31-check-coverage-defaults-design.md
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** The rust preset covers the root `Cargo.toml` and `Cargo.lock` and exempts generic non-code files, and the upgrade steps are documented.

**Architecture:** Two new entries in `crates/factory-core/presets/rust/config.toml` (a `[[checks]]` rule and a top-level `unchecked_paths`), pinned by tests that parse the embedded preset and call `gate::uncovered_code_paths`. Doc edits are pinned by one new `factory-cli` test file that reads the docs.

**Tech Stack:** Rust, TOML presets, Markdown docs.

## Global Constraints

- No `--workspace` or `--all` in any cargo command, and `-j4` on every cargo command.
- No new `Config` field: use `unchecked_paths` and `[[checks]]`, which exist (`crates/factory-core/src/config.rs:67`).
- The preset must still parse with `Config::parse` and must not mention Rocket (`preset.rs` test banning the word).
- Root rule: `paths = ["Cargo.toml", "Cargo.lock"]`, `commands = ["cargo check -j4"]`.
- `unchecked_paths = ["LICENSE", "LICENSE-*", "LICENSE.txt", "COPYING", "COPYING.*", ".gitignore", ".gitattributes", ".editorconfig"]`.
- Not exempt and not covered by the preset: `dashboard/Cargo.toml`, `rustfmt.toml`, `rust-toolchain.toml`, `scripts/release.sh`, `cliff.toml`, and code files named like a license such as `xtask/src/license.rs` and `tools/licenses.rs`. The exempt names have no `**/` prefix and no trailing `*` after `LICENSE` or `COPYING`, so they match only at the repo root.
- No edit of any file under `~/.factory`, no change to `cliff.toml`, no change to the other presets.

## Review Focus

1. `Cargo.toml` and `build.rs` in one diff: the same `cargo check -j4` must come out once. Pinned by `rust_preset_root_command_equals_the_src_rule_command` (Task 1), because the planner drops duplicate commands only when the strings are identical.
2. A nested manifest such as `crates/factory-core/Cargo.toml`: must stay under the per-crate rule and not be reported. Pinned by `rust_preset_nested_crate_manifest_is_covered` (Task 1).
3. A lowercase `license` and a `LICENSE-MIT` at the root: exempt, because `unchecked_paths` matching ignores case. A code file named like a license, such as `xtask/src/license.rs`: must stay uncovered, so the gate still parks it. Pinned by `rust_preset_exempts_license_and_dotfiles` and `rust_preset_does_not_exempt_tooling_files` (Task 1).
4. `dashboard/Cargo.toml` and tooling files: stay uncovered, so a reviewer sees a park instead of a silent pass. Pinned by `rust_preset_root_rule_does_not_cover_other_manifests` and `rust_preset_does_not_exempt_tooling_files` (Task 1).
5. A project override: the docs must say it replaces the preset lists, with the exact sentence the test looks for. Pinned by `reference_documents_the_override_rule` (Task 2).

## File Structure

- Modify `crates/factory-core/presets/rust/config.toml`: root rule, `unchecked_paths`, comment fix.
- Modify `crates/factory-core/src/preset.rs`: tests only, in the existing `tests` module.
- Create `crates/factory-cli/tests/check_coverage_docs.rs`: doc pin tests.
- Modify `docs/reference/project-toml-reference.md`, `docs/reference/software-factory.md`: Task 2.
- Modify `README.md`, `docs/guides/getting-started.md`, `docs/operations/releasing.md`: Task 3.

### Task 1: Preset root rule and generic exemptions

**Files:**
- Modify: `crates/factory-core/presets/rust/config.toml`
- Test: `crates/factory-core/src/preset.rs` (module `tests`, next to `rust_checks_use_cargo_with_j4_and_go_checks_use_go`)

**Interfaces:**
- Consumes: `embedded("rust").expect("rust").config` (`preset.rs:162`), `Config::parse(&str)`, `crate::gate::uncovered_code_paths(paths: &[String], config: &Config) -> Vec<String>` (`gate.rs:52`).
- Produces: the preset text the later doc tasks describe. Tests added: six, named below.

- [ ] **Step 1: Read `crates/factory-core/src/gate.rs`, `crates/factory-core/presets/rust/config.toml` and the `tests` module of `preset.rs` around line 570.**

- [ ] **Step 2: Add the failing tests to the `tests` module of `preset.rs`.**

Add a helper and one full example:

```rust
fn rust_uncovered(paths: &[&str]) -> Vec<String> {
    let config = Config::parse(embedded("rust").expect("rust").config).expect("parse");
    let owned: Vec<String> = paths.iter().map(|p| p.to_string()).collect();
    crate::gate::uncovered_code_paths(&owned, &config)
}

#[test]
fn rust_preset_covers_the_root_manifest_and_lockfile() {
    assert!(rust_uncovered(&["Cargo.toml", "Cargo.lock"]).is_empty());
    let config = Config::parse(embedded("rust").expect("rust").config).expect("parse");
    let rule = config
        .checks
        .iter()
        .find(|r| r.matches("Cargo.toml"))
        .expect("rule for Cargo.toml");
    assert_eq!(rule.commands, vec!["cargo check -j4".to_string()]);
}
```

Four more, each using `rust_uncovered`:

| Test | Input | Assertion |
|---|---|---|
| `rust_preset_root_rule_does_not_cover_other_manifests` | `["dashboard/Cargo.toml"]` | result equals `["dashboard/Cargo.toml"]` |
| `rust_preset_exempts_license_and_dotfiles` | `LICENSE`, `LICENSE-MIT`, `license`, `LICENSE.txt`, `COPYING`, `COPYING.txt`, `.gitignore`, `.gitattributes`, `.editorconfig` | result is empty |
| `rust_preset_does_not_exempt_tooling_files` | `rustfmt.toml`, `rust-toolchain.toml`, `scripts/release.sh`, `cliff.toml`, `xtask/src/license.rs`, `tools/licenses.rs`, `vendor/x/LICENSE`, `sub/.gitattributes` | result has all eight, in input order |
| `rust_preset_nested_crate_manifest_is_covered` | `crates/factory-core/Cargo.toml` | result is empty |

Add `rust_preset_root_command_equals_the_src_rule_command`: find the rule that matches `src/main.rs` and the rule that matches `Cargo.toml`, and assert the first command of each is the same string `cargo check -j4`.

- [ ] **Step 3: Run the tests and expect failures.**

Run: `cargo test -j4 -p factory-core rust_preset`
Expected: `rust_preset_covers_the_root_manifest_and_lockfile`, `rust_preset_exempts_license_and_dotfiles` and `rust_preset_root_command_equals_the_src_rule_command` fail (`rule for Cargo.toml` is missing, the license paths are uncovered); the other three pass already.

- [ ] **Step 4: Edit `crates/factory-core/presets/rust/config.toml`.**

Add the top-level `unchecked_paths` line from Global Constraints right after the closing `]` of the `risk_paths` array (top-level keys must come before the first `[[checks]]` table). Add a third `[[checks]]` table at the end with the root rule from Global Constraints and the comment `# Root manifest and lockfile. A change here can break any member, so check the whole workspace.`. In the comment above the single-crate rule, replace the sentence about a virtual workspace relying on the `crates/*/**` rule with: `A virtual workspace's root manifest is covered by the root rule below.`

- [ ] **Step 5: Run the tests and expect a pass.**

Run: `cargo test -j4 -p factory-core rust_preset`
Expected: 6 passed.

Then run: `cargo test -j4 -p factory-core` and `cargo clippy -j4 -p factory-core --all-targets -- -D warnings`
Expected: all tests pass, no warnings.

Then run: `cargo fmt -p factory-core`, then `cargo test -j4 -p factory-checks`.
Expected: the existing rust preset tests there (`the_rust_preset_checks_a_single_crate_repo`, `a_mixed_diff_runs_the_crate_and_root_commands_once_each`) pass unchanged. The `settings_block` tests in `factory-core` also read the preset and must keep passing.

### Task 2: Reference docs

**Files:**
- Modify: `docs/reference/project-toml-reference.md`, `docs/reference/software-factory.md`
- Create: `crates/factory-cli/tests/check_coverage_docs.rs`

**Interfaces:**
- Consumes: the preset values from Task 1.
- Produces: the file `check_coverage_docs.rs` with a helper `fn read(rel: &str) -> String` (reads `Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel)` as `plugin_files.rs:7` does, using `.expect("read")`). Task 3 adds tests to it. Tests added: two.

- [ ] **Step 1: Read the `unchecked_paths` row (about line 59) and the checks section (about lines 195-215) of `project-toml-reference.md`, and the `no-checks` bullet of `software-factory.md` (about line 195).**

- [ ] **Step 2: Create `crates/factory-cli/tests/check_coverage_docs.rs` with the helper and two failing tests.**

```rust
use std::path::Path;

fn read(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel);
    std::fs::read_to_string(&path).expect("read doc file")
}
```

```rust
#[test]
fn reference_documents_the_override_rule() {
    let text = read("docs/reference/project-toml-reference.md");
    assert!(text.contains(
        "A project that sets `[[settings.checks]]` or `unchecked_paths` replaces the preset value, so copy the preset entries it still wants."
    ));
    assert!(text.contains("paths = [\"Cargo.toml\", \"Cargo.lock\"]"));
}

#[test]
fn software_factory_mentions_the_root_manifest_rule() {
    let text = read("docs/reference/software-factory.md");
    assert!(text.contains(
        "The rust preset covers the root `Cargo.toml` and `Cargo.lock` with `cargo check -j4`."
    ));
}
```

- [ ] **Step 3: Run the tests and expect failures.**

Run: `cargo test -j4 -p factory-cli --test check_coverage_docs`
Expected: 2 failed on the missing text.

- [ ] **Step 4: Edit the docs.**

Each exact sentence the tests look for must sit on one single line of the file, never wrapped. In `project-toml-reference.md`: add, after the checks "How it works" list, the override sentence from the test (verbatim) and a TOML snippet holding the root rule from Global Constraints plus an `unchecked_paths` line with the eight names; add to the `unchecked_paths` row that the rust preset sets the generic list. In `software-factory.md`: add the root rule sentence from the test (verbatim) to the `no-checks` bullet.

- [ ] **Step 5: Run the tests and expect a pass.**

Run: `cargo test -j4 -p factory-cli --test check_coverage_docs`
Expected: 2 passed.

Then run: `cargo fmt -p factory-cli` and `cargo clippy -j4 -p factory-cli --all-targets -- -D warnings`
Expected: no warnings.

### Task 3: Upgrade note and release step

**Files:**
- Modify: `README.md`, `docs/guides/getting-started.md`, `docs/operations/releasing.md`
- Test: `crates/factory-cli/tests/check_coverage_docs.rs`

**Interfaces:**
- Consumes: `read(rel: &str) -> String` from Task 2. Tests added: three.

- [ ] **Step 1: Read the Upgrading section of `README.md` (about line 255-285), section 15 of `getting-started.md` (about line 432-448) and "Cutting a release" in `releasing.md`.**

- [ ] **Step 2: Add three failing tests to `check_coverage_docs.rs`.**

```rust
#[test]
fn readme_and_getting_started_carry_the_check_coverage_note() {
    for rel in ["README.md", "docs/guides/getting-started.md"] {
        let text = read(rel);
        assert!(text.contains("## Check coverage"), "{rel} has the heading");
        assert!(text.contains("factory --project <name> status"), "{rel} names status");
        assert!(text.contains("[[settings.checks]]"), "{rel} names the override");
    }
}

#[test]
fn releasing_has_the_paste_step() {
    let text = read("docs/operations/releasing.md");
    assert!(text.contains(
        "Paste the \"Check coverage\" note from `README.md` (Upgrading) into the draft release."
    ));
}
```

Add `check_coverage_note_comes_after_the_upgrade_steps`: in `README.md`, the index of `"## Check coverage"` is greater than the index of `"factory upgrade --dry-run"`.

- [ ] **Step 3: Run the tests and expect failures.**

Run: `cargo test -j4 -p factory-cli --test check_coverage_docs`
Expected: 3 failed, 2 passed.

- [ ] **Step 4: Edit the docs.**

Add a `#### Check coverage` section at the end of the README Upgrading section and a `### Check coverage` section at the end of section 15 of `getting-started.md`. Both say: run `factory upgrade` (it refreshes an unedited rust preset); a project with `[[settings.checks]]` or `unchecked_paths` in `project.toml` keeps its own values and must add the root rule and the exempt paths itself, with the same snippet as Task 2; a job that parks as `no-checks` shows in `factory --project <name> status`. In `releasing.md`, add to "Cutting a release", right after the paragraph that starts `A failing channel blocks the release`, a new paragraph of two lines. The first line is exactly the line the test looks for (one line, no wrapping):

```text
Paste the "Check coverage" note from `README.md` (Upgrading) into the draft release.
```

The second line says the changelog skips `chore:` commits, so this note is not generated.

- [ ] **Step 5: Run the tests and expect a pass.**

Run: `cargo test -j4 -p factory-cli --test check_coverage_docs`
Expected: 5 passed.

Then run: `cargo fmt -p factory-cli` and `cargo clippy -j4 -p factory-cli --all-targets -- -D warnings`
Expected: no warnings.

Next plan: none
