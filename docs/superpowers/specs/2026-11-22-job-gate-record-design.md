# Every gate of a job on its dashboard card

Date: 2026-11-22. Status: draft for approval. Reviewed by a builder check and twice by an adviser against HEAD 92ba70d (CHANGES 2 blockers / 8 majors, then 0 / 2; all findings taken). Corrected after the plan review (no checks stage on the board, the shared sync park, `format`/`lint` skipped), and again after a reference check against c984339 (round numbering, the merge choke point, fixtures, card colours). Changed by the user to 18 gates (one reviewer gate) and to a project setting, on first for rocket-factory only. The user asked that nothing about this change is committed until they have checked it by hand and decided to keep or drop it.

## Goal

The driver writes down the result of every gate a job meets, and the dashboard job card shows all of them: a compact strip of marks grouped by stage, which opens into a full list with one line of detail per gate.

## Why

Today a gate leaves a trace only when it parks a job. A merged card shows the code and security chips, `format`, `lint` and the budget bar, and nothing else. You cannot see that the no-checks gate passed, that the Cargo lock check cleared a risky diff, that the final gate reused the checks run, or what the spec reviewer said. The user wants one place that shows, for each job, every gate and how it came out.

The same per-job record is the first data source for the planned self-improvement loop (roadmap item 1, "learning loop", in `docs/roadmap/roadmap-2026-10-18.md`), which needs to know, job by job, which gates fired and how.

## Current state

| Piece | Where | Today |
|---|---|---|
| Park record | `factory-core/src/park.rs:252` `ParkRecord`, `park.json` | Written only when a job parks. Every engine park goes through `Engine::park_with_resume` (`factory-engine/src/park.rs:186`); every driver-level park (merge, sync) through `park_with_parts_resume` (`factory-engine/src/merge.rs:54`). |
| Format and lint | `factory-engine/src/checks_stage.rs:137` `record_gates` | Board `reviews` keys, set inside a `self.update` closure, removed by `reset_reviews` (`factory-core/src/board.rs:472`). |
| Review state | `factory-engine/src/review.rs:187-268` | Board `reviews` per role, `last_review` after the round. Risk is computed at `review.rs:100`. |
| Spec review | `factory-engine/src/spec_check.rs:391`, files in `state/spec-reviews/<hash>.md` | Not linked to a job. `driver.last_spec_review()` (`compat.rs:377`) gives the file and the verdict words "approved", "asked for changes" or "gave no verdict" (`spec_check.rs:225-231`) inside the process that ran `factory new`. |
| Waivers | `factory-engine/src/waivers.rs`, `waivers.json` | The only recorded human gate today. |
| Structured log | `state/log.jsonl` (`decide`, `check.run`, `park`, `operator.*`) | Rotates, has no pass events for most gates, and is not a contract. |
| Card | `dashboard/static/board.js:759` | Review chips only in `review`, `approve`, `needs-human`, `merged` (`CHIP_STAGES`, `board.js:76`); park kind label and reason when parked. |

## Non-goals

- No change to any gate's decision, order or park kind. Recording is a side effect only.
- No backfill for jobs that ran before this change. Their cards stay as they are today.
- No change to `board.json`. The board stays compatible with older dashboards and older binaries.
- No change for projects that leave `gate_record` off: they write no `gates.jsonl` and their cards look as they do today.
- No change to the park dialog, the `factory status` output or `factoryd`.
- No learning loop. This change only produces the data.

## Anti-goals

- A failure to append to `gates.jsonl` must never change a job's outcome: no park, no failed merge, no refused approve. It is logged and the job goes on, like the ticket step after a merge.
- The card must not get taller for a job that is fine. The list stays closed unless the job is parked.

## The gates

The record names 18 gate keys, the user's list. The reviewer verdict is one gate, `review`, for both roles (code and security). The order below is the order on the card.

**Who writes what.** Every `fail` line, and the `approve: waiting` line, is written by the two park choke points (section 2), from the park kind. The gate sites below write only `pass`, `note`, `skipped`, `running` and `waiting` lines, plus the few fails that do not park (a checks run that leads to a fix round, a review that asks for changes).

