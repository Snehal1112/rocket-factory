# Cancel the Rest of a Series Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-22-series-cancel-and-replace-design.md
> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** `factory cancel` on a series plan cancels every later unmerged plan too, `cleanup` clears a finished series in one call, and the supervisor and dashboard follow.

**Commit:** feat(cancel): cancel the rest of a series

**Architecture:** `cancel` in `factory-cli` collects the target and its later unmerged plans, checks every lease, and marks them all in one `store().update`. `cleanup` loops over the cancelled members of a finished series. The supervisor's `CancelBody` gains `only`, and the dashboard names the cascade in its cancel dialog title.

**Tech Stack:** Rust, `cargo test -j4 -p <crate> <filter>`, plain JS tested with `node --test dashboard/tests/*.test.js`.

## Global Constraints

- cargo commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- The new `operator.cancel` field `cascade_from` is optional: it is logged only on cascaded jobs. No board or ledger field is added.
- Git only through `factory_git::Repo`; paths only through `Layout`.
- No unwrap in production Rust. Tests use `.expect("...")` and `expect_err`.
- Comments are short full sentences that end with a period.
- Each task ends with `cargo fmt -p <crate> -- --check` and `cargo clippy -j4 -p <crate> --all-targets -- -D warnings` for every crate it touched.

## File Structure

| File | Change |
|---|---|
| `crates/factory-cli/src/cli.rs` | `Cancel` gets `--only`. |
| `crates/factory-cli/src/commands.rs` | `cancel` cascades; `cleanup` cleans a finished series. |
| `crates/factory-cli/tests/new_to_merged.rs` | `series_pair` uses `--only`; new cascade and cleanup tests. |
| `crates/factory-cli/tests/operator_events.rs` | `cascade_from` event test. |
| `crates/factory-supervisor/src/api/write.rs` | `CancelBody.only`. |
| `dashboard/static/projects.js`, `dashboard/static/app.js` | `cancelTitle(job)` for the cancel dialog. |
| `dashboard/tests/projects.test.js` | `cancelTitle` tests. |
| `docs/reference/software-factory.md`, `README.md` | Cascade, `--only`, series cleanup, API `only`. |

## Review Focus

1. A cascade where a later plan's lease is stale (owner gone) rather than live: it must not block the cancel. Test: `cancel_ignores_a_stale_lease_on_a_later_plan` (set a lease whose expiry is in the past, as `lease_is_live` reads it).
2. `cancel <slug>` of a draft must still remove the draft card and never cascade. Test: `cancel_of_a_draft_slug_still_removes_the_draft` (reuse the draft fixture in `crates/factory-cli/tests/draft_cards.rs`).
3. Cleanup of a finished series where one member's branch has commits and `--discard-commits` is not given: the others are cleaned, that one is reported and keeps its card. Test: `cleanup_reports_a_refused_member_and_goes_on`.
4. A later plan that is merged while an earlier one is cancelled (legacy order): the cascade must leave it merged. Test: `cancel_never_touches_a_merged_later_plan`.
5. A job in a one-plan "series" (no series id) with `--only`: same output as plain cancel. Test: `cancel_without_series_is_unchanged`, run once with and once without `--only`.

---

### Task 1: Cancel cascades to later plans

**Files:**
- Modify: `crates/factory-cli/src/cli.rs:147-156` (`Command::Cancel`)
- Modify: `crates/factory-cli/src/commands.rs:415-418` and `cancel` (`:910-954`)
- Test: `crates/factory-cli/tests/new_to_merged.rs`, `crates/factory-cli/tests/operator_events.rs`
- Modify: `docs/reference/software-factory.md:45`

**Interfaces:**
- Consumes: `factory_core::Board::replacement_of(&self, id: &str) -> Option<&Job>`, added by the replace plan.
- Produces: `--only` on `factory cancel` (`only: bool`). `cancel(ctx: &CliCtx, id: &str, reason: &str, force: bool, only: bool) -> Result<String, CliError>`.
- No engine port: the work is in `factory-cli`, on the board through `Driver::store()`.

- [ ] **Step 1: Read the code and docs**

