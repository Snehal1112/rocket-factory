# Retry at the parked stage: resume field and parks (plan 1 of 3)

> **Spec:** docs/superpowers/specs/2026-11-19-retry-at-parked-stage-design.md
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Every park outside the builder records the stage to resume at, and a reviewer that fails for an environment reason parks as `review-failed` instead of starting a fix round.

**Architecture:** A `ResumeAt` enum in `factory-core/src/park.rs` is stored as an optional field on `ParkRecord` (park.json) and on the board `Job`. The two engine park paths (`Engine::park_with` and `merge::park_with_parts`) gain `_resume` variants; the old names pass the kind's default. `decide()` learns a `role_failed` input, and `review_stage_as` returns the resume value in `ReviewOutcome`.

**Tech Stack:** Rust, factory-core, factory-engine. No new dependency.

## Global Constraints

- New JSON fields are optional: `#[serde(default, skip_serializing_if = "Option::is_none")]`. Board and park.json written before this plan must still load.
- `ResumeAt` serializes as `"review" | "rereview" | "merge" | "sync"`. `ParkKind::ReviewFailed` serializes as `"review-failed"`.
- A reviewer role counts as failed only when its run ends `StopReason::Failed` or `StopReason::Stalled`. `TurnLimit` and `BudgetStop` keep today's behaviour.
- Agent role names in specs are `"reviewer"` and `"security-review"` (see `tests/multi_review.rs:124-125`).
- `SyncOutcome` keeps its shape (`Parked(String, Option<ParkKind>)`); it is matched in `factory-cli/src/commands.rs:667` and `tests/sync_review.rs`.
- An existing test that now parks `review-failed` because its fake script ran out gets the missing reviewer reply added; its assertions are not weakened.
- Cargo: `-j4`, one crate at a time (`-p <crate>`), never `--workspace`, `--all` or `CARGO_TARGET_DIR`.
- Tests use `.expect("...")`, never the unwrap call. Code comments are short full sentences that end with a period.
- Git only through `factory_git::Repo`. Paths only through `Layout`.

## Review Focus

1. A park.json or board.json from before this plan (no `resume` field): loads with `None`. Pinned by `an_old_park_record_loads_without_resume` and `an_old_job_loads_without_park_resume`.
2. A second park while already parked (sync paths): keeps the first park's resume, as it keeps its kind. Pinned by `an_earlier_park_keeps_its_resume`.
3. One reviewer fails and the other returns Changes: the real findings win, a fix round starts. Pinned by `a_failed_role_with_real_findings_starts_a_fix_round`.
4. A failed reviewer on a job that is also over budget or over the diff cap or has failing checks: those parks keep their order and win. Pinned by `a_failed_review_on_failing_checks_still_starts_a_fix_round`.
5. A sync review that fails during an automatic merge-stage sync: parks `review-failed` with `Rereview`, not as a plain `Other`. Pinned by `a_failed_auto_sync_review_parks_with_resume_rereview`.

## File Structure

- `crates/factory-core/src/park.rs`: `ResumeAt`, `ParkKind::ReviewFailed`, `ParkKind::resume`, `ParkRecord.resume`, `next_with_resume`.
- `crates/factory-core/src/board.rs`: `Job.park_resume`, clearing in `apply_move`, `Job::stage_before_park`.
- `crates/factory-engine/src/park.rs`: `Engine::park_with_resume`, `decision_parts` for `ReviewFailed`.
- `crates/factory-engine/src/merge.rs`: `park_with_parts_resume`, `Step::Park` with a resume, the merge-stage sites.
- `crates/factory-engine/src/decide.rs`: `DecideInput.role_failed` and the review-failed arm.
- `crates/factory-engine/src/review.rs`: role failure detection and text, `ReviewOutcome.resume`, `apply_decision` passes it.
- `crates/factory-engine/src/sync.rs`: the review-failed branch of `sync_with`.
- Tests: unit tests in the two core files and `decide.rs`; `crates/factory-engine/tests/common/mod.rs` (`RoleFailing`, `approved_job_dyn`), `gates.rs`, `park_evidence.rs`, `merge.rs`, `review_decide.rs`, `multi_review.rs`, `sync_review.rs`.

