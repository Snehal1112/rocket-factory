# Cargo Lock Check Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A diff whose only risky paths are Cargo files, and that brings in no outside code and no new build-time code, counts as normal and merges after review instead of parking `risky-diff`.

**Architecture:** Pure rules live in `factory-core` (`cargo_lock.rs`: lock compare, manifest key diff, path-package rules; `risk.rs`: which risky paths may be cleared). `factory-git` exports a commit's tree into a clean folder and reads a file at a commit. `factory-checks` runs `cargo locate-project` and `cargo metadata --locked` in that folder. The engine's new `cargo_risk.rs` ties them together behind one `Engine::risk(paths) -> Risk`, stores the result per head and config in `cargo-check.txt`, and every whole-diff risk call uses it.

**Tech Stack:** Rust, git2 (only in factory-git), `toml` and `serde_json` (already dependencies), cargo on `PATH`.

**Spec:** `docs/superpowers/specs/2026-11-21-cargo-lock-check-design.md`

## Global Constraints

- No new external crate. `toml` is already in factory-core, `serde_json` in factory-checks.
- `factory-core` stays pure: no I/O, no process calls. git goes only through `factory_git`; cargo runs only from `factory-checks`.
- Board JSON is unchanged. `record.risky` keeps its meaning and is set from the new verdict.
- Paths reach the driver only through `Layout` (`factory-cli/tests/no_hardcoded_paths.rs`).
- cargo in commands and docs: always `-j4`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Tests never need the network: fixture workspaces hold path crates only (no registry or git dependency anywhere, not even one the lock lacks), and their lock is made with `cargo generate-lockfile --offline` in the test.
- The spec names `Engine::diff_class` and `path_is_risky`; this plan uses one `Engine::risk(paths) -> Risk` that carries both answers. That is within "exact shapes left to the builder".
- Cargo error reasons use cargo's first stderr line that starts with `error`, else its first non-empty line.
- `CheckError::Cargo(String)` displays as the bare message: `#[error("{0}")]`.
- `cargo-check.txt` format: line 1 `key <head sha> <config text>`, line 2 `same` or `risky`, line 3 the ledger note (may be empty).
- Every reason (toml errors span several lines) is collapsed to one line, whitespace joined, before it is stored or written as a ruling.
- Ledger lines carry a `Ruling: ` prefix, so tests match ledger text with `contains`.
- The export folder is a fresh folder named `factory-cargo-check-<job id>-<pid>-<nanos>-<counter>` (a process-wide `AtomicU64` counter; no new dependency) under a temp root (`std::env::temp_dir()` in production). If any parent of it holds `Cargo.toml`, `.cargo/config` or `.cargo/config.toml` (except inside `$CARGO_HOME`, else `$HOME/.cargo`), the result is Unknown `"a Cargo file sits above the export folder"`. A comment notes the check-then-use gap on a shared temp folder.
- Anything the check can't settle gives `Risky`. The check only ever turns `Risky` into `Normal`, never the other way.
- Ledger lines, exact text:
  - `cargo lock check: no outside code changed, Cargo files are not risky`
  - `cargo lock check: outside crates added: <name version, ...|none>; removed: <name version, ...|none>, diff stays risky`
  - `cargo lock check: <reason>, diff stays risky`
- Setting: `cargo_lock_check: bool`, default `true`.
- Store file: `cargo-check.txt` in the job folder, keyed by head sha plus the text `cargo_lock_check=<bool>; risk_paths=<globs joined by ",">`, written with `factory_core::write_atomic`.
- Code comments are short full sentences ending with a period. No `unwrap` in production code.
- Commits go through the `dev-workflow-skills:1-git-commit` skill.

## Review Focus

