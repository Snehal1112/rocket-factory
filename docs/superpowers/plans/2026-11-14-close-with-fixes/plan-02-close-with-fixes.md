# Close With Fixes, Plan 2: Intake Follow-ups and Docs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **Spec:** docs/superpowers/specs/2026-11-14-close-with-fixes-design.md
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** An intake job files its spec follow-ups before it labels its issue done, comments and closes it, and the docs state the new footer rule and default.

**Architecture:** The follow-up filing inside `tickets_step` becomes a public function that takes a tracker, so `run_intake` can call it with its own raw tracker while the guarded tracker stays for every other step. Intake adds one fixed line to its merged comment. Logic sits behind the existing `Tracker` port (`tracker.rs`); no new trait.

**Tech Stack:** Rust, `cargo test -j4 -p <crate>`.

## Global Constraints

- No agent text goes into an intake issue comment: only issue numbers and fixed words.
- A failure to file a follow-up never stops the merge, the relabel, the comment or the close.
- `GuardedTracker` and `raw_tracker` stay as they are. Only `intake.rs` may call `raw_tracker` (a source test pins it).
- `factory tickets <id>` on an intake job keeps skipping it.
- No `git` shell-outs, no `/proc` reads, no `Cargo.toml` change, no new board field.
- Use `-j4`, never `--workspace` or `--all`. Comments are short full sentences.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.

## Review Focus

1. A follow-up whose title holds text from an untrusted issue: the issue comment must not contain it. Pinned by `the_intake_comment_names_only_issue_numbers` in the intake task.
2. A two-plan intake series: follow-ups are filed once, after the last plan merged, and the not-filed line names the last job. Pinned by `a_two_plan_series_files_its_follow_ups_once` in the intake task.
3. A `gh` failure while filing: the issue is still labelled done and closed, and the comment says so. Pinned by `a_failed_follow_up_still_closes_the_intake_issue_and_says_so` in the intake task.
4. A rerun after a half-filed set adopts what exists and never files twice. Pinned by `file_follow_ups_adopts_a_pending_title` in the file_follow_ups task.
5. `tickets_step` for a normal job keeps its behaviour and writes one ledger line. Pinned by the existing `tests/tickets.rs` suite and `tickets_step_still_writes_one_ledger_line` in the file_follow_ups task.

## File Structure

- `crates/factory-engine/src/tickets.rs`: the new `file_follow_ups` and `FiledFollowUps`; `tickets_step` calls the shared part.
- `crates/factory-engine/src/lib.rs`: exports.
- `crates/factory-engine/src/intake.rs`: `run_intake` calls it after `RunEnd::Merged`.
- `crates/factory-engine/tests/tickets.rs`, `tests/intake.rs`: tests.
- Docs and `crates/factory-core/src/settings_block.rs`: wording only.

### Task 1: file_follow_ups as its own function

**Files:**
- Modify: `crates/factory-engine/src/tickets.rs` (`tickets_step` from line 143)
- Modify: `crates/factory-engine/src/lib.rs:118`
- Test: `crates/factory-engine/tests/tickets.rs`

**Interfaces:**
- Consumes: `Tracker` (`tracker.rs`), `Driver::job`, `Driver::job_dir`, `follow_ups(&str)` (`followups.rs:39`), `Marker`.
- Produces, exported from the crate root:

```rust
pub struct FiledFollowUps {
    pub report: TicketsReport,
    /// Issue numbers created or adopted for this job, in the order of the spec bullets.
    pub numbers: Vec<u32>,
    /// How many bullets the spec lists under `## Follow-ups`.
    pub listed: usize,
}
pub fn file_follow_ups(driver: &Driver, id: &str, tracker: &dyn Tracker, dry_run: bool)
    -> Result<FiledFollowUps, EngineError>
