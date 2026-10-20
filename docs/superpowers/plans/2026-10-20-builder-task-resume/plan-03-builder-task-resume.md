# Builder task resume, plan 3: the carry decision and the carried run

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** With `builder_carry_session = true`, the builder of task N+1 resumes task N's session when the previous builder ended recently and its end context is small enough. Every other case starts a fresh builder, and the ledger line of each task says which happened and why.

**Architecture:** A pure `carry_decision` takes plain values and returns yes with a session id or no with a reason. `build_task` calls it next to the crash-resume check, runs a carried spec built like the existing resume spec, and widens the failed-resume fallback to cover a failed carry.

**Tech Stack:** Rust.

**Spec:** docs/superpowers/specs/2026-10-20-builder-task-resume-design.md

## Global Constraints

- Always `-j4`. Never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`. No `unwrap` in production code. Tests use `.expect("...")` and `expect_err`. No git shell-outs.
- The setting is off by default. With it off, the ledger text of a task stays exactly `fresh` and nothing else changes.
- Rule order of the decision: off, crash session, first task or no `builder_last`, rework note, previous session is not task N-1, idle gap, oversized context, yes.
- The idle limit follows `cache_ttl`: 3300 seconds for `"1h"`, 240 for `"5m"` or unset. The gap counts as 0 when the clock is earlier than `ended_at`.
- A carried run keeps `--agent factory-builder`, `strict_mcp`, the tools, `add_dirs` from `kb_dirs()`, `max_turns` 120, the builder permissions and limits, and the session hook of its task.
- Task 1, the whole-plan builder, a task with a rework note, and a task with a saved crash session are never carried. Fix rounds are unchanged. The builder agent files and `RULES_VERSION` do not change.

## Review Focus

1. A plan whose tasks are checkboxes, with no `### Task N:` section: the carried prompt points at the plan file instead of pasting a section (test `a_plan_without_a_task_section_points_at_the_plan_file`, Task 3).
2. A clock earlier than `ended_at`: no panic, the gap is 0 (test `a_clock_before_ended_at_counts_as_no_gap`, Task 1).
3. A carried resume of a session that no longer exists: one fresh builder, ledger `fresh (carry failed)` (test `a_failed_carry_gets_one_fresh_builder`, Task 3).
4. A crash during a carried task: the saved crash session wins (test `a_crash_session_beats_a_carry`, Task 3).
5. A fix round after a chain keeps resuming the last task's session (test `a_fix_round_after_a_chain_resumes_the_last_session`, Task 3).

## File Structure

- `crates/factory-engine/src/carry.rs` (create): `Carry`, `CarryInput`, `carry_idle_limit`, `carry_decision`, unit tests.
- `crates/factory-engine/src/lib.rs` (modify): `mod carry;`.
- `crates/factory-engine/src/specs.rs` (modify): `builder_task_carry_spec`.
- `crates/factory-engine/src/build.rs` (modify): the decision, the spec choice, the ledger text, the widened fallback.
- `crates/factory-engine/tests/task_carry.rs` (create), `tests/common/mod.rs` (modify: the shared `Tagged` runner).
- `README.md`, `docs/software-factory.md` (modify): documentation. `crates/factory-cli/tests/agent_lint.rs` (modify): one doc pin.

### Task 1: the pure carry decision

**Files:**
- Create: `crates/factory-engine/src/carry.rs`
- Modify: `crates/factory-engine/src/lib.rs`
- Test: `crates/factory-engine/src/carry.rs` (unit tests)

**Interfaces:**
- Consumes: `factory_core::BuilderLast { task, id, ended_at, end_ctx }` (plan 2).
- Produces (all `pub(crate)`):
  - `enum Carry { Yes { id: String }, No { reason: String } }` (`Debug, PartialEq`).
  - `struct CarryInput<'a> { enabled: bool, max_ctx: u64, idle_limit: u64, last: Option<&'a BuilderLast>, task_n: u32, now: u64, rework_note: bool, saved: bool }`.
  - `fn carry_idle_limit(cache_ttl: Option<&str>) -> u64`: 3300 for `Some("1h")`, 240 otherwise.
  - `fn carry_decision(i: &CarryInput) -> Carry`, with these reasons: `off`, `crash resume`, `first task`, `rework note`, `previous session is not task N-1`, `idle {gap}s over {limit}s` and `context {end_ctx} over {max_ctx}`. The idle rule fires when the gap is strictly above the limit, the context rule when `end_ctx` is strictly above the cap.
  - The function is pure and sits behind no port.