| Stage | Key | Scope | Lines written at the gate site | Site |
|---|---|---|---|---|
| spec | `spec-check` | once | pass with the count of plan and decision warnings; `waived` with the `--accept-spec` reason; `skipped` "no spec" or "no plan" | one helper (section 3) |
| spec | `spec-review` | once | `approved` = pass; `asked for changes` or `gave no verdict` accepted with `--accept-spec` = waived; `skipped` when `spec_review = false` (read from the config, not from `last_spec_review()`, which returns `None` both then and for a failed run, and keeps the last review of the process), when there is no spec, or when an accepted `NoUserDecision` refusal stopped before any review ran; detail: the review file stem (its hash) | the same helper |
| start | `series` | once | pass "plan k of n, earlier plan merged" when `index > 1` and the earlier member is merged; pass "plan 1 of n" for the first plan; pass "plan k of n, earlier plan not on the board" when `wait_for_earlier_plan` returns `None` because the member is missing or the series has no id; `skipped` "single plan" when the job has no `series`; `waiting` comes from the `series-waiting` park | `series_guard` (`series_guard.rs:14`), after `wait_for_earlier_plan` returns `None`. The early return for a job with commits of its own (`series_guard.rs:29-33`) writes nothing, since the line of the first pass stands. |
| start | `machine` | round | pass, with the free disk when `min_free_disk_mb > 0` (measured again with `(self.machine.free_mb)(dir)`, since `machine_gate` does not return it); `waiting` while `wait_while_down` waits; pass "ok after N min" when the wait ends | `machine_gate` (`recover.rs:192`). A line is written only when its result differs from the result of the last `machine` line of the round (the detail is not compared). |
| build | `builder-guard` | round | pass | `run_builder_guarded` (`permissions.rs:39`), on `Ok(Ok(run))` |
| build | `builder` | round | pass with the task count or "whole plan"; `note` "no change, work already on the branch" on the `no_change = true` paths (`build.rs:281`, `:443`, `:852`) | `finish_build` (`build.rs:964`), before the cap check at `build.rs:976` |
| checks | `checks` | round | pass or fail with "N of M commands passed". No word on whether a fix round or a park follows: that is the next line. | `run_checks_now` (`checks_stage.rs:84`), after `checks.md` is written and outside the `update` closure |
| checks | `format` | round | pass or fail with the commands; `skipped` "no format command" when the plan has none | `run_checks_now`, from the same results `record_gates` uses, after `record_gates` returns |
| checks | `lint` | round | as `format` | as `format` |
| checks | `no-checks` | round | pass "all N code paths covered" or "no code paths" | `gate_checks` (`checks_stage.rs:194`) when it returns `None`, in the checks stage and in the final gate |
| checks | `no-tests` | round | pass when a test was added or no code changed; `note` with the paths when no test was added and `require_tests` is false | the decision at `checks_stage.rs:326-340` |
| review | `budget` | round | pass with the percent used; `note` at 80 percent or more | `review.rs`, after `decide(&input)` (`review.rs:317`); `decide` itself stays pure |
| review | `diff-cap` | round | pass "N / cap lines" | `finish_build` (`build.rs:977`), when the cap check passes |
| review | `review` | round | `running` when the roles start; pass when every role that ran gave `APPROVE`; fail when a role gave `CHANGES` (with the blocker count) or did not finish; fail "security agent missing" when the security agent file is not found. Detail: each role's verdict, for example "code APPROVE, security skipped (docs only)", and the review file name | the role loop of `review_stage_as` (`review.rs:146-275`), after the per-role results are set and outside any `update` closure; the missing-agent case writes its line before `review_stage_as` returns the `ReviewFailed` decision (`review.rs:~168`), which `apply_decision` parks later |
| review | `risk` | round | pass "normal"; pass "cleared by the Cargo lock check" with its note; `note` "risky: <paths>" whenever the class is risky (a fail line follows only if the job parks `risky-diff`) | `review_stage_as`, where the risk is fixed (`review.rs:100`) |
| merge | `final-gate` | round | pass "reused the checks run" or "ran N commands" | `final_gate` (`review.rs:401`), when it returns `Ok(None)` |
| merge | `merge` | round | pass with the merge sha | `merge_stage` (`merge.rs:212`), on `Step::Merged`, after the merge lock is released |
| merge | `approve` | round | pass "auto-merge"; pass "approved by the user"; `waived` with the reason and the blocker count after `--waive`; `note` "approved without a review" after `--without-review`; pass "retried" when a merge passes and the last `approve` line of the round is `waiting` | auto-merge in `apply_decision` (`review.rs:365`); the user's approve in the CLI (`factory-cli/src/commands.rs:748`) after `d.ledger(&job).ruling(&ruling)?` and before `merge_stage` (`commands.rs:~847`); "retried" with the `merge` pass |

