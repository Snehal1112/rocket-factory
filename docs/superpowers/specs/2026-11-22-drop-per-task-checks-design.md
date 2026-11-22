# Drop the per-task checks

**Commit:** perf(engine): drop the per-task checks

## Goal

A task plan no longer runs the mapped checks after each task. Only the whole-branch checks stage runs them, so jobs spend less wall time in checks and the result is the same.

## Overview

Item 5 of the 2026-11-19 efficiency adviser brief. Since 61047ec (2026-10-18) the driver runs the mapped checks over each changed task's diff, writes `checks-task{n}.md` in the job folder and adds the ledger line `task N checks: pass`, `fail <command>` or `not run, <err>`. The 10-18 design made them informational "until the user says otherwise". A failure never parks a job or starts a fix round.

Nothing reads the result. `builder_notes` (`review.rs:42-49`) keeps only `builder task` rulings, so the reviewer never sees these lines. The dashboard, the docs and the agents do not mention them. Over all 88 jobs the per-task runs were 43-54% of check time, failed in 12 of 199 runs, and only one of those jobs (066) then failed its final checks, so they caught almost nothing the checks stage would miss.

## Current state

| Path | What it does today |
|---|---|
| `crates/factory-engine/src/build.rs:607-619` | `note_task_checks(n, from, to)`: calls `run_checks_between` with `checks-task{n}.md` and writes the ledger ruling. |
| `crates/factory-engine/src/build.rs:821` | Called after `save_commit_note` for every task that changed something. |
| `crates/factory-engine/src/checks_stage.rs:221-249` | `run_checks_between`: plans checks for a diff range, runs them and writes the named file. Its only caller is `note_task_checks`. |
| `crates/factory-engine/tests/build_checks.rs:1688-1858` | Seven tests on the per-task path. Four test the per-task feature itself (`each_task_writes_a_checks_file_for_its_own_range`, `a_failing_task_check_is_a_ledger_line_not_a_park`, `a_failing_check_after_task_two_is_reported_against_task_two`, `a_task_with_no_mapped_checks_writes_nothing`). Three use the per-task path to test the check planner in a real run (`a_capped_away_dependents_check_still_leaves_its_note`, `a_crate_template_only_in_lint_expands_in_a_real_run`, `a_broken_workspace_map_still_runs_the_checks`). |

## Approaches

- **A. Remove them.** Delete `note_task_checks`, its call, `run_checks_between` and the per-task tests. Saves the full 43-54% with no new setting. Chosen by the user.
- **B. A `task_checks` key, off by default.** Same saving, but keeps code that nobody turns on and adds a setting to document and test. Not chosen.
- **C. A key, on by default.** Saves nothing. Dropped.

## Non-goals

- No change to the whole-branch checks stage, `checks.md`, the final gate, fix rounds or the check reuse in `reusable_checks`.
- No change to the check planner (`factory-checks`).
- No change to the builder session carry, which ran these checks between tasks. It keeps working; there is just nothing between the snapshot and the next task.
- Old job folders keep their `checks-task{n}.md` files and ledger lines. Nothing reads them, so no cleanup.

## Design

- Remove the `note_task_checks` call at `build.rs:821`, the function itself and `run_checks_between` in `checks_stage.rs`. Remove any import or helper that becomes unused (`cargo check -j4` with no warnings).
- Delete the four tests of the per-task feature.
- Keep the coverage of the three planner tests: move each to the whole-branch checks stage, so it reads `checks.md` instead of `checks-task1.md` and asserts the same note, command or fallback text. If an equal test already exists for the checks stage, delete the per-task one instead and name the existing test in the commit note.
- Add one test: a two-task plan with a mapped check that writes no `checks-task1.md` or `checks-task2.md`, no `task N checks` ledger line, and still writes `checks.md` once from the checks stage.
- Docs: `docs/reference/software-factory.md` does not mention the per-task checks today. Add one line under "Per-task builders": checks run only in the checks stage, over the whole branch.

## Decisions

- Remove instead of a setting. Options: remove; key off by default; key on by default. Chosen: remove. Decided by: user
- The three planner tests move to the checks stage instead of being dropped. Decided by: call
- No cleanup of old `checks-task{n}.md` files. Decided by: call

## Acceptance criteria

- A task plan writes no `checks-task{n}.md` and no `task N checks` ledger line.
- The whole-branch checks stage still writes `checks.md` and drives fix rounds as before.
- The capped-away note, the crate template expansion and the broken workspace map fallback are still covered by tests through the checks stage.
- `cargo check -j4` and `cargo test -j4 -p factory-engine` pass with no new warnings.
