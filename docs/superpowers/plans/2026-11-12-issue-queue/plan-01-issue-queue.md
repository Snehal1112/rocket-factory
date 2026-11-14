# Issue queue, part 1: settings, tracker and picking Implementation Plan

> **Spec:** docs/superpowers/specs/2026-11-12-issue-queue-design.md
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Add the intake settings, the four new `Tracker` calls and the pure function that picks the next labelled issue.

**Architecture:** Settings live in `factory-core` `Config` with the validation of the other settings. The `Tracker` port in `factory-engine` gains list, label-event, label-edit and current-user calls, implemented by `GhTracker` through `gh` and faked in tests. `pick_next` is a pure function over `IssueInfo` values, so nothing here touches the network. The tracker and the later agent runs stay behind the existing `Tracker` and `AgentRunner` ports.

**Tech Stack:** Rust, `serde`, `serde_json`, the `gh` CLI behind the port.

## Global Constraints

- Commands are per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`. Never `--workspace` or `--all`.
- No `Cargo.toml` change, no `git` shell-outs. `gh` is allowed behind the `Tracker` port only. Never read `/proc`.
- No board, ledger or events field is added. A bad intake setting is a config error, so the config fails closed.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call. Comments are short full sentences.

## Review Focus

1. A comma or an empty `intake_label` is refused. Pinned by `bad_intake_settings_are_config_errors`.
2. An issue with no priority label sorts after every labelled one. Pinned by `pick_next_sorts_unprioritised_issues_last`.
3. An issue whose label was set by an untrusted user is skipped. Pinned by `pick_next_skips_untrusted_labellers`.
4. An issue that already carries a `factory:` state label is never picked again. Pinned by `pick_next_skips_state_labelled_issues`.
5. A missing label event gives no actor, so the issue is skipped. Pinned by `pick_next_skips_an_issue_with_no_label_event`.

## File Structure

| File | Responsibility |
|---|---|
| `crates/factory-core/src/config.rs`, `settings_block.rs` | The seven settings, defaults, validation, commented block |
| `crates/factory-core/src/board.rs` | `Job.intake_issue`, an optional field |
| `docs/reference/project-toml-reference.md` | The settings table rows |
| `crates/factory-engine/src/tracker.rs` | `IssueInfo`, four trait methods, `Arc` forwarding, `GhTracker`, argument builders and parsers |
| `crates/factory-engine/src/intake.rs`, `lib.rs` | `pick_next` and the state label constants |
| `crates/factory-engine/tests/common/mod.rs` | `FakeTracker` support for the new calls |
| `crates/factory-cli/tests/tickets.rs` | The `NoCalls` tracker gets the new methods |

### Task 1: The intake settings and the job marker

**Files:**
- Modify: `crates/factory-core/src/config.rs`, `crates/factory-core/src/settings_block.rs`, `crates/factory-core/src/board.rs`, `docs/reference/project-toml-reference.md`
- Test: unit tests in `crates/factory-core/src/config.rs`

**Interfaces:**
- Consumes: the way `minor_tickets` and `kb_budget_weighted` are declared, defaulted and validated in `Config`.
- Produces: `Config` fields `intake: bool` (false), `intake_label: String` ("factory"), `priority_labels: Vec<String>` (`priority: P0`, `priority: P1`, `priority: P2`), `intake_poll_minutes: u32` (15), `intake_trusted: Vec<String>` (empty), `intake_model: String` ("claude-opus-5-5"), `intake_budget_weighted: u64` (400000). `Job.intake_issue: Option<u32>` with `#[serde(default, skip_serializing_if = "Option::is_none")]` and `None` in `Job::new`; this is an optional board field. The next tasks and the later plans read them.

- [ ] **Step 1: Read `Config` and `Config::parse` validation in `crates/factory-core/src/config.rs` (the tests `minor_tickets_is_on_by_default_and_can_be_turned_off` and the kb budget test are the pattern), the `GROUPS` table in `settings_block.rs` and its tests, and the `minor_tickets` row of `docs/reference/project-toml-reference.md`.**
- [ ] **Step 2: Write the failing tests.**
  - `intake_settings_have_the_documented_defaults`: `Config::default()` has the seven defaults above.
  - `bad_intake_settings_are_config_errors`: `Config::parse` with `expect_err` for `intake_label = ""`, `intake_label = "a,b"`, `priority_labels = ["x", "x"]`, `intake_poll_minutes = 0`, `intake_budget_weighted = 0`, `intake_model = ""`.
  - `job_intake_issue_is_optional_and_absent_from_old_boards`: a board JSON written without the field loads with `intake_issue == None`, and a job with `None` serialises with no `intake_issue` key.
  - `intake_settings_parse_from_toml`: a config with `intake = true`, `intake_label = "bot"`, `priority_labels = ["p1", "p2"]`, `intake_trusted = ["alice"]` round-trips.
- [ ] **Step 3: Run `cargo test -j4 -p factory-core intake`.** Expected: `no field intake on type Config`.
- [ ] **Step 4: Implement the fields with the doc comments of the table in the spec, the defaults, the checks in `Config::parse`, one new group "Issue intake." in `settings_block.rs` listing the seven keys (commented, no example text), the optional `Job.intake_issue` field, and the table rows in `project-toml-reference.md`.** The `settings_block` tests that compare keys with `Config` must pass.
- [ ] **Step 5: Run `cargo test -j4 -p factory-core`.** Expected: all pass.

### Task 2: Tracker calls and their fakes

**Files:**
- Modify: `crates/factory-engine/src/tracker.rs`, `crates/factory-engine/tests/common/mod.rs`, `crates/factory-cli/tests/tickets.rs`
- Test: unit tests in `crates/factory-engine/src/tracker.rs`