## Design

### 1. The record: `gates.jsonl` in the job folder

One JSON object per line, appended. A new module `factory-core/src/gate_record.rs` holds the types and the append and read functions, so the engine and the CLI share them. (`gate.rs` is taken by the no-checks and no-tests helpers.) This follows `ParkRecord`: core already does std file I/O.

```json
{"v":1,"gate":"no-checks","result":"pass","round":2,"head":"b83bfbc","at":1795367505,"detail":"all 7 code paths covered"}
```

| Field | Type | Meaning |
|---|---|---|
| `v` | number | Format version, 1. A reader skips lines with a version it does not know. |
| `gate` | string | One of the 18 keys above. |
| `result` | string | `pass`, `fail`, `note`, `waived`, `skipped`, `running`, `waiting`. |
| `round` | number | The job's round when the line was written, always counted from 1: the board stores 0 for a job's first round, so every writer writes `max(board round, 1)` and the reader compares with `max(board round, 1)`. Once-scoped gates write the round too; the reader ignores it for them. |
| `head` | string or absent | Short sha of the job's head when the line was written, when there is one. |
| `at` | number | Unix seconds. |
| `detail` | string | One short line for a human, at most 300 characters (cut with "…"). Never a secret: only paths, counts, verdict words and reasons the park record already shows. |

- In Rust: `GateKey` and `GateResult` are enums with `serde(rename_all = "kebab-case")`; `GateLine` is the struct. `GATES_FILE = "gates.jsonl"`.
- `append(dir, &GateLine) -> Result<(), CoreError>` opens the file in append mode (create the file, never the folder: unlike `Ledger::append`, it does not call `create_dir_all`, so a removed job folder is not brought back). It writes the line with one `write_all`. When the file is not empty and does not end with a newline (a cut line after a crash), it writes a newline first.
- `read(dir) -> Vec<GateLine>` skips blank, broken and unknown-version lines and unknown keys or results.
- The engine gets one best-effort wrapper, `Engine::gate(key, result, detail)`, which does nothing when the project's `gate_record` setting is off, in a new `factory-engine/src/gate_record.rs`. It reads round and head with `self.job().ok()` (round 1 when the board cannot be read, as `park_with_resume` does at `park.rs:193-194`), never with `?`. On an append error it logs `tracing::warn!(event = "gates.record_error", ..)` and returns `()`. A free function `record_gate(driver, job, ..)` does the same for driver-level code (merge, CLI), reading the setting from the driver's config and the round as `job.round.max(1)`.
- **Rule for every call site:** never call it inside a `self.update` / `update_job` closure, under the merge lock, or before the step it reports has been written. It is always the last thing at its site, after the step's own state is saved.

### 1a. The `gate_record` setting

`gate_record` is a bool in `project.toml`, default `false`, next to `tickets_on_merge` and `minor_tickets` in `Config` (`factory-core/src/config.rs:163-165`), listed in `factory project show` (`settings_block.rs`) and in `docs/reference/project-toml-reference.md`. Off, no code path writes `gates.jsonl`. The user sets `gate_record = true` in rocket-factory's `project.toml` for the trial; Rocket and every other project stay off until the user turns it on.

