# Intake sourced decisions design

## Goal

Let an intake draft settle its own decisions from the project's documented architecture, with a source the driver can check for every decision. Today every intake spec carries only `Decided by: call` lines, so the warning "no decision in this spec was answered by the user" is always true, and a spec that touches risky paths can never pass. After this change an unattended draft passes when its decisions are grounded in the project, and is refused with a named reason when they are not.

## Overview

Job 076 (issue #72) showed the gap. The spec had five `Decided by: call` decisions and no source for any of them. The driver recorded the warning `spec_warning` on the board, which says nothing for intake. One decision (the reason code for an infrastructure error) had an answer in `docs/guides/factory-user-guide.md:692-693` and in `docs/superpowers/specs/2026-11-14-intake-first-run-fixes-design.md:135`, and the draft guessed against both. Its two follow-ups were never filed.

The change has four parts:

1. A `Source:` rule for every decision of an intake spec, checked by the driver against the exported tree.
2. The refusal of risky paths in intake mode asks for sourced decisions instead of a user answer, and the warning is dropped.
3. The spec reviewer gets an intake-aware rule for finding kind 3.
4. The drafter prompt tells it where the architecture lives and how to cite it.

## Current state

- `crates/factory-core/presets/<preset>/agents/factory-intake-drafter.md:27`: "Every line in `## Decisions` ends with `Decided by: call`. Never write `Decided by: user`." The same line exists in all five presets (rust, go, node, ruby, csharp).
- `crates/factory-engine/src/runloop.rs:187-195` `user_decisions` counts list items under `## Decisions` that contain `Decided by: user`. For intake it is always 0.
- `crates/factory-engine/src/spec_check.rs:411-433` `risky_without_user_decision` returns a refusal when `user_decisions` is 0 and a plan path is risky. `runloop.rs:378-405` turns it into `SpecRefusal::NoUserDecision` before the agent review runs, and only `accept_spec` can pass it.
- `crates/factory-engine/src/intake.rs:471-479` calls `create_job_with_spec_accepting` with `accept_spec = None`, and `intake.rs:480-492` redrafts once and then fails with `spec-review`. So a risky intake spec fails both attempts.
- `crates/factory-engine/src/runloop.rs:536-541` and `one_job.rs:145-150` add `NO_USER_DECISION` (`runloop.rs:184`) whenever `user_decisions` is 0.
- `crates/factory-core/presets/rust/agents/factory-spec-reviewer.md:16`: finding kind 3 is a `call` decision in a risky area, fixed by `Decided by: user`. The reviewer has Read, Grep and Glob only and cannot see the risk config.
- `crates/factory-engine/src/intake.rs:57-92` `draft_spec` builds the prompt from the issue, the folders, `plan_rules` and prior findings. It does not name `CLAUDE.md`, `docs/` or `.claude/factory/config.toml`. The drafter reads the tracked files of the base branch in `<state>/intake/<n>/tree` (`intake.rs:108-111`, `:422-424`).
- `crates/factory-core/presets/rust/agents/factory-intake-drafter.md:40` lets the drafter write `refused.md` when an issue "needs a decision that a person should make", with no rule for when that applies.
- `docs/guides/factory-user-guide.md:700`: "Intake never posts text an agent wrote, and an intake job files no follow-up or minor ticket." The ticket step returns early for intake jobs (`tickets.rs:161`, `minors.rs:35`).

## Non-goals

- Changing the interactive `/factory` and `factory new` flows. They keep `Decided by: user`, the warning and `--accept-spec`.
- Posting any agent-written text to GitHub, including the content of `refused.md` and follow-ups. See Follow-ups.
- Filing follow-ups as GitHub issues from intake.
- Checking that a cited source supports the decision beyond existence and range. The spec reviewer judges that.
- Reading the external knowledge base. The drafter sees only tracked files of the base branch.

## Anti-goals

- A risky spec that passes with a source that is not real. The driver must verify every `Source:` against the tree.
- A missing or broken `.claude/factory/config.toml` making a diff less risky. It still counts as risky, as today.

## Design

### 1. The decision line format

In an intake spec every list item under `## Decisions` has this shape:

```
- <the decision and its reason>. Source: <ref>[, <ref>]. Decided by: call
```

A `<ref>` is `path`, `path:line` or `path:first-last`. The path is relative to the exported tree, with no leading `/`, no `..` segment and no backslash. `Decided by: user` in an intake spec is invalid, because no user answered it.

### 2. The driver check

- Add `fn check_intake_decisions(spec: &str, tree: &Path) -> Result<(), String>` to `crates/factory-engine/src/spec_check.rs`. It uses the same section parsing as `user_decisions` (`section_body`, `is_list_item`).
- It fails with a message that names the decision (its first 80 characters and its position) when:
  - the section is missing or has no list item;
  - an item has no `Source:` clause, or one that does not parse;
  - an item contains `Decided by: user`;
  - a path is absolute, holds `..`, or does not resolve to a regular file inside `tree` after `canonicalize` and a prefix check against the canonical tree (so a symlink cannot escape);
  - a `line` or `last` is 0, `first > last`, or beyond the file's line count.
- A spec whose plans touch no risky path still needs sourced decisions. It is the same rule for every intake spec, so a reviewer never has to guess which mode applies.
- Add `SpecRefusal::UnsourcedDecision(String)`. Its text is the message from `check_intake_decisions` and it displays bare, so the redraft findings (`intake.rs:480-485`) carry it to the drafter.

### 3. Intake mode in validation

- Add a parameter `intake_tree: Option<&Path>` to `create_job_with_spec_accepting` and `validate_series`. Every existing caller passes `None`, and `draft_jobs` passes `Some(&tree)`. `create_one_job_accepting` passes `None`.
- In `validate_series_inner`, when `intake_tree` is `Some`:
  - after `check_spec`, call `check_intake_decisions`; a failure returns `EngineError::SpecRefused(SpecRefusal::UnsourcedDecision(..))` and `accept_spec` is `None` for intake, so nothing can override it;
  - skip `risky_without_user_decision`, since the sourced check is stricter for risky and non-risky specs alike;
  - run the agent review with the intake flag of section 4.
- With `intake_tree` of `Some`, `run_series` and `create_job_with_spec_accepting` do not add `NO_USER_DECISION`. A warning `intake spec: N decisions, all sourced` is not added either: the ledger already holds the spec.
- The reason code does not change. Two refused attempts still end in `spec-review` (`intake.rs:486-492`).

### 4. The spec reviewer

- The driver adds one line to the reviewer prompt in intake mode: `This spec was drafted unattended from an issue. Every decision cites a source file that exists.`
- In all five `factory-spec-reviewer.md` files, add under the three finding kinds: "When the prompt says the spec was drafted unattended, finding kind 3 does not apply. Instead report as a blocker a decision whose cited source does not exist in the checkout, does not say what the decision claims, or is contradicted by other code you read. Open each cited source before you decide."
- Kinds 1 and 2 stay as they are.

### 5. The drafter

In all five `factory-intake-drafter.md` files:

- Add a `## Read first` section before `## Write the spec`: read `CLAUDE.md`, `docs/README.md`, `.claude/factory/config.toml` (its `risk_paths` name the risky areas), and the spec in `docs/superpowers/specs/` that is nearest to the area the issue touches, before writing anything. Settle each decision from what these say. Cite the file.
- Replace the line about `Decided by: call` with the section 1 format, and say that the driver checks every source and refuses a spec with a missing or unreal one.
- In the refuse section, say: when a decision has no answer in any file you can read and it touches a risky path from the config, write `refused.md` instead of inventing a source.
- Keep `Never write Decided by: user`.

### 6. Documentation

- `docs/guides/factory-user-guide.md` intake section: describe the sourced decisions rule, say that `spec-review` now also covers an unsourced decision, and keep the line that intake posts no agent text.
- `docs/reference/software-factory.md`: one sentence next to the risky-path rule saying intake uses sourced decisions in its place.

### 7. Parsing and plumbing rules

- The check runs only when `intake_tree` is `Some`, so this spec and every hand-written spec are exempt. Its own `## Decisions` has no `Source:` clauses on purpose.
- One decision is one line. A list item that wraps onto a continuation line is read as its first line only, so `Source:` and `Decided by: call` must sit on that line. The drafter prompt says so.
- The parser takes the last `Source:` on the line, up to the last `. Decided by: call`, and splits the refs on `, `. A ref ends at the first space or comma, so paths with spaces are not supported and are refused as an unreal path. A trailing `.` after the last ref is not part of it.
- The line count is `str::lines().count()`, so `a\nb\nc\n` has three lines. A file that is not valid UTF-8 is refused as a source.
- The reviewer flag is a `bool` parameter `intake: bool` on `review_spec_with` (`spec_check.rs:263`), set from `intake_tree.is_some()` in `validate_series_inner`. `review_spec_with` adds the prompt line when it is true. This replaces the phrase "the intake flag of section 4" in section 3.
- `UnsourcedDecision` displays its bare message. It must not carry `SPEC_OVERRIDE_HINT`, because `--accept-spec` has no effect in intake mode.
- The redraft already carries any `SpecRefusal` text: `intake.rs:480-485` sets `findings = Some(e.to_string())`, and `Display` writes the refusal. No new step is needed.
- A drafter that writes `refused.md` keeps the reason code `draft-refused` (`intake.rs:470-471`). No new code is added.
- The two callers of `validate_series` are `create_job_with_spec_accepting` and `one_job.rs:71`. Add the parameter at both and in the tests that call them. No options struct.
- The fake drafter and the intake integration fixtures write specs with `Decided by: call` and no `Source:` (no `Decided by` text is in `tests/intake.rs` or `tests/common/mod.rs`). Update them to write a `Source:` that names a file in the test tree, and add the file to the test repo.
- The tree comes from `Repo::export_tree` (`factory-git/src/repo.rs:254`). The symlink test creates the link in a temp tree directly, so it does not depend on what the export keeps.

## Error handling and edge cases

- A spec with `## Decisions` empty or missing: refused in intake mode with "the spec has no decisions".
- A `Source:` that names a file outside the tree, such as `~/.claude/CLAUDE.md` or an absolute path: refused, because the drafter cannot read it either.
- A source on a file the same change adds: refused, since it is not in the tree. The decision cites the closest existing file instead.
- A symlink in the tree that points outside it: refused after `canonicalize`.
- A very long `Decisions` section: all items are checked; the message names the first failing one only.
- `Decided by: call` text inside a code fence: ignored, because `section_body` and `is_list_item` already work on lines, and only list items count. A fenced line that starts with `- ` would count; the check does not parse fences, as `user_decisions` does not.
- The interactive flows pass `intake_tree = None` and behave exactly as before.

## Testing

All tests use `.expect("...")`, never the unwrap call.

- `check_intake_decisions_accepts_a_sourced_decision` (unit, `spec_check.rs`): one item with `Source: docs/a.md:2` on a tree holding a 3-line file passes.
- `check_intake_decisions_refuses_a_missing_source`, `..._a_user_decision`, `..._an_empty_section`, `..._a_path_outside_the_tree`, `..._a_symlink_escape`, `..._a_line_out_of_range` and `..._a_missing_file` (unit, `spec_check.rs`): each fails and the message names the decision.
- `check_intake_decisions_takes_a_range_and_two_refs` (unit): `path:2-3, other.md` passes.
- `validate_series_in_intake_mode_skips_the_risky_user_refusal` (unit, `runloop.rs`): a plan that touches a risky path and sourced decisions passes, where the same spec in normal mode is refused.
- `validate_series_in_intake_mode_refuses_an_unsourced_spec` (unit, `runloop.rs`): the result is `SpecRefused(UnsourcedDecision(..))` and `accept_spec` does not change it.
- `an_intake_job_has_no_user_decision_warning` (integration, `tests/intake.rs`): the created job has no `spec_warning` for the missing user answer.
- `an_unsourced_draft_is_redrafted_once_then_fails_spec_review` (integration, `tests/intake.rs`): the drafter runs twice, the second prompt holds the decision named by the refusal, and the outcome is `Failed { reason: "spec-review" }`.
- `a_risky_draft_with_sourced_decisions_creates_the_job` (integration, `tests/intake.rs`): the case that was a deadlock now creates the series.
- `the_reviewer_prompt_in_intake_mode_says_the_spec_was_drafted_unattended` (unit): the prompt line is present in intake mode and absent otherwise.
- The existing `factory-cli/tests/agent_lint.rs` runs over the five edited presets; add an assertion that every `factory-intake-drafter.md` holds `Source:` and `## Read first`, and that every `factory-spec-reviewer.md` holds `drafted unattended`.
- The existing one-job, draft-card and `new` tests keep passing, which covers the interactive flows.

## Files changed

- `crates/factory-engine/src/spec_check.rs`: `check_intake_decisions`, `SpecRefusal::UnsourcedDecision`, the reviewer prompt line, unit tests.
- `crates/factory-engine/src/runloop.rs`: the `intake_tree` parameter, the intake branch of `validate_series_inner`, the skipped warning, unit tests.
- `crates/factory-engine/src/one_job.rs`: passes `None`.
- `crates/factory-engine/src/intake.rs`: passes `Some(&tree)`.
- `crates/factory-engine/tests/intake.rs`: three integration tests.
- `crates/factory-core/presets/{rust,go,node,ruby,csharp}/agents/factory-intake-drafter.md` and `factory-spec-reviewer.md`.
- `crates/factory-cli/tests/agent_lint.rs`: the preset assertions.
- `docs/guides/factory-user-guide.md`, `docs/reference/software-factory.md`.

## Constraints

- Use `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Do not shell out to `git`. Do not read `/proc`. Paths reach the driver only through `Layout`.
- No board, ledger or events field is added. Board JSON stays compatible with `factory-server`.
- Intake still posts no agent-written text.
- Run `factory-cli/tests/agent_lint.rs` after editing the agent files.
- Code comments are short, full sentences.

## Acceptance criteria

- An intake spec whose decisions all cite an existing file creates its jobs, with or without risky paths in the plans, and carries no `NO_USER_DECISION` warning.
- An intake spec with a missing, unreal, out-of-tree or out-of-range source, or a `Decided by: user` line, is refused with a message that names the decision, redrafted once, and then fails with `spec-review`.
- `accept_spec` has no effect in intake mode.
- `factory new` and `factory new --one-job` behave as before, including the warning.
- All five presets carry the `## Read first` section, the `Source:` format and the intake-aware reviewer rule, and `agent_lint` passes.
- The user guide and the software factory reference describe the rule.

## Out of scope

- A cross-check that a source supports its decision beyond the reviewer's reading.
- The external knowledge base as a source.
- Intake file follow-ups (see Follow-ups).

## Follow-ups

- Intake drops the drafter's `## Follow-ups` section on the floor. Filing them as GitHub issues means posting agent-written text, which the intake design forbids. Decide whether a driver-written title with the spec path, and no agent text, is enough.
- A `refused.md` question is never shown to a person. The same rule blocks posting it. Decide whether the driver may copy a bounded, escaped excerpt.
- A mechanical check for decisions the docs already answer, such as the 076 reason code, is not feasible. The `## Read first` rule and the reviewer's source check are the only guards.

## Approaches

1. **Sourced decisions, driver-enforced (chosen).** Every decision cites a file in the tree, the driver verifies it, and the reviewer judges that it supports the decision. Checkable and keeps the safe default.
2. **Prompt-only.** Tell the drafter to read the docs and cite, and drop the warning. Cheap, but nothing verifies a citation and risky specs still hit the refusal.
3. **Intake bypass.** Accept risky specs when the reviewer approves. Removes the deadlock, but the reviewer cannot see the risk config and would be the only guard.

## Decisions

- Intake uses sourced decisions (approach 1) instead of a prompt-only rule or a bypass. Decided by: user
- A decision cites existing tracked files only, so the drafter and the driver read the same tree. Decided by: call
- The rule applies to every intake spec, not only risky ones, so no mode guess is needed. Decided by: call
- The reason code stays `spec-review` for an unsourced decision, so no new code is added. Decided by: call
- Intake keeps posting no agent-written text, so follow-ups and the `refused.md` question stay out of this change. Decided by: call
