# factoryd enqueue pads the job id: Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-22-enqueue-job-id-design.md
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** `factoryd enqueue` stores padded job ids and refuses the same duplicate and invalid requests the HTTP API refuses.

**Commit:** fix(supervisor): pad job ids in factoryd enqueue

**Architecture:** All admission logic lives in `crates/factory-supervisor/src/admit.rs`: `job_id` refuses job 0, a private `is_queued` compares padded ids, `queue_run` admits a run and `queue_approve` admits an approve with the checks moved out of the HTTP approve route. The HTTP approve route and the `factoryd` binary both call them. No factory-engine code changes, so no engine port is involved.

**Tech Stack:** Rust, crate `factory-supervisor`. No board, ledger or event fields change.

## File Structure

| File | Change |
|---|---|
| `crates/factory-supervisor/src/admit.rs` | `job_id` job-0 rule; `is_queued`; `refuse_if_queued` uses it; new `queue_run` and `queue_approve`; tests. Module doc comment names `factoryd enqueue` instead of `factoryd enqueue --retry`. |
| `crates/factory-supervisor/src/api/write.rs` | `approve_inner` (375-441) calls `admit::queue_approve`; `approve_checks_the_job_id` gains `"0"`. |
| `crates/factory-supervisor/src/request.rs` | `Action::name`. |
| `crates/factory-supervisor/src/bin/factoryd.rs` | `--approve` conflicts with `--series`; `enqueue` (63-83) calls `queue_run` / `queue_approve`; status line (206) shows the action; tests. |
| `docs/reference/factory-admin-manual.md` | Row at 193 and paragraph at 196. |
| `docs/guides/factory-user-guide.md` | `factoryd status` list (1099), `--job` row (1142), enqueue notes (1157-1158). |

## Review Focus

1. A queue entry written before this change with job `"1"`: a user expects it to block an approve or retry of `001`. Pinned by `refuse_if_queued_matches_an_unpadded_entry`.
2. A run and an approve of the same job queued in either order: both must queue, because the run rule only blocks runs. Pinned by `queue_run_and_approve_do_not_block_each_other`.
3. The HTTP approve route after the move: its codes and messages must not change. Pinned by the existing approve route tests in `api/write.rs`, which must pass unchanged.
4. Job `00` and `000` as well as `0`: all are refused. Pinned by `job_id_refuses_job_zero`.
5. A refusal must leave the queue as it was. Every refusal test asserts the queue length.

---

### Task 1: Padded duplicate checks, job 0 and `queue_run`

**Files:**
- Modify: `crates/factory-supervisor/src/admit.rs`
- Test: `crates/factory-supervisor/src/admit.rs` (`tests` module)

**Interfaces:**
- Consumes: `project_layout`, `job_id`, `refuse_if_queued`, `enqueue`, `queue_retry` in `admit.rs`; `Request::new(project, action, job, series, now_ms)`, `INTAKE_JOB` and `validate_job` in `request.rs`; `normalize_id` from `factory_core`. Test fixtures `fixture()`, `job_json`, `write_board`, `parked(&home, RESUME_MERGE)` and `queue::list`, as `queue_retry_queues_one_retry_and_refuses_a_second` uses them.
- Produces: `pub fn queue_run(home: &Home, name: &str, raw_id: &str, series: bool, now_ms: u64) -> Result<String, Refusal>` and the private `fn is_queued(home: &Home, name: &str, id: &str, actions: &[Action]) -> bool`. The `queue_approve` task and the `factoryd` task use them.

- [ ] **Step 1: Read** `crates/factory-supervisor/src/admit.rs` (all), `crates/factory-supervisor/src/request.rs:1-140`, `crates/factory-supervisor/src/queue.rs:1-60`, `crates/factory-core/src/ids.rs:1-12`, and the spec's Design sections 1 to 3.

- [ ] **Step 2: Write the failing tests** in the `tests` module of `admit.rs`. Add a helper and this test in full:

```rust
fn jobs(home: &Home) -> Vec<(Action, String)> {
    queue::list(home)
        .into_iter()
        .map(|item| match item {
            queue::QueueItem::Ok(r) => (r.action, r.job.clone()),
            other => panic!("{other:?}"),
        })
        .collect()
}

#[test]
fn queue_run_refuses_a_second_run() {
    let (_t, home) = fixture();
    queue_run(&home, "good", "1", false, 1).expect("queued");
    let err = queue_run(&home, "good", "001", false, 2).expect_err("refused");
    assert_eq!(err, Refusal::Conflict("job 001 already has a run queued".into()));
    assert_eq!(jobs(&home), vec![(Action::Run, "001".to_string())]);
}
```

The other tests in the same shape:

| Test | Steps | Assertions |
|---|---|---|
| `job_id_refuses_job_zero` | `job_id` of `"0"`, `"00"`, `"000"`, then `"1"` | each of the first three `expect_err` equals `Refusal::BadRequest("job 000 is not a job, ids start at 001".into())`; `job_id("1").expect("ok") == "001"` |
| `refuse_if_queued_matches_an_unpadded_entry` | `parked(&home, RESUME_MERGE)`; `queue::enqueue(&home, &Request::new("good", Action::Approve, "1", false, 1).expect("req")).expect("enqueue")`; `queue_retry(&home, "good", "1", 2)` | `expect_err` equals `Refusal::Conflict("job 001 already has an approve or retry queued".into())`; `queue::list(&home).len() == 1` |
| `queue_run_pads_the_job_id` | `queue_run(&home, "good", "0001", true, 1)` | `jobs(&home) == vec![(Action::Run, "001".to_string())]`; the one request has `series == true` |
| `queue_run_does_not_block_an_approve_or_retry` | `parked(&home, RESUME_MERGE)`; `queue_run("1")` at 1; `queue_retry("1")` at 2 | both `expect("queued")`; `queue::list(&home).len() == 2` |
| `queue_run_refuses_an_unknown_project_and_a_bad_id` | `queue_run(&home, "nope", "1", false, 1)`; then `"abc"`, `""`, `"1234567"` for `"good"` | first equals `Refusal::NotFound("no project named nope".into())`; each bad id equals `Refusal::BadRequest(validate_job(id).expect_err("bad").to_string())`; `queue::list(&home).is_empty()` |

Build the bad-id messages from `validate_job` as shown, not as literals.

- [ ] **Step 3: Run** `cargo test -j4 -p factory-supervisor --lib -- admit::tests`. Expected: compile error `cannot find function `queue_run` in this scope`.

- [ ] **Step 4: Implement.**
  - `job_id`: after padding, return `Refusal::BadRequest("job 000 is not a job, ids start at 001".into())` when the id equals `INTAKE_JOB` (import it from `crate::request`).
  - `is_queued`: true when a `queue::QueueItem::Ok(r)` has `r.project == name`, `actions.contains(&r.action)` and `normalize_id(&r.job).is_ok_and(|j| j == id)`.
  - `refuse_if_queued` calls `is_queued(home, name, id, &[Action::Approve, Action::Retry])` and keeps its message.
  - `queue_run` follows the spec's Design section 3, in that order. Its doc comment says a run blocks only a run, and that the entry stays while the child runs.
  - Change the module doc comment's first sentence to name `factoryd enqueue` instead of `factoryd enqueue --retry`.

- [ ] **Step 5: Run** `cargo test -j4 -p factory-supervisor --lib -- admit::tests`. Expected: every `admit::tests` test passes, 0 failed. Then run `cargo test -j4 -p factory-supervisor --lib -- api::` to confirm the job-0 rule broke no route test. Expected: 0 failed.

### Task 2: `admit::queue_approve` shared with the HTTP approve route

**Files:**
- Modify: `crates/factory-supervisor/src/admit.rs`
- Modify: `crates/factory-supervisor/src/api/write.rs:375-441` and the test `approve_checks_the_job_id` (974)
- Test: `crates/factory-supervisor/src/admit.rs` (`tests` module)

**Interfaces:**
- Consumes: `project_layout`, `job_id`, `refuse_if_queued`, `enqueue`, `queue_retry` and `queue_run(home, name, raw_id, series, now_ms) -> Result<String, Refusal>` from `admit.rs`; the test helper `jobs(home: &Home) -> Vec<(Action, String)>` in its `tests` module, added with `queue_run`; `Board::load`, `Job::approve_refusal(without_review: bool, waive: bool) -> Option<String>` and `Stage` from `factory_core`; `Request::without_review()` and `Request::waive(&str)` from `request.rs`.
- Produces: `pub fn queue_approve(home: &Home, name: &str, raw_id: &str, without_review: bool, waive_reason: Option<&str>, now_ms: u64) -> Result<String, Refusal>`. The `factoryd` task calls it with `false, None`.

- [ ] **Step 1: Read** `crates/factory-supervisor/src/api/write.rs:200-240` and `:360-445`, and its approve route tests (`approve_queues_only_for_needs_human_or_approve` through `approve_checks_the_job_id`, 846-995). Read the spec's Design section 4.

- [ ] **Step 2: Write the failing tests** in `admit.rs`. A helper writes a one-job board as `board_with_review` in `api/write.rs` does:

```rust
fn board(home: &Home, stage: &str, cancelled: bool, review: &str) {
    let job = job_json("001", "001-first")
        .replace(
            r#""stage":"build""#,
            &format!(r#""stage":"{stage}","cancelled":{cancelled}"#),
        )
        .replace(r#""reviews":{}"#, &format!(r#""reviews":{{"code":"{review}"}}"#));
    write_board(home, &format!(r#"{{"jobs":[{job}]}}"#));
}

#[test]
fn queue_approve_pads_and_refuses_a_second_approve() {
    let (_t, home) = fixture();
    board(&home, "approve", false, "pass");
    queue_approve(&home, "good", "1", false, None, 1).expect("queued");
    let err = queue_approve(&home, "good", "001", false, None, 2).expect_err("refused");
    assert_eq!(
        err,
        Refusal::Conflict("job 001 already has an approve or retry queued".into())
    );
    assert_eq!(jobs(&home), vec![(Action::Approve, "001".to_string())]);
}
```

| Test | Steps | Assertions |
|---|---|---|
| `queue_retry_refuses_after_an_unpadded_approve` (the #120 pair) | `parked(&home, RESUME_MERGE)`; `queue_approve(&home,"good","1",true,None,1)` (`without_review` true, because the parked job has no code review and `approve_refusal(false, false)` would refuse it); `queue_retry(&home,"good","1",2)` | approve `expect("queued")`; retry `expect_err` equals the approve-or-retry `Conflict`; `queue::list(&home).len() == 1` |
| `queue_approve_refuses_an_approve_after_a_retry` | `parked(&home, RESUME_MERGE)`; `queue_retry(&home,"good","1",1)`; `queue_approve(&home,"good","1",true,None,2)` (`without_review` true for the same reason) | approve `expect_err` equals the same `Conflict`; length 1 |
| `queue_run_and_approve_do_not_block_each_other` | `board(&home,"approve",false,"pass")`; `queue_run(&home,"good","1",false,1)` then `queue_approve(&home,"good","1",false,None,2)`; then in a second `fixture()` home with the same board, `queue_approve` at 1 then `queue_run` at 2 | all four `expect("queued")`; each queue has length 2. A run and an approve at the same ms would share a request id, so the times differ |
| `queue_approve_checks_the_board` | `board(&home,"build",false,"pass")` → approve; `board(&home,"approve",true,"pass")` → approve; `board(&home,"approve",false,"pass")` → approve `"2"`; `board(&home,"needs-human",false,"pending")` → approve | `Conflict("job 001 is in stage build, approve needs needs-human or approve")`; `Conflict("job 001 was cancelled")`; `NotFound("no job 002 in project good")`; `Conflict(why)` where `why` is `Board::load(...)`'s job `.approve_refusal(false, false).expect("refused")`; queue empty after all four |
| `queue_approve_passes_the_flags` | `board(&home,"needs-human",false,"pending")`; `queue_approve(&home,"good","1",true,None,1)` | `expect("queued")`; the one request has `without_review == true` |

In the board test, build `why` from the real `approve_refusal`, not a literal. Load the board through `project_layout(&home, "good").expect("layout").board_file()`.

- [ ] **Step 3: Run** `cargo test -j4 -p factory-supervisor --lib -- admit::tests`. Expected: compile error `cannot find function `queue_approve` in this scope`.

- [ ] **Step 4: Implement** `queue_approve` with the checks and messages of the spec's Design section 4, in that order, after `queue_run`. Then change `approve_inner`: keep `enabled(home, name)?` and `job_id(id)?` first, then the body parse and the `one_line` check of the waive reason, then `admit::queue_approve(home, name, id, without_review, b.waive_reason.as_deref(), (deps.now_ms)()).map_err(refusal)?`. Remove what the move leaves unused in `write.rs`, or clippy fails: the private wrapper `fn refuse_if_queued` (364-366), the `Board` and `Stage` imports (line 10; `Layout` stays, `enabled` returns it), and the `let layout =` binding in `approve_inner` (call `enabled(home, name)?;` alone). Keep the `enqueue` helper, which the create route still uses. Keep the 202 reply. In `approve_checks_the_job_id`, add `"0"` and `"000"` to the list of ids that give `bad_request`.

- [ ] **Step 5: Run** `cargo test -j4 -p factory-supervisor --lib`. Expected: all tests pass, 0 failed, including every existing approve route test unchanged.

### Task 3: `factoryd enqueue`, `factoryd status` and the manual

**Files:**
- Modify: `crates/factory-supervisor/src/request.rs` (`impl Action`)
- Modify: `crates/factory-supervisor/src/bin/factoryd.rs:46-83` and `:206`
- Modify: `docs/reference/factory-admin-manual.md:193` and `:196`
- Modify: `docs/guides/factory-user-guide.md:1097-1101`, `:1142` and `:1157-1158`
- Test: `crates/factory-supervisor/src/bin/factoryd.rs` (`tests` module), `crates/factory-supervisor/src/request.rs` (`tests` module)

**Interfaces:**
- Consumes: `admit::queue_run` and `admit::queue_approve` from the earlier tasks.
- Produces: `pub fn name(self) -> &'static str` on `Action`. `enqueue(home, project, job, series, approve, retry, now) -> Result<String, String>` keeps its signature.

- [ ] **Step 1: Read** `crates/factory-supervisor/src/bin/factoryd.rs:40-90`, `:190-215` and its `tests` module (especially `enqueue_retry_conflicts_with_approve_and_series` and `enqueue_retry_refuses_an_unknown_project_and_queues_nothing`), and the manual rows at 193 and 196.

- [ ] **Step 2: Write the failing tests.** In `factoryd.rs`, with a bare temp home as the existing retry test builds it:

```rust
#[test]
fn status_shows_the_action_of_a_queued_request() {
    let t = tempfile::tempdir().expect("tempdir");
    let home = Home::at(t.path().to_path_buf());
    let req = Request::new("p", Action::Approve, "001", false, 1).expect("req");
    queue::enqueue(&home, &req).expect("enqueue");
    let text = status(&home, None);
    assert!(
        text.contains(&format!("  {} p approve job 001\n", req.id)),
        "{text}"
    );
}
```

| Test | Steps | Assertions |
|---|---|---|
| `enqueue_refuses_an_unknown_project_for_run_and_approve` | `enqueue(&home,"nope","1",false,false,false,1)` and `enqueue(&home,"nope","1",false,true,false,1)` | both `expect_err` equal `"no project named nope"`; `queue::list(&home).is_empty()` |
| `enqueue_approve_conflicts_with_series` | `Cli::try_parse_from(["factoryd","enqueue","--project","p","--job","001","--approve","--series"])` | `.err().expect("refused").kind() == clap::error::ErrorKind::ArgumentConflict` |

In `request.rs` tests, `action_name_matches_the_serde_name`: for each of the four actions, `serde_json::to_string(&a).expect("json") == format!("\"{}\"", a.name())`.

- [ ] **Step 3: Run** `cargo test -j4 -p factory-supervisor --bin factoryd` and `cargo test -j4 -p factory-supervisor --lib -- request::tests`. Expected: compile error `no method named `name` found for enum `Action`` in `request.rs`; in `factoryd.rs`, `enqueue_refuses_an_unknown_project_for_run_and_approve` fails because the run returns `Ok`, `enqueue_approve_conflicts_with_series` fails because the parse succeeds, and the status test fails because the line has no action.

- [ ] **Step 4: Implement.**
  - `Action::name` returns `"run"`, `"approve"`, `"retry"`, `"intake"`.
  - `#[arg(long, conflicts_with = "series")]` on `approve`.
  - In `enqueue`, keep the retry branch; the approve branch calls `admit::queue_approve(home, project, job, false, None, now)`, the run branch `admit::queue_run(home, project, job, series, now)`, each `.map_err(|e| e.to_string())`. `Request` and `Action` from `use factory_supervisor::request::{Action, Request};` (line 12) are then used only by tests: move that `use` line into `mod tests`, or clippy fails on an unused import and the status test loses `Request`.
  - The status line becomes `"  {} {} {} job {}\n"` with `r.id, r.project, r.action.name(), r.job`.
  - Manual row 193: add `The job id is padded, so --job 7 queues job 007. An approve or retry is refused while an approve or retry of that job is queued, and a run while a run of it is queued. An approve is refused for the same job states as the API.`
  - Manual paragraph 196: after `--job takes a one to six digit ID.` add `Job 0 is refused.` and say that `factoryd status` shows each queued request's action.
  - User guide: item 2 of the status list (1099) says each queued line shows id, project, action and job. The `--job` row (1142) adds that the id is padded and 0 is refused. Replace line 1157 with: `Without --retry, enqueue checks that the project exists and refuses an unknown one. It never needs factoryd to be running; if none runs, the request waits in the queue until one starts.` Replace line 1158 with: `--series together with --approve is refused by the argument parser.` Add a line after it: `An approve is refused for the same job states as the dashboard's Approve button and while an approve or retry of the job is queued; a run is refused while a run of the job is queued.`

- [ ] **Step 5: Run** `cargo test -j4 -p factory-supervisor` and `cargo clippy -j4 -p factory-supervisor --all-targets -- -D warnings`. Expected: all tests pass, 0 failed; clippy prints no warnings.

Next plan: none