```toml
gate_record = true
```

### 2. Fail and waiting lines from the two park choke points

`Engine::park_with_resume` (`factory-engine/src/park.rs:186`) and `park_with_parts_resume` (`factory-engine/src/merge.rs:54`) write, right after the park record, one line from the park kind:

| Park kind | Gate key | Result |
|---|---|---|
| `builder-stopped`, `builder-blocked`, `builder-committed`, `nothing-changed`, `commit-failed` | `builder` | fail |
| `builder-tampered` | `builder-guard` | fail |
| `checks-failed` | `checks` (also when it comes from `nothing_changed_after_checks`, `build.rs:452`, and from the final gate) | fail |
| `no-checks` | `no-checks` | fail |
| `no-tests` | `no-tests` | fail |
| `diff-too-large` | `diff-cap` | fail |
| `budget-stopped` | `budget` | fail |
| `review-failed`, `changes-remain` | none: the role loop already wrote the `review: fail` line, also for a missing security agent | no line |
| `risky-diff` | `risk` | fail |
| `series-waiting` | `series` | waiting |
| `machine-waiting` | `machine` | fail |
| `other` | the explicit gate the caller passes (below); else by the stage read before the park update: build → `builder`, review → `review`, approve → `merge`; any other stage → no line. (There is no checks stage on the board: the checks run while the job is at `review`.) | fail |

Both choke points get one more argument, `gate: GateTarget`, with three values: `FromKind` (the default, the map above), `Key(GateKey)` (write the fail line under this key) and `NoFailLine` (write only the `approve: waiting` line, if any). Callers that pass something other than `FromKind`:

| Caller | Passes | Why |
|---|---|---|
| `merge_stage` (`merge.rs:212`), every `Step::Park` and the `Err` arm (`merge.rs:259-270`) | `Key(merge)` | These are all merge-step parks: scope violation (`merge.rs:327`), tip moved, main busy, merge lock busy, conflict, refused, "main moved again after N syncs", "merge failed". A sync inside `try_merge` sets the stage to review (`sync.rs:216`), so the stage would give the wrong key. |
| the requested-sync park at `sync.rs:120-122`, only when the reason is "checks failed after sync" or "final checks failed after sync" | `Key(checks)` | That park is shared by every requested-sync outcome (conflict, refused, changes); every other reason keeps `FromKind`. |
| `sync.rs:130` "sync review passed, run factory approve" | `NoFailLine` | No gate failed; only `approve: waiting` is written. |
| `park_gate` (`checks_stage.rs:204-220`) when the kind is `Other` (the scope violation at `checks_stage.rs:313`) | `Key(checks)` | The job is at stage review there, so the stage would give `review`. |

"The stage read before the park update" means: in `Engine::park_with_resume`, the job read at `park.rs:194` before the board update; in `park_with_parts_resume`, the stage captured inside its `update_job` closure before `set_stage_logged` moves it to `needs-human` (`merge.rs:54-76`; there is no read before the closure, and the job it returns already has stage `needs-human`). The closure returns the captured stage next to the job.

`park_with_parts_resume` returns early, writing no record, when the job is already parked (`already`). It writes no gate line then either: the earlier park wrote one. This covers `park_after_sync` followed by the no-op `park_with_parts` at `sync.rs:122`. In `Engine::park_with_resume` the line is written inside the `in_park_order` record step, right after `write_record`, so a ruling error after it does not drop the line. The remaining `other` parks (`park_missing_plan`, `park_unsettled`, `runloop.rs:910-945`, and the rest) fall back to the stage.

The detail is the park reason, cut to 300 characters. Then, when the park's `next` commands include one that starts with `factory approve`, the choke point also writes `approve: waiting`. This fires on every park that offers approve, as the card's buttons do today; it is not tied to an approving review.

This replaces about a dozen fail sites (`build.rs` alone parks at 12 places) with two, and closes the window between the park record and the gate line.

### 3. The spec gates helper

