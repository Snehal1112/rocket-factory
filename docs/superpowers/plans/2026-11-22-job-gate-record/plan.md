# Job Gate Record Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The driver writes the result of all 18 gates of a job to `gates.jsonl` when the project's `gate_record` setting is on, and the dashboard card shows them as a strip that opens into a list.

**Architecture:** `factory-core` holds the line format and the setting. `factory-engine` writes pass and note lines at each gate site and every fail line from the two park choke points, through one best-effort wrapper that never changes a job's outcome. The dashboard server reads the file, reduces it to the latest state per gate, and `board.js` draws the strip.

**Tech Stack:** Rust (workspace crates and the separate `dashboard/` workspace), plain JS with `node:test`.

**Spec:** `docs/superpowers/specs/2026-11-22-job-gate-record-design.md`

## Global Constraints

- Nothing is committed. All work is in the sibling worktree `../rocket-factory-gate-record` on branch `gate-record`; the main checkout stays clean.
- `gate_record` defaults to `false`. With it off, no code path writes `gates.jsonl`, and a card with no record is exactly as today.
- 18 gate keys, card order: `spec-check`, `spec-review`, `series`, `machine`, `builder-guard`, `builder`, `checks`, `format`, `lint`, `no-checks`, `no-tests`, `budget`, `diff-cap`, `review`, `risk`, `final-gate`, `merge`, `approve`.
- Stages, in card order: spec (`spec-check`, `spec-review`), start (`series`, `machine`), build (`builder-guard`, `builder`), checks (`checks`, `format`, `lint`, `no-checks`, `no-tests`), review (`budget`, `diff-cap`, `review`, `risk`), merge (`final-gate`, `merge`, `approve`).
- Results: `pass`, `fail`, `note`, `waived`, `skipped`, `running`, `waiting`. Once-scoped gates: `spec-check`, `spec-review`, `series`.
- `round` in every line is `max(board round, 1)`; the dashboard compares with `max(job.round, 1)`.
- `detail` is at most 300 characters, cut with `…`.
- A gate line is never written inside a `self.update` or `update_job` closure or under the merge lock, and an append failure only logs `gates.record_error`.
- Board JSON does not change. Never shell out to `git`. Paths come from `Layout` and the `GATES_FILE` constant.
- Cargo: `-j4`, per crate (`-p <crate>` or `--manifest-path dashboard/Cargo.toml`), never `--workspace`, `--all` or `CARGO_TARGET_DIR`.
- Comments are short full sentences. Tests use `.expect("...")` and `expect_err`.

## Review Focus

1. A first-round job: the board says round 0 and the line says 1; the card must show the gate as reached, not "not reached". Pinned by `first_round_lines_count_for_a_board_round_of_zero` (dashboard server) and `a_first_round_line_has_round_one` (engine).
2. A park on a job that is already parked (`park_after_sync`, then the no-op `park_with_parts` at `sync.rs:122`): one fail line, not two. Pinned by `a_second_park_writes_no_second_fail_line`.
3. `gate_record` off: a full auto-merge writes no file at all, and the old card is unchanged. Pinned by `no_gate_file_when_the_setting_is_off` (engine) and `a_card_without_a_record_has_no_strip` (JS).
4. A user opens the list on a running card and the board polls: the list stays open. Pinned by `the_gate_list_is_a_details_element` (JS), since `patch.js` keeps `<details open>`.
5. The append fails (the path is a folder): the job still merges with the same outcome. Pinned by `a_gate_file_that_is_a_folder_changes_nothing`.

---

### Task 1: The worktree

**Files:** none in the repo.

- [ ] **Step 1:** From the main checkout run `git worktree add ../rocket-factory-gate-record -b gate-record`. Expected: `Preparing worktree (new branch 'gate-record')`.
- [ ] **Step 2:** Copy the untracked spec and this plan into the worktree at the same paths, so the worktree's builder sees them: `cp docs/superpowers/specs/2026-11-22-job-gate-record-design.md ../rocket-factory-gate-record/docs/superpowers/specs/` and `mkdir -p ../rocket-factory-gate-record/docs/superpowers/plans/2026-11-22-job-gate-record && cp docs/superpowers/plans/2026-11-22-job-gate-record/plan.md ../rocket-factory-gate-record/docs/superpowers/plans/2026-11-22-job-gate-record/`.
- [ ] **Step 3:** In the worktree run `cargo check -j4 -p factory-core`. Expected: finishes with no errors. Every later task runs in the worktree.

### Task 2: Core format and the setting

**Files:**
- Create: `crates/factory-core/src/gate_record.rs`
- Modify: `crates/factory-core/src/lib.rs` (`pub mod gate_record;` between `gate` and `home`), `crates/factory-core/src/config.rs` (field next to `minor_tickets`, default in `Default`), `crates/factory-core/src/settings_block.rs:80` (add `("gate_record", None)`), `docs/reference/project-toml-reference.md` (a row next to `minor_tickets`)
- Test: `crates/factory-core/src/gate_record.rs` (`mod tests`), `crates/factory-core/src/config.rs` (`mod tests`)

