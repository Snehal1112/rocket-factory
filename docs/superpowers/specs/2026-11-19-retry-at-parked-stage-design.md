# Retry at the parked stage

Date: 2026-11-19. Status: design, approved approach.

## Why

When a job parks for a reason outside the builder, the user fixes the cause and then has no clear way to continue. `rework` is the verb they reach for, and it always sends the job back to the builder, which correctly has nothing to change. In the rocket-factory project all 5 "builder made no changes on resume" parks came from this, and two of those jobs were then cancelled:

| Job | Park | What the user needed | What happened |
|---|---|---|---|
| 082 | Both reviewers failed on an invalid API key; the driver treated it as findings, started fix round 2, the fix builder failed the same way | Re-run the review | `rework`, no changes, then a real change was needed to get a review |
| 086 | `no check covers 2 changed code paths: Cargo.lock, Cargo.toml` | Re-run the checks after fixing the config | `rework` twice, no changes |
| 077 | `main busy: the main checkout has tracked changes` | Retry the merge (`approve` already does this) | `rework` plus a hand rebase, then `committer rewrote history`, cancelled |
| 066 | `branch tip differs from the reviewed head`, main needed merging in | `factory sync` | `rework` asking the builder to merge, builder refused (its rules forbid git), cancelled |

Two of these had a working command that the user did not find. Two had no command: nothing re-runs only the checks or only the review on the current head.

Success: for each of those four parks there is one obvious command that continues the job without a builder run, and `rework` cannot be used by mistake on them.

## Goals