`record_spec_gates(driver, jobs, Option<&Validated>, accepted)` writes (`None` in the no-plan branch, which has no `Validated`; then both lines are `skipped` "no plan"). `Validated` gets a `review_ran: bool` field, so an accepted `NoUserDecision` refusal, where no agent review ran, is told apart by a field, not by the " (no agent review ran)" text of `accepted` (`runloop.rs:487`). The helper writes the `spec-check` and `spec-review` lines into each job's folder. It is called at every place that creates jobs:

- `create_job_with_spec_accepting` (`runloop.rs:562`), in both branches: after `add_series_jobs`, and in the no-plan branch before its early return (`runloop.rs:578-590`). This also covers the one-plan `--one-job` path (`one_job.rs:93`) and intake (`intake.rs:516`).
- `create_one_job_accepting`, combined branch, after `add_job` (`one_job.rs:134`).
- `create_replacement` (`runloop.rs:813`).

### 4. Latest state per gate

A shared rule, implemented in the dashboard server (and later by the learning loop):

- Once-scoped gates (`spec-check`, `spec-review`, `series`): the last line wins, whatever its round.
- Every other gate is round-scoped: its state is the last line with `round == max(job.round, 1)`. Lines of earlier rounds are the gate's history ("CHANGES r1, r2").
- A round-scoped gate with no line in the current round is "not reached", even if an earlier round has a line.
- Retry at the same round, sync and rebase reviews append new lines; the last one wins.
- **Stale states.** A cancelled job is one with `cancelled == Some(true)` (`dashboard/src/model.rs:164`, an `Option<bool>`); its stage stays as it was. These show as "interrupted" (a grey mark with the note "interrupted"):
  - a `running` or `waiting` state on a job that is `merged` or cancelled;
  - a `running` state on a `needs-human` job;
  - a `waiting` state on a job in `spec`, `build`, `checks` or `review`, except `series: waiting` and `machine: waiting`, which are written while the job runs. (A mid-task `factory rework` keeps the round, `commands.rs:897-913`, so an old `approve: waiting` would otherwise pulse on a job back in build.)

  `waiting` on a `needs-human` job is real and stays.
- **The parked gate.** The server also returns `stopped_at`: the gate of the last `fail` line of the current round, written by the choke point. The card's "stopped at <gate>" uses it, not the first fail in card order, so an older fail in the same round (a `machine: fail` a retry did not re-run) does not win.

### 5. Dashboard server

- `dashboard/src/gates.rs` reads `<sdd>/<dir>/gates.jsonl` with the same path check as `park_path` (`dashboard/src/park.rs:14`). It reads at most the last 512 KB of the file. The server already reads `plan.md` and stats `park.json` per job on each poll (`state.rs:252`, `:306`), so one small read per job is fine.
- `JobView` (`dashboard/src/state.rs:19`) gets an optional `gates` field: `null` when the file does not exist (an old job, or a project with `gate_record` off), else a list of 18 entries in card order, each `{gate, stage, state, detail, round, history}` where `state` is one of the results, `not-reached` or `interrupted`, and `history` is a short text of earlier rounds' results.
- Works in both modes: `--root` and `--home` both reach `build_state` (`home_mode.rs` → `server.rs:348 state_reply`), which reads the job folders from disk.
- Builder's call: whether to cache the parsed file by size and mtime between polls.

### 6. The card

As in the approved mockup `.superpowers/brainstorm/3077297-1795367505/content/gate-card-a-plus-c.html`, with the corrections below. Where the mockup and this spec differ, this spec wins.

