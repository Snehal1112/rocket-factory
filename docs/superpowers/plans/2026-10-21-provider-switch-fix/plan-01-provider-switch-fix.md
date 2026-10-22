# Provider Switch Fix Implementation Plan

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** No saved session id of the old provider reaches the new one after the project switches `agent_cli`, a fix round without a saved session starts a fresh builder that gets the findings, and an unopenable KB repo is noted.

**Architecture:** `Engine::sync_provider` returns whether it switched and runs at the top of `build_stage`, so every spec and write-back sees cleared state. `builder_resume_spec` takes an optional session and builds a fresh-builder prompt for `None`. `run_agent` keeps a defensive backstop. The KB branch of `prepare_for_codex` reports a failed `Repo::open` through `notice`.

**Tech Stack:** Rust, `factory-engine` only. The agent port is `AgentRunner` (`factory-agent/src/runner.rs`); tests use the scripted fake runner from `tests/common/mod.rs`. Git is the concrete `factory_git::Repo`.

**Spec:** docs/superpowers/specs/2026-10-21-provider-switch-fix-design.md

## Global Constraints

- Cargo with `-j4`, per crate (`-p factory-engine`), never `--workspace` or `--all`, never `CARGO_TARGET_DIR`.
- No board, ledger or event field is added (a `Notice` event already exists). Any new board field would be optional; none is planned.
- A job with a saved session on an unchanged provider behaves exactly as before.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call. Comments are short full sentences.
- Before finishing run `cargo fmt -p factory-engine`. The checks do not run clippy: run `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings` once and fix what it reports.

## Review Focus

1. A legacy whole-plan job (`session_id` set, `task_index == 0`) that is switched must take the fix path, not `build_task`. Pinned by `a_switched_legacy_job_takes_the_fix_path_and_not_task_one` (Task 2).
2. A job with a `rework-note.md` but no `head` (no build ever finished) keeps the first-build prompt. Pinned by `a_first_build_still_uses_the_first_build_prompt` (Task 2).
3. A Codex session saved after a switch survives and is resumed; a second `sync_provider` must not clear it. Pinned by `the_provider_switch_clears_once_and_keeps_the_new_session` (Task 2).
4. A Claude project with provider unset or `claude` keeps its crash-resume and fix-round sessions. Pinned by `a_claude_project_keeps_its_saved_sessions` (Task 2).
5. A switch back from Codex to Claude clears Codex ids. Pinned by `a_switch_back_to_claude_clears_a_codex_crash_resume` (Task 2).

## File Structure

- `crates/factory-engine/tests/common/mod.rs`: gains the shared helpers and constants (Task 1).
- `crates/factory-engine/tests/build_checks.rs`: imports them, loses their definitions (Task 1).
- `crates/factory-engine/src/ports.rs`: `sync_provider` return type, `run_agent` backstop (Task 2).
- `crates/factory-engine/src/build.rs`: the sync call and the spec choice in `build_stage` (Task 2).
- `crates/factory-engine/src/specs.rs`: `builder_resume_spec` takes `Option<&str>`; `notice` becomes `pub(crate)` (Tasks 2 and 3).
- `crates/factory-engine/src/codex_prep.rs`: the KB `match` (Task 3).
- `crates/factory-engine/tests/codex_prep.rs`: the new tests; the old weak test is removed (Tasks 2 and 3).

### Task 1: Share the fix-round test helpers

Port: tests only, no production code; the engine still sits behind `AgentRunner` and `Tracker` as before.

**Files:**
- Modify: `crates/factory-engine/tests/common/mod.rs`
- Modify: `crates/factory-engine/tests/build_checks.rs`

**Interfaces:**
- Produces, all `pub` in `tests/common/mod.rs` with the signatures they have today in `build_checks.rs`: `change_effect`, `fix_round_from`, `two_task_fixture`, `start_fix_round`, `crashed_in_task_one`, `dispatched_lines`, `assert_started_fresh`, `pub const TWO_TASKS`, `pub const INTERRUPTED`, and the `TaskSession` import.
- Consumes: `committing_fixture`, `scripted_auto`, `specs_for` already in `common/mod.rs`.

- [ ] **Step 1: Read the helpers**

Read `tests/build_checks.rs` at the definitions of the seven helpers and of `TWO_TASKS` (about line 384, also used about line 2419) and `INTERRUPTED` (about line 2127), and `tests/common/mod.rs` around `committing_fixture`, `scripted_auto`, `specs_for`. Note which `use` lines each helper needs.

- [ ] **Step 2: Record the baseline**

Run: `cargo test -j4 -p factory-engine --test build_checks`
Expected: all tests pass. Note the pass count.

- [ ] **Step 3: Move the helpers**

Cut the seven functions and the two constants from `build_checks.rs` and paste them into `common/mod.rs` as `pub fn` and `pub const`, moving the `use` lines they need (including `TaskSession`). In `build_checks.rs` import them with `use common::{...}`. Mark any helper that `codex_prep.rs` does not use with `#[allow(dead_code)]` only if the compiler warns, as other `common` items do. No test logic changes.

