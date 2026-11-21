# Replace One Plan of a Series Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-22-series-cancel-and-replace-design.md
> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** `factory new --replaces <id>` puts a fixed plan in a failed plan's series place, and series lookups follow the replacement instead of the cancelled job.

**Commit:** feat(series): replace one plan of a series

**Architecture:** Two pure board helpers in `factory-core` find a series member by series id and index (live jobs first) and the replacement of a job. The series guard and `run_series` use them. A new engine function `create_replacement` validates one plan, makes one job at the target's series place and cancels the target. The CLI adds `--replaces` to `new`.

**Tech Stack:** Rust, `cargo test -j4 -p <crate> <filter>`, `cargo clippy -j4 -p <crate> --all-targets -- -D warnings`.

## Global Constraints

- cargo commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- No new board, ledger or event field in this plan. Board JSON stays compatible with `dashboard/`.
- Git only through `factory_git::Repo`; paths only through `Layout`.
- No unwrap in production Rust. Tests use `.expect("...")` and `expect_err`.
- Comments are short full sentences that end with a period.
- Each task ends with `cargo fmt -p <crate> -- --check` and `cargo clippy -j4 -p <crate> --all-targets -- -D warnings` for every crate it touched.

## File Structure

| File | Change |
|---|---|
| `crates/factory-core/src/board.rs` | Add `Board::series_member` and `Board::replacement_of`, with unit tests. |
| `crates/factory-engine/src/series_guard.rs` | `wait_for_earlier_plan` uses `series_member`; new cancelled-predecessor reason. |
| `crates/factory-engine/src/runloop.rs` | `run_series` next-plan lookup by series id and index; new `create_replacement`. |
| `crates/factory-engine/src/series.rs` | `parse_one_plan` (one plan, chain not walked). |
| `crates/factory-engine/src/lib.rs` | Export `create_replacement`, `Replacement`. |
| `crates/factory-engine/tests/series_guard.rs` | Update the cancelled-predecessor test; add replacement tests. |
| `crates/factory-cli/src/cli.rs`, `crates/factory-cli/src/commands.rs` | `new --replaces`. |
| `crates/factory-cli/tests/new_to_merged.rs` | CLI tests for `--replaces`. |
| `docs/reference/software-factory.md`, `README.md` | Document `--replaces` and the new guard reason. |

## Review Focus

1. A fixed plan whose `Next plan:` is a relative path while the original's is absolute (or the other way round): both resolve to the same file, so `new --replaces` must accept it. Test: `new_replaces_accepts_a_relative_next_line_that_resolves_to_the_same_file`.
2. A target at stage `spec` that never ran (no worktree, no branch): replacing it must work and leave no branch behind. Test: `create_replacement_of_an_unstarted_plan_makes_one_job`.
3. A replacement of plan 1 of a two-plan series while plan 2 is parked `series-waiting` on the cancelled plan 1: after the replacement merges inside `run --series`, plan 2 continues. Test: `run_series_continues_a_waiting_plan_after_its_replaced_predecessor_merges`.
4. Replacing the last plan (`Next plan: none`) with a fixed plan whose line names a file: refused, naming `Next plan: none`. Test: row `wrong_next_on_last` in `create_replacement_refusals`.
5. A spec refusal with `--spec` must leave the board unchanged: no new job and the target not cancelled. Test: `create_replacement_with_a_refused_spec_changes_nothing`.

---

### Task 1: Series lookups follow live jobs

**Files:**
- Modify: `crates/factory-core/src/board.rs` (impl `Board`, near `series_with_siblings` at line 628; tests in `mod tests`)
- Modify: `crates/factory-engine/src/series_guard.rs:44-72`
- Modify: `crates/factory-engine/src/runloop.rs:787-795` (the `existing` lookup in `run_series`)
- Test: `crates/factory-engine/tests/series_guard.rs`

**Interfaces:**
- Produces, in `factory_core::Board`:
  - `pub fn series_member(&self, series_id: &str, index: u32) -> Option<&Job>`: the job with that series id and index that is not cancelled; when none, the first cancelled one; else `None`.
  - `pub fn replacement_of(&self, id: &str) -> Option<&Job>`: for a job with a series id, another job with the same series id and index that is not cancelled. `None` for a job without a series id, a missing job, or no such other job.
