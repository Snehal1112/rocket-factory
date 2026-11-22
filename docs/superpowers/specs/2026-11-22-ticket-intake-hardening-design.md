# Ticket step, intake export, sync and waiver leftovers

**Commit:** fix(tickets): skip code refs and keep waivers

## Goal

Finish the leftovers that issue 132 lists for the ticket step, intake export, close-with-fixes, sync and waivers, so no `gh` call closes or files the wrong issue, an export never writes outside its folder, and a waiver is never lost.

## Overview

Issue 132 keeps the open parts of issues 57, 70, 79, 80 and 24. A survey of main at `5da3514` confirmed the items below are still open. Issue 57 C (same-titled follow-ups) is already fixed by `unique_titles` (`crates/factory-engine/src/followups.rs:70-93`, commit `dd539d0`) and its test `two_follow_ups_with_the_same_title_file_two_tickets` (`crates/factory-engine/tests/tickets.rs:823`), so it is not part of this spec.

- **57 A:** issue refs inside inline code or a fence still count. A feature text with `` `#42` `` makes the ticket step comment on #42 and close it.
- **70:** `export_entry` refuses only `""`, `.` and `..`, so a tree entry named `a/b` or holding `\0` reaches `dest.join`. The `.git` skip, the non-UTF-8 refusal and the unsafe-name refusal have no tests.
- **79:** an intake job's feature text carries the issue title. When intake files the follow-ups (`file_follow_ups`, called from `crates/factory-engine/src/intake.rs:279` with `skip_intake = false`), a GitHub issue URL in the title still parses as a parent. The follow-ups then copy that issue's labels, cite it in the `from the follow-ups of` footer, and are skipped when the URL names another repository. (`factory tickets` and the merge hook skip intake jobs, so they never close one.)
- **80:** a requested sync that merged main and then stopped (for example on a failing check) parks the job. Running `factory sync` again returns `UpToDate`, because the tip equals the head and the branch is not behind, so nothing is checked or reviewed. `factory retry` refuses it too ("has no stage to retry at"), because the park has kind `other` and no resume stage. No test covers this.
- **24:** `approve --waive` moves the job to Approve before it writes the waiver, so a failed write leaves an approved job with no waiver. A retry with a new reason keeps the old reason in `waivers.json`, while the ledger records the new one.

The fix-design for 57 A is the one issue 57 proposed. Live checks, docs and the other areas of issue 132 are separate specs.

## Current state

| Piece | Where | Today |
|---|---|---|
| `#N` refs | `crates/factory-core/src/commitmsg.rs:102-119` `hash_refs` | Reads every `#` after a non-word char, in code too. |
| `owner/repo#N` refs | `commitmsg.rs:121-159` `shorthand_refs` | Same, no code skip. |
| URL refs | `commitmsg.rs:161-183` `url_refs` | Same, no code skip. |
| Entry points | `commitmsg.rs:185-210` `plain_refs`, `parse_issue_refs`, `parse_issues` | `parse_issue_refs` feeds the ticket step (`crates/factory-engine/src/tickets.rs:243`) and `title_subject` (`commitmsg.rs:631`, which drops ref words from a job subject); `parse_issues` feeds commit footers (`one_job.rs:126`, `runloop.rs:533,579,814`). |
| Fence helper | `commitmsg.rs:285-295` `unfenced_lines` | Drops lines inside ``` fences, used for builder replies. |
| Follow-ups | `crates/factory-engine/src/followups.rs:70-93` `unique_titles` | A repeated title gets a ` (N)` suffix, so same-titled bullets already file separate tickets. |
| Export | `crates/factory-git/src/repo.rs:297-330` `export_entry` | Non-UTF-8 name is `BadInput`, `.git` is skipped, `""`/`.`/`..` are `BadInput`. |
| Intake feature | `crates/factory-engine/src/intake.rs:545-556` `issue_feature` | Title without `#`, then `, #N`. `mark_series` (`intake.rs:560-581`) sets `intake_issue` and `issues = [N]`. |
| Ticket parents | `tickets.rs:216-262` `stage_follow_ups` | `tickets_step` passes `skip_intake = true` (`tickets.rs:393`), so it stops on an intake job; `file_follow_ups` passes `false` (`tickets.rs:182`). Parents come from `parse_issue_refs(&job.feature)`. |
| Sync | `crates/factory-engine/src/sync.rs:53-125` `sync_requested`, `sync_requested_by` | Returns `UpToDate` when `tip == head && !behind && !retry` (around line 95). Otherwise a sync that is not an approved head resets reviews and the floor in one write, merges main when behind, then reviews the whole branch. |
| Sync test | `crates/factory-engine/tests/sync_review.rs:162` | `a_second_sync_after_a_stopped_first_one_still_reviews_the_whole_branch` covers a stop after the reset, before the merge, by setting that state by hand. |
| Approve | `crates/factory-cli/src/commands.rs:748-840` `approve` | `update_job` to Approve (around line 797), then `record_waiver(...)?`. |
| Waiver | `crates/factory-engine/src/waivers.rs:69-79` `record_waiver` | A retry with the same `head` and `file` as the last, not superseded entry returns `Ok(())` and changes nothing. |

