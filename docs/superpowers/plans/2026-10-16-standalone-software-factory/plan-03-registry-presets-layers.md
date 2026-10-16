# Project Registry, Presets and Layered Settings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a project be registered once in `~/.factory/` with a preset, so its settings, agent files and prompt wording come from the factory home and a repo needs no committed factory files.

**Architecture:** Prompt wording that names cargo, yarn or Rocket moves out of `specs.rs` into new `Config` keys that the `rust` and `go` presets fill. A new `Home` and project registry live in `factory-core`. A resolver merges three layers (preset, project, repo override read from the base branch tip) into one `Config`, one `Layout` and a list of agent files, and falls back to the fail-closed config on any problem. The engine writes the resolved agent files into each job worktree and git-excludes them. A `factory project` command group manages the registry and works outside a git repo.

**Tech Stack:** Rust 2021 (cargo 1.94), serde, toml 0.8, clap 4, git2 0.19.

**Spec:** `docs/superpowers/specs/2026-10-16-standalone-software-factory-design.md` (sections Layout, Settings and agent resolution, Agent files in job worktrees, Components and Plan series item 3).

## Global Constraints

- Work in `/home/numericlabs/data/rocket/rocket-factory`, on a branch `plan-03-registry` created from `main`. Fast-forward `main` at the end.
- Every `cargo` command has `-j4`. Never `--workspace` or `--all`. Never set `CARGO_TARGET_DIR`. Test one crate at a time with `-p <crate>`.
- Do not push anywhere. The repo has no remote.
- Commits use conventional commits and are written through the `dev-workflow-skills:1-git-commit` skill. Stage explicit paths and commit with a pathspec, never `git add -A`, `--all` or `.`.
- No unwrap calls in production paths (tests may use `expect`). No shelling out to `git` (use `git2`).
- Do not edit the byte copies of Rocket files: `.claude/factory/config.toml`, `.claude/factory/commit-context.md`, `.claude/software-factory.md`, `.claude/commands/factory.md`, `.claude/agents/*.md`. Tests read them.
- Board JSON, `events.jsonl` and `costs.json` formats do not change. The `dashboard/` crate is untouched.
- A missing or invalid setting anywhere in the layers makes the effective config fail closed (every diff risky), with the reason recorded. The existing `Config::fail_closed()` rule is not weakened.
- A repo override can add risk paths and checks. It cannot remove or replace a risk path. `.factory/**` is always a risk path. The override is read from the base branch tip, never from the working tree or a job branch.
- Project names match `^[a-z0-9][a-z0-9-]{0,39}$`. A name is used as a folder name, so a path separator, `..`, a dot, an uppercase letter or a space is refused.
- A preset or project file that already exists in the factory home is never overwritten by seeding.
- Behavior without a project (the in-repo `Layout`, no agent files, no prompt keys) keeps working. Existing tests keep passing, except tests that pinned the removed Rocket prompt text (Task 1 says how to change them).

## Out of scope for this plan

- `--project` on the existing commands and the project lease: plan 4.
- The cargo `-j4` / no `--workspace` check rule in `factory-checks/src/plan.rs` (`validate_command`), `DEFAULT_RISK` in `config.rs` and `is_doc_path` in `risk.rs`: they stay and the guard test keeps allowing them. They are removed when Rocket moves over in plan 8.
- The KB refresher agent, which runs in the main checkout and still needs its agent file there.
- The global `~/.factory/config.toml` (parallel limit, daily budget, listen address): plans 5 and 7.
- Rocket's own prompt lines (shadcn, Tauri, Zustand): they become Rocket's project settings in plan 8.

## Review Focus

- A repo override weakens the gates: an override with an unknown key, an empty `checks`, a `risk_paths` that tries to drop a preset path, or a missing `.factory/**` risk path. Each must either be refused (fail closed) or have no effect on removal.
- The override is read from the wrong place: a changed `.factory/override.toml` in the working tree or on a job branch must be ignored, only the base branch tip counts.
- A project name or a path escapes the factory home: `../x`, `a/b`, `A`, an empty name, a name of 41 characters, `.`.
- Agent files are committed into the target repo by a job: the committer's staging must not include them, and a file that is already tracked in the repo must not be overwritten.
- A broken project hides the others: a registry entry whose repo path is gone is listed as disabled with a reason, and `list` still shows every other project.

---

### Task 1: Presets and prompt keys

**Files:**
- Create: `crates/factory-core/src/preset.rs`
- Create: `crates/factory-core/presets/rust/config.toml`, `commit-context.md`, `agents/factory-builder.md`, `agents/factory-code-reviewer.md`, `agents/factory-committer.md`, `agents/factory-kb-refresher.md`
- Create: the same set under `crates/factory-core/presets/go/`
- Modify: `crates/factory-core/src/lib.rs` (add `pub mod preset;`)
- Modify: `crates/factory-core/src/config.rs` (four new keys)
- Modify: `crates/factory-engine/src/specs.rs` (use the keys, remove the Rocket wording)
- Modify: tests that pinned the removed wording (find them with the grep in Step 4)
- Modify: `crates/factory-cli/src/cli.rs` (`about` text)

