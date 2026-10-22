# Cancelled Series Cleanup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Allow safe cleanup of cancelled cards after their full series finishes, and display cancelled jobs clearly in CLI status.

**Architecture:** Add an error-reporting supervisor queue listing API while retaining the existing best-effort API for current callers. Carry registered project identity into `CliCtx`, then check normalized queued job IDs and finished-series state before removing cancelled cards. Render the cancellation flag in CLI status and verify the dashboard's existing cancelled-row behavior.

**Tech Stack:** Rust workspace, `factory-core`, `factory-supervisor`, `factory-cli`, dashboard Rust tests.

**Spec:** docs/superpowers/specs/2026-10-22-cleanup-cancelled-series-design.md

> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

## Global Constraints

- Cleanup of a cancelled job never deletes the job directory, events, ledger, or usage history.
- Merged job cards remain on the board.
- A cancelled card is removable only when every series member is merged or cancelled, there is no live lease protecting the relevant work, and no matching queued request protects a series member.
- Queue matching is scoped by exact registered project name and normalized numeric job ID.
- A blocked cleanup attempt does not remove requests or automatically retry later.
- Existing repository-local CLI and test contexts remain usable without a registered project or factory home.
- A missing queue directory is an empty queue; other queue directory read errors block removal only for registered project contexts.
- Status output changes only for jobs whose `cancelled` flag is true.

## Review Focus

1. A queued request uses an unpadded numeric ID such as `2` while its board member is `002`; `cleanup_blocks_a_queued_request_for_a_zero_padded_series_member` pins normalization before matching.
2. A missing queue directory is normal for a new factory home; `try_list_returns_empty_for_missing_queue_directory` pins the empty result.
3. A queue path that exists as a regular file cannot be scanned; `try_list_reports_queue_directory_read_errors` pins a returned error and `cleanup_fails_closed_when_registered_project_queue_is_unreadable` pins card retention.
4. A malformed queue request cannot safely be attributed to a project or job; `try_list_returns_bad_items_for_malformed_requests` pins the bad item while preserving valid entries.
5. A repository-local context has no registered project identity for queue filtering; `cleanup_without_registered_context_uses_board_and_lease_guards` pins the existing local behavior.

---

## File Structure

- `crates/factory-supervisor/src/queue.rs` — add a fallible queue scan and focused tests without changing `list` callers.
- `crates/factory-cli/Cargo.toml` — depend on the supervisor queue API.
- `crates/factory-cli/src/commands.rs` — carry factory home/project identity in `CliCtx` and enforce series, lease, and queue guards.
- `crates/factory-cli/src/main.rs` — populate registered project context through `CliCtx::for_project`; repository-local construction remains unchanged.
- `crates/factory-cli/src/status.rs` — display `cancelled` when the job cancellation flag is set.
- `crates/factory-cli/tests/new_to_merged.rs` — test cancelled-card removal and preservation rules.
- `crates/factory-cli/tests/project_flag.rs` — assert registered contexts retain queue identity.
- `dashboard/src/specs.rs` — inspect the existing cancelled-row projection; no edit is expected unless its test reveals a regression.

### Task 1: Add a fallible supervisor queue listing API

**Files:**
- Modify: `crates/factory-supervisor/src/queue.rs`
- Test: `crates/factory-supervisor/src/queue.rs`

**Interfaces:**
- Consumes: existing `Home`, `QueueItem`, `QueueError`, `read_item`, and `queue::list` behavior in `crates/factory-supervisor/src/queue.rs`.
- Produces: `pub fn try_list(home: &Home) -> Result<Vec<QueueItem>, QueueError>`; missing queue directory returns `Ok(Vec::new())`, directory/entry scan errors return `Err(QueueError::Core(_))`, and malformed files remain `QueueItem::Bad`.

- [ ] **Step 1: Read the queue implementation and error types**

Read `crates/factory-supervisor/src/queue.rs` and `crates/factory-supervisor/src/records.rs`. Follow `list`, `read_item`, `QueueError::Core`, and the existing tests `list_of_a_missing_folder_is_empty` and `bad_files_are_reported_and_good_ones_still_listed`.

- [ ] **Step 2: Write failing tests for the fallible scan**

Add the following tests to `queue.rs`'s existing `tests` module. Reuse its `home()` and `req()` helpers.