**Interfaces:**
- Produces:
  - `pub const GATES_FILE: &str = "gates.jsonl";`
  - `pub const MAX_DETAIL: usize = 300;`
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "kebab-case")] pub enum GateKey { SpecCheck, SpecReview, Series, Machine, BuilderGuard, Builder, Checks, Format, Lint, NoChecks, NoTests, Budget, DiffCap, Review, Risk, FinalGate, Merge, Approve }` with `pub const ALL: [GateKey; 18]` in card order, `pub fn as_str(self) -> &'static str`, `pub fn stage(self) -> &'static str` (`spec`, `start`, `build`, `checks`, `review`, `merge`) and `pub fn once(self) -> bool`.
  - `#[serde(rename_all = "kebab-case")] pub enum GateResult { Pass, Fail, Note, Waived, Skipped, Running, Waiting }`.
  - `pub struct GateLine { pub v: u32, pub gate: GateKey, pub result: GateResult, pub round: u32, #[serde(skip_serializing_if = "Option::is_none")] pub head: Option<String>, pub at: u64, pub detail: String }` and `pub fn GateLine::new(gate, result, round: u32, head: Option<String>, at: u64, detail: &str) -> GateLine`, which sets `v: 1`, `round.max(1)` and cuts `detail` with `cut_detail`.
  - `pub fn cut_detail(text: &str) -> String`: at most `MAX_DETAIL` characters, the last one `…` when cut; newlines become spaces.
  - `pub fn append(dir: &Path, line: &GateLine) -> Result<(), CoreError>` and `pub fn read(dir: &Path) -> Vec<GateLine>`.
  - `Config::gate_record: bool`, default `false`.

- [ ] **Step 1: Read** `crates/factory-core/src/park.rs:1-60` and `:250-300` (`ParkRecord`, `write`, `read`), `ledger.rs:60-90` (`append`), `error.rs` (`CoreError::io`), `lib.rs`, `config.rs:150-170` and `:270-290`, the test `minor_tickets_is_on_by_default_and_can_be_turned_off`, and spec sections "The gates", 1 and 1a.

- [ ] **Step 2: Write the failing tests.** In `gate_record.rs`:

```rust
#[test]
fn read_skips_broken_blank_newer_and_unknown_lines() {
    let dir = tempfile::tempdir().expect("tempdir");
    let good = GateLine::new(GateKey::NoChecks, GateResult::Pass, 2, Some("b83bfbc".into()), 7, "all 7 code paths covered");
    let text = format!(
        "{}\n\n{{not json\n{}\n{}\n",
        serde_json::to_string(&good).expect("json"),
        r#"{"v":2,"gate":"checks","result":"pass","round":1,"at":1,"detail":""}"#,
        r#"{"v":1,"gate":"no-such-gate","result":"pass","round":1,"at":1,"detail":""}"#,
    );
    std::fs::write(dir.path().join(GATES_FILE), text).expect("write");
    assert_eq!(read(dir.path()), vec![good]);
}
```

| Test | Input | Assertion |
|---|---|---|
| `a_line_round_trips` | `GateLine::new(GateKey::Review, GateResult::Fail, 3, None, 9, "code CHANGES, 2 blockers")` through `append` then `read` | `read == vec![line]`; the file text contains `"gate":"review"` and no `"head"` |
| `append_after_a_cut_line_starts_a_new_line` | file holds `{"v":1,"gate":"chec` (no newline); `append` one good line | `read` returns only the good line; file text ends with `\n` |
| `append_into_a_missing_folder_fails_and_creates_nothing` | `append(&tmp.path().join("gone"), &line)` | `expect_err`; `!tmp.path().join("gone").exists()` |
| `cut_detail_keeps_300_characters` | `"é".repeat(400)`, and `"a\nb"` | first has `chars().count() == 300` and ends with `…`; second is `"a b"` |
| `round_zero_is_written_as_one` | `GateLine::new(.., 0, ..)` | `.round == 1` |
| `all_keys_are_in_card_order_with_their_stages` | `GateKey::ALL` | length 18; `ALL.map(as_str)` equals the Global Constraints order; `ALL.map(stage)` gives 2 spec, 2 start, 2 build, 5 checks, 4 review, 3 merge; `once()` is true only for `spec-check`, `spec-review`, `series` |

In `config.rs`: `gate_record_is_off_by_default_and_can_be_turned_on`: `!Config::default().gate_record`; `Config::parse("gate_record = true").expect("ok").gate_record`; `Config::parse("gate_record = \"yes\"").is_err()`.

- [ ] **Step 3: Run** `cargo test -j4 -p factory-core gate_record`. Expected: compile errors, `could not find `gate_record`` and `no field `gate_record``.

- [ ] **Step 4: Implement** the Interfaces. `append` opens with `OpenOptions::new().create(true).append(true)` and never calls `create_dir_all`; when the file exists, is not empty and its last byte is not `\n`, it writes `\n` first; the line is written with one `write_all` of `json + "\n"`. Errors go through `CoreError::io(path, e)`. `read` returns an empty list for a missing file. Add the reference-doc row: `| gate_record | bool | false | Write gates.jsonl in each job folder, one line per gate result, for the dashboard's gate strip. |`.

- [ ] **Step 5: Run** `cargo test -j4 -p factory-core`. Expected: all pass, 0 failed.

