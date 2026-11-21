# Cargo Lock Check Hardening, Plan 1: Small Gaps

> **Spec:** docs/superpowers/specs/2026-11-21-cargo-check-hardening-design.md
> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Close the five small gaps of issue #128: the clearable rule needs a matching glob, the built-in risk list gains two rust patterns, a checksum or source change gets its own ledger list, the git diff runs after the cheap gate, and the store key cannot collide.

**Commit:** fix(engine): close small Cargo lock check gaps

**Architecture:** Spec sections 1 to 5. Pure changes in `factory-core` (`risk.rs`, `config.rs`, `cargo_lock.rs`) and two small changes in `factory-engine/src/cargo_risk.rs` (`risk_in` order, `config_text`). No new type except one field on `LockDiff::Changed`.

**Tech Stack:** Rust, `toml` (already a factory-core dependency), `git2` in engine tests.

## Global Constraints

- No new dependency; no change to any `Cargo.toml` or `Cargo.lock`.
- The check only ever turns risky into normal; every new failure path stays risky.
- cargo commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Never shell out to git; git reads go through `factory_git::Repo`.
- Engine logic in `cargo_risk.rs` sits behind no port: it calls the concrete `factory_git::Repo` and `factory_checks` directly, as today.
- No board, ledger or events field is added.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- Ledger line for a changed lock, exactly: `cargo lock check: outside crates added: <list|none>; removed: <list|none>; source or checksum changed: <list|none>, diff stays risky`.
- Store key, exactly: `key <head> cargo_lock_check=<bool>; risk_paths=<len>:<glob><len>:<glob>...`, where `<len>` is the glob's byte length.

## Review Focus

1. A lock where one crate changes checksum and another is newly added: `added` lists only the new one, `changed` only the first. Pinned by `a_checksum_change_beside_an_added_crate_splits_the_lists`.
2. A glob that contains a comma (`["a,b"]` against `["a", "b"]`) must give a different key. Pinned by `store_key_length_prefixes_each_glob`.
3. A glob with a multi-byte character: the prefix is the byte length, not the char count. Pinned by `store_key_counts_bytes`.
4. An empty `risk_paths` with a Cargo path: not clearable. Pinned by `a_cargo_path_no_glob_matches_is_not_clearable`.
5. A diff whose risky paths include a non-Cargo path runs no git diff and no cargo. The existing `a_cargo_file_beside_another_risky_path_stays_risky_without_cargo` (engine `tests/cargo_risk.rs`) keeps passing, but it passes with either order, so the reviewer checks the moved call by reading `risk_in`.

## File Structure

| File | Change |
|---|---|
| `crates/factory-core/src/risk.rs` | `cargo_clearable` needs a matching glob; one test. |
| `crates/factory-core/src/config.rs` | `DEFAULT_RISK` gains two strings; one test. |
| `crates/factory-core/src/cargo_lock.rs` | `LockDiff::Changed` gains `changed`; `compare_locks` fills it; tests. |
| `crates/factory-engine/src/cargo_risk.rs` | `outside_note` takes `changed`; `risk_in` gates before the git diff; `config_text` length-prefixes globs; tests. |
| `crates/factory-engine/tests/cargo_risk.rs` | `a_new_config_runs_the_check_again` uses the new key text. |
| `docs/reference/software-factory.md` | The changed-lock ledger line and the key format (around lines 268-272). |

---

### Task 1: Core rules: clearable, built-in list, changed crates

**Files:**
- Modify: `crates/factory-core/src/risk.rs:84-98` (`cargo_clearable`) and its tests module
- Modify: `crates/factory-core/src/config.rs:33` (`DEFAULT_RISK`) and its tests module
- Modify: `crates/factory-core/src/cargo_lock.rs:7-16` (`LockDiff`), `:86-102` (`compare_locks`) and its tests module
- Modify: `crates/factory-engine/src/cargo_risk.rs:50-62` (`outside_note`) and `:366` (its call)
- Modify: `crates/factory-engine/tests/cargo_risk.rs:199-202` (`an_outside_crate_in_the_lock_parks_naming_it`, expected line)
- Modify: `docs/reference/project-toml-reference.md:146` (`risk_paths` default count)

**Interfaces:**
- Produces: `LockDiff::Changed { added: Vec<String>, removed: Vec<String>, changed: Vec<String> }`, each a sorted list of unique `name version` strings; a `name version` in both set differences goes only to `changed`.
- Produces: `fn outside_note(added: &[String], removed: &[String], changed: &[String]) -> String` in `cargo_risk.rs`.
- `cargo_clearable(path: &str, config: &Config) -> bool` keeps its signature.

