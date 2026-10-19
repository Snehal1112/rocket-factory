# One job per spec (opt-in) design

## Goal

Let a user run all the plans of one spec as a single factory job by passing `--one-job` to `factory new`, or the same token to `/factory`, so the per-plan committer, reviewer, merge message, worktree and approval overhead is paid once. Without the flag nothing changes.

## Acceptance criteria

- `factory new <slug> '<feature>' --plans <folder> --spec <file> --one-job` makes exactly one job with no series from a plan folder of several plans.
- The one job's `plan.md` holds every task of every plan, renumbered 1 to N, with one header, merged Global Constraints and Review Focus, and no `Next plan:` line.
- Without `--one-job` the same plans still make one job per plan, and a plan of more than 3 tasks is still refused on that path.
- When the estimate `110000 + 100000 * tasks` passes `budget_weighted`, `factory new --one-job` creates nothing and prints a `needs consent:` line, and `--accept-over-budget` creates the job with a stored warning.
- `--one-job` needs `--plans`, and `--accept-over-budget` needs `--one-job`.
- A single plan with `--one-job` goes through the normal path and the output says the flag had no effect.
- The global `/factory` command accepts `--one-job`, passes it to `new`, and asks the user on `needs consent:` before re-running with `--accept-over-budget`.

## Decisions

- The capability is additive and on demand, chosen with a flag, and the default is unchanged. Decided by: user
- The flag lives on `factory new` and the `/factory` command supports it. Decided by: user
- Over the budget the run warns and goes ahead only after the user gives consent. Decided by: user
- Plans keep at most 3 tasks each and the driver combines them. Decided by: call
- The flag is named `--one-job` and the consent flag `--accept-over-budget`. Decided by: call
- The estimate constants, the combined header rules and the single plan behaviour are as described below. Decided by: call

## Intent

Today `factory new --plans <folder> --spec <file>` makes one job per plan. A spec of 4 plans with 3 tasks each becomes 4 jobs. Every job pays for its own committer, reviewer and merge message agent, its own worktree (a cold cargo build) and its own human approval when a diff is risky.

The user wants an opt-in way to run all the plans of one spec as a single job, chosen with a flag, while everything else stays exactly as it is.

## Why (measured, 2026-10-19)

- Real series: `rocket` jobs 021 to 024 (4 plans, 12 tasks, 2.11M weighted tokens). One job would drop 3 committers, 3 reviewers and 3 merge message agents, which saves about 345K to 363K weighted (16 to 17 percent). The other 4-plan series would save 24 percent, a 2-plan series 13 percent. These are upper bounds, because one reviewer reads a bigger diff.
- Builders do not shrink. Every task still gets one fresh builder, so the 12 builder cold starts stay. A single resumed builder session across tasks was modelled and rejected: it costs 1.83M against 1.01M weighted over 12 tasks.
- Fewer worktrees means fewer cold cargo builds, and a risky diff stops once for one approval instead of up to four.

## Goals

- A new optional flag on `factory new` that creates one job for the whole spec.
- The `/factory` command can use it: the user types `--one-job` with the feature and the command passes it on.
- No change when the flag is absent. Series behaviour, the 3 tasks per plan rule, the 6 plan chain cap, board JSON and every existing test stay as they are.
- The plan files on disk stay the source and are never edited by the driver.

## Non-goals

- No change to the default. The one job path is never chosen automatically.
- No per-project setting in this first version. The flag is chosen per feature, never as a project default.
- No change to the builder model (one fresh builder per task) and no builder session reuse.
- No partial merge of the safe plans when one plan is risky. A risky path anywhere stops the whole job.
- No dashboard change. A series-less job already shows as a normal job card.

## Anti-goals

Failure even if it technically works:

- A job that silently drops tasks because two plans reused a task number.
- A one job run that goes over budget without the user having said yes to it.
- Any change that alters how an existing series job is created, run or shown.

## Decided by the user

- The capability is additive and on demand, chosen by a flag. Without the flag nothing changes.
- The flag lives on `factory new`.
- The `/factory` command supports it in this first version, so it works from a normal `/factory` run.
- Over the budget estimate, the user is asked for consent. The run goes ahead only after a yes.