### Task 3: Engine wrapper and the park choke points

**Files:**
- Create: `crates/factory-engine/src/gate_record.rs`
- Modify: `crates/factory-engine/src/lib.rs` (`mod gate_record;`, re-export `record_gate`, `GateTarget`), `crates/factory-engine/src/park.rs:176-235`, `crates/factory-engine/src/merge.rs:43-100`
- Test: `crates/factory-engine/src/gate_record.rs` (`mod tests`), `crates/factory-engine/tests/gate_record.rs` (new)

**Interfaces:**
- Consumes: `GateKey`, `GateResult`, `GateLine`, `append`, `read`, `GATES_FILE`, `Config::gate_record` from the core task.
- Produces:
  - `pub enum GateTarget { FromKind, Key(GateKey), NoFailLine }`.
  - `pub(crate) fn fail_key(kind: ParkKind, stage_before: Option<Stage>, target: GateTarget) -> Option<(GateKey, GateResult)>`: the spec's park-kind table (section 2); `SeriesWaiting` gives `(Series, Waiting)`; `ReviewFailed` and `ChangesRemain` give `None`; `Other` uses the stage (build → `Builder`, review → `Review`, approve → `Merge`, else `None`); `Key(k)` gives `(k, Fail)`; `NoFailLine` gives `None`.
  - `pub(crate) fn offers_approve(next: &[String]) -> bool`: any entry starts with `factory approve`.
  - `impl Engine { pub(crate) fn gate(&self, key: GateKey, result: GateResult, detail: &str) }`: does nothing when `!self.config.gate_record`; round and head from `self.job().ok()` (round 1, no head, when it fails), head cut to 7 characters with `short_sha`; `at` from `now()`; append to `self.job_dir`; on `Err` logs `tracing::warn!(event = "gates.record_error", job = %self.job_id, error = %e)`.
  - `pub fn record_gate(driver: &Driver, job: &Job, key: GateKey, result: GateResult, detail: &str)`: the same for driver code, with `driver.config().gate_record`, `job.round.max(1)`, `job.head`, `driver.job_dir(job)`.
  - `Engine::park_with_target(kind, reason: String, parts, resume: Option<ResumeAt>, target: GateTarget) -> Result<StageOutcome, EngineError>`; `park_with_resume` becomes a call of it with `GateTarget::FromKind`.
  - `pub fn park_with_parts_target(driver, id, reason: &str, kind, parts, resume, target: GateTarget) -> Result<(), EngineError>`; `park_with_parts_resume` calls it with `FromKind`.

- [ ] **Step 1: Read** `crates/factory-engine/src/park.rs:150-260`, `merge.rs:36-110`, `ports.rs:130-215` (`job`, `update`, `short_sha`, `now`), `compat.rs:410-420` and `:625-640` (`Driver::config`, `layout`, `job`, `job_dir`), `tests/common/mod.rs:60-200` (`Fixture::new`, `scripted`, `ScriptedRunner`), and spec sections 1 and 2.

- [ ] **Step 2: Write the failing tests.** Unit tests in `src/gate_record.rs`:

```rust
#[test]
fn park_kinds_map_to_their_gates() {
    use GateResult::*;
    let cases = [
        (ParkKind::BuilderStopped, None, Some((GateKey::Builder, Fail))),
        (ParkKind::BuilderTampered, None, Some((GateKey::BuilderGuard, Fail))),
        (ParkKind::ChecksFailed, None, Some((GateKey::Checks, Fail))),
        (ParkKind::DiffTooLarge, None, Some((GateKey::DiffCap, Fail))),
        (ParkKind::RiskyDiff, None, Some((GateKey::Risk, Fail))),
        (ParkKind::SeriesWaiting, None, Some((GateKey::Series, Waiting))),
        (ParkKind::MachineWaiting, None, Some((GateKey::Machine, Fail))),
        (ParkKind::ReviewFailed, None, None),
        (ParkKind::ChangesRemain, None, None),
        (ParkKind::Other, Some(Stage::Build), Some((GateKey::Builder, Fail))),
        (ParkKind::Other, Some(Stage::Review), Some((GateKey::Review, Fail))),
        (ParkKind::Other, Some(Stage::Approve), Some((GateKey::Merge, Fail))),
        (ParkKind::Other, Some(Stage::Spec), None),
    ];
    for (kind, stage, want) in cases {
        assert_eq!(fail_key(kind, stage, GateTarget::FromKind), want, "{kind:?} {stage:?}");
    }
    assert_eq!(fail_key(ParkKind::Other, Some(Stage::Review), GateTarget::Key(GateKey::Merge)), Some((GateKey::Merge, Fail)));
    assert_eq!(fail_key(ParkKind::Other, Some(Stage::Review), GateTarget::NoFailLine), None);
}
```

The test also covers every remaining kind of the spec table (`BuilderBlocked`, `BuilderCommitted`, `NothingChanged`, `CommitFailed` → `Builder`; `NoChecks`, `NoTests`, `BudgetStopped` → their keys).

Integration tests in `tests/gate_record.rs`, each building `Fixture::new()` and setting `fx.config.gate_record = true` unless the test says otherwise (read how `tests/common/mod.rs` builds `config` and mutate it the same way other tests change config fields):

