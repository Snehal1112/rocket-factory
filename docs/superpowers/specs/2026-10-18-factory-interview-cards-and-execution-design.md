# Factory interview, spec cards and execution rework

## Goal

Make the factory's planning phase thorough and visible, and its execution phase obey the superpowers skills it is built on, while keeping token cost low. Specs come from a full `superpowers:brainstorming` interview, plans from `superpowers:writing-plans`, and a spec card shows in the dashboard's Spec column from the first minute of the interview.

## Why

An audit of rocket-factory and Rocket's original `software-factory` worktree on 2026-10-17 found the following. The two share the same command text, builder agent and engine logic.

- The `/factory` command was written for superpowers 6.4.1. Version 7.0.0 is installed (since 2026-10-16) and removed the classification paths, "propose 2-3 approaches", the inline spec self-review and "2-5 minute steps with exact code". The command still asks for them, and `agent_lint.rs` pins that wording.
- The user found the interview weak: questions were open and hard to answer (for example "describe a real moment where you would set extends or a .env file"), so specs came out thin with gaps. Real specs lacked `## Non-goals`, and one recorded no answers at all.
- Plans ran 5 to 24 times longer than their specs, mostly code in fences. Only the home-dir plan used the 7.0.0 signature style.
- The driver loses builder output: a `BLOCKED:` reply is ignored and partial work is committed (`crates/factory-engine/src/build.rs`), builder rulings never reach the reviewer, there is no per-task check, and the reviewer gets no Review Focus.
- A card exists only after `factory new`, which needs a finished spec and plans, so the Spec column is empty during the interview.

## Decisions

Each entry says who decided. "User" means the user said it in the interview. "Call" means the author chose it and the user has not seen it until this document.

1. **Interview skill.** Use `superpowers:brainstorming` 7.0.0 as it is, not our own copy of its rules. Options were: keep 7.0.0, or go back to 6.4.1 where multiple choice was the default. User chose 7.0.0.
2. **Question style.** Every interview question carries a recommended answer plus a couple of alternatives. User, because open questions were too hard to answer. This overrides the skill's open-question default.
3. **Command layer.** Keep a thin layer in the command: the two headings the driver requires (`## Goal` with at least 8 words, `## Acceptance criteria`), and nothing that repeats the skill. Options were: a two-line note in the command, or no layer and loosen the driver. User accepted the two-line note.
4. **Document roles.** `superpowers:brainstorming` writes the spec (the design document). `superpowers:writing-plans` writes only the plans, in the 7.0.0 style. User.
5. **Execution shape.** The driver stays the controller and starts one fresh builder per task (variant A). Each builder follows the `subagent-driven-development` implementer rules, not the inline `executing-plans`. No per-task LLM review. The driver runs its cheap checks after each task and one review covers the whole branch. User.
6. **Variant B parked.** One controller session dispatching a subagent per task is not used now. A spike (tiny 3-task plan, one run each) measured A at about 73.5K weighted tokens ($0.25) and B at about 130K ($0.37). User chose A now and wants B explored in detail later, ideally on a real multi-task plan.
7. **Draft card from the start.** A card appears in the Spec column as soon as `/factory` starts, so the column is never empty. User.
8. **Draft cards are removed by hand.** No automatic removal. `factory cancel` removes a draft. User.
9. **Draft cards can be resumed.** User.
10. **What resume brings back.** The session saves each answered question, with its options and the choice, into the draft as the interview goes. A resumed session reads it and continues from the next open topic. Options were: stage and paths only, or Claude's own session resume. User chose the saved draft.
11. **Draft location.** The draft is the spec file itself at `docs/superpowers/specs/<date>-<slug>-design.md`, uncommitted until the user approves it. Option was a separate draft in the driver's state folder. The user confirmed this after the builder check, knowing that switching branch or stashing takes the draft with it.
12. **Interview check.** `factory new` shows a warning on the card when `## Decisions` holds no answer that came from the user. The job still runs. Options were: refuse the spec, or no check. The user confirmed the warning after the builder check.

13. **Per-job budget.** Tighten the default per-job budget from 3M to a lower value; the user suggested 1M or 2M and asked for a recommendation. Recommended and recorded as 1M weighted tokens (warn at 80 percent, stop at 100 percent as today): the largest measured single job was about 0.48M and a three-plan series about 0.95M across plans, so 1M leaves about twice the worst measured job for fix rounds and still stops a runaway early, while 2M would be four times the worst job. The user limited the choice to 1M or 2M and left the pick to the author, so the number is the author's call within the user's range.
14. **Plan lint, version pin and review prompts.** The user said yes to a sixth plan covering a plan lint, a skill-version test and the adoption of review skills (items 2 and 3 of the call list below). The user also asked whether more superpowers skills could improve quality and token use, and asked for them to be adopted where they fit.