- [ ] **Step 1: Read** `crates/factory-core/src/risk.rs:60-120`, `crates/factory-core/src/cargo_lock.rs:1-110` and its tests (`lock`, `ROOT`, `REG`, `libc_a`, `libc_b`), `crates/factory-core/src/config.rs:30-60`, `crates/factory-engine/src/cargo_risk.rs:44-70` and `:355-370`, and spec sections 3 to 5.

- [ ] **Step 2: Write the failing tests.**

In `risk.rs` tests:

```rust
#[test]
fn a_cargo_path_no_glob_matches_is_not_clearable() {
    let config = Config {
        risk_paths: Vec::new(),
        ..Config::default()
    };
    assert!(!cargo_clearable("Cargo.toml", &config));
    assert!(!cargo_clearable("crates/a/Cargo.lock", &config));
}
```

In `config.rs` tests:

```rust
#[test]
fn default_risk_lists_cargo_config_and_toolchain() {
    let paths = Config::default().risk_paths;
    assert!(paths.iter().any(|p| p == "**/.cargo/**"), "{paths:?}");
    assert!(paths.iter().any(|p| p == "**/rust-toolchain*"), "{paths:?}");
}
```

In `cargo_lock.rs` tests, change `a_changed_checksum_is_changed` and `a_version_bump_shows_both_versions`, and add two:

```rust
#[test]
fn a_changed_checksum_is_changed() {
    let other = libc_a().replace("aaaa", "dddd");
    assert_eq!(
        compare_locks(&lock(&[&libc_a()]), &lock(&[&other])),
        LockDiff::Changed {
            added: vec![],
            removed: vec![],
            changed: vec!["libc 0.2.190".into()],
        }
    );
}
```

| Test | Base entries | Head entries | Expected |
|---|---|---|---|
| `a_version_bump_shows_both_versions` | `ROOT`, `libc_a()` | `ROOT`, `libc_b()` | `added ["libc 0.2.191"]`, `removed ["libc 0.2.190"]`, `changed []` |
| `a_changed_source_lists_it_as_changed` | `libc_a()` | `libc_a()` with `REG` replaced by `"git+https://example.com/libc#abc"` | `added []`, `removed []`, `changed ["libc 0.2.190"]` |
| `a_checksum_change_beside_an_added_crate_splits_the_lists` | `ROOT`, `libc_a()` | `ROOT`, `libc_a()` with `aaaa` replaced by `dddd`, `hook()` | `added ["signal-hook 0.3.17"]`, `removed []`, `changed ["libc 0.2.190"]` |

Each row is written like the full example, with `assert_eq!` on the whole `LockDiff::Changed`. `a_new_registry_crate_is_added` (`cargo_lock.rs:208-218`), the other test that builds a two-field `LockDiff::Changed`, gets `changed: vec![]`.

In `crates/factory-engine/tests/cargo_risk.rs`, `an_outside_crate_in_the_lock_parks_naming_it` expects exactly:
`"cargo lock check: outside crates added: signal-hook 0.3.17; removed: none; source or checksum changed: none, diff stays risky"`.

In `cargo_risk.rs` tests:

```rust
#[test]
fn the_outside_note_names_changed_crates() {
    let note = outside_note(&[], &[], &["libc 0.2.190".to_string()]);
    assert_eq!(
        note,
        "cargo lock check: outside crates added: none; removed: none; source or checksum changed: libc 0.2.190, diff stays risky"
    );
}
```

- [ ] **Step 3: Run them to see them fail.**

Run: `cargo test -j4 -p factory-core` and `cargo test -j4 -p factory-engine --lib cargo_risk`
Expected: FAIL to compile with `struct variant LockDiff::Changed has no field named changed` and `this function takes 2 arguments but 3 arguments were supplied`; once those compile, `a_cargo_path_no_glob_matches_is_not_clearable` and `default_risk_lists_cargo_config_and_toolchain` fail on their asserts.

- [ ] **Step 4: Implement.**
  - `cargo_clearable`: the matching globs must be non-empty and all Cargo-only (`any && all`).
  - `DEFAULT_RISK`: add `"**/.cargo/**"` and `"**/rust-toolchain*"` after `"**/Cargo.lock"`.
  - `compare_locks`: compute the two `name version` lists as today, move every string present in both into `changed`, keep all three sorted and unique, and return `Same` only when all three are empty.
  - `outside_note(added, removed, changed)`: the exact line from Global Constraints, `none` for an empty list; the call at `cargo_risk.rs:366` passes `changed`.
  - Docs: `project-toml-reference.md:146` says `19 built-in globs` instead of `17 built-in globs`.

- [ ] **Step 5: Run again.**

