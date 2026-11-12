# Ticket step safety design

## Goal

Make the post-merge ticket step and `factory tickets` safe to run unattended: no unrelated issue is closed, no follow-up issue is filed twice, and a long follow-up cannot block the step for good.

## Overview

GitHub issue #57 reports five defects in the ticket step (`crates/factory-engine/src/tickets.rs`). A code read in this session confirmed all five and narrowed one:

- A. `hash_refs` (`crates/factory-core/src/commitmsg.rs:86`) reads `#1a2b3c` as issue 1, and drops `owner/repo#5`, so a foreign shorthand ref neither skips the step nor counts. The refs come from the job's `feature` text (`tickets.rs:129`), one typed line, so code-block and hex-colour inputs are unlikely and are not handled here.
- B. The marker entry for a created issue is written only after `tracker.create` returns `Ok` (`tickets.rs:197-200`). A crash, the 60 s `gh` timeout, an unreadable `gh` reply or a failed marker save leaves no entry, and the next run files the issue again.
- C. The marker is keyed on the follow-up title (`tickets.rs:179`). Two bullets with the same first sentence share one entry, so the second is never filed and the comment reads `filed as #200, #200`.
- D. `factory tickets` takes no lease (`crates/factory-cli/src/commands.rs:1062`), so two runs on one job can both file every follow-up.
- E. `create_args` (`tracker.rs:57`) passes the body unchecked, so a body over GitHub's 65,536 characters fails on every re-run and the parent is never closed.

## Current state

| Piece | Today | Gap |
|---|---|---|
| `hash_refs` | `#` at the start or after a non-alphanumeric, non-`_` char, then the leading digits (`commitmsg.rs:86-107`) | No check of the char after the digits; `owner/repo#N` skipped |
| `parse_issue_refs` | `#N` plus GitHub URLs, sorted, de-duplicated (`commitmsg.rs:134`) | A shorthand repo never reaches the foreign-repo skip at `tickets.rs:141-150` |
| `Marker` | `created: Vec<(title, number)>`, `commented`, `closed`, saved after each call (`tickets.rs:45-100`) | No record of an attempt in flight |
| `Tracker` | six methods, `GhTracker` is the `gh` adapter, `Arc<T>` forwards (`tracker.rs:12-42`) | No way to ask whether an issue was already created |
| `follow_ups` | bullets under `## Follow-ups`, title is the first sentence cut at 100 chars (`followups.rs:26-60`) | Equal titles collide |
| `tickets` command | refuses a live lease of another owner, takes none (`commands.rs:1062-1076`) | Two runs can overlap |
| `Driver` leases | `hold_project` and `renew_lease` take the project lease too (`compat.rs:324-377`); `release_lease` drops the job lease (`compat.rs:403`) | No job-lease-only helper |

The merge hook (`merge.rs`) calls `tickets_step` directly while the driver holds the job lease, so D concerns the CLI only.

## Non-goals

- Skipping `#N` inside code spans, code fences or hex colours. The feature text is one typed line.
- Catching a panic in a post-merge step, and the missing ledger line on an `Err`. Issue #58 owns both.
- The same create-then-save pattern in `waivers.rs` (see Follow-ups).
- Any change to `Cargo.toml` or a new crate dependency.

## Design

Flow of one real run, per follow-up: skip when created, else adopt a pending issue or save a pending entry, create, record the number, then comment on and close the parents as today.

### 1. Narrow ref parsing (`factory-core`)

- A `#N` ref counts only when the char after the digits is not alphanumeric or `_`. The end of the text counts as a boundary. `#1a2b3c` gives no ref. `#12.`, `#12)` and `#12,` give 12.
- Shorthand `owner/repo#N` becomes `IssueRef { number: N, repo: Some("owner/repo") }`, the same shape a URL gives. The text before the `#` must be `<owner>/<repo>` where both parts pass `is_repo_name`, and the char before the owner is the start of the text or a char that is not a repo-name char and not `/`. So `x.com/a/b#12` is not a ref.
- `parse_issues` keeps its behaviour. It is built from the `#N` and URL parsers only and leaves shorthand refs out, so a shorthand ref to another repository never reaches `job.issues` or a commit footer. Its callers (`runloop.rs`, `one_job.rs`) share the new boundary rule.

