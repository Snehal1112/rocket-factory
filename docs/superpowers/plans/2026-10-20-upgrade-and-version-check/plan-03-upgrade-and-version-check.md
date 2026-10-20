# Upgrade and version check, Plan 3: Preset manifest and refresh

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Record a SHA-256 hash of every preset file the factory writes in `<home>/presets/.manifest`, so `factory presets refresh` (and later `factory upgrade`) can tell an unedited older file from a hand edit without a hand-kept list, and add `preset::plan_refresh` as a dry run that writes nothing.

**Architecture:** All logic stays in `factory-core`. A new private submodule `crates/factory-core/src/preset/manifest.rs` (declared `mod manifest;` in `preset.rs`) reads, parses, renders and atomically writes the manifest and hashes bytes with `sha2`. `preset::seed` and `preset::refresh` build a fresh manifest from what they find and write. `refresh` is split into a private planning step, shared with the new `pub fn plan_refresh`, and an apply step. `factory-cli` is unchanged; its output is pinned by a golden test. No `factory-engine` code is touched, so no port is involved, and no board, ledger or events field is added.

**Tech Stack:** Rust, `sha2` 0.10.9 (pinned `=0.10.9`, already in `~/.cargo/registry` with `digest` 0.10.7, `block-buffer` 0.10.4, `crypto-common` 0.1.7, `generic-array` 0.14.7, `typenum`, `cpufeatures` 0.2.17 and `cfg-if` 1.0.5, so it resolves offline), `tempfile` for tests.

**Spec:** docs/superpowers/specs/2026-10-20-upgrade-and-version-check-design.md

## Global Constraints

