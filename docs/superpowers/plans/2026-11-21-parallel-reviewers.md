# Parallel Reviewers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A review round that holds the code and security roles runs both reviewer agents at the same time, and everything the round produces stays as it is today.

**Commit:** perf(review): run both reviewers at once

**Architecture:** `Engine::run_agent` (`ports.rs`) splits into prepare, run and finish. A new `run_agents` prepares every spec in order, runs the runner calls inside `std::thread::scope`, then finishes them in order. `review_stage_as` (`review.rs`) builds its specs first, calls `run_agents`, and handles the replies in `to_run` order exactly as the loop body does today.

**Tech Stack:** Rust std threads (`std::thread::scope`), `tracing` (already a dependency of factory-engine). No new crates.

**Spec:** `docs/superpowers/specs/2026-11-21-parallel-reviewers-design.md`

## Global Constraints

- No new dependency and no change to any `Cargo.toml` or `Cargo.lock`.
- No async runtime. Only the `runner.run` calls cross threads; the `Engine` stays on the calling thread.
- Replies are handled in `to_run` order (code, then security), never in finish order.
- With one spec, `run_agents` spawns no thread.
- A prepare error (lost flag, provider sync, `agents.json`) returns before any run starts.
- A `shape_spec` failure gives that role `AgentRun::failed(..)` without a run, as today; the other role still runs.
- cargo: always `-j4`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Commits go through the `dev-workflow-skills:1-git-commit` skill.

## Review Focus

- The security run finishes first: the merged review file still lists `## code review` before `## security review`, and `verdicts` keep code first. Pinned by `merged_review_keeps_code_first_when_security_finishes_first`.
- Each run's `agent.end` `duration_ms` and usage row belong to its own role, not to the slowest run. Pinned by `parallel_runs_log_their_own_duration`.
- A line the runner logs inside its thread keeps the job id. Pinned by `parallel_runs_keep_the_job_span`.
- The lost flag is set before the review: no reviewer starts at all. Pinned by `lost_flag_starts_no_reviewer`.
- A sync review (code only) spawns no thread and behaves as today; covered by the existing `sync_review.rs` tests.

---

### Task: Split run_agent and make runners shareable

**Files:**
- Modify: `crates/factory-agent/src/runner.rs` (`AgentRunner` trait)
- Modify: `crates/factory-engine/src/ports.rs` (`run_agent` ~258-311)
- Modify: `crates/factory-engine/tests/common/mod.rs` (new `scripted_roles`)
- Modify: `crates/factory-engine/tests/multi_review.rs`, plus any test in `crates/factory-engine/tests/` or `crates/factory-cli/tests/` whose review holds both roles and scripts replies in call order

**Interfaces:**
- Produces: `pub trait AgentRunner: Send + Sync` (same single method `run`).
- Produces in `ports.rs`: `pub(crate) enum Prepared { Ready(AgentSpec), Refused(AgentRun) }`; `pub(crate) fn prepare_agent(&self, spec: AgentSpec) -> Result<Prepared, EngineError>` (lost flag, `sync_provider`, model, `shape_spec`, `agents.json`, `agent.start` log); `pub(crate) fn finish_agent(&self, spec: &AgentSpec, run: AgentRun, elapsed: std::time::Duration) -> Result<AgentRun, EngineError>` (`agent.end` log with `duration_ms` from `elapsed`, token add, `log_usage` with `elapsed.as_secs()`). `run_agent` keeps its signature and chains the three; a `Refused` run returns as today, with no finish.
- Produces in tests: `pub fn scripted_roles(effects: Vec<(&'static str, Effect)>) -> ScriptedRunner`. Effects are queued per spec role (`"reviewer"`, `"security-review"`, `"builder"`, ...), in order within a role; a role with no queued effect fails with `"script exhausted"`. `call_count`, `specs_for` and `last_spec` keep working.

- [ ] **Step 1: Make `AgentRunner` require `Send + Sync`.** Run `cargo check -j4 -p factory-engine --tests` and `cargo check -j4 -p factory-cli --tests`. Fix any test runner that is not `Sync` by swapping its `Cell`/`RefCell` for a `Mutex` or an atomic. Expected: both compile.

- [ ] **Step 2: Split `run_agent`** into `prepare_agent`, the run, and `finish_agent` as in Interfaces, with no behaviour change.

- [ ] **Step 3: Add `scripted_roles`** to `tests/common/mod.rs`, and move every test that scripts both a `"reviewer"` and a `"security-review"` reply to it. In `multi_review.rs` that is each `scripted(vec![reply(CODE_OK), reply(SECURITY_..)])`, plus the `inner` runners built for `RoleFailing`. Example: `scripted_roles(vec![("reviewer", reply(CODE_OK)), ("security-review", reply(SECURITY_BAD))])`. Find the others with `grep -rn "security" crates/factory-engine/tests crates/factory-cli/tests`.

