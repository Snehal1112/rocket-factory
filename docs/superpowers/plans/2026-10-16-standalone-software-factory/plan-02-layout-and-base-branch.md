# Layout, Injectable Paths and Base Branch Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every folder the driver uses and the branch it merges into a value that is passed in, so a repository on any branch (such as `v-4.0.0`) can run a job with its factory state kept outside the repository.

**Architecture:** A new `Layout` struct in `factory-core` holds all folder and file paths. A new `base_branch` config key (default `main`) names the branch. `factory-git`'s `Repo` takes the base branch and the worktrees folder as builder values. `Driver`, `BoardStore`, `Engine` and `CliCtx` take a `Layout` and read nothing from a literal path. The lock wait becomes a parameter, and only the real binary reads `FACTORY_LOCK_WAIT`. `Layout::in_repo(repo)` reproduces today's paths, so Rocket's layout works unchanged.

**Tech Stack:** Rust 2021 (cargo 1.94), git2, clap, serde and toml.

**Spec:** `docs/superpowers/specs/2026-10-16-standalone-software-factory-design.md` (sections Layout, Components and Plan series item 2).

## Global Constraints

- Work in `/home/numericlabs/data/rocket/rocket-factory`, on branch `main`. Rocket's checkout is not touched by this plan.
- Every `cargo` command has `-j4`. Never `--workspace` or `--all`. Never set `CARGO_TARGET_DIR`. Test one crate at a time with `-p <crate>`.
- Do not push anywhere. The repo has no remote.
- Commits use conventional commits and are written through the `dev-workflow-skills:1-git-commit` skill. Stage explicit paths and commit with a pathspec, never `git add -A`, `--all` or `.`.
- No `unwrap()` in production paths. No shelling out to `git` (use `git2`).
- Behavior with `Layout::in_repo` and base `main` is byte-for-byte what it is today. Every existing test keeps passing, and its expected text does not change, except where a step below says so.
- A missing or invalid config still makes every diff risky. `Config::fail_closed()` keeps `base_branch = "main"`. Do not touch the risk rule.
- Board JSON stays compatible with the dashboard: no field is added, renamed or removed in `board.json`, `events.jsonl` or `costs.json`.
- Do not edit the byte copies of Rocket files: `.claude/factory/config.toml`, `.claude/factory/commit-context.md`, `.claude/software-factory.md`, `.claude/commands/factory.md` and `.claude/agents/*.md`. Tests read them and they must stay identical to Rocket's. New config documentation goes into `README.md`.
- The dashboard crate (`dashboard/`) keeps its own hardcoded paths. Plan 6 changes it.
- `crates/factory-cli/src/bin/factory-shots.rs` is a developer tool for screenshots. Its default output folder `.superpowers/factory/screenshots` stays.

## Review Focus

- A branch name that is unsafe or wrong is accepted as `base_branch` (empty, spaces, `..`, a leading `-`, a trailing `/` or `.lock`, or starting with `factory/`): each is a config error and falls back to the fail-closed config.
- The base branch is hardcoded again in one place that tests on `main` cannot see: a repo whose base is `v-4.0.0` and that has a local branch also named `main` must merge into `v-4.0.0` and leave `main` alone.
- A path stays hardcoded in the engine, CLI or git crate: a guard test fails when a non-test source line contains `".superpowers`, `".claude` or the word `"main"` as a string literal outside the allowed files.
- A job on a layout whose state, jobs and worktrees folders are outside the repository writes anything inside the repository: after a full job, `<repo>/.superpowers` and `<repo>/.claude` do not exist.
- The lock wait silently changes: with no `FACTORY_LOCK_WAIT` set the wait is 15 seconds as before, a number in the variable still sets it for the real binary, and library code never reads the environment.

---

### Task 1: `Layout` and the `base_branch` setting in factory-core

**Files:**
- Create: `crates/factory-core/src/layout.rs`
- Modify: `crates/factory-core/src/lib.rs` (add `mod layout;` and `pub use layout::Layout;`, next to the other module lines)
- Modify: `crates/factory-core/src/config.rs` (add `base_branch`, default, validation, tests)
- Modify: `README.md` (document the layout and the key)

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces, used by tasks 2 and 3:
  - `factory_core::Layout` with public fields `repo`, `state_dir`, `jobs_dir`, `worktrees_dir`, `config_file`, `commit_context_file` (all `PathBuf`), derives `Debug, Clone, PartialEq, Eq`.
  - `Layout::in_repo(repo: &Path) -> Layout`, and methods `board_file()`, `events_file()`, `costs_file()`, `kb_stop_file()` (all `-> PathBuf`) and `job_dir(&self, dir: &str) -> PathBuf`.
  - `Config.base_branch: String` (default `"main"`), validated by `Config::parse`.

- [ ] **Step 1: Write the failing tests for `Layout`**

Create `crates/factory-core/src/layout.rs` with only the tests first:

