# Final Check Skip and Series Guard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop `final_gate` from rerunning checks that just passed on the same head, and stop a series plan from building before the plan before it has merged.

**Architecture:** A passing check run records its head and command list on the job. `final_gate` reuses that record when the head, a clean worktree and the planned commands all match. A new park kind with a new resume point lets a series plan wait for its predecessor; `run_series` continues it once the predecessor merges, and `factory retry` does the same by hand. A job with no commits of its own gets its worktree cut again from the current base branch before its builder starts.

**Tech Stack:** Rust, `factory-core` (board, park), `factory-git` (worktrees), `factory-engine` (build, checks, review, retry, run loop), `dashboard/static` (JS labels and actions).

**Spec:** `docs/superpowers/specs/2026-11-19-final-gate-skip-and-series-guard-design.md`

## Global Constraints

- Cargo always with `-j4`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- New board fields are optional (`#[serde(default, skip_serializing_if = "Option::is_none")]`), so `dashboard/` still reads the board.
- Never merge on checks that did not run on the merged head. When any condition for reuse is unclear, or its check errors, run the checks.
- Never rewrite or remove a branch that has job commits or uncommitted work.
- No change to any `Cargo.toml`, `Cargo.lock` or other path in `risk_paths`.
- Git goes through `factory_git::Repo` only; never shell out to `git`.
- Check commands run without a shell (`crates/factory-checks/src/run.rs:143`). A test that needs redirection writes a script and runs `sh <script>`.
- Code comments are short full sentences that end with a period.

## Review Focus

- The checks config changes between the checks stage and the final gate: the planned command list differs, so the final checks run again (`final_check_failure_blocks_auto_merge` keeps passing).
- A check or builder leaves an untracked, non-ignored file in the worktree after the checks passed: the final checks run again.
- A rework of a stale empty job must still deliver the operator's note to the new builder, on the whole-plan and the per-task path.
- Two legacy series (series `id` absent) must never be treated as one series.
- A cancelled predecessor must not let a later plan build without its code: the later plan stays parked and its reason says to cancel it (a series card is never removed by `factory cleanup`, `software-factory.md:317`).

---

### Reuse the passing checks in the final gate

**Files:**
- Modify: `crates/factory-core/src/board.rs` (`ChecksPass`, `Job::checks_pass`, `Job::new` literal, tests)
- Modify: `crates/factory-core/src/lib.rs:43` (re-export `ChecksPass`)
- Modify: `crates/factory-engine/src/ports.rs:89-147` (`JobView::checks_pass`, filled in `job()`)
- Modify: `crates/factory-engine/src/checks_stage.rs` (shared plan helper, record the pass, `reusable_checks`)
- Modify: `crates/factory-engine/src/build.rs:381` (`job_worktree` becomes `pub(crate)`)
- Modify: `crates/factory-engine/src/review.rs` (`final_gate` reuses)
- Modify: `docs/reference/software-factory.md:244-246`
- Test: `crates/factory-engine/tests/review_decide.rs`, `crates/factory-engine/tests/retry.rs:468`, `board.rs` tests

**Interfaces:**
- Produces: `factory_core::ChecksPass { pub head: String, pub commands: Vec<String> }` (Debug, Clone, PartialEq, Eq, Serialize, Deserialize); `Job::checks_pass: Option<ChecksPass>`; `JobView::checks_pass: Option<ChecksPass>`.
- Produces: `Engine::reusable_checks(&self) -> Result<Option<Vec<String>>, EngineError>` in `checks_stage.rs`, `pub(crate)`.

- [ ] **Step 1: Write the failing tests**

In `review_decide.rs`, a counter that works without a shell. Call it before `build_and_pass_checks`, because `fx.engine()` clones `fx.config`:

