# Close tickets with Fixes design

## Goal

Make the last plan of a job close its GitHub ticket with `Fixes: #N`, and make every merged job with an issue file its spec follow-ups first and then close the ticket, intake jobs included, so a partly fixed ticket ends closed with follow-up tickets for the rest.

## Overview

Tickets stay open after the work is merged. Job 076 (intake, issue #72) merged with `Relates to: #72`, because its spec had a `## Follow-ups` section. Three rules produce this.

- The merge footer is `Relates to` whenever the spec has follow-ups, `Fixes` otherwise (`message.rs:67-72`). It ignores the plan's place in a series, so a series without follow-ups puts `Fixes` on plan 1 of N.
- The step that files follow-ups and closes the ticket (`tickets_step`) runs after a merge only when `tickets_on_merge` is true (`merge.rs:174`), and that setting defaults to false (`config.rs:246`).
- An intake job gets a guarded tracker that refuses every write, and `tickets_step` returns early for it (`tickets.rs:161`). Intake closes its own issue, but nobody files the spec follow-ups, so job 076 lost two.

Builder, fix and sync commits always say `Relates to` (`commit.rs:25`, `message.rs:71`). That stays, because those commits must never close a ticket.

## Current state

| Piece | Today | Gap |
|---|---|---|
| `driver_footers` (`message.rs:67-77`) | Merge: `Relates to` if `req.partial`, else `Fixes`. Sync: `Relates to`. | Depends on follow-ups, not on the last plan |
| `partial` (`compat.rs:603`) | `spec_has_follow_ups(job_dir)` (`message.rs:51-55`) | Wrong input for the footer |
| `merge_stage` (`merge.rs:174-180`) | Runs `tickets_step` only when `tickets_on_merge` | Default false |
| `Config.tickets_on_merge` (`config.rs:129,246`) | Default false | Ticket stays open |
| `tickets_step` (`tickets.rs:143-165`) | Skips a job that is not merged, not the last plan, an intake job, or has no `#N` | Intake never files |
| Label copy (`tickets.rs:196-208`) | Copies the parents' labels except `tracking` | Would copy `factory` and `factory: ...` from an intake parent |
| `run_intake` (`intake.rs:262-277`) | After `RunEnd::Merged`: relabel done, comment `merged as job <ids>`, close | No follow-ups filed |
| `GuardedTracker` (`tracker.rs:133-175`) | Refuses create, comment, close, label edits for an intake job | Stays as it is |

## Non-goals

- Changing builder, fix or sync commit footers.
- Closing a ticket only after the push. The step closes at merge time, as `gh issue close` does today.
- Filing follow-ups for a manual `factory tickets <id>` on an intake job. That command keeps skipping intake jobs.
- Reading issue numbers from the spec. Only the feature text names issues (`software-factory.md:80`).
- Changing the waiver and minor-finding tickets.

## Design

```
merge of plan k of N ──► footer: Fixes if k == N (or no series), else Relates to
merge_stage ──► tickets_step (tickets_on_merge, default true): file follow-ups, comment, close
run_intake  ──► Merged ──► file follow-ups (raw tracker) ──► label done, comment, close
```

### 1. Merge footer follows the last plan

`MessageRequest.partial: bool` is replaced by `closes_ticket: bool`. `Driver::commit_message` (`compat.rs:576`) sets it to true when `job.series` is `None` or `pos.index >= pos.total`, the same test as `tickets.rs:153`. `driver_footers` gives a merge `Fixes` when `closes_ticket`, else `Relates to`; a sync is always `Relates to`. `spec_has_follow_ups` is removed with its tests. A spec with follow-ups no longer changes the footer.

Example: a 3-plan series for `#40`. Merges of plans 1 and 2 end `Relates to: #40`, then `Factory-Job: <id>`. The merge of plan 3 ends `Fixes: #40`, then `Factory-Job: <id>`, whether or not the spec lists follow-ups.

### 2. The ticket step runs by default

`Config.tickets_on_merge` defaults to true. `tickets_on_merge = false` stays valid and turns the step off. `merge_stage` is unchanged: it runs the step when the setting is true. The step already files follow-ups first, then comments, then closes (`tickets.rs:262-360`), is gated on the last plan, records progress in `tickets.json` and never fails the merge.

### 3. Follow-up labels never include intake labels

In the label copy of `tickets_step`, skip `tracking`, the configured `intake_label`, and the three state labels `factory: running`, `factory: needs-human` and `factory: done` (the constants in `intake.rs:14-18`). Without this, a follow-up filed from an intake parent would carry `factory` and intake would pick it up unattended. The skip applies to every job, intake or not.

### 4. Intake files the follow-ups

The follow-up filing of `tickets_step` becomes its own function in `tickets.rs`:

`pub fn file_follow_ups(driver: &Driver, id: &str, tracker: &dyn Tracker, dry_run: bool) -> Result<FiledFollowUps, EngineError>`, where `FiledFollowUps { report: TicketsReport, numbers: Vec<u32>, listed: usize }` carries the issue numbers created or adopted and the count of spec bullets

It runs the part of `tickets_step` from the label copy to the last `created` record (parents, labels, marker, one issue per bullet). It does not comment and does not close. `tickets_step` calls it for a normal job, then comments and closes as today. Its skips for a job that is not merged, not the last plan, or has no issue reference apply to both callers; the intake skip stays in `tickets_step` only.

`run_intake` calls `file_follow_ups(driver, &last_job_id, tracker, false)` with its own raw tracker after `RunEnd::Merged` and before the relabel. `last_job_id` is the last job of `jobs`. The issue comment `merged as job <ids>` gets one fixed line after it: `follow-ups filed as #a, #b` (numbers only), `no follow-ups`, or `follow-ups not filed, see the spec of job <id>`. Agent text, titles and bodies are never posted in the comment. A filing failure is written to the ledger and events and never stops the relabel, comment and close.

Filed follow-up issues carry the footer `Filed by factory job <id> (<slug>) from the follow-ups of #N.` as today (`tickets.rs:213`), so a rerun adopts one it had started (`find_created`).

### 5. Docs

`software-factory.md` (footer table and the merge-footer line near 138, ticket step, intake paragraph), `README.md` (Ticket hygiene), `project-toml-reference.md:138`, `getting-started.md:364,380`, `factory-user-guide.md:667,1300` and the settings comment in `settings_block.rs:87` say: default true, `Fixes` on the last plan only, follow-ups filed before the close, intake files follow-ups through its own call.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| Single-plan job (`series` is `None`) | Merge footer `Fixes` |
| Plan k of N, k < N | `Relates to`; the step skips with "not the last plan" |
| Last plan, spec has follow-ups | `Fixes`; follow-ups filed, comment, close at merge time |
| Follow-up filing fails (`gh` error) | The step stops before the close and reports a failure; the `Fixes` footer still closes on push; the follow-ups stay in the spec; `factory tickets <id>` retries, comments, and tolerates an already-closed ticket |
| Feature text has no `#N` | No footer, no step work, as today |
| Issue URL of another repository | Step skips, as today |
| `tickets_on_merge = false` | Step does not run; footer is still `Fixes` on the last plan |
| Parent is an intake issue | Follow-ups carry no `factory` or `factory: ...` label |
| Intake series of several plans | Footers by position; follow-ups filed once, after the whole series merged |
| Intake filing fails | Comment says `follow-ups not filed, see the spec of job <id>`; the issue is still labelled done and closed |
| `gh` already closed the ticket by push | Step records `already closed` and comments only |
| `factory tickets <id>` on an intake job | Skips, as today |

## Testing

- `crates/factory-engine/src/message.rs` unit tests: `driver_footers_gives_a_merge_fixes_when_it_closes_the_ticket` and `driver_footers_gives_a_merge_relates_to_when_it_does_not_close` (replace the `partial` pair at `message.rs:208-234`); `a_sync_is_always_relates_to`. Remove `spec_has_follow_ups` tests (`message.rs:237-246`).
- `crates/factory-engine/tests/message.rs`: update the merge tests to set `closes_ticket`.
- `crates/factory-engine/tests/message.rs`, through `Driver::commit_message`: `commit_message_uses_fixes_on_the_last_plan_even_with_follow_ups`, `commit_message_uses_relates_to_on_plan_one_of_three`, `commit_message_uses_fixes_for_a_single_plan_job`.
- `crates/factory-core/src/config.rs`: change `tickets_on_merge_is_off_by_default_and_can_be_turned_on` (`config.rs:1272`) to `tickets_on_merge_is_on_by_default_and_can_be_turned_off`; `resolve.rs:423` test follows.
- `crates/factory-engine/tests/tickets.rs`: `follow_up_labels_drop_the_intake_label_and_state_labels` (a parent with `factory: running`, `factory`, `tracking`, `tech-debt` gives only `tech-debt`); `file_follow_ups_files_and_does_not_close` (fake tracker records create calls and no close).
- `crates/factory-engine/tests/intake.rs`: `a_merged_intake_job_files_its_follow_ups_before_the_close` (fake tracker call order: create, comment, close); `the_intake_comment_names_only_issue_numbers`; `a_failed_follow_up_still_closes_the_intake_issue_and_says_so`.
- `crates/factory-engine/tests/tickets.rs`: `a_merge_with_the_default_config_runs_the_ticket_step`. `waivers.rs:124` and `log_events.rs:313` already set `tickets_on_merge: true`; a test that relied on the old default sets `false` explicitly.
- Commands: `cargo test -j4 -p factory-core`, `-p factory-engine`, `-p factory-cli`, then `cargo clippy -j4 -p factory-core -p factory-engine -p factory-cli` and `cargo fmt --check` per crate.

## Files changed

- `crates/factory-engine/src/message.rs`, `compat.rs`, `tickets.rs`, `intake.rs`, `lib.rs` (exports)
- `crates/factory-core/src/config.rs`, `settings_block.rs`, `resolve.rs` (test)
- Tests listed above
- `docs/reference/software-factory.md`, `docs/reference/project-toml-reference.md`, `docs/guides/getting-started.md`, `docs/guides/factory-user-guide.md`, `README.md`

## Constraints

- Builder, fix and sync commits keep `Relates to`. Only a merge on the last plan says `Fixes`.
- No agent text goes into an intake issue comment. No `git` shell-outs, no `/proc` reads, no `Cargo.toml` change.
- Board JSON stays compatible with the dashboard. No new board field.
- `GuardedTracker` and `raw_tracker` stay as they are; only `intake.rs` may call `raw_tracker` (a source test pins it).
- Use `-j4`, never `--workspace`. Comments are short full sentences.

## Acceptance criteria

- The merge of the last plan of a series, and of a single-plan job, ends with `Fixes: #N` when the job has issues, also when the spec has follow-ups.
- The merge of any earlier plan ends with `Relates to: #N`.
- Builder, fix and sync commits still end with `Relates to: #N`.
- `tickets_on_merge` defaults to true, `tickets_on_merge = false` turns the step off, and a project that sets it keeps its value.
- After a merge with the default config, follow-ups are filed first, then the ticket gets a comment and is closed.
- A follow-up filed from an intake issue never carries `factory`, `factory: running`, `factory: needs-human` or `factory: done`.
- An intake job files its spec follow-ups through `run_intake` before it labels the issue done, comments and closes it.
- The intake comment names only issue numbers and never agent text.
- A failure to file a follow-up never stops the merge, the intake close or the relabel.
- The docs listed above state the new default and footer rule.

## Out of scope

- Closing only after a push, and holding the `Fixes` footer back when filing failed.
- A retry command for intake follow-ups.
- Removing the `tickets_on_merge` setting.

## Follow-ups

None.

## Decisions

- What does `tickets_on_merge` do now? Options: default true and keep as opt-out, remove it, keep default false. Chosen: default true, keep as opt-out. Decided by: user
- How does intake file follow-ups? Options: intake calls the step itself, loosen the guarded tracker, do not file. Chosen: intake calls the step itself, with intake labels dropped from follow-ups. Decided by: user
- What if a follow-up cannot be filed while `Fixes` closes the ticket on push? Options: accept and warn, file before the merge commit, keep `Relates to` with follow-ups. Chosen: accept and warn. Decided by: user
- Does the merge of the last plan say `Fixes` even with follow-ups, and earlier plans `Relates to`? Chosen: yes. Decided by: user
- Does the step close at merge time or after a push? Chosen: at merge time. Decided by: user
- Are builder, fix and sync commits changed? Chosen: no, they stay `Relates to`. Decided by: call
- Are the intake labels dropped for every job or only for intake? Options: every job, intake only. Chosen: every job, because a parent can carry them for other reasons. Decided by: call
- Is `MessageRequest.partial` renamed? Options: rename to `closes_ticket`, keep the name. Chosen: rename, because it no longer means follow-ups. Decided by: call
