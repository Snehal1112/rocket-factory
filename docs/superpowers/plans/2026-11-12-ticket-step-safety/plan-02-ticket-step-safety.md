# Ticket step safety, step changes Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-12-ticket-step-safety-design.md
> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Make `tickets_step` recover from a partial create, cut an over-long body, and make a real `factory tickets` run hold the job lease.

**Architecture:** The step uses the pieces from the previous plan: narrow ref parsing, unique titles and `Tracker::find_created`. A new `pending` list in `tickets.json` records an attempt in flight. A new `Driver::hold_job` takes only the job lease, and the CLI uses it.

**Tech Stack:** Rust, `serde_json`, the `Tracker` port, `BoardStore` leases.

## Global Constraints

- Commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`. Never `--workspace` or `--all`.
- The ticket step never changes the merge result: failures go to the report, the ledger line, stderr and events.
- Only `git` may not be shelled out to. `gh` stays behind the `Tracker` port.
- No `Cargo.toml` change and no new crate dependency.
- `tickets.json` stays readable by old files: the new `pending` key is optional on load. A task that adds a field to the board, ledger or events says it is optional; this plan adds none to those.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- Comments are short full sentences that end with a punctuation mark.

## Review Focus

1. A `tickets.json` whose `pending` key has the wrong type loads as an empty list. Pinned by `a_marker_with_a_bad_pending_value_loads_as_empty`.
2. A feature that cites both `#7` and `other/repo#5` skips the step and closes nothing. Pinned by `a_mixed_feature_skips_and_closes_nothing`.
3. A body of exactly 60,000 characters is not cut, 60,001 is, and a multi-byte char at the cut point never panics. Pinned by `fit_body_keeps_a_body_at_the_limit`, `fit_body_cuts_one_char_over_the_limit` and `fit_body_cuts_on_a_char_boundary`.
4. A refused `hold_job` leaves the other owner's lease untouched. Pinned by `hold_job_refusal_leaves_the_other_lease_intact`.
5. A dry run is still refused by another owner's live lease, and still takes no lease. Pinned by `tickets_dry_run_is_refused_by_another_owners_live_lease`.

## File Structure

| File | Responsibility |
|---|---|
| `crates/factory-engine/src/tickets.rs` | `pending` in `Marker`, the recovery flow, `fit_body` |
| `crates/factory-engine/src/compat.rs` | `Driver::hold_job` and `JobGuard` |
| `crates/factory-engine/src/lib.rs` | Export `JobGuard` |
| `crates/factory-cli/src/commands.rs` | `tickets` takes the job lease on a real run |
| `crates/factory-engine/tests/tickets.rs` | Step tests and `hold_job` tests |
| `crates/factory-cli/tests/tickets.rs` | CLI lease tests |
| `README.md`, `docs/reference/software-factory.md` | Ticket hygiene text |

### Task 1: Pending entries and recovery in `tickets_step`

**Files:**
- Modify: `crates/factory-engine/src/tickets.rs` (`Marker` at ~45, `tickets_step` loop at ~178 to 221)
- Modify: `crates/factory-engine/tests/common/mod.rs` (`FakeState`, `FakeTracker::create`)
- Test: `crates/factory-engine/tests/tickets.rs`

**Interfaces:**
- Consumes: `Tracker::find_created(&self, title: &str, footer: &str) -> Result<Option<u32>, String>`, `FakeTracker::with_found(self, title: &str, number: u32) -> FakeTracker`, `parse_issue_refs` with shorthand refs, and unique `FollowUp` titles, all from the previous plan.
- Produces: `Marker` gains `pending: Vec<String>`, saved as `"pending": ["title"]` and optional on load. `tickets_step` keeps its signature. A test helper `FakeTracker::with_on_create(self, hook: impl Fn() + Send + Sync + 'static) -> FakeTracker` in `crates/factory-engine/tests/common/mod.rs`: `FakeState` gains `on_create: Option<Arc<dyn Fn() + Send + Sync>>`, and `FakeTracker::create` calls the hook just before it returns `Ok`. If `FakeState` derives `Debug`, drop that derive or give the field a small wrapper type. This logic sits behind the existing `Tracker` port.