- [ ] **Step 4: Run the tests.** Run `cargo test -j4 -p factory-agent`, `cargo test -j4 -p factory-engine` and `cargo test -j4 -p factory-cli`. Expected: PASS, with no test changed in what it asserts.

- [ ] **Step 5: Commit:** `refactor(engine): split run_agent into three steps`.

### Task: Run the review roles in parallel

**Files:**
- Modify: `crates/factory-engine/src/ports.rs` (new `run_agents`)
- Modify: `crates/factory-engine/src/review.rs` (the `for role in to_run` loop in `review_stage_as`, ~186-260)
- Modify: `docs/reference/software-factory.md:28`
- Test: `crates/factory-engine/tests/multi_review.rs`

**Interfaces:**
- Consumes: `prepare_agent`, `finish_agent`, `Prepared` and `scripted_roles` from the split above.
- Produces: `pub(crate) fn run_agents(&self, specs: Vec<AgentSpec>) -> Result<Vec<AgentRun>, EngineError>`, which returns the runs in input order.

- [ ] **Step 1: Write the failing tests** in `multi_review.rs`. Use a test runner that wraps a `scripted_roles` runner and adds per-role sleeps and an in-flight counter. All of them run a round holding both roles through the existing `reviewed_fixture_with` and `engine` helpers.
  - `reviewers_run_at_the_same_time`: each reviewer run increments an `AtomicUsize`, waits up to 2 s for it to reach 2, records the highest value seen, then decrements. Assert the highest value seen `== 2`. The wait has a timeout so a sequential build fails rather than hangs.
  - `merged_review_keeps_code_first_when_security_finishes_first`: the `"reviewer"` run sleeps 300 ms, security returns at once with `SECURITY_BAD`. Assert that `review-code-r1.md` has `## code review` before `## security review`, the outcome verdict is `Changes`, and on the board the `code` review is `Pass` and `security` is `Changes`.
  - `parallel_runs_log_their_own_duration`: inside `factory_telemetry::capture("info", ..)` and `tracing::info_span!("job", job_id = "001")`, the `"reviewer"` run sleeps 1200 ms and security none. Assert that the `agent.end` line with role `reviewer` has `duration_ms >= 1200` and the one with role `security-review` has `duration_ms < 1000`.
  - `parallel_runs_keep_the_job_span`: in the same capture and span, each reviewer run logs `tracing::info!(event = "test.run", role = %spec.role)`. Assert two `test.run` lines, both with `job_id == "001"`.
  - `lost_flag_starts_no_reviewer`: an engine whose `lost` flag is set before `review_stage`. Assert it returns the lost error and the runner's `call_count() == 0`.

- [ ] **Step 2: Run them to see them fail.** Run `cargo test -j4 -p factory-engine --test multi_review`. Expected: the overlap and duration tests FAIL (highest value seen 1, security duration ≥ 1200 ms); the others may already pass.

- [ ] **Step 3: Implement `run_agents`.**
  - Prepare every spec in order with `?`, so an error returns before any run starts.
  - With one `Ready` spec, run it on the calling thread.
  - Otherwise, inside `std::thread::scope`, spawn one thread per `Ready` spec. Each thread measures its own `Instant`, runs `self.runner.run(&spec)` under the dispatcher and span captured before the scope (`tracing::dispatcher::get_default(|d| d.clone())`, `tracing::Span::current()`), and returns `(run, elapsed)`.
  - Join in input order. A thread panic is re-raised with `std::panic::resume_unwind`.
  - Then `finish_agent` each in input order. `Refused` entries keep their failed run.

- [ ] **Step 4: Rework the review loop.**
  - First pass over `to_run`: build each role's spec, as the loop does today, and set its review to `ReviewState::Running`.
  - Call `run_agents(specs)`.
  - Second pass: zip roles with runs, and keep today's body unchanged from the `multi` raw-file write onward (`parse_verdict`, `failed`, `role_text`, `role_verdict`, state, `verdicts`, `parts`).

- [ ] **Step 5: Run the tests.** Run `cargo test -j4 -p factory-engine` and `cargo test -j4 -p factory-cli`. Expected: PASS. Also run `cargo clippy -j4 -p factory-engine --tests` and `cargo fmt --check`. Expected: no warnings, no diff.

- [ ] **Step 6: Docs.** In `docs/reference/software-factory.md:28`, change "runs the roles one after the other, `code` then `security`" to say the roles run at the same time, and that the review file and verdicts always list `code` first.

- [ ] **Step 7: Commit:** `perf(review): run both reviewers at once`.

Next plan: none