## Non-goals

- No change to the `Tracker` trait, `GhTracker`, the board JSON or the dashboard.
- No change to how `factory tickets --dry-run` prints, except the item names it already prints.
- The other parts of issue 57 (A boundary rules, B pending marker, D lease, E body cut) are done and stay as they are.
- Issue 132's other areas (job creation, harness and CI, docs, live checks) are not in this spec.

## Design

```
feature text --strip_code--> plain text --> #N, owner/repo#N, URL refs     (57 A)
tree entry name --> UTF-8? .git? safe? --> write or skip or BadInput        (70)
intake follow-ups --> parents = [intake_issue], feature refs ignored       (79)
sync parks after reset --> resume Rereview --> second sync or retry --> whole-branch review   (80)
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

### 2. Export refuses names that leave the folder (70)

The name rule moves out of `export_entry` into a pure function in `repo.rs`:

```rust
fn entry_name<'a>(name: Option<&'a str>, root: &str) -> Result<Option<&'a str>>
```

- `None` (not UTF-8) gives `GitError::BadInput(format!("a path under {root:?} is not UTF-8"))`, as today.
- `".git"` in any ASCII case (`.GIT`, `.Git`) gives `Ok(None)`, which `export_entry` turns into `TreeWalkResult::Skip`. macOS filesystems ignore case by default, so `.GIT` would otherwise land in `<dest>/.git`.
- `""`, `"."`, `".."` and any name holding `/`, `\` or `\0` give `GitError::BadInput(format!("unsafe path component {name:?} under {root:?}"))`.
- Any other name gives `Ok(Some(name))`.

The tests call `entry_name` directly, because libgit2 refuses to build trees with most of these names.

### 3. Intake parents come from the intake issue (79)

In `stage_follow_ups`, after the `skip_intake` check, a job whose `intake_issue` is `Some(n)` takes its parents as `[IssueRef { number: n, repo: None }]` and does not parse the feature text. This changes only the `file_follow_ups` path that intake uses: labels come from #n, the footer cites only #n, and a URL to another repository in the title no longer skips the follow-ups. `issue_feature` does not change.

### 4. A sync stopped after its merge runs again (80)

Two changes in `sync_requested_by` (`crates/factory-engine/src/sync.rs`).

**The park records where to resume.** Where a requested sync parks (`if let SyncOutcome::Parked(reason, kind)`), the `park_with_parts` call stays. Then, when `whole_branch` is true and the job (read again after the park) still has a `Pending`, `Running` or `Failed` review, the sync calls a new helper in `crates/factory-engine/src/merge.rs`. A sync park after a review verdict, such as "sync review asked for changes", keeps no resume stage, so a plain rework reuses the review's findings, as today:

```rust
pub(crate) fn set_park_resume(driver: &Driver, id: &str, resume: ResumeAt) -> Result<(), EngineError>
```

- It sets `job.park_resume = Some(resume)` on a job in needs-human, and rewrites `park.json` so its `resume` is the same and its `next` is recomputed with `next_with_resume`. The record's kind, reason, round and evidence stay as they are. A missing record is left missing; the board field is the source of truth. A record that cannot be written is logged, as `write_record` does today.
- It runs last, so it wins over a park that the checks stage wrote earlier in the same run. `park_with_parts` keeps such a park's kind, evidence and resume stage (`merge.rs:66-74`); checks that fail with no fix rounds left park as `checks-failed`, whose own resume stage is `None`.
- A `checks-failed` sync park then offers both `factory retry` (rerun the checks and the review, for a flaky or environment failure) and `factory rework` (send it to the builder), because of the rule in section 6.

**The early `UpToDate` return skips unfinished sync parks:**

```rust
let no_verdict = job.reviews.values().any(|s| {
    matches!(s, ReviewState::Pending | ReviewState::Running | ReviewState::Failed)
});
let unfinished = no_verdict
    && (job.stage == Stage::Review
        || (job.stage == Stage::NeedsHuman && job.park_resume == Some(ResumeAt::Rereview)));
