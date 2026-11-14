# Cancel Stops Driver, Plan 2: Docs Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-14-cancel-stops-driver-design.md
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** The reference doc and the user guide describe that a cancelled job never reaches review or merge, how needs-human is left, and the window of milliseconds inside the git merge.

**Architecture:** Docs only. The behaviour comes from the previous plan; this plan states it where an operator reads about `factory cancel`.

**Tech Stack:** Markdown, `cargo test -j4 -p factory-cli` for the doc tests.

## Global Constraints

- Docs go in `docs/`, not `.claude/`. Prose is short and plain.
- No code change in this plan. No `Cargo.toml` change.
- Use `-j4`, never `--workspace` or `--all`.

## Review Focus

1. The docs must not promise that a cancel stops the live driver's agent at once: it ends at the next stage write. Pinned by the checks in the docs task (a grep for "at once" in the changed paragraphs).
2. The millisecond window inside the git merge must be stated where `factory cancel` is described. Pinned by the docs task's grep for `window`.
3. Doc tests that read the docs (`agent_lint.rs`, `check_coverage_docs.rs`, `skill_drift.rs`) keep passing. Pinned by the docs task's last step.

## File Structure

- `docs/reference/software-factory.md`: the stage flow near line 23 and the `/factory cancel` row near line 261.
- `docs/guides/factory-user-guide.md`: section 6.7 `factory cancel` near line 490, and the `was cancelled` row near line 461.

### Task 1: Describe the cancel behaviour in the docs

**Files:**
- Modify: `docs/reference/software-factory.md`
- Modify: `docs/guides/factory-user-guide.md`

**Interfaces:**
- Consumes: the behaviour of the previous plan: a cancelled job refuses every stage move except to needs-human; a running driver ends the run as needs-human with the cancel reason; `try_merge` checks the board right before the git merge; needs-human is left only through rework, approve or sync.
- Produces: the paragraphs below, in the two files.

- [ ] **Step 1: Read the docs and the spec**

Read `docs/superpowers/specs/2026-11-14-cancel-stops-driver-design.md` (Design and Error handling), `software-factory.md` lines 15-30 and 255-266, and `factory-user-guide.md` lines 455-500.

- [ ] **Step 2: Write the text**

In `factory-user-guide.md` section 6.7 add a short paragraph with these facts, in this order: `--force` cancels a job whose driver is still running; the driver is not stopped, it ends at its next stage move; the cancelled job never moves to review, approve or merged, and the run ends as `needs-human: cancelled: <reason>`; the work already done stays in the worktree; `rework` revives the job and `approve` and `sync` refuse it. State the window: a cancel that lands during the git merge itself, which takes milliseconds, cannot stop that merge; main then has the merge while the job stays cancelled, and a person can revert it like any merge. In `software-factory.md` extend the `/factory cancel` row with one sentence on the same behaviour, and add one sentence near the stage flow that a job in needs-human is left only through rework, approve or sync.

- [ ] **Step 3: Check the text**

Run: `grep -n "window" docs/guides/factory-user-guide.md docs/reference/software-factory.md` and `grep -n "at once" docs/guides/factory-user-guide.md`
Expected: the first shows the window sentence in the guide; the second shows no promise that the cancel stops the driver at once in the new paragraph.

- [ ] **Step 4: Run the doc tests**

Run: `cargo test -j4 -p factory-cli --test check_coverage_docs`, `cargo test -j4 -p factory-cli --test agent_lint`, `cargo test -j4 -p factory-cli --test skill_drift`
Expected: all pass.

Next plan: none