**Interfaces:**
- Consumes: `view_args`, `create_args`, `parse_view` and `GhTracker::run` as the pattern.
- Produces: `pub struct IssueInfo { pub number: u32, pub title: String, pub body: String, pub labels: Vec<String>, pub created_at: String }` (derive `Debug, Clone, PartialEq`); on `Tracker`: `fn list_labelled(&self, label: &str) -> Result<Vec<IssueInfo>, String>`, `fn labelled_by(&self, issue: u32, label: &str) -> Result<Option<String>, String>`, `fn edit_labels(&self, issue: u32, add: &[String], remove: &[String]) -> Result<(), String>`, `fn current_user(&self) -> Result<String, String>`. Forwarded by the `Arc<T>` impl. `FakeTracker` records calls named `list_labelled`, `labelled_by`, `edit_labels`, `current_user` and gets builders `with_issues(Vec<IssueInfo>)`, `with_label_actor(issue, label, login)` and `with_user(login)`. Plan 2 uses them.

- [ ] **Step 1: Read `crates/factory-engine/src/tracker.rs` end to end, `FakeState`, `FakeTracker` and its `impl Tracker` in `crates/factory-engine/tests/common/mod.rs`, and `NoCalls` in `crates/factory-cli/tests/tickets.rs`.**
- [ ] **Step 2: Write the failing tests** in `tracker.rs`: `list_labelled_args` (equals `issue list --state open --label factory --limit 100 --json number,title,body,labels,createdAt`), `edit_labels_args` (`issue edit 7 --add-label a --add-label b --remove-label c`), `events_args` (`api repos/{owner}/{repo}/issues/7/events --paginate`, built from a given `owner/repo`), `label_create_args` (`label create <name> --force`), `parse_issue_list_reads_labels_and_dates` (JSON with two issues, one with no body; `labels` come from `[{"name":..}]`), `parse_actor_returns_the_last_labeled_event_for_the_label` (events for two labels and two actors; the later event of the asked label wins), `parse_actor_is_none_without_a_matching_event`.
- [ ] **Step 3: Run `cargo test -j4 -p factory-engine --lib tracker`.** Expected: `cannot find function list_labelled_args in this scope`.
- [ ] **Step 4: Implement** the argument builders and parsers, the four trait methods with the `Arc` forwarding, and `GhTracker`'s versions: `edit_labels` first runs `label create <name> --force` for every label to add; `labelled_by` reads the repo from `repo_name()`; `current_user` runs `api user --jq .login`. Add the new methods to `FakeTracker` (state fields for the issues, the actors, the user and the label edits; every call goes through `enter` so `fail_on` works) and to `NoCalls` (`Err("not used")`).
- [ ] **Step 5: Run `cargo test -j4 -p factory-engine --lib tracker`, then `cargo check -j4 -p factory-engine` and `cargo check -j4 -p factory-cli --tests`.** Expected: tests pass, both checks clean.

### Task 3: Picking the next issue

**Files:**
- Create: `crates/factory-engine/src/intake.rs`
- Modify: `crates/factory-engine/src/lib.rs`
- Test: unit tests in `crates/factory-engine/src/intake.rs`

**Interfaces:**
- Consumes: `IssueInfo` and `Config` from the earlier tasks.
- Produces: `pub const STATE_RUNNING: &str = "factory: running"`, `STATE_NEEDS_HUMAN = "factory: needs-human"`, `STATE_DONE = "factory: done"`; `pub fn pick_next(issues: &[IssueInfo], cfg: &Config, labelled_by: &dyn Fn(u32) -> Option<String>, trusted: &[String]) -> Option<IssueInfo>`; both exported from the crate root.

- [ ] **Step 1: Read `crates/factory-engine/src/lib.rs` for the export style and the `IssueInfo` type just added.**
- [ ] **Step 2: Write the failing tests** with a helper `issue(n, labels, created)`:
  - `pick_next_prefers_the_higher_priority_then_the_older_issue`: issues 3 (`factory`, `priority: P1`, older), 4 (`factory`, `priority: P0`, newer), 5 (`factory`, `priority: P1`, newer); the pick is 4, and with 4 removed it is 3.
  - `pick_next_sorts_unprioritised_issues_last`: an older issue with only `factory` loses to a newer one with `priority: P2`.
  - `pick_next_skips_state_labelled_issues`: an issue with `factory` and `factory: running` is never picked, nor `factory: needs-human` or `factory: done`.
  - `pick_next_skips_untrusted_labellers`: `labelled_by` returns `mallory` for issue 3 and `alice` for 4 with `trusted = ["alice"]`; the pick is 4.
  - `pick_next_skips_an_issue_with_no_label_event`: `labelled_by` returns `None`; the result is `None`.
  - `pick_next_ties_break_by_issue_number`: same priority and same `created_at`; the lower number wins.
  - `pick_next_ignores_issues_without_the_intake_label`.
- [ ] **Step 3: Run `cargo test -j4 -p factory-engine --lib intake`.** Expected: `cannot find function pick_next in this scope`.
- [ ] **Step 4: Implement `pick_next` and the constants.** Eligible means the issue has `cfg.intake_label`, none of the three state labels, and `labelled_by(number)` is in `trusted`. Sort by the index of the first `cfg.priority_labels` entry the issue has (none sorts after all), then `created_at`, then `number`.
- [ ] **Step 5: Run `cargo test -j4 -p factory-engine --lib intake` then `cargo clippy -j4 -p factory-core -p factory-engine`.** Expected: 7 passed, no warnings.

Next plan: docs/superpowers/plans/2026-11-12-issue-queue/plan-02-issue-queue.md
