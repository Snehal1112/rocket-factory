# Act On Review Findings, Plan 02: Approve And Waive

> **Executor:** run with superpowers:subagent-driven-development: one fresh implementer per task, then a reviewer per task.

**Goal:** Approve refuses an open CHANGES verdict unless the human gives a waiver reason, every waiver is recorded, and each waived blocker becomes a ticket after the merge.

**Architecture:** One rule function on `Job` decides whether an approve is allowed, so the CLI and the supervisor's API answer the same way. The CLI records a waiver in an append-only `waivers.json` before it merges. A best-effort waiver-tickets step runs after every merge and from `factory tickets`, through the `Tracker` port. The supervisor carries the reason in its queued request, and the dashboard asks for it.

**Tech Stack:** Rust, clap, serde, `node --test` for the dashboard page.

**Spec:** `docs/superpowers/specs/2026-10-29-act-on-review-findings-design.md` (section 3, section 7 dashboard)

**Depends on:** plan 01 (`Job.last_review`, `Job::changes_open`, `open_blockers`).

## Global Constraints

- Cargo per crate with `-j4`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`. Dashboard: `cargo test -j4 --manifest-path dashboard/Cargo.toml` and `node --test dashboard/tests/*.test.js`.
- A waiver reason follows the rework-note rules: one line, not empty, at most 200 characters.
- `--waive` and `--without-review` never combine.
- Waiver tickets never change a merge result: failures go to the ledger, stderr and events, like the tickets step.
- Waiver tickets are filed whether or not `tickets_on_merge` is set.
- New request, board-view and park-view fields are optional, so older queue files and boards still load.
- `gh` is reached only through `Tracker`; tests use `FakeTracker` (`crates/factory-engine/tests/common/mod.rs`).

## Review Focus

1. A job whose last review asked for changes, then a fix round reset `reviews.code`, then parked `NothingChanged`: plain approve and `--without-review` are both refused (`without_review_is_refused_while_changes_are_open`, Task 1).
2. A CHANGES review with no `[blocker]` line: the waiver holds the whole reply as one blocker and files one ticket (`untagged_changes_waive_one_blocker`, Task 2).
3. Two waivers on one job (a sync review asked for changes again): both entries stay in `waivers.json` and both sets get tickets (`a_second_waiver_is_appended`, Task 2).
4. The tracker fails on `create`: the merge stands, the ledger says so, and `factory tickets <id>` files the pending ones later (`waiver_tickets_retry_through_factory_tickets`, Task 2).
5. A queued request file written before this plan (no `waive_reason`) still parses and runs (`old_request_without_waive_reason_parses`, Task 3).

## File Structure

- Modify `crates/factory-core/src/board.rs`: `Job::approve_refusal`, `Job::changes_open_message`.
- Modify `crates/factory-core/src/park.rs`: `next_for` offers `--waive`.
- Create `crates/factory-engine/src/waivers.rs`: the waiver record and the tickets step.
- Modify `crates/factory-engine/src/{lib.rs,park.rs,merge.rs}`.
- Modify `crates/factory-cli/src/{cli.rs,commands.rs}`.
- Modify `crates/factory-supervisor/src/{api/write.rs,request.rs,spawn.rs}`.
- Modify `dashboard/src/{model.rs,park.rs}`, `dashboard/static/{board.js,app.js,projects.js}`.
- Docs: `docs/guides/factory-user-guide.md`, `docs/reference/factory-admin-manual.md`, `crates/factory-core/assets/global-command.md`.

---

### Task 1: The approve rule and the CLI flag

**Files:**
- Modify: `crates/factory-core/src/board.rs`, `crates/factory-core/src/park.rs`
- Modify: `crates/factory-engine/src/park.rs`, `crates/factory-engine/src/merge.rs` (the two `next_for` callers)
- Modify: `crates/factory-cli/src/cli.rs`, `crates/factory-cli/src/commands.rs`
- Test: `crates/factory-core/src/board.rs` tests, `crates/factory-core/src/park.rs` tests, `crates/factory-cli/tests/new_to_merged.rs`

**Interfaces:**
- Consumes: `Job.last_review`, `Job::changes_open`, `Job::review_missing` (plan 01).
- Produces:
  - `impl Job { pub fn approve_refusal(&self, without_review: bool, waive: bool) -> Option<String> }`: `None` when allowed, else the refusal text. Rules, in this order:
    1. `waive && !self.changes_open()`: `job {id} has no open review findings to waive`.
    2. `self.changes_open() && !waive`: `self.changes_open_message()`.
    3. `self.stage == NeedsHuman && self.review_missing() && !without_review && !self.changes_open()`: `self.review_missing_message()`, as today. A `missing` last review is a missing review, so `--without-review` covers it.
  - `impl Job { pub fn changes_open_message(&self) -> String }`: `job {id} has {B} open review blockers from round {N} ({file}): use rework, cancel, or approve --waive "<reason>" to merge anyway`.
  - `ParkKind::next_for(self, id: &str, review_missing: bool, changes_open: bool) -> Vec<String>`: when `changes_open` and the list holds `factory approve {id}`, it is replaced by `factory approve {id} --waive "<reason>"`; otherwise as today.
  - `Command::Approve { id, without_review, #[arg(long, conflicts_with = "without_review")] waive: Option<String> }`.
  - `pub fn validate_one_line(text: &str, what: &str) -> Result<(), CliError>` in `commands.rs`, with the messages `the {what} is empty`, `the {what} must be one line`, `the {what} is longer than 200 characters`. `validate_rework_note(note)` calls it with `rework note`, so its messages do not change.
  - `approve(ctx, id, without_review, waive: Option<&str>)` calls `validate_one_line(reason, "waive reason")`, then `job.approve_refusal(..)` in place of today's `review_missing` check.

- [ ] **Step 1: Write the failing tests**

In `board.rs`: `approve_refusal_follows_the_last_review` covers every row of spec section 3's table: no `last_review` (as today), `approve`, `changes` (plain refused with the message, `without_review` refused, `waive` allowed), `missing` (plain refused, `without_review` allowed, `waive` refused). `without_review_is_refused_while_changes_are_open`: `last_review` `changes`, `reviews.code` pending, stage `needs-human`: `approve_refusal(true, false)` is `Some` and names `--waive`.

In `park.rs`: `next_offers_waive_while_changes_are_open`: `ParkKind::ChangesRemain.next_for("012", false, true)` contains `factory approve 012 --waive "<reason>"` and not `factory approve 012`. Update the two existing `next_for` tests to pass `false` as the third argument.

In `new_to_merged.rs` (use `Scenario` from `tests/support/mod.rs` with a risky path and `review(CHANGES)` steps until the job parks `ChangesRemain`):
- `approve_refuses_open_changes_and_names_waive`: the error contains `open review blockers` and `--waive`; the stage stays `needs-human`.
- `approve_with_waive_on_a_clean_job_is_refused`: a job parked risky with APPROVE refuses `--waive "x"` with `no open review findings to waive`.
- `waive_reason_must_be_one_short_line`: `--waive ""` and a 201-character reason are refused.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -j4 -p factory-core approve_refusal next_offers_waive without_review_is_refused` and `cargo test -j4 -p factory-cli --test new_to_merged approve_refuses_open approve_with_waive waive_reason`
Expected: FAIL.

- [ ] **Step 3: Implement the rule, the flag and the `next_for` change**

Callers of `next_for` pass `job.changes_open()`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-cli`
Expected: PASS, including `approve_refuses_a_needs_human_job_whose_review_never_ran` and `rework_notes_are_one_short_line`.

---

### Task 2: Waiver records and tickets

**Files:**
- Create: `crates/factory-engine/src/waivers.rs`
- Modify: `crates/factory-engine/src/lib.rs` (module and re-exports), `crates/factory-engine/src/merge.rs`
- Modify: `crates/factory-cli/src/commands.rs` (`approve` records the waiver; `tickets` also runs the waiver step)
- Test: `crates/factory-engine/tests/waivers.rs` (new), `crates/factory-cli/tests/operator_events.rs`, `crates/factory-cli/tests/new_to_merged.rs`

**Interfaces:**
- Consumes: `Tracker::create(&self, title: &str, body: &str, labels: &[String]) -> Result<u32, String>`, `TicketsReport`, `tickets::notify_failure`, `factory_core::open_blockers`, `factory_core::write_atomic`.
- Produces:
  - `pub const WAIVERS_FILE: &str = "waivers.json";`
  - `#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)] pub struct Waiver { pub head: String, pub round: u32, pub file: String, pub reason: String, pub at: u64, pub blockers: Vec<String>, pub tickets: Vec<Option<u32>> }`.
  - `pub fn waived_blockers(review_text: &str) -> Vec<String>`: `open_blockers(text)`, or the whole trimmed reply as one entry when that is empty.
  - `pub fn record_waiver(job_dir: &Path, waiver: Waiver) -> Result<(), EngineError>`: reads the list (missing file is empty), appends, writes atomically.
  - `pub fn read_waivers(job_dir: &Path) -> Vec<Waiver>`.
  - `pub fn waiver_tickets_step(driver: &Driver, id: &str, dry_run: bool) -> Result<TicketsReport, EngineError>`: skips unless the job is `merged`; for every `None` ticket creates an issue titled `Waived review finding from job {id}: {first 80 characters of the blocker}` with a body holding the blocker, the reason, the job id and the merge sha, no labels, stores the number, writes the file after each success, and marks the report failed on any error. `dry_run` lists what it would file.
  - In `approve` (CLI), after the stage moves to Approve and before `merge_stage`: when `waive` is set, `record_waiver` with `head = job.head`, `round` and `file` from `last_review`, `blockers = waived_blockers(<review file text>)`, `tickets` all `None`; the ledger line `approved by the user, waived {B} blockers: {reason}`; the event `operator.approve` with `waived = true` and `waive_reason`.
  - `merge_stage` runs `waiver_tickets_step(driver, id, false)` after the tickets step, unconditionally, with the same failure handling (`step = "waivers"`).
  - `factory tickets <id>` prints the tickets report and then the waiver report.

- [ ] **Step 1: Write the failing tests**

In `tests/waivers.rs` (with `FakeTracker` and `MergeFixture`/`approved_job` helpers):
- `a_waiver_files_one_ticket_per_blocker_after_merge`: two blockers give two `create` calls with the title prefix `Waived review finding from job`, and the stored tickets are `[Some(200), Some(201)]`.
- `untagged_changes_waive_one_blocker`: `waived_blockers("The helper is wrong.\nVERDICT: CHANGES")` has one entry.
- `a_second_waiver_is_appended`: two `record_waiver` calls leave two entries; the step files tickets for both.
- `waiver_tickets_are_idempotent`: a second run makes no `create` call.
- `a_tracker_failure_keeps_the_merge`: `fail_on("create")` leaves the job `merged`, the report failed, the tickets `None`.
- `waiver_tickets_retry_through_factory_tickets`: after that failure, `clear_failures()` and `waiver_tickets_step` files them.

In `operator_events.rs`: `approve_with_waive_logs_the_reason`: the `operator.approve` event has `waived == true` and the reason.

In `new_to_merged.rs`: `approve_with_waive_merges_and_records_the_waiver`: the job merges, `waivers.json` has one entry with the reason, `progress.md` has `waived 1 blockers`.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -j4 -p factory-engine --test waivers` and `cargo test -j4 -p factory-cli approve_with_waive`
Expected: FAIL.

- [ ] **Step 3: Implement `waivers.rs`, the approve record and the merge step**

- [ ] **Step 4: Run the tests**

Run: `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`
Expected: PASS.

---

### Task 3: Supervisor API and dashboard

**Files:**
- Modify: `crates/factory-supervisor/src/api/write.rs`, `crates/factory-supervisor/src/request.rs`, `crates/factory-supervisor/src/spawn.rs`
- Modify: `dashboard/src/model.rs`, `dashboard/src/park.rs`
- Modify: `dashboard/static/board.js`, `dashboard/static/app.js`, `dashboard/static/projects.js`
- Test: `crates/factory-supervisor/src/api/write.rs` tests, `request.rs` tests, `spawn.rs` tests, `dashboard/tests/board.test.js`, `dashboard/tests/projects.test.js`

**Interfaces:**
- Consumes: `Job::approve_refusal` (Task 1).
- Produces:
  - `ApproveBody { without_review: Option<bool>, waive_reason: Option<String> }` (still `deny_unknown_fields`). `approve_inner` checks `one_line(reason, "waive reason")`, then answers 409 `conflict` with `job.approve_refusal(without_review, waive_reason.is_some())` when it is `Some`.
  - `Request.waive_reason: Option<String>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`), `pub fn waive(self, reason: &str) -> Result<Request, RequestError>`, and validation: approve only, not with `without_review`. `enqueue` gains `waive_reason: Option<&str>`.
  - `args_of` pushes `--waive=<reason>` for an approve with a reason (the `=` form, like cancel's `--reason=`, so a reason starting with `-` is safe).
  - Dashboard `model::Job.last_review: Option<LastReviewView>` (lenient, `{round, verdict, blockers, file}`), `ParkView.verdict: Option<String>`, `ParkView.risky: Option<bool>`, all `#[serde(default)]`.
  - `board.js`: `openChanges(job)` is true when `job.last_review && job.last_review.verdict === "changes"`. `approveCommand(job)` returns `factory approve {id} --waive "<reason>"` when `openChanges(job)`, else today's result. A needs-human card with open changes shows `The review asked for changes ({B} blockers).`. `MAX_ROUNDS` goes: `roundPips(round)` draws one pip per round used (at least 2 drawn, at most 6) and the label is `round N`.
  - `projects.js`: `buildWaiveBody(reason)` returns `{waive_reason}` or `{error}` with the same checks as `buildReworkBody`; exported.
  - `app.js`: approve on a job with open changes opens the existing action dialog in a `waive` mode (label "Why merge with open blockers?") and posts `buildWaiveBody`; a 409 from a plain approve shows its message.

- [ ] **Step 1: Write the failing tests**

Supervisor: replace the `changes` half of `approve_of_a_job_in_stage_approve_or_with_a_verdict_does_not_need_the_flag` with `approve_with_open_changes_needs_a_waive_reason`: a board job with `last_review` `changes` answers 409 for `{}` and for `{"without_review":true}`, 202 for `{"waive_reason":"known flake"}` with `req.waive_reason == Some("known flake")`, 400 for a two-line reason. `old_request_without_waive_reason_parses` in `request.rs`. `approve_args_carry_the_waive_reason` in `spawn.rs`: `["--project", p, "approve", "1", "--waive=known flake"]`.

Dashboard: update `a needs-human card with a review verdict keeps the plain approve and no warning` to cover `pass` only; add `a needs-human card with open changes offers approve --waive` (data-copy `factory approve 003 --waive "<reason>"` and the blockers sentence); `round pips follow the round` (round 3 draws three pips and says `round 3`); `buildWaiveBody refuses empty and multi-line reasons` in `projects.test.js`. Rust: `park_view_reads_verdict_and_risky` and an old `park.json` still parses.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -j4 -p factory-supervisor`, `cargo test -j4 --manifest-path dashboard/Cargo.toml`, `node --test dashboard/tests/*.test.js`
Expected: FAIL in the new tests.

- [ ] **Step 3: Implement the API, the request field, the spawn args and the page changes**

- [ ] **Step 4: Update the docs**

`factory-user-guide.md` and `factory-admin-manual.md`: `approve --waive "<reason>"`, when it is needed, and what it records. `global-command.md`: where it lists approve, add the waive form. Run `cargo test -j4 -p factory-cli --test agent_lint`, because it pins that file's text.

- [ ] **Step 5: Run every touched suite and clippy**

Run: `cargo test -j4 -p factory-supervisor`, `cargo test -j4 -p factory-cli`, `cargo test -j4 --manifest-path dashboard/Cargo.toml`, `node --test dashboard/tests/*.test.js`, then `cargo clippy -j4 -p factory-core -p factory-engine -p factory-cli -p factory-supervisor --all-targets -- -D warnings`
Expected: all pass, no warnings.

Next plan: docs/superpowers/plans/2026-10-29-act-on-review-findings/plan-03-rework-and-history.md
