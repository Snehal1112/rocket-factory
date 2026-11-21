# Cancel the rest of a series, or replace one plan

## Goal

When one plan of a series fails, the user cancels the rest of the series in one step, or puts a fixed plan in its place and keeps the plans after it.

## Overview

Item 2 of the efficiency review (all 88 rocket-factory jobs): 11 of 13 cancels were series jobs. The base problem (087) is fixed by the series guard (`2026-11-19-final-gate-skip-and-series-guard-design.md`). What is left:

- 030 (plan 1 of 5) blocked because its plan asked for code that did not exist. The user cancelled 030, then 031, 032, 033 and 034 one by one. Those four never ran a builder; their folders hold only `plan.md`, `progress.md` and `spec.md`.
- 066 (plan 1 of 4) parked "builder made no changes on resume" after 5 rounds. The user cancelled 066, then 067, 068 and 069 one by one.

No tokens were lost on the siblings. The losses were the user's time on one cancel per plan, and the planned work in plans 2 to N, because there is no way to fix plan 1 and keep the rest.

## Current state

| Piece | Today |
|---|---|
| `factory cancel <id>` | `crates/factory-cli/src/commands.rs:910`: cancels one job only (stage `needs-human`, `cancelled = true`, ledger ruling). Refuses a merged job and a live lease without `--force`. |
| Supervisor cancel | `crates/factory-supervisor/src/api/write.rs:500-527`: `CancelBody { reason, force }` becomes `factory cancel --reason=<r> [--force] -- <id>`. |
| Dashboard | `dashboard/static/projects.js:96` builds the cancel body; a series-waiting job offers `retry` and `cancel` (line 121). |
| Series guard | `crates/factory-engine/src/series_guard.rs:44-72`: plan N parks `series-waiting` while plan N-1 is not merged. It finds plan N-1 with the first board job of the same series id and index N-1 (line 51). A cancelled plan N-1 parks plan N with "cancel this plan too, or plan the work again" (line 64). |
| Series run | `crates/factory-engine/src/runloop.rs:769`: after plan N merges, `run_series` reads `Next plan:` of plan N's `plan_file`, finds the job with that `plan_file` and index N+1 (line 789) and continues it if it waits (`continue_waiting`, line 829). |
| Job creation | `create_job_with_spec_accepting` (`runloop.rs:511`) makes one job per plan of the chain, copies each plan into its job folder as `plan.md` and gives a chain of two or more plans a new series id. |
| `factory cleanup <id>` | `commands.rs:958`: one job at a time. A cancelled card stays on the board until every member of its series is merged or cancelled (`series_is_finished`, line 1057). A branch with commits not in the base needs `--discard-commits` (line 1134). |

Gaps: no way to cancel the later plans together, no way to swap in a fixed plan, and the guard's "first job at index N-1" lookup would pick a cancelled job over its replacement.

## Non-goals

- Catching a plan that asks for code that does not exist (030) before the run. That is the spec reviewer's job.
- Changing the later plans when a replacement changes what plan N builds. The user decides whether they still fit.
- A `/factory` command shape for replacing a plan. The user runs `factory new --replaces` by hand for now (Follow-ups).

## Design

```
plan N fails (parked or blocked)
 ├─ give up:  factory cancel <N>          → N and every later unmerged plan cancelled
 │            factory cleanup <N>          → every cancelled member cleaned, cards removed
 └─ fix plan: edit plan file, commit
              factory new <slug> '<feature>' --plans <fixed plan file> --replaces <N>
                                           → old N cancelled (only N), new job takes index N
              factory run <new> --series   → new N merges, N+1 continues, and so on
```

### 1. Cancel cascades to later plans