- One command, `factory retry <id>`, that continues a parked job from the stage that parked it.
- Every park outside the builder records which stage to resume at.
- A reviewer run that fails for an environment reason parks the job instead of starting a fix round (the fix proposed in issue #110).
- `rework` refuses on a park that records a resume stage, and names `retry`, unless `--force` is given.
- The same action through the factoryd API and the dashboard.

## Non-goals

- Automatic retries without the user (approach B). It comes later, for causes that are clearly transient (main busy, merge lock busy, network, API key), and builds on the resume field from this design.
- Changing how builder parks work (checks failed, changes remain, builder blocked, nothing changed, budget stop). `rework` stays the command for those.
- Re-running a builder that stopped with `Failed`. A failed builder may leave partial work, so it stays a `rework` case.
- The other "no changes on resume" cause seen in 086 (a rework with a concrete note that still produced no change). It is a separate builder issue.
- Splitting `ParkKind::Other` into named kinds.

## Anti-goals

- A retry must never merge something that was not reviewed and checked on the exact head it merges. Every merge gate that applies to `approve` applies to `retry`.
- A retry must never start by running the builder. After the stage it resumes at has run, the normal rules apply: a review that asks for real changes, or checks that fail, start a fix round as they would in any run. (Orchestrator's call: the builder runs only when there is real work for it.)
- A retry whose cause is still present must not loop. It parks again with the same reason and stops.

## Constraints

- Board and park JSON stay compatible with the dashboard (`dashboard/`, separate workspace): new fields are optional, and records written before this change still load.
- Git only through `factory_git::Repo`. Paths only through `Layout`.
- Board, ledger and events are written only by the driver.
- The project and job leases are held during a retry exactly as for `approve` and `run`.

## Approaches considered

1. **Optional "resume at" field on each park (picked).** Each park site that is not a builder park also records the stage to resume at. `retry` reads it; `rework` checks it. Builder parks leave it empty. Additive, so the dashboard and old records keep working, and nothing that reads `park_kind` changes. Parks written before the change have no field, so `retry` refuses on them and names the old command.
2. **Split `ParkKind::Other` into named kinds** (main busy, branch moved, review failed, checks not run). Clearer labels, but `park_kind` drives `next` hints, approve, rework and the dashboard, so every one of those paths changes.
3. **Infer the stage from the park reason text.** Rejected: it breaks when a message is reworded.

The user picked approach 1, plus the #110 reviewer-failure park, and chose that `rework` refuses and points to `retry`.

## Design

### The resume field

A new enum in `factory-core` (`park.rs`):

```
ResumeAt = Review | Rereview | Merge | Sync
```

- `Review`: run the checks and the reviewers on the recorded `job.head`, then apply the usual decision (`decide.rs`). This covers "checks did not run or were not mapped" too, because the review stage runs the checks first (`runloop.rs:740`).
- `Rereview`: the branch tip moved after the review. Do what `sync_requested` does for a moved or rewritten branch (`sync.rs:76-93`): set `head` to the tip, reset the reviews, merge the base branch if it is behind, and run one whole-branch review from the merge base with the security role. Unlike `factory sync` on a `needs-human` job, do not re-park with "sync review passed, run factory approve" (`sync.rs:101-104`); apply the usual decision instead. (Call: the whole-branch review is the safe floor when history may have been rewritten, as in job 077.)
- `Merge`: run the merge stage again with all its gates (`merge_stage`).
- `Sync`: main moved more than `MAX_SYNCS` times. Allow one more automatic sync (`sync_stage`, review of the sync diff only) by resetting the job's `syncs` counter, then continue to the merge. (Call: the cheap sync-diff review, because the branch itself was already reviewed.)

`ParkRecord` (park.json) gets `resume: Option<ResumeAt>`, serialized as `"review" | "rereview" | "merge" | "sync"`, skipped when `None`. The board's `Job` gets the same optional field next to `park_kind`, so the dashboard and `status` can offer `retry` without reading park.json. Both are cleared whenever the job leaves `needs-human`.

### Which parks set it

| Park (today's reason or kind) | Site | Resume at |
|---|---|---|
| New `review-failed` (see below) | `review.rs` | Review |
| Security agent file not found | `review.rs:156-166` | Review |
| `no-checks` (no check covers changed paths) | `checks_stage.rs:310-314`, `review.rs:368` | Review |
| Branch tip differs from the reviewed head | `merge.rs:288-297` | Rereview |
| Main busy (dirty main checkout) | `merge.rs:298-326` | Merge |
| Merge lock busy (another project is merging) | `merge.rs` `take_merge_lock` callers | Merge |
| Main moved again after `MAX_SYNCS` syncs | `merge.rs:373-376`, and via `sync.rs:118-126` → `merge.rs:331` | Sync |

Every other park leaves the field empty: builder parks, `no-tests` (the builder added no test, a code problem that `rework` fixes; call), budget stop, risky diff and changes remain (approve or rework decide those), merge conflict, sync conflict and sync refused (`sync.rs:147-153`), base not on main, scope violation (including `checks_stage.rs:305`), missing plan, run loop did not settle.

The builder of this plan confirms each site against the code and lists any park site not in the table with its chosen value (empty unless it is clearly outside the builder and safe to re-run).

### A failed review parks (issue #110)

Today a reviewer run that fails (auth error, rate limit, timeout, empty reply) ends with verdict `Missing`, and `decide.rs:64` treats `Missing` like `Changes`, so a fix round starts with the error text as findings.

New behaviour: a reviewer role counts as failed when its run ends with stop reason `Failed` or `Stalled` (`factory-agent/src/run.rs:4-10`). `TurnLimit` and `BudgetStop` keep today's behaviour, because a retry would likely hit the same limit (call).

- When at least one role failed and no role returned a `Changes` verdict, the review stage parks the job with a new kind `review-failed` and `resume: Review`, with the failed role's last message as evidence. No fix round starts and no `changes-r<n>.md` is written.
- When another role returned `Changes`, the real findings win and the fix round starts as today; the failed role runs again in the next round (call).
- A reply that contains a verdict is never treated as a failure. A `Missing` verdict from a run that finished `Done` but gave no parsable verdict keeps today's behaviour.
- A failed sync or rebase review (`ReviewRun::Sync` / `Rebase`, `sync.rs:202-217`) parks the same way with `resume: Rereview`, which repeats the whole-branch review and so covers the sync diff (call).

`ParkKind::ReviewFailed` (`"review-failed"`) is a new kind; its `next` is `factory retry <id>` and `factory cancel <id>`.

### `factory retry <id>`

Refuses unless the job is in `needs-human`, is not cancelled, and its park has a `resume` value. The refusal for a park without one names that park's own next commands.

Then, holding the project and job leases like `approve`:

- `Review`: reset the review states of the current round, set the stage to `review`, and drive the job like `run` until it merges or parks. No new round and no change to the fix-round count; later fix rounds follow the normal rules (see Anti-goals).
- `Rereview`: as described above, then drive the job on like `run`.
- `Merge`: run the merge stage with all its gates. When the park happened after the user approved the job (the job was in stage `approve`, with or without `--waive` or `--without-review`) and the branch tip still equals the approved head, that approval carries over and `approve_refusal` (`board.rs:323-346`) is not asked again. Otherwise the retry applies the same refusals as a plain `approve` (call: a merge the user already approved on this exact head should not need the flags again; the existing waiver record keeps its tickets).
- `Sync`: as described above, then drive the job on like `run`.

When the merged job belongs to a series with later plans, the retry continues the series exactly as `factory run --series` would (user's decision). A job not in a series stops after the merge.

It writes a ledger ruling (`retry at <stage>`) and a log event `operator.retry`. If the same cause is still there, the job parks again with the same reason and `retry` reports the park.

`retry` is listed first in the park's `next` commands whenever `resume` is set, so `factory status`, park.json and the dashboard show it.

### `rework` refuses on these parks

`factory rework <id> <note>` on a job whose park has a `resume` value refuses with a message of the form:

`job 086 parked outside the builder (no check covers 2 changed code paths); run factory retry 086, or factory rework 086 --force <note> to send it to the builder anyway`

`--force` keeps today's rework behaviour exactly.

### API and dashboard

- factoryd: a new request action `Retry`, queued like `Approve` but held by the daily budget like `Run` (a retry can run reviewers, fix rounds and later plans), with child command `factory --project <p> retry <id>`. The series continuation happens inside `retry`, so the request needs no `series` flag. A new endpoint `POST /projects/{p}/jobs/{id}/retry`, `202 {"requestId"}`, `409 conflict` when the job has no resume value. `POST .../rework` gains an optional `force` boolean.
- Dashboard: a `retry` action on a `needs-human` card whose job has a resume value, shown first.

### Docs

`docs/reference/software-factory.md` (commands table and park section), `README.md` API table, and `factory-core/assets/global-command.md` if it lists the commands (`factory-cli/tests/agent_lint.rs` pins its text).

## Tests (what must be shown)

- Each park site in the table writes the listed resume value; builder parks write none.
- A reviewer fake that fails (`Failed`) parks as `review-failed` with `resume: Review` and starts no fix round; a real `Changes` verdict still starts one; one role failed plus another returning `Changes` starts a fix round; `TurnLimit` keeps today's behaviour.
- `retry` on a `review-failed` job runs checks and reviewers on the same head and merges on approve, with no builder run and the same round number; a `Changes` verdict on retry starts a normal fix round.
- `retry` on a `no-checks` park after the check mapping is fixed runs the checks and continues; `no-tests` has no resume value and `rework` works on it without `--force`.
- `retry` on a main-busy park merges once main is clean, and parks again with the same reason while it is still dirty; a job approved with `--without-review` or `--waive` before the park merges on retry without the flags when the tip is unchanged.
- `retry` on a tip-differs park moves `head` to the tip and runs a whole-branch review, and does not re-park with "sync review passed".
- `retry` on a max-syncs park runs one more sync and merges.
- `retry` that merges plan 1 of a series starts plan 2.
- `retry` refuses on a builder park, on a cancelled job, and on a park.json written before this change.
- `rework` refuses on a park with a resume value and names `retry`; `rework --force` behaves as today.
- Old park.json and board JSON without the field load, and the dashboard fixture test still passes.

## Left to the builder

- Exact names of the enum and fields, and where the shared "drive the job like run" helper lives.
- Whether the merge-lock-busy park is set in `merge.rs` or in `take_merge_lock`.
- The wording of messages, within the form given above.
