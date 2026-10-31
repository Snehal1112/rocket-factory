# Spec review budget cap and failure handling Implementation Plan

> **Spec:** docs/superpowers/specs/2026-10-31-spec-review-budget-cap-design.md
>
> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Give the spec reviewer its own cap and make `factory new` report a failed review, an unsaved review, an approved review, an accepted risky spec and the spec review cost honestly.

**Architecture:** `factory-core` gets one setting. `factory-engine` (`spec_check.rs`, `compat.rs`, `runloop.rs`) refuses a review run that did not finish, keeps the last review in a private `Driver` field and exposes it through `last_spec_review`. `factory-cli` prints one note after `created job` and moves the `spec-review` cost record to a summary line.

**Tech Stack:** Rust, `cargo test -j4 -p <crate>`, the scripted runner in `crates/factory-engine/tests/common/mod.rs`.

## Global Constraints

- Always `-j4` and `-p <crate>`, never `--workspace` or `--all`, never set `CARGO_TARGET_DIR`.
- Each Rust task ends with `cargo fmt -p <crate> -- --check` and `cargo clippy -j4 -p <crate> --all-targets -- -D warnings` for every crate it changed. No `#[allow(...)]` to silence a lint.
- No unwrap in production Rust. Tests use `.expect("...")` and `expect_err`.
- No new board, ledger or events field. The `created job` line stays first, because `factoryd` parses it.
- New public items carry a doc comment. Code comments are short full sentences that end with a period.
- The new setting is `spec_review_budget_weighted`, default `100_000`, refused at 0.
- The review refusal text is `the spec review did not finish ({reason}). {SPEC_OVERRIDE_HINT}`.
- The note text is `note: spec review {verdict}, saved at {path}`, or `note: spec review {verdict}, not saved ({path})`.
- The cost line is `SPEC REVIEWS  {runs} runs, {weighted} weighted (no job)`, with ` ~` after the figure when approximate.

## Review Focus

1. A run that stops at the cap but whose partial text ends in `VERDICT: APPROVE` must refuse, never approve. Pinned by `a_budget_stop_with_an_approve_line_still_refuses`.
2. A one-job series validates twice. With `--accept-spec`, an unwritable review folder and a failed review run must each still run the agent once. Pinned by `an_unwritable_review_folder_still_reviews_once` and `a_failed_review_with_accept_spec_in_one_job_runs_once`.
3. `factory new --one-job` prints the review note exactly once. Pinned by `new_prints_the_spec_review_note_once_after_created`.
4. A `spec_review_budget_weighted = 0` in a project file is refused and the message names the key. Pinned by `a_zero_spec_review_budget_is_refused`.
5. A spec review record whose figure is approximate shows ` ~`, and a table with only the spec review record prints `no jobs` first. Pinned by `an_approximate_spec_review_gets_a_tilde` and `only_spec_reviews_print_no_jobs_and_the_line`.

## File Structure

| File | Responsibility |
|---|---|
| `crates/factory-core/src/config.rs` | the `spec_review_budget_weighted` field, default, validation and tests |
| `crates/factory-core/src/settings_block.rs` | lists the new key beside `spec_review` |
| `crates/factory-engine/src/specs.rs` | the reviewer spec uses the new cap |
| `crates/factory-engine/src/spec_check.rs` | `ReviewFailed`, `incomplete_reason`, `unsaved_warning`, record handling |
| `crates/factory-engine/src/compat.rs` | `ReviewRecord`, the `last_review` field, `last_spec_review` |
| `crates/factory-engine/src/lib.rs` | exports `SpecReviewInfo` |
| `crates/factory-engine/src/runloop.rs` | the `(no agent review ran)` suffix |
| `crates/factory-engine/tests/spec_review.rs` | integration tests |
| `crates/factory-cli/src/commands.rs` | the review note after `created job` |
| `crates/factory-cli/src/costs.rs` | the `SPEC REVIEWS` line and its tests |
| `crates/factory-cli/tests/draft_cards.rs` | the note test |
| `docs/reference/project-toml-reference.md`, `docs/reference/software-factory.md` | one table row and the failure and note behaviour |

### Task 1: The `spec_review_budget_weighted` setting

**Files:**
- Modify: `crates/factory-core/src/config.rs`, `crates/factory-core/src/settings_block.rs`, `crates/factory-engine/src/specs.rs`, `docs/reference/project-toml-reference.md`
- Test: `crates/factory-core/src/config.rs` (tests module), `crates/factory-engine/tests/spec_review.rs`

