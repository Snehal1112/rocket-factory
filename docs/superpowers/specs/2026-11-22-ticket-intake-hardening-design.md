# Ticket step, intake export, sync and waiver leftovers

**Commit:** fix(tickets): skip code refs and keep waivers

## Goal

Finish the leftovers that issue 132 lists for the ticket step, intake export, close-with-fixes, sync and waivers, so no `gh` call closes or files the wrong issue, an export never writes outside its folder, and a waiver is never lost.

## Overview

Issue 132 keeps the open parts of issues 57, 70, 79, 80 and 24. A survey of main at `5da3514` confirmed every item below is still open:

- **57 A:** issue refs inside inline code or a fence still count. A feature text with `` `#42` `` makes the ticket step comment on #42 and close it.
- **57 C:** two follow-up bullets with the same first sentence collapse into one ticket, because `tickets.json` keys entries by title.
- **70:** `export_entry` refuses only `""`, `.` and `..`, so a tree entry named `a/b` or holding `\0` reaches `dest.join`. The `.git` skip, the non-UTF-8 refusal and the unsafe-name refusal have no tests.
- **79:** an intake job's feature text carries the issue title. A GitHub issue URL in the title still parses as a parent, so the ticket step could close an unrelated issue.
- **80:** a requested sync that merged main and then stopped (for example on a failing check) parks the job. Running `factory sync` again returns `UpToDate`, because the tip equals the head and the branch is not behind, so nothing is checked or reviewed. Only `factory retry` reruns it. No test covers this.
- **24:** `approve --waive` moves the job to Approve before it writes the waiver, so a failed write leaves an approved job with no waiver. A retry with a new reason keeps the old reason in `waivers.json`, while the ledger records the new one.

The fix-design for 57 A and 57 C is the one issue 57 proposed. Live checks, docs and the other areas of issue 132 are separate specs.

## Current state

| Piece | Where | Today |
|---|---|---|
| `#N` refs | `crates/factory-core/src/commitmsg.rs:102-119` `hash_refs` | Reads every `#` after a non-word char, in code too. |
| `owner/repo#N` refs | `commitmsg.rs:121-159` `shorthand_refs` | Same, no code skip. |
| URL refs | `commitmsg.rs:161-183` `url_refs` | Same, no code skip. |
| Entry points | `commitmsg.rs:185-210` `plain_refs`, `parse_issue_refs`, `parse_issues` | `parse_issue_refs` feeds the ticket step (`crates/factory-engine/src/tickets.rs:243`); `parse_issues` feeds commit footers (`one_job.rs:126`, `runloop.rs:533,579,814`). |
| Fence helper | `commitmsg.rs:285-295` `unfenced_lines` | Drops lines inside ``` fences, used for builder replies. |
| Marker | `tickets.rs:37-110` `Marker` | `created: Vec<(String, u32)>` keyed by title, `pending: Vec<String>` of titles. |
| Marker use | `tickets.rs:201-214` `filed_numbers`, `tickets.rs:296-360` the create loop | Every lookup compares `item.title`. |
| Follow-up | `crates/factory-engine/src/followups.rs:12-15` `FollowUp` | `title` (first sentence) and `body` (the full bullet text). |
| Export | `crates/factory-git/src/repo.rs:297-330` `export_entry` | Non-UTF-8 name is `BadInput`, `.git` is skipped, `""`/`.`/`..` are `BadInput`. |
| Intake feature | `crates/factory-engine/src/intake.rs:545-556` `issue_feature` | Title without `#`, then `, #N`. `mark_series` (`intake.rs:560-581`) sets `intake_issue` and `issues = [N]`. |
| Ticket parents | `tickets.rs:238-260` | Skips intake jobs only with `skip_intake`; otherwise parents come from `parse_issue_refs(&job.feature)`. |
| Sync | `crates/factory-engine/src/sync.rs:53-125` `sync_requested`, `sync_requested_by` | Returns `UpToDate` when `tip == head && !behind && !retry` (around line 95). Otherwise a sync that is not an approved head resets reviews and the floor in one write, merges main when behind, then reviews the whole branch. |
| Sync test | `crates/factory-engine/tests/sync_review.rs:162` | `a_second_sync_after_a_stopped_first_one_still_reviews_the_whole_branch` covers a stop after the reset, before the merge, by setting that state by hand. |
| Approve | `crates/factory-cli/src/commands.rs:790-840` `approve` | `update_job` to Approve, then `record_waiver(...)?`. |
| Waiver | `crates/factory-engine/src/waivers.rs:69-79` `record_waiver` | A retry with the same `head` and `file` as the last, not superseded entry returns `Ok(())` and changes nothing. |

