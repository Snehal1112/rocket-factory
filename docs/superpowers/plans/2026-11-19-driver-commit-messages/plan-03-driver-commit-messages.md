# Driver commit messages: remove the committer agent (plan 3 of 4)

> **Spec:** docs/superpowers/specs/2026-11-19-driver-commit-messages-design.md (phase 1)
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Every task and every fix, rework or whole-plan round ends in a driver-made `wip:` snapshot commit, no committer agent ever runs, jobs stopped mid-batch by an older driver resume safely, and the five `commit_*` settings are accepted but ignored with a warning.

**Commit:** refactor(engine): remove the committer agent

**Architecture:** `build.rs` gets one snapshot helper used by the task loop and by `build_stage`; it keeps today's off-branch park and the empty-stage branches, and the note saving the previous plan added. The batch committer becomes a recovery step for old boards with four states, after which `commit.rs`, the commit context and the committer probe role are deleted. The five config fields stay in `Config` so old files load, are skipped when serializing, and `factory run` warns about each one a project file sets.

**Tech Stack:** Rust, factory-core, factory-git, factory-engine, factory-cli. No new dependency.

## Global Constraints

- Snapshot subjects: `wip: task <N> <title>` (unchanged), `wip: build the plan` for a whole-plan build, `wip: fix round <R>` for a fix or rework round (R = `job.round`).
- These parks keep their kind and text: the builder moving HEAD off the branch (`ParkKind::BuilderCommitted`, today's text in `build_task`'s snapshot block, with `in task <N>` or `in fix round <R>` or nothing for a whole-plan build); `task <N> changed nothing` and `builder made no changes` / `builder made no changes on resume` (`ParkKind::NothingChanged`); a check-started fix round that changed nothing still reruns the checks (`nothing_changed_after_checks`).
- The `save_commit_note` calls the previous plan added stay: in `build_task` after a made snapshot with `changed_paths(&tip, &sha)`, in `build_stage` after a made snapshot with `changed_paths(&tip_before, &head)`. The note tests in `build_checks.rs` stay green.
- `ParkKind::CommitFailed` stays in core for old boards; only the old-batch recovery parks with it.
- `batch_snapshot` stays an optional board field; the guards in `retry.rs:43`, `sync.rs:75`, `admit.rs:105` and `commands.rs:727` stay.
- Ignored keys: `commit_model`, `commit_subagents`, `commit_budget_weighted`, `commit_batch`, `commit_plugins`. Each one present in the project's config file prints one line on stderr when `factory run` starts: `warning: <key> has no effect, the driver writes commit messages`. No other command prints it.
- Old usage records with roles `committer` and `message` still show in `factory tokens` and `factory costs` (no change there).
- Cargo: `-j4`, one crate at a time (`-p <crate>`), never `--workspace`, `--all` or `CARGO_TARGET_DIR`.
- Tests use `.expect("...")`, never the unwrap call. Code comments are short full sentences that end with a period.
- Test changes: a scripted `committer` effect is dropped; an assertion about a committer spec, a committer role, a committer prompt or a committer park is deleted (that is not weakening, the behaviour is gone); an assertion on commit counts or subjects changes to the snapshot form; no other assertion is weakened.
- Line numbers below are from before the previous plan; use the function names to find the code.

## Review Focus

1. A three-task job: the job branch holds exactly three `wip: task` commits and the review head is the last one; no agent run with role `committer` happens. Pinned by `a_three_task_plan_keeps_three_snapshots_and_runs_no_committer`.
2. An old board whose committer reset the branch to the base and stopped, with only part of the work staged: the work is restaged, the branch moves back to the snapshot, the files are unchanged, the job goes to review. Pinned by `an_old_batch_partly_unstaged_at_the_base_recovers`.
3. An old board whose committer made one of several commits and stopped: the job parks with the `git reset --soft <base7>` hint, and following that hint recovers the job. Pinned by `an_old_batch_with_partial_commits_parks_with_the_reset_hint`.
4. A fix round where the builder detaches HEAD: it parks as `builder-committed` before any snapshot. Pinned by `a_fix_round_that_detaches_head_parks_before_the_snapshot`.
5. A project file with `commit_budget_weighted = 0` or `commit_plugins = ["two words"]`, which were refused before: it now loads, and `factory run` warns. Pinned by `an_ignored_commit_key_loads_even_when_it_was_invalid`.

