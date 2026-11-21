# Cargo Lock Check Hardening, Plan 2: Stored Verdicts, Deleted Manifests, Ledger Line

> **Spec:** docs/superpowers/specs/2026-11-21-cargo-check-hardening-design.md
> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** The Cargo lock check stores only verdicts from the commit's own content, covers a deleted manifest only when the base lock lists it as a path package, and writes its ledger line once per run (issue #128).

**Commit:** fix(engine): store only content Cargo verdicts

**Architecture:** Spec sections 6 to 8. A pure `deleted_member` in `factory-core/src/cargo_lock.rs`; in `factory-engine/src/cargo_risk.rs`, `cargo_check` returns a `Stop` with a `StopKind`, `risk_in` stores only `Content` stops and passes, writes the ledger line itself and keeps a per-engine memo; `review.rs` stops writing the note.

**Tech Stack:** Rust, `toml` (already a factory-core dependency), `git2` and `cargo generate-lockfile --offline` in engine tests (as `tests/cargo_risk.rs` does today).

## Global Constraints

- No new dependency; no change to any `Cargo.toml` or `Cargo.lock`.
- The check only ever turns risky into normal; every new failure path stays risky.
- cargo commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Never shell out to git; git reads go through `factory_git::Repo`.
- Engine logic in `cargo_risk.rs` sits behind no port: it calls the concrete `factory_git::Repo` and `factory_checks` directly, as today.
- No board, ledger or events field is added. The memo is an in-memory `Engine` field, never written to disk.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- Content outcomes (stored) and transient outcomes (never stored) are exactly the two lists in spec section 6.
- `Risk.note` is kept for every outcome, so `risk_part` (`park.rs:376-380`) still shows it.

## Review Focus

1. A retry on the same head after a transient failure runs the check again and can clear the diff. Pinned by `a_transient_failure_is_not_stored` (no `cargo-check.txt` after the call).
2. A stored `risky` verdict read by a later stage writes no second ledger line but the park card still names it. Pinned by `a_stored_verdict_writes_no_ledger_line`, with the existing `a_park_card_uses_the_cargo_verdict`.
3. One engine asked three times (checks stage, review, park) with a transient failure runs the check once and writes one line. Pinned by `a_fresh_run_writes_one_ledger_line`.
4. A deleted virtual manifest (a nested workspace root with no `[package]`) keeps the diff risky. Pinned by `a_deleted_virtual_manifest_is_unknown` and `a_deleted_nested_workspace_manifest_parks`.
5. A deleted crate whose name appears in the base lock only with a `source` (an outside crate of the same name) is not covered. Pinned by `a_deleted_crate_with_a_source_entry_is_unknown`.

## File Structure

| File | Change |
|---|---|
| `crates/factory-core/src/cargo_lock.rs` | New `pub fn deleted_member(base_manifest: &str, base_lock: &str) -> CargoVerdict`; tests. |
| `crates/factory-core/src/lib.rs` | Re-export `deleted_member` next to `compare_locks`. |
| `crates/factory-engine/src/cargo_risk.rs` | `StopKind`, `Stop`, `content`, `transient`; `cargo_check` returns `Result<(), Stop>`; deleted-manifest rule; `risk_in` stores only content, writes the ledger line, uses the memo; tests. |
| `crates/factory-engine/src/lib.rs` | `Engine` field `pub(crate) cargo_memo: std::cell::RefCell<Option<(String, Risk)>>`, set to `RefCell::new(None)` in `Engine::new`. |
| `crates/factory-engine/src/review.rs:100-103` | Drop the `self.ruling(note)?` for the risk note. |
| `crates/factory-engine/tests/cargo_risk.rs` | Deleted-manifest tests, one-line test, metadata-failure store assertion. |
| `docs/reference/software-factory.md` | "once per check run" instead of "at each review stage"; only content verdicts are stored. |

---

### Task 1: `deleted_member` in factory-core

**Files:**
- Modify: `crates/factory-core/src/cargo_lock.rs` (new function after `compare_locks`, tests module)
- Modify: `crates/factory-core/src/lib.rs` (re-export)

**Interfaces:**
- Produces: `pub fn deleted_member(base_manifest: &str, base_lock: &str) -> CargoVerdict`. `Same` when `base_manifest` has `[package]` with a string `name` and `base_lock` has a `[[package]]` with that `name` and no `source` key. Otherwise `Unknown(reason)`, with one reason each for: the manifest does not parse; no `[package]` or no string `name`; the lock does not parse or has no `package` array; no sourceless entry with that name. Each reason names the package or the side, for example `crate old is not a path package of the base lock`.