**Interfaces:**
- Consumes: nothing from earlier tasks of this plan.
- Produces, used by tasks 2 and 3:
  - `Config` keys, all `Vec<String>`, default empty: `read_first`, `builder_rules`, `check_hints`, `reviewer_rules`.
  - `factory_core::preset::Preset { pub name: &'static str, pub config: &'static str, pub commit_context: &'static str, pub agents: &'static [(&'static str, &'static str)] }` where `agents` pairs a file name such as `"factory-builder.md"` with its content.
  - `preset::PRESET_NAMES: [&str; 2] = ["rust", "go"]`, `preset::embedded(name: &str) -> Option<&'static Preset>`, and `preset::seed(presets_dir: &Path) -> Result<Vec<PathBuf>, CoreError>` which writes every embedded preset file under `presets_dir/<name>/` that does not exist yet and returns the files it wrote.

- [ ] **Step 1: Write the failing config tests**

In `crates/factory-core/src/config.rs`, inside `mod tests`:

```rust
    #[test]
    fn prompt_keys_default_to_empty_and_parse() {
        let cfg = Config::default();
        assert!(cfg.read_first.is_empty() && cfg.builder_rules.is_empty());
        assert!(cfg.check_hints.is_empty() && cfg.reviewer_rules.is_empty());
        let cfg = Config::parse(
            "read_first = [\"CLAUDE.md\"]\nbuilder_rules = [\"a\", \"b\"]\ncheck_hints = [\"make test\"]\nreviewer_rules = [\"c\"]\n",
        )
        .expect("parses");
        assert_eq!(cfg.read_first, vec!["CLAUDE.md".to_string()]);
        assert_eq!(cfg.builder_rules.len(), 2);
        assert_eq!(cfg.check_hints, vec!["make test".to_string()]);
        assert_eq!(cfg.reviewer_rules, vec!["c".to_string()]);
    }

    #[test]
    fn prompt_lines_must_be_one_line_and_not_empty() {
        for body in ["builder_rules = [\"\"]", "reviewer_rules = [\"a\\nb\"]", "read_first = [\"  \"]", "check_hints = [\"x\\ry\"]"] {
            assert!(Config::parse(&format!("{body}\n")).is_err(), "{body} must be refused");
        }
    }
```

Run: `cargo test -j4 -p factory-core prompt_`
Expected: FAIL to compile (`no field read_first`).

- [ ] **Step 2: Implement the keys**

In `config.rs`, add to `struct Config` (after `base_branch`, before `checks`) with doc comments:

```rust
    /// Files or folders the builder reads before it starts, such as `CLAUDE.md`.
    pub read_first: Vec<String>,
    /// Rules added to the builder prompt, one sentence each.
    pub builder_rules: Vec<String>,
    /// Example check commands named to the builder, one command each.
    pub check_hints: Vec<String>,
    /// Rules added to the reviewer prompt, one sentence each.
    pub reviewer_rules: Vec<String>,
```

Add `Vec::new()` for each in `Default`. In `Config::parse` before `Ok(cfg)`:

```rust
        for (key, lines) in [
            ("read_first", &cfg.read_first),
            ("builder_rules", &cfg.builder_rules),
            ("check_hints", &cfg.check_hints),
            ("reviewer_rules", &cfg.reviewer_rules),
        ] {
            if lines.iter().any(|l| l.trim().is_empty() || l.contains('\n') || l.contains('\r')) {
                return Err(CoreError::Config(format!("{key}: every line must be one non-empty line")));
            }
        }
```

Run: `cargo test -j4 -p factory-core prompt_` — Expected: PASS.

- [ ] **Step 3: Use the keys in the prompts**

Read `crates/factory-engine/src/specs.rs` lines 50 to 170. The methods `builder_first_spec`, `builder_task_spec`, `builder_resume_spec` and `reviewer_spec` build their text with `format!`. Change them as follows, and keep every sentence that does not name a tool or a Rocket file:

1. "You are the builder in Rocket's software factory" becomes "You are the builder in a software factory" in both builder prompts.
2. The line "Read CLAUDE.md, the rule files for the areas you touch under .claude/rules/, and the plan at {plan}." becomes: when `config.read_first` is empty, "Read the plan at {plan}."; otherwise "Read {list}, and the plan at {plan}." where `{list}` joins `read_first` with ", ".
3. The "Rules:" sentence that names `cargo check -j4`, `cargo test`, `--workspace`, `CARGO_TARGET_DIR`, `yarn install`, no unwrap, no git CLI, shadcn and the rest is replaced by the `builder_rules` lines joined with a single space (each line already ends its own sentence). With no rules the "Rules:" sentence is left out entirely.
4. `builder_resume_spec`: the parenthesis "(`cargo check -j4`, targeted `cargo test -j4 -p <crate> <test_name>`, yarn checks)" becomes "(for example {hints})" with `{hints}` the `check_hints` joined with ", ", each wrapped in backticks. The parenthesis is left out when `check_hints` is empty.
5. `reviewer_spec` (it is a `pub fn` that takes no config today): add a parameter `rules: &[String]` after the existing ones. The sentence naming the three `.claude/rules/*.md` files and the Rocket hard rules becomes the `rules` lines joined with a single space; with no rules it is left out. Update its callers (`review.rs:30` and any test) to pass `&self.config.reviewer_rules`.

