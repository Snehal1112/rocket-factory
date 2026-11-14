# Intake sourced decisions, Plan 01: sourced decisions for intake specs

> **Spec:** docs/superpowers/specs/2026-11-15-intake-sourced-decisions-design.md
>
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** An intake spec passes when every decision cites a real file of the exported tree, is refused with a named reason when one does not, and no longer carries the "no decision answered by the user" warning or the risky-path refusal.

**Architecture:** A pure check `check_intake_decisions` in `spec_check.rs` reads the `## Decisions` section against the exported tree. `validate_series` takes an optional `intake_tree` and, when it is set, runs that check instead of the risky-path user-decision rule, and tells the spec reviewer the spec was drafted unattended. The five preset drafter and reviewer agent files carry the new rules.

**Tech Stack:** Rust, `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli --test agent_lint`.

## Global Constraints

- Always `-j4` and `-p <crate>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Each task ends with `cargo fmt -p <crate> -- --check` and `cargo clippy -j4 -p <crate> --all-targets -- -D warnings` for every crate it touched. No `#[allow(...)]` to silence a lint.
- No unwrap in production Rust. Tests use `.expect("...")` and `expect_err`.
- Code comments are short full sentences that end with a period.
- Do not shell out to `git`. Do not read `/proc`. Paths reach the driver only through `Layout`.
- No board, ledger or events field is added. Intake still posts no agent-written text.
- The interactive flows (`factory new`, `--one-job`, `/factory`) keep `Decided by: user`, the warning and `--accept-spec`. They pass `intake_tree = None`.
- Exact texts: the decision line is `- <decision>. Source: <ref>[, <ref>]. Decided by: call`, a ref is `path`, `path:line` or `path:first-last`, and the reviewer prompt line is `This spec was drafted unattended from an issue. Every decision cites a source file that exists.`
- Reason codes do not change: an unsourced decision ends as `spec-review`, a `refused.md` as `draft-refused`.

## Review Focus

1. A decision wrapped onto a second line has no `Source:` on its first line and must be refused, not read as sourced. Pinned by `check_intake_decisions_refuses_a_missing_source`.
2. A source of `../x`, an absolute path or a symlink out of the tree must be refused, never read. Pinned by `check_intake_decisions_refuses_a_path_outside_the_tree` and `check_intake_decisions_refuses_a_symlink_escape`.
3. A file with a trailing newline must count its lines with `str::lines`, and a non-UTF-8 file must be refused. Pinned by `check_intake_decisions_counts_lines_without_the_trailing_newline` and `check_intake_decisions_refuses_a_file_that_is_not_utf8`.
4. A `Source:` inside the decision text must not hide a missing real one: the last `Source:` on the line wins. Pinned by `check_intake_decisions_uses_the_last_source_on_the_line`.
5. `accept_spec` must never turn an unsourced intake spec into a pass. Pinned by `validate_series_in_intake_mode_refuses_an_unsourced_spec`.

## File Structure

- `crates/factory-engine/src/spec_check.rs`: `check_intake_decisions`, `SpecRefusal::UnsourcedDecision`, the `intake` flag of `review_spec_with`, unit tests.
- `crates/factory-engine/src/runloop.rs`: the `intake_tree` parameter of `validate_series` and `create_job_with_spec_accepting`, the intake branch, the skipped warning, unit tests.
- `crates/factory-engine/src/one_job.rs`: passes `None`.
- `crates/factory-engine/src/intake.rs`: passes `Some(&tree)`.
- `crates/factory-engine/tests/intake.rs` and `crates/factory-engine/tests/common/mod.rs`: fixtures that write sourced decisions, and the integration tests.
- `crates/factory-core/presets/{rust,go,node,ruby,csharp}/agents/factory-intake-drafter.md` and `factory-spec-reviewer.md`.
- `crates/factory-cli/tests/agent_lint.rs`: the preset assertions.
- `docs/guides/factory-user-guide.md` and `docs/reference/software-factory.md`.

### Task 1: The decision check and the reviewer flag

**Files:**
- Modify: `crates/factory-engine/src/spec_check.rs`
- Test: unit tests in `crates/factory-engine/src/spec_check.rs`