| Test | Steps | Assertions on `factory_core::gate_record::read(&fx.job_dir)` |
|---|---|---|
| `a_park_writes_its_fail_line_and_approve_waiting` | engine parks with `ParkKind::NoTests` via a builder script that adds code and no test, with `require_tests = true` | contains `(NoTests, Fail)` whose detail equals the park reason, then `(Approve, Waiting)` |
| `a_park_without_approve_writes_no_waiting_line` | park `BuilderTampered` (a builder that edits `plan.md`) | `(BuilderGuard, Fail)` present; no `Approve` line |
| `a_second_park_writes_no_second_fail_line` | call `park_with_parts` twice on the same job | exactly one `Fail` line |
| `a_first_round_line_has_round_one` | any park in round 1 | every line has `round == 1` while `fx.job().round` on the board is 0 |
| `no_gate_file_when_the_setting_is_off` | leave `gate_record` false; park as in the first row | `!fx.job_dir.join("gates.jsonl").exists()` |
| `a_gate_file_that_is_a_folder_changes_nothing` | `create_dir_all(fx.job_dir.join("gates.jsonl"))`; park as in the first row | the park result and `park.json` equal the run without the folder; a `gates.record_error` warn is captured with `factory_telemetry::capture` (find an existing test that uses it and copy its setup) |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine gate_record`. Expected: compile error `cannot find function `fail_key``.

- [ ] **Step 4: Implement.** In `park_with_target`, keep `in_park_order` and write the gate lines inside its record closure, right after `write_record(&self.job_dir, &record)`: the `fail_key` line with the reason as detail, then `(Approve, Waiting)` when `offers_approve(&record.next)`. The stage before the park is `job.as_ref().map(|j| j.stage)` from the read at the top. In `park_with_parts_target`, return the stage from inside the `update_job` closure before `set_stage_logged` (change the tuple to `(job.clone(), already, stage_before)`); after the `already` early return and after `write_record`, call `record_gate` the same way. Neither writes inside a closure.

- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine`. Expected: all pass, 0 failed.

### Task 4: Spec gates at job creation

**Files:**
- Modify: `crates/factory-engine/src/gate_record.rs` (`record_spec_gates`), `crates/factory-engine/src/runloop.rs:361-375` (`Validated`), `:~500` (`validate_series`), `:562-640` (`create_job_with_spec_accepting`), `:813` (`create_replacement`), `crates/factory-engine/src/one_job.rs:120-140`
- Test: `crates/factory-cli/tests/draft_cards.rs`, `crates/factory-cli/tests/one_job.rs`

**Interfaces:**
- Consumes: `record_gate` from the wrapper task.
- Produces: `Validated.review_ran: bool`; `pub(crate) fn record_spec_gates(driver: &Driver, jobs: &[Job], validated: Option<&Validated>, spec_review_on: bool)`.

Rules for `record_spec_gates` (spec, "The gates" table): `spec-check` is `pass` with "N plan warnings, M decision warnings", or `waived` with the `accepted` reason, or `skipped` "no plan" when `validated` is `None`, or `skipped` "no spec" when there is no spec text. `spec-review` is `skipped` when `!spec_review_on` (from `driver.config().spec_review`), when there is no spec, or when `!review_ran`; `pass` when `driver.last_spec_review()` says "approved"; `waived` when it says "asked for changes" or "gave no verdict" and `accepted` is set; detail is the review file stem.

- [ ] **Step 1: Read** `runloop.rs:355-640` and `:800-860`, `one_job.rs:60-160`, `spec_check.rs:30-60` and `:220-245`, `compat.rs:370-380`, and in `factory-cli/tests`: `draft_cards.rs` (`new_output_with_a_review`), `one_job.rs` (`plans_folder`, `SPEC`, `set_spec_review_off`, the `--accept-spec` cases) and `common/mod.rs` (`Fixture`, `RoleRunner`). Find where a CLI test sets a project setting and set `gate_record = true` the same way.

- [ ] **Step 2: Write the failing tests.**

```rust
#[test]
fn new_writes_spec_gates_into_every_job_of_a_series() {
    let fx = Fixture::new();
    // Turn gate_record on the way this file's other tests change a setting.
    let out = /* run `factory new` with a two-plan folder, as `new_output_with_a_review` does */;
    for dir in /* both job folders from the board */ {
        let lines = factory_core::gate_record::read(&dir);
        let keys: Vec<_> = lines.iter().map(|l| (l.gate, l.result)).collect();
        assert_eq!(keys, vec![(GateKey::SpecCheck, GateResult::Pass), (GateKey::SpecReview, GateResult::Pass)], "{out}");
    }
}
```

Fill the two placeholders from the helpers you read; the assertion stays as written.

