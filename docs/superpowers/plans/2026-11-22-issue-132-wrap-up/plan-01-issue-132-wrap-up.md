# Codex role prompts, skills and the probe gap Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-22-issue-132-wrap-up-design.md
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Codex role prompts leave out the command-only map section, a Codex role allowed no skill gets every skill turned off, and `factory probe` passes a role whose named skill is not installed.

**Commit:** fix(codex): fit role prompts, skills and the probe

**Architecture:** A pure map filter in factory-core feeds `codex_agent_text`. `codex_skill_args` in factory-engine gains the full off list. The probe learns the absent skills through a helper shared with `prepare_for_codex` and passes them to a new pure `evaluate_with_absent` in factory-core; `render_probe` in factory-cli prints the result.

**Tech Stack:** Rust.

## Global Constraints

- Cargo commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- No task adds a dependency or edits any `Cargo.toml`, `.github/**`, `scripts/**` or `journeybook/**`.
- No board, ledger or event field. `RoleProbe` and the probe JSON keep their fields.
- `TOOL_MAP_CODEX` and `tool-map-codex.md` stay unchanged; `build_codex_skill` keeps the whole map.

## Review Focus

1. The `### Model option` section moved to the middle of the map, or removed: a user expects the filter to drop only that section, or return the map unchanged. Pinned by `codex_runtime_tool_map_drops_only_the_model_option_section`.
2. A kb role under Codex that lists `kb-refresh` anyway while it is not installed: a user expects no `no_extra_skills` failure. Pinned by `an_absent_named_skill_listed_anyway_is_not_an_extra`.
3. A failed Codex run of an all-absent role: a user expects `agent_text` to fail and `skills` to keep the not-installed detail. Pinned by `a_failed_codex_run_keeps_the_not_installed_detail`.
4. A Claude probe: a user expects no change. Pinned by `evaluate_matches_evaluate_with_absent_on_an_empty_list` and the existing Claude probe tests, which pass unchanged.
5. The CLI probe reads the real `HOME` skill folders, so a machine-dependent CLI test would be flaky: the render test builds its `ProbeReport` by hand. Pinned by `the_probe_report_names_a_not_installed_skill`.

## File Structure

- Modify: `crates/factory-core/src/lib.rs` (`codex_runtime_tool_map` and its test), `crates/factory-core/src/probe.rs`
- Modify: `crates/factory-engine/src/codex_prep.rs`, `crates/factory-engine/src/codex_skills.rs`, `crates/factory-engine/src/probe.rs`
- Modify: `crates/factory-engine/tests/codex_prep.rs`, `crates/factory-engine/tests/probe.rs`
- Modify: `crates/factory-cli/src/probe.rs`

---

### Task 1: Codex role prompts without the command-only section

**Files:**
- Modify: `crates/factory-core/src/lib.rs`
- Modify: `crates/factory-engine/src/codex_prep.rs`
- Modify: `crates/factory-engine/tests/codex_prep.rs`

**Interfaces:**
- Port: factory-core pure text function; in factory-engine the change sits in `Engine::prepare_for_codex`, which feeds the `AgentRunner` port. Tests use the scripted runner and `Fixture` already in `tests/codex_prep.rs`.
- Produces: `pub fn codex_runtime_tool_map() -> String` in factory-core, exported from `lib.rs`.

**Tests added:** `codex_runtime_tool_map_drops_only_the_model_option_section` (factory-core), `codex_role_text_names_no_factory_skill` (`tests/codex_prep.rs`).

- [ ] **Step 1: Read the code**

Read `crates/factory-core/assets/tool-map-codex.md`, `TOOL_MAP_CODEX` in `crates/factory-core/src/lib.rs`, `codex_agent_text` and `prepare_for_codex` in `crates/factory-engine/src/codex_prep.rs`, `build_codex_skill` in `crates/factory-cli/src/install.rs`, and `run_agent_prepends_the_agent_file_and_the_codex_preface` and the `agent_text` helper in `crates/factory-engine/tests/codex_prep.rs`.

- [ ] **Step 2: Write the failing tests**

`codex_runtime_tool_map_drops_only_the_model_option_section`, in a `#[cfg(test)]` module of `lib.rs` (or the factory-core module that holds the function):
- `let map = codex_runtime_tool_map();`
- assert `!map.contains("### Model option")`, `!map.contains("$factory")`, `!map.contains("this Codex skill")`;
- assert `map.contains("## Harness notes: Codex")`, `map.contains("### Task list")`, `map.contains("### Commit")`;
- every other `### ` heading of `TOOL_MAP_CODEX` is still in `map`: loop over `TOOL_MAP_CODEX.lines().filter(|l| l.starts_with("### ") && *l != "### Model option")` and assert each.
- Call the inner helper on a map with the section in the middle, `"## H\n\n### A\n\na\n\n### Model option\n\nm\n\n### B\n\nb\n"`, and assert the result keeps `### A`, `a`, `### B`, `b` and has no `m` line. A map without the section comes back unchanged. Name the inner helper `without_section(map: &str, heading: &str) -> String` and keep it private.

