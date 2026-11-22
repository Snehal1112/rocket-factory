# Clean up a job right after it merges

## Goal

Remove a job's worktree and branch automatically right after the job merges, so nobody has to run `factory cleanup <id>` by hand.

## Overview

Every merged job leaves its worktree under `~/.factory/projects/<project>/worktrees/` and its `factory/<id>-<slug>` branch behind until someone runs `factory cleanup <id>`. On rocket-factory, five merged jobs (103 to 107) were still checked out on 2026-11-22 and had to be cleaned one at a time. Each worktree holds a full checkout and often a `target/` folder, so leftovers cost disk space, and the space ran out once during this work.

## Current state

- `factory cleanup <id>` is `cleanup` (`crates/factory-cli/src/commands.rs:1065-1089`). It refuses a job that is neither merged nor cancelled, then calls `cleanup_job` (`:1119-1132`), which calls `cleanup_checkout` (`:1237-1307`), sets `cleaned = true` on the card, and for a cancelled job only removes its card.
- `cleanup_checkout` handles a leftover folder with no `.git`, a branch or worktree that is already gone ("nothing to remove for job <id>, ..."), refuses unmerged commits without `--discard-commits`, and then calls `Repo::remove_worktree` (`crates/factory-git/src/worktree.rs:169`). That refuses a worktree with untracked or modified files unless forced, and deletes the branch. It returns "removed worktree and branch of job <id>".
- `merge_stage` (`crates/factory-engine/src/merge.rs:261`, its merged arm ending at `:306`) runs the soft steps after a merge in this order: KB refresh, tickets (behind `tickets_on_merge`), waiver tickets, minor tickets, costs refresh. Each soft step's failure goes to the ledger, the job output and the events, and never changes the merge result. `run_soft_step` is at `:180`, `report_unsigned` (stderr warning plus ledger line) at `:242` and `write_step_ledger` at `:250`. `merge_stage` is called by the run loop (`runloop.rs:1070`) and by `factory approve` (`commands.rs:861`).
- `tickets_on_merge` (`crates/factory-core/src/config.rs:163`, default `true` at `:285`) is listed in `settings_block.rs:79` and documented in `docs/reference/project-toml-reference.md:134`.
- The card field `cleaned` (`crates/factory-core/src/board.rs:101`) marks a cleaned job.
- Nothing after `merge_stage` uses the job's worktree: the run loop returns `RunEnd::Merged` (`runloop.rs:1070`), the next plan of a series gets its own worktree, `factory approve` only prints the merged line, and the KB refresh, ticket steps and costs refresh read the main checkout or commits by sha.
- Real merged worktrees are clean: jobs 103 to 107 were all removed by `factory cleanup <id>` without `--discard-commits`, because the driver stages everything after each task and writes agent files into the job folder, not the worktree.

Gap: nothing cleans up a job when it merges.

## Non-goals

- Cleaning cancelled jobs, or removing any card.
- Sweeping jobs that merged earlier; they keep using `factory cleanup <id>`.
- Removing a worktree that has untracked or modified files, or a branch with unmerged commits.
- Any change to the job folder, which always stays.
- Any change to the dashboard.

## Design

```
merge_stage: merged
  ... KB refresh, tickets, waivers, minors, costs refresh (unchanged)
  cleanup_on_merge is true?
    yes -> cleanup_checkout(driver, job, discard = false)
             removed / nothing left -> cleaned = true, event cleanup.done
             refused or error      -> ledger line, stderr warning, event cleanup.kept
    no  -> nothing
  MergeResult::Merged(sha)   (always, whatever the cleanup did)
```

### 1. The setting

`cleanup_on_merge: bool` in `Config`, default `true`, parsed like `tickets_on_merge`, listed in `settings_block.rs` and documented in `project-toml-reference.md` next to `tickets_on_merge`:

| Setting | Type | Default | Meaning |
|---|---|---|---|
| `cleanup_on_merge` | bool | true | Right after a job merges, remove its worktree and branch, as `factory cleanup <id>` does. A worktree with untracked or modified files is kept and reported. A failure never changes the merge. `false` turns it off. |

