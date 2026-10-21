# Provider switch fix

## Goal

Make a project that switches its agent CLI never send a saved session id of the old provider to the new one, pin that with tests that follow the real resume paths, and warn when the KB repo cannot be opened.

## Overview

Job 026 added `Engine::sync_provider`, which clears the saved session ids of a job when the project's `agent_cli` differs from the job's provider. It runs only inside `run_agent`, after every caller has already read the job and built its spec. The reviewer of job 026 called this a blocker, and a read of the merged code confirmed it for two paths:

- A task builder resumed after a crash: `saved_session` returns the old Claude id, `builder_task_resume_spec` sets `spec.resume`, and `run_agent` then clears the board but not the spec, so `codex exec resume <claude id>` runs. The run fails and the job retries fresh, so a run is wasted.
- A fix-round resume: `builder_resume_spec` does the same from `job.session_id` and has no fresh retry, so the job likely parks.

Clearing the ids first fixes the first case, but it changes the second: with `builder_id` cleared, `build_stage` takes the first-build branch (`build.rs:44-63`, `new_work`), so a fix round would send the whole-plan prompt again and never show the builder the findings (`changes-rN.md` or `rework-note.md`). A parked job would leave the user stuck with only `factory cancel`, so the user chose a fresh builder that receives the findings (Design 3). The same gap exists today for `factory rework` when a job has no saved session: it goes through the first-build prompt and the note never reaches the builder.

The test meant to pin this (`a_provider_switch_clears_saved_session_ids_once`) seeds a first-build job, so no resume path runs and its `spec.resume == None` assertion passes anyway. A separate finding: when the KB repo cannot be opened with `kb_auto_commit` on, the KB writable root is dropped without any message.

## Current state

| Area | Today | Reference |
|---|---|---|
| Provider reset | `sync_provider` returns `Result<(), EngineError>` and is called once, first in `run_agent` | `crates/factory-engine/src/ports.rs:210`, `:171` |
| Build entry | `build_stage` reads `self.job()` first, then picks `resume_batch`, `build_task` or the legacy path | `crates/factory-engine/src/build.rs:36-50` |
| Stale write-backs | `run.session_id.or(job.session_id)` written to `builder_id` | `build.rs:91-93`, `:696-708`, `crates/factory-engine/src/permissions.rs:69` |
| Resume specs | `spec.resume = Some(id)` only from `builder_task_resume_spec`, `builder_task_carry_spec`, `builder_resume_spec` | `crates/factory-engine/src/specs.rs` (`:346`, `:382`, `:442`) |
| KB root | `if let Ok(kb_repo) = Repo::open(&kb_dir)` with no `else` | `crates/factory-engine/src/codex_prep.rs:93-95` |
| Driver notes | `notice(&self, text)` writes a `Notice` event, private to the `specs` module | `crates/factory-engine/src/specs.rs:258` |
| Weak test | seeds `builder_id` and `builder_last` on a first-build job | `crates/factory-engine/tests/codex_prep.rs:204-221` |
| Test helpers | `change_effect`, `fix_round_from`, `two_task_fixture`, `start_fix_round`, `crashed_in_task_one`, `dispatched_lines`, `assert_started_fresh` are private | `crates/factory-engine/tests/build_checks.rs:11,106,386,623,2082,2100,2344` |
| Shared fixtures | `committing_fixture`, `scripted_auto`, `specs_for` | `crates/factory-engine/tests/common/mod.rs:75,259,174` |

Gaps: the reset runs after the spec is built, the stale write-backs can put the old id back, and no test follows a resume.

## Non-goals

- No change to a Claude job that has a saved session. A job with no saved session but with findings now gets the fix prompt instead of the first-build prompt, as the user asked.
- No change to the Codex runner, the carry rule or the usage mapping.
- No new setting.

## Design

Flow: `build_stage` -> `sync_provider` (clears the board once) -> `self.job()` (clean) -> specs without a stale id -> `run_agent` -> `sync_provider` again (no-op) with a backstop on `spec.resume`.

### 1. `sync_provider` returns whether it switched