- A cargo failure after the lock check passed (in production often a download failure for other-platform crates) must give `Risky` with cargo's error line, never a pass. Pinned offline by `a_metadata_failure_after_a_same_lock_keeps_the_diff_risky` in the engine-verdict task (a path dependency on a crate that exists only as an ignored folder, so `cargo metadata` fails in the export).
- An export folder left behind after a cargo error or an early return fills the temp folder. The engine must remove it on every return. Pinned by the in-crate test `the_export_folder_is_gone_after_a_pass_and_an_error`.
- A subfolder project whose workspace root sits above `subpath` must give `Risky`, not read the root lock. Pinned by `a_workspace_root_above_the_subpath_stays_risky`.
- A stored verdict from an older head or an older config must not be reused. Pinned by `a_new_config_runs_the_check_again` and `a_stored_verdict_for_another_head_is_ignored`.
- A risky Cargo file next to another risky path (`*auth*`) must never call cargo. Pinned by `a_cargo_file_beside_another_risky_path_stays_risky_without_cargo`.

---

### Task 1: Lock compare and manifest rules in factory-core

**Files:**
- Create: `crates/factory-core/src/cargo_lock.rs`
- Modify: `crates/factory-core/src/lib.rs` (add `pub mod cargo_lock;` and re-exports)

**Interfaces:**
- Produces:
  - `pub enum LockDiff { Same, Changed { added: Vec<String>, removed: Vec<String> }, Unknown(String) }` (`added`/`removed` are sorted `"name version"` strings).
  - `pub fn compare_locks(base: &str, head: &str) -> LockDiff`
  - `pub enum CargoVerdict { Same, Changed(String), Unknown(String) }`
  - `pub fn manifest_keys_diff(base: Option<&str>, head: &str) -> CargoVerdict` (base `None` means a new manifest, compared against an empty table).
  - `pub struct CargoTarget { pub kinds: Vec<String>, pub src_path: PathBuf }`
  - `pub struct PathPackage { pub manifest_path: PathBuf, pub targets: Vec<CargoTarget> }`
  - `pub fn check_path_packages(project: &Path, packages: &[PathPackage], changed: &BTreeSet<PathBuf>, new_manifests: &BTreeSet<PathBuf>) -> CargoVerdict` — all paths absolute and already canonical; pure path comparison, no fs calls.

- [ ] **Step 1: Write the failing lock tests** in `cargo_lock.rs` `mod tests`, using small inline lock texts (format 4, `[[package]]` with `name`, `version`, `source`, `checksum`):
  - `reordered_packages_are_same` — same entries, other order → `Same`.
  - `a_new_registry_crate_is_added` → `Changed { added: ["signal-hook 0.3.17"], removed: [] }`.
  - `a_version_bump_shows_both_versions` → `added: ["libc 0.2.191"], removed: ["libc 0.2.190"]`.
  - `a_changed_checksum_is_changed` and `a_new_git_source_is_changed` → `Changed`.
  - `a_new_path_package_is_same` — entry with no `source` added → `Same`.
  - `garbage_no_packages_and_v1_are_unknown` — `"not toml ["`, a text with no `[[package]]`, a lock whose `[metadata]` has a key `"checksum serde 1.0.0 (registry+...)"` → each `Unknown(_)`; an entry missing `version` → `Unknown(_)`.
  - `a_format_version_bump_alone_is_same` — `version = 3` vs `version = 4`, same packages → `Same`.

- [ ] **Step 2: Write the failing manifest and path tests:**
  - `manifest_key_changes_are_changed` — each of `package.build = "src/gen.rs"`, `package.links = "z"`, `lib.proc-macro = true`, `lib.proc_macro = true`, `cargo-features = ["x"]`, a `[patch.crates-io]` entry, a `[replace]` entry, added to a base manifest → `Changed(_)` whose reason names the key.
  - `free_manifest_edits_are_same` — `[[bin]]`, `[features]`, `[profile.release]`, a new `[dependencies] foo = "1"`, and pure reformatting → `Same`.
  - `a_new_manifest_is_compared_with_an_empty_one` — `manifest_keys_diff(None, "[package]\nname=\"x\"\n[lib]\nproc-macro = true\n")` → `Changed`; the same without `proc-macro` → `Same`.
  - `an_unparsable_manifest_is_unknown` — base or head garbage → `Unknown`.
  - `path_packages_outside_the_project_are_changed` — a package whose `manifest_path` or a target `src_path` is not under `project` → `Changed`.
  - `a_changed_build_script_is_changed` — target kinds `["custom-build"]` with `src_path` in `changed` (`<project>/crates/a/b/c/build.rs`) → `Changed`; the same not in `changed` → `Same`.
  - `a_new_proc_macro_package_is_changed` — kinds `["proc-macro"]`, `manifest_path` in `new_manifests` → `Changed`; not new → `Same`.

