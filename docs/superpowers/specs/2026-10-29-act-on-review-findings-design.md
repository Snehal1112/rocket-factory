# Act on review findings

## Goal

When the reviewer asks for changes, the factory fixes them before a human sees the job, and a human can merge an open blocker only on purpose, with a recorded reason that becomes a ticket. Risky diffs still always wait for a human.

## Why

Over Rocket jobs 025-046 and Rocket's older in-repo factory, no fix round was ever started by a review. Every fix round came from failing checks. Three jobs (034, 041, 043) merged with a reviewer blocker still open, and an older one (013) merged through a sync review that asked for changes. The 043 missing-folder test is still missing on main. The study is `docs/roadmap/quality-investigation-2026-10-29.md`, cause 1.

The causes, verified at HEAD 3a246b4:

| Problem | Where |
|---|---|
| A risky diff parks before the budget, the diff cap, the checks and the verdict are looked at. About half of Rocket's jobs are risky, so a CHANGES verdict on them never gets a fix round. | `crates/factory-engine/src/decide.rs:26` |
| The risky-diff park shows only the reason and the diff stat, not the verdict or the blockers. | `crates/factory-engine/src/park.rs:66-83` |
| `factory approve` and the supervisor's approve endpoint merge without looking at the verdict. | `crates/factory-cli/src/commands.rs:599-657`, `crates/factory-supervisor/src/api/write.rs:337-401` |
| `factory rework` gives the builder only the human note (one line, at most 200 characters), not the reviewer's findings. It sets `round = 0`, which the engine reads as round 1, so the next review overwrites `review-code-r1.md`, `builder-r1.md` is overwritten, the re-review has no earlier findings, and a full new set of fix rounds is granted. On 041 the builder answered "no changes" twice to "complete the code review". | `commands.rs:661-691`, `review.rs:101`, `build.rs:152`, `specs.rs:586-599` |
| A sync review reuses the round number and overwrites the round's review file (job 013). | `crates/factory-engine/src/sync.rs:77` |

Nothing in the original specs requires "risky first". Decision 6 of Rocket's v3 spec says a risky job goes to needs-human "after the reviewer runs", and decision 7 allows up to two fix rounds. The order came from the implementation.

## Goals

- A CHANGES verdict starts a fix round on every diff, risky or not, while fix rounds remain.
- Every risky-diff park shows the review first: verdict, open blockers, one line per round.
- Approve refuses an open CHANGES verdict unless the human gives a waiver reason. A waiver is recorded and each waived blocker becomes a ticket after the merge.
- Rework is a real next round: the builder gets the human note and the open findings, the re-review rules on the earlier findings, and no review or builder file is ever overwritten.
- A sync review never overwrites the round's review file.
- Every changed code path is covered by a check or the job parks, `src-tauri` and dependent crates get their own checks (section 11).
- A spec with a contradiction, a list-based security design or no user decision on risky code does not start a build without the user's say (section 12).
- Real problems are graded `[blocker]`, the remaining `[minor]` findings become a ticket, and a project's own agent files cannot fall behind the preset unnoticed (section 10).
- Every park says exactly why it stopped: no park has empty evidence, and the dashboard card shows a one-line summary of the evidence without opening the dialog (section 9).

## Non-goals

- Structured findings (a `findings.json` registry with ids and per-finding status). That is phase 2 and builds on this.
- Builder context and carry (cause 3).
- Changing what counts as risky, or Rocket's `risk_paths`.
- Changing the re-park of an approved risky job after a sync (`sync.rs:84-86`). No job has hit it.
- Changing what a Missing verdict does (it starts a fix round today and still will).

## Anti-goals

- A risky diff must never reach main without a human approving the exact final head after the last agent change. Every change here keeps that.
- A waiver must never be silent: no waiver without a reason, a ledger line, an event and a ticket attempt.
- No lost history: no file of an earlier round is overwritten.

## Design

### 1. Decision order

`decide()` checks in this order:

```
budget Stop            -> park BudgetStopped          (nothing more can run)
diff_lines > diff_cap  -> park DiffTooLarge           (a fix only adds lines)
used = fix_rounds_used(round, rework_base)
left = used < rounds_for(class)
!checks_ok             -> left ? FixRound : park ChecksFailed
Changes | Missing      -> left ? FixRound : park ChangesRemain
Approve                -> risky ? park RiskyDiff : AutoMerge
```