Keep the order of the remaining sentences. Run `cargo check -j4 -p factory-engine`.

- [ ] **Step 4: Fix the tests that pinned the removed wording**

Run: `grep -rnE 'cargo check -j4|yarn|Rocket|rust-ddd|shadcn|CLAUDE.md' crates/factory-engine/tests crates/factory-engine/src crates/factory-cli/tests | grep -v 'preset'`
For every test that asserts a prompt contains a removed sentence, change the test to build its `Config` with the matching key (for example `Config { builder_rules: vec!["Run cargo check -j4.".into()], ..Config::default() }`) and assert the sentence comes from that key, so the test proves the wiring. Add two tests in `specs.rs` (or the engine tests where the others live): with default config the first builder prompt contains the plan path and does not contain "cargo", "yarn" or "Rocket"; with one rule and one `read_first` entry the prompt contains both. Do not weaken an assertion to make it pass.

- [ ] **Step 5: Write the failing preset tests**

Create `crates/factory-core/src/preset.rs` with tests first:

```rust
//! The presets that ship with the factory. `seed` copies them into the factory home.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;

    #[test]
    fn both_presets_exist_and_parse_as_config() {
        for name in PRESET_NAMES {
            let p = embedded(name).unwrap_or_else(|| panic!("{name} missing"));
            let cfg = Config::parse(p.config).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(!cfg.degraded);
            assert!(!cfg.risk_paths.is_empty(), "{name} sets risk_paths");
            assert!(!cfg.builder_rules.is_empty() && !cfg.reviewer_rules.is_empty(), "{name} fills the prompt keys");
            assert!(!cfg.checks.is_empty(), "{name} has checks");
        }
        assert!(embedded("cobol").is_none());
    }

    #[test]
    fn presets_ship_the_four_agents_and_name_no_rocket_file() {
        for name in PRESET_NAMES {
            let p = embedded(name).expect("preset");
            let mut names: Vec<&str> = p.agents.iter().map(|(n, _)| *n).collect();
            names.sort();
            assert_eq!(names, ["factory-builder.md", "factory-code-reviewer.md", "factory-committer.md", "factory-kb-refresher.md"]);
            for (file, text) in p.agents {
                assert!(text.starts_with("---\n"), "{name}/{file} has frontmatter");
                assert!(text.contains(&format!("name: {}", file.trim_end_matches(".md"))), "{name}/{file} names itself");
                for banned in ["Rocket", "src-tauri", "shadcn", "rocket-", "tools/factory", "yarn"] {
                    assert!(!text.contains(banned), "{name}/{file} must not mention {banned}");
                }
            }
            assert!(!p.commit_context.contains("Rocket"));
        }
    }

    #[test]
    fn rust_checks_use_cargo_with_j4_and_go_checks_use_go() {
        let rust = Config::parse(embedded("rust").expect("rust").config).expect("parse");
        let all: Vec<&String> = rust.checks.iter().flat_map(|c| &c.commands).collect();
        assert!(all.iter().all(|c| !c.starts_with("cargo") || c.contains("-j4")));
        let go = Config::parse(embedded("go").expect("go").config).expect("parse");
        assert!(go.checks.iter().flat_map(|c| &c.commands).any(|c| c.starts_with("go test")));
    }

    #[test]
    fn seed_writes_missing_files_and_never_overwrites() {
        let dir = tempfile::tempdir().expect("tmp");
        let wrote = seed(dir.path()).expect("seed");
        assert!(wrote.iter().any(|p| p.ends_with("rust/config.toml")));
        assert!(wrote.iter().any(|p| p.ends_with("go/agents/factory-builder.md")));
        let edited = dir.path().join("rust/config.toml");
        std::fs::write(&edited, "# mine\n").expect("edit");
        std::fs::remove_file(dir.path().join("go/commit-context.md")).expect("rm");
        let again = seed(dir.path()).expect("seed again");
        assert_eq!(again, vec![dir.path().join("go/commit-context.md")]);
        assert_eq!(std::fs::read_to_string(&edited).expect("read"), "# mine\n");
    }
}
```

Run: `cargo test -j4 -p factory-core preset` — Expected: FAIL to compile.

- [ ] **Step 6: Write the preset files and the module**

Create the files under `crates/factory-core/presets/<name>/`.