- **Strip.** Under the title, on every card from the `spec` stage on: six groups (spec, start, build, checks, review, merge), one 10 px mark per gate. Colours from the dashboard tokens: pass `--success`, fail `--destructive`, note and waived `--attention`, running and waiting `--primary` (pulsing), skipped a dashed outline, not reached and interrupted `--muted-foreground` (the `--muted` token is a pale background and would not show on a light card). Each mark has a `title` with the gate name and detail.
- **Summary** at the right end of the strip: "N / 18" while the job runs (N = gates with a state other than not reached); "stopped at <gate>" when parked (from `stopped_at`); "<p> passed · <n> notes · <s> skipped" when merged, leaving out zero parts; "cancelled" when `cancelled` is true.
- **List.** The strip is a button. Tapping it opens a list under it: stage headings, then one row per gate with the mark, the name and the detail (and the history in brackets). The failing gate's row is red and has the park reason under it. Not-reached gates are grey.
- **Open by default** only for a `needs-human` card. The strip and list are a `<details>` element with the strip as its `<summary>`, so `patch.js` keeps a card the user opened open across polls (`patch.js:14-15`); the open state is not stored.
- **Chips.** On a job with a gate record, the review chips (`code`, `security`, `format`, `lint`) are not drawn; the strip replaces them. The park kind chip, reason, blocker notice, action buttons, budget bar and spec warning stay as they are.
- **No record** (`gates` is `null`: an old job, or a project with `gate_record` off): no strip is drawn and the card is exactly as today, review chips included.
- Phone width: the strip wraps by group; the list is one column.
- Mockup corrections: the counts are of 18; one `review` mark replaces the code and security marks; `approve: waiting` pulses in `--primary`, it is not grey; on a risky park `final-gate` is a pass, since `apply_decision` runs the final gate before a risky park (`review.rs:369-372`); the `merge` row's detail is the merge sha; a merged job with several rounds still shows `series` from its first round.

### 7. Docs

- `docs/reference/software-factory.md`: a short "Gate record" section (file, fields, the latest-state rule, the list of keys, the park-kind map) and a line in the dashboard part.
- `dashboard/README.md`: the card strip and the `gates` field.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| The append fails (the file is a folder, the disk is full) | The warn is logged and the job goes on. The rest of the job's file writes are unchanged and keep their own error handling: a read-only job folder still fails at `checks.md` as it does today. |
| The job folder was removed | `append` does not create it; the warn is logged. |
| A line is cut by a crash | `read` skips it; the next `append` starts a new line. |
| The board cannot be read when a line is written | The wrapper uses round 1 and no head, as `park_with_resume` does. |
| `factory new` runs the spec review in another process, or the review run failed and the user passed `--accept-spec` | `spec-review` shows not reached; `spec-check` records the waiver. |
| A series | Each job of the series gets its own spec lines at creation. Later plans show only spec marks until they start. |
| Rework | The round goes up, so every round-scoped gate is "not reached" again until it runs; history shows the earlier rounds. |
| A merge park, then `factory retry` | The park writes `merge: fail` and `approve: waiting`; the retried merge writes `merge: pass` and `approve: pass "retried"`. |
| Cancel or crash during a review | `review: running` stays in the file; the card shows it as interrupted. |
| A mid-task rework after an approve-offering park | The round stays; the old `approve: waiting` shows as interrupted while the job is back in build. |
| A gate called many times per round | `machine` writes only on change. `checks`, `no-checks` and `final-gate` write each run; the last wins. |
| A file grows large | Real jobs write tens of lines. The server reads only the tail. |
| Unknown gate key or result in a newer file | The reader ignores the line, so an older dashboard keeps working. |

## Testing