**Interfaces:**
- Produces: `Config.spec_review_budget_weighted: u64` (default `100_000`). `spec_reviewer_agent` sets `agent.budget_weighted = Some(config.spec_review_budget_weighted)`.
- Consumes: the existing `fixture`, `reviewer`, `new` and `APPROVE` in `tests/spec_review.rs`.

- [ ] **Step 1: Read the code this task touches**

Read `Config` and `Config::default` (`config.rs:100-125,192-222`), the zero checks (`config.rs:328-337`), the group that holds `spec_review` in `settings_block.rs:80-90`, `spec_reviewer_agent` (`specs.rs:680-717`) and the `spec_review` row in `project-toml-reference.md`.

- [ ] **Step 2: Write the failing tests**

In the `config.rs` tests module:

```rust
#[test]
fn spec_review_budget_defaults_to_100000() {
    assert_eq!(Config::default().spec_review_budget_weighted, 100_000);
    let cfg = Config::parse("spec_review = true").expect("valid");
    assert_eq!(cfg.spec_review_budget_weighted, 100_000);
}

#[test]
fn spec_review_budget_key_overrides_the_default() {
    let cfg = Config::parse("spec_review_budget_weighted = 150000").expect("valid");
    assert_eq!(cfg.spec_review_budget_weighted, 150_000);
}

#[test]
fn a_zero_spec_review_budget_is_refused() {
    let err = Config::parse("spec_review_budget_weighted = 0").expect_err("refused");
    assert!(err.to_string().contains("spec_review_budget_weighted"), "{err}");
}
```

In `tests/spec_review.rs`:

```rust
#[test]
fn the_reviewer_gets_its_own_budget_cap() {
    let cfg = Config {
        spec_review_budget_weighted: 12_345,
        ..Config::default()
    };
    let fx = fixture(vec![reviewer(APPROVE)], cfg);
    new(&fx, None).expect("created");
    assert_eq!(fx.runner.specs_for("spec-reviewer")[0].budget_weighted, Some(12_345));
    let fx = fixture(vec![reviewer(APPROVE)], Config::default());
    new(&fx, None).expect("created");
    assert_eq!(fx.runner.specs_for("spec-reviewer")[0].budget_weighted, Some(100_000));
}
```

- [ ] **Step 3: Run the tests to see them fail**

Run: `cargo test -j4 -p factory-core spec_review_budget` and `cargo test -j4 -p factory-engine --test spec_review the_reviewer_gets`
Expected: compile errors `no field 'spec_review_budget_weighted' on type 'Config'`.

- [ ] **Step 4: Implement the setting**

Add `pub spec_review_budget_weighted: u64` with a doc comment ("Per-run cap on weighted tokens for one spec review run.") after `spec_review` in `Config`, default `100_000` in `Config::default`. Extend the zero check with the new field and change its message to `commit_budget_weighted, kb_budget_weighted and spec_review_budget_weighted must be above zero`. Add `("spec_review_budget_weighted", None)` after `("spec_review", None)` in `settings_block.rs`. In `spec_reviewer_agent` use the new field for `agent.budget_weighted`. Add a table row after `spec_review` in `project-toml-reference.md`: key, `integer`, `100000`, "Cap on one spec review run. A run that reaches it is refused as not finished. Above 0."

- [ ] **Step 5: Run the tests to see them pass**

Run: `cargo test -j4 -p factory-core` and `cargo test -j4 -p factory-engine --test spec_review`
Expected: all pass, the four new tests included, and `every_config_key_is_listed_once` still passes.

- [ ] **Step 6: Format and lint**

Run `cargo fmt -p factory-core -- --check`, `cargo fmt -p factory-engine -- --check`, `cargo clippy -j4 -p factory-core --all-targets -- -D warnings` and `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`. Fix any finding with `cargo fmt -p <crate>` or by changing the code, then run the same command once to confirm.

### Task 2: Failed runs, the kept review and the note data

**Files:**
- Modify: `crates/factory-engine/src/spec_check.rs`, `crates/factory-engine/src/compat.rs`, `crates/factory-engine/src/lib.rs`, `crates/factory-engine/src/runloop.rs`, `docs/reference/software-factory.md`
- Test: `crates/factory-engine/src/spec_check.rs` (tests module), `crates/factory-engine/tests/spec_review.rs`

