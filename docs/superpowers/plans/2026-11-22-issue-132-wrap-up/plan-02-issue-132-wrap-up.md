# Command binary rule and full-coverage path lints Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-22-issue-132-wrap-up-design.md
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** The `/factory` command always calls `factory` on PATH and names no shell variable, the exact tool-map strip is pinned, and both path lints scan every workspace crate's production code.

**Commit:** fix(command): drop FACTORY_BIN, widen path lints

**Architecture:** Text edits to the shared command and its plugin copy, with the tests that pin them in factory-cli. The lints in `crates/factory-cli/tests/no_hardcoded_paths.rs` get three pure helpers that unit tests can call on literal text.

**Tech Stack:** Rust, Markdown.

## Global Constraints

- Cargo commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- No task adds a dependency or edits any `Cargo.toml`, `.github/**`, `scripts/**` or `journeybook/**`. `scripts/live-smoke.sh` already puts the binary's folder on `PATH` (commit 8c73488).
- `plugins/factory/commands/factory.md` stays equal to the text `plugin_files_match_generated_text` expects from `global-command.md`.
- factoryd keeps reading `FACTORY_BIN` (`crates/factory-supervisor/src/spawn.rs`); only the command text and the docs about the command change.

## Review Focus

1. A user who set `FACTORY_BIN` for the command: they expect the docs to say plainly that the command uses `factory` on PATH. Pinned by `the_command_names_no_shell_variable` and the getting-started row text.
2. A command with an extra blank line or text after the Claude map: a user expects `factory install` to refuse, not to ship Claude tool names to Codex. Pinned by `codex_skill_refuses_text_after_the_claude_map`.
3. `"}"` or `"{"` inside a string in a test module: a user expects the lint not to cut early or swallow the rest of the file. Pinned by `production_lines_drops_only_test_items`.
4. A `#[cfg(test)]` inside a string (as in `factory-core/src/gate.rs`): a user expects it scanned as text. Pinned by `production_lines_drops_only_test_items`.
5. A new workspace member outside `crates/`: a user expects a loud failure. Pinned by `a_member_outside_crates_panics`.

## File Structure

- Modify: `crates/factory-core/assets/global-command.md`, `plugins/factory/commands/factory.md`
- Modify: `plugins/factory/README.md`, `README.md`, `docs/guides/getting-started.md`
- Modify: `crates/factory-cli/src/install.rs` (tests only), `crates/factory-cli/tests/global_command.rs`, `crates/factory-cli/tests/plugin_files.rs`
- Modify: `crates/factory-cli/tests/no_hardcoded_paths.rs`

---

### Task 1: the command calls `factory` literally, and the strip stays exact

**Files:**
- Modify: `crates/factory-core/assets/global-command.md`, `plugins/factory/commands/factory.md`, `plugins/factory/README.md`, `README.md`, `docs/guides/getting-started.md`
- Modify: `crates/factory-cli/src/install.rs`, `crates/factory-cli/tests/global_command.rs`, `crates/factory-cli/tests/plugin_files.rs`

**Interfaces:**
- Port: none. Text and tests.
- Consumes: `GLOBAL_COMMAND` (factory-core), `install::claude_command_text()`, `build_codex_skill`, `codex_skill` in `install.rs`.

**Tests added:** `the_command_names_no_shell_variable` (`plugin_files.rs`), `codex_skill_refuses_text_after_the_claude_map` (`install.rs`), changed asserts in `the_command_text_has_the_required_phrases_and_no_rocket_bits` and `command_names_the_path_failure`.

- [ ] **Step 1: Read the code**

Read "Find the project" and the `<factory>` line in `crates/factory-core/assets/global-command.md`, `plugin_files_match_generated_text` and `command_names_the_path_failure` in `crates/factory-cli/tests/plugin_files.rs`, `the_command_text_has_the_required_phrases_and_no_rocket_bits` in `crates/factory-cli/tests/global_command.rs`, `build_codex_skill` and its tests in `crates/factory-cli/src/install.rs`, the README lines that say `$FACTORY_BIN` near "In a repository, `/factory` runs", line 5 of `plugins/factory/README.md`, and the `FACTORY_BIN` row in `docs/guides/getting-started.md`.

