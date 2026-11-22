# Drop the per-task checks: Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-22-drop-per-task-checks-design.md
> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** A task plan no longer runs the mapped checks after each task; only the whole-branch checks stage runs them.

**Commit:** perf(engine): drop the per-task checks

**Architecture:** Delete `note_task_checks` and its call in `build.rs`, and `run_checks_between` in `checks_stage.rs`, its only caller gone. The three tests that use the per-task path to test the check planner move to the checks stage, which writes `checks.md` with the same planner notes (`run_checks_now` in `checks_stage.rs`) before any gate can park. No port, board, ledger format or event changes.

**Tech Stack:** Rust; crate `factory-engine` only.

## File Structure

| File | Change |
|---|---|
| `crates/factory-engine/tests/build_checks.rs` | Three planner tests read `checks.md` after `checks_stage()`; four per-task tests deleted; one new test that a task plan runs no checks between tasks. |
| `crates/factory-engine/src/build.rs` | Remove `note_task_checks` (607-619) and its call (821). |
| `crates/factory-engine/src/checks_stage.rs` | Remove `run_checks_between` (221-249) and any import it alone used. |
| `docs/reference/software-factory.md` | One line under "Per-task builders" (375-385). |

## Review Focus

1. A capped-away or unplanned check must still leave its note in `checks.md` even when no command runs, so a skipped check is not silent. Pinned by the moved `the_checks_stage_keeps_the_note_of_a_capped_away_dependents_check`.
2. The whole-branch checks still run after the last task of a task plan and still write `checks.md` exactly once. Pinned by `a_task_plan_runs_no_checks_between_tasks`.
3. Session carry between tasks (`builder_carry_session`) must keep working with nothing between the snapshot and the next task. Pinned by the existing `crates/factory-engine/tests/task_carry.rs`, which must pass unchanged.
4. No leftover warning from a now-unused import or helper. Pinned by `cargo check -j4 -p factory-engine` with no warnings.

---

### Task 1: Planner tests run through the checks stage

**Files:**
- Test: `crates/factory-engine/tests/build_checks.rs:1757-1848`

**Interfaces:**
- Consumes: `Fixture::new()`, `scripted(Vec<Effect>)`, `Engine::build_stage()`, `Engine::checks_stage()`, the `rule(path, command)` helper (`build_checks.rs:1688`), all existing.
- Produces: nothing new for the next task. It only stops these tests from reading `checks-task1.md`.
- Port: none. Tests only; the engine code under test is plain `Engine` methods behind no trait (`AgentRunner` and `Tracker` are not involved).
- Tests added: the three renamed tests below.

- [ ] **Step 1: Read** `crates/factory-engine/tests/build_checks.rs:300-400` (the checks-stage tests and `two_task_fixture`) and `:1688-1858`, and `crates/factory-engine/src/checks_stage.rs:85-130` and `:338-378`.

- [ ] **Step 2: Rewrite the three planner tests**

Rename and change each so it builds with `Fixture::new()` (the whole-plan builder) instead of `two_task_fixture()`, then calls `fx.engine(&runner).checks_stage().expect("checks")` and reads `fx.job_dir.join("checks.md")`. Keep the config, the workspace-writing effect and every assertion on the file text as they are today. Do not assert the stage outcome: a gate may park after `checks.md` is written, and that is not what these tests pin.

| Old name | New name | Assertions kept |
|---|---|---|
| `a_capped_away_dependents_check_still_leaves_its_note` | `the_checks_stage_keeps_the_note_of_a_capped_away_dependents_check` | `md.contains("SKIP dependents over the cap: app") && !md.contains("FAIL")` |
| `a_crate_template_only_in_lint_expands_in_a_real_run` | `the_checks_stage_expands_a_crate_template_only_in_lint` | `md.contains("cargo clippy -j4 -p corepkg")`, no `-p core\n`, no `-p core ` |
| `a_broken_workspace_map_still_runs_the_checks` | `the_checks_stage_runs_the_checks_with_a_broken_workspace_map` | `NOTE workspace map unavailable`, `PASS true`, no `cargo test -j4 -p` |

