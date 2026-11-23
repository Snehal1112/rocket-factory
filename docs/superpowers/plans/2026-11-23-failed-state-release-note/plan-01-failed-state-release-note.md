# Failed Review State Release Note Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-23-failed-state-release-note-design.md
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Document in the README and the release steps that v0.4.0 and older cannot read a board holding the review state `"failed"`, and how to roll back.
**Commit:** docs(upgrade): note the failed review state

**Architecture:** Docs only. A new `#### Review states` subsection under README "Upgrading", one paste line in `docs/operations/releasing.md`, and two `require_all` doc tests in `crates/factory-cli/tests/agent_lint.rs` that pin both.

**Tech Stack:** Markdown, Rust integration tests (`agent_lint.rs` helpers `read`, `require_all`, `forbid_all`, `TestResult`).

## Global Constraints

- cargo commands are per crate: `cargo test -j4 -p factory-cli --test agent_lint <filter>`, never `--workspace` or `--all`.
- Tests return `TestResult` (`Result<(), String>`) and use `?`, as `readme_documents_upgrading` does; never the unwrap call.
- Docs use short, plain full sentences.
- No doc suggests editing `board.json` by hand.
- No code change outside `agent_lint.rs`.

## File Structure

| File | Change |
|---|---|
| `README.md` | Add `#### Review states` after the `#### Check coverage` subsection (it ends with "A job that parks as `no-checks` shows in `factory --project <name> status`."), before `### Refreshing seeded presets` |
| `docs/operations/releasing.md` | Add one line after "Paste the "Check coverage" note from `README.md` (Upgrading) into the draft release." |
| `crates/factory-cli/tests/agent_lint.rs` | Add `readme_documents_the_failed_review_state` and `releasing_doc_pastes_the_review_states_note`, next to `readme_documents_upgrading` |

## Review Focus

1. A reader who rolls back to v0.5.0 or v0.5.1 expects those to read `"failed"`; the note must name v0.4.0 and older as the broken versions, not "older than this release". Pinned by `readme_documents_the_failed_review_state` (`v0.4.0 and older`, `v0.5.0`).
2. A reader with a cancelled job showing `failed` expects to be told it never clears. Pinned by `readme_documents_the_failed_review_state` (`cancelled or merged`).
3. A reader looking for a quick fix must not be sent to edit `board.json`. Pinned by the `forbid_all` call in `readme_documents_the_failed_review_state`.
4. A release maker cutting a release after the next one must not paste the note again. Pinned by `releasing_doc_pastes_the_review_states_note` (`first release after v0.5.1`).
5. The existing `readme_documents_upgrading` must still pass after the README edit. Pinned by running the whole `agent_lint` test file in each task.

---

### Task 1: README review states note

**Files:**
- Modify: `README.md` (Upgrading section, after `#### Check coverage`)
- Test: `crates/factory-cli/tests/agent_lint.rs`

**Interfaces:**
- Consumes: `read(rel: &str) -> Result<String, String>`, `require_all(text: &str, label: &str, phrases: &[&str]) -> TestResult`, `forbid_all(text: &str, label: &str, phrases: &[&str]) -> TestResult` from `agent_lint.rs`.
- Produces: the README heading `#### Review states`, which the releasing.md paste line names as the "Review states" note.

- [ ] **Step 1: Read the code and docs this task touches**

Read `docs/superpowers/specs/2026-11-23-failed-state-release-note-design.md` (Design 1 and 3), `README.md` from `### Upgrading` to `### Refreshing seeded presets`, and `crates/factory-cli/tests/agent_lint.rs` lines 1-70 and the test `readme_documents_upgrading`.

- [ ] **Step 2: Write the failing test**

Add after `readme_documents_upgrading`:

```rust
#[test]
fn readme_documents_the_failed_review_state() -> TestResult {
    let doc = read("README.md")?;
    require_all(
        &doc,
        "README.md",
        &[
            "#### Review states",
            "`\"failed\"`",
            "v0.4.0 and older",
            "v0.5.0",
            "`unknown`",
            "<home>/VERSION",
            "restart `factoryd`",
            "retry or rework",
            "cancelled or merged",
        ],
    )?;
    forbid_all(&doc, "README.md", &["edit board.json", "`\"pending\"`"])
}
```

- [ ] **Step 3: Run it and see it fail**

Run: `cargo test -j4 -p factory-cli --test agent_lint readme_documents_the_failed_review_state`
Expected: FAIL, the `require_all` message names `#### Review states` as missing.

- [ ] **Step 4: Add the README subsection**

Insert `#### Review states` with the four paragraphs of spec Design 1, keeping every phrase the test requires. Light rewording is allowed; adding `edit board.json` or `` `"pending"` `` is not.

- [ ] **Step 5: Run the test file and see it pass**

Run: `cargo test -j4 -p factory-cli --test agent_lint`
Expected: all tests pass, including `readme_documents_the_failed_review_state` and `readme_documents_upgrading`.

### Task 2: Release step paste line

**Files:**
- Modify: `docs/operations/releasing.md` (Cutting a release)
- Test: `crates/factory-cli/tests/agent_lint.rs`

**Interfaces:**
- Consumes: the README heading `#### Review states` added by the README task; `read` and `require_all` from `agent_lint.rs`.
- Produces: nothing later tasks use.

- [ ] **Step 1: Read the code and docs this task touches**

Read spec Design 2 and `docs/operations/releasing.md` section `## Cutting a release`.

- [ ] **Step 2: Write the failing test**

Add after `readme_documents_the_failed_review_state`:

```rust
#[test]
fn releasing_doc_pastes_the_review_states_note() -> TestResult {
    let doc = read("docs/operations/releasing.md")?;
    require_all(
        &doc,
        "releasing.md",
        &["\"Review states\" note", "first release after v0.5.1"],
    )
}
```

- [ ] **Step 3: Run it and see it fail**

Run: `cargo test -j4 -p factory-cli --test agent_lint releasing_doc_pastes_the_review_states_note`
Expected: FAIL, the `require_all` message names `"Review states" note` as missing.

- [ ] **Step 4: Add the paste line**

Right after the "Check coverage" paste line, add: In the first release after v0.5.1, also paste the "Review states" note from `README.md` (Upgrading) into the draft release. Keep the following line about `chore:` commits where it is.

- [ ] **Step 5: Run the test file and see it pass**

Run: `cargo test -j4 -p factory-cli --test agent_lint`
Expected: all tests pass, including both new tests.

Next plan: none