- Core (`gate_record.rs`): round-trip of `GateLine`; `read` skips a broken, a blank, a `v: 2` and an unknown-key line; `append` after a cut line starts a new line; `append` into a missing folder fails and creates nothing; the 300-character cut.
- Core (`config.rs`): `gate_record` is off by default, parses `true`, and refuses a non-bool, as `minor_tickets_is_on_by_default_and_can_be_turned_off` does.
- Engine, with the existing fakes (`tests/common/mod.rs` `Fixture::new`, `scripted`, `scripted_roles`, `ScriptedRunner::specs_for`, `MergeFixture` and the `approved_job*` helpers; `tests/cargo_risk.rs` `fixture()`, `change`, `approving` and `run_job` for the offline lock check), each with `gate_record = true` set on the fixture config:
  - a job that auto-merges writes a pass line for each round-scoped gate it meets and no `fail`;
  - a checks failure writes `checks: fail` in round 1 and `checks: pass` in round 2;
  - a risky diff writes `risk: note`, `final-gate: pass`, `risk: fail` and `approve: waiting`;
  - a no-tests note writes `no-tests: note`;
  - a Cargo-only risky diff cleared by the lock check writes `risk: pass` with the note;
  - a builder that changes `plan.md` writes `builder-guard: fail`;
  - each `ParkKind` maps to the key in the table (a unit test of the map), including `review-failed` and `changes-remain` → no line, `other` by the stage before the park, and the explicit targets: a merge park after a sync writes `merge: fail`, "checks failed after sync" writes `checks: fail`, "sync review passed" writes only `approve: waiting`;
  - a review that asks for changes writes `review: fail` with the blocker count, and the fix round then writes `review: pass` in round 2;
  - a docs-only diff writes `review: pass` with "security skipped (docs only)" in its detail;
  - a missing security agent writes `review: fail "security agent missing"` and no second fail line from the park;
  - with `gate_record` off (the fixture default), a job that auto-merges writes no `gates.jsonl`;
  - the round of a first-round line is 1, while the board round is 0;
  - with `gates.jsonl` made a folder, the job still merges with the same outcome and the warn is logged.
- CLI: `approve --waive` writes `approve: waived` with the blocker count after the ledger line; `factory new` writes `spec-check` and `spec-review` into every job of a two-plan series, and the combined `--one-job` path writes them too (fake spec reviewer).
- Dashboard server: the latest-state rule (once vs round-scoped, not reached, history); each interrupted case (merged, cancelled flag, needs-human running, `approve: waiting` on a job back in build, while `machine: waiting` in build stays); `stopped_at` is the last fail of the round, not the first in card order; `gates: null` without a file; a bad `dir` reads nothing; the tail read.
- Dashboard JS (`dashboard/tests/board.test.js`, `node:test`, with `makeJob`): the strip has 18 marks in order; the summary texts for running, parked, merged and cancelled; a needs-human card renders its `<details>` open; with `gates: null` no strip is drawn and the chips are shown; with a record the chips are hidden.
- `factory-cli/tests/no_hardcoded_paths.rs` must still pass (the file name is a constant, the folder comes from `Layout`).

## Files changed

- `crates/factory-core/src/gate_record.rs` (new), `crates/factory-core/src/lib.rs`, `crates/factory-core/src/config.rs` and `settings_block.rs` (the setting).
- `crates/factory-engine/src/gate_record.rs` (new: the wrapper, `record_gate`, the park-kind map, `record_spec_gates`), `lib.rs`.
- `crates/factory-engine/src/`: `park.rs` and `merge.rs` (choke points, merge pass), `runloop.rs`, `one_job.rs`, `series_guard.rs`, `recover.rs`, `permissions.rs`, `build.rs`, `checks_stage.rs`, `review.rs`.
- `crates/factory-cli/src/commands.rs` (approve).
- `dashboard/src/gates.rs` (new), `dashboard/src/state.rs`, `dashboard/src/main.rs` (module list).
- `dashboard/static/board.js`, `dashboard/static/dashboard.css`, `dashboard/tests/board.test.js`.
- `docs/reference/software-factory.md`, `docs/reference/project-toml-reference.md`, `dashboard/README.md`.

## Constraints

- Nothing is committed until the user has checked the change by hand and decided. The work is done by hand (not as a factory job, which would commit and merge) in a sibling git worktree, `../rocket-factory-gate-record` on branch `gate-record`, with no commits. The main checkout stays clean, so other factory jobs are not parked as main busy.
- The user's trial: one small real rocket-factory job run with the worktree's own `target/debug/factory` by full path (the installed `~/.local/bin/factory` is not replaced), and the worktree's dashboard on a separate port. That trial job commits and merges its own work as usual; that is not a commit of this change.
- Cargo commands follow the repo rules: `-j4`, per crate, never `--workspace` or `--all`, never `CARGO_TARGET_DIR`.
- Never shell out to `git`. Paths reach the driver only through `Layout` (the job folder) and constants.
- Board JSON is unchanged.
- Dashboard JS tests use `node:test`; CI runs `node --test dashboard/tests/*.test.js`, and `bun test dashboard/tests/` runs the same files.