**Interfaces:**
- Consumes: `section_body` and `is_list_item` from `crate::runloop` (both `pub(crate)`).
- Produces: `pub(crate) fn check_intake_decisions(spec: &str, tree: &std::path::Path) -> Result<(), String>`, `SpecRefusal::UnsourcedDecision(String)` whose `Display` is the bare message with no `SPEC_OVERRIDE_HINT`, and `review_spec_with(driver, spec, plans, reuse_saved, intake: bool)` which adds the reviewer prompt line from Global Constraints when `intake` is true. The existing wrapper at `spec_check.rs:258` passes `false`.

- [ ] **Step 1: Write the failing tests** in the `tests` module of `spec_check.rs`, each building a temp tree with `tempfile` as the module already does:
  - `check_intake_decisions_accepts_a_sourced_decision`: a 3-line `docs/a.md` and `- Use the guide. Source: docs/a.md:2. Decided by: call` gives `Ok(())`.
  - `check_intake_decisions_takes_a_range_and_two_refs`: `Source: docs/a.md:2-3, docs/b.md` gives `Ok(())`.
  - `check_intake_decisions_refuses_a_missing_source`: an item with no `Source:` gives `Err` whose text holds the first 80 characters of the decision. A wrapped item whose second line holds the `Source:` is also refused.
  - `check_intake_decisions_refuses_a_user_decision`: an item ending `Decided by: user` gives `Err`.
  - `check_intake_decisions_refuses_an_empty_section`: a missing section and an empty one give `Err` holding "the spec has no decisions".
  - `check_intake_decisions_refuses_a_path_outside_the_tree`: `../x`, `/etc/hostname` and `docs/../../x` each give `Err`.
  - `check_intake_decisions_refuses_a_symlink_escape`: a symlink in the temp tree to a file outside it gives `Err` (use `std::os::unix::fs::symlink`).
  - `check_intake_decisions_refuses_a_line_out_of_range`: `:0`, `:4` on a 3-line file and `:3-2` each give `Err`.
  - `check_intake_decisions_refuses_a_missing_file`: a path that does not exist gives `Err`.
  - `check_intake_decisions_refuses_a_file_that_is_not_utf8`: a file of bytes `[0xff, 0xfe]` with `Source: bin.md:1` gives `Err`.
  - `check_intake_decisions_counts_lines_without_the_trailing_newline`: `a\nb\nc\n` accepts `:3` and refuses `:4`.
  - `check_intake_decisions_uses_the_last_source_on_the_line`: `Why Source: nope.md. Source: docs/a.md. Decided by: call` gives `Ok(())`.
  - `the_reviewer_prompt_in_intake_mode_says_the_spec_was_drafted_unattended`: the prompt text builder (find it in `review_spec_with`) holds the Global Constraints line when `intake` is true and does not when false. Extract the prompt into a small private function so the test can call it without a driver.

- [ ] **Step 2: Run `cargo test -j4 -p factory-engine check_intake_decisions`.** Expected: FAIL with an unresolved name.

- [ ] **Step 3: Implement `check_intake_decisions`** per the spec sections 1, 2 and 7: list items of `## Decisions` only, the last `Source:` up to the last `. Decided by: call`, refs split on `, `, canonicalize and prefix-check against the canonical tree, `str::lines().count()` for ranges, error text naming the decision by its position and first 80 characters.

- [ ] **Step 4: Implement `SpecRefusal::UnsourcedDecision` and the `intake` flag** on `review_spec_with`, with every existing call passing `false`. The intake caller is wired in the next task.

- [ ] **Step 5: Run `cargo test -j4 -p factory-engine spec_check`.** Expected: PASS.

- [ ] **Step 6: Run `cargo fmt -p factory-engine -- --check` and `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`.** Expected: no output and no warnings.

### Task 2: Intake mode in validation and the intake pass

**Files:**
- Modify: `crates/factory-engine/src/runloop.rs`, `crates/factory-engine/src/one_job.rs`, `crates/factory-engine/src/intake.rs`
- Test: `crates/factory-engine/src/runloop.rs` (unit), `crates/factory-engine/tests/intake.rs`, `crates/factory-engine/tests/common/mod.rs`

**Interfaces:**
- Consumes: `check_intake_decisions`, `SpecRefusal::UnsourcedDecision` and the `intake` flag of `review_spec_with` from the previous task.
- Produces: `validate_series(driver, start, spec_path, accept_spec, intake_tree: Option<&Path>, parse)` and `create_job_with_spec_accepting(driver, slug, feature, plan, spec_path, model, accept_spec, intake_tree: Option<&Path>)`. When `intake_tree` is `Some`: the decisions check runs after `check_spec`, a failure returns `EngineError::SpecRefused(SpecRefusal::UnsourcedDecision(..))` whatever `accept_spec` is, `risky_without_user_decision` is skipped, the reviewer runs with `intake = true`, and `NO_USER_DECISION` is not added. With `None` nothing changes.

