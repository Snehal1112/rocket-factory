# Spec review: budget cap and failure handling

Ticket: https://github.com/Snehal1112/rocket-factory/issues/26

## Goal

Give the spec reviewer its own token cap and make `factory new` handle a failed review run, an unsaved review file, an approved review, an accepted risky spec and the `spec-review` cost row in plain, honest ways.

## Overview

`factory new` runs a read-only spec reviewer on the spec and its plans before any job exists (`crates/factory-engine/src/spec_check.rs:196-237`). Six follow-ups from the act-on-review-findings series (PR #17) are open in ticket #26:

- The reviewer's cap is the whole job budget (`specs.rs:714`, `agent.budget_weighted = Some(config.budget_weighted)`). The job budget is 2,000,000 weighted in this project. Three real reviews used 4 to 7 turns and about 2,500 output tokens each (`state/spec-reviews/usage.jsonl`), so a runaway review runs very long before anything stops it.
- A failed run (runner error, stall, budget stop, turn limit) still saves its text, often an empty line, as a review file (`spec_check.rs:229-236`). The user then reads "no verdict" and cannot tell a failed run from a reviewer that forgot its verdict line.
- A failed write of the review file is ignored (`spec_check.rs:234-235`). The next validation of the same text pays for a second agent run.
- The review path is printed only inside a refusal (`spec_check.rs:38-47`), never after an APPROVE.
- `--accept-spec` on a "no user decision" refusal skips the agent review without saying so (`runloop.rs:375-398`).
- `factory costs` lists the synthetic `spec-review` record in the per-job table (`factory-engine/src/costs.rs:49-72`).

## Current state

| Piece | Today | Reference |
|---|---|---|
| Reviewer cap | `config.budget_weighted` | `specs.rs:714` |
| Other agent caps | `commit_budget_weighted` 60,000 and `kb_budget_weighted` 150,000, both refused at 0 | `config.rs:108,125,333-337` |
| Run result | `AgentRun` has `stop_reason`, `error`, `exit_code` | `factory-agent/src/run.rs:27-40` |
| Review text save | `run.result_text` plus a newline, written with the result ignored | `spec_check.rs:229-236` |
| Saved review reuse | read from the file only, so an unsaved review is never reused | `spec_check.rs:219-223` |
| Refusal variants | `NoUserDecision`, `ReviewChanges`, `ReviewMissing`, `ReviewerMissing` | `spec_check.rs:19-28` |
| Accepted refusal | recorded as `spec accepted by the user: <reason>` | `runloop.rs:304-311` |
| `created job` output | the first line, then `note:` and `warning:` lines | `commands.rs:513-530` |
| Cost table | `render_costs` prints every `JobCost`, spec reviews included | `factory-cli/src/costs.rs:16-44` |
| Driver state | private fields, some behind `Mutex` | `compat.rs:64-90` |

## Non-goals

- No change to the reviewer's agent file, its prompt or its three kinds of finding.
- No new board, ledger or events field. Board JSON stays as it is.
- No dashboard change. The dashboard does not show the `spec-review` record.
- No retry of a failed review run.

## Design

Flow: `factory new` validates the series, which calls `review_spec_with`. The reviewer runs under the new cap. A run that did not finish refuses with its own reason and leaves no file. A finished run is saved, kept in memory and reported with a `note:` line after `created job`.

### 1. The `spec_review_budget_weighted` setting

`Config.spec_review_budget_weighted: u64`, default `100_000`, in `config.rs` next to `spec_review`. `Config::parse` refuses 0 with the commit and KB budgets, so the message becomes `commit_budget_weighted, kb_budget_weighted and spec_review_budget_weighted must be above zero`. `spec_reviewer_agent` sets `agent.budget_weighted = Some(config.spec_review_budget_weighted)`. The probe builds the same spec, so it uses the same cap. `settings_block.rs` lists the key in the group that holds `spec_review`, with no preset default. `docs/reference/project-toml-reference.md` gets one table row.

Example: `spec_review_budget_weighted = 150000` in `[settings]` raises the cap for a project with very large specs.

### 2. A review run that did not finish

New `SpecRefusal::ReviewFailed { reason: String }`. Display: `the spec review did not finish ({reason}). {SPEC_OVERRIDE_HINT}`. A pure function `incomplete_reason(run: &AgentRun, cap: u64) -> Option<String>` in `spec_check.rs` gives the reason:

| `stop_reason` | reason |
|---|---|
| `Done` | `None` (a finished run, even with no verdict, stays `ReviewMissing`) |
| `Failed` | `failed: <error>`, or `failed: no error text` when `error` is `None` |
| `Stalled` | `stalled` |
| `TurnLimit` | `stopped at the turn limit` |
| `BudgetStop` | `stopped at the cap of <cap> weighted tokens; raise spec_review_budget_weighted` |

When the reason is `Some`, `review_spec_with` logs the usage, saves no file, keeps nothing in memory and returns `ReviewFailed`. The next `factory new` runs the reviewer again. `--accept-spec` turns the refusal into a pass like any other, with the user's reason recorded.

### 3. A review file that cannot be saved

`Driver` gets a private field `last_review: Mutex<Option<ReviewRecord>>`, where `ReviewRecord { key: String, file: PathBuf, text: String, saved: bool }`. `review_spec_with` stores the record after every finished run. Before it runs the agent, it reads the file as today; when the file is empty and the record's key equals this key, it uses the record's text. A failed `std::fs::write` sets `saved = false` and prints `warning: could not save the spec review to <path>: <error>` to stderr once. A pure function `unsaved_warning(file: &Path, err: &std::io::Error) -> String` builds the text.

### 4. The review note after `created job`

`pub struct SpecReviewInfo { pub file: PathBuf, pub verdict: &'static str, pub saved: bool }` and `Driver::last_spec_review(&self) -> Option<SpecReviewInfo>`, exported from `lib.rs`. `verdict` is `approved`, `asked for changes` or `gave no verdict`, from the last line of the text. It is set when a review ran or a saved one was reused, so it is `None` with `spec_review = false`, with no spec, and after an accepted "no user decision" refusal. `factory new` appends, after the `created job` line and the other notes, `note: spec review approved, saved at <path>`, or `note: spec review approved, not saved (<path>)` when `saved` is false.

### 5. An accepted "no user decision" refusal

The agent review stays skipped. `validate_series` records the reason as `<reason> (no agent review ran)` when the refusal is `NoUserDecision`, so the recorded warning reads `spec accepted by the user: <reason> (no agent review ran)`. Other accepted refusals are recorded as today.

### 6. The `spec-review` row in `factory costs`

`render_costs` takes the record with id `spec-review` out of the per-job table and prints one line after the tables: `SPEC REVIEWS  <runs> runs, <weighted> weighted (no job)`, with ` ~` after the figure when the record is approximate. With no real job and no spec review it prints `no jobs`. With no real job and a spec review it prints `no jobs` and the line. `costs.json`, the group figures and the trend report are unchanged.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| `spec_review_budget_weighted = 0` | `Config::parse` refuses, naming the key |
| Run stops at the cap with partial text | `ReviewFailed` with the cap and the setting name, no file |
| Run fails with no error text | reason `failed: no error text` |
| Run ends `Done` with an empty text | `ReviewMissing`, as today |
| Failed run with `--accept-spec` | the job is created, the reason is recorded, no review note |
| Review folder is read-only or full | warning on stderr, review kept in memory, one agent run per process |
| Second `factory new` process after an unsaved review | the agent runs again, which cannot be avoided without a file |
| Reused saved review | `last_spec_review` is set, so the note is printed |
| `spec_review = false` | no run, no note |
| Accepted risky spec without a user decision | no run, no note, warning says `(no agent review ran)` |
| Only the `spec-review` record exists | `no jobs`, then the `SPEC REVIEWS` line |

## Testing

`factory-core` (`config.rs` tests): `spec_review_budget_defaults_to_100000`, `spec_review_budget_key_overrides_the_default`, `a_zero_spec_review_budget_is_refused`. In `settings_block.rs` the existing `every_config_key_is_listed_once` must pass with the new key.

`factory-engine` unit tests in `spec_check.rs`: `incomplete_reason_names_each_stop`, `the_unsaved_warning_names_the_path_and_error`, and `ReviewFailed` added to `refusals_end_with_the_override_hint`.

`factory-engine` integration tests in `tests/spec_review.rs`: `the_reviewer_gets_its_own_budget_cap`, `a_failed_review_run_refuses_and_saves_nothing`, `a_failed_review_is_run_again_next_time`, `accept_spec_passes_a_failed_review_and_records_the_reason`, `an_unwritable_review_folder_still_reviews_once`, `last_spec_review_reports_the_file_and_verdict`, `a_reused_review_is_still_reported`, `no_review_means_no_note`, `accept_spec_on_a_risky_spec_says_no_agent_review_ran`.

`factory-cli`: in `costs.rs` tests, `the_spec_review_record_leaves_the_job_table` and `only_spec_reviews_print_no_jobs_and_the_line`; in `tests/e2e.rs` or the existing `new` tests, `new_prints_the_spec_review_note_after_created`.

Commands: `cargo test -j4 -p factory-core`, `cargo test -j4 -p factory-engine`, `cargo test -j4 -p factory-cli`, then `cargo fmt -p <crate> -- --check` and `cargo clippy -j4 -p <crate> --all-targets -- -D warnings` for each of the three crates.

## Files changed

| File | Change |
|---|---|
| `crates/factory-core/src/config.rs` | new field, default, validation, tests |
| `crates/factory-core/src/settings_block.rs` | list the new key |
| `crates/factory-engine/src/specs.rs` | use the new cap |
| `crates/factory-engine/src/spec_check.rs` | `ReviewFailed`, `incomplete_reason`, `unsaved_warning`, record handling |
| `crates/factory-engine/src/compat.rs` | `last_review` field, `last_spec_review` |
| `crates/factory-engine/src/lib.rs` | export `SpecReviewInfo` |
| `crates/factory-engine/src/runloop.rs` | the `(no agent review ran)` suffix |
| `crates/factory-engine/tests/spec_review.rs` | the integration tests above |
| `crates/factory-cli/src/commands.rs` | the review note |
| `crates/factory-cli/src/costs.rs` | the cost line and its tests |
| `docs/reference/project-toml-reference.md` | one row |
| `docs/reference/software-factory.md` | the failure and note behaviour of the spec review |

## Constraints

- Always `-j4`, `-p <crate>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Rust changes pass `cargo fmt -p <crate> -- --check` and `cargo clippy -j4 -p <crate> --all-targets -- -D warnings` for each changed crate. No `#[allow(...)]` to silence a lint.
- No unwrap in production Rust. Tests use `.expect("...")` and `expect_err`.
- No new board, ledger or events field. The `created job` line stays first, because `factoryd` parses it.
- New public items carry a doc comment. Paths reach the driver only through `Layout`.

## Acceptance criteria

- The reviewer's `AgentSpec.budget_weighted` is `spec_review_budget_weighted`, default 100,000.
- `spec_review_budget_weighted = 0` is refused by `Config::parse`, and the key is documented and listed in the settings block.
- A run that is not `Done` refuses with `the spec review did not finish (<reason>)`, saves no review file and is run again on the next `factory new`.
- A budget stop names the cap and `spec_review_budget_weighted`.
- A failed write of the review file prints a warning with the path and the error, and the agent runs once per process.
- After `created job`, `factory new` prints `note: spec review <verdict>, saved at <path>` whenever a review ran or was reused.
- An accepted "no user decision" refusal records `(no agent review ran)`.
- `factory costs` prints no `spec-review` row in the job table and one `SPEC REVIEWS` line instead.
- fmt and clippy `-D warnings` pass for `factory-core`, `factory-engine` and `factory-cli`.

## Out of scope

- Retrying a failed review run.
- A dashboard view of spec reviews.
- A shared cache of reviews across processes beyond the saved file.

## Decisions

- How is the spec reviewer's budget cap set? Options: a new setting `spec_review_budget_weighted` with default 100,000; a fixed constant; a share of `budget_weighted`; leave it. Chosen: the new setting with default 100,000. Decided by: user
- What happens when the review run fails (runner error, stall, budget stop, turn limit)? Options: refuse with a new reason and save no file; refuse and save the partial text; treat it as approval; keep today's behaviour. Chosen: refuse with a new reason that names the stop reason and error, and save no file. Decided by: user
- What happens when the review file cannot be written? Options: keep the review in memory for the process and warn once; warn only; refuse; leave it. Chosen: keep the review in memory for the process and warn once with the path and OS error. Decided by: user
- Should `factory new` print the review path after an APPROVE? Options: a `note:` line whenever a review ran or a saved one was reused; only when the agent ran; nothing; leave it. Chosen: one `note:` line after the `created job` line whenever a review ran or a saved one was reused. Decided by: user
- Should `--accept-spec` on a "no user decision" refusal still run the agent review? Options: keep skipping it and say so in the recorded warning; run it and refuse on CHANGES; run it and never refuse; leave it. Chosen: keep skipping, and add "(no agent review ran)" to the recorded warning. Decided by: user
- What happens to the `spec-review` row in `factory costs`? Options: take it out of the job table and print one summary line; keep the row and mark its columns; remove it from `factory costs`; leave it. Chosen: take it out of the job table and print one `SPEC REVIEWS` summary line. Decided by: user
- Where does the in-memory review live, and how does the CLI learn the path? Options: a private `Mutex` field on `Driver` with a `last_spec_review` accessor; a new field on `Job`; a return value through `Validated`. Chosen: the `Driver` field and accessor, because it adds no board field and no signature change. Decided by: call