Read spec sections "Cancel cascades to later plans" and "Constraints", `cancel` and `remove_cancelled_card` in `crates/factory-cli/src/commands.rs` (lines 910-1031), the helpers `series_pair`, `parked_and_cancelled`, `board_ids` and `cli_series` (added by the replace plan) in `crates/factory-cli/tests/new_to_merged.rs`, and `cancel_logs_reason_and_force` in `crates/factory-cli/tests/operator_events.rs`.

- [ ] **Step 2: Keep the old fixture meaning**

`series_pair` calls `fx.run(&["cancel", "1"])` while job 002 must stay as set. Change that call to `fx.run(&["cancel", "1", "--only"])`. Keep `fx.run(&["cancel", "2"])` for `Sibling::Cancelled`.

- [ ] **Step 3: Write the failing tests**

In `new_to_merged.rs`, a three-plan series helper and the tests:

```rust
/// A three-plan series made through the CLI: jobs 001, 002, 003.
fn cli_three(fx: &Fixture) {
    let three = fx.write_plan("plan-03-c.md", 1, "none");
    let two = fx.write_plan("plan-02-b.md", 1, three.to_str().expect("utf8"));
    let one = fx.write_plan("plan-01-a.md", 1, two.to_str().expect("utf8"));
    fx.run(&["new", "a", "three plans", "--plans", one.to_str().expect("utf8")]).expect("new");
}

fn job_of(fx: &Fixture, id: &str) -> factory_core::Job {
    factory_engine::BoardStore::new(
        &factory_core::Layout::in_repo(&fx.root).board_file(),
        std::time::Duration::from_secs(15),
    )
    .load()
    .expect("board")
    .find(id)
    .expect("job")
    .clone()
}

#[test]
fn cancel_cascades_to_later_unmerged_plans() {
    let fx = Fixture::new();
    cli_three(&fx);
    let out = fx.run(&["cancel", "1", "--reason", "plan asked for missing code"]).expect("cancel");
    let sid = job_of(&fx, "001").series.expect("pos").id.expect("sid");
    assert_eq!(
        out,
        format!("job 001 cancelled\njob 002 cancelled (later plan of {sid})\njob 003 cancelled (later plan of {sid})")
    );
    assert_eq!(job_of(&fx, "001").reason.as_deref(), Some("cancelled: plan asked for missing code"));
    for id in ["002", "003"] {
        let j = job_of(&fx, id);
        assert!(j.cancelled, "{id}");
        assert_eq!(j.reason.as_deref(), Some("cancelled: plan 1 (job 001) was cancelled"));
    }
}

#[test]
fn cancel_only_leaves_later_plans() {
    let fx = Fixture::new();
    cli_three(&fx);
    assert_eq!(fx.run(&["cancel", "1", "--only"]).expect("cancel"), "job 001 cancelled");
    assert!(!job_of(&fx, "002").cancelled);
    assert!(!job_of(&fx, "003").cancelled);
}

#[test]
fn cancel_refuses_when_a_later_plan_holds_a_live_lease() {
    let fx = Fixture::new();
    cli_three(&fx);
    fx.hold_lease("003", "other");
    let e = fx.run(&["cancel", "1"]).expect_err("lease");
    assert!(e.to_string().contains("job 003 has a live lease"), "{e}");
    for id in ["001", "002", "003"] {
        assert!(!job_of(&fx, id).cancelled, "{id} unchanged");
    }
}
```

The other cases share this shape: build the state, run the command, assert the output and each job's `cancelled` flag.

| Test | Setup | Command | Expect |
|---|---|---|---|
| `cancel_middle_plan_keeps_earlier_merged` | `cli_three`; job 001 set to `Stage::Merged` through `BoardStore::update_job` | `cancel 2` | 001 merged and not cancelled; 002, 003 cancelled; 003's reason names `plan 2 (job 002)` |
| `cancel_never_touches_a_merged_later_plan` | `cli_three`; job 003 set to `Stage::Merged` | `cancel 1` | 001, 002 cancelled; 003 merged, not cancelled, not in the output |
| `cancel_skips_an_already_cancelled_later_plan` | `cli_three`; `cancel 3 --only` first | `cancel 1` | output has 001 and 002 lines only |
| `cancel_of_a_cancelled_job_cascades` | `cli_three`; `cancel 1 --only` first | `cancel 1` | 002 and 003 cancelled |
| `cancel_of_a_replaced_job_is_refused` | `cli_series`; `new fixed x --plans <fixed> --replaces 1` (job 003) | `cancel 1` | error contains `job 001 was replaced by job 003, cancel that one`; 002 not cancelled |
| `cancel_without_series_is_unchanged` | `parked_and_cancelled(&fx, "hello")` | `cancel 1`, then on a fresh fixture `cancel 1 --only` | both outputs are exactly `job 001 cancelled` |
| `cancel_ignores_a_stale_lease_on_a_later_plan` | `cli_three`; set job 003's lease to `factory_core::Lease { owner: "other".into(), expires: 1.0 }` through `BoardStore::update_job` | `cancel 1` | all three cancelled |
| `cancel_of_a_draft_slug_still_removes_the_draft` | in `draft_cards.rs`, the draft fixture used by its existing `cancel <slug>` test | `cancel <slug>` | the same output and board as that existing test |