- [ ] **Step 1: Read the code and docs this task touches.** Read `crates/factory-engine/src/tickets.rs` in full, the whole of `crates/factory-engine/tests/tickets.rs` (fixtures `merged`, `tracker`, `SPEC_WITH_TWO`, and how `a_broken_marker_after_a_merge_is_reported_and_the_job_stays_merged` writes a marker file), `FakeTracker` in `tests/common/mod.rs`, and the "Design, 2" section and the edge-case table of the spec.
- [ ] **Step 2: Write the failing tests** in `crates/factory-engine/tests/tickets.rs`. Add a helper `fn seed_marker(fx: &MergeFixture, json: &str)` that writes `tickets.json` into `fx.driver.job_dir(&fx.driver.job(&fx.id).expect("job"))`, the way the broken-marker test does.

```rust
#[test]
fn a_pending_entry_adopts_the_existing_issue() {
    let t = tracker().with_found("Shared name helpers", 210);
    let fx = merged(Some(SPEC_WITH_TWO), &t);
    seed_marker(&fx, r#"{"created":[],"pending":["Shared name helpers"]}"#);
    let report = tickets_step(&fx.driver, &fx.id, false).expect("step");
    assert!(!report.failed, "{report:?}");
    assert!(report.lines.iter().any(|l| l == "adopted #210: Shared name helpers"), "{report:?}");
    assert_eq!(t.count("create Shared name helpers"), 0);
    assert_eq!(t.count("create Merge QueryParam and Header"), 1);
}
```

Write the others the same way, each with its own seeded marker and asserts on `t.count(..)`, `report.lines` and the marker file text:
  - `a_pending_entry_with_no_match_creates_once`: pending title, nothing found, `create` count is 1 for that title, and the saved marker lists it under `created` and not under `pending`.
  - `a_failed_find_stops_without_creating`: `t.fail_on("find_created")`, the report is `failed`, `create` count is 0, the marker still lists the title under `pending`.
  - `a_failed_create_leaves_a_pending_entry`: `t.fail_on("create")` on a fresh job, the report is `failed`, the saved marker lists the title under `pending`.
  - `a_marker_save_failure_reports_the_created_number`: build the tracker with `.with_on_create(hook)`, where the hook removes `tickets.json` (it exists after the pending save) and creates a directory at that path, so the save after `create` fails on every platform and for any user. Assert the report has a line that starts with `failed: created #` and contains `could not record it`.
  - `an_old_marker_without_pending_is_honoured`: seed `{"created":[{"title":"Shared name helpers","number":205}],"commented":[],"closed":[]}`, run the step, and assert `create` count for that title is 0.
  - `a_marker_with_a_bad_pending_value_loads_as_empty`: seed `{"created":[],"pending":"nope"}`, run the step, and assert the step creates both follow-ups and `find_created` count is 0.
  - `a_dry_run_with_a_pending_entry_makes_no_find_call`: seed a pending title, run with `dry_run = true`, assert the report has the line `would look for an existing issue, then create: Shared name helpers`, `t.count("find_created")` is 0 and the marker file text is unchanged.
  - `skips_a_shorthand_ref_to_another_repository`: `set_feature` to `"dedup cleanups other/repo#5"`, run the step, assert the report is `skipped` with a line containing `belongs to other/repo`, and `t.calls()` holds no `create`, `comment` or `close`.
  - `a_mixed_feature_skips_and_closes_nothing`: feature `"cleanups #7 and other/repo#5"`, assert skipped and `t.state.lock().expect("lock").closes` is empty.
  - `two_follow_ups_with_the_same_title_file_two_tickets`: a spec whose Follow-ups hold two bullets that start with `Same name.`; assert two `create` calls with titles `Same name` and `Same name (2)` and that the parent comment lists two different numbers.

