# Crash resume, Plan 3: Resume after a crash and the README

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** A re-run of a `Dispatched` task whose driver crashed resumes the saved builder session once with a short "you were interrupted" prompt, falls back to one fresh builder when that resume fails, writes `resumed` or `fresh` in the ledger line, and the README explains the job lease, the resume and how to keep a run alive when the terminal drops.

**Architecture:** All logic sits in `factory-engine`. `specs.rs` gets the constant `INTERRUPTED_PROMPT`, a helper that fills in the task number and a `builder_task_resume_spec` built like the fix-round `builder_resume_spec`. `build_task` in `build.rs` compares `job.task_session` (written by plan 2's hook) with the task number and the branch tip at its start, picks the resume or the fresh spec, writes the ledger suffix, and on a resumed run that returns `StopReason::Failed` clears `task_session` and runs `builder_task_spec` once more on the worktree as it is. Every builder run still goes through `run_builder_guarded`, so the tamper guard applies to both runs. No board, ledger or events field is added: `Job.task_session` comes from plan 2, and the ledger only gets a longer detail text. The README gets a `### The job lease` subsection pinned by a doc test in `agent_lint.rs`.

**Tech Stack:** Rust, `git2` through `factory_git::Repo`, the `ScriptedRunner` fake in `crates/factory-engine/tests/common/mod.rs`, `tempfile` for tests.

**Spec:** docs/superpowers/specs/2026-10-20-crash-resume-design.md

## Global Constraints

- Always `-j4`; never `--workspace` or `--all`; never set `CARGO_TARGET_DIR`. Use per-crate commands: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>` (with `--test <file>` for an integration test file).
- Board JSON stays compatible with `factory-server`: `task_session` is optional (plan 2 adds it with `#[serde(default, skip_serializing_if = "Option::is_none")]`). This plan adds no board, ledger or events field. The ledger `Dispatched` detail gets a third comma-separated part (`resumed` or `fresh`); `Ledger::tasks` and the dashboard's `apply_ledger` (`dashboard/src/plan.rs`) match on the state label only, so they read it unchanged.
- Board, ledger and events are written only by the driver; the hook runs inside the driver process.
- Engine logic stays behind the ports in `ports.rs`; tests use fakes. Both tasks that touch `factory-engine` run agents through the `factory_agent::AgentRunner` port (`run_builder_guarded` calls `Engine::run_agent` in `ports.rs`), write the board through `Engine::update` in `ports.rs` (a `BoardStore` with the board lock) and the ledger through `Engine::ledger_task` in `ports.rs`. Tests use the `ScriptedRunner` fake and a small wrapper over it.
- Never read `/proc` directly; process checks go through `procinfo.rs`. This plan adds no process check.
- Only `git` may not be shelled out to: the tip comes from `factory_git::Repo::tip(&self, branch: &str)`, as `build_task` does today.
- Each task lists the tests it adds. Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- Comments are short full sentences ending with a punctuation mark.
- Plan 2 must be merged first: this plan consumes `factory_core::board::TaskSession`, `Job.task_session`, `AgentSpec.on_session` and the hook and clearing rules plan 2 puts in `build_task`. Keep plan 2's hook on every builder spec this plan builds, the resume spec and the fallback spec included.

## Review Focus

1. A resumed run that the tamper guard parks (the builder changed `plan.md`). Expected: the job parks as `builder-tampered`, no fresh builder runs, `task_session` is `None` afterwards. Pinned by `a_tampered_resume_parks_without_a_fresh_run` (Task 2).
2. A resumed run that returns `StopReason::Done` with a `BLOCKED` reply. Expected: it parks as `builder-blocked` like a normal run, no fresh builder runs. Pinned by `a_blocked_resume_parks_without_a_fresh_run` (Task 2).
3. A resumed run that stops on the budget or the turn limit (`StopReason::BudgetStop`, `StopReason::TurnLimit`). Expected: parked as `builder-stopped`, never retried fresh. Pinned by `a_resume_that_hits_a_limit_is_not_retried` (Task 2), next to the stall case `a_stalled_resume_is_not_retried`.
4. A failed resume whose fresh fallback also fails. Expected: exactly one resumed and one fresh builder run, then a `builder-stopped` park; no loop. Pinned by `a_failed_fallback_parks_after_one_fresh_run` (Task 2).
5. A successful resume of task 1 followed by task 2. Expected: task 2 starts fresh (`resume` is `None`, the prompt says `Do only Task 2`) and its ledger line ends `fresh`. Pinned by `the_resume_is_used_once` (Task 1).
6. A plan without `### Task N:` headings (the legacy whole-plan builder in `build_stage`). Expected: its `Dispatched` line keeps the old text `BASE <base>, <model>` with no `resumed` or `fresh`. Pinned by `the_whole_plan_dispatched_line_is_unchanged` (Task 1).
7. A crash in a later task: `task_session` for task 2 with the tip after task 1. Expected: the resume prompt names task 2. Pinned by `a_resume_prompt_names_the_task` (Task 1, second case).

---

## File Structure

- `crates/factory-engine/src/specs.rs` (modify): `INTERRUPTED_PROMPT`, `interrupted_prompt`, `Engine::builder_task_resume_spec`.
- `crates/factory-engine/src/build.rs` (modify): `build_task` picks resume or fresh, writes the ledger suffix, clears a stale `task_session`, runs the fallback; a private helper `saved_session`.
- `crates/factory-engine/src/ports.rs` (modify only if plan 2 left it out): `JobView.task_session` filled in `Engine::job`.
- `crates/factory-engine/tests/build_checks.rs` (modify): the helper `crashed_in_task_one`, the wrapper runner `ResumeStops`, the tests of Tasks 1 and 2.
- `README.md` (modify): the `### The job lease` subsection.
- `crates/factory-cli/tests/agent_lint.rs` (modify): `readme_documents_the_job_lease_and_resume`.

### Task 1: Resume a saved builder session and say so in the ledger

**Files:**
- Modify: `crates/factory-engine/src/specs.rs`, `crates/factory-engine/src/build.rs`, `crates/factory-engine/tests/build_checks.rs`
- Modify if needed: `crates/factory-engine/src/ports.rs`

**Interfaces:**
- Consumes (plan 2): `factory_core::board::TaskSession { pub task: u32, pub id: String, pub tip: String }`; `Job.task_session: Option<TaskSession>`; the session hook plan 2 sets with `spec.on_session` in `build_task`. Existing: `Engine::builder_task_spec(&self, job: &JobView, cwd: &Path, task: &PlanTask, base: &str, tip: &str, earlier: &[String]) -> AgentSpec`, `Engine::with_builder_permissions(&self, spec: AgentSpec) -> AgentSpec`, `Engine::with_limits(&self, spec: AgentSpec, model: &str) -> AgentSpec`, `Engine::ledger_task(&self, n: u32, state: TaskState, detail: &str) -> Result<(), EngineError>`, `Repo::tip(&self, branch: &str)`, the constants `BUILDER_AGENT`, `BUILDER_MAX_TURNS`, `BUILDER_TOOLS` in `specs.rs`.
- Produces:
  - In `crates/factory-engine/src/specs.rs`: `pub(crate) const INTERRUPTED_PROMPT: &str = "You were interrupted. Run git status and git diff, check what is already done, continue task {n}, and do not redo finished work.";` and `pub(crate) fn interrupted_prompt(n: u32) -> String`, which replaces `{n}` with the task number.
  - `pub(crate) fn builder_task_resume_spec(&self, cwd: &Path, task: &PlanTask, session: &str) -> AgentSpec` on `Engine`: role `builder`, prompt `interrupted_prompt(task.n)`, `resume = Some(session)`, and the same `agent`, `max_turns`, `strict_mcp`, `tools`, `add_dirs`, builder permissions and limits as `builder_task_spec`.
  - In `crates/factory-engine/src/build.rs`: `fn saved_session(job: &JobView, n: u32, tip: &str) -> Option<String>`: the session id when `job.task_session` is `Some` with `task == n` and `tip` equal to the recorded tip, else `None`.
  - `JobView` has `pub task_session: Option<factory_core::board::TaskSession>`, copied in `Engine::job` from `job.task_session`. Add it only if plan 2 did not.
  - `build_task`: after `let tip = self.repo.tip(&branch)`, the `Dispatched` detail is `BASE {base}, {model}, resumed` when `saved_session` gives an id and `BASE {base}, {model}, fresh` otherwise. The resume uses `builder_task_resume_spec`; a fresh start uses `builder_task_spec` as today. Plan 2's session hook is set on whichever spec runs, and a rework note is appended to its prompt as today. The legacy line in `build_stage` is not changed.

- [ ] **Step 1: Read** spec sections 2 and 3, the edge-case table and the Decisions on the resume, the tip and the clearing. Read `crates/factory-engine/src/build.rs` (`build_stage`, `build_task`, `start_worktree`, `existing_worktree`), `crates/factory-engine/src/specs.rs` (`builder_task_spec`, `builder_resume_spec`, `with_limits`, `with_builder_permissions`), `crates/factory-engine/src/permissions.rs` (`run_builder_guarded`, `GUARDED_FILES`), `crates/factory-engine/src/ports.rs` (`JobView`, `Engine::job`, `update`, `ledger_task`), `crates/factory-core/src/ledger.rs` (`task`, `parse_task_line`), plan 2's changes to `build_task` and `crates/factory-core/src/board.rs` (`TaskSession`), and in `crates/factory-engine/tests/build_checks.rs` the fixtures `two_task_fixture()`, `change_effect`, `blocked_effect` and the test `a_stopped_first_task_parks_and_a_rework_resumes_at_that_task_with_the_note`, plus `scripted`, `scripted_auto`, `ScriptedRunner::specs_for` and `Fixture::ledger` in `crates/factory-engine/tests/common/mod.rs`.
- [ ] **Step 2: Write the failing tests** in `crates/factory-engine/tests/build_checks.rs`. First the shared helper, which plays a driver that died in task 1: a failed first run creates the worktree, then the job goes back to Build and the session plan 2's hook would have saved is written by hand.

```rust
use factory_core::board::TaskSession;

/// Leaves job 001 as a driver crash in task 1 does: a worktree, stage Build, task index 0 and a saved
/// session. Returns the branch tip recorded in the session.
fn crashed_in_task_one(fx: &Fixture, session: &str) -> String {
    // An empty script makes the runner report a failed run, so the worktree exists and nothing is committed.
    let dead = scripted(vec![]);
    fx.engine(&dead).build_stage().expect("first run");
    let mut board = factory_core::Board::load(&fx.board_path).expect("load");
    let job = board.find_mut("001").expect("job");
    job.set_stage(Stage::Build, None, true, 1.0).expect("stage");
    let tip = fx.repo.tip(&job.branch).expect("tip");
    job.task_session = Some(TaskSession {
        task: 1,
        id: session.to_string(),
        tip: tip.clone(),
    });
    board.save(&fx.board_path).expect("save");
    tip
}

/// The `Task N: dispatched` lines of the ledger, in order.
fn dispatched_lines(fx: &Fixture) -> Vec<String> {
    fx.ledger()
        .lines()
        .filter(|l| l.contains(": dispatched"))
        .map(str::to_string)
        .collect()
}
```

  One full test:

```rust
#[test]
fn a_rerun_resumes_the_saved_session() {
    let fx = two_task_fixture();
    crashed_in_task_one(&fx, "sess-old");
    let runner = scripted_auto(vec![change_effect("a.txt", 3)]);
    let out = fx.engine(&runner).build_stage().expect("rerun");
    assert_eq!(out, StageOutcome::Advanced(Stage::Build));
    let builders = runner.specs_for("builder");
    assert_eq!(builders.len(), 1);
    assert_eq!(builders[0].resume.as_deref(), Some("sess-old"));
    assert_eq!(builders[0].agent.as_deref(), Some("factory-builder"));
    assert_eq!(builders[0].max_turns, Some(120));
    assert!(builders[0].strict_mcp);
    let job = fx.job();
    assert_eq!(job.task_index, 1);
    assert!(job.task_session.is_none());
    assert_eq!(job.builder_id.as_deref(), Some("sess-1"));
}
```

  The other tests:

| Test | Input | Assertion |
|---|---|---|
| `a_resume_prompt_names_the_task` | case 1: `crashed_in_task_one(&fx, "sess-old")`, then `build_stage` with `scripted_auto(vec![change_effect("a.txt", 3)])`. Case 2, on a new `two_task_fixture()`: task 1 built with `scripted_auto(vec![change_effect("a.txt", 3)])`, then `task_session = Some(TaskSession { task: 2, id: "sess-two".into(), tip: <branch tip now> })` written to the board, then `build_stage` with `scripted_auto(vec![change_effect("b.txt", 2)])` | case 1 builder prompt equals `"You were interrupted. Run git status and git diff, check what is already done, continue task 1, and do not redo finished work."`; case 2 the last builder spec has `resume == Some("sess-two")` and the same text with `continue task 2` |
| `the_ledger_says_resumed_or_fresh` | `crashed_in_task_one(&fx, "sess-old")`, then two `build_stage` calls with `scripted_auto(vec![change_effect("a.txt", 3), change_effect("b.txt", 2)])` | `dispatched_lines(&fx)` has 3 lines: `Task 1: dispatched (BASE <base>, sonnet, fresh)`, `Task 1: dispatched (BASE <base>, sonnet, resumed)`, `Task 2: dispatched (BASE <base>, sonnet, fresh)`, with `<base>` from `fx.job().base.expect("base")`; `factory_core::Ledger::new(fx.job_dir.join("progress.md")).tasks()` still parses each as `TaskState::Dispatched` |
| `the_resume_is_used_once` | as `the_ledger_says_resumed_or_fresh` | the second builder spec has `resume` `None` and its prompt contains `Do only Task 2`; `fx.job().task_index == 2` |
| `the_whole_plan_dispatched_line_is_unchanged` | `committing_fixture()` with the default one-line plan, `scripted_auto(vec![change_effect("a.txt", 3)])`, one `build_stage` | `dispatched_lines(&fx) == vec![format!("Task 1: dispatched (BASE {base}, sonnet)")]` |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine --test build_checks` and expect FAIL: `a_rerun_resumes_the_saved_session` (`resume` is `None`), `a_resume_prompt_names_the_task` (the prompt is the fresh task prompt), `the_ledger_says_resumed_or_fresh` (lines end `sonnet)`). `the_resume_is_used_once` and `the_whole_plan_dispatched_line_is_unchanged` already PASS; they pin behaviour that must not change. Every older test still PASSES.
- [ ] **Step 4: Implement** the Interfaces above. In `build_task`, call `saved_session(job, task.n, &tip)` right after the tip is read, pass the suffix to the `ledger_task` call, and build the spec from it. Give `INTERRUPTED_PROMPT` the doc comment `/// The prompt of a resumed task builder after a crash. `{n}` is the task number.` and `builder_task_resume_spec` the doc comment `/// The resumed builder of a task whose driver died while it ran.` Do not touch the `Dispatched` line in `build_stage`.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine --test build_checks` and expect every test to PASS, then `cargo test -j4 -p factory-engine` and expect PASS (plan 2's session tests in `build_checks.rs` included).