- [ ] **Step 3: Run to verify they fail**

Run: `cargo test -j4 -p factory-core cargo_lock`
Expected: FAIL to compile (module items missing).

- [ ] **Step 4: Implement the items listed under Interfaces in `crates/factory-core/src/cargo_lock.rs`**

`compare_locks` collects `(name, version, source, checksum)` tuples for entries with `source` into a `BTreeSet` per side and diffs them; a name present on both sides with a different tuple shows in both lists. `manifest_keys_diff` compares the parsed `toml::Value`s at the listed keys (`package.build`, `package.links`, `lib.proc-macro`, `lib.proc_macro`, `cargo-features`, `patch`, `replace`). Re-export the public items from `lib.rs`.

- [ ] **Step 5: Run to verify they pass**

Run: `cargo test -j4 -p factory-core cargo_lock`
Expected: PASS.

- [ ] **Step 6: Commit** through the commit skill: `feat(core): compare Cargo locks and manifest keys`.

---

### Task 2: Clearable paths, the setting and the preset globs

**Files:**
- Modify: `crates/factory-core/src/risk.rs`
- Modify: `crates/factory-core/src/config.rs` (field, default, test)
- Modify: `crates/factory-core/src/settings_block.rs` (list the key in "Merge, tickets and logs." after `risk_paths`)
- Modify: `crates/factory-core/presets/rust/config.toml`
- Modify: `crates/factory-core/src/preset.rs` (tests)
- Modify: `docs/reference/project-toml-reference.md`

**Interfaces:**
- Produces:
  - `pub fn cargo_clearable(path: &str, config: &Config) -> bool` in `risk.rs`, re-exported from `lib.rs`.
  - `Config::cargo_lock_check: bool` (default `true`).

- [ ] **Step 1: Write the failing tests:**
  - In `risk.rs`: `cargo_files_under_cargo_only_globs_are_clearable` — with `risk_paths = ["**/Cargo.toml", "**/Cargo.lock", "src-tauri/**"]`: `Cargo.lock`, `crates/a/Cargo.toml`, `crates/a/cargo.toml` → `true`; `src-tauri/Cargo.toml` → `false`; `crates/a/build.rs` → `false`; `../Cargo.toml` → `false`.
  - In `config.rs`: `cargo_lock_check_defaults_on_and_parses` — `Config::default().cargo_lock_check` is `true`; `Config::parse("cargo_lock_check = false")` gives `false`; `"yes"` is an error.
  - In `preset.rs`: `rust_preset_marks_cargo_config_and_toolchain_risky` — `.cargo/config.toml`, `crates/x/.cargo/config.toml`, `rust-toolchain.toml`, `rust-toolchain` classify `Risky` with the rust preset; `crates/factory-engine/src/build.rs` stays `Normal` (the existing test already pins this; keep it passing).

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -j4 -p factory-core cargo_` and `cargo test -j4 -p factory-core rust_preset`
Expected: FAIL.

- [ ] **Step 3: Implement**
  - `cargo_clearable`: false on a `..` segment; true only when the last segment, lowercased, is `cargo.toml` or `cargo.lock`, and every lowercased `risk_paths` glob that matches the lowercased path has a last segment of `cargo.toml` or `cargo.lock`.
  - Add `cargo_lock_check` to `Config` with a one-line doc comment, default `true`, and to the settings block.
  - Add `"**/.cargo/**"` and `"**/rust-toolchain*"` to the rust preset's `risk_paths`, with a comment line: source replacement, registries, wrappers and toolchains change what builds without touching `Cargo.lock`.
  - Document `cargo_lock_check` in `project-toml-reference.md` next to `risk_paths`: what it clears, that the three checks must pass, that off means Cargo files stay risky by path, and that cargo may download crates already in the lock.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -j4 -p factory-core` and `cargo clippy -j4 -p factory-core --all-targets -- -D warnings`
Expected: PASS (includes `every_config_key_is_listed_once`), no warnings.