- [ ] **Step 1: Read** `crates/factory-engine/src/lib.rs` (module list), `ports.rs` (`JobView`) and `crates/factory-core/src/board.rs` (`BuilderLast`).
- [ ] **Step 2: Write the failing tests** in `carry.rs`. One full example:

```rust
fn input<'a>(last: Option<&'a BuilderLast>) -> CarryInput<'a> {
    CarryInput { enabled: true, max_ctx: 100_000, idle_limit: 240, last, task_n: 2, now: 1_000, rework_note: false, saved: false }
}
fn last(task: u32, ended_at: u64, end_ctx: u64) -> BuilderLast {
    BuilderLast { task, id: "sess-1".into(), ended_at, end_ctx }
}

#[test]
fn a_recent_small_previous_task_is_carried() {
    let l = last(1, 900, 30_000);
    assert_eq!(carry_decision(&input(Some(&l))), Carry::Yes { id: "sess-1".into() });
}
```

  The other tests:

| Test | Input | Assertion |
|---|---|---|
| `off_never_carries` | `enabled = false` | `No("off")` |
| `a_crash_session_comes_before_the_other_rules` | `saved = true`, `enabled = true`, a good `last` | `No("crash resume")` |
| `task_one_and_a_missing_last_are_first_task` | `task_n = 1`; and `last = None` | both `No("first task")` |
| `a_rework_note_blocks_the_carry` | `rework_note = true` | `No("rework note")` |
| `a_non_adjacent_previous_task_is_refused` | `last(3, ..)` for `task_n = 2`; `last(1, ..)` for `task_n = 3` | `No("previous session is not task N-1")` |
| `an_idle_gap_over_the_limit_is_refused` | `ended_at = 600`, `now = 1000`, limit 240 | `No("idle 400s over 240s")` |
| `a_gap_equal_to_the_limit_is_still_carried` | gap 240 | `Yes` |
| `a_clock_before_ended_at_counts_as_no_gap` | `ended_at = 2000`, `now = 1000` | `Yes` |
| `an_oversized_context_is_refused` | `end_ctx = 130_000` | `No("context 130000 over 100000")` |
| `a_context_equal_to_the_cap_is_carried` | `end_ctx = 100_000` | `Yes` |
| `carry_idle_limit_follows_the_cache_ttl` | `Some("1h")`, `Some("5m")`, `None` | 3300, 240, 240 |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine --lib carry` and expect a compile failure `cannot find module 'carry'` or unresolved items.
- [ ] **Step 4: Implement** the module and add `mod carry;` to `lib.rs`. The rules are checked in the order of the Global Constraints, first match wins.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine --lib carry` and expect all 12 tests to PASS.

### Task 2: the carried run in build_task

**Files:**
- Modify: `crates/factory-engine/src/specs.rs`, `crates/factory-engine/src/build.rs`, `crates/factory-engine/tests/common/mod.rs`
- Create: `crates/factory-engine/tests/task_carry.rs`
- Test: `crates/factory-engine/tests/task_carry.rs`

**Interfaces:**
- Consumes: `carry_decision`, `CarryInput`, `Carry`, `carry_idle_limit` (Task 1); `Config.builder_carry_session`, `builder_carry_max_ctx`, `cache_ttl` (plan 2); `JobView.builder_last` and the `builder_last` recording (plan 2); `builder_task_resume_spec` in `specs.rs` (the pattern to copy); `task_section(&plan, n)` from `tasks.rs`; `short_sha`; the ledger helper `ledger_task`. The `Tagged` runner of `tests/builder_last.rs` moves to `tests/common/mod.rs`, becomes `pub` with a `pub inner: ScriptedRunner` field and gets `pub fn new(inner: ScriptedRunner, end_ctx: u64) -> Tagged`; `tests/builder_last.rs` then uses it from there.
- Produces:
  - `fn builder_task_carry_spec(&self, cwd: &Path, task: &PlanTask, session: &str, base: &str, tip: &str) -> AgentSpec` in `specs.rs`, with `resume = Some(session)`, `agent`, `strict_mcp`, `tools`, `add_dirs`, `max_turns` 120, then `with_builder_permissions` and `with_limits`. Its prompt: `Task {prev} is done and committed as {sha7}. Base {base7}, current tip {tip7}. The driver now gives you Task {n} in this same session; this replaces the earlier line that later tasks belong to other agents. Do only Task {n}: {title}. Do not redo or rework earlier tasks.` followed by `--- your task ---` and the task section from `self.plan_file`. When the plan has no `### Task {n}:` section the last part is `Your task is in the plan at {plan path}.` The prompt never says the worktree is clean. `prev` is `task.n - 1`, `sha7` is `short_sha(tip)`.
  - In `build_task`: the decision is made right after `saved_session`. The ledger text after `BASE <base>, <model>, ` is `resumed` for a crash session, `continued` for a carry, and otherwise `fresh`, or `fresh (<reason>)` when `builder_carry_session` is on. A carry uses `builder_task_carry_spec`. A `Failed` stop of a carried run clears the task session and runs one fresh task builder with the ledger text `fresh (carry failed)`. The existing crash fallback keeps `fresh`.
  - The logic sits in the engine; the runs go through the `AgentRunner` port and board writes through `Engine::update`.

