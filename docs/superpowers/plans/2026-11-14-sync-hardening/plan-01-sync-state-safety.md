# Sync hardening, Plan 01: state safety

> **Spec:** docs/superpowers/specs/2026-11-14-sync-hardening-design.md (sections 1, 2 and 3)
>
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** A requested `factory sync` never lets a head merge without a review of that head: the reviewed floor is kept only in `approve` with `tip == head`, a reset clears the reviews in one write, and the whole-branch review diff starts at the merge base.

**Architecture:** Only `sync_requested` and `sync_with` in `sync.rs` and the round diff in `review.rs` change. Git is the concrete `factory_git::Repo`, so no trait sits behind this logic.

**Tech Stack:** Rust, `cargo test -j4 -p factory-engine`.

## Global Constraints

- Always `-j4` and `-p <crate>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Each task ends with `cargo fmt -p factory-engine -- --check` and `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`. No `#[allow(...)]`.
- No unwrap in production Rust. Tests use `.expect("...")` and `expect_err`.
- Code comments are short full sentences that end with a period.
- Do not shell out to `git`. Do not read `/proc`. No board, ledger or events field is added.
- `last_review` is never cleared by the reset. Open blockers keep refusing `approve`, also with `--without-review`.
- The whole-branch path is: `head = tip`, `prev_head = None`, `reset_reviews()`, stage `review` forced, all in one `store().update_job`, then `ReviewRun::Rebase` (code and security).
- The decision table of a requested sync: `tip == head` and not behind main gives `UpToDate` in every stage; `tip == head`, behind main and stage `approve` keeps the floor (`prev_head = old head`, `ReviewRun::Sync`); every other case takes the whole-branch path. A pending `batch_snapshot` refuses the sync.
- The ledger ruling is written before the board update.

## Review Focus

1. A job that parks after the reset and before the rebase review must not be approvable. Pinned by `a_rewrite_resets_the_review_states_and_keeps_last_review`.
2. A second sync after an interrupted first one must still review the whole branch. Pinned by `a_second_sync_after_a_stopped_first_one_still_reviews_the_whole_branch`.
3. A job in `review`, `build` or `needs-human` that is behind main must not keep a floor, and an up-to-date job with `tip == head` must change nothing in any stage. Pinned by `a_plain_sync_keeps_the_floor_only_in_approve_with_tip_equal_head`.
4. Commits added by hand on top of `head` must get the whole-branch review. Pinned by `commits_added_on_top_of_head_get_the_whole_branch_review`.
5. After main moved, the rebase review diff must hold none of the main changes. Pinned by `a_rebase_review_diff_starts_at_the_merge_base`.

## File Structure

- `crates/factory-engine/src/sync.rs`: the whole-branch condition, the durable reset and the floor rule.
- `crates/factory-engine/src/review.rs`: the round diff base.
- `crates/factory-engine/tests/sync_review.rs`: the new tests.

### Task 1: The floor rule and the durable reset

**Files:**
- Modify: `crates/factory-engine/src/sync.rs`
- Test: `crates/factory-engine/tests/sync_review.rs`

**Interfaces:**
- Consumes: `Job::reset_reviews()` (`factory-core/src/board.rs:389`), `Job.last_review: Option<LastReview>`, `set_stage_logged`, `driver.ledger(&job).ruling(..)`, the existing `sync_with(driver, id, kind, rewritten)`.
- Produces: in `sync_requested`, `let behind = ..` (the base branch tip is not an ancestor of the branch tip), `if tip == head && !behind { return Ok(SyncOutcome::UpToDate) }`, then `let whole_branch = tip != head || job.stage != Stage::Approve`. When `whole_branch` is true the single update described in Global Constraints runs, after the ruling, and `sync_with` is called with `rewritten = true` (its parameter keeps its meaning: the floor stays empty and the review is `Rebase`). The `rewritten` test `!is_ancestor(head, tip)` is removed.

