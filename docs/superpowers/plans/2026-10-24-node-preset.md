# Node preset Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `factory project add <path> --preset node` registers a Node.js or React project with Bun-driven, script-driven checks.

**Architecture:** One new built-in preset, `node`, shaped like the go and csharp presets: a `config.toml`, a `commit-context.md` and four agent files under `crates/factory-core/presets/node/`, embedded in `factory-core/src/preset.rs`. React is covered by path rules and a short rules block, not a second preset. No engine change.

**Tech Stack:** Rust (`factory-core`, `factory-cli` tests), Markdown and TOML preset files, Bun as the package manager in the preset text.

**Spec:** The design approved in the brainstorming chat on 2026-10-24 (no separate spec file). Its decisions are copied under Global Constraints.

## Global Constraints

- Bun only: no npm, yarn or pnpm detection, no `npm`/`yarn`/`pnpm` in any preset text.
- One preset named `node`. No `react` preset.
- Checks are script-driven and exactly these, in this order: `bun run typecheck`, `bun run lint`, `bun test`.
- Check paths: `**/*.ts`, `**/*.tsx`, `**/*.js`, `**/*.jsx` (separate entries, no brace globs, as the csharp preset does).
- Risk paths: `package.json`, `**/package.json`, `tsconfig*.json`, `**/tsconfig*.json`, `vite.config.*`, `**/vite.config.*`, `next.config.*`, `**/next.config.*`, `.env*`, `**/.env*`, `**/*auth*`, `**/*secret*`, `**/*credential*`, `**/Dockerfile`, `.github/**`. Lockfiles (`bun.lock`, `bun.lockb`) are NOT risk paths.
- Builder rules say: use `bun install --frozen-lockfile` and the project's own `package.json` scripts; add the standard `typecheck`, `lint` and `test` scripts if the project lacks them.
- A short React block in `builder_rules` and `reviewer_rules`: hooks rules, no `any`, accessible markup, tests for behaviour not implementation.
- No engine change. No skipping of missing scripts. No `package.json` script inspection.
- A missing or broken repo config must still make every diff risky. Do not touch that rule.
- Run cargo from the repo root, always `-j4`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Code comments are short full sentences ending in a punctuation mark.

## Review Focus

- A `.tsx` change that touches only a test file: still maps to all three commands (the planner does not special-case tests).
- A changed `bun.lock` alone: not risky, and maps to no check (it matches no check path).
- A nested `apps/web/package.json` or `apps/web/vite.config.ts` in a monorepo: risky, via the `**/` entries.
- `.env.local` and `.env.production`: risky, via `.env*`.
- `tsconfig.base.json` and `tsconfig.app.json`: risky, via `tsconfig*.json`.

---

### Task 1: The node preset files and registration

**Files:**
- Create: `crates/factory-core/presets/node/config.toml`
- Create: `crates/factory-core/presets/node/commit-context.md` (byte copy of `presets/go/commit-context.md`)
- Create: `crates/factory-core/presets/node/agents/factory-committer.md` (byte copy of the go one)
- Create: `crates/factory-core/presets/node/agents/factory-kb-refresher.md` (byte copy of the go one)
- Create: `crates/factory-core/presets/node/agents/factory-builder.md` (copy of go; replace line 37, the "This is a Go project..." paragraph)
- Create: `crates/factory-core/presets/node/agents/factory-code-reviewer.md` (copy of go; replace line 14 and the "error handling" bullet on line 25)
- Modify: `crates/factory-core/src/preset.rs:12` (`PRESET_NAMES`), add `static NODE: Preset`, add `"node"` arm in `embedded`
- Test: `crates/factory-core/src/preset.rs` (existing tests iterate `PRESET_NAMES`) and a new test below

**Interfaces:**
- Consumes: `Preset` struct and `embedded(name: &str) -> Option<&'static Preset>` in `preset.rs`.
- Produces: `PRESET_NAMES: [&str; 5] = ["rust", "go", "ruby", "csharp", "node"]` and `embedded("node")`. Later tasks and `project.rs` ("unknown preset ... choose one of rust, go, ruby, csharp, node") rely on the order shown.

