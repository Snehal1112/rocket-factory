# Tidy docs and guards, plan 2: settings doc test, agent lint and docs

> **Spec:** docs/superpowers/specs/2026-11-22-tidy-docs-and-guards-design.md

> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** A test keeps `project-toml-reference.md` in step with `Config` (#96), `agent_lint` uses the shared preset list and catches the Rocket-only words in any case (#55, #43 item 5), and the docs named by #55 and #145 are corrected.

**Commit:** test(docs): guard the settings reference

**Architecture:** A new integration test in factory-cli reads the reference doc and `Config::default()` as JSON. The `agent_lint.rs` edits are local. The doc edits are text only.

**Tech Stack:** Rust, `serde_json`, Markdown.

## Global Constraints

- cargo commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`, never --workspace or --all.
- No change to any `Config` default or key. Only the doc text changes to match.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- Line numbers are from 9a0755d. Find text by its words when it has moved.

## File Structure

| File | Change |
|---|---|
| `crates/factory-cli/tests/project_toml_reference.rs` | New test file. |
| `docs/reference/project-toml-reference.md` | `risky_fix_rounds` and `log_level` Default cells, plus any row the test flags. |
| `crates/factory-cli/tests/agent_lint.rs` | `PRESETS` (line 9), forbid list (lines 1184-1204), kb refresher pin (line 375). |
| `docs/reference/software-factory.md` | Agent-file layers (line 64), gate summary (line 387), `waiting` sentence (line 424), the review detail words in the Gate record section. |
| `CLAUDE.md` | "Carried over from Rocket" (lines 57-59). |
| `dashboard/README.md` | The blank line at line 39. |

## Review Focus

1. An Effect cell holding a backticked `|`: a user expects the Default cell still read as the third cell. Pinned by `a_pipe_in_the_effect_cell_keeps_the_default` (string fixture).
2. A row in the `### Cost markers` `name`/`job` table: a user expects it not read as a key. Pinned by `every_config_key_is_documented` passing on the real doc, plus `only_key_tables_are_read` (string fixture).
3. A `None` key whose cell is `same as x`: a user expects a failure naming the key. Pinned by `a_none_default_must_start_with_unset`.
4. Preset text with `Zustand` or `ZUSTAND`: both are caught. Pinned by the lowercase forbid check, exercised by `forbid_words_match_in_any_case`.
5. A sixth preset added to `PRESET_NAMES`: `agent_lint` lints it without an edit. Pinned by the use of `PRESET_NAMES` itself, so no extra test.

---

### Task 1: Settings reference test

**Files:**
- Create: `crates/factory-cli/tests/project_toml_reference.rs`
- Modify: `docs/reference/project-toml-reference.md`

**Interfaces:**
- Consumes: `factory_core::Config` (`Default`, `Serialize`), and `serde_json::to_value`, as in `config_keys()` at `crates/factory-core/src/settings_block.rs:327-330`.
- Produces: in the test file only, `fn doc_rows(doc: &str) -> Vec<Row>` with `struct Row { key: String, default: String, line: usize }`, and `fn check(doc: &str, config: &serde_json::Value) -> Vec<String>`, which returns one failure line per problem.
- Port: none.

- [ ] **Step 1: Read.** Read `docs/reference/project-toml-reference.md` in full, `crates/factory-core/src/config.rs:100-230` and `crates/factory-cli/tests/check_coverage_docs.rs`, which shows how a test finds the repo root.

- [ ] **Step 2: Write the failing tests.**

```rust
const HEADER: &str = "| Key | Type | Default | Effect |\n|---|---|---|---|\n";

#[test]
fn every_config_key_is_documented() {
    let doc = std::fs::read_to_string(repo_root().join("docs/reference/project-toml-reference.md")).expect("doc");
    let config = serde_json::to_value(factory_core::Config::default()).expect("config json");
    let failures = check(&doc, &config);
    assert!(failures.is_empty(), "fix docs/reference/project-toml-reference.md:\n{}", failures.join("\n"));
}

#[test]
fn a_wrong_default_names_the_line() {
    let doc = format!("### A\n\n{HEADER}| `fix_rounds` | int | `7` | x |\n");
    let config = serde_json::json!({ "fix_rounds": 2 });
    let failures = check(&doc, &config);
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(failures[0].contains("fix_rounds") && failures[0].contains("line 5"), "{}", failures[0]);
    assert!(failures[0].contains('7') && failures[0].contains('2'), "{}", failures[0]);
}
```

| Test | Doc | Config | Expected failures |
|---|---|---|---|
| `a_missing_key_fails` | `HEADER` only | `{"fix_rounds": 2}` | 1, names `fix_rounds` |
| `a_none_default_must_start_with_unset` | row `` | `risky_fix_rounds` | int | same as x | y | `` | `{"risky_fix_rounds": null}` | 1, names `risky_fix_rounds` |
| `a_prose_default_is_not_compared` | row `` | `risk_paths` | list | 19 built-in globs | y | `` | `{"risk_paths": ["a"]}` | 0 |
| `a_pipe_in_the_effect_cell_keeps_the_default` | row `` | `fix_rounds` | int | `2` | a `x \| y` b | `` | `{"fix_rounds": 2}` | 0 |
| `only_key_tables_are_read` | a `\| name \| job \|` table with row `` | `fix_rounds` | z | ``, no key table | `{"fix_rounds": 2}` | 1, names `fix_rounds` |
| `quoted_string_defaults_match` | row `` | `builder_model` | string | `"m"` | x | `` | `{"builder_model": "m"}` | 0 |

The real-doc rules from the spec also go into `check`:
- `cost_markers` passes when a `### Cost markers` heading exists.
- The five `commit_*` names (`commit_model`, `commit_subagents`, `commit_budget_weighted`, `commit_batch`, `commit_plugins`) must each appear in backticks in the text of the `### Ignored settings` section.

- [ ] **Step 3: Run them.** `cargo test -j4 -p factory-cli --test project_toml_reference`. Expected: compile errors for `check` and `repo_root` until they are written. After that, `every_config_key_is_documented` fails on `log_level` and `risky_fix_rounds`.

- [ ] **Step 4: Implement** `doc_rows` and `check`:
  - Read rows only under a `| Key | Type | Default | Effect |` header line.
  - Split a row into at most four cells from the left, on ` | ` outside backticks.
  - Strip backticks and quotes from the Default cell.
  - Compare scalars only when the stripped cell is one token.
  - A JSON `null` default needs a cell that starts with `unset`.

  Then fix the doc: `risky_fix_rounds` becomes `unset (same as fix_rounds)`, `log_level` becomes `unset (logs at info)`, and fix every other row the test names.

- [ ] **Step 5: Run them again.** `cargo test -j4 -p factory-cli --test project_toml_reference`. Expected: 8 passed.

### Task 2: agent_lint fixes

**Files:**
- Modify: `crates/factory-cli/tests/agent_lint.rs:9`, `:375`, `:1184-1204`

**Interfaces:**
- Consumes: `factory_core::preset::PRESET_NAMES` (`crates/factory-core/src/preset.rs:12`).
- Produces: `fn rocket_only_hits(text: &str) -> Vec<&'static str>` in `agent_lint.rs`.
- Port: none.

- [ ] **Step 1: Read.** Read `crates/factory-cli/tests/agent_lint.rs:1-40`, `:360-380` and `:1170-1210`, and `crates/factory-cli/tests/tool_map.rs`, which already uses `PRESET_NAMES`.

- [ ] **Step 2: Write the failing test:**

```rust
#[test]
fn forbid_words_match_in_any_case() {
    assert_eq!(rocket_only_hits("Uses ZUSTAND and Lucide icons in src-Tauri."), vec!["lucide", "zustand", "tauri"]);
    assert_eq!(rocket_only_hits("A rocket-infra deploy for Rocket."), vec!["rocket-infra", "Rocket"]);
    assert!(rocket_only_hits("plain rust text about a rocket").is_empty());
}
```

- [ ] **Step 3: Run it.** `cargo test -j4 -p factory-cli --test agent_lint forbid_words`. Expected: compile error `cannot find function 'rocket_only_hits'`.

- [ ] **Step 4: Implement** `rocket_only_hits`. It returns the hits in a fixed order: `shadcn`, `lucide`, `zustand`, `rocket-infra` and `tauri` matched on the lowercased text, then `Rocket` matched case-sensitively on the raw text. `preset_agent_files_name_no_rocket_only_term` fails when the list is not empty, naming the file and the words. Replace `PRESETS` with `factory_core::preset::PRESET_NAMES`, and change the kb refresher pin from `workflow scripts` to `Never use workflow scripts`.

- [ ] **Step 5: Run them again.** `cargo test -j4 -p factory-cli --test agent_lint`. Expected: all pass.

### Task 3: Docs fixes

**Files:**
- Modify: `docs/reference/software-factory.md`, `CLAUDE.md`, `dashboard/README.md`
- Test: `crates/factory-cli/tests/check_coverage_docs.rs`

**Interfaces:**
- Consumes: the text in `README.md:629-632`, `dashboard/static/board.js:779-796` (`gateSummary`), `dashboard/src/gates.rs` `is_stale` and `crates/factory-engine/src/review.rs` (the `did not finish` and `gave no verdict` details).
- Produces: doc text pinned by one new test.
- Port: none.

- [ ] **Step 1: Read.** Read the files above, `docs/reference/software-factory.md:55-70` and `:380-430`, and `CLAUDE.md`.

- [ ] **Step 2: Write the failing test** in `check_coverage_docs.rs`. Reuse its existing helper that reads a repo file:

```rust
#[test]
fn reference_docs_carry_the_size_s_fixes() {
    let sf = read_repo("docs/reference/software-factory.md");
    assert!(sf.contains("a `.claude/agents/<name>.md` tracked on the base branch still wins"));
    assert!(sf.contains("`cancelled`") && sf.contains("skipped"));
    assert!(sf.contains("gave no verdict") && sf.contains("did not finish"));
    let claude = read_repo("CLAUDE.md");
    assert!(!claude.contains("These files in `.claude/` are copies of Rocket's factory files. Tests read them."));
    let dash = read_repo("dashboard/README.md");
    assert!(!dash.contains("There is no hook.\n\n- `.superpowers/sdd/<job dir>/gates.jsonl`"));
}
```

- [ ] **Step 3: Run it.** `cargo test -j4 -p factory-cli --test check_coverage_docs reference_docs_carry`. Expected: fails on the first assertion.

- [ ] **Step 4: Edit the docs.**
  - `software-factory.md:64`: add README's `.claude/agents` sentence.
  - `:387`: add `cancelled` and the skipped count to the summary forms.
  - `:424`: name the working stages `spec`, `build` and `review`.
  - The Gate record section: say that a review role line's detail is `did not finish` for a failed run and `gave no verdict` for a reply without a verdict.
  - CLAUDE.md: say that `.claude/factory/config.toml` is the only file tests read, and that the folder also holds `agents/factory-spec-reviewer.md` and the Claude settings files.
  - `dashboard/README.md`: remove the blank line inside the file list.

- [ ] **Step 5: Run it again.** `cargo test -j4 -p factory-cli --test check_coverage_docs`. Expected: all pass.

Next plan: docs/superpowers/plans/2026-11-22-tidy-docs-and-guards/plan-03-tidy-docs-and-guards.md