### 2. Cleanup moves into the engine

The checkout part of `factory cleanup` moves from factory-cli into a new `crates/factory-engine/src/cleanup.rs`, so the merge step and the command share one implementation:

```rust
/// Removes the worktree, the branch and any leftover folder of a job. Returns what was done, in words.
pub fn cleanup_checkout(driver: &Driver, job: &Job, discard: bool) -> Result<String, EngineError>;

/// Runs the cleanup after a merge as a soft step: marks the card cleaned, or reports why it was kept.
pub(crate) fn cleanup_after_merge(driver: &Driver, id: &str);
```

`cleanup_checkout` keeps today's behaviour and texts exactly. Every refusal it makes today, including the leftover-folder and outside-the-worktrees-folder ones and the wrapped `remove_worktree` error, becomes `EngineError::Refused(<the same text>)`. The CLI maps `EngineError::Refused` back to `CliError::Refused`, so the series loop in `cleanup` still reads `Err(CliError::Refused(why)) => why` and goes on to the next member. The worktree may be built with the existing `Driver::job_worktree` (`build.rs:510`). `cleanup_job`, the card removal of a cancelled job and the series leftovers stay in factory-cli and call the engine function. `factory cleanup` output does not change.

### 3. The soft step

`merge_stage` calls `cleanup_after_merge(driver, id)` as the last soft step, after the costs refresh, when `driver.config().cleanup_on_merge` is true. It runs under `run_soft_step`, so a panic is caught like the other steps.

- On `Ok(text)`, it sets `cleaned = true` on the card and logs `tracing::info!(event = "cleanup.done", job = id, msg = text)`.
- On a refusal or error, it writes the ledger line `cleanup kept the worktree: <reason>` through `write_step_ledger`. `<reason>` is the refusal text, which already names `--discard-commits` where that is the way out. It also prints `warning: job <id>: cleanup kept the worktree: <reason>` to stderr and logs `tracing::warn!(event = "cleanup.kept", job = id, reason)`. The card keeps `cleaned = false`.

`merge_stage` returns `MergeResult::Merged(sha)` in every case. A later `factory cleanup <id>` (with `--discard-commits` if needed) works as today.

Example ledger line: `cleanup kept the worktree: worktree <path> has uncommitted or untracked work, use --discard-commits to drop it`. The worktree stays for the user to look at, then remove the files or run `factory cleanup <id> --discard-commits`.

The merging driver holds the job's lease through the whole of `merge_stage`, the automatic cleanup included (the run loop and `factory approve` both renew it). `factory cleanup <id>` now refuses while another driver holds a live lease on that job, with the text `job <id> is leased by <owner>`, the same check `factory tickets` makes (`commands.rs:1316-1325`). So a manual cleanup can never run at the same time as the automatic one. A dead or expired lease does not block it, as today.

### 4. Tests that inspect a merged job's worktree

The setting is on by default, so existing tests that merge a job and then use its worktree or branch would see them gone. Three edits keep them as they are:

- `crates/factory-engine/tests/common/mod.rs`, `test_config()` (line 445): set `cleanup_on_merge = false`. It is applied by `driver_for` and `approved_job_dyn`, so it covers `MergeFixture` and the `approved_job*` tests, including `message.rs:119` and `:144` and `merge.rs:546`, which read the job branch after the merge. The new engine tests set the field back to `true` after `test_config` runs.
- `crates/factory-cli/tests/common/mod.rs:159`, which writes the test repo's `.claude/factory/config.toml`: add `cleanup_on_merge = false`. It covers every `common::Fixture` test, including the six cleanup tests in `new_to_merged.rs` and `a_job_goes_from_new_to_merged_through_the_cli`. The new CLI test writes the line as `true`.
- `crates/factory-cli/tests/external_layout.rs:53`, which builds its own config: add `cleanup_on_merge = false`.

`log_events.rs` tests that assert exact event lists get `cleanup_on_merge = false` too, where a new `cleanup.done` event would change them.