```rust
fn counting_config(fx: &mut Fixture) -> std::path::PathBuf {
    let count = fx.job_dir.join("runs.txt");
    let script = fx.job_dir.join("count.sh");
    std::fs::write(&script, format!("echo run >> {}\n", count.display())).expect("script");
    fx.config.checks = vec![factory_core::CheckRule {
        paths: vec!["**".into()],
        commands: vec![format!("sh {}", script.display())],
        ..factory_core::CheckRule::default()
    }];
    count
}

fn runs(count: &std::path::Path) -> usize {
    std::fs::read_to_string(count).unwrap_or_default().lines().count()
}
```

Tests, each starting with `counting_config`, then `build_and_pass_checks`; the gate tests then review `APPROVE` and call `apply_decision`:
- `checks_stage_records_the_passing_head_and_commands`: after `build_and_pass_checks`, `fx.job().checks_pass == Some(ChecksPass { head: <job head>, commands: vec!["sh <script>"] })`.
- `a_failing_check_run_clears_the_record`: with `let mut fx`, after the passing run set `fx.config.checks` to the command `false` and call `fx.engine(&r).checks_stage()` again (it does not check the stage); `checks_pass == None`.
- `final_gate_reuses_checks_on_an_unchanged_head`: `runs == 1`, stage `Approve`, the ledger has a `checks ran: ` line containing `(reused from the checks stage)`.
- `final_gate_reruns_checks_when_the_worktree_is_dirty`: write an untracked file in `fx.job().worktree` before apply; `runs == 2`.
- `final_gate_reruns_checks_when_the_head_moved`: commit a file in the worktree (`common::commit_file` on `fx.job().worktree`), set `head` to it with `BoardStore::update_job` as `common/mod.rs:143-145` does; `runs == 2`.
- `a_risky_approve_reuses_the_checks_and_parks`: build `src-tauri/src/lib.rs` (a risky path in the default config); `runs == 1`, park kind `risky-diff`.