- `factory cancel <id>` on a job whose series has an id cancels the target and every member with a higher index that is neither merged nor cancelled. Lower indexes are never touched.
- New flag `--only`: cancel just the target, as today.
- An already cancelled target cascades the same way, so an old cancel can still clear its later plans. A target that was replaced (another member that is not cancelled holds its series id and index) is refused: `job <old> was replaced by job <new>, cancel that one`, so its later plans, which now follow the replacement, are never cancelled through it.
- The live-lease rule covers every job to be cancelled. If any of them holds a live lease and `--force` is not given, nothing is cancelled and the refusal names that job. With `--force`, the project lease is released for whichever cancelled job holds it, the target or a later plan, and the output says `project lease released` as today.
- Each later job gets reason `cancelled: plan <n> (job <id>) was cancelled` and its own ledger ruling and `operator.cancel` event, emitted inside that job's `job_span`, with an extra field `cascade_from = <target id>`. The target keeps the user's reason.
- Output: `job 030 cancelled`, then one line per later job, `job 031 cancelled (later plan of S7)`.
- A job without a series id (single plan, legacy series) behaves exactly as today.
- Supervisor: `CancelBody` gets an optional `only: bool`; `true` adds `--only`. The dashboard sends no `only`, so its cancel button cascades. Its cancel confirm text says later plans of the series are cancelled too when the job is in a series.

Example: S7 holds 030 (parked), 031-034 (stage spec). `factory cancel 030 --reason='plan asked for missing code'` cancels all five.

### 2. Cleanup of a whole series

- `factory cleanup <id>` of a cancelled series member, once `series_is_finished` holds, also cleans every other cancelled, not yet cleaned member of that series, in index order. Merged members are left alone.
- `--discard-commits` applies to each of them. A member whose cleanup is refused is reported on its own line and keeps its card; the others go on.
- Before the series is finished it cleans only the target, as today.

### 3. Replace one plan

- New flag on `factory new`: `--replaces <id>`. It needs `--plans`, which must name one plan file, not a folder. It conflicts with `--one-job`.
- The target must have a series id, must not be merged, and must have no live lease. It may be parked, cancelled, or not started (stage `spec`).
- The fixed plan's `Next plan:` line must resolve to the same path as the `Next plan:` line of the target's plan file (`series.plan_file`), or be `none` when the target's is `none`. Otherwise `new` refuses and names the expected line. The fixed plan may live at any path; the usual case is the original plan file edited in place and committed.
- Only that one plan is checked (task count, `Commit:` line, placeholders, the plan lint of `new`); the chain behind it is not walked again and no job is made for it.
- The new job gets the target's `SeriesPos` with `plan_file` set to the fixed plan's path: same series id, index and total. It is made with `add_job` at stage `build` like the first plan of a new series (`runloop.rs` copies the plan into the job folder as `plan.md` there); the worktree and branch are cut at build start as for any job. Nothing of the target's branch carries over. Slug and feature come from the arguments; `--model` defaults to the target's model.
- Spec: with `--spec`, the spec check runs on that file with the fixed plan, as for any `new`. Without `--spec`, the same check runs on the target's `spec.md` (in its job folder) with the fixed plan, so the risky-path rule and the spec reviewer still apply; `--accept-spec` works as for any `new`. A target without `spec.md` and no `--spec` is refused: `job <id> has no spec.md, pass --spec`. The checked spec text is copied into the new job's folder.
- The target, if not already cancelled, is cancelled with `--only` semantics and reason `cancelled: replaced by job <new id>`. The new job's ledger gets `replaces job <old id>`. Output: `created job 091 in <folder>`, then `job 030 replaced by job 091`.
- The new job then runs like any series plan: the guard parks it while plan N-1 is unmerged; after it merges, `run_series` finds plan N+1 by series id and index (section 5) and continues it.

### 4. Rework of a replaced job

- `factory rework <id>` (also with `--force`) on a job that was replaced is refused with `job <old> was replaced by job <new>, rework that one`, using `Board::replacement_of`. Today a rework revives a cancelled job (`commands.rs:863-864`, `:889-890`); that stays for a cancelled job that was not replaced, including a later plan cancelled by a cascade. So no series place ever holds two live jobs.

### 5. Series lookups skip cancelled jobs

