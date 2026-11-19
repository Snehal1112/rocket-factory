# Driver commit messages: builder notes and the driver-built merge message (plan 2 of 4)

> **Spec:** docs/superpowers/specs/2026-11-19-driver-commit-messages-design.md (phase 1)
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Builders end every reply with a commit note, the driver stores the notes per task and round, and the merge and sync messages are built by the driver instead of the message agent.

**Commit:** feat(engine): write merge messages from notes

**Architecture:** Every builder prompt and every preset builder file asks for the note. A new `notes.rs` saves each checked note (or the fallback) to `<job folder>/commit-notes/` and reads them back. A new `jobmsg.rs` assembles the merge message from the plan's `**Commit:**` line, its Goal line and the notes, using the core helpers from the previous plan. `Driver::commit_message` keeps its signature, so `merge.rs` and `sync.rs` do not change; the message agent code in `message.rs` goes. The committer agent still runs in this plan; the next plan removes it.

**Tech Stack:** Rust, factory-core, factory-engine, preset Markdown files. No new dependency.

## Global Constraints

- Note files: `commit-notes/task-<N>.md` for plan task N, `commit-notes/round-<R>.md` for a fix or rework round R (R = `job.round`), `commit-notes/build.md` for a whole-plan (legacy) build. A rerun overwrites its file. File text is `CommitNote::to_file_text`.
- A missing, malformed or unwritable note never parks and never fails a stage. It writes one ledger ruling: `commit note for <label>: <reason>, fallback used` (or `commit note for <label> not saved: <error>`). A git error while listing the changed files gives an empty file list, never an error.
- The merge message is the section 4 layout of the spec: subject, Goal paragraph, task sections, `Review fixes:`, then the footers of today's `driver_footers` (`Fixes: #N` for a single job or the last plan of a series, else `Relates to: #N`; always `Factory-Job: <id>`). The sync message is `fixed_message(MessageKind::Sync, req.job_id, req.slug, self.repo.base_branch())` plus the same footers, with no agent run. The fourth argument of `fixed_message` is always the base branch name, never a sha.
- No agent run with role `message` happens anywhere after this plan.
- The builder still never commits. The phrase `driver commits them` stays in every builder file (`agent_lint` pins it).
- Cargo: `-j4`, one crate at a time (`-p <crate>`), never `--workspace`, `--all` or `CARGO_TARGET_DIR`.
- Tests use `.expect("...")`, never the unwrap call. Code comments are short full sentences that end with a period.
- A test that scripted a `message` run drops that effect; an assertion on today's fixed merge subject changes to the driver-built subject (that is not weakening it); no other assertion is weakened.

## Review Focus

1. A resumed builder session (a fix round resume, a resumed or carried task) still ends with a note, because the prompt asks again. Pinned by the note-line assertions on every builder prompt in `slim_prompts.rs`, `task_carry.rs` and `build_checks.rs`.
2. A plan placed by hand with a broken `**Commit:**` line, which lint never saw: the merge still happens with the plan-title subject and a ledger line. Pinned by `a_broken_commit_line_at_merge_falls_back_with_a_ruling`.
3. A job that started before this change and has no `commit-notes/` folder: the message lists the job branch's non-`wip:`, non-sync commit subjects. Pinned by `a_job_without_notes_uses_the_old_commit_subjects`.
4. Task note files numbered past 9 (`task-10.md`): sorted by number, not by name. Pinned by `task_sections_follow_the_task_number_order`.
5. A rework of task 2 after a park: its note file is overwritten, not duplicated. Pinned by `a_rerun_task_overwrites_its_note`.

## File Structure

- `crates/factory-engine/src/specs.rs`: `COMMIT_NOTE_LINE` and its use in every builder prompt.
- `crates/factory-core/presets/{rust,go,ruby,node,csharp}/agents/factory-builder.md`: the commit note section. `presets/legacy/*` is not edited: those files are the exact texts older versions seeded (`preset.rs` `LEGACY`), and `refresh` replaces a home's copy that matches them with the new builder text.
- `crates/factory-engine/src/notes.rs` (new): `NOTES_DIR`, `Engine::save_commit_note`, `Notes`, `read_notes`.
- `crates/factory-engine/src/jobmsg.rs` (new): `Engine::job_message`.
- `crates/factory-engine/src/build.rs`: the two call sites of `save_commit_note`.
- `crates/factory-engine/src/message.rs`: keeps `MessageKind`, `MessageRequest`, `fixed_message`, `driver_footers`; loses the agent code.
- `crates/factory-engine/src/compat.rs`: `Driver::commit_message` (at 674).
- `crates/factory-engine/src/lib.rs`: modules and exports.
- Tests: `crates/factory-engine/tests/slim_prompts.rs`, `task_carry.rs`, `build_checks.rs`, `message.rs`, `merge.rs`, `permission_pins.rs`, `sync_review.rs`, `cancel.rs`, `lease_heartbeat.rs`, `runloop.rs`; `crates/factory-cli/tests/agent_lint.rs`, `e2e.rs`, `new_to_merged.rs`.

