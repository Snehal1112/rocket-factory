# Act On Review Findings, Plan 06: Check Coverage

> **Executor:** run with superpowers:subagent-driven-development: one fresh implementer per task, then a reviewer per task.

**Goal:** Every changed code path is covered by a check or the job parks, `{crate}` follows real Cargo packages, and a project can test the direct dependents of a change.

**Architecture:** `factory-checks` gains a workspace map read from `cargo metadata --no-deps`, which `plan_checks` uses to expand `{crate}` by package and the new `{dependents}`. The engine's no-checks gate counts uncovered code paths instead of looking at an empty report, and stops skipping risky diffs.

**Tech Stack:** Rust, serde_json (already a dependency of the workspace), `cargo metadata` output.

**Spec:** `docs/superpowers/specs/2026-10-29-act-on-review-findings-design.md` (section 11)

**Depends on:** plan 01 (`final_gate` runs the gate before a risky park), plan 04 (gate parks carry evidence).

## Global Constraints

- Cargo per crate with `-j4`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Never shell out to git. Running `cargo metadata` is allowed.
- A failed `cargo metadata` never fails the checks: `{crate}` falls back to today's rule and `checks.md` says so.
- `dependents_cap` defaults to 6; `unchecked_paths` defaults to empty.
- Doc paths are never counted as uncovered.

## Review Focus

1. A repository with no `Cargo.toml` (a node or go project) plans the same commands as before (`no_cargo_falls_back`, Task 1).
2. A changed path at the repo root (`Cargo.lock`) matched by a `{crate}` rule expands to no command, not to an empty `-p` (`root_path_gives_no_crate_command`, Task 1).
3. A risky diff whose only code path has no rule parks `NoChecks` (`risky_diff_with_an_unchecked_path_parks`, Task 2).
4. A path listed in `unchecked_paths` does not park (`unchecked_paths_are_exempt`, Task 2).
5. A diff with one covered and one uncovered code path parks and names only the uncovered one (`partial_coverage_names_the_gap`, Task 2).

## File Structure

- Create `crates/factory-checks/src/workspace.rs`: the workspace map.
- Modify `crates/factory-checks/src/plan.rs` (`plan_checks`), `crates/factory-checks/src/lib.rs`.
- Modify `crates/factory-core/src/config.rs`, `crates/factory-core/src/settings_block.rs`: `dependents_cap`, `unchecked_paths`.
- Modify `crates/factory-core/src/gate.rs` or `crates/factory-engine/src/checks_stage.rs`: the uncovered-path gate.
- Docs: `docs/reference/project-toml-reference.md`, `docs/reference/software-factory.md` (checks section).

---

### Task 1: Workspace map, package-aware `{crate}` and `{dependents}`

**Files:**
- Create: `crates/factory-checks/src/workspace.rs`
- Modify: `crates/factory-checks/src/plan.rs`, `crates/factory-checks/src/lib.rs`, the engine caller of `plan_checks`
- Modify: `crates/factory-core/src/config.rs`, `crates/factory-core/src/settings_block.rs`
- Test: `crates/factory-checks/src/workspace.rs` tests, `crates/factory-checks/src/plan.rs` tests

**Interfaces:**
- Produces:
  - `#[derive(Debug, Clone, PartialEq, Eq)] pub struct Package { pub name: String, pub dir: String, pub deps: Vec<String> }` (`dir` relative to the repo root with `/`, empty for a root package; `deps` are names of workspace members it depends on by path).
  - `#[derive(Debug, Clone, Default, PartialEq, Eq)] pub struct Workspace { pub packages: Vec<Package> }`, with `pub fn from_metadata(json: &str, root: &Path) -> Result<Workspace, CheckError>` (pure, for tests), `pub fn load(root: &Path) -> Result<Workspace, CheckError>` (runs `cargo metadata --no-deps --format-version 1` in `root`), `pub fn owner(&self, path: &str) -> Option<&str>` (longest `dir` prefix; a root package owns only paths under no other package) and `pub fn dependents(&self, names: &BTreeSet<String>) -> Vec<String>` (direct dependents not in `names`, sorted).
  - `Config.dependents_cap: u32` (default 6) and `Config.unchecked_paths: Vec<String>` (default empty), both listed in `settings_block`.
  - `pub fn plan_checks_in(changed_paths: &[String], cfg: &Config, ws: Option<&Workspace>) -> CheckPlan`: `{crate}` from `ws.owner` when `ws` is `Some`, else today's `crate_of`; `{dependents}` from `ws.dependents` capped at `cfg.dependents_cap`. `CheckPlan` gains `pub notes: Vec<String>` for `SKIP dependents over the cap: ...` and `NOTE workspace map unavailable: ...`, which the engine writes into `checks.md`. `plan_checks(paths, cfg)` stays as `plan_checks_in(paths, cfg, None)`.
  - The engine loads the workspace only when a rule's commands contain `{crate}` or `{dependents}` and the worktree has a `Cargo.toml`.

