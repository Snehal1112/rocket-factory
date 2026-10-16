# Extract into rocket-factory Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the software factory out of Rocket into the new repo `~/data/rocket/rocket-factory` with its git history, and make every existing test pass there without Rocket's files.

**Architecture:** `git filter-repo` runs in a throwaway clone of Rocket and keeps only `tools/factory/` and `tools/factory-server/`. The first becomes the repo root (a Cargo workspace of six crates), the second becomes `dashboard/` (its own workspace, unchanged). A few tests read Rocket's repo files by relative path. Those get Rocket's factory files copied in, and the tests that pin Rocket-only state are removed.

**Tech Stack:** Rust 2021 (cargo 1.94), git2, `git filter-repo`, Node 20+ (dashboard unit tests), bash (dashboard smoke test).

**Spec:** `docs/superpowers/specs/2026-10-16-standalone-software-factory-design.md` in the Rocket repo (copied into the new repo in Task 3).

## Global Constraints

- Work in `/home/numericlabs/data/rocket/rocket-factory`. Rocket's checkout is only read, never edited, by this plan.
- Every `cargo` command has `-j4`. Never `--workspace` or `--all`. Never set `CARGO_TARGET_DIR`.
- Do not push anywhere. The new repo has no remote.
- Commits use conventional commits and are written through the `dev-workflow-skills:1-git-commit` skill. Stage explicit paths, never `git add -A`, `--all` or `.`.
- No behavior change in the driver or dashboard. Only paths, docs and Rocket-coupled tests change.
- A missing or broken config must still make every diff risky. Do not touch that rule.

## Review Focus

- History is dropped or reordered by the rewrite: a known file keeps its commits (`git log --oneline -- crates/factory-engine/src/decide.rs` is not empty) and the commit count stays near 105.
- A test passes only because it reads Rocket's files by a relative path that now points nowhere or outside the repo: no `../../../../` remains in the Rust sources.
- The two copied Rocket files that tests read (`config.toml`, `commit-context.md`) drift from Rocket's: they are byte-identical copies at the time of extraction.
- Dashboard scripts assume the old path layout: `smoke.sh` and the node tests run from the new location.
- A test that pinned Rocket's repo state is deleted without checking nothing else needed it: after deletion `cargo check -j4 --tests -p factory-cli` has no unused-function warning.

---

### Task 1: Extract with history

**Files:**
- Create: everything in `/home/numericlabs/data/rocket/rocket-factory` (through git history, no hand-written files)
- Scratch: `/home/numericlabs/data/rocket/.rocket-factory-extract` (deleted at the end)

**Interfaces:**
- Consumes: Rocket's local `main` branch, including its unpushed commits.
- Produces: in `rocket-factory`, branch `main` with root `Cargo.toml` (workspace members `crates/factory-core`, `factory-git`, `factory-agent`, `factory-checks`, `factory-engine`, `factory-cli`), root `CLAUDE.md`, `.gitignore`, `crates/`, and `dashboard/` (crate `factory-server`, its own `[workspace]`). Later tasks rely on exactly these paths.

- [ ] **Step 1: Confirm `git filter-repo` is installed and Rocket is clean enough**

Run:
```bash
git filter-repo --version
git -C /home/numericlabs/data/rocket/rocket status --short tools/factory tools/factory-server
git -C /home/numericlabs/data/rocket/rocket rev-parse main
```
Expected: a version string, no output from `status` (no uncommitted factory changes), and one commit sha. Write the sha down as `ROCKET_MAIN`. If `status` prints anything, stop and tell the user: uncommitted factory changes would be lost.

- [ ] **Step 2: Make a throwaway clone**

Run:
```bash
SCRATCH=/home/numericlabs/data/rocket/.rocket-factory-extract
rm -rf "$SCRATCH"
git clone --no-local /home/numericlabs/data/rocket/rocket "$SCRATCH"
git -C "$SCRATCH" rev-parse HEAD
```
Expected: the printed sha equals `ROCKET_MAIN`. `--no-local` is required, because `git filter-repo` refuses a clone that shares objects with its source.

- [ ] **Step 3: Rewrite the clone so only the factory remains**

Run:
```bash
cd "$SCRATCH"
git filter-repo \
  --path tools/factory/ \
  --path tools/factory-server/ \
  --path-rename tools/factory/: \
  --path-rename tools/factory-server/:dashboard/
```
Expected: output ending with `New history written` and `Completely finished`.

- [ ] **Step 4: Check the layout and the history**