- [ ] **Step 5: Commit** through the commit skill: `feat(core): add cargo_lock_check and Cargo-only clearing`.

---

### Task 3: Read a file and export a tree at a commit in factory-git

**Files:**
- Modify: `crates/factory-git/src/tip.rs` (`file_at`)
- Modify: `crates/factory-git/src/repo.rs` (`export_commit`, sharing the body of `export_tree`)
- Test: in-module tests next to the existing ones for `file_at_branch` and `export_tree`

**Interfaces:**
- Produces:
  - `pub fn Repo::file_at(&self, commit: &str, rel: &str) -> Result<Option<String>>` — reuses `entry_at` and `is_regular`; `None` when absent; `Refused` when not a regular file; `BadInput` when not UTF-8.
  - `pub fn Repo::export_commit(&self, commit: &str, dest: &Path) -> Result<()>` — same rules as `export_tree` (tracked regular files only, symlinks and submodules skipped, a non-empty `dest` refused). `export_tree` calls it with the branch tip.

- [ ] **Step 1: Write the failing tests:**
  - `file_at_reads_a_file_at_an_older_commit` — commit `a.txt = "1"`, then `"2"`; `file_at(first, "a.txt")` is `Some("1")`, `file_at(second, "missing")` is `None`, `file_at(second, "dir")` for a folder is an error.
  - `export_commit_writes_the_tree_of_that_commit` — export the first commit; `a.txt` holds `"1"` and no `.git` is written; a non-empty `dest` is refused.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -j4 -p factory-git file_at` and `cargo test -j4 -p factory-git export_commit`
Expected: FAIL.

- [ ] **Step 3: Implement both functions** as described under Interfaces. `export_commit` resolves the commit: do not use a bare `revparse_single`, use the existing `commit_of` (repo.rs ~193) instead; both it and `export_tree` call one private body that takes a `&git2::Commit`, so `export_tree` keeps resolving its branch with `branch_commit` and never mistakes a tag for a branch.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -j4 -p factory-git` and `cargo clippy -j4 -p factory-git --all-targets -- -D warnings`
Expected: PASS, no warnings.

- [ ] **Step 5: Commit** through the commit skill: `feat(git): read and export the tree of a commit`.

---

### Task 4: Cargo calls in factory-checks

**Files:**
- Modify: `crates/factory-checks/src/workspace.rs`
- Modify: `crates/factory-checks/src/lib.rs` (re-exports; a `CheckError::Cargo(String)` variant with `#[error("{0}")]`)

**Interfaces:**
- Consumes: `factory_core::cargo_lock::{PathPackage, CargoTarget}` from the lock-compare task.
- Produces:
  - `pub fn workspace_root(manifest: &Path) -> Result<PathBuf, CheckError>` — runs `cargo locate-project --workspace --message-format plain --manifest-path <manifest>` and returns the parent folder of the printed path. It runs with `current_dir` set to the manifest's folder, because cargo finds its config from the working folder.
  - `pub fn locked_path_packages(root_manifest: &Path) -> Result<Vec<PathPackage>, CheckError>` — runs `cargo metadata --locked --format-version 1 --manifest-path <root_manifest>` in the manifest's folder and parses with `path_packages_from_metadata`.
  - `pub fn path_packages_from_metadata(json: &str) -> Result<Vec<PathPackage>, CheckError>` — pure: every package with `"source": null`, its `manifest_path` and each target's `kind` list and `src_path`, as raw `PathBuf`s.
  - `locked_path_packages` canonicalises every path after parsing; a failed canonicalise is an error.
  - Every error is `CheckError::Cargo(msg)` with `msg` = `cargo <subcommand> failed: <error line>` or `cargo did not start: <io error>` (the error line rule is in Global Constraints).