- [ ] **Step 1: Write the failing tests**

In `workspace.rs`, with a `cargo metadata` JSON literal of three packages (`core` in `crates/core`, `app` in `crates/app` depending on `core`, `rocket` in `src-tauri` depending on `app` and `core`):
- `owner_follows_the_longest_prefix`: `src-tauri/src/lib.rs` gives `rocket`; `crates/app/src/x.rs` gives `app`; `README.md` gives `None`.
- `dependents_are_direct_only`: dependents of `{core}` are `["app", "rocket"]`; of `{app}` are `["rocket"]`.

In `plan.rs`:
- `crate_follows_the_package`: rule `src-tauri/**` with `cargo test -j4 -p {crate}` gives `cargo test -j4 -p rocket`.
- `dependents_expand_and_cap`: with `dependents_cap = 1`, `cargo test -j4 -p {dependents}` for a change in `crates/core` gives one command for `app` and a note `SKIP dependents over the cap: rocket`.
- `root_path_gives_no_crate_command`: `Cargo.lock` matched by a `{crate}` rule adds no command.
- `no_cargo_falls_back`: `ws = None` gives today's commands.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -j4 -p factory-checks`
Expected: FAIL.

- [ ] **Step 3: Implement the map, the expansion and the engine wiring**

- [ ] **Step 4: Run the tests**

Run: `cargo test -j4 -p factory-checks`, `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-engine`
Expected: PASS.

---

### Task 2: The uncovered-path gate

**Files:**
- Modify: `crates/factory-engine/src/checks_stage.rs` (`no_checks_reason`, `gate_checks`), `crates/factory-core/src/gate.rs` if the helper fits there
- Modify: `docs/reference/project-toml-reference.md`, `docs/reference/software-factory.md`
- Test: `crates/factory-engine/src/checks_stage.rs` tests, `crates/factory-engine/tests/` gate tests

**Interfaces:**
- Consumes: `code_paths_of` (`factory-core/src/gate.rs:37`), `CheckRule::matches`, `Config.unchecked_paths` (Task 1).
- Produces:
  - `pub fn uncovered_code_paths(paths: &[String], config: &Config) -> Vec<String>`: `code_paths_of(paths)` minus paths matched by any `[[checks]]` rule, minus paths matched by `unchecked_paths`.
  - `no_checks_reason(paths, config) -> Option<String>`: `Some("no check covers N changed code path(s): a, b (+k more)")` when `uncovered_code_paths` is not empty, for risky and normal diffs alike. The `report_is_empty` parameter goes; callers are updated.

- [ ] **Step 1: Update and add the tests**

Rename and rewrite the reason tests for the new wording (`no_checks_reason_names_five_paths_and_a_count` keeps the five-plus-count shape). Add `partial_coverage_names_the_gap` (one path under a rule, one not: the reason names only the second), `risky_diff_with_an_unchecked_path_parks` (an engine test with a risky fixture), `unchecked_paths_are_exempt`, and `doc_paths_are_never_uncovered`.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -j4 -p factory-engine no_checks partial_coverage risky_diff_with_an_unchecked unchecked_paths doc_paths`
Expected: FAIL.

- [ ] **Step 3: Implement the helper and the new gate**

Update existing engine and CLI fixtures whose diffs now trip the gate by adding a rule for their paths, not by weakening the gate.

- [ ] **Step 4: Update the docs**

`project-toml-reference.md`: `dependents_cap`, `unchecked_paths`, the `{dependents}` placeholder and package-aware `{crate}`. `software-factory.md`: the gate now counts uncovered code paths and covers risky diffs.

- [ ] **Step 5: Run every touched suite and clippy**

Run: `cargo test -j4 -p factory-checks`, `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`, then `cargo clippy -j4 -p factory-checks -p factory-core -p factory-engine --all-targets -- -D warnings`
Expected: all pass, no warnings.

Next plan: docs/superpowers/plans/2026-10-29-act-on-review-findings/plan-07-spec-review.md