Run:
```bash
cd "$SCRATCH"
ls -a
test ! -e tools && echo "no tools dir"
git log --oneline | wc -l
git log --oneline -- crates/factory-engine/src/decide.rs | wc -l
git log --oneline -- dashboard/static/app.js | wc -l
git log -1 --format='%an %s'
```
Expected: `ls` shows `.gitignore`, `CLAUDE.md`, `Cargo.lock`, `Cargo.toml`, `crates`, `dashboard` (and `.git`); `no tools dir`; a commit count between 90 and 110; the two per-file counts are both at least 1; the last line shows the original author and a factory commit subject. If the count is below 90 or a per-file count is 0, stop: the rewrite lost history.

- [ ] **Step 5: Bring the history into rocket-factory**

Run:
```bash
cd /home/numericlabs/data/rocket/rocket-factory
git fetch "$SCRATCH" main
git checkout -B main FETCH_HEAD
git log --oneline | wc -l
git status --short
```
Expected: the same commit count as Step 4, and an empty `git status`. `rocket-factory` must have no remote: `git remote -v` prints nothing.

- [ ] **Step 6: Confirm both workspaces still compile**

Run:
```bash
cd /home/numericlabs/data/rocket/rocket-factory
cargo check -j4
cargo check -j4 --manifest-path dashboard/Cargo.toml
```
Expected: both end with `Finished`. The first build compiles libgit2 and takes several minutes. If it fails with a missing path dependency, the crate paths changed: stop and report the error.

- [ ] **Step 7: Delete the scratch clone and confirm Rocket is untouched**

Run:
```bash
rm -rf /home/numericlabs/data/rocket/.rocket-factory-extract
git -C /home/numericlabs/data/rocket/rocket rev-parse main
git -C /home/numericlabs/data/rocket/rocket status --short | head
```
Expected: the sha equals `ROCKET_MAIN`. `status` shows only files that were already untracked before this plan (the manual-tests files), nothing under `tools/`.

- [ ] **Step 8: No commit in this task**

The imported history is the commit. Run `git -C /home/numericlabs/data/rocket/rocket-factory log -1 --format='%h %s'` and confirm it prints the last factory commit from Rocket, not a new commit.

---

### Task 2: Make the Rocket-coupled tests stand on their own

**Files:**
- Create: `.claude/agents/factory-builder.md`, `.claude/agents/factory-code-reviewer.md`, `.claude/agents/factory-committer.md`, `.claude/agents/factory-kb-refresher.md`, `.claude/commands/factory.md`, `.claude/software-factory.md`, `.claude/factory/config.toml`, `.claude/factory/commit-context.md` (byte copies from Rocket)
- Modify: `crates/factory-core/src/config.rs` (the `shipped_config_loads_and_maps_the_expected_paths` test), `crates/factory-engine/src/commit.rs` (the `shipped_context_file_has_the_style_and_the_standing_answers` test), `crates/factory-cli/tests/agent_lint.rs`, `CLAUDE.md`

**Interfaces:**
- Consumes: the layout produced by Task 1.
- Produces: a repo where `cargo test -j4 -p factory-core`, `-p factory-engine` and `-p factory-cli` find every file they read. The copied `.claude/` files are Rocket's and are replaced by presets in plan 3 and by the global command in plan 7.

- [ ] **Step 1: Run the three suites and record the failures**

Run:
```bash
cd /home/numericlabs/data/rocket/rocket-factory
cargo test -j4 -p factory-core 2>&1 | tail -25
cargo test -j4 -p factory-engine shipped_context 2>&1 | tail -15
cargo test -j4 -p factory-cli --test agent_lint 2>&1 | tail -40
```
Expected: FAIL. `shipped_config_loads_and_maps_the_expected_paths` and `shipped_context_file_has_the_style_and_the_standing_answers` fail on a missing file, and most `agent_lint` tests fail with `repo root with .claude/agents not found`. This shows the tests are the ones that depend on Rocket's files.

- [ ] **Step 2: Copy Rocket's factory files in, byte for byte**

Run:
```bash
R=/home/numericlabs/data/rocket/rocket
D=/home/numericlabs/data/rocket/rocket-factory
mkdir -p "$D/.claude/agents" "$D/.claude/commands" "$D/.claude/factory"
cp "$R"/.claude/agents/factory-builder.md "$R"/.claude/agents/factory-code-reviewer.md "$R"/.claude/agents/factory-committer.md "$R"/.claude/agents/factory-kb-refresher.md "$D/.claude/agents/"
cp "$R/.claude/commands/factory.md" "$D/.claude/commands/"
cp "$R/.claude/software-factory.md" "$D/.claude/"
cp "$R/.claude/factory/config.toml" "$R/.claude/factory/commit-context.md" "$D/.claude/factory/"
cd "$D" && ls .claude/agents
```
Expected: the `ls` lists exactly the four `factory-*.md` files. (`only_the_known_factory_agents_remain` needs exactly these four.) Check the copies match: `cmp "$R/.claude/factory/config.toml" "$D/.claude/factory/config.toml"` prints nothing.

