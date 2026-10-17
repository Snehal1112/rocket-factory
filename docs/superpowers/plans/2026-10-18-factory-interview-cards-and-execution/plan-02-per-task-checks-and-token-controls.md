# Per-task checks and token controls Implementation Plan

> **For agentic workers:** This plan is executed by the factory driver using `superpowers:subagent-driven-development`: one fresh `factory-builder` implementer per task, no per-task review, and the driver does the ledger, commits and the final review. Do not commit. Steps use checkbox syntax for tracking.

**Goal:** Record check results against each task, slim the builder and reviewer agent starts, and take every turn cap from one place.

**Architecture:** A new `run_checks_between` runs the mapped checks over one task's diff and writes `checks-task{n}.md`. The result is a ledger line only, so fix rounds and budgets are unchanged. Builder and reviewer specs get `strict_mcp` and a tool list like the committer. Turn caps become public constants that a test compares with every agent file's `maxTurns`.

**Tech Stack:** Rust, cargo with `-j4`, `factory-checks`, `factory-agent` (`AgentSpec`, `fake-claude`).

**Spec:** `docs/superpowers/specs/2026-10-18-factory-interview-cards-and-execution-design.md` (Design 3 per-task checks, Design 4).

## Global Constraints

- Every cargo call has `-j4`. Never `--workspace`, `--all` or `CARGO_TARGET_DIR`.
- Never shell out to `git`. Paths reach the driver only through `Layout`.
- Per-task checks never start a fix round, never park, and never change `fix_rounds` or any budget.
- The per-task result file is `checks-task{n}.md`, never `checks.md` (the review stage reads `checks.md`).
- No commit steps in any task. The driver commits.

## Review Focus

- A task whose diff maps to no check command must write no file and no failure line.
- A check that times out in a task must be recorded as a failure line, and the job must still reach the Review stage.
- `commit_batch` mode (task is only a `wip:` snapshot) and non-batch mode must both produce `checks-task{n}.md` for the right range.
- The builder tool list must still include every tool the agent file tells the builder to use (Bash, Read, Edit, Write, Grep, Glob, Skill, Agent).
- An agent file with a `maxTurns` that differs from the constant must fail the test with the file path in the message.

---

### Task 1: Record checks per task

**Files:**
- Modify: `crates/factory-engine/src/checks_stage.rs`, `crates/factory-engine/src/build.rs`
- Test: `crates/factory-engine/tests/build_checks.rs`

**Interfaces:**
- Produces: `pub(crate) fn run_checks_between(&self, from: &str, to: &str, out: &str) -> Result<Option<ChecksReport>, EngineError>`: `changed_paths(from, to)`, `plan_checks`, `run_checks` in the worktree, writes `<job_dir>/<out>`; returns `None` when the plan has no commands.
- Consumes: `Repo::changed_paths(base, head)`, `plan_checks`, `run_checks`, `write_checks_md`, `ledger`/`ruling`.

- [ ] **Step 1: Write the failing tests** in `build_checks.rs`: `each_task_writes_a_checks_file_for_its_own_range` (two-task fixture with a passing check; assert `checks-task1.md` and `checks-task2.md` exist and name different changed files), `a_failing_task_check_is_a_ledger_line_not_a_park` (check command `false` for task 1; assert ledger has `task 1 checks: fail` and the job reaches the next task, `task_index == 1`), `a_task_with_no_mapped_checks_writes_nothing`. Run `cargo test -j4 -p factory-engine --test build_checks each_task a_failing_task a_task_with_no` and confirm failure.
- [ ] **Step 2: Implement** `run_checks_between` and call it in `build_task` after the new tip is known and before `ImplementerDone` is written, passing the previous tip and the new tip; write `task N checks: pass` or `task N checks: fail <first failing command>` through `ruling()`.
- [ ] **Step 3: Verify.** Run `cargo test -j4 -p factory-engine`. Expected: pass, and the existing round-count tests are unchanged.

### Task 2: Slimmer agent starts and one source for turn caps

**Files:**
- Modify: `crates/factory-engine/src/specs.rs` (builder and reviewer specs, constants), `crates/factory-engine/src/commit.rs`, `crates/factory-engine/src/kb.rs` (constants)
- Create: `crates/factory-core/presets/legacy/{rust,go}-factory-builder.md`, `crates/factory-core/presets/legacy/{rust,go}-factory-code-reviewer.md` (unchanged copies of today's preset files)
- Modify: `.claude/agents/factory-builder.md`, `crates/factory-core/presets/{rust,go}/agents/factory-builder.md` (`maxTurns` only)
- Test: `crates/factory-engine/tests/build_checks.rs`, `crates/factory-engine/tests/review_decide.rs`, `crates/factory-cli/tests/agent_lint.rs`

**Interfaces:**
- Produces: `pub const BUILDER_MAX_TURNS: u32 = 120`, `pub const REVIEWER_MAX_TURNS: u32 = 40`, `pub const BUILDER_TOOLS: &str = "Bash,Read,Edit,Write,Grep,Glob,Skill,Agent"` in `specs.rs`, re-exported from the engine crate.
- Produces: builder specs (`builder_first_spec`, `builder_task_spec`, `builder_resume_spec`) with `strict_mcp = true`, `tools = Some(BUILDER_TOOLS)`; reviewer spec with `strict_mcp = true` and `max_turns = Some(REVIEWER_MAX_TURNS)`.

- [ ] **Step 0: Keep the old texts first.** Before editing any preset agent file, copy each current `crates/factory-core/presets/{rust,go}/agents/factory-builder.md` and `factory-code-reviewer.md` unchanged to `crates/factory-core/presets/legacy/<preset>-<file name>` (four files, still saying `maxTurns: 150` for the builder). Plan 05 Task 3 embeds them as the known-old texts for `factory presets refresh`. Nothing embeds them in this plan.
- [ ] **Step 1: Write the failing tests.** `build_checks.rs`: `builder_specs_are_strict_and_name_their_tools` (all three builder specs: `strict_mcp`, `tools == BUILDER_TOOLS`, `max_turns == Some(120)`). `review_decide.rs`: `the_reviewer_spec_is_strict_and_capped`. `agent_lint.rs`: `agent_max_turns_match_the_driver_constants` (read `maxTurns` from `.claude/agents/factory-builder.md`, both preset builders, `factory-code-reviewer.md` in-repo and presets; assert builder files equal `BUILDER_MAX_TURNS`, reviewer files equal `REVIEWER_MAX_TURNS`, message names the file). Run them and confirm failure (builder files say 150).
- [ ] **Step 2: Implement** the constants and the spec changes; set `maxTurns: 120` in the three builder agent files. Update the `Some(120)` assertion at `build_checks.rs` if it moved.
- [ ] **Step 3: Verify the flags reach the command line.** Add `factory-agent/tests/stream.rs` coverage `builder_args_carry_strict_mcp_and_tools` that builds an `AgentSpec` with the same settings and asserts `--strict-mcp-config` and `--tools Bash,...` appear in `args()`.
- [ ] **Step 4: Verify.** Run `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-agent`, `cargo test -j4 -p factory-cli --test agent_lint`. Expected: pass.

Next plan: docs/superpowers/plans/2026-10-18-factory-interview-cards-and-execution/plan-03-draft-cards-driver.md