| Test (file) | Case | Assertion |
|---|---|---|
| `new_one_job_writes_spec_gates` (`one_job.rs`) | `--one-job` combined path, two plans | the one job has `SpecCheck: Pass` and `SpecReview: Pass` |
| `accept_spec_writes_waived_spec_check` (`one_job.rs`) | the existing `--accept-spec` no-user-decision case | `SpecCheck: Waived` with the reason; `SpecReview: Skipped` (no review ran) |
| `spec_review_off_writes_skipped` (`one_job.rs`) | `set_spec_review_off` | `SpecReview: Skipped` |
| `no_spec_gates_when_the_setting_is_off` (`draft_cards.rs`) | default settings | no `gates.jsonl` in the job folder |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-cli --test draft_cards --test one_job`. Expected: compile error `no field `review_ran`` once you add it to a test, or the new tests fail with an empty `read`.

- [ ] **Step 4: Implement.** Set `review_ran` in `validate_series`: `false` for an accepted `NoUserDecision` refusal and when `spec_review` is off, else `true`. Call `record_spec_gates` in `create_job_with_spec_accepting` after `add_series_jobs` (collect the series' jobs from `driver.store().load()` by the series id or slug, since `add_series_jobs` returns only the first) and in the no-plan branch before its return (with `None`); in `create_one_job_accepting`'s combined branch after `add_job`; in `create_replacement` after `swap_in`.

- [ ] **Step 5: Run** `cargo test -j4 -p factory-cli` and `cargo test -j4 -p factory-engine`. Expected: all pass, 0 failed.

### Task 5: Start and build gates

**Files:**
- Modify: `crates/factory-engine/src/series_guard.rs:14-75`, `recover.rs:190-240`, `permissions.rs:39-85`, `build.rs:964-1000`
- Test: `crates/factory-engine/tests/gate_record.rs`

**Interfaces:**
- Consumes: `Engine::gate` from the wrapper task.
- Produces: pass and note lines for `series`, `machine`, `builder-guard`, `builder`, `diff-cap`.

Sites and texts (spec "The gates" table):
- `series_guard`: after `wait_for_earlier_plan` returns `None`, one line by the spec's four cases ("plan k of n, earlier plan merged", "plan 1 of n", "plan k of n, earlier plan not on the board", `skipped` "single plan"). The early return for a job with its own commits writes nothing.
- `machine_gate`: after `wait_while_down` returns `None`, `pass` with "free X MiB" when `min_free_disk_mb > 0` (from `(self.machine.free_mb)(dir)`), else plain `pass`; `waiting` when a wait starts; "ok after N min" when it ends. Write a line only when its result differs from the last `machine` line of this round (read the file).
- `run_builder_guarded`: `pass` on `Ok(Ok(run))`.
- `finish_build`: after the cap check passes and the board is updated, `diff-cap: pass` "N / cap lines", then `builder: pass` with "task k of n" or "whole plan", or `builder: note` "no change, work already on the branch" when `no_change` is true.

- [ ] **Step 1: Read** those four functions and `tests/common/mod.rs` `SeriesFixture` and `BuilderScript`.

- [ ] **Step 2: Write the failing tests** in `tests/gate_record.rs`, in the shape of the earlier tests:

| Test | Setup | Assertion |
|---|---|---|
| `a_build_writes_guard_builder_and_cap_passes` | `Fixture::new`, a builder effect that commits one file, run the build stage | lines in order contain `BuilderGuard: Pass`, `DiffCap: Pass`, `Builder: Pass`; `DiffCap` detail starts with a number and contains `/ 4000` |
| `a_single_plan_writes_series_skipped` | `Fixture::new` job without `series` | `Series: Skipped` "single plan" |
| `plan_two_writes_series_pass_after_plan_one_merged` | `SeriesFixture`, plan 1 merged | `Series: Pass` "plan 2 of 2, earlier plan merged" |
| `machine_writes_one_pass_per_round` | run the machine gate twice in one round | exactly one `Machine` line |
| `no_change_writes_builder_note` | the no-change path (`nothing_changed_after_checks` with passing checks) | `Builder: Note` "no change, work already on the branch" |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine --test gate_record`. Expected: the new tests fail with missing lines.
- [ ] **Step 4: Implement** the four sites with `self.gate(..)`, each as the last statement before the site returns, outside any closure.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine`. Expected: all pass, 0 failed.

### Task 6: Checks gates

**Files:**
- Modify: `crates/factory-engine/src/checks_stage.rs:84-135`, `:137-190`, `:194-225`, `:300-345`
- Test: `crates/factory-engine/tests/gate_record.rs`

**Interfaces:**
- Consumes: `Engine::gate`, `GateTarget`, `Engine::park_with_target`.
- Produces: lines for `checks`, `format`, `lint`, `no-checks`, `no-tests`; `park_gate` passes `GateTarget::Key(GateKey::Checks)` for `ParkKind::Other`.

- `run_checks_now`: after `checks.md` is written and `checks_pass` updated, `checks: pass|fail` "N of M commands passed"; then for `format` and `lint`: `pass`/`fail` with the command, or `skipped` "no format command" (or "no lint command") when `plan.gate_of` finds none. Compute these from `plan` and `report` outside `record_gates`' closure.
- `gate_checks`: when it returns `None`, `no-checks: pass` "all N code paths covered" or "no code paths".
- The no-tests decision (`checks_stage.rs:332-339`): `no-tests: pass` when a test was added or no code changed; `note` with the paths when no test was added and `require_tests` is false.

- [ ] **Step 1: Read** `checks_stage.rs` in full and `review.rs:395-430` (`final_gate` calls `gate_checks`).
- [ ] **Step 2: Write the failing tests:**

| Test | Setup | Assertion |
|---|---|---|
| `checks_fail_then_pass_across_rounds` | a builder that breaks a check in round 1 and fixes it in round 2 (as the fix-round tests in `tests/build_checks.rs` do) | `Checks: Fail` with `round == 1`, `Checks: Pass` with `round == 2` |
| `no_format_command_writes_skipped` | default fixture check rules (`**` → `true`) | `Format: Skipped` "no format command", `Lint: Skipped` "no lint command" |
| `uncovered_code_writes_no_tests_note` | code change without a test, `require_tests = false` | `NoTests: Note` whose detail names the path |
| `covered_paths_write_no_checks_pass` | default fixture | `NoChecks: Pass` |
| `a_checks_scope_violation_fails_checks_not_review` | the scope-violation case at `checks_stage.rs:313` | the fail line's key is `Checks` |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine --test gate_record`. Expected: the new tests fail.
- [ ] **Step 4: Implement** the sites. `park_gate` calls `park_with_target(kind, reason, parts, kind.resume(), target)` with `Key(Checks)` when `kind == ParkKind::Other`, else `FromKind`.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine`. Expected: all pass, 0 failed.

### Task 7: Review, merge and approve gates

**Files:**
- Modify: `crates/factory-engine/src/review.rs:57-420`, `merge.rs:212-275`, `sync.rs:115-135`, `crates/factory-cli/src/commands.rs:748-856`
- Test: `crates/factory-engine/tests/gate_record.rs`, `crates/factory-engine/tests/cargo_risk.rs`, `crates/factory-cli/tests/` (the file holding the `approve --waive` tests)

**Interfaces:**
- Consumes: `Engine::gate`, `record_gate`, `GateTarget`, `park_with_parts_target`.
- Produces: lines for `budget`, `review`, `risk`, `final-gate`, `merge`, `approve`.

Sites (spec "The gates" table and section 2):
- `review_stage_as`: at the risk computation, `risk: pass` "normal", `risk: pass` "cleared by the Cargo lock check" plus `Risk.note`, or `risk: note` "risky: <paths>". When the roles start, `review: running`. After the per-role results are set, one `review` line: `pass` when every role that ran approved; `fail` with "code CHANGES, N blockers" or "security did not finish"; the detail lists each role's verdict and "security skipped (docs only)" when skipped, then the review file name. Before the missing security agent return, `review: fail` "security agent missing". After `decide(&input)`, `budget: pass` "N% used", or `note` at 80% or more.
- `final_gate`: before `Ok(None)`, `final-gate: pass` "reused the checks run" or "ran N commands".
- `apply_decision`, auto-merge arm: `approve: pass` "auto-merge" after the ruling.
- `merge_stage`: every `Step::Park` and the `Err` arm call `park_with_parts_target(.., GateTarget::Key(GateKey::Merge))`. On `Step::Merged(sha)`, after `finish_merge`, `record_gate(.., Merge, Pass, short sha)`, then `approve: pass` "retried" when the last `approve` line of this round is `waiting`.
- `sync.rs:122`: `Key(Checks)` when the reason is "checks failed after sync" or "final checks failed after sync"; `sync.rs:130`: `NoFailLine`.
- CLI `approve`: after `d.ledger(&job).ruling(&ruling)?` and before `merge_stage`: `approve: waived` "reason, N blockers" with `--waive`; `note` "approved without a review" with `--without-review`; else `pass` "approved by the user".

- [ ] **Step 1: Read** `review.rs:57-430`, `merge.rs:212-380`, `sync.rs:100-260`, `commands.rs:748-860`, `tests/cargo_risk.rs` (`fixture`, `change`, `approving`, `run_job`) and `tests/common/mod.rs` `MergeFixture` and `approved_job*`.
- [ ] **Step 2: Write the failing tests:**

```rust
#[test]
fn an_auto_merge_writes_only_passes_notes_and_skips() {
    let fx = /* an approved job through `MergeFixture`, with gate_record on */;
    /* run review, then merge, as the auto-merge tests in tests/common use */
    let lines = factory_core::gate_record::read(&fx.job_dir);
    assert!(lines.iter().all(|l| matches!(l.result, GateResult::Pass | GateResult::Note | GateResult::Skipped | GateResult::Running)), "{lines:?}");
    for key in [GateKey::Review, GateKey::Risk, GateKey::FinalGate, GateKey::Merge, GateKey::Approve, GateKey::Budget] {
        assert!(lines.iter().any(|l| l.gate == key && l.result == GateResult::Pass), "{key:?}");
    }
}
```

| Test | Setup | Assertion |
|---|---|---|
| `changes_then_approve_writes_review_fail_then_pass` | reviewer `CHANGES` with 2 blockers, then `APPROVE` in round 2 | `Review: Fail` round 1 with "2 blockers"; `Review: Pass` round 2 |
| `a_docs_only_diff_skips_security_in_the_review_detail` | a diff only under `docs/` | `Review: Pass` with "security skipped (docs only)" |
| `a_missing_security_agent_writes_one_review_fail` | remove the security agent file | exactly one `Review: Fail` "security agent missing"; no other fail line |
| `a_risky_diff_writes_note_final_gate_fail_and_waiting` | a diff in a `risk_paths` glob, reviewer approves | in order `Risk: Note`, `FinalGate: Pass`, `Risk: Fail`, `Approve: Waiting` |
| `a_merge_park_after_sync_writes_merge_fail` | a merge whose base moves so `try_merge` syncs, then conflicts | the fail line's key is `Merge` |
| `sync_review_passed_writes_only_approve_waiting` | the `sync.rs:130` branch | `Approve: Waiting`; no `Fail` line |
| `cargo_lock_clears_risk` (`cargo_risk.rs`) | the existing Cargo-only risky case cleared by the lock check, gate_record on | `Risk: Pass` with "cleared by the Cargo lock check" |
| `approve_waive_writes_waived_with_blockers` (CLI) | the existing `approve --waive` test setup | `Approve: Waived` whose detail contains the reason and the blocker count |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-engine --test gate_record --test cargo_risk` and the CLI test file. Expected: the new tests fail.
- [ ] **Step 4: Implement** the sites. No line inside `update` or `update_job` closures, and the merge lines only after `try_merge` returns (the merge lock is a local inside it).
- [ ] **Step 5: Run** `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`, `cargo clippy -j4 -p factory-engine -p factory-cli -p factory-core --all-targets -- -D warnings`. Expected: all pass, no warnings.

