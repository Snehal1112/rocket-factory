# Sync after rebase, Plan 01: factory sync on request

> **Spec:** docs/superpowers/specs/2026-11-14-sync-after-rebase-design.md
>
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** `factory sync <id>` merges the base branch into a job, resets the floor of a rewritten branch for a whole-branch review, and the parks that hit this dead end name the command.

**Architecture:** `sync_stage` in `sync.rs` becomes `sync_with(driver, id, SyncKind)`, so the automatic and the requested sync share the merge, checks and review steps. A new `sync_requested` adds the refusals, the rewrite detection and the up-to-date exit. The CLI wraps it like `run`. Git is the concrete `factory_git::Repo`, so no trait sits behind this logic.

**Tech Stack:** Rust, `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`.

## Global Constraints

- Always `-j4` and `-p <crate>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Each task ends with `cargo fmt -p <crate> -- --check` and `cargo clippy -j4 -p <crate> --all-targets -- -D warnings` for every crate it touched. No `#[allow(...)]` to silence a lint.
- No unwrap in production Rust. Tests use `.expect("...")` and `expect_err`.
- Code comments are short full sentences that end with a period.
- Do not shell out to `git`. Do not read `/proc`. Paths reach the driver only through `Layout`.
- No board, ledger or events field is added. Board JSON stays compatible with `factory-server`.
- The driver never rewrites history: a sync merges main in and never rebases.
- A requested sync does not call the `MAX_SYNCS` check and does not change `job.syncs`. An automatic sync is unchanged.
- Reviewers: `ReviewRun::Rebase` always runs `code` and `security`, whatever roles the job holds, and skips no role for a docs-only diff. The driver adds the `security` entry to the job before the review when it is missing. A sync that is not a rewrite keeps `ReviewRun::Sync`, the code reviewer only.
- Order of a requested sync: reset the floor when the branch was rewritten, then merge main when the branch is behind, then one review. The merge never starts a second review, and after a rewrite it leaves `prev_head` empty so the review covers `base..head`.
- Exact texts: `job <id> is up to date with <base>`, `the worktree of job <id> has uncommitted changes`, `job <id> is <state>`, `branch rewritten outside the factory, floor reset to base, head is <sha>`, and `the branch no longer holds the reviewed head <sha7>; it was rewritten outside the factory: run factory sync <id>`. The old text `committer rewrote history at or below the floor` stays for a real committer rewrite.

## Review Focus

1. A job whose branch is behind main and not rewritten must merge main, not reset the floor. Pinned by `sync_requested_merges_main_into_a_job_that_is_behind`.
2. A rewritten branch must end with `prev_head` empty so the reviewer sees `base..head`, and the earlier review files stay. Pinned by `sync_requested_resets_the_floor_after_a_rebase_and_reviews_the_whole_branch`.
3. A requested sync after two automatic syncs must still run and leave `syncs` at 2. Pinned by `sync_requested_does_not_count_against_max_syncs`.
4. A rewritten branch must get the security reviewer, and a rewritten branch that is also behind main must get main merged before the one review. Pinned by `a_rebase_review_runs_the_security_reviewer` and `a_rewritten_branch_that_is_behind_main_gets_main_merged_before_one_review`.
5. An up-to-date job must cost nothing and change no stage. Pinned by `sync_requested_on_an_up_to_date_job_changes_nothing`.
6. A committer that really rewrites history must keep the old park text, and only a rewrite that happened before the run gets the sync hint. Pinned by `a_committer_that_rewrites_history_keeps_the_old_text` and `a_branch_rewritten_before_the_committer_parks_with_the_sync_hint`.

## File Structure

- `crates/factory-engine/src/sync.rs`: `SyncKind`, `sync_with`, `sync_requested`.
- `crates/factory-engine/src/review.rs`: `ReviewRun::Rebase`.
- `crates/factory-engine/src/compat.rs`: a `review_rebase` helper next to `review_sync`.
- `crates/factory-engine/src/lib.rs`: export `sync_requested`.
- `crates/factory-engine/src/commit.rs`, `build.rs`, `merge.rs`: the park texts and next commands.
- `crates/factory-cli/src/cli.rs`, `commands.rs`: the `sync` command.
- `crates/factory-engine/tests/sync_review.rs`, `tests/commit.rs`, `tests/park_evidence.rs`, `tests/merge.rs`, and a CLI test in `crates/factory-cli/tests/`.
- `docs/reference/software-factory.md`, `docs/guides/factory-user-guide.md`.