- [ ] **Step 1: Read** `sync.rs`, the fixtures of `tests/sync_review.rs` and `Job::reset_reviews`.
- [ ] **Step 2: Write the failing tests** in `tests/sync_review.rs`, reusing its job-behind-main fixture and its branch rewrite helper:
  - `a_rewrite_resets_the_review_states_and_keeps_last_review`: rewrite the branch, make the checks park the job after the reset, then assert the reviews read pending, `last_review` is unchanged and the approve refusal text appears when `approve` is attempted. A second case sets `last_review` to `changes` and asserts that `approve --without-review` is still refused.
  - `a_second_sync_after_a_stopped_first_one_still_reviews_the_whole_branch`: stop the first run after the reset (a runner that errors on the first review call), run `sync_requested` again, assert one `Rebase` review and `prev_head` none.
  - `a_plain_sync_keeps_the_floor_only_in_approve_with_tip_equal_head`: a table of four jobs with the same behind-main branch and `tip == head`, in stage `approve`, `review`, `build` and `needs-human`. Show one full case for `approve` (`prev_head` equals the old head, a `-sync1` review) and list the others by stage with the expected `prev_head` none and a `-rebase` review.
  - `commits_added_on_top_of_head_get_the_whole_branch_review`: add a commit on top of `head` in `approve`, assert the `-rebase` review and a security run.
  - `an_up_to_date_job_with_tip_equal_head_changes_nothing_in_every_stage`: `UpToDate` in `approve`, `review`, `build` and `needs-human`, with no stage change and no agent run.
  - `a_sync_runs_the_checks`: a requested sync runs the checks before the review (assert the check runner was called and the order).
  - `sync_refuses_a_job_with_no_worktree`: the job has no worktree, expect `Err`.
- [ ] **Step 3: Run `cargo test -j4 -p factory-engine --test sync_review a_rewrite_resets`.** Expected: FAIL with an assertion on the review states.
- [ ] **Step 4: Implement** the condition and the single update in `sync_requested`, writing the ruling first. In `sync_with`, drop the `prev_head = Some(old_head)` write whenever `rewritten` is true, as it does now.
- [ ] **Step 5: Run `cargo test -j4 -p factory-engine --test sync_review`.** Expected: all pass, including the existing sync tests.
- [ ] **Step 6: Run `cargo fmt -p factory-engine -- --check` and `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`.** Expected: no output and no warnings.

### Task 2: The whole-branch diff from the merge base

**Files:**
- Modify: `crates/factory-engine/src/review.rs`
- Test: `crates/factory-engine/tests/sync_review.rs`

**Interfaces:**
- Consumes: `Engine::range_start(&self, base: &str, head: &str) -> Result<String, EngineError>` (`checks_stage.rs:149`, the merge base with the base branch).
- Produces: in `review_stage_as`, `round_base = job.prev_head.clone()` when set, else `self.range_start(&base, &head)?`. A round-1 review of a plain job is unchanged, because its merge base is `job.base`.

- [ ] **Step 1: Read** `review_stage_as` (`review.rs:44-75`) and `range_start`.
- [ ] **Step 2: Write the failing test** `a_rebase_review_diff_starts_at_the_merge_base` in `tests/sync_review.rs`: move main forward by a commit that touches `docs/other.txt`, run a requested sync on a rewritten branch, and assert the saved round diff file does not mention `docs/other.txt`. Add `a_first_round_review_diff_is_unchanged` asserting a normal round-1 review diff still starts at `job.base`.
- [ ] **Step 3: Run `cargo test -j4 -p factory-engine --test sync_review a_rebase_review_diff`.** Expected: FAIL because the diff holds `docs/other.txt`.
- [ ] **Step 4: Implement** the `round_base` change.
- [ ] **Step 5: Run `cargo test -j4 -p factory-engine`.** Expected: all pass.
- [ ] **Step 6: Run `cargo fmt -p factory-engine -- --check` and `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`.** Expected: no output and no warnings.

Next plan: docs/superpowers/plans/2026-11-14-sync-hardening/plan-02-sync-park-state.md