### 2. Pending entries and recovery (`factory-engine`)

New method on the port, forwarded by the `Arc<T>` impl:

```rust
/// The number of an issue this job already filed, or None. Reads only.
fn find_created(&self, title: &str, footer: &str) -> Result<Option<u32>, String>;
```

`GhTracker` runs `gh issue list --state all --limit 100 --json number,title,body`. `parse_found(json, title, footer)` keeps issues whose title equals `title` exactly and whose body contains `footer`, and returns the lowest number. `footer` is `Filed by factory job {id} ({slug})`, the start of the line `tickets_step` already puts in every body.

`Marker` gains `pending: Vec<String>` (titles), saved as `"pending": ["title"]`. A file without the key loads as empty, and a file with the key stays valid for old readers because they ignore it. Per follow-up in a real run:

1. If `created` holds the title, skip it.
2. If `pending` holds the title, call `find_created`. `Ok(Some(n))` adopts it: push `(title, n)` to `created`, drop the pending title, save, add the line `adopted #n: title`. `Ok(None)` goes on to step 3. `Err(e)` reports `failed: find `title`: e` and stops the step.
3. Push the title to `pending` and save. A save error reports `failed` and stops before any create.
4. Call `create`. On `Ok(n)`: drop the pending title, push `(title, n)`, save. A save error reports `failed: created #n `title` but could not record it: e` and stops, so the number reaches the report and the ledger line.
5. On a `create` error, leave the pending title and report `failed` as today.

A dry run never writes. For a pending title it adds the line `would look for an existing issue, then create: title` and makes no `find_created` call.

### 3. Unique follow-up titles (`factory-engine`)

After the bullets are read, `follow_ups` makes titles unique in order: the second bullet with a title gets ` (2)`, the third ` (3)`. The base title is cut so the final title stays within 100 characters. A suffix is never reused: if `X (2)` is already a title, the next free number is taken. The marker shape does not change.

### 4. Job lease for a real `factory tickets` run

```rust
pub fn hold_job(&self, id: &str) -> Result<JobGuard<'_>, EngineError>;
```

It calls `acquire_lease` for the job with the driver's owner and TTL, and takes no project lease. Without a lease owner it returns a guard that does nothing, like `hold_project`. Dropping the guard drops the job lease and ignores a failed release, because a lease expires by itself. In `commands.rs` a real run calls `hold_job` first; a refusal is `CliError::Refused("job {id} is leased by {owner}")`. A dry run writes nothing, so it keeps today's read-only check of a live lease. The merge hook is unchanged.

### 5. Body cap

`fit_body(item_body: &str, footer_text: &str) -> String` in `tickets.rs` keeps the whole body when `item_body` plus the footer is at most 60,000 characters. Otherwise it cuts `item_body` on a char boundary so the total is at most 60,000 and adds the line `This text was cut to fit GitHub's limit.` before the footer. Length counts chars, not bytes.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| `#1a2b3c`, `#123abc` in the feature | No ref; the step skips with "no issue references" if nothing else is cited |
| `other/repo#5` in the feature | Ref with repo `other/repo`; the step skips with "belongs to other/repo" |
| `x.com/a/b#12` | Not a ref |
| `tickets.json` without `pending` | Loads with an empty list |
| Pending title and `find_created` finds nothing | Creates once |
| Pending title and `find_created` errors | `failed`, nothing created, pending title kept |
| Two issues match title and footer | The lowest number is adopted |
| Marker save fails after create | `failed` with the issue number in the report line |
| Create times out after `gh` made the issue | Pending title stays; the next run adopts it |
| Three bullets with one title | Titles `X`, `X (2)`, `X (3)`, one ticket each |
| A bullet titled `X (2)` next to two bullets titled `X` | The second `X` takes `X (3)` |
| Body of 70,000 chars | Filed cut to 60,000 with the cut line; the parent is closed |
| Second `factory tickets` on a job while one runs | Refused: `job N is leased by <owner>` |
| `factory tickets` while another job builds | Runs; only the job lease is taken |
| Dry run during a live lease of another owner | Refused, as today |