- [ ] **Step 1: Read** `crates/factory-core/src/cargo_lock.rs` (the `CargoVerdict` type, `lock_entries`, the tests helpers `lock`, `ROOT`, `REG`, `libc_a`), and spec section 7.

- [ ] **Step 2: Write the failing tests** in `cargo_lock.rs` tests:

```rust
#[test]
fn a_deleted_member_in_the_base_lock_is_same() {
    let manifest = "[package]\nname = \"old\"\nversion = \"0.1.0\"\n";
    let base = lock(&[ROOT, "name = \"old\"\nversion = \"0.1.0\""]);
    assert_eq!(deleted_member(manifest, &base), CargoVerdict::Same);
}
```

| Test | `base_manifest` | `base_lock` entries | Expected |
|---|---|---|---|
| `a_deleted_virtual_manifest_is_unknown` | `"[workspace]\nmembers = [\"x\"]\n"` | `ROOT` | `Unknown(_)` |
| `a_deleted_crate_missing_from_the_lock_is_unknown` | package `old` | `ROOT` | `Unknown(_)` |
| `a_deleted_crate_with_a_source_entry_is_unknown` | package `libc` | `libc_a()` | `Unknown(_)` |
| `an_unparsable_base_lock_is_unknown` | package `old` | the text `"not = [toml"` instead of `lock(..)` | `Unknown(_)` |
| `an_unparsable_base_manifest_is_unknown` | `"[package"` | `ROOT` | `Unknown(_)` |

Each row asserts `matches!(deleted_member(..), CargoVerdict::Unknown(_))`. Two rows also assert the reason text: `a_deleted_crate_missing_from_the_lock_is_unknown` contains `old`, and `a_deleted_crate_with_a_source_entry_is_unknown` contains `libc`.

- [ ] **Step 3: Run them to see them fail.**

Run: `cargo test -j4 -p factory-core cargo_lock`
Expected: FAIL to compile with `cannot find function 'deleted_member' in this scope`.

- [ ] **Step 4: Implement `deleted_member`** with the `toml` parsing style `lock_entries` uses, and re-export it from `factory-core/src/lib.rs`.

- [ ] **Step 5: Run again.**

Run: `cargo test -j4 -p factory-core cargo_lock`
Expected: PASS, with the 6 new tests.

### Task 2: Content versus transient stops, deleted-manifest rule

**Files:**
- Modify: `crates/factory-engine/src/cargo_risk.rs` (`stays` and `outside_note`, `risk_in`, `cargo_check`, tests module; plan 1 has moved their line numbers, so find them by name)
- Modify: `crates/factory-engine/tests/cargo_risk.rs`

**Interfaces:**
- Consumes: `factory_core::deleted_member(base_manifest: &str, base_lock: &str) -> CargoVerdict` from the task above.
- Produces in `cargo_risk.rs`: `pub(crate) enum StopKind { Content, Transient }`, `pub(crate) struct Stop { pub note: String, pub kind: StopKind }`, `fn content(reason: impl Display) -> Stop`, `fn transient(reason: impl Display) -> Stop` (both note texts through `stays`), and `fn cargo_check(&self, paths: &[String], start: &str, head: &str, temp_root: &Path) -> Result<(), Stop>`. `verdict(v)` and `outside_note` results become `content` stops. `ExportDir::create` and `cargo_file` return `Result<_, Stop>` with `transient`.

- [ ] **Step 1: Read** `crates/factory-engine/src/cargo_risk.rs` whole, `crates/factory-engine/tests/cargo_risk.rs:1-160` (`fixture`, `change`, `run_job`, `check_line`, `assert_risky`, `package`, `lock_in`, `write_files`, `commit_all`, `commit_workspace`), and spec sections 6 and 7 with their two lists.

- [ ] **Step 2: Write the failing tests.** In `cargo_risk.rs` tests (temp root with a `Cargo.toml` above it, as `a_cargo_file_above_the_export_folder_is_unknown` builds it):