`rust/config.toml` (keys must all be valid `Config` keys; use real content, not placeholders):

```toml
# Preset: Rust workspace. Edit this file in ~/.factory/presets/rust/ to change every project that uses it.
risk_paths = [
  "**/Cargo.toml",
  "**/Cargo.lock",
  "**/build.rs",
  "**/*auth*",
  "**/*secret*",
  "**/*credential*",
  ".github/**",
]
read_first = ["CLAUDE.md"]
builder_rules = [
  "Run `cargo check -j4` and `cargo test -j4 -p <crate> <test_name>`; never use --workspace or --all; never set CARGO_TARGET_DIR.",
  "No unwrap in production Rust.",
  "Do not shell out to the git CLI from Rust code, use the git2 crate.",
]
check_hints = ["cargo check -j4", "cargo test -j4 -p <crate> <test_name>"]
reviewer_rules = [
  "Judge correctness, error handling (no unwrap in production paths), test quality and the project's own rules in CLAUDE.md.",
]

[[checks]]
paths = ["crates/*/**"]
commands = ["cargo check -j4 -p {crate}", "cargo test -j4 -p {crate}"]
```

`go/config.toml`: same shape with `risk_paths = ["go.mod", "go.sum", "**/*auth*", "**/*secret*", "**/*credential*", "**/Dockerfile", ".github/**"]`, `read_first = ["CLAUDE.md"]`, `builder_rules = ["Run `go build ./...` and `go test ./...` for the packages you touch; keep functions small and handle every error.", "Do not ignore returned errors."]`, `check_hints = ["go build ./...", "go test ./..."]`, `reviewer_rules = ["Judge correctness, error handling (no ignored errors), test quality and the project's own rules in CLAUDE.md."]` and one check `paths = ["**/*.go"]`, `commands = ["go vet ./...", "go test ./..."]`.

`commit-context.md` for each: a short generic document for the committer: conventional commits, one logical change per commit, subject at most 72 characters, explain why in the body. Do not mention Rocket.

`agents/*.md`: start from the four Rocket files in `.claude/agents/` (read them), keep the frontmatter keys (`name`, `description`, `tools`, `model` and any others they have, with the same values for `tools`), and rewrite the body so it names no Rocket file, crate, rule path or tool: refer to "the project's CLAUDE.md and the rules in the job prompt" instead. The Rust and Go variants differ only in one short "Stack" paragraph (cargo or go commands). The `description` lines must not contain "Rocket".

`preset.rs` (above the tests):

```rust
use std::path::{Path, PathBuf};

use crate::CoreError;

pub const PRESET_NAMES: [&str; 2] = ["rust", "go"];

/// One preset, embedded in the binary.
pub struct Preset {
    pub name: &'static str,
    pub config: &'static str,
    pub commit_context: &'static str,
    pub agents: &'static [(&'static str, &'static str)],
}

static RUST: Preset = Preset {
    name: "rust",
    config: include_str!("../presets/rust/config.toml"),
    commit_context: include_str!("../presets/rust/commit-context.md"),
    agents: &[
        ("factory-builder.md", include_str!("../presets/rust/agents/factory-builder.md")),
        ("factory-code-reviewer.md", include_str!("../presets/rust/agents/factory-code-reviewer.md")),
        ("factory-committer.md", include_str!("../presets/rust/agents/factory-committer.md")),
        ("factory-kb-refresher.md", include_str!("../presets/rust/agents/factory-kb-refresher.md")),
    ],
};

static GO: Preset = Preset { /* the same with "go" */ };

pub fn embedded(name: &str) -> Option<&'static Preset> {
    match name {
        "rust" => Some(&RUST),
        "go" => Some(&GO),
        _ => None,
    }
}

/// Writes every embedded preset file that is missing. Existing files are never touched.
pub fn seed(presets_dir: &Path) -> Result<Vec<PathBuf>, CoreError> {
    let mut wrote = Vec::new();
    for name in PRESET_NAMES {
        let Some(p) = embedded(name) else { continue };
        let base = presets_dir.join(name);
        let mut files: Vec<(PathBuf, &str)> =
            vec![(base.join("config.toml"), p.config), (base.join("commit-context.md"), p.commit_context)];
        for (file, text) in p.agents {
            files.push((base.join("agents").join(file), text));
        }
        for (path, text) in files {
            if path.exists() {
                continue;
            }
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| CoreError::io(parent, e))?;
            }
            std::fs::write(&path, text).map_err(|e| CoreError::io(&path, e))?;
            wrote.push(path);
        }
    }
    Ok(wrote)
}
```

Add `pub mod preset;` to `lib.rs` next to the other modules. Check `CoreError::io` has the signature used above (it is used in `Config::try_load`); adapt only if it differs. Change `crates/factory-cli/src/cli.rs` `about` from "Rocket software factory driver" to "Software factory driver".

- [ ] **Step 7: Run everything and commit**