- [ ] **Step 1: Write the failing tests** (in `workspace.rs` `mod tests`, each building a temp workspace with path crates only and running `cargo generate-lockfile --offline` first):
  - `workspace_root_finds_the_root_of_a_member` — root `[workspace] members = ["a"]`; `workspace_root(<tmp>/a/Cargo.toml)` is `<tmp>`.
  - `a_nested_workspace_gives_its_own_root` — `a` excluded and holding its own `[workspace]`; root of `a/Cargo.toml` is `<tmp>/a`.
  - `a_current_lock_lists_the_path_packages` — two members, `a` with a `build.rs`; the result holds both manifests and a target with kind `custom-build` whose `src_path` ends with `a/build.rs`.
  - `a_dependency_the_lock_lacks_fails_under_locked` — after the lock is made, add `b = { path = "../b" }` to `a` with `b` a new path crate outside the members list; `locked_path_packages` is an `Err` whose message starts with `cargo metadata failed:`.
  - `a_missing_cargo_is_an_error` — call through a helper that takes the program name, with `"cargo-does-not-exist"`; the result is an `Err` starting with `cargo did not start:`.
  - `metadata_json_keeps_only_path_packages` — `path_packages_from_metadata` on a fixed JSON text (paths need not exist) with one registry package and one `"source": null` package returns only the second, with its paths unchanged.
  - `stderr_reason_prefers_the_error_line` — the reason picked from `"warning: x\n    Blocking waiting for file lock\nerror: failed to select\n"` is `error: failed to select`; from `"\nonly line\n"` it is `only line`.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -j4 -p factory-checks workspace`
Expected: FAIL.

- [ ] **Step 3: Implement** the functions under Interfaces, following the style of `Workspace::load` (same error shape, `Command::new` with `current_dir`). No `--no-deps`, no `--offline`, no `-j`.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -j4 -p factory-checks` and `cargo clippy -j4 -p factory-checks --all-targets -- -D warnings`
Expected: PASS, no warnings.

- [ ] **Step 5: Commit** through the commit skill: `feat(checks): prove a Cargo lock current`.

---

### Task 5: The engine's risk verdict

**Files:**
- Create: `crates/factory-engine/src/cargo_risk.rs` (add `mod cargo_risk;` in `lib.rs`)
- Test: `crates/factory-engine/tests/cargo_risk.rs` (uses `tests/common`)

**Interfaces:**
- Consumes: `compare_locks`, `manifest_keys_diff`, `check_path_packages`, `cargo_clearable`, `Config::cargo_lock_check` (factory-core); `Repo::file_at`, `Repo::export_commit` (factory-git); `workspace_root`, `locked_path_packages` (factory-checks).
- Produces:
  - `pub(crate) struct Risk { pub class: DiffClass, pub risky: Vec<String>, pub note: Option<String> }` with `pub fn is_risky(&self) -> bool`.
  - `cargo-check.txt` read and write helpers in `cargo_risk.rs`, format as in Global Constraints.
  - `pub(crate) fn Engine::risk(&self, paths: &[String]) -> Risk` — `paths` are scoped (`job_changed_paths()`). Never returns an error: any failure gives `Risky` with the reason in `note`.

Behaviour, in order:
1. `classify_paths(paths)` is not `Risky` → that class, `risky` empty, `note` `None`.
2. `risky` = paths that classify `Risky` on their own. `cargo_lock_check` off, or any of them not `cargo_clearable` → `Risky`, `note` `None`, no cargo call.
3. Key = head sha (from `job_range()`) + the config text in Global Constraints. `cargo-check.txt` with the same key → its stored result.
4. Otherwise compute, write `cargo-check.txt`, and return:
   - `.gitmodules` at head (repo root, via `file_at`) → Unknown `"the repo has git submodules"`.
   - Export head with `export_commit` into the export folder from Global Constraints (created fresh, removed on every return), after the parent-folder guard. Project folder = export folder joined with `repo.subpath()`.
   - `risk(paths)` is `risk_in(paths, &std::env::temp_dir())`. `pub(crate) fn risk_in(&self, paths: &[String], temp_root: &Path) -> Risk` does all of steps 1-4 (gate, store lookup, compute); in-crate tests call it with their own temp root.
   - A symlinked crate is skipped by the export, so cargo fails and the result is Unknown; that is the symlink case of the spec.
   - Touched manifests: every changed `Cargo.toml` that exists at head, plus `<dir>/Cargo.toml` for every changed `Cargo.lock` (absent at head → Unknown). Roots from `workspace_root`, deduplicated; a root outside the project folder → Unknown `"workspace root <rel> is outside the project"`. Every changed `Cargo.lock` must be `<root>/Cargo.lock` of one of them → else Unknown.
   - Per root: `compare_locks(base, head)` with texts from `file_at(range_start, rel)` and `file_at(head, rel)` (repo-relative = `subpath/` + scoped); a missing base or head lock → Unknown. Not `Same` → stop with that result.
   - Then `locked_path_packages(<root>/Cargo.toml)`, `check_path_packages` (changed paths and new manifests mapped into the canonical export folder), and `manifest_keys_diff` for each changed manifest present at head.
   - All `Same` → `Normal`, `risky` empty, `note` = the "no outside code changed" line. Otherwise `Risky`, `risky` as in step 2, `note` = the matching "diff stays risky" line.

