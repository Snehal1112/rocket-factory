# Close With Fixes, Plan 1: Footer and Default Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **Spec:** docs/superpowers/specs/2026-11-14-close-with-fixes-design.md
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** The merge of the last plan says `Fixes: #N`, earlier plans say `Relates to: #N`, the ticket step runs by default, and follow-ups never inherit intake labels.

**Architecture:** `MessageRequest.partial` becomes `closes_ticket`, set from the job's series position in `Driver::commit_message`. `Config.tickets_on_merge` flips its default to true. The label copy in `tickets_step` drops the intake labels. Logic sits behind the existing `Tracker` port (`tracker.rs`); no new trait.

**Tech Stack:** Rust, `cargo test -j4 -p <crate>`.

## Global Constraints

- Builder, fix and sync commits keep `Relates to`. Only a merge on the last plan says `Fixes`.
- No agent text goes into an intake issue comment. No `git` shell-outs, no `/proc` reads, no `Cargo.toml` change.
- Board JSON stays compatible with the dashboard. No new board field.
- `GuardedTracker` and `raw_tracker` stay as they are.
- Use `-j4`, never `--workspace` or `--all`. Comments are short full sentences.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.

## Review Focus

1. A series where plan 1 of 3 merges with a spec that has no follow-ups: the footer must be `Relates to`, not `Fixes`. Pinned by `plan_one_of_three_says_relates_to_even_without_follow_ups` in the footer task.
2. A custom `intake_label` (not `factory`) on the parent, and a parent label in another case such as `Factory`: both must be dropped too. Pinned by `a_custom_intake_label_is_dropped_from_follow_up_labels` in the default task.
3. A project that sets `tickets_on_merge = false` explicitly keeps the step off. Pinned by `with_the_key_off_no_merge_calls_the_tracker` (existing, must keep passing) in the default task.
4. Every existing merge test whose feature text cites an issue must still pass with the new default. Pinned by the full-suite run in the default task.
5. A merge with no issues gets no issue footer on any plan. Pinned by the existing `a_job_without_issues_gets_only_the_job_footer` in the footer task.

## File Structure

- `crates/factory-engine/src/message.rs`: `MessageRequest`, `driver_footers`, unit tests. `spec_has_follow_ups` is deleted.
- `crates/factory-engine/src/compat.rs`: `Driver::commit_message` sets `closes_ticket`.
- `crates/factory-engine/src/tickets.rs`: the label copy in `tickets_step`.
- `crates/factory-core/src/config.rs`, `resolve.rs`: the default and its tests.
- `crates/factory-engine/tests/message.rs`, `tests/tickets.rs`: integration tests.

### Task 1: Merge footer follows the last plan

**Files:**
- Modify: `crates/factory-engine/src/message.rs` (struct at 40-47, `spec_has_follow_ups` at 50-55, `driver_footers` at 65-76, tests from 208)
- Modify: `crates/factory-engine/src/compat.rs:576-607`
- Test: `crates/factory-engine/tests/message.rs`

**Interfaces:**
- Consumes: `Job.series: Option<SeriesPos>` with `index` and `total` (`factory-core/src/board.rs:202`), `Job.issues: Vec<u32>`.
- Produces: `MessageRequest { ..., issues: &[u32], closes_ticket: bool }` (the `partial` field is removed). `driver_footers` gives a merge `Fixes` when `closes_ticket` and `Relates to` otherwise, and a sync always `Relates to`.

- [ ] **Step 1: Read the code**

Read `message.rs` 40-80 and its tests, `compat.rs` 570-610, `tickets.rs:153-158` (the last-plan test this task copies), and `tests/message.rs` (the `with_issues` helper at 145 and the footer tests from 155).

- [ ] **Step 2: Write the failing unit tests in `message.rs`**

Replace `merge_footer_relates_to_when_the_spec_has_follow_ups` and `spec_has_follow_ups_reads_spec_md_of_the_job_folder` with the three tests below, built from the request literal of the old test with `closes_ticket` in place of `partial`.

