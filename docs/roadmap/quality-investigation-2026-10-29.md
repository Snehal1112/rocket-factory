# Code quality investigation (2026-10-29)

Why does the factory's output on Rocket need human follow-up fixes? Research over Rocket jobs 025-046 by four read-only agents: escaped defects, review to merge, builder inputs, check coverage. The `decide.rs` order and `spec_excerpt` were re-checked by hand at HEAD 3a246b4. The rest is a lead, not proof: re-verify against source before designing.

## Verdict

The factory's code is not clearly worse than interactive work on the same repo. The bigger problem is that when the reviewer finds a real problem, the pipeline almost never acts on it. Builders also start every task after the first with little context, and the checks have gaps.

- 4 of 17 merged jobs had a real escaped defect: 033, 034, 040, 041. The worst was secrets leaking into the collection export (033/034).
- Interactive commits in the same window (2026-10-17 to 10-29) have a fix-to-feat ratio of about 0.85, and real escapes of their own (6f14990a, 4a1f9faa, the #114 audit findings). The two sets are not directly comparable, because interactive review fixes land as separate commits.
- Many post-merge "fix" commits are not factory defects. da47ed6c, 2dd1bb0f and 3c2b2c50 are tickets #126-128 filed by job 046. a409082d (#125) fixes code from the interactive commit e8a44106.

## Causes, most important first

### 1. Reviewer findings rarely lead to a fix

- `crates/factory-engine/src/decide.rs:26` parks a risky diff before it looks at the budget, the diff cap, the checks or the verdict.
- Rocket's `risk_paths` match `src-tauri/**`, `crates/rocket-infra/**`, `**/*auth*`, `**/*secret*` and the Cargo files. 8 of 16 reviewed jobs parked as risky: 025, 026, 028, 030, 033, 034, 041, 043.
- In jobs 025-046 the reviewer started no fix round. The only fix round (041 round 2) came from failing checks.
- Jobs 034, 041 and 043 had a CHANGES verdict with a blocker. All three parked as risky and merged after `factory approve` with the blocker open:
  - 034: `Auth::None` exported as `noauth`. Fixed later by hand in 62ab16e0.
  - 041: formatting churn in `crates/rocket-app/src/lib.rs` and three other files outside the job.
  - 043: no test that a missing parent folder is an error. Still missing on main.
- The risky-diff park evidence (`park.rs:77-83`) is only "Why it stopped" plus the diff stat. `blocker_lines` is used only for ChangesRemain (`park.rs:66-75`).
- `factory approve` (`crates/factory-cli/src/commands.rs:599-656`) does not check the verdict.
- `factory rework` (`commands.rs:661-680`) passes only the human note and resets the round to 0, so the next review overwrites `review-code-r1.md`. The first 041 review was lost that way.

| Job | Verdict | Blockers, minors | Parked | Outcome |
|---|---|---|---|---|
| 025 | APPROVE | 0, 4 | risky | approved, minors open |
| 026 | APPROVE | 0, 3 | risky | approved, minors open |
| 027 | APPROVE | 0, 0 | no | auto-merged |
| 028 | APPROVE | 0, 3 | risky | approved, minors open |
| 029 | APPROVE | 0, 0 | no | auto-merged |
| 030 | APPROVE | 0, 2 | risky | approved, minors open |
| 031 | APPROVE | 0, 0 | no | auto-merged |
| 032 | APPROVE | 0, 1 | main busy | merged, minor open |
| 033 | APPROVE | 0, 8 | risky | approved, minors open (real UI bug, serde rule breach) |
| 034 | CHANGES | 1, 4 | risky | approved, blocker open |
| 035 | none | none | builder blocked | cancelled |
| 036-039 | none | none | none | cancelled, landed as direct commits |
| 040 | APPROVE | 0, 2 | no | auto-merged, minors open |
| 041 | CHANGES | 1, 1 | risky, 3 times | approved, blocker open |
| 042 | APPROVE | 0, 0 | no | auto-merged |
| 043 | CHANGES | 1, 4 | risky | approved, blocker open |
| 044 | APPROVE | 0, 0 | no | auto-merged |
| 045 | APPROVE | 0, 2 | no | auto-merged, minors open |
| 046 | APPROVE | 0, 0 | no | auto-merged |

### 2. Minor findings merge silently, and the reviewer under-grades

- An APPROVE with minors goes straight to AutoMerge (`decide.rs:60`). Nothing turns `[minor]` lines into tickets; `followups.rs` reads only the spec's `## Follow-ups`. 39 minors merged this way. Only the export ones were rescued by hand (#120, #121).
- The reviewer's blockers are real and it does not rubber-stamp. But it anchors on the plan's Review Focus and grades real problems as minor against its own rules (`~/.factory/projects/rocket/agents/factory-code-reviewer.md:29`):
  - 033 #1: the Close button leaves a stale report on reopen (UI bug).
  - 033 #3: blocking file I/O and ZIP work on the async runtime. Fixed by hand in 553a7488.
  - 033 #7: camelCase serde on non-IPC types, against an explicit `reviewer_rules` line.
  - 040 #2: clones on every send.
  - 045 #1: a missing test for a reachable non-HTTP clash.
- The reviewer did not look for credential leaks in 033, and did not check that the UI and backend halves of 040/041 agreed.
- Root cause found later the same day: Rocket ran its own agent files (`~/.factory/projects/rocket/agents/`, copied 2026-10-17), which override the preset and had drifted. The reviewer copy had no `Judging` section (so no "a rule broken or missing tests is `[blocker]`"), no `Declined to judge` list and no `factory-rules` marker; all four copies differed from the preset. On 2026-10-29 the reviewer, committer and KB-refresher overrides were removed and their Rocket-only lines moved into `project.toml` rules (backup in `agents-backup-2026-10-29/`). The builder override stays until the rust preset runs cargo per crate. Fix series: `docs/superpowers/plans/2026-10-29-act-on-review-findings/`, plan 05.

### 3. Builders get little context and start cold

- The builder prompt is `builder_task_spec` (`crates/factory-engine/src/specs.rs:297`, full branch :330-353). It holds the plan header, the task's own section, and `spec_excerpt` (`tasks.rs:262`): only the spec's Goal and Acceptance criteria. The Design, edge cases, Testing, Constraints, Decisions and Out of scope sections never reach the builder.
- Every task 2 and 3 in jobs 040-046 started a fresh builder, because the previous task ended over `builder_carry_max_ctx` 45000 (actual 47k-86k). The new builder sees only earlier commit subjects (`build.rs:605`), not the diff, notes or rulings.
- Builders explore little: 3-22 turns per task, none ran out of turns or context. Nothing points them to similar existing code or architecture docs. The KB is off for Rocket (`kb_path` unset).
- The agent file and the driver prompt contradict each other. The agent file says to use `superpowers:executing-plans` and not to re-read CLAUDE.md. The prompt says to follow the subagent-driven-development implementer rules and to read CLAUDE.md (`specs.rs:120`, :339). `maxTurns` is 150 in the agent file and 120 in `specs.rs:15`.
- The agent file has no rules marker (`slim_prompt`, `specs.rs:79`), so every prompt repeats the full rules.
- Builders skipped simplify in 040 T3, 042 T3 and 043 T1.
- Plans are fine (7.8-19.9 KB, anchors, signatures, test tables, not code dumps). They sometimes assume wrong things about existing code (040 T2 had to fix Task 1's dedupe; 043 T1 was not clippy-clean until T2).

### 4. The checks have gaps

The effective config is `~/.factory/projects/rocket/project.toml`. Its `[[settings.checks]]` replace the `rust` preset's checks. Rocket has no repo-level `.claude/factory/config.toml`.

| Area | Clippy | Build or check | Tests |
|---|---|---|---|
| 12 gated crates | yes | yes | whole crate |
| rocket-app, export, import, infra | no (Rocket #122) | yes | whole crate, not when only a dependency changed |
| src-tauri | no | workspace `cargo check -j4` | no |
| `src/**` frontend | n/a | `tsc` | vitest |
| Frontend test files | n/a | `tsc` only | vitest, excluded from biome |
| e2e, scripts, configs, lockfiles | none | none | none |

- Crates are matched by changed path only (`crates/factory-checks/src/plan.rs:54`, :68-98). A change in rocket-infra does not run rocket-app's tests.
- The no-checks gate (`checks_stage.rs:13-27`) fires only when no changed path has any check. A diff with one mapped path and several unmapped ones passes.
- Per-task checks are informational only (`build.rs:546-553`). Failing final checks do block the merge and re-run after every fix round.

### 5. Some defects started in the spec

- The 033 spec scrubbed known fields only, so secrets under names like `token` or `key`, OAuth URL variables and path params leaked. Fixed by hand in 61e380e0 and the export-scrub-fix branch (merged in 1d3cbb26).
- The 034 spec prescribed `None` → `noauth`, against Rocket's auth semantics.
- The board notes for 033 that no decision in the spec was answered by the user.

## Escaped defects

| Job | Fix commit | What | Category | Could have caught it |
|---|---|---|---|---|
| 022 | af106fbb | dotenvy errors quoted the failing line into the dialog and log | security | reviewer brief on leakage |
| 033 | 61e380e0, export-scrub-fix branch | credentials and secret variables in the Bruno ZIP | security, spec too narrow | spec, a secret-leak test |
| 033 | 553a7488 | blocking I/O on the async runtime, any path accepted | design, rule breach | reviewer (graded minor) |
| 033 | f5699000 | legacy `folder.yml` not converted for Bruno | logic bug | reviewer |
| 034 | 62ab16e0 | `Auth::None` exported as `noauth` | logic bug | reviewer (blocker, approved through) |
| 034 | 9c23d05d | Postman `url.query` empty | missing edge case | plan |
| 033/034 | 11a548ea | dialog over-promised the scrub | incomplete vs spec | reviewer |
| 033/034 | daf86069 | clippy in `postman.rs`, `tree.rs` | lint | clippy gate (#122) |
| 040/041 | bcdf452d | dialog rejected parent-only aliases, MCP read lost bindings on a cycle, weak tests | logic bug, tests | reviewer, spec sections in the builder prompt |
| 040/041 | 0290454d | cycle fallback not logged, fake empty `secretId` | design | reviewer minors |
| 043 | not fixed | missing-parent-folder test | missing test | reviewer (blocker, approved through) |

## Candidate work, by value per effort

1. In `decide()`, let a risky diff with CHANGES or failing checks run its fix rounds first, then park for the human. Show the verdict and blockers in the risky-diff park evidence.
2. Make `factory approve` refuse a CHANGES verdict without an explicit override. Make `factory rework` pass the reviewer's findings to the builder and keep earlier review files.
3. File `[minor]` findings as tickets after the merge. Tighten grading: a real bug, a rule breach or a missing test for a reachable case is a blocker. Add a leakage check to the reviewer brief for code that handles secrets.
4. Give builders the spec's Design and edge-case sections. Raise the carry cap to about 90k on Rocket, or pass the previous task's diff and notes to a fresh builder. Make the agent file and prompt agree and add the rules marker.
5. Close the check gaps: clear Rocket #122, add `src-tauri` tests, run tests for dependent crates, and park when changed code paths have no check.

## Related

- The quality levers eval (jobs 040-045 vs 025-031) found builder cost up 37% with quality flat. A re-measure was planned after five more Rocket jobs; only 046 has run.
- `roadmap-2026-10-18.md` item 1 (learning loop) could consume the minor findings from item 3 above.
