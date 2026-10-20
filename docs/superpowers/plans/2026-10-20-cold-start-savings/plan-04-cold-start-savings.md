# Cold-start savings, plan 4: the marker in the shipped agent files, and docs

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** The shipped builder and reviewer agent files carry `<!-- factory-rules: 1 -->`, a lint keeps the marker and the engine constant in step, and the README explains the marker and the fallback.

**Architecture:** One line is added after the frontmatter of six files. `agent_lint.rs` reads the files and `factory_engine::RULES_VERSION`. The README gets a short section next to the upgrade notes.

**Tech Stack:** Markdown, Rust tests.

**Spec:** docs/superpowers/specs/2026-10-20-cold-start-savings-design.md

## Global Constraints

- Always `-j4`. Never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Tests use `.expect("...")` and `expect_err`; `agent_lint.rs` tests return `TestResult`.
- The marker is the exact line `<!-- factory-rules: 1 -->`, placed on the line right after the closing `---` of the frontmatter. No other text of the agent files changes: the rules already live there.
- `RULES_VERSION` in `factory-engine` and the number in the shipped files stay equal.
- `.claude/agents/*.md` are linted by `agent_lint.rs`. Run `cargo test -j4 -p factory-cli --test agent_lint` after editing them.

## Review Focus

1. A shipped file whose frontmatter still parses after the marker is added (test `builder_frontmatter_is_headless` and `reviewer_frontmatter_is_read_only` keep passing, Task 1).
2. A preset file edited but the marker missing in one of go or rust: the lint names the file (test `shipped_builder_and_reviewer_files_carry_the_rules_marker`, Task 1).
3. The engine constant bumped without the files: the lint fails (test `the_marker_version_matches_the_engine_constant`, Task 1).
4. The README stays in step with the code: it names the marker text, the fallback and the upgrade (test `readme_documents_the_rules_marker`, Task 2).

## File Structure

- `crates/factory-core/presets/rust/agents/factory-builder.md`, `factory-code-reviewer.md` (modify).
- `crates/factory-core/presets/go/agents/factory-builder.md`, `factory-code-reviewer.md` (modify).
- `.claude/agents/factory-builder.md`, `.claude/agents/factory-code-reviewer.md` (modify).
- `crates/factory-cli/tests/agent_lint.rs` (modify): three tests.
- `README.md` (modify): one section.

### Task 1: Marker in the six shipped files and the lint

**Files:**
- Modify: the six agent files above, `crates/factory-cli/tests/agent_lint.rs`
- Test: `crates/factory-cli/tests/agent_lint.rs`

**Interfaces:**
- Consumes: `factory_engine::RULES_VERSION` (plan 3), the helpers `read`, `require_all`, `frontmatter` and the existing preset path strings in `agent_lint.rs`.
- Produces: the six files each hold the exact line `<!-- factory-rules: 1 -->` directly after the frontmatter. The lint tests below. The files are static text; no engine port is involved.

- [ ] **Step 1: Read** `crates/factory-cli/tests/agent_lint.rs` (`read`, `require_all`, `frontmatter`, `builder_frontmatter_is_headless`, `reviewer_frontmatter_is_read_only` and the tests near line 680 that list the preset paths), the head of each of the six files, and the preset tests in `crates/factory-core/src/preset.rs` that compare embedded text with the files.
- [ ] **Step 2: Write the failing tests** in `agent_lint.rs`. One full example:

```rust
const MARKED: [&str; 6] = [
    "crates/factory-core/presets/rust/agents/factory-builder.md",
    "crates/factory-core/presets/rust/agents/factory-code-reviewer.md",
    "crates/factory-core/presets/go/agents/factory-builder.md",
    "crates/factory-core/presets/go/agents/factory-code-reviewer.md",
    ".claude/agents/factory-builder.md",
    ".claude/agents/factory-code-reviewer.md",
];

#[test]
fn shipped_builder_and_reviewer_files_carry_the_rules_marker() -> TestResult {
    for path in MARKED {
        let text = read(path)?;
        let after_frontmatter = text.splitn(3, "---\n").nth(2).unwrap_or_default();
        if after_frontmatter.lines().next() != Some("<!-- factory-rules: 1 -->") {
            return Err(format!("{path} has no marker on the line after the frontmatter"));
        }
    }
    Ok(())
}
```

  The other tests:

| Test | Input | Assertion |
|---|---|---|
| `the_marker_version_matches_the_engine_constant` | `factory_engine::RULES_VERSION` | every file in `MARKED` contains `format!("<!-- factory-rules: {} -->", RULES_VERSION)` |
| `the_committer_and_kb_files_have_no_rules_marker` | `.claude/agents/factory-committer.md` and `factory-kb-refresher.md` | neither contains `factory-rules` |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-cli --test agent_lint marker` and expect FAIL: `has no marker on the line after the frontmatter`.
- [ ] **Step 4: Edit** the six files: insert `<!-- factory-rules: 1 -->` as the first line after the closing `---`.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-cli --test agent_lint`, `cargo test -j4 -p factory-core` and `cargo test -j4 -p factory-engine --test slim_prompts` and expect PASS in all three. The last one shows the slim prompts follow a marked file.

### Task 2: README section for the marker

**Files:**
- Modify: `README.md`
- Test: `crates/factory-cli/tests/agent_lint.rs`

**Interfaces:**
- Consumes: the README "Upgrading" section (`factory upgrade`, the note about a shadowing agent file) and the "Cache report" section.
- Produces: a README section `## Agent file rules marker` placed after the Upgrading section. Text, in plain sentences: the builder and reviewer agent files carry `<!-- factory-rules: 1 -->`; when the worktree's file has a current marker the prompts leave out the rules the file already holds, otherwise the full prompts are used and one ledger line says `agent file <name>.md has no current rules marker, prompts keep the full rules`; `factory upgrade` refreshes unedited files; an edited file or a repo file that shadows it keeps working with the full prompt until it is merged by hand; the committer, message and KB prompts are unchanged.

- [ ] **Step 1: Read** the README "Upgrading" section (about lines 200 to 260), the "Cache report" section and how `agent_lint.rs` pins README text, for example `readme_points_to_builder_permissions`.
- [ ] **Step 2: Write the failing test** in `agent_lint.rs`:

```rust
#[test]
fn readme_documents_the_rules_marker() -> TestResult {
    let text = read("README.md")?;
    require_all(
        &text,
        "README",
        &[
            "## Agent file rules marker",
            "<!-- factory-rules: 1 -->",
            "no current rules marker",
            "factory upgrade",
            "prompts keep the full rules",
        ],
    )
}
```

- [ ] **Step 3: Run** `cargo test -j4 -p factory-cli --test agent_lint readme_documents_the_rules_marker` and expect FAIL: `README is missing`.
- [ ] **Step 4: Write** the README section from the Produces text.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-cli --test agent_lint` and expect PASS. Then `cargo clippy -j4 -p factory-cli --all-targets -- -D warnings` and `cargo fmt -p factory-cli -- --check`, both clean.

Next plan: none