### Task 1: The engine sync

**Files:**
- Modify: `crates/factory-engine/src/sync.rs`, `review.rs`, `compat.rs`, `lib.rs`
- Test: `crates/factory-engine/tests/sync_review.rs`

**Interfaces:**
- Consumes: the existing `sync_stage` body, `Driver::job`, `driver.repo().is_ancestor(older, newer)`, `driver.repo().tip(branch)`, `merge_main_into`, `set_stage_logged`, `driver.checks`.
- Produces: `pub enum SyncKind { Auto, Requested }`, `pub fn sync_requested(driver: &Driver, id: &str) -> Result<SyncOutcome, EngineError>`, `ReviewRun::Rebase` (saves `review-code-r<round>-rebase.md`), and `SyncOutcome::UpToDate`. `sync_stage(driver, id)` keeps its signature and calls `sync_with(driver, id, SyncKind::Auto)`.

- [ ] **Step 1: Read** `sync.rs`, `review.rs` (`review_stage_as`, `ReviewRun`), `compat.rs` (`review_sync`) and the fixtures in `tests/sync_review.rs`.
- [ ] **Step 2: Write the failing tests** in `tests/sync_review.rs`, reusing its job-behind-main fixture and the repo helpers for a branch rewrite with `git2`:
  - `sync_requested_merges_main_into_a_job_that_is_behind`: the job has a merge commit, stage `review` and a review ran.
  - `sync_requested_does_not_count_against_max_syncs`: with `syncs = 2` it runs and `syncs` stays 2.
  - `sync_requested_resets_the_floor_after_a_rebase_and_reviews_the_whole_branch`: after the rewrite `prev_head` is none, the stage is `review`, the ledger holds the ruling text from Global Constraints, and `review-code-r<round>-rebase.md` exists next to the earlier review files.
  - `a_rebase_review_runs_the_security_reviewer`: a job whose reviewers are `["code"]` with a docs-only diff still runs both the code and the security reviewer after a rewrite. `a_sync_without_a_rewrite_runs_the_code_reviewer_only` pins the other case.
  - `a_rewritten_branch_that_is_behind_main_gets_main_merged_before_one_review`: one review run, the merge commit is part of the reviewed head, `prev_head` is none and the review diff starts at `base`.
  - `sync_requested_on_an_up_to_date_job_changes_nothing`: the outcome is `UpToDate`, the stage and `head` are unchanged and no agent ran.
  - `sync_requested_refuses_a_merged_job` and `sync_requested_refuses_a_dirty_worktree`: each gives `Err` with the exact text.
  - `sync_requested_conflict_parks_and_names_the_paths`: the outcome is `Parked` and names the file.
- [ ] **Step 3: Run `cargo test -j4 -p factory-engine --test sync_review sync_requested`.** Expected: FAIL with `cannot find function sync_requested in this scope`.
- [ ] **Step 4: Implement** `sync_with` by moving the body of `sync_stage` and gating the `MAX_SYNCS` check and the `syncs += 1` on `SyncKind::Auto`. Implement `sync_requested`: refuse a merged or cancelled job, a job with no worktree and a dirty worktree (`repo.worktree_is_clean`); detect a rewrite with `!is_ancestor(job.head, tip(branch))` and, when true, set `head = tip`, `prev_head = None`, force stage `review` and write the ledger ruling in one `store().update`. Then merge main when the branch is behind, without a review of its own, and run the checks and one review: `ReviewRun::Rebase` after a rewrite, `ReviewRun::Sync` otherwise. When nothing was rewritten and main was already merged, return `UpToDate`. Export it from `lib.rs`.
- [ ] **Step 5: Run `cargo test -j4 -p factory-engine`.** Expected: all pass, including the existing `sync_review` and merge tests.
- [ ] **Step 6: Run `cargo fmt -p factory-engine -- --check` and `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`.** Expected: no output and no warnings.

### Task 2: Park texts that name the command

