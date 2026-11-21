# Builder recovers instead of parking

Date: 2026-11-21. Item 1 of the 2026-11-19 efficiency adviser brief. Relates to issue #42.

## Intent

Builder-side parks caused 28 parks and 3 cancels in jobs 031-088, and each one waited for a human. Almost none needed new code. They needed the factory to carry on once the machine was fixed, or to see that the work was already on the branch:

- Machine problems: disk full (037, 053), network (050), invalid API key (082). The human fixed the machine and sent a rework note like "fixed, rerun".
- That note reached a builder as findings. With nothing to change, the builder parked again with "made no changes on resume" (066, 077, 082, 086 twice). The human then made up a small change to get past it, or cancelled (066).
- Work already on the branch: 037 task 3 and 058 task 1 changed nothing because their work was already committed (#42).
- Decision notes such as "use 6.4.2 as-is" became a findings file with no findings, so the builder asked for context (047, 066). This design removes the "fixed, rerun" kind of note by giving machine parks `factory retry`. Decision notes on other parks are out of scope (see Non-goals).

Success: a machine problem fixed within 25 minutes costs the human the fix and no command (later than that, one `factory retry`), and a builder that changes nothing on a branch with work goes on to checks and review.

## Goals

1. Before each builder run, check the machine locally. While a check fails, wait and check again, then start the builder. Park only after the wait limit.
2. When a builder fails or reports blocked because of the machine, wait for the machine the same way, then rerun the builder once.
3. When a builder fails for a reason the factory can't place, rerun it once right away. A second failure parks.
4. When a builder changes nothing and the branch has commits above base, go on to checks and review (or to the next task) instead of parking.
5. Machine parks resume with `factory retry` (no note). `factory rework` keeps its current job: real findings.

## Non-goals

- Retrying reviewers, the spec reviewer or check runs. In 082 the bad key first hit the reviewers (verdict MISSING), so this work alone would not have saved that first park; reviewer retries are issue #124.
- Fixing the machine. The factory only waits for the human or the world to fix it.
- Calling the API before a builder to test the network or the key (needs a new crate or costs tokens, see Approaches).
- Retrying `BudgetStop`, `TurnLimit` or `Stalled` runs. Those keep their parks.
- Changing how real findings are passed to a builder.
- Refusing or rewriting rework notes that hold a decision rather than findings (047, 066 "use 6.4.2 as-is").

## Anti-goals

- A loop that burns tokens: a retry or a wait must never start a builder that does real work twice for the same failure without a cap.
- Hiding a real failure: every wait, retry and skip writes a ledger line and an event, and the final park names the cause and the error text.
- Merging work that was never reviewed: "nothing changed, go to review" must still pass checks, the diff cap, the risky-diff rule and every review.
- A wait that `factory cancel` or a shutdown can't interrupt.

## Approaches

- **A. Local checks plus reading the failure (picked).** Before a builder: free disk and a readable key file, using code that already exists (`fs2`, `check_key_file`). After a builder fails or reports blocked: classify the error text as a machine problem or not. While waiting, rerun the local checks; for network and key problems the probe is the builder rerun itself, which fails before doing work and costs about nothing. This works for the Claude and Codex runners alike.
- **B. Ask the API directly before each builder.** It catches problems a few seconds earlier. But it needs a new HTTP crate, which trips the Cargo risky-diff park. It only covers API-key auth, and it still misses a network drop mid-run (050), so it would need A's classification anyway.
- **C. A tiny `claude` call before each builder.** It costs tokens and about 10 seconds on every builder start, and it still needs A for failures mid-run.

The user picked A. For a builder that changes nothing on a branch with work, the user picked "go straight to checks and review" over "start a fresh builder first" (costs a whole builder run that usually changes nothing too) and "keep parking" (today's behaviour). The user then chose to apply that to every round, the factory's own fix rounds included, over limiting it to the user's reworks and first runs. That choice keeps one rule, the cost is capped by `fix_rounds`, and passing the builder's reason to the reviewers turns a re-review into a judgement on evidence.

## Design

### Machine checks (preflight)

A preflight runs before every builder start: first run, resume, carry, fresh and every rerun or probe, in both the whole-plan path (`build_stage`) and the per-task path (`build_task`).

- **Disk:** two filesystems must each have at least `min_free_disk_mb` free: the one holding the job worktree, and the one holding the factory home (state, ledger, events), when that is a different filesystem. `min_free_disk_mb` is a new `project.toml` setting, default 2048. The setting is documented in `docs/reference/project-toml-reference.md`.
- **Key file:** when the runner uses a key file, `check_key_file` must pass. When it doesn't use one (environment key or login), this check is skipped.

When a check fails, the job waits (see Waiting). When it passes, the builder starts.

### Classifying a failed or blocked builder

A new pure function in `factory-agent` sorts a finished run into one of two classes: `Machine(cause)`, where cause is `Disk`, `Network` or `Auth`; or `Other`.

- **`Failed` runs:** the function reads `AgentRun.error`, never the builder's prose, so a test that "timed out" or a `401` in code is not misread. On Claude today `error` doesn't hold the API text: for an `is_error` result, child.rs ~242-247 drops `result_text` when the label is `claude`, so `error` reads only "claude reported success" while "API Error: Can't reach the API server (EAI_AGAIN)" or "Invalid API key" sits in `result_text`. The plan must change child.rs so that for a Failed run with an `is_error` result, `error` carries the CLI's result text (capped at 500 chars). The classifier then still reads `error` only. Builder prose on a Done run never reaches it.
  - Examples: "Invalid API key", "authentication_error" and "401" give Auth. "Connection error", "ECONNRESET", "ENOTFOUND", "529" and "overloaded" give Network. "No space left on device" and "ENOSPC" give Disk.
  - The exact list is left to the builder. Unit tests pin positive and negative cases.
- **Blocked reports:** `BuilderBlocked` and `NEEDS_CONTEXT` (both go through `park_if_blocked`) count as `Machine(Disk)` when the preflight disk check fails at that moment (037 and 053 said "The disk is full"). Otherwise they park as today.
- `BudgetStop`, `TurnLimit` and `Stalled` are never classified. They keep their parks.

### What happens after a builder run

| Run outcome | New behaviour |
|---|---|
| Failed, `Machine` | wait for the machine (probes), then rerun once |
| Failed, `Other` | rerun once right away; a second failure parks `BuilderStopped` |
| Blocked or needs context, disk check fails | wait for the machine, then rerun once |
| Blocked or needs context otherwise | unchanged |
| Nothing changed, branch has commits above base | ledger line, then continue (see below) |
| Nothing changed, no commits above base | unchanged: parks `NothingChanged` |

**What a rerun runs, per path.** A rerun is the same builder step again, never a step that needs a file the round doesn't have.

- **A Failed run writes no session state.** Today build.rs ~163-171 stores `builder_id`, `builder_last` (with `end_ctx` 0) and `builder_session_model` from every run before it checks the stop reason. After a Failed run, that makes the next pass take the resume arm, which needs a findings file and fails in round 1 with "missing findings file" (build.rs ~136, ~282). It also stops `cold_session` (build.rs ~52-66) and `needs_fresh_session` (build.rs ~43-48) from firing, so a fresh-fix or escalated round would rerun as a resume of the failed session. The plan must keep the earlier session fields when a run ends Failed. Then the next pass picks the same arm as the failed one, on every arm.
- **Whole-plan path, round 1:** the rerun is a fresh `builder_first_spec` on the worktree as it stands, and it counts as new work (`new_work`, build.rs ~113: commit subject, `build.md`, ledger wording). The same holds for `factory retry` after such a park.
- **Whole-plan path, round with findings:** resume the stored session with the same findings file. When resuming fails because the session is gone, the rerun is a fresh `builder_fix_fresh_spec`. That fresh start is the rerun, not an extra one.
- **Per-task path:** the run is classified before the existing fresh fallback for a failed resume or carry (build.rs ~625-655). A `Machine` failure goes to the wait first; its probes and its rerun use the fresh task spec. An `Other` failure takes the fresh fallback at once, and a plain failed first run of a task now gets that too. That fresh run is the step's one rerun in every case, so nothing is counted twice. `task_session` and `builder_last` are cleared after each run (build.rs ~657-697), so a retry after a park always starts the task fresh.

The rerun writes the ledger line `builder failed (<cause or first 200 chars of error>), rerunning once`. A park after a failure carries the error text, for example `builder stopped: Failed: <first 200 chars>`. Today the error text is dropped.

**Rework note.** Both paths delete `rework-note.md` today before they look at the stop reason (whole-plan build.rs ~156-160, per-task build.rs ~699-702). Both must delete it only after a run that is accepted: Done and not blocked or needs-context. Blocked and needs-context runs also end Done (build.rs ~464-473), so "after Done" alone would lose the note before a disk-blocked rerun. A run that parks keeps the note as well, so `factory retry` still has it.

**Budget.** Before every rerun and probe, the factory checks the job budget with `budget_state`. A probe or rerun never starts once the budget is spent: the job parks `BudgetStopped` as usual. Today the budget is checked only in `decide` (decide.rs ~31).

### Waiting

- **The job stays at Build.** The board gets a new optional job field, `machine_wait`, holding a short text such as `disk: 0.4 GB free` or `network`. Older dashboards ignore it.
- **The field is cleared** when the wait ends, when the job parks, and at the start of every Build pass. A kill or shutdown mid-wait can't leave it stale for longer than the next run.
- **Probe schedule:** after 1, 2, 4, 8 and 15 minutes, then every 15 minutes, for 25 minutes in all (see Lease TTL).
  - For Disk, each probe reruns the local checks.
  - For Network and Auth, each probe is a builder rerun, whatever the key source. A readable key file can still hold a bad key (082).
- **What counts as a probe.** A builder rerun counts as a probe, not as the job's one rerun, only when it did no work: at most 1 turn, fewer than 2,000 output tokens and fewer than 20,000 weighted tokens (my call; the weighted cap catches a resume of a large session that reads its whole cache and then fails). A bad-key run reports `turns:1, output:0` (082), so it counts as a probe. A rerun that did more than that is the rerun, even if it then fails with the same cause. This keeps a mid-run network drop (050) from doing real work five times.
- **Interrupts.** The wait sleeps in steps of at most 5 seconds. After each step it checks two things:
  - a cancel, by reading the board with no write: the job's stage is no longer Build (cancel moves it to NeedsHuman, commands.rs ~920) or `cancelled` is set. A plain `update` never returns `Cancelled` (only `set_stage` and `resume` check, board.rs ~452, ~463), and a write would fail on a full disk;
  - the lost-lease flag that `run_agent` already checks (ports.rs ~252).

  Either one ends the wait at once, with the result a cancel or a lost lease gives at the next stage change today. While the lease is held, `factory cancel` needs `--force` (commands.rs ~912), as for any running job.
- **Shutdown.** A SIGTERM, SIGINT or SIGHUP exits the process through the procgroup handler (shutdown.rs ~19-21). The job stays at Build, and the next run starts the step again with a fresh preflight.
- **Testing.** The wait helper takes its sleep, its clock and its free-space probe as parameters, so tests run the schedule and a disk that frees up without real sleeps or a real full disk. No injectable clock exists today: `clock.rs` has only `now_secs`. A Driver builder in the style of `with_lease_timing` (compat.rs ~407) lets the e2e tests inject them.
- **Writes are best effort.** During a disk wait, the ledger line, the board field and the events may fail to write. Those failures are logged to stderr and never end the wait.
- **Lease TTL.** `LEASE_TTL_SECS` is 1800 s (compat.rs ~239), the same as the wait, and on a full disk the heartbeat renewal fails quietly (compat.rs ~302). The wait cap is therefore 25 minutes, below the TTL, and the schedule above ends at 25 minutes (my call).
- **The project lease is held during the wait** (up to 25 minutes plus the rerun), so factoryd's queue for the project and the next plans of a series wait too. That is accepted (my call): disk, network and key are shared by every job of the project, so the next job would hit the same problem.
- **After 25 minutes,** the job parks with a new `ParkKind::MachineWaiting`, as string `machine-waiting`. The reason names the cause, for example `disk full: 0.4 GB free on <path>, waited 25 min` or `network: <error>`. The park resumes at Build.
- **Ledger:** one line when the wait starts and one when it ends, either `machine ok after N min` or the park. There is no line per probe.
- **Events:** `builder.wait` when the wait starts, `builder.probe` per probe, `builder.wait_end` when it ends, and `builder.rerun` for the rerun.

### Resuming a machine park

- `ParkKind::MachineWaiting`, and a `BuilderStopped` park whose stop reason was `Failed`, resume at `ResumeAt::Build`. The resume is set per park with `park_with_resume`. A `BuilderStopped` from `BudgetStop`, `TurnLimit` or `Stalled` gets no resume stage and stays as it is today (my call). `factory retry <id>` reruns the builder step with no note through the existing Build arm in `retry.rs`, using the per-path rerun rules above.
- `rework_hint` for these parks names `factory retry`.
- `factory rework` on these parks stays allowed without `--force` (my call), because real findings after a stop are a valid use. Today a park with a resume stage blocks rework in three places, and the plan must change all three for these two kinds:
  - plain rework is refused (`commands.rs` rework); allow it;
  - rework is dropped from `next` (`park.rs` `next_with_resume`); keep both commands;
  - `approve_refusal` reads every Build resume as a series wait (`board.rs` ~360-366); limit that refusal to `SeriesWaiting`.
- **Dashboard and API.** Today `park_resume === "build"` stands for series-waiting in two places: `projects.js` ~107, which offers only retry and cancel, and `board.js` ~42-43, which hides approve. Both must check `park_kind === "series-waiting"` instead. For machine parks the dashboard offers Retry, Rework and Cancel.
- **`machine_wait` on the dashboard.** The dashboard builds cards from the board file through `dashboard/src/model.rs`, in `--home` mode too (home_mode.rs ~344-356). The field goes into `model.rs` and the card shows it. The supervisor's `api/read.rs` field list (~139-143) gets `machine_wait` and `park_kind` as well, for API clients.

### A builder that changes nothing on a branch with work (#42)

This applies in both paths when the snapshot is `Snapshot::Nothing`, the builder reported done, and the job branch has commits above its base.

- **Ledger line:** `builder changed nothing; it says the work is already on the branch`. For a task it reads `task N changed nothing; ...`.
- **Whole-plan path:** go on to checks and review as if the builder had committed. The existing `nothing_changed_after_checks` path for checks-sourced fixes keeps its behaviour, and it gets the same whole-branch floor below.
- **Per-task path, last task:** continue to the diff cap check and review.
- **Per-task path, a middle task:** do everything a finished task does (advance `task_index`, set `builder_last`, write the task-done line, build.rs ~772-789), then go to the next task.
- **Every round counts, whoever started it.** The rule covers first runs, resumes, the user's reworks (077, 086) and the factory's own fix rounds from review findings. The user chose this one rule over parking automatic fix rounds that change nothing. In those rounds the builder is disagreeing with the reviewers, and the reviewers settle it.
- **The reviewers see the whole branch for that round.** The round diff is `prev_head..head` (review.rs ~70), which is empty when the head didn't move. A no-change round calls `finish_build` with no previous head, so `prev_head` is None and `round_base` is the whole base (review.rs ~69-70, build.rs ~874). `nothing_changed_after_checks` must do the same; today it passes `previous_head == head`, an empty diff (build.rs ~330). `prev_head` None also means round 1 and sync, so the review stage can't tell a no-change round from it. A new optional job field, `no_change_round: Option<u32>`, set by the build and read by the review stage (which may run in a separate process), marks it; it is cleared when the next build round starts. Decide, round counters and `fix_source` are unaffected. Rounds that changed something keep the round diff. The bigger diff can pick the large reviewer model (review.rs ~105-108). That is accepted, because it happens only on no-change rounds.
- **The reviewers get the builder's reason.** Today the re-review prompt already points at `builder-r{round}.md` when it exists (specs.rs ~656-661). But only the whole-plan fix path saves that file: new work doesn't save it (build.rs ~177-181), and `build_task` never calls `save_builder_reply`. The plan must:
  - save the builder's reply to `builder-r{round}.md` on every no-change run in both paths (per-task replies appended under a task heading);
  - point the reviewers at it on a no-change round, whether or not the round is a re-review;
  - for a no-change re-review, replace "Inspect the fix diff only" (specs.rs ~652) with wording that names the whole-branch diff and asks for a verdict on each earlier finding, given the builder's reply.
- **Every gate still runs:** checks, the diff cap (`DiffTooLarge`), the risky-diff rule and every review.
- **Repeat guard:** the existing cap (`fix_rounds`, or `risky_fix_rounds` for a risky diff; 2 in this project) ends a builder-reviewer disagreement with the usual `ChangesRemain` park. No new cap is added. In 88 jobs no automatic fix round changed nothing, so this path adds no cost today.
- With no commits above base, the job still parks `NothingChanged`.

## Constraints

- No new external crate.
- Only `git2` through `factory_git`; never shell out to git. Disk free goes through `fs2`.
- Board fields are added as optional, for compatibility with `factory-server`.
- Paths reach the driver only through `Layout`.
- Process checks go through `procinfo.rs` only.
- The engine reaches agents only through `AgentRunner`. Tests use the fake runner or `fake-claude`, which needs a way to fail with a given error text.
- A missing or broken config keeps today's strict behaviour. A missing `min_free_disk_mb` uses the default.
- `Config` and `project.toml` use `deny_unknown_fields`, so an older `factory` or `factoryd` rejects a `project.toml` that sets `min_free_disk_mb`. The release notes must say to upgrade both before setting it.
- cargo: `-j4`, never `--workspace`, never set `CARGO_TARGET_DIR`. Dashboard tests run with `node --test dashboard/tests/*.test.js`.
- Docs to update:
  - `docs/reference/software-factory.md`: parks, retry, waiting and the nothing-changed rule;
  - `docs/reference/project-toml-reference.md`: the new setting;
  - the user guide's park table.
- Close #42 with this work.

## Tests the plan must include

- `factory retry` and the automatic rerun after a Failed round-1 whole-plan build: the step starts the first-build spec, counts as new work, and doesn't fail with "missing findings file". A Failed escalated or fresh-fix round reruns as the same arm.
- A Failed run leaves `builder_id`, `builder_last` and `builder_session_model` as they were.
- child.rs: a Claude `is_error` result puts the CLI's result text into `error`. The test fake (`RoleFailing`, tests/common/mod.rs ~229) and fake-claude return the real shape (`is_error: true`, subtype `success`, text in `result`), so the classifier tests use what production sees.
- Per-task: a Machine failure of a resume waits before the fresh fallback; the fresh fallback is the only rerun.
- A disk-blocked fix round keeps its rework note for the rerun.
- A no-change review round sets and then clears `no_change_round`, and gets the whole-branch diff, also on the checks-sourced path.
- The rework note survives a Failed run, in both paths.
- A Failed `Other` run reruns once, then parks `BuilderStopped` with the error text and a Build resume.
- Budget stop holds: no probe or rerun starts once the budget is spent.
- A probe that did work counts as the rerun.
- Disk wait: the preflight fails, then the disk frees, and the builder starts with no park. With the disk still full after 25 minutes, the job parks `MachineWaiting`. The ledger being unwritable doesn't end the wait.
- Network wait through probe reruns. An Auth failure with a readable key file waits through probe reruns.
- `cancel --force` and lease loss each end a wait within one sleep step.
- `machine_wait` is cleared on wait end, on park, and at the start of the next Build pass.
- Classifier: positive cases per cause, and negative cases (builder prose with "timed out" or "401").
- Nothing-changed rounds:
  - a last task that changes nothing goes to review;
  - a middle task that changes nothing advances;
  - a first task with no commits above base still parks;
  - a whole-plan rework that changes nothing goes to review with the whole-branch diff and the builder's reply in the prompt, without "fix diff only";
  - the `fix_rounds` cap (and the risky cap) ends with `ChangesRemain`;
  - `DiffTooLarge` and the risky park still fire on the no-change path.
- Rework and approve on a `MachineWaiting` park: plain rework is accepted, `next` lists both retry and rework, and approve doesn't give the series-wait refusal.
- Dashboard: `jobActions` for a machine park includes retry, rework and cancel. A series-waiting park keeps its current actions. `machine_wait` shows in `--home` mode.

## Left to the builder

- The exact error patterns per cause.
- Where the shared wait helper lives, as long as both builder paths use it.
- How fake-claude is told to fail with a given error text.
