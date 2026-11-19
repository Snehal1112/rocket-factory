# Driver-written commit messages and one commit per job

Date: 2026-11-19. Status: draft for approval. Reviewed once by an adviser against HEAD 8dc6411.

## Intent

Two agents in every job only write commit messages: the **committer** (rewrites the driver's
snapshot commits into at most 4 commits per batch, using the `1-git-commit` skill) and the
**message** agent (drafts the merge and sync messages). Across all jobs they used 4.8M tokens,
13% of all job tokens, and most of each run is the cold start. This is the largest token saving
left (efficiency brief item 2, roadmap item 4).

The owner reads factory commits later in `git log` and in pull requests. What they need from a
commit is a detailed message, in the shape the `1-git-commit` skill produces, that says what the
commit carries and why, plus the ticket it fixes. They prefer one squashed commit per job.

So this change removes both agents and has the driver build one detailed commit per job on the
base branch, from text the builder writes while its context is already loaded.

## Goals

- No committer and no message agent runs in any job.
- The base branch gets exactly one commit per job, replacing today's no-ff merge commit and
  the branch history under it.
- That commit's message has the shape of a `1-git-commit` message: a subject of at most 50
  characters, a blank line, a why paragraph, sections of bullets for what changed, then footers.
- It carries `Fixes: #N` for the job's ticket, with today's series rule (below), and
  `Factory-Job: <id>`.
- The content that lands is exactly the reviewed content.

## Non-goals

- Per-task or per-fix-round commits on the base branch.
- Any change to how review, fix rounds, checks, approve, retry or sync decide things.
- Rewriting jobs that were already merged the old way.

## Anti-goals (a failure even if it works)

- The commit on the base branch has a different file tree from the head that was reviewed.
- A message problem (missing or malformed builder text) blocks or parks a merge.
- Generic `chore:` messages become the normal case instead of the fallback.
- Branch history is rewritten below a reviewed head.
- A commit made on the base branch by hand, or by another job, is silently undone.

## Constraints

- Git only through `factory_git` (git2), never the `git` binary.
- Board JSON stays compatible with the dashboard: new fields optional, old boards still load.
  `batch_snapshot` stays on the board as an optional field.
- Old `project.toml` files that set `commit_*` keys must still load.
- `factory tokens` must still read old usage records with the `committer` and `message` roles.

## Approaches considered

1. **Driver template only.** The driver writes every message from the plan's task titles and the
   changed file list. No tokens, but no "why": rejected, the owner wants detailed messages.
2. **Builder writes the text, driver commits (chosen).** The builder, which knows what it changed
   and why, ends its reply with a commit note. The driver checks it and builds the message. A few
   hundred output tokens per task, no cold start.
3. **Driver for task commits, one cheap agent for the merge message.** Keeps a cold start per
   job for prose the driver can assemble itself: rejected.
4. **Keep both agents on a cheaper model.** Saves 30-40% of the cost only: rejected.

For the squash, two shapes were considered:

- **Squash when the build ends, keep the no-ff merge.** Fix rounds would still land as extra
  commits on the base branch, next to a merge commit. Rejected: not one commit per job.
- **Squash at merge time (chosen).** The job branch keeps its working commits, so review and fix
  rounds work as today. At merge, the driver writes one new commit on the base branch whose
  parent is the base tip and whose tree is the reviewed head's tree.

For crash recovery after the squash, two ways were considered: a `merging_commit` board field
written before the base moves (chosen, explicit and cheap to test), or searching the base branch
for a commit with `Factory-Job: <id>` and the head's tree (no new field, but a history walk on
every settle). The tree check from the second is kept as a guard (section 6).

## Phases (owner's decision)

The work ships in two phases. Each is its own series of plans and is checked in real jobs before
the next starts.

- **Phase 1: messages and tokens.** Sections 1, 2, 4, 5 and 7, and the old-batch states in
  section 6. The merge stays today's no-ff merge, and its message is the section 4 message. This
  removes both agents, so it delivers the whole token saving. Until phase 2, the base branch also
  gets the job's `wip:` commits as second-parent history; `git log --first-parent` shows only the
  detailed commits.
- **Phase 2: the squash.** Section 3, and the `merging_commit` crash rule with its settle helper in
  section 6. This changes merge behavior and crash recovery, so it ships separately.

## Design

### 1. The builder's commit note

- Every preset builder file (`presets/*/agents/factory-builder.md`; not `presets/legacy/*`, which
  are the exact texts older versions seeded, so `refresh` can replace them) asks
  the builder to end its reply with a commit note between `---BEGIN COMMIT---` and
  `---END COMMIT---`:

  ```
  ---BEGIN COMMIT---
  Add a retry endpoint for parked jobs

  The dashboard had no way to continue a parked job.

  - Add POST /projects/{p}/jobs/{id}/retry with 409 refusals
  - Refuse approve while a retry is queued
  ---END COMMIT---
  ```

  The note comes after today's `Status:` line, rest of the reply, and `Ruling:` lines. The
  builder still never commits. The builder file line that says a committer agent writes the
  message (`factory-builder.md:52`) is replaced.
