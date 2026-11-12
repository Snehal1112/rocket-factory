# Codex KB canonical folder Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-12-codex-kb-canonical-design.md
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Make the Codex KB run use the real KB folder from `spec.add_dirs`, so a symlinked `kb_path` behaves the same on Codex and Claude.

**Architecture:** One small pure function picks the folder from `add_dirs`, and `prepare_for_codex` uses it for the working folder and the KB git dir instead of calling `config.kb_dir()`. The agent calls stay behind the existing `AgentRunner` port in `factory-engine`; no new trait.

**Tech Stack:** Rust, `std::os::unix::fs::symlink` in tests.

## Global Constraints

- Commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`. Never `--workspace` or `--all`.
- Claude runs, `kb_spec` and `kb_allowed_tools` do not change. No `Cargo.toml` change, no `git` shell-outs, no `/proc` reads.
- No board, ledger or events field is added. Tests use `.expect("...")` and `expect_err`, never the unwrap call. Comments are short full sentences.

## Review Focus

1. A symlinked `kb_path` puts the Codex `cwd` on the target. Pinned by `kb_under_codex_runs_in_the_target_of_a_symlinked_kb_path`.
2. With `kb_auto_commit` on, the KB repo is found through the real folder. Pinned by `kb_under_codex_gets_the_kb_git_dir_when_auto_commit_is_on`.
3. With `kb_auto_commit` off, no extra writable root. Pinned by `kb_under_codex_has_no_extra_root_without_auto_commit`.
4. An empty `add_dirs` leaves `cwd` alone. Pinned by `kb_folder_is_the_first_add_dir_or_none`.
5. A plain real `kb_path` behaves as before. Pinned by the updated `kb_runs_in_the_kb_folder_under_codex`.

## File Structure

| File | Responsibility |
|---|---|
| `crates/factory-engine/src/codex_prep.rs` | `kb_folder`, the changed KB branch of `prepare_for_codex` |
| `crates/factory-engine/tests/codex_prep.rs` | Codex KB run tests |

### Task 1: Use the real KB folder under Codex

**Files:**
- Modify: `crates/factory-engine/src/codex_prep.rs`
- Test: `crates/factory-engine/tests/codex_prep.rs`, unit test in `crates/factory-engine/src/codex_prep.rs`

**Interfaces:**
- Consumes: `kb_refresh_step` already puts the canonical folder in `spec.add_dirs` through `kb_spec`.
- Produces: `fn kb_folder(add_dirs: &[PathBuf]) -> Option<&Path>` (private to the module).

- [ ] **Step 1: Read `prepare_for_codex` in `crates/factory-engine/src/codex_prep.rs`, `kb_spec` and `kb_refresh_step` in `crates/factory-engine/src/kb.rs`, and the helpers `codex`, `with_kb` and the test `kb_runs_in_the_kb_folder_under_codex` in `crates/factory-engine/tests/codex_prep.rs`.**
- [ ] **Step 2: Write the failing tests.**
  - Unit test `kb_folder_is_the_first_add_dir_or_none` in `codex_prep.rs`: `kb_folder(&[])` is `None`; `kb_folder(&[PathBuf::from("/a"), PathBuf::from("/b")])` is `Some(Path::new("/a"))`.
  - Update `kb_runs_in_the_kb_folder_under_codex`: expect `spec.cwd` to equal `fx.config.kb_dir().expect("kb dir").canonicalize().expect("canonical")`.
  - `kb_under_codex_runs_in_the_target_of_a_symlinked_kb_path`: build the fixture as `with_kb(codex(Fixture::new()))` does, create a symlink in the temp dir to the KB folder with `std::os::unix::fs::symlink`, set `fx.config.kb_path` to the link, run `kb_refresh_step(None)` as the neighbouring test does, assert `spec.cwd` equals the canonicalized target.
  - `kb_under_codex_gets_the_kb_git_dir_when_auto_commit_is_on`: the same symlink setup, `git2::Repository::init` on the KB folder, `fx.config.kb_auto_commit = true`; assert `spec.writable_roots` equals a one-element vec holding `factory_git::Repo::open(&target).expect("open").common_git_dir()`.
  - `kb_under_codex_has_no_extra_root_without_auto_commit`: the same repo, `kb_auto_commit` false; assert `spec.writable_roots.is_empty()`.
- [ ] **Step 3: Run `cargo test -j4 -p factory-engine --test codex_prep kb_` and `cargo test -j4 -p factory-engine --lib kb_folder`.** Expected: `cannot find function kb_folder in this scope`, and the symlink test fails because `cwd` is the link path.
- [ ] **Step 4: Implement** `fn kb_folder(add_dirs: &[PathBuf]) -> Option<&Path>` returning the first entry. In the `kb` branch of `prepare_for_codex`, replace the `self.config.kb_dir()` call by `kb_folder(&spec.add_dirs).map(Path::to_path_buf)`; keep the `kb_auto_commit` and `Repo::open` logic and `spec.cwd = kb_dir` on the new value.
- [ ] **Step 5: Run `cargo test -j4 -p factory-engine`, then `cargo clippy -j4 -p factory-engine`.** Expected: all pass, no warnings.

Next plan: none
