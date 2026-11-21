# Builder Recovers Instead of Parking Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A builder that fails because of the machine waits and reruns by itself. Any other failure gets one rerun. A builder that changes nothing on a branch with commits goes on to review instead of parking.

**Architecture:** `factory-agent` gets a pure failure classifier and carries the Claude CLI's error text into `AgentRun.error`. `factory-core` gets the new park kind, two optional job fields and one setting. A new engine module `recover.rs` owns the preflight, the wait loop and the rerun, and both builder paths in `build.rs` call it. The nothing-changed rule lives in `build.rs`; its review side lives in `review.rs` and `specs.rs`. The dashboard and the supervisor API learn the new kind and field.

**Tech Stack:** Rust (cargo `-j4`), `fs2` for free space, plain JS dashboard tested with `node --test`.

**Spec:** `docs/superpowers/specs/2026-11-21-builder-recovers-design.md`

## Global Constraints

- No new external crate.
- Never shell out to git; use `factory_git`. Free disk space goes through `fs2::available_space`.
- New board fields are optional: `#[serde(default, skip_serializing_if = "Option::is_none")]`.
- Paths reach the driver only through `Layout` or fields the engine already holds (`board_path`, `job_dir`, the worktree path).
- Code comments are short full sentences that end with a period.
- cargo: always `-j4`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`. Dashboard: `node --test dashboard/tests/*.test.js`.
- Exact values from the spec:
  - setting `min_free_disk_mb`, default `2048`;
  - probe schedule 1, 2, 4, 8 and 15 minutes, then every 15 minutes, with a total cap of 25 minutes;
  - a probe is free at `turns <= 1`, `output_tokens < 2000` and `weighted < 20000`;
  - wait sleep step at most 5 s;
  - error text capped at 500 chars in `AgentRun.error` and 200 chars in park reasons and ledger lines;
  - park kind `machine-waiting`;
  - job fields `machine_wait: Option<String>` and `no_change_round: Option<u32>`;
  - events `builder.wait`, `builder.probe`, `builder.wait_end`, `builder.rerun`.
- Exact ledger texts:
  - `builder failed (<cause or error>), rerunning once`
  - `machine ok after N min`
  - `builder changed nothing; it says the work is already on the branch`
  - `task N changed nothing; it says the work is already on the branch`
- Commits go through the `dev-workflow-skills:1-git-commit` skill.
- Tests never touch the real disk check or sleep. The engine `Fixture` (`tests/common/mod.rs` ~124) and the CLI test configs set `min_free_disk_mb = 0` or inject a fake `MachineWait`, so a test machine with less than 2 GB free never waits.
- The rework note is removed only when the build pass advances: right before `finish_build`, or when a task advances. A run that parks or reruns for any reason keeps it.
- No plan text may refer to tasks by number. Use test and file names.

## Review Focus

- A Claude bad-key or network failure as the real CLI emits it (`is_error: true`, subtype `success`, text in `result`) must classify as Machine, not Other. Covered by `claude_api_error_text_reaches_error` and the classifier tests.
- A retry after a Failed first whole-plan build must start a builder, not error with "missing findings file". Covered by `retry_after_failed_first_build_runs_first_spec`.
- A rework note must survive every run that is not accepted: Failed, blocked, needs-context or parked. Covered by `rework_note_survives_runs_that_do_not_advance` (both paths) and `disk_blocked_report_waits_then_reruns_with_note`.
- A wait must end within one sleep step on `cancel --force` or lease loss, and must not end because the ledger can't be written. Covered by `wait_ends_on_cancel`, `wait_ends_on_lost_lease` and `wait_survives_unwritable_ledger`.
- A no-change round must still pass every gate: the diff cap and the risky park fire, and `fix_rounds` ends a disagreement with `ChangesRemain`. Covered by the nothing-changed tests.

---

### Failure text and classifier in factory-agent

**Files:**
- Create: `crates/factory-agent/src/failure.rs`
- Modify: `crates/factory-agent/src/child.rs` (the `result_is_error` arm, ~226-247), `crates/factory-agent/src/lib.rs`, `crates/factory-agent/src/bin/fake-claude.rs`
- Test: `crates/factory-agent/tests/runner.rs`, unit tests in `failure.rs`

**Interfaces:**
- Produces:
  - `pub enum MachineCause { Disk, Network, Auth }`, which derives Debug, Clone, Copy, PartialEq and Eq, with `pub fn as_str(self) -> &'static str` returning "disk", "network" or "auth".
  - `pub enum FailureClass { Machine(MachineCause), Other }`.
  - `pub fn classify_failure(run: &AgentRun) -> Option<FailureClass>`: `None` unless `run.stop_reason == StopReason::Failed`. It reads `run.error` only.
  - `pub fn did_no_work(run: &AgentRun) -> bool`: `run.turns <= 1 && run.usage.output_tokens < 2000 && run.weighted < 20000`.
  - All of these are re-exported from `lib.rs`.

- [ ] **Write the failing tests.**
  - In `failure.rs`:
    - `auth_patterns`: errors containing "Invalid API key", "authentication_error", "API Error: 401" or "api_key_file" give `Machine(Auth)`.
    - `network_patterns`: "Can't reach the API server", "Connection error", "ECONNRESET", "ENOTFOUND", "EAI_AGAIN", "API Error: 529" or "overloaded_error" give `Machine(Network)`. A bare "401" or "529" elsewhere in the text, such as a stderr tail, is not a match.
    - `disk_patterns`: "No space left on device" or "ENOSPC" give `Machine(Disk)`.
    - `other_and_negative`: a Failed run with error "claude reported success: it broke" gives `Other`. A Done run whose `result_text` says "test timed out after 401 retries" gives `None`. A Failed run with `error: None` gives `Other`.
    - `did_no_work_limits`: a run at 1 turn, 1999 output tokens and 19999 weighted is true; 2 turns is false; 2000 output tokens is false.
  - In `tests/runner.rs`, `missing_session_text_reaches_error`: fake-claude mode `missing-session` gives an `error` containing "No conversation found".
  - In `tests/runner.rs`, `claude_api_error_text_reaches_error`: fake-claude mode `api-error` with second prompt line `API Error: 401 Invalid API key`. Assert `stop_reason == Failed` and that `run.error` contains "Invalid API key".
- [ ] **Run them and check they fail.** `cargo test -j4 -p factory-agent failure` and `cargo test -j4 -p factory-agent --test runner claude_api_error_text_reaches_error`. Expect compile errors or FAIL.
- [ ] **Implement.**
  - Add the fake-claude mode `api-error`: `init("fake-1")`, then `result(<second prompt line>, true, "success")`, exit 1.
  - In child.rs, the `result_is_error` arm stops dropping the result text for label `claude`. The message becomes `format!("{label} reported {subtype}: {text}")`, with the result text capped at 500 chars. When the result text is empty, the trimmed stderr tail is appended instead, also capped at 500 chars, so a missing session's "No conversation found" reaches `error`.
  - Write `failure.rs` with case-insensitive substring matches. Check Disk first, then Auth, then Network.
- [ ] **Run them and check they pass.** Run the same two commands, then `cargo test -j4 -p factory-agent`. All must pass.
- [ ] **Commit** with the commit skill.

### Park kind, job fields and setting in factory-core

**Files:**
- Modify: `crates/factory-core/src/park.rs` (`ParkKind`, `as_str`, `resume`, `next`, `next_with_resume`), `crates/factory-core/src/board.rs` (`Job` fields, `approve_refusal` ~359), `crates/factory-core/src/config.rs` (field, default, docs), `crates/factory-core/src/settings_block.rs` ("Limits and rounds." list), `docs/reference/project-toml-reference.md`
- Test: unit tests in `park.rs`, `board.rs` and `config.rs`. Also `crates/factory-core/tests/` for the settings-block snapshot test, if one pins the list.

**Interfaces:**
- Produces:
  - `ParkKind::MachineWaiting`, with `as_str() == "machine-waiting"` and `resume() == Some(ResumeAt::Build)`. `next(id)` returns `[factory retry {id}, factory rework {id} <note>, factory cancel {id}]`.
  - `ParkKind::BuilderStopped.resume()` stays `None`. The engine passes `Some(ResumeAt::Build)` through `park_with_resume` for Failed stops only.
  - `next_with_resume` keeps the rework entry when the kind is `MachineWaiting` or `BuilderStopped`.
  - `Job.machine_wait: Option<String>` and `Job.no_change_round: Option<u32>`, both optional in serde.
  - `Config.min_free_disk_mb: u64`, default 2048. A value of 0 turns the disk check off.

- [ ] **Write the failing tests.**
  - `machine_waiting_round_trips_and_resumes_at_build`.
  - `machine_waiting_next_lists_retry_rework_cancel`.
  - `builder_stopped_with_build_resume_keeps_rework`: `next_with_resume(BuilderStopped, "001", false, None, Some(Build))` returns `["factory retry 001", "factory rework 001 <note>"]`.
  - `approve_refusal_series_message_only_for_series_waiting`. It extends the existing `approve_refusal_names_retry_on_a_build_resume_park` by setting `park_kind = Some("series-waiting")`. A job with `park_kind = Some("machine-waiting")` and a Build resume must not get the "earlier plan" message.
  - `new_job_fields_are_optional`: a board without the fields loads, and serializing omits them.
  - `min_free_disk_mb_defaults_to_2048` and `min_free_disk_mb_parses`.
- [ ] **Run them and check they fail.** `cargo test -j4 -p factory-core`. Expect FAIL.
- [ ] **Implement** the items in Interfaces.
  - Add `("min_free_disk_mb", None)` to the "Limits and rounds." list.
  - Add a row to `project-toml-reference.md`: "Free space, in MB, that the job worktree's filesystem and the factory home's must each have before a builder starts. While either is short, the job waits up to 25 minutes, then parks `machine-waiting`. `0` turns the check off."
  - Fix every exhaustive `match` on `ParkKind` that the compiler flags, in all crates. Run `cargo check -j4`.
- [ ] **Run them and check they pass.** Run `cargo test -j4 -p factory-core`, then `cargo check -j4`.
- [ ] **Commit.**

### Rework allowed on machine parks in factory-cli

**Files:**
- Modify: `crates/factory-cli/src/commands.rs` (`rework`, ~836-846)
- Test: `crates/factory-cli/tests/retry_cmd.rs`

**Interfaces:**
- Consumes: `Job.park_kind` strings `"machine-waiting"` and `"builder-stopped"`.

- [ ] **Write the failing test** `plain_rework_is_allowed_on_machine_and_stopped_parks`. A job at needs-human with `park_kind = "machine-waiting"` and `park_resume = Build` accepts `factory rework 001 "fix x"` without `--force`. The same holds for `builder-stopped`. A `review-failed` park is still refused with "parked outside the builder".
- [ ] **Run it and check it fails.** `cargo test -j4 -p factory-cli --test retry_cmd plain_rework`.
- [ ] **Implement.** The refusal condition also requires that `job.park_kind` is not one of those two strings. If the API has its own rework refusal (grep `parked outside the builder` in `crates/factory-supervisor` and `dashboard/src`), give it the same exception and a test there.
- [ ] **Run it and check it passes.** Then run `cargo test -j4 -p factory-cli --test retry_cmd`.
- [ ] **Commit.**

### Failed runs keep session state and the rework note

**Files:**
- Modify: `crates/factory-engine/src/build.rs` (`build_stage` ~156-189; `build_task` ~656-660)
- Create: `crates/factory-engine/tests/builder_recover.rs`
- Modify: `crates/factory-engine/tests/common/mod.rs` (new helper)

**Interfaces:**
- Produces, in `tests/common/mod.rs`, for every later test:
  - `pub fn failed_run(error: &str, turns: u32, output: u64, weighted: u64) -> AgentRun`: stop Failed, the given error, `usage.output_tokens = output`, `usage.input_tokens = weighted - output` (saturating), and `weighted` as given. The board tally comes from `usage` (`run_agent`, ports.rs ~292), so it then rises by `weighted`.
  - `pub struct BuilderScript`, which implements `AgentRunner`. `BuilderScript::new(inner: Arc<ScriptedRunner>, steps: Vec<Step>)`, with `pub struct Step { pub when: fn(&AgentSpec) -> bool, pub run: Option<AgentRun> }`.
    - For a builder call, the first unused step whose `when` matches is used up. `run: Some(r)` returns `r`, and `None` hands the call to `inner`.
    - Other roles, and builder calls with no matching step, go to `inner`.
    - `BuilderScript::with_clock(self, Arc<dyn Fn() -> f64 + Send + Sync>)` records the clock at each builder call. `call_times() -> Vec<f64>` returns them.
    - `specs() -> Vec<AgentSpec>` returns every builder spec it was called with, in order, whether a step or `inner` answered.
    - When `inner` answers, the session id is `inner`'s ("sess-1", common/mod.rs ~189), so the `resumed` predicate works in fix rounds.
    - Ready-made predicates: `any`, `resumed` (`spec.resume.is_some()`) and `fresh` (`spec.resume.is_none()`).
  - `RoleFailing`'s default error becomes `"claude reported success: API Error: 401 Invalid API key"`, the shape child.rs now produces. Update `tests/multi_review.rs` ~431, which asserts `"run failed (failed): Invalid API key"`, to the new text.

- [ ] **Write the failing tests** in `builder_recover.rs`.
  - `failed_run_keeps_session_fields`: a whole-plan job in a fix round with `builder_id = "sess-old"` and `builder_session_model = "m"`. Every builder call fails with `failed_run("boom", 3, 10, 10)`. After the build pass, `builder_id`, `builder_last` and `builder_session_model` equal their values before it.
  - `rework_note_survives_runs_that_do_not_advance`: for both paths, write `rework-note.md`, then let the builder end (a) Failed on every call, (b) Done with a `BLOCKED:` status, (c) Done with `NEEDS_CONTEXT:`, and (d) Done with no change and no commits above base. The note still exists after each. After a Done run that advances to review it is gone.
- [ ] **Run them and check they fail.** `cargo test -j4 -p factory-engine --test builder_recover`.
- [ ] **Implement.**
  - In `build_stage`, write the session fields (the `self.update` at ~171) only when `run.stop_reason != StopReason::Failed`.
  - In both paths, remove the note only where the pass advances (see Global Constraints).
- [ ] **Run them and check they pass.** Then run `cargo test -j4 -p factory-engine`; existing tests must still pass.
- [ ] **Commit.**

### Preflight and wait helper in a new recover module

**Files:**
- Create: `crates/factory-engine/src/recover.rs`
- Modify:
  - `crates/factory-engine/src/lib.rs`: the `Engine` field and builder, `mod recover`, and re-export `MachineWait`;
  - `crates/factory-engine/src/compat.rs`: `Driver::with_machine_wait`, passed to every `Engine` it builds (~617, ~650);
  - `crates/factory-engine/src/ports.rs`: add `stage: Stage` and `cancelled: bool` to `JobView`, plus `no_change_round: Option<u32>` for the nothing-changed section. Also fix the `JobView` literal in `probe.rs` ~57;
  - `crates/factory-engine/src/build.rs`: the call at the top of the Build pass;
  - `crates/factory-engine/tests/common/mod.rs`: `Fixture` and `driver_for` set `min_free_disk_mb = 0` unless a test sets it.
  - Every test that writes its own config or `project.toml` settings gets `min_free_disk_mb = 0`: the CLI helpers in `crates/factory-cli/tests/common` and `tests/support`; the CLI tests `one_job.rs`, `scope.rs`, `external_layout.rs`, `intake_cmd.rs`, `new_to_merged.rs`, `project_flag.rs` and `project_cmd.rs`; and the supervisor tests `tests/supervisor.rs`, `api_read.rs` and `api_write.rs`. Then `grep -rln "budget_weighted\|\[settings\]\|config.toml" crates/*/tests` to find any others.
- Test: unit tests in `recover.rs` and `crates/factory-engine/tests/builder_recover.rs`

**Interfaces:**
- Produces:
  - `pub struct MachineWait`, which implements Clone. Its fields:
    - `pub free_mb: Arc<dyn Fn(&Path) -> Option<u64> + Send + Sync>`;
    - `pub sleep: Arc<dyn Fn(Duration) + Send + Sync>`;
    - `pub now: Arc<dyn Fn() -> f64 + Send + Sync>`.
  - `impl Default for MachineWait` uses `fs2::available_space` / 1 MiB, `std::thread::sleep` and `clock::now_secs`.
  - `Engine.machine: MachineWait`, with `Engine::with_machine_wait(self, MachineWait) -> Self` and `Driver::with_machine_wait(self, MachineWait) -> Driver`.
  - `pub(crate) fn preflight(&self, dir: &Path) -> Option<(MachineCause, String)>`.
    - When `config.min_free_disk_mb > 0`, it checks `dir` and the directory of `board_path`. Either one short gives `(Disk, "disk: 0.4 GB free on <path>")`.
    - When `config.agent_cli() == "claude"`, `config.api_key_path()` is set and `factory_agent::check_key_file` fails, it gives `(Auth, <error text>)`.
    - Otherwise `None`.
  - `pub(crate) enum Probe { Fine, StillDown, Final(AgentRun), Parked(StageOutcome) }`.
  - `pub(crate) enum WaitEnd { Ready, Final(AgentRun), Parked(StageOutcome) }`.
  - `pub(crate) fn wait_for_machine(&self, cause: MachineCause, detail: &str, probe: &mut dyn FnMut() -> Result<Probe, EngineError>) -> Result<WaitEnd, EngineError>`.
    - It sleeps in steps of at most 5 s through `self.machine.sleep`.
    - It probes at 60, 120, 240, 480 and 900 s after the start, then a last probe at 1500 s (the cap), measured with `self.machine.now`. If that one fails too, it parks.
    - `Fine` gives `Ready`. `Final` and `Parked` pass straight through.
    - After each step it reads the board with `self.job()` and no write. A stage other than Build, or `cancelled`, gives `Err(EngineError::Cancelled)`. A set lost flag gives `Err(self.lost.as_ref().unwrap().error())`. `Engine::lost()` only returns a bool.
    - At the cap it parks `MachineWaiting` with the reason `"<detail>, waited 25 min"`, where the detail is `disk full: 0.4 GB free on <path>`, `network: <error>` or `auth: <error>`. `preflight`'s Disk detail uses the same `disk full: ...` form; `machine_wait` holds the short form (`disk: 0.4 GB free`, `network`, `auth`).
    - It sets `machine_wait` (for example `disk: 0.4 GB free` or `network`) when it starts and clears it on every exit.
    - Ledger: one line at the start, then `machine ok after N min` when it ends Ready or Final.
    - Events: `builder.wait`, `builder.probe` per probe, and `builder.wait_end`.
    - Every write inside the loop (ledger, board field, events) is best effort: on error it logs `tracing::warn!` and goes on.
  - **Where it runs:**
    - At the top of the Build pass, after the series guard and before `is_mid_task`, `start_worktree`, any ledger line or any board write.
    - The pass first clears `machine_wait`, best effort.
    - It calls `preflight(job.worktree, or self.repo.worktrees_dir() when the job has none yet)`. It also runs, with the `machine_wait` clear, before `resume_old_batch`. On `Some`, it waits with a probe that reruns `preflight`, returning `Fine` or `StillDown`. On `Ready` the pass goes on; `Parked` is returned.

- [ ] **Write the failing tests.** Each uses a fake `MachineWait` with a counter clock that `sleep` advances, and sets `min_free_disk_mb = 2048`.
  - `disk_wait_starts_builder_when_disk_frees`: `free_mb` returns 100 until the clock passes 200 s, then 10_000. The builder runs once and the job advances with no park. The ledger has `machine ok after 4 min` (the 240 s probe).
  - `disk_wait_parks_after_25_minutes`: `free_mb` is always 100.
    - The job parks `machine-waiting` with a reason containing "waited 25 min".
    - `machine_wait` is None afterwards, and the builder never ran.
    - The probe times, from `builder.probe` events or a counter in `free_mb`, are 60, 120, 240, 480, 900 and 1500 s.
  - `machine_wait_is_set_during_and_cleared_after`: the fake `sleep` reads the board and sees `machine_wait = Some("disk: ...")`. After Ready it is None.
  - `machine_wait_is_cleared_at_next_build_pass`: the board holds a stale `machine_wait` from a killed run. The next build pass clears it.
  - `wait_ends_on_cancel`: the fake `sleep` sets `cancelled = true` and stage NeedsHuman on its 2nd call. The build returns the cancel result, with no more probes.
  - `wait_ends_on_lost_lease`: the same, setting the engine's lost flag.
  - `wait_survives_unwritable_ledger`: make `progress.md` read-only before `build_stage`, so the wait's start line fails. The fake `sleep` restores it at the step where the disk frees. The wait still ends Ready and the builder runs.
  - `missing_key_file_waits_then_runs`: `api_key_file` points to a missing file, and the fake `sleep` creates it (mode 600) on its 1st call. The builder then runs.
- [ ] **Run them and check they fail.** `cargo test -j4 -p factory-engine --test builder_recover`.
- [ ] **Implement** `recover.rs`, the seams and the top-of-pass call.
- [ ] **Run them and check they pass.** Then run `cargo test -j4 -p factory-engine` and `cargo test -j4 -p factory-cli`.
- [ ] **Commit.**

### Rerun after a failed or disk-blocked builder

**Files:**
- Modify: `crates/factory-engine/src/recover.rs`, `crates/factory-engine/src/build.rs` (`build_stage` ~131-189; `build_task` ~596-667, which replaces the fallback at ~627-653)
- Test: `crates/factory-engine/tests/builder_recover.rs`
- Update: `tests/build_checks.rs` ~521. It pins `rec.next == ["factory rework 001 <note>"]` for a stopped builder. A Failed stop now has a Build resume, so `next` is `["factory retry 001", "factory rework 001 <note>"]`. A BudgetStop or TurnLimit stop keeps the old list.
- Check that these still pass: `tests/task_carry.rs`, `tests/fix_fresh.rs`, `tests/escalation.rs`, `tests/retry.rs`.

**Interfaces:**
- Consumes:
  - `classify_failure` and `did_no_work` from `factory_agent`;
  - `preflight`, `wait_for_machine`, `Probe` and `WaitEnd` from the preflight and wait helper;
  - `budget_state(self.job_weighted_tokens(), self.config.budget_weighted)`;
  - `BuilderScript` and `failed_run` in the tests.
- Produces: `pub(crate) fn run_builder_recovering(&self, first: AgentSpec, rerun: &dyn Fn(&AgentRun) -> AgentSpec, before_rerun: &mut dyn FnMut() -> Result<(), EngineError>, dir: &Path, task: Option<u32>) -> Result<Result<AgentRun, StageOutcome>, EngineError>`.
  - It has the same contract as `run_builder_guarded`, which it calls for every run.
  - `rerun(&failed)` builds the spec for a rerun or a probe. It is pure: no ledger or board writes.
  - `before_rerun` runs once, before the first probe run or rerun. It is where the per-task path clears `task_session` and writes its `fresh` ledger task line.
  - The flow:
    1. Run `first`. There is no budget check before it.
    2. **Failed and `Machine(c)`:** wait.
       - For Disk, the probe is `preflight`, giving `Fine` or `StillDown`. After `Ready`, run the rerun once.
       - For Network and Auth, the probe is a run of `rerun(&failed)`, preceded by a budget check that gives `Parked(BudgetStopped)` on `Stop`. A probe run that fails with the same cause and `did_no_work` gives `StillDown`. Any other probe run gives `Final(run)`.
    3. **Failed and `Other`:** check the budget (on `Stop`, park `BudgetStopped` with "budget is stopped"), then run the rerun once.
    4. **Done, but `parse_builder_status` gives Blocked or NeedsContext while the `preflight` disk check fails:** treat it as `Machine(Disk)`, as in step 2.
    5. **Every builder run after the first** (rerun, disk rerun after `Ready`, disk-blocked rerun, probe run) is preceded by a budget check (on `Stop`, park `BudgetStopped` with "budget is stopped") and by `preflight(dir)`. A failing preflight enters the Disk wait first.
    6. **Ledger and event:** the line `builder failed (<cause, or error at 200 chars>), rerunning once` and the event `builder.rerun` are written once per builder step, when the first builder run after the failure starts (right after `before_rerun`). That covers an Other rerun, a disk rerun and the first probe run of a network or auth wait. Later probe runs write no ledger line.
    7. Return the final run. The caller's existing checks park a second failure as `BuilderStopped`.
       - A Failed stop parks with `park_with_resume(.., Some(ResumeAt::Build))` and the reason `builder stopped: Failed: <error, 200 chars>`; per task, `builder stopped in task N: Failed: <error>`.
       - Other stop reasons keep `park_with` and their current reason.
- `rerun` per path:
  - **Whole-plan, first-spec arm:** `builder_first_spec` with the same findings argument.
  - **Whole-plan, resume arm:** `builder_resume_spec` with the same session and findings. When the failed run's error contains "No conversation found", the session is gone, and `builder_fix_fresh_spec` with the same findings is used instead.
  - **Whole-plan, fresh-fix arm** (escalated or cold): the same `builder_fix_fresh_spec`.
  - **Per task:** `builder_task_spec` with the note and a new session hook. `before_rerun` clears `task_session` and writes the existing `fresh` or `fresh (carry failed)` ledger task line, once. The old fallback block is removed; this is the only rerun.

- [ ] **Write the failing tests.**
  - `other_failure_reruns_once_then_parks_with_error`: two `failed_run("claude reported success: it broke", 3, 50, 500)` steps. The job parks `builder-stopped` with `park_resume = Build`, the reason contains "it broke", the builder was called twice, and the ledger has one `rerunning once` line.
  - `other_failure_rerun_succeeds`: one failed step, then `inner` makes a change. The job advances to review.
  - `retry_after_failed_first_build_runs_first_spec`: through `driver_for`. Round 1, two Other failures, so the job parks. Then `factory_engine::retry_job(&driver, "001")`, and the next build pass calls the builder with the first-build prompt (assert on a phrase unique to `builder_first_spec`) and gives no "missing findings file" error.
  - `failed_resume_reruns_same_session`: a fix round with a stored session. The first call (resumed) fails Other, and the rerun spec has `resume == Some(<same session>)`. With the error "No conversation found with session ID", the rerun spec has no resume and uses the fresh-fix prompt.
  - `failed_escalated_round_reruns_fresh_fix`: an escalated round whose first call fails Other. The rerun is again the fresh-fix spec on the escalated model.
  - `network_failure_probes_until_back`: three `failed_run("claude reported success: API Error: Can't reach the API server (EAI_AGAIN)", 0, 0, 0)` steps, then `inner` succeeds. The job reaches review. `call_times()` is `[0, 60, 120, 240]`, and the ledger has one `machine ok after` line and one `rerunning once` line.
  - `auth_failure_with_readable_key_file_waits`: a readable key file, and the builder fails with "API Error: 401 Invalid API key" twice, then succeeds. Probes are builder runs, and the job reaches review.
  - `probe_that_did_work_counts_as_the_rerun`: the first call fails Network, then a probe fails Network with `turns = 5`. The job parks `builder-stopped`, with 2 builder calls.
  - `budget_stop_blocks_rerun`: the first call fails Other with `weighted` equal to `budget_weighted`. The job parks `budget-stopped`, with 1 builder call.
  - `budget_stop_blocks_probe`: the first call fails Network with `weighted` equal to `budget_weighted`. No probe run starts, and the job parks `budget-stopped`.
  - `disk_blocked_report_waits_then_reruns_with_note`: a rework note exists. The builder replies `BLOCKED: disk is full` while `free_mb` is short, then the disk frees. The rerun gets the note: per task, its prompt holds the note text; in the whole-plan path, its prompt names `rework-note.md`, and the file still exists. The job then advances. With the disk fine, the same reply parks `builder-blocked` as today.
  - `per_task_machine_failure_waits_before_fresh_fallback`: a resumed task whose `resumed` step fails Auth. No builder call happens before clock 60. The calls after that use the fresh task spec, and the `fresh` ledger task line appears once.
- [ ] **Run them and check they fail.** `cargo test -j4 -p factory-engine --test builder_recover`.
- [ ] **Implement** `run_builder_recovering` and switch both paths to it.
- [ ] **Run them and check they pass.** Run `cargo test -j4 -p factory-engine`, updating `build_checks.rs` ~521 as listed. Then run `cargo test -j4 -p factory-cli` and `cargo test -j4 -p factory-supervisor`. The CLI `ScriptRunner` (tests/support/mod.rs ~158-165) pops a step even when the role doesn't match, so a new rerun can eat later steps. Fix the script of any test that now fails that way. Update an assertion elsewhere only where it pinned the old immediate fallback, and name it in the commit body.
- [ ] **Commit.**

### A builder that changes nothing goes on to review

**Files:**
- Modify:
  - `crates/factory-engine/src/build.rs`: `build_stage` ~177-233, `nothing_changed_after_checks` ~317-331, the `build_task` `Snapshot::Nothing` arm ~690, and `save_builder_reply` used in both paths;
  - `crates/factory-engine/src/review.rs` ~69-70: the review floor. The review never clears `no_change_round`;
  - `crates/factory-engine/src/specs.rs` `review_spec` ~631-661;
  - `crates/factory-engine/src/ports.rs` `JobView` (`no_change_round`).
- Test: create `crates/factory-engine/tests/nothing_changed.rs`
- Existing tests that pin the old park on a branch with commits. Each keeps its purpose:
  - `tests/build_checks.rs` ~142-151 (`a_no_change_fix_round_after_a_review_still_parks_as_nothing_changed`) and ~157-175 (`a_resume_that_pushes_back_on_everything_parks_with_the_reply_as_a_part`): the round now goes to review. Assert that, plus the builder reply in `builder-r{round}.md` and the reviewer prompt.
  - `tests/build_checks.rs` ~456-463 and ~1071 expect "task 2 changed nothing" with task 1 committed. Rewrite them to assert the new behaviour: the task advances, or the job goes to review when it is the last task.
  - `tests/park_evidence.rs` ~59-90 and `tests/review_decide.rs` ~1127-1145 test the `nothing-changed` park's evidence (lead parts, the `was_rewritten` sync hint at park.rs ~214). Re-stage them on a job with no commits above base, where the park still happens, so that evidence code stays covered. `nothing_changed_parts` and the hint are still used there and are not dead.

**Interfaces:**
- Consumes: `self.repo.commits_above(base, tip)` from factory-git.
- Produces: `Job.no_change_round`. The build sets it to the round, and the review stage reads it. It is cleared at the start of the next build round, when the Build pass begins with `no_change_round` set to an older round, so a review retried after a park still sees it.

- [ ] **Write the failing tests** in `nothing_changed.rs`.
  - `whole_plan_rework_that_changes_nothing_goes_to_review`: the branch has a commit, then a rework note, then the builder changes nothing and replies "finding 1 is already handled in src/a.rs".
    - The job reaches review, and the ledger has `builder changed nothing; it says the work is already on the branch`.
    - The reviewer's diff file is the whole-branch diff, not empty.
    - The reviewer prompt names `builder-r{round}.md`, and that file holds the reply.
    - On a re-review the prompt does not contain "Inspect the fix diff only".
  - `review_fix_round_that_changes_nothing_reaches_cap`: the reviewer asks for changes each round and the builder changes nothing each time. The job ends `changes-remain` after `fix_rounds` rounds. With a risky diff, the cap is `risky_fix_rounds`.
  - `checks_round_that_changes_nothing_uses_whole_diff`: on the `nothing_changed_after_checks` route with passing checks, the reviewer's diff is the whole branch.
  - `last_task_that_changes_nothing_goes_to_review` and `middle_task_that_changes_nothing_advances`. The latter checks that `task_index` goes up, `builder_last` is set, there is a task-done ledger line, and the next build call runs the next task.
  - `first_task_with_no_commits_still_parks`: there are no commits above base, so the job parks `nothing-changed`.
  - `no_change_round_still_hits_diff_cap`: the whole-branch diff is above `diff_cap_lines`, so the job parks `diff-too-large`.
  - `no_change_round_still_hits_risky_park`: a branch whose commits touch a risky path. The no-change round reaches decide, and the job parks `risky-diff` after the risky cap.
  - `no_change_round_survives_review_retry`: the review parks `review-failed`, then `factory retry`. The retried review still uses the whole-branch diff and the reply line.
  - `no_change_round_is_cleared_at_next_build`: the next build round clears it, and a round that changed something reviews the round diff.
- [ ] **Run them and check they fail.** `cargo test -j4 -p factory-engine --test nothing_changed`.
- [ ] **Implement.**
  - When the snapshot is `Nothing` and `commits_above(base, tip_before)` is not empty, write the ledger line and save the reply to `builder-r{round}.md`; per task, append it under `## Task N`. Then:
    - `finish_build` gets a new last parameter, `no_change: bool`. When it is true, the same board update that records the head sets `no_change_round = Some(job.round)`. Every existing caller passes false;
    - in the whole-plan path, call `finish_build(&job, &base, tip_before, None, true)`;
    - per task, a middle task does the success bookkeeping of ~719-738 with the unchanged tip, and the last task calls `finish_build(.., None, true)`.
  - `nothing_changed_after_checks` calls `finish_build(.., None, true)`.
  - The existing `checks`-sourced branch keeps its order: it runs before the new rule.
  - In `review_spec`, for a plain review run (`ReviewRun::Round`, never Sync or Rebase), when `no_change_round == Some(round)`:
    - always add the builder-reply line when the file exists;
    - on a re-review, replace "Inspect the fix diff only." with: "The builder changed nothing this round and says why in its reply. The diff is the whole branch. Judge each earlier finding against it and the reply.";
    - keep the out-of-scope sentence.