Run, one at a time: `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`, `cargo check -j4 --manifest-path dashboard/Cargo.toml`.
Expected: PASS. The `no_hardcoded_paths` guard still passes (the presets are data files, `preset.rs` has no path literal).
Commit through the commit skill, split in two if the diff is large: `feat(core): add prompt keys and rust and go presets`, then `refactor(engine): read prompt wording from config`.

---

### Task 2: Registry, `project.toml` and three-layer resolve

**Files:**
- Create: `crates/factory-core/src/home.rs`, `project.rs`, `resolve.rs`
- Modify: `crates/factory-core/src/lib.rs`
- Modify: `crates/factory-git/src/repo.rs` or a new `crates/factory-git/src/tip.rs` (read files at a branch tip), `lib.rs`
- Create: `crates/factory-engine/src/tipfiles.rs` (adapter), modify `crates/factory-engine/src/lib.rs`
- Test: unit tests in the new files, `crates/factory-git/tests/tip_files.rs`

**Interfaces:**
- Consumes: `Config`, `Layout`, `preset::{embedded, seed, PRESET_NAMES}`.
- Produces, used by task 3:
  - `Home { pub dir: PathBuf }` with `Home::at(dir)`, `Home::from_env() -> Result<Home, CoreError>` (`FACTORY_HOME`, else `$HOME/.factory`, error when neither is set), `presets_dir()`, `projects_dir()`, `project_dir(name)`.
  - `project::validate_name(&str) -> Result<(), CoreError>`, `ProjectFile { pub repo: PathBuf, pub preset: String, pub settings: toml::Table }` (`deny_unknown_fields`, `repo` must be absolute), `project::{add, list, get, remove}`:
    - `add(home, name, repo: &Path, preset: &str, base: Option<&str>, repo_ok: &dyn Fn(&Path) -> Result<(), String>) -> Result<PathBuf, CoreError>`: validates the name, the preset, an absolute existing repo path and `repo_ok`, refuses a second project for the same repo path and an existing name, seeds the presets, creates `projects/<name>/{agents,state,worktrees}` and writes `project.toml`. Returns the project folder.
    - `list(home) -> Result<Vec<ProjectEntry>, CoreError>` with `ProjectEntry { name, file: Result<ProjectFile, String> }` sorted by name; a broken `project.toml` is an `Err(reason)` entry, never a failure of the whole list.
    - `get(home, name) -> Result<ProjectFile, CoreError>`, `remove(home, name) -> Result<(), CoreError>` (deletes `projects/<name>/` only).
    - `project::layout(home, name, file: &ProjectFile) -> Layout` (repo from the file, `state_dir` = `projects/<name>/state`, `jobs_dir` = `state/jobs`, `worktrees_dir` = `projects/<name>/worktrees`, `config_file` = `project.toml`, `commit_context_file` = the project's own `commit-context.md` when it exists, else the preset's).
  - `trait BaseTipFiles { fn read_file(&self, rel: &str) -> Result<Option<String>, String>; fn list_dir(&self, rel: &str) -> Result<Vec<(String, String)>, String>; }` in `resolve.rs`.
  - `AgentFile { pub name: String, pub content: String }` and `Resolved { pub config: Config, pub layout: Layout, pub agents: Vec<AgentFile>, pub problems: Vec<String> }`.
  - `resolve::resolve(home: &Home, name: &str, tip: &dyn BaseTipFiles) -> Result<Resolved, CoreError>` (only an unknown project is an `Err`; every other problem sets `config = fail-closed` and fills `problems`).
  - `factory_git::Repo::file_at_branch(&self, branch: &str, rel: &str) -> Result<Option<String>>` and `Repo::dir_at_branch(&self, branch: &str, rel: &str) -> Result<Vec<(String, String)>>` (file names with text content of the regular files directly inside the folder; an absent folder gives an empty list).
  - `factory_engine::RepoTip<'a>(pub &'a Repo)` implementing `BaseTipFiles` for the repo's own base branch.

- [ ] **Step 1: Write failing tests for the git tip reader**

Create `crates/factory-git/tests/tip_files.rs` using `tests/common/mod.rs` (`Fixture`, `commit_files` helpers; read them first). Tests: `file_at_branch` returns the text of a file committed on a branch and `None` for a missing file; a change made in the working tree but not committed is not seen; a file committed only on a job branch is not seen from the base branch; `dir_at_branch(".factory/agents")` lists the committed files with content and returns an empty list when the folder is absent; a missing branch is an error.
Run: `cargo test -j4 -p factory-git --test tip_files` — Expected: FAIL to compile.

- [ ] **Step 2: Implement the tip reader**

In `factory-git`, add `Repo::file_at_branch` and `Repo::dir_at_branch` using git2 (`find_branch` / `revparse_single`, `commit.tree()`, `tree.get_path`, `blob.content()` as UTF-8, entries of a subtree that are blobs). A non-UTF-8 file is an error naming the path. Export nothing new from `lib.rs` (they are methods). Run the test: PASS.