- Always `-j4`; never `--workspace` or `--all`; never set `CARGO_TARGET_DIR`. Use per-crate commands: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`. If the crate index cannot be reached, add `--offline`; every crate `sha2` needs is in the local registry.
- Paths reach the driver through `Home` and `Layout` only (`crates/factory-cli/tests/no_hardcoded_paths.rs`). The manifest path is always `presets_dir.join(MANIFEST_FILE)`, and callers pass `home.presets_dir()`.
- `factory-core` stays free of I/O-heavy dependencies; `sha2` is pure. Add it to `[workspace.dependencies]` in the root `Cargo.toml` as `sha2 = "=0.10.9"` with a short comment, and to `crates/factory-core/Cargo.toml` as `sha2.workspace = true`. Hex encoding uses `format!("{:02x}")`; no `hex` crate.
- `factory presets refresh` output, and the existing `install` messages, keep their text. `RefreshReport` keeps its four fields and `refresh(presets_dir: &Path, force: bool) -> Result<RefreshReport, CoreError>` keeps its signature.
- `upgrade` never overwrites an edited preset file. In this plan: `plan_refresh` takes no `force` and always plans as `force = false`.
- Each task lists the tests it adds. Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- No task touches `factory-engine`, so no port is named. No board, ledger or events field is added.
- Comments are short full sentences ending with a punctuation mark.

## Review Focus

1. A manifest that lists one path twice, even with the same hash. Expected: that path counts as unmanifested, so a file that differs from the embedded text and is no legacy text is refused. Pinned by `a_path_listed_twice_counts_as_unmanifested` (Task 1).
2. A manifest line with bytes that are not UTF-8, an upper-case or short hash, one space instead of two, an empty path, an absolute path or a `..` component. Expected: the line is ignored, other lines still count. Pinned by `bad_lines_are_ignored_and_good_lines_still_count` and `a_non_utf8_line_is_ignored` (Task 1).
3. `.manifest` exists but cannot be read (it is a directory). Expected: `refresh` returns `CoreError::Io` naming the manifest path before it writes any preset file. Pinned by `refresh_fails_before_writing_when_the_manifest_cannot_be_read` (Task 2).
4. A refused file. Expected: its old manifest entry is kept byte for byte, so a later run still treats it as edited and `--force` is the only way past it. Pinned by `an_edited_file_with_a_manifest_entry_is_refused` (Task 2).
5. A manifest entry for a path no preset ships (for example `rust/agents/retired.md`). Expected: dropped when the manifest is rewritten; the file on disk is left alone. Pinned by `unknown_manifest_entries_are_dropped_on_write` (Task 2).
6. A run that changes nothing. Expected: the manifest file is not rewritten (`Manifest::write` returns `false`). Pinned by `write_skips_an_identical_file` (Task 1).
7. `plan_refresh` on a presets folder that does not exist. Expected: the report lists all 12 files as written and the folder is still absent afterwards. Pinned by `plan_refresh_on_a_missing_dir_creates_nothing` (Task 3).
8. `.manifest` never appears in a report or in the command output, and the 12-file counts stay. Pinned by `refresh_output_matches_the_golden_text` (Task 2) and the existing `refresh_writes_absent_files` and `refresh_skips_equal_files`.

---

## File Structure

- `Cargo.toml` (modify): workspace dependency `sha2 = "=0.10.9"`.
- `Cargo.lock` (modify, by cargo): the `sha2` 0.10.9 entries and their dependencies.
- `crates/factory-core/Cargo.toml` (modify): `sha2.workspace = true`.
- `crates/factory-core/src/preset/manifest.rs` (new): `MANIFEST_FILE` is re-exported from `preset`, plus `sha256_hex` and `Manifest` with its unit tests.
- `crates/factory-core/src/preset.rs` (modify): `mod manifest;`, `pub use manifest::MANIFEST_FILE;`, `seed` and `refresh` record hashes, the new unedited rule, `plan_refresh`, unit tests.
- `crates/factory-cli/tests/presets_refresh.rs` (modify): the golden output test. `crates/factory-cli/src/presets.rs` is not changed.

### Task 1: The manifest module and the sha2 dependency

**Files:**
- Modify: `Cargo.toml`, `Cargo.lock`, `crates/factory-core/Cargo.toml`, `crates/factory-core/src/preset.rs` (only the `mod manifest;` and `pub use manifest::MANIFEST_FILE;` lines)
- Create: `crates/factory-core/src/preset/manifest.rs`

**Interfaces:**
- Consumes: `crate::write_atomic(path: &Path, bytes: &[u8]) -> Result<(), CoreError>` (`crates/factory-core/src/fsutil.rs`), `CoreError::io(path: &Path, source: std::io::Error) -> CoreError` (`crates/factory-core/src/error.rs`).
- Produces (in `crates/factory-core/src/preset/manifest.rs`):
  - `pub const MANIFEST_FILE: &str = ".manifest";`, re-exported as `factory_core::preset::MANIFEST_FILE`.
  - `pub(crate) fn sha256_hex(bytes: &[u8]) -> String`: 64 lower-case hex characters.
  - `#[derive(Debug, Default, Clone, PartialEq, Eq)] pub(crate) struct Manifest { entries: BTreeMap<String, String> }`, keyed by the path relative to `presets_dir` with `/` separators (for example `rust/agents/factory-builder.md`), value the hash.
  - `pub(crate) fn read(presets_dir: &Path) -> Result<Manifest, CoreError>`: a missing file gives an empty manifest; any other read error is `CoreError::io(<manifest path>, e)`. Bytes are split on `\n`; each line is parsed on its own (a non-UTF-8 line is bad). A good line is `<hash>  <path>` split at the first two-space separator, the hash is exactly 64 characters of `0-9a-f`, the path is not empty, does not start with `/` and has no `..` component. Bad lines are ignored. A path that appears on two or more good lines is removed.
  - `pub(crate) fn get(&self, rel: &str) -> Option<&str>`.
  - `pub(crate) fn set(&mut self, rel: String, hash: String)`.
  - `pub(crate) fn render(&self) -> String`: one `<hash>  <path>\n` line per entry, sorted by path (the `BTreeMap` order).
  - `pub(crate) fn write(&self, presets_dir: &Path) -> Result<bool, CoreError>`: writes `render()` with `write_atomic` to `presets_dir.join(MANIFEST_FILE)` and returns `true`, unless the file already holds exactly that text, or is missing and the manifest is empty; then it writes nothing and returns `false`.

- [ ] **Step 1: Read** spec section 4 and the edge-case row "Manifest missing or unreadable line", `crates/factory-core/src/preset.rs` (all), `crates/factory-core/src/fsutil.rs` (`write_atomic` and its temp-file name `.<name>.<pid>.tmp`), `crates/factory-core/src/error.rs`, the root `Cargo.toml` `[workspace.dependencies]` (note the comment style on `sysinfo`) and `crates/factory-core/Cargo.toml`.
- [ ] **Step 2: Write the failing tests** in a `#[cfg(test)] mod tests` at the bottom of `crates/factory-core/src/preset/manifest.rs`, and add `mod manifest;` plus `pub use manifest::MANIFEST_FILE;` to `preset.rs` so the file compiles into the crate. Use `tempfile::tempdir()`. One full example:

