# Codex Support Audit Plan

> **For agentic workers:** Follow the approved design and use `superpowers:executing-plans` to carry out this plan task by task. Checkboxes track the work.

**Goal:** Verify Rocket Factory's Codex support end to end, then add focused coverage and fix confirmed gaps.

**Architecture:** Treat the existing Codex support design as the contract. First map each acceptance criterion to current deterministic coverage, then run the package and dashboard suites, inspect uncovered paths (including the working-tree Codex skill install and upgrade), and close only evidenced gaps. Finish with an optional real Codex smoke test and a concise evidence report in the design's follow-up section or audit notes.

**Tech Stack:** Rust workspace (`cargo test -j4`), dashboard Rust workspace, Node tests, Codex CLI smoke test.

**Spec:** `docs/superpowers/specs/2026-10-22-codex-support-audit-design.md`; baseline `docs/superpowers/specs/2026-10-21-codex-support-design.md`.

## Global Constraints

- Preserve the current uncommitted work; do not reset or overwrite it.
- Keep Claude as the default and preserve existing Claude behavior.
- Do not require Codex credentials for deterministic tests.
- Keep the real Codex smoke test optional when the CLI or a usable login is unavailable.
- Do not commit, publish, or deploy.
- Use Cargo with `-j4`, no `--workspace`, and no `CARGO_TARGET_DIR`; the dashboard is a separate workspace.

## Review Focus

- Codex JSONL errors, missing completion, and nonzero process exits must produce failed runs without losing useful diagnostics; cover in Task 1.
- Sandbox roots and API-key filtering must not expose the job folder or accidentally select API billing; cover in Task 2.
- Provider changes must clear incompatible saved session ids and preserve old board and usage compatibility; cover in Task 2.
- Installation and upgrade must handle existing or differing Claude and Codex files safely, with `--dir` behavior matching docs; cover in Task 3.
- Token and cost reports must group and label providers consistently, including old records without provider; cover in Task 3.

---

### Task 1: Verify Codex runner and engine lifecycle

**Files:**
- Inspect: `crates/factory-agent/src/codex.rs`, `crates/factory-agent/tests/codex_runner.rs`, `crates/factory-agent/tests/codex_smoke.rs`
- Inspect: `crates/factory-engine/src/codex_prep.rs`, `crates/factory-engine/tests/codex_prep.rs`, and engine run-loop, commit, and carry tests
- Modify only if a confirmed gap is found: the owning source and test files above

**Interfaces:**
- Consumes: the existing `AgentRunner`, `AgentSpec`, `AgentRun`, and `Config::agent_cli()` interfaces.
- Produces: passing deterministic coverage for runner arguments and stream parsing, engine preparation, session switching, budget behavior, and commit fallback.

- [x] **Step 1: Map the design acceptance criteria to existing named tests.** Record criteria with no direct deterministic test.
- [x] **Step 2: Run the focused suites.** Run `cargo test -j4 -p factory-agent` and `cargo test -j4 -p factory-engine`; record failures without changing unrelated baseline behavior.
- [x] **Step 3: Close confirmed coverage or behavior gaps.** For each untested or failing criterion, add a focused regression test first, confirm it fails when applicable, then implement the smallest fix.
- [x] **Step 4: Re-run the factory-agent package suite and Codex engine integration suite.** Run `cargo test -j4 -p factory-agent` and `cargo test -j4 -p factory-engine --test codex_prep`; expected: all pass, including existing Claude runner tests. The full engine suite was also run and its unrelated `/tmp` versus `/private/tmp` golden failures are recorded in the audit ledger.

### Task 2: Verify configuration, sandbox, compatibility, and provider selection

**Files:**
- Inspect: `crates/factory-core/src/config.rs`, `project.rs`, `board.rs`, `cost.rs`, `crates/factory-cli/src/project.rs`, `commands.rs`, and `crates/factory-agent/src/codex.rs`
- Modify only if a confirmed gap is found: the owning source and test files

**Interfaces:**
- Consumes: project setting `agent_cli`, optional job provider, Codex sandbox configuration, and API-key guard described in the baseline design.
- Produces: evidence that `claude` remains the default, Codex setup validates correctly, and old persisted data remains readable.

- [x] **Step 1: Map configuration, compatibility, and security criteria to tests.** Include invalid config, missing executable, model aliases, old board/usage records, writable roots, network policy, and API-key removal.
- [x] **Step 2: Run `cargo test -j4 -p factory-core` and `cargo test -j4 -p factory-cli`.** Note any failures and criteria without coverage.
- [x] **Step 3: Add regression tests and fix demonstrated gaps.** Keep tests deterministic and use fake executables or temporary directories where possible.
- [x] **Step 4: Re-run core and CLI tests.** Expected: all tests pass with unchanged Claude defaults.

### Task 3: Verify install, upgrade, reports, and dashboard

**Files:**
- Inspect: `crates/factory-cli/src/install.rs`, `upgrade.rs`, `tokens.rs`, `costs.rs`, `crates/factory-cli/tests/global_command.rs`, `upgrade.rs`, `tokens_cache.rs`, `dashboard/src/model.rs`, `dashboard/src/costs.rs`, `dashboard/static/board.js`, and dashboard tests
- Modify only if a confirmed gap is found: the owning source, test, and documentation files
- Review the existing uncommitted changes in README and CLI/core files without discarding them

**Interfaces:**
- Consumes: Codex install location `~/.agents/skills/factory/SKILL.md`, `factory install --dir` behavior, provider-tagged usage records, and dashboard provider fields.
- Produces: covered and documented install/upgrade safety, provider report output, and dashboard rendering.

- [x] **Step 1: Map these acceptance criteria to current tests.** Cover identical/different target files, force and dry-run, explicit `--dir`, absent provider on legacy records, and dashboard cards.
- [x] **Step 2: Run the CLI-focused tests and dashboard suites.** Run `cargo test -j4 -p factory-cli`, `cargo test -j4 --manifest-path dashboard/Cargo.toml`, and `node --test` from `dashboard`; capture output.
- [x] **Step 3: Add regression coverage and fix demonstrated gaps.** Preserve user edits and update docs only to match verified behavior.
- [x] **Step 4: Re-run the affected suites.** Expected: all relevant suites pass.

### Task 4: Run end-to-end smoke checks and report evidence

**Files:**
- Inspect: `crates/factory-agent/tests/codex_smoke.rs`, `docs/software-factory.md`, and the two Codex design/audit documents
- Modify: audit evidence in `docs/superpowers/specs/2026-10-22-codex-support-audit-design.md` only if useful; documentation fixes only for verified mismatches

**Interfaces:**
- Consumes: passing deterministic tests from Tasks 1–3.
- Produces: recorded outcome of the real Codex smoke test and explicit distinction between verified behavior and environment-limited behavior.

- [x] **Step 1: Check whether `codex` is on PATH and whether the ignored smoke test can run.** Do not expose credentials in output.
- [x] **Step 2: If usable, run `cargo test -j4 -p factory-agent real_codex_smoke -- --ignored`; otherwise record why it could not run.** Expected: the test passes or the environment limitation is documented.
- [x] **Step 3: Review the full acceptance matrix against test output and inspect the final diff.** Remove no user changes; correct only issues introduced by this audit.
- [x] **Step 4: Report findings, fixes, exact verification commands, and any remaining limits.**