## File Structure

- `crates/factory-engine/src/build.rs`: `Snapshot`, `snapshot`, the task loop and `build_stage` on snapshots, `resume_old_batch`.
- `crates/factory-git/src/stage.rs`: `staged_tree_is`.
- `crates/factory-engine/src/commit.rs`: deleted. `crates/factory-engine/src/lib.rs`, `compat.rs` (`with_engine`), `probe.rs`: committer code removed. `crates/factory-core/src/probe.rs`: `ProbeRole::Committer` removed.
- `crates/factory-core/src/layout.rs`, `project.rs`, `resolve.rs`: `commit_context_file` removed. `crates/factory-core/src/commitmsg.rs`: committer-only items removed.
- `crates/factory-core/src/config.rs`, `settings_block.rs`, `project.rs` (codex settings): the ignored keys. `crates/factory-engine/src/codex_prep.rs`: the committer and message roles. `crates/factory-cli/src/commands.rs` (`run`), `crates/factory-cli/src/project.rs`: warning and model check.
- Tests: `crates/factory-engine/tests/` (`build_checks.rs`, `common/mod.rs`, `commit.rs` deleted, and the files named per task), `crates/factory-git/tests/stage.rs`, `crates/factory-cli/tests/` (`e2e.rs`, `external_layout.rs`, `probe_cmd.rs`, `one_job.rs` for the binary setup).

---

### Task 1: Snapshot commits for every round

**Files:**
- Modify: `crates/factory-engine/src/build.rs` (`build_stage`, `commit_work`, `note_rewrites`, `build_task`'s commit block, `snapshot_subject`'s doc comment). Leave `resume_batch` and the code it calls untouched in this task: no new code sets `batch_snapshot`, so it runs only for old boards.
- Test: `crates/factory-engine/tests/build_checks.rs`; `codex_prep.rs` (`committer_message_and_kb_get_the_skill_sentence`, `committer_keeps_the_skill_sentence_only_when_the_commit_skill_is_missing`, `network_is_on_only_for_builder_roles`, `committer_gets_the_git_dir_as_a_writable_root`, `codex_builder_runs_in_the_project_folder`); `agent_files.rs` (385, 525); `escalation.rs` (`the_reviewer_and_committer_are_not_escalated` at 171); `job_model.rs` (49); `permission_pins.rs` (10, 29, 48); `fix_fresh.rs` (157); `park_evidence.rs` (32-40); `runloop.rs` (234); `crates/factory-cli/tests/e2e.rs` (role lists at 23-37, 59-68, 97, 219)

**Interfaces:**
- Produces:
  - `enum Snapshot { Made(String), Nothing, OffBranch(String) }` and `fn snapshot(&self, wt: &Worktree, subject: &str, place: &str, tip_before: &str) -> Result<Snapshot, EngineError>`: `head_is_on_branch` false gives `OffBranch(<today's park text, with place>)`; else `stage_all`, empty gives `Nothing`, else `commit_staged(wt, subject)` gives `Made(sha)` and logs `commit.done` with today's fields (`kind = "snapshot"`, `sha`, `fallback = false`; `task_carry.rs:386` asserts `fallback == false`). The `OffBranch` text is `builder moved HEAD off the job branch{place} (the branch was at ...` as today, with `place` = `" in task N"`, `" in fix round R"` or `""`.
  - `build_task` always snapshots (the `commit_batch` branch and the per-task committer branch go); after the last task it calls `finish_build(job, &base, new_tip, job.head.clone())` and never sets `batch_snapshot`.
  - `build_stage` replaces `commit_work` with `snapshot(.., "wip: build the plan" | "wip: fix round {R}", ..)`: `OffBranch` parks `BuilderCommitted`, `Nothing` keeps today's branches (`nothing_changed_after_checks` for `fix_source == "checks"`, else the `NothingChanged` parks with `nothing_changed_parts`), `Made(head)` goes to `finish_build` as today. `commit_work` and `note_rewrites` go.
  - Test changes, named: in `codex_prep.rs` keep only the kb part of `committer_message_and_kb_get_the_skill_sentence` (rename `kb_gets_the_skill_sentence`), delete `committer_keeps_the_skill_sentence_only_when_the_commit_skill_is_missing` and `committer_gets_the_git_dir_as_a_writable_root`, drop the committer asserts from `network_is_on_only_for_builder_roles` and `codex_builder_runs_in_the_project_folder`; rename `the_reviewer_and_committer_are_not_escalated` to `the_reviewer_is_not_escalated`; drop the committer line from the `permission_pins.rs` GOLDEN; delete the `park_evidence.rs` committer-rewrite case at 32-40; drop `"committer"` from the role lists in `runloop.rs:234` and `e2e.rs`; fix `agent_files.rs`, `job_model.rs` and `fix_fresh.rs` at the given lines the same way.

