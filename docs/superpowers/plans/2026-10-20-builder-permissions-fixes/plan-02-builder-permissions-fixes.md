# Builder Permissions Fixes, Plan 2: Dashboard label, docs and the spec

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Show the new `builder-tampered` park kind in the dashboard, and make the docs and the spec say what the code now does, including the honest limits.

**Architecture:** One label and one class entry in `dashboard/static/board.js` with its test. The rest is Markdown, guarded by the doc tests in `crates/factory-cli/tests/agent_lint.rs`. No production Rust changes, so no port is involved.

**Tech Stack:** JavaScript (`node --test`), Markdown, Rust doc tests.

**Spec:** docs/superpowers/specs/2026-10-19-builder-permissions-design.md

## Global Constraints

- Always `-j4`; never `--workspace` or `--all`; never set `CARGO_TARGET_DIR`. The dashboard is its own workspace: use `--manifest-path dashboard/Cargo.toml` for cargo and `node --test dashboard/tests/*.test.js` for the JavaScript tests.
- Every statement must match the code after plan 1. Read `crates/factory-core/src/config.rs`, `crates/factory-engine/src/permissions.rs` and `specs.rs` first.
- The probes for `find`, `git diff --output`, `Bash(*)` and `acceptEdits` with `rm` have not been run against the real `claude`. The docs must say so and must not claim the worktree protects a builder from `find` or `--output`. Say instead: the allowlist is not a sandbox, `find` (with `-exec`, `-delete`) and `git diff|log|show --output` can act outside the worktree, and the deny rules and the file guard cover only the job folder files named in the docs and the KB folders.
- The new park kind is a rework-only kind and the dashboard chip class is `chip-changes` (red), like `builder-blocked`.
- Prose stays short and plain. No emojis.

## Review Focus

- `parkKindLabel("builder-tampered")` is `Builder changed a job file` and `parkKindClass("builder-tampered")` is `chip-changes` (Task 1 test, in `dashboard/tests/board.test.js`).
- The docs name the longer never-allowed list, the shell-operator, quote, backslash and wildcard rules, the wrapper programs that are never derived, the KB deny rules and the `builder-tampered` park kind (Task 2 tests).
- The docs state that duplicates are removed, that a compound check command such as `cd web && bun test` derives only `cd`, and that the entry form is `Bash(<command>:*)` where the command may hold arguments, such as `git log`.
- The README no longer says a job never blocks on a prompt, and lists `check_hints` with `checks`.
- The spec's design sections, edge-case table, acceptance criteria and decisions match the code.

---

## File Structure

- `dashboard/static/board.js`, `dashboard/tests/board.test.js` (modify): the label and the test.
- `docs/project-toml-reference.md`, `docs/software-factory.md`, `README.md` (modify).
- `docs/superpowers/specs/2026-10-19-builder-permissions-design.md` (modify).
- `crates/factory-cli/tests/agent_lint.rs` (modify): doc test phrases.

### Task 1: The dashboard label

**Files:**
- Modify: `dashboard/static/board.js`, `dashboard/tests/board.test.js`

**Interfaces:**
- Produces: the kind map entry `"builder-tampered": "Builder changed a job file"`; `parkKindClass("builder-tampered")` returns `chip-changes`.

- [ ] **Step 1: Read** the kind label map and `parkKindClass` in `dashboard/static/board.js`, and the test `parkKindLabel and parkKindClass cover every kind` in `dashboard/tests/board.test.js`.
- [ ] **Step 2: Write the failing test change:** add `"builder-tampered"` to the `red` list in that test and add `assert.equal(board.parkKindLabel("builder-tampered"), "Builder changed a job file");`.
- [ ] **Step 3: Run** `node --test dashboard/tests/board.test.js` and expect FAIL on the label and the class.
- [ ] **Step 4: Implement** the map entry and the class rule next to `builder-blocked`.
- [ ] **Step 5: Run** `node --test dashboard/tests/*.test.js` and `cargo test -j4 --manifest-path dashboard/Cargo.toml` and expect PASS.

### Task 2: Docs and spec

**Files:**
- Modify: `crates/factory-cli/tests/agent_lint.rs`, `docs/project-toml-reference.md`, `docs/software-factory.md`, `README.md`, `docs/superpowers/specs/2026-10-19-builder-permissions-design.md`

**Interfaces:**
- Produces: extended phrase lists in the three doc tests added by series 3 (`reference_documents_the_builder_permission_keys`, `software_factory_doc_describes_builder_permissions`, `readme_points_to_builder_permissions`) and one new test `spec_matches_the_builder_permission_fixes`.

- [ ] **Step 1: Read** the three doc tests in `agent_lint.rs`, the `## Builder permissions` sections of the three docs, and the spec sections 1, 3, 4, 5, 6 and the edge-case table, acceptance criteria and decisions.
- [ ] **Step 2: Write the failing tests.**
  - `reference_documents_the_builder_permission_keys` also requires: `timeout`, `quotes`, `wildcards`, `duplicates`, `Bash(<command>:*)`.
  - `software_factory_doc_describes_builder_permissions` also requires: `builder-tampered`, `knowledge base`, `not a sandbox`, `--output`, `has not been probed`, `cd web && bun test`, `duplicates are removed`.
  - `readme_points_to_builder_permissions` also requires `check_hints`, and forbids the phrase `never blocks on a prompt`.
  - `spec_matches_the_builder_permission_fixes` reads the spec and requires: `builder-tampered`, `timeout`, `Edit(//<kb dir>/**)` (spelled as the spec writes the KB deny rule), `shell operators`, `quotes`.
- [ ] **Step 3: Run** `cargo test -j4 -p factory-cli --test agent_lint` and expect FAIL listing the missing phrases.
- [ ] **Step 4: Write the docs.** Reference: the entry form, the extra rejected characters, the longer never list in one sentence, duplicates, and that compound check commands derive their first word only. Software-factory doc: replace the known-limit paragraph as the Global Constraints say, add the KB deny rules, the wrapper note, the `builder-tampered` kind with `rework` and `cancel`, and that a failed restore shows in the park reason. README: drop the overclaim, name `check_hints`. Spec: update sections 1, 3, 4 and 6 and the edge-case table, add acceptance bullets for the KB deny rules, the wrapper rule and the rework-only park, and add decisions ending `Decided by: call` for the longer never list, the stricter validator, the KB deny rules and the new park kind.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-cli --test agent_lint`, `cargo test -j4 -p factory-cli --test skill_drift` and `cargo fmt -p factory-cli -- --check`. Expect PASS.

Next plan: none
