# Issue queue, part 2: the base branch export Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-12-issue-queue-design.md
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Add `Repo::export_tree`, which writes the tracked files of a branch into a fresh folder, so the intake drafter can read tracked files only.

**Architecture:** One method on the git2-based `Repo` in `factory-git`. It walks the tip tree of a branch and writes regular files. It has no agent and no tracker involved, and it never shells out to git.

**Tech Stack:** Rust, `git2`.

## Global Constraints

- Commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`. Never `--workspace` or `--all`.
- No `Cargo.toml` change, no `git` shell-outs, no `/proc` reads.
- No board, ledger or events field is added. Tests use `.expect("...")` and `expect_err`, never the unwrap call. Comments are short full sentences.

## Review Focus

1. A file that is untracked in the working tree is never in the export. Pinned by `export_tree_writes_tracked_files_only`.
2. No `.git` folder appears in the export. Pinned by `export_tree_writes_tracked_files_only`.
3. A tracked symlink is not written, so it cannot point outside the export. Pinned by `export_tree_skips_symlinks`.
4. A destination that already holds files is refused and left alone. Pinned by `export_tree_refuses_a_non_empty_destination`.
5. A branch that does not exist is an error. Pinned by `export_tree_fails_for_an_unknown_branch`.

## File Structure

| File | Responsibility |
|---|---|
| `crates/factory-git/src/repo.rs` | `Repo::export_tree` |
| `crates/factory-git/tests/export_tree.rs` | Tests |

### Task 1: Export the tracked files of a branch

**Files:**
- Modify: `crates/factory-git/src/repo.rs`
- Create: `crates/factory-git/tests/export_tree.rs`
- Test: `crates/factory-git/tests/export_tree.rs`

**Interfaces:**
- Consumes: `Repo::tip` and the way `file_at_branch` reads a blob of a branch; `GitError::Refused` and `GitError::NotFound`.
- Produces: `pub fn export_tree(&self, branch: &str, dest: &Path) -> Result<()>` on `Repo`. The intake pass of the next plan calls it.

- [ ] **Step 1: Read `file_at_branch` and `tip` in `crates/factory-git/src/repo.rs` and the fixtures in `crates/factory-git/tests/common/mod.rs` (`Fixture::on_branch`, `commit_files`) and `tests/tip_files.rs`.**
- [ ] **Step 2: Write the failing tests** in `export_tree.rs`, building the repo with `Fixture::on_branch("main")` and `commit_files`:
  - `export_tree_writes_tracked_files_only`: commit `src/a.txt` (text `a\n`) and `docs/b.txt`, then write an untracked `.env` in the working tree; after `repo.export_tree("main", dest)` the two files exist with the same text, `dest/.env` and `dest/.git` do not, and `base.txt` from the fixture is there.
  - `export_tree_skips_symlinks`: commit a symlink entry (`std::os::unix::fs::symlink` and add it to the index) named `link`; `dest/link` does not exist.
  - `export_tree_refuses_a_non_empty_destination`: `dest` holds one file; the result is `Err` matching `GitError::Refused` and the file is untouched.
  - `export_tree_fails_for_an_unknown_branch`: `export_tree("nope", dest)` is `Err`.
- [ ] **Step 3: Run `cargo test -j4 -p factory-git --test export_tree`.** Expected: `no method named export_tree found for struct Repo`.
- [ ] **Step 4: Implement `export_tree`.** Resolve the branch tip commit, walk its tree with `Tree::walk`, create parent folders, write each blob entry with filemode `0o100644` or `0o100755` (set the executable bit for the second), skip symlink (`0o120000`) and submodule (`0o160000`) entries, create `dest` when it is missing, and refuse a `dest` that exists and is not empty before writing anything.
- [ ] **Step 5: Run `cargo test -j4 -p factory-git` then `cargo clippy -j4 -p factory-git`.** Expected: all pass, no warnings.

Next plan: docs/superpowers/plans/2026-11-12-issue-queue/plan-03-issue-queue.md
