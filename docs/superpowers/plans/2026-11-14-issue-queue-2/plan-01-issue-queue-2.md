# Issue queue, part 1: fixes after the review of the first plan Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-12-issue-queue-design.md
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Fix the three blockers and the worthwhile minor findings an independent review found in the merged intake settings, tracker calls and `pick_next`.

**Architecture:** The same three places change: `GhTracker` and its parsers in `factory-engine/src/tracker.rs`, `pick_next` in `factory-engine/src/intake.rs`, and the validation in `factory-core/src/config.rs`. The `Tracker` port gains no method; `labelled_by` changes its return type and `IssueInfo` gains a field.

**Tech Stack:** Rust, `serde_json`, the `gh` CLI behind the `Tracker` port.

## Global Constraints

- Commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`. Never `--workspace` or `--all`.
- No `Cargo.toml` change, no `git` shell-outs (`gh` only behind the `Tracker` port), no `/proc` reads. No board, ledger or events field is added.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call. Comments are short full sentences.

## Review Focus

1. A last `labeled` event with no login gives no actor, even when an earlier event had one. Pinned by `parse_actor_gives_none_when_the_last_event_has_no_login`.
2. An issue touched after the trusted label is skipped, and an untouched one is picked. Pinned by `pick_next_skips_an_issue_edited_after_labelling`.
3. A `gh` call with more than 64 KiB of output returns. Pinned by `a_gh_call_that_prints_more_than_the_pipe_buffer_still_returns`.
4. An empty `trusted` list and an erroring lookup pick nothing. Pinned by `pick_next_returns_none_for_an_empty_trusted_list` and `pick_next_skips_an_issue_whose_lookup_fails`.
5. A state label with a different case is still a state label. Pinned by `pick_next_compares_labels_without_regard_to_case`.

## File Structure

| File | Responsibility |
|---|---|
| `crates/factory-engine/src/tracker.rs` | `LabelEvent`, `IssueInfo.updated_at`, argument builders, parsers, pipe reading, label creation |
| `crates/factory-engine/src/intake.rs` | `pick_next` with the lookup result type, the edit gate and the case rules |
| `crates/factory-engine/tests/common/mod.rs`, `crates/factory-cli/tests/tickets.rs` | The fakes follow the new `labelled_by` type |
| `crates/factory-core/src/config.rs` | The extra validation of the intake settings |

### Task 1: Tracker fixes

**Files:**
- Modify: `crates/factory-engine/src/tracker.rs`, `crates/factory-engine/src/lib.rs`, `crates/factory-engine/tests/common/mod.rs`, `crates/factory-cli/tests/tickets.rs`
- Test: unit tests in `crates/factory-engine/src/tracker.rs`

**Interfaces:**
- Consumes: `parse_actor`, `events_args`, `list_labelled_args`, `label_create_args`, `edit_labels_args`, `GhTracker::run` and `GhTracker::edit_labels` as the merged code has them; the `Tracker` port stays the place all `gh` calls sit behind.
- Produces: `pub struct LabelEvent { pub actor: String, pub at: String }` (derive `Debug, Clone, PartialEq`), exported from the crate root; `Tracker::labelled_by(&self, issue: u32, label: &str) -> Result<Option<LabelEvent>, String>`; `IssueInfo.updated_at: String`. The next task uses them.

- [ ] **Step 1: Read `crates/factory-engine/src/tracker.rs` end to end (the four new calls, `parse_actor`, `GhTracker::run`) and how `FakeTracker` and `NoCalls` implement `labelled_by`.**
- [ ] **Step 2: Write the failing tests.**
  - `parse_actor_gives_none_when_the_last_event_has_no_login`: events `[{"event":"labeled","label":{"name":"factory"},"actor":{"login":"alice"},"created_at":"2026-11-01T10:00:00Z"},{"event":"labeled","label":{"name":"factory"},"actor":null,"created_at":"2026-11-02T10:00:00Z"}]`; the result is `None`.
  - `parse_actor_keeps_the_time_of_the_last_matching_event`: the same with a login on the second event (`bob`); the result is `LabelEvent { actor: "bob", at: "2026-11-02T10:00:00Z" }`, and an `unlabeled` event after it does not change it.
  - `edit_labels_with_nothing_to_change_makes_no_call`: with a fake `gh` program (a script that records its argv in a file, as the neighbouring `GhTracker::with_program` tests do) both lists empty leave the record file absent.
  - `label_create_args_set_a_fixed_colour_and_description`: the args are `label create <name> --color ededed --description "factory intake state"` with no `--force`.
  - `list_labelled_args_ask_oldest_first_with_updated_at`: the args hold `--limit 500`, `--search sort:created-asc` and a `--json` list ending `createdAt,updatedAt`.
  - `parse_issue_list_reads_updated_at`: the field is read, and a missing one is the empty string.
  - `a_gh_call_that_prints_more_than_the_pipe_buffer_still_returns`: a fake `gh` script that prints 200 KiB (`head -c 204800 /dev/zero | tr '\\0' 'a'`) and exits 0; `GhTracker::run` returns all 204800 bytes in well under the 60 second timeout.
- [ ] **Step 3: Run `cargo test -j4 -p factory-engine --lib tracker`.** Expected: compile errors for `LabelEvent` and `updated_at`, then failing assertions.
- [ ] **Step 4: Implement.** `parse_actor` takes the actor and time of every matching event, so a null actor on the last event gives `None`; the events call uses `--jq` to print only `labeled` events as compact JSON lines (`parse_actor` reads that shape, and keeps accepting the full event shape the tests use). `GhTracker::run` reads stdout and stderr in two threads while it waits. `edit_labels` returns at once when both lists are empty, and creates a missing label with the fixed colour and description, ignoring the "already exists" error. `labelled_by` returns `LabelEvent`. Update the two fakes and every caller.
- [ ] **Step 5: Run `cargo test -j4 -p factory-engine --lib tracker`, then `cargo check -j4 -p factory-engine -p factory-cli --tests` (one `-p` per call).** Expected: tests pass, checks clean.

### Task 2: Picking and validation fixes

**Files:**
- Modify: `crates/factory-engine/src/intake.rs`, `crates/factory-core/src/config.rs`
- Test: unit tests in `crates/factory-engine/src/intake.rs` and `crates/factory-core/src/config.rs`

**Interfaces:**
- Consumes: `LabelEvent`, `IssueInfo.updated_at` from the previous task and the existing `pick_next` tests.
- Produces: `pub fn pick_next(issues: &[IssueInfo], cfg: &Config, labelled_by: &dyn Fn(u32) -> Result<Option<LabelEvent>, String>, trusted: &[String]) -> Option<IssueInfo>`.

- [ ] **Step 1: Read `crates/factory-engine/src/intake.rs` with its tests, and the intake validation and `bad_intake_settings_are_config_errors` in `crates/factory-core/src/config.rs`.**
- [ ] **Step 2: Write the failing tests.**
  - `pick_next_skips_an_issue_edited_after_labelling`: an issue with `updated_at` later than the label event's `at` is not picked; the same issue with `updated_at` equal to `at` is picked.
  - `pick_next_skips_a_null_actor` (a lookup returning `Ok(None)`), `pick_next_skips_an_issue_whose_lookup_fails` (`Err`), `pick_next_returns_none_for_an_empty_trusted_list`.
  - `pick_next_ranks_a_missing_date_last`: two issues of equal priority, one with an empty `created_at`; the dated one wins.
  - `pick_next_compares_labels_without_regard_to_case`: an issue labelled `Factory: Running` is skipped; a trusted login written `Alice` matches `alice`.
  - `pick_next_stops_asking_after_the_first_eligible_issue`: a counting lookup; with the best-ranked issue eligible it is called once.
  - In `config.rs`, extend `bad_intake_settings_are_config_errors` with `expect_err` for `intake_trusted = [""]`, `intake_trusted = ["  "]`, `intake_label = "-x"`, `intake_label = " factory"`, `intake_label = "factory: running"` and `priority_labels = [""]`.
- [ ] **Step 3: Run `cargo test -j4 -p factory-engine --lib intake` and `cargo test -j4 -p factory-core intake`.** Expected: the new tests fail.
- [ ] **Step 4: Implement.** Sort the candidates first, then walk them and return the first eligible one, with the lookup returning a `Result`; apply the edit rule `updated_at <= at`; compare state labels and logins with `eq_ignore_ascii_case`; give an empty `created_at` the last rank; add the validation rules to `Config::parse` with the existing error style.
- [ ] **Step 5: Run `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-core`, then `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings` and the same for `factory-core`.** Expected: all pass, no warnings.

Next plan: docs/superpowers/plans/2026-11-14-issue-queue-2/plan-02-issue-queue-2.md