- [ ] **Step 1: Write the failing integration tests** in `tests/cargo_risk.rs`. Each starts from `Fixture::new()` with `risk_paths = ["**/Cargo.toml", "**/Cargo.lock", "**/*auth*", "**/.cargo/**"]`, commits on main a path-only workspace (root manifest with members `a` and `b`, a `.gitignore` holding `vendor/`, and a lock from `cargo generate-lockfile --offline`), then has a scripted builder effect write the job's change (running `cargo generate-lockfile --offline` in `spec.cwd` where the case says "regenerated lock"), runs `build_stage`, `checks_stage` and `review_stage` with an approving reviewer, and inspects the decision and the ledger. No case uses a registry or git dependency.
  - `an_internal_path_dependency_merges` — `a` gains `b = { path = "../b" }` and the regenerated lock → decision `AutoMerge`; the ledger holds `cargo lock check: no outside code changed, Cargo files are not risky`.
  - `a_dependency_missing_from_the_lock_parks` — a new path crate `c` (not a member, inside the project) and `a` gains `c = { path = "../c" }`, lock not regenerated → `NeedsHuman` with `RiskyDiff`; the ledger line starts `cargo lock check: cargo metadata failed:` and ends `, diff stays risky`.
  - `an_outside_crate_in_the_lock_parks_naming_it` — the head lock gains a hand-written registry entry `signal-hook 0.3.17` with `source` and `checksum` → `RiskyDiff` (the lock check stops before cargo); the ledger line is `cargo lock check: outside crates added: signal-hook 0.3.17; removed: none, diff stays risky`.
  - `a_metadata_failure_after_a_same_lock_keeps_the_diff_risky` — the builder writes an ignored crate `vendor/v` and `a` gains `v = { path = "../vendor/v" }` with the regenerated lock. In the job worktree `cargo metadata --locked` would pass; the clean export has no `vendor/v`, so it fails → `RiskyDiff`, ledger line starts `cargo lock check: cargo metadata failed:`. This also proves cargo does not run on the worktree.
  - `a_cargo_file_beside_another_risky_path_stays_risky_without_cargo` — change `a/Cargo.toml` and `src/auth.rs` → `RiskyDiff`; no `cargo-check.txt` is written and no `cargo lock check` ledger line.
  - `a_cargo_config_change_parks` — the job adds `.cargo/config.toml` plus the internal path change → `RiskyDiff`, no `cargo lock check` ledger line.
  - `an_unparsable_lock_parks_with_the_reason` — head lock `"garbage ["` → `RiskyDiff`; the ledger line names the parse reason.
  - `the_setting_off_keeps_cargo_files_risky` — `cargo_lock_check = false`, the internal-path change → `RiskyDiff`, no ledger line.
  - `a_new_internal_crate_merges_and_parks_with_a_build_script` — new member `c` with its own manifest and the regenerated lock → `AutoMerge`; the same plus `c/build.rs` (`fn main() {}`) → `RiskyDiff`.
  - `a_broader_glob_keeps_the_manifest_risky` — config adds `"src-tauri/**"`, change `src-tauri/Cargo.toml` → `RiskyDiff`, no `cargo lock check` ledger line.
  - `submodules_keep_the_diff_risky` — main also holds a `.gitmodules` file; the internal-path change → `RiskyDiff` with `git submodules` in the ledger line.
  - `a_subfolder_project_uses_its_own_workspace` and `a_workspace_root_above_the_subpath_stays_risky` — the `scoped_fixture` pattern of `tests/gates.rs` (~329-336: `Fixture::new()` then `fx.repo = Repo::open(..).with_scope("web", "apps/web")`); `spec.cwd` is `<worktree>/apps/web` and changed paths are project-relative. With the workspace inside `apps/web` the internal-path change merges. With `apps/web` a member of a root workspace, the builder only adds a `[features]` table to `apps/web/Cargo.toml` and does not touch any lock (a root lock change would be a scope violation); the result is `RiskyDiff` with `workspace root` in the ledger line.
  - `a_new_config_runs_the_check_again` — after the merging case, set `fx.config.risk_paths` to the same globs plus `"x/**"`, run `review_stage` again with a fresh approving reviewer; `cargo-check.txt` line 1 holds the new config text.