In `board.rs` tests: a job JSON without `checks_pass` loads as `None` and serializes without the key; one with it round-trips.

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -j4 -p factory-engine --test review_decide` and `cargo test -j4 -p factory-core checks_pass`
Expected: compile errors for `ChecksPass`.

- [ ] **Step 3: Add `ChecksPass` and the fields**

The board field, the `Job::new` literal (`checks_pass: None`), the `lib.rs` re-export, and `JobView` plus its fill in `ports.rs::job()`.

- [ ] **Step 4: Share the plan inputs and record the pass**

In `checks_stage.rs`, extract `fn planned_checks(&self) -> Result<(String, PathBuf, CheckPlan), EngineError>` returning the head of `job_range()`, the workdir and `self.plan_for(...)` over the scoped changed paths, exactly what `run_checks_now` computes today. `run_checks_now` uses it, and after the report, in one `self.update`, sets `checks_pass = Some(ChecksPass { head, commands })` when `report.all_passed()`, else `None`. `commands` is `report.results` commands in order. The other callers (`build.rs:309`, `park_after_sync`) record too; that is intended.

- [ ] **Step 5: Implement `reusable_checks` and use it in `final_gate`**

`reusable_checks` returns `Some(commands)` only when all hold, else `None`:
- `checks_pass.head == job.head`;
- the worktree is clean: build the `Worktree` with `job_worktree(&job, cwd, base, branch)` (`build.rs:381`, now `pub(crate)`; `cwd` from `job.worktree`, `base` from `job.base`, `branch` from `job.branch`, either of the first two missing gives `None`), then `Repo::worktree_is_clean`. A missing worktree or a git error means `None`, not `Err`;
- the commands of `planned_checks()` equal `checks_pass.commands`.

In `final_gate` (`review.rs:395`): on `Some`, skip `final_checks` and the failure branch, still run `gate_checks`, and write `checks ran: <list> (reused from the checks stage)`. On `None`, today's code. `park_after_sync` is not changed and always reruns.

- [ ] **Step 6: Keep the rereview retry test's intent**

`tests/retry.rs:468 a_rereview_retry_with_failing_final_checks_starts_a_fix_round` would now reuse the passing sync run and end in `Approve`. Change only its script: on the first run it also creates `stray.txt` in its cwd (the worktree), so the worktree is dirty, the final gate reruns, and the rerun fails. Its assertions (stage `Build`, round 2) stay.

- [ ] **Step 7: Update the docs**

`software-factory.md:244` and `:246`: the final checks are reused when the head, a clean worktree and the planned commands match the passing run, and the ledger line then reads `checks ran: <list> (reused from the checks stage)`. After a sync park they always run again.

- [ ] **Step 8: Run the tests, clippy and fmt**

Run: `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-engine`, `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`, `cargo fmt -p factory-engine -p factory-core -- --check`
Expected: all pass, including `final_check_failure_blocks_auto_merge`, `gates.rs` and `park_evidence.rs`.

- [ ] **Step 9: Commit**

Use the `dev-workflow-skills:1-git-commit` skill. Suggested subject: `perf(engine): reuse passing checks in final gate`.

### Series-waiting park that retry can resume

**Files:**
- Modify: `crates/factory-core/src/park.rs` (`ParkKind::SeriesWaiting`, `ResumeAt::Build`, tests)
- Modify: `crates/factory-core/src/board.rs:324-333` (`rework_hint`), `:343-366` (`approve_refusal`), tests near `:1063-1100`
- Modify: `crates/factory-engine/src/retry.rs` (`ResumeAt::Build` arm)
- Modify: `crates/factory-cli/src/cli.rs:122-124` (retry help)
- Modify: `dashboard/static/projects.js:107` (`jobActions`), `dashboard/static/board.js` (`PARK_LABEL`), `dashboard/tests/projects.test.js`, `dashboard/tests/board.test.js`
- Modify: `docs/reference/software-factory.md:47` and `:271-278`, `docs/reference/factory-admin-manual.md:96`, `docs/guides/factory-user-guide.md:480`, `README.md:399-403` and `:530`
- Test: `crates/factory-engine/tests/retry.rs`

**Interfaces:**
- Produces: `ParkKind::SeriesWaiting` (`"series-waiting"`), `resume() == Some(ResumeAt::Build)`, `next(id) == ["factory retry <id>", "factory cancel <id>"]`.
- Produces: `ResumeAt::Build` (`"build"`). Serde follows the enums' existing `kebab-case` and `lowercase` naming.

- [ ] **Step 1: Write the failing tests**

`park.rs`: add `SeriesWaiting` to `kind_names_are_stable_kebab_case`, `(ResumeAt::Build, "build")` to `resume_at_round_trips`, a `ParkRecord` round-trip like `the_new_kinds_round_trip_through_park_json`, and `next_with_resume(SeriesWaiting, "086", false, None, Some(Build)) == ["factory retry 086", "factory cancel 086"]`.

`board.rs`: a job in needs-human with `park_resume == Some(Build)` gets a refusal from `approve_refusal(true, false)` (with `--without-review`) and `approve_refusal(false, false)`, naming `factory retry <id>`. Test `Build` in its own test, not in the Review/Rereview loop of `approve_refusal_names_retry_on_a_review_resume_park` (`board.rs:1061`), because the new check gives a different message.

`tests/retry.rs`, `a_build_retry_moves_the_job_to_build`: an `approved_job` forced to needs-human and parked with `park(&fx, ParkKind::SeriesWaiting, Some(ResumeAt::Build))`; after `retry_job`, the ledger has `retry at build`, `stage_history` has needs-human then build, `park_kind` and `park_resume` are `None`, and the end is a `RunEnd` (no `Err`). The full resume flow is tested in the series guard task.

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -j4 -p factory-core` and `cargo test -j4 -p factory-engine --test retry a_build_retry`
Expected: compile errors for the missing variants.

- [ ] **Step 3: Add the variants and the board arms**

