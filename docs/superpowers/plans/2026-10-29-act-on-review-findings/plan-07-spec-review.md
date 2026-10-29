# Act On Review Findings, Plan 07: Spec Review Before The Build

> **Executor:** run with superpowers:subagent-driven-development: one fresh implementer per task, then a reviewer per task.

**Goal:** `factory new` refuses a spec that touches risky paths without a user decision, or that a read-only spec reviewer finds contradicts existing behaviour or relies on a list-based security design, unless the user accepts it with a reason.

**Architecture:** Two checks run inside `factory new` before any job is created: a pure check on the spec's decisions against the plans' risky paths, and one run of a new `factory-spec-reviewer` agent through the existing `AgentRunner`. The verdict is cached by a hash of the spec and plans. A refusal is an error; `--accept-spec "<reason>"` overrides it and records the reason.

**Tech Stack:** Rust, the `AgentRunner` port (`fake-claude` and scripted runners in tests), sha2 if already in the workspace (else a stable hash from an existing dependency).

**Spec:** `docs/superpowers/specs/2026-10-29-act-on-review-findings-design.md` (section 12)

## Global Constraints

- Cargo per crate with `-j4`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- The spec reviewer is read-only: tools exactly `Read, Grep, Glob`.
- A missing verdict is never a pass.
- No job, board entry or worktree is created when `factory new` refuses.
- `--accept-spec` reason: one line, not empty, at most 200 characters.
- `spec_review` defaults to `true`; the decisions check runs whatever its value.
- Run `cargo test -j4 -p factory-cli --test agent_lint` after editing any agent file.

## Review Focus

1. A spec with no `**Files:**` paths in its plans never trips the risky-decision check (`no_files_no_risky_check`, Task 1).
2. Changing one plan after an APPROVE runs the reviewer again (`a_changed_plan_is_reviewed_again`, Task 2).
3. The reviewer stops without a verdict (budget, crash): `factory new` refuses and says the review did not finish (`a_missing_verdict_refuses`, Task 2).
4. `--accept-spec` on a spec that passes both checks is accepted silently and records nothing (`accept_spec_on_a_clean_spec_records_nothing`, Task 2).
5. The supervisor's create endpoint returns 409 with the refusal text and queues nothing (`create_refusal_is_a_conflict`, Task 3).

## File Structure

- Create `crates/factory-engine/src/spec_check.rs`: both checks and the cache.
- Create `.claude/agents/factory-spec-reviewer.md` and `crates/factory-core/presets/{rust,ruby,node,csharp,go}/agents/factory-spec-reviewer.md`.
- Modify `crates/factory-engine/src/{runloop.rs,one_job.rs,specs.rs,lib.rs}`, `crates/factory-core/src/{config.rs,settings_block.rs,layout.rs}` (the `spec-reviews` folder under the project state).
- Modify `crates/factory-cli/src/{cli.rs,commands.rs}`, `crates/factory-cli/tests/agent_lint.rs` (known agents list and lint).
- Modify `crates/factory-supervisor/src/api/write.rs` (create endpoint).
- Modify `crates/factory-core/assets/global-command.md`, `docs/reference/software-factory.md`, `docs/guides/factory-user-guide.md`, `docs/reference/project-toml-reference.md`.

---

### Task 1: The risky-decision check

**Files:**
- Create: `crates/factory-engine/src/spec_check.rs`
- Modify: `crates/factory-engine/src/runloop.rs` (the series `new` path near line 475), `crates/factory-engine/src/one_job.rs` (near line 116), `crates/factory-engine/src/lib.rs`
- Test: `crates/factory-engine/src/spec_check.rs` tests, `crates/factory-cli/tests/` new-job tests