## Calls made by the author

These are not agreed. Check them while reading.

- **One spec, ordered parts.** This is three related parts: driver fixes and execution, draft cards, command text. They stay in one spec so the interview and cards are designed together. Suggested plan order: driver fixes and execution first, cards second, command text last, so the new command text describes behavior that exists.
- **Scope.** Change the standalone rocket-factory only. That includes its in-repo `.claude/commands/factory.md` and `.claude/agents/factory-*.md`, which `agent_lint.rs` reads, and the presets and embedded command in `factory-core`. The copies in the real Rocket repo and in the old `software-factory` worktree stay as they are until a follow-up, because the standalone is the superset.
- **Draft card on the board.** Drafts live in a new optional top-level board field, `drafts[]`, not as `Job` records, so an old dashboard ignores them. `factory new` removes the draft entry and the existing series logic takes over, so a one-plan series still gets no spec card, as today. The Spec column shows drafts first.
- **Per-task checks are informational.** The user did not pick between this, an immediate fix of that task, and parking, so this recommendation stands until they say otherwise. After each task the driver runs the mapped checks on the working tree, before the `wip:` snapshot in `commit_batch` mode, and records the result against that task. A failure does not start a fix round or stop the job. The whole-branch checks stage still runs after the last task and still drives fix rounds, so round counts and budgets do not change.
- **`NEEDS_CONTEXT` parks like `BLOCKED`.** The job parks with the builder's text and no commit is made. `DONE_WITH_CONCERNS` is committed and the concerns are recorded and passed to the reviewer. A reply with no status line is treated as `DONE`, with a note on the card, so builder files in already seeded homes keep working.
- **After a park, the changes stay.** A parked task's uncommitted changes stay in the worktree for a human and are not reset, so no new git port method is needed. `factory rework` resumes the builder with the partial work present.
- **Seeded homes.** The builder and reviewer agent text changes in the embedded presets and in the in-repo copies. The plan must check how seeding treats existing files and add a one-time refresh that keeps a project's own overrides. Until a home is refreshed, the "no status line means DONE" rule above applies.
- **Cancel keeps the draft file.** `factory cancel` on a draft removes the card and leaves the draft spec file in place, because it is the user's work.
- **Stale is computed at read time.** The driver writes only an updated-at time on the draft. The dashboard and factoryd compute the stale marker against a threshold that is a setting of each (default 24 hours), so the dashboard does not need to read the repo's `config.toml`.
- **Card stages.** `interviewing`, `writing-spec`, `writing-plans`. Names are open to change.
- **Stale marker.** A draft card shows the time since its last update and is marked stale after 24 hours. The threshold is a config key with that default.
- **Builder statuses.** The driver reads `DONE`, `BLOCKED`, `NEEDS_CONTEXT` and `DONE_WITH_CONCERNS` from the builder's reply.
- **Token controls.** Add `--strict-mcp-config` and `--tools` to builder and reviewer runs. Give the reviewer a plan excerpt (header and task headings, the full path kept as a fallback) instead of only the path. Make the builder's `maxTurns` come from one place.
- **Plan lint (plan 6).** `factory new` refuses a plan that contains `TBD`, `TODO`, `FIXME` or `???` outside code, as it does for the spec. It warns, without refusing, about a missing header field (Goal, Architecture, Tech Stack, Spec, Global Constraints, Review Focus), a missing executor line naming `subagent-driven-development`, a task without `**Files:**` or `**Interfaces:**`, and a commit step. Warnings are printed after the `created job` line and stored on the first job.
- **Skill drift test (plan 6).** A test checks that key phrases the command and agents rely on still exist in the installed superpowers skill text, and records the version tested against (7.0.0). It is skipped, not failed, when no superpowers install is found, so it never breaks a machine without the plugin.
- **Review skills adopted as prompt text (plan 6).** The reviewer prompt takes the vision-document rule, the "Declined to judge" list, Critical, Important and Minor grading and the no-subagents rule from `requesting-code-review`. Fix-round reviews (round 2 onward) take the scoped re-review rule from `subagent-driven-development` (verdict each earlier finding, inspect only the fix diff, report out-of-scope notes without extending the loop). The resumed fix builder is told to follow `receiving-code-review` (verify each finding against the code, one item at a time, push back with reasons). Deferred minors are a follow-up.
- **Builder skill digest (plan 5).** The builder agent file carries a short digest of the test-first, debugging and verification rules, because builders were not loading those skills (no Skill events in jobs 012 to 020). The skill names stay in the file.