In `operator_events.rs` add `cancel_cascade_logs_one_event_per_job_with_cascade_from`: a three-plan series made with `new` (as `cli_three` does), `factory_telemetry::capture("info", || sc.run(&["cancel", "1"]))`, then `events(&lines, "operator.cancel")` has 3 entries; the entries for `002` and `003` have `["cascade_from"] == "001"` and the entry for `001` has no `cascade_from` key.

- [ ] **Step 4: Add the `--only` flag with no behaviour yet**

`Fixture::run` uses `Cli::parse_from`, which exits the test process on an unknown argument, so the flag comes first. `Command::Cancel` gets `#[arg(long)] only: bool` with the doc comment `Cancel only this job, not the later plans of its series.` Pass it through `dispatch` to `cancel(ctx, &id, &reason, force, only)`, which ignores it for now.

- [ ] **Step 5: Run the tests and see them fail**

Run: `cargo test -j4 -p factory-cli cancel_`
Expected: FAIL. `cancel_cascades_to_later_unmerged_plans` sees output `job 001 cancelled` only, and `cancel_of_a_replaced_job_is_refused` cancels job 001.

- [ ] **Step 6: Implement the cascade**

- In `cancel`, inside one `d.store().update(|board| ...)`:
  1. The target must exist and not be merged (`job <id> is already merged`). If `board.replacement_of(id)` is `Some(n)`, refuse with `job <id> was replaced by job <n>, cancel that one`.
  2. The set is the target, plus, unless `only`, every job with the same series id, a higher index, not merged and not cancelled, in index order.
  3. Without `force`, if any job in the set has `lease_is_live(job, now)`, refuse with `job <that id> has a live lease, use --force to cancel anyway` and change nothing. For the target alone keep today's text `job has a live lease, use --force to cancel anyway`.
  4. Mark each job as `cancel` does today: `set_stage_logged(j, Stage::NeedsHuman, Some(text), false, now)`, `cancelled = true`, clear `lease`, `task_session`, `builder_last`. The target's text is `cancelled: <reason>`; a later job's is `cancelled: plan <target index> (job <target id>) was cancelled`.
- After the board write, per job in order: a ledger ruling with its text folded to one line, and `operator.cancel` inside `factory_engine::job_span(<that id>)`. Later jobs log `cascade_from = <target id>` as well.
- Output: `job <target> cancelled`, then one line `job <id> cancelled (later plan of <sid>)` per later job, then the existing `project lease released` line when `--force` released it.

- [ ] **Step 7: Run the tests**

Run: `cargo test -j4 -p factory-cli cancel_`
Expected: all pass. Then `cargo test -j4 -p factory-cli` passes. Any other test that cancelled a series member and expected the later plans untouched gets `--only`, with no other change.

- [ ] **Step 8: Update the docs**

In `docs/reference/software-factory.md:45`, replace "Cancelling one plan stops the series and leaves the later plans waiting at `spec`, so cancel each of them too if the series is dropped." with: `factory cancel <id>` on a series plan also cancels every later plan of the series that is not merged or cancelled, and prints one line per job (`job 031 cancelled (later plan of S7)`); their reason is `cancelled: plan <n> (job <id>) was cancelled`. `--only` cancels only that job. A live lease on any of them refuses the whole cancel without `--force`. A job that was replaced cannot be cancelled; cancel its replacement. Each cancelled job logs `operator.cancel`, and cascaded ones carry `cascade_from`. Add `cascade_from` to the `operator.cancel` field list at line 198.