```rust
#[test]
fn write_then_read_round_trips_sorted_lines() {
    let dir = tempfile::tempdir().expect("tmp");
    let a = sha256_hex(b"a");
    let b = sha256_hex(b"b");
    let mut m = Manifest::default();
    m.set("rust/config.toml".to_string(), b.clone());
    m.set("go/config.toml".to_string(), a.clone());
    assert!(m.write(dir.path()).expect("write"));
    let text = std::fs::read_to_string(dir.path().join(MANIFEST_FILE)).expect("read");
    assert_eq!(text, format!("{a}  go/config.toml\n{b}  rust/config.toml\n"));
    let names: Vec<String> = std::fs::read_dir(dir.path())
        .expect("list")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec![MANIFEST_FILE.to_string()], "no temp file is left");
    assert_eq!(Manifest::read(dir.path()).expect("read back"), m);
}
```

  The other tests:

| Test | Input | Assertion |
|---|---|---|
| `sha256_hex_matches_the_known_vectors` | `b""` and `b"abc"` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` and `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad` |
| `a_missing_manifest_reads_as_empty` | empty temp dir, and a path `dir/absent` that does not exist | both `read` give `Manifest::default()` |
| `write_skips_an_identical_file` | write a one-entry manifest, then write it again; also an empty manifest in an empty dir | second write returns `false`; empty write returns `false` and creates no `.manifest` |
| `bad_lines_are_ignored_and_good_lines_still_count` | file text: a good line for `rust/config.toml` (`sha256_hex(b"x")`), then `nothex  go/config.toml`, the hash uppercased `  go/commit-context.md`, a 63-char hash `  go/agents/factory-builder.md`, `<good hash> rust/commit-context.md` (one space), `<good hash>  ` (empty path), `<good hash>  /etc/passwd`, `<good hash>  rust/../go/config.toml`, an empty line | `get("rust/config.toml") == Some(<good hash>)`; `get` is `None` for `go/config.toml`, `go/commit-context.md`, `go/agents/factory-builder.md`, `rust/commit-context.md`, `/etc/passwd`, `rust/../go/config.toml`; `render()` has exactly one line |
| `a_non_utf8_line_is_ignored` | bytes `b"\xff\xfe  rust/config.toml\n"` followed by a good line for `go/config.toml` | `get("rust/config.toml")` is `None`, `get("go/config.toml")` is `Some` |
| `a_path_listed_twice_counts_as_unmanifested` | two good lines for `rust/config.toml` with the same hash, one good line for `go/config.toml` | `get("rust/config.toml")` is `None`, `get("go/config.toml")` is `Some` |
| `an_unreadable_manifest_is_an_error` | `std::fs::create_dir(dir.join(MANIFEST_FILE))` | `Manifest::read(dir).expect_err("a directory is no manifest")` matches `CoreError::Io { path, .. }` with `path` ending in `.manifest` |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-core preset::manifest` and expect a compile failure: `cannot find function sha256_hex` and `cannot find type Manifest` (and, before the `Cargo.toml` change, an unresolved `sha2` import if the builder writes the `use` first).
- [ ] **Step 4: Implement.** Add `sha2 = "=0.10.9"` to `[workspace.dependencies]` with the comment `# Preset manifest hashes. Pinned to the version in the local registry.`, and `sha2.workspace = true` to `crates/factory-core/Cargo.toml`. Write the module from the signatures above (`sha2::{Digest, Sha256}`), with a one-line module doc: `//! The preset manifest: one SHA-256 hash per preset file the factory wrote.` Run `cargo check -j4 -p factory-core` so `Cargo.lock` gets the new entries.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-core preset::manifest` and expect the 8 tests to PASS, then `cargo test -j4 -p factory-core preset` and expect every existing `preset.rs` test to PASS unchanged. A `dead_code` warning on `Manifest` methods not yet used outside tests is acceptable only until Task 2; prefer no `#[allow]`.

### Task 2: Seed and refresh use the manifest with the new unedited rule

**Files:**
- Modify: `crates/factory-core/src/preset.rs`, `crates/factory-cli/tests/presets_refresh.rs`

