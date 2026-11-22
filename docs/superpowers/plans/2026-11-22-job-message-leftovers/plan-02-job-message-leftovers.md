# Merge message rulings, footers and fix rounds Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-22-job-message-leftovers-design.md
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** The merge message writes each ruling once, lists each footer once, puts "and N more" under bullets, and treats a fix round with no session as a fix round, with every rule pinned by a test.

**Commit:** fix(message): write rulings and footers once

**Architecture:**
- `Engine::ruling_once` in `crates/factory-engine/src/ports.rs` checks the ledger before it appends.
- `build_job_message` in `crates/factory-core/src/commitmsg.rs` dedupes footers and places the dropped-count line.
- `build_stage` in `crates/factory-engine/src/build.rs` changes its `new_work` rule.

The engine code sits behind the `AgentRunner` port, and the tests use the scripted fakes from `crates/factory-engine/tests/common/mod.rs`.

**Tech Stack:** Rust.

## Global Constraints

- Cargo commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- No new board, ledger or event field. Ruling texts stay exactly as they are today.
- The merge message format is unchanged except for the two `build_job_message` fixes.

## Review Focus

1. A ledger that does not exist yet when `ruling_once` runs: a user expects the ruling written. Pinned by `a_second_merge_message_writes_the_refused_line_ruling_once`, which first removes `fx.job_dir.join("progress.md")` if it exists.
2. A ruling whose text has extra whitespace: a user expects it compared in its folded form, the form `ruling` writes. Pinned by `a_second_merge_message_writes_the_refused_line_ruling_once`.
3. A fresh worktree at round 2 (the tree was cleared): a user expects the build path. Pinned by the existing `a_whole_plan_build_makes_one_wip_snapshot` and the build tests, which must pass unchanged.
4. A message with no footers and a dropped count: a user expects the count line still present. Pinned by the existing `the_body_is_capped_at_sixty_bullets`, which must pass unchanged.
5. The sync commit: a user expects its exact subject. Pinned by `the_sync_commit_has_the_fixed_subject_and_footers`.

## File Structure

- Modify: `crates/factory-engine/src/ports.rs` (add `ruling_once`) and `crates/factory-engine/src/jobmsg.rs` (use it).
- Modify: `crates/factory-core/src/commitmsg.rs` (`build_job_message` and `mod tests`).
- Modify: `crates/factory-engine/src/build.rs` (`new_work`).
- Modify: `crates/factory-engine/tests/job_message.rs`, `tests/message.rs`, `tests/build_checks.rs`.

---

### Task 1: merge rulings are written once and the subjects are exact

**Files:**
- Modify: `crates/factory-engine/src/ports.rs`, `crates/factory-engine/src/jobmsg.rs`
- Modify: `crates/factory-engine/tests/job_message.rs`, `crates/factory-engine/tests/message.rs`

**Interfaces:**
- Port: `Engine` ledger helpers in `ports.rs`. There is no trait, and the tests use `Fixture` with `scripted(vec![])`.
- Produces: `pub(crate) fn ruling_once(&self, text: &str) -> Result<(), EngineError>` on `Engine`.

**Tests added:** `a_second_merge_message_writes_the_refused_line_ruling_once`, `a_title_ending_in_a_period_drops_the_period`, the exact asserts in `a_plan_without_a_commit_line_uses_the_plan_title` and `a_plan_without_a_heading_uses_the_feature`, and `the_sync_commit_has_the_fixed_subject_and_footers` (in `tests/message.rs`).

- [ ] **Step 1: Read the code**

Read these:
- `Engine::ruling` and `ledger_file` in `ports.rs`;
- `job_message` in `jobmsg.rs`, its two `self.ruling` calls;
- `Ledger::ruling` and `single_line` in `crates/factory-core/src/ledger.rs`;
- `title_subject` and `validate_message` in `crates/factory-core/src/commitmsg.rs`;
- `fixed_message` and `driver_footers` in `crates/factory-engine/src/message.rs`;
- in `tests/job_message.rs`, `request`, `write_plan`, `a_broken_commit_line_at_merge_falls_back_with_a_ruling` and the two subject tests;
- `sync_message_uses_relates_to_then_the_job_footer` in `tests/message.rs`;
- the spec's Design section 6.