Run: `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-engine --lib cargo_risk` and `cargo test -j4 -p factory-engine --test cargo_risk`
Expected: PASS, including these 8 new or changed tests: `a_cargo_path_no_glob_matches_is_not_clearable`, `default_risk_lists_cargo_config_and_toolchain`, `a_changed_checksum_is_changed`, `a_version_bump_shows_both_versions`, `a_new_registry_crate_is_added`, `a_changed_source_lists_it_as_changed`, `a_checksum_change_beside_an_added_crate_splits_the_lists`, `the_outside_note_names_changed_crates`, plus `an_outside_crate_in_the_lock_parks_naming_it` with its new line.

### Task 2: Engine: gate before the git diff, length-prefixed key

**Files:**
- Modify: `crates/factory-engine/src/cargo_risk.rs:74-85` (`config_text`, `store_key`), `:186-216` (`risk_in`) and its tests module
- Modify: `crates/factory-engine/tests/cargo_risk.rs` (`a_new_config_runs_the_check_again`)
- Modify: `docs/reference/software-factory.md` (the ledger lines list and the `cargo-check.txt` paragraph, around lines 268-272)

**Interfaces:**
- Consumes: `outside_note(added, removed, changed)` from the task above.
- `store_key(head: &str, config: &Config) -> String` keeps its signature; only its text changes.

- [ ] **Step 1: Read** `crates/factory-engine/src/cargo_risk.rs:70-120` and `:180-260`, its tests helpers (`config`, `outside_job`, `risk_of`, `is_empty`), `crates/factory-engine/tests/cargo_risk.rs:355-375`, and spec sections 1 and 2.

- [ ] **Step 2: Write the failing tests** in `cargo_risk.rs` tests:

```rust
#[test]
fn store_key_length_prefixes_each_glob() {
    let with = |globs: &[&str]| Config {
        risk_paths: globs.iter().map(|g| g.to_string()).collect(),
        ..Config::default()
    };
    assert_eq!(
        store_key("abc", &with(&["**/Cargo.toml", "**/Cargo.lock"])),
        "key abc cargo_lock_check=true; risk_paths=13:**/Cargo.toml13:**/Cargo.lock"
    );
    assert_ne!(
        store_key("abc", &with(&["a,b"])),
        store_key("abc", &with(&["a", "b"]))
    );
}

#[test]
fn store_key_counts_bytes() {
    let config = Config {
        risk_paths: vec!["é/**".to_string()],
        ..Config::default()
    };
    assert!(store_key("h", &config).ends_with("risk_paths=5:é/**"));
}
```

In `tests/cargo_risk.rs`, `a_new_config_runs_the_check_again` builds its expected text from the globs:

```rust
let prefixed: String = GLOBS
    .iter()
    .chain(["x/**"].iter())
    .map(|g| format!("{}:{g}", g.len()))
    .collect();
let expected = format!("cargo_lock_check=true; risk_paths={prefixed}");
```

- [ ] **Step 3: Run them to see them fail.**

Run: `cargo test -j4 -p factory-engine --lib cargo_risk store_key`
Expected: FAIL, `assertion left == right failed` with the comma-joined key on the left.

- [ ] **Step 4: Implement.**
  - `config_text`: each glob becomes `format!("{}:{g}", g.len())`, concatenated with no separator.
  - `risk_in`: move the `job_changed_paths_raw()` / `out_of_scope` call below the `cargo_lock_check` and `cargo_clearable` gate, so a diff that stays risky without cargo runs no git diff. The outside-of-scope result is matched right after the gate, as today.
  - Docs: in `software-factory.md:266`, "The rust preset marks `**/.cargo/**` and `**/rust-toolchain*` risky" becomes "The built-in list and the rust preset mark `**/.cargo/**` and `**/rust-toolchain*` risky". The changed-lock ledger line gains `; source or checksum changed: <name version, ...|none>`, and the store paragraph says line 1 is `key <head sha> cargo_lock_check=<bool>; risk_paths=<len>:<glob>...` with `<len>` the glob's byte length.

- [ ] **Step 5: Run again.**

Run: `cargo test -j4 -p factory-engine --lib cargo_risk` and `cargo test -j4 -p factory-engine --test cargo_risk`
Expected: PASS, including the 2 new key tests, `a_new_config_runs_the_check_again` and `a_cargo_file_beside_another_risky_path_stays_risky_without_cargo`. Then `cargo clippy -j4 -p factory-core -p factory-engine --tests` and `cargo fmt -p factory-core -p factory-engine -- --check`: no warnings, no diff.

Next plan: docs/superpowers/plans/2026-11-21-cargo-check-hardening/plan-02-cargo-check-hardening.md