### Task 2: Fall back to one fresh builder when the resume fails

**Files:**
- Modify: `crates/factory-engine/src/build.rs`, `crates/factory-engine/tests/build_checks.rs`

**Interfaces:**
- Consumes: Task 1 (`saved_session`, `builder_task_resume_spec`, the ledger suffix); `Engine::run_builder_guarded(&self, spec: AgentSpec) -> Result<Result<AgentRun, StageOutcome>, EngineError>`; `Engine::update`; `factory_agent::{AgentRun, AgentRunner, AgentSpec, StopReason}` (`StopReason` is `Copy`, variants `Done`, `BudgetStop`, `Stalled`, `TurnLimit`, `Failed`); `AgentRun::failed(msg: &str) -> AgentRun`; `ParkKind::{BuilderStopped, BuilderTampered, BuilderBlocked}`.
- Produces (behaviour of `build_task`, no new public item):
  - When `job.task_session` is `Some` but `saved_session` gives `None` (another task, or the tip moved), `task_session` is set to `None` through `Engine::update` before the fresh builder runs.
  - When the resumed run returns `Ok(run)` with `run.stop_reason == StopReason::Failed`: `task_session` is set to `None`, a second `Dispatched` line with the `fresh` suffix is written, `builder_task_spec(job, &cwd, task, &base, &tip, &earlier)` is built with plan 2's hook and the rework note (if any), and run once through `run_builder_guarded`. Its result replaces the first one and goes through the usual checks; a second `Failed` parks as `builder-stopped`. The worktree is never reset or cleaned.
  - Any other stop reason of the resumed run, a tamper park (`Err(parked)`) and a `BLOCKED` or `NEEDS_CONTEXT` reply are handled as for a normal run, with no fallback. `task_session` is `None` after every builder run that returns, the tamper park included.
  - The rework note is removed only after the last builder run returns.