if tip == head && !behind && !retry && !unfinished {
    return Ok(SyncOutcome::UpToDate);
}
```

- `no_verdict`: a review is `Pending`, `Running` or `Failed`. `Pass`, `Changes` and `Skipped` are verdicts.
- A job parked during build (blocked builder, failed checks) has a `Pending` review but resumes at `Build`, so it still gets `UpToDate` and keeps its resume stage.
- After a sync park, `factory retry` resumes at `Rereview` too.
- Then `whole_branch` is true (the stage is not `Approve`), so the ruling and the one-write reset run, followed by the checks and the whole-branch review.
- The sync review always adds a pending `security` review (`sync.rs:218-223`), so a test needs the security agent file (`with_security_agent` in `sync_review.rs`).

Example: sync 1 merges main and its check fails, so the job parks with resume `Rereview`. The check is fixed, and sync 2 (or `factory retry`) runs the checks and the code review on `base..head`.

### 5. Waivers are written before the approve (24)

- In `approve` (`commands.rs`), the waiver is recorded before the `update_job` that moves the stage to Approve. A failed write returns the error and leaves the stage unchanged.
- `record_waiver` on a retry (same `head` and `file` as the last, not superseded entry) replaces that entry's `reason` and `at` and writes the file. `tickets`, `blockers` and `round` stay as they are. A retry with the same reason still writes the same content.

### 6. One rule for the parks that keep rework

Which parks with a resume stage still accept a plain `factory rework` is decided in three places today, each with its own list: `next_with_resume` (`crates/factory-core/src/park.rs:43-66`, `MachineWaiting | BuilderStopped`), the rework refusal in `crates/factory-cli/src/commands.rs:872-885` (`"machine-waiting" | "builder-stopped"`), and the dashboard buttons in `dashboard/static/projects.js:128-137`. `Job::rework_hint` (`crates/factory-core/src/board.rs:358-370`) ignores the kind.

- New in `crates/factory-core/src/park.rs`:

```rust
impl ParkKind {
    /// A park the builder can fix, so a plain rework stays open even with a resume stage.
    pub fn keeps_rework(self) -> bool // MachineWaiting | BuilderStopped | ChecksFailed
    /// The kind for its stable name, the inverse of `as_str`.
    pub fn from_name(name: &str) -> Option<ParkKind>
}
```

- `next_with_resume` uses `kind.keeps_rework()`.
- The rework refusal in `commands.rs` uses `job.park_kind.as_deref().and_then(ParkKind::from_name).is_some_and(ParkKind::keeps_rework)`.
- `Job::rework_hint` returns `"rework"` when the job's park kind keeps rework.
- `board.js` card chips: `retryRework` is true for a build resume that is not `series-waiting`, or for a `park_kind` that keeps rework; `retryApprove` returns no approve chip for `checks-failed`. A comment names `ParkKind::keeps_rework`.
- `projects.js` `baseActions`: a needs-human job with a resume stage whose `park_kind` is `machine-waiting`, `builder-stopped` or `checks-failed` shows `["retry", "rework", "cancel"]`, the same list as a build resume today. A comment names `ParkKind::keeps_rework` as the rule it mirrors. The `series-waiting` and other rows stay as they are.

## Error handling and edge cases

| Case | Result |
|---|---|
| ``see `#42` `` | no ref |
| ```` ```\n#42\n``` ```` | no ref |
| unclosed ```` ``` ```` then `#42` on a later line | no ref |
| ``a ` b #7`` (lone backtick) | `[7]` |
| `` `o/r#5` `` or a URL in backticks | no ref |
| `#7` outside code | `[7]`, as today |
| tree entry `a/b`, `a\b`, `a\0b`, `..` | `BadInput` with `unsafe path component` |
| tree entry `.git`, `.GIT` or `.Git` | skipped |
| intake follow-ups with a URL to #5 in the title | labels from #76 only, footer cites #76 only |
| intake follow-ups with a URL to another repo in the title | filed, not skipped |
| `factory tickets` on an intake job | skipped, as today |
| sync again after a sync that parked with pending reviews | checks and whole-branch review run |
| sync again on an approved, up-to-date job | `UpToDate`, as today |
| sync again on a parked job whose reviews all have verdicts (`Pass`, `Changes`, `Skipped`) | `UpToDate`, as today |
| sync on a job parked during build (resume `Build`, review `Pending`) | `UpToDate`, resume stage kept |
| `factory retry` after a sync park | resumes at re-review |
| sync whose checks fail with no fix rounds left (the checks stage parks first) | resume `Rereview` on the job and in `park.json`; `next` offers retry and rework; the next sync or retry reviews the whole branch |
| plain `factory rework` on a `checks-failed` park with a resume stage | accepted |
| sync review asked for changes | no resume stage, plain rework accepted, as today |
| plain `factory rework` on a `review-failed` park | refused with the retry hint, as today |
| dashboard card of a `checks-failed` park with resume `rereview` | retry, rework, cancel |
| non-intake job | parents from the feature text, as today |
| waiver write fails | error, job stays in needs-human |
| approve retry with a new reason | `waivers.json` holds the new reason, still one entry |