- `wait_for_earlier_plan` picks plan N-1 among members that are not cancelled first, and falls back to a cancelled one only when no other job has that index. So a replaced plan N-1 does not park plan N for good, and an unreplaced cancel still parks it.
- The cancelled-predecessor reason becomes `plan <n> (job <id>) was cancelled; cancel this plan, or replace plan <n> with factory new --replaces <id>`.
- When the merged job has a series id, the `run_series` lookup of plan N+1 (`runloop.rs:789`) matches by series id and index N+1 and skips cancelled jobs, instead of by `plan_file`. So plan N-1 finds a replacement at a new path, and a replaced plan N finds the old plan N+1. Without a series id (legacy) the `plan_file` match stays. When no job at all holds index N+1 (for example a card removed by hand), the job is created as today (`runloop.rs:794-821`), with the series id and index N+1. When the only job at N+1 is cancelled, the series ends there with `RunEnd::Merged`; no new job is made for it.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| Cancel the last plan of a series | Only that plan; nothing later. |
| Cancel a middle plan | That plan and later ones; earlier merged plans stay merged. |
| A later plan is already cancelled | Skipped, not listed. |
| A later plan holds a live lease, no `--force` | Nothing cancelled; refusal names the job. |
| `--only` on a job without a series | Same as plain cancel. |
| `--replaces` with a plans folder | Refused: needs one plan file. |
| `--replaces` a merged job | Refused: `job <id> is already merged`. |
| `--replaces` a job without a series id | Refused: `job <id> is not in a series`. |
| `--replaces` a job with a live lease | Refused, as cancel does. |
| `--replaces` with a wrong `Next plan:` line | Refused, naming the expected line. |
| `--replaces` without `--spec`, target has no `spec.md` | Refused: `job <id> has no spec.md, pass --spec`. |
| `--replaces` without `--spec`, fixed plan touches a risk path the old spec has no user decision for | Refused by the spec check, as for any `new`; nothing changes. |
| Forced cascade where a later plan holds the project lease | The lease is released for that job. |
| `--replaces` a job that was already replaced | Refused: `job <old> was replaced by job <new>, replace that one`. A job counts as replaced when another member that is not cancelled holds its series id and index. |
| Replace a plan whose later plans were cancelled by a cascade | Allowed; the later plans stay cancelled. Their reason says so; the user plans them again. |
| Cleanup of a member while the series is not finished | Target only, as today. A replaced job keeps its card until the series is finished, like any cancelled member. |
| Cancel a target that was replaced | Refused, naming the replacement. |
| Cancel an already cancelled target | Cascades to later unmerged plans. |
| Rework a replaced job | Refused, naming the replacement. |
| Rework a later plan cancelled by a cascade | Revived as today; the plans after it stay cancelled. |
| No job at index N+1 after plan N merges | Created as today, with the series id and index N+1. |

## Testing

`factory-cli` (`crates/factory-cli/tests/`, reusing the board and repo fixtures of the existing cancel and cleanup tests):

- `cancel_cascades_to_later_unmerged_plans`: S with 3 plans, cancel plan 1; all three cancelled, reasons of 2 and 3 name job 1.
- `cancel_only_leaves_later_plans`: `--only` cancels plan 1 alone.
- `cancel_middle_plan_keeps_earlier_merged`: plan 1 merged, cancel plan 2; plan 1 merged, plans 2-3 cancelled.
- `cancel_refuses_when_a_later_plan_holds_a_live_lease`: nothing changes, refusal names the job.
- `cancel_without_series_is_unchanged`.
- `cancel_of_a_replaced_job_is_refused`, `cancel_of_a_cancelled_job_cascades`.
- `rework_of_a_replaced_job_is_refused`, `rework_revives_a_cancelled_job_that_was_not_replaced` (`crates/factory-cli/tests/retry_cmd.rs`).
- `cleanup_of_finished_series_cleans_every_cancelled_member`, `cleanup_reports_a_refused_member_and_goes_on`.
- `new_replaces_takes_the_index_and_cancels_the_target`, `new_replaces_checks_the_target_spec_without_spec_flag`, `new_replaces_without_spec_applies_the_risky_path_rule`, `forced_cascade_releases_a_project_lease_held_by_a_later_plan`, `new_replaces_refuses_folder_merged_no_series_wrong_next_line` (one test per refusal row).

`factory-engine`:

- `series_guard_prefers_the_replacement_of_a_cancelled_plan`, `series_guard_parks_on_an_unreplaced_cancel_with_the_new_reason`.
- `run_series_continues_the_next_plan_after_a_replacement_merges` (fake runner, replacement at a new path).
- `run_series_finds_a_replacement_at_a_new_path_after_the_plan_before_merges`.
- `run_series_stops_when_the_next_plan_is_cancelled`.