- Port: board reads go through `Board::load` / `Driver::store()` (BoardStore). No `AgentRunner` or `Tracker` use.

- [ ] **Step 1: Read the code and docs**

Read the spec sections "Series lookups skip cancelled jobs" and "Current state", `crates/factory-engine/src/series_guard.rs`, `run_series` and `continue_waiting` in `crates/factory-engine/src/runloop.rs` (lines 768-847), `Board` in `crates/factory-core/src/board.rs:591-647`, and the fixtures `two_plans`, `assert_waiting` and `SeriesFixture::plan_file` in `crates/factory-engine/tests/series_guard.rs` and `crates/factory-engine/tests/common/mod.rs`.

- [ ] **Step 2: Write the failing core tests**

In `board.rs` `mod tests`, reuse `in_series(id, sid, total)` and set the index by hand:

```rust
fn at(id: &str, sid: &str, index: u32, cancelled: bool) -> Job {
    let mut j = in_series(id, sid, 3);
    j.series.as_mut().expect("series").index = index;
    j.cancelled = cancelled;
    j
}

#[test]
fn series_member_prefers_a_live_job_over_a_cancelled_one() {
    let b = Board {
        jobs: vec![at("001", "S1", 1, true), at("002", "S1", 2, false), at("003", "S1", 1, false)],
        ..Default::default()
    };
    assert_eq!(b.series_member("S1", 1).expect("member").id, "003");
    assert_eq!(b.series_member("S1", 2).expect("member").id, "002");
    assert!(b.series_member("S1", 3).is_none());
    assert!(b.series_member("S2", 1).is_none());
}

#[test]
fn series_member_falls_back_to_a_cancelled_job() {
    let b = Board { jobs: vec![at("001", "S1", 1, true)], ..Default::default() };
    assert_eq!(b.series_member("S1", 1).expect("member").id, "001");
}

#[test]
fn replacement_of_names_the_live_job_at_the_same_place() {
    let b = Board {
        jobs: vec![at("001", "S1", 1, true), at("002", "S1", 2, false), at("003", "S1", 1, false)],
        ..Default::default()
    };
    assert_eq!(b.replacement_of("001").expect("replaced").id, "003");
    assert!(b.replacement_of("003").is_none(), "a live job is not replaced by itself");
    assert!(b.replacement_of("002").is_none());
    assert!(b.replacement_of("009").is_none());
    let plain = Board { jobs: vec![Job::new("001", "d", "s", "c", "f", 1.0)], ..Default::default() };
    assert!(plain.replacement_of("001").is_none());
}
```

- [ ] **Step 3: Run them and see them fail**

Run: `cargo test -j4 -p factory-core series_member replacement_of`
Expected: FAIL to compile with `no method named 'series_member' found for struct 'Board'`.

- [ ] **Step 4: Implement `series_member` and `replacement_of` in `board.rs`**

`replacement_of` takes the job's series id and index and calls `series_member`, then returns it only when its id differs from `id` and it is not cancelled.

- [ ] **Step 5: Run the core tests**

Run: `cargo test -j4 -p factory-core series_member replacement_of`
Expected: 3 passed.

- [ ] **Step 6: Write the failing engine tests**

In `crates/factory-engine/tests/series_guard.rs`, change `plan_two_parks_when_plan_one_was_cancelled`: replace `assert!(reason.contains("cancel this plan too"), "{reason}");` with

```rust
assert!(reason.contains("cancel this plan, or replace plan 1 with factory new --replaces"), "{reason}");
assert!(reason.contains(&format!("--replaces {one}")), "{reason}");
```

Add this helper and tests. The helper cancels a job and adds a live job at the same series place, the way `create_replacement` will later, so these tests do not depend on it:

```rust
/// Cancels `id` and adds a new job at its series place with plan file `plan`. Returns the new id.
/// The job is made from a lone plan, so `create_job` does not walk the chain behind `plan`.
fn stand_in(fx: &SeriesFixture, id: &str, plan: &Path) -> String {
    let old = fx.driver.job(id).expect("job");
    fx.driver
        .store()
        .update_job(id, |j| {
            j.cancelled = true;
            j.set_stage(Stage::NeedsHuman, Some("cancelled"), true, 2.0)
        })
        .expect("cancel");
    let lone = lone_plan(fx, &format!("stand-in-{id}"));
    let made = create_job(&fx.driver, "fixed", "fixed plan", Some(&lone), None).expect("create");
    let mut pos = old.series.expect("series");
    pos.plan_file = plan.to_string_lossy().into_owned();
    fx.driver
        .store()
        .update_job(&made.id, |j| {
            j.series = Some(pos.clone());
            Ok(())
        })
        .expect("series");
    made.id
}

#[test]
fn series_guard_prefers_the_replacement_of_a_cancelled_plan() {
    let fx = SeriesFixture::new("fine\nVERDICT: APPROVE");
    let (one, two) = two_plans(&fx);
    let fixed = fx.plan_file("plan-01-fixed.md", 1, "plan-02-b.md");
    let new_one = stand_in(&fx, &one, &fixed);
    assert_eq!(run_job(&fx.driver, &new_one).expect("run"), RunEnd::Merged);
    assert_eq!(run_series(&fx.driver, &two).expect("series"), RunEnd::Merged, "{}", ledger_of(&fx, &two));
}

#[test]
fn run_series_finds_a_replacement_at_a_new_path_after_the_plan_before_merges() {
    let fx = SeriesFixture::new("fine\nVERDICT: APPROVE");
    let (one, two) = two_plans(&fx);
    let fixed = fx.plan_file("plan-02-fixed.md", 1, "none");
    let new_two = stand_in(&fx, &two, &fixed);
    assert_eq!(run_series(&fx.driver, &one).expect("series"), RunEnd::Merged);
    assert_eq!(fx.driver.job(&new_two).expect("job").stage, Stage::Merged);
    assert!(fx.driver.job(&two).expect("job").cancelled);
    assert_eq!(fx.driver.store().load().expect("board").jobs.len(), 3, "no job made for the old plan 2");
}

#[test]
fn run_series_continues_the_next_plan_after_a_replacement_merges() {
    let fx = SeriesFixture::new("fine\nVERDICT: APPROVE");
    let (one, two) = two_plans(&fx);
    let fixed = fx.plan_file("plan-01-fixed.md", 1, "plan-02-b.md");
    let new_one = stand_in(&fx, &one, &fixed);
    assert_eq!(run_series(&fx.driver, &new_one).expect("series"), RunEnd::Merged);
    assert_eq!(fx.driver.job(&two).expect("job").stage, Stage::Merged);
}

#[test]
fn run_series_stops_when_the_next_plan_is_cancelled() {
    let fx = SeriesFixture::new("fine\nVERDICT: APPROVE");
    let (one, two) = two_plans(&fx);
    fx.driver
        .store()
        .update_job(&two, |j| {
            j.cancelled = true;
            j.set_stage(Stage::NeedsHuman, Some("cancelled"), true, 2.0)
        })
        .expect("cancel");
    assert_eq!(run_series(&fx.driver, &one).expect("series"), RunEnd::Merged);
    let board = fx.driver.store().load().expect("board");
    assert_eq!(board.jobs.len(), 2, "no new job for a cancelled plan");
    assert!(fx.driver.job(&two).expect("job").cancelled);
}
```

- [ ] **Step 7: Run them and see them fail**

Run: `cargo test -j4 -p factory-engine --test series_guard`
Expected: FAIL. `plan_two_parks_when_plan_one_was_cancelled` fails on the new reason text. `series_guard_prefers_the_replacement_of_a_cancelled_plan` parks plan 2 on the cancelled job. `run_series_finds_a_replacement_at_a_new_path_after_the_plan_before_merges` fails because no job matches the old `plan_file`. `run_series_stops_when_the_next_plan_is_cancelled` runs or creates a job for the cancelled plan.

- [ ] **Step 8: Implement the lookups**