- [ ] **Step 2: Write the failing tests**

- `the_command_text_has_the_required_phrases_and_no_rocket_bits`: replace the needle `"FACTORY_BIN"` with `"never use a shell variable"`.
- `command_names_the_path_failure`: keep `factory binaries are not on PATH`; replace `text.contains("FACTORY_BIN")` with `text.contains("never use a shell variable")`.
- `the_command_names_no_shell_variable` in `plugin_files.rs`: for `factory_core::GLOBAL_COMMAND` and the text of `root().join("plugins/factory/commands/factory.md")`, assert neither contains `FACTORY_BIN` nor `$FACTORY`.
- `codex_skill_refuses_text_after_the_claude_map` in `install.rs` tests: `build_codex_skill(&format!("{}\nextra\n", claude_command_text()))` is an error whose text contains `Claude tool map`.

- [ ] **Step 3: Run them and see them fail**

Run: `cargo test -j4 -p factory-cli --test global_command`, `cargo test -j4 -p factory-cli --test plugin_files`, `cargo test -j4 -p factory-cli --lib install`
Expected: the three changed or new text tests fail on the old wording. `codex_skill_refuses_text_after_the_claude_map` passes already, because it pins today's exact strip.

- [ ] **Step 4: Implement**

- `global-command.md` step 1: ``1. Run `factory project which` from the current directory. Its output is the project name, called `<name>` below.``
- The `<factory>` line: `` `<factory>` is `factory` on PATH. Write it literally in every shell command and never use a shell variable, because the Claude permission check refuses a command that holds one. Pass `--project <name>` on every `<factory>` call, so each call is `<factory> --project <name> <subcommand> ...`. ``
- Make the same two edits in `plugins/factory/commands/factory.md`, so `plugin_files_match_generated_text` passes.
- `README.md`: the command sentence ends ``on every `factory` call (`factory` on PATH).``. Leave the factoryd `FACTORY_BIN` lines as they are.
- `plugins/factory/README.md`: drop "(or set in `$FACTORY_BIN`)".
- `docs/guides/getting-started.md`: the sentence "If you keep the binaries somewhere else, set `FACTORY_BIN` (see section 5)." becomes "If you keep the binaries somewhere else, set `FACTORY_BIN` for `factoryd` (see section 5); the `/factory` command still needs `factory` on PATH." The `FACTORY_BIN` row: "Tells `factoryd` where the `factory` binary is. The `/factory` command always uses `factory` on PATH."

- [ ] **Step 5: Run them and see them pass**

Run: `cargo test -j4 -p factory-cli --test global_command`, `--test plugin_files`, `--test tool_map`, `--test agent_lint`, `--test check_coverage_docs` (it reads `README.md` and `getting-started.md`), `--lib install`
Expected: all pass, 0 failed.

### Task 2: both path lints scan all production code

**Files:**
- Modify: `crates/factory-cli/tests/no_hardcoded_paths.rs`

**Interfaces:**
- Port: none. A source lint test.
- Produces (private to the test file):
  - `fn production_lines(text: &str) -> Vec<(usize, &str)>`, 1-based line numbers of the original text;
  - `fn path_hits(rel: &str, text: &str) -> Vec<String>`, each `"{rel}:{n}: {trimmed line}"`;
  - `fn crates_in(manifest: &str) -> Vec<String>`; `workspace_crates()` calls it on `root_text("Cargo.toml")`.

**Tests added:** `production_lines_drops_only_test_items`, `the_proc_lint_catches_a_read_after_an_early_test_use`, `the_paths_lint_catches_main_after_an_early_test_use`, `a_member_outside_crates_panics`.

- [ ] **Step 1: Read the code**