- **Parsing** lives in `factory-core/src/commitmsg.rs`. The markers are found in the raw reply
  text, the way `extract_message` finds its block, and the last BEGIN/END pair wins. A note inside
  a code fence is accepted: the fence lines are dropped from the note.
- `parse_builder_status` stops at the `---BEGIN COMMIT---` marker, so the note never becomes part
  of a DONE_WITH_CONCERNS text, with or without `Ruling:` lines.
- **Checks.** The subject is at most 50 characters, not empty, and has no trailing period. The
  body is not empty. The driver rewraps body lines to 72 characters itself rather than rejecting
  them; a wrapped bullet continues with a two-space hanging indent. A note that fails a check is
  replaced by the fallback below, and the ledger gets one line naming the failed check. It never
  parks and never asks the builder again.
- **Fallback.** Subject: the task title (cut at 50 characters on a word boundary). Body: one
  bullet per changed path from the task's diff, at most 20, then `- and N more files`.
- **Storage.** The driver writes each accepted or fallback note to the job folder:
  `commit-notes/task-<N>.md` for a plan task, `commit-notes/round-<R>.md` for a fix round or a
  rework round, `commit-notes/build.md` for a legacy job with no task list. A rerun of the same
  task or round overwrites its file. These files are the input to the final message.

### 2. Commits on the job branch

- After each task, the driver makes today's git2 snapshot commit, `wip: task <N> <title>`. After
  a fix round it makes `wip: fix round <R>`. These are working commits on the job branch only.
- The batch committer step goes away: no `reset_soft` to the base, no `resume_batch`. The head
  recorded by `finish_build` is the last snapshot. Review, fix-round floors (`prev_head`),
  `task_session` resume and the `BuilderCommitted` park keep working unchanged.
- **Fix rounds, rework rounds and legacy jobs.** Today these always run the committer through
  `commit_work`, even with `commit_batch` on (build.rs:180-207). They move to the same snapshot
  path, which keeps every branch `commit_work` has today:
  - the `head_is_on_branch` park (build.rs:647-660);
  - the empty-stage branches: `nothing_changed_after_checks` for check-started rounds, and the
    "builder made no changes" parks;
  - the `fix_source` handling.
- A board loaded with `batch_snapshot` set (a job stopped mid-batch by an older driver) is
  handled by section 6. The guards in retry, sync, admit and the CLI that refuse such a job stay
  until that field is cleared.
- Sync keeps merging the base into the job branch with a merge commit. Its message is the driver's
  fixed text, `chore(factory): sync <base> into job <id> <slug>`, plus `Relates to: #N` and
  `Factory-Job: <id>`. This commit never reaches the base branch as its own commit.

### 3. The squash at merge

- `merge.rs` keeps every check it makes today up to the merge itself: scope, base state, branch
  tip equals the reviewed head, merge lock, and the sync when the base moved.
- Instead of `merge_no_ff`, a new `factory_git` function writes the job commit, all under the
  merge lock:
  - It requires the current base tip to be an ancestor of the reviewed head (the sync before it
    guarantees this; otherwise it refuses, as `merge_no_ff` refuses today). This is the rule that
    keeps the squash from undoing newer commits on the base branch.
  - When the base tip's tree already equals the head's tree, it refuses with today's
    `UpToDate` result ("nothing to merge") and never makes an empty commit.
  - It creates one commit with parent = base tip, tree = the reviewed head's tree, and the message
    from section 4. Before moving the branch it checks that the new commit's tree id equals the
    head's tree id.
  - It moves the base branch with a compare-and-swap (`reference_matching` with the expected old
    tip), so a commit made on the base in that window makes the move fail instead of being
    dropped. A failed move is treated like today's "base moved" case: sync, review if needed,
    merge again. Main-busy and checkout rules stay as for `merge_no_ff`.
- The board records the new commit's sha in a new optional field, `merged_commit`, and keeps
  `head` as the reviewed head.
- A job branch's own commits are no longer ancestors of the base branch. Every place that decides
  "is this job merged" by ancestry must use the stage `merged` and `merged_commit` instead.
  Checked and fine as they are: series (each job starts from the base tip), KB refresh and tickets
  on merge (both use the base tip), the dashboard (reads no git), scope and checks (unmerged heads
  only). Today's `BaseState::Merged` crash path (merge.rs:304-307, 380-382) no longer fires for a
  squashed job; section 6 replaces it.
