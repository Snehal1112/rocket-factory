# Factory quality fixes, Plan 03: Builder context

> **Spec:** docs/superpowers/specs/2026-11-01-factory-quality-fixes-design.md (section Plan 3, Compatibility)
>
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** A task builder sees the spec's Design, edge-case, Decisions and Out of scope sections as well as its goal and acceptance criteria, within a fixed size, on both harnesses.

**Architecture:** `spec_excerpt` always keeps the goal and the whole acceptance criteria section, as it does today. It then spends the room left under a byte cap on the other kept `## ` sections in spec order, with fenced blocks respected, and says where anything left out is. The excerpt preamble in `task_excerpt` names the wider content.

**Tech Stack:** Rust, `cargo test -j4 -p factory-engine`.

## Global Constraints

- Always `-j4` and `-p <crate>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Each task ends with `cargo fmt -p factory-engine -- --check` and `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`. No `#[allow(...)]`.
- No unwrap in production Rust. Tests use `.expect("...")` and `expect_err`.
- Code comments are short full sentences that end with a period.
- The excerpt text is shared by Claude and Codex builders and names no harness tool.
- No new setting. The cap is a constant `SPEC_EXCERPT_CAP: usize = 8192` in `tasks.rs`.
- Extra kept headings are the `## ` headings whose lowercase text contains one of `design`, `edge case`, `decision`, `decided`, `out of scope`. The goal and the `acceptance criteria` section are always kept whole and never count against the room.
- Texts are exact: the cut marker is `[excerpt cut at 8192 bytes, the rest is in spec.md]`. The preamble starts `Task excerpt: the plan header, your task, and the spec goal, design, edge cases, decisions, scope and acceptance criteria follow.`

## Review Focus

1. A long Design section must never push the acceptance criteria out: the goal and the whole acceptance criteria section stay, whatever the size of the other sections. Pinned by `acceptance_criteria_survive_a_design_section_over_the_cap`. A cut elsewhere must fall on a line break, never inside a multi-byte character. Pinned by `an_extra_section_over_the_room_is_cut_at_a_line_with_a_marker`.
2. A `## ` line inside a fenced block must not end a kept section or start one. Pinned by `a_fenced_heading_neither_starts_nor_ends_a_kept_section`.
3. A spec with none of the kept sections must still give its goal, as today. Pinned by `a_spec_without_kept_sections_still_gives_the_goal`.
4. Sections must come out in spec order, not in a fixed order. Pinned by `kept_sections_keep_the_spec_order`.
5. The builder prompt for a job on Codex must carry the same preamble and sections. Pinned by `a_codex_builder_prompt_carries_the_wider_excerpt`.

## File Structure

| File | Responsibility |
|---|---|
| `crates/factory-engine/src/tasks.rs` | `SPEC_EXCERPT_CAP`, `spec_excerpt`, its tests |
| `crates/factory-engine/src/specs.rs` | the preamble of `task_excerpt` |
| `crates/factory-engine/tests/slim_prompts.rs`, `crates/factory-engine/tests/build_checks.rs` | the tests that pin the preamble |
| `crates/factory-engine/tests/codex_prep.rs` | the Codex prompt test |
| `docs/reference/project-toml-reference.md`, `docs/reference/software-factory.md`, `README.md` | the sentences that name the goal and acceptance criteria |

### Task 1: Widen `spec_excerpt` and cap it

**Files:**
- Modify: `crates/factory-engine/src/tasks.rs`
- Test: `crates/factory-engine/src/tasks.rs` (tests module)

**Interfaces:**
- Produces: `pub const SPEC_EXCERPT_CAP: usize = 8192;` and the unchanged signature `pub fn spec_excerpt(spec: &str) -> String`. The result is the goal part, then the whole acceptance criteria section, then each extra kept section in spec order, joined by a blank line. The room for the extra sections is `SPEC_EXCERPT_CAP` minus the length of the goal part, the acceptance section and the marker line. An extra section that does not fit is cut at the last line break inside the room, later extra sections are left out, and the marker line is appended. The goal and the acceptance section are never cut.

- [ ] **Step 1: Read the code this task touches**

Read `spec_excerpt`, `goal_section`, `spec_goal` and `lines_with_fence` in `tasks.rs` and the tests that call `spec_excerpt` (`the_spec_excerpt_has_the_goal_and_the_acceptance_section_only`, `a_fenced_h2_does_not_end_the_acceptance_section`, the CRLF case in `reads_heading_tasks_in_order`).