- [ ] **Step 2: Write the failing tests**

```rust
#[test]
fn a_second_merge_message_writes_the_refused_line_ruling_once() {
    let fx = Fixture::new();
    let _ = std::fs::remove_file(fx.job_dir.join("progress.md"));
    write_plan(&fx, "# Retry\n\n**Commit:** Added a thing\n");
    let runner = scripted(vec![]);
    let engine = fx.engine(&runner);
    for _ in 0..2 {
        engine.job_message(&request("", "", &[], true)).expect("message");
    }
    assert_eq!(fx.ledger().matches("plan **Commit:** line refused").count(), 1, "{}", fx.ledger());
}
```

| Test | Input | Assertion |
|---|---|---|
| `a_title_ending_in_a_period_drops_the_period` | `write_plan(&fx, "# Retry a parked job.\n\n**Goal:** Retry it.\n")` | the subject equals `feat: retry a parked job`; `fx.ledger()` does not contain `fixed message used` |
| `a_plan_without_a_commit_line_uses_the_plan_title` | unchanged input | replace both asserts with `assert_eq!(subject, "feat: retry a parked job")` |
| `a_plan_without_a_heading_uses_the_feature` | unchanged input | `assert_eq!(subject, "feat: demo feature")` |
| `the_sync_commit_has_the_fixed_subject_and_footers` | the setup of `sync_message_uses_relates_to_then_the_job_footer` | the branch tip message equals `format!("chore(factory): sync main into job 001 {}\n\nRelates to: #20\nFactory-Job: 001", job.slug)`, where `job = fx.driver.job(&fx.id).expect("job")` |

No input reaches the fixed-message fallback today, because `cut_text` drops a trailing period, so it gets no test.

- [ ] **Step 3: Run them and see them fail**

Run: `cargo test -j4 -p factory-engine --test job_message`
Expected: `a_second_merge_message_writes_the_refused_line_ruling_once` fails with a count of 2. The other new and changed tests pass.

- [ ] **Step 4: Implement**

- `ruling_once`:
  - Fold `text` as `ruling` does.
  - Read `self.ledger_file().path` with `read_to_string(..).unwrap_or_default()`.
  - Return `Ok(())` when a line equals `format!("Ruling: {folded}")`, else call `self.ruling(&folded)`.
  - Give it a one-line doc comment.
- In `job_message`, both rulings call `ruling_once`.

- [ ] **Step 5: Run them and see them pass**

Run: `cargo test -j4 -p factory-engine --test job_message`, then `cargo test -j4 -p factory-engine --test message`.
Expected: all pass, 0 failed.

### Task 2: each footer once, and "and N more" under bullets

**Files:**
- Modify: `crates/factory-core/src/commitmsg.rs`

**Interfaces:**
- Port: pure message building in factory-core, with no trait.
- Consumes: `JobMessage`, `NoteSection` and the test helpers `sample_sections` and `sample_message`.
- Produces: `pub fn build_job_message(m: &JobMessage<'_>) -> String` keeps its signature.

**Tests added:** `a_repeated_footer_appears_once`, `a_footer_equal_to_a_body_line_is_left_out`, `and_n_more_goes_under_the_last_section_with_bullets`.

- [ ] **Step 1: Read the code**

Read `build_job_message`, its `take` closure, the `dropped` line and the footer filter. Read `sample_sections`, `sample_message`, `the_body_is_capped_at_sixty_bullets` and `a_section_with_no_bullet_left_after_the_cap_is_left_out` in `mod tests`. Read the spec's Design section 7.

- [ ] **Step 2: Write the failing tests**

```rust
#[test]
fn and_n_more_goes_under_the_last_section_with_bullets() {
    let big = NoteSection {
        title: "Big".to_string(),
        bullets: (0..61).map(|i| format!("b{i}")).collect(),
    };
    let later = NoteSection { title: "Later".to_string(), bullets: Vec::new() };
    let msg = build_job_message(&JobMessage {
        subject: "feat: x",
        why: None,
        tasks: &[big, later],
        fixes: &[],
        footers: &[],
    });
    assert!(msg.contains("- b59\n- and 1 more changes\n\nLater:"), "{msg}");
    assert!(msg.ends_with("Later:"), "{msg}");
}
```