**Interfaces:**
- Consumes: `user_decisions(spec) -> usize` (`runloop.rs`), `classify_paths`, `Config.risk_paths`.
- Produces:
  - `pub fn plan_file_paths(plan: &str) -> Vec<String>`: the backticked paths on lines under each `**Files:**` block (lines starting `- Create:`, `- Modify:`, `- Test:`), with any `:line` suffix removed.
  - `pub fn risky_without_user_decision(spec: &str, plans: &[String], cfg: &Config) -> Option<String>`: `Some("the spec touches risky paths (<up to 5>) and no decision in it was answered by the user")` when any plan path is risky and `user_decisions(spec) == 0`.
  - `#[derive(Debug)] pub enum SpecRefusal { NoUserDecision(String), ReviewChanges { file: PathBuf, findings: String }, ReviewMissing { file: PathBuf } }` with a `Display` that the CLI prints.
  - `factory new` (both the series and the one-job path) calls the check before `add_job`; a refusal returns `EngineError::SpecRefused(SpecRefusal)` and creates nothing. The existing `NO_USER_DECISION` warning stays for specs that pass.

- [ ] **Step 1: Write the failing tests**

`plan_file_paths_reads_the_files_blocks` (a plan with `- Modify: \`src-tauri/src/lib.rs:12-40\`` gives `src-tauri/src/lib.rs`); `risky_spec_without_user_decision_is_refused`; `risky_spec_with_a_user_decision_passes`; `no_files_no_risky_check`. In the CLI tests: `new_refuses_a_risky_spec_without_a_user_decision` (non-zero, message contains `no decision in it was answered by the user`, `factory status` lists no new job).

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -j4 -p factory-engine spec_check` and the CLI test by name.
Expected: FAIL.

- [ ] **Step 3: Implement the check and wire it into both `new` paths**

- [ ] **Step 4: Run the tests**

Run: `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`
Expected: PASS. Existing fixtures whose specs touch risky paths gain a `Decided by: user` line.

---

### Task 2: The spec reviewer agent, its cache and `--accept-spec`

**Files:**
- Create: `.claude/agents/factory-spec-reviewer.md`, `crates/factory-core/presets/{rust,ruby,node,csharp,go}/agents/factory-spec-reviewer.md` (same text)
- Modify: `crates/factory-engine/src/spec_check.rs`, `crates/factory-engine/src/specs.rs` (`spec_reviewer_spec`)
- Modify: `crates/factory-core/src/config.rs`, `crates/factory-core/src/settings_block.rs`, `crates/factory-core/src/layout.rs`
- Modify: `crates/factory-cli/src/cli.rs` (`New` gains `#[arg(long)] accept_spec: Option<String>`), `crates/factory-cli/src/commands.rs`
- Modify: `crates/factory-cli/tests/agent_lint.rs`
- Test: `crates/factory-engine/tests/spec_review.rs` (new), `crates/factory-cli/tests/agent_lint.rs`

**Interfaces:**
- Produces:
  - The agent file: frontmatter `name: factory-spec-reviewer`, `tools: Read, Grep, Glob`, a description, the rules marker `<!-- factory-rules: 1 -->`. Body: it reads the spec and the plans named in the prompt and the code they touch, and reports only the three kinds of finding of spec section 12, each as `N. [blocker] <spec heading> - problem - evidence - fix` or `[minor]`, at most 40 lines, last line exactly `VERDICT: APPROVE` or `VERDICT: CHANGES`. Any `[blocker]` means CHANGES. A contradiction finding must cite `path:line`.
  - `Config.spec_review: bool` (default `true`), listed in `settings_block`.
  - `Layout` gains `pub fn spec_reviews_dir(&self) -> PathBuf` (`<state>/spec-reviews`).
  - `fn spec_reviewer_spec(&self, cwd: &Path, spec: &Path, plans: &[PathBuf]) -> AgentSpec` with role `spec-reviewer`, model `reviewer_model_large`, the agent file above.
  - `pub fn review_spec(driver: &Driver, spec: &Path, plans: &[PathBuf]) -> Result<(), SpecRefusal>`: the key is a hex hash of the spec text followed by each plan's text in order; when `<key>.md` exists and its last line is `VERDICT: APPROVE`, return `Ok`. Else run the agent in the main checkout, write `<key>.md`, and map the verdict: APPROVE `Ok`, CHANGES `ReviewChanges`, else `ReviewMissing`. Skipped when `spec_review` is false.
  - `factory new ... --accept-spec "<reason>"`: validated with `validate_one_line(reason, "accept-spec reason")` (plan 02), turns a refusal into a pass, and records `spec accepted by the user: <reason>` in the first job's ledger and in its `spec_warning`. With no refusal it records nothing.