- [ ] **Step 3: Write failing tests for `Home` and the registry**

In `home.rs` and `project.rs` put unit tests (use `tempfile`; a repo path only needs to be an existing directory because the git check is the `repo_ok` closure). Cover:
- `Home::at(dir)` paths; `from_env` honors `FACTORY_HOME` (set the variable inside the test and restore it; keep this test in one test fn to avoid races with other tests, and use a serial guard `static ENV: Mutex<()>`).
- `validate_name` accepts `rocket`, `rocket-vault`, `a1`, 40 characters; refuses `""`, `A`, `a b`, `../x`, `a/b`, `.`, `-x`, 41 characters.
- `add` creates the folder tree, seeds presets (a `presets/rust/config.toml` exists afterwards), writes a `project.toml` that `get` reads back with the absolute repo path, and stores `base_branch` in `settings` when `base` is given.
- `add` refuses: an unknown preset, a relative repo path, a repo path that does not exist, a `repo_ok` closure that returns an error, an existing name, and a second name for the same repo path. Each refusal leaves no folder behind.
- `list` returns projects sorted by name and keeps going past a project whose `project.toml` is broken (an `Err` entry).
- `remove` deletes `projects/<name>` and nothing else; removing an unknown name is an error.
- `layout` points `state_dir`, `jobs_dir` and `worktrees_dir` inside the project folder, and picks the project's `commit-context.md` over the preset's when both exist.

Run: `cargo test -j4 -p factory-core home project` — Expected: FAIL to compile.

- [ ] **Step 4: Implement `Home` and the registry**

Implement exactly the interfaces above. Use `toml` for `project.toml` (`serde` derive with `deny_unknown_fields`). `add` checks everything first, then creates the folders, then writes the file, and removes the folder it created if a later step fails. Repo identity for the duplicate check is `std::fs::canonicalize` of the path, compared against the canonicalized `repo` of every existing project (skip entries whose file is broken). Store the canonical path.

Run: `cargo test -j4 -p factory-core` — Expected: PASS.

- [ ] **Step 5: Write failing tests for `resolve`**

In `resolve.rs` tests, with an in-memory `BaseTipFiles` (a struct holding a `HashMap<String, String>` for files and a map for dirs) and a registered temp project (use `project::add`):
- No override: `config` equals the preset parsed with the project's `settings` on top (`base_branch` from `add` with a base), `problems` is empty, `agents` are the preset's four files.
- A project `settings` key replaces the preset's (`diff_cap_lines`); `risk_paths` in `settings` replaces the preset list.
- A repo override `risk_paths = ["extra/**"]` gives `risk_paths` = the layer's list plus `extra/**`, never fewer entries than without the override.
- `.factory/**` is in `risk_paths` in every case, once, including when the override already lists it.
- A repo override `checks` are appended after the preset's checks.
- A repo override with any other key (`diff_cap_lines`, `base_branch`, `budget_weighted`, a typo) gives `problems` non-empty and `config.degraded == true` with `risk_paths == ["**"]`; the fail-closed config still carries the project's `base_branch`.
- An override that is not valid TOML, a broken preset file, a preset directory that is missing and a `settings` key that fails `Config::parse` validation each give a degraded config and a problem text.
- Agent files: a file in `projects/<name>/agents/` replaces the preset's file of the same name, a file from `.factory/agents/` (via `list_dir`) replaces both, a new name is added, and the result is sorted by name.
- Unknown project: `Err`.
- `layout` in the result equals `project::layout(...)`.

Run: `cargo test -j4 -p factory-core resolve` — Expected: FAIL to compile.

- [ ] **Step 6: Implement `resolve`**

Rules, in this order:
1. `get` the project (unknown → `Err`). Read `presets/<preset>/config.toml` from the home (not the embedded copy: users edit the home copy); a missing file is a problem.
2. Parse the preset text and the project `settings` as `toml::Table`. Merge top level key by key, later wins (a value replaces, it does not deep-merge).
3. Read the override with `tip.read_file(".factory/override.toml")`. `None` means no override. Parse as `toml::Table`; allowed keys are exactly `risk_paths` (array of strings) and `checks` (array of tables). Any other key is a problem.
4. Apply the override: `risk_paths` = merged list followed by the override's entries; `checks` = merged checks followed by the override's. Then append `.factory/**` to `risk_paths` if it is not there. Remove duplicates keeping the first occurrence.
5. Serialize the merged table to a string and run `Config::parse` on it (so validation and `deny_unknown_fields` apply).
6. Agents: preset files from `presets/<preset>/agents/`, then `projects/<name>/agents/`, then `tip.list_dir(".factory/agents")`, each layer replacing files of the same name. Only `*.md` file names are taken, and a name containing a path separator is a problem. Sort by name.
7. Any problem anywhere: `config = Config { base_branch: <project's base_branch if valid else "main">, ..Config::fail_closed() }`, `agents` still returned if they could be read (they are used only for display), `problems` filled with one sentence per problem.