Read all of `crates/factory-cli/tests/no_hardcoded_paths.rs`. Look at the layout of `#[cfg(test)]` in `crates/factory-engine/src/permissions.rs` (line 6, a `use`), `crates/factory-engine/src/tasks.rs` (a test module mid-file), `crates/factory-core/src/gate.rs` (the marker inside a string) and `crates/factory-cli/src/plugin.rs` (a `"}\n"` string in its tests).

- [ ] **Step 2: Write the failing tests**

`production_lines_drops_only_test_items` on this text, written as a Rust string literal in the test:

```text
#[cfg(test)]
use x::y;
fn keep_one() {}
const S: &str = "#[cfg(test)]";
#[cfg(test)]
mod tests {
    fn t() { let a = "}"; let b = "{"; }
}
fn keep_two() {}
```

Assert the kept line texts are exactly `["fn keep_one() {}", "const S: &str = \"#[cfg(test)]\";", "fn keep_two() {}"]` and their numbers are `[3, 4, 9]`.

- `the_proc_lint_catches_a_read_after_an_early_test_use`: `proc_lines` on `"#[cfg(test)]\nuse x;\nfn f() { let _ = std::fs::read(\"/proc/self/stat\"); }\n"` has length 1.
- `the_paths_lint_catches_main_after_an_early_test_use`: `path_hits("a/src/x.rs", "#[cfg(test)]\nuse x;\nconst B: &str = \"main\";\n")` equals `["a/src/x.rs:3: const B: &str = \"main\";"]`.
- `a_member_outside_crates_panics`: `#[should_panic(expected = "is not under crates/")]`, calling `crates_in("members = [\n    \"crates/a\",\n    \"tools/x\",\n]\n")`.
- Keep `the_proc_lint_catches_a_proc_literal` as it is.

- [ ] **Step 3: Run them and see them fail**

Run: `cargo test -j4 -p factory-cli --test no_hardcoded_paths`
Expected: compile errors `cannot find function 'production_lines'`, `'path_hits'` and `'crates_in'`.

- [ ] **Step 4: Implement**

- `production_lines`: walk the lines. A line whose trimmed text is exactly `#[cfg(test)]` starts a test item; let `ind` be its leading whitespace. When the next line ends with `;` after trimming, drop both lines. Otherwise drop every line up to and including the first later line equal to `format!("{ind}}}")`. Keep every other line with its 1-based number.
- `proc_lines` keeps its signature and filters `production_lines` instead of the split.
- `path_hits` holds the body of today's inner loop in `no_source_names_a_state_path_or_the_main_branch` (skip `//` lines; hit on `".superpowers`, `".claude` or `"main"`) over `production_lines`.
- `no_source_names_a_state_path_or_the_main_branch` loops over `workspace_crates()` in place of its fixed list, keeps `ALLOWED`, and calls `path_hits`.
- `crates_in` holds today's parse of the `members = [` block. For a quoted member path that does not start with `crates/`, it panics with `format!("workspace member {path} is not under crates/; update workspace_crates")`. It still asserts the list is not empty.
- A file reached only through a `#[cfg(test)] mod name;` line (`factory-core/src/test_support.rs`, `factory-supervisor/src/api/testutil.rs`) is scanned like production code. Neither has a hit today; if one shows up, move the literal into a test module or add the file to `ALLOWED` with a comment.

- [ ] **Step 5: Run them and see them pass**

Run: `cargo test -j4 -p factory-cli --test no_hardcoded_paths`
Expected: all pass, 0 failed, including `no_source_reads_proc` and `no_source_names_a_state_path_or_the_main_branch` over all 9 crates.

- [ ] **Step 6: Format and lint**

Run: `cargo fmt -p factory-cli`, then `cargo clippy -j4 -p factory-cli --all-targets -- -D warnings`.
Expected: clippy finishes with no warnings.

Next plan: docs/superpowers/plans/2026-11-22-issue-132-wrap-up/plan-03-issue-132-wrap-up.md
