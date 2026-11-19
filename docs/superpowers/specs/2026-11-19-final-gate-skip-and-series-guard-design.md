# Skip the repeated final checks, guard series order

Status: draft, 2026-11-19. Source: the efficiency review of all 88 rocket-factory jobs
(items 2 and 3 of the re-ranked list).

## Why

- After an Approve verdict, `final_gate` (`review.rs:395`) runs every mapped check again
  through `run_checks_now`. They ran a moment earlier on the same head. The run loop
  always runs checks right before review (`runloop.rs:750`), reviewers do not commit,
  and a sync reruns the checks itself. The repeat is 10-15% of check time on every job,
  and checks have grown from 7% to 22% of wall time.
- Job 087 (plan 3 of series S20) started while plan 2 (job 086) was still parked. Its
  worktree was cut from `ed8dc8d`, which does not have 086's code. After 086 merged,
  a rework reused the same stale worktree. The builder blocked twice and the job was
  cancelled. Nothing stops a later plan from starting early, and a job with no commits
  never gets a fresh branch.

## Goals

- `final_gate` does not run the checks again when the head and worktree have not
  changed since the checks passed.
- A series plan does not start its builder before the plan before it has merged.
- A job with no commits of its own builds from the current base branch when it
  starts again.

## Non-goals

- Job 030 (a plan that asked for code that did not exist). That is a planning
  problem, not a base problem.
- Cascade cancels of sibling plans. That is a human call after a park.
- Per-task informational checks and the Cargo risky-diff rule (separate items).

## Anti-goals

- A merge on checks that never ran on the merged head. When in doubt, run them.
- Rebasing or rewriting a branch that already has job commits.

## Approaches

Series guard, decided in conversation:

- **(a) Park at build start (picked).** The engine checks the plan before it. If that
  plan has not merged, the job parks before any builder runs and before any tokens are
  spent. A job with no commits re-cuts its branch from current main. This covers every
  way into the engine (CLI, factoryd, dashboard) and also fixes the stale worktree
  from a rework.
- (b) Refuse the run request in the CLI or factoryd. It answers earlier, but other
  routes into the engine get around it.
- (c) Run plan N-1 first automatically. It does not work when N-1 is parked waiting
  for a human, as 086 was.

Final checks: there is only one sensible way (below). Removing `final_gate` outright
was rejected, because its gate checks and ruling must stay, and a changed head or
dirty worktree still needs a real run.

## Design

### Final checks skip

- When `checks_stage` passes, record the head it checked on the job as a new optional
  board field (for example `checks_head`). Optional, so `dashboard/` still reads the
  board. A failing run or a new head clears it or overwrites it.
- In `final_gate`, skip `final_checks` when `job.head == checks_head` and the worktree
  is clean. Otherwise run as today.
- On a skip, still apply `gate_checks` and still write the `checks ran: ...` ruling,
  with the command list from the recorded run. The ruling says the run was reused.
- `park_after_sync` keeps its rerun. A sync moved the head there.

### Series guard

- At build start, before the first builder (fresh start, rework or retry), a job with
  a series position above 1 looks up the job for the plan before it (same series id,
  index - 1).
- If that job is not merged, the job parks with a reason that names it, for example
  "plan 2 (job 086) is not merged yet". The park resumes at build, so `factory retry`
  after the merge carries on. A cancelled predecessor parks the same way and names
  the cancel.
- If the job has no commits of its own and its base is not the current base-branch
  tip, the branch and worktree are cut again from current main before the builder
  starts. A job with commits keeps its branch; the existing sync handles it.

## Left to the builder

- Field name and where the head is recorded; the park kind for the series guard
  (new kind or `Other`) as long as it resumes at build.
- How the clean-worktree check and the re-cut reuse `factory-git` helpers.
- Tests: a skip on an unchanged head, a rerun on a changed head or dirty worktree,
  a park when the plan before is unmerged, a re-cut of an empty stale branch, and no
  re-cut of a branch with commits.