```rust
//! Where the driver keeps its files. Everything is passed in, nothing is a literal elsewhere.

use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_repo_reproduces_the_rocket_paths() {
        let l = Layout::in_repo(Path::new("/r"));
        assert_eq!(l.repo, PathBuf::from("/r"));
        assert_eq!(l.board_file(), PathBuf::from("/r/.superpowers/factory/board.json"));
        assert_eq!(l.events_file(), PathBuf::from("/r/.superpowers/factory/events.jsonl"));
        assert_eq!(l.costs_file(), PathBuf::from("/r/.superpowers/factory/costs.json"));
        assert_eq!(l.kb_stop_file(), PathBuf::from("/r/.superpowers/factory/kb-stopped"));
        assert_eq!(l.job_dir("2026-10-16-001-x"), PathBuf::from("/r/.superpowers/sdd/2026-10-16-001-x"));
        assert_eq!(l.worktrees_dir, PathBuf::from("/r/.claude/worktrees"));
        assert_eq!(l.config_file, PathBuf::from("/r/.claude/factory/config.toml"));
        assert_eq!(l.commit_context_file, PathBuf::from("/r/.claude/factory/commit-context.md"));
    }

    #[test]
    fn fields_can_point_outside_the_repo() {
        let mut l = Layout::in_repo(Path::new("/r"));
        l.state_dir = PathBuf::from("/home/u/.factory/projects/p/state");
        assert_eq!(l.board_file(), PathBuf::from("/home/u/.factory/projects/p/state/board.json"));
        assert_eq!(l.kb_stop_file(), PathBuf::from("/home/u/.factory/projects/p/state/kb-stopped"));
    }
}
```