---

### Task 1: The resume field in core

**Files:**
- Modify: `crates/factory-core/src/park.rs`, `crates/factory-core/src/board.rs` (`Job` park fields at 113-119, `Job::new` literal at 246, `apply_move` at 433-450), `crates/factory-core/src/lib.rs` (exports)
- Test: unit test modules of `park.rs` and `board.rs`

**Interfaces:**
- Produces:
  - `pub enum ResumeAt { Review, Rereview, Merge, Sync }` with `Serialize`, `Deserialize`, `Clone`, `Copy`, `PartialEq`, `Eq`, `Debug`, `#[serde(rename_all = "lowercase")]`, and `pub fn as_str(self) -> &'static str`.
  - `ParkKind::ReviewFailed` (`as_str` `"review-failed"`, `next()` = `["factory retry <id>", "factory cancel <id>"]`), and `pub fn resume(self) -> Option<ResumeAt>`: `ReviewFailed` and `NoChecks` give `Some(ResumeAt::Review)`, every other kind `None`.
  - `ParkRecord.resume: Option<ResumeAt>`.
  - `pub fn next_with_resume(kind: ParkKind, id: &str, review_missing: bool, verdict: Option<&str>, resume: Option<ResumeAt>) -> Vec<String>`: with `resume` set, `factory retry <id>` first, then `kind.next_for(...)` without any `factory rework` entry and without any `factory retry` entry; with `None`, exactly `kind.next_for(...)`.
  - `Job.park_resume: Option<ResumeAt>`, cleared in `apply_move` next to `park_kind`.
  - `pub fn stage_before_park(&self) -> Option<Stage>`: the stage of the `stage_history` entry before the last `needs-human` entry; `None` when the job is not in needs-human or has no earlier entry. Its doc comment says it reports the stage, not who approved (an auto-merge decision also reaches `approve`).

- [ ] **Step 1: Read** `park.rs` (`ParkKind`, `next`, `next_for`, `ParkRecord`, `ParkRecord::read`) and `board.rs` (the park fields, `Job::new`, `apply_move`, `resume`).
- [ ] **Step 2: Write the failing tests**:
  - `resume_at_round_trips`: each variant serializes to its lowercase word and back.
  - `review_failed_kind_resumes_at_review`: `ParkKind::ReviewFailed.as_str() == "review-failed"`, `.resume() == Some(ResumeAt::Review)`, `ParkKind::NoChecks.resume() == Some(ResumeAt::Review)`, `ParkKind::NoTests.resume() == None`, `ParkKind::ChecksFailed.resume() == None`.
  - `next_with_resume_puts_retry_first_and_drops_rework`: `next_with_resume(NoChecks, "086", false, None, Some(Review)) == ["factory retry 086", "factory approve 086"]`; `next_with_resume(ReviewFailed, "001", false, None, Some(Review)) == ["factory retry 001", "factory cancel 001"]`; with `None` the result equals `ParkKind::NoChecks.next_for("086", false, None)`.
  - `an_old_park_record_loads_without_resume`: a park.json string without `resume` parses with `None`; a record with `None` serializes without the key.
  - `an_old_job_loads_without_park_resume`; `leaving_needs_human_clears_park_resume` (needs-human job with `Some(Merge)`, `resume(Stage::Approve, now)`, then `None`).
  - `stage_before_park_reads_history`: history `build, review, approve, needs-human` gives `Some(Stage::Approve)`; a job not in needs-human gives `None`.