### Task 2: Cleanup of a whole series

**Files:**
- Modify: `crates/factory-cli/src/commands.rs` (`cleanup` at `:958`, `series_is_finished` at `:1057`)
- Test: `crates/factory-cli/tests/new_to_merged.rs`
- Modify: `docs/reference/software-factory.md` (the cleanup paragraph), `README.md`

**Interfaces:**
- Consumes: `cleanup_checkout(ctx, job, discard)`, `remove_cancelled_card(ctx, id)`, `series_is_finished(board, id)` in `commands.rs`.
- Produces: no new public names. `cleanup` output gains one line per extra member: `job <id>: <message>`.
- No engine port.

- [ ] **Step 1: Read the code**

Read `cleanup`, `remove_cancelled_card`, `series_is_finished`, `member_ids` and `cleanup_checkout` in `crates/factory-cli/src/commands.rs:956-1160`, and the tests `cleanup_removes_cancelled_card_when_series_sibling_is_cancelled` and `cleanup_keeps_the_card_of_a_cancelled_series_member_with_a_sibling` in `new_to_merged.rs`.

- [ ] **Step 2: Write the failing tests**

`cleanup_removes_cancelled_card_when_series_sibling_is_cancelled` now expects both cards gone. Change its last assertion to `assert!(board_ids(&fx).is_empty());` and add `assert!(out.contains("job 002: "), "{out}");`.

New tests:

```rust
#[test]
fn cleanup_of_finished_series_cleans_every_cancelled_member() {
    let fx = Fixture::new();
    cli_three(&fx);
    fx.run(&["cancel", "1"]).expect("cancel");
    let out = fx.run(&["cleanup", "1", "--discard-commits"]).expect("cleanup");
    assert!(out.contains("job 002: "), "{out}");
    assert!(out.contains("job 003: "), "{out}");
    assert!(board_ids(&fx).is_empty(), "{out}");
}

#[test]
fn cleanup_of_unfinished_series_cleans_only_the_target() {
    let fx = Fixture::new();
    cli_three(&fx);
    fx.run(&["cancel", "3", "--only"]).expect("cancel");
    let out = fx.run(&["cleanup", "3"]).expect("cleanup");
    assert!(!out.contains("job 00"), "{out}");
    assert_eq!(board_ids(&fx).len(), 3);
}
```

`cleanup_reports_a_refused_member_and_goes_on`, on `Fixture::with_risk_path("docs/**")`:
1. `cli_three(&fx)`, then set job 001 to `Stage::Merged` through `BoardStore::update_job`, so plan 2 may build.
2. `fx.run(&["run", "2"])` returns a line starting `needs-human`: the builder wrote under `docs/`, a risk path, so job 002's branch has commits (as in `approve_merges_a_risky_job_that_run_parked`).
3. `fx.run(&["cancel", "2"])` cancels 002 and 003.
4. `fx.run(&["cleanup", "3"])` without `--discard-commits` is `Ok`; its output contains `job 002: ` and `--discard-commits`.
5. `board_ids(&fx)` is `["001", "002"]`: 003's card is gone, 002 keeps its card, 001 is merged and stays.

- [ ] **Step 3: Run them and see them fail**

Run: `cargo test -j4 -p factory-cli --test new_to_merged cleanup_`
Expected: FAIL. `cleanup_of_finished_series_cleans_every_cancelled_member` finds `002` and `003` still on the board.

- [ ] **Step 4: Implement**

In `cleanup`, after the target is handled: when the target is cancelled and not merged, load the board; if it has a series id and `series_is_finished` holds, take every other member that is cancelled, not merged and not `cleaned`, in index order. For each, run the same steps as for the target (`cleanup_checkout`, set `cleaned`, `remove_cancelled_card`) and append `\njob <id>: <message>`. A `CliError::Refused(why)` for a member appends `\njob <id>: <why>` and goes on; other errors stop with that error.

- [ ] **Step 5: Run the tests**

Run: `cargo test -j4 -p factory-cli --test new_to_merged`
Expected: all pass. Then `cargo test -j4 -p factory-cli` passes.

- [ ] **Step 6: Update the docs**

In `docs/reference/software-factory.md` and the `cleanup` lines of `README.md`: once every member of a series is merged or cancelled, `factory cleanup <id>` of a cancelled member also cleans every other cancelled member, one line each (`job <id>: ...`); `--discard-commits` applies to all of them, and a member whose cleanup is refused keeps its card while the others go on.