- **`factory cleanup`.** `remove_worktree` (factory-git worktree.rs:187-209) has one switch,
  `force`, which skips both the ancestry check and the dirty-worktree check, so it must not be
  used for this. factory-git gains a separate rule: a worktree and branch may be removed when the
  stage is `merged`, `merged_commit` is an ancestor of the base, and the branch tip equals
  `job.head`. The dirty and untracked check still applies. `is_merged` in `commands.rs:1109-1118`
  uses the same rule. A squash-merged job needs no `--discard-commits`.

### 4. The job commit message

```
<subject>

<why: the plan's Goal line>

<task 1 subject>:
- <task 1 bullet>
- <task 1 bullet>

<task 2 subject>:
- ...

Review fixes:
- <round 2 bullets>
- <round 3 bullets>

Fixes: #142
Factory-Job: 042
```

- **Subject:** the plan's own commit line (owner's decision, see section 5). Each plan of a series
  has its own, so every job's commit on the base branch has a distinct subject.
- **Why:** the `**Goal:**` line of the job's plan, wrapped at 72. No Goal line: the paragraph is
  left out.
- **Task sections:** one per `commit-notes/task-<N>.md`, in task order: the note's subject plus a
  colon, then its body's bullet lines. A note's prose lines before the bullets are left out, to
  keep the commit readable. A legacy job uses `build.md` as one section.
- **Review fixes:** the bullets of every `round-<R>.md`, in round order, under one heading. Left
  out when there were no fix rounds.
- **Length cap:** the body between the why paragraph and the footers holds at most 60 bullets. The
  rest is cut and replaced by one line, `- and N more changes`.
- **Jobs that predate notes** (in flight when the driver is upgraded, no `commit-notes/`): the
  sections come from the subjects of the job branch's commits above the base that are not `wip:`
  and not sync commits. Those are the old committer's messages.
- **Footers:** built by today's `driver_footers` rules, unchanged: `Fixes: #N` when the job
  closes its ticket (a single job, or the last plan of a series), otherwise `Relates to: #N`.
  No issues: no `Fixes` or `Relates to` line. Always `Factory-Job: <id>`.
- The message is checked with `validate_message`, plus a 50-character limit on this subject (the
  shared 72-character limit stays for other messages). It cannot fail the merge: if it fails, the
  driver merges with `fixed_message` plus the footers and writes a ledger line.
- Known limit, accepted: task bullets describe what each task did, so a later fix round that
  reverted part of it still shows the task's bullet next to the round's bullet.

### 5. The plan's commit line

- Every plan header gains one line next to `**Goal:**`:
  `**Commit:** feat(retry): retry a parked job from the dashboard`.
- It must pass the same rules as the job subject: Conventional Commits form `type(scope): text`
  or `type: text` with a type `validate_message` allows, at most 50 characters, no trailing
  period.
- **No board field.** The driver reads the line from the job folder's `plan.md` when it builds the
  message at merge. This covers every way a job gets its plan: `factory new`, series jobs created
  at run time (runloop.rs ~785-800), a `plan.md` placed by hand after `park_missing_plan`, and
  intake.
- **Lint.** A broken `**Commit:**` line is refused where plans are linted today (`planlint.rs:72`,
  called from `factory new` and at runloop.rs:381), with the rule it broke, and where intake
  adopts a draft. `factory-intake-drafter.md` in every preset writes the line.
- A plan with no `**Commit:**` line (older plans, hand-written plans) still runs. Its subject falls
  back to `feat: <plan title>`, where the plan title is the `# ...` heading without a trailing
  "Implementation Plan", lowercased first letter, cut to 50 characters on a word boundary. A job
  with no plan at all uses its feature line the same way, with issue refs and URLs removed. This
  fallback can mislabel a docs or fix plan as `feat:`; that is accepted for a fallback.