`ParkKind::next` and `resume()` arms. `rework_hint` puts `Build` with `Review | Rereview` (`factory retry <id>`). `approve_refusal` refuses any approve while `stage == NeedsHuman && park_resume == Some(Build)`, as its first check, before `changes_open`: `job <id> waits for an earlier plan of its series: use factory retry <id> after it merges, or cancel`.

- [ ] **Step 4: Handle `ResumeAt::Build` in `retry_inner`**

Ruling `retry at build` first, then `crate::resume_logged(j, Stage::Build, now)`, without `reset_reviews` or an approve gate. It falls through to the shared tail (`operator.retry` event, `run_series`); it must not return early like the `Rereview` arm.

- [ ] **Step 5: Dashboard**

`jobActions` returns `["retry", "cancel"]` when `park_resume === "build"`. `PARK_LABEL` gets `"series-waiting": "Waiting for the earlier plan"`. Add a case to each JS test file. These tests use `node:test`; run them from the repo root with `node --test dashboard/tests/*.test.js` (`dashboard/README.md:71`).

- [ ] **Step 6: Docs and help**

- `software-factory.md:47` Kinds list: add `series-waiting`, and the missing `no-checks`, `no-tests`, `builder-tampered`.
- `software-factory.md:271-278`: the retry text no longer says "without a builder run" for every park; the resume table gets a row `series-waiting` / `build` / "continues at build; the guard checks the plan before it again; the series also continues it on its own once that plan merges".
- The admin manual, user guide and README word lists add `build`; the README `parkResume` list too.
- `cli.rs` retry help adds "or a series plan whose earlier plan has not merged". The only test on that help (`cli.rs:445-454`) checks substrings that stay, so no test change.

- [ ] **Step 7: Run the tests**

Run: `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-engine --test retry`, `cargo test -j4 -p factory-cli`, `cargo test -j4 -p factory-supervisor`, and `node --test dashboard/tests/*.test.js`
Expected: all pass.

- [ ] **Step 8: Commit**

Use the `dev-workflow-skills:1-git-commit` skill. Suggested subject: `feat(core): add series-waiting park, retry at build`.

### Guard the series order at build start

**Files:**
- Create: `crates/factory-engine/src/series_guard.rs` (`impl Engine`: the guard and the re-cut)
- Modify: `crates/factory-engine/src/lib.rs` (module line)
- Modify: `crates/factory-engine/src/build.rs` (call the guard in `build_stage`; carry the rework note into a fresh whole-plan build)
- Modify: `crates/factory-engine/src/specs.rs` (`builder_first_spec` takes optional findings)
- Modify: `crates/factory-engine/src/probe.rs:103` (pass `None`)
- Modify: `crates/factory-engine/src/runloop.rs` (`run_series` continues a series-waiting next plan)
- Modify: `docs/reference/software-factory.md` (series section)
- Test: `crates/factory-engine/tests/series_guard.rs` (new)

**Interfaces:**
- Consumes: `ParkKind::SeriesWaiting`, `ResumeAt::Build`.
- Produces: `pub(crate) fn series_guard(&self, job: &JobView) -> Result<Option<StageOutcome>, EngineError>`. `Some` means it parked. `None` means build on, after reloading the job with `self.job()?`. `JobView` has no `series`, `stage` or `cancelled`, so the guard reads them with `Board::load(&self.board_path)`.

- [ ] **Step 1: Write the failing tests**

In `tests/series_guard.rs`. Series jobs come from `create_job` over `plan-01-a.md` and `plan-02-b.md` (as `tests/retry.rs:516`). `SeriesFixture`'s `RoleRunner` records no specs, so "no builder ran" is asserted as `worktree.is_none()` and no `dispatched` line in job 2's `progress.md`. Rework and cancel are CLI commands; simulate them as described per test. Helpers: drop a card with `driver.store().update(|b| { b.jobs.retain(|j| j.id != id); Ok(()) })`; set fields with `store().update_job`; make a worktree by hand with `driver.repo().create_worktree(&driver.repo().job_ref(&id, &slug))` and record `wt.path`, `wt.branch`, `wt.base` on the board.

