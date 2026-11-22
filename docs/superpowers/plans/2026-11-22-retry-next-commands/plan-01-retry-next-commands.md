# A retry through factoryd names the park's next commands: Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-22-retry-next-commands-design.md
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** `factoryd enqueue --retry` and the HTTP retry route refuse a park without a resume stage with the same text and next commands as `factory retry`.

**Commit:** fix(supervisor): name park commands on retry

**Architecture:** The engine's private `park_commands` moves to `factory-core/src/park.rs` as `park_commands` plus `no_resume_refusal`, both taking the job dir, the job id and the board's park kind. The engine's `retry_inner` and the supervisor's `admit::queue_retry` call `no_resume_refusal`. The engine change is a plain function call in `retry.rs`; it sits behind no port (`AgentRunner` and `Tracker` are not involved).

**Tech Stack:** Rust; crates `factory-core`, `factory-engine`, `factory-supervisor`. No board, ledger or event fields change.

## File Structure

| File | Change |
|---|---|
| `crates/factory-core/src/park.rs` | Add `park_commands` and `no_resume_refusal` after `impl ParkRecord`; tests. |
| `crates/factory-core/src/lib.rs:85-87` | Export both from `pub use park::{...}`. |
| `crates/factory-engine/src/retry.rs` | Remove the private `park_commands` (8-21); the no-resume refusal (48-53) calls `no_resume_refusal`; drop imports left unused. |
| `crates/factory-supervisor/src/admit.rs:193-197` | The no-resume refusal calls `no_resume_refusal`; tests. |
| `crates/factory-supervisor/src/api/write.rs` | Test `retry_without_resume_is_409_and_queues_nothing` expects the new text. |
| `README.md:549` | The example text. |
| `docs/guides/factory-user-guide.md:1156`, `docs/reference/software-factory.md:346`, `docs/reference/factory-admin-manual.md:193` | Say the refusal names the park's next commands. |

## Review Focus

1. A `park.json` that exists but does not parse: a user expects the kind's commands, not an empty list. Pinned by `park_commands_fall_back_to_the_kind`, which also writes a broken file.
2. The engine text must not change for any park. Pinned by the existing `factory-engine/tests/retry.rs` and `factory-cli/tests/retry_cmd.rs` tests, which must pass unchanged.
3. The supervisor must read the job folder of the right project: the park file lives under the project's state, not the repo. Pinned by `queue_retry_names_the_commands_of_the_park_file`, which writes the file through `project_layout(...).job_dir(...)`.
4. A board job with no `park_kind` at all reads as `Other`. Pinned by `park_commands_read_an_unknown_kind_as_other`.

---

### Task 1: Shared `park_commands` and `no_resume_refusal` in factory-core

**Files:**
- Modify: `crates/factory-core/src/park.rs`, `crates/factory-core/src/lib.rs`
- Test: `crates/factory-core/src/park.rs` (`tests` module, line 337)

**Interfaces:**
- Consumes: `ParkRecord::read(dir: &Path) -> Option<ParkRecord>`, `ParkRecord::new(kind, reason, id, round, at, parts)`, `ParkRecord::write(&self, dir: &Path)`, `ParkKind::next(self, id: &str) -> Vec<String>`, all in `park.rs`.
- Produces: `pub fn park_commands(dir: &Path, id: &str, park_kind: Option<&str>) -> Vec<String>` and `pub fn no_resume_refusal(dir: &Path, id: &str, park_kind: Option<&str>) -> String`, exported as `factory_core::park_commands` and `factory_core::no_resume_refusal`. The second task calls `no_resume_refusal`.

- [ ] **Step 1: Read** `crates/factory-core/src/park.rs` (all), `crates/factory-core/src/lib.rs:80-90`, `crates/factory-engine/src/retry.rs:1-60`, and the spec's Design section 1.

- [ ] **Step 2: Write the failing tests** in the `tests` module of `park.rs`:

```rust
#[test]
fn no_resume_refusal_joins_the_commands() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert_eq!(
        no_resume_refusal(dir.path(), "001", Some("nothing-changed")),
        "job 001 has no stage to retry at; use factory rework 001 <note>, factory cancel 001"
    );
}
```