## Design (the parts below are my calls unless marked)

### Flag

`factory new <slug> '<feature>' --plans <folder|file> --spec <file> --one-job`. The name `--one-job` is my call. It requires `--plans` (a clap `requires`, like `--spec`). With a single plan the flag changes nothing: the job is made by the normal path, so a single plan of more than 3 tasks is still refused, and the output says the flag had no effect (my call).

### Entry point

`create_job_with_spec` in `crates/factory-engine/src/runloop.rs` gets a new sibling for the one job path. The existing function is not edited except to dispatch, or the new function is called from `new_job` in `crates/factory-cli/src/commands.rs`. Either way the series code path is untouched.

The one job path:

1. Validates the chain without the 3 task cap. `parse_series` stays as is and calls a new `parse_series_with(root, start, max_tasks)` with 3. The one job path passes no upper bound. The 6 plan cap still applies.
2. Lints every source plan and checks the spec exactly as today, so a placeholder in any plan still refuses the call. Only the source plans are linted. The combined file is not linted again, and no lint rule enforces the 3 task cap (only `series.rs` does, verified).
3. Builds one combined plan text (below) and creates one job at stage `build` with `series: None`.
4. Writes the combined text to the job's `plan.md` itself, because `add_job` copies a plan file only when `job.series` is set.
5. Copies the spec into the job folder, adopts the draft and prints warnings the same way as today.

### Combined plan text

A pure function from the ordered plan texts to one text, with no I/O so it can be unit tested:

- Header, my call. Real plans repeat boilerplate in the header (Goal, Architecture, Tech Stack, Spec, Global Constraints, for example the `-j4` rule), but a later plan also adds its own constraints, for example "use the `Config` fields from plan 1, do not redefine them". Dropping later headers would lose those. So the combined header keeps the first plan's header, adds one `Plan N goal:` line per later plan, and merges the `## Global Constraints` bullets of all plans in order with exact duplicate bullets removed. Constraints that name another plan by number ("from plan 1") are kept and the command prints a warning, like the task body mentions below.
- Tasks: every plan's task sections in order, renumbered 1 to N. Only the `### Task N:` heading, or the `- [ ] Task N:` line for the older checkbox form, is rewritten. A repeated number would otherwise be silently dropped by `collect_tasks` (`crates/factory-engine/src/tasks.rs:14`).
- Review Focus: the sections of all plans are merged into one under a single heading, my call.
- The `Next plan:` line of every plan is removed. A combined file must not end in one, because `task_section` stops at it (`tasks.rs:156`).
- The first line of the combined file after the title is a comment listing the source plan paths in order, my call. That is the only trace of where the job came from, so no board field is added. The ledger header already names the job's `plan.md`.
- Body text is not rewritten. If a body mentions another task by number, the combined plan keeps the old number and the command prints a warning, my call.

### Budget guard

The job has one budget for all its tasks and its committer, reviewer and merge message runs. The limit that is enforced is the project's `config.budget_weighted` (`specs.rs:76`, `park.rs:79`, `review.rs:87`). The per-job `budget_weighted` field on the board is only stamped at creation and is not read by the run, so a per-job override would need new engine plumbing and is out of scope.