## Design

### 1. Interview and spec (command text)

The embedded global command (`crates/factory-core/assets/global-command.md`) step 1 becomes: invoke `superpowers:brainstorming` and follow it fully; give each question a recommended answer plus alternatives; save the spec where the driver expects it; the spec has `## Goal` and `## Acceptance criteria` headings as the driver requires; the 7.0.0 builder check runs before the user sees the document; no plan is written before the user approves the spec. The command stops repeating the 6.4.1 rules: classification, approaches, per-section approval, spec self-review, the coverage gate and the ban on design subagents (which blocks the builder check).

The opt-out offer in the first message of the skill stays. If the user takes it, the skill ends with no spec, so the command says to tell the user that `factory new` needs a spec and to ask whether to continue with the interview.

Step 2 invokes `superpowers:writing-plans` in the 7.0.0 style: signatures and named assertions instead of bodies, an Interfaces block per task, a File Structure map, the plan self-review (spec coverage, step scan, type consistency, Review Focus, proportion), no commit steps, and `Next plan:` as the last line. Every plan header carries an executor line saying the plan is executed by the factory driver using `superpowers:subagent-driven-development`: one fresh `factory-builder` implementer subagent per task, no per-task review, and the driver doing the ledger, commits and the final review. The skill's own execution handoff question stays skipped, because the driver is the executor. The plan names `subagent-driven-development` and never `executing-plans`. Review Focus is used as the skill defines it (input classes the spec implies and no task's tests exercise).

Rocket's copy and `agent_lint.rs` are updated in the same change that rewrites the embedded command, so the lint never pins removed wording.

### 2. Draft spec cards

- `factory spec start <slug> '<feature>'` creates a draft card at stage `spec` with a draft marker and the stage `interviewing`. The `/factory` session runs it first. Only the driver writes the board.
- `factory spec stage <slug> <stage>` moves the card through `writing-spec` and `writing-plans`.
- The session writes the draft spec file during the interview, adding to `## Decisions` as each question is answered. The card links to that file.
- `factory new` adopts the draft card as the series spec card and does not create a duplicate. After that the existing behavior applies: plan 1 starts at build, later plans wait in the Spec column.
- `factory cancel <slug>` removes a draft card. `factory spec resume <slug>` prints the draft file and its stage so a new session can continue from the next open topic.
- The board field is optional so the dashboard and existing boards keep working. The dashboard shows the card in the Spec column with its stage, the time since the last update and a stale marker.
- When `factory new` runs and `## Decisions` has no entry that came from the user, the card shows a warning. The job is not blocked.

### 3. Execution and driver fixes

- Plans are executed using `superpowers:subagent-driven-development`, with the driver as its controller: it dispatches one fresh implementer per task, handles the statuses and rulings, and owns the ledger, commits and the final review. The per-task review that skill describes is switched off.
- The builder agent file follows that skill's implementer rules (its implementer prompt, statuses and rulings format) and no longer loads `executing-plans`. The prompt the driver sends each builder also tells it that the plan is run through `subagent-driven-development`, so the plan's executor line and the agent agree. It keeps its test-first, debugging and verification skills.
- The driver reads the builder's status. `BLOCKED` parks the job with the builder's reason and does not commit partial work. `NEEDS_CONTEXT` and `DONE_WITH_CONCERNS` are recorded on the card and passed to the reviewer.
- The builder's rulings are written to the ledger by the driver from the builder's reply, in the skill's format (`Ruling: what, why, cost if wrong`), and passed to the reviewer.
- The driver runs its mapped checks after each task, so a failure is attributed to a task.
- The reviewer prompt includes the plan's Review Focus and the rulings.
- `parse_tasks` ignores fenced code, like `task_section` and `count_tasks`, and accepts only `### Task N:` headings.
- Agent `maxTurns` and the driver's turn cap come from one place.

### 4. Token controls

Builder and reviewer runs use `--strict-mcp-config` and `--tools` as the committer, message and KB runs already do. The reviewer gets a plan excerpt. Measured baseline for comparison: recent jobs cost about 0.2M to 0.46M weighted tokens, and a cold agent start is about 20K to 37K.

## Non-goals

- A review after every task. The user does not want it.
- Variant B (one controller session with subagents per task) in this work. It is parked for later.
- Changing the copies in the real Rocket repo or in the old worktree. The in-repo copies in this repository do change.
- Refusing specs because the interview was thin. Only a warning.
- Automatic removal of stale draft cards.
- Changing the merge rules or risk gates. Only the default per-job budget changes (decision 13). The daily budget stays off by default.
- Listing deferred minor findings for the human after an approved review.
- A superpowers skill replacing the committer's `1-git-commit` skill load.