- `rounds_for(Risky)` is the new setting `risky_fix_rounds`, `rounds_for(Normal)` is `fix_rounds`.
- `fix_rounds_used(round, rework_base) = round - 1 - rework_base`, saturating at 0. `rework_base` is the new board field from section 4.
- `fix_or_park` (`checks_stage.rs:164`) uses the same two functions, so checks fix rounds and review fix rounds count against the same limit as today, and a risky job uses `risky_fix_rounds`.
- `DecideInput` gains `rework_base` and `risky_fix_rounds`.
- When a risky job runs out of rounds with CHANGES, it parks as `ChangesRemain` (the findings are the reason it stopped). Its evidence also lists the risky paths (section 2).
- A risky job with APPROVE runs the same final checks and gates as AutoMerge (`review.rs:177-200`) before it parks as `RiskyDiff`, so the human approves a job that has passed everything the driver can check. A final-check failure goes to `fix_or_park` as today.
- Escalation is unchanged: on Rocket the second fix round runs the builder on `escalation_model`, risky or not.

### 2. Park evidence

`decision_parts` for `RiskyDiff` and `ChangesRemain` returns these parts, in this order. `factory status` shows only the first 12 lines of the first part (`factory-cli/src/status.rs:5`, `:17`), so the review comes first.

1. **Review**: `verdict <Approve|Changes|Missing>, round N`, then the `[blocker]` lines of the latest review file (`blocker_lines`, `park.rs`). For an Approve, the verdict line alone.
2. **Risk** (risky jobs only): the `risk_summary` text, then `added by fix rounds: <paths>` when a fix round touched risky paths that the first review's diff did not. The first-round risky paths are recorded when round 1's review runs.
3. **Rounds**: one line per review that ran, read from the ledger's verdict lines: `round N (<source>): <verdict>, <B> blockers`. The source is `build`, `checks`, `review`, `rework` or `sync`.
4. **Changed files**: the diff stat, as today.

`ParkRecord` (`factory-core/src/park.rs`) gains two optional fields, written only when set: `verdict` (`approve`, `changes`, `missing`, from `last_review`) and `risky` (bool). These parks also carry the Review part first, and the Risk part plus `risky: true` when the diff is risky: `NothingChanged` after a review or rework round, and `NoChecks`/`NoTests` gate parks of a risky APPROVE (a risky job can now reach the gates before its `RiskyDiff` park).

### 2a. Re-review tags

The scoped re-review lists each earlier finding with a ruling. A finding it rules fixed must not count as open, or the blocker count, the waiver and the tickets would include it. The reviewer agent file is told: in a re-review, a finding ruled fixed is tagged `[fixed]`, one ruled not fixed keeps `[blocker]` or `[minor]`. `blocker_lines` counts only `[blocker]`. This changes `.claude/agents/factory-code-reviewer.md` and `crates/factory-core/presets/{rust,ruby,node,csharp,go}/agents/factory-code-reviewer.md`, and `factory-cli/tests/agent_lint.rs` pins the new line. Registered projects keep their own copy (for example `~/.factory/projects/rocket/agents/`); the release notes tell users to refresh it, and until they do, an untagged fixed finding is counted as open, which errs on the safe side.

### 3. Approve

**Where the verdict lives.** `reviews.code` cannot be the source: `fix_or_park` calls `reset_reviews()` (`checks_stage.rs:205-212`), so after any fix round it is Pending again, and parks that follow a review fix round (`NothingChanged`, `ChecksFailed`, `BuilderStopped`, `CommitFailed`, budget) would look like "review missing" and pass `--without-review`. So `Job` gains an optional `last_review` field, written by `review_stage` after every review (normal or sync) and never cleared by `reset_reviews`:

```json
"last_review": {"round": 2, "verdict": "changes", "blockers": 1, "file": "review-code-r2.md", "source": "review"}
```

`verdict` is `approve`, `changes` or `missing`. A review that approves replaces it, so "open changes" means `last_review.verdict == "changes"`.

**Rules**, in both entry points, the CLI `approve` (`commands.rs:599`) and the API `approve_inner` (`write.rs:337`):