- **`--one-job`** (owner's decision). A spec header may carry its own `**Commit:**` line for the
  whole series, with the same rules. `combine_plans` writes that line into the combined plan's
  header, in place of the first plan's line, so the merge reads it like any other. A spec with no
  such line falls back to `feat: <spec title>`, built like the plan-title fallback. `--one-job`
  never refuses for a missing line.
- The `/factory` command text (`global-command.md` and the plugin copy), where it tells the
  planner how to write plans for the factory, says to add the `**Commit:**` line to every plan
  and gives the rules. `factory-cli/tests/global_command.rs` pins the instruction.

### 6. Crash safety and older jobs

- **A crash during the squash.** Before moving the base branch, the driver writes the new commit's
  sha to the board as `merging_commit`. After moving it, it writes `merged_commit` and the stage
  `merged`, and clears `merging_commit`.
- **One settle helper** handles a board with `merging_commit` set: if that sha is on the base
  branch, the job is finished as merged (no second commit); otherwise the field is cleared. Every
  write path runs it first, before its own checks: `try_merge` (before scope, `base_state` and
  the `tip != head` checks, merge.rs:300-327), `rework`, `sync`, `cancel`, `approve`, `retry`,
  supervisor admit and the dashboard retry. Without it a rework could sync the landed squash
  back in and land a second commit.
- The tree guard in section 3 is the second line of defense: a base tip whose tree equals the
  head's never gets another commit.
- **A job stopped mid-batch by an older driver** (`batch_snapshot` set on load):
  - Tip equals the snapshot: clear the field and continue to `finish_build` on that tip.
  - Tip equals the base, and the worktree's staged tree equals the snapshot's tree (the old
    committer reset the branch and stopped): move the branch back to the snapshot with git2 (the
    files do not change), clear the field, and continue.
  - The old committer finished (today's `batch_already_committed`, build.rs:760-763 and 801-816:
    the tip is above the base, has no `wip:` commits, and has the snapshot's tree): clear the
    field and continue to `finish_build` on that tip.
  - Any other state (the old committer made some commits and stopped): park with today's message
    and `git reset --soft <base7>` hint. The driver never guesses, so no work is lost.

### 7. What goes away

- The committer and message agent runs: `commit_step`, `committer_prompt`, `committer_spec`,
  `fallback_batch`, `message_step` and `message_spec`. The fixed messages and footer helpers stay.
- The `factory-committer.md` preset agent files and `commit-context.md`, and their entries in
  `preset.rs:25-181`, the preset manifest and refresh tests, and `resolve.rs:403`. Projects that
  still have copies keep them on disk. The driver no longer reads them.
- The committer and message roles in `codex_prep.rs` (64, 113, 179-214) and in `config.rs:315-324`
  (`plugins_for`, `skill_allow_for`).
- Config keys `commit_model`, `commit_subagents`, `commit_budget_weighted`, `commit_batch` and
  `commit_plugins` still parse, but are ignored, and the
  `commit_budget_weighted > 0` check (`config.rs:379-384`) goes. Each key set in a project file
  produces one warning line on stderr at the start of `factory run`, saying it has no effect.
  Other commands stay quiet, so Rocket's config and the test fixtures do not warn on every load.
- `ProbeRole::Committer` is removed from `factory probe`.
- `factory tokens` and `factory costs` keep showing old `committer` and `message` usage.
- Docs that name the old flow: the getting-started prerequisite for `dev-workflow-skills`
  (line 48), "no-ff merge" in CLAUDE.md's crate list, `project-toml-reference.md`,
  `software-factory.md`, README and the user guide. `agent_lint` runs after the builder file edits.

## Testing

- The commit note parser: an accepted note, a missing note, an empty body, a long subject, a
  trailing period, a note inside a code fence, two notes (the last wins), long body lines
  rewrapped with a hanging indent.
- A builder reply with a note but no `Ruling:` lines: the DONE_WITH_CONCERNS text does not contain
  the note.
- A three-task job with one fix round merges as exactly one new commit on the base branch. Its
  tree equals the reviewed head's tree, its parent is the old base tip, and its message has the
  subject, Goal, three sections, Review fixes and `Fixes` / `Factory-Job` footers.
- A fix round where the builder changed nothing still takes today's "no changes" path.
- A series: plan 1 of 2 says `Relates to: #N`, plan 2 says `Fixes: #N`.
- A job with no issues gets only `Factory-Job`.
- A builder reply with no commit note still merges, with the fallback section and a ledger line.
- A job from before the upgrade with old committer commits on its branch: the sections come from
  their subjects.
- The base moving between approve and merge: the sync runs, and the squash parent is the new base
  tip.
- A commit landing on the base between the ancestor check and the ref move: the move fails and
  nothing is lost.
- A base tip whose tree equals the head's tree: "nothing to merge", no empty commit.
- A board with `batch_snapshot` set from an older driver: each of the four states in section 6.
- A crash after moving the base branch but before the board update: the next run finishes the job
  as merged and adds no second commit; a `rework` in that state settles first and does the same.
- `factory new` refuses a plan whose `**Commit:**` line is too long, has no type or ends with a
  period, and accepts a plan with no `**Commit:**` line (fallback subject).
- A series of two plans with different `**Commit:**` lines lands two commits with those subjects.
- `--one-job` uses the spec's `**Commit:**` line, and `feat: <spec title>` when the spec has none.
- `factory cleanup` removes a squash-merged job's branch without `--discard-commits`, and still
  refuses one with uncommitted files.
- An old `project.toml` with `commit_model` loads; `factory run` warns once, `factory status` does
  not.
- No committer or message agent spec is ever built in any engine test.

## Left to the builder

- Exact names of new functions, the `merged_commit` field name if a better one fits the board,
  and how the warning line for ignored keys is worded.
- How fake runners in tests produce commit notes.
- The order of the docs updates.
- Splitting each phase into plans of at most 3 tasks each.
