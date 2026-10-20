# Builder Permissions 3, Plan 1: Docs and the real-claude check record

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Document `builder_permissions`, `builder_allow`, the allowlist and deny rules and the driver-only file guard, and record the real-`claude` probe results, with a test that fails when a doc loses its key phrases.

**Architecture:** Docs only. Two new tests in `crates/factory-cli/tests/agent_lint.rs`, next to `docs_describe_the_new_design`, read the docs with the existing `read` and `require_all` helpers. No production code changes, so no port is involved.

**Tech Stack:** Markdown, and Rust tests in `factory-cli`.

**Spec:** docs/superpowers/specs/2026-10-19-builder-permissions-design.md

## Global Constraints

- Always `-j4`; never `--workspace` or `--all`; never set `CARGO_TARGET_DIR`.
- Every statement must match the code on the base branch. Read `crates/factory-engine/src/permissions.rs` and `crates/factory-core/src/config.rs` first and copy names, lists and defaults from them, never from memory.
- `builder_permissions` is a bool, default `true`. `builder_allow` is a string list, default `[]`, with entries of the form `Bash(<command>:*)` or `Bash(<command>)`, at most 200 characters, one line, no shell operators (`&&`, `||`, `;`, `|`, `>`, `<`, a backtick, `$(`), and never one of `git` (except `status`, `diff`, `log`, `show`, `rev-parse`), `rm`, `rmdir`, `curl`, `sudo`, `sh`, `bash`, `env`, `xargs`.
- The probe table in the spec is copied as it is: `claude` 2.1.303, haiku, a scratch folder, 2026-10-19. Nothing is re-measured in this plan.
- Docs describe the known limit that the base command list includes `find`, which can delete files or run other commands, and that the protection is the disposable job worktree plus the deny rules and the file guard on the job folder.
- Prose stays short and plain. No emojis.

## Review Focus

- `docs/project-toml-reference.md` lists both keys with type, default and effect, and states that a bad `builder_allow` entry makes the whole config fail closed (Task 1 test `reference_documents_the_builder_permission_keys`).
- The software-factory doc names the fixed base list, the derived programs, the deny rules, the guard and the `builder changed <file>` park reason (Task 2 test `software_factory_doc_describes_builder_permissions`).
- The probe table keeps all nine rows and the version and date line (same test).
- The README points to the two docs and states the kill switch (Task 2 test `readme_points_to_builder_permissions`).
- The config key list in `docs/software-factory.md` (the "Config:" bullet) names the two new keys (same Task 2 test).

---

## File Structure

- `crates/factory-cli/tests/agent_lint.rs` (modify): three doc tests.
- `docs/project-toml-reference.md` (modify): two rows and a short subsection.
- `docs/software-factory.md` (modify): the key list, a `## Builder permissions` section with the probe table.
- `README.md` (modify): a short section.

### Task 1: The reference

**Files:**
- Modify: `crates/factory-cli/tests/agent_lint.rs`, `docs/project-toml-reference.md`

**Interfaces:**
- Produces: test `reference_documents_the_builder_permission_keys`; two table rows in the `### Builder prompt` table of `docs/project-toml-reference.md` (after `check_hints`), and a `### Builder permissions` subsection right after that table.

- [ ] **Step 1: Read** `docs/project-toml-reference.md` (the `### Builder prompt` table and the example at the end), `crates/factory-core/src/config.rs` (the two fields and `check_builder_allow`), `crates/factory-engine/src/permissions.rs`, and `docs_describe_the_new_design` with the `read`, `require_all` and `forbid_all` helpers in `crates/factory-cli/tests/agent_lint.rs`.
- [ ] **Step 2: Write the failing test**, modelled on `docs_describe_the_new_design`:
  ```rust
  #[test]
  fn reference_documents_the_builder_permission_keys() -> TestResult {
      let doc = read("docs/project-toml-reference.md")?;
      require_all(
          &doc,
          "project-toml-reference.md",
          &[
              "`builder_permissions`",
              "`builder_allow`",
              "acceptEdits",
              "Bash(<program>:*)",
              "shell operators",
              "rmdir",
              "xargs",
              "fails closed",
          ],
      )
  }
  ```
- [ ] **Step 3: Run** `cargo test -j4 -p factory-cli --test agent_lint reference_documents` and expect FAIL with `project-toml-reference.md is missing: [...]` listing all eight phrases.
- [ ] **Step 4: Write the docs.** The two rows: `builder_permissions` (bool, `true`, "Each builder run gets permission mode `acceptEdits`, an allowlist of tools and the job folder as an extra directory, so it never waits on a prompt. `false` restores the old builder arguments.") and `builder_allow` (string list, `[]`, "Extra allow rules such as `Bash(make:*)`, added after the derived ones. Rules for the programs in `git`, `rm`, `rmdir`, `curl`, `sudo`, `sh`, `bash`, `env` and `xargs` are refused, as are shell operators; a bad entry makes the whole config fail closed."). The subsection `### Builder permissions` explains in a short list: the fixed base, the programs derived from `[[checks]]` commands and `check_hints` (the `rust` preset gives `Bash(cargo:*)`), `builder_allow` last, and an example `builder_allow = ["Bash(make:*)"]`.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-cli --test agent_lint` and expect PASS.

### Task 2: The factory doc, the probe record and the README

**Files:**
- Modify: `crates/factory-cli/tests/agent_lint.rs`, `docs/software-factory.md`, `README.md`

**Interfaces:**
- Produces: tests `software_factory_doc_describes_builder_permissions` and `readme_points_to_builder_permissions`; a `## Builder permissions` section in `docs/software-factory.md` placed after `## Per-task builders`, with the probe table; the two keys added to the "Config:" bullet; a `## Builder permissions` section in `README.md` after `## Logging`.

- [ ] **Step 1: Read** the `## Per-task builders` section and the "Config:" bullet in `docs/software-factory.md`, the `## Logging` section of `README.md`, and the "Verified behaviour" table of the spec.
- [ ] **Step 2: Write the failing tests** in `agent_lint.rs`:
  - `software_factory_doc_describes_builder_permissions` requires in `docs/software-factory.md`: `` `builder_permissions` ``, `` `builder_allow` ``, `` `--disallowedTools` ``, `Edit(//`, `builder changed`, `acceptEdits`, `claude 2.1.303`, `2026-10-19`, `Deny rule plus an allow`, `` `find` ``, `Bash(cd:*)`, `Bash(git status:*)`.
  - `readme_points_to_builder_permissions` requires in `README.md`: `` `builder_permissions` ``, `docs/project-toml-reference.md`, `docs/software-factory.md`.
- [ ] **Step 3: Run** `cargo test -j4 -p factory-cli --test agent_lint builder_permissions` and expect FAIL with both docs missing phrases.
- [ ] **Step 4: Write the docs.** In `docs/software-factory.md`: add `builder_permissions`, `builder_allow` to the "Config:" bullet; a `## Builder permissions` section with short subsections or bullets for why (headless runs cannot answer prompts, repository settings are ignored in an untrusted folder), the flags each builder run gets, the fixed base list and derived programs, the deny rules on the job folder and the guard (`builder changed <file>`, `ParkKind::Other`), the kill switch, the known limit about `find`, and the probe table with the line `Probes with the real claude 2.1.303 on haiku in a scratch folder, 2026-10-19`. In `README.md`: a five-line section with the kill switch and the two doc pointers.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-cli --test agent_lint` and expect PASS, then `cargo test -j4 -p factory-cli --test skill_drift` and `cargo fmt -p factory-cli -- --check`, expect PASS and no output.

Next plan: none