- [ ] **Step 3: Fix the two unit tests that climb four folders**

In `crates/factory-core/src/config.rs`, in the test `shipped_config_loads_and_maps_the_expected_paths`, replace

```rust
let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../../.claude/factory/config.toml");
```
with
```rust
let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../.claude/factory/config.toml");
```

In `crates/factory-engine/src/commit.rs`, in the test `shipped_context_file_has_the_style_and_the_standing_answers`, replace

```rust
let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../../.claude/factory/commit-context.md");
```
with
```rust
let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../.claude/factory/commit-context.md");
```

The crates now sit at `crates/<name>/`, so two folders up is the repo root.

- [ ] **Step 4: Trim `agent_lint.rs` to what this repo can pin**

In `crates/factory-cli/tests/agent_lint.rs`:

1. In `docs_describe_the_new_design`, replace
```rust
    require_all(&root_doc, "CLAUDE.md", &[".claude/software-factory.md", "tools/factory/"])?;
```
with
```rust
    require_all(&root_doc, "CLAUDE.md", &[".claude/software-factory.md"])?;
```
2. Delete everything after the closing brace of `docs_describe_the_new_design`. That removes `v2_is_marked_superseded`, `path_exists`, `old_python_helper_and_hook_are_gone`, `settings_have_no_factory_hooks` and `settings_allow_the_factory_binary`. They pin Rocket's own docs, hooks and `settings.json`, which do not exist here.

- [ ] **Step 5: Update `CLAUDE.md` for the new home**

Apply these four replacements in `CLAUDE.md`:

1. Replace
```
This is the software factory driver, a Rust workspace of its own (not part of the root Cargo workspace, not shipped with the app). Read `../../.claude/software-factory.md` first for the pipeline, job model, merge rules and dashboard. Do not repeat it here.
```
with
```
This is rocket-factory, the standalone software factory: a Rust driver workspace at the repo root and a local dashboard in `dashboard/` (its own workspace). It was extracted from Rocket's `tools/factory` and `tools/factory-server` with history. Read `.claude/software-factory.md` first for the pipeline, job model, merge rules and dashboard. Do not repeat it here. The design for the standalone version is `docs/superpowers/specs/2026-10-16-standalone-software-factory-design.md`.
```
2. Replace
```
Run from `tools/factory/`. Harness rules still apply: always `-j4`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
```
with
```
Run from the repo root. Always `-j4`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`. Rocket's hooks enforced these, and this repo has no hooks yet, so follow them by hand.
```
3. Replace `(`../factory-server`, separate workspace)` with `(`dashboard/`, separate workspace)`.
4. Append at the end of the file:
```

## Carried over from Rocket

These files in `.claude/` are copies of Rocket's factory files. Tests read them. They are Rocket-specific until later plans replace them: `agents/factory-*.md` and `factory/config.toml` become presets (plan 3), `commands/factory.md` becomes a global command (plan 7), and `software-factory.md` is rewritten for the standalone design.
```

- [ ] **Step 6: Run the suites and check for warnings**

Run:
```bash
cd /home/numericlabs/data/rocket/rocket-factory
cargo test -j4 -p factory-core 2>&1 | tail -8
cargo test -j4 -p factory-engine 2>&1 | tail -8
cargo test -j4 -p factory-cli --test agent_lint 2>&1 | tail -20
cargo check -j4 --tests -p factory-cli 2>&1 | grep -E "warning|error" || echo "no warnings"
grep -rn "\.\./\.\./\.\./\.\." crates --include='*.rs' || echo "no four-up paths"
```
Expected: every `test result:` line says `ok` with `0 failed`, `no warnings`, and `no four-up paths`.

- [ ] **Step 7: Commit**

Stage only these paths, then run the `dev-workflow-skills:1-git-commit` skill:
```bash
cd /home/numericlabs/data/rocket/rocket-factory
git add .claude CLAUDE.md crates/factory-core/src/config.rs crates/factory-engine/src/commit.rs crates/factory-cli/tests/agent_lint.rs
git status --short
```
Expected `git status`: only those paths. Suggested message: `test: make extracted tests repo-independent`.

---

### Task 3: Verify the whole extraction, tidy the repo, bring the docs over

**Files:**
- Modify: `.gitignore`, `dashboard/README.md`
- Create: `README.md`, `docs/superpowers/specs/2026-10-16-standalone-software-factory-design.md`, `docs/superpowers/plans/2026-10-16-standalone-software-factory/00-plan-index.md`, `docs/superpowers/plans/2026-10-16-standalone-software-factory/plan-01-extract-into-rocket-factory.md`

**Interfaces:**
- Consumes: Tasks 1 and 2.
- Produces: a repo ready for plan 2: all tests green, ignore rules for factory state, and the spec and plans inside it.

- [ ] **Step 1: Run every crate's tests, one crate at a time**

Run:
```bash
cd /home/numericlabs/data/rocket/rocket-factory
for c in factory-core factory-git factory-agent factory-checks factory-engine factory-cli; do
  echo "== $c"; cargo test -j4 -p $c 2>&1 | grep -E "^test result|FAILED|panicked|error" ;