- `wait_for_earlier_plan`: find plan N-1 with `board.series_member(sid, n)`. The cancelled reason is exactly `format!("plan {n} (job {id}) was cancelled; cancel this plan, or replace plan {n} with factory new --replaces {id}")`.
- `run_series`: when `pos.id` is `Some(sid)`, `existing` is `board.series_member(sid, pos.index + 1)`. If that job is cancelled, return `Ok(RunEnd::Merged)`. Without a series id keep today's `plan_file` match. Keep the `None` branch that creates a late job for legacy series only (no series id).

- [ ] **Step 9: Run the engine tests**

Run: `cargo test -j4 -p factory-engine --test series_guard`
Expected: all pass (the 10 existing tests plus 4 new).
Then run `cargo test -j4 -p factory-engine` and `cargo test -j4 -p factory-core`; both pass.

### Task 2: `create_replacement` in the engine

**Files:**
- Modify: `crates/factory-engine/src/series.rs` (add `parse_one_plan` next to `parse_series_with`)
- Modify: `crates/factory-engine/src/runloop.rs` (add `Replacement` and `create_replacement` after `create_job_with_spec_accepting`)
- Modify: `crates/factory-engine/src/lib.rs:106-108` (export them)
- Test: `crates/factory-engine/tests/series_guard.rs`

**Interfaces:**
- Consumes: `Board::series_member`, `Board::replacement_of`, added with the series lookups.
- Produces:
  - `pub(crate) fn parse_one_plan(root: &Path, path: &Path) -> Result<Series, SeriesError>`: `read_plan(root, path, Some(MAX_TASKS_PER_PLAN))` wrapped in a one-element `Series`.
  - `pub struct Replacement<'a> { pub target: &'a str, pub slug: &'a str, pub feature: &'a str, pub plan: &'a Path, pub spec: Option<&'a Path>, pub model: Option<&'a str>, pub accept_spec: Option<&'a str> }`
  - `pub fn create_replacement(driver: &Driver, r: &Replacement<'_>) -> Result<Job, EngineError>`: returns the new job.
- Port: board through `Driver::store()` (BoardStore), ledger through `Driver::ledger`. No `AgentRunner` or `Tracker` use, except that the spec check inside `validate_series` may run the spec reviewer as it does for any `new`.

- [ ] **Step 1: Read the code and docs**

Read spec section "Replace one plan" and its edge-case table, `create_job_with_spec_accepting`, `validate_series` and `add_job` in `runloop.rs` (lines 18-90 and 332-640), `read_plan` in `series.rs`, and `lease_is_live` in `crates/factory-engine/src/lease.rs:16`.

- [ ] **Step 2: Write the failing tests**

In `crates/factory-engine/tests/series_guard.rs`, import `create_replacement` and `Replacement`, then add:

```rust
fn replace<'a>(target: &'a str, plan: &'a Path) -> Replacement<'a> {
    Replacement { target, slug: "fixed", feature: "fixed plan", plan, spec: None, model: None, accept_spec: None }
}

#[test]
fn create_replacement_takes_the_index_and_cancels_the_target() {
    let fx = SeriesFixture::new("fine\nVERDICT: APPROVE");
    let (one, _two) = two_plans(&fx);
    let fixed = fx.plan_file("plan-01-fixed.md", 2, "plan-02-b.md");
    let made = create_replacement(&fx.driver, &replace(&one, &fixed)).expect("replace");
    let old = fx.driver.job(&one).expect("old");
    let new = fx.driver.job(&made.id).expect("new");
    assert!(old.cancelled);
    assert_eq!(old.reason.as_deref(), Some(format!("cancelled: replaced by job {}", made.id).as_str()));
    let (op, np) = (old.series.expect("old pos"), new.series.clone().expect("new pos"));
    assert_eq!((np.id, np.index, np.total), (op.id, op.index, op.total));
    assert_eq!(std::fs::canonicalize(&np.plan_file).expect("canon"), std::fs::canonicalize(&fixed).expect("canon"));
    assert_eq!(new.stage, Stage::Build);
    assert!(ledger_of(&fx, &made.id).contains(&format!("replaces job {one}")));
    let plan_md = std::fs::read_to_string(fx.driver.job_dir(&new).join("plan.md")).expect("plan.md");
    assert!(plan_md.contains("### Task 2"), "the fixed plan was copied");
    assert_eq!(fx.driver.store().load().expect("board").jobs.len(), 3, "one new job only");
}

#[test]
fn create_replacement_copies_the_target_spec_without_a_spec_path() {
    let fx = SeriesFixture::new("fine\nVERDICT: APPROVE");
    let (one, _two) = two_plans(&fx);
    let old = fx.driver.job(&one).expect("old");
    std::fs::write(fx.driver.job_dir(&old).join("spec.md"), "# Old spec\n").expect("spec");
    let fixed = fx.plan_file("plan-01-fixed.md", 1, "plan-02-b.md");
    let made = create_replacement(&fx.driver, &replace(&one, &fixed)).expect("replace");
    let text = std::fs::read_to_string(fx.driver.job_dir(&made).join("spec.md")).expect("spec");
    assert_eq!(text, "# Old spec\n");
}

#[test]
fn create_replacement_of_an_unstarted_plan_makes_one_job() {
    let fx = SeriesFixture::new("fine\nVERDICT: APPROVE");
    let (_one, two) = two_plans(&fx);
    let fixed = fx.plan_file("plan-02-fixed.md", 1, "none");
    let made = create_replacement(&fx.driver, &replace(&two, &fixed)).expect("replace");
    assert!(fx.driver.job(&two).expect("old").cancelled);
    assert!(!fx.driver.repo().branch_exists(&fx.driver.job(&two).expect("old").branch));
    assert_eq!(fx.driver.job(&made.id).expect("new").series.expect("pos").index, 2);
}

#[test]
fn run_series_continues_a_waiting_plan_after_its_replaced_predecessor_merges() {
    let fx = SeriesFixture::new("fine\nVERDICT: APPROVE");
    let (one, two) = two_plans(&fx);
    fx.driver.store().update_job(&one, |j| { j.cancelled = true; j.set_stage(Stage::NeedsHuman, Some("cancelled"), true, 2.0) }).expect("cancel");
    assert!(matches!(run_series(&fx.driver, &two).expect("series"), RunEnd::NeedsHuman(_)));
    assert_waiting(&fx, &two);
    let fixed = fx.plan_file("plan-01-fixed.md", 1, "plan-02-b.md");
    let made = create_replacement(&fx.driver, &replace(&one, &fixed)).expect("replace");
    assert_eq!(run_series(&fx.driver, &made.id).expect("series"), RunEnd::Merged);
    assert_eq!(fx.driver.job(&two).expect("job").stage, Stage::Merged);
}

#[test]
fn create_replacement_with_a_refused_spec_changes_nothing() {
    let fx = SeriesFixture::new("fine\nVERDICT: APPROVE");
    let (one, _two) = two_plans(&fx);
    let fixed = fx.plan_file("plan-01-fixed.md", 1, "plan-02-b.md");
    let bad = fx.tmp.path().join("bad-spec.md");
    std::fs::write(&bad, "# Spec\n\n## Goal\n\nshort\n").expect("spec");
    let mut r = replace(&one, &fixed);
    r.spec = Some(bad.as_path());
    create_replacement(&fx.driver, &r).expect_err("refused spec");
    assert!(!fx.driver.job(&one).expect("old").cancelled);
    assert_eq!(fx.driver.store().load().expect("board").jobs.len(), 2);
}
```

Refusals share one shape. Add `create_replacement_refusals`, which loops over this table and, for each row, builds the state, calls `create_replacement` with `expect_err`, asserts `err.to_string().contains(<text>)` and asserts the board still has the same number of jobs:

| Row | Setup | Message contains |
|---|---|---|
| `folder` | `plan` is the plans folder `fx.tmp.path().join("plans")` | `needs one plan file` |
| `merged` | target stage set to `Stage::Merged` | `is already merged` |
| `no_series` | a single-plan job from `lone_plan(&fx, "p1")` | `is not in a series` |
| `live_lease` | `factory_engine::acquire_lease(&fx.driver.store(), &one, "other", 600.0, factory_engine::now_secs())` | `live lease` |
| `wrong_next` | fixed plan of plan 1 with `Next plan: none` | `Next plan: ` and the path of `plan-02-b.md` |
| `wrong_next_on_last` | target plan 2, fixed plan with next `plan-01-a.md` | `Next plan: none` |
| `already_replaced` | `create_replacement` of plan 1 once, then again on the old id | `was replaced by job` |