The command estimates `110000 + 100000 * tasks` weighted tokens (from the measured 107K per job overhead and 76K to 94K per task, rounded up, my call; the constants are the builder's to tune) and compares it with `budget_weighted`.

- At or under the budget: the job is created and the output carries one note line with the estimate.
- Over the budget: the user decides, never the driver and never a silent default. The driver cannot ask a question, so `factory new --one-job` stops before it creates anything, prints a `needs consent:` line with the estimate, the budget and the setting name `budget_weighted`, and exits with a non-zero status. A second flag, `--accept-over-budget` (name is my call), gives the consent: the job is created, the output carries a `warning:` line, and the same warning is stored on the job like the existing plan warnings. The run then goes ahead and may still park as `budget-stopped` if the real cost passes the budget, which the user already accepted.
- Decided by the user: warn and go ahead, and ask the user for consent when the estimate passes the budget. Raising `budget_weighted` in `project.toml` is the user's own edit and is never done by the driver or the command.

### The `/factory` command

Changes to `crates/factory-core/assets/global-command.md`, the copy that `factory install` puts in `~/.claude/commands`:

- `argument-hint` gains `[--one-job]`.
- A feature may start or end with the single token `--one-job`, handled the same way as `--opus` (line 32): the command removes the token from the feature text and remembers it. It is not part of the feature text or the slug.
- Step 4 (the `new` call, line 86) adds `--one-job` when the token was given. Step 5 (start the run) is unchanged, because `--series` is harmless on a job with no series.
- Plan writing is unchanged, my call: the planner still writes plans of at most 3 tasks each and the validation step still counts them. The driver does the combining, so plans stay small and reviewable, and the same plan folder works with or without the flag.
- When `new` prints a `needs consent:` line, the command does not guess. It tells the user the estimate and the budget in one or two plain sentences and asks whether to go ahead over the budget, with raising `budget_weighted` as the other way out. On a yes it runs the same `new` call again with `--accept-over-budget` added. On a no, or when the user wants a different budget, it stops without creating a job. It never edits `project.toml` itself.
- The Rocket copy `.claude/commands/factory.md` is left alone, my call, because `CLAUDE.md` keeps it as Rocket's own copy. The lint and command tests that pin the global text (`crates/factory-cli/tests/global_command.rs`) gain `--one-job` as a required phrase. The pins on "at most 3 tasks" stay.

### Running and the rest

- Run it with a plain `factory run <id>`. `factory run <id> --series` also works, because `run_series` returns after a merge when `job.series` is empty (`runloop.rs:441`). So `factoryd enqueue`, which defaults `series` to true, needs no change.
- The `factoryd` API `new` request has no field for the flag in this version. The CLI flag is enough for the first release, my call.
- The cost estimate in `add_job` runs only for series jobs. The one job path computes it from the combined plan size, best effort, as today.
- Tickets (`tickets.rs:92`), status, costs and the dashboard treat a job with no series as a standalone job, so they need no change.

## Constraints

- Follow `CLAUDE.md`: `cargo ... -j4`, per crate, no `--workspace`, no `CARGO_TARGET_DIR`. Engine logic stays behind the ports in `ports.rs`. Board JSON fields stay optional.
- Existing tests must pass unchanged, including `crates/factory-engine/tests/series.rs` (a 4 task plan is still refused on the series path) and the agent lint and global command pins on "at most 3 tasks".

## Tests expected

- Unit tests for the combine function: renumbering across plans, one header, merged Review Focus, `Next plan:` lines removed, the checkbox form, a repeated task number across plans, and the body mention warning.
- An engine test that a 4 plan, 12 task spec with `--one-job` makes exactly one job with `series: None`, a 12 task `plan.md`, and no jobs for plans 2 to 4.
- A global command test that the asset mentions `--one-job` in the argument hint and in the `new` step, and that the token is described as removed from the feature text.
- An engine test that the series path still refuses a 4 task plan and still makes one job per plan without the flag.
- A CLI parse test that `--one-job` needs `--plans` and that `--accept-over-budget` needs `--one-job`.
- Budget guard tests for 12 tasks: at 1M the call prints `needs consent:`, exits non-zero and creates no job and no folder. With `--accept-over-budget` it creates the job with a stored warning. At 1.5M it creates the job with only the note line.
- A global command test that the asset describes asking the user on `needs consent:` and re-running with `--accept-over-budget`.

## Left to the builder

The exact function and error names, how the plan is split into header and sections (reuse `plan_header`, `task_section` and `review_focus` from `tasks.rs`), the wording of messages, and where the combine function lives.

## Open risks

- A fix round resumes only the last task's builder, so findings about an early plan go to a builder that never built it. Accepted for the first version.
- The 4000 line diff cap now counts all plans together, so a one job spec is likelier to stop as `diff-too-large`. The user can raise `diff_cap_lines` for such a spec.
- Nothing in the data shows a plan larger than 3 tasks. The first real use should be a spec with no risky paths, and its fix rounds and parks should be checked before relying on it.