- [ ] **Step 1: Read** all of `build.rs`, and in `build_checks.rs` the tests from `a_whole_plan_build_commits_as_legacy` to `a_rework_note_left_by_a_batch_goes_to_the_ledger_and_a_fix_round_uses_the_findings` with their helpers, and each other test file at the given lines.
- [ ] **Step 2: Write the failing tests** in `build_checks.rs`:
  - `a_three_task_plan_keeps_three_snapshots_and_runs_no_committer` (replaces `a_three_task_plan_runs_one_committer_and_leaves_no_snapshots`).
  - `a_one_task_plan_makes_one_snapshot_and_goes_to_review` (replaces `a_one_task_plan_makes_one_snapshot_and_one_committer_run`).
  - `a_whole_plan_build_makes_one_wip_snapshot` (replaces `a_whole_plan_build_commits_as_legacy`, subject `wip: build the plan`).
  - `a_fix_round_snapshot_sits_above_the_reviewed_head` (replaces `a_fix_round_commits_above_the_reviewed_head`, subject `wip: fix round 2`, parent is the reviewed head).
  - `a_fix_round_that_detaches_head_parks_before_the_snapshot`.
  - Rename `an_empty_task_still_parks_with_the_batch_flag_on` to `an_empty_task_still_parks` and keep its assertions.
  - Delete the committer-only tests: `the_committer_prompt_names_the_kind_the_task_the_plan_and_the_base_as_floor`, `a_committer_that_rewrites_the_reviewed_head_parks_the_fix_round`, `a_committer_that_rewrites_an_earlier_task_commit_leaves_a_ruling`, `the_flag_off_keeps_one_committer_per_task`. Delete `a_parked_batch_resumes_on_the_next_build_call_without_new_builders` and `a_batch_committer_that_fails_twice_falls_back_to_one_chore_commit_per_snapshot` now. Rewrite only the helper `park_a_batch`: build both tasks with `scripted_auto` (the job reaches review on two `wip:` snapshots), then set `batch_snapshot = Some(tip)` and stage Build through `BoardStore::update_job`, and `reset_soft` the worktree to the base (the state an old committer leaves). Its callers stay unchanged and still run `resume_batch`. Leave the other batch tests (`a_park_right_after_the_soft_reset_...`, `a_finished_batch_with_a_stale_flag_...`, `a_branch_that_moved_to_other_work_...`, `a_stop_between_commit_groups_...`, `a_rework_note_left_by_a_batch_...`, `a_long_stale_note_is_cut_to_300_characters_in_the_ledger`) for the next task.
  - Also in `build_checks.rs`: drop `"committer"` from the role loop of `cache_ttl_reaches_every_agent_run_as_a_claude_variable` (2151) and delete the committer block of `every_agent_run_loads_only_its_roles_plugins` (2187).
- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine --test build_checks` and see the new tests fail.
- [ ] **Step 4: Implement** the Produces items and the named test changes.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli` and `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`. Expected: pass.

### Task 2: Old-batch recovery and the committer code deleted