- [ ] **Step 2: Write the failing tests**

In the `tasks.rs` tests module:
- `the_excerpt_keeps_design_edge_cases_decisions_and_scope`: a spec with the sections Background, Design, Error handling and edge cases, Decisions, Out of scope, Testing and Acceptance criteria keeps the text of the Design, edge-case, Decisions, Out of scope and Acceptance criteria sections, and leaves out Background and Testing.
- `kept_sections_keep_the_spec_order`: with Decisions written before Design in the spec, the excerpt shows Decisions before Design, and the acceptance criteria come after the goal and before them.
- `a_fenced_heading_neither_starts_nor_ends_a_kept_section`: a `## Design notes` line inside a fenced block does not start a section, and a `## comment` line inside a fence in the Design section does not end it.
- `a_spec_without_kept_sections_still_gives_the_goal`: a spec with only a goal line and a Background section gives exactly the goal line.
- `acceptance_criteria_survive_a_design_section_over_the_cap`: a spec whose Design section alone is 20000 bytes, with Acceptance criteria written after it, gives an excerpt that holds the goal and the whole acceptance criteria section and ends with the marker.
- `an_extra_section_over_the_room_is_cut_at_a_line_with_a_marker`: a spec whose Design section is 20000 bytes of short lines, with a multi-byte character near the cut, gives an excerpt of at most 8192 bytes that ends with the marker and whose Design text before the marker ends at a line break.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -j4 -p factory-engine --lib -- the_excerpt_keeps_design_edge_cases_decisions_and_scope an_excerpt_over_the_cap_is_cut_at_a_line_with_a_marker`.
Expected: FAIL.

- [ ] **Step 4: Implement the wider excerpt and the cap**

Keep the goal and acceptance-section code as it is. Add one pass over `lines_with_fence`: an unfenced `## ` line starts an extra kept section when its lowercase text contains an extra topic and ends the current one otherwise. Fill the room with those sections in order, cutting the one that overflows. Update the doc comment of `spec_excerpt`.

- [ ] **Step 5: Run the tests, fmt and clippy**

Run: `cargo test -j4 -p factory-engine`, then fmt and clippy for `factory-engine`.
Expected: all pass, no warnings.

### Task 2: Say what the excerpt holds

**Files:**
- Modify: `crates/factory-engine/src/specs.rs`, `crates/factory-engine/tests/slim_prompts.rs`, `crates/factory-engine/tests/build_checks.rs`, `docs/reference/project-toml-reference.md`, `docs/reference/software-factory.md`, `README.md`
- Test: `crates/factory-engine/tests/codex_prep.rs`

**Interfaces:**
- Consumes: the wider `spec_excerpt` from the previous task.
- Changes: the preamble string in `task_excerpt` (`specs.rs`) starts with the exact text from Global Constraints. The rest of the preamble is unchanged.

- [ ] **Step 1: Read the code this task touches**

Read `task_excerpt` in `specs.rs`, the golden string `GOLDEN_TASK` in `slim_prompts.rs`, the assertion on `"Task excerpt"` in `build_checks.rs` (the test `a_task_builder_gets_its_own_task_the_header_and_the_spec_excerpt`), the Codex prompt helpers in `codex_prep.rs` (`agent_text`, `codex`), and the three doc sentences that say the excerpt holds the goal and acceptance criteria.

- [ ] **Step 2: Write the failing test**

In `codex_prep.rs`: `a_codex_builder_prompt_carries_the_wider_excerpt` runs a task builder on a Codex fixture whose `spec.md` has a Design section, and asserts the prompt that reaches the runner contains the new preamble start and the Design text.

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -j4 -p factory-engine --test codex_prep -- a_codex_builder_prompt_carries_the_wider_excerpt`.
Expected: FAIL.

- [ ] **Step 4: Change the preamble, the golden string and the docs**

Change the preamble text, update `GOLDEN_TASK` and the `build_checks.rs` assertion to match, and reword the three doc sentences to name the goal, design, edge cases, decisions, scope and acceptance criteria and the 8192 byte cap.

- [ ] **Step 5: Run the tests, fmt and clippy**

Run: `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli --test tool_map`, then fmt and clippy for `factory-engine`.
Expected: all pass, no warnings.

Next plan: docs/superpowers/plans/2026-11-01-factory-quality-fixes/plan-04-reviewer-input.md