| `last_review.verdict` | Plain approve | `--without-review` | `--waive "<reason>"` |
|---|---|---|---|
| none (no review ever ran) | as today: refused when `review_missing()` | allowed, as today | refused |
| `approve` | as today: allowed, or refused when a later fix round reset `reviews.code` (`review_missing()`), because the head moved after the review | as today | refused |
| `changes` | refused | refused | allowed |
| `missing` (the reviewer did not finish) | refused | allowed: this is a missing review, not open findings | refused |

- The refusal for `changes` names the blocker count and offers `factory rework`, `factory cancel` or `--waive "<reason>"`.
- The reason has the same rules as a rework note (one line, not empty, at most 200 characters, `validate_rework_note`). The API takes `{"waive_reason": "..."}` in the body, answers 409 with the refusal message when it is needed and missing, and the supervisor passes it to the child as `--waive`. `--waive` and `--without-review` do not combine.
- **What is waived:** the `[blocker]` lines of `last_review.file` (`blocker_lines`). When a CHANGES review has no tagged line, the whole reply is one blocker, so one ticket.
- A waived approve writes a ledger line `approved by the user, waived <B> blockers: <reason>`, the event `operator.approve` with `waived = true` and `waive_reason`, and appends one entry to `waivers.json` in the job folder. The file is a list, so a second waiver (for example after a sync review asks for changes again) never overwrites the first: `[{"head","round","file","reason","at","blockers":[text...],"tickets":[issue number or null...]}]`.
- After the merge, `merge.rs` files one ticket per waived blocker that has no issue number yet, through `Tracker::create`, next to the tickets step (`merge.rs:67-82`). Title: `Waived review finding from job <id>: <first 80 characters of the blocker>`. Body: the blocker text, the reason, the job id and the merge sha. It runs whether or not `tickets_on_merge` is set, because a waiver must not be lost. It is best effort: a failure is reported to the ledger, stderr and events and never changes the merge result. It is idempotent: blockers with an issue number are skipped. `Tracker::create` returns the number.
- `factory tickets <id>` also files the pending waiver tickets of a merged job, so a failed filing has a manual retry. `--dry-run` lists them.
- The park's `next` hint (`ParkKind::next_for`) offers `factory rework` and `factory approve <id> --waive "<reason>"` when `last_review.verdict` is `changes`.

### 4. Rework

`factory rework <id> "<note>"` becomes a real next round:

- The board starts every job at `round = 0` and the engine reads it as 1 (`ports.rs:111`). So: `new_round = max(round, 1) + 1` and `rework_base = new_round - 1`, so `fix_rounds_used` is 0 right after the rework. A rework grants a full new set of fix rounds, as today, because a human looked at the job.
- The driver writes `changes-r{new_round}.md`: the human note first, marked as winning on any conflict, then the open findings:
  - when `last_review.verdict` is `changes` or `missing`: the text of `last_review.file` (the latest review that ran, sync files included). This also covers parks after a review fix round, where the board's `reviews.code` was reset but the findings were never fixed (job 041);
  - else, when the park was `ChecksFailed`: the failing checks from `checks.md`;
  - else nothing more.
- `fix_source` is `rework`. A rework round that changes nothing parks as `NothingChanged` with the Review part, as a review fix round does.
- The build step resumes the builder with `changes-r{new_round}.md` the same way a fix round does (`build.rs:217-231`, `fixing` is true because `new_round > 1`), including the fresh fix builder rules.
- **The re-review** reads the latest review file that exists (`last_review.file`) instead of assuming `review-code-r{round-1}.md`, plus `builder-r{new_round}.md`, and rules on each earlier finding. This applies to every round after the first, not only reworks, so the scoped re-review in `specs.rs:583-606` no longer falls back to a full review when the round before ran no review. `prev_head` handling is unchanged.
- **A job parked mid-task** (inside `build_task`, before any review ran) keeps today's path: `rework-note.md`, which `build_task` reads (`build.rs:607`), and no change to `round` or `rework_base`, because no fix round was spent. The CLI asks the engine whether the job is mid-task (the same test `build_stage` uses to choose `build_task`) and does not parse the plan itself.
- The CLI result line says `job <id> back to build, round <N>`.
- The API rework endpoint keeps its contract; it already calls the CLI.

### 5. Sync review files