```rust
#[test]
fn a_merge_that_closes_the_ticket_says_fixes() {
    let req = MessageRequest { kind: MessageKind::Merge, job_id: "001", slug: "s", title: "t",
        base: "a", tip: "b", issues: &[76], closes_ticket: true };
    assert_eq!(driver_footers(&req), vec!["Fixes: #76".to_string(), "Factory-Job: 001".to_string()]);
}
```

| Test | Input | Expected footers |
|---|---|---|
| `a_merge_that_does_not_close_the_ticket_says_relates_to` | Merge, `closes_ticket: false`, issues `[76]` | `Relates to: #76`, `Factory-Job: 001` |
| `a_sync_is_always_relates_to` | Sync, `closes_ticket: true`, issues `[76]` | `Relates to: #76`, `Factory-Job: 001` |

In the other unit tests that build a `MessageRequest`, replace `partial: false` with `closes_ticket: true` so each keeps its expected `Fixes` text.

- [ ] **Step 3: Run the tests and see them fail**

Run: `cargo test -j4 -p factory-engine --lib message`
Expected: compile error `struct MessageRequest has no field named closes_ticket`.

- [ ] **Step 4: Implement the field and the footer rule**

In `message.rs` rename the field to `pub closes_ticket: bool` with the doc line "True when this merge is the last plan of its job, so the footer says `Fixes`." Change `driver_footers` to `MessageKind::Merge if req.closes_ticket => "Fixes"`, `MessageKind::Merge => "Relates to"`, `MessageKind::Sync => "Relates to"`. Delete `spec_has_follow_ups` and update the doc comment of `driver_footers`. In `compat.rs` set `closes_ticket: job.series.as_ref().map_or(true, |pos| pos.index >= pos.total)`, the same test as `tickets.rs:153`.

- [ ] **Step 5: Write the failing integration tests in `tests/message.rs`**

Build each fixture as `merge_message_ends_with_the_fixes_and_job_footers` does (`approved_job_with(Arc::new(scripted(vec![reply(GOOD)])), true)`, `with_issues(&fx, vec![20])`). A helper `set_series(fx, index, total)` sets `j.series = Some(factory_core::SeriesPos { plan_file: "p".into(), index, total, id: None })` through `store().update_job`, as `skips_a_job_that_is_not_the_last_plan` in `tests/tickets.rs` does. A constant `SPEC_WITH_FOLLOW_UPS` is `"## Goal\nsomething with enough words to pass the check here\n\n## Follow-ups\n\n- More work\n\n## Acceptance criteria\n- a\n"`, written with `common::set_feature(&fx, "x #20", Some(SPEC_WITH_FOLLOW_UPS))` followed by `with_issues(&fx, vec![20])`.

| Test | Series | Spec | Assert on the merge message |
|---|---|---|---|
| `the_last_plan_says_fixes_even_with_follow_ups` | index 3 of 3 | `SPEC_WITH_FOLLOW_UPS` | ends with `Fixes: #20\nFactory-Job: 001` |
| `plan_one_of_three_says_relates_to_even_without_follow_ups` | index 1 of 3 | none | ends with `Relates to: #20\nFactory-Job: 001` |
| `a_single_plan_job_with_follow_ups_says_fixes` | none | `SPEC_WITH_FOLLOW_UPS` | ends with `Fixes: #20\nFactory-Job: 001` |

- [ ] **Step 6: Run the tests and see them pass**

Run: `cargo test -j4 -p factory-engine --test message` then `cargo test -j4 -p factory-engine --lib message`
Expected: all pass, including `merge_message_ends_with_the_fixes_and_job_footers`, `the_fixed_merge_message_gets_the_footers_too`, `a_job_without_issues_gets_only_the_job_footer` and `sync_message_uses_relates_to_then_the_job_footer`.

### Task 2: The ticket step runs by default and drops intake labels

**Files:**
- Modify: `crates/factory-core/src/config.rs:128-129,246,1272-1280`
- Modify: `crates/factory-core/src/resolve.rs:423-435`
- Modify: `crates/factory-engine/src/tickets.rs:196-208`
- Test: `crates/factory-engine/tests/tickets.rs`

