# Cargo lock check hardening

Date: 2026-11-21. Issue #128, follow-ups from the reviews of the Cargo lock check (`docs/superpowers/specs/2026-11-21-cargo-lock-check-design.md`).

## Goal

Make the Cargo lock check store only verdicts it reached from the commit, cover a deleted manifest only when it was a member of a checked workspace, and close the six smaller gaps listed in issue #128.

## Overview

The Cargo lock check (`crates/factory-engine/src/cargo_risk.rs`) clears a risky diff whose only risky paths are Cargo files when no outside code came in. Its reviews left eight follow-ups. None lets outside code merge; each makes the check stricter than needed, repeat itself, or rest on an assumption that holds by luck:

- A transient failure (export folder, temp disk, a cargo download) is stored as `risky` for the head, so a retry on that head never runs the check again and the job parks for a reason that is already gone.
- A deleted `Cargo.toml` of a separate nested or excluded workspace counts as covered by the root check, because the test is a folder prefix, not membership.
- The ledger line is written again at every review stage of the same head.
- Five small items: the git diff runs before the cheap gate, the store key can collide, `cargo_clearable` is true when no glob matches, the built-in risk list lacks two rust patterns, and a checksum-only lock change reads as "added and removed".

## Current state

| Piece | Where | Today |
|---|---|---|
| Verdict and store | `cargo_risk.rs:186` `risk_in` | Runs `job_changed_paths_raw()` (a git diff) first, then classifies; reads `cargo-check.txt`; on a miss runs `cargo_check` and stores `same` or `risky` with the note, whatever the failure was. |
| Check | `cargo_risk.rs:261` `cargo_check` | Returns `Result<(), String>`; every failure, from git reads to `cargo metadata`, is the same `Err(note)`. |
| Store key | `cargo_risk.rs:74` `config_text`, `:83` `store_key` | `key <head> cargo_lock_check=<bool>; risk_paths=<globs joined with ",">`. |
| Deleted manifests | `cargo_risk.rs:338` | Covered when `roots.iter().any(\|r\| dir.starts_with(r))`. |
| Lock compare | `factory-core/src/cargo_lock.rs:86` `compare_locks` | Entries are `(name, version, source, checksum)`; `added` and `removed` are the `name version` of the set differences, so a checksum or source change lists the same name in both. |
| Clearable | `factory-core/src/risk.rs:84` `cargo_clearable` | `.filter(matching globs).all(cargo-only)`, true when no glob matches. |
| Built-in list | `factory-core/src/config.rs:33` `DEFAULT_RISK` | Has `**/Cargo.toml` and `**/Cargo.lock`, lacks `**/.cargo/**` and `**/rust-toolchain*` (the rust preset has them). |
| Ledger line | `review.rs:100-103` | `if let Some(note) = &risk.note { self.ruling(note)?; }` at every review stage. |
| Other readers | `checks_stage.rs:283`, `park.rs:319`, `park.rs:360` | Call `self.risk(..)`; `risk_part` appends `risk.note` to the park card (`park.rs:376-380`). |

Gaps: a stored transient failure blocks a retry; a nested workspace's deleted crate is waved through; the ledger is noisy; the key, the clearable rule and the built-in list rest on assumptions; the ledger wording misleads.

## Non-goals

- No change to which Cargo diffs pass the check when every step succeeds.
- No new cargo call, no export of the base commit, no new dependency.
- No change to how other risky paths (`*auth*`, `.github/**`) are judged.

## Design

```
risk_in(paths)
  classify -> not risky: return
  risky Cargo-only? (any && all)  -> no: risky, no cargo
  outside-of-scope git diff        -> moved here, after the gate
  key = store_key(head, config)    -> length-prefixed globs
  memo hit (same engine, same key) -> return it
  store hit                        -> return it (no ledger line)
  run cargo_check -> Ok | Stop { note, kind }
    kind Content   -> store it, write ledger line
    kind Transient -> do not store, write ledger line, memo it
```