```rust
#[test]
fn a_transient_failure_is_not_stored() {
    let outer = tempfile::tempdir().expect("temp root");
    std::fs::write(outer.path().join("Cargo.toml"), "[workspace]\n").expect("write");
    let inner = outer.path().join("inner");
    std::fs::create_dir(&inner).expect("mkdir");
    let job = passing_job();
    assert!(risk_of(&job, &inner).is_risky());
    assert!(!job.job_dir.join(CARGO_CHECK_FILE).exists());
    let root = tempfile::tempdir().expect("clean root");
    assert!(!risk_of(&job, root.path()).is_risky());
}
```

| Test | Setup | Assertions |
|---|---|---|
| `a_content_verdict_is_stored` | `outside_job()`, clean temp root | risky; `cargo-check.txt` line 2 is `risky`, line 3 contains `outside crates added: libc 0.2.1` |

In `tests/cargo_risk.rs`, a builder effect that deletes files and regenerates the lock:

```rust
/// A builder that deletes `gone`, writes `files` and regenerates the lock.
fn delete_and_change(gone: Vec<&'static str>, files: Vec<(&'static str, String)>) -> Effect {
    Box::new(move |spec| {
        for g in &gone {
            let path = spec.cwd.join(g);
            if path.is_dir() {
                std::fs::remove_dir_all(&path).expect("remove dir");
            } else {
                std::fs::remove_file(&path).expect("remove file");
            }
        }
        write_files(&spec.cwd, &files);
        lock_in(&spec.cwd);
        "done".to_string()
    })
}
```

| Test | Setup | Assertions |
|---|---|---|
| `a_deleted_member_crate_merges` | `fixture()`; effect deletes `b` and writes root `Cargo.toml` with `members = ["a"]` | `run_job(..) == Decision::AutoMerge`; `check_line` is `PASS_LINE` |
| `a_deleted_nested_workspace_manifest_parks` | `fixture()`, then `commit_workspace(&fx, "nested/")` (a second workspace with its own lock, not a root member); effect deletes `nested/Cargo.toml` and adds a comment line to `a/Cargo.toml`, so the root workspace is touched | `assert_risky`; `check_line` contains `nested/Cargo.toml` (today it merges, because the folder prefix covers it) |
| `a_metadata_failure_after_a_same_lock_keeps_the_diff_risky` (existing) | as today | also `assert!(!fx.job_dir.join("cargo-check.txt").exists())` |

- [ ] **Step 3: Run them to see them fail.**

Run: `cargo test -j4 -p factory-engine --lib cargo_risk` and `cargo test -j4 -p factory-engine --test cargo_risk`
Expected: `a_transient_failure_is_not_stored` FAILS on `!exists()` (the file holds `risky`); `a_deleted_nested_workspace_manifest_parks` FAILS with an `AutoMerge` decision; the metadata test FAILS on the new assertion.

- [ ] **Step 4: Implement.**
  - Sort every `Err` in `cargo_check` into `content` or `transient` exactly as spec section 6 lists them.
  - Deleted manifests: for each deleted `Cargo.toml`, take the checked root with the longest prefix of its folder; no root keeps today's stop (as `content`). Read the manifest and `<root>/Cargo.lock` at `start` with `cargo_file` (a missing file is a `content` stop naming it), then `deleted_member(..)`; an `Unknown(reason)` becomes `content(format!("{path}: {reason}"))`, so the line names the deleted path. The reason comes from `deleted_member` (for a nested workspace root, its no-`[package]` reason), not from the spec's example sentence; tests match only the path.
  - `risk_in`: on `Ok`, store `same` with `PASS_NOTE`; on a `Content` stop, store `risky` with its note; on a `Transient` stop, store nothing. The returned `Risk` carries the note in every case.

- [ ] **Step 5: Run again.**

Run: `cargo test -j4 -p factory-engine --lib cargo_risk` and `cargo test -j4 -p factory-engine --test cargo_risk`
Expected: PASS, with the 4 new tests and the extended metadata test.

### Task 3: Ledger line once per run, per-engine memo

**Files:**
- Modify: `crates/factory-engine/src/lib.rs` (`Engine` struct :174-195 and `Engine::new`)
- Modify: `crates/factory-engine/src/cargo_risk.rs` (`risk_in`, tests module)
- Modify: `crates/factory-engine/src/review.rs:100-103`
- Modify: `crates/factory-engine/tests/cargo_risk.rs`
- Modify: `docs/reference/software-factory.md` (the paragraph ending "at each review stage, and a park repeats it in its Risk part", around line 266, and the store paragraph)