## Testing

`cargo test -j4 -p factory-core`:

- `commitmsg.rs`: `hash_ref_needs_a_boundary_after_the_digits`, `hash_ref_may_end_at_punctuation`, `owner_repo_shorthand_carries_the_repo`, `shorthand_inside_a_longer_path_is_not_a_ref`, `parse_issues_ignores_a_shorthand_ref_to_another_repository`.

`cargo test -j4 -p factory-engine`:

- `tracker.rs` unit tests: `find_args_list_all_states`, `parse_found_picks_the_lowest_number_with_title_and_footer`, `parse_found_ignores_a_title_without_the_footer`, `parse_found_rejects_invalid_json`.
- `followups.rs` unit tests: `repeated_titles_get_a_numbered_suffix`, `a_suffix_never_collides_with_another_title`, `a_suffixed_title_stays_within_100_chars`.
- `tests/tickets.rs` (fixtures `merged`, `tracker`, `SPEC_WITH_TWO`, `FakeTracker`): `skips_a_shorthand_ref_to_another_repository`, `a_pending_entry_adopts_the_existing_issue`, `a_pending_entry_with_no_match_creates_once`, `a_failed_find_stops_without_creating`, `a_failed_create_leaves_a_pending_entry`, `a_marker_save_failure_reports_the_created_number`, `an_old_marker_without_pending_is_honoured`, `two_follow_ups_with_the_same_title_file_two_tickets`, `an_over_long_body_is_cut_and_the_parent_closes`, `a_dry_run_with_a_pending_entry_makes_no_find_call`.
- `tests/common/mod.rs`: `FakeState` gets `found: HashMap<String, u32>`; `FakeTracker::find_created` records the call `find_created`, honours `fail_on("find_created")` and answers from `found`.
- `tests/lock_lease.rs`: `hold_job_refuses_another_owner_and_releases_on_drop`, `hold_job_takes_no_project_lease`.

`cargo test -j4 -p factory-cli --test tickets` (fixture `Fixture`, tracker `NoCalls`): `tickets_refuses_while_another_owner_holds_the_job_lease`, `tickets_releases_its_lease_after_a_run`, `tickets_dry_run_takes_no_lease`, `tickets_runs_while_the_project_is_busy`.

Every `Tracker` implementor in tests gets the new method. Run `cargo clippy -j4 -p factory-core -p factory-engine -p factory-cli`.

## Files changed

| File | Change |
|---|---|
| `crates/factory-core/src/commitmsg.rs` | Boundary rule, shorthand refs, tests |
| `crates/factory-engine/src/tracker.rs` | `find_created`, `find_args`, `parse_found`, tests |
| `crates/factory-engine/src/tickets.rs` | `pending` in `Marker`, recovery flow, `fit_body` |
| `crates/factory-engine/src/followups.rs` | Unique titles, tests |
| `crates/factory-engine/src/compat.rs` | `Driver::hold_job`, `JobGuard` |
| `crates/factory-cli/src/commands.rs` | `tickets` takes the job lease on a real run |
| `crates/factory-engine/tests/common/mod.rs` | `FakeState.found`, `FakeTracker::find_created` |
| `crates/factory-engine/tests/tickets.rs`, `tests/lock_lease.rs`, `crates/factory-cli/tests/tickets.rs` | New tests |
| `README.md`, `docs/reference/software-factory.md` | Ticket hygiene: pending recovery, job lease, cut body |

## Constraints

