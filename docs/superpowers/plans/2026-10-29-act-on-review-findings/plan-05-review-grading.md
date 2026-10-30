# Act On Review Findings, Plan 05: Grading, Minor Tickets And Agent Drift

> **Executor:** run with superpowers:subagent-driven-development: one fresh implementer per task, then a reviewer per task.

**Goal:** Real problems are graded `[blocker]`, the remaining `[minor]` findings become one ticket per job, and a project agent file that falls behind its preset is reported.

**Architecture:** Resolve compares each project or repo agent override with the preset file it replaces and adds a note on drift; notes are already printed by `factory project show` and on stderr by every command. The reviewer files get a stricter rubric and a re-check pass, and the rust preset builder runs cargo per crate. A minor-tickets step, built like the waiver step of plan 02, runs after every merge.

**Tech Stack:** Rust, the `Tracker` port.

**Spec:** `docs/superpowers/specs/2026-10-29-act-on-review-findings-design.md` (section 10)

**Depends on:** plan 01 (`last_review`, `open_blockers`), plan 02 (the waiver step and the `factory tickets` rendering it extends), plan 03 (the `[fixed]` tag).

## Global Constraints

- Cargo per crate with `-j4`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Run `cargo test -j4 -p factory-cli --test agent_lint` after editing any agent file.
- Agent drift is a note, never a problem: a project with drift still resolves with its full config.
- Minor tickets never change a merge result and are idempotent.
- `minor_tickets` defaults to `true`.
- Exact rubric text, used in every reviewer file:
  - `These are always [blocker]: a bug a user can hit, however small; a breach of any rule in the job prompt or the project rules; a missing test for an edge case or error path the code makes reachable; data loss or a leaked secret.`
  - `Before the verdict, re-read every [minor] against that list and change it to [blocker] when it matches.`
  (with backticks around each tag, as the files already write them)

## Review Focus

1. A project with no agent overrides gets no drift note (`no_override_no_drift`, Task 1).
2. An override with a higher rules marker than the preset is not drift (`a_newer_marker_is_not_drift`, Task 1).
3. A merged job whose last review has only `[fixed]` and `[blocker]` lines files no minor ticket (`no_open_minors_no_ticket`, Task 3).
4. `minor_tickets = false` files nothing, even from `factory tickets` (`minor_tickets_off_files_nothing`, Task 3).
5. A job in the middle of a series files its own minor ticket even though the tickets step skips it (`a_series_job_files_its_minor_ticket`, Task 3).

## File Structure

- Create `crates/factory-core/src/agent_drift.rs`: `rules_marker` (moved from `factory-engine/src/specs.rs`) and `agent_drift`.
- Modify `crates/factory-core/src/resolve.rs` (`merged_agents`), `crates/factory-core/src/lib.rs`, `crates/factory-engine/src/specs.rs` (import `rules_marker`).
- Modify `.claude/agents/factory-code-reviewer.md`, `crates/factory-core/presets/{rust,ruby,node,csharp,go}/agents/factory-code-reviewer.md`, `crates/factory-core/presets/rust/agents/factory-builder.md`, `crates/factory-cli/tests/agent_lint.rs`.
- Modify `crates/factory-core/src/{config.rs,settings_block.rs,verdict.rs}`.
- Create `crates/factory-engine/src/minors.rs`; modify `crates/factory-engine/src/{lib.rs,merge.rs}`, `crates/factory-cli/src/commands.rs` (`tickets`).
- Docs: `docs/reference/project-toml-reference.md`, `docs/reference/software-factory.md`, `docs/guides/factory-user-guide.md`.

---

### Task 1: Agent drift notes

**Files:**
- Create: `crates/factory-core/src/agent_drift.rs`
- Modify: `crates/factory-core/src/resolve.rs`, `crates/factory-core/src/lib.rs`, `crates/factory-engine/src/specs.rs`
- Test: `crates/factory-core/src/agent_drift.rs` tests, `crates/factory-core/src/resolve.rs` tests