A review run by `sync_stage` saves to `review-code-r{round}-sync{n}.md`, where `n` counts the sync reviews of that round from 1. `review_stage` gets the file name from its caller; the normal path keeps `review-code-r{round}.md`. The verdict ledger line says `sync review round N verdict X`. The latest-review lookup for park evidence and rework takes the sync file when it is newer.

### 6. Settings

`risky_fix_rounds` (u32) in `[settings]`, default equal to `fix_rounds`. It bounds every fix round of a risky diff, from failing checks or from the review. 0 parks a risky diff at its first failing check or `CHANGES` verdict, with no fix round; the default keeps today's behaviour for failing checks and adds review fix rounds. It is validated like `fix_rounds` and documented in `docs/reference/project-toml-reference.md`.

### 7. Board and dashboard

- `Job` gains `rework_base: u32` (`#[serde(default, skip_serializing_if = "is_zero")]`) and `last_review: Option<LastReview>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`), so old boards load and the dashboard (which ignores unknown fields) is not affected. The dashboard reads `last_review.verdict` for the approve form below.
- No new `Stage`: the dashboard renders an unknown stage as "spec" (`dashboard/static/board.js:579`).
- The dashboard's approve button posts an empty body (`app.js:452`). On a 409 it shows the refusal and a one-line reason field, then posts `{"waive_reason": ...}`. `approveCommand` (`board.js:30`) shows the `--waive` form when the job's code review is `changes`.
- `MAX_ROUNDS = 2` (`board.js:33`) is wrong once rounds can exceed it; the round pips use the board's `round` instead of a constant.
- `dashboard/src/park.rs` reads the two new optional `ParkRecord` fields with `#[serde(default)]`.

### 8. Events and ledger

- The `decide` event (`review.rs:138`) gains `verdict` and `risky`.
- The verdict ledger line gains the round source and the blocker count: `review round N (review) verdict Changes, 2 blockers`. The source is the job's `fix_source` when the review ran (`build` when none, `sync` for a sync review). The Rounds part in section 2 reads these lines; older lines without a source or count show `?`.

### 9. Park reasons on every park and on the card

**Problem.** The card shows only the board's one-line `reason` plus a "See what went wrong" button (`dashboard/static/board.js:577-583`). The reason is often not the real cause: most Rocket parks read `risky diff: risky paths (N): ...` while the real issue was a reviewer blocker. And many parks record no evidence, so the button opens an empty page: `decision_parts` ends with `_ => Vec::new()` (`factory-engine/src/park.rs`), nine `CommitFailed`/`BuilderCommitted`/`BuilderTampered` parks pass `Vec::new()`, and `merge::park` (`merge.rs:23`) always writes kind `Other` with no parts. Seen on real jobs: `main busy: the main checkout has tracked changes` (19 times), `branch tip … differs from the reviewed head …` (4), `builder made no changes on resume` with no reply (5), `no plan.md in the job folder` (2).

**Evidence for every park.**
- `park_with` never writes an empty `parts` list: when a caller passes none, it adds `Why it stopped` with the reason. So `CommitFailed`, `BuilderCommitted` and `BuilderTampered` always have one part.
- `NothingChanged` with an empty builder reply gets a `Builder reply` part saying `The builder wrote no reply.`
- `merge::park` takes a kind and parts: `pub fn park_with_parts(driver, id, reason, kind, parts)`; `park(driver, id, reason)` stays as a wrapper with `Other` and the `Why it stopped` part. The merge step passes:
  - main busy: `factory-git` `Preflight::Busy` gains `paths: Vec<String>` (the tracked changed paths, at most 50, from the same `statuses` call). Part `Main checkout` lists them and says `Commit or stash these in the main checkout, then run factory approve <id> again.`
  - branch tip differs from the reviewed head: part `Branch moved` with both shas and `Something was committed on the job branch after the review. Run factory rework <id> <note> so it is reviewed, or reset the branch to the reviewed head.`
  - merge failed: part `Merge error` with the error text.
- The run loop's `no plan.md in the job folder` park names the expected path; `run loop did not settle` carries the last 20 lines of `progress.md`.

