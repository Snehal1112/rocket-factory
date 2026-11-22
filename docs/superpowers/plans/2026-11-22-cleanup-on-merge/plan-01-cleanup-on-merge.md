# Cleanup on merge Implementation Plan

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Remove a job's worktree and branch automatically right after the job merges, so nobody has to run `factory cleanup <id>` by hand.

**Commit:** feat(merge): clean up a job right after it merges

**Architecture:** A `cleanup_on_merge` setting (default true) in factory-core. The checkout part of `factory cleanup` moves from factory-cli into a new `factory-engine/src/cleanup.rs`, shared by the command and a new last soft step in `merge_stage`. Git stays the concrete `factory_git::Repo`; no port is involved.

**Tech Stack:** Rust, factory-core, factory-engine, factory-cli, git2 through factory-git.

> **Spec:** docs/superpowers/specs/2026-11-22-cleanup-on-merge-design.md

## Global Constraints

- `cleanup_on_merge: bool`, default `true`, parsed like `tickets_on_merge`.
- A cleanup failure never changes the merge result, never parks a job and never removes a card.
- `factory cleanup <id>` keeps its exact texts and behaviour, and newly refuses a job whose live lease another driver holds, with `job <id> is leased by <owner>` (the check `factory tickets` makes at `commands.rs:1316-1325`).
- Kept-worktree ledger line: `cleanup kept the worktree: <reason>`. Stderr: `warning: job <id>: cleanup kept the worktree: <reason>`. Events: `cleanup.done` (info, `job`, `msg`) and `cleanup.kept` (warn, `job`, `reason`).
- Never shell out to `git`. The board JSON keeps its shape; `cleaned` already exists. No `Cargo.toml` change and no file under `dashboard/` changes.
- Cargo: `-j4 -p <crate>`, never `--workspace`, `--all` or `CARGO_TARGET_DIR`. Tests use `.expect("...")` and `expect_err`. Comments are short full sentences.

## File Structure

- `crates/factory-core/src/config.rs`, `settings_block.rs`, `resolve.rs`, `tests/rocket_parity.rs`: the setting.
- `crates/factory-cli/src/project.rs`: `cleanup on merge: <bool>` in `project show`.
- `crates/factory-engine/tests/common/mod.rs`, `crates/factory-cli/tests/common/mod.rs`, `crates/factory-cli/tests/external_layout.rs`: tests turn the setting off.
- `crates/factory-engine/src/cleanup.rs` (new), `lib.rs`: `cleanup_checkout` and `cleanup_after_merge`.
- `crates/factory-cli/src/commands.rs`: `cleanup_job` calls the engine's `cleanup_checkout`.
- `crates/factory-engine/src/merge.rs`: the soft step.
- `crates/factory-engine/tests/cleanup_on_merge.rs` (new), `crates/factory-cli/tests/new_to_merged.rs`: new tests.
- `docs/reference/project-toml-reference.md`, `docs/reference/software-factory.md`, `docs/guides/factory-user-guide.md`, `docs/guides/getting-started.md`.

## Review Focus

1. A manual `factory cleanup <id>` while the merging driver is cleaning the same job must be refused, not race it. Pinned by `cleanup_refuses_a_job_leased_by_another_driver`.
2. A refusal for one member must not stop `factory cleanup` over a series: the engine's `EngineError::Refused` must come back as `CliError::Refused`, so the loop at `commands.rs:1081-1085` goes on. Pinned by the existing series cleanup tests in factory-cli, unchanged.
3. A dirty worktree after the merge must still give `MergeResult::Merged`, and its untracked file must survive. Pinned by `a_dirty_worktree_is_kept_and_reported`.
4. With the setting off, the worktree must stay and no ledger line must appear. Pinned by `the_setting_off_keeps_the_worktree`.
5. Tests that read a merged job's branch must keep passing through the helper defaults, for example `message.rs:119` and `merge.rs:546`. Pinned by the full factory-engine and factory-cli suites.

---

### Task 1: The `cleanup_on_merge` setting