```rust
#[test]
fn try_list_returns_empty_for_missing_queue_directory() {
    let (_tmp, home) = home();
    assert_eq!(try_list(&home).expect("missing queue is empty"), Vec::new());
}

#[test]
fn try_list_returns_bad_items_for_malformed_requests() {
    let (_tmp, home) = home();
    let good = req("a", 1);
    enqueue(&home, &good).expect("enqueue valid request");
    std::fs::write(home.queue_dir().join("junk.json"), "{not json").expect("write bad request");

    let items = try_list(&home).expect("scan queue");
    assert_eq!(items.len(), 2);
    assert_eq!(items[0], QueueItem::Ok(good));
    match &items[1] {
        QueueItem::Bad { file, reason } => {
            assert_eq!(file, "junk.json");
            assert!(!reason.is_empty());
        }
        QueueItem::Ok(request) => panic!("unexpected valid request: {request:?}"),
    }
}

#[test]
fn try_list_reports_queue_directory_read_errors() {
    let (_tmp, home) = home();
    std::fs::create_dir_all(&home.dir).expect("create home");
    std::fs::write(home.queue_dir(), "not a directory").expect("make queue path a file");
    assert!(matches!(try_list(&home), Err(QueueError::Core(_))));
}
```

- [ ] **Step 3: Run the new tests to verify they fail**

Run: `cargo test -p factory-supervisor queue::tests::try_list_`
Expected: compile failure because `try_list` is not yet defined.

- [ ] **Step 4: Implement `try_list(home: &Home) -> Result<Vec<QueueItem>, QueueError>`**

Scan the queue directory once, preserving the ordering and `QueueItem::Bad` handling of `list`. Treat `NotFound` for the queue directory as an empty queue; convert other `read_dir` and iterator-entry errors to `QueueError::Core(CoreError::io(...))`. Keep `list(home) -> Vec<QueueItem>` unchanged for existing best-effort callers.

- [ ] **Step 5: Run the new tests to verify they pass**

Run: `cargo test -p factory-supervisor queue::tests::try_list_`
Expected: 3 passed, 0 failed.

### Task 2: Remove cancelled cards only after series and queue guards pass

**Files:**
- Modify: `crates/factory-cli/Cargo.toml`
- Modify: `crates/factory-cli/src/commands.rs`
- Modify: `crates/factory-cli/src/main.rs`
- Test: `crates/factory-cli/tests/new_to_merged.rs`
- Test: `crates/factory-cli/tests/project_flag.rs`

**Interfaces:**
- Consumes: Task 1's `factory_supervisor::queue::try_list(&Home) -> Result<Vec<QueueItem>, QueueError>`, plus `Request`, `Action`, and `QueueItem` from `factory-supervisor`.
- Produces: `CliCtx.home: Option<Home>` and `CliCtx.project_name: Option<String>`; `CliCtx::for_project` sets both, while `new`, `with_layout`, and `from_env` set both to `None`.
- Produces: `fn series_is_finished(board: &Board, target_id: &str) -> bool`, treating no series ID or one remaining board member as finished and requiring every matching member to be merged or cancelled otherwise.
- Produces: `fn queued_request_matches_series(request: &Request, project: &str, member_ids: &[String]) -> bool`, requiring an exact project match and `factory_core::normalize_id(&request.job)` to match a member ID.

- [ ] **Step 1: Read the cleanup, context, queue, and test fixtures**

Read `crates/factory-cli/src/commands.rs` around `CliCtx`, `for_project`, and `remove_cancelled_card`; `crates/factory-cli/src/main.rs` around registered project construction; `crates/factory-cli/tests/new_to_merged.rs` around `parked_and_cancelled`, `board_ids`, and existing cleanup tests; and `crates/factory-cli/tests/project_flag.rs` around `Env::ctx` and `setup`.

- [ ] **Step 2: Write failing series, queue, and context tests**

In `new_to_merged.rs`, reuse `Fixture::with_risk_path`, `parked_and_cancelled`, the existing `BoardStore` pattern, and `board_ids`. Create jobs `001` and `002`, then set their `series` fields inside `BoardStore::update` to `Some(SeriesPos { plan_file: "p.md".into(), index: 1, total: 2, id: Some("S3".into()) })` for `001` and the same value with `index: 2` for `002`. Cancel `001` with `fx.run(&["cancel", "1"])`; cancel `002` for the cancelled-sibling case or set its stage with `job.set_stage(Stage::Merged, None, false, factory_engine::now_secs())` or `job.set_stage(Stage::Review, None, false, factory_engine::now_secs())` for the merged/review cases. Leave `002` untouched for the queued case. Bind the fixture mutably for registered queue cases, then set `fx.ctx.home = Some(home.clone())` and `fx.ctx.project_name = Some("p".to_string())`.