`factory-supervisor`: `cancel_body_only_adds_the_flag`. Dashboard: confirm text test where the dashboard tests cover `projects.js`.

Commands: `cargo test -j4 -p factory-cli`, `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-supervisor`, `cargo test -j4 --manifest-path dashboard/Cargo.toml`.

## Files changed

- `crates/factory-cli/src/cli.rs`, `crates/factory-cli/src/commands.rs` (cancel, cleanup, new).
- `crates/factory-engine/src/runloop.rs` (replacement job creation, next-plan lookup), `crates/factory-engine/src/series_guard.rs`.
- `crates/factory-supervisor/src/api/write.rs`, `dashboard/static/projects.js`.
- `README.md`, `docs/reference/software-factory.md` (cancel, cleanup, replace).

## Constraints

- New board fields, if any, are optional, so `dashboard/` still reads the board.
- Git only through `factory_git::Repo`; paths only through `Layout`.
- A cascade writes the target and its later jobs in one `store().update(|board| ...)`, with the live-lease check inside the closure as `remove_cancelled_card` does (`commands.rs` ~1000), so a crash leaves all of them cancelled or none. The ledger rulings and events follow the board write.

## Acceptance criteria

- `factory cancel <id>` on a series plan cancels it and every later unmerged plan, and lists them.
- `factory cancel <id> --only` cancels just that job.
- A live lease on any job of the cascade refuses the whole cancel without `--force`.
- Dashboard and supervisor cancel cascade by default; the API accepts `only`.
- `factory cleanup <id>` on a finished series cleans every cancelled member.
- `factory new --replaces <id> --plans <file>` makes a job at the target's series index and cancels only the target.
- Without `--spec`, `--replaces` runs the spec check on the target's `spec.md` with the fixed plan, and refuses a target without `spec.md`.
- `--replaces` refuses a folder, a merged target, a target without a series id, a target already replaced, a live lease and a wrong `Next plan:` line.
- After the replacement merges, the next plan of the series continues, also when the replacement plan file has a new path.
- Cancelling a replaced job is refused; cancelling an already cancelled job cascades.
- Reworking a replaced job is refused and names the replacement.
- The series guard waits on the replacement, not on the cancelled job.
- README and `software-factory.md` describe the cascade, `--only`, series cleanup and `--replaces`.

## Out of scope

- Re-planning later plans automatically after a replacement.
- Replacing several plans in one call.

## Follow-ups

- A `/factory replace <id>` shape in `factory-core/assets/global-command.md` that drafts the fixed plan and calls `factory new --replaces`.

## Decisions

- What should happen to later plans when a plan dies? Options: cancel the rest in one step (A), put a fixed plan in its place (B), re-plan the chain from plan N (C). Chosen: A and B together. Decided by: user
- Cascade by default or on request? Options: cascade by default with `--only` to opt out, one job by default with `--series` to opt in, cascade in the CLI but one job in the API. Chosen: cascade by default with `--only`, in the CLI and the API. Decided by: user
- Replacement interface? Options: `factory new --replaces <id>`, a new `factory replace` command. Chosen: `factory new --replaces <id>`, as sketched to the user. Decided by: call
- Spec check for a replacement without `--spec`? Options: run the check on the target's spec.md with the fixed plan, always require `--spec`, skip the check. Chosen: run the check on the target's spec.md. Decided by: user
- Project lease on a forced cascade? Options: release it for any cancelled job that holds it, only for the target. Chosen: any cancelled job. Decided by: call
- Next-plan lookup? Options: series id and index, plan file path. Chosen: series id and index for series with an id. Decided by: call
- Cancel of a replaced job? Options: refuse, cancel only that job. Chosen: refuse. Decided by: call
- Dashboard cancel? Options: follow the CLI default with a dialog title that names the later plans, add an "also cancel the later plans" checkbox, always one job. Chosen: follow the CLI default with the dialog title. Decided by: user
- Rework of a replaced job? Options: refuse and name the replacement, revive the old job and cancel its replacement. Chosen: refuse and name the replacement. Decided by: user
- No job at index N+1 of a series with an id? Options: create it as today, end the series. Chosen: create it as today. Decided by: call