---

### Task 1: Builders write a commit note

**Files:**
- Modify: `crates/factory-engine/src/specs.rs` (`interrupted_prompt` at 26, `builder_first_spec` at 190-212, `builder_task_spec` at 277-325, `builder_task_carry_spec` at 369, `builder_resume_spec` at 406 (both branches), `builder_fix_fresh_spec` at 451-467), `crates/factory-engine/src/lib.rs` (export)
- Modify: the five preset `factory-builder.md` files (the `## Commits` section at 50-54 and the `## Reply` section). Do not edit `presets/legacy/*`.
- Test: `crates/factory-engine/tests/slim_prompts.rs` (`GOLDEN_FIRST` 127, `GOLDEN_TASK` 129, `GOLDEN_FIX` 131, `GOLDEN_FIX_FRESH` 369), `crates/factory-engine/tests/build_checks.rs` (`a_resume_prompt_names_the_task` at 2259-2293), `crates/factory-engine/tests/task_carry.rs` (the carry tests at 538-570), `crates/factory-cli/tests/agent_lint.rs` (builder pins near 193-198)

**Interfaces:**
- Produces:
  - `pub const COMMIT_NOTE_LINE: &str` in `specs.rs`, exported from `lib.rs`, exact text: `End your reply with a commit note between ---BEGIN COMMIT--- and ---END COMMIT---, written as plain text and not inside a code fence: a subject of at most 50 characters in the imperative mood with no trailing period, a blank line, one or two sentences on why, a blank line, then one "- " bullet per change. The driver writes the commit message from it.`
  - Every builder prompt ends with `\n` and `COMMIT_NOTE_LINE`: both branches (slim and full) of the first, task and fix-resume prompts, the carry and fix-fresh prompts, and `interrupted_prompt` (used by `builder_task_resume_spec`).
  - In each preset builder file, the first bullet of `## Commits` ends `... The driver commits them for you after you finish and writes the commit message from your commit note.` (replacing `with a committer agent that writes the message`). The `## Reply` section gains a paragraph that asks for the note after the `Ruling:` lines, written as plain text and not inside a code fence, with this example (shown in the file inside a fence, as an illustration only):
    ```text
    ---BEGIN COMMIT---
    Add a retry endpoint for parked jobs

    The dashboard had no way to continue a parked job.

    - Add POST /projects/{p}/jobs/{id}/retry with 409 refusals
    - Refuse approve while a retry is queued
    ---END COMMIT---
    ```
    and the rules: subject at most 50 characters, imperative, no trailing period; one or two sentences on why; one bullet per change; in a fix round, the bullets describe this round's fixes.