done
```
Expected: every crate prints only `test result: ok` lines. If a test fails because of a path or a missing Rocket file, fix that test the way Task 2 did and note it in the commit body. Do not skip or delete a failing test without telling the user.

- [ ] **Step 2: Run the dashboard checks from their new location**

Run:
```bash
cd /home/numericlabs/data/rocket/rocket-factory
cargo test -j4 --manifest-path dashboard/Cargo.toml 2>&1 | grep -E "^test result|FAILED|error"
node --test dashboard/tests/board.test.js dashboard/tests/patch.test.js 2>&1 | tail -8
bash dashboard/tests/smoke.sh 2>&1 | tail -15
```
Expected: `test result: ok`, the node run ends with `# fail 0`, and `smoke.sh` prints `PASS:` lines and no `FAIL:` line. If `node` is missing, install Node 20+ and rerun.

- [ ] **Step 3: Fix the dashboard README run command**

In `dashboard/README.md`, replace the cargo run command

```bash
cargo run -j4 --manifest-path tools/factory-server/Cargo.toml -- --root "$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")" --port 8765
```
with
```bash
cargo run -j4 --manifest-path dashboard/Cargo.toml -- --root <project checkout> --port 8765
```
and replace the line `node --test tools/factory-server/tests/board.test.js tools/factory-server/tests/patch.test.js` with `node --test dashboard/tests/board.test.js dashboard/tests/patch.test.js`. Leave the dashboard title "for Rocket" alone; plan 6 rewrites the dashboard.

- [ ] **Step 4: Set the ignore rules**

Write `.gitignore` as:
```
target/
node_modules/
.superpowers/
.claude/worktrees/
```
The factory writes `.superpowers/` (board, events, job folders) and `.claude/worktrees/` in whatever repo it works on, and this repo is such a repo from plan 4 on.

- [ ] **Step 5: Copy the spec and plans in and add a README**

Run:
```bash
R=/home/numericlabs/data/rocket/rocket
D=/home/numericlabs/data/rocket/rocket-factory
mkdir -p "$D/docs/superpowers/specs" "$D/docs/superpowers/plans/2026-10-16-standalone-software-factory"
cp "$R/docs/superpowers/specs/2026-10-16-standalone-software-factory-design.md" "$D/docs/superpowers/specs/"
cp "$R"/docs/superpowers/plans/2026-10-16-standalone-software-factory/*.md "$D/docs/superpowers/plans/2026-10-16-standalone-software-factory/"
```
Then create `README.md`:
```markdown
# rocket-factory

A standalone software factory. A Rust driver builds, checks, reviews and merges one feature at a time in a separate git worktree, and a local dashboard shows what it is doing. It was extracted from Rocket's `tools/factory` and is being generalized to drive several projects at once.

- Design: `docs/superpowers/specs/2026-10-16-standalone-software-factory-design.md`
- Build plans: `docs/superpowers/plans/2026-10-16-standalone-software-factory/`
- Contributor rules and commands: `CLAUDE.md`

Status: extracted and tested. It still behaves like Rocket's in-repo factory. The standalone work is described in the plan index.
```

- [ ] **Step 6: Final checks**

Run:
```bash
cd /home/numericlabs/data/rocket/rocket-factory
git status --short
git ls-files | grep -c "^tools/" || true
git log --oneline -- crates/factory-engine/src/decide.rs | wc -l
cargo check -j4 && cargo check -j4 --manifest-path dashboard/Cargo.toml
```
Expected: `status` lists only the files from Steps 3 to 5, the `tools/` count is `0`, the per-file history count is at least 1, and both checks end with `Finished`.

- [ ] **Step 7: Commit**

Stage explicit paths and run the `dev-workflow-skills:1-git-commit` skill. Two logical groups are acceptable: repo setup (`.gitignore`, `README.md`, `dashboard/README.md`) and docs (the spec and plans):
```bash
git add .gitignore README.md dashboard/README.md
git add docs/superpowers/specs/2026-10-16-standalone-software-factory-design.md docs/superpowers/plans/2026-10-16-standalone-software-factory
git status --short
```
Suggested messages: `chore: add ignore rules, readme and new dashboard paths` and `docs: add standalone factory spec and plans`.