`codex_role_text_names_no_factory_skill`: build a builder run as `run_agent_prepends_the_agent_file_and_the_codex_preface` does, take `agent_text(&runner, "builder")`, and assert it contains `## Harness notes: Codex`, and contains neither `### Model option` nor `$factory`.

- [ ] **Step 3: Run them and see them fail**

Run: `cargo test -j4 -p factory-core codex_runtime_tool_map`, then `cargo test -j4 -p factory-engine --test codex_prep codex_role_text`
Expected: factory-core fails with `cannot find function 'codex_runtime_tool_map'`; the engine test fails because the text holds `### Model option`.

- [ ] **Step 4: Implement**

- `without_section` removes the line equal to `heading` and every line after it up to, not including, the next line starting with `### ` or `## `, or the end. It trims the blank lines left at the cut so the map has no run of three newlines.
- `codex_runtime_tool_map()` returns `without_section(TOOL_MAP_CODEX, "### Model option")`.
- `codex_agent_text` uses `codex_runtime_tool_map()` in place of `TOOL_MAP_CODEX`.

- [ ] **Step 5: Run them and see them pass**

Run: `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-engine --test codex_prep`, `cargo test -j4 -p factory-cli --test tool_map`
Expected: all pass, 0 failed.

### Task 2: every skill off for a Codex role allowed none

**Files:**
- Modify: `crates/factory-engine/src/codex_skills.rs`
- Modify: `crates/factory-engine/tests/codex_prep.rs`

**Interfaces:**
- Port: pure function in factory-engine, used by `prepare_for_codex` before the `AgentRunner` port.
- Consumes: `FoundSkill { group, name, path }` and `allowed(skill, allow)` in `codex_skills.rs`.
- Produces: `codex_skill_args(found: &[FoundSkill], allow: &[String]) -> Vec<String>` keeps its signature.

**Tests added:** `empty_or_unmatched_allowlist_turns_every_skill_off` (replaces `empty_or_unmatched_allowlist_turns_instructions_off`), `an_empty_find_with_no_allowlist_has_no_skills_config`; changed `unmatched_allowlist_has_no_skills_preface` and `reviewer_gets_no_skills` in `tests/codex_prep.rs`.

- [ ] **Step 1: Read the code**

Read `codex_skill_args`, `toml_string` and the unit tests `empty_or_unmatched_allowlist_turns_instructions_off`, `allowlist_disables_every_other_canonical_path` and `paths_are_toml_escaped` in `crates/factory-engine/src/codex_skills.rs`. Read `unmatched_allowlist_has_no_skills_preface`, `reviewer_gets_no_skills` and `skill_root` in `crates/factory-engine/tests/codex_prep.rs`.

- [ ] **Step 2: Write the failing tests**

`empty_or_unmatched_allowlist_turns_every_skill_off`: `let skills = [found("", "foo", "/p/foo"), found("", "bar", "/p/bar")];`. For both `&[]` and `&list(&["nothing"])`, assert the result equals `["skills.include_instructions=false", "skills.bundled.enabled=false", "skills.config=[{path=\"/p/bar\",enabled=false},{path=\"/p/foo\",enabled=false}]"]`.

`an_empty_find_with_no_allowlist_has_no_skills_config`: `codex_skill_args(&[], &[])` equals `["skills.include_instructions=false", "skills.bundled.enabled=false"]`.

In `tests/codex_prep.rs`:
- `unmatched_allowlist_has_no_skills_preface`: `spec.codex_config` equals `["skills.include_instructions=false", "skills.bundled.enabled=false"]` (no skill root is set, so nothing is found).
- `reviewer_gets_no_skills`: `spec.codex_config` equals the two values above plus `format!("skills.config=[{{path=\"{}\",enabled=false}}]", tdd.display())`, where `tdd = std::fs::canonicalize(root.join("superpowers/tdd/SKILL.md")).expect("canon")`, as `builder_with_superpowers_installed_gets_skills_and_the_allowlist` builds its value. `toml_string` is private, so do not call it. `FoundSkill.path` is the canonical `SKILL.md` path, and macOS resolves `/var` to `/private/var`.

- [ ] **Step 3: Run them and see them fail**

Run: `cargo test -j4 -p factory-engine --lib codex_skills`, then `cargo test -j4 -p factory-engine --test codex_prep`
Expected: the four tests fail, because today the result is only `skills.include_instructions=false`.