The finished-series case uses a merged sibling and asserts:

```rust
let out = fx.run(&["cleanup", "1", "--discard-commits"]).expect("cleanup");
assert!(out.ends_with("; card removed from the board"), "{out}");
assert_eq!(board_ids(&fx), vec!["002".to_string()]);
```

Add `cleanup_removes_cancelled_card_when_series_sibling_is_cancelled` with sibling `002` cancelled and the same card-removal assertions. Retain and update `cleanup_keeps_the_card_of_a_cancelled_series_member_with_a_sibling` with sibling `002` still queued; assert the output contains `series S3 still has active jobs` and both IDs remain.

Add `cleanup_blocks_a_queued_request_for_a_zero_padded_series_member` using this exact request and assertions:

```rust
let request = factory_supervisor::Request::new(
    "p",
    factory_supervisor::Action::Run,
    "2",
    true,
    1,
).expect("request");
factory_supervisor::queue::enqueue(&home, &request).expect("enqueue");
let out = fx.run(&["cleanup", "1", "--discard-commits"]).expect("cleanup");
assert!(out.contains("series S3 has queued requests"), "{out}");
assert_eq!(board_ids(&fx), vec!["001".to_string(), "002".to_string()]);
```

Add these cases with the same finished-series fixture shape:

| Test | Exact input | Assertions |
| --- | --- | --- |
| `cleanup_removes_cancelled_card_when_series_sibling_is_merged` | Sibling `002` has `Stage::Merged`; no queue request | Output ends with `; card removed from the board`; only `002` remains. |
| `cleanup_removes_cancelled_card_when_series_sibling_is_cancelled` | Sibling `002` has `cancelled == true` | Output ends with `; card removed from the board`; only `002` remains. |
| `cleanup_keeps_the_card_of_a_cancelled_series_member_with_a_sibling` | Sibling `002` remains in its initial queued state | Output contains `series S3 still has active jobs`; both IDs remain. |
| `cleanup_keeps_cancelled_card_when_series_sibling_is_in_review` | Sibling `002` has `Stage::Review` | Output contains `series S3 still has active jobs`; both IDs remain. |
| `cleanup_blocks_a_queued_request_for_a_zero_padded_series_member` | Project `p`, request job `2`, action `Run`, series `true`; board member `002` | Output contains `series S3 has queued requests`; both IDs remain. |
| `cleanup_blocks_an_approve_request_for_series_member` | Project `p`, request job `2`, action `Approve`; board member `002` | Output contains `series S3 has queued requests`; both IDs remain. |
| `cleanup_ignores_unrelated_project_and_job_queue_requests` | Requests `("other", "2")` and `("p", "3")` | Output ends with `; card removed from the board`; only `002` remains. |
| `cleanup_fails_closed_when_registered_project_queue_is_unreadable` | `home.queue_dir()` is a regular file | Output contains `queued work could not be checked`; both IDs remain. |
| `cleanup_ignores_malformed_queue_request` | `home.queue_dir()` contains `junk.json` with `{not json` and no valid requests | Output ends with `; card removed from the board`; only `002` remains. |
| `cleanup_can_remove_card_when_retried_after_queue_request_finishes` | Enqueue matching request, remove it with `queue::remove(&home, &request.id)`, retry cleanup | Output ends with `; card removed from the board`; only `002` remains. |
| `cleanup_keeps_card_for_single_cancelled_job_with_matching_queue_request` | No series ID; project `p`, request job `1`, action `Run` | Output contains `job 001 has a queued request`; card `001` remains. |

For the unrelated-request test, construct both requests with `Request::new` and add them with `queue::enqueue`. For the unreadable-queue test, create `home.dir` before writing the regular file at `home.queue_dir()`.

Add `cleanup_without_registered_context_uses_board_and_lease_guards` using `Fixture::with_risk_path("docs/**")` without setting `home` or `project_name`; cancel a single job and assert cleanup ends with `; card removed from the board`.

In `project_flag.rs`, add `registered_project_context_carries_queue_identity` using `let e = setup(&[]); let ctx = e.ctx().expect("ctx");`; assert `ctx.home.as_ref() == Some(&e.home)` and `ctx.project_name.as_deref() == Some("p")`.

- [ ] **Step 3: Run the new tests to verify they fail**

Run: `cargo test -p factory-cli --test new_to_merged cleanup_`
Expected: compile failure because `CliCtx` does not yet define `home` and `project_name`.