- [ ] **Step 3: Run them and see them fail.** Run `cargo test -j4 -p factory-engine --test tickets`. Expected: the new tests fail (no `adopted` line, no `pending` handling); the existing tests pass.
- [ ] **Step 4: Implement `pending` in `Marker`.** Add `pending: Vec<String>`; in `Marker::load` read `v["pending"]` as an array of strings and treat any other shape as an empty list; in `Marker::save` write `"pending": self.pending`.
- [ ] **Step 5: Implement the recovery flow in the `tickets_step` loop.** Per item: skip when `created` holds the title. In a dry run, for a pending title push `would look for an existing issue, then create: {title}` and continue; otherwise keep today's `would create` line. In a real run, for a pending title call `tracker.find_created(&item.title, &footer)` where `footer` is `format!("Filed by factory job {} ({})", job.id, job.slug)`; on `Ok(Some(n))` move the title from `pending` to `created`, save, push `adopted #{n}: {title}` and continue; on `Err(e)` call `report.fail(format!("find `{}`: {e}", item.title))` and return through `finish`. Before `create`, add the title to `pending` when it is not there and save; a save error becomes `report.fail` and returns through `finish` without creating. On `create` `Ok(n)` remove the title from `pending`, push to `created` and save; a save error becomes `report.fail(format!("created #{n} `{}` but could not record it: {e}", item.title))` returned through `finish`. On `create` `Err` leave the title pending and fail as today. The body text keeps the line `Filed by factory job {id} ({slug}) from the follow-ups of {parents}.`, which starts with the footer.
- [ ] **Step 6: Run the tests and see them pass.** Run `cargo test -j4 -p factory-engine --test tickets` and `cargo clippy -j4 -p factory-engine`. Expected: every test in `tests/tickets.rs`, old and new, passes.

### Task 2: Cut an over-long follow-up body

**Files:**
- Modify: `crates/factory-engine/src/tickets.rs` (the `body` built in the loop at ~190)
- Test: `crates/factory-engine/src/tickets.rs` unit tests and `crates/factory-engine/tests/tickets.rs`

**Interfaces:**
- Consumes: the recovery flow from the previous task, which builds `body` for `tracker.create`.
- Produces: `pub(crate) fn fit_body(item_body: &str, footer_text: &str) -> String` in `tickets.rs`, returning `item_body` plus the footer lines, at most 60,000 chars in total. The logic is pure and sits behind no port.

- [ ] **Step 1: Read the code this task touches.** Read the body construction in `crates/factory-engine/src/tickets.rs` lines 190 to 196, and the "Design, 5" section of the spec.
- [ ] **Step 2: Write the failing tests.** Unit tests in a `#[cfg(test)] mod tests` of `tickets.rs`, with `const FOOT: &str = "\n\nFiled by factory job 001 (s) from the follow-ups of #1.";`:

```rust
#[test]
fn fit_body_keeps_a_body_at_the_limit() {
    let body = "a".repeat(60_000 - FOOT.chars().count());
    let out = fit_body(&body, FOOT);
    assert_eq!(out.chars().count(), 60_000);
    assert!(!out.contains("cut to fit"));
}

#[test]
fn fit_body_cuts_one_char_over_the_limit() {
    let body = "a".repeat(60_000 - FOOT.chars().count() + 1);
    let out = fit_body(&body, FOOT);
    assert!(out.chars().count() <= 60_000, "{}", out.chars().count());
    assert!(out.contains("This text was cut to fit GitHub's limit."));
    assert!(out.ends_with(FOOT));
}

#[test]
fn fit_body_cuts_on_a_char_boundary() {
    let body = "é".repeat(70_000);
    let out = fit_body(&body, FOOT);
    assert!(out.chars().count() <= 60_000);
    assert!(out.ends_with(FOOT));
}
```

  Add the engine test `an_over_long_body_is_cut_and_the_parent_closes` in `tests/tickets.rs`: a spec whose one Follow-ups bullet is `"x".repeat(70_000)`; after the step, the created body has at most 60,000 chars and contains the cut line, and the parent is in `closes`.