**Files:**
- Modify: `crates/factory-git/src/stage.rs`, `crates/factory-engine/src/build.rs` (`resume_batch`, `batch_already_committed`, `keep_unsent_note`, `finish_batch`, `snapshot_refs`), `crates/factory-engine/src/lib.rs` (the commit exports, `commit_context`, `with_commit_context`, `commit_context_text`), `crates/factory-engine/src/compat.rs` (the `with_commit_context` lines in `with_engine`), `crates/factory-engine/src/probe.rs` (the `Committer` arm), `crates/factory-core/src/probe.rs` (57, 66, 75, 323), and the committer wording in the doc comments of `spec_goal` (`tasks.rs:59`), `message.rs` (1, 50, if still there) and `compat.rs::commit_message` (672)
- Delete: `crates/factory-engine/src/commit.rs`, `crates/factory-engine/tests/commit.rs`
- Test: `crates/factory-git/tests/stage.rs`; `crates/factory-engine/tests/common/mod.rs` (`Fixture::engine` at 152-157 returns the engine directly; its commit-context branch and the `with_commit_context` call go); `crates/factory-engine/tests/build_checks.rs` (the batch tests left by the task before); `crates/factory-engine/tests/codex_prep.rs` (`fallback_commit_runs_when_a_codex_committer_fails`, near 582-615, uses `CommitRequest`, `CommitKind` and `commit_step`: delete it); `crates/factory-engine/tests/probe.rs` (`append_delivery_puts_the_marker_in_agent_prompt` at 98 is retargeted to `ProbeRole::Kb` and its agent name); `crates/factory-cli/tests/probe_cmd.rs`

**Interfaces:**
- Produces:
  - `pub fn staged_tree_is(&self, wt: &Worktree, rev: &str) -> Result<bool>` in `factory-git/src/stage.rs`: true when the tree written from the worktree's index has the same id as the tree of `rev`.
  - `fn resume_old_batch(&self, job: &JobView) -> Result<StageOutcome, EngineError>` replaces `resume_batch` and is called where `build_stage` checks `batch_snapshot`. With `tip` the branch tip and `snapshot` the field:
    - `tip == snapshot`: ruling `old batch commit skipped, continuing at <short>`, `keep_unsent_note`, clear the field, `finish_build` on `tip`.
    - `tip == base`: call `stage_all(wt)`, then `staged_tree_is(wt, snapshot)`. When true: `reset_soft(wt, snapshot)`, the same ruling, `keep_unsent_note`, clear the field, `finish_build` on `snapshot`. When false: park `CommitFailed` with `the worktree does not match the old batch snapshot <short>; restore the files, then factory rework <id>`.
    - `tip != base` and `batch_already_committed(base, tip, snapshot)`: ruling `old batch commit already done, the branch is at <short>`, `keep_unsent_note`, clear the field, `finish_build` on `tip`.
    - anything else: today's `CommitFailed` park with the `git reset --soft <base7>` text.
  - `finish_batch`, `snapshot_refs`, `commit.rs` and every `CommitKind`, `CommitRequest`, `CommitOutcome`, `SnapshotRef`, `commit_step` and committer constant go. `Engine` has no `commit_context`, `with_commit_context` or `commit_context_text`.
  - `ProbeRole` without `Committer`; `factory probe --role committer` is refused by clap as an unknown value.
  - `grep -rn "committer\|commit_context" crates/factory-engine/src` finds only comments about old boards and old usage, and the `codex_prep.rs` role checks, which the next task removes.