Run: `cargo test -j4 -p factory-core` — Expected: PASS.

- [ ] **Step 7: The engine adapter**

Create `crates/factory-engine/src/tipfiles.rs`:

```rust
use factory_core::BaseTipFiles;
use factory_git::Repo;

/// Reads files from the tip of the repo's base branch.
pub struct RepoTip<'a>(pub &'a Repo);

impl BaseTipFiles for RepoTip<'_> {
    fn read_file(&self, rel: &str) -> Result<Option<String>, String> {
        self.0.file_at_branch(self.0.base_branch(), rel).map_err(|e| e.to_string())
    }
    fn list_dir(&self, rel: &str) -> Result<Vec<(String, String)>, String> {
        self.0.dir_at_branch(self.0.base_branch(), rel).map_err(|e| e.to_string())
    }
}
```

Export `BaseTipFiles` from `factory-core` (`pub use resolve::{...}` in `lib.rs`) and `RepoTip` from `factory-engine`. Add one engine test: open a temp repo on branch `v-4.0.0` whose tip has `.factory/override.toml` and `.factory/agents/factory-builder.md`, change both in the working tree without committing, and assert `RepoTip` returns the committed text.

- [ ] **Step 8: Run everything and commit**

Run one at a time: `cargo test -j4 -p factory-core`, `-p factory-git`, `-p factory-engine`, `-p factory-cli`.
Commit via the commit skill: `feat(git): read files at a branch tip`, then `feat(core): add project registry and layered settings`, then `feat(engine): add base tip reader`.

---

### Task 3: Agent files in worktrees and the `factory project` commands

**Files:**
- Modify: `crates/factory-git/src/repo.rs` or a new `crates/factory-git/src/exclude.rs`, `lib.rs`
- Modify: `crates/factory-engine/src/lib.rs` (`Engine::with_agent_files`), `build.rs` (`start_worktree`), `compat.rs` (`Driver::with_agent_files`, pass to the engine)
- Modify: `crates/factory-cli/src/cli.rs`, `commands.rs`, `main.rs`, `lib.rs`; create `crates/factory-cli/src/project.rs`
- Modify: `README.md`
- Test: `crates/factory-git/tests/exclude.rs`, `crates/factory-engine/tests/agent_files.rs`, `crates/factory-cli/tests/project_cmd.rs`

**Interfaces:**
- Consumes: `AgentFile`, `Resolved`, `Home`, `project::*`, `RepoTip`, `Repo::file_at_branch`.
- Produces:
  - `Repo::exclude_paths(&self, patterns: &[&str]) -> Result<()>` (appends missing lines to `<common git dir>/info/exclude`, creating the file and folder, idempotent) and `Repo::is_tracked_at_branch(&self, branch: &str, rel: &str) -> Result<bool>`.
  - `Engine::with_agent_files(self, files: Vec<AgentFile>) -> Engine`, `Driver::with_agent_files(self, files: Vec<AgentFile>) -> Driver`. With no files (the default) nothing changes.
  - `factory project add <path> --preset <name> [--base <branch>] [--name <name>]`, `list`, `show <name>`, `remove <name> [--force]`, implemented in `crates/factory-cli/src/project.rs` as `pub fn run(home: &Home, cmd: ProjectCmd) -> Result<String, CliError>`.

- [ ] **Step 1: Failing tests for the exclude helper**