### 1. Gate before the git diff

`risk_in` calls `job_changed_paths_raw()` and `out_of_scope` only after `cargo_lock_check` is on and every risky path is `cargo_clearable`. A diff that stays risky without cargo runs no git diff.

### 2. Store key without a separator collision

`config_text` writes each glob as `<byte length>:<glob>`, concatenated, so no glob text can fake a boundary: `cargo_lock_check=true; risk_paths=13:**/Cargo.toml13:**/Cargo.lock`. A store written with the old key never matches, so the check runs once again on old jobs. That is accepted.

### 3. `cargo_clearable` needs a matching glob

`cargo_clearable(path, config) -> bool` is true only when at least one `risk_paths` glob matches the path and every matching glob names Cargo files alone. A Cargo path no glob matches returns false.

### 4. Built-in risk list

`DEFAULT_RISK` gains `**/.cargo/**` and `**/rust-toolchain*`, the same strings as the rust preset (`crates/factory-core/presets/rust/config.toml`).

### 5. Checksum or source changes

`LockDiff::Changed` gains `changed: Vec<String>`: the sorted `name version` strings that appear in both the added and the removed set. They are taken out of `added` and `removed`. `Changed` is returned when any of the three lists is non-empty. The ledger line becomes:

`cargo lock check: outside crates added: <list|none>; removed: <list|none>; source or checksum changed: <list|none>, diff stays risky`

Example: base `libc 0.2.1` with checksum `aaaa`, head the same with `dddd` gives `added: none; removed: none; source or checksum changed: libc 0.2.1`.

### 6. Stored versus transient outcomes

`cargo_check` returns `Result<(), Stop>` with

```rust
pub(crate) enum StopKind { Content, Transient }
pub(crate) struct Stop { pub note: String, pub kind: StopKind }
```

Two helpers build it: `content(reason)` and `transient(reason)`, both through `stays` for the note text. Content outcomes, stored:

- the repo has git submodules;
- a `Cargo.lock` has no `Cargo.toml` beside it at head;
- a workspace root outside the project; no manifest or lock left to check;
- a deleted manifest that is not covered (section 7); a lock that is not the lock of a touched workspace;
- the base or head lock missing; `compare_locks` giving `Changed` or `Unknown`;
- `check_path_packages` and `manifest_keys_diff` giving `Changed` or `Unknown`.

Transient outcomes, never stored: reading `.gitmodules` or any Cargo file fails; resolving the temp folder; a Cargo file above the export folder; making the export folder; exporting the head; the project folder missing in the export; resolving a workspace root; `workspace_root` (`cargo locate-project`) failing; `locked_path_packages` (`cargo metadata --locked`) failing.

A pass is stored as `same`, as today. `Risk` keeps its `note` for every outcome, so the park card still names the reason.

### 7. Deleted manifest coverage

For each deleted `Cargo.toml` at path `P`, pick the checked root with the longest prefix of `P`'s folder. A new pure function in `factory-core/src/cargo_lock.rs`:

```rust
pub fn deleted_member(base_manifest: &str, base_lock: &str) -> CargoVerdict
```

It returns `Same` when `base_manifest` has a `[package]` with a string `name` and `base_lock` has a `[[package]]` with that `name` and no `source`. It returns `Unknown(reason)` when the manifest has no `[package]` or no name (a deleted workspace root), when either text does not parse, or when no sourceless entry has that name. The engine reads `P` and `<root>/Cargo.lock` at the start commit; a missing base file is `Unknown`. A deleted manifest under no checked root keeps today's stop. Every such stop is a Content outcome.

Example: root lock lists `name = "old"` with no source and `crates/old/Cargo.toml` was deleted: covered. `nested/Cargo.toml` with `[workspace]` and its own lock was deleted: `nested/Cargo.toml was deleted outside the checked workspaces`, diff stays risky.