## Non-goals

- No change to the `Tracker` trait, `GhTracker`, the board JSON or the dashboard.
- No change to how `factory tickets --dry-run` prints, except the item names it already prints.
- The other parts of issue 57 (A boundary rules, B pending marker, D lease, E body cut) are done and stay as they are.
- Issue 132's other areas (job creation, harness and CI, docs, live checks) are not in this spec.

## Design

```
feature text --strip_code--> plain text --> #N, owner/repo#N, URL refs     (57 A)
follow-up bullet --> key = bullet body --> tickets.json entry {title, key, number}   (57 C)
tree entry name --> UTF-8? .git? safe? --> write or skip or BadInput        (70)
intake job --> parents = [intake_issue], feature refs ignored              (79)
sync stopped after merge --> second sync: reviews unfinished --> whole-branch review   (80)
approve --waive --> record_waiver --> stage Approve --> merge               (24)
```

### 1. Code is never a reference (57 A)

A new private function in `commitmsg.rs`:

```rust
fn strip_code(text: &str) -> String
```

- Lines inside a ``` fence, and the fence lines themselves, become empty lines. An unclosed fence runs to the end of the text, as `unfenced_lines` does.
- On the other lines, every inline code span (from one backtick to the next backtick on the same line) is replaced by a single space. A lone backtick with no closing one is kept as text.
- `plain_refs` and `shorthand_refs` run on `strip_code(text)` instead of `text`, so `parse_issue_refs` and `parse_issues` both skip code. The boundary rules of `hash_refs` and `url_refs` stay as they are.

Example: ``fix `#42` and #7`` gives `[7]`. A fence holding `https://github.com/o/r/issues/5` gives no ref.

### 2. Follow-ups are keyed by bullet text (57 C)

- `Marker.created` becomes `Vec<Created>` with `struct Created { title: String, key: Option<String>, number: u32 }`. New entries get `key: Some(item.body.clone())`. `tickets.json` writes `"key"` only when it is set, so the file stays readable by older drivers.
- `Marker.pending` holds keys for new entries. An old pending entry is a title.
- An item matches a created or pending entry when the entry's key equals `item.body`, or the entry has no key and its title equals `item.title`. Old markers therefore still stop repeat creates.
- `filed_numbers` uses the same match, so the parent comment lists one number per bullet.
- An adopted number (`find_created` on a pending item) that another created entry already holds is not adopted; the item is created instead. That keeps the second of two same-titled bullets from taking the first one's issue.

Example: two bullets `Shared helpers. One.` and `Shared helpers. Two.` both have the title `Shared helpers`; they now file two tickets.

### 3. Export refuses names that leave the folder (70)

The name rule moves out of `export_entry` into a pure function in `repo.rs`:

```rust
fn entry_name<'a>(name: Option<&'a str>, root: &str) -> Result<Option<&'a str>>
```