```

`file_follow_ups` applies the not-merged, not-last-plan, no-issue-reference and other-repository skips of `tickets_step`, copies the parent labels (with the filter from plan 1), files one ticket per bullet through the marker, and returns. It posts no comment and closes nothing. It writes one ledger line for a real run, as `finish` does. `tickets_step` keeps its intake skip, calls the shared part with `driver.tracker_for(&job)`, then comments and closes as today and writes exactly one ledger line.

- [ ] **Step 1: Read the code**

Read `tickets.rs` from 143 to the end of `tickets_step` and `tests/tickets.rs` 1-60, 98-120 and 507-560 (the pending-title tests).

- [ ] **Step 2: Write the failing tests in `tests/tickets.rs`**

Build each fixture with `merged(Some(SPEC_WITH_TWO), &t)` and `tracker()`.

```rust
#[test]
fn file_follow_ups_files_and_does_not_comment_or_close() {
    let t = tracker();
    let fx = merged(Some(SPEC_WITH_TWO), &t);
    let out = file_follow_ups(&fx.driver, &fx.id, &t, false).expect("file");
    assert!(!out.report.failed && !out.report.skipped, "{:?}", out.report);
    assert_eq!(out.numbers, vec![201, 202]);
    assert_eq!(out.listed, 2);
    let s = t.state.lock().expect("lock");
    assert_eq!(s.created.len(), 2);
    assert!(s.comments.is_empty() && s.closes.is_empty());
}
```

| Test | Setup | Assertions |
|---|---|---|
| `file_follow_ups_reports_zero_listed_for_a_spec_without_follow_ups` | `merged(Some(SPEC_NONE), &t)` | `numbers` empty, `listed == 0`, no create call |
| `file_follow_ups_adopts_a_pending_title` | seed the marker with `pending` holding `Shared name helpers` as `a_pending_entry_adopts_the_existing_issue` does, tracker `with_found("Shared name helpers", 150)` | `numbers` starts with 150, one create for the other bullet |
| `file_follow_ups_skips_a_job_that_is_not_the_last_plan` | series index 1 of 2 as `skips_a_job_that_is_not_the_last_plan` does | `report.skipped`, `numbers` empty, no create call |
| `tickets_step_still_writes_one_ledger_line` | `merged`, then `tickets_step` | the ledger holds exactly one line starting `tickets done` |

Add `file_follow_ups` to the `use factory_engine::{...}` line.

- [ ] **Step 3: Run the tests and see them fail**

Run: `cargo test -j4 -p factory-engine --test tickets file_follow_ups`
Expected: compile error `cannot find function file_follow_ups in crate factory_engine`.

- [ ] **Step 4: Implement the split**

Move the code of `tickets_step` between the skips and the line that builds `filed` into a private function shared by both callers, returning the marker, the parents, the items and the report. `file_follow_ups` builds `FiledFollowUps` from the marker's `created` list in spec order, and calls `finish`. `tickets_step` continues with the comment and close loop unchanged. Export `file_follow_ups` and `FiledFollowUps` from `lib.rs` next to `tickets_step`.

- [ ] **Step 5: Run the tests and see them pass**

Run: `cargo test -j4 -p factory-engine --test tickets`
Expected: every test passes, the new ones and all existing ones.

### Task 2: Intake files its follow-ups

**Files:**
- Modify: `crates/factory-engine/src/intake.rs:262-277` (the `RunEnd::Merged` arm of `run_intake`)
- Test: `crates/factory-engine/tests/intake.rs`

**Interfaces:**
- Consumes: `file_follow_ups(driver, id, tracker, dry_run) -> Result<FiledFollowUps, EngineError>` from the file_follow_ups task; the `tracker: &dyn Tracker` parameter `run_intake` already has; `jobs: Vec<String>`.
- Produces: the merged comment is `merged as job <ids>` followed by a newline and one fixed line. The line is `follow-ups filed as #a, #b` when `numbers.len() == listed` and `listed > 0`, `no follow-ups` when `listed == 0`, and `follow-ups not filed, see the spec of job <last>` in every other case, including an `Err` or a failed report. `<last>` is the last id of `jobs`.

- [ ] **Step 1: Read the code**

Read `intake.rs` 240-290, `tests/intake.rs` 1-110 (fixtures and the `one_plan` and `two_plans` drafts) and 399-432, and the failing-create setup of `rerun_after_a_failed_create_files_once` in `tests/tickets.rs:98`.

- [ ] **Step 2: Write the failing tests in `tests/intake.rs`**

Add the draft helpers below, next to `one_plan` and `two_plans`.

```rust
const SPEC_WITH_FOLLOW_UPS: &str = "# Spec: Demo\n\n## Goal\n\nBuild the demo so that a user can finish the task in one step.\n\n## Acceptance criteria\n\n- It is done.\n\n## Follow-ups\n\n- SECRET-TITLE other work. More detail.\n";

fn one_plan_with_follow_ups(draft: &Path) {
    std::fs::write(draft.join("spec.md"), SPEC_WITH_FOLLOW_UPS).expect("spec");
    std::fs::write(draft.join("plan-01-a.md"), plan(1, "none")).expect("plan");
}
```

`two_plans_with_follow_ups` is the same with `plan-01-a.md` and `plan-02-b.md` as in `two_plans`.

```rust
#[test]
fn a_merged_intake_job_files_its_follow_ups_before_the_close() {
    let it = setup(one_plan_with_follow_ups);
    it.tracker.state.lock().expect("lock").labels.insert(
        7, vec!["factory".into(), "priority: P0".into(), "tech-debt".into()]);
    assert!(matches!(pass(&it), IntakeOutcome::Merged { .. }));
    let calls = it.tracker.calls();
    let at = |p: &str| calls.iter().position(|c| c.starts_with(p)).expect("call");
    assert!(at("create") < at("close"), "{calls:?}");
    let s = it.tracker.state.lock().expect("lock");
    assert_eq!(s.created.len(), 1);
    assert_eq!(s.created[0].2, vec!["priority: P0".to_string(), "tech-debt".to_string()]);
    assert_eq!(s.closes, vec![7]);
}
```