### Task 8: Dashboard server

**Files:**
- Create: `dashboard/src/gates.rs`
- Modify: `dashboard/src/main.rs` (module list), `dashboard/src/state.rs:19` (`JobView.gates`), `:295-307` (fill it)
- Test: `dashboard/src/gates.rs` (`mod tests`), `dashboard/src/state.rs` (`mod tests`)

**Interfaces:**
- Produces:
  - `pub const GATES_FILE: &str = "gates.jsonl";` and `pub const TAIL_BYTES: u64 = 512 * 1024;`
  - `#[derive(Serialize)] pub struct GateView { pub gate: String, pub stage: String, pub state: String, pub detail: String, pub round: u32, pub history: String }`
  - `#[derive(Serialize)] pub struct GatesView { pub list: Vec<GateView>, pub stopped_at: Option<String> }`
  - `pub fn gates_path(paths: &Paths, dir: &str) -> Option<PathBuf>`, the same check as `park_path`.
  - `pub fn load_gates(paths: &Paths, job: &Job) -> Option<GatesView>`: `None` when the file is missing; reads the last `TAIL_BYTES` and drops the first partial line when it cut.
  - `pub fn latest(lines: &[RawLine], job: &Job) -> GatesView`: spec section 4, with `max(job.round, 1)` and `job.cancelled == Some(true)`.
  - `JobView.gates: Option<GatesView>`, serialised as `null` when `None`.
  - The 18 keys and stages as a `const KEYS: [(&str, &str); 18]` in card order (the dashboard does not depend on factory-core).

