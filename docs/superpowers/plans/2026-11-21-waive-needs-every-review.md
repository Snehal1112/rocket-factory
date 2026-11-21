# Waive Needs Every Review Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** No approve, with any flag, merges a job that has open code-review blockers while a required review role has no verdict (issue #117).

**Architecture:** One new refusal in `Job::approve_refusal` (factory-core), which the CLI, the factoryd API and retry already share. The dashboard drops its approve chip and Approve button for such a job. Docs get the new refusal row.

**Tech Stack:** Rust (factory-core, factory-cli, factory-supervisor tests), plain JS dashboard with `node --test`.

**Spec:** `docs/superpowers/specs/2026-11-21-waive-needs-every-review-design.md`

## Global Constraints

- Condition: `changes_open() && review_missing()`. Refused whatever `without_review` and `waive` say.
- The refusal names the open blockers (count, round, file) and the missing roles (`missing_reviews()` joined with " and "), offers `rework_hint()` and cancel, and contains neither `--waive` nor `--without-review`.
- Message shape: `job 001 has 3 open review blockers from round 2 (review-code-r2.md) and no security review yet: use factory retry 001, or cancel`.
- The check sits after the series-waiting check and before both existing `changes_open` checks.
- No override: `--waive` and `--without-review` stay mutually exclusive. Jobs without open changes behave as today.
- cargo: always `-j4`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Dashboard JS tests run with `cd dashboard && node --test`, as the repo does today.

## Review Focus

- Open changes with security `pending` (not only `failed`) must refuse: a parked job whose security reviewer never started. Pinned in Task 1's core test loop.
- Open changes with code `Changes` and no `security` key at all must still allow `--waive` (a job without a security role). Pinned in Task 1.
- The factoryd API answers this refusal as 409 `conflict`, like the other refusals, not 400. Pinned in Task 1's supervisor test.
- Retry at merge (`retry.rs` calls `approve_refusal(false, false)`) must keep refusing such a job; covered by the core rule, no extra test.
- The dashboard Approve button for such a job must not appear even in retry parks (`park_resume` = `review`). Pinned in Task 2.

---

### Task 1: Core refusal, CLI and API

**Files:**
- Modify: `crates/factory-core/src/board.rs` (`approve_refusal` ~367, new `waive_blocked_message`, tests module)
- Test: `crates/factory-supervisor/src/api/write.rs` (tests module, next to `waive_without_open_changes_is_a_conflict`)

**Interfaces:**
- Produces: `pub fn waive_blocked_message(&self) -> String` on `Job`, the refusal text for this case.

- [ ] **Step 1: Write the failing core tests** in the `board.rs` tests module, using `with_last_review("changes")` (round 2, 3 blockers, `review-code-r2.md`):

```rust
#[test]
fn approve_refusal_blocks_every_flag_while_a_role_has_no_verdict() {
    for state in [ReviewState::Failed, ReviewState::Pending, ReviewState::Running] {
        for resume in [None, Some(crate::park::ResumeAt::Review)] {
            let mut j = with_last_review("changes");
            j.set_review("code", ReviewState::Changes);
            j.set_review("security", state);
            j.park_resume = resume;
            for (without_review, waive) in [(false, false), (true, false), (false, true)] {
                let m = j.approve_refusal(without_review, waive).expect("refused");
                assert!(m.contains("3 open review blockers from round 2 (review-code-r2.md)"), "{m}");
                assert!(m.contains("no security review yet"), "{m}");
                assert!(!m.contains("--waive") && !m.contains("--without-review"), "{m}");
                let hint = if resume.is_some() { "use factory retry 001, or cancel" } else { "use rework, or cancel" };
                assert!(m.contains(hint), "{m}");
            }
        }
    }
}

#[test]
fn waive_still_works_when_every_role_has_a_verdict() {
    let mut j = with_last_review("changes");
    j.set_review("code", ReviewState::Changes);
    assert!(j.approve_refusal(false, true).is_none(), "no security role");
    j.set_review("security", ReviewState::Pass);
    assert!(j.approve_refusal(false, true).is_none());
    j.set_review("security", ReviewState::Skipped);
    assert!(j.approve_refusal(false, true).is_none());
}
```

- [ ] **Step 2: Run them to verify the first fails**