**Interfaces:**
- Produces:
  - `pub fn rules_marker(text: &str) -> Option<u32>`, moved unchanged with its test from `factory-engine/src/specs.rs`; the engine imports it from `factory_core`.
  - `pub fn agent_drift(preset: &str, own: &str) -> Vec<String>`: `the rules marker` when `rules_marker(own)` is `None` or lower than `rules_marker(preset)` (and the preset has one); then `the section <Heading>` for each line starting `## ` in `preset` with no identical line in `own`, in preset order.
  - In `merged_agents`: before an override (project folder or the repo's `.factory/agents`) replaces a preset file, compare them; when `agent_drift` is not empty push the note `agent drift: <name> lacks <item>, <item> and <item>; compare it with the <preset> preset or remove the override`.

- [ ] **Step 1: Write the failing tests**

In `agent_drift.rs`: `drift_lists_the_marker_and_missing_sections` (preset with marker 1 and `## Stack`, `## Judging`; own with neither gives `["the rules marker", "the section Stack", "the section Judging"]`); `same_file_has_no_drift`; `a_newer_marker_is_not_drift` (own marker 2, preset 1).

In `resolve.rs` (fixture style of the test near line 604 that writes `project_dir("p").join("agents")`): `an_old_override_gives_a_drift_note` (an override without the marker gives a note starting `agent drift: factory-code-reviewer.md lacks the rules marker`, and `problems` stays empty); `no_override_no_drift`.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -j4 -p factory-core drift`
Expected: FAIL.

- [ ] **Step 3: Implement `agent_drift.rs`, move `rules_marker`, add the note**

- [ ] **Step 4: Run the tests**

Run: `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-engine`
Expected: PASS.

---

### Task 2: The grading rubric and the per-crate rust builder

**Files:**
- Modify: `.claude/agents/factory-code-reviewer.md`, `crates/factory-core/presets/{rust,ruby,node,csharp,go}/agents/factory-code-reviewer.md`
- Modify: `crates/factory-core/presets/rust/agents/factory-builder.md` (`## Stack`)
- Test: `crates/factory-cli/tests/agent_lint.rs`

**Interfaces:**
- Produces:
  - In each reviewer file's `## Judging`, after the existing grading line, the two rubric lines from Global Constraints.
  - In the rust preset reviewer's `## Stack`, the sentence `Blocking file, network or CPU-heavy work on an async runtime path is a [blocker].`
  - In the rust preset builder's `## Stack`, `Run cargo check -j4 -p <crate> and cargo test -j4 -p <crate> <test_name> for each crate you touched.` in place of the bare `cargo check -j4` instruction, and `Run cargo check -j4 -p <crate> for each crate you touched before you finish` in place of the bare warm-up line. Keep `-j4`, the `--workspace`/`--all` ban and the `CARGO_TARGET_DIR` ban.

- [ ] **Step 1: Write the failing lint tests in `agent_lint.rs`**

`reviewer_files_carry_the_grading_rubric`: for the root file and all five presets, require `a bug a user can hit`, `a breach of any rule`, `a missing test for an edge case or error path`, `re-read every`. `rust_reviewer_blocks_blocking_async_work`: the rust preset reviewer contains `async runtime path`. `rust_builder_runs_cargo_per_crate`: the rust preset builder contains `cargo check -j4 -p <crate>` and has no line containing `` `cargo check -j4` `` without `-p`.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -j4 -p factory-cli --test agent_lint grading_rubric async per_crate`
Expected: FAIL.

- [ ] **Step 3: Edit the six reviewer files and the rust builder**

- [ ] **Step 4: Run the lint suite**

Run: `cargo test -j4 -p factory-cli --test agent_lint`
Expected: PASS, including `skill_drift` if it compares preset copies.

---

### Task 3: Minor findings become tickets

**Files:**
- Modify: `crates/factory-core/src/config.rs`, `crates/factory-core/src/settings_block.rs`, `crates/factory-core/src/verdict.rs`
- Create: `crates/factory-engine/src/minors.rs`
- Modify: `crates/factory-engine/src/lib.rs`, `crates/factory-engine/src/merge.rs`, `crates/factory-cli/src/commands.rs` (`tickets`)
- Modify: `docs/reference/project-toml-reference.md`, `docs/reference/software-factory.md`, `docs/guides/factory-user-guide.md`
- Test: `crates/factory-core/src/verdict.rs` tests, `crates/factory-engine/tests/minors.rs` (new)

**Interfaces:**
- Consumes: `Job.last_review` (plan 01); `Tracker::create`, `TicketsReport`, `tickets::notify_failure`; the waiver step's place in `merge_stage` and in `factory tickets` (plan 02).
- Produces:
  - `Config.minor_tickets: bool` (default `true`), listed in `settings_block::GROUPS` next to `tickets_on_merge`.
  - `pub fn open_minors(review_text: &str) -> Vec<String>` in `verdict.rs`: trimmed lines containing `[minor]` and not `[fixed]`, in order.
  - `pub const MINORS_FILE: &str = "minors.json";` holding `{"issue": <number>}` once filed.
  - `pub fn minor_tickets_step(driver: &Driver, id: &str, dry_run: bool) -> Result<TicketsReport, EngineError>`: skips (report `skipped`) when `minor_tickets` is false, the job is not `merged`, `minors.json` already has an issue, or `open_minors` of `last_review.file` is empty. Else creates one issue titled `Minor review findings from job {id}: {slug}` whose body is a `- [ ] <line>` checklist, then the review file name, the job id and the merge sha, with no labels, and writes `minors.json`. It does not apply the series gate of `tickets_step`.
  - `merge_stage` runs it after the waiver step, with the same failure handling (`step = "minors"`). `factory tickets <id>` prints its report after the waiver report.

- [ ] **Step 1: Write the failing tests**

In `verdict.rs`: `open_minors_skips_fixed_and_blockers`.

In `tests/minors.rs` (`FakeTracker`, merged-job helpers):
- `a_merged_job_files_one_minor_ticket`: a last review with two `[minor]` lines and one `[minor] [fixed]` line gives one `create` with the title prefix `Minor review findings from job` and a body with exactly two `- [ ]` lines; `minors.json` holds the number.
- `minor_tickets_are_idempotent`: a second run makes no `create` call.
- `no_open_minors_no_ticket`.
- `minor_tickets_off_files_nothing`: `minor_tickets = false` makes no call from the step or from `factory tickets`.
- `a_series_job_files_its_minor_ticket`: a job at series position 1 of 3 still files.
- `a_tracker_failure_keeps_the_merge`.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -j4 -p factory-core open_minors` and `cargo test -j4 -p factory-engine --test minors`
Expected: FAIL.

- [ ] **Step 3: Implement the setting, `open_minors`, `minors.rs` and the two call sites**

Add the `minor_tickets` row to `project-toml-reference.md` and a short paragraph on minor tickets to `software-factory.md` (merge section) and the user guide's tickets section.

- [ ] **Step 4: Run every touched suite and clippy**

Run: `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`, then `cargo clippy -j4 -p factory-core -p factory-engine -p factory-cli --all-targets -- -D warnings`
Expected: all pass, no warnings.

- [ ] **Step 5: Replay check (manual, after merge and install, not part of CI)**

For each of Rocket jobs 033, 040 and 045, run the installed rust preset reviewer on the saved round diff in `~/.factory/projects/rocket/state/jobs/<job>/review-*.diff` with that job's `spec.md` and `plan.md`, from a Rocket checkout at the job's reviewed head. Expected: 033's stale report on reopen, blocking I/O on the async runtime and camelCase on non-IPC types, 040's clones on every send, and 045's missing clash test come back as `[blocker]`. Record the result in `docs/roadmap/quality-investigation-2026-10-29.md`. Then remove Rocket's builder override (`~/.factory/projects/rocket/agents/factory-builder.md`, backup in `agents-backup-2026-10-29/`) and confirm `factory project show rocket` shows no drift note and `problems: none`.

Next plan: docs/superpowers/plans/2026-10-29-act-on-review-findings/plan-06-check-coverage.md