- [ ] **Step 4: Verify**

Run: `cargo test -j4 -p factory-engine --test build_checks`
Expected: the same pass count as Step 2, no warnings.

### Task 2: Sync first, fresh fix builder, backstop

Port: the logic sits behind `AgentRunner`; tests drive it with the scripted fake runner. Board writes stay in the driver.

**Files:**
- Modify: `crates/factory-engine/src/ports.rs` (`sync_provider` at about 210, `run_agent` at about 171)
- Modify: `crates/factory-engine/src/build.rs` (`build_stage` at 36, `new_work` at 49)
- Modify: `crates/factory-engine/src/specs.rs` (`builder_resume_spec` at about 393)
- Modify: `crates/factory-engine/tests/codex_prep.rs`

**Interfaces:**
- Consumes: the Task 1 helpers; `codex()` (line 25), `set_job` (182) and `saved_claude_state` (192) in `tests/codex_prep.rs`.
- Produces: `pub(crate) fn sync_provider(&self) -> Result<bool, EngineError>`; `builder_resume_spec(&self, ..., session: Option<&str>, ...)` (the other parameters keep their order, `session` takes the place of the current `&str` id).

- [ ] **Step 1: Read the code**

Read `ports.rs` `run_agent` and `sync_provider`, `build.rs` lines 36-110 and 690-710, `permissions.rs:69`, `specs.rs` `builder_first_spec` (187) and `builder_resume_spec` (393), and the existing tests in `tests/codex_prep.rs` (`codex`, `set_job`, `saved_claude_state`, the weak test at about 204-221).

- [ ] **Step 2: Write the failing tests**

In `tests/codex_prep.rs` delete `a_provider_switch_clears_saved_session_ids_once`, and add the tests below. Switch a fixture with `let mut fx = ...;` built as Claude, then `fx.config.agent_cli = Some("codex".into());`. Full example:

```rust
#[test]
fn codex_switch_drops_a_crash_resume_session() {
    let mut fx = two_task_fixture();
    crashed_in_task_one(&fx);
    fx.config.agent_cli = Some("codex".into());
    let run = fx.engine().build_stage().expect("build after the switch");
    let specs = fx.runner.dispatched();
    assert!(specs.iter().all(|s| s.resume.is_none()));
    assert!(!specs[0].prompt.contains(INTERRUPTED));
    let job = fx.engine().job().expect("job");
    assert_eq!(job.provider.as_deref(), Some("codex"));
    assert!(job.task_session.is_none() && job.builder_last.is_none());
    assert!(dispatched_lines(&fx).iter().any(|l| l.contains("fresh")));
    let _ = run;
}
```

Build the other fixtures as the cited helpers do (`fix_round_from` and `start_fix_round` for a fix round; `set_job` for a custom board). Adapt names to the real helper signatures you read in Task 1.

| Test | Setup | Assertions |
|---|---|---|
| `a_provider_switch_in_a_fix_round_starts_a_fresh_builder_with_the_findings` | fix round with Claude `builder_id`, `changes-r2.md` exists, switch to Codex | builder spec `resume == None`, agent `factory-builder`, prompt contains `Fix only the problems listed in` and the path of `changes-r2.md`, `provider == Some("codex")`, builder reply saved |
| `rework_without_a_saved_session_sends_the_note_to_a_fresh_builder` | `builder_id` none, `head` set, `rework-note.md` written | `resume == None`, prompt names the note, note file removed after the run |
| `a_first_build_still_uses_the_first_build_prompt` | three jobs: fresh; worktree with no session and no findings; `rework-note.md` but `head == None` | each spec equals what `builder_first_spec` builds (prompt starts as the first-build prompt, no `Fix only the problems listed in`) |
| `a_switched_legacy_job_takes_the_fix_path_and_not_task_one` | plan with `### Task` headings, `task_index == 0`, Claude `builder_id`, findings, switch to Codex | one fresh fix-builder spec with `resume == None`; no spec carries the task-one prompt |
| `builder_resume_spec_without_a_session_adds_the_plan_and_spec_lines` | call `builder_resume_spec(.., None, ..)` | `resume == None`; prompt starts with `You are the builder in a software factory`, contains `already on branch`, `Never edit the plan.` and `Fix only the problems listed in` |
| `a_switch_back_to_claude_clears_a_codex_crash_resume` | Codex project, job with provider `codex` and a saved `task_session`, switch to Claude | every spec `resume == None`, `provider == Some("claude")` |
| `the_provider_switch_clears_once_and_keeps_the_new_session` | switch to Codex, build; then a fix round | the builder saves `sess-1` (from `scripted_auto`); the fix round spec has `resume == Some("sess-1")`; `builder_id` is still `sess-1` after it |
| `a_claude_project_keeps_its_saved_sessions` | Claude job with provider `claude`, then with provider unset | crash resume still has `resume == Some(<saved id>)`; the fix round has `resume == Some("sess-1")` |