**Interfaces:**
- Consumes: `Config.spec_review_budget_weighted` from the setting task, `factory_agent::{AgentRun, StopReason}`.
- Produces: `SpecRefusal::ReviewFailed { reason: String }`; `fn incomplete_reason(run: &AgentRun, cap: u64) -> Option<String>`; `fn unsaved_warning(file: &Path, err: &std::io::Error) -> String`; `pub struct SpecReviewInfo { pub file: PathBuf, pub verdict: &'static str, pub saved: bool }` exported from `lib.rs`; `Driver::last_spec_review(&self) -> Option<SpecReviewInfo>`; `pub(crate) struct ReviewRecord { key: String, file: PathBuf, text: String, saved: bool, failure: Option<String> }` in `compat.rs`, with a `failure: Option<String>` field, held in `last_review: Mutex<Option<ReviewRecord>>` (set to `Mutex::new(None)` in `Driver::new`).

- [ ] **Step 1: Read the code this task touches**

Read all of `spec_check.rs`, `Driver` and `Driver::new` (`compat.rs:64-90,210-245`), `validate_series` and `record_accepted` (`runloop.rs:304-407`), `AgentRun` and `StopReason` (`factory-agent/src/run.rs`), and the fixtures in `tests/spec_review.rs` plus `driver_for` and `ScriptedRunner` in `tests/common/mod.rs:164-211,296-304`. Note that an exhausted script gives `AgentRun::failed("script exhausted")`.

- [ ] **Step 2: Write the failing unit tests**

In the `spec_check.rs` tests module:

```rust
fn stopped(reason: factory_agent::StopReason, error: Option<&str>) -> AgentRun {
    AgentRun {
        stop_reason: reason,
        error: error.map(str::to_string),
        ..AgentRun::done("")
    }
}

#[test]
fn incomplete_reason_names_each_stop() {
    use factory_agent::StopReason::{BudgetStop, Failed, Stalled, TurnLimit};
    assert_eq!(incomplete_reason(&AgentRun::done("x"), 100), None);
    assert_eq!(
        incomplete_reason(&stopped(Failed, Some("boom")), 100).as_deref(),
        Some("failed: boom")
    );
    assert_eq!(
        incomplete_reason(&stopped(Failed, None), 100).as_deref(),
        Some("failed: no error text")
    );
    assert_eq!(incomplete_reason(&stopped(Stalled, None), 100).as_deref(), Some("stalled"));
    assert_eq!(
        incomplete_reason(&stopped(TurnLimit, None), 100).as_deref(),
        Some("stopped at the turn limit")
    );
    assert_eq!(
        incomplete_reason(&stopped(BudgetStop, None), 100).as_deref(),
        Some("stopped at the cap of 100 weighted tokens; raise spec_review_budget_weighted")
    );
}

#[test]
fn the_unsaved_warning_names_the_path_and_error() {
    let err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
    let text = unsaved_warning(Path::new("/s/spec-reviews/k.md"), &err);
    assert_eq!(text, "warning: could not save the spec review to /s/spec-reviews/k.md: denied");
}
```

Add `SpecRefusal::ReviewFailed { reason: "stalled".into() }` to the array in `refusals_end_with_the_override_hint`, and one assertion that its text contains `did not finish (stalled)`.

- [ ] **Step 3: Write the failing integration tests**

In `tests/spec_review.rs`. First split `fixture` so the driver comes from a closure, keeping the `fixture(effects, config)` signature unchanged. Then add:

| Test | Setup | Assertions |
|---|---|---|
| `a_failed_review_run_refuses_and_saves_nothing` | `fixture(vec![], Config::default())` | `new(&fx, None)` fails with `did not finish (failed: script exhausted)` and `--accept-spec`; zero `.md` files in `spec_reviews_dir()` (a missing folder counts as zero); `jobs(&fx) == 0` |
| `a_failed_review_is_run_again_by_a_new_process` | same | `new` fails; build a second driver on the same checkout as `driver_for` does (`fx.driver.root()`) and call `create_job_with_spec_accepting` with it; it fails too; `specs_for("spec-reviewer").len() == 2` |
| `a_failed_review_with_accept_spec_in_one_job_runs_once` | `fixture(vec![], Config::default())` | `create_one_job_accepting` with `Some("checked")` as in `an_unwritable_review_folder_still_reviews_once` creates the job; `specs_for("spec-reviewer").len() == 1` |
| `accept_spec_passes_a_failed_review_and_records_the_reason` | same | `new(&fx, Some("checked by hand"))` creates the job; its `spec_warning` contains `spec accepted by the user: checked by hand` and not `no agent review ran` |
| `last_spec_review_reports_the_file_and_verdict` | `[reviewer(APPROVE)]`, then `[reviewer(CHANGES)]` with `Some("checked")` | verdict `approved` then `asked for changes`; `saved` is true; `file.exists()` |
| `a_reused_review_is_still_reported` | call `new` twice as `an_approved_spec_is_not_reviewed_twice` does | one reviewer call; `last_spec_review()` is `Some` |
| `no_review_means_no_note` | `Config { spec_review: false, ..Config::default() }` | `new` succeeds; `last_spec_review()` is `None` |
| `accept_spec_on_a_risky_spec_says_no_agent_review_ran` | `Config { risk_paths: vec!["a.rs".into()], ..Config::default() }`, overwrite `fx.spec` with `SPEC` where `Decided by: user` becomes `Decided by: call` | `new(&fx, Some("checked"))` creates the job; warning contains `spec accepted by the user: checked (no agent review ran)`; zero reviewer calls; `last_spec_review()` is `None` |

The unwritable folder test, in full:

```rust
#[test]
fn an_unwritable_review_folder_still_reviews_once() {
    let fx = fixture(vec![reviewer(CHANGES)], Config::default());
    let dir = fx.driver.layout().spec_reviews_dir();
    std::fs::create_dir_all(dir.parent().expect("state dir")).expect("state dir");
    std::fs::write(&dir, "not a folder").expect("block the folder");
    let made = factory_engine::create_one_job_accepting(
        &fx.driver, "demo", "f", &fx.plan, Some(&fx.spec), None, false, Some("checked"),
    )
    .expect("created");
    assert_eq!(fx.runner.specs_for("spec-reviewer").len(), 1);
    assert!(made.job.spec_warning.expect("warning").contains("spec accepted by the user"));
    let info = fx.driver.last_spec_review().expect("info");
    assert!(!info.saved);
}
```

The budget stop test uses a local runner, with `AgentRunner` implemented as `ScriptedRunner` does in `tests/common/mod.rs:195`, and a driver built as `driver_for` builds it:

```rust
struct StopRunner;
impl AgentRunner for StopRunner {
    fn run(&self, _spec: &AgentSpec) -> AgentRun {
        AgentRun {
            stop_reason: StopReason::BudgetStop,
            ..AgentRun::done("looks fine\nVERDICT: APPROVE")
        }
    }
}
// a_budget_stop_with_an_approve_line_still_refuses: new(..) fails with
// "did not finish (stopped at the cap of 100000 weighted tokens; raise spec_review_budget_weighted)".
```

- [ ] **Step 4: Run the tests to see them fail**

Run: `cargo test -j4 -p factory-engine spec_check` and `cargo test -j4 -p factory-engine --test spec_review`
Expected: compile errors `cannot find function 'incomplete_reason'`, `no variant named 'ReviewFailed'` and `no method named 'last_spec_review'`.

- [ ] **Step 5: Implement**

In `review_spec_with`, in this order: read the file; when it is empty and `last_review` holds the same key, use that record's text; apply today's reuse rule to that text; otherwise run the agent and log the usage as today. At the top, once the key is known, return `ReviewFailed` with the stored reason when the record has the same key and a `failure`, before reading the file or running the agent. Then call `incomplete_reason(&run, driver.config().spec_review_budget_weighted)`: on `Some(reason)` store a record with `failure: Some(reason.clone())` and `saved = false`, and return `ReviewFailed { reason }` before any save and any verdict parsing. `last_spec_review` returns `None` for a record with a `failure`. Otherwise write the file; on a write error print `unsaved_warning(&file, &err)` to stderr with `eprintln!` and set `saved = false`. Store the `ReviewRecord` after every finished run and every reuse (also when the saved text is read from disk). `last_spec_review` maps the record to `SpecReviewInfo`, with the verdict from `last_line`: `VERDICT: APPROVE` is `approved`, `VERDICT: CHANGES` is `asked for changes`, anything else is `gave no verdict`. Add the `ReviewFailed` display arm. In `validate_series`, when the refusal is `SpecRefusal::NoUserDecision(_)` and a reason is accepted, store `format!("{reason} (no agent review ran)")` in `accepted`. Update the spec review paragraph of `software-factory.md` (line 31) with the failure behaviour, the cap and the note.

- [ ] **Step 6: Run the tests to see them pass**