| Test | Inputs | Assertion |
|---|---|---|
| `park_commands_prefer_the_park_file` | a `ParkRecord::new(ParkKind::Other, "r", "001", 1, 1, vec![])` with `next` set to `vec!["factory rework 001 <note>".to_string()]`, written with `.write(dir.path()).expect("write")`; kind `Some("other")` | `park_commands(dir.path(), "001", Some("other")) == vec!["factory rework 001 <note>".to_string()]` |
| `park_commands_fall_back_to_the_kind` | three dirs: empty; a record with `next` set to `vec![]`; a `park.json` (`PARK_FILE`) holding `"not json"`; kind `Some("nothing-changed")` each time | each equals `ParkKind::NothingChanged.next("001")` |
| `park_commands_read_an_unknown_kind_as_other` | empty dir; kinds `None` and `Some("nonsense")` | each equals `ParkKind::Other.next("001")` |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-core park::tests`. Expected: compile errors `cannot find function `park_commands` in this scope` and `cannot find function `no_resume_refusal` in this scope`.

- [ ] **Step 4: Implement** both functions after `impl ParkRecord`, from the spec's Design section 1. Read the kind with `park_kind.and_then(ParkKind::from_name).unwrap_or(ParkKind::Other)` (`ParkKind::from_name`, `park.rs:116-138`). Each gets a one-line doc comment. Add both names to the `pub use park::{...}` list in `lib.rs`, sorted: `next_with_resume, no_resume_refusal, park_commands, park_summary, ...`.

- [ ] **Step 5: Run** `cargo test -j4 -p factory-core park`. Expected: all `park` tests pass, 0 failed.

### Task 2: The engine and the supervisor use `no_resume_refusal`

**Files:**
- Modify: `crates/factory-engine/src/retry.rs`
- Modify: `crates/factory-supervisor/src/admit.rs:193-197` and its `tests` module
- Modify: `crates/factory-supervisor/src/api/write.rs` (test `retry_without_resume_is_409_and_queues_nothing`)
- Modify: `README.md:549`, `docs/guides/factory-user-guide.md:1156`, `docs/reference/software-factory.md:346`, `docs/reference/factory-admin-manual.md:193`

**Interfaces:**
- Consumes: `factory_core::no_resume_refusal(dir: &Path, id: &str, park_kind: Option<&str>) -> String` from the factory-core task; `Driver::job_dir(&self, job: &Job) -> PathBuf` (`factory-engine/src/compat.rs:602`); `Layout::job_dir(&self, dir: &str) -> PathBuf`; in the admit tests, `fixture()`, `parked(&home, extra)` and `project_layout`, as `queue_retry_refuses_a_park_without_resume` uses them.
- Produces: no new names.

- [ ] **Step 1: Read** `crates/factory-engine/src/retry.rs:1-60`, `crates/factory-supervisor/src/admit.rs` around `queue_retry` (165-215) and its test `queue_retry_refuses_a_park_without_resume`, the test `retry_without_resume_is_409_and_queues_nothing` in `api/write.rs` (about line 1306), `factory-engine/tests/retry.rs:280-292`, `factory-cli/tests/retry_cmd.rs:70-87` and `README.md:549`.

- [ ] **Step 2: Write the failing tests.** In `admit.rs`, change `queue_retry_refuses_a_park_without_resume` to expect

```rust
Refusal::Conflict(
    "job 001 has no stage to retry at; use factory approve 001, factory rework 001 <note>, factory cancel 001".into()
)
```

and add:

```rust
#[test]
fn queue_retry_names_the_commands_of_the_park_file() {
    let (_t, home) = fixture();
    parked(&home, r#""park_kind":"other""#);
    let dir = project_layout(&home, "good").expect("layout").job_dir("001-first");
    std::fs::create_dir_all(&dir).expect("job dir");
    let kind = factory_core::ParkKind::Other;
    let mut record = factory_core::ParkRecord::new(kind, "r", "001", 1, 1, vec![]);
    record.next = vec!["factory rework 001 <note>".to_string()];
    record.write(&dir).expect("park file");
    let err = queue_retry(&home, "good", "1", 1).expect_err("refused");
    assert_eq!(
        err,
        Refusal::Conflict("job 001 has no stage to retry at; use factory rework 001 <note>".into())
    );
    assert!(queue::list(&home).is_empty());
}
```

In `api/write.rs`, `retry_without_resume_is_409_and_queues_nothing` expects the message `job 001 has no stage to retry at; use factory approve 001, factory rework 001 <note>, factory cancel 001`.

- [ ] **Step 3: Run** `cargo test -j4 -p factory-supervisor --lib -- retry`. Expected: `queue_retry_refuses_a_park_without_resume`, `queue_retry_names_the_commands_of_the_park_file` and `retry_without_resume_is_409_and_queues_nothing` fail, each showing the old text `job 001 has no resume stage, see its park for the next command` on the left.

- [ ] **Step 4: Implement.**
  - `retry.rs`: delete the private `park_commands`; the `let Some(resume) = job.park_resume else` branch returns `EngineError::Refused(factory_core::no_resume_refusal(&driver.job_dir(&job), &job.id, job.park_kind.as_deref()))`. `Job`, `ParkKind` and `ParkRecord` were used only by the removed function, so line 3 becomes `use factory_core::{ResumeAt, Stage};`, or clippy fails on unused imports.
  - `admit.rs`: the same branch in `queue_retry` returns `conflict(factory_core::no_resume_refusal(&layout.job_dir(&job.dir), &id, job.park_kind.as_deref()))`.
  - `README.md:549`: replace ``(`job 020 has no resume stage, see its park for the next command`)`` with ``(for example `job 020 has no stage to retry at; use factory rework 020 <note>, factory cancel 020`)``.
  - `docs/guides/factory-user-guide.md:1156`: after `a park with no resume stage` add ` (naming that park's next commands)`. `docs/reference/software-factory.md:346`: after `Refused for a park with no resume stage` add ` (the refusal names that park's next commands)`.
  - `docs/reference/factory-admin-manual.md:193`: after `it refuses a job that cannot be retried` add ` (a park without a resume stage names its next commands)`.
  - Keep new lines at rustfmt width.

- [ ] **Step 5: Run** `cargo test -j4 -p factory-supervisor`, `cargo test -j4 -p factory-engine --test retry`, `cargo test -j4 -p factory-cli --test retry_cmd` and `cargo clippy -j4 -p factory-core -p factory-engine -p factory-supervisor --all-targets -- -D warnings`. Expected: all tests pass, 0 failed, with the engine and CLI retry tests unchanged; clippy prints no warnings.

Next plan: none
