# Job creation, merge message and recovery test leftovers

**Commit:** fix(factory): job creation and message leftovers

## Goal

Finish the leftovers that issue 132 lists for issues 4, 121, 123 and 126. After this, a failed job creation leaves no half-made job, factoryd can create a one-job, every merge message rule is pinned by a test, and the recovery and config paths have the tests they are missing.

## Overview

Issue 132 keeps the open parts of issues 4 (one-job), 121 (merge message), 123 (test gaps) and 126 (builder recovery). A survey of main at `6cd4e92` confirmed that each item below is still open. Most of them are tests. Four change behaviour:
- a failed write during `factory new` leaves a job on the board;
- factoryd cannot create a one-job;
- a fix round without a session is committed as `wip: build the plan`;
- `build_job_message` can repeat a footer, and can put `- and N more changes` under a section with no bullets.

## Current state

| Area | Today | Gap |
|---|---|---|
| Job creation | `add_job_marked` (`crates/factory-engine/src/runloop.rs:32-110`) pushes the job to the board (`:52-71`), then creates the folder (`:74`), the ledger (`:75-78`) and the series `plan.md` copy (`:80`). `create_one_job_accepting` (`one_job.rs:127-140`) writes `plan.md` and `spec.md` after `add_job`. `create_job_with_spec_accepting` (`runloop.rs:510-628`) adds each series job, then writes its `spec.md`. | A failed write leaves a job on the board, which later parks as "no plan.md". In a series, the jobs made before the failure stay. Only `create_replacement` (`runloop.rs:820-827`) cleans up after itself, with inline code. |
| Count check | `one_job.rs:118-125` refuses when `parse_checkbox_tasks(&combined.text).len() != combined.tasks`. | No test can reach it, because `combine_plans` never produces a mismatch. |
| `[x]` marks | `split_plan` (`combine.rs:265`, the `as_headings` branch at `:334-355`) strips `- [ ] ` and `- [x] ` when a checkbox plan is written as headings, which happens only when the plans mix both forms (`combine.rs:70-72`). | The dropped mark is neither documented nor tested, and the mixed-form warning has no test. |
| Task cap | `parse_series_with(root, start, max_tasks)` (`series.rs:130`). Tests cover `None` and the default `Some(3)`. | No test of another cap. |
| factoryd create | `CreateBody` (`crates/factory-supervisor/src/api/write.rs:25-36`) has `deny_unknown_fields` and no one-job field. A `needs consent:` exit 1 maps to 500 (`finished`, `write.rs:192-212`). The dashboard form (`dashboard/static/dashboard.html:28-40`, `projects.js:61-77`) has no one-job input. | A one-job cannot be created over the API or from the dashboard. |
| Ruling repeats | `job_message` (`jobmsg.rs:51-64`, `:118-130`) appends its "plan **Commit:** line refused" and "fixed message used" rulings on every call. It runs at each merge attempt (`compat.rs:703`). | A retried merge writes the same ruling twice. |
| Fixed message | `jobmsg.rs:118-130` falls back to `fixed_message` when `validate_message` fails. No input reaches it today: `title_subject` drops a trailing period (`cut_text`, `commitmsg.rs:608`) and the `**Commit:**` line is checked first. It stays as a guard. | No test pins that a title ending in a period still gets a real subject. |
| Loose test | `a_plan_without_a_commit_line_uses_the_plan_title` (`tests/job_message.rs:146`) asserts with `\|\|`. | It does not pin the exact `title_subject` output. |
| Footers | `build_job_message` (`commitmsg.rs:705-751`) drops a footer equal to a body line, but not a footer repeated in `m.footers`. The `- and N more changes` line goes to `blocks.last_mut()` (`:729-733`), which can be a title-only section. | Repeated footers, and a misplaced "and N more" line. |
| Fix round with no session | `build.rs:150`: `new_work = !fresh_fix && (fresh \|\| job.session_id.is_none())`. `session_id` is cleared by `sync_provider` (`ports.rs:425`) and `clear_tree_fields` (`series_guard.rs:113`). | A fix round on an existing worktree with no session is committed as `wip: build the plan`, and its note is written to `build.md`, which a later build overwrites. |
| End-to-end message | `tests/job_message.rs` writes the notes by hand. | No test runs a task job and a fix round through to the merge commit. |
| Sync message | `sync_message_uses_relates_to_then_the_job_footer` (`tests/message.rs:112`) checks only the end of the sync commit message. | The sync subject is not pinned. |
| Index | `staged_tree_is` (`crates/factory-git/src/stage.rs:59`) says it leaves the index alone. `staged_tree_is_compares_the_index_with_a_commit` (`crates/factory-git/tests/stage.rs:98`) checks only its answers. | Nothing pins the claim. |
| Note ruling | `keep_unsent_note` (`build.rs:942-956`) writes "rework note not sent to a builder because the batch commit ran no builder". | The batch committer is gone. The text should name the old batch recovery. |
| Ignored keys | `ignored_keys_in` (`crates/factory-core/src/config.rs:21-30`). | No test of a key set in both places, or of a commented-out key. |
| `/factory` step 3 | `crates/factory-core/assets/global-command.md:93` checks the plan `**Commit:**` lines only. | A `--one-job` spec's `**Commit:**` line is not checked. |
| Recovery tests | `wait_ends_on_cancel` (`tests/builder_recover.rs:289`) does not check `machine_wait`. Nothing tests a failed run whose error says `No space left on device`, or a disk wait before an Other rerun (`Rerun::run`, `recover.rs:125-136`). | Untested paths. |
| Unit test config | `runloop.rs:1074` and `:1123`, `drafts.rs:184` and `park.rs:635` build drivers or engines on `Config::default()`, so the real 2048 MB disk check runs. | A later test that reaches `build_stage` would hang on a host with less free space. |