- `None` (not UTF-8) gives `GitError::BadInput(format!("a path under {root:?} is not UTF-8"))`, as today.
- `".git"` gives `Ok(None)`, which `export_entry` turns into `TreeWalkResult::Skip`.
- `""`, `"."`, `".."` and any name holding `/`, `\` or `\0` give `GitError::BadInput(format!("unsafe path component {name:?} under {root:?}"))`.
- Any other name gives `Ok(Some(name))`.

The tests call `entry_name` directly, because libgit2 refuses to build trees with most of these names.

### 4. Intake parents come from the intake issue (79)

In the ticket step, a job whose `intake_issue` is `Some(n)` takes its parents as `[IssueRef { number: n, repo: None }]` and does not parse the feature text. The `skip_intake` skip stays first. `issue_feature` does not change.

### 5. A sync stopped after its merge runs again (80)

In `sync_requested_by`, the early `UpToDate` return also needs the review to be finished:

```rust
let unfinished = matches!(job.stage, Stage::Review | Stage::NeedsHuman)
    && job.reviews.values().any(|s| *s != ReviewState::Pass);
if tip == head && !behind && !retry && !unfinished {
    return Ok(SyncOutcome::UpToDate);
}
```

- `unfinished` is true when the job is in review or parked and at least one review is not `Pass` (`Pending`, `Running`, `Changes`, `Skipped` or `Failed`).
- Then `whole_branch` is true (the stage is not `Approve`), so the ruling and the one-write reset run, followed by the checks and the whole-branch review, as for any other non-approved sync.
- An approved job, or a parked job whose reviews all passed, still gets `UpToDate`.

Example: sync 1 merges main and its check fails, so the job parks with pending reviews. The check is fixed, and sync 2 runs the checks and the code review on `base..head`.

### 6. Waivers are written before the approve (24)

- In `approve` (`commands.rs`), the waiver is recorded before the `update_job` that moves the stage to Approve. A failed write returns the error and leaves the stage unchanged.
- `record_waiver` on a retry (same `head` and `file` as the last, not superseded entry) replaces that entry's `reason` and `at` and writes the file. `tickets`, `blockers` and `round` stay as they are. A retry with the same reason still writes the same content.

## Error handling and edge cases

| Case | Result |
|---|---|
| ``see `#42` `` | no ref |
| ```` ```\n#42\n``` ```` | no ref |
| unclosed ```` ``` ```` then `#42` on a later line | no ref |
| ``a ` b #7`` (lone backtick) | `[7]` |
| `` `o/r#5` `` or a URL in backticks | no ref |
| `#7` outside code | `[7]`, as today |
| two bullets with one title | two tickets, two numbers in the parent comment |
| old `tickets.json` with title-only entries | those items are not created again |
| pending item whose `find_created` number is already recorded for another bullet | created, not adopted |
| tree entry `a/b`, `a\b`, `a\0b`, `..` | `BadInput` with `unsafe path component` |
| tree entry `.git` | skipped |
| intake job with a URL in its title | only `intake_issue` is a parent |
| sync again after a sync that parked with pending reviews | checks and whole-branch review run |
| sync again on an approved, up-to-date job | `UpToDate`, as today |
| sync again on a parked job whose reviews all passed | `UpToDate`, as today |
| non-intake job | parents from the feature text, as today |
| waiver write fails | error, job stays in needs-human |
| approve retry with a new reason | `waivers.json` holds the new reason, still one entry |

## Testing

`crates/factory-core/src/commitmsg.rs` `mod tests`:
- `refs_skip_inline_code_spans`, `refs_skip_fenced_blocks`, `refs_skip_an_unclosed_fence`, `a_lone_backtick_does_not_hide_a_ref`, `urls_and_shorthand_in_code_are_not_refs`, `parse_issues_skips_code_too`.

`crates/factory-engine/tests/tickets.rs`:
- `a_ref_inside_a_code_block_is_not_closed`, `two_follow_ups_with_the_same_title_file_two_tickets`, `an_old_marker_without_keys_is_still_honoured`, `a_pending_match_held_by_another_bullet_is_not_adopted`, `an_intake_job_closes_only_its_intake_issue`.

`crates/factory-git/src/repo.rs` `mod tests` (new):
- `entry_name_refuses_a_non_utf8_name`, `entry_name_skips_dot_git`, `entry_name_refuses_unsafe_names`, `entry_name_accepts_a_plain_name`.

`crates/factory-engine/tests/sync_review.rs`:
- `a_sync_stopped_after_its_merge_reviews_the_whole_branch_on_rerun`, `an_up_to_date_approved_job_still_gets_up_to_date`.