- [ ] **Step 3: Run** `cargo test -j4 -p factory-core` and see the new tests fail.
- [ ] **Step 4: Implement** the Produces items.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-core`, `cargo clippy -j4 -p factory-core --all-targets -- -D warnings`, `cargo check -j4 -p factory-engine`, `cargo check -j4 -p factory-cli` and `cargo check -j4 -p factory-supervisor`. Expected: pass.
- [ ] **Step 6: Commit** `feat(core): add the resume stage of a park`.

### Task 2: Park sites record where to resume

**Files:**
- Modify: `crates/factory-engine/src/park.rs` (`park_with` at 176), `crates/factory-engine/src/merge.rs` (`Step` at 18-28, `park`, `park_with_parts` at 35-70, the merge-failed park at 208-212, `take_merge_lock` callers at 234-237 and 300-340, tip differs at 288-297, the sync call at 328-331, loop exhaustion at 373-377), `crates/factory-engine/src/lib.rs`
- Test: `crates/factory-engine/tests/merge.rs`, `crates/factory-engine/tests/gates.rs`, `crates/factory-engine/tests/park_evidence.rs`

**Interfaces:**
- Consumes: `ResumeAt`, `ParkKind::resume`, `next_with_resume`, `Job.park_resume`.
- Produces:
  - `pub(crate) fn park_with_resume(&self, kind: ParkKind, reason: String, parts: Vec<ParkPart>, resume: Option<ResumeAt>) -> Result<StageOutcome, EngineError>`; `park_with` calls it with `kind.resume()`.
  - `pub fn park_with_parts_resume(driver: &Driver, id: &str, reason: &str, kind: ParkKind, parts: Vec<ParkPart>, resume: Option<ResumeAt>) -> Result<(), EngineError>`; `park_with_parts` calls it with `kind.resume()`.
  - `Step::Park(String, ParkKind, Vec<ParkPart>, Option<ResumeAt>)` (private to merge.rs); `Step::plain` passes `None`.
  - Both park paths write `record.resume`, set `job.park_resume` in the same board write as `park_kind`, and build `record.next` with `next_with_resume`. An earlier park that is kept (`already` at `merge.rs:49`) keeps its own `park_resume`.

- [ ] **Step 1: Read** both park paths, `Step`, the sites listed in Files, and the fixtures in `tests/merge.rs` (`approved_job`, `worktree_of`, `branch_tip`, `hold_merge_lock` near 330 and 364) and `tests/gates.rs` (no-checks near 40-65, no-tests near 245-255).
- [ ] **Step 2: Write the failing tests** (each asserts both park.json `resume` and the board's `park_resume`):
  - `tests/merge.rs`: `main_busy_parks_with_resume_merge` (also `next[0] == "factory retry 001"`); `merge_lock_busy_parks_with_resume_merge` (with `hold_merge_lock`); `tip_differs_parks_with_resume_rereview`; `max_syncs_parks_with_resume_sync` (job with `syncs = MAX_SYNCS`, main moved); `a_merge_conflict_parks_without_resume`.
  - `tests/gates.rs`: `no_checks_parks_with_resume_review`; `no_checks_at_the_final_gate_parks_with_resume_review`; `no_tests_parks_without_resume`.
  - `tests/park_evidence.rs`: `a_builder_park_has_no_resume` (a `ChecksFailed` park); `an_earlier_park_keeps_its_resume` (park with `Merge`, then `park_with_parts_resume` with `Sync` while still parked: stays `Merge`).
- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine --test merge --test gates --test park_evidence` and see them fail.
- [ ] **Step 4: Implement** the Produces items and set the value at each site:
  - Main busy, both `take_merge_lock` failures (lock held, and "cannot take the merge lock"), and `merge failed: {err}` (`merge.rs:208-212`): `Merge`.
  - Tip differs: `Rereview`.
  - Max syncs: inside `if check.state == BaseState::MainMoved` at `merge.rs:328`, before calling `sync_stage` (so tip-differs and main-busy still come first), when `job.syncs >= MAX_SYNCS`, return `Step::Park(format!("{main} moved again after {} syncs", job.syncs), ParkKind::Other, vec![], Some(ResumeAt::Sync))` before calling `sync_stage` (same text as `sync.rs:120-124`); the loop-exhaustion park at 373-377 also gets `Some(ResumeAt::Sync)`.
  - Merge conflict, sync conflict, sync refused, base not on main, scope violation: `None`.
  - Known `None` sites: `runloop.rs:634` and `658` (missing plan, run loop did not settle), `checks_stage.rs:305` (scope), `sync.rs:100` and `104` (requested-sync parks).
  - For each remaining `park_with`, `park_gate`, `Step::plain` and `park_with_parts` call site, keep the default (`kind.resume()` or `None`) unless it is clearly outside the builder and safe to re-run. List each site and its value in the commit body.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine` and `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`. Expected: pass.
- [ ] **Step 6: Commit** `feat(engine): record the resume stage at each park`.

### Task 3: A failed review parks

**Files:**
- Modify: `crates/factory-engine/src/decide.rs` (`DecideInput`, the arm at 64), `crates/factory-engine/src/review.rs` (security agent missing at 157-168, role loop at 173-227, `DecideInput` built at 266-277, `ReviewOutcome`, `apply_decision` at 314-325), `crates/factory-engine/src/park.rs` (`decision_parts` near 273), `crates/factory-engine/src/sync.rs` (sync review applied at 194-232)
- Test: a new `#[cfg(test)] mod tests` in `decide.rs`; `crates/factory-engine/tests/common/mod.rs`, `review_decide.rs` (its `DecideInput` literal at line 8 gains `role_failed: false`), `multi_review.rs` (existing `a_missing_security_agent_parks_with_a_clear_reason` at 267-282), `sync_review.rs`, `merge.rs`; the whole `factory-cli` test suite