**Interfaces:**
- Consumes: `Stop`, `StopKind` and the storing `risk_in` from the task above; `Engine::ruling(&self, text: &str) -> Result<(), EngineError>` (`ports.rs:208`, `pub(crate)`).
- Produces: `pub(crate) cargo_memo: std::cell::RefCell<Option<(String, Risk)>>` on `Engine`, `None` at `Engine::new`. It must be `pub(crate)`, because `Risk` is `pub(crate)` and a `pub` field would raise the `private_interfaces` warning. `Risk` derives `Clone` already.

- [ ] **Step 1: Read** `crates/factory-engine/src/lib.rs:170-230`, `crates/factory-engine/src/review.rs:90-110`, `crates/factory-engine/src/recover.rs:355-362`, `crates/factory-engine/src/checks_stage.rs:275-290`, `crates/factory-engine/src/park.rs:315-382`, and spec section 8.

- [ ] **Step 2: Write the failing tests.** In `cargo_risk.rs` tests, with a helper that counts `cargo lock check` lines in `job.job_dir.join("progress.md")` (an empty count when the file is missing):

```rust
#[test]
fn a_fresh_run_writes_one_ledger_line() {
    let outer = tempfile::tempdir().expect("temp root");
    std::fs::write(outer.path().join("Cargo.toml"), "[workspace]\n").expect("write");
    let inner = outer.path().join("inner");
    std::fs::create_dir(&inner).expect("mkdir");
    let job = passing_job();
    let engine = job.engine(config());
    let paths = engine.job_changed_paths().expect("paths");
    for _ in 0..3 {
        assert!(engine.risk_in(&paths, &inner).is_risky());
    }
    assert_eq!(check_lines(&job), 1);
}
```

| Test | Setup | Assertions |
|---|---|---|
| `a_stored_verdict_writes_no_ledger_line` | `outside_job()`, `write_store(.., false, "stored note")` with the current key, clean temp root | risky, `note == Some("stored note")`, `check_lines(&job) == 0` |
| `a_stored_pass_writes_no_ledger_line` | `outside_job()`, `write_store(.., true, PASS_NOTE)` | normal, `check_lines(&job) == 0` |

In `tests/cargo_risk.rs`:

| Test | Setup | Assertions |
|---|---|---|
| `the_check_line_appears_once_per_head` | `fixture()`, `run_job(&fx, change(internal_dep(), true))`, then one more `fx.engine(&approving()).review_stage()`: two review stages on one head, each with its own engine, so the store (not the memo) keeps the second one silent | `AutoMerge`; the ledger has exactly one line containing `cargo lock check: ` |

- [ ] **Step 3: Run them to see them fail.**

Run: `cargo test -j4 -p factory-engine --lib cargo_risk` and `cargo test -j4 -p factory-engine --test cargo_risk the_check_line`
Expected: `a_fresh_run_writes_one_ledger_line` FAILS with `left: 0, right: 1` (nothing writes the line yet); `the_check_line_appears_once_per_head` FAILS with two lines.

- [ ] **Step 4: Implement.**
  - `risk_in`: after the key is known, return the memo's `Risk` when its key matches; on a store hit, return without a ledger line and without setting the memo (`a_park_card_uses_the_cargo_verdict` rewrites the store between two calls on one engine and relies on this); after a fresh `cargo_check`, write the note with `self.ruling(&note)`, log a failure with `tracing::warn!(event = "cargo_check.ledger", msg = %format!("cannot write the ledger: {e}"))` and keep the verdict, then set the memo to `(key, risk.clone())`.
  - Notes made in `risk_in` before `cargo_check` runs (a changed path outside the project, a `job_range` error, a git-diff error) write no ledger line, as spec section 8 says; the park card still shows them through `Risk.note`.
  - `review.rs`: remove the `if let Some(note) = &risk.note { self.ruling(note)?; }` block.
  - Docs: the ledger gets the line once each time the check runs, never from a stored verdict and never for a stop before the check runs (the park card shows those); only a pass and content verdicts are stored, so a cargo or disk failure runs again on retry.

- [ ] **Step 5: Run again.**

Run: `cargo test -j4 -p factory-engine` and `cargo test -j4 -p factory-cli`
Expected: PASS, including the 4 new tests. Then `cargo clippy -j4 -p factory-core -p factory-engine --tests` and `cargo fmt -p factory-core -p factory-engine -- --check`: no warnings, no diff.

Next plan: none
