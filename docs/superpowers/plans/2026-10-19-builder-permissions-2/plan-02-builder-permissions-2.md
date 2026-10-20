# Builder Permissions 2, Plan 2: The driver-only file guard

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Restore `plan.md`, `spec.md` and `progress.md` and park the job when a builder run changes any of them, and reject shell operators in `builder_allow` entries.

**Architecture:** One engine method wraps `run_agent` for the builder role: it reads the three files before the run, compares them after it, restores the changed ones and parks through `park_with`. It sits in `crates/factory-engine/src/permissions.rs`, next to the allowlist code from plan 1, and reads the job folder directly like `spec_line` in `specs.rs` does, so it needs no port. The `builder_allow` change is one more check in the validator in `factory-core`.

**Tech Stack:** Rust 2021, `factory-engine` and `factory-core`.

**Spec:** docs/superpowers/specs/2026-10-19-builder-permissions-design.md

## Global Constraints

- Always `-j4`; never `--workspace` or `--all`; never set `CARGO_TARGET_DIR`.
- The guard holds with `builder_permissions = true` and with `false`.
- The guarded files are `plan.md`, `spec.md` and `progress.md` in `Engine.job_dir`. A file that is absent before and after the run counts as unchanged. A file that appears during the run counts as changed and is removed again.
- On a change the driver restores every changed file, then parks with `ParkKind::Other` and the reason `builder changed <file>`, where `<file>` is the first changed name in the order `plan.md`, `spec.md`, `progress.md`. The park path is `Engine::park_with`; the board, ledger and events formats do not change.
- Only the two builder call sites in `crates/factory-engine/src/build.rs` use the guard. Reviewer, committer, message and KB runs do not.
- A `builder_allow` entry containing `&&`, `||`, `;`, `|`, `>`, `<`, a backtick or `$(` is rejected with `must be one plain command, without shell operators`.

## Review Focus

- A builder that rewrites `plan.md` leaves `plan.md` with its original text and parks the job (Task 1 test `builder_changing_plan_md_is_restored_and_parked`).
- A builder that appends to `progress.md` has it restored (Task 1 test `builder_changing_progress_md_is_restored_and_parked`).
- A builder that creates a missing `spec.md` has it removed again (Task 1 test `a_created_job_file_is_removed_and_parks`).
- A run that touches none of the files does not park, and a job without a spec file stays without one (Task 1 tests `unchanged_job_files_do_not_park`, `absent_job_file_stays_absent`).
- The guard also works when the permission flags are off (Task 1 test `guard_holds_with_builder_permissions_false`).
- `Bash(make && rm -rf x:*)`, `Bash(make | tee f:*)` and ``Bash(make `id`:*)`` are rejected, and `Bash(make test:*)` is accepted (Task 2 tests).

---

## File Structure

- `crates/factory-engine/src/permissions.rs` (modify): `run_builder_guarded`, the snapshot type and the unit-free helpers.
- `crates/factory-engine/src/build.rs` (modify): the two builder call sites use it.
- `crates/factory-engine/tests/driver_files_guard.rs` (new): the guard tests.
- `crates/factory-core/src/config.rs` (modify): the operator check and its tests.

### Task 1: Guard the driver-only files

**Files:**
- Modify: `crates/factory-engine/src/permissions.rs`, `crates/factory-engine/src/build.rs`
- Create: `crates/factory-engine/tests/driver_files_guard.rs`

**Interfaces:**
- Produces: `impl Engine<'_> { pub(crate) fn run_builder_guarded(&self, spec: AgentSpec) -> Result<Result<AgentRun, StageOutcome>, EngineError> }`. `Ok(Ok(run))` is a normal run; `Ok(Err(outcome))` is the parked outcome the caller returns as `Ok(outcome)`.
- Consumes: `Engine::run_agent`, `Engine::park_with(ParkKind::Other, reason, Vec::new())`, `Engine.job_dir`.
- Changes: the two `let run = self.run_agent(spec)?;` lines for the builder in `crates/factory-engine/src/build.rs` become a `match self.run_builder_guarded(spec)? { Ok(run) => run, Err(parked) => return Ok(parked) }`.