- [ ] **Step 1: Read** `build_task` and the fallback block in `build.rs`, `builder_task_resume_spec` and `builder_task_spec` in `specs.rs`, `tests/builder_last.rs` (plan 2), `tests/build_checks.rs` (`two_task_fixture`, `change_effect`, `the_ledger_says_resumed_or_fresh`, `the_resume_is_used_once`) and `tests/common/mod.rs`.
- [ ] **Step 2: Write the failing tests** in `tests/task_carry.rs`. Copy `two_task_fixture` and `change_effect`. One full example:

```rust
fn dispatched(fx: &Fixture) -> Vec<String> {
    fx.ledger().lines().filter(|l| l.contains(": dispatched")).map(str::to_string).collect()
}

#[test]
fn task_two_continues_the_session_when_gap_and_context_allow() {
    let mut fx = two_task_fixture();
    fx.config.builder_carry_session = true;
    let runner = Tagged::new(scripted_auto(vec![change_effect("a.txt", 3), change_effect("b.txt", 2)]), 30_000);
    let e = fx.engine(&runner);
    e.build_stage().expect("task 1");
    e.build_stage().expect("task 2");
    let builders = runner.inner.specs_for("builder");
    assert_eq!(builders[0].resume, None);
    assert_eq!(builders[1].resume.as_deref(), Some("sess-1"));
    assert!(builders[1].prompt.contains("Do only Task 2: second thing"), "{}", builders[1].prompt);
    assert!(dispatched(&fx)[1].ends_with(", continued"), "{:?}", dispatched(&fx));
}
```

  The other tests, each on `two_task_fixture()` with the setting on unless the row says otherwise:

| Test | Input | Assertion |
|---|---|---|
| `the_setting_off_starts_every_task_fresh` | setting off, two tasks | `builders[1].resume == None`; the dispatched line of task 2 ends with `, fresh` exactly |
| `a_stale_gap_starts_fresh_and_names_the_reason` | after task 1 set `builder_last.ended_at = 0` on the board (`Board::load`, `find_mut`, `save`) | task 2 fresh; dispatched line ends with `fresh (idle` and contains `over 240s` |
| `an_oversized_context_starts_fresh` | `Tagged::new(.., 150_000)` | fresh; line ends with `fresh (context 150000 over 100000)` |
| `a_rework_note_starts_fresh` | write `fx.job_dir.join("rework-note.md")` before task 2 | fresh; `fresh (rework note)`; the prompt holds the note |
| `task_one_is_never_carried` | the first call | `resume == None`; line ends with `fresh (first task)` |
| `the_whole_plan_builder_is_never_carried` | `committing_fixture()` (no task headings) with the setting on | the builder spec has `resume == None` |
| `the_carried_spec_has_permissions_agent_tools_and_turns` | the two specs from the first example | task 2 equals task 1 in `agent`, `tools`, `max_turns`, `strict_mcp`, `allowed_tools`, `disallowed_tools`, `add_dirs`, `model`; `resume` differs |
| `the_carried_prompt_names_the_new_task_and_overrides_later_tasks` | same run | the prompt contains `Task 1 is done and committed as`, `The driver now gives you Task 2 in this same session`, `Do not redo or rework earlier tasks`, and not `clean` |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine --test task_carry` and expect FAIL: `builders[1].resume` is `None` where `Some("sess-1")` is expected.
- [ ] **Step 4: Implement** `builder_task_carry_spec`, the decision and spec choice in `build_task`, the ledger text, and the widened fallback. Keep the existing crash-resume behaviour and text byte for byte.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine --test task_carry`, `cargo test -j4 -p factory-engine --test build_checks`, `cargo test -j4 -p factory-engine --test builder_last` and expect PASS; every existing test that pins `fresh` or `resumed` still passes unchanged.