- [ ] **Run them and check they pass.** Then run `cargo test -j4 -p factory-engine`. Update the old tests that expected `nothing-changed` on a branch with commits, listing each in the commit body.
- [ ] **Commit.** The commit message closes #42 (`Fixes: #42`).

### Dashboard and supervisor API

**Files:**
- Modify:
  - `dashboard/static/projects.js`: `jobActions` ~105-106;
  - `dashboard/static/board.js`: `retryApprove` ~42, the park-kind label map ~421 (add `machine-waiting`: "machine waiting"), and the stage-reason block ~744, where `machine_wait` shows while the job is at build. `parkLines` (~726-735) renders only for needs-human;
  - `dashboard/src/model.rs`: the `machine_wait` field next to `park_resume` ~178;
  - `crates/factory-supervisor/src/api/read.rs` ~139-143: add `"parkKind"` and `"machineWait"`.
- Test: `dashboard/tests/projects.test.js`, `dashboard/tests/board.test.js`, unit tests in `model.rs` and `api/read.rs`

- [ ] **Write the failing tests.**
  - `jobActions` returns `["retry", "rework", "cancel"]` for `{stage: "needs-human", park_resume: "build", park_kind: "machine-waiting"}`, and the same for `builder-stopped`. It stays `["retry", "cancel"]` for `series-waiting`. Update `projects.test.js` ~104, which passes `park_resume: "build"` with no kind: give it `park_kind: "series-waiting"`.
  - `retryApprove` stays empty for every Build resume.
  - The card shows `machine_wait` text, for example "waiting: network", while the job is at build.
  - The model round-trips `machine_wait` as optional, and a `--home` mode card built through `model.rs` (home_mode.rs ~344-356) carries it.
  - read.rs includes `parkKind` and `machineWait`.