**Files:**
- Modify: `crates/factory-core/src/config.rs` (field next to `tickets_on_merge` at `:163`, default at `:285`), `crates/factory-core/src/settings_block.rs:79`, `crates/factory-core/src/resolve.rs` (test next to `tickets_on_merge_is_a_valid_project_setting`), `crates/factory-core/tests/rocket_parity.rs:61`, `crates/factory-cli/src/project.rs:306`
- Modify: `crates/factory-engine/tests/common/mod.rs` (`test_config` at `:445`), `crates/factory-cli/tests/common/mod.rs:159`, `crates/factory-cli/tests/external_layout.rs:53`
- Test: `crates/factory-core/src/config.rs` (`mod tests`), `crates/factory-core/src/resolve.rs` (`mod tests`)

**Interfaces:**
- Produces: `pub cleanup_on_merge: bool` on `factory_core::Config`, default `true`.
- Produces: in `tests/common/mod.rs` of factory-engine, `test_config` sets `cleanup_on_merge = false`, and a new `pub fn approved_job_with_cleanup() -> MergeFixture` builds as `approved_job` does (a job already at the approve stage) but with `cleanup_on_merge = true` after `test_config`.

Tests added: `cleanup_on_merge_is_on_by_default_and_can_be_turned_off`, `cleanup_on_merge_is_a_valid_project_setting`.

- [ ] **Step 1: Read** `crates/factory-core/src/config.rs:150-300` and `:1320-1340`, `settings_block.rs:60-100`, `resolve.rs:440-470`, `tests/rocket_parity.rs:40-70`, `crates/factory-cli/src/project.rs:290-310`, and the three test helpers listed under Files.

- [ ] **Step 2: Write the failing tests:**

```rust
#[test]
fn cleanup_on_merge_is_on_by_default_and_can_be_turned_off() {
    assert!(Config::default().cleanup_on_merge);
    assert!(
        !Config::parse("cleanup_on_merge = false")
            .expect("parse")
            .cleanup_on_merge
    );
    assert!(Config::parse("cleanup_on_merge = \"yes\"").is_err());
}
```

`cleanup_on_merge_is_a_valid_project_setting` in `resolve.rs` is `tickets_on_merge_is_a_valid_project_setting` with the key renamed.

- [ ] **Step 3: Run** `cargo test -j4 -p factory-core cleanup_on_merge`. Expected: does not compile, with `no field 'cleanup_on_merge'`.

- [ ] **Step 4: Implement.**
  - Add the field and its default, with a one-sentence doc comment.
  - List it in `settings_block.rs`.
  - Compare it in `rocket_parity.rs` like `tickets_on_merge`.
  - Print `cleanup on merge: <bool>` after `tickets on merge` in `project.rs`.
  - Turn the setting off in the test helpers:
    - factory-engine `test_config`: set `config.cleanup_on_merge = false`, with a comment that tests reading a merged job's worktree need it, and add `approved_job_with_cleanup`.
    - factory-cli `common/mod.rs:159`: append `cleanup_on_merge = false\n` to the written config.
    - `external_layout.rs:53`: add the same line to its config.

- [ ] **Step 5: Run** `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-engine` and `cargo test -j4 -p factory-cli`. Expected: all pass, including the two new tests and `every_config_key_is_listed_once`.

### Task 2: Cleanup moves into the engine, and the lease guard

**Files:**
- Create: `crates/factory-engine/src/cleanup.rs`
- Modify: `crates/factory-engine/src/lib.rs` (module and `pub use cleanup::cleanup_checkout`), `crates/factory-cli/src/commands.rs:1119-1307` (`cleanup_job`, `cleanup_checkout`)
- Test: the existing cleanup tests in `crates/factory-cli/tests/new_to_merged.rs` (`cleanup_*`), `tests/project_lease_cli.rs` (`cleanup_*`), unchanged

**Interfaces:**
- Consumes: `factory_engine::Driver` (`repo()`, `job_worktree` at `build.rs:510`), `factory_core::Job`.
- Produces: `pub fn cleanup_checkout(driver: &Driver, job: &Job, discard: bool) -> Result<String, EngineError>`. Every refusal is `EngineError::Refused(<today's exact text>)`.