- [ ] **Step 1: Write the failing tests:**
  - `validate_series_in_intake_mode_skips_the_risky_user_refusal` (unit, `runloop.rs`): a plan that touches a path matching a risk glob of the test config and a sourced spec pass with `Some(tree)`, and the same spec is refused with `None`.
  - `validate_series_in_intake_mode_refuses_an_unsourced_spec` (unit, `runloop.rs`): the result is `SpecRefused(UnsourcedDecision(..))`, also when `accept_spec` is `Some("reason")`.
  - `an_intake_job_has_no_user_decision_warning` (integration, `tests/intake.rs`): the created job has no `spec_warning` holding "answered by the user".
  - `an_unsourced_draft_is_redrafted_once_then_fails_spec_review` (integration): the drafter runs twice, the second prompt holds the decision named by the refusal, and the outcome is `Failed { reason: "spec-review" }`.
  - `a_risky_draft_with_sourced_decisions_creates_the_job` (integration): plans that touch a risky path and sourced decisions create the series.

- [ ] **Step 2: Update the fake drafter and fixtures** in `tests/intake.rs` and `tests/common/mod.rs` so their specs write `Source:` lines that name a file in the test repo, and add that file to the repo. Run `cargo test -j4 -p factory-engine --test intake`. Expected: the new tests FAIL, the old ones PASS.

- [ ] **Step 3: Implement the parameter and the intake branch** in `validate_series` and `validate_series_inner`, and skip the warning at the `create_job_with_spec_accepting` site when `intake_tree` is `Some`. `draft_jobs` passes `Some(&tree)`. `one_job.rs` and every other caller pass `None`. Set the `intake` flag of `review_spec_with` from `intake_tree.is_some()`.

- [ ] **Step 4: Run `cargo test -j4 -p factory-engine`.** Expected: all pass, including the existing one-job, spec review and redraft tests.

- [ ] **Step 5: Run `cargo fmt -p factory-engine -- --check` and `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`.** Expected: no output and no warnings.

### Task 3: Preset agents, lint and docs

**Files:**
- Modify: the ten agent files named in File Structure, `crates/factory-cli/tests/agent_lint.rs`, `docs/guides/factory-user-guide.md`, `docs/reference/software-factory.md`
- Test: `crates/factory-cli/tests/agent_lint.rs`

**Interfaces:**
- Consumes: the decision line format and the reviewer prompt line from Global Constraints.
- Produces: every `factory-intake-drafter.md` holds a `## Read first` section and the string `Source:`. Every `factory-spec-reviewer.md` holds the string `drafted unattended`.

- [ ] **Step 1: Write the failing assertions** in `agent_lint.rs`, next to the existing loops over `PRESETS` at the drafter and reviewer checks: the drafter files hold `## Read first` and `Source:`, the reviewer files hold `drafted unattended`. Run `cargo test -j4 -p factory-cli --test agent_lint`. Expected: FAIL naming a preset file.

- [ ] **Step 2: Edit the five drafter files** per the spec section 5: a `## Read first` section before `## Write the spec` (`CLAUDE.md`, `docs/README.md`, `.claude/factory/config.toml` for `risk_paths`, the nearest spec under `docs/superpowers/specs/`), the decision line format on one line, a refuse rule for a risky decision with no answer in any readable file, and `Never write Decided by: user` kept.

- [ ] **Step 3: Edit the five reviewer files** per the spec section 4: under the three finding kinds, the rule that kind 3 does not apply when the prompt says the spec was drafted unattended, and that a decision whose source is missing, does not say what it claims or is contradicted by other code is a blocker.

- [ ] **Step 4: Update the two docs** per the spec section 6.

- [ ] **Step 5: Run `cargo test -j4 -p factory-cli --test agent_lint` and `cargo test -j4 -p factory-cli`.** Expected: PASS.

- [ ] **Step 6: Run `cargo fmt -p factory-cli -- --check` and `cargo clippy -j4 -p factory-cli --all-targets -- -D warnings`.** Expected: no output and no warnings.

Next plan: none