### 8. Ledger line once per run

`risk_in` writes the note to the ledger itself, through `ruling_best_effort` (`recover.rs:357`), when it ran `cargo_check` in this call. A verdict read from the store or the memo writes nothing. `review.rs` drops its own `self.ruling(note)?` for the risk note. Whichever stage first runs the check writes the line.

The engine holds a memo, `cargo_memo: RefCell<Option<(String, Risk)>>`, set after every run with the store key. Within one engine, a repeat call with the same key returns the memo, so a transient failure runs cargo and writes its line once per engine, not once per caller (checks stage, review, park).

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| `cargo metadata` fails because the download dropped | Transient: not stored, line written, a retry on the same head runs the check again. |
| `cargo metadata --locked` fails because the lock is stale | Transient as well (same error type); reruns once per engine. |
| A lock gains an outside crate | Content: stored `risky`, line written once. |
| A retry on the same head after a stored `risky` | Store hit: verdict reused, no ledger line, park card still shows the note. |
| Store write fails | Warned in the log, verdict unchanged (today's behaviour). |
| Ledger write fails | Warned in the log, verdict unchanged. |
| Old store file with a comma-joined key | No match, the check runs and rewrites the file. |
| A deleted manifest under two nested roots | The longest matching root decides. |
| A deleted virtual manifest (no `[package]`) | Unknown, stays risky. |
| A Cargo path that no `risk_paths` glob matches | Not clearable; such a path is not risky anyway. |
| Checksum and version both change | Lists differ in version, so they stay in `added` and `removed`. |

## Testing

`factory-core`, `cargo test -j4 -p factory-core`:

- `risk.rs`: `a_cargo_path_no_glob_matches_is_not_clearable` (empty `risk_paths`, `Cargo.toml` gives false); the existing `cargo_files_under_cargo_only_globs_are_clearable` still passes.
- `config.rs`: `default_risk_lists_cargo_config_and_toolchain` (both strings in `Config::default().risk_paths`).
- `cargo_lock.rs`: `a_changed_checksum_is_changed` now asserts `changed == ["libc 0.2.1"]` and empty `added` and `removed`; `a_changed_source_lists_it_as_changed`; `a_version_bump_shows_both_versions` keeps both lists.
- `cargo_lock.rs`: `a_deleted_member_in_the_base_lock_is_same`, `a_deleted_virtual_manifest_is_unknown`, `a_deleted_crate_missing_from_the_lock_is_unknown`, `a_deleted_crate_with_a_source_entry_is_unknown`, `an_unparsable_base_lock_is_unknown`.

`factory-engine` unit tests in `cargo_risk.rs` (reuse `engine_with_job`, `outside_job`, `passing_job`), `cargo test -j4 -p factory-engine --lib cargo_risk`:

- `store_key_length_prefixes_each_glob` (exact key text for two globs; `["a,b"]` and `["a", "b"]` give different keys).
- `a_transient_failure_is_not_stored` (temp root with a Cargo file above it: risky, no `cargo-check.txt`).
- `a_content_verdict_is_stored` (outside crate: store holds `risky` and the note).
- `a_stored_verdict_writes_no_ledger_line` (store written first: no `Ruling:` line in `progress.md`).
- `a_fresh_run_writes_one_ledger_line` (two `risk_in` calls on one engine: one line, cargo run once).
- `a_risky_diff_without_cargo_runs_no_git_diff` is checked through the existing `a_cargo_file_beside_another_risky_path_stays_risky_without_cargo`.
- Existing `a_stored_verdict_for_the_same_key_is_reused` and `a_park_card_uses_the_cargo_verdict` keep passing.

`factory-engine` integration tests in `tests/cargo_risk.rs` (reuse `fixture`, `run_job`, `change`, `check_line`), `cargo test -j4 -p factory-engine --test cargo_risk`:

- `a_deleted_member_crate_merges` (a workspace member crate removed with its lock entry: `AutoMerge`).
- `a_deleted_nested_workspace_manifest_parks` (a nested workspace with its own lock deleted: risky, line names the path).
- `a_new_config_runs_the_check_again` updated for the length-prefixed key.
- `a_metadata_failure_after_a_same_lock_keeps_the_diff_risky` additionally asserts that `cargo-check.txt` does not exist.
- `the_check_line_appears_once_per_head` (checks stage and review on one head: one line).

## Files changed

- `crates/factory-core/src/risk.rs`, `crates/factory-core/src/config.rs`, `crates/factory-core/src/cargo_lock.rs`
- `crates/factory-engine/src/cargo_risk.rs`, `crates/factory-engine/src/review.rs`, `crates/factory-engine/src/lib.rs` (memo field)
- `crates/factory-engine/tests/cargo_risk.rs`
- `docs/reference/software-factory.md` (ledger line wording, "at each review stage" becomes "once per check run", the key format)

## Constraints

- No new dependency; no change to any `Cargo.toml` or `Cargo.lock`.
- The check only ever turns risky into normal; every new failure path stays risky.
- cargo: always `-j4`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Never shell out to git; git reads go through `factory_git::Repo`.

## Acceptance criteria

- A transient failure leaves no `cargo-check.txt`, and a retry on the same head runs the check again.
- A content verdict and a pass are stored and reused for the same head and config.
- A deleted workspace member whose base lock lists it without a source is covered; a deleted nested or virtual manifest keeps the diff risky.
- The ledger gets the check's line once per run, never from a stored verdict.
- `risk_in` runs no git diff for a diff that stays risky without cargo.
- `["a,b"]` and `["a", "b"]` give different store keys.
- `cargo_clearable` is false for a Cargo path no glob matches.
- `Config::default().risk_paths` contains `**/.cargo/**` and `**/rust-toolchain*`.
- A checksum-only lock change reads `source or checksum changed: <name version>`.
- `cargo test -j4 -p factory-core` and `cargo test -j4 -p factory-engine` pass; clippy and fmt are clean.

## Out of scope

- Telling a stale-lock `cargo metadata` failure apart from a network failure.
- Checking membership with `cargo metadata` on the base commit.

## Decisions

- How much of #128 does this job cover? Options: all eight items as two plans (small items first, then the transient split and the deleted-manifest rule), only the three behaviour changes, all eight as one plan. Chosen: all eight items as two plans. Decided by: user
- When does a deleted `Cargo.toml` count as covered? Options: its base `[package] name` is a sourceless entry in the base `Cargo.lock` of the checked root above it (text only, no cargo run); every deleted manifest stays risky; run `cargo metadata` on an export of the base and check membership. Chosen: the base lock entry, text only. Decided by: user
- Which outcomes are stored in `cargo-check.txt`? Options: only outcomes from the commit's own content (pass, lock comparison, structural checks, file-layout refusals), never cargo-call or I/O failures; also store a `metadata` failure whose text says the lock needs updating; store only a pass. Chosen: only outcomes from the commit's content. Decided by: user
- When is the check's ledger line written? Options: only when the check runs, never from a stored verdict; once per head by searching the ledger; at every review, marked `(stored)`. Chosen: only when the check runs. Decided by: user
- Who writes the ledger line? Options: `risk_in` itself when it ran the check; the review stage only. Chosen: `risk_in`, because the checks stage reads the risk first and would store the verdict before the review sees it. Decided by: call
- How are repeat transient failures within one driver call kept from running cargo several times? Options: a per-engine memo keyed by the store key; no memo. Chosen: the memo, so the checks stage, review and park run cargo once. Decided by: call
- How does the store key avoid collisions? Options: length-prefixed globs; a hash. Chosen: length-prefixed globs, readable and with no new dependency. Decided by: call
- Which root decides a deleted manifest under nested roots? Options: the longest prefix; any root. Chosen: the longest prefix. Decided by: call