Run: `cargo test -j4 -p factory-engine` and `cargo test -j4 -p factory-supervisor`
Expected: all pass, the new tests included. The supervisor tests confirm that a refusal ending in the override hint still maps to 409.

- [ ] **Step 7: Format and lint**

Run `cargo fmt -p factory-engine -- --check` and `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`. Fix any finding, then run the same command once to confirm.

### Task 3: The review note and the spec review cost line

**Files:**
- Modify: `crates/factory-cli/src/commands.rs`, `crates/factory-cli/src/costs.rs`
- Test: `crates/factory-cli/src/costs.rs` (tests module), `crates/factory-cli/tests/draft_cards.rs`

**Interfaces:**
- Consumes: `Driver::last_spec_review() -> Option<SpecReviewInfo>` from the engine task, and the `cost(...)` helper in the `costs.rs` tests module.
- Produces: the `note:` line after `created job`, and the `SPEC REVIEWS` line in `render_costs`.

- [ ] **Step 1: Read the code this task touches**

Read the end of the `new` command (`commands.rs:485-535`), `render_costs` and the tests module in `costs.rs`, and `new_prints_the_created_line_first_then_the_warning` in `tests/draft_cards.rs`.

- [ ] **Step 2: Write the failing tests**

In the `costs.rs` tests module:

```rust
fn review_record(approx: bool) -> JobCost {
    let mut r = cost("spec-review", None, "spec-review", 90_000, approx, 3);
    r.lines = None;
    r.files = None;
    r
}

#[test]
fn the_spec_review_record_leaves_the_job_table() {
    let jobs = vec![
        cost("1", Some(SizeGroup::Small), "merged", 300_000, false, 1),
        review_record(false),
    ];
    let text = render_costs(&jobs, &stats(&jobs));
    assert!(!text.lines().any(|l| l.starts_with("spec-review")), "{text}");
    assert!(text.contains("SPEC REVIEWS  3 runs, 90.0K weighted (no job)"), "{text}");
}

#[test]
fn only_spec_reviews_print_no_jobs_and_the_line() {
    let jobs = vec![review_record(false)];
    let text = render_costs(&jobs, &stats(&jobs));
    assert_eq!(text, "no jobs\nSPEC REVIEWS  3 runs, 90.0K weighted (no job)\n");
}

#[test]
fn an_approximate_spec_review_gets_a_tilde() {
    let jobs = vec![review_record(true)];
    let text = render_costs(&jobs, &stats(&jobs));
    assert!(text.contains("SPEC REVIEWS  3 runs, 90.0K ~ weighted (no job)"), "{text}");
}
```

In `tests/draft_cards.rs`, built as `new_prints_the_created_line_first_then_the_warning` does (the fixture runner approves a review by default):

```rust
#[test]
fn new_prints_the_spec_review_note_once_after_created() {
    // Same setup as new_prints_the_created_line_first_then_the_warning.
    let notes: Vec<&str> = out.lines().filter(|l| l.starts_with("note: spec review ")).collect();
    assert_eq!(notes.len(), 1, "{out}");
    assert!(notes[0].starts_with("note: spec review approved, saved at "), "{out}");
    assert!(out.lines().next().expect("first line").starts_with("created job "), "{out}");
}
```

Add a second `--one-job` variant of the same call, asserting the same single note.

- [ ] **Step 3: Run the tests to see them fail**

Run: `cargo test -j4 -p factory-cli costs` and `cargo test -j4 -p factory-cli --test draft_cards`
Expected: the three cost tests fail on the missing line, and the note test fails because the output has no `note: spec review` line.

- [ ] **Step 4: Implement**

In `render_costs`, split the records into real jobs and the one with id `spec-review` (a local constant). The empty check covers real jobs only. Append the `SPEC REVIEWS` line before every return, including the early return for no merged jobs, with `human(weighted)` and ` ~` when `approx`. In the `new` command, after the loop over `notes` and before the warnings, call `d.last_spec_review()` and push the note line in the two forms of the Global Constraints.

- [ ] **Step 5: Run the tests to see them pass**

Run: `cargo test -j4 -p factory-cli`
Expected: all pass, the new tests included, and the existing `factory costs` tests unchanged.

- [ ] **Step 6: Format and lint**

Run `cargo fmt -p factory-cli -- --check` and `cargo clippy -j4 -p factory-cli --all-targets -- -D warnings`. Fix any finding, then run the same command once to confirm.

Next plan: none