**Interfaces:**
- Consumes: `Manifest`, `sha256_hex`, `MANIFEST_FILE` (Task 1); `write_atomic`; `Home::presets_dir(&self) -> PathBuf`; in the CLI test, the existing helper `fn run(home: &Home, args: &[&str]) -> String` in `presets_refresh.rs`.
- Produces (signatures unchanged):
  - `pub fn seed(presets_dir: &Path) -> Result<Vec<PathBuf>, CoreError>`: still returns only the preset files it wrote, never `.manifest`. It reads the manifest first (a read error is returned), writes missing files as today, then writes a fresh manifest: the hash of every file it wrote or found equal to the embedded text, and the old entry for any other existing file.
  - `pub fn refresh(presets_dir: &Path, force: bool) -> Result<RefreshReport, CoreError>`: reads the manifest before touching any file. Per embedded file, in order: absent, `written`; bytes equal to the embedded text, `skipped`; `sha256_hex(existing)` equals the manifest entry, `updated`; equal to the `LEGACY` text for that preset and agent file, `updated`; else `updated` with `force`, otherwise `refused`. The new manifest holds the embedded hash for every written, updated and skipped file, the old entry (if any) for every refused file, and nothing else. It is written once at the end.
  - The private helper `files_of` also yields the `/`-separated relative path used as the manifest key.

- [ ] **Step 1: Read** `crates/factory-core/src/preset.rs` (`files_of`, `seed`, `refresh`, `LEGACY`, the test helpers `legacy_builder`, `shipped` and `other_preset`), `crates/factory-cli/src/presets.rs` (the output this task must not change), `crates/factory-cli/tests/presets_refresh.rs`, and `crates/factory-core/src/project.rs` around `preset::seed` in `add` and the test whose `expect_err` reads `"seed must fail"`.
- [ ] **Step 2: Write the golden test first** in `crates/factory-cli/tests/presets_refresh.rs`, so the output is pinned before `refresh` changes:

```rust
#[test]
fn refresh_output_matches_the_golden_text() {
    let tmp = tempfile::tempdir().expect("tmp");
    let home = Home::at(tmp.path().join("home"));
    preset::seed(&home.presets_dir()).expect("seed");
    std::fs::write(home.presets_dir().join("rust/config.toml"), "# mine\n").expect("edit");
    std::fs::remove_file(home.presets_dir().join("go/commit-context.md")).expect("rm");
    let p = |rel: &str| home.presets_dir().join(rel).display().to_string();
    let mut expected = vec![format!("written: {}", p("go/commit-context.md"))];
    for rel in [
        "rust/commit-context.md",
        "rust/agents/factory-builder.md",
        "rust/agents/factory-code-reviewer.md",
        "rust/agents/factory-committer.md",
        "rust/agents/factory-kb-refresher.md",
        "go/config.toml",
        "go/agents/factory-builder.md",
        "go/agents/factory-code-reviewer.md",
        "go/agents/factory-committer.md",
        "go/agents/factory-kb-refresher.md",
    ] {
        expected.push(format!("unchanged: {}", p(rel)));
    }
    expected.push(format!("refused: {}", p("rust/config.toml")));
    expected.push("1 written, 0 updated, 10 unchanged, 1 refused".to_string());
    expected.push(
        "refused files differ from every known text, use --force to overwrite them".to_string(),
    );
    assert_eq!(run(&home, &["refresh"]), expected.join("\n"));
}
```

  Run `cargo test -j4 -p factory-cli --test presets_refresh` and expect both tests to PASS on the current code. Then write the failing unit tests in the `preset.rs` test module (`use super::manifest::{sha256_hex, Manifest};`). One full example:

```rust
#[test]
fn an_unedited_file_that_differs_from_the_embedded_text_is_updated_through_the_manifest() {
    let dir = tempfile::tempdir().expect("tmp");
    seed(dir.path()).expect("seed");
    let rel = "rust/agents/factory-committer.md";
    let path = dir.path().join(rel);
    let older = "---\nname: factory-committer\n---\nAn older shipped text.\n";
    std::fs::write(&path, older).expect("old");
    let mut m = Manifest::read(dir.path()).expect("read");
    m.set(rel.to_string(), sha256_hex(older.as_bytes()));
    m.write(dir.path()).expect("write");

    let r = refresh(dir.path(), false).expect("refresh");
    assert_eq!(r.updated, vec![path.clone()]);
    assert!(r.refused.is_empty() && r.written.is_empty());
    assert_eq!(r.skipped.len(), 11);
    let shipped_text = shipped("rust", "factory-committer.md");
    assert_eq!(std::fs::read_to_string(&path).expect("read"), shipped_text);
    let after = Manifest::read(dir.path()).expect("read again");
    assert_eq!(after.get(rel), Some(sha256_hex(shipped_text.as_bytes()).as_str()));
}
```

  The other tests:

| Test | Input | Assertion |
|---|---|---|
| `seed_records_a_hash_for_every_file_it_writes` | `seed` into an empty temp dir | `Manifest::read` has an entry for each of the 12 files, each equal to `sha256_hex` of the file bytes; `render().lines().count() == 12`; the returned `Vec` has 12 paths and none ends with `.manifest` |
| `seed_keeps_the_entry_of_an_existing_edited_file` | `seed`; write `# mine\n` to `rust/config.toml`; `seed` again | `rust/config.toml` entry still equals the embedded config hash; the second `seed` returns an empty `Vec` |
| `seed_records_files_equal_to_the_embedded_text` | `seed`; delete `.manifest`; write `# mine\n` to `rust/config.toml`; `seed` again | manifest has 11 entries and no `rust/config.toml` entry |
| `an_edited_file_with_a_manifest_entry_is_refused` | `seed`; read the manifest text; write `# mine\n` to `go/agents/factory-code-reviewer.md`; `refresh(dir, false)` | `refused == vec![that path]`; file bytes are `# mine\n`; the `.manifest` text equals the text read before |
| `refresh_force_records_the_new_hash` | as above, then `refresh(dir, true)` | `updated == vec![that path]`; the entry equals `sha256_hex` of the embedded reviewer text |
| `a_home_without_a_manifest_uses_the_legacy_rule_and_then_gets_one` | `seed`; delete `.manifest`; write `legacy_builder()` to `rust/agents/factory-builder.md`; `refresh(dir, false)` | `updated == vec![that path]`; `.manifest` now exists with 12 entries, each the embedded hash |
| `a_home_without_a_manifest_refuses_a_changed_config_until_force` | `seed`; delete `.manifest`; write `# an older shipped config\n` to `go/config.toml`; `refresh(dir, false)` twice, then `refresh(dir, true)` | both plain runs give `refused == vec![that path]` and no `go/config.toml` entry; the forced run gives `updated == vec![that path]` and the entry equals the embedded go config hash |
| `a_bad_manifest_line_makes_that_file_unmanifested` | `seed`; write an older text to `rust/agents/factory-committer.md`; replace its manifest line with `nothex  rust/agents/factory-committer.md` | `refresh(dir, false)` gives `refused == vec![that path]` |
| `a_deleted_preset_file_is_written_again_and_recorded` | `seed`; remove `go/commit-context.md`; `refresh(dir, false)` | `written == vec![that path]`; its entry equals the embedded hash |
| `unknown_manifest_entries_are_dropped_on_write` | `seed`; append `<sha256_hex(b"x")>  rust/agents/retired.md\n` to `.manifest` and create that file with `x`; `refresh(dir, false)` | `Manifest::read(dir).get("rust/agents/retired.md")` is `None`; the file `rust/agents/retired.md` still holds `x` |
| `refresh_fails_before_writing_when_the_manifest_cannot_be_read` | `create_dir(dir.join(MANIFEST_FILE))`, no preset files | `refresh(dir, false).expect_err("manifest is a directory")` is `CoreError::Io`; `dir.join("rust")` does not exist |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-core preset` and expect FAIL: `seed_records_a_hash_for_every_file_it_writes` (no `.manifest`, 0 entries), `an_unedited_file_..._through_the_manifest` (`refused` instead of `updated`), `refresh_force_records_the_new_hash`, `..._then_gets_one`, `..._until_force`, `a_deleted_preset_file_...`, `seed_keeps_...` and `seed_records_files_equal_...` on the missing entries, and `refresh_fails_before_writing_...` because today's `refresh` writes the files. The existing tests still PASS.
- [ ] **Step 4: Implement** the `seed` and `refresh` behaviour in the Interfaces block. Update the doc comments of `seed`, `refresh` and `LEGACY` to say the manifest comes first and the legacy texts are the fallback for files without an entry. Leave `crates/factory-cli/src/presets.rs` untouched.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-core preset` and expect every test, old and new, to PASS. Then `cargo test -j4 -p factory-cli --test presets_refresh` (both tests PASS, output unchanged), `cargo test -j4 -p factory-core project` (the `seed must fail` case still fails as expected), `cargo test -j4 -p factory-core --test rocket_parity` and `cargo test -j4 -p factory-core --test rocketvault_registration` and expect PASS.