## Non-goals

- No change to the 3-task cap, the 6-plan cap or the one-job estimate.
- No change to the merge message format beyond the two `build_job_message` fixes.
- No change to the disk, network or auth wait logic.
- `parse_checkbox_tasks` stays public.

## Design

```
factory new ─ add jobs ─ write plan/spec ─┬─ ok   → keep the jobs
                                          └─ error → remove every job of this call, return the error
merge ─ job_message ─ ruling (written once) ─ build_job_message (one footer each, "and N more" under bullets)
build ─ fix round with no session ─ "wip: fix round N", round-N.md
```

### 1. Roll back a failed job creation (issue 4)

In `crates/factory-engine/src/runloop.rs`:
- `pub(crate) fn remove_new_jobs(driver: &Driver, jobs: &[Job])`: one board write that drops every job whose id is in `jobs`, then `remove_dir_all` on each job folder. A folder that does not exist is skipped without a warning. Each other failure is logged as a `tracing::warn!` with `event = "job.rollback"` and never returned, so the caller's error is the one reported.
- `pub(crate) fn rolled_back<T>(driver: &Driver, f: impl FnOnce(&mut Vec<Job>) -> Result<T, EngineError>) -> Result<T, EngineError>`: runs `f` with an empty list that `f` fills with each job it creates. When `f` returns `Err`, it calls `remove_new_jobs` on the list and returns the error. When `f` returns `Ok`, it changes nothing.

Callers:
- `add_job_marked`: after the board push, the folder, ledger and series `plan.md` copy run in one block. When it fails, `remove_new_jobs(driver, &[job])` runs and the error is returned. The estimate write stays best effort.
- `create_one_job_accepting`: `add_job` and the `plan.md` and `spec.md` writes run inside `rolled_back`.
- `create_job_with_spec_accepting`: every `add_job` and `copy_spec` of the series run inside `rolled_back`. The plan-less `add_job` needs no wrapper, because `add_job_marked` removes its own job on a failed write.
- `create_replacement`: its inline cleanup (`runloop.rs:820-827`) becomes `remove_new_jobs(driver, std::slice::from_ref(&new))`.

`adopt_draft`, the warnings and `record_accepted` stay outside the rollback, as today: their failures keep the job.

### 2. Count check as a pure function (issue 4)