| Test | Setup | Assertions |
|---|---|---|
| `the_intake_comment_names_only_issue_numbers` | `setup(one_plan_with_follow_ups)` | the second comment equals `merged as job 001\nfollow-ups filed as #201` and does not contain `SECRET-TITLE` |
| `a_merged_intake_job_without_follow_ups_says_so` | `setup(one_plan)` | the second comment equals `merged as job 001\nno follow-ups` |
| `a_failed_follow_up_still_closes_the_intake_issue_and_says_so` | `setup(one_plan_with_follow_ups)` with the tracker made to fail `create` as `rerun_after_a_failed_create_files_once` does | the second comment equals `merged as job 001\nfollow-ups not filed, see the spec of job 001`, `closes == [7]`, the last label edit is `(7, s(STATE_DONE), s(STATE_RUNNING))`, outcome is `Merged` |
| `a_two_plan_series_files_its_follow_ups_once` | `setup(two_plans_with_follow_ups)` | `created.len() == 1`, the second comment equals `merged as job 001, 002\nfollow-ups filed as #201` |

Update `a_merged_issue_is_commented_closed_and_labelled_done`: its assertion on `said[1].1` becomes `"merged as job 001\nno follow-ups"`.

- [ ] **Step 3: Run the tests and see them fail**

Run: `cargo test -j4 -p factory-engine --test intake merged`
Expected: the new tests fail on the comment text and on `created.len() == 0`.

- [ ] **Step 4: Implement the call**

In the `RunEnd::Merged` arm, before the relabel, call `crate::tickets::file_follow_ups(driver, last_id, tracker, false)` with `last_id = jobs.last()`. Build the fixed line from the result by the rule in the Interfaces block. Log an `Err` or a failed report with `tracing::error!(event = "step.error", step = "intake_follow_ups", ...)` and carry on. Post `format!("merged as job {ids}\n{line}")` in the existing comment call; the relabel and the close stay as they are and never depend on the filing result.

- [ ] **Step 5: Run the tests and see them pass**

Run: `cargo test -j4 -p factory-engine --test intake`, then `cargo test -j4 -p factory-engine`
Expected: all pass, including the source test that only `intake.rs` calls `raw_tracker`.

### Task 3: Docs state the new rules

**Files:**
- Modify: `docs/reference/software-factory.md` (footer table near 85-90, the merge-footer line near 138, the ticket step text near 134-137, the intake paragraph)
- Modify: `docs/reference/project-toml-reference.md:138`
- Modify: `docs/guides/getting-started.md:49,364,380`
- Modify: `docs/guides/factory-user-guide.md:667,1300`
- Modify: `README.md` (section Ticket hygiene, about 89-110)
- Modify: `crates/factory-core/src/settings_block.rs:87` only if its commented default text is fixed rather than read from `Config`
- Test: the factory-cli and factory-core suites

**Interfaces:**
- Consumes: the behaviour built in plan 1 and the intake task.
- Produces: docs that say the following, in the wording of each file.

- [ ] **Step 1: Read the docs**

Read each passage above and `grep -rn "Relates to\|tickets_on_merge" docs README.md` for any other mention.

- [ ] **Step 2: Edit the docs**

State exactly these facts and delete the ones they replace:

| Fact | Where |
|---|---|
| The merge footer is `Fixes: #N` on the last plan of a job (and on a single-plan job) and `Relates to: #N` on every earlier plan; the spec's follow-ups no longer change it | footer table, merge-footer line, README |
| Builder, fix and sync commits stay `Relates to` | footer table |
| `tickets_on_merge` defaults to true; `false` turns the step off; `getting-started.md:49` no longer calls `gh` optional for ticket hygiene, since the step now runs by default | `project-toml-reference.md:138`, `getting-started.md:364,380`, user guide 667 and 1300, README |
| The step files follow-ups first, then comments, then closes; `Fixes` also closes on push; a failed filing is a warning and `factory tickets <id>` retries | ticket step text, README |
| Intake files its spec follow-ups through its own call and adds a fixed line to the merged comment: `follow-ups filed as #a, #b`, `no follow-ups` or `follow-ups not filed, see the spec of job <id>`; follow-ups never carry `factory` or `factory: ...` labels; `factory tickets <id>` still skips intake jobs | intake paragraph, user guide intake section |
| Upgrade note: a project that never set `tickets_on_merge` now closes tickets after a merge | README and the user guide |

- [ ] **Step 3: Check the docs and the suites**

Run: `grep -rn "Relates to: #N when the spec has follow-ups\|default \`false\`" docs README.md`
Expected: no line that still states the old rules.
Run: `cargo test -j4 -p factory-core` and `cargo test -j4 -p factory-cli`
Expected: all pass.

- [ ] **Step 4: Lint and format the whole series**

Run: `cargo clippy -j4 -p factory-core -p factory-engine -p factory-cli`, then `cargo fmt -p factory-core -p factory-engine -p factory-cli -- --check`
Expected: no warnings, no diff.

Next plan: none