- The step never changes the merge result: failures go to the ledger, stderr and events.
- Only `git` may not be shelled out to. `gh` stays behind the `Tracker` port in `GhTracker`; tests use the fake.
- `tickets.json` stays readable by old files: the new key is optional.
- No `Cargo.toml` change; `**/Cargo.toml` is a risk path in `.claude/factory/config.toml`. No file above matches a risk path.
- Commands use `-j4`, never `--workspace` or `--all`.

## Acceptance criteria

- A feature text with `#1a2b3c` closes no issue; `#12.` and `#12)` still cite issue 12.
- `other/repo#5` in the feature makes the step skip with the "belongs to other/repo" line, and adds no issue to `job.issues` and no commit footer.
- After a create that failed in any of the four ways, a rerun files no second issue when the first exists.
- A failed marker save after a create shows `#N` in the report and the ledger line.
- Two follow-ups with the same first sentence give two tickets, and the parent comment lists two numbers.
- An existing `tickets.json` without `pending` still stops repeat creates.
- A second real `factory tickets <id>` while one runs is refused with `leased by`.
- `factory tickets` runs while another job holds the project lease.
- A follow-up body of 70,000 chars is filed cut and the parent is closed.
- A dry run makes no `find_created` call and writes nothing.
- `cargo test -j4 -p factory-core`, `-p factory-engine` and `-p factory-cli` pass, and clippy is clean.

## Out of scope

- Code-span and code-fence skipping in `hash_refs`.
- Panic handling and ledger lines on `Err` (#58).
- Search-index lag: recovery uses `gh issue list`, not `--search`.

## Follow-ups

- Apply the pending-entry recovery to `waivers.rs`, which files issues with the same create-then-save order (`waivers.rs:162`).
- Make the commit footer skip a ref that carries another repository, because `parse_issues` keeps the number of a foreign URL ref and a footer line `Closes: #N` could then name an issue of this repository.

## Decisions

- Which of the five defects in #57 does this job fix? Options: B, C, D, E plus narrow A; everything as written; only B and C. Chosen: B, C, D, E plus narrow A (digit boundary and the `owner/repo#N` shorthand, no code-span or code-fence skipping, because the feature text is one typed line). Decided by: user
- How does the step avoid a duplicate after a partial failure? Options: pending entry plus find on recovery; search before every create; report only. Chosen: save a pending entry before each create, and call a new `Tracker::find_created` only when a later run finds a pending entry. Decided by: user
- Two follow-ups share their first sentence. Options: make the title unique in `follow_ups`; key the marker on the full bullet text; fail the step. Chosen: `follow_ups` makes a repeated title unique with a ` (2)`, ` (3)` suffix, and the marker keeps its shape. Decided by: user
- `factory tickets` takes no lease, so two runs can both file. Options: take the job lease on a real run; a lock file in the job folder; leave it and document it. Chosen: a real run takes the job lease with `acquire_lease` and releases it on every path, a dry run takes nothing. Decided by: user
- A follow-up body is over GitHub's 65,536-character limit. Options: cut it and say so; fail with a message; leave it. Chosen: cut the body to 60,000 characters and add a closing line that says it was cut. Decided by: user
- Should `factory tickets` hold only the job lease or the project lease too? Options: job lease only; project and job lease via `renew_lease`. Chosen: job lease only, through a new `Driver::hold_job`, so tickets runs while another job builds. Decided by: user
- What counts as a ref boundary and a shorthand? Chosen: the char after the digits is not alphanumeric or `_`; the shorthand needs `<owner>/<repo>` with both parts valid and no `/` or repo-name char before the owner. Decided by: user
- What does recovery match, and what does a dry run do for a pending title? Chosen: an exact title plus the body line `Filed by factory job {id} ({slug})`, the lowest number wins, and a dry run only reports it would look. Decided by: user
- The spec review found that shorthand refs would reach `parse_issues` and so commit footers. Chosen: `parse_issues` is built from the `#N` and URL parsers only, so it keeps its behaviour. Decided by: user
- Does a dry run take the job lease? Chosen: no, it writes nothing and keeps the read-only live-lease check. Decided by: call