Signature `pub(crate) fn sync_provider(&self) -> Result<bool, EngineError>`. It returns `Ok(false)` at the existing early return (stored provider equals the project's) and when it only stamps a missing provider on a Claude project. It returns `Ok(true)` when the stored provider (`claude` when absent) differs from `agent_cli`, after it cleared `task_session`, `builder_id` and `builder_last` and wrote the new provider. It is `true` at most once per switch.

### 2. The call at the top of `build_stage`

`build_stage` calls `self.sync_provider()?;` before it reads the job for the stage decisions (`build.rs:37`), after computing `legacy` from a first read (see Design 3). Every resume spec and every write-back then sees cleared state, so `build.rs:91-93`, `:696-708` and `permissions.rs:69` need no edit: `job.session_id` is `None` after a switch. A fix round re-enters `build_stage`, so it is covered too.

### 3. A fix round without a saved session starts a fresh builder

`builder_resume_spec` (`specs.rs:393`) takes `session: Option<&str>`. With `Some(id)` it is unchanged. With `None` it sets no `spec.resume`, and its prompt starts with `You are the builder in a software factory. Your working directory is the job worktree, already on branch <branch>.`, then the `read_line(false)` text, `Never edit the plan.` and the `spec_line()`, followed by the same `Fix only the problems listed in <findings>` text. The agent, tools, limits and permissions are the same.

In `build_stage` the findings file is the existing `<job_dir>/rework-note.md`, else the existing `<job_dir>/changes-r{round}.md`, else none. Outside a first build (`!fresh`) the spec is chosen by the saved session and the findings:

| Saved `builder_id` | Findings file | Spec | `new_work` |
|---|---|---|---|
| set | exists | `builder_resume_spec(.., Some(id))` | false |
| set | missing | error `missing findings file <path>`, as today | n/a |
| none | exists, and `job.head` is set | `builder_resume_spec(.., None)`, a fresh fix builder | false |
| none | exists, but `job.head` is `None` (no build ever finished) | `builder_first_spec`, as today; the note is not delivered, which is existing behavior | true |
| none | missing | `builder_first_spec`, as today | true |

`new_work` (`build.rs:49`) becomes `true` only for the two `builder_first_spec` rows. `new_work` false means, as today, that `rework-note.md` is removed after the run and the builder reply is saved for the reviewer. A first build (`fresh`) always uses `builder_first_spec`.

The `legacy` test (`build.rs:44`, `job.session_id.is_some() && job.task_index == 0`) is computed from the job as read before the sync, so a legacy whole-plan job that is switched still takes the fix path and is not sent into `build_task` to redo task 1. `build_stage` reads the job, computes `legacy`, calls `sync_provider`, then reads the job again for everything else.

### 4. The backstop in `run_agent`

Replace `self.sync_provider()?;` with `if self.sync_provider()? { spec.resume = None; }`. It applies in both directions, because a saved id of one provider is invalid for the other. It protects any caller other than `build_stage` that builds a resume spec first. A Claude project that never switched sees `false` and keeps its spec.

### 5. The warning for a KB repo that cannot be opened

`notice` becomes `pub(crate)`. In `prepare_for_codex` the block becomes:

```rust
match Repo::open(&kb_dir) {
    Ok(kb_repo) => spec.writable_roots.push(kb_repo.common_git_dir()),
    Err(e) => self.notice(&format!(
        "kb auto-commit is on but the KB repo at {} cannot be opened, so the agent gets no git writable root: {e}",
        kb_dir.display()
    )),
}
```

`GitError` implements `Display` (`thiserror`). The sandbox is otherwise unchanged. The note is written once per KB agent run.

### 6. Tests that follow the resume paths

The helpers listed under Current state move to `tests/common/mod.rs` as `pub fn`, together with the constants `TWO_TASKS` (`build_checks.rs:384`, also used at `:2419`) and `INTERRUPTED` (`:2127`), which become `pub const`, and the `TaskSession` import. `build_checks.rs` imports them; no test logic changes. A test switches the provider by building the fixture as Claude, then setting `fx.config.agent_cli = Some("codex".into())` on a `let mut fx`. The old weak test is replaced by the tests below in `tests/codex_prep.rs`. The backstop of Design 4 is defensive and is not reachable from an integration test, because no stage builds a resume spec before `build_stage`'s sync.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| Job with no provider, project on Claude | `false`; provider stamped `claude`; sessions kept |
| Job with no provider, project on Codex | `true`; sessions cleared; provider `codex` |
| Second call after a switch | `false`; a Codex session saved in between is kept |
| Switch back from Codex to Claude | `true`; Codex ids cleared; a crash resume and a fix round start a fresh builder |
| Switch while the job is in a fix round (findings file exists) | a fresh builder runs with the findings prompt, no resume; the note is removed and the reply saved |
| `factory rework` on a job with no saved session | the same fresh fix builder receives the note |
| Switch on the first build or a crash resume | the builder starts fresh |
| `sync_provider` fails to read the board | the error returns from `build_stage` as any board error does |
| KB repo cannot be opened, `kb_auto_commit` off | no note, as today |
| KB repo cannot be opened, `kb_auto_commit` on | one `Notice` event, no writable root |

## Testing

`factory-engine`, `tests/codex_prep.rs` (helpers from `tests/common/mod.rs`):
- `codex_switch_drops_a_crash_resume_session`: Claude job, `task_session` for task 1 and the current tip (via `crashed_in_task_one`), then a Codex `build_stage`. Every builder spec has `resume == None`, the prompt does not say it was interrupted, the board has `provider == Some("codex")`, `task_session == None`, `builder_last == None`, and the ledger line of the task says `fresh`.
- `a_provider_switch_in_a_fix_round_starts_a_fresh_builder_with_the_findings`: a job in a fix round with a Claude `builder_id` (via `fix_round_from` and `start_fix_round`), then a Codex `build_stage`. The builder spec has `resume == None`, agent `factory-builder`, a prompt containing `Fix only the problems listed in` and the path of `changes-r2.md`, the board has `provider == Some("codex")`, and the builder reply is saved.
- `rework_without_a_saved_session_sends_the_note_to_a_fresh_builder`: a job with no `builder_id` and a `rework-note.md`: the spec has `resume == None`, its prompt names the note, and the note file is removed after the run.
- `a_first_build_still_uses_the_first_build_prompt`: a fresh job, and a job with a worktree but no session and no findings, and a job with a `rework-note.md` but no `head`, all get `builder_first_spec`.
- `a_switched_legacy_job_takes_the_fix_path_and_not_task_one`: a whole-plan job (plan with `### Task` headings, `task_index == 0`, a Claude `builder_id`, findings) switched to Codex gets a fresh fix builder, and `build_task` is not entered.
- `builder_resume_spec_without_a_session_adds_the_plan_and_spec_lines`: a unit-level check of the prompt text, `resume == None`.
- `a_switch_back_to_claude_clears_a_codex_crash_resume`: the other direction on the crash-resume path: a Codex job with a saved task session, a Claude `build_stage`; every builder spec has `resume == None` and `provider == Some("claude")`.
- `the_provider_switch_clears_once_and_keeps_the_new_session`: it also proves `sync_provider` returns `true` only once, since a repeat would clear the new id. After the switch the fresh builder saves its own id (`sess-1` from `scripted_auto`); a later fix round resumes it (`resume == Some("sess-1")`) and `builder_id` is not reset.
- `a_claude_project_keeps_its_saved_sessions`: a Claude job with provider `claude`, then with provider unset: the crash resume still resumes with the saved id and a fix round resumes `sess-1`.
- `a_kb_repo_that_cannot_open_is_noted`: `kb_auto_commit = true`, the KB folder is a plain directory, role `kb`: the events file holds a `Notice` whose text contains `cannot be opened`, and `writable_roots` is empty.
- The old `a_provider_switch_clears_saved_session_ids_once` is removed; `an_old_job_counts_as_claude` stays.

`tests/build_checks.rs`: every existing test still passes after the helper move.

Commands: `cargo test -j4 -p factory-engine`, `cargo clippy -j4 -p factory-engine --all-targets -- -D warnings`, `cargo fmt -p factory-engine -- --check`.

## Files changed

- `crates/factory-engine/src/ports.rs`: `sync_provider` return type, the backstop.
- `crates/factory-engine/src/build.rs`: the call and the spec choice in `build_stage`.
- `crates/factory-engine/src/specs.rs`: `notice` is `pub(crate)`; `builder_resume_spec` takes an optional session.
- `crates/factory-engine/src/codex_prep.rs`: the KB `match`.
- `crates/factory-engine/tests/common/mod.rs`, `tests/build_checks.rs`, `tests/codex_prep.rs`: helper move and new tests.

## Constraints

- Cargo with `-j4`, per crate, no `--workspace`, no `CARGO_TARGET_DIR`.
- No board, ledger or event field is added; `Notice` events already exist.
- A job with a saved session behaves exactly as before.
- The code must pass `cargo fmt --check` and `cargo clippy -j4 --all-targets -- -D warnings`. The builder runs `cargo fmt -p factory-engine` before it finishes.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call. Comments are short full sentences.

## Acceptance criteria

- `sync_provider` returns `true` once per switch and `false` otherwise.
- `build_stage` syncs the provider before it reads the job.
- A fix round without a saved session starts a fresh builder that receives the findings, and `factory rework` works the same way.
- No `spec.resume` built from an old-provider id reaches the runner after a switch, on the crash-resume path, and a fix round starts a fresh builder instead of resuming.
- `run_agent` clears `spec.resume` when a switch happens, in both directions (defensive, no direct test).
- A Codex session saved after a switch is kept and resumed later.
- A Claude project keeps its saved sessions, with provider `claude` or unset.
- An unopenable KB repo with `kb_auto_commit` on writes one `Notice` and adds no writable root.
- The helpers are shared from `tests/common/mod.rs` and all `build_checks.rs` tests still pass.
- `cargo fmt --check` and clippy with `-D warnings` are clean for `factory-engine`.

## Out of scope

- Resuming a Codex session across a crash is not changed, only protected against old ids.
- Adding fmt and clippy to the project checks (a separate job).

## Follow-ups

None.

## Decisions

- What does the backstop in `run_agent` do? Options: clear `spec.resume` for either direction, only when going to Codex, no backstop. Chosen: clear for either direction. Decided by: user
- What should the second run after a switch do in the clear-once test? Options: the new Codex session is kept and resumed, kept but never resumed. Chosen: kept and resumed. Decided by: user
- Where do the new tests live? Options: move the helpers to `tests/common/mod.rs`, write the tests in `build_checks.rs`. Chosen: move the helpers. Decided by: user
- What happens when the provider switches during a fix round? Options: park the job with a clear message, a fresh builder that receives the findings, accept the first-build prompt. First chosen: park. The user then pointed out that a parked job leaves them stuck, and revisited it. Chosen: a fresh builder that receives the findings, which also fixes `factory rework` without a session. Decided by: user
- Call `sync_provider` first in `build_stage`, so the write-backs need no edit. Decided by: call
- Report the unopenable KB repo with `notice` (an events note), not `tracing`. Decided by: call
- Remove the old weak test, because the new ones cover it. Decided by: call