| Test | Input | Assertion |
|---|---|---|
| `a_repeated_footer_appears_once` | `msg_with_footers(&["Fixes: #142", "Factory-Job: 042", "Fixes: #142"])` | `msg.matches("Fixes: #142").count() == 1`; `msg.ends_with("Fixes: #142\nFactory-Job: 042")` |
| `a_footer_equal_to_a_body_line_is_left_out` | `msg_with_footers(&["- a", "Factory-Job: 042"])` | `msg.matches("- a").count() == 1`; `msg.ends_with("- Refuse approve while a retry is queued\n\nFactory-Job: 042")` |

`sample_message()` takes no arguments, so add a test helper `fn msg_with_footers(footers: &[&str]) -> String`. It builds a `JobMessage` with the subject, why, `sample_sections()` and fixes of `sample_message`, and the given footers as `String`s, and returns `build_job_message` of it.

- [ ] **Step 3: Run them and see them fail**

Run: `cargo test -j4 -p factory-core commitmsg::tests`
Expected: `a_repeated_footer_appears_once` and `and_n_more_goes_under_the_last_section_with_bullets` fail. `a_footer_equal_to_a_body_line_is_left_out` passes, because it pins today's rule.

- [ ] **Step 4: Implement**

- The footer filter also skips a footer already kept earlier in the list, using a `Vec<&str>` of the kept ones, so the order stays.
- The `dropped` line goes to the last block that has a line starting with `- `. When no block has one, it is pushed as a block of its own.
- Update the doc comment of `build_job_message` to say that a footer appears once.

- [ ] **Step 5: Run them and see them pass**

Run: `cargo test -j4 -p factory-core commitmsg`
Expected: all pass, 0 failed.

### Task 3: a fix round with no session is a fix round, end to end

**Files:**
- Modify: `crates/factory-engine/src/build.rs`
- Modify: `crates/factory-engine/tests/build_checks.rs`, `crates/factory-engine/tests/job_message.rs`

**Interfaces:**
- Port: behind `AgentRunner`. The tests use `Fixture` and `scripted` in `build_checks.rs`, and a test-local `AgentRunner` with `SeriesFixture::build` in `job_message.rs`.
- Consumes: `Job.round`, and `JobView.session_id`, which `Engine::job()` fills from `builder_id`.
- Produces: no new items.

**Tests added:** `a_fix_round_without_a_session_is_still_a_fix_round`, `a_task_job_with_a_fix_round_merges_with_its_sections_and_review_fixes`.

- [ ] **Step 1: Read the code**

Read these:
- in `crates/factory-engine/src/build.rs`, `build_stage` from `let fresh =` to the note file choice (`build.md` or `round-N.md`), and every use of `new_work`;
- `start_fix_round` and `a_fix_round_snapshot_sits_above_the_reviewed_head` in `tests/build_checks.rs`;
- `RoleRunner`, `SeriesFixture::build` and `plan_file` in `tests/common/mod.rs`;
- `run_job_goes_from_build_to_merged_in_one_call` and `a_series_lands_one_merge_per_plan_with_its_commit_subject` in `tests/runloop.rs`;
- `NOTES_DIR` and `read_notes` in `crates/factory-engine/src/notes.rs`;
- the spec's Design section 8.

- [ ] **Step 2: Write the failing tests**

`a_fix_round_without_a_session_is_still_a_fix_round`, in `build_checks.rs`:
- Run the first build as `a_fix_round_snapshot_sits_above_the_reviewed_head` does, then `start_fix_round(&fx)`.
- Set `builder_id = None` on job `001` in the board file, the same way `start_fix_round` edits it.
- Run `build_stage`, expecting `Advanced(Stage::Review)`.
- The one commit above the reviewed head has subject `wip: fix round 2`.
- `fx.job_dir.join(NOTES_DIR).join("round-2.md")` exists. `build.md` holds the first build's note, unchanged.