`crates/factory-engine/tests/waivers.rs`:
- `a_retry_with_a_new_reason_updates_the_waiver`.

`crates/factory-cli/tests/new_to_merged.rs`:
- `a_failed_waiver_write_leaves_the_job_in_needs_human`.

Commands:

```bash
cargo test -j4 -p factory-core commitmsg
cargo test -j4 -p factory-git
cargo test -j4 -p factory-engine --test tickets
cargo test -j4 -p factory-engine --test sync_review
cargo test -j4 -p factory-engine --test waivers
cargo test -j4 -p factory-cli --test new_to_merged
```

## Files changed

- `crates/factory-core/src/commitmsg.rs`: `strip_code`, its use in `plain_refs` and `shorthand_refs`, tests.
- `crates/factory-engine/src/tickets.rs`: `Created`, the marker load and save, the match rule, the intake parent rule.
- `crates/factory-git/src/repo.rs`: `entry_name` and its tests.
- `crates/factory-engine/src/sync.rs`: the `unfinished` condition on the `UpToDate` return.
- `crates/factory-engine/src/waivers.rs`: the retry update.
- `crates/factory-cli/src/commands.rs`: the waiver-first order in `approve`.
- Tests listed above.

## Constraints

- Cargo commands are per crate with `-j4`, never `--workspace` or `--all`.
- `tickets.json` and `waivers.json` stay backward compatible: new fields are optional and old files load.
- The ticket step never changes a merge result.
- Never shell out to `git`. Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- No new crate dependency.

## Acceptance criteria

- A ref inside an inline code span or a fence is not returned by `parse_issue_refs` or `parse_issues`, for `#N`, `owner/repo#N` and URLs.
- A feature text with `` `#42` `` makes the ticket step comment on and close nothing for 42.
- Two follow-ups with the same title file two tickets, and the parent comment lists both numbers.
- A `tickets.json` without `key` fields still stops repeat creates.
- `entry_name` refuses `""`, `.`, `..` and names with `/`, `\` or `\0`, skips `.git` and refuses a non-UTF-8 name.
- An intake job's ticket step treats only `intake_issue` as a parent.
- A requested sync on a job in review or needs-human with any review not `Pass` runs the checks and the whole-branch review instead of returning `UpToDate`; an approved up-to-date job still gets `UpToDate`.
- A failed waiver write leaves the job in needs-human.
- A waiver retry with a new reason stores the new reason in the one entry.
- All listed tests and the existing tests of the touched crates pass.

## Out of scope

- Telling `#1 priority` apart from a real reference.
- Changing the waiver ticket flow after merge.

## Decisions

- Which refs skip code? Options: all ref kinds, only `#N`, only fences. Chosen: all ref kinds, by stripping code once before every parser. Decided by: user
- How are same-titled follow-ups kept apart? Options: key by bullet text with an optional field, refuse duplicate titles, number the titles. Chosen: key by bullet text, old title-only entries still match. Decided by: user
- Where do an intake job's parents come from? Options: only `intake_issue`, strip URLs from the title, both. Chosen: only `intake_issue`. Decided by: user
- What order do approve and the waiver write take? Options: write the waiver first, roll back the stage on error. Chosen: write the waiver first. Decided by: user
- What does a waiver retry with a new reason do? Options: update reason and time, refuse, keep the first reason. Chosen: update reason and time. Decided by: user
- Fix every open item of issues 57, 70, 79, 80 and 24 in this spec, one job, no follow-up tickets. Decided by: user
- A pending item whose found number belongs to another bullet is created rather than adopted, accepting a rare duplicate after a crash over a wrong adoption. Decided by: call
- The export name rule becomes a pure function so the unreachable tree names can be tested. Decided by: call
- What does a second `factory sync` do after a sync that merged and then stopped? Options: rerun when reviews are unfinished, keep `UpToDate` and test `retry`. Chosen: rerun when reviews are unfinished. Decided by: user