- [ ] **Step 1: Read** the six prompt functions and `interrupted_prompt`, the four goldens in `slim_prompts.rs` and the tests that use them (148, 240), `a_resume_prompt_names_the_task` in `build_checks.rs`, the carry tests at 538-570 in `task_carry.rs`, the rust builder file, and the builder pins in `agent_lint.rs`.
- [ ] **Step 2: Write the failing tests**:
  - `slim_prompts.rs`: append `\n` plus `COMMIT_NOTE_LINE` to `GOLDEN_FIRST`, `GOLDEN_TASK`, `GOLDEN_FIX` and `GOLDEN_FIX_FRESH`; `the_slim_fix_resume_prompt_asks_for_a_commit_note` (the slim branch ends with `COMMIT_NOTE_LINE`); `the_slim_first_and_task_prompts_ask_for_a_commit_note` (with `builder_file(Some("<!-- factory-rules: 1 -->"))`, both prompts end with `COMMIT_NOTE_LINE`).
  - `task_carry.rs`: `the_carry_prompt_asks_for_a_commit_note` (the carry spec's prompt ends with `COMMIT_NOTE_LINE`).
  - `build_checks.rs`: the expected prompt in `a_resume_prompt_names_the_task` gains `\n` plus `COMMIT_NOTE_LINE`.
  - `agent_lint.rs`: each of the five preset builder files contains `---BEGIN COMMIT---`, `---END COMMIT---`, `not inside a code fence` and `driver commits them`, and none contains `committer agent`.
- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine --test slim_prompts --test task_carry --test build_checks` and `cargo test -j4 -p factory-cli --test agent_lint`, and see the new assertions fail.
- [ ] **Step 4: Implement** the Produces items.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-cli`, `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings` and `cargo clippy -j4 -p factory-cli --all-targets -- -D warnings`. Expected: pass.

### Task 2: Notes on disk and the job message

**Files:**
- Create: `crates/factory-engine/src/notes.rs`, `crates/factory-engine/src/jobmsg.rs`
- Modify: `crates/factory-engine/src/lib.rs` (modules and exports)
- Test: unit tests in `notes.rs`; a new `crates/factory-engine/tests/job_message.rs` using the engine fixture of `tests/common/mod.rs` (`Fixture` and its engine) with a hand-built `MessageRequest`

**Interfaces:**
- Consumes: `parse_commit_note`, `check_note`, `fallback_note`, `CommitNote`, `note_bullets`, `check_job_subject`, `title_subject`, `NoteSection`, `JobMessage`, `build_job_message`, `validate_message`, `append_footers` from factory-core; `header_commit_line`, `goal_line`, `plan_header`, `plan_title` from `tasks.rs`; `MessageRequest`, `MessageKind`, `fixed_message`, `driver_footers`.
- Produces (all `pub` and exported from `lib.rs`, so nothing is dead code before the next task calls it):
  - `pub const NOTES_DIR: &str = "commit-notes";`
  - `impl Engine<'_> { pub fn save_commit_note(&self, file: &str, label: &str, reply: &str, fallback_title: &str, files: &[String]) -> Result<(), EngineError> }`: parse, check, fall back as the Global Constraints say, create the folder and write `<job_dir>/commit-notes/<file>`. Only a ledger write error is returned as an error.
  - `pub struct Notes { pub build: Option<CommitNote>, pub tasks: Vec<(u32, CommitNote)>, pub rounds: Vec<(u32, CommitNote)> }` and `pub fn read_notes(job_dir: &Path) -> Notes`: reads the folder, `tasks` and `rounds` sorted by number, unreadable or unparsable files skipped; an absent folder gives an empty `Notes`.
  - `impl Engine<'_> { pub fn job_message(&self, req: &MessageRequest<'_>) -> Result<String, EngineError> }` (reads the plan from `self.plan_file`, which is `job_dir/plan.md` in both `with_engine` and `Fixture`):
    - Subject: the plan's `header_commit_line` when `check_job_subject` accepts it; a line it refuses writes the ruling `merge message: plan **Commit:** line refused (<why>), plan title used` and falls through; then `title_subject(<the plan's # heading>)` when `plan.md` exists and has a `# ` heading; else `title_subject(req.title)`.
    - Why: `goal_line(&plan_header(plan))`.
    - Sections: `build.md` first when present, then each task note in number order, each as `NoteSection { title: note.subject, bullets: note_bullets(&note.body) }`. Fixes: the bullets of every round note in number order.
    - With no notes at all: one section titled `Changes` whose bullets are the subjects of `repo.commits_above(req.base, req.tip)`, oldest first, without subjects starting `wip:` or `chore(factory): sync`; no section when none remain. A git error from `commits_above` (an empty base included) gives no `Changes` section, never an error.
    - Footers: `driver_footers(req)`. A message that fails `validate_message` writes the ruling `merge message: <why>, fixed message used` and returns `append_footers(&fixed_message(MessageKind::Merge, req.job_id, req.slug, self.repo.base_branch()), &footers)`.

- [ ] **Step 1: Read** `message.rs` (`MessageRequest`, `fixed_message`, `driver_footers`), the `Fixture` in `tests/common/mod.rs` and how a test gets an `Engine` from it, `Repo::commits_above`.
- [ ] **Step 2: Write the failing tests**:
  - `notes.rs`: `read_notes_sorts_by_number_and_skips_bad_files` (`task-10.md`, `task-2.md`, `round-3.md`, `round-2.md`, `build.md`, and an empty `task-4.md`: tasks `[2, 10]`, rounds `[2, 3]`, build set); `an_absent_folder_gives_no_notes`.
  - `tests/job_message.rs`:
    - `save_commit_note_writes_a_valid_note`, `save_commit_note_falls_back_with_a_ruling` (no note: the file holds the fallback; the ledger has `commit note for task 1: missing, fallback used`), `a_bad_note_gets_the_fallback_with_the_reason` (a 60-char subject: the ledger names `the subject is 60 characters`).
    - `the_merge_message_is_built_from_the_notes`: plan.md `# Retry\n\n**Goal:** Let a user retry a parked job.\n\n**Commit:** feat(retry): retry a parked job\n`; notes `task-1.md` (`Add the endpoint` / why / `- Add POST retry`), `task-2.md` (`Show the button` / why / `- Add a Retry button`), `round-2.md` (`Address review` / why / `- Refuse approve while queued`); request with `issues: &[142]`, `closes_ticket: true`, `job_id: "001"`. The message is exactly:
      ```text
      feat(retry): retry a parked job

      Let a user retry a parked job.

      Add the endpoint:
      - Add POST retry

      Show the button:
      - Add a Retry button

      Review fixes:
      - Refuse approve while queued

      Fixes: #142
      Factory-Job: 001
      ```
    - `task_sections_follow_the_task_number_order` (`task-2.md` and `task-10.md`).
    - `a_plan_without_a_commit_line_uses_the_plan_title`; `a_plan_without_a_heading_uses_the_feature` (no `# ` line: subject from `req.title`); `a_broken_commit_line_at_merge_falls_back_with_a_ruling`.
    - `a_job_without_notes_uses_the_old_commit_subjects` (branch commits `feat(x): add y`, `wip: task 1 a`, `chore(factory): sync main into job 001 demo`: one `Changes:` section with `- feat(x): add y`).
    - `a_job_without_issues_gets_only_the_job_footer` (the message ends `\n\nFactory-Job: 001`, no `Fixes` or `Relates to`).
    - `the_first_plan_of_a_series_relates` (`closes_ticket: false`: `Relates to: #142`).
- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine --lib notes` and `cargo test -j4 -p factory-engine --test job_message`, and see the new tests fail.
- [ ] **Step 4: Implement** the Produces items.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine` and `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`. Expected: pass.

### Task 3: Wire notes and messages in, remove the message agent

**Files:**
- Modify: `crates/factory-engine/src/build.rs` (`build_task`: one call right after the `new_tip` if/else, before `note_task_checks` at 706; `build_stage`: after `head` is known, before `note_rewrites` at 213), `crates/factory-engine/src/compat.rs` (`commit_message` at 672-707), `crates/factory-engine/src/message.rs` (remove `MESSAGE_TOOLS`, `message_prompt`, `message_spec`, `message_step`, `pick` and their tests), `crates/factory-engine/src/lib.rs` (exports)
- Test: `crates/factory-engine/tests/build_checks.rs`, `message.rs` (agent-run tests removed), `merge.rs` (18), `permission_pins.rs` (38 and the `message:` line of its GOLDEN), `sync_review.rs`, `cancel.rs` (47-63), `lease_heartbeat.rs` (187, deleted), `runloop.rs` (234 and a new series test); `crates/factory-cli/tests/e2e.rs` (30-36, 67-68, 74, 221-222), `new_to_merged.rs` (23)

**Interfaces:**
- Consumes: `save_commit_note`, `job_message`, `fixed_message`, `driver_footers`; `Repo::changed_paths(base, head)`.
- Produces:
  - In `build_task`: `save_commit_note(&format!("task-{}.md", task.n), &format!("task {}", task.n), &run.result_text, &task.title, &files)` with `files = changed_paths(&tip, &new_tip)` (empty on a git error).
  - In `build_stage`: for `new_work`, file `build.md`, label `build`, fallback title `Build the plan`; else file `round-{job.round}.md`, label `fix round {job.round}`, fallback title `Address review findings`; files are `changed_paths(&tip_before, &head)`.
  - `Driver::commit_message(id, kind)` keeps its signature: `Merge` builds the `MessageRequest` as today and returns `self.with_engine(id, |e| e.job_message(&req))`; `Sync` returns `append_footers(&fixed_message(MessageKind::Sync, &job.id, &job.slug, self.repo.base_branch()), &driver_footers(&req))` with no agent run. The unused `cwd` goes.
  - Test fallout, each named: remove `message_spec` and the `message:` line from the `permission_pins.rs` GOLDEN; in `sync_review.rs` drop every `reply(GOOD)` (each is a sync message, including both in `two_sync_reviews_get_their_own_files`) and the `GOOD` const; delete `a_cancel_during_the_commit_message_run_leaves_main_unchanged` in `cancel.rs` (`a_cancelled_job_does_not_merge` still covers a cancel before the merge) and remove the then-unused `GOOD`, `Effect`, `scripted`, `approved_job_with` and `AgentSpec` imports; delete `a_lost_lease_stops_before_the_merge_commit` in `lease_heartbeat.rs` (the merge has no agent run any more, so there is no window between the start of the merge and the merge commit; `another_owner_in_the_lease_sets_lost_and_stops_the_run` still covers a lost lease); drop `"message"` from the role lists in `runloop.rs:234` and `e2e.rs`; change the merge-subject asserts from `chore(factory): merge job 001 ...` to the driver-built subject: `merge.rs:18` → `feat: say hello` (plan.md has no `# `, the feature is "say hello"), `e2e.rs:74` → `feat: b` (plan `# B`; update its "Spec decision 10: ... fixed message" comment too), `new_to_merged.rs:23` → `feat: plan` (heading `# Plan`); remove the dead `spec.role == "message"` branch in `crates/factory-cli/tests/support/mod.rs` (119-125). In `tests/message.rs`: delete `merge_uses_the_committer_message`, `an_invalid_reply_retries_once_then_uses_the_fixed_message`, `a_valid_retry_is_used`, `a_failed_run_still_merges`, `sync_uses_the_committer_message`, `a_message_run_never_commits_the_index`, `main_moving_during_the_message_run_syncs_before_merging`, `a_reply_that_has_the_footer_gets_no_duplicate`, `the_fixed_merge_message_gets_the_footers_too`; keep, with the `reply(GOOD)` effect dropped and the footer-ending asserts unchanged, `merge_message_ends_with_the_fixes_and_job_footers` (ends with `\n\nFixes: #20\nFactory-Job: 001`; drop its prompt assert), `the_last_plan_says_fixes_even_with_follow_ups`, `plan_one_of_three_says_relates_to_even_without_follow_ups`, `a_single_plan_job_with_follow_ups_says_fixes`, `a_job_without_issues_gets_only_the_job_footer` (ends with `\n\nFactory-Job: 001`) and `sync_message_uses_relates_to_then_the_job_footer` (effects `[reply(APPROVE)]`); remove the then-unused `GOOD` and imports.

- [ ] **Step 1: Read** `build_task` from 622 to 745, `build_stage` from 127 to 215, `compat.rs` 641-707, and each test named in Files around the given lines.
- [ ] **Step 2: Write the failing tests**:
  - `build_checks.rs`: `a_task_note_is_saved_from_the_builder_reply` (task 1's builder replies with a valid note; `commit-notes/task-1.md` holds its subject and body); `a_task_without_a_note_gets_the_fallback_and_a_ruling` (the file holds the task title and the changed file as a bullet); `a_fix_round_note_is_saved_as_its_round` (`round-2.md`); `a_whole_plan_build_saves_build_md`; `a_rerun_task_overwrites_its_note`.
  - `merge.rs`: `a_merge_commit_carries_the_driver_built_message` (after `merge_stage`, main's merge commit message starts with the plan's `**Commit:**` subject and contains `Factory-Job:`); `no_message_agent_runs_for_a_merge_or_a_sync` (built with `approved_job_with(Arc::new(scripted(vec![reply("ok\nVERDICT: APPROVE")])), true)` and `commit_file(&fx.main, "other.md", "o")` before `merge_stage` so a sync runs; the runner records no spec with role `message`).
  - `runloop.rs`: `a_series_lands_one_merge_per_plan_with_its_commit_subject` (the series fixture with feature `#7` and two plan files written by hand with absolute `Next plan:` paths, carrying `**Commit:** feat: first plan` and `**Commit:** fix: second plan`, since `SeriesFixture::plan_file` writes no `**Commit:**` line; walking main's first parents with git2, no `git` binary, shows both subjects, the first with `Relates to: #7`, the second with `Fixes: #7`).
- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine --test build_checks --test merge --test runloop` and see the new tests fail.
- [ ] **Step 4: Implement** the Produces items and the test fallout.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`, `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings` and `cargo clippy -j4 -p factory-cli --all-targets -- -D warnings`. Expected: pass.

Next plan: docs/superpowers/plans/2026-11-19-driver-commit-messages/plan-03-driver-commit-messages.md