### 5. Docs

- `docs/reference/project-toml-reference.md`: the row above, and the key in the example block (near line 426).
- `docs/guides/factory-user-guide.md` and `docs/guides/getting-started.md`: where they mention cleaning up after a merge or list `tickets_on_merge`, mention `cleanup_on_merge`.
- `factory project show` (`crates/factory-cli/src/project.rs:306`) prints `cleanup on merge: <bool>` after `tickets on merge`.
- `docs/reference/software-factory.md`: the `/factory cleanup <id>` row (near line 362) says that a merged job is cleaned automatically when `cleanup_on_merge` is on, and the command is for kept worktrees and cancelled jobs. The events list (near line 194) gains `cleanup.done` and `cleanup.kept`.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| Clean worktree, merged branch | Worktree and branch removed, card `cleaned = true`, `cleanup.done`. |
| Worktree has untracked or modified files | Kept and reported (ledger, stderr, `cleanup.kept`); the merge result is unchanged. |
| Worktree folder already removed by hand | The branch is deleted as `factory cleanup` does today; counts as done. |
| Nothing left to remove | "nothing to remove ..."; counts as done, `cleaned = true`. |
| Git error or panic during cleanup | Reported like a refusal; the merge result is unchanged. |
| `cleanup_on_merge = false` | No cleanup; behaviour as today. |
| A job in a series whose next plan is still running | Only the merged job's own worktree and branch go; the next job has its own. |
| The merge came from `factory approve` | The same step runs, because `approve` calls `merge_stage`. |
| A merged job that was already cleaned | `factory cleanup <id>` says nothing was left, as today. |
| A rerun of a job that is already merged | The run loop returns before `merge_stage`, so no cleanup runs; `factory cleanup <id>` is the way. |
| Two projects sharing one repository merge at once | Each removes only its own worktree and branch. |
| `factory cleanup <id>` while another driver holds a live lease on the job | Refused with `job <id> is leased by <owner>`; nothing is removed. |

## Testing

- factory-core `config.rs`: `cleanup_on_merge_is_on_by_default_and_can_be_turned_off`, shaped like `tickets_on_merge_is_on_by_default_and_can_be_turned_off`, including the `"yes"` parse error. `settings_block.rs`'s `every_config_key_is_listed_once` covers the listing. `resolve.rs` gets `cleanup_on_merge_is_a_valid_project_setting`, shaped like `tickets_on_merge_is_a_valid_project_setting`, and `tests/rocket_parity.rs` compares the field like `tickets_on_merge`.
- factory-engine `tests/cleanup_on_merge.rs` (new), on `MergeFixture` and an auto-merge as `an_auto_merge_writes_only_passes_notes_and_skips` in `tests/gate_record.rs` does:
  - `a_merge_removes_the_worktree_and_branch`: after `merge_stage` returns `Merged`, the worktree path does not exist, the branch is gone, and the card has `cleaned == true`.
  - `a_dirty_worktree_is_kept_and_reported`: an untracked file is written into the worktree before the merge. Afterwards the result is still `Merged`, the worktree and branch exist, `cleaned` is false, and the ledger has one line starting `cleanup kept the worktree:` that contains `--discard-commits`.
  - `the_setting_off_keeps_the_worktree`: with `cleanup_on_merge = false`, the worktree and branch exist after the merge and the ledger has no cleanup line.
- factory-cli `tests/new_to_merged.rs`:
  - `cleanup_refuses_a_job_leased_by_another_driver`: a merged job whose card carries a live lease owned by `someone-else` (set as `project_lease_cli.rs` sets leases) gives `Err` with text `job 001 is leased by someone-else`, and the worktree and branch still exist.
  - `a_run_to_merged_cleans_up_by_default`: `run 1` returns `merged`, then `cleanup 1` says `nothing to remove for job 001, the worktree and branch are already gone`.
  - The existing cleanup tests pass, with `cleanup_on_merge = false` in their fixture config where they need the worktree after the merge.