- [ ] **Step 4: Implement**

In `codex_skill_args`, when `allowed_paths` is empty, return `skills.include_instructions=false`, then `skills.bundled.enabled=false`, then, when `found` is not empty, one `skills.config=[...]` entry built the same way as the allowed case builds its disabled list. Keep the allowed case as it is. Update the doc comment.

- [ ] **Step 5: Run them and see them pass**

Run: `cargo test -j4 -p factory-engine --lib codex_skills`, `cargo test -j4 -p factory-engine --test codex_prep`
Expected: all pass, 0 failed.

### Task 3: the probe follows the Codex skill gap

**Files:**
- Modify: `crates/factory-core/src/probe.rs`
- Modify: `crates/factory-engine/src/codex_prep.rs`, `crates/factory-engine/src/probe.rs`, `crates/factory-engine/tests/probe.rs`
- Modify: `crates/factory-cli/src/probe.rs`

**Interfaces:**
- Port: pure probe rules in factory-core; the engine probe runs behind the `AgentRunner` port (fakes in `crates/factory-engine/tests/probe.rs`).
- Consumes: `discover_skills`, `allowed` (`codex_skills.rs`), `codex_worktree_skills_dir`, `Repo::worktree_root_of`, `allowed_skill_entries`.
- Produces:
  - factory-core: `pub fn evaluate_with_absent(role: ProbeRole, harness: Harness, config: &Config, answer: &ProbeAnswer, absent: &[String]) -> RoleProbe`; `evaluate` calls it with `&[]`; `pub const NOT_INSTALLED: &str = "not installed, short rules used: ";`.
  - factory-engine: `pub(crate) fn codex_found_skills(&self, cwd: &Path) -> Vec<FoundSkill>` on `Engine`, in `codex_prep.rs`.

**Tests added:** factory-core `a_not_installed_named_skill_is_not_expected`, `one_absent_entry_of_two_checks_only_the_other`, `an_absent_named_skill_listed_anyway_is_not_an_extra`, `an_extra_skill_still_fails_when_the_named_one_is_absent`, `evaluate_matches_evaluate_with_absent_on_an_empty_list`; factory-engine `a_codex_kb_probe_without_kb_refresh_passes`, `a_failed_codex_run_keeps_the_not_installed_detail`; factory-cli `the_probe_report_names_a_not_installed_skill`.

- [ ] **Step 1: Read the code**

Read `evaluate`, `expected_channels`, `allowed_skill_entries`, `skill_matches`, `RoleProbe` and `Channel` and their tests in `crates/factory-core/src/probe.rs`. Read `probe_run`, `probe_base_spec`, `hide_codex_tools` in `crates/factory-engine/src/probe.rs`, `shape_spec` in `crates/factory-engine/src/ports.rs`, `prepare_for_codex` in `codex_prep.rs`, and the Codex tests and runner fakes in `crates/factory-engine/tests/probe.rs`. Read `render_probe` and `cell` in `crates/factory-cli/src/probe.rs`.

- [ ] **Step 2: Write the failing tests**

factory-core, using the `cfg()` helper and answer builders the probe tests already use, for `ProbeRole::Kb` and `Harness::Codex` (allowlist `["kb-refresh"]`):

```rust
#[test]
fn a_not_installed_named_skill_is_not_expected() {
    let answer = ProbeAnswer { marker_seen: true, ..ProbeAnswer::default() };
    let p = evaluate_with_absent(ProbeRole::Kb, Harness::Codex, &cfg(), &answer, &["kb-refresh".to_string()]);
    let skills = p.channels.iter().find(|c| c.name == "skills").expect("skills");
    assert!(!skills.expected);
    assert!(!skills.present);
    assert_eq!(skills.detail, format!("{NOT_INSTALLED}kb-refresh"));
    assert!(p.channels.iter().filter(|c| c.expected).all(|c| c.present), "{:?}", p.channels);
}
```

| Test | Setup | Assertion |
|---|---|---|
| `one_absent_entry_of_two_checks_only_the_other` | `kb_plugins = ["superpowers"]` parsed into the config, so kb allows `superpowers` and `kb-refresh`; absent `["kb-refresh"]`; answer skills `["superpowers:tdd"]` | `skills` expected and present; detail is `format!("1 skills listed; {NOT_INSTALLED}kb-refresh")` |
| `an_absent_named_skill_listed_anyway_is_not_an_extra` | absent `["kb-refresh"]`; answer skills `["kb-refresh"]` | `no_extra_skills` present |
| `an_extra_skill_still_fails_when_the_named_one_is_absent` | absent `["kb-refresh"]`; answer skills `["factory"]` | `no_extra_skills` expected and not present, detail `extra: factory` |
| `evaluate_matches_evaluate_with_absent_on_an_empty_list` | each `ProbeRole::ALL` role, both harnesses, one answer with the marker and one skill | `assert_eq!(evaluate(..), evaluate_with_absent(.., &[]))` (`RoleProbe` derives `PartialEq`) |