- [ ] **Step 1: Read** `dashboard/src/park.rs:1-60`, `state.rs:1-60` and `:200-320` and its tests (`job(..)` helper, `TestResult`), `model.rs:130-170`, and spec sections 4 and 5.
- [ ] **Step 2: Write the failing tests:**

```rust
#[test]
fn first_round_lines_count_for_a_board_round_of_zero() {
    let job = job_at("build", 0);
    let lines = vec![raw("builder-guard", "pass", 1)];
    let view = latest(&lines, &job);
    let guard = view.list.iter().find(|g| g.gate == "builder-guard").expect("row");
    assert_eq!(guard.state, "pass");
    assert_eq!(view.list.len(), 18);
}
```

`job_at(stage, round)` and `raw(gate, result, round)` are small helpers in the test module.

| Test | Lines and job | Assertion |
|---|---|---|
| `once_gates_keep_their_last_line_across_rounds` | `series: pass` round 1; job round 3 | `series` state `pass` |
| `a_round_gate_from_an_earlier_round_is_not_reached` | `checks: fail` round 1; job round 2 | `checks` state `not-reached`, history `"fail r1"` |
| `the_last_line_of_the_round_wins` | `checks: fail` then `checks: pass`, both round 2 | `pass` |
| `stale_running_and_waiting_are_interrupted` | `review: running` on a merged job; on a cancelled (`Some(true)`) job; on a needs-human job; `approve: waiting` on a build job | each `interrupted` |
| `live_waiting_states_stay` | `machine: waiting` and `series: waiting` on a build job; `approve: waiting` on needs-human | each keeps `waiting` |
| `stopped_at_is_the_last_fail_of_the_round` | `machine: fail` then `checks: fail`, both round 2 | `stopped_at == Some("checks")` |
| `no_file_gives_null` | no file | `load_gates == None`; the serialised `JobView` has `"gates":null` |
| `a_bad_dir_reads_nothing` | `dir = "../x"` | `gates_path` is `None` |
| `only_the_tail_is_read` | a file of 600 KB of valid lines plus one known last line | `load_gates` returns the last line's state and no error |