Add to `crates/factory-core/src/lib.rs` the lines `mod layout;` and `pub use layout::Layout;` next to the existing `mod` and `pub use` lines (keep the file's order and style).

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -j4 -p factory-core layout`
Expected: FAIL to compile, `cannot find type Layout`.

- [ ] **Step 3: Implement `Layout`**

Put this above the `#[cfg(test)]` block in `layout.rs`:

```rust
/// The folders and files of one project. `in_repo` gives the layout Rocket uses today.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// The main checkout of the project.
    pub repo: PathBuf,
    /// Holds `board.json`, `events.jsonl`, `costs.json` and `kb-stopped`.
    pub state_dir: PathBuf,
    /// One folder per job, named by the job's `dir`.
    pub jobs_dir: PathBuf,
    /// Holds the job worktrees, one folder per job.
    pub worktrees_dir: PathBuf,
    /// The factory config file.
    pub config_file: PathBuf,
    /// The context file the committer prompt reads.
    pub commit_context_file: PathBuf,
}

impl Layout {
    /// Everything inside the repository, the way Rocket keeps it.
    pub fn in_repo(repo: &Path) -> Layout {
        Layout {
            repo: repo.to_path_buf(),
            state_dir: repo.join(".superpowers/factory"),
            jobs_dir: repo.join(".superpowers/sdd"),
            worktrees_dir: repo.join(".claude/worktrees"),
            config_file: repo.join(".claude/factory/config.toml"),
            commit_context_file: repo.join(".claude/factory/commit-context.md"),
        }
    }

    pub fn board_file(&self) -> PathBuf {
        self.state_dir.join("board.json")
    }

    pub fn events_file(&self) -> PathBuf {
        self.state_dir.join("events.jsonl")
    }

    pub fn costs_file(&self) -> PathBuf {
        self.state_dir.join("costs.json")
    }

    /// The marker that stops automatic KB refreshes.
    pub fn kb_stop_file(&self) -> PathBuf {
        self.state_dir.join("kb-stopped")
    }

    pub fn job_dir(&self, dir: &str) -> PathBuf {
        self.jobs_dir.join(dir)
    }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -j4 -p factory-core layout`
Expected: PASS (2 tests).

- [ ] **Step 5: Write the failing tests for `base_branch`**

Add these tests inside the existing `mod tests` of `crates/factory-core/src/config.rs`:

```rust
    #[test]
    fn base_branch_defaults_to_main_and_survives_fail_closed() {
        assert_eq!(Config::default().base_branch, "main");
        assert_eq!(Config::parse("").expect("empty").base_branch, "main");
        assert_eq!(Config::fail_closed().base_branch, "main");
    }

    #[test]
    fn base_branch_accepts_ordinary_names() {
        for name in ["main", "master", "v-4.0.0", "release/2026.10", "dev_1"] {
            let cfg = Config::parse(&format!("base_branch = \"{name}\"\n")).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(cfg.base_branch, name);
        }
    }

    #[test]
    fn base_branch_rejects_unsafe_names() {
        for name in ["", "  ", "a b", "a..b", "-x", "x/", "/x", "x.lock", "a\\tb", "factory/001-x", "a~b", "a^b", "a:b", "a?b", "a*b", "a[b", "@", "a@{b"] {
            assert!(Config::parse(&format!("base_branch = \"{name}\"\n")).is_err(), "{name:?} must be refused");
        }
    }
```

- [ ] **Step 6: Run to verify it fails**

Run: `cargo test -j4 -p factory-core base_branch`
Expected: FAIL to compile, `no field base_branch`.

- [ ] **Step 7: Implement the key**

In `config.rs`:

1. In `struct Config`, after the `size_medium_max_bytes` field and before `checks`, add:

```rust
    /// The branch jobs start from and merge into.
    pub base_branch: String,
```

2. In `Default for Config`, add `base_branch: "main".to_string(),` in the same position.
3. In `Config::parse`, before `Ok(cfg)`, add:

```rust
        if let Err(why) = check_branch_name(&cfg.base_branch) {
            return Err(CoreError::Config(format!("base_branch: {why}")));
        }
```

4. Below `expand_home`, add:

```rust
/// A branch name git accepts, and not one of the factory's own job branches.
fn check_branch_name(name: &str) -> Result<(), &'static str> {
    if name.trim().is_empty() {
        return Err("must not be empty");
    }
    if name.starts_with('-') || name.starts_with('/') || name.ends_with('/') || name.ends_with(".lock") || name == "@" {
        return Err("is not a valid branch name");
    }
    if name.contains("..") || name.contains("@{") {
        return Err("is not a valid branch name");
    }
    if name.chars().any(|c| c.is_whitespace() || c.is_control() || "~^:?*[\\".contains(c)) {
        return Err("is not a valid branch name");
    }
    if name.starts_with("factory/") {
        return Err("must not start with factory/, that is the job branch prefix");
    }
    Ok(())
}
```

- [ ] **Step 8: Run to verify it passes**

Run: `cargo test -j4 -p factory-core`
Expected: PASS. The existing test that parses `.claude/factory/config.toml` still passes, because a file without the key gets `main`.

- [ ] **Step 9: Document in README**

In `README.md`, add a short section "Layout and base branch": the six `Layout` fields with the `in_repo` defaults, and the config key `base_branch` (default `main`, the branch jobs start from and merge into; a name that git would refuse, or one starting with `factory/`, makes the whole config invalid and falls back to the fail-closed config). Easy short sentences.

- [ ] **Step 10: Check and commit**

Run: `cargo check -j4 -p factory-core --tests` (no warnings).
Commit with the `dev-workflow-skills:1-git-commit` skill, pathspec `crates/factory-core/src/layout.rs crates/factory-core/src/lib.rs crates/factory-core/src/config.rs README.md`. Suggested subject: `feat(core): add Layout and base_branch setting`.

---

### Task 2: Base branch and worktrees folder in factory-git

**Files:**
- Modify: `crates/factory-git/src/repo.rs`
- Modify: `crates/factory-git/src/lib.rs` (export)
- Modify: `crates/factory-git/src/worktree.rs`, `merge.rs`, `state.rs` (use the repo's base branch)
- Modify: `crates/factory-engine/src/kb.rs` (line 137 only, so the engine keeps compiling)
- Modify: `crates/factory-git/tests/common/mod.rs`
- Create: `crates/factory-git/tests/base_branch.rs`

**Interfaces:**
- Consumes: nothing from task 1 (this crate does not depend on `factory-core`).
- Produces, used by task 3:
  - `factory_git::DEFAULT_BASE_BRANCH: &str = "main"` (replaces `MAIN_BRANCH`, which is removed).
  - `Repo::with_base_branch(self, name: &str) -> Repo`, `Repo::with_worktrees_dir(self, dir: PathBuf) -> Repo`, `Repo::base_branch(&self) -> &str`, `Repo::worktrees_dir(&self) -> &Path`.
  - `Repo::open(path)` is unchanged and defaults to base `main` and worktrees at `<root>/.claude/worktrees`.

- [ ] **Step 1: Add a helper that builds a repo on another branch**

In `crates/factory-git/tests/common/mod.rs`, add a second constructor to `Fixture` and make `new` call it:

```rust
    pub fn new() -> Fixture {
        Fixture::on_branch("main")
    }

    /// A temp repo with one commit on `base` and a file `base.txt`.
    pub fn on_branch(base: &str) -> Fixture {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("repo");
        std::fs::create_dir_all(&path).expect("mkdir");
        let mut opts = RepositoryInitOptions::new();
        opts.initial_head(base);
        let repo = Repository::init_opts(&path, &opts).expect("init");
        commit_files(&repo, &[("base.txt", "base\n")], "init");
        Fixture { dir, path }
    }
```

(Move the existing body of `new` into `on_branch`, replacing the literal `"main"`.) Keep `main_sha()` as it is, and add:

```rust
    pub fn branch_tip(&self, branch: &str) -> String {
        branch_sha(&self.git(), branch)
    }
```

- [ ] **Step 2: Write the failing tests**

Create `crates/factory-git/tests/base_branch.rs`. First read `crates/factory-git/tests/worktree.rs` and `crates/factory-git/tests/merge.rs` (or whichever file tests merge) to copy how they create a worktree, commit in it, call the merge and check state, then write four tests with that same API:

1. `worktree_starts_from_the_base_branch`: `Fixture::on_branch("v-4.0.0")`, create a local branch `main` on a different commit (`fx.job_branch` style helper or commit), open `fx.repo().with_base_branch("v-4.0.0")`, call `create_worktree`, assert the new branch's base sha equals `fx.branch_tip("v-4.0.0")` and not the `main` tip.
2. `merge_moves_the_base_branch_only`: with the same two-branch setup, make a job branch with a commit, run the merge the way `merge.rs` tests do, assert `v-4.0.0` advanced and `main` did not.
3. `state_checks_use_the_base_branch`: the preflight / state check that today requires HEAD to be on `main` (`state.rs` around lines 48 to 70) is satisfied when HEAD is on `v-4.0.0` with `with_base_branch("v-4.0.0")`, and refuses (same error as today) when the repo's base is `main` but HEAD is on `v-4.0.0`.
4. `worktrees_dir_can_be_outside_the_repo`: `with_worktrees_dir(<tempdir>/wt)`, `create_worktree` puts the worktree at `<tempdir>/wt/factory-001-x`, nothing is created under `<repo>/.claude`, and `repo.worktrees_dir()` returns the same folder.

- [ ] **Step 3: Run to verify they fail**

Run: `cargo test -j4 -p factory-git --test base_branch`
Expected: FAIL to compile, `no method with_base_branch`.

- [ ] **Step 4: Implement**

In `crates/factory-git/src/repo.rs`:

1. Replace `pub const MAIN_BRANCH: &str = "main";` with:

```rust
/// The base branch of a repo opened without `with_base_branch`.
pub const DEFAULT_BASE_BRANCH: &str = "main";
```

2. Add two fields to `Repo`: `pub(crate) base: String` and `pub(crate) worktrees: PathBuf`. In `open`, set `base: DEFAULT_BASE_BRANCH.to_string()` and `worktrees: root.join(".claude").join("worktrees")` (compute `root` before building the struct; it is already a local).
3. Add methods:

```rust
    /// The same repo with another base branch.
    pub fn with_base_branch(mut self, name: &str) -> Repo {
        self.base = name.to_string();
        self
    }

    /// The same repo with the job worktrees kept in `dir`.
    pub fn with_worktrees_dir(mut self, dir: PathBuf) -> Repo {
        self.worktrees = dir;
        self
    }

    /// The branch jobs start from and merge into.
    pub fn base_branch(&self) -> &str {
        &self.base
    }

    /// The folder that holds the job worktrees.
    pub fn worktrees_dir(&self) -> &Path {
        &self.worktrees
    }
```

4. Replace every use of `MAIN_BRANCH` (`repo.rs:141`, `merge.rs:2,118,122`, `worktree.rs:3,73,93,116,143,155`, `state.rs:2,48,51,70`, `lib.rs:18`) with `self.base` / `&self.base` (`format!("refs/heads/{}", self.base)` in `merge.rs`). Update the `use` lines and the `lib.rs` export to `DEFAULT_BASE_BRANCH`.
5. In `worktree.rs` `create_worktree`, replace `let parent = self.root.join(".claude").join("worktrees");` with `let parent = self.worktrees.clone();`, and fix the doc comment above it (no fixed path in the words). Search the file for other uses of the `.claude/worktrees` literal and fix any.
6. Any error text that says `main` for the base branch (grep the three files for `"main"` and `main` inside `format!`) must name the base: use `self.base`. Texts that mean the checkout stay.

- [ ] **Step 5: Run to verify**

Run: `cargo test -j4 -p factory-git`
Expected: PASS, including the old tests unchanged and the four new ones.

- [ ] **Step 6: Check and commit**

Run: `cargo check -j4 -p factory-git --tests` (no warnings; engine and cli will not compile yet if they use `MAIN_BRANCH`, so also run `grep -rn MAIN_BRANCH crates` and note the remaining uses for task 3: `factory-engine/src/kb.rs:137`).
Commit through the commit skill with pathspec `crates/factory-git`. Suggested subject: `feat(git): take the base branch and worktrees folder`.

Note: `factory-engine` does not compile between this commit and the next one, because `kb.rs` still names `MAIN_BRANCH`. To keep every commit green, in this task also change `crates/factory-engine/src/kb.rs:137` to `self.repo.base_branch()` and make no other engine change. Run `cargo check -j4 -p factory-engine` before the commit.

---

### Task 3: Engine and CLI use the layout, the base branch and an injected lock wait

**Files:**
- Modify: `crates/factory-engine/src/store.rs`, `compat.rs`, `runloop.rs`, `costs.rs`, `sync.rs`, `merge.rs`, `message.rs`, `commit.rs`, `ports.rs`, `lib.rs`, and the file that defines `Engine` (find it with `grep -rn "pub struct Engine" crates/factory-engine/src`)
- Modify: `crates/factory-cli/src/commands.rs`, plus any other CLI source that builds a path from `root` (find them with the grep in Step 1)
- Modify: tests that construct these types: `crates/factory-engine/tests/common/mod.rs`, `build_checks.rs`, `lock_lease.rs`, `costs.rs`, `crates/factory-cli/tests/common/mod.rs`, `crates/factory-cli/tests/support/mod.rs`
- Create: `crates/factory-cli/tests/external_layout.rs`
- Create: `crates/factory-cli/tests/no_hardcoded_paths.rs`

**Interfaces:**
- Consumes: `Layout`, `Config.base_branch` (task 1); `Repo::with_base_branch`, `with_worktrees_dir`, `base_branch()` (task 2).
- Produces:
  - `BoardStore::new(board_path: &Path, wait: Duration) -> BoardStore` (replaces `new(root)` and `at(path)`; `with_wait` stays), `BoardStore::wait(&self) -> Duration`, and `pub const DEFAULT_LOCK_WAIT: Duration` (15 seconds).
  - `Driver::new(layout: Layout, config: Config, runner_for: RunnerFactory) -> Result<Driver, EngineError>`, with `Driver::layout(&self) -> &Layout` and `Driver::with_lock_wait(self, wait: Duration) -> Driver`. `Driver::root()` stays and returns `layout.repo`.
  - `Engine::with_lock_wait(self, wait: Duration) -> Engine` and `Engine::with_commit_context(self, path: PathBuf) -> Engine`. Without `with_commit_context` the committer step is skipped, exactly as today when the file is missing.
  - `engine::costs::costs_path` takes the layout: `costs_path(layout: &Layout) -> PathBuf`.
  - `CliCtx::with_layout(layout: Layout, runner: Arc<dyn AgentRunner>, owner: &str) -> CliCtx`; `CliCtx::new(root, runner, owner)` stays as a wrapper over `Layout::in_repo`; `CliCtx::from_env(root)` reads `FACTORY_LOCK_WAIT` once.
  - Removed: `KB_STOP_FILE`, `COMMIT_CONTEXT_FILE`, `BoardStore::at`, `BoardStore::new(root)`.

- [ ] **Step 1: List every literal to replace**

Run:
```bash
grep -rnE '"main"|\.superpowers|\.claude|FACTORY_LOCK_WAIT|COMMIT_CONTEXT_FILE|KB_STOP_FILE|costs_path|events_path' crates/factory-engine/src crates/factory-cli/src crates/factory-agent/src crates/factory-checks/src | grep -v 'factory-shots.rs'
```
Every hit that is a path or the base branch must end up read from the layout, the repo or the config. Hits inside comments, and text that says "main checkout" (the checkout, not the branch: `compat.rs` lines about the KB stop and `kb.rs:143`), stay. Known hits: `store.rs:33,35`, `compat.rs:60,72,113,121,157,181,189`, `runloop.rs:25,51`, `costs.rs:47`, `sync.rs:37`, `commit.rs:19,228,457`, `message.rs:136`, `commands.rs:68,75,256`.

- [ ] **Step 2: Write the guard test first**

Create `crates/factory-cli/tests/no_hardcoded_paths.rs`:

```rust
//! The engine, the git crate and the CLI take every path and the base branch from outside.

use std::path::{Path, PathBuf};

const ALLOWED: &[&str] = &[
    "factory-core/src/layout.rs",
    "factory-git/src/repo.rs",
    "factory-cli/src/bin/factory-shots.rs",
];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_source_names_a_state_path_or_the_main_branch() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut hits = Vec::new();
    for name in ["factory-engine", "factory-git", "factory-cli", "factory-agent", "factory-checks", "factory-core"] {
        let mut files = Vec::new();
        rust_files(&crates.join(name).join("src"), &mut files);
        for file in files {
            let rel = file.strip_prefix(&crates).expect("under crates").to_string_lossy().replace('\\', "/");
            if ALLOWED.contains(&rel.as_str()) {
                continue;
            }
            let text = std::fs::read_to_string(&file).expect("read");
            // Everything from the first test module on is test code.
            let code = text.split("#[cfg(test)]").next().unwrap_or("");
            for (n, line) in code.lines().enumerate() {
                let t = line.trim_start();
                if t.starts_with("//") {
                    continue;
                }
                if line.contains("\".superpowers") || line.contains("\".claude") || line.contains("\"main\"") {
                    hits.push(format!("{rel}:{}: {}", n + 1, t));
                }
            }
        }
    }
    assert!(hits.is_empty(), "hardcoded paths or branch:\n{}", hits.join("\n"));
}
```

Note `factory-core/src/config.rs` holds `DEFAULT_RISK` strings such as `".claude/factory/config.toml"`. Those are risk patterns, a policy and not a path the driver opens, and plan 3 moves them into presets. Add `"factory-core/src/config.rs"` to `ALLOWED` with a comment saying so.

Run: `cargo test -j4 -p factory-cli --test no_hardcoded_paths`
Expected: FAIL, listing the known hits from Step 1.

- [ ] **Step 3: Write the failing external-layout scenario test**

In `crates/factory-cli/tests/common/mod.rs`, give `RoleRunner` a public constructor so another test file can build it:

```rust
impl RoleRunner {
    pub fn new() -> RoleRunner {
        RoleRunner { counter: AtomicUsize::new(0) }
    }
}
```

Create `crates/factory-cli/tests/external_layout.rs`. It builds a repository whose only branch is `v-4.0.0` (no `main` at all, then add a second local branch `main` pointing at the first commit, to prove it is left alone), with the factory's state, jobs, worktrees, config and commit context in sibling temp folders:

```rust
mod common;

use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use common::{commit_file, RoleRunner};
use factory_agent::AgentRunner;
use factory_cli::{dispatch, Cli, CliCtx};
use factory_core::Layout;
use git2::{Repository, RepositoryInitOptions};

struct External {
    tmp: tempfile::TempDir,
    repo: PathBuf,
    layout: Layout,
    ctx: CliCtx,
}

impl External {
    fn new() -> External {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).expect("mkdir");
        let mut opts = RepositoryInitOptions::new();
        opts.initial_head("v-4.0.0");
        Repository::init_opts(&repo, &opts).expect("init");
        commit_file(&repo, "README.md", "hello\n");
        let git = Repository::open(&repo).expect("open");
        let first = git.head().expect("head").peel_to_commit().expect("commit");
        git.branch("main", &first, false).expect("main branch");
        let home = tmp.path().join("factory-home");
        let layout = Layout {
            repo: repo.clone(),
            state_dir: home.join("state"),
            jobs_dir: home.join("state/jobs"),
            worktrees_dir: home.join("worktrees"),
            config_file: home.join("project.toml"),
            commit_context_file: home.join("commit-context.md"),
        };
        std::fs::create_dir_all(&home).expect("home");
        std::fs::write(&layout.config_file, "base_branch = \"v-4.0.0\"\nrisk_paths = [\"src-tauri/**\"]\n").expect("config");
        std::fs::write(&layout.commit_context_file, "TEST CONTEXT\n").expect("context");
        let runner: Arc<dyn AgentRunner> = Arc::new(RoleRunner::new());
        let ctx = CliCtx::with_layout(layout.clone(), runner, "tester");
        External { tmp, repo, layout, ctx }
    }

    fn run(&self, args: &[&str]) -> Result<String, factory_cli::CliError> {
        let mut argv = vec!["factory"];
        argv.extend_from_slice(args);
        dispatch(Cli::parse_from(argv), &self.ctx)
    }

    fn tip(&self, branch: &str) -> String {
        let git = Repository::open(&self.repo).expect("open");
        let id = git.revparse_single(branch).expect("branch").peel_to_commit().expect("commit").id();
        id.to_string()
    }
}

#[test]
fn a_job_merges_into_a_non_main_base_with_state_kept_outside_the_repo() {
    let fx = External::new();
    let plan_dir = fx.tmp.path().join("plans");
    std::fs::create_dir_all(&plan_dir).expect("plans");
    let plan = plan_dir.join("plan-01-hello.md");
    std::fs::write(&plan, "# Plan\n\n### Task 1: t\n\nNext plan: none\n").expect("plan");
    fx.run(&["new", "hello", "say hello", "--plans", plan.to_str().expect("utf8")]).expect("new");
    let base_before = fx.tip("v-4.0.0");
    let main_before = fx.tip("main");
    assert_eq!(fx.run(&["run", "1"]).expect("run"), "merged");
    assert_ne!(fx.tip("v-4.0.0"), base_before, "the base branch advanced");
    assert_eq!(fx.tip("main"), main_before, "the other branch main is untouched");
    assert!(fx.layout.board_file().exists(), "board kept in the state folder");
    assert!(fx.layout.events_file().exists() || fx.layout.state_dir.exists());
    assert!(fx.layout.worktrees_dir.join("factory-001-hello").exists(), "worktree kept in the worktrees folder");
    assert!(!fx.repo.join(".superpowers").exists(), "nothing written to .superpowers in the repo");
    assert!(!fx.repo.join(".claude").exists(), "nothing written to .claude in the repo");
    fx.run(&["cleanup", "1"]).expect("cleanup");
    assert!(!fx.layout.worktrees_dir.join("factory-001-hello").exists());
}
```

If `a job` needs the `RoleRunner` to import `commit_index` from `common` (it does inside `common`), no change is needed. If the `new` command checks that the plan file is inside the repo or in `docs/`, read `new_job` in `commands.rs` and place the plan file where it accepts one, without changing that rule.

Run: `cargo test -j4 -p factory-cli --test external_layout`
Expected: FAIL to compile, `no function with_layout`.

- [ ] **Step 4: Change `BoardStore` and the lock wait**

In `crates/factory-engine/src/store.rs`:

```rust
/// How long a write waits for the board lock when the caller does not say.
pub const DEFAULT_LOCK_WAIT: Duration = Duration::from_secs(15);

impl BoardStore {
    pub fn new(board_path: &Path, wait: Duration) -> BoardStore {
        BoardStore { board_path: board_path.to_path_buf(), wait }
    }

    pub fn wait(&self) -> Duration {
        self.wait
    }
```

Remove the `std::env::var` call and the old `new(root)` and `at(path)`. Keep `with_wait`, `board_path()` and the rest. Export `DEFAULT_LOCK_WAIT` from `lib.rs`. Update `crates/factory-engine/tests/lock_lease.rs` (`BoardStore::new(d.path()).with_wait(...)` becomes `BoardStore::new(&d.path().join("board.json"), Duration::from_millis(200))`, and the same for the second use). Check that old tests used the same board file name the old `new(root)` implied (`.superpowers/factory/board.json` under the root) and keep the lock-file expectations.

- [ ] **Step 5: Change `Engine`**

In the file that defines `Engine`, add fields `lock_wait: Duration` (default `DEFAULT_LOCK_WAIT`) and `commit_context: Option<PathBuf>` (default `None`), and the two builders:

```rust
    pub fn with_lock_wait(mut self, wait: Duration) -> Engine<'a> {
        self.lock_wait = wait;
        self
    }

    pub fn with_commit_context(mut self, path: PathBuf) -> Engine<'a> {
        self.commit_context = Some(path);
        self
    }
```

(Match the real lifetime parameters of `Engine`.) In `ports.rs:62` replace `crate::BoardStore::at(&self.board_path)` with `crate::BoardStore::new(&self.board_path, self.lock_wait)`. In `commit.rs` (the `std::fs::read_to_string(self.repo.root().join(COMMIT_CONTEXT_FILE))` at about line 228) and `message.rs:136`, read `self.commit_context` instead: when it is `None`, behave as today when the file is missing. Delete the `COMMIT_CONTEXT_FILE` constant and its re-export in `lib.rs`. The test at `commit.rs:457` that reads `.claude/factory/commit-context.md` through `CARGO_MANIFEST_DIR` stays as it is (it reads the byte copy).

- [ ] **Step 6: Change `Driver`**

In `compat.rs`:

1. Replace the `root: PathBuf` field with `layout: Layout` (keep `root()` returning `&self.layout.repo`), and add `lock_wait: Duration`.
2. New constructor:

```rust
    pub fn new(layout: Layout, config: Config, runner_for: RunnerFactory) -> Result<Driver, EngineError> {
        let repo = Repo::open(&layout.repo)?
            .with_base_branch(&config.base_branch)
            .with_worktrees_dir(layout.worktrees_dir.clone());
        let store = BoardStore::new(&layout.board_file(), DEFAULT_LOCK_WAIT);
        Ok(Driver { layout, repo, config, store, runner_for, lease_owner: None, lock_wait: DEFAULT_LOCK_WAIT })
    }

    /// Changes how long board writes wait for the lock.
    pub fn with_lock_wait(mut self, wait: Duration) -> Driver {
        self.store = BoardStore::new(&self.layout.board_file(), wait);
        self.lock_wait = wait;
        self
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }
```

3. `events_path()` returns `self.layout.events_file()`. `job_dir()` returns `self.layout.job_dir(&job.dir)`. `with_engine` builds the `Engine` and chains `.with_lock_wait(self.lock_wait)` and, when `self.layout.commit_context_file.exists()`, `.with_commit_context(self.layout.commit_context_file.clone())`.
4. `commit_message` (`compat.rs:157`): `self.repo.tip(self.repo.base_branch())?`.
5. `kb_refresh`: `let marker = self.layout.kb_stop_file();` and the text with `{KB_STOP_FILE}` prints `marker.display()`. Delete the `KB_STOP_FILE` constant and its re-export. Search the whole workspace for other users of `KB_STOP_FILE` and fix them (`grep -rn KB_STOP_FILE crates dashboard/src`; the dashboard is a separate crate and does not use this one, so a dashboard hit means leave it).
6. `runloop.rs:25`: `driver.layout().jobs_dir`. `runloop.rs:51`: the ledger header names the real plan file, `format!("# SDD ledger, plan: {}", dir.join("plan.md").display())`. No test pins the old header text.
7. `sync.rs:37`: `driver.repo().tip(driver.repo().base_branch())?`.
8. `costs.rs:47`: `pub fn costs_path(layout: &Layout) -> PathBuf { layout.costs_file() }`, and update its callers (`grep -rn costs_path crates`).
9. Texts that name the base as `main`: `merge.rs` lines 85, 95, 109, 117 and 121 become `format!` strings using the repo's base branch name (`repo.base_branch()`); `message.rs` lines 49, 79, 80 and 84 take the base name as an argument (`fixed_message(kind, job_id, slug, base)` and `message_prompt(..., base)`), and their callers pass `self.repo.base_branch()`. The existing unit tests in `message.rs` pass `"main"` and keep their expected text.

- [ ] **Step 7: Change the CLI**

In `crates/factory-cli/src/commands.rs`:

1. `CliCtx` gets `layout: Layout` and `lock_wait: Duration`; `root` is replaced by `layout.repo` (keep a `root()` accessor if other CLI files read `ctx.root`; grep for it).
2. Constructors:

```rust
    pub fn new(root: PathBuf, runner: Arc<dyn AgentRunner>, owner: &str) -> CliCtx {
        CliCtx::with_layout(Layout::in_repo(&root), runner, owner)
    }

    pub fn with_layout(layout: Layout, runner: Arc<dyn AgentRunner>, owner: &str) -> CliCtx {
        CliCtx { layout, runner: Some(runner), owner: owner.to_string(), lock_wait: DEFAULT_LOCK_WAIT }
    }
```

3. `from_env` builds `Layout::in_repo(&root)` and reads the environment once:

```rust
        let lock_wait = std::env::var("FACTORY_LOCK_WAIT")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .map(Duration::from_secs)
            .unwrap_or(DEFAULT_LOCK_WAIT);
```

4. `runner_for` uses `self.layout.events_file()`. `driver()` uses `Config::load(&self.layout.config_file)`, then `Driver::new(self.layout.clone(), config, ...)?.with_lock_wait(self.lock_wait)` and the lease owner.
5. `cleanup` (about line 256): the fallback worktree path is `d.repo().worktrees_dir().join(jr.dir_name())`.
6. Anything else in `crates/factory-cli/src` that reads a path from the Step 1 grep must use `ctx.layout`. `status.rs`, `tokens.rs` and `costs.rs` in the CLI take paths from the driver (`d.job_dir`, `costs_path`): update the callers to the new signatures.
7. Re-export what tests need: `factory_core::Layout` is used by tests directly, so no re-export is needed. Check `crates/factory-cli/src/lib.rs` exports `CliCtx`.

- [ ] **Step 8: Update the remaining test fixtures**

- `crates/factory-engine/tests/common/mod.rs`: the `Engine::new(...)` call (about line 110) adds `.with_commit_context(main.join(".claude/factory/commit-context.md"))` when that file exists, to keep `committing_fixture` behavior. Fix its `Driver::new` use to the new signature (`Driver::new(Layout::in_repo(&main), config, runner)`), if it has one.
- `crates/factory-engine/tests/build_checks.rs:128` and `costs.rs`: same adjustments.
- `crates/factory-cli/tests/common/mod.rs` `hold_lease` and any `BoardStore::new(&self.root)`: use `BoardStore::new(&Layout::in_repo(&self.root).board_file(), Duration::from_secs(15))`. Same for `support/mod.rs`.

- [ ] **Step 9: Run every crate**

Run, one at a time, with these exact commands:
```bash
cargo test -j4 -p factory-core
cargo test -j4 -p factory-git
cargo test -j4 -p factory-agent
cargo test -j4 -p factory-checks
cargo test -j4 -p factory-engine
cargo test -j4 -p factory-cli
cargo check -j4 --manifest-path dashboard/Cargo.toml
```
Expected: all PASS, including `external_layout` and `no_hardcoded_paths`. The dashboard does not depend on these crates, so it is only checked for compile.

If `external_layout` fails because a step writes into the repo (for example the engine creating `.claude` for something), the fix is in the engine, not in the test. If a failure is outside this plan's scope (a prompt that says `cargo` or `yarn`, a Rocket risk path), do not fix it here: plan 3 owns it. Write it down in the report.

- [ ] **Step 10: Final checks and commit**

Run: `grep -rnE 'MAIN_BRANCH|KB_STOP_FILE|COMMIT_CONTEXT_FILE|FACTORY_LOCK_WAIT' crates` and confirm the only hit for `FACTORY_LOCK_WAIT` is `factory-cli/src/commands.rs` (and its doc comment).
Commit through the commit skill with a pathspec that lists exactly the files changed. If the diff is large, split into two commits that each compile and pass: first `refactor(engine): pass a Layout and lock wait` (store, engine, driver, CLI wiring and fixtures), then `test: pin paths and run a job on a non-main base` (the two new test files and the `RoleRunner::new` helper). Suggested subjects are in that order.