- [ ] **Step 1: Read** `resume_batch` and the functions it calls, `stage.rs`, `crates/factory-git/tests/stage.rs`, the batch tests in `build_checks.rs`, `lib.rs`, `core/src/probe.rs` and `tests/probe_cmd.rs`.
- [ ] **Step 2: Write the failing tests**:
  - `crates/factory-git/tests/stage.rs`: `staged_tree_is_compares_the_index_with_a_commit` (index equal to a commit's tree: true; after staging one more change: false).
  - `build_checks.rs`, each making the old board by setting `batch_snapshot` and the branch state by hand with git2: `an_old_batch_at_its_snapshot_continues_to_review`; `an_old_batch_reset_to_the_base_moves_back_to_the_snapshot` (replaces `a_park_right_after_the_soft_reset_resumes_from_the_base`; the worktree files are byte-equal before and after); `an_old_batch_partly_unstaged_at_the_base_recovers` (tip at base, the index holds a subset, the files on disk equal the snapshot: the job reaches review); `an_old_batch_the_committer_finished_continues_on_its_tip` (replaces `a_finished_batch_with_a_stale_flag_is_recognised_without_a_committer_run`); `an_old_batch_with_partial_commits_parks_with_the_reset_hint` (replaces `a_stop_between_commit_groups_resumes_without_losing_or_repeating_a_commit`; then run `reset_soft` to the base as the hint says and build again: the job reaches review). Keep `a_branch_that_moved_to_other_work_parks_with_the_recovery_command`, `a_rework_note_left_by_a_batch_goes_to_the_ledger_and_a_fix_round_uses_the_findings` and `a_long_stale_note_is_cut_to_300_characters_in_the_ledger`, adapted to an old board (the rewritten `park_a_batch` helper makes one and stays).
  - `probe_cmd.rs`: `probe_refuses_the_committer_role` (`factory probe --role committer` exits non-zero and stderr names the allowed values); `core/src/probe.rs`: the role list test asserts the list has no `committer`.
- [ ] **Step 3: Run** `cargo test -j4 -p factory-git --test stage`, `cargo test -j4 -p factory-engine --test build_checks`, `cargo test -j4 -p factory-core probe` and `cargo test -j4 -p factory-cli --test probe_cmd`, and see the new tests fail.
- [ ] **Step 4: Implement** the Produces items.
- [ ] **Step 5: Run** `cargo test -j4` for factory-git, factory-core, factory-engine and factory-cli, one at a time, and `cargo clippy -j4 --all-targets -- -D warnings` for factory-git, factory-core and factory-engine. Expected: pass.

### Task 3: Ignore the commit settings and clean up

**Files:**
- Modify: `crates/factory-core/src/config.rs` (the five fields, their defaults, `plugins_for`, `named_skill_for`, the budget check at 379-386, the model lists at 425-450, the plugin-name check at 581-596), `crates/factory-core/src/settings_block.rs` (52-61, 145), `crates/factory-core/src/project.rs` (codex settings at 175-183, `commit_context_file` at 406-420), `crates/factory-core/src/layout.rs` (116-132, 328), `crates/factory-core/src/resolve.rs` (823-826), `crates/factory-core/src/commitmsg.rs` (committer-only items), `crates/factory-core/src/lib.rs` (their re-exports), `crates/factory-engine/src/codex_prep.rs` (64, 113, tests 179-214), `crates/factory-cli/src/project.rs` (394), `crates/factory-cli/src/commands.rs` (`run`)
- Test: `config.rs` tests (1286-1314, 1352-1367, 1444-1447, 1716-1718, 1792-1815, 1846-1849, 1890-1892), `settings_block.rs` tests, `project.rs` tests (563-578, 890-895, 1089-1091), `crates/factory-core/tests/rocket_parity.rs`, `crates/factory-engine/tests/common/mod.rs` and its users, `crates/factory-engine/tests/task_carry.rs` (355-387), `crates/factory-cli/tests/external_layout.rs` (39, 48), a CLI test file for the warning

**Interfaces:**
- Produces:
  - The five fields stay in `Config` with their types and `#[serde(skip_serializing)]`, each with the doc comment `Ignored: the driver writes commit messages. Kept so old files load.` They are never read by the engine.
  - `Config::parse` no longer checks `commit_budget_weighted`, `commit_model` (the empty-name and Fable checks drop that key) or `commit_plugins` (the plugin-name check drops it); the error text of the remaining budget check becomes `kb_budget_weighted and spec_review_budget_weighted must be above zero`.
  - `pub const IGNORED_COMMIT_KEYS: [&str; 5]` and `pub fn ignored_keys_in(text: &str) -> Vec<&'static str>` in `config.rs`: the ignored keys that the TOML text sets at the top level or in its `[settings]` table, in the order of the constant; an unparsable text gives an empty list.
  - `pub fn ignored_key_warnings(config_file: &Path) -> Vec<String>` in `factory-cli/src/commands.rs`: one `warning: <key> has no effect, the driver writes commit messages` per key from `ignored_keys_in` (a missing file gives none). `run` prints each to stderr before `ctx.runnable()`.
  - `plugins_for` and `named_skill_for` drop the `committer` and `message` arms; `codex_prep.rs` drops both roles from its role checks and tests.
  - `settings_block.rs` drops `commit_model` and the `Commits.` group; `project.rs` stops writing `commit_model` for a Codex project; `factory-cli/src/project.rs` drops `commit_model` from its model check.
  - `Layout` without `commit_context_file` (and `project.rs` stops computing it).
  - Test fixtures: `committing_fixture()` becomes `Fixture::new()` and `scripted_auto(` becomes `scripted(` in every file under `crates/factory-engine/tests`, with the `use common::{..}` lines fixed; `write_commit_context`, the commit-context fixture, `auto_committer`, `scripted_auto`, the committer arm near `common/mod.rs:588` and the `with_context` parameter of `approved_job_with`, `approved_job_config` and `approved_job_dyn` go, with every caller updated; `task_carry.rs` drops its `commit_batch = true` line; `external_layout.rs` drops `commit_context_file`.
  - In `commitmsg.rs`, grep each of `MAX_COMMITS_PER_RUN`, `fallback_subject`, `extract_message`, `MESSAGE_BEGIN`, `MESSAGE_END`; delete each item and its tests when only `commitmsg.rs` uses it (a re-export in `crates/factory-core/src/lib.rs` at 48-52 is not a use; drop it with the item). Drop the leftover `fx.config.commit_batch = ...` lines in `build_checks.rs` (433, 1684 with its `for batch in [true, false]` loop, 2668).

- [ ] **Step 1: Read** the listed parts of `config.rs`, the `settings_block.rs` test `every_config_key_is_listed_once`, `codex_prep.rs`, `run` in `commands.rs`, `tests/common/mod.rs`, and the `CARGO_BIN_EXE_factory` setup in `crates/factory-cli/tests/one_job.rs`.
- [ ] **Step 2: Write the failing tests**:
  - `config.rs`: `an_ignored_commit_key_loads_even_when_it_was_invalid` (`commit_budget_weighted = 0`, `commit_model = ""`, `commit_model = "fable"` and `commit_plugins = ["two words"]` all parse); `ignored_keys_in_lists_the_set_keys_in_order` (`commit_batch = false\ncommit_model = "x"` gives `["commit_model", "commit_batch"]`; the same keys under `[settings]` give the same; a text with none gives `[]`); `the_ignored_keys_are_not_serialized` (serialized `Config::default()` has no `commit_` key). Update the existing tests that assert the old defaults or the old errors, and the `project.rs` test at 563-578 that expects `commit_model` for a Codex project.
  - `codex_prep.rs` (engine tests): the role table has no `committer` or `message` rows.
  - CLI: `ignored_key_warnings_lists_each_set_key` in the `#[cfg(test)]` module of `commands.rs` (`commands` is a private module; a config with `commit_model` and `commit_batch` gives two lines); `factory_run_warns_once_per_ignored_key` and `factory_status_does_not_warn` in `crates/factory-cli/tests/one_job.rs`, using its `Fixture` and `--root` (the config file is `.claude/factory/config.toml`): `factory run 999` for a job that does not exist prints the two warnings on stderr, then the error; `factory status` with the same config prints no `has no effect` line. `run` passes `&ctx.layout.config_file`.
- [ ] **Step 3: Run** `cargo test -j4 -p factory-core config`, `cargo test -j4 -p factory-engine --lib codex_prep`, `cargo test -j4 -p factory-cli commands` and `cargo test -j4 -p factory-cli --test one_job`, and see the new tests fail (in `config.rs`, add `named_skill_for("committer") == None` and `plugins_for("message").is_empty()` as the failing asserts).
- [ ] **Step 4: Implement** the Produces items. Keep the `commit_*` keys in `crates/factory-core/tests/fixtures/rocket-project.toml`.
- [ ] **Step 5: Run** `cargo test -j4` for factory-core, factory-engine, factory-cli and factory-supervisor, one at a time, and `cargo clippy -j4 --all-targets -- -D warnings` for each of them. Expected: pass.

Next plan: docs/superpowers/plans/2026-11-19-driver-commit-messages/plan-04-driver-commit-messages.md