In `one_job.rs`: `fn count_mismatch(text: &str, expected: usize) -> Option<String>`. It returns `Some("combining the plans failed: the combined plan has {found} tasks but {expected} were expected")` when `parse_checkbox_tasks(text).len() != expected`, else `None`. `create_one_job_accepting` calls it in place of the inline check, with the same text.

### 3. `[x]` marks and the mixed-form path (issue 4)

The behaviour stays. The doc comment of `split_plan` gains: "A checked `- [x]` mark is dropped. The ledger, not the mark, says which tasks are done." Tests pin both the dropped mark and the warning.

### 4. Task cap boundary (issue 4)

A test of `parse_series_with(root, start, Some(2))` accepts 2 tasks and refuses 3 with `SeriesError::TaskCount { found: 3, .. }`. The error text keeps saying "1 to 3", because 3 is the only cap the code uses.

### 5. factoryd and the dashboard create a one-job (issue 4)

- `CreateBody` gains `one_job: Option<bool>` and `accept_over_budget: Option<bool>`.
- `accept_over_budget: true` without `one_job: true` answers 400 `bad_request` with "accept_over_budget needs one_job", and runs nothing. This mirrors the CLI's `requires = "one_job"`.
- `one_job: true` adds `--one-job`, and `accept_over_budget: true` adds `--accept-over-budget`. Both go before `--`, after `--accept-spec`.
- `factory_engine` exports `pub const NEEDS_CONSENT: &str = "needs consent:"`. `create_inner` treats an exit 1 as a conflict when any line of stderr, or of stdout when stderr is blank, starts with `NEEDS_CONSENT` after `trim_start`. Under `--project` the CLI may print `note:` lines first. It answers 409 `conflict` with the tail of the text, as a spec refusal does.
- The dashboard form gets a checkbox `nj-onejob`, "One job", unchecked by default. `buildCreateBody` adds `one_job: true` only when it is checked, so a body without it is unchanged.
- README's `POST /projects/{p}/jobs` row documents both fields and the 409.

### 6. Merge rulings are written once (issue 121)

In `ports.rs`: `pub(crate) fn ruling_once(&self, text: &str) -> Result<(), EngineError>`. It folds `text` as `ruling` does, reads `progress.md` (a missing file reads as empty), and appends only when no line equals `Ruling: <folded text>`. `job_message` uses it for both its rulings.

### 7. Footers and the "and N more" line (issue 121)

In `build_job_message`:
- A footer is kept only on its first appearance in `m.footers`, and only when no body line equals it. The order stays.
- The `- and {dropped} more changes` line goes to the last block that holds a line starting with `- `. When no block does, it becomes a block of its own after the others.

### 8. A fix round with no session is a fix round (issue 121)

In `build.rs`: `let new_work = !fresh_fix && (fresh || (job.session_id.is_none() && job.round <= 1));`. A fix round (`job.round > 1`) on an existing worktree is then always committed as `wip: fix round N` and noted in `round-N.md`, whatever happened to the session. A fresh worktree, or a round 0 or 1 build, keeps today's behaviour.

### 9. Wording and the `/factory` command (issue 123)

- `keep_unsent_note` writes "rework note not sent to a builder because the old batch recovery ran no builder: {kept}".
- `global-command.md` step 3 gains, after the plan `**Commit:**` sentence: "With `--one-job`, also check the spec's `**Commit:**` line under its title by the same rule. If the line is missing, the subject is `feat: <spec title>`." `plugins/factory/commands/factory.md` is regenerated with `cargo run -j4 -q -p factory-cli --bin factory -- plugin sync`.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| Folder creation fails after the board push | The job is removed from the board and the error is returned. |
| `plan.md` write fails on the one-job path | The job and its folder are removed, and the write error is returned. This runs through `rolled_back`, which `a_failed_creation_removes_every_job_it_made` tests. No test makes the write itself fail. |
| The third job of a series fails | The first two jobs and their folders are removed too. |
| The rollback's own board write fails | It is logged, and the original error is returned. |
| `adopt_draft` fails | The job stays, as today. |
| `accept_over_budget` without `one_job` | 400, nothing runs. |
| `one_job: false` | No flag, same as leaving it out. |
| `needs consent:` refusal | 409 `conflict` with the estimate text, also when `note:` lines come first. |
| Any other `factory new` failure | 500 `failed`, as today. |
| `job_message` called twice | Each ruling appears once in the ledger. |
| Ledger missing when the ruling is checked | Read as empty, so the ruling is written. |
| A footer listed twice | It appears once. |
| The last section has no bullets, and bullets were dropped | "and N more" goes under the last section that has bullets. |
| A fix round at round 2 with no session | `wip: fix round 2`, `round-2.md`. |
| A fresh worktree at round 2 | `wip: build the plan`, as today. |