- [ ] **Step 3: Run them and see them fail.** Run `cargo test -j4 -p factory-engine fit_body` and `cargo test -j4 -p factory-engine --test tickets an_over_long`. Expected: `cannot find function 'fit_body' in this scope`, then a body longer than 60,000 chars.
- [ ] **Step 4: Implement `fit_body`** with a `const MAX_BODY_CHARS: usize = 60_000;`. When the char count of `item_body` plus `footer_text` is at most the limit, return their concatenation. Otherwise keep the leading chars of `item_body` that leave room for the cut line `\n\nThis text was cut to fit GitHub's limit.` and `footer_text`, and return the three joined. Count chars, never bytes.
- [ ] **Step 5: Use it in the loop.** Build the footer text `\n\nFiled by factory job {id} ({slug}) from the follow-ups of {parents}.` and pass `fit_body(&item.body, &footer_text)` to `tracker.create`.
- [ ] **Step 6: Run the tests and see them pass.** Run `cargo test -j4 -p factory-engine` and `cargo clippy -j4 -p factory-engine`. Expected: all pass.

### Task 3: Job lease for a real `factory tickets` run, and the docs

**Files:**
- Modify: `crates/factory-engine/src/compat.rs` (near `hold_project` at ~358 and `ProjectGuard` at ~147), `crates/factory-engine/src/lib.rs` (the `compat` re-export at ~66)
- Modify: `crates/factory-cli/src/commands.rs` (`tickets` at ~1062)
- Modify: `README.md` (Ticket hygiene at ~89), `docs/reference/software-factory.md` (Ticket hygiene at ~119)
- Test: `crates/factory-engine/tests/tickets.rs`, `crates/factory-cli/tests/tickets.rs`

**Interfaces:**
- Consumes: `acquire_lease(store, id, owner, ttl, now)` and `release_lease(store, id, owner, now)` from `crates/factory-engine/src/lease.rs`, the driver's `lease_owner`, `lease_ttl` and `store`.
- Produces: `pub fn hold_job(&self, id: &str) -> Result<JobGuard<'_>, EngineError>` on `Driver`, and `pub struct JobGuard<'a>` exported from the crate. It takes only the job lease, never the project lease. Without a lease owner it returns a guard that does nothing. Dropping the guard calls the free function `release_lease` and ignores its error. The logic sits behind no trait; it uses `BoardStore`.

- [ ] **Step 1: Read the code and docs this task touches.** Read `hold_project`, `ProjectGuard`, `renew_lease` and `release_lease` in `crates/factory-engine/src/compat.rs` (lines 140 to 160 and 320 to 415), `crates/factory-engine/src/lease.rs`, the `tickets` function in `crates/factory-cli/src/commands.rs` (~1062), the two Ticket hygiene sections, and `SeriesFixture::owned` in `crates/factory-engine/tests/common/mod.rs` for how a driver gets a lease owner.
- [ ] **Step 2: Write the failing engine tests** in `crates/factory-engine/tests/tickets.rs`. Add a helper `fn owned(owner: &str) -> MergeFixture` that calls `approved_job()` and then sets `fx.driver = fx.driver.with_lease_owner(owner)` the way `SeriesFixture::owned` does (move the driver out first).