- [ ] **Step 1b: Write the failing in-crate tests** in `cargo_risk.rs` `#[cfg(test)] mod tests` (in-crate, because `Risk`, `risk_in` and the store helpers are crate-private). First add one `#[cfg(test)] pub(crate) fn engine_with_job(base_files, head_files) -> (TempDir, Engine-building parts)` helper in `cargo_risk.rs`: it inits a git2 repo with `initial_head("main")`, commits the base files on main, commits the head files on a side branch, writes `board.json` with `Job::new("001", …)` whose `base` and `head` are those shas, and gives what `Engine::new` needs (the `with_unreadable_board` pattern in park.rs ~614-635 shows the `Engine::new` call; that one has no job range). `park.rs` tests reuse it.
  - `the_export_folder_is_gone_after_a_pass_and_an_error` — `risk_in` with a temp root, once on a passing workspace and once on one whose manifest is invalid TOML; the temp root is empty afterwards.
  - `a_cargo_file_above_the_export_folder_is_unknown` — a temp root whose parent folder holds `Cargo.toml` → `Risky` with `a Cargo file sits above the export folder`.
  - `a_stored_verdict_for_the_same_key_is_reused` — write a `same` record for the current head and config key for a diff that would compute `Risky`; `risk_in` with a fresh temp root gives `Normal` and leaves that root empty.
  - `a_stored_verdict_for_another_head_is_ignored` — the same record with another sha → the check runs again.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -j4 -p factory-engine --test cargo_risk` and `cargo test -j4 -p factory-engine cargo_risk`
Expected: FAIL (the review stage still classifies by path; `Engine::risk` does not exist). Wire only the review stage's class at review.rs ~100 to `self.risk(&paths)` in this task, so these tests can pass; the other callers follow in the next task.

- [ ] **Step 3: Implement `cargo_risk.rs`** as described above, and in `review_stage` write `risk.note` with `self.ruling` once per review stage when it is `Some`.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -j4 -p factory-engine --test cargo_risk`, then `cargo test -j4 -p factory-engine`, then `cargo test -j4 -p factory-cli --test no_hardcoded_paths`
Expected: PASS.

- [ ] **Step 5: Commit** through the commit skill: `feat(engine): clear Cargo files when no outside code changed`.

---

### Task 6: Every risk caller uses the verdict

**Files:**
- Modify: `crates/factory-engine/src/review.rs` (~100-113 first-risky list, ~119 model, ~163 security skip, ~309 `risk_summary`, ~314 `fix_rounds_for`)
- Modify: `crates/factory-engine/src/park.rs` (`is_risky` ~318, `risk_part` ~360-375)
- Modify: `crates/factory-engine/src/checks_stage.rs` (`fix_or_park` ~283)
- Test: `crates/factory-engine/tests/cargo_risk.rs`

**Interfaces:**
- Consumes: `Engine::risk(&self, paths) -> Risk` from the engine-verdict task.
- No new public items. `decide.rs::risk_summary` keeps its signature; callers pass `&risk.risky` so cleared Cargo files are never named. `lead_parts`, `lead_parts_for_card`, `park_after_sync`, `decision_parts`, `nothing_changed_parts` and `park_gate` reach the verdict through `risk_part` and `is_risky`.