### Task 3: plan_refresh dry run and the unchanged output check

**Files:**
- Modify: `crates/factory-core/src/preset.rs`

**Interfaces:**
- Consumes: the Task 2 `refresh` rule, `Manifest::read`, `Manifest::write`, `write_atomic`.
- Produces:
  - `pub fn plan_refresh(presets_dir: &Path) -> Result<RefreshReport, CoreError>`: the report `refresh(presets_dir, false)` would return on the same files, with nothing written: no preset file, no `.manifest`, no folder.
  - `refresh(presets_dir: &Path, force: bool) -> Result<RefreshReport, CoreError>` becomes "plan with `force`, then apply": a private planning function returns the `RefreshReport` and the new `Manifest`; a private apply step writes each `written` and `updated` path with `write_atomic` and then calls `Manifest::write`. `plan_refresh` calls the planning function with `force = false` and drops the manifest. Plan 4 consumes `plan_refresh` and the `updated` field for `factory upgrade --dry-run`.

- [ ] **Step 1: Read** the Task 2 `refresh` in `crates/factory-core/src/preset.rs`, spec section 4 (the dry-run bullet) and spec sections 5 and 6, which say how plan 4 uses `plan_refresh` and the `updated` paths.
- [ ] **Step 2: Write the failing tests** in the `preset.rs` test module. A helper `fn snapshot(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>>` walks `dir` recursively and maps every file to its bytes (an absent `dir` gives an empty map). One full example:

```rust
#[test]
fn plan_refresh_writes_nothing() {
    let dir = tempfile::tempdir().expect("tmp");
    seed(dir.path()).expect("seed");
    std::fs::remove_file(dir.path().join("go/commit-context.md")).expect("rm");
    std::fs::write(dir.path().join("rust/config.toml"), "# mine\n").expect("edit");
    std::fs::write(dir.path().join("rust/agents/factory-builder.md"), legacy_builder())
        .expect("old");
    let before = snapshot(dir.path());

    let plan = plan_refresh(dir.path()).expect("plan");
    assert_eq!(snapshot(dir.path()), before);
    assert_eq!(plan.written, vec![dir.path().join("go/commit-context.md")]);
    assert_eq!(plan.updated, vec![dir.path().join("rust/agents/factory-builder.md")]);
    assert_eq!(plan.refused, vec![dir.path().join("rust/config.toml")]);
    assert_eq!(plan.skipped.len(), 9);
}
```

  The other tests:

| Test | Input | Assertion |
|---|---|---|
| `plan_refresh_reports_what_refresh_then_does` | the same setup as `plan_refresh_writes_nothing`, plus an older text with a matching manifest entry for `go/agents/factory-committer.md` | `plan_refresh(dir)` equals the report of a following `refresh(dir, false)` |
| `plan_refresh_on_a_missing_dir_creates_nothing` | `dir.path().join("presets")`, never created | `written.len() == 12`, the other three fields empty, and the path still does not exist |
| `plan_refresh_never_forces` | `seed`; write `# mine\n` to `go/config.toml` | `plan_refresh` gives `refused == vec![that path]` and `updated` empty |
| `plan_refresh_fails_when_the_manifest_cannot_be_read` | `create_dir(dir.join(MANIFEST_FILE))` | `plan_refresh(dir).expect_err("manifest is a directory")` is `CoreError::Io` |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-core plan_refresh` and expect a compile failure: `cannot find function plan_refresh in this scope`.
- [ ] **Step 4: Implement** `plan_refresh` and split `refresh` into the private planning and apply steps described in Interfaces. Give `plan_refresh` a doc comment: `/// Reports what refresh without force would do. It writes nothing.`
- [ ] **Step 5: Run** `cargo test -j4 -p factory-core preset` and expect every test to PASS, including all Task 2 tests (the split must not change any report). Then run the unchanged-output check `cargo test -j4 -p factory-cli --test presets_refresh` and expect `the_command_reports_each_outcome_and_refused_paths` and `refresh_output_matches_the_golden_text` to PASS. Finish with `cargo test -j4 -p factory-cli --test no_hardcoded_paths`, `cargo clippy -j4 -p factory-core --all-targets -- -D warnings` and `cargo fmt -p factory-core -p factory-cli -- --check`, all PASS.

Next plan: docs/superpowers/plans/2026-10-20-upgrade-and-version-check/plan-04-upgrade-and-version-check.md