- [ ] **Step 1: Read** `run_agent` in `crates/factory-engine/src/ports.rs`, `park_with` in `crates/factory-engine/src/park.rs`, the two builder run sites in `crates/factory-engine/src/build.rs` (`build_stage` and the task builder), and `change_effect`, `committing_fixture` and `two_task_fixture` in `crates/factory-engine/tests/build_checks.rs` and `common/mod.rs` (an effect is a closure that receives the `AgentSpec` and returns the reply text).
- [ ] **Step 2: Write the failing tests** in `driver_files_guard.rs`, with `mod common;` as the neighbouring test files do. Each test uses `committing_fixture()` and a `scripted_auto` runner whose first effect edits a job-folder file through `fx.job_dir` and returns `"done"`, then calls `fx.engine(&runner).build_stage()`.
  - Full example:
    ```rust
    #[test]
    fn builder_changing_plan_md_is_restored_and_parked() {
        let fx = common::committing_fixture();
        let original = std::fs::read_to_string(&fx.plan_file).expect("plan");
        let plan = fx.plan_file.clone();
        let runner = common::scripted_auto(vec![Box::new(move |_spec| {
            std::fs::write(&plan, "# rewritten\n").expect("write");
            "done".to_string()
        })]);
        match fx.engine(&runner).build_stage().expect("stage") {
            factory_engine::StageOutcome::NeedsHuman(reason) => {
                assert_eq!(reason, "builder changed plan.md")
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(std::fs::read_to_string(&fx.plan_file).expect("plan"), original);
    }
    ```
  - The others, by name and inputs:

    | Test | Builder effect | Expected |
    |---|---|---|
    | `builder_changing_progress_md_is_restored_and_parked` | appends `Task 9: Done` to `progress.md` | parked with `builder changed progress.md`; the file equals its text from before the run |
    | `a_created_job_file_is_removed_and_parks` | writes `spec.md` in a fixture that has none | parked with `builder changed spec.md`; the file does not exist afterwards |
    | `unchanged_job_files_do_not_park` | edits only `a.txt` in the worktree (use `change_effect` from `build_checks.rs`, copied as a small helper) | `StageOutcome::Advanced(_)` |
    | `absent_job_file_stays_absent` | edits only `a.txt` | no `spec.md` in the job folder afterwards, no park |
    | `guard_holds_with_builder_permissions_false` | as the plan test, with `fx.config.builder_permissions = false` set on a `let mut fx` | parked with `builder changed plan.md`, file restored |
    | `two_changed_files_are_both_restored_and_the_first_is_named` | rewrites `progress.md` and `plan.md` | reason `builder changed plan.md`; both files restored |
- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine --test driver_files_guard` and expect FAIL (the changing tests see `Advanced` instead of `NeedsHuman`).
- [ ] **Step 4: Implement** `run_builder_guarded` (read the three files as bytes before the run, `None` when absent; run; compare; restore or remove the changed ones; park) and switch the two call sites in `build.rs`.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine` and expect PASS, then `cargo test -j4 -p factory-cli` (the end-to-end tests run builders) and expect PASS.

### Task 2: Reject shell operators in `builder_allow`

**Files:**
- Modify: `crates/factory-core/src/config.rs`

**Interfaces:**
- Changes: private `check_builder_allow(entry: &str) -> Result<(), String>` also fails with `must be one plain command, without shell operators` when the entry contains `&&`, `||`, `;`, `|`, `>`, `<`, a backtick or `$(`. The check runs after the `Bash(...)` form check and before the program check, so the existing messages keep their order.

- [ ] **Step 1: Read** `check_builder_allow` and `builder_allow_rejects_forbidden_and_malformed_entries` in `crates/factory-core/src/config.rs`.
- [ ] **Step 2: Write the failing tests** in the `tests` module.
  - `builder_allow_rejects_shell_operators`: for each of `Bash(make && rm -rf x:*)`, `Bash(make || true:*)`, `Bash(make; ls:*)`, `Bash(make | tee f:*)`, `Bash(make > f:*)`, `Bash(make < f:*)`, ``Bash(make `id`:*)``, `Bash(make $(id):*)`, parse `builder_allow = [<entry as a TOML string via {entry:?}>]` and assert `Config::parse(...)` is `Err` whose text contains `builder_allow:` and `shell operators`.
  - `builder_allow_still_accepts_plain_arguments`: `Config::parse` accepts `Bash(make test:*)`, `Bash(git log --format=%h)` and `Bash(cargo test -j4 -p x:*)`.
- [ ] **Step 3: Run** `cargo test -j4 -p factory-core builder_allow` and expect FAIL (`builder_allow_rejects_shell_operators`: `Config::parse` returns `Ok`).
- [ ] **Step 4: Implement** the check.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-core`, `cargo clippy -j4 -p factory-core -p factory-engine --all-targets -- -D warnings` and `cargo fmt -p factory-core -p factory-engine -- --check`. Expect PASS and no warnings.

Next plan: none