- [ ] **Run them and check they fail.** `node --test dashboard/tests/*.test.js`, `cargo test -j4 --manifest-path dashboard/Cargo.toml`, `cargo test -j4 -p factory-supervisor`.
- [ ] **Implement.** `jobActions` checks `park_kind === "series-waiting"` for the retry-and-cancel-only case. Other Build resumes get retry, rework and cancel.
- [ ] **Run them and check they pass.** Run the same three commands.
- [ ] **Commit.**

### Docs

**Files:**
- Modify: `docs/reference/software-factory.md` (parks, retry, the nothing-changed rule, waiting), `docs/guides/factory-user-guide.md` (the park table and `factory retry` section 6.5a), `README.md` only if it lists park kinds

- [ ] Document the following in each file that covers it:
  - the preflight;
  - the wait, with the cap, the `machine_wait` card text and the events;
  - the single rerun;
  - `machine-waiting` and `builder-stopped` parks, which resume with `factory retry` and still accept `factory rework`;
  - the nothing-changed rule, with the whole-branch review and the builder's reply;
  - the version note: upgrade `factory` and `factoryd` together before setting `min_free_disk_mb`, because older binaries reject unknown keys.
- [ ] Check: `grep -rn "builder made no changes on resume" docs` shows only historical mentions.
- [ ] The commit body carries the upgrade note too, because release notes come from commit messages (`cliff.toml`).
- [ ] **Commit.**