Run: `cargo test -j4 -p factory-core approve_refusal_blocks_every_flag`
Expected: FAIL (`--waive` returns None, or the message names `--waive`).

- [ ] **Step 3: Implement** `waive_blocked_message` next to `changes_open_message` with the Global Constraints shape, and in `approve_refusal` return it when `self.changes_open() && self.review_missing()`, placed right after the series-waiting check. Update the doc comment of `review_missing_message`'s neighbour if it says changes skip the rule.

- [ ] **Step 4: Run the core tests**

Run: `cargo test -j4 -p factory-core`
Expected: PASS, existing `approve_refusal_*` tests unchanged.

- [ ] **Step 5: Write the supervisor test** `waive_with_a_failed_security_review_is_a_conflict`: board like `open_changes_board` plus `"reviews":{"code":"changes","security":"failed"}` (replace the `"reviews":{}` part of `job_json`, as the test at ~829 does), POST `{"waive_reason":"known flake"}` to `/projects/good/jobs/1/approve`, assert `(409, "conflict")`, the message contains `no security review yet`, and `queued(&f) == 0`.

- [ ] **Step 6: Run it**

Run: `cargo test -j4 -p factory-supervisor waive_with_a_failed_security`
Expected: PASS (the API already maps `approve_refusal` to 409; if it does not, map it there).

- [ ] **Step 7: Run the touched crates fully**

Run: `cargo test -j4 -p factory-core && cargo test -j4 -p factory-supervisor && cargo test -j4 -p factory-cli --test new_to_merged && cargo test -j4 -p factory-engine --test retry --test sync_review && cargo clippy -j4 -p factory-core -p factory-supervisor --all-targets -- -D warnings`
Expected: all PASS, no warnings.

- [ ] **Step 8: Commit** (through the `dev-workflow-skills:1-git-commit` skill): `fix(core): refuse approve while a review is missing` with `Relates to: #117`.

### Task 2: Dashboard and docs

**Files:**
- Modify: `dashboard/static/board.js` (`approveCommand` ~36, export the new helper)
- Modify: `dashboard/static/projects.js` (`jobActions` ~105)
- Test: `dashboard/tests/board.test.js`, `dashboard/tests/projects.test.js`
- Modify: `docs/guides/factory-user-guide.md` (refusal table ~465, `--waive` row ~446), `docs/reference/software-factory.md` (approve rules)

**Interfaces:**
- Consumes: the Task 1 message text for the docs row.
- Produces: `approveBlocked(job) -> boolean` in `board.js`: `openChanges(job) && reviewMissing(job)`.

- [ ] **Step 1: Write the failing dashboard tests.** In `board.test.js`: for `makeJob({ stage: "needs-human", last_review: { verdict: "changes", blockers: 3 }, reviews: { code: "changes", security: "failed" } })`, `board.approveBlocked(job) === true` and `board.approveCommand(job) === ""`; with `security: "pass"` it is `false` and the command still contains `--waive`. In `projects.test.js`: `jobActions` of that blocked job is `["rework", "cancel"]`, and with `park_resume: "review"` it is `["retry", "cancel"]`; with `security: "pass"` the old lists stay.

- [ ] **Step 2: Run them**

Run: `cd dashboard && node --test`
Expected: FAIL on the new cases.

- [ ] **Step 3: Implement.** `approveCommand` returns `""` when `approveBlocked(job)`; `copyChip("")` must render nothing, so guard `retryApprove` and the needs-human hint line (~760) to skip an empty command. `jobActions` drops `"approve"` for such a job with its own copy of the same two checks (`projects.js` has no DOM access and does not load `board.js`; keep the check one line and name it `approveBlocked` there too).

- [ ] **Step 4: Run the dashboard tests**

Run: `(cd dashboard && node --test) && cargo test -j4 --manifest-path dashboard/Cargo.toml`
Expected: PASS.

- [ ] **Step 5: Docs.** In the user guide refusal table add the row for the Task 1 message: meaning "the last review asked for changes and a required review (for example security) failed or never ran; `--waive` cannot skip it", action "`retry` (or `rework`) to rerun the review, or cancel". Add to the `--waive` row: "Refused while a required review has no verdict." Add one line to the approve rules in `software-factory.md`.

- [ ] **Step 6: Commit** (through the `dev-workflow-skills:1-git-commit` skill): `fix(dashboard): hide approve while a review is missing` with `Fixes: #117`.
