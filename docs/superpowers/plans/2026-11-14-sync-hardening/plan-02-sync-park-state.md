# Sync hardening, Plan 02: park state and small fixes

> **Spec:** docs/superpowers/specs/2026-11-14-sync-hardening-design.md (sections 4 and 5)
>
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** A park after a requested sync replaces the old park record and keeps the kind of a non-risky decision, the sync refusals exit as `CliError::Refused`, and earlier raw review files are kept.

**Architecture:** `sync_with` returns the park kind with its reason, `sync_requested` parks with that kind after clearing the old park fields, and the CLI maps the refusals. Git is the concrete `factory_git::Repo`, so no trait sits behind this logic.

**Tech Stack:** Rust, `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`.

## Global Constraints

- Always `-j4` and `-p <crate>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Each task ends with `cargo fmt -p <crate> -- --check` and `cargo clippy -j4 -p <crate> --all-targets -- -D warnings` for every crate it touched. No `#[allow(...)]`.
- No unwrap in production Rust. Tests use `.expect("...")` and `expect_err`.
- Code comments are short full sentences that end with a period.
- Do not shell out to `git`. Do not read `/proc`. No board, ledger or events field is added. Board JSON stays compatible with `factory-server`.
- Raw review file names: `review-<role>-r<round>-rebase.raw.md` for a rebase review and `review-<role>-r<round>-sync<n>.raw.md` for a sync review. A plain round keeps `review-<role>-r<round>.raw.md`.

## Review Focus

1. A conflict on a job that was already in `needs-human` must show the new conflict in `park.json`, not the old evidence. Pinned by `a_sync_conflict_on_a_needs_human_job_replaces_the_park_record`.
2. A `changes-remain` decision after a sync must keep its kind and the `approve --waive` hint. Pinned by `a_changes_remain_decision_keeps_its_park_kind_after_a_sync`.
3. A pending batch snapshot must refuse a sync like it refuses an approve. Pinned by `sync_refuses_a_pending_batch_snapshot`.
4. A rebase review must not overwrite the round's raw files. Pinned by `a_rebase_review_keeps_the_round_raw_files`.
5. A refused sync must exit as a refusal, not an engine error. Pinned by `sync_refusals_are_cli_refused`.

## File Structure

- `crates/factory-engine/src/sync.rs`: `SyncOutcome::Parked` carries an optional `ParkKind`, the refusal for a pending snapshot, and the clearing of old park fields.
- `crates/factory-engine/src/review.rs`: the raw file names.
- `crates/factory-engine/src/merge.rs`: nothing changes unless the park helper needs the cleared fields.
- `crates/factory-cli/src/commands.rs`: the refusal mapping.
- `crates/factory-engine/tests/sync_review.rs`, `crates/factory-cli/tests/sync_cmd.rs`, `docs/guides/factory-user-guide.md`, `docs/reference/software-factory.md`.

### Task 1: Park state of a requested sync

**Files:**
- Modify: `crates/factory-engine/src/sync.rs`, `crates/factory-engine/src/merge.rs` only if needed
- Test: `crates/factory-engine/tests/sync_review.rs`

**Interfaces:**
- Consumes: `merge::park_with_parts(driver, id, reason, kind, parts)`, `ParkKind`, `Job.park_kind`, `Job.park_summary`, `Job.park_at`, `Job.batch_snapshot`.
- Produces: `SyncOutcome::Parked(String, Option<ParkKind>)`, `sync_with` returning the kind of a non-risky `Decision::NeedsHuman`, and `sync_requested` that clears `park_kind`, `park_summary` and `park_at` in one update before it parks with the carried kind (`ParkKind::Other` when none). `sync_requested` refuses a job with a pending `batch_snapshot` with the same text `approve` uses.

- [ ] **Step 1: Read** `sync.rs`, `merge.rs` (`park_with_parts`), and the approve refusal for `batch_snapshot`.
- [ ] **Step 2: Write the failing tests** in `tests/sync_review.rs`:
  - `a_sync_conflict_on_a_needs_human_job_replaces_the_park_record`: a job parked as `nothing-changed`, then a requested sync that conflicts; assert `park.json` holds the conflict path and kind `other`, a new ruling line exists, and the old evidence is gone.
  - `a_changes_remain_decision_keeps_its_park_kind_after_a_sync`: a whole-branch review that leaves blockers; assert `park.json` kind is `changes-remain` and `next` holds `factory approve 001 --waive`.
  - `sync_refuses_a_pending_batch_snapshot`: set `batch_snapshot`, expect `Err` with the approve text.
- [ ] **Step 3: Run `cargo test -j4 -p factory-engine --test sync_review a_sync_conflict_on_a_needs_human_job`.** Expected: FAIL with an assertion on the park record.
- [ ] **Step 4: Implement** the outcome change, the clearing update and the refusal. Update every match on `SyncOutcome::Parked` in `merge.rs` and tests.
- [ ] **Step 5: Run `cargo test -j4 -p factory-engine`.** Expected: all pass.
- [ ] **Step 6: Run `cargo fmt -p factory-engine -- --check` and `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`.** Expected: no output and no warnings.

### Task 2: Raw file names, CLI refusals and docs

**Files:**
- Modify: `crates/factory-engine/src/review.rs`, `crates/factory-cli/src/commands.rs`, `docs/guides/factory-user-guide.md`, `docs/reference/software-factory.md`
- Test: `crates/factory-engine/tests/sync_review.rs`, `crates/factory-cli/tests/sync_cmd.rs`

**Interfaces:**
- Consumes: `ReviewRun` (with `Round`, `Sync`, `Rebase`), the refusal `EngineError::Msg` texts of `sync_requested`, `CliError::Refused(String)`.
- Produces: the raw file names from Global Constraints, a `sync` command that maps the refusal errors of `sync_requested` to `CliError::Refused`, the user guide heading renumbered to fit the section order, and both docs describing the floor rule, the whole-branch review and the park behaviour.

- [ ] **Step 1: Read** `review.rs` around the raw file write, the `sync` function in `commands.rs`, and the user guide section that holds `6.6a`.
- [ ] **Step 2: Write the failing tests:**
  - `a_rebase_review_keeps_the_round_raw_files` (`tests/sync_review.rs`): with multiple reviewer roles, both `review-security-r1.raw.md` and `review-security-r1-rebase.raw.md` exist after a rebase review.
  - `sync_refusals_are_cli_refused` (`tests/sync_cmd.rs`): a merged job gives `CliError::Refused` with the text `job 001 is merged`; show one full case and list a cancelled job and a dirty worktree by name with their texts.
- [ ] **Step 3: Run `cargo test -j4 -p factory-engine --test sync_review a_rebase_review_keeps`.** Expected: FAIL because the rebase file is missing.
- [ ] **Step 4: Implement** the file name suffix by run kind, the refusal mapping, the heading renumbering and the doc paragraphs.
- [ ] **Step 5: Run `cargo test -j4 -p factory-engine` and `cargo test -j4 -p factory-cli`.** Expected: all pass.
- [ ] **Step 6: Run `cargo fmt` and `cargo clippy -j4 --all-targets -- -D warnings` for `factory-engine` and `factory-cli`, each with `-p`.** Expected: no output and no warnings.

Next plan: none
