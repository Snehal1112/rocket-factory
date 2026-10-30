# Harness Hardening, Plan 02: Codex Skills By Role

> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Codex runs see the skills their role is allowed (superpowers for builders, the commit skill for the committer), and no others, instead of being told to use no skills.

**Architecture:** Probes showed Codex already lists every skill under `~/.agents/skills`, `$CODEX_HOME/skills` (default `~/.codex/skills`) and `<worktree>/.agents/skills`, plus bundled skills. So nothing needs to be injected. A new `codex_skills.rs` finds skills in those roots (canonical paths, symlinks followed) and turns a per-role allowlist into `-c` values: bundled off and every non-allowed `SKILL.md` disabled by path. The allowlist reuses the role plugin lists. `AgentSpec.codex_config` carries the values into `codex_args`. `codex_prep` stops banning skills and keeps `SKILL_SENTENCE` only where the role's named skill is not available.

**Tech Stack:** Rust, `std::fs`, `tempfile` (dev) with real symlinks (`std::os::unix::fs::symlink`, tests gated `#[cfg(unix)]`).

**Spec:** `docs/superpowers/specs/2026-10-30-harness-hardening-design.md` (GitHub issue #36, parent #40). The issue's "find a way to get skills in" framing is superseded by the 2026-10-30 probes: skills are already visible, the work is the allowlist.

## Global Constraints

- Cargo per crate with `-j4`: `cargo test -j4 -p <crate> <filter>`; never `--workspace` or `--all`; never set `CARGO_TARGET_DIR`.
- Git only through `factory_git::Repo`; never shell out to git. `gh`, `claude`, `codex` may be spawned.
- Paths reach the driver only through `Layout` / `factory_core::layout` helpers; `crates/factory-cli/tests/no_hardcoded_paths.rs` fails on hardcoded state paths outside `layout.rs`, so all new path literals (`.agents`, `.codex`, `skills`) go in `layout.rs`.
- Board JSON stays compatible with the dashboard: this plan adds no board fields.
- Tests use `fake-claude`, never the real CLI. No test reads the real `$HOME` or `$CODEX_HOME`: roots are passed in.
- Skill discovery never fails a run. A missing root or an unusable entry is skipped.
- Comments are short full sentences ending with a period. Docs go in `docs/` (reference docs in `docs/reference/`).
- The driver makes the commits (with the `dev-workflow-skills:1-git-commit` skill); no task has a commit step.

## Review Focus

1. A symlinked skill folder: only the canonical `SKILL.md` path is disabled, and the same skill reached through two roots is disabled once (`symlinked_roots_give_one_canonical_entry`, Task 1).
2. An allowlist of `[]`, or entries that match nothing found, give only `skills.include_instructions=false` and a preface that still bans skills (`empty_or_unmatched_allowlist_turns_instructions_off`, Task 1; `unmatched_allowlist_has_no_skills_preface`, Task 3).
3. A path with a quote, backslash or non-ASCII character is a valid TOML string (`paths_are_toml_escaped`, Task 1).
4. A missing root, a broken symlink and a skill folder without `SKILL.md` do not error (`missing_root_and_broken_entries_are_skipped`, Task 1).
5. The committer under Codex without the commit skill installed still gets `SKILL_SENTENCE`; with it installed, it does not (Task 3).

## File Structure

- Modify `crates/factory-core/src/layout.rs`, `crates/factory-core/src/lib.rs`: skill-root helpers.
- Create `crates/factory-engine/src/codex_skills.rs`; add `mod codex_skills;` in `crates/factory-engine/src/lib.rs`.
- Modify `crates/factory-agent/src/spec.rs`, `crates/factory-agent/src/codex.rs`: `codex_config`.
- Modify `crates/factory-engine/src/codex_prep.rs`, `crates/factory-engine/src/lib.rs` (`Engine`), `crates/factory-engine/src/compat.rs` (`Driver`), `crates/factory-cli/src/commands.rs`: wiring.
- Docs: `docs/reference/software-factory.md`, `docs/reference/project-toml-reference.md`.
- Tests: unit tests in each file above, `crates/factory-engine/tests/codex_prep.rs`.

---

### Task 1: Skill-root helpers in layout

**Files:**
- Modify: `crates/factory-core/src/layout.rs`, `crates/factory-core/src/lib.rs` (re-exports)

**Interfaces:**
- Produces (all pure, no I/O):
  - `pub fn codex_home_skills_dir(codex_home: &Path) -> PathBuf`: `<codex_home>/skills`.
  - `pub fn codex_home_from_env(user_home: &Path, codex_home_var: Option<&OsStr>) -> PathBuf`: the var when set and non-empty, else `<user_home>/.codex`.
  - `pub fn codex_worktree_skills_dir(worktree: &Path) -> PathBuf`: `<worktree>/.agents/skills`.
  - `pub fn codex_user_skill_roots(user_home: &Path, codex_home_var: Option<&OsStr>) -> Vec<PathBuf>`: `[codex_skills_dir(user_home), codex_home_skills_dir(&codex_home_from_env(..))]`.

- [ ] **Step 1: Write failing unit tests** in `layout.rs`: `codex_home_defaults_to_dot_codex` (`None` and `Some("")` give `/h/.codex`), `codex_home_var_wins` (`Some("/x")` gives `/x`), `user_skill_roots_lists_agents_then_codex_home` (`["/h/.agents/skills", "/h/.codex/skills"]`), `worktree_skills_dir_is_under_dot_agents`.
- [ ] **Step 2: Run** `cargo test -j4 -p factory-core layout`. Expected: FAIL, functions missing.
- [ ] **Step 3: Implement** the four functions in `layout.rs` and re-export them from `lib.rs` next to `codex_skills_dir`.
- [ ] **Step 4: Run** the same command. Expected: PASS.

#### Then: Discovery and `-c` values

**Files:**
- Create: `crates/factory-engine/src/codex_skills.rs`
- Modify: `crates/factory-engine/src/lib.rs` (`pub(crate) mod codex_skills;`)

**Interfaces:**
- Port: none. Discovery is plain `std::fs` called directly, like `factory_checks`; it feeds the spec that the `AgentRunner` port runs. Tests use real temp folders.
- Produces:
  - `#[derive(Debug, Clone, PartialEq, Eq)] pub struct FoundSkill { pub group: String, pub name: String, pub path: PathBuf }`. `group` is the subfolder name, empty for a top-level skill. `name` is the skill folder name (the frontmatter is not read). `path` is the canonical `SKILL.md`.
  - `pub fn discover_skills(roots: &[PathBuf]) -> Vec<FoundSkill>`.
  - `pub fn allowed(skill: &FoundSkill, allow: &[String]) -> bool`: each entry is compared after dropping any `@marketplace` suffix; a skill with a group is allowed when an entry equals the group, a top-level skill when an entry equals its name.
  - `pub fn codex_skill_args(found: &[FoundSkill], allow: &[String]) -> Vec<String>`: the `-c` values, one string each.
  - `fn toml_string(s: &str) -> String`: a TOML basic string; escapes `\`, `"`, and control characters (`\n`, `\t`, `\r` by name, the rest as `\uXXXX`); non-ASCII stays as is.
- Discovery rules: for each root in order, read its entries sorted by name, skipping names that start with `.` (this skips `.system`, the bundled folder) and following symlinks. A directory holding `SKILL.md` is a top-level skill. Otherwise it is a group: each child directory holding `SKILL.md` is a skill of that group (one level only). `SKILL.md` is canonicalized with `std::fs::canonicalize`; on any error (missing root, broken symlink, no `SKILL.md`, unreadable folder) skip that entry silently. Skills with an equal canonical `path` collapse to the first one found. The file content is never read, so an unreadable `SKILL.md` that still canonicalizes is kept and Codex itself decides what to do with it.
- Argument rules for `codex_skill_args`:
  - No found skill is allowed (this includes an empty `allow`): return `["skills.include_instructions=false"]`.
  - Otherwise return `"skills.bundled.enabled=false"`, plus one value `skills.config=[{path="<p>",enabled=false},...]` when any path is disabled. The disabled paths are those of found skills that are not allowed, minus any path that another found entry allows, sorted and deduplicated. All entries go in one `-c` value, because a second `skills.config` value would replace the first.

- [ ] **Step 1: Write failing tests** in the module (temp dirs, real symlinks):
  - `discovers_top_level_and_grouped_skills`: a root with `foo/SKILL.md`, `superpowers/brainstorming/SKILL.md`, `.system/imagegen/SKILL.md` and a folder without `SKILL.md`; expect `[("", "foo"), ("superpowers", "brainstorming")]` with canonical paths.
  - `symlinked_roots_give_one_canonical_entry`: `a/superpowers` and `b/superpowers` are symlinks to `real/skills`; discovering `[a, b]` gives one entry per skill, whose path equals `real/skills/<name>/SKILL.md` canonicalized.
  - `missing_root_and_broken_entries_are_skipped`: a missing root, a dangling symlink and a skill folder without `SKILL.md` give an empty list, no panic.
  - `allowed_matches_group_or_name_and_drops_marketplace`: `["superpowers@official"]` allows a skill grouped under `superpowers`; `["foo"]` allows top-level `foo`; `["brainstorming"]` does not allow `superpowers/brainstorming`.
  - `empty_or_unmatched_allowlist_turns_instructions_off`: `[]` and `["nothing"]` both give exactly `["skills.include_instructions=false"]`.
  - `allowlist_disables_every_other_canonical_path`: found `foo`, `bar`, `superpowers/x`, `allow=["superpowers"]` give `["skills.bundled.enabled=false", "skills.config=[{path=\"<bar>\",enabled=false},{path=\"<foo>\",enabled=false}]"]` (sorted).
  - `an_allowed_path_is_never_disabled`: two found entries with the same path, one allowed, give no `skills.config`.
  - `everything_allowed_has_no_skills_config`: result is `["skills.bundled.enabled=false"]`.
  - `paths_are_toml_escaped`: for inputs with `"`, `\`, `\n`, a tab, and `é`, parsing `x = <toml_string(input)>` with the `toml` crate returns the input (add `toml` as a dev-dependency only if the crate lacks it).
- [ ] **Step 2: Run** `cargo test -j4 -p factory-engine codex_skills`. Expected: FAIL, module missing.
- [ ] **Step 3: Implement** the items above.
- [ ] **Step 4: Run** the same command. Expected: PASS.

### Task 2: `AgentSpec.codex_config`

**Files:**
- Modify: `crates/factory-agent/src/spec.rs`, `crates/factory-agent/src/codex.rs`

**Interfaces:**
- Produces: `AgentSpec.codex_config: Vec<String>`, empty from `AgentSpec::new`, doc comment "Extra `-c` values for the Codex runner. Claude ignores them." `codex_args` appends each as `-c <value>` after the sandbox and network values and before the resume id and the final `-`.

- [ ] **Step 1: Write failing tests** in `codex.rs`: `codex_config_values_become_c_flags_in_order` (values `a=1`, `b=2` appear as `-c a=1 -c b=2`, after `sandbox_mode=...`, before the final `-`); `codex_config_comes_before_the_resume_id` (with `resume = Some("sess")` the id is the last element before `-`); `no_codex_config_leaves_args_unchanged` (the existing arg expectations still hold).
- [ ] **Step 2: Run** `cargo test -j4 -p factory-agent codex`. Expected: FAIL to compile.
- [ ] **Step 3: Implement** the field and the append in `codex_args`.
- [ ] **Step 4: Run** `cargo test -j4 -p factory-agent`. Expected: PASS.

### Task 3: Wire the allowlist into `codex_prep`

**Files:**
- Modify: `crates/factory-engine/src/codex_prep.rs`, `crates/factory-engine/src/lib.rs`, `crates/factory-engine/src/compat.rs`, `crates/factory-cli/src/commands.rs`
- Test: `crates/factory-engine/tests/codex_prep.rs`

**Interfaces:**
- Port: `AgentRunner`. `prepare_for_codex` only shapes the `AgentSpec` that the runner receives; tests read the shaped spec from `FakeRunner`.
- Consumes: `discover_skills`, `codex_skill_args`, `allowed`, `FoundSkill` (Task 1); `AgentSpec.codex_config` (Task 2); `codex_user_skill_roots`, `codex_worktree_skills_dir` (Task 1); `Config::skill_allow_for` (plan 01, Task 1).
- Produces:
  - `Engine.codex_skill_roots: Vec<PathBuf>` (default empty) with `pub fn with_codex_skill_roots(self, roots: Vec<PathBuf>) -> Self`; the same builder on `Driver`, passed to each `Engine` it creates (`compat.rs` near line 448).
  - `pub(crate) fn codex_preface(role: &str, has_skills: bool) -> String`. Without skills: today's `PREFACE` text. With skills: "You run under Codex. Use the skills in your skill catalog when your instructions call for them. Do not start subagents. Where this text names a skill that is not in your catalog, follow the rules written in short in this text instead." Subagents stay forbidden in both. `SKILL_SENTENCE` leaves this function.
  - The allowlist is `Config::skill_allow_for(role)` from plan 01 (`plugins_for(role)` plus `dev-workflow-skills` for `committer` and `message`, `kb-refresh` for `kb`). Do not define a second one here; the probe and the run must agree on what a role may see.
  - `fn skill_gap(role: &str, found: &[FoundSkill], allow: &[String]) -> bool`: true when the role names a skill (above) and no found skill that is allowed belongs to it (group `dev-workflow-skills`, or top-level name `kb-refresh`). `prepare_for_codex` appends `SKILL_SENTENCE` to the preface only then.
- In `prepare_for_codex`: roots = `self.codex_skill_roots` plus `codex_worktree_skills_dir(<worktree root of spec.cwd, else spec.cwd>)` (use `self.repo.worktree_root_of`); `found = discover_skills(&roots)`; `allow = config.skill_allow_for(&spec.role)`; `spec.codex_config = codex_skill_args(&found, &allow)`; `has_skills` is true when some found skill is `allowed`. This runs for every role, also those without an agent file.
- In `commands.rs`, where the `Driver` is built, read `HOME` and `CODEX_HOME` once and pass `codex_user_skill_roots(&home, codex_home.as_deref())` to `with_codex_skill_roots`; with `HOME` unset pass an empty list. No other code reads these variables.

- [ ] **Step 1: Write failing tests.**
  - Unit, `codex_prep.rs`: `preface_with_skills_allows_the_catalog_and_forbids_subagents` (contains "skill catalog" and "Do not start subagents", not "Do not load or invoke skills"); `preface_without_skills_keeps_the_ban` (equals today's text); `skill_gap_is_true_only_for_a_missing_named_skill` (builder never; committer with an allowed `dev-workflow-skills` skill found is false, with none is true; kb with a top-level `kb-refresh` is false). `skill_allow_for` is tested in plan 01.
  - Integration, `tests/codex_prep.rs`, with `with_codex_skill_roots(vec![tmp/skills])` over temp skill folders:
    - `builder_with_superpowers_installed_gets_skills_and_the_allowlist`: root has `superpowers/tdd/SKILL.md` and `other/SKILL.md`; the builder spec has `codex_config == ["skills.bundled.enabled=false", "skills.config=[{path=\"<canonical other>\",enabled=false}]"]` and agent text without "Do not load or invoke skills".
    - `unmatched_allowlist_has_no_skills_preface`: no roots; the builder spec has `codex_config == ["skills.include_instructions=false"]` and the old ban text.
    - `reviewer_gets_no_skills`: default `reviewer_plugins = []` gives `skills.include_instructions=false`.
    - `committer_keeps_the_skill_sentence_only_when_the_commit_skill_is_missing`: without `dev-workflow-skills/1-git-commit/SKILL.md` the sentence is present; with it, absent.
    - `kb_with_kb_refresh_installed_has_no_skill_sentence`.
    - `committer_message_and_kb_get_the_skill_sentence` (existing, no roots) passes unchanged.
- [ ] **Step 2: Run** `cargo test -j4 -p factory-engine codex_prep`. Expected: FAIL.
- [ ] **Step 3: Implement** as listed. Keep `strip_frontmatter` and `sandbox_for` as they are.
- [ ] **Step 4: Run** `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli --test no_hardcoded_paths`, `cargo build -j4 -p factory-cli`, and `cargo clippy -j4 -p factory-engine -p factory-agent -p factory-core -p factory-cli`. Expected: all pass, no warnings.

#### Then: Docs

**Files:**
- Modify: `docs/reference/software-factory.md` (Using Codex), `docs/reference/project-toml-reference.md` (the four plugin-list rows, lines 190-193)

- [ ] **Step 1: Edit software-factory.md.** Replace the line "The agent file text and a short preface go into the prompt. No skill is loaded." with: the agent file text and a short preface go into the prompt; a Codex run lists the skills in `~/.agents/skills`, `$CODEX_HOME/skills` (default `~/.codex/skills`) and `<worktree>/.agents/skills`, only the catalog (name, description, path) is in context. The factory turns bundled skills off and disables, by canonical path, every skill the role is not allowed. The allowlist is the role's plugin list: an entry `superpowers` allows the skills in a `superpowers` subfolder, an entry `foo` allows a top-level `foo` folder, an empty list gives `skills.include_instructions=false`. Committer and message roles also get `dev-workflow-skills`, KB gets `kb-refresh`. Subagents stay off. When the named skill is not installed, the agent text keeps its short rules. Also name the `skills.*` flags next to the other `-c` flags.
- [ ] **Step 2: Edit project-toml-reference.md.** Append to each of the four plugin-list rows: "Under Codex the same list selects the skills the run sees (see Using Codex)." Each row stays on one line.
- [ ] **Step 3: Run** `grep -rn "No skill is loaded" crates docs` (expected: nothing) and `cargo test -j4 -p factory-cli --test no_hardcoded_paths` (expected: PASS).

## Maintainer steps after merge

Not part of the tasks. Plan 01 must merge before this plan: Task 3 uses its `Config::skill_allow_for`, and this step uses `factory probe`.

If the probe lists a skill that should be off, Codex found it in a root this plan does not scan (the 2026-10-30 probe showed superpowers twice, from two roots). Add that root to `codex_user_skill_roots` in a small follow-up.

1. With a real Codex login and superpowers installed for Codex, run `factory --project <codex project> probe --harness codex`.
2. Expected: the builder lists the superpowers skills and no bundled ones (`skills` and `no_extra_skills` present); the reviewer lists none; the committer lists `dev-workflow-skills` skills only; no user MCP tools in `TOOLS`.
3. Record the result as a finding in `docs/reference/software-factory.md` (Using Codex) and close #36 with the probe report path.

Next plan: docs/superpowers/plans/2026-10-30-harness-hardening/plan-03-tool-mapping.md