**A summary on the card.**
- `Job` gains `park_summary: Option<String>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`), written by `park_with` and `merge::park`, and cleared whenever the job leaves `needs-human` (where `reason` is cleared today).
- The summary is the first two non-empty lines of the first part joined by ` · `, else the reason, folded to one line and capped at 160 characters. With the section 2 order a review park reads, for example, `verdict changes, round 2 · [blocker] no test that a missing parent folder is an error`.
- The dashboard (`model.rs`, lenient) reads `park_summary`. A needs-human card shows the park-kind chip (`parkKindLabel`, already in `board.js`), then the summary in place of the bare reason, then the reason in muted text when it differs, then the existing button and copy chips. A job parked before this change has no summary and shows the reason, as today.
- The board still carries no evidence text beyond this one capped line; the dialog keeps the full parts.

### 10. Grading, minor findings and agent drift

**Problem.** Over Rocket jobs 025-046, 39 `[minor]` findings merged with no ticket, and several were real: a stale report on reopen (033), blocking file I/O and ZIP work on the async runtime (033), camelCase serde on non-IPC types against an explicit rule (033), clones on every send (040), a missing test for a reachable clash (045). The main cause found on 2026-10-29: Rocket ran its own agent files from `~/.factory/projects/rocket/agents/`, copied on 2026-10-17 and never refreshed. The reviewer copy had no `Judging` section at all (no "a rule broken or missing tests is `[blocker]`"), no `Declined to judge` list and no `factory-rules` marker. Nothing warns when a project's own agent file falls behind the preset.

**Done already (operations, not code).** On 2026-10-29 Rocket's reviewer, committer and KB-refresher overrides were removed (backup in `~/.factory/projects/rocket/agents-backup-2026-10-29/`), and their Rocket-only lines moved into `reviewer_rules` and `builder_rules` in Rocket's `project.toml`. The builder override stays until the rust preset's builder runs cargo per crate (below), because a bare `cargo check -j4` on Rocket builds the whole app and V8 and stalls.

**Agent drift warning.**
- A project agent file overrides the preset file of the same name (`resolve.rs`, `merged_agents`). Resolve now compares each override with the preset file it replaces and reports drift when the override lacks the preset's `<!-- factory-rules: N -->` marker (or has a lower N), or lacks a `## ` heading the preset has.
- Drift is a note, not a problem: the project still resolves. Resolve notes are already shown by `factory project show` and printed to stderr by every command (`factory-cli/src/main.rs:70`), so the line `agent drift: factory-code-reviewer.md lacks the rules marker, the section Judging and the section Stack; compare it with the rust preset or remove the override` appears there.

**A sharper grading rubric.** Every reviewer file (the repo copy and the five presets) gets, in `## Judging`:
- These are always `[blocker]`: a bug a user can hit, however small; a breach of any rule in the job prompt or the project rules; a missing test for an edge case or error path the code makes reachable; data loss or a leak of a secret.
- Before the verdict, re-read every `[minor]` against that list and change it to `[blocker]` when it matches.
- The rust preset's reviewer `## Stack` adds: blocking file, network or CPU-heavy work on an async runtime path is a `[blocker]`.

**The rust preset builder runs cargo per crate.** Its `## Stack` line becomes `Run cargo check -j4 -p <crate> and cargo test -j4 -p <crate> <test_name> for each crate you touched`, so a large workspace never runs a full check. After this ships and is installed, Rocket's builder override is removed the same way (operations).

**Minor findings become tickets.**
- After a merge, the driver files one ticket per job that lists the open `[minor]` lines of `last_review.file` (lines with `[minor]` and without `[fixed]`), as a checklist. Title: `Minor review findings from job <id>: <slug>`. Body: the checklist, the review file name, the job id and the merge sha. No ticket when there are none.
- It goes through the `Tracker` port next to the waiver tickets, best effort and idempotent (`minors.json` in the job folder holds the issue number). `factory tickets <id>` retries it.
- New setting `minor_tickets` (bool, default `true`). Rocket keeps the default.