- [ ] **Step 1: Read** spec section 3 bullets three to eight and the edge-case rows from "`task_session` is for another task" to "Resumed run parks", the Task 1 `build_task`, `run_builder_guarded` and `GUARDED_FILES` in `crates/factory-engine/src/permissions.rs`, `builder_stop_parts` in `crates/factory-engine/src/park.rs`, `AgentRun` and `StopReason` in `crates/factory-agent/src/run.rs`, and the `AgentRunner` impl of `ScriptedRunner` in `crates/factory-engine/tests/common/mod.rs`.
- [ ] **Step 2: Write the failing tests** in `crates/factory-engine/tests/build_checks.rs`. A wrapper runner answers every resumed builder run with one stop reason and passes the rest to a `ScriptedRunner`:

```rust
use factory_agent::{AgentRun, AgentRunner, AgentSpec, StopReason};

/// Answers every builder run that resumes a session with `stop`, and passes all other runs to `inner`.
struct ResumeStops {
    inner: ScriptedRunner,
    stop: StopReason,
    resumed: std::sync::Mutex<Vec<AgentSpec>>,
}

impl AgentRunner for ResumeStops {
    fn run(&self, spec: &AgentSpec) -> AgentRun {
        if spec.role == "builder" && spec.resume.is_some() {
            self.resumed.lock().expect("resumed").push(spec.clone());
            return AgentRun {
                stop_reason: self.stop,
                ..AgentRun::failed("resume stopped")
            };
        }
        self.inner.run(spec)
    }
}

fn resume_stops(stop: StopReason, inner: ScriptedRunner) -> ResumeStops {
    ResumeStops {
        inner,
        stop,
        resumed: std::sync::Mutex::new(Vec::new()),
    }
}
```

  One full test:

```rust
#[test]
fn a_failed_resume_falls_back_to_a_fresh_builder() {
    let fx = two_task_fixture();
    crashed_in_task_one(&fx, "sess-old");
    let runner = resume_stops(
        StopReason::Failed,
        scripted_auto(vec![change_effect("a.txt", 3)]),
    );
    let out = fx.engine(&runner).build_stage().expect("rerun");
    assert_eq!(out, StageOutcome::Advanced(Stage::Build));
    let resumed = runner.resumed.lock().expect("resumed").clone();
    assert_eq!(resumed.len(), 1);
    assert_eq!(resumed[0].resume.as_deref(), Some("sess-old"));
    let fresh = runner.inner.specs_for("builder");
    assert_eq!(fresh.len(), 1);
    assert!(fresh[0].resume.is_none());
    assert!(fresh[0].prompt.contains("Do only Task 1"), "{}", fresh[0].prompt);
    let job = fx.job();
    assert_eq!(job.task_index, 1);
    assert!(job.task_session.is_none());
    let lines = dispatched_lines(&fx);
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(lines[1].ends_with(", sonnet, resumed)"), "{lines:?}");
    assert!(lines[2].ends_with(", sonnet, fresh)"), "{lines:?}");
}
```

  The other tests, each on a new `two_task_fixture()` after `crashed_in_task_one(&fx, "sess-old")`:

| Test | Input | Assertion |
|---|---|---|
| `a_stalled_resume_is_not_retried` | `resume_stops(StopReason::Stalled, scripted_auto(vec![change_effect("a.txt", 3)]))` | `NeedsHuman` with a reason containing `builder stopped in task 1: Stalled`; `ParkRecord::read(&fx.job_dir)` kind `ParkKind::BuilderStopped`; `runner.inner.specs_for("builder")` is empty; `dispatched_lines` has 2 lines, the second ending `resumed)`; `task_session` is `None`; `task_index == 0` |
| `a_resume_that_hits_a_limit_is_not_retried` | the same, once with `StopReason::BudgetStop` and once with `StopReason::TurnLimit`, each on its own fixture | as `a_stalled_resume_is_not_retried`, with `BudgetStop` and `TurnLimit` in the reason |
| `a_failed_fallback_parks_after_one_fresh_run` | `resume_stops(StopReason::Failed, scripted(vec![]))` | `NeedsHuman` with `builder stopped in task 1: Failed`; one resumed spec; `runner.inner.specs_for("builder").len() == 1` with `resume` `None`; `dispatched_lines` has 3 lines ending `fresh)`, `resumed)`, `fresh)` |
| `a_session_of_another_task_is_ignored` | before the run, the board's `task_session.task` is set to `2`; `scripted_auto(vec![change_effect("a.txt", 3)])` | the one builder spec has `resume` `None` and contains `Do only Task 1`; the last dispatched line ends `fresh)`; `task_session` is `None`; `task_index == 1` |
| `a_moved_tip_starts_fresh` | before the run, the board's `task_session.tip` is set to `"0".repeat(40)`; `scripted_auto(vec![change_effect("a.txt", 3)])` | as `a_session_of_another_task_is_ignored` |
| `a_fresh_fallback_keeps_the_partial_edits` | `std::fs::write(<fx.job().worktree>/partial.txt, "half\n")` before the run; `resume_stops(StopReason::Failed, scripted_auto(vec![effect]))`, where `effect` stores `spec.cwd.join("partial.txt").exists()` in an `Arc<AtomicBool>` and writes `a.txt` | the flag is `true`; `partial.txt` in the worktree still reads `half\n`; `task_index == 1` |
| `a_tampered_resume_parks_without_a_fresh_run` | `scripted_auto(vec![effect])`, where `effect` writes `"# changed\n"` to `<fx.job_dir>/plan.md` (a clone of the path is captured) | `ParkRecord` kind `ParkKind::BuilderTampered`; `runner.specs_for("builder").len() == 1` with `resume == Some("sess-old")`; `task_session` is `None`; `plan.md` holds `TWO_TASKS` again |
| `a_blocked_resume_parks_without_a_fresh_run` | `scripted_auto(vec![blocked_effect("a.txt", "BLOCKED: no schema")])` | `ParkRecord` kind `ParkKind::BuilderBlocked`; `runner.specs_for("builder").len() == 1` with `resume == Some("sess-old")`; `task_index == 0` |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine --test build_checks` and expect FAIL: `a_failed_resume_falls_back_to_a_fresh_builder`, `a_failed_fallback_parks_after_one_fresh_run` and `a_fresh_fallback_keeps_the_partial_edits` park as `builder stopped in task 1: Failed` with no fresh run. `a_tampered_resume_parks_without_a_fresh_run` fails on `task_session` if plan 2 clears it only on `Ok(run)`. The stall, limit, other-task, moved-tip and blocked tests may already PASS after Task 1; they pin the rules this task must keep.
- [ ] **Step 4: Implement** the behaviour in Interfaces inside `build_task`, keeping one `run_builder_guarded` call per builder run. Add the comment `// A failed resume, such as a missing transcript, gets one fresh builder on the worktree as it is.` at the fallback. Do not add a retry loop.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine --test build_checks` and expect every test to PASS, then `cargo test -j4 -p factory-engine` and expect PASS, then `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings` and `cargo fmt -p factory-engine -- --check`, both PASS.