## Testing

`crates/factory-core/src/commitmsg.rs` `mod tests`:
- `refs_skip_inline_code_spans`, `refs_skip_fenced_blocks`, `refs_skip_an_unclosed_fence`, `a_lone_backtick_does_not_hide_a_ref`, `urls_and_shorthand_in_code_are_not_refs`, `parse_issues_skips_code_too`.

`crates/factory-engine/tests/tickets.rs`:
- `a_ref_inside_inline_code_is_not_closed`, `intake_follow_ups_take_only_the_intake_issue_as_parent`.

`crates/factory-core/src/park.rs` `mod tests`:
- `keeps_rework_covers_builder_fixable_parks`, `from_name_reads_every_as_str_name`, `a_checks_failed_park_with_a_resume_offers_retry_and_rework`.

`crates/factory-core/src/board.rs` `mod tests`:
- `rework_hint_is_rework_for_a_checks_failed_park`.

`crates/factory-cli/tests/retry_cmd.rs`:
- `plain_rework_is_allowed_on_a_checks_failed_park` (next to `plain_rework_is_allowed_on_machine_and_stopped_parks`, using its `parked_as` helper).

`dashboard` tests of `projects.js` actions:
- `jobActions offers rework on a checks-failed park with a resume` in `dashboard/tests/projects.test.js`, and `a checks-failed card with a resume offers retry and rework chips` in `dashboard/tests/board.test.js`, run with `node --test dashboard/tests/*.test.js`.

