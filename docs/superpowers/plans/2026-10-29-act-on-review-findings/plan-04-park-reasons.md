# Act On Review Findings, Plan 04: Park Reasons

> **Executor:** run with superpowers:subagent-driven-development: one fresh implementer per task, then a reviewer per task.

**Goal:** Every park records why it stopped, and the dashboard card shows a one-line summary of that evidence without opening the dialog.

**Architecture:** `park_with` and `merge::park` become the only two writers of park evidence, and neither ever writes an empty `parts` list. The merge step passes real evidence (dirty paths from `factory-git`, both shas, the merge error). Both writers also store a capped `park_summary` on the board, built from the first part, and `Job::set_stage` clears it with `reason`. The dashboard reads the field leniently and shows it on the card.

**Tech Stack:** Rust, git2 (through `factory-git`), `node --test` for the page.

**Spec:** `docs/superpowers/specs/2026-10-29-act-on-review-findings-design.md` (section 9)

**Depends on:** plan 01 (the Review part comes first, so a review park's summary names the verdict and the first blocker).

## Global Constraints

- Cargo per crate with `-j4`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`. Dashboard: `cargo test -j4 --manifest-path dashboard/Cargo.toml` and `node --test dashboard/tests/*.test.js`.
- Git stays in `factory-git` through git2; never shell out to git.
- `park_summary` is optional on the board, at most 160 characters, one line. The board carries no other evidence text.
- Writing evidence stays best effort: a failure to write `park.json` never stops the park.
- Main is never touched by the merge-step park.

## Review Focus

1. A job parked before this plan (no `park_summary`) still renders its reason on the card (`card_without_a_summary_shows_the_reason`, Task 2).
2. A job that leaves needs-human through approve, rework or cancel no longer carries the old summary (`leaving_needs_human_clears_the_summary`, Task 2).
3. Main busy with more than 50 tracked changes lists 50 paths and `(+N more)` (`busy_lists_at_most_fifty_paths`, Task 1).
4. A summary built from evidence with very long lines or control characters is one line of at most 160 characters (`summary_is_one_capped_line`, Task 2).
5. A second park of a job already parked by the sync step keeps the first kind and evidence, as `merge::park` does today (`an_earlier_park_keeps_its_evidence`, Task 1).

## File Structure

- Modify `crates/factory-git/src/state.rs`: `Preflight::Busy` gains the dirty paths.
- Modify `crates/factory-engine/src/park.rs`: the never-empty rule and the summary helper.
- Modify `crates/factory-engine/src/build.rs`: `NothingChanged` with no reply.
- Modify `crates/factory-engine/src/merge.rs`: `park_with_parts` and the evidence it passes.
- Modify `crates/factory-engine/src/runloop.rs`: evidence for the two run-loop parks.
- Modify `crates/factory-core/src/board.rs`: `Job.park_summary`, cleared in `set_stage`.
- Modify `dashboard/src/model.rs`, `dashboard/static/board.js`, `dashboard/static/dashboard.css`.

---

### Task 1: Evidence for every park

**Files:**
- Modify: `crates/factory-git/src/state.rs` (`preflight_main`, `Preflight`)
- Modify: `crates/factory-engine/src/park.rs` (`park_with`)
- Modify: `crates/factory-engine/src/build.rs` (the `NothingChanged` park near line 197)
- Modify: `crates/factory-engine/src/merge.rs` (`park`, `merge_main`'s `Step::Park` sites near lines 158 and 170, the `Err` arm)
- Modify: `crates/factory-engine/src/runloop.rs:519`, `:530`, `:552`
- Test: `crates/factory-git` state tests, `crates/factory-engine/tests/park_evidence.rs` (new), existing merge tests

**Interfaces:**
- Produces:
  - `Preflight::Busy { reason: String, paths: Vec<String> }`: `paths` is filled only for the tracked-changes case, from the same `statuses` call, at most 50 entries, then one `(+N more)` entry.
  - In `park_with`: when `parts` is empty, it becomes `vec![ParkPart::new("Why it stopped", "driver", &reason)]`.
  - `NothingChanged` with no builder reply gets `ParkPart::new("Builder reply", "builder", "The builder wrote no reply.")`.
  - `pub fn park_with_parts(driver: &Driver, id: &str, reason: &str, kind: ParkKind, parts: Vec<ParkPart>) -> Result<(), EngineError>` in `merge.rs`, holding today's body of `park` with `kind` and `parts` in place of `Other` and `Vec::new()` (an earlier park still keeps its kind and evidence). `park(driver, id, reason)` calls it with `ParkKind::Other` and an empty list, so the `Why it stopped` rule applies.
  - `Step::Park` carries `(String, ParkKind, Vec<ParkPart>)`. The merge step passes:
    - main busy: `Other` with `ParkPart::new("Main checkout", "git status", "<paths, one per line>\n\nCommit or stash these in the main checkout, then run factory approve <id> again.")`;
    - tip differs from the reviewed head: `Other` with `ParkPart::new("Branch moved", "driver", "reviewed head <head>\nbranch tip <tip>\n\nSomething was committed on the job branch after the review. Run factory rework <id> <note> so it is reviewed, or reset the branch to the reviewed head.")`;
    - merge failed: `Other` with `ParkPart::new("Merge error", "git", "<error text>")`.
  - Run loop: `no plan.md in the job folder` gets `ParkPart::new("Missing plan", "driver", "expected <job dir>/plan.md")`; `run loop did not settle` gets `ParkPart::new("Ledger", "progress.md", "<last 20 lines>")`.

- [ ] **Step 1: Write the failing tests**

In `factory-git`: `busy_lists_the_tracked_changes` (two modified tracked files give both paths, untracked files are not listed) and `busy_lists_at_most_fifty_paths` (60 modified files give 50 paths and `(+10 more)`).

In `tests/park_evidence.rs`:
- `no_park_has_empty_evidence`: drive a `CommitFailed` park and a `BuilderCommitted` park (scripted runner); each `park.json` has exactly one part titled `Why it stopped` holding the reason.
- `nothing_changed_without_a_reply_says_so`: a builder that changes nothing and replies with an empty string parks with a `Builder reply` part reading `The builder wrote no reply.`
- `main_busy_lists_the_dirty_files`: an approved job whose main checkout has a modified tracked file parks with a `Main checkout` part naming that file and `factory approve`.
- `a_moved_branch_names_both_shas`: commit on the branch after the review; the `Branch moved` part has both full shas and `factory rework`.
- `an_earlier_park_keeps_its_evidence`: a job already parked by the sync step keeps its first kind and parts when `park` is called again.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -j4 -p factory-git busy_lists` and `cargo test -j4 -p factory-engine --test park_evidence`
Expected: FAIL.

- [ ] **Step 3: Implement the evidence**

- [ ] **Step 4: Run the tests**

Run: `cargo test -j4 -p factory-git`, `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`
Expected: PASS. Existing tests that asserted an empty `parts` list for these parks change to the new part.

---

### Task 2: `park_summary` on the board and the card

**Files:**
- Modify: `crates/factory-core/src/board.rs` (`Job`, `Job::new`, `set_stage`)
- Modify: `crates/factory-engine/src/park.rs` (`park_with`), `crates/factory-engine/src/merge.rs` (`park_with_parts`)
- Modify: `dashboard/src/model.rs`, `dashboard/static/board.js` (`renderCard` near line 577), `dashboard/static/dashboard.css`
- Test: `crates/factory-core/src/board.rs` tests, `crates/factory-engine/tests/park_evidence.rs`, `dashboard/tests/board.test.js`, dashboard Rust model tests

**Interfaces:**
- Consumes: the parts of Task 1 and the Review part of plan 01.
- Produces:
  - `Job.park_summary: Option<String>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`), `None` in `Job::new`, set to `None` by `Job::set_stage` whenever it clears `reason` (moving out of `needs-human`).
  - `pub fn park_summary(reason: &str, parts: &[ParkPart]) -> String` in `factory-core/src/park.rs`: the first two non-empty lines of `parts[0].text` joined by ` · `, or `reason` when there is no part or no line, with whitespace folded to single spaces and capped at 160 characters (cut on a char boundary, ending with `…` when cut).
  - `park_with` and `park_with_parts` set `j.park_summary = Some(park_summary(&reason, &parts))` in the same board update that sets `park_kind`.
  - Dashboard `model::Job.park_summary: Option<String>` (lenient, like `park_kind`).
  - `renderCard` for `needs-human`: a chip with `parkKindLabel(job.park_kind)` when `park_kind` is set; `<p class="reason park-summary">` with the summary when set, else the reason; `<p class="reason-detail">` with the reason when a summary is shown and differs from it; then the existing park button, missing-review note and copy chips.

- [ ] **Step 1: Write the failing tests**

In `board.rs`: `park_summary_is_optional_and_round_trips`; `leaving_needs_human_clears_the_summary` (set stage `needs-human` with a reason and a summary, then `set_stage(Build, ..)`: both `None`).

In `factory-core/src/park.rs`: `summary_joins_the_first_two_lines` (`"verdict changes, round 2\n\n- [blocker] no test\n- [blocker] two"` gives `verdict changes, round 2 · - [blocker] no test`); `summary_falls_back_to_the_reason`; `summary_is_one_capped_line` (a 500-character line with tabs and newlines gives at most 160 characters, no newline, ending with `…`).

In `park_evidence.rs`: `a_park_writes_the_summary_on_the_board` (a `ChangesRemain` park's board entry has a summary starting `verdict changes`).

In `board.test.js`:
- `a parked card shows the kind chip and the summary` (a job with `park_kind: "changes-remain"`, `park_summary: "verdict changes, round 2 · [blocker] no test"`, `reason: "changes remain after 2 fix rounds"`): the card contains the kind label, the summary in `.park-summary` and the reason in `.reason-detail`.
- `card_without_a_summary_shows_the_reason`: no `park_summary` gives today's markup with the reason.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -j4 -p factory-core park_summary summary_ leaving_needs_human`, `cargo test -j4 -p factory-engine --test park_evidence a_park_writes`, `node --test dashboard/tests/board.test.js`
Expected: FAIL.

- [ ] **Step 3: Implement the field, the helper, the writes and the card**

Style `.park-summary` with the existing card text styles (weight 600) and `.reason-detail` muted; no new colors.

- [ ] **Step 4: Run every touched suite and clippy**

Run: `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-git`, `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`, `cargo test -j4 --manifest-path dashboard/Cargo.toml`, `node --test dashboard/tests/*.test.js`, then `cargo clippy -j4 -p factory-core -p factory-git -p factory-engine --all-targets -- -D warnings`
Expected: all pass, no warnings.

Next plan: docs/superpowers/plans/2026-10-29-act-on-review-findings/plan-05-review-grading.md