`crates/factory-git/tests/exclude.rs` (use the common fixture): `exclude_paths(&["/.claude/agents/factory-*.md"])` creates `.git/info/exclude` with the line; calling it twice leaves one line; an existing exclude file keeps its content; a file matching the pattern is not reported as untracked by git2 `statuses` in the main checkout and in a linked worktree created with `create_worktree`; `is_tracked_at_branch` is true for a committed file and false for an absent or uncommitted one.
Run: `cargo test -j4 -p factory-git --test exclude` — Expected: FAIL to compile. Then implement with git2 (`self.inner.path().join("info/exclude")` for the main checkout's git dir; the `Repo` always points at the main checkout). Run again: PASS.

- [ ] **Step 2: Failing tests for agent files in the engine**

`crates/factory-engine/tests/agent_files.rs`, built on `tests/common/mod.rs` (read how `build_checks.rs` starts a build with a fake runner): with `Engine::with_agent_files(vec![AgentFile { name: "factory-builder.md".into(), content: "BUILDER".into() }, ...])`:
1. After the first build starts, `<worktree>/.claude/agents/factory-builder.md` exists with that content, before the fake builder runs (assert from inside the fake runner, which records the file text when it is called).
2. The files are git-excluded: `statuses` of the worktree reports none of them, and after the committer stages and commits the job, the commit tree has no `.claude/agents/` entry.
3. A file that is tracked on the base branch is not overwritten: commit `.claude/agents/factory-builder.md` with text `REPO` on the base branch, build, and assert the worktree file still reads `REPO` and the other files are written.
4. With no agent files, no `.claude/agents` folder is created in the worktree.
5. A second build step in the same worktree (a later task) rewrites nothing and does not fail when the files already exist.
Run: `cargo test -j4 -p factory-engine --test agent_files` — Expected: FAIL to compile.

- [ ] **Step 3: Implement agent files**

In `Engine` add `agent_files: Vec<AgentFile>` (default empty) and the builder `with_agent_files`. In `build.rs` `start_worktree`, right after `create_worktree` returns and before the builder runs (about line 93), call a new private `write_agent_files(&wt_path)`: if the list is empty return; call `self.repo.exclude_paths(&["/.claude/agents/factory-*.md"])`; create `<wt>/.claude/agents/`; for each file whose name starts with `factory-` and ends with `.md`, skip it when `self.repo.is_tracked_at_branch(self.repo.base_branch(), ".claude/agents/<name>")` is true, otherwise write it. Files with other names cannot be covered by the exclude line, so refuse them: return an engine error naming the file (the resolver can return such names from an override). Add `Driver::with_agent_files` that stores the list and passes it to each engine it builds (`with_engine` in `compat.rs`).
Run the new test: PASS. Run `cargo test -j4 -p factory-engine` (everything).

- [ ] **Step 4: Failing tests for the project commands**

`crates/factory-cli/tests/project_cmd.rs`: build the commands by `Cli::try_parse_from` and run `factory_cli::project::run(&Home::at(tmp), cmd)` with a temp home and temp git repos (made with `git2`), no `CliCtx`:
- `project add <repo> --preset rust --name demo --base main` prints the project folder and `list` shows `demo rust main <repo>`; the home has `presets/rust/config.toml`.
- Name defaults to the repo folder name lowercased: `/tmp/x/My Repo` is refused with a message that suggests `--name`.
- `add` refuses a non-git directory, an unknown preset, a duplicate name and a duplicate repo, with the registry's message in the text.
- `list` with a project whose repo folder was deleted shows `demo  disabled: repo path is missing` and still shows the others.
- `show demo` prints the repo, preset, base branch, the folders (state, worktrees), the number of risk paths, the agent file names and any problems; with a base branch that does not exist in the repo it prints the problem and says `degraded`.
- `remove demo` deletes the project folder and keeps the repo and the presets; with a job that is not merged or cancelled in `state/board.json` it refuses unless `--force`.
- `Cli::try_parse_from(["factory","project","add"])` is a `MissingRequiredArgument` error.
Run: `cargo test -j4 -p factory-cli --test project_cmd` — Expected: FAIL to compile.

- [ ] **Step 5: Implement the commands**

In `cli.rs` add `Project { #[command(subcommand)] cmd: ProjectCmd }` to `Command` and a `ProjectCmd` enum with `Add { path: PathBuf, #[arg(long)] preset: String, #[arg(long)] base: Option<String>, #[arg(long)] name: Option<String> }`, `List`, `Show { name: String }`, `Remove { name: String, #[arg(long)] force: bool }`. In `project.rs` implement `run` over `factory_core::project` and `resolve`:
- `add` passes `repo_ok` = a closure that calls `factory_git::Repo::open(path)` and checks that the base branch exists locally. The base is `--base`, or else the repo's current branch name (read through git2 `head().shorthand()`).
- `list` and `show` call `resolve` with `RepoTip(&Repo::open(repo)?.with_base_branch(&base))`; a repo that cannot be opened is shown disabled with the error text.
- `remove` loads `state/board.json` (`Board::load`; a missing file is fine) and refuses when any job is not `merged` and not cancelled, unless `--force`.
In `main.rs`, handle `Command::Project` before `CliCtx::from_env`: build `Home::from_env()`, call `project::run`, print, exit 0 or 1 like the other commands. In `dispatch`, the `Project` arm returns `CliError::Refused("project commands do not need a repository, run them through project::run".into())` so the match stays exhaustive. Export `project` from `lib.rs`.
Run the new test: PASS.

- [ ] **Step 6: Document, run everything, commit**

In `README.md` add a section "Projects and presets": the home layout, `FACTORY_HOME`, the four `project` commands with an example, the three layers and what a repo override may contain (`risk_paths`, `checks` only, additive; `.factory/**` is always a risk path; read from the base branch tip), the four prompt keys, and where agent files come from.
Run one at a time: `cargo test -j4 -p factory-core`, `-p factory-git`, `-p factory-agent`, `-p factory-checks`, `-p factory-engine`, `-p factory-cli`, and `cargo check -j4 --manifest-path dashboard/Cargo.toml`.
Commit via the commit skill: `feat(git): exclude paths and test tracked files`, `feat(engine): write agent files into job worktrees`, `feat(cli): add factory project commands`, plus `docs: document projects and presets` if the README change is separate.