## Acceptance criteria

- A new rocket-factory job that auto-merges has a `gates.jsonl` with a line for each gate it reached, and its merged card shows a full strip where every mark is pass, note or skipped.
- A parked job's card opens its list by itself, and the red row names the same reason as the park record.
- A job from before the change, and every job of a project with `gate_record` off, has no `gates.jsonl` and a card exactly as today.
- With `gate_record = true` only in rocket-factory's `project.toml`, rocket-factory jobs write `gates.jsonl` and jobs of other projects do not.
- A failing append to `gates.jsonl` (for example, the path is a folder) does not change the job's outcome.
- `cargo test -j4` passes for `factory-core`, `factory-engine`, `factory-cli` and the dashboard; `node --test dashboard/tests/*.test.js` (what CI runs) and `bun test dashboard/tests/` pass; clippy and fmt are clean.

## Approaches considered

| Approach | Trade-off | Outcome |
|---|---|---|
| A. Put the gates into the board `reviews` map | Cheapest, the chip code draws them; but `reviews` drives approve and "review missing", is reset each round, and keeps no history. | Dropped. |
| B. Rebuild the gates in the dashboard from `log.jsonl` | No driver change for logged gates; but the log rotates, has no pass events for most gates, and is not a contract. | Dropped. |
| C. The driver writes a per-job gate record; the server summarises it | More work; explicit, testable format, history per round, and the data the learning loop needs. Fail lines come from the two park choke points, so most new calls are pass lines. | Picked by the user. |

Card layouts considered: A, a stage strip; B, one chip per stage; C, a summary line with a full list. The user picked A with C's list behind a tap.

## Decisions

- Decided by: user. Approach C, the per-job gate record.
- Decided by: user. Card layout A plus C's list, as in the mockup.
- Decided by: user. No commit until the user checked it by hand.
- Decided by: user. Built by hand in the sibling worktree `../rocket-factory-gate-record`, tried with one real rocket-factory job on that worktree's binary.
- Claude's call. The record is an append-only `gates.jsonl`, so a crash loses at most one line and the history is kept.
- Decided by: user. 18 keys, the user's list: one `review` gate for both reviewer roles. Options were 18 with one reviewer mark, or 19 with code and security apart.
- Decided by: user. A `gate_record` project setting, default off, turned on first for rocket-factory only. Options were the setting, the trial binary only, or a dashboard filter.
- Claude's call. A card without a record draws no strip at all (no grey "not recorded" strip), so projects with the setting off look exactly as today.
- Claude's call. Rounds are counted from 1 in the record (`max(board round, 1)`), because the board stores 0 for the first round and the engine counts it as 1.
- Claude's call. Scope violations map to the gate of the stage they park in, not a key of their own.
- Claude's call. Every fail line comes from the two park choke points, mapped from the park kind, with an explicit gate for merge and sync parks (adviser findings). Review parks write no fail line there; the role loop already did.
- Claude's call. `spec-check`, `spec-review` and `series` are once-scoped; the rest are round-scoped.
- Claude's call. `approve: waiting` for every park whose next commands include `factory approve`.
- Claude's call. Stale `running` and `waiting` states show as interrupted on merged and cancelled cards.
- Claude's call. The review chips are hidden on cards that have a record, to keep one place for gates.
- Claude's call. `machine` writes only when its result changes. No backfill of old jobs.
- Claude's call. `format` and `lint` write `skipped` when the plan has no such command, so a merged card has no grey mark for them (adviser finding on the acceptance criterion).
- Claude's call. A "checks failed after sync" inside the merge step's own auto-sync is recorded as `merge: fail` with that reason, since it is a merge-step park.

## Follow-ups

- The learning loop reads `gates.jsonl` across jobs (roadmap item 1).
- `factory status` could show the same strip as text.