## Anti-goals

- A spec that passes the driver's checks but was never discussed. The warning exists to catch this.
- A command that repeats skill text and goes stale at the next skill release.
- A draft card that nobody can remove or resume.

## Rejected alternatives

- **Go back to superpowers 6.4.1** for multiple choice questions. Rejected because 7.0.0 is installed and the preference is one line in the command or in `MY.md`.
- **One controller session with a subagent per task.** Parked after the spike measured it at about 70 percent more weighted tokens on a toy plan.
- **No command layer, loosen the driver.** Rejected because the driver's heading check also protects every job.
- **Separate draft in the driver's state folder.** Rejected because it adds a second copy that must be moved at approval.
- **Card only once the spec file exists.** Rejected because the interview itself would show nothing and the column would stay empty.
- **Refuse a spec with no user answers, or no check.** Rejected in favor of a warning.

## What is left to the builder

- Exact command names and flags for `factory spec`, as long as they follow the existing CLI style.
- The board field name and shape, as long as it is optional and old boards load.
- The dashboard's visual treatment of draft cards and the stale marker.
- Wording of the command text, as long as it keeps the points in Design 1.
- Where the draft file's answered-question format is parsed, as long as the user-answer test in Design 2 works on it.

The builder stops and asks the user for: anything that would let the driver write a board field an old dashboard cannot read, and any change to merge, risk or budget behavior.

## Behavior that must not change

- The board JSON stays compatible with `dashboard/`. New fields are optional.
- Only the driver writes the board, ledger and events.
- A missing or broken config makes every diff risky.
- `factory new` still validates the spec and the plan chain.
- Paths reach the driver only through `Layout`.

## Acceptance criteria

- Running `factory spec start demo 'a feature'` creates a card in the Spec column of `/api/state`, marked as a draft, with stage `interviewing`.
- `factory spec stage demo writing-plans` changes the card's stage and its last-updated time.
- `factory new` for the same slug adopts the draft card: the board has one spec card for the series and no duplicate.
- `factory cancel demo` removes a draft card, leaves other cards untouched, and leaves the draft spec file in place.
- A draft card with no update for 24 hours is reported as stale in `/api/state`, and the threshold is a setting of the dashboard and of factoryd.
- `factory spec resume demo` prints the draft file path and the stage, and the draft file keeps each answered question with its options and the user's choice.
- A spec whose `## Decisions` has no user answer produces a warning on the card, and the job still runs.
- Old boards without the new field load and render unchanged.
- The embedded command tells the session to write an executor line naming `superpowers:subagent-driven-development` in every plan header, and no plan or agent file the driver uses names `superpowers:executing-plans`.
- The builder agent file and the builder prompt name `superpowers:subagent-driven-development` as the way the plan runs, and a test pins both.
- The default per-job budget is 1M weighted tokens, and an existing config that sets its own value is unchanged.
- `factory new` refuses a plan containing a placeholder outside code, and prints a warning, still creating the job, for a missing header field, executor line, `**Files:**`, `**Interfaces:**` or a commit step.
- The skill drift test passes against superpowers 7.0.0 and is skipped when the plugin is not installed.
- The reviewer prompt contains the vision-document rule, a "Declined to judge" list and Critical, Important and Minor grading; a round 2 review prompt contains the scoped re-review rule; the fix-round resume prompt names `superpowers:receiving-code-review`.
- The builder agent file carries the test-first, debugging and verification digest, and a test pins it.
- A builder reply starting with `BLOCKED:` parks the job with that reason and no partial commit is made.
- A builder reply with rulings produces `Ruling:` lines in the ledger, and the reviewer prompt includes them and the plan's Review Focus.
- A failing check after task 2 is reported against task 2.
- A plan that quotes `### Task 4:` inside a code fence is counted as the number of real task headings.
- The builder and reviewer runs are started with `--strict-mcp-config` and `--tools`.
- The embedded global command contains none of the removed 6.4.1 rules, and `agent_lint.rs` passes against the updated Rocket copy.
- `cargo test -j4` passes for `factory-core`, `factory-engine`, `factory-cli` and `factory-supervisor`, and the dashboard tests pass.

## Follow-ups

- Explore variant B (controller session with subagents) on a real multi-task plan from the repo history.
- Apply the same command rewrite to Rocket's `.claude/commands/factory.md` and the old worktree.
- List deferred minor findings for the human after an approved review.
- Fix the leftover bare `cargo check -j4` wording in the `rust` preset and in Rocket's `project.toml`.
- Refresh `.claude/factory/config.toml` so it has `tickets_on_merge`.