**Files:**
- Modify: `crates/factory-engine/src/commit.rs`, `build.rs`, `merge.rs`
- Test: `crates/factory-engine/tests/commit.rs`, `tests/park_evidence.rs`, `tests/merge.rs`

**Interfaces:**
- Consumes: `verify_commit_run` and its `before`/`req.floor` inputs, the `nothing-changed` park in `build.rs`, and the `main busy` park text in `merge.rs`.
- Produces: the sync hint text from Global Constraints in the floor check, `factory sync <id>` in the `next` list of the `nothing-changed` park when the reviewed head is not an ancestor of the tip, and a `main busy` text that says plain `factory approve <id>` retries the merge once main is clean.

- [ ] **Step 1: Read** `commit.rs` around `verify_commit_run`, `build.rs` around the `NothingChanged` parks and `park_with`, and the `main busy` text in `merge.rs`.
- [ ] **Step 2: Write the failing tests:**
  - `a_branch_rewritten_before_the_committer_parks_with_the_sync_hint` (`tests/commit.rs`): the floor is not an ancestor of the tip before the run, and the park reason holds `run factory sync` and not `committer rewrote`.
  - `a_committer_that_rewrites_history_keeps_the_old_text` (`tests/commit.rs`): the floor is an ancestor before the run and the fake committer rewrites it, and the reason is `committer rewrote history at or below the floor`.
  - `a_nothing_changed_park_after_a_rewrite_lists_sync` (`tests/park_evidence.rs`): `park.json` `next` holds `factory sync <id>`; with an unrewritten branch it does not.
  - `a_main_busy_park_says_approve_retries_the_merge` (`tests/merge.rs`): the park text holds `factory approve`.
- [ ] **Step 3: Run `cargo test -j4 -p factory-engine --test commit a_branch_rewritten`.** Expected: FAIL with an assertion on the park reason.
- [ ] **Step 4: Implement** the three text changes. In `verify_commit_run`, test the floor against the branch tip from before the committer ran, which the caller already holds as `tip_before`, and pass it in if the signature lacks it.
- [ ] **Step 5: Run `cargo test -j4 -p factory-engine`.** Expected: all pass.
- [ ] **Step 6: Run `cargo fmt -p factory-engine -- --check` and `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`.** Expected: no output and no warnings.

### Task 3: The command and the docs

**Files:**
- Modify: `crates/factory-cli/src/cli.rs`, `crates/factory-cli/src/commands.rs`, `docs/reference/software-factory.md`, `docs/guides/factory-user-guide.md`
- Test: unit test in `cli.rs`, `crates/factory-cli/tests/`

**Interfaces:**
- Consumes: `factory_engine::sync_requested` and `SyncOutcome` from the engine work, and the `run` pattern in `commands.rs` (`ctx.runnable`, `hold_project`, `renew_lease`, `LeaseGuard`, `outcome_lines`).
- Produces: `Command::Sync { id: String }` with the doc comment "Merge the base branch into a job, reset a rewritten branch, then rerun the checks and reviews", and `fn sync(ctx: &CliCtx, id: &str) -> Result<String, CliError>` that prints one line: the up-to-date text, a synced line, or the needs-human line.

- [ ] **Step 1: Read** `cli.rs` (`Tickets` and its parse test), `commands.rs` (`run`, `LeaseGuard`) and the two docs' command tables.
- [ ] **Step 2: Write the failing tests:** `sync_parses_one_id` in `cli.rs` (parse `factory sync 012`, expect `Command::Sync { ref id } if id == "012"`), and `sync_refuses_a_job_with_a_live_lease` in `crates/factory-cli/tests/` using the live-lease fixture of the existing `run` tests.
- [ ] **Step 3: Run `cargo test -j4 -p factory-cli sync`.** Expected: FAIL with `no variant named Sync`.
- [ ] **Step 4: Implement** the variant, the dispatch and `sync`. Then add `factory sync <id>` to both docs next to `approve` and `rework`, with the stages it accepts, the rewrite recovery and the whole-branch review.
- [ ] **Step 5: Run `cargo test -j4 -p factory-cli`.** Expected: PASS.
- [ ] **Step 6: Run `cargo fmt -p factory-cli -- --check` and `cargo clippy -j4 -p factory-cli --all-targets -- -D warnings`.** Expected: no output and no warnings.

Next plan: none