- [ ] **Step 3: Run them**

Run: `cargo test -j4 -p factory-engine --test build_checks the_checks_stage_`
Expected: 3 passed.


### Task 2: No checks between tasks

**Files:**
- Modify: `crates/factory-engine/src/build.rs:607-619`, `:821`
- Modify: `crates/factory-engine/src/checks_stage.rs:221-249`
- Modify: `docs/reference/software-factory.md:375-385`
- Test: `crates/factory-engine/tests/build_checks.rs:1696-1756`, `:1850-1858`

**Interfaces:**
- Consumes: the three moved planner tests stay green.
- Produces: no new item. `Engine::run_checks_between` and `Engine::note_task_checks` no longer exist.
- Port: none. The removed code is plain `Engine` methods in `build.rs` and `checks_stage.rs`, behind no trait.
- Tests added: `a_task_plan_runs_no_checks_between_tasks`.

- [ ] **Step 1: Write the failing test** in `build_checks.rs`, next to `two_task_fixture`:

```rust
#[test]
fn a_task_plan_runs_no_checks_between_tasks() {
    let mut fx = two_task_fixture();
    fx.config.checks = vec![rule("a.txt", "echo task-a"), rule("b.txt", "echo task-b")];
    let runner = scripted(vec![change_effect("a.txt", 3), change_effect("b.txt", 2)]);
    let e = fx.engine(&runner);
    e.build_stage().expect("task 1");
    e.build_stage().expect("task 2");
    assert!(!fx.job_dir.join("checks-task1.md").exists());
    assert!(!fx.job_dir.join("checks-task2.md").exists());
    let ledger = fx.ledger();
    assert!(!ledger.contains("task 1 checks") && !ledger.contains("task 2 checks"), "{ledger}");
    e.checks_stage().expect("checks");
    let md = std::fs::read_to_string(fx.job_dir.join("checks.md")).expect("checks.md");
    assert!(md.contains("PASS echo task-a") && md.contains("PASS echo task-b"), "{md}");
}
```

If `rule` is defined below `two_task_fixture`, place the test after `rule`.

- [ ] **Step 2: Run it**

Run: `cargo test -j4 -p factory-engine --test build_checks a_task_plan_runs_no_checks_between_tasks`
Expected: FAIL on `checks-task1.md` existing.

- [ ] **Step 3: Remove the per-task checks**

Delete the `self.note_task_checks(task.n, &tip, &new_tip)?;` line at `build.rs:821`, the `note_task_checks` function with its doc comment, and `run_checks_between` with its doc comment in `checks_stage.rs`. Remove each import, helper or `use` that only these used, until `cargo check -j4 -p factory-engine` shows no warning. Keep the `save_commit_note` call and the `if changed` block around it.

- [ ] **Step 4: Delete the four per-task tests**

`each_task_writes_a_checks_file_for_its_own_range`, `a_failing_task_check_is_a_ledger_line_not_a_park`, `a_failing_check_after_task_two_is_reported_against_task_two`, `a_task_with_no_mapped_checks_writes_nothing`. Keep `rule` and `two_task_fixture`; they are still used.

- [ ] **Step 5: Docs**

Add one bullet at the end of the "Per-task builders" list in `docs/reference/software-factory.md`, exactly:

```markdown
- No checks run between tasks. The checks stage runs the mapped checks once over the whole branch after the last task and writes `checks.md`. Job folders from before this change may still hold `checks-task{n}.md` files and `task N checks` ledger lines; nothing reads them.
```

- [ ] **Step 6: Run the crate**

Run: `cargo check -j4 -p factory-engine` then `cargo test -j4 -p factory-engine`
Expected: no warnings, all tests pass, including `task_carry`.

Next plan: none