`crates/factory-git/src/repo.rs` `mod tests` (new):
- `entry_name_refuses_a_non_utf8_name`, `entry_name_skips_dot_git`, `entry_name_refuses_unsafe_names`, `entry_name_accepts_a_plain_name`.

`crates/factory-engine/tests/sync_review.rs`:
- `a_sync_stopped_after_its_merge_reviews_the_whole_branch_on_rerun`, `a_parked_job_with_only_verdicts_stays_up_to_date`, `a_build_parked_job_stays_up_to_date_on_sync`, `a_sync_park_resumes_at_rereview`, `a_sync_review_with_changes_keeps_plain_rework`, `a_sync_parked_by_its_checks_resumes_at_rereview` (also asserts `park.json` has resume `rereview` and its `next` holds both `factory retry` and `factory rework`).
- Existing test change: `an_up_to_date_job_with_tip_equal_head_changes_nothing_in_every_stage` sets the code review to `Pass` before each stage, because a `Pending` review in Review, or in needs-human with resume `Rereview`, now reruns.
- `sync_requested_on_an_up_to_date_job_changes_nothing` (existing) keeps pinning the approved, up-to-date case.

`crates/factory-engine/tests/waivers.rs`:
- `a_retry_with_a_new_reason_updates_the_waiver`.

`crates/factory-cli/tests/new_to_merged.rs`:
- `a_failed_waiver_write_leaves_the_job_in_needs_human`.

Commands:

```bash
cargo test -j4 -p factory-core commitmsg
cargo test -j4 -p factory-git
cargo test -j4 -p factory-engine --test tickets
cargo test -j4 -p factory-engine --test intake
cargo test -j4 -p factory-engine --test sync_review
cargo test -j4 -p factory-engine --test retry
cargo test -j4 -p factory-cli --test sync_cmd
cargo test -j4 -p factory-engine --test waivers
cargo test -j4 -p factory-cli --test new_to_merged
cargo test -j4 -p factory-cli --test operator_events
cargo test -j4 -p factory-cli --test retry_cmd
cargo test -j4 -p factory-core park
node --test dashboard/tests/*.test.js
```

## Files changed

- `crates/factory-core/src/commitmsg.rs`: `strip_code`, its use in `plain_refs` and `shorthand_refs`, tests.
- `crates/factory-engine/src/tickets.rs`: the intake parent rule.
- `crates/factory-git/src/repo.rs`: `entry_name` and its tests.
- `crates/factory-engine/src/sync.rs`: the `set_park_resume` call on a sync park and the `unfinished` condition on the `UpToDate` return.
- `crates/factory-engine/src/merge.rs`: `set_park_resume`.
- `crates/factory-core/src/park.rs`: `keeps_rework`, `from_name`, their use in `next_with_resume`.
- `crates/factory-core/src/board.rs`: `rework_hint` reads `keeps_rework`.
- `crates/factory-cli/src/commands.rs`: the rework refusal reads `keeps_rework`.
- `dashboard/static/projects.js` and `dashboard/static/board.js`: the card buttons and chips follow the same rule.
- `crates/factory-engine/src/waivers.rs`: the retry update.
- `crates/factory-cli/src/commands.rs`: the waiver-first order in `approve`.
- Tests listed above.

## Constraints

- Cargo commands are per crate with `-j4`, never `--workspace` or `--all`.
- `tickets.json` and `waivers.json` keep their shapes.
- The ticket step never changes a merge result.
- Never shell out to `git`. Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- No new crate dependency.

## Acceptance criteria