### Task 3: The job lease and resume in the README

**Files:**
- Modify: `README.md`, `crates/factory-cli/tests/agent_lint.rs`

**Interfaces:**
- Consumes: the helpers in `crates/factory-cli/tests/agent_lint.rs`: `type TestResult = Result<(), String>`, `fn read(rel: &str) -> Result<String, String>`, `fn require_all(text: &str, label: &str, phrases: &[&str]) -> TestResult`, `fn forbid_all(text: &str, label: &str, phrases: &[&str]) -> TestResult`; plan 1's dead-owner job lease (`lease_owner_dead`, `lease_is_live`).
- Produces: a `### The job lease` subsection in `README.md` right after the `### The project lease` subsection (after the paragraph that ends "Only `stall_minutes` covers a silent one.") and before `### factoryd`; the test `fn readme_documents_the_job_lease_and_resume() -> TestResult`. No code changes.

- [ ] **Step 1: Read** spec sections 1, 3 and 5, `README.md` from `### The project lease` to `### factoryd` (note the existing sentence that both leases are renewed every 60 seconds, and the `factoryd enqueue` usage line), and `readme_documents_upgrading` in `crates/factory-cli/tests/agent_lint.rs` for the test shape.
- [ ] **Step 2: Write the failing test** in `crates/factory-cli/tests/agent_lint.rs`:

```rust
#[test]
fn readme_documents_the_job_lease_and_resume() -> TestResult {
    let doc = read("README.md")?;
    require_all(
        &doc,
        "README.md",
        &[
            "### The job lease",
            "every 60 seconds",
            "`driver-<pid>-<secs>`",
            "counts as stale at once, for the job lease too",
            "resumes that session",
            "one fresh builder runs on the worktree as it is",
            "`resumed` or `fresh`",
            "`factoryd enqueue`",
            "`nohup`",
            "`tmux`",
            "survive a dropped terminal",
        ],
    )?;
    forbid_all(&doc, "README.md", &["the job lease is judged by expiry only"])
}
```

- [ ] **Step 3: Run** `cargo test -j4 -p factory-cli --test agent_lint readme_documents_the_job_lease_and_resume` and expect FAIL: `README.md` is missing `### The job lease`.
- [ ] **Step 4: Write the subsection.** Three short paragraphs, in the README's wrapped style:
  - The lease: each job also has a lease on the board, held by the driver that works on it. A running driver renews both the project lease and the job lease every 60 seconds. A job lease whose owner is a `driver-<pid>-<secs>` process that no longer runs on this machine counts as stale at once, for the job lease too, so `run`, `approve`, `cancel` without `--force`, `tickets` and `kb-refresh` take it over on the first retry. Other owners keep it until it expires (30 minutes).
  - The resume: while a task builder runs, the driver saves its session id and the branch tip on the board. After a crash the next `run` resumes that session with a short "you were interrupted" prompt, as long as the branch tip has not moved. If the resume fails, one fresh builder runs on the worktree as it is, with the interrupted edits kept. The ledger line of the dispatched task ends in `resumed` or `fresh`.
  - The terminal: a run started in a terminal stops when the terminal goes away. For runs that must survive a dropped terminal, use `factoryd enqueue`, or start `factory run` under `nohup` or in `tmux`.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-cli --test agent_lint` and expect every test to PASS, `readme_documents_the_job_lease_and_resume` included. Then `cargo test -j4 -p factory-engine` and `cargo test -j4 -p factory-cli` and expect PASS.

Next plan: none