```rust
#[test]
fn hold_job_refuses_another_owner_and_releases_on_drop() {
    let fx = owned("owner-a");
    {
        let _guard = fx.driver.hold_job(&fx.id).expect("hold");
        let lease = fx.driver.job(&fx.id).expect("job").lease.expect("lease held");
        assert_eq!(lease.owner, "owner-a");
        let err = factory_engine::acquire_lease(
            fx.driver.store(), &fx.id, "owner-b", 600.0, factory_engine::now_secs(),
        )
        .expect_err("held");
        assert!(err.to_string().contains("owner-a"), "{err}");
    }
    assert!(fx.driver.job(&fx.id).expect("job").lease.is_none());
}

#[test]
fn hold_job_takes_no_project_lease() {
    let fx = owned("owner-a");
    let _guard = fx.driver.hold_job(&fx.id).expect("hold");
    let state = factory_core::Layout::in_repo(&fx.main).state_dir;
    assert!(matches!(
        factory_engine::project_lease_status(&state, factory_engine::now_secs()),
        factory_engine::ProjectLeaseStatus::Free
    ));
}

#[test]
fn hold_job_refusal_leaves_the_other_lease_intact() {
    let fx = owned("owner-a");
    factory_engine::acquire_lease(fx.driver.store(), &fx.id, "owner-b", 600.0, factory_engine::now_secs())
        .expect("b holds");
    fx.driver.hold_job(&fx.id).expect_err("refused");
    let lease = fx.driver.job(&fx.id).expect("job").lease.expect("still held");
    assert_eq!(lease.owner, "owner-b");
}

#[test]
fn hold_job_without_a_lease_owner_does_nothing() {
    let fx = approved_job();
    let _guard = fx.driver.hold_job(&fx.id).expect("hold");
    assert!(fx.driver.job(&fx.id).expect("job").lease.is_none());
}
```

  Add the CLI tests to `crates/factory-cli/tests/tickets.rs`. Create the job as the existing test there does (`fx.write_plan`, `fx.run(&["new", ...])`) and use `fx.hold_lease("001", owner)` from `tests/common`. They run against an unmerged job with the `NoCalls` tracker, so the step itself skips without a call:
  - `tickets_dry_run_is_refused_by_another_owners_live_lease`: `fx.hold_lease("001", "someone-else")`, run `["tickets", "1", "--dry-run"]`, expect an error containing `leased by someone-else`.
  - `tickets_refuses_while_another_owner_holds_the_job_lease`: the same with a real run.
  - `tickets_releases_its_lease_after_a_run`: run `["tickets", "1"]` with no foreign lease and assert the job's `lease` on the board is `None` afterwards (read the board the way `job_lease` in `project_lease_cli.rs` does).
  - `tickets_runs_while_the_project_is_busy`: take the project lease for another job with `acquire_project_lease` (see `hold_project` in `project_lease_cli.rs`), run `["tickets", "1"]`, expect `Ok` with `skipped` in the output.

- [ ] **Step 3: Run them and see them fail.** Run `cargo test -j4 -p factory-engine --test tickets hold_job` and `cargo test -j4 -p factory-cli --test tickets`. Expected: `no method named 'hold_job' found for struct 'Driver'`; the CLI lease-refusal and project-busy tests may already pass and then pin today's behaviour.
- [ ] **Step 4: Implement `hold_job` and `JobGuard`** in `compat.rs`, modelled on `hold_project` and `ProjectGuard`. `hold_job` returns a no-op guard when `lease_owner` is `None`; otherwise it calls `crate::acquire_lease(&self.store, id, owner, self.lease_ttl, crate::now_secs())?` and returns a guard holding the driver and the id. `Drop` calls `crate::release_lease(&driver.store, &id, owner, crate::now_secs())` and ignores the result. Do not call `Driver::release_lease`, because it also releases the project lease. Export `JobGuard` next to `ProjectGuard` in `lib.rs`.
- [ ] **Step 5: Use it in the `tickets` command.** Keep the existing live-lease check and its `leased by` message first. Then, when `dry_run` is false, bind `let _job = d.hold_job(id)?;` before the three steps run, so the guard lives to the end of the function. A dry run takes no lease. Update the doc comment above `tickets`, which today says it takes no lease.
- [ ] **Step 6: Update the docs.** In the Ticket hygiene section of `README.md` and of `docs/reference/software-factory.md`, add short plain lines: a real `factory tickets` run takes the job lease, so a second run on the same job is refused; a follow-up that was started but not recorded is found again by title and the `Filed by factory job` line; a follow-up body over 60,000 characters is cut with a note; a repeated follow-up title gets a ` (2)` suffix.
- [ ] **Step 7: Run the tests and see them pass.** Run `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli --test tickets --test project_lease_cli` and `cargo clippy -j4 -p factory-engine -p factory-cli`. Expected: all pass and clippy is clean.

Next plan: none