Also add `new_replaces_accepts_a_relative_next_line_that_resolves_to_the_same_file`: write the fixed plan by hand into `fx.tmp.path().join("plans/plan-01-rel.md")` with the line `Next plan: plan-02-b.md` (relative), call `create_replacement` and assert `.expect("relative next resolves")`.

- [ ] **Step 3: Run them and see them fail**

Run: `cargo test -j4 -p factory-engine --test series_guard replacement`
Expected: FAIL to compile with `unresolved imports 'factory_engine::create_replacement', 'factory_engine::Replacement'`.

- [ ] **Step 4: Implement `parse_one_plan` and `create_replacement`**

Order in `create_replacement`, so that a refusal changes nothing:
1. `r.plan` must be a file, else `EngineError::Invalid("--replaces needs one plan file, not a folder")`.
2. Load the board. The target must exist, not be merged (`job <id> is already merged`), have a series id (`job <id> is not in a series`) and no live lease (`job <id> has a live lease`). When `board.replacement_of(target)` is `Some(n)`, refuse with `job <old> was replaced by job <n>, replace that one`.
3. Expected next: `read_plan` of the target's `series.plan_file` gives `next`. The fixed plan's `next` (from `parse_one_plan`) must equal it after `canonicalize` on both sides, or both be `None`. Otherwise refuse with `the fixed plan must end with Next plan: <expected path or none>`.
4. `validate_series(driver, r.plan, r.spec, r.accept_spec, None, |root, p| parse_one_plan(root, p).map_err(EngineError::from))`.
5. `add_job(driver, r.slug, r.feature, &factory_core::parse_issues(r.feature), Some(pos), Stage::Build, r.model.or(target.model.as_deref()))`, where `pos` is the target's `SeriesPos` with `plan_file` set to the canonical fixed plan path. Copy the validated spec text into `spec.md`, else copy the target's `spec.md` when it exists.
6. If the target is not cancelled, mark it cancelled in one `update_job` (stage `NeedsHuman`, reason `cancelled: replaced by job <new id>`, clear `lease`, `task_session` and `builder_last`, as `cancel` in `crates/factory-cli/src/commands.rs:910` does). Write the ruling `cancelled: replaced by job <new id>` to the target's ledger and `replaces job <old id>` to the new job's ledger.
7. Return the new job, with plan warnings added as `create_job_with_spec_accepting` adds them.

- [ ] **Step 5: Run the tests**

Run: `cargo test -j4 -p factory-engine --test series_guard`
Expected: all pass. Then `cargo test -j4 -p factory-engine` passes.

### Task 3: `factory new --replaces` and the docs

**Files:**
- Modify: `crates/factory-cli/src/cli.rs:63-88` (`Command::New`)
- Modify: `crates/factory-cli/src/commands.rs:372-393` and `new_job` (`:481`)
- Test: `crates/factory-cli/tests/new_to_merged.rs`, `crates/factory-cli/src/cli.rs` (tests)
- Modify: `docs/reference/software-factory.md:45-46`, `README.md` (the `factory new` section)

**Interfaces:**
- Consumes: `factory_engine::{create_replacement, Replacement}` from the engine.
- Produces: the CLI flag `--replaces <id>` (`replaces: Option<String>`, `requires = "plans"`, `conflicts_with = "one_job"`).

- [ ] **Step 1: Read the code and docs**

Read `Command::New` in `cli.rs`, `new_job` in `commands.rs` and the tests `a_job_goes_from_new_to_merged_through_the_cli` and `parked_and_cancelled` in `crates/factory-cli/tests/new_to_merged.rs`.

- [ ] **Step 2: Write the failing tests**

In `crates/factory-cli/tests/new_to_merged.rs`:

```rust
/// A two-plan series made through the CLI. Returns the path of plan 2.
fn cli_series(fx: &Fixture) -> std::path::PathBuf {
    let two = fx.write_plan("plan-02-b.md", 1, "none");
    let one = fx.write_plan("plan-01-a.md", 1, two.to_str().expect("utf8"));
    fx.run(&["new", "a", "two plans", "--plans", one.to_str().expect("utf8")]).expect("new");
    two
}

#[test]
fn new_replaces_prints_the_replacement_and_cancels_the_target() {
    let fx = Fixture::new();
    let two = cli_series(&fx);
    let fixed = fx.write_plan("plan-01-fixed.md", 1, two.to_str().expect("utf8"));
    let out = fx
        .run(&["new", "fixed", "fixed plan", "--plans", fixed.to_str().expect("utf8"), "--replaces", "1"])
        .expect("replace");
    let mut lines = out.lines();
    assert!(lines.next().expect("first").starts_with("created job 003 in "), "{out}");
    assert!(out.contains("job 001 replaced by job 003"), "{out}");
    let show = fx.run(&["show", "1"]).expect("show");
    assert!(show.contains("\"cancelled\": true"), "{show}");
}
```

`Fixture::run` uses `Cli::parse_from`, which exits on a parse error, so the flag rules are tested in the `cli.rs` tests with `Cli::try_parse_from`, as `new_parses_accept_spec` does:

```rust
#[test]
fn new_replaces_conflicts_with_one_job_and_needs_plans() {
    let both = Cli::try_parse_from(["factory", "new", "d", "f", "--plans", "p.md", "--replaces", "1", "--one-job"]);
    assert!(both.is_err(), "--replaces conflicts with --one-job");
    let bare = Cli::try_parse_from(["factory", "new", "d", "f", "--replaces", "1"]);
    assert!(bare.is_err(), "--replaces needs --plans");
    let cli = Cli::try_parse_from(["factory", "new", "d", "f", "--plans", "p.md", "--replaces", "1"]).expect("parse");
    assert!(matches!(cli.command, Command::New { replaces: Some(ref id), .. } if id == "1"));
}
```

- [ ] **Step 3: Run them and see them fail**

Run: `cargo test -j4 -p factory-cli new_replaces`
Expected: FAIL to compile with `struct variant 'Command::New' has no field named 'replaces'`.

- [ ] **Step 4: Implement the flag**

Add `replaces` to `Command::New` and `NewArgs`. In `new_job`, when `replaces` is `Some(id)`, normalize it with `factory_core::normalize_id`, call `create_replacement` with the slug, feature, plan, spec, model and accept-spec, and print `created job <new> in <dir>` (same estimate lines, notes and warnings as today), then `job <old> replaced by job <new>`. The doc comment on the flag reads: `Put this plan in the place of a failed series plan, which is cancelled. Needs one plan file in --plans.`

- [ ] **Step 5: Run the tests**

Run: `cargo test -j4 -p factory-cli new_replaces`
Expected: 2 passed. Then `cargo test -j4 -p factory-cli` passes.

- [ ] **Step 6: Update the docs**

- `docs/reference/software-factory.md:45`: after the sentence on plans being copied into their job folders, add: to fix a failed plan, edit and commit its plan file, then run `factory new <slug> '<feature>' --plans <fixed plan file> --replaces <id>`. The new job takes the failed job's series id and index, starts at `build`, and the old job is cancelled with `cancelled: replaced by job <new>`. The fixed plan's `Next plan:` line must point to the same file as the original's. Without `--spec` the old job's `spec.md` is copied and no spec check runs. Refusals: a folder, a merged job, a job without a series, a live lease, a wrong `Next plan:` line, a job already replaced.
- `docs/reference/software-factory.md:46`: the cancelled-predecessor sentence becomes: a cancelled plan before it keeps the later plan parked with `plan <n> (job <id>) was cancelled; cancel this plan, or replace plan <n> with factory new --replaces <id>`; after a replacement the check waits on the replacement. Series lookups by index prefer a job that is not cancelled.
- `README.md`: one line under `factory new` naming `--replaces <id>` and pointing to `docs/reference/software-factory.md`.

Next plan: docs/superpowers/plans/2026-11-22-series-cancel-and-replace/plan-02-series-cancel-and-replace.md
