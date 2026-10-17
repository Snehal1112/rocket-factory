# Sync review judges the job's own work

Ticket: Snehal1112/rocket#109 "fix(factory): sync review judges main's commit against the plan".

## Goal

After main is merged into a job, the factory must not reject an approved job because of commits that have nothing to do with it, and any sync review that does run must read the whole job diff.

## Problem

`sync_stage` (`crates/factory-engine/src/sync.rs`) stores the old head in `prev_head` and sends the job back to review. `review_stage` (`review.rs`) then shows the reviewer only `prev_head..head`, which is the sync merge. When main moved because of an unrelated commit, that diff holds none of the job's files. The reviewer says the diff does not implement the plan and returns CHANGES, the job parks in needs-human, and the board keeps `code:changes` after a human approves. This was seen in job 013.

## Decisions

The interview ran with a user who answered one question directly. The other answers are the author's proposed defaults, shown to the user in chat, and the user went on without changing them. The user should confirm or correct them when approving this spec.

| Topic | Question asked | Answer |
|---|---|---|
| Purpose, who benefits | What should happen after a sync pulls in unrelated commits? | User chose option A: skip the review when main did not touch the job's files, keep the earlier Approve. |
| Edge case: overlap | Main touched files the job also changed and git merged them cleanly: review or skip? | User chose A: when main touched a job file, run one review on the job's own changes. |
| Constraints | Do checks and risk rules still apply? | Proposed default, not yet confirmed: the sync checks always run, and risk and size judge the whole diff as today. |
| Compatibility | Board or dashboard changes? | Proposed default, not yet confirmed: no required board field is added, optional fields only. |
| Success checks | How is each measured? | Proposed default, not yet confirmed: the three tests under Acceptance criteria. |
| Scope against related tickets | Any related ticket or branch? | Proposed default, not yet confirmed: only the sync review in `factory-engine`. No related ticket was checked. |
| Non-goals | What stays out? | Proposed default, not yet confirmed: see Non-goals. |
| Rejected alternatives | What else was considered? | Option B (always review the whole diff) costs tokens when nothing relevant changed. Skipping every sync review would miss bad merge results. |

## Design

1. Decide overlap in `sync_stage` after a `Merged` outcome. The job paths are `changed_paths(base, old_head)`. The main paths are the paths changed on main since the previous merge point, which the builder derives from git (the merge result has two parents, one of them is the old head). If the two sets share no path, the sync is clean.
2. Clean sync: still run the checks. Do not run the reviewer. Keep the review state of the previous round, so the board still shows the earlier verdict. Return `Synced` after the checks pass. A failing check still parks the job as today.
3. Overlapping sync: the reviewer gets the job's own changes judged against the plan. `job.base` never moves, so a plain `base..head` diff also holds every commit main brought in. The builder must diff the job's own work: from the merge base of the job and the new main tip, or the equivalent, to `head`. This is the author's call, not the user's.
4. The board verdict is the latest review round. A clean sync must not wipe it (`reset_reviews` in `checks_stage.rs` is the place to check), and a later Approve replaces an earlier CHANGES.
5. A clean sync returns the job to the Approve stage after the checks pass, because `sync_stage` moves it back to Review first and nothing else would return it. Author's call.
6. A failing sync check keeps today's path (`fix_or_park`), including its round bump and review reset. Only a passing clean sync keeps the earlier verdict. Author's call.
7. A conflict always parks today, so "a resolved conflict triggers a review" cannot happen. Only the overlap case runs a review.

8. Risk and size rules judge the job's own changes too (user chose A). Today they judge `base..head`, which also holds main's files after a sync, so a main commit on a risky path would send the job to needs-human. The same "job's own changes" diff from point 3 feeds `classify_paths` and the size cap in `review_stage`.

## Follow-ups

- A human approve does not mark the board's code review as passed. This spec does not change the approve command.

## Behavior that must not change

- The first review and fix rounds still review only the new work of the round.
- Risk paths and the size cap still apply and still send a risky or oversized job to a human. Only the diff they judge changes (see Design point 8). Before any sync the job's own changes equal `base..head`, so results do not change. The budget is unchanged.
- A conflict or refused merge still parks the job. `MAX_SYNCS` is unchanged.
- Board JSON stays readable by `factory-server` (`dashboard/`).

## Acceptance criteria

- A sync that only brings in commits touching none of the job's files does not park the job and does not call the reviewer.
- A sync review that does run gets the job's own changes, not only the sync merge and not main's commits.
- The board review verdict shows the latest round: a clean sync keeps the earlier Approve, and an Approve after a CHANGES replaces it.
- After a sync, a commit from main on a risky path, or a large commit from main, does not by itself send the job to needs-human. The job's own risky or oversized changes still do.
- The sync checks still run in both cases, and a failing check still parks the job.

## Non-goals

- No change to the first review, to fix rounds, or to `MAX_SYNCS`.
- No change to merge conflict handling.
- No new board fields that the dashboard must read.