Commands: `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`, `cargo clippy -j4 -p factory-core -p factory-engine -p factory-cli --all-targets -- -D warnings`, `cargo fmt --check`.

## Files changed

- `crates/factory-core/src/config.rs`, `crates/factory-core/src/settings_block.rs`, `crates/factory-core/src/resolve.rs`, `crates/factory-core/tests/rocket_parity.rs`
- `crates/factory-engine/src/cleanup.rs` (new), `crates/factory-engine/src/lib.rs`, `crates/factory-engine/src/merge.rs`
- `crates/factory-cli/src/commands.rs`, `crates/factory-cli/src/project.rs`
- `crates/factory-engine/tests/cleanup_on_merge.rs` (new), `crates/factory-engine/tests/common/mod.rs`, `crates/factory-cli/tests/common/mod.rs`, `crates/factory-cli/tests/external_layout.rs`, `crates/factory-cli/tests/new_to_merged.rs`, and `log_events.rs` where needed
- `docs/reference/project-toml-reference.md`, `docs/reference/software-factory.md`, `docs/guides/factory-user-guide.md`, `docs/guides/getting-started.md`

## Constraints

- Never shell out to `git`; the cleanup uses `factory_git::Repo` as today.
- A cleanup failure never changes the merge result, never parks a job and never removes a card.
- `factory cleanup` keeps its texts and behaviour, apart from the new lease refusal.
- The board JSON keeps its shape; `cleaned` already exists. No `Cargo.toml` change and no file under `dashboard/` changes.
- Cargo: `-j4 -p <crate>`, never `--workspace`, `--all` or `CARGO_TARGET_DIR`. Tests use `.expect("...")` and `expect_err`. Comments are short full sentences.

## Acceptance criteria

- `cleanup_on_merge` exists, defaults to true, rejects a non-bool value, is listed, shown by `factory project show`, and documented.
- With it on, a job that merges with a clean worktree loses its worktree and branch, and its card has `cleaned = true`.
- A worktree with untracked or modified files is kept, and the ledger, stderr and a `cleanup.kept` event say why, quoting the refusal text that names `--discard-commits`.
- With it off, nothing changes after a merge.
- The merge result is `Merged` whatever the cleanup did.
- `factory approve` merges get the same cleanup.
- `factory cleanup <id>` behaves and prints exactly as before, except that it refuses a job another driver holds a live lease on.
- The docs describe the setting, the new events and the changed `/factory cleanup` row.
- Every test in Testing passes, with clippy and fmt clean.

## Out of scope

- Cancelled jobs.
- A sweep of older merged jobs, and a `cleanup --merged` command.

## Decisions

- How is the automatic cleanup switched? Options: a setting on by default, a setting off by default, always with no setting. Chosen: a `cleanup_on_merge` setting, default true, like `tickets_on_merge`. Decided by: user
- What happens when the worktree has untracked files after the merge? Options: keep it and report, remove it anyway, remove the folder but keep the branch. Chosen: keep the worktree and branch, and report why. Decided by: user
- Should older merged jobs be swept too? Options: only the job that merged, also sweep at run start, add a `cleanup --merged` command. Chosen: only the job that merged. Decided by: user
- The checkout cleanup moves into a new factory-engine module, shared by the merge step and `factory cleanup`. Decided by: call
- The cleanup is the last soft step after a merge, after the costs refresh. Decided by: call
- A successful cleanup logs `cleanup.done` and writes no ledger line; only a kept worktree writes one. Decided by: call
- Existing tests that need a merged job's worktree get `cleanup_on_merge = false` through the two shared test helpers and `external_layout.rs`. Decided by: call
- The kept-worktree ledger line quotes the refusal text, which names `--discard-commits`, instead of advising a plain `factory cleanup <id>`. Decided by: call
- How are a manual and the automatic cleanup kept apart? Options: `factory cleanup` refuses a job with a live lease held by another driver, no extra guard, a repository lock around both. Chosen: `factory cleanup` refuses a job leased by another driver, as `factory tickets` does. Decided by: user