- `plan_two_parks_while_plan_one_is_not_merged`: `run_series` on job 2 while job 1 is in build. `NeedsHuman` naming `plan 1` and job 1's id; `park_kind == "series-waiting"`; `park_resume == Some(Build)`; no builder ran.
- `plan_two_parks_when_plan_one_was_cancelled`: set job 1 `cancelled = true` and stage needs-human with `store().update_job`; the reason names job 1 and says to cancel this plan.
- `merging_plan_one_continues_a_waiting_plan_two`: job 2 parks, then `run_series` on job 1 ends `Merged` for both; job 2's base has job 1's merge as an ancestor (`Repo::is_ancestor`); ledger of job 2 has `retry at build, plan 1 merged`.
- `a_manual_retry_after_the_merge_builds_plan_two`: job 2 parks, job 1 merges by `run_job`, `retry_job` on job 2 ends `Merged`.
- `legacy_series_do_not_match_each_other`: two jobs with `series.id == None`, indexes 1 and 2, from different plan files; job 2 builds, no park.
- `a_per_task_rework_after_main_moved_recuts_and_keeps_the_note`: `common::Fixture` with `fx.plan_file` overwritten to `### Task 1: t`, driven by `fx.engine(&runner).build_stage()` with `scripted(vec![reply("Status: BLOCKED\nReason: x"), <second effect>])`. The first builder replies `Status: BLOCKED\nReason: x` (`ScriptedRunner`), so it parks with a worktree and no commits. Commit a file on main; write `rework-note.md`, `fx.resume_to_build()`, `build_stage()` again. Job base == new main tip, ledger has `re-cut`, the second effect sees the main file in `spec.cwd`, and `runner.specs_for("builder")[1].prompt` contains the note.
- `a_whole_plan_rework_after_main_moved_recuts_and_keeps_the_note`: same with a `- [ ] Task 1` plan, `changes-r2.md`, round 2 and `fix_source = "rework"` set with `update_job`. The second prompt names the `changes-r2.md` path.
- `a_branch_with_commits_is_never_recut`: create job 2's worktree through the driver's repo (`create_worktree(job_ref)`), record `worktree`, `branch`, `base` on the board, commit a file in it while job 1 is unmerged; merge job 1 with `run_job` (not `run_series`, which would go on into job 2); build job 2. Base and tip unchanged, no `re-cut`.
- `a_dirty_empty_worktree_is_kept_with_a_ruling`: same setup, one untracked file instead of a commit. The ledger has `kept the stale worktree` and no `re-cut`, and the file is still there.
- `an_interrupted_recut_is_finished`: a job whose board names a worktree and branch that no longer exist (remove them with `remove_worktree` and leave the board as is); `build_stage` does not error, the ledger has `finished an interrupted re-cut`, and the job builds on a fresh worktree.

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -j4 -p factory-engine --test series_guard`
Expected: the park tests fail (job 2 builds), the re-cut tests fail (base is stale).

- [ ] **Step 3: Implement `series_guard`**

In order:
0. Recovery first: when `worktree` is set but its folder does not exist and `!self.repo.branch_exists(&branch)` (a crash between a removal and the board write), clear the fields listed in sub-step 3, write the ruling `finished an interrupted re-cut`, and go on. This must come before any `repo.tip(branch)`, which errors on a missing branch.
1. Return `None` when the job has commits of its own: a worktree, a `base`, and `repo.tip(branch) != base`. A job with no worktree yet (the 087 first start) goes on to the series check. This exempts every fix round and later task; jobs with commits from before the guard existed are left to the existing sync.
2. Series check, only when the board job's `series` has `index > 1` and `id.is_some()`: find the board job with the same `id` and `index - 1`. None found: go on. `stage == Merged`: go on. `cancelled`: park with reason `plan <n> (job <id>) was cancelled; cancel this plan too, or plan the work again`. Otherwise park with `plan <n> (job <id>) is not merged yet`. Park through `self.park_with(ParkKind::SeriesWaiting, reason, vec![])`, which sets the resume from `kind.resume()`.
3. Re-cut, skipped when there is no worktree or no `base`, for any job with a worktree whose `base` is not `self.repo.tip(self.repo.base_branch())`: call `remove_worktree(&self.job_worktree(..), false)` first. On `GitError::Refused`, write only `kept the stale worktree: <why>` and go on. Other errors propagate. On success, write the ruling `re-cut the branch from <base branch> <short_sha(tip)>, the job had no commits`, then in one `self.update` clear `worktree`, `base`, `head`, `prev_head`, `builder_id`, `task_session`, `builder_last` and `builder_session_model` (the set `sync_provider` clears, `ports.rs:320-326`, plus the tree fields). Keep `round`, `fix_source`, `rework_base`, `reviews`, `escalated_model` and the branch name.

Skip the re-cut for a legacy whole-plan job (`builder_id.is_some() && task_index == 0` with a `### Task` plan), so clearing `builder_id` does not switch it to per-task mode.