**Interfaces:**
- Consumes: `ParkKind::ReviewFailed`, `ResumeAt`, `park_with_resume`.
- Produces:
  - `DecideInput.role_failed: bool`. In `decide`, a new arm `Verdict::Missing if i.role_failed => Decision::NeedsHuman { kind: ParkKind::ReviewFailed, reason: "review failed: a reviewer run did not finish".into() }` placed directly before the `Changes | Missing` arm, so the budget, diff-cap and checks gates keep their order and any `Changes` verdict (which `combine_verdicts` returns when any role said Changes, `verdict.rs:23-30`) still starts a fix round.
  - In the role loop, `role_failed` is true when any role's run ended `Failed` or `Stalled`; that role's text is `run failed (<stop word>): <error, or "no error text" when error is None>` followed by its `result_text`, so the review file carries the error.
  - `ReviewOutcome.resume: Option<ResumeAt>`: `Some(Review)` for a review-failed decision of `ReviewRun::Round`, `Some(Rereview)` for `ReviewRun::Sync` and `ReviewRun::Rebase`, else `None`. `apply_decision` parks a `NeedsHuman` outcome with `park_with_resume(kind, reason, parts, outcome.resume)`.
  - The security-agent-missing early return uses `ParkKind::ReviewFailed` and the same `resume` by run kind.
  - `decision_parts` (`park.rs:233`) gets a `ParkKind::ReviewFailed` arm: first a `Why it stopped` part whose text is `reason` (so `park_summary` reads the reason); then, when `last_review.round == job.round` and its file exists, the full text of that file (`last_review.path_in(&self.job_dir)`), which holds each failed role's `run failed (...)` line; then the Review part. The security-agent-missing return writes no review file, so its parts are only `Why it stopped`.
  - In `sync_with`'s `Decision::NeedsHuman { kind, why }` branch (`sync.rs:218`), before the `!= RiskyDiff` early return at 220: when `kind == ParkKind::ReviewFailed`, let `parked = driver.apply(id, &outcome)` (which parks with `Rereview`) and `text = format!("after sync: {why}")`, and return `SyncOutcome::Parked(crate::park::sync_park_text(parked, &text), None)`, mirroring the `RiskyDiff` branch. Both callers keep that earlier park (`already` at `merge.rs:49`).
  - In `tests/common/mod.rs`: `pub struct RoleFailing { pub spec_role: &'static str, pub reason: StopReason, pub fails_left: AtomicU32, pub inner: Arc<ScriptedRunner> }` with `pub fn new(spec_role: &'static str, reason: StopReason, fails: u32, inner: Arc<ScriptedRunner>) -> RoleFailing`, implementing `AgentRunner`: while `fails_left > 0` and `spec.role == spec_role` it decrements and returns `AgentRun { stop_reason: reason, error: Some("Invalid API key".into()), ..AgentRun::done("") }`; otherwise it delegates to `self.inner.run(spec)`. So a test can fail once and then approve on a retry with the same driver.
  - `pub fn approved_job_dyn(runner: Arc<dyn AgentRunner>, scripted: Arc<ScriptedRunner>, with_context: bool, config: Config) -> MergeFixture`: move the body of `approved_job_config` into it, build the driver with `Box::new(move |_| runner.clone())` as the runner factory, and set `MergeFixture.runner = scripted`. `approved_job_config` becomes a call to it with `(runner.clone(), runner, ..)`, so the about 39 existing `fx.runner.<method>` uses keep working. Tests pass the same `Arc<ScriptedRunner>` as `RoleFailing.inner` and as `scripted`.

