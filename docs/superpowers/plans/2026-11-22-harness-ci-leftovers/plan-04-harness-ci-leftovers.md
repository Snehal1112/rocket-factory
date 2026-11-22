# Retired committer agent and its text Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-22-harness-ci-leftovers-design.md
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** No retired agent file reaches `agents.json` from any layer, each dropped copy is noted, and tests pin the journeybook, turns SVG and Commit-line edits made by hand before this job.

**Commit:** fix(agents): drop the retired committer everywhere

**Architecture:** `merged_agents` in `crates/factory-core/src/resolve.rs` applies `RETIRED_AGENTS` after merging the preset, project and base-tip layers. It writes into the `notes` list that `factory --project` already prints. The journeybook and SVG text was edited by hand on main before this job, because no check covers those paths. This plan adds only the tests that pin them, and never edits `journeybook/` or the SVG.

**Tech Stack:** Rust, TypeScript data files, SVG.

## Global Constraints

- Cargo commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- No task adds a dependency or edits any `Cargo.toml`, which is a risk path.
- No board, ledger or event field.
- A task that edits an agent file under `crates/factory-core/presets/*/agents/` ends with `cargo test -j4 -p factory-cli --test agent_lint`.

## Review Focus

1. A committer copy only in the preset layer: a user expects no note, as today. Pinned by `a_retired_preset_agent_left_on_disk_is_not_resolved`, which gains an assert that no note names `factory-committer.md`.
2. A copy in both the project folder and the base branch: a user expects two notes, one per layer. Pinned by `a_retired_agent_is_dropped_from_every_layer_with_a_note`.
3. A job whose base branch tracks a committer file: a user expects `agents.json` without it. `effective_files` only swaps text for files it is given and never adds one, so the resolve filter covers it. Pinned by `a_retired_agent_is_dropped_from_every_layer_with_a_note`.
4. The tokens report for old jobs, which may hold `committer` usage lines: a user expects them still shown. Pinned by the existing `tokens.rs` test with a `committer` record, which must pass unchanged.
5. The SVG: a user expects it to stay a whole SVG file. Pinned by `the_turns_svg_names_the_worst_case_counts`, which checks the open and close tags.

## File Structure

- Modify: `crates/factory-core/src/resolve.rs` (filter and tests), `crates/factory-core/src/preset.rs` (assert messages)
- Modify: `crates/factory-engine/tests/agent_files.rs`
- Modify: `crates/factory-cli/tests/agent_lint.rs`
---

### Task 1: the retired committer is dropped from every layer

**Files:**
- Modify: `crates/factory-core/src/resolve.rs`
- Modify: `crates/factory-engine/tests/agent_files.rs`

**Interfaces:**
- Port: pure resolve logic in factory-core, behind the `BaseTipFiles` trait for the tip layer. The tests use the fakes already in `resolve.rs` `mod tests`. The engine tests use `Fixture` and the agent file helpers in `agent_files.rs`.
- Produces: no new public items. `merged_agents(..)` keeps its signature and adds notes.

**Tests added:**
- `a_retired_agent_is_dropped_from_every_layer_with_a_note` (in `resolve.rs`), and a no-note assert in `a_retired_preset_agent_left_on_disk_is_not_resolved`.
- `agent_layers_replace_by_name_and_the_result_is_sorted` gets a new sample name. `factory-kb-refresher` already exists there, so the names list drops to 8 and the `"repo committer"` check moves to the kb-refresher index (4), with its text.
- In `agent_files.rs`, `six_files` and the tests built on it use `intake-drafter` in place of `committer`. kb-refresher would make a duplicate, and `intake-drafter` keeps `len() == 6`.

- [ ] **Step 1: Read the code**

Read `RETIRED_AGENTS`, `merged_agents`, `read_agent_dir` and `note_drift` in `crates/factory-core/src/resolve.rs`. Read its tests `a_retired_preset_agent_left_on_disk_is_not_resolved` and `agent_layers_replace_by_name_and_the_result_is_sorted`. Read `six_files`, `a_tracked_agent_text_wins_in_the_agents_file`, `a_job_that_deletes_a_tracked_agent_file_still_gets_its_agents` and the delete-all-tracked test in `crates/factory-engine/tests/agent_files.rs`. Read `effective_agent_files` in `crates/factory-engine/src/agents_file.rs`.

- [ ] **Step 2: Write the failing tests**

`a_retired_agent_is_dropped_from_every_layer_with_a_note`:
- Set up as `agent_layers_replace_by_name_and_the_result_is_sorted` does, with `factory-committer.md` in the preset, the project folder and the tip.
- Assert no resolved agent is named `factory-committer.md`.
- Assert `r.notes` contains `ignored retired agent factory-committer.md from the project folder; delete it` and `ignored retired agent factory-committer.md from the base branch; delete it`.
- Assert `r.problems` is empty.

Also add to `a_retired_preset_agent_left_on_disk_is_not_resolved`: assert that no `r.notes` entry contains `factory-committer.md`.