## Testing

factory-engine, `src/runloop.rs` `mod tests`:
- `a_failed_creation_removes_every_job_it_made`
- `a_finished_creation_keeps_its_jobs`
- `unit_test_drivers_skip_the_disk_check`

factory-engine, `tests/one_job.rs`: `a_failed_job_folder_leaves_no_job_on_the_board`.

factory-engine, `tests/intake.rs`: `a_job_creation_io_error_fails_at_once_without_a_redraft` also asserts that the board holds no job.

factory-engine, `src/one_job.rs` `mod tests`: `count_mismatch_names_both_counts`, `count_mismatch_is_none_when_the_counts_agree`.

factory-engine, `src/combine.rs` `mod tests`: `mixed_forms_write_checkbox_tasks_as_headings_with_a_warning`, `mixed_forms_drop_the_done_mark`.

factory-engine, `tests/series.rs`: `parse_series_with_a_cap_of_two_refuses_three_tasks`.

factory-engine, `tests/job_message.rs`:
- `a_second_merge_message_writes_the_refused_line_ruling_once`
- `a_title_ending_in_a_period_drops_the_period`
- `a_plan_without_a_commit_line_uses_the_plan_title` (exact subject)
- `a_plan_without_a_heading_uses_the_feature` (exact subject)
- `a_task_job_with_a_fix_round_merges_with_its_sections_and_review_fixes`

factory-engine, `tests/message.rs`: `the_sync_commit_has_the_fixed_subject_and_footers`.

factory-engine, `tests/build_checks.rs`: `a_fix_round_without_a_session_is_still_a_fix_round`, and `a_rework_note_left_by_a_batch_goes_to_the_ledger_and_a_fix_round_uses_the_findings` asserts the new wording.

factory-engine, `tests/builder_recover.rs`: `wait_ends_on_cancel` asserts `machine_wait` is None, plus `a_failed_disk_run_waits_for_the_disk_then_reruns` and `an_other_failure_waits_for_the_disk_before_its_rerun`.

factory-core, `src/commitmsg.rs` `mod tests`: `a_repeated_footer_appears_once`, `a_footer_equal_to_a_body_line_is_left_out`, `and_n_more_goes_under_the_last_section_with_bullets`.

factory-core, `src/config.rs` `mod tests`: `ignored_keys_in_lists_a_key_set_in_both_places_once`, `ignored_keys_in_skips_commented_keys`.

factory-git, `tests/stage.rs`: `staged_tree_is_leaves_the_index_and_files_alone`.

factory-supervisor, `src/api/write.rs` `mod tests`:
- `create_passes_one_job_and_accept_over_budget_before_the_separator`
- `create_refuses_accept_over_budget_without_one_job`
- `create_answers_needs_consent_with_a_conflict`, also with a `note:` line first

factory-cli: `tests/global_command.rs` `the_command_asks_for_a_commit_line_in_every_plan` asserts that step 3 names the spec's `**Commit:**` line, and `tests/plugin_files.rs` passes after the sync.

dashboard, `tests/projects.test.js`: `buildCreateBody adds one_job only when checked`.

Commands: `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-git --test stage`, `cargo test -j4 -p factory-supervisor`, `cargo test -j4 -p factory-cli --test global_command --test plugin_files --test agent_lint`, and `node --test dashboard/tests/*.test.js`.

## Files changed