factory-engine `tests/probe.rs`, building a Codex probe with `fixture(Harness::Codex)` as `codex_marker_rides_agent_prompt_after_the_preface` and `all_channels_present_when_the_answer_echoes_everything` do. The fixture sets no Codex skill root and its repo has no `.agents/skills`, so `kb-refresh` is absent; if the fixture does set roots, pass an empty tempdir with `with_codex_skill_roots`:
- `a_codex_kb_probe_without_kb_refresh_passes`: the fake answers the marker and `SKILLS: NONE`; the kb `RoleProbe` has `skills.expected == false`, detail starting with `NOT_INSTALLED`, and every expected channel present.
- `a_failed_codex_run_keeps_the_not_installed_detail`: the fake run returns `AgentRun::failed("boom")`, as `failed_run_marks_all_expected_channels_missing` does; `agent_text` is expected and not present; `skills` keeps `expected == false` and the `NOT_INSTALLED` detail.

factory-cli, a unit test in `crates/factory-cli/src/probe.rs` on a hand-built `ProbeReport` with one kb role whose `skills` channel is `expected: false, present: false, detail: format!("{NOT_INSTALLED}kb-refresh")`: `render_probe(&report)` contains `Not installed (kb): kb-refresh, short rules used` and no `Missing (kb) skills`.

- [ ] **Step 3: Run them and see them fail**

Run: `cargo test -j4 -p factory-core probe`, `cargo test -j4 -p factory-engine --test probe`, `cargo test -j4 -p factory-cli --lib probe`
Expected: compile errors `cannot find function 'evaluate_with_absent'` and `cannot find value 'NOT_INSTALLED'`; after those exist, the engine and CLI tests fail on MISSING and on the absent render line.

- [ ] **Step 4: Implement**

- factory-core: `evaluate_with_absent` computes `entries` as today, then `kept` = entries not in `absent`. `missing` is checked against `kept`; `extra` is still checked against the full `entries`. The `skills` channel: when `absent` is not empty and `kept` is empty, `expected = false`, `present = false`, detail `format!("{NOT_INSTALLED}{}", absent.join(", "))`; when both are non-empty, the pass detail is `format!("{} skills listed; {NOT_INSTALLED}{}", answer.skills.len(), absent.join(", "))`. The expected set is `expected_channels` minus `skills` when `kept` is empty and `absent` is not.
- factory-engine `codex_prep.rs`: move the root building and `discover_skills` call of `prepare_for_codex` into `codex_found_skills(&self, cwd)`, and call it there.
- factory-engine `probe_run`: for `Harness::Codex`, before `shape_spec`, compute `absent` from `allowed_skill_entries(role, Codex, &self.config)` where no skill in `self.codex_found_skills(&spec.cwd)` is `allowed(s, std::slice::from_ref(entry))`; for Claude, `absent` is empty. Both branches call `evaluate_with_absent` with it. The failed branch overwrites only channels whose own `expected` is true.
- Re-export `evaluate_with_absent` and `NOT_INSTALLED` from the `pub use probe::{..}` line in `crates/factory-core/src/lib.rs`. In `crates/factory-engine/src/probe.rs`, import `allowed_skill_entries, evaluate_with_absent` in place of `evaluate, expected_channels`, which become unused there, so clippy `-D warnings` stays clean.
- factory-cli `render_probe`: after the `Missing` lines of a role, for a `skills` channel with `!expected` and a detail starting with `NOT_INSTALLED`, print `Not installed ({role}): {rest}, short rules used`, where `rest` is the text after the prefix.

- [ ] **Step 5: Run them and see them pass**

Run: `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-engine --test probe`, `cargo test -j4 -p factory-engine --test codex_prep`, `cargo test -j4 -p factory-cli --lib probe`, `cargo test -j4 -p factory-cli --test probe_cmd`
Expected: all pass, 0 failed. `probe_notes_a_delivery_codex_ignores` passes unchanged.

- [ ] **Step 6: Format and lint**

Run: `cargo fmt -p factory-core`, `cargo fmt -p factory-engine`, `cargo fmt -p factory-cli`, then `cargo clippy -j4 -p factory-core --all-targets -- -D warnings`, `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`, `cargo clippy -j4 -p factory-cli --all-targets -- -D warnings`.
Expected: clippy finishes with no warnings.

Next plan: docs/superpowers/plans/2026-11-22-issue-132-wrap-up/plan-02-issue-132-wrap-up.md