### Task 3: Supervisor `only` and the dashboard dialog

**Files:**
- Modify: `crates/factory-supervisor/src/api/write.rs:59-62` (`CancelBody`) and `cancel_inner` (`:506-530`)
- Test: `crates/factory-supervisor/src/api/write.rs` (`mod tests`)
- Modify: `dashboard/static/projects.js` (add `cancelTitle`, export it next to `buildCancelBody` at line 220), `dashboard/static/app.js:485`
- Test: `dashboard/tests/projects.test.js`
- Modify: `README.md:546` (API table row)

**Interfaces:**
- Consumes: `factory cancel --only`, added with the cancel cascade.
- Produces: `CancelBody { reason: Option<String>, force: Option<bool>, only: Option<bool> }`; JS `cancelTitle(job) -> string`.
- No engine port.

- [ ] **Step 1: Read the code**

Read `cancel_inner` and `cancel_checks_force_and_surfaces_a_cli_refusal_as_conflict` in `write.rs`, `buildCancelBody` in `projects.js`, the dialog code in `app.js:476-512` and the `buildCancelBody` test in `dashboard/tests/projects.test.js:160`.

- [ ] **Step 2: Write the failing tests**

In `write.rs` tests, following `cancel_checks_force_and_surfaces_a_cli_refusal_as_conflict`:

```rust
#[test]
fn cancel_body_only_adds_the_flag() {
    let f = fx();
    f.cli.reply_stdout("job 001 cancelled\n");
    let r = post(&f, "/projects/good/jobs/1/cancel", br#"{"only":true}"#);
    assert_eq!(r.status, 200);
    assert_eq!(
        f.cli.calls()[0].1,
        vec!["cancel".to_string(), "--reason=cancelled by user".to_string(), "--only".to_string(), "--".to_string(), "001".to_string()]
    );
    let r = post(&f, "/projects/good/jobs/1/cancel", br#"{"only":"yes"}"#);
    assert_eq!(code(&r), (400, "bad_request".to_string()));
}
```

In `dashboard/tests/projects.test.js`:

```js
test("cancelTitle names the later plans of a series", () => {
  assert.equal(projects.cancelTitle({ id: "030" }), "Cancel job 030");
  assert.equal(projects.cancelTitle({ id: "030", series: { id: "S7", index: 1, total: 5 } }), "Cancel job 030 and the later plans of S7");
  assert.equal(projects.cancelTitle({ id: "034", series: { id: "S7", index: 5, total: 5 } }), "Cancel job 034");
  assert.equal(projects.cancelTitle({ id: "040", series: { index: 1, total: 2 } }), "Cancel job 040");
});
```

- [ ] **Step 3: Run them and see them fail**

Run: `cargo test -j4 -p factory-supervisor cancel_body_only_adds_the_flag`
Expected: FAIL, the CLI args have no `--only`. `{"only":true}` is refused as an unknown field if the body is `deny_unknown_fields`, which also fails the test.
Run: `node --test dashboard/tests/projects.test.js`
Expected: FAIL with `projects.cancelTitle is not a function`.

- [ ] **Step 4: Implement**

- `CancelBody` gets `only: Option<bool>`; `cancel_inner` pushes `--only` after `--force` handling when it is `Some(true)`.
- `cancelTitle(job)` returns `Cancel job <id> and the later plans of <series.id>` when `job.series` has an `id` and `index < total`, else `Cancel job <id>`. Export it.
- In `app.js`, the cancel title uses `FactoryProjects.cancelTitle(job)` for the job the button belongs to, found the same way the surrounding click handler finds the job for its other actions; when that job cannot be found, keep `Cancel job <id>`.

- [ ] **Step 5: Run the tests**

Run: `cargo test -j4 -p factory-supervisor`
Expected: all pass.
Run: `node --test dashboard/tests/*.test.js`
Expected: all pass.

- [ ] **Step 6: Update the docs**

`README.md:546`: the cancel row body becomes `{"reason"?, "force"?, "only"?}` (`force` and `only` are booleans). Add: without `only`, the cancel also cancels the later unmerged plans of the job's series; `only: true` passes `--only`.

Next plan: none