- [ ] **Step 1: Write the failing tests**

In `tests/spec_review.rs` with a scripted runner for role `spec-reviewer`: `an_approved_spec_creates_the_jobs`; `a_changes_verdict_refuses_and_names_the_file` (no job created; the message contains the findings and the `spec-reviews/<key>.md` path); `a_missing_verdict_refuses`; `an_approved_spec_is_not_reviewed_twice` (second `new` makes no `spec-reviewer` call); `a_changed_plan_is_reviewed_again`; `spec_review_off_skips_the_agent`; `accept_spec_overrides_and_records_the_reason`; `accept_spec_on_a_clean_spec_records_nothing`.

In `agent_lint.rs`: add `factory-spec-reviewer.md` to the known agents; `spec_reviewer_is_read_only_and_pins_its_rules` for the root file and the five presets (tools exactly `Read, Grep, Glob`; contains `path:line`, `default-deny`, `Decided by: call`, both VERDICT lines alone).

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -j4 -p factory-engine --test spec_review` and `cargo test -j4 -p factory-cli --test agent_lint spec_reviewer`
Expected: FAIL.

- [ ] **Step 3: Write the agent files and implement the review, the cache and the flag**

- [ ] **Step 4: Run the tests**

Run: `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`, `cargo test -j4 -p factory-core`
Expected: PASS.

---

### Task 3: Interview command, supervisor and docs

**Files:**
- Modify: `crates/factory-core/assets/global-command.md` (step 4)
- Modify: `crates/factory-supervisor/src/api/write.rs` (create endpoint)
- Modify: `docs/reference/software-factory.md`, `docs/guides/factory-user-guide.md`, `docs/reference/project-toml-reference.md`
- Test: `crates/factory-cli/tests/agent_lint.rs` (pins `global-command.md`), `crates/factory-supervisor/src/api/write.rs` tests

**Interfaces:**
- Produces:
  - `global-command.md` step 4: after `factory new`, when it refuses, show the user the reason or findings and the review file, fix the spec with them (ask the user where a finding needs their decision, and record the answer as `Decided by: user`), and run `factory new` again; pass `--accept-spec "<reason>"` only when the user says so, with their reason.
  - The create endpoint maps a refused `factory new` child (its refusal exit and text) to 409 `conflict` with that text.

- [ ] **Step 1: Write the failing tests**

`agent_lint`: the global command contains `--accept-spec` and `Decided by: user` in step 4. Supervisor: `create_refusal_is_a_conflict` (a CLI double that refuses gives 409 with the text).

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -j4 -p factory-cli --test agent_lint` and `cargo test -j4 -p factory-supervisor create_refusal`
Expected: FAIL.

- [ ] **Step 3: Edit the command, the endpoint and the docs**

Docs: `software-factory.md` gets a "Spec check" subsection before the build stage; the user guide explains a refusal and `--accept-spec`; `project-toml-reference.md` gets `spec_review`.

- [ ] **Step 4: Run every touched suite and clippy**

Run: `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`, `cargo test -j4 -p factory-supervisor`, then `cargo clippy -j4 -p factory-core -p factory-engine -p factory-cli -p factory-supervisor --all-targets -- -D warnings`
Expected: all pass, no warnings.

Next plan: none