Repoint the tests that use `committer` as a sample name, as the Tests added list says: in `resolve.rs` to `factory-kb-refresher`, with the list of 8 names, the moved index (`got[3]` becomes the kb-refresher index, 4) and `got[8]` becoming `got[7]`; in `agent_files.rs` to `intake-drafter`, keeping their counts.

- [ ] **Step 3: Run them and see them fail**

Run: `cargo test -j4 -p factory-core resolve`, then `cargo test -j4 -p factory-engine --test agent_files`
Expected: `a_retired_agent_is_dropped_from_every_layer_with_a_note` fails. The repointed tests pass.

- [ ] **Step 4: Implement**

- In `merged_agents`, push the note as each layer is read: `ignored retired agent {name} from the project folder; delete it` for each retired name in the project folder's files, and `ignored retired agent {name} from the base branch; delete it` for each in the base-branch listing. A copy in both gives two notes. A copy only in the preset layer adds none.
- After all three layers are merged, run `by_name.retain(|n, _| !RETIRED_AGENTS.contains(&n.as_str()))`. Keep the preset-layer `retain` as well, so `preset_files` never holds a retired name and `note_drift` adds no drift note for it.
- `effective_agent_files` needs no filter: it only swaps the text of files it is given.
- [ ] **Step 5: Run them and see them pass**

Run: `cargo test -j4 -p factory-core resolve`, `cargo test -j4 -p factory-engine --test agent_files`, `cargo test -j4 -p factory-cli --test project_cmd`.
Expected: all pass, 0 failed.

### Task 2: tests pin the committer text edits and the Commit-line rule

**Files:**
- Modify: `crates/factory-core/src/preset.rs`, `crates/factory-cli/tests/agent_lint.rs`

**Interfaces:**
- Port: none. These are text and test edits.
- Consumes: `require_all(text, label, needles)` and `preset_agent(preset, file)` in `crates/factory-cli/tests/agent_lint.rs`.

**Tests added:** the stronger `every_intake_drafter_asks_for_the_commit_line`, the assert messages in `no_preset_ships_the_committer_or_the_commit_context`, `the_turns_svg_names_the_worst_case_counts` and `the_journeybook_names_no_committer_role` (both in `agent_lint.rs`, or in the docs test that already reads `docs/` files there).

- [ ] **Step 1: Read the code**

Read these:
- `every_intake_drafter_asks_for_the_commit_line` and the docs test near it in `crates/factory-cli/tests/agent_lint.rs`;
- line 40 of `crates/factory-core/presets/rust/agents/factory-intake-drafter.md`;
- `no_preset_ships_the_committer_or_the_commit_context` in `crates/factory-core/src/preset.rs`;
- the four journeybook lines, already edited by hand: `journeys-f-h.ts:658`, `journeys-a-c.ts:603`, `journeys-d-e.ts:608` and `reference.ts:397-398`;
- `docs/assets/images/factory-turns.svg`.

- [ ] **Step 2: Write the failing tests**

- `every_intake_drafter_asks_for_the_commit_line` also requires these needles:
  - `` `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert` ``;
  - `the scope is one word with no spaces`;
  - `no trailing period`;
  - ``next to `**Goal:**` ``.
- `the_turns_svg_names_the_worst_case_counts`: the SVG contains `At most N + 2 builder runs, 3 runs per review role and 1 spec review, with fix_rounds = 2.`, and starts with `<svg` (after any XML header) and ends with `</svg>` (after trimming).
- `the_journeybook_names_no_committer_role`:
  - Read the four `.ts` files.
  - Assert none contains `` `committer` ``, `the committer,`, `reviewer, committer` or `message-only committer`. `reference.ts` keeps an old claim ("Only the builder, reviewer and committer spend tokens.") as the `claimed` side of a correction, which none of these needles matches.
  - Assert `reference.ts` contains `The driver writes commits and merge messages with no agent.`

- [ ] **Step 3: Run them; they pin today's text**

Run: `cargo test -j4 -p factory-cli --test agent_lint`
Expected: all pass, because the text was edited by hand before this job and the drafter files already hold the full rule.

- [ ] **Step 4: Implement**

In `preset.rs`, give the two asserts the messages `"{name} ships factory-committer.md"` and `"{name} ships commit-context.md"`. Do not edit `journeybook/` or the SVG.

- [ ] **Step 5: Run them and see them pass**

Run: `cargo test -j4 -p factory-cli --test agent_lint`, then `cargo test -j4 -p factory-core preset`.
Expected: all pass, 0 failed.

- [ ] **Step 6: Format and lint**

Run: `cargo fmt -p factory-core`, `cargo fmt -p factory-engine`, `cargo fmt -p factory-cli`, then `cargo clippy -j4 -p factory-core --all-targets -- -D warnings`, `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`, `cargo clippy -j4 -p factory-cli --all-targets -- -D warnings`.
Expected: clippy finishes with no warnings.

Next plan: none