**A replay check (manual, part of the plan's verification).** Run the new preset reviewer on the saved round diffs of jobs 033, 040 and 045 (`review-*.diff` in their job folders) and confirm that the five findings listed above now come back as `[blocker]`. This is a one-off measurement, not a test in CI.

### 11. Check coverage

**Problem** (study, cause 4). `plan_checks` (`crates/factory-checks/src/plan.rs`) expands `{crate}` from the folder after `crates/` only, so `src-tauri` (package `rocket`) never gets per-package commands. A change reaches only the changed crate's tests, never its dependents (17 Rocket packages depend on `rocket-shared`). `no_checks_reason` (`factory-engine/src/checks_stage.rs:13-27`) fires only when no check ran at all, and returns `None` for every risky diff. With an empty `code_paths`, every non-doc path counts as code (`factory-core/src/gate.rs:37`), so lockfiles, `package.json`, scripts and e2e files are code that no Rocket rule covers.

**Package-aware `{crate}`.**
- `factory-checks` loads a workspace map from `cargo metadata --no-deps --format-version 1`, run in the worktree when a rule uses `{crate}` or `{dependents}` (running cargo is allowed; only git must not be shelled out to). Each package has a name, its directory relative to the repo root, and its path dependencies on other workspace members.
- `{crate}` expands to the names of the packages that own the changed paths matched by the rule, by longest directory prefix. `src-tauri/src/lib.rs` gives `rocket`.
- When `cargo metadata` fails or there is no `Cargo.toml`, `{crate}` falls back to today's folder-after-`crates/` rule and `checks.md` gets the line `NOTE workspace map unavailable: <error>`.

**Dependents.**
- A new placeholder `{dependents}` expands to the direct workspace dependents of the `{crate}` packages, not already in `{crate}`, sorted by name, at most `dependents_cap` (new setting, default 6). The rest are listed in `checks.md` as `SKIP dependents over the cap: a, b`.
- It is opt-in per rule: a project adds a command such as `cargo test -j4 -p {dependents}`.

**A stricter no-checks gate.**
- The gate parks `NoChecks` when any changed code path matched no `[[checks]]` rule, not only when the whole report is empty. The reason is `no check covers N changed code paths: a, b (+k more)`. Paths that are not code (docs) are never counted.
- The gate applies to risky diffs too (remove the risky exemption at `checks_stage.rs:18`). With section 1, a risky diff the reviewer approved meets the gate in `final_gate` before its `RiskyDiff` park.
- A project can list paths that need no check in a new setting `unchecked_paths` (globs, default empty), for files such as `LICENSE` or `.gitignore`.

**Rocket (operations, after the code ships).**
- New rules in Rocket's `project.toml`: `Cargo.toml` and `Cargo.lock` at the root run `cargo check -j4 -p rocket` (slow, but dependency changes are rare); `package.json`, `yarn.lock` and `scripts/**` run `yarn tsc --noEmit` and `yarn check`; `e2e/**` runs `yarn tsc --noEmit`; `src-tauri/**` runs `cargo test -j4 -p {crate}`; `crates/*/**` adds `cargo test -j4 -p {dependents}`.
- Clear Rocket issue #122 (clippy errors in rocket-app, rocket-export, rocket-import, rocket-infra) as a Rocket factory job, then add those four crates and `src-tauri` to the clippy rule.

### 12. Spec review before the build

**Problem** (study, cause 5). Job 033's spec scrubbed a fixed list of known secret fields, so secrets under other names leaked, and it prescribed `None` → `noauth` against Rocket's auth semantics. No decision in it was answered by the user. Today the driver only warns (`NO_USER_DECISION`, `factory-engine/src/runloop.rs:183`, `:475`; `one_job.rs:116`) and builds anyway.

**The user's decision (2026-10-30): `factory new` stops when the spec check finds a problem.** The user fixes the spec or overrides with a reason.

**Two checks, run by `factory new` before any job is created:**
1. **User decisions on risky specs (no agent).** When any path named in the plans' `**Files:**` blocks matches `risk_paths`, the spec must have at least one `Decided by: user` decision. Otherwise `factory new` refuses: `the spec touches risky paths (<up to 5>) and no decision in it was answered by the user`.
2. **The spec reviewer (agent).** A new read-only agent `factory-spec-reviewer` (tools `Read, Grep, Glob`) runs in the main checkout with the spec and the plans. It reports only:
   - a decision that changes or contradicts existing behaviour, with the `path:line` of the code it contradicts;
   - a security or data-handling design (scrubbing, redaction, permissions, trust, auth) that relies on a list of known names or cases where a default-deny rule is needed;
   - a design decision in a risky area marked `Decided by: call`.

   Findings use `N. [blocker] <spec heading> - problem - evidence - fix` (or `[minor]`), and the last line is `VERDICT: APPROVE` or `VERDICT: CHANGES`. A CHANGES verdict, or a missing verdict, makes `factory new` refuse.
- The model is `reviewer_model_large`. A new setting `spec_review` (bool, default `true`) turns the agent off; check 1 always runs.
- The review is saved to `<state>/spec-reviews/<sha256 of spec and plans>.md`, and its path is printed. When the same spec and plans already have an APPROVE review on file, the agent is not run again.

**Refusal and override.**
- On a refusal `factory new` prints the reason or the findings and the review file path, creates no job and exits non-zero.
- `factory new ... --accept-spec "<reason>"` (one line, at most 200 characters) creates the jobs anyway. The reason goes to the first job's ledger (`spec accepted by the user: <reason>`) and to its `spec_warning`.
- The interview command (`factory-core/assets/global-command.md`, step 4) says: when `factory new` refuses, show the user the findings, fix the spec with them (asking where a finding needs their decision), and run `factory new` again; use `--accept-spec` only when the user says so.
- The supervisor's create endpoint, which runs `factory new` as a child, returns the refusal text with status 409.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| Risky job, CHANGES, rounds left | Fix round. Escalation as configured. |
| Risky job, CHANGES, no rounds left | Park `ChangesRemain` with Review, Risk, Rounds and Changed files. |
| Risky job, APPROVE, final checks pass | Park `RiskyDiff`, Review first. |
| Risky job, APPROVE, final checks fail | `fix_or_park` with the check failures, as AutoMerge does today. |
| Risky job, budget Stop or over the diff cap | `BudgetStopped` or `DiffTooLarge`, not `RiskyDiff`. The evidence still lists the risk. |
| `risky_fix_rounds = 0` | Risky CHANGES parks `ChangesRemain` at once; risky APPROVE parks `RiskyDiff`. |
| Fix round touches new risky paths | Listed under "added by fix rounds" in the Risk part. |
| Builder pushes back and changes nothing | `NothingChanged` park with the Review part. |
| Review file missing or unreadable | The Review part says `verdict <X>, findings file missing`; approve still reads the verdict from the board. |
| Approve on CHANGES without `--waive` | Refused, CLI and API (409). Nothing changes. |
| `--without-review` after a review asked for changes and a fix round reset `reviews.code` | Refused: `last_review` is `changes`, so only `--waive` merges it. |
| Approve after a Missing verdict | `--without-review`, as for any missing review. |
| `--waive` without an open CHANGES | Refused. |
| CHANGES with no `[blocker]` line | The whole reply is one waived blocker, one ticket. |
| Re-review rules a finding fixed | Tagged `[fixed]`, not counted. |
| Waiver ticket filing fails or no tracker | Reported to ledger, stderr and events; merge stands; `factory tickets <id>` files the pending ones later. |
| Second waiver after a sync review asks for changes again | Appended to `waivers.json`; both sets of blockers get tickets. |
| Rework after `ChecksFailed` | Note plus the failing checks. |
| Rework after a park that followed a review fix round (`NothingChanged`, 041) | Note plus `last_review.file`, so the builder finally sees the findings. |
| Rework of a job parked in round 1 (board `round = 0`) | `new_round = 2`, `rework_base = 1`; `review-code-r1.md` is kept. |
| Rework of a job parked mid-task | `rework-note.md`, round unchanged, as today. |
| Board written before this change (no `last_review`) | Approve behaves as today; the first new review writes the field. |
| A park with no evidence passed by its caller | `Why it stopped` with the reason; never an empty dialog. |
| Main busy with hundreds of changed files | The first 50 paths and `(+N more)`. |
| Job leaves needs-human (approve, rework, cancel) | `park_summary` is cleared with `reason`. |
| Board written before this change | No `park_summary`; the card shows the reason as today. |
| Sync review asks for changes | Parks as today ("sync review asked for changes"), file `review-code-rN-sync1.md`, approve then needs `--waive`. |

## Testing

- `decide` unit tests: risky + Changes with rounds left gives FixRound; without rounds gives ChangesRemain; risky + Approve gives RiskyDiff; risky + budget Stop gives BudgetStopped; risky + over cap gives DiffTooLarge; `risky_fix_rounds = 0` parks risky Changes and risky failing checks at once; `rework_base` resets the count.
- Engine tests with the fake reviewer returning CHANGES then APPROVE on a risky fixture: one fix round runs, the job parks `RiskyDiff`, `review-code-r1.md` and `-r2.md` both exist, the park's first part is the Review.
- Final checks run before a risky APPROVE park; a failure starts a fix round.
- Approve: CLI and API refuse CHANGES without a waiver; a waiver writes the ledger line, the event and `waivers.json`; tickets are filed once per blocker through a fake `Tracker`, and a rerun files none; a tracker error leaves the merge in place.
- Rework: keeps `review-code-r1.md` and `builder-r1.md`, writes `changes-r2.md` with the note first and the findings after, the re-review prompt names `review-code-r1.md`.
- Sync: the sync review writes `review-code-rN-sync1.md` and `review-code-rN.md` is unchanged.
- Old board JSON without `rework_base` loads; a board written by the new code loads in the dashboard.
- Tests that pin today's behaviour change with it: `factory-engine/tests/review_decide.rs`, the `park.rs` tests of `next`/`next_for`, the rework tests that set `round = 0` (`build_checks.rs`, `escalation.rs`, `fix_fresh.rs`, `task_carry.rs`, `new_to_merged.rs`, `operator_events.rs`), the supervisor approve tests (`api/write.rs`, `request.rs`), `dashboard/tests/board.test.js`, and `agent_lint.rs` if `global-command.md` changes.

## Docs

`docs/reference/software-factory.md` (merge rules and the pipeline diagram), `docs/reference/project-toml-reference.md` (`risky_fix_rounds`), `docs/guides/factory-user-guide.md` and `docs/reference/factory-admin-manual.md` (`approve --waive`, rework behaviour), `factory-core/assets/global-command.md` where it describes approve and rework.

## Cost

Measured on Rocket's `usage.jsonl`, one fix round costs about 200-370k weighted tokens (builder 131-265k, committer 33-42k, Opus reviewer 44-60k). Replayed over jobs 025-046, the change adds about -0.06M to +0.29M on 8.06M, roughly -1% to +4%. Job 041 gets cheaper, because its three human reworks cost 1.16M.

## Decisions

Made with the user on 2026-10-29: move the risk gate after the fix rounds; show the review first in the park; approve refuses an open CHANGES unless waived with a reason that becomes a ticket; rework carries the findings as a real next round; sync reviews get their own file; a `risky_fix_rounds` setting.

My calls, for the user to check:
- `risky_fix_rounds` defaults to `fix_rounds`, so the change is on for every project.
- A risky APPROVE runs the final checks and gates before it parks.
- Waiver tickets are filed even when `tickets_on_merge` is false.
- A rework still grants a full new set of fix rounds.
- The re-park after a sync and the Missing-verdict behaviour stay out of scope.
- The open verdict lives in a new board field `last_review`, because fix rounds reset `reviews.code`. `--without-review` is refused when the last review asked for changes.
- A Missing verdict is approved with `--without-review`, not `--waive`.
- `waivers.json` is an append-only list, and `factory tickets <id>` files pending waiver tickets.
- A mid-task rework keeps today's `rework-note.md` path.
- Re-reviews tag fixed findings `[fixed]`; the reviewer agent files and `agent_lint` change for that.
- Sections 11 and 12 were added at the user's request on 2026-10-30. The user chose that `factory new` stops on a spec problem (with `--accept-spec` as the override). My calls: `{dependents}` is direct dependents only with a cap of 6; the stricter gate applies to risky diffs too; the spec reviewer runs on Opus and is skipped for an unchanged, already approved spec; Rocket's check rules and #122 are operations after the code ships.
- Section 10 (grading, minor tickets, agent drift) was added at the user's request on 2026-10-29; the user chose to drop Rocket's agent overrides rather than refresh them. My calls: one ticket per job for minors (not one per finding), `minor_tickets` on by default, drift is a warning not a refusal, the builder override waits for the per-crate preset.
- Section 9 (park reasons) was added at the user's request on 2026-10-29, after they reported that a parked card does not show why it parked. The summary is two lines of evidence capped at 160 characters, so the board stays free of full evidence text.