- [ ] **Step 1: Read** `decide.rs`, the role loop, `combine_verdicts`, `apply_decision`, `decision_parts`, `sync_with` from 180 to 232, `tests/common/mod.rs` (`ScriptedRunner`, `MergeFixture`, `driver_for`, `approved_job_config`), and one `review_stage` test in `review_decide.rs`.
- [ ] **Step 2: Write the failing tests**:
  - `decide.rs`: `a_failed_role_with_missing_verdict_parks_review_failed`; `a_failed_review_on_failing_checks_still_starts_a_fix_round`; `budget_stop_wins_over_a_failed_review`.
  - `review_decide.rs`: `a_failed_reviewer_parks_as_review_failed` (`RoleFailing::new("reviewer", StopReason::Failed, 1, inner)`: needs-human, `park_kind == "review-failed"`, `park_resume == Some(Review)`, no `changes-r2.md`, round still 1, no builder spec run, park.json evidence contains `Invalid API key`, `park_summary` starts with `review failed`); `a_stalled_reviewer_parks_as_review_failed`; `a_turn_limit_review_is_not_review_failed` (fix round starts as today); `a_real_changes_verdict_still_starts_a_fix_round`.
  - `multi_review.rs`: `a_failed_role_with_real_findings_starts_a_fix_round` (security-review `Failed`, inner script makes the reviewer answer `VERDICT: CHANGES` with one blocker: fix round 2); `a_failed_role_with_an_approving_role_parks`; rename and update the existing `a_missing_security_agent_parks_with_a_clear_reason` to `a_missing_security_agent_parks_as_review_failed`, expecting `ParkKind::ReviewFailed` and `outcome.resume == Some(Review)`.
  - `sync_review.rs`: `a_failed_requested_sync_review_parks_with_resume_rereview`.
  - `merge.rs`: `a_failed_auto_sync_review_parks_with_resume_rereview` (main moved, `approved_job_dyn` with `RoleFailing` on `"reviewer"`, `merge_stage`: `park_kind == "review-failed"`, `park_resume == Some(Rereview)`).
- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine --lib decide` and `cargo test -j4 -p factory-engine --test review_decide --test multi_review --test sync_review --test merge`, and see the new tests fail.
- [ ] **Step 4: Implement** the Produces items. A `Done` run without a parsable verdict keeps today's `Missing` path (fix round).
- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`, and clippy for both crates. Fix any existing test whose fake script ran out (see Global Constraints). Expected: pass.
- [ ] **Step 6: Commit** `feat(engine): park a failed review instead of a fix round`.

Next plan: docs/superpowers/plans/2026-11-19-retry-at-parked-stage/plan-02-retry-at-parked-stage.md