- [ ] **Step 4: Call the guard and keep the rework note**

In `build_stage` after the `batch_snapshot` check and before `is_mid_task`: `if let Some(out) = self.series_guard(&job)? { return Ok(out); }`, then `let job = self.job()?;`.

Whole-plan path: change `builder_first_spec(&self, job, cwd)` to `builder_first_spec(&self, job, cwd, findings: Option<&Path>)`. With `Some(path)`, both prompt variants (slim `specs.rs:198-208` and full `specs.rs:210-224`) append the line `Fix also the problems listed in <path>.`; with `None` the prompt is byte for byte unchanged (`tests/slim_prompts.rs:133` `GOLDEN_FIRST` pins it). Callers: `build.rs:135` passes `self.fix_findings(&job).ok()` filtered to an existing file when the fresh build is fixing (`round > 1` or `rework-note.md` exists), else `None`; `probe.rs:103` passes `None`. After the builder run, remove `rework-note.md` whenever it was the findings passed in (today it is removed only when `!new_work`, `build.rs:142-145`), so a later review fix round does not get the stale note. The per-task path already reads `rework-note.md` when fresh (`build.rs:536,575-578`) and needs no change.

- [ ] **Step 5: Continue a waiting next plan in `run_series`**

Between the existing next-job lookup (`runloop.rs:787-794`) and the loop's next `run_job` (which returns `NeedsHuman` at once for a parked job, `runloop.rs:728`): when the next job has `stage == NeedsHuman`, `park_kind.as_deref() == Some(ParkKind::SeriesWaiting.as_str())` and is not cancelled, write the ruling `retry at build, plan <n> merged` on it and move it with `crate::resume_logged(j, Stage::Build, now)`, without `reset_reviews`. `run_series` already holds the project (`runloop.rs:771`). Only the immediate next plan is continued; the guard re-checks its predecessor, so an early resume parks again harmlessly.

- [ ] **Step 6: Document the guard**

One paragraph in the series part of `software-factory.md`, near line 45: a later plan parks as `series-waiting` until the plan before it merges; the series then continues it, or `factory retry` does; a cancelled predecessor keeps the later plan parked until it is cancelled; a job with no commits of its own gets its branch cut again from the base branch, and a dirty one is kept with a ruling.

- [ ] **Step 7: Run the tests**

Run: `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`, `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`, `cargo fmt -p factory-engine -- --check`
Expected: all pass.

- [ ] **Step 8: Commit**

Use the `dev-workflow-skills:1-git-commit` skill. Suggested subject: `fix(engine): wait for the earlier plan of a series`.
