# Tidy docs and guards, plan 3: command text

> **Spec:** docs/superpowers/specs/2026-11-22-tidy-docs-and-guards-design.md

> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** The command text points at the Commit note, Claude plans keep the shared executor line, the Codex map loses its filler (#43), and `/factory retry` shows factoryd refusals as `/factory` commands (#140).

**Commit:** docs(command): tidy notes and retry refusals

**Architecture:** Text edits in `crates/factory-core/assets/global-command.md`, `tool-map-claude.md` and `tool-map-codex.md`, kept in step with their copies and pins. `global-command.md` must end with `tool-map-claude.md` (`claude_map_is_the_command_tail` in `crates/factory-cli/tests/tool_map.rs`). The plugin copy `plugins/factory/commands/factory.md` and the Codex skill copy follow whatever `plugin_files.rs` and `skill_drift.rs` compare.

**Tech Stack:** Markdown, Rust tests in factory-cli.

## Global Constraints

- cargo commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`, never --workspace or --all.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- No other wording in the command changes.

## File Structure

| File | Change |
|---|---|
| `crates/factory-core/assets/global-command.md` | Commit pointers (lines 76, 93), Interview and Plan writing pointers, retry bullet (line 45), and the Claude tail. |
| `crates/factory-core/assets/tool-map-claude.md` | Plan writing note without the executor sentence. |
| `crates/factory-core/assets/tool-map-codex.md` | Skills note (line 13), filler sentence (line 37). |
| `crates/factory-cli/tests/global_command.rs` | Executor pin (line 457), new tests. |
| `crates/factory-cli/tests/tool_map.rs` | `maps_name_their_own_tools` still finds `$factory`. |
| `plugins/factory/commands/factory.md` and any other copy the drift tests compare | Follow the edits. |

## Review Focus

1. The neutral executor line that `planlint.rs` `FACTORY_EXECUTOR` accepts: a user expects the shared text to still give it verbatim. Pinned by `the_shared_executor_line_is_the_only_one`.
2. A refusal naming `factory approve 7 --waive "<reason>"`: a user expects `/factory approve 7 --waive "<reason>"`. Pinned by `retry_bullet_maps_refusals_to_slash_forms`, which checks the mapping rule text.
3. The Codex skill still never mentions Claude tools. Pinned by the existing `shared_text_names_no_harness_tool` and the Codex unsupported list in `global_command.rs:268-280`.

---

### Task 1: Commit pointers and the shared executor line

**Files:**
- Modify: `crates/factory-core/assets/global-command.md`, `tool-map-claude.md`
- Test: `crates/factory-cli/tests/global_command.rs`

**Interfaces:**
- Consumes: `FACTORY_EXECUTOR` in `crates/factory-engine/src/planlint.rs:25`.
- Produces: text only.
- Port: none.

- [ ] **Step 1: Read.** Read `crates/factory-core/assets/global-command.md`, `tool-map-claude.md`, `crates/factory-cli/tests/global_command.rs:420-510`, `crates/factory-cli/tests/tool_map.rs:100-160` and `crates/factory-cli/tests/plugin_files.rs`.

- [ ] **Step 2: Write the failing test** in `global_command.rs`, reading the command text the way the neighbouring tests do:

```rust
#[test]
fn the_shared_executor_line_is_the_only_one() {
    let text = command_text();
    let shared = "> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.";
    assert!(text.contains(shared), "shared executor line missing");
    assert!(!text.contains("runs this plan using superpowers:subagent-driven-development"), "the Claude note still replaces the executor line");
    assert_eq!(text.matches("See the `Commit` note.").count(), 2);
}
```

Replace the exact pin at line 457, which holds the subagent-driven variant, with the shared line. `command_text()` stands for whatever helper those tests use.

- [ ] **Step 3: Run it.** `cargo test -j4 -p factory-cli --test global_command the_shared_executor`. Expected: fails on the second assertion.

- [ ] **Step 4: Edit.**
  - End both commit sentences (in step 1's last bullet and in step 3) with `See the `Commit` note.`
  - Move the `Interview` and `Plan writing` note pointers to the end of their bullets.
  - In `tool-map-claude.md` Plan writing, drop the clause from "and the header's line about who executes the plan is replaced by" through the quoted executor line, so the shared line stands.
  - Make the same edit in the tail of `global-command.md` and in each copy the drift tests compare.

- [ ] **Step 5: Run them.** `cargo test -j4 -p factory-cli --test global_command --test tool_map --test plugin_files --test skill_drift --test agent_lint`. Expected: all pass.

### Task 2: Codex text and retry refusals

**Files:**
- Modify: `crates/factory-core/assets/tool-map-codex.md:13,37`, `global-command.md:45` (and its copies)
- Test: `crates/factory-cli/tests/global_command.rs`, `crates/factory-cli/tests/tool_map.rs`

**Interfaces:**
- Produces: text only.
- Port: none.

- [ ] **Step 1: Read.** Read `crates/factory-core/assets/tool-map-codex.md`, `crates/factory-cli/tests/global_command.rs:370-400` and `crates/factory-cli/tests/tool_map.rs:120-135`.

- [ ] **Step 2: Write the failing tests:**

```rust
#[test]
fn retry_bullet_maps_refusals_to_slash_forms() {
    let text = command_text();
    assert!(text.contains("naming each `factory <sub> <id> ...` it lists as `/factory <sub> <id> ...`"), "retry bullet");
    assert!(!text.contains("report a refusal as it is"), "old wording");
}
```

In `tool_map.rs`, add `codex_map_has_no_filler`. It asserts that `TOOL_MAP_CODEX` does not contain `follows the shared text.`, contains `Invoke this workflow as $factory`, and contains `In factory runs, use only the skill catalog the run preface allows.`

- [ ] **Step 3: Run them.** `cargo test -j4 -p factory-cli --test global_command retry_bullet` and `cargo test -j4 -p factory-cli --test tool_map codex_map`. Expected: both fail.

- [ ] **Step 4: Edit.**
  - In the retry bullet, replace "report a refusal as it is" with: report a refusal, naming each `factory <sub> <id> ...` it lists as `/factory <sub> <id> ...`.
  - Keep the phrase `refuses a job that cannot be retried`, which `global_command.rs:391` pins.
  - In `tool-map-codex.md`, replace the filler with `Invoke this workflow as $factory; the shared text's /factory examples are written as $factory here.`, and start the Skills note with `In factory runs, use only the skill catalog the run preface allows.`
  - Update the copies the drift tests compare.

- [ ] **Step 5: Run them.** `cargo test -j4 -p factory-cli --test global_command --test tool_map --test plugin_files --test skill_drift --test agent_lint`. Expected: all pass.

Next plan: none