- [ ] **Step 3: Run** `cargo test -j4 --manifest-path dashboard/Cargo.toml gates`. Expected: compile error, `could not find `gates``.
- [ ] **Step 4: Implement** the Interfaces and fill `gates` in `build_state` next to `has_park`. Leave a mtime cache out unless a test shows it is needed.
- [ ] **Step 5: Run** `cargo test -j4 --manifest-path dashboard/Cargo.toml` and `cargo clippy -j4 --manifest-path dashboard/Cargo.toml --all-targets -- -D warnings`. Expected: all pass, no warnings.

### Task 9: The card

**Files:**
- Modify: `dashboard/static/board.js:755-770` (`renderCard`), `:971-1014` (`api`), `dashboard/static/dashboard.css`
- Test: `dashboard/tests/board.test.js`

**Interfaces:**
- Consumes: `job.gates` (`{list, stopped_at}` or `null`) from the server task.
- Produces: `gateStrip(job) -> string` (empty string when `job.gates` is null) and `gateSummary(job) -> string`, both added to `api`.

- [ ] **Step 1: Read** `board.js:1-140` and `:700-780`, `patch.js`, `dashboard.css:1-60`, `board.test.js:1-80` (`makeJob`, `makeState`), the mockup `.superpowers/brainstorm/3077297-1795367505/content/gate-card-a-plus-c.html` (main checkout), and spec section 6.
- [ ] **Step 2: Write the failing tests:**

```js
test("the strip has 18 marks in card order", () => {
  const job = makeJob({ stage: "build", round: 1, gates: gatesOf({ "builder-guard": "pass" }) });
  const html = FactoryBoard.gateStrip(job);
  const marks = [...html.matchAll(/data-gate="([a-z-]+)"/g)].map((m) => m[1]);
  assert.deepEqual(marks, GATE_ORDER);
});
```

`gatesOf(states)` builds 18 rows with `not-reached` except the given ones; `GATE_ORDER` is the 18 keys from Global Constraints.

| Test | Job | Assertion |
|---|---|---|
| `the summary counts reached gates while running` | build, 3 reached | `gateSummary` is `"3 / 18"` |
| `the summary names the stopped gate when parked` | needs-human, `stopped_at: "checks"` | `"stopped at checks"` |
| `the summary of a merged job leaves out zero parts` | merged, 15 pass, 3 skipped | `"15 passed · 3 skipped"` |
| `the summary of a cancelled job` | `cancelled: true` | `"cancelled"` |
| `a needs-human card opens the list` | needs-human with gates | `renderCard` output contains `<details class="gates" open` |
| `the_gate_list_is_a_details_element` | build with gates | contains `<details class="gates"` without `open`, and the strip is inside `<summary>` |
| `a_card_without_a_record_has_no_strip` | `gates: null`, stage review | no `class="gates"`; the review chips are present as today |
| `a card with a record hides the review chips` | review stage with gates | no `class="chips"` |
| `the failing row shows the park reason` | needs-human, `stopped_at: "checks"`, `park_summary: "checks failed"` | the `checks` row has class `fail` and contains `checks failed` |

- [ ] **Step 3: Run** `node --test dashboard/tests/board.test.js`. Expected: `FactoryBoard.gateStrip is not a function`.
- [ ] **Step 4: Implement** `gateStrip` and `gateSummary`; in `renderCard`, when `job.gates` is set, draw the strip from the `spec` stage on and skip the chips line; mark colours from `--success`, `--destructive`, `--attention`, `--primary` (pulsing for running and waiting), a dashed outline for skipped, `--muted-foreground` for not reached and interrupted; each mark has a `title`. Escape every detail with the file's existing escape helper. The strip wraps by group at phone width.
- [ ] **Step 5: Run** `node --test dashboard/tests/*.test.js` and `bun test dashboard/tests/`. Expected: all pass.

### Task 10: Docs, full checks and the trial

**Files:**
- Modify: `docs/reference/software-factory.md` (a "Gate record" section and a dashboard line), `dashboard/README.md` (the strip and the `gates` field)

- [ ] **Step 1: Write** the docs: the file, the fields, the round rule, the 18 keys with their stages, the park-kind map, the latest-state rule, and the `gate_record` setting with its default.
- [ ] **Step 2: Run** in the worktree: `cargo fmt --check`, `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`, `cargo test -j4 -p factory-cli --test no_hardcoded_paths`, `cargo clippy -j4 -p factory-core -p factory-engine -p factory-cli --all-targets -- -D warnings`, `cargo test -j4 --manifest-path dashboard/Cargo.toml`, `node --test dashboard/tests/*.test.js`. Expected: all pass, no warnings.
- [ ] **Step 3: Build** `cargo build -j4 -p factory-cli` in the worktree. Expected: `target/debug/factory` in the worktree.
- [ ] **Step 4: Hand over for the trial.** Tell the user the steps, and do not do them yourself: add `gate_record = true` to rocket-factory's `project.toml`; run one small rocket-factory job with `<worktree>/target/debug/factory --project rocket-factory ...` by full path; run the worktree dashboard with `cargo run -j4 --manifest-path <worktree>/dashboard/Cargo.toml -- --root <main checkout> --port 8766`; check the card. Nothing is committed until the user decides.