- A ref inside an inline code span or a fence is not returned by `parse_issue_refs` or `parse_issues`, for `#N`, `owner/repo#N` and URLs.
- A feature text with `` `#42` `` makes the ticket step comment on and close nothing for 42.
- `entry_name` refuses `""`, `.`, `..` and names with `/`, `\` or `\0`, skips `.git` and refuses a non-UTF-8 name.
- When intake files follow-ups, only `intake_issue` is a parent: labels, footer and the other-repo skip ignore issue refs in the title.
- A requested sync that parks after its whole-branch reset leaves the job and `park.json` with resume `Rereview` and a recomputed `next`, also when the checks stage parked it first in the same run.
- `ParkKind::keeps_rework` is the one rule for parks that keep a plain rework; the CLI refusal, `next_with_resume`, `rework_hint` and the dashboard card follow it, and a `checks-failed` park keeps rework.
- `entry_name` skips `.git` in any ASCII case.
- A requested sync on a job in review, or parked with resume `Rereview`, that has a `Pending`, `Running` or `Failed` review runs the checks and the whole-branch review instead of returning `UpToDate`.
- An approved up-to-date job, a parked job whose reviews all have verdicts, and a job parked during build still get `UpToDate`.
- A failed waiver write leaves the job in needs-human.
- A waiver retry with a new reason stores the new reason in the one entry.
- All listed tests and the existing tests of the touched crates pass.

## Out of scope

- Double-backtick code spans, `~~~` fences, and a line with two stray backticks around a ref: `strip_code` handles single-backtick spans and ``` fences only.
- A failed stage change after the waiver write leaves an unsuperseded waiver; `rework` supersedes it, as today.

- Telling `#1 priority` apart from a real reference.
- Changing the waiver ticket flow after merge.

## Decisions

- Which refs skip code? Options: all ref kinds, only `#N`, only fences. Chosen: all ref kinds, by stripping code once before every parser. Decided by: user
- Issue 57 C turned out to be fixed already by `unique_titles`. Options: drop it, key the marker by bullet text anyway. Chosen: drop it. Decided by: user
- Where do an intake job's parents come from? Options: only `intake_issue`, strip URLs from the title, both. Chosen: only `intake_issue`. Decided by: user
- What order do approve and the waiver write take? Options: write the waiver first, roll back the stage on error. Chosen: write the waiver first. Decided by: user
- What does a waiver retry with a new reason do? Options: update reason and time, refuse, keep the first reason. Chosen: update reason and time. Decided by: user
- Fix every open item of issues 57, 70, 79, 80 and 24 in this spec, one job, no follow-up tickets. Decided by: user
- Which reviews make a requested sync rerun? Options: only `Pending`, `Running` or `Failed`; the same but never in build; any review not `Pass`. Chosen: only `Pending`, `Running` or `Failed`. Decided by: user
- How is a stopped sync told apart from a job parked during build, which also has a pending review? Options: mark the sync park with resume `Rereview` and rerun only those, fix only `retry`. Chosen: mark the sync park; both `factory sync` and `factory retry` then finish it. Decided by: user
- How does the sync's `Rereview` win over a park the checks stage wrote first? Options: set `park_resume` after the park, pass a resume override into the checks stage. Chosen: set it after the park, leaving kind, reason and evidence alone. Decided by: user
- How is a `checks-failed` sync park handled once it has a resume stage? Options: leave `checks-failed` without `Rereview`, give every sync park `Rereview` and keep rework for `checks-failed`. Chosen: the long-term rule: every sync park gets `Rereview` through one helper that also rewrites `park.json`, and one `keeps_rework` rule, which includes `checks-failed`, drives the CLI, the hints and the dashboard. Decided by: user
- Should the export `.git` skip ignore case? Chosen: yes, as part of the long-term fix. Decided by: user
- A sync park gets `Rereview` only while a review has no verdict; a park after "sync review asked for changes" keeps no resume stage, so plain rework reuses the findings. Decided by: user
- The export name rule becomes a pure function so the unreachable tree names can be tested. Decided by: call
- What does a second `factory sync` do after a sync that merged and then stopped? Options: rerun when reviews are unfinished, keep `UpToDate` and test `retry`. Chosen: rerun when reviews are unfinished. Decided by: user