Run: `cargo test -p factory-cli --test project_flag registered_project_context_carries_queue_identity`
Expected: compile failure because `CliCtx` does not yet carry the registered home and project name.

- [ ] **Step 4: Add the supervisor dependency and registered project identity**

Add the `factory-supervisor` path dependency. Populate `CliCtx.home: Option<Home>` and `CliCtx.project_name: Option<String>` in all constructors: registered context uses `Some(home.clone())` and `Some(name.to_string())`; `new`, `with_layout`, and `from_env` use `None`.

- [ ] **Step 5: Run tests to verify the guards still fail**

Run: `cargo test -p factory-cli --test new_to_merged cleanup_`
Expected: compilation succeeds; tests for finished siblings, matching queued requests, and the new guard messages fail because cleanup still uses the any-sibling rule.

Run: `cargo test -p factory-cli --test project_flag registered_project_context_carries_queue_identity`
Expected: 1 passed, 0 failed.

- [ ] **Step 6: Implement finished-series and queue guards**

In `remove_cancelled_card`, determine series members and retain the card if any sibling is neither merged nor cancelled; preserve the existing target lease and project lease checks. For registered contexts, call `try_list`; retain the card with the queue-check explanation on error, or with the queued-request explanation when a valid request matches project and normalized member ID. Ignore `QueueItem::Bad`. Continue to remove only the target card after all checks pass. Do not change `queue::list` or delete request files.

- [ ] **Step 7: Run the new tests to verify they pass**

Run: `cargo test -p factory-cli --test new_to_merged cleanup_`
Expected: 21 passed, 0 failed, including the existing cleanup, merged-card, and lease cases.

Run: `cargo test -p factory-cli --test project_flag registered_project_context_carries_queue_identity`
Expected: 1 passed, 0 failed.

### Task 3: Render cancelled state and verify dashboard projection

**Files:**
- Modify: `crates/factory-cli/src/status.rs`
- Test: `crates/factory-cli/src/status.rs`
- Verify: `dashboard/src/specs.rs`

**Interfaces:**
- Consumes: `Job.cancelled`, `Job.stage`, and the existing `stage_name` function in `status.rs`.
- Produces: status table stage text `cancelled` when `job.cancelled` is true; non-cancelled jobs continue to use `stage_name(job.stage)`.

- [ ] **Step 1: Read the status and dashboard projections**

Read `crates/factory-cli/src/status.rs` around `render_jobs` and `status_shows_stage_round_reviews_tokens_and_series_position`. Read `dashboard/src/specs.rs:34-60` and its tests `a_plan_row_state_follows_the_stage_and_the_cancelled_flag` and `card_stage_uses_needs_human_for_stuck_or_cancelled_rows`.

- [ ] **Step 2: Write a failing cancelled-status test**

Add to `status.rs` tests:

```rust
#[test]
fn status_shows_cancelled_instead_of_needs_human() {
    let mut board = factory_core::Board::default();
    let mut job = factory_core::Job::new("002", "d-002-x", "cleanup", "2026-10-22", "f", 0.0);
    job.set_stage(Stage::NeedsHuman, Some("cancelled: done"), false, 1.0)
        .expect("needs human stage");
    job.cancelled = true;
    board.jobs.push(job);

    let text = render_status(&board, &factory_core::Config::default());
    let row = text.lines().nth(1).expect("job row");
    assert!(row.contains("cancelled"), "{row}");
    assert!(!row.contains("needs-human"), "{row}");
}
```

- [ ] **Step 3: Run the new test to verify it fails**

Run: `cargo test -p factory-cli status_shows_cancelled_instead_of_needs_human`
Expected: 1 failed because the row currently prints `needs-human`.

- [ ] **Step 4: Render the cancellation flag in status**

In `render_jobs`, use `"cancelled"` when `job.cancelled` is true and otherwise use `stage_name(job.stage)`. Leave board state and non-cancelled stage rendering unchanged. Do not edit dashboard code; its existing row projection maps cancellation to `cancelled`, and removing the board row removes it from the dashboard response.

- [ ] **Step 5: Run the status and dashboard projection tests**

Run: `cargo test -p factory-cli status_shows_cancelled_instead_of_needs_human`
Expected: 1 passed, 0 failed.

Run: `cargo test -j4 --manifest-path dashboard/Cargo.toml a_plan_row_state_follows_the_stage_and_the_cancelled_flag`
Expected: 1 passed, 0 failed; the dashboard still maps a cancelled job row to `cancelled`.

Run: `cargo test -p factory-cli`
Expected: all factory-cli unit and integration tests pass.

Next plan: none