- [ ] **Step 3: Run them to see them fail**

Run: `cargo test -j4 -p factory-engine --test codex_prep`
Expected: compile error such as `this function takes 8 arguments` or `expected Option<&str>`, then, once the call sites compile, assertion failures on `resume.is_none()`.

- [ ] **Step 4: Implement `sync_provider` and the backstop in `ports.rs`**

`pub(crate) fn sync_provider(&self) -> Result<bool, EngineError>`: `Ok(false)` at the early return (stored provider equals `agent_cli`) and when it only stamps a missing provider on a Claude project; `Ok(true)` after it clears `task_session`, `builder_id`, `builder_last` and writes the new provider. A missing stored provider counts as `claude`. In `run_agent` replace `self.sync_provider()?;` with `if self.sync_provider()? { spec.resume = None; }`.

- [ ] **Step 5: Implement `builder_resume_spec(.., session: Option<&str>, ..)` in `specs.rs`**

With `Some(id)` nothing changes (keep the existing golden or prompt test green). With `None`: no `spec.resume`; the prompt starts with `You are the builder in a software factory. Your working directory is the job worktree, already on branch <branch>.`, then the `read_line(false)` text, `Never edit the plan.` and `spec_line()`, then the same `Fix only the problems listed in <findings>` text. Agent, tools, limits and permissions as for `Some`.

- [ ] **Step 6: Implement the `build_stage` changes in `build.rs`**

Read the job, compute `legacy` (`job.session_id.is_some() && job.task_index == 0`), call `self.sync_provider()?;`, then read the job again for everything else. Outside a first build pick the spec by the table, with the findings file being `<job_dir>/rework-note.md`, else `<job_dir>/changes-r{round}.md`, else none:

| Saved `builder_id` | Findings file | Spec | `new_work` |
|---|---|---|---|
| set | exists | `builder_resume_spec(.., Some(id))` | false |
| set | missing | error `missing findings file <path>`, as today | n/a |
| none | exists and `job.head` set | `builder_resume_spec(.., None)` | false |
| none | exists, `job.head` None | `builder_first_spec` | true |
| none | missing | `builder_first_spec` | true |

`new_work` is true only for the two `builder_first_spec` rows. Write-backs at `build.rs:91-93`, `:696-708` and `permissions.rs:69` need no edit.

- [ ] **Step 7: Run the tests**

Run: `cargo test -j4 -p factory-engine --test codex_prep` then `cargo test -j4 -p factory-engine --test build_checks`
Expected: all pass, including the new tests; `an_old_job_counts_as_claude` still passes.

### Task 3: Note an unopenable KB repo

Port: `prepare_for_codex` runs before `AgentRunner`; the note goes through the driver's `notice` events writer.

**Files:**
- Modify: `crates/factory-engine/src/specs.rs` (`notice` at about 258 becomes `pub(crate)`)
- Modify: `crates/factory-engine/src/codex_prep.rs` (lines 90-99)
- Modify: `crates/factory-engine/tests/codex_prep.rs`

**Interfaces:**
- Consumes: `Engine::notice(&self, text: &str)`.
- Produces: one `Notice` event per KB agent run when the repo cannot be opened and `kb_auto_commit` is on.

- [ ] **Step 1: Read the code**

Read `codex_prep.rs` lines 60-110 and `specs.rs` `notice`. Find how existing tests in `tests/codex_prep.rs` read the events file and build a role `kb` spec.

- [ ] **Step 2: Write the failing test**

```rust
#[test]
fn a_kb_repo_that_cannot_open_is_noted() {
    // kb_auto_commit on, the KB folder is a plain directory, role kb.
    let spec = prepare_kb_spec_with_plain_dir_kb(); // build as the neighbouring KB test does
    assert!(spec.writable_roots.is_empty());
    let events = std::fs::read_to_string(events_path()).expect("events file");
    assert!(events.contains("cannot be opened"), "events: {events}");
}
```

Replace the two placeholder helpers with the real setup of the existing KB test in the same file.

- [ ] **Step 3: Run it to see it fail**

Run: `cargo test -j4 -p factory-engine --test codex_prep a_kb_repo_that_cannot_open_is_noted`
Expected: FAIL with `events: ` not containing `cannot be opened`.

- [ ] **Step 4: Implement**

Make `notice` `pub(crate)`. In `prepare_for_codex` replace the `if let Ok(kb_repo) = Repo::open(&kb_dir)` block with:

```rust
match Repo::open(&kb_dir) {
    Ok(kb_repo) => spec.writable_roots.push(kb_repo.common_git_dir()),
    Err(e) => self.notice(&format!(
        "kb auto-commit is on but the KB repo at {} cannot be opened, so the agent gets no git writable root: {e}",
        kb_dir.display()
    )),
}
```

- [ ] **Step 5: Run the crate**

Run: `cargo test -j4 -p factory-engine`, then `cargo fmt -p factory-engine`, then `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`
Expected: all tests pass, clippy clean.

Next plan: none