Tests added: `cleanup_refuses_a_job_leased_by_another_driver` in `crates/factory-cli/tests/project_lease_cli.rs`. The moved code is pinned by the existing cleanup tests.

- [ ] **Step 1: Read** `crates/factory-cli/src/commands.rs:1060-1307`, `crates/factory-engine/src/lib.rs:140-175`, `crates/factory-engine/src/build.rs:505-520`, the `CliError` enum in factory-cli, and every `cleanup_*` test in `crates/factory-cli/tests/`.

- [ ] **Step 2: Run** `cargo test -j4 -p factory-cli cleanup` before any change. Expected: all pass. Note the count; it must be one more after this task.

- [ ] **Step 2b: Write the failing lease test** in `tests/project_lease_cli.rs`, building the merged job and a live lease the way `cleanup_removes_the_card_despite_a_dead_owner_lease` builds its dead one, with owner `someone-else` and a fresh heartbeat. Assert `fx.run(&["cleanup", "1"]).expect_err("leased")` shows `job 001 is leased by someone-else`, and that the worktree path and the branch still exist. Run `cargo test -j4 -p factory-cli --test project_lease_cli cleanup_refuses_a_job_leased_by_another_driver`. Expected: it fails, because the cleanup succeeds.

- [ ] **Step 3: Implement.**
  - Move `cleanup_checkout` into `cleanup.rs` with the signature above. Keep every message text, including "nothing to remove for job <id>, the worktree and branch are already gone", the leftover-folder texts, "<path> is not a git checkout and lies outside <dir>, remove it by hand", "the branch has commits that are not in <base>, use --discard-commits", "<git error>, use --discard-commits to drop it" and "removed worktree and branch of job <id>".
  - In factory-cli, `cleanup_job` calls `factory_engine::cleanup_checkout(&ctx.driver()?, job, discard)`. It maps `Err(EngineError::Refused(why))` to `CliError::Refused(why)` and other errors through `CliError::Engine`.
  - Delete the CLI copy.
  - At the top of the CLI `cleanup`, refuse when `job.lease` is live (`lease_is_live(&job, now_secs())`) and its owner is not `ctx.owner`, with `CliError::Refused(format!("job {id} is leased by {owner}"))`, as `tickets` does.

- [ ] **Step 4: Run** `cargo test -j4 -p factory-cli` and `cargo test -j4 -p factory-engine`. Expected: all pass, and the `cleanup` filter count is one more than in Step 2.

### Task 3: The soft step after a merge, and docs

**Files:**
- Modify: `crates/factory-engine/src/cleanup.rs` (`cleanup_after_merge`), `crates/factory-engine/src/merge.rs:261-306` (`merge_stage`)
- Modify: `docs/reference/project-toml-reference.md` (row after `tickets_on_merge` at `:134`, example block near `:426`), `docs/reference/software-factory.md` (events list near `:194`, `/factory cleanup` row near `:362`), `docs/guides/factory-user-guide.md`, `docs/guides/getting-started.md`
- Test: `crates/factory-engine/tests/cleanup_on_merge.rs` (new), `crates/factory-cli/tests/new_to_merged.rs`

**Interfaces:**
- Consumes: `cleanup_checkout` (from the engine task), `run_soft_step` (`merge.rs:180`), `write_step_ledger` (`merge.rs:250`), `approved_job_with_cleanup` (from the setting task).
- Produces: `pub(crate) fn cleanup_after_merge(driver: &Driver, id: &str)`.

Tests added: `a_merge_removes_the_worktree_and_branch`, `a_dirty_worktree_is_kept_and_reported`, `the_setting_off_keeps_the_worktree`, `a_run_to_merged_cleans_up_by_default`.

- [ ] **Step 1: Read** `crates/factory-engine/src/merge.rs:175-310`, `crates/factory-engine/tests/merge.rs` (a test that calls `merge_stage` on `approved_job()`), `crates/factory-engine/tests/common/mod.rs` (`MergeFixture`, `approved_job`, `approved_job_with_cleanup`), `crates/factory-cli/tests/new_to_merged.rs:300-345`, and the spec's Design 3 and edge-case table.