`a_task_job_with_a_fix_round_merges_with_its_sections_and_review_fixes`, in `job_message.rs`, uses a test-local runner:

```rust
struct NoteRunner {
    builds: std::sync::atomic::AtomicUsize,
    reviews: std::sync::atomic::AtomicUsize,
}

impl factory_agent::AgentRunner for NoteRunner {
    fn run(&self, spec: &factory_agent::AgentSpec) -> factory_agent::AgentRun {
        use std::sync::atomic::Ordering::SeqCst;
        let text = if spec.role == "builder" {
            let n = self.builds.fetch_add(1, SeqCst);
            std::fs::create_dir_all(spec.cwd.join("docs")).expect("mkdir");
            std::fs::write(spec.cwd.join(format!("docs/f{n}.txt")), "x\n").expect("write");
            let subject = ["Add the first file", "Add the second file", "Fix the finding"][n.min(2)];
            format!("Done.\n\n---BEGIN COMMIT---\n{subject}\n\nWhy it matters.\n\n- Change f{n}\n---END COMMIT---\n")
        } else if spec.role == "reviewer" && self.reviews.fetch_add(1, SeqCst) == 0 {
            "1. [blocker] docs/f0.txt:1 handle the empty case.\nVERDICT: CHANGES".to_string()
        } else {
            "VERDICT: APPROVE".to_string()
        };
        factory_agent::AgentRun { session_id: Some("sess-1".to_string()), ..factory_agent::AgentRun::done(&text) }
    }
}
```

The files go under `docs/`, because a changed path that no check covers parks the job. The code reviewer's role is `reviewer`. Check the import paths of `AgentRunner`, `AgentSpec` and `AgentRun` against `tests/common/mod.rs`.

- The driver asks the factory for a runner on every agent call, so one runner must serve them all. Create `let runner = Arc::new(NoteRunner { .. });` first, and build the fixture with `SeriesFixture::build("VERDICT: APPROVE", Config::default(), move |_| runner.clone())`.
- Write a plan with two tasks: `"# Two\n\n**Goal:** Ship two files.\n\n**Commit:** feat(x): ship two files\n\n### Task 1: first\n\n### Task 2: second\n\nNext plan: none\n"`.
- `create_job` it, then `run_job` it to `RunEnd::Merged`.
- Read main's tip merge commit message with git2, as the series test does.

Assertions:
- it starts with `feat(x): ship two files\n\nShip two files.\n\n`;
- it contains `Add the first file:\n- Change f0` and `Add the second file:\n- Change f1`;
- it contains `Review fixes:\n- Change f2`;
- it ends with `Factory-Job: 001`, after trimming;
- no line starts with `wip:`.

- [ ] **Step 3: Run them and see them fail**

Run: `cargo test -j4 -p factory-engine --test build_checks a_fix_round_without_a_session`
Expected: it fails because the subject is `wip: build the plan`.

Run: `cargo test -j4 -p factory-engine --test job_message a_task_job_with_a_fix_round`
Expected: it passes, because the fix round keeps its session. If it fails, fix the test setup, not the driver.

- [ ] **Step 4: Implement**

In `build_stage`, change the rule to `let new_work = !fresh_fix && (fresh || (job.session_id.is_none() && job.round <= 1));`. Add a comment above it saying that a fix round stays a fix round when its session was cleared.

- [ ] **Step 5: Run the files**

Run: `cargo test -j4 -p factory-engine --test build_checks`, then `cargo test -j4 -p factory-engine --test job_message`, `cargo test -j4 -p factory-engine --test fix_fresh`, `cargo test -j4 -p factory-engine --test builder_recover` and `cargo test -j4 -p factory-engine --test slim_prompts`.
Expected: all pass, 0 failed.

- [ ] **Step 6: Format and lint**

Run: `cargo fmt -p factory-engine`, `cargo fmt -p factory-core`, `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`, `cargo clippy -j4 -p factory-core --all-targets -- -D warnings`.
Expected: clippy finishes with no warnings.

Next plan: docs/superpowers/plans/2026-11-22-job-message-leftovers/plan-03-job-message-leftovers.md