- [ ] **Step 1: Write the failing tests** in `tests/cargo_risk.rs`:
  - `a_cleared_round_then_an_outside_crate_hits_the_risky_cap` — round 1 is the internal-path change with reviewer `CHANGES`; the fix round adds `signal-hook 0.3.17` to the lock and round 2's reviewer answers `APPROVE`, so the job parks `RiskyDiff`; `risky-paths.txt` exists and lists the Cargo files; the park's Risk part names the crate and has no `added by fix rounds` line.
  - `a_failing_check_on_a_cleared_diff_uses_normal_fix_rounds` — the internal-path change with a failing check command and `fix_rounds = 2`, `risky_fix_rounds = 0` → `checks_stage` starts a fix round instead of parking.
  - `a_park_card_uses_the_cargo_verdict` (in-crate, using `engine_with_job` from `cargo_risk.rs`) — with a stored `same` record for the current key, `is_risky()` is false and `risk_part` gives `None`; with a stored `risky` record and note, `risk_part`'s text ends with that note.
  - `the_risk_part_never_lists_a_cleared_cargo_file` — the internal-path change with reviewer `CHANGES` and `fix_rounds = 0` parks `changes-remain`; its park card shows no `risky paths` text.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -j4 -p factory-engine --test cargo_risk`
Expected: the new tests FAIL.

- [ ] **Step 3: Replace each `classify_paths` whole-diff call listed under Files** with one `self.risk(&paths)` per stage and use `risk.class`, `risk.is_risky()` and `risk.risky`. The first-risky list at review.rs ~104 writes `risk.risky`. `risk_part`'s "added by fix rounds" filter uses the verdict's `risky` list, and when the diff is risky and `risk.note` is set, `risk_part` appends the note as its last line (spec section 5: the risk part repeats the "diff stays risky" line). Then grep: `grep -rn "classify_paths\|DiffClass::Risky" crates/factory-engine/src` must show only `spec_check.rs`, `decide.rs` (`risk_summary`'s per-path filter and the `Approve` match), `cargo_risk.rs`, `review.rs::security_skippable`'s parameter use, and tests.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`, `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`
Expected: PASS, no warnings.

- [ ] **Step 5: Commit** through the commit skill: `feat(engine): use the Cargo verdict in every risk call`.

---

### Task 7: Docs and Rocket's risk list

**Files:**
- Modify: `docs/reference/software-factory.md` (the risk rule)
- Modify: `docs/guides/factory-user-guide.md` (~1233, the `risky diff` row, and the risky-diff text near it)
- Modify (outside the repo, approved by the user with the spec): `~/.factory/projects/rocket/project.toml`

- [ ] **Step 1: Write the docs.** In `software-factory.md`, next to the risk rule: a diff whose only risky paths are Cargo files under Cargo-only globs counts as normal when the lock's outside crates are unchanged, `cargo metadata --locked` passes in a clean export of the head commit, and no path crate or target leaves the project, no build script changed and no proc-macro, `links`, `build`, `cargo-features`, `[patch]` or `[replace]` change came in; the three ledger lines; `cargo-check.txt`; the setting. In the user guide: one sentence in the `risky diff` row's area saying a Cargo-only change that adds no outside crate no longer parks, and how the ledger says why.

- [ ] **Step 2: Edit Rocket's project file.** Add to its `risk_paths`: `"**/.cargo/**"`, `"**/rust-toolchain*"`, `"build.rs"`, `"*/build.rs"`, `"*/*/build.rs"`. Then check it still loads: `factory --project rocket status` prints no config error.

- [ ] **Step 3: Refresh the installed preset.** Run `cargo build -j4 -p factory-cli`, then `factory presets refresh` and confirm `~/.factory/presets/rust/config.toml` holds the two new globs.

- [ ] **Step 4: Run the doc and agent lints.**

Run: `cargo test -j4 -p factory-cli --test agent_lint` and `cargo test -j4 -p factory-cli --test no_hardcoded_paths`
Expected: PASS.

- [ ] **Step 5: Commit** the repo docs through the commit skill: `docs(factory): describe the Cargo lock check`. The Rocket project file is outside the repo and is not committed.