- [ ] **Step 2: Write the failing tests.** In `crates/factory-engine/tests/cleanup_on_merge.rs`, each test takes an approved job and calls `merge_stage` directly:

```rust
#[test]
fn a_merge_removes_the_worktree_and_branch() {
    let fx = common::approved_job_with_cleanup();
    let job = fx.driver.job(&fx.id).expect("job");
    let worktree = std::path::PathBuf::from(job.worktree.clone().expect("worktree"));
    let merged = factory_engine::merge_stage(&fx.driver, &fx.id).expect("merge");
    assert!(matches!(merged, factory_engine::MergeResult::Merged(_)), "{merged:?}");
    assert!(!worktree.exists(), "worktree still at {}", worktree.display());
    let repo = git2::Repository::open(&fx.main).expect("open");
    assert!(repo.find_branch(&job.branch, git2::BranchType::Local).is_err());
    assert!(fx.driver.job(&fx.id).expect("job").cleaned);
}
```

| Test | Setup | Assertions |
|---|---|---|
| `a_dirty_worktree_is_kept_and_reported` | as above, plus `std::fs::write(worktree.join("scratch.txt"), "x")` before `merge_stage` | result `Merged`; `worktree.join("scratch.txt")` exists; the branch exists; `cleaned` is false; `progress.md` in the job folder has exactly one line starting `cleanup kept the worktree:` and that line contains `--discard-commits` |
| `the_setting_off_keeps_the_worktree` | `common::approved_job()` (the setting is off through `test_config`) | result `Merged`; the worktree and branch exist; `progress.md` has no line containing `cleanup kept` |

In `crates/factory-cli/tests/new_to_merged.rs`, `a_run_to_merged_cleans_up_by_default` builds the `Fixture`, then overwrites `.claude/factory/config.toml` with the same text but `cleanup_on_merge = true`. It runs `new` and then `run 1` as `cleanup_twice_succeeds_and_says_nothing_was_left` does. It asserts that `run 1` returns `merged` and that `cleanup 1` returns exactly `nothing to remove for job 001, the worktree and branch are already gone`.

- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine --test cleanup_on_merge` and `cargo test -j4 -p factory-cli --test new_to_merged a_run_to_merged_cleans_up_by_default`. Expected: `a_merge_removes_the_worktree_and_branch`, `a_dirty_worktree_is_kept_and_reported` and `a_run_to_merged_cleans_up_by_default` fail (the worktree still exists, or no ledger line). `the_setting_off_keeps_the_worktree` passes already.

- [ ] **Step 4: Implement** `cleanup_after_merge` and the call.
  - `cleanup_after_merge` loads the job. It runs `cleanup_checkout(driver, &job, false)` inside `run_soft_step`.
  - On `Ok(text)` it sets `cleaned = true` on the card through the board store and logs `tracing::info!(event = "cleanup.done", job = id, msg = %text)`.
  - On a refusal, error or panic, it writes `cleanup kept the worktree: <reason>` with `write_step_ledger` and prints the stderr warning from the Global Constraints. It logs `tracing::warn!(event = "cleanup.kept", job = id, reason = %reason)`.
  - In `merge_stage`, call it after the costs refresh and before `Ok(MergeResult::Merged(sha))`, only when `driver.config().cleanup_on_merge`. Give the call a one-sentence comment saying it is the last soft step and never changes the result.

- [ ] **Step 5: Update the docs** as the spec's Design 5 says:
  - the `cleanup_on_merge` row and example key in `project-toml-reference.md`;
  - `cleanup.done` and `cleanup.kept` in the events list of `software-factory.md`;
  - the `/factory cleanup <id>` row there, saying a merged job is cleaned automatically when the setting is on and the command is for kept worktrees and cancelled jobs;
  - a short mention in the two guides wherever they describe cleaning up after a merge.

- [ ] **Step 6: Run** `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`, `cargo clippy -j4 -p factory-core -p factory-engine -p factory-cli --all-targets -- -D warnings` and `cargo fmt --check`. Expected: all pass with the four new tests, no warnings and no formatting changes.

Next plan: none