- [ ] **Step 1: Write the failing test** in the `tests` module of `preset.rs`:

```rust
#[test]
fn the_node_preset_is_bun_and_script_driven() {
    let p = embedded("node").expect("node preset");
    let cfg = Config::parse(p.config).expect("parses");
    assert!(!cfg.degraded);
    let commands: Vec<&str> = cfg.checks.iter().flat_map(|c| c.commands.iter().map(String::as_str)).collect();
    assert_eq!(commands, ["bun run typecheck", "bun run lint", "bun test"]);
    for path in ["package.json", "apps/web/vite.config.ts", ".env.local", "tsconfig.base.json"] {
        assert!(cfg.is_risky(path), "{path} is risky");
    }
    assert!(!cfg.is_risky("bun.lock"));
    assert!(!cfg.is_risky("bun.lockb"));
    for forbidden in ["npm ", "yarn", "pnpm"] {
        assert!(!p.config.contains(forbidden), "{forbidden} in node config");
    }
}
```

Use the real risk-matching method on `Config`; find its name in `factory-core/src/config.rs` (the same one `factory-checks` and the risk code call) and adjust `is_risky` and the `checks` field access to match.

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -j4 -p factory-core the_node_preset_is_bun_and_script_driven`
Expected: FAIL, "node preset" not found.

- [ ] **Step 3: Create the preset files.** `config.toml` header comment: `# Preset: Node.js or React project (Bun). Edit this file in ~/.factory/presets/node/ to change every project that uses it.` Fill `risk_paths`, `read_first = ["CLAUDE.md"]`, `builder_rules`, `check_hints = ["bun run typecheck", "bun run lint", "bun test"]`, `reviewer_rules` and one `[[checks]]` block from Global Constraints. Builder agent paragraph: "This is a Node.js or React project that uses Bun. Run `bun install --frozen-lockfile` once, then the project's own `typecheck`, `lint` and `test` scripts for what you touch. If a script is missing, add the standard one first. Run them yourself before you finish, so the driver's checks start warm." Reviewer paragraph names: `any` types, missing hook dependencies and hooks called conditionally, unhandled promise rejections, inaccessible markup, and tests that assert implementation instead of behaviour; keep the sentence "You cannot run commands, so judge from the diff and the source." The error-handling bullet becomes "no unhandled promise rejections, failures surfaced with context".

- [ ] **Step 4: Register it.** In `preset.rs`: `PRESET_NAMES` gets `"node"` last and length 5; `static NODE: Preset` mirrors `CSHARP` with the six `include_str!` paths under `../presets/node/`; `embedded` gets `"node" => Some(&NODE)`.

- [ ] **Step 5: Run the crate tests**

Run: `cargo test -j4 -p factory-core`
Expected: PASS, including the existing `every_preset_exists_and_parses_as_config` and `presets_ship_the_four_agents_and_name_no_rocket_file`.

- [ ] **Step 6: Commit** with the `dev-workflow-skills:1-git-commit` skill (message type `feat(core)`).

---

### Task 2: Planner test for `.tsx` paths and the lint and refresh tests

**Files:**
- Test: `crates/factory-checks/src/plan.rs` (tests module, next to `rust_preset`)
- Modify: `crates/factory-cli/tests/agent_lint.rs` (the maxTurns list near line 700, `PRESET_BUILDERS` near 793, `MARKED` near 877)
- Modify: `crates/factory-cli/tests/presets_refresh.rs` (lines 77-82 list, the `22 unchanged` counts at lines 36, 87, 146, and the `for name in ["ruby", "csharp"]` loop at line 129)

**Interfaces:**
- Consumes: `factory_core::preset::embedded("node")` from Task 1; `plan_checks(&[String-like paths], &Config)` as used by the existing `rust_preset` tests.
- Produces: none.

- [ ] **Step 1: Write the failing planner test** in `plan.rs`:

```rust
fn node_preset() -> Config {
    let text = factory_core::preset::embedded("node").expect("node preset").config;
    Config::parse(text).expect("preset parses")
}

#[test]
fn the_node_preset_runs_three_scripts_for_a_tsx_change() {
    let plan = plan_checks(&paths(&["src/App.tsx", "src/App.test.tsx"]), &node_preset());
    assert_eq!(plan.commands(), ["bun run typecheck", "bun run lint", "bun test"]);
}

#[test]
fn the_node_preset_runs_nothing_for_a_lockfile() {
    let plan = plan_checks(&paths(&["bun.lock"]), &node_preset());
    assert!(plan.commands().is_empty());
}
```

Adapt `commands()` and the plan field access to what the existing rust tests in that module use.

- [ ] **Step 2: Run, confirm they pass** (Task 1 already supplies the preset):

Run: `cargo test -j4 -p factory-checks node_preset`
Expected: PASS. If the lockfile test fails, the planner has a catch-all; report it, do not change the engine.

- [ ] **Step 3: Extend `agent_lint.rs`.** Add the node builder and reviewer to the maxTurns list, `PRESET_BUILDERS` (length 5) and `MARKED` (length 10), each in the same entry style.

- [ ] **Step 4: Extend `presets_refresh.rs`.** Add the six `node/...` relative paths to the list, change every `22 unchanged` to `28 unchanged` (five presets, 30 files, minus one written and one refused), and add `"node"` to the ruby/csharp loop.

- [ ] **Step 5: Run the affected suites**

Run: `cargo test -j4 -p factory-cli --test agent_lint` then `cargo test -j4 -p factory-cli --test presets_refresh` then `cargo test -j4 -p factory-engine`
Expected: PASS. The engine run covers `permissions.rs`, which iterates `PRESET_NAMES`.

- [ ] **Step 6: Commit** with the `dev-workflow-skills:1-git-commit` skill (message type `test`).

---

### Task 3: Docs

**Files:**
- Modify: `README.md:185`
- Modify: `docs/project-toml-reference.md:15`
- Modify: `docs/getting-started.md` (lines 31, 52, 159, 331, 376)
- Modify: `docs/factory-user-guide.md` (lines 73, 675, 691, 700, 1222)

**Interfaces:**
- Consumes: the preset name `node` and its checks from Task 1.
- Produces: none.

- [ ] **Step 1: Update every preset list** from "rust, go, ruby, csharp" (in all its spellings: `rust|go|ruby|csharp`, "rust, go, ruby or csharp") to include `node`. The `unknown preset` error text at `docs/factory-user-guide.md:691` must read `choose one of rust, go, ruby, csharp, node`.
- [ ] **Step 2: Add the node row** to the preset table at `docs/factory-user-guide.md:700`: risky paths "manifests, TypeScript and bundler config, env files", checks "`bun run typecheck`, `bun run lint` and `bun test`".
- [ ] **Step 3: Add a prerequisite row** to `docs/getting-started.md` near line 52: "`bun` on the machine, only for Node and React projects", why "The `node` preset checks run the project's `typecheck`, `lint` and `test` scripts through Bun", check `bun --version`. Add `--preset node` to the line 159 sentence.
- [ ] **Step 4: Verify no list was missed**

Run: `grep -rn "ruby" README.md docs/*.md | grep -v node`
Expected: no line that lists presets without `node`.

- [ ] **Step 5: Commit** with the `dev-workflow-skills:1-git-commit` skill (message type `docs`).

---

### Task 4: Final check

- [ ] **Step 1: Build and lint**

Run: `cargo check -j4` then `cargo clippy -j4 -p factory-core -p factory-checks -p factory-cli --tests`
Expected: no errors or warnings.

- [ ] **Step 2: Smoke test the preset end to end**

Run: `cargo build -j4 -p factory-cli`, then with a temp `FACTORY_HOME` and a throwaway git repo: `target/debug/factory project add <repo> --preset node --name demo` and `target/debug/factory presets refresh`.
Expected: the project registers, `presets/node/` is seeded, refresh prints `unchanged:` lines for the node files.