- `crates/factory-engine/src/`: `runloop.rs`, `one_job.rs`, `combine.rs`, `jobmsg.rs`, `ports.rs`, `build.rs`, `drafts.rs`, `park.rs`, `lib.rs`
- `crates/factory-engine/tests/`: `one_job.rs`, `intake.rs`, `series.rs`, `job_message.rs`, `message.rs`, `build_checks.rs`, `builder_recover.rs`
- `crates/factory-core/src/`: `commitmsg.rs`, `config.rs`
- `crates/factory-core/assets/global-command.md`, `plugins/factory/commands/factory.md`, `crates/factory-cli/tests/global_command.rs`
- `crates/factory-git/tests/stage.rs`
- `crates/factory-supervisor/src/api/write.rs`
- `dashboard/static/dashboard.html`, `dashboard/static/projects.js`, `dashboard/static/app.js`, `dashboard/tests/projects.test.js`
- `README.md`

## Constraints

- Cargo commands are per crate with `-j4`, never `--workspace` or `--all`, and never set `CARGO_TARGET_DIR`.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- Board JSON is unchanged. No new board field.
- Git stays behind `factory_git::Repo`, and tests read git state with `git2` directly, as `stage.rs` does.
- The dashboard tests run with `node --test`, as CI does (`.github/workflows/ci.yml:63`).
- The rollback never hides the original error.

## Acceptance criteria

- A one-job or series creation that fails after a board write leaves no job and no folder from that call.
- `create_replacement` uses `remove_new_jobs`.
- The count check is a tested pure function with the same refusal text.
- A test pins the dropped `[x]` mark and the mixed-form warning.
- A test pins a task cap of 2.
- factoryd passes `--one-job` and `--accept-over-budget`, refuses the second without the first, and answers `needs consent:` with 409.
- The dashboard sends `one_job: true` only when its checkbox is checked.
- A repeated `job_message` call writes each ruling once.
- A title ending in a period, and the exact title and feature subjects, are tested.
- `build_job_message` lists each footer once and puts "and N more" under bullets.
- A fix round with no session is committed as `wip: fix round N` with `round-N.md`.
- A task job with a fix round merges with its task sections and "Review fixes" in the merge commit.
- The sync commit's full message is pinned.
- `staged_tree_is` is shown to leave the index and the files alone.
- The `keep_unsent_note` ruling names the old batch recovery.
- `ignored_keys_in` is tested for a key set twice and for commented keys.
- `/factory` step 3 checks a `--one-job` spec's `**Commit:**` line, and the plugin copy matches.
- `wait_ends_on_cancel` checks `machine_wait`, and the disk-failure and Other-rerun disk waits are tested.
- No unit test in `crates/factory-engine/src` runs the real disk check.
- clippy finishes with no warnings, and `cargo fmt` changes nothing.

## Out of scope

- The 4000-line diff cap and fix-round resume risks listed in issue 4. They are watch items, not work.
- Issue 122 and every other item of issue 132, which the area 3 and area 4 specs cover.

## Decisions

- When a write fails after a job is on the board? Options: roll back the job, leave it and report, retry the write. Chosen: roll back, with one helper for the one-job and series paths that removes the board entry and the folder. Decided by: user
- What happens to `[x]` marks dropped when plans are combined? Options: document and pin, keep the mark, refuse mixed plans. Chosen: document and pin, because the ledger decides which tasks are done. Decided by: user
- How does factoryd create a one-job? Options: add `one_job` and `accept_over_budget` with a 409 for `needs consent` and a dashboard checkbox, add `one_job` only, leave it to the CLI. Chosen: both fields, the 409 and a dashboard one-job checkbox. Decided by: user
- The dashboard gets no over-budget checkbox. A refusal shows the needs-consent text in the banner. Decided by: call
- The count check becomes a pure function, `count_mismatch`, so it can be tested without a real mismatch. Decided by: call
- The task-count error keeps saying "1 to 3", the only cap the code uses. Decided by: call
- A ruling is written once by checking the ledger for the same line, not by moving it out of `job_message`. Decided by: call
- The fix-round signal is `job.round > 1` on an existing worktree, so a fresh worktree keeps the build path. Decided by: call
- Repeated footers keep their first place. "and N more" goes under the last block with bullets. Decided by: call
- The fixed-message fallback stays as an untested guard, because no input reaches it. A test pins that a title ending in a period gets a real subject. Decided by: call