**Interfaces:**
- Consumes: `Config.intake_label: String` (default `factory`) and the matching rule in `pick_next`, `crates/factory-engine/src/intake.rs:137-146`.
- Produces: `Config::default().tickets_on_merge == true`. Follow-up labels exclude `tracking`, any label equal to `cfg.intake_label` ignoring ASCII case, and any label starting with `factory:` ignoring ASCII case, for every job.

- [ ] **Step 1: Read the code**

Read `config.rs` 124-130 and 240-250, the two tests named above, `tickets.rs` 143-210, and `tests/tickets.rs` 1-60 and 268-310 (the `approved_with_hook` fixture).

- [ ] **Step 2: Write the failing tests**

In `config.rs` rename `tickets_on_merge_is_off_by_default_and_can_be_turned_on` to `tickets_on_merge_is_on_by_default_and_can_be_turned_off`: `assert!(Config::default().tickets_on_merge)`, `assert!(!Config::parse("tickets_on_merge = false").expect("parse").tickets_on_merge)`, and the existing `"yes"` rejection. In `resolve.rs` the first assert of `tickets_on_merge_is_a_valid_project_setting` becomes true by default, and the settings text sets `tickets_on_merge = false` and asserts the opposite.

In `tests/tickets.rs`:

```rust
#[test]
fn a_merge_with_the_default_config_runs_the_ticket_step() {
    let t = tracker();
    let fx = with_tracker(
        common::approved_job_config(common::approving_runner(3), false, Config::default()),
        t.clone(),
    );
    set_feature(&fx, "dedup cleanups #76", Some(SPEC_WITH_TWO));
    assert!(matches!(merge_stage(&fx.driver, &fx.id).expect("merge"), MergeResult::Merged(_)));
    let s = t.state.lock().expect("lock");
    assert_eq!(s.created.len(), 2);
    assert_eq!(s.closes, vec![76]);
}
```

| Test | Parent #76 labels and config | Expected created labels |
|---|---|---|
| `follow_up_labels_drop_the_intake_label_and_state_labels` | `Factory`, `Factory: Running`, `factory: needs-human`, `FACTORY: DONE`, `tracking`, `tech-debt`; default config | `["tech-debt"]` for each created ticket |
| `a_custom_intake_label_is_dropped_from_follow_up_labels` | `bot`, `tech-debt`; `Config { intake_label: "bot".into(), ..Config::default() }` | `["tech-debt"]` |

Build the fixtures as `approved_with_hook` does and call `tickets_step(&fx.driver, &fx.id, false)` after the merge with `tickets_on_merge: false` so the step runs once, by hand.

- [ ] **Step 3: Run the tests and see them fail**

Run: `cargo test -j4 -p factory-core tickets_on_merge` and `cargo test -j4 -p factory-engine --test tickets`
Expected: the default tests fail on the old default, and the label tests fail because `factory` is still copied.

- [ ] **Step 4: Implement the default and the label filter**

In `config.rs` set the default to `true` and change the doc line to "After a merge, file the follow-up tickets and close the parent tickets. On by default; set false to turn it off." In `tickets.rs` the label loop skips a label that equals `"tracking"`, one that equals `driver.config().intake_label` with `eq_ignore_ascii_case`, and one whose lowercase form starts with `factory:`. This is the rule `pick_next` uses at `intake.rs:137-146`; keep no list of exact names.

- [ ] **Step 5: Run the crate suites**

Run: `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`
Expected: all pass. A test that fails only because the step now runs sets `tickets_on_merge: false` explicitly, and the step makes no tracker call when the feature has no issue reference.

- [ ] **Step 6: Lint and format**

Run: `cargo clippy -j4 -p factory-core -p factory-engine -p factory-cli`, then `cargo fmt -p factory-core -p factory-engine -p factory-cli -- --check`
Expected: no warnings, no diff.

Next plan: docs/superpowers/plans/2026-11-14-close-with-fixes/plan-02-close-with-fixes.md