### Task 3: interactions, a doc pin and the documentation

**Files:**
- Modify: `crates/factory-engine/tests/task_carry.rs`, `README.md`, `docs/software-factory.md`, `crates/factory-cli/tests/agent_lint.rs`
- Test: `crates/factory-engine/tests/task_carry.rs`, `crates/factory-cli/tests/agent_lint.rs`

**Interfaces:**
- Consumes: the carry from Task 2, `ScriptedRunner`, the crash helper `crashed_in_task_one` pattern in `build_checks.rs`, the tamper test pattern in `tests/driver_files_guard.rs`, the fix-round setup of `the_resume_prompt_names_check_hints_only_when_set`.
- Produces: tests for the interactions; a README section `Builder carry` and a paragraph in `docs/software-factory.md` after the builder excerpt paragraph that name `builder_carry_session`, `builder_carry_max_ctx`, the idle limit taken from `cache_ttl`, the ledger texts `continued` and `fresh (<reason>)`, that fix rounds are unchanged, and the `AVG-END` column; a doc pin in `agent_lint.rs`.

- [ ] **Step 1: Read** the tests named above and the README and software-factory paragraphs about `builder_excerpt` and the cache report.
- [ ] **Step 2: Write the failing tests.** One full example:

```rust
#[test]
fn a_failed_carry_gets_one_fresh_builder() {
    let mut fx = two_task_fixture();
    fx.config.builder_carry_session = true;
    let inner = scripted_auto(vec![change_effect("a.txt", 3), change_effect("b.txt", 2)]);
    let runner = FailResume { inner };   // returns AgentRun::failed("no conversation") when spec.resume is set
    let e = fx.engine(&runner);
    e.build_stage().expect("task 1");
    e.build_stage().expect("task 2");
    let builders = runner.inner.specs_for("builder");
    assert_eq!(builders.len(), 3, "one fresh, one failed carry, one fresh again");
    assert!(builders[1].resume.is_some() && builders[2].resume.is_none());
    assert!(fx.ledger().contains("fresh (carry failed)"), "{}", fx.ledger());
}
```

  The other tests:

| Test | Input | Assertion |
|---|---|---|
| `a_crash_session_beats_a_carry` | seed `task_session` for task 2 at the current tip and a good `builder_last` | the builder resumes the saved id with the interrupted prompt; the line ends with `, resumed` |
| `a_tampering_carried_builder_parks_without_a_second_run` | the carried effect appends to `fx.job_dir.join("plan.md")` | `NeedsHuman` whose reason contains `builder changed plan.md`; no further builder run |
| `a_fix_round_after_a_chain_resumes_the_last_session` | two tasks with carry, then `changes-r2.md`, round 2, stage Build, `build_stage` | the last builder spec has `resume == Some("sess-2")` and the fix-round prompt |
| `the_batch_snapshot_and_reset_work_with_a_carry` | two tasks with carry and `commit_batch` on | stage Review, `batch_snapshot == None`, `head` set |
| `a_plan_without_a_task_section_points_at_the_plan_file` | a plan with `- [ ] Task 1: a` and `- [ ] Task 2: b` lines only | the carried prompt contains `Your task is in the plan at` |
| `builder_last_survives_a_carried_advance` | after a carried task 2 | `builder_last.task == 2` and `id == "sess-2"` |

  In `agent_lint.rs`: `docs_describe_the_builder_carry` reads `README.md` and `docs/software-factory.md` and requires `builder_carry_session`, `builder_carry_max_ctx`, `continued` and `cache_ttl` in both.
- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine --test task_carry` and `cargo test -j4 -p factory-cli --test agent_lint docs_describe_the_builder_carry` and expect the doc pin to FAIL (`README is missing`); the engine tests added here should already PASS or show the failures to fix in the carry code.
- [ ] **Step 4: Fix** any carry bug the new engine tests show, and write the documentation described under Produces.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli --test agent_lint` and expect PASS. Then `cargo clippy -j4 -p factory-agent -p factory-core -p factory-engine -p factory-cli --all-targets -- -D warnings` and `cargo fmt -p factory-agent -p factory-core -p factory-engine -p factory-cli -- --check`, both clean.

Next plan: none
