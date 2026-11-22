# Software Factory

Status: the standalone Rust driver. Design: `docs/superpowers/specs/2026-10-16-standalone-software-factory-design.md`. v1 (LLM orchestrator, Python helper) and the v2 plan series are superseded. Checked against the code on 2026-11-22 (commit `3939891`).

The Rust driver does everything mechanical: state, checks, diffs, merge, sync, series, budget and events. The LLM is used only where judgment is needed: design and planning with the user, one fresh builder per plan task (one whole-plan builder when a plan has no task headings), the code reviewer and, when enabled, the security reviewer per round, plus the spec reviewer at `factory new`, the KB refresher after a merge and the intake drafter for issues.

## Pipeline

```
/factory <feature>  (interactive, with the user)
  factory spec start <slug> '<feature>' -> draft card in the Spec column (factory spec stage moves it)
  superpowers:brainstorming (full interview) -> docs/superpowers/specs/<date>-<slug>-design.md, approved, committed
  superpowers:writing-plans -> docs/superpowers/plans/<date>-<slug>/plan-01-<slug>.md ... (at most 3 tasks each, Next plan: line), approved, committed
  factory new <slug> '<feature>' --plans <plans folder> --spec <spec file>  (spec check: decisions + spec reviewer)
  then factoryd enqueue --project <p> --job <id> --series when factoryd runs,
  else factory --project <p> run <id> --series  (background Bash, session idle)

factory run (no LLM tokens):
  worktree -> builder (claude -p, Sonnet) -> checks -> reviewer (claude -p, Sonnet)
                 ^                              |  red: resume builder with trimmed output
                 |  CHANGES, at most 2 rounds   |
                 +------------------------------+
  main moved -> merge main in, re-run checks and a sync review (review-code-r<N>-sync<K>.md, at most 2) -> merge
  APPROVE + no risk + under size cap + budget ok -> merge reviewed sha to local main
    -> kb refresh, tickets, waiver tickets, minor ticket, costs.json -> next plan
  otherwise                                      -> needs-human (factory retry | factory approve | factory rework | factory cancel)
```

A job in needs-human is left only through `retry`, `rework`, `approve` or `sync`.

With `reviewers = ["code", "security"]` the review step runs the roles at the same time, and both run every round, so one fix round gets both sets of findings. The security role is skipped only when the round changed at least one path, the diff is not risky and every changed path is under `docs/**`. The review file and the verdicts always list `code` first, whatever role finishes first. The review file `review-code-r<N>.md` then holds one `## code review` and one `## security review` section, each with a `Role verdict:` line, and ends with one combined `VERDICT:` line (`APPROVE` only when every role approves). The raw replies are kept as `review-code-r<N>.raw.md` and `review-security-r<N>.raw.md`. With only `code`, the file is the code reply as before. The security run is recorded in `usage.jsonl` under the role `security-review`, so `factory tokens` and `factory costs` show it. The two reviewers run at the same time, so their mid-run budget checks do not count each other's tokens. A round can therefore go over `budget_weighted` by up to one reviewer run, and the job then parks on the budget after the review.

## Spec check

`factory new` checks the spec and its plans before it creates anything, whenever it is given a spec (`--spec`, or a plans folder). A call with no plans and no spec runs no check. Two checks run, and a refusal creates no job, board entry or worktree.

- The decisions check always runs. When the plans touch risky paths and no `## Decisions` item says `Decided by: user`, the call is refused. Intake uses sourced decisions in its place: every decision cites a file of the exported tree (`Source: path[:line[-last]]`), the driver checks it, and `--accept-spec` has no effect there.
- The spec review runs when `spec_review` is true (the default). A read-only reviewer (tools `Read, Grep, Glob`) reads the spec and the plans and answers APPROVE or CHANGES. A CHANGES verdict, or a missing verdict, refuses the call. The review is saved under `<state>/spec-reviews/` and the refusal names the file. The run has its own cap, `spec_review_budget_weighted` (default 100000). A run that does not finish (a failure, a stall, the turn limit or the cap, even when its partial text ends in APPROVE) is refused with `the spec review did not finish (<reason>)`, saves no review file and is run again by the next `factory new`; `--accept-spec` passes it. A review file that cannot be written prints a warning with the path and the error, and the agent still runs once per process. After `created job`, `factory new` prints `note: spec review <verdict>, saved at <path>` (or `not saved (<path>)`) whenever a review ran or was reused. An accepted risky spec with no user decision records `(no agent review ran)`. When a preset is in use but has no spec reviewer agent file (a preset from before this check), the call is refused with `the spec reviewer agent is missing; run factory upgrade or factory presets refresh, or set spec_review = false`, and no agent runs.

A refusal ends with the hint to record the user's answer as a `Decided by: user` decision, or to pass `--accept-spec "<reason>"` (one line, not empty, at most 200 characters). The `/factory` command shows the findings to the user, fixes the spec with them and runs `factory new` again. The supervisor create endpoint answers a refusal with 409 `conflict` and the same text when it carries the `--accept-spec` override hint (a missing user decision, or a spec review that asked for changes or did not finish). Every other failure of `factory new` stays a 500. It takes `accept_spec` in its body, and waits up to the project's `stall_minutes` plus two minutes for the review.

## Job model

- One job is one feature. It holds a series of plans, each of at most 3 tasks, run one after another, never in parallel.
- Job id `NNN`, branch `factory/NNN-<slug>` (a subfolder project: `factory/<project>/NNN-<slug>`), worktree `<factory home>/projects/<name>/worktrees/factory-<project>-NNN-<slug>` for a registered project (`.claude/worktrees/factory-NNN-<slug>` in the legacy in-repo layout). The driver creates the worktree with `git2` and starts the builder inside it.
- Job directory `<state>/jobs/YYYY-MM-DD-NNN-<slug>/` (`.superpowers/sdd/...` in the legacy in-repo layout, gitignored): `spec.md`, `plan.md` (the current plan), `progress.md` (the ledger), `checks.md`, `checks-failures.md` (the failing check output, read by a rework), `review-*.diff`, review replies.
- `factory new` without `--plans` makes one job at `build` with no plan, so `plan.md` must be placed by hand in the job folder (`new` prints the path) before `run`. Without it the run parks the job at needs-human with `no plan.md in the job folder` and starts no agent.
- `factory new --plans` makes one job per plan at once. Plan 1 starts at `build`. Plans 2 to N wait at `spec` and share a series id `S<n>` (`series.id` on the board). The spec file given with `--spec` (or `spec.md` beside the plans when `--spec` is left out) is checked and copied into every job's folder as `spec.md`, so every builder and the reviewer can read it. The check needs a `## Goal` of at least 8 words, an `## Acceptance criteria` section with at least one list item, and no `TBD`, `TODO`, `FIXME` or `???` outside code, else `new` creates no job. A section ends only at a level-1 or level-2 heading outside code fences, and only a level-2 heading that reads `Goal`, in any letter case, names the Goal. `--spec` needs `--plans`. Paths in a committed series are repo-relative, and so are the `Next plan:` lines. `run --series` runs the next waiting job when the plan before it merges, and `run <id>` on a waiting job moves it to `build`, unless the series check parks it as `series-waiting` while the plan before it has not merged. A series with one plan has no id. A series made by an older version has no id either, and its next job is created after the previous one merges. `factory cancel <id>` on a series plan also cancels every later plan of the series that is not merged or cancelled, and prints one line per job (`job 031 cancelled (later plan of S7)`); their reason is `cancelled: plan <n> (job <id>) was cancelled`. `--only` cancels only that job. A live lease on any of them refuses the whole cancel without `--force`. A job that was replaced cannot be cancelled; cancel its replacement. Each cancelled job logs `operator.cancel`, and cascaded ones carry `cascade_from`. Plans are copied into their job folders when `factory new` runs, so edit a plan's `plan.md` in its job folder, not the committed plan file, and do not change a `Next plan:` line after `new`. To fix a failed plan, edit and commit its plan file, then run `factory new <slug> '<feature>' --plans <fixed plan file> --replaces <id>`. The new job takes the failed job's series id and index, starts at `build`, and the old job is cancelled with `cancelled: replaced by job <new>`. The fixed plan's `Next plan:` line must point to the same file as the original's. Without `--spec` the spec check (risky-path rule and spec reviewer) runs on the old job's `spec.md` with the fixed plan, and that spec is copied. Refusals: a folder, a merged job, a job without a series, a live lease, a wrong `Next plan:` line, a job already replaced, no `spec.md` and no `--spec`. `factory rework` of a replaced job is refused too and names the replacement; a cancelled job that was not replaced is still revived by rework.
- Before a builder runs, the driver checks the series order. A later plan that starts while the plan before it is not merged (a run request, `run <id>`) parks as `series-waiting` with `plan <n> (job <id>) is not merged yet`, before any builder runs. When the plan before it merges inside `run --series`, the series moves it back to `build` and goes on with it (ruling `retry at build, plan <n> merged`); otherwise `factory retry <id>` does. A cancelled plan before it keeps the later plan parked with `plan <n> (job <id>) was cancelled; cancel this plan, or replace plan <n> with factory new --replaces <id>`; after a replacement the check waits on the replacement. Series lookups by index prefer a job that is not cancelled. Only series with a series id are matched. The same check also cuts the branch again from the base branch for any job that has a worktree but no commits of its own while the base branch moved since (ruling `re-cut the branch from ...`), and the next builder still gets the rework note or the findings file. A worktree with uncommitted or untracked work is never removed: it is kept with the ruling `kept the stale worktree: <why>`. A job with commits is never cut again.
- State folder: in a registered project the board, events, `kb-stopped` and the job folders live under `<factory home>/projects/<name>/state/`; the `.superpowers/...` paths below are the layout of an unregistered checkout.
- `.superpowers/factory/board.json`: stage per job. Stages `spec`, `build`, `review`, `approve`, `merged`, `needs-human`. Same names and fields as v1, so the dashboard keeps working.
- `park.json` (job folder, written by the driver when it parks a job): `kind`, `reason`, `at`, `round`, `next` (the commands to run) and `parts` (title, source, text, truncated; each text capped at 200 KB, keeping the end), and, when they apply, `verdict` and `risky` (after a review) and `resume` (the stage `factory retry` continues from). Kinds: `checks-failed`, `changes-remain`, `risky-diff`, `diff-too-large`, `budget-stopped`, `builder-stopped`, `builder-blocked`, `builder-committed`, `builder-tampered`, `nothing-changed`, `no-checks`, `no-tests`, `commit-failed` (only when the driver recovers a job an older version stopped in the middle of a batch commit), `review-failed`, `series-waiting`, `machine-waiting`, `other`. The board job has `park_kind` and `park_at`, cleared when the job leaves needs-human. `factory status` prints the reason, the first 12 lines of the first part and the next commands for each parked job.
- `.superpowers/factory/events.jsonl`: events written by the driver, each with the job id.
- Only the driver writes the ledger, board and events. Agents and the `/factory` command never do.
- Config: settings resolve in this order, later wins: the built-in defaults, the installed preset `<factory home>/presets/<preset>/config.toml`, `[settings]` in `<factory home>/projects/<name>/project.toml`, then `.factory/override.toml` on the base-branch tip, which may add only `risk_paths` and `checks`. `.factory/**` is always a risk path, so a repo cannot change its own factory settings without review. `.claude/factory/config.toml` is read only in the legacy in-repo layout. Every setting is listed with its default in `docs/reference/project-toml-reference.md`.

## Modes

- Default: builder and reviewer run on Sonnet. The reviewer moves to `reviewer_model_large` (default Opus) for a risky diff or a round over `reviewer_large_over_lines` (800) changed lines, and the builder moves to `escalation_model` (default Opus) after `escalate_after_fix_rounds` (1) fix round. See "Builder escalation and the fresh fix builder".
- `/factory <feature> --opus`: this job uses Opus (`factory new --model claude-opus-5-5`). Every agent of the job runs on it: builders, fix rounds, both reviewers and the KB refresher, and their helper agents. `factory new` refuses a blank or Fable model, and a Claude model on a Codex project. The model is never in the agent files, the driver passes it.

## Agents

The agent files ship in `crates/factory-core/presets/<preset>/agents/` and are installed into `<factory home>/presets/<preset>/agents/`. A job resolves each file by name from three layers, later wins: the installed preset, the project's own `<factory home>/projects/<name>/agents/` folder, then `.factory/agents/<name>.md` tracked on the base-branch tip. The agents reach Claude through `--agents <job folder>/agents.json`, and the driver writes no agent file into a worktree or the main checkout. A job that edits or deletes a tracked `.factory/agents/` file does not change the text its own agents get. When an override lacks the preset's `<!-- factory-rules: N -->` marker (or has a lower N), or one of its `## ` sections, the driver prints `note: agent drift: <name> lacks ...`. A retired file (`factory-committer.md`) is ignored in every layer, with the note `ignored retired agent <name> from the project folder|the base branch; delete it`. The factory needs a claude CLI that accepts `--agents` together with `--agent`. Claude Code 2.1.330 was probed on 2026-11-22 and works.

| Agent | Tools | Duties |
|---|---|---|
| `factory-builder` | Bash, Read, Edit, Write, Grep, Glob, Skill, Agent (`BUILDER_TOOLS`) | Runs headless in the worktree. Follows the implementer rules of `superpowers:subagent-driven-development` (its implementer-prompt.md), one fresh builder per task. It is the implementer, not the controller: no implementer or reviewer subagents and no per-task review (the driver does the ledger, commits and final review), with a `Rules in short` digest of TDD, systematic debugging and verification. Never commits, the driver does (see Commits). Its reply ends with a commit note between `---BEGIN COMMIT---` and `---END COMMIT---`. At most 2 helper agents per task, only to read or search code. The reply starts with `Status: DONE`, `DONE_WITH_CONCERNS`, `BLOCKED` or `NEEDS_CONTEXT` (a first-line `BLOCKED: <reason>` is still accepted), then at most five concern lines, then `Ruling: <what> — <why> — <cost if wrong>` lines. BLOCKED and NEEDS_CONTEXT park the job as `builder-blocked`. |
| `factory-code-reviewer` | Read, Grep, Glob | Loaded by the driver with `--agent`. The prompt names the round diff, the plan, the spec and, from round 2, the earlier findings and the builder's reply to them. It also carries the plan's Review Focus, the builder's rulings and concerns recorded while building, and the plan outline. One combined review: correctness, error handling, placement per the project's own rules, persistence compatibility, tests (one per acceptance criterion), efficiency and the project rules from the job prompt. The preset's stack section adds its own checks (the Rust preset: no unwrap in production code, serde attributes, no git CLI). In a re-review each earlier finding is tagged `[fixed]` or keeps its tag, and anything it cannot judge goes under `Declined to judge`, before the verdict. At most 60 lines, findings tagged `[blocker]` or `[minor]`, last line `VERDICT: APPROVE` or `VERDICT: CHANGES`. |
| `factory-security-reviewer` | Read, Grep, Glob | Runs only when `reviewers` lists `security` (default `["code"]`, which must always hold `code`). Gets the same diff, plan, spec and notes as the code reviewer, plus the `security_rules` line, and reviews for security problems only. Same reply format, findings tagged `[blocker]` or `[minor]`, last line `VERDICT: APPROVE` or `VERDICT: CHANGES`. A missing agent file parks the job and shows under `problems:` in `factory project show`. |

Each preset also ships `factory-spec-reviewer` (read-only, runs at `factory new`, see "Spec check"), `factory-intake-drafter` (Read, Grep, Glob, Write; drafts the spec and plans for issue intake) and `factory-kb-refresher` (see "Knowledge base (refresh side)"). `factory-security-reviewer` is the only extra code reviewer. There are no spec-writer, ux, ui or approver agents. Risky diffs go to the human.

The reviewer can use a stronger model on risky or large diffs. `reviewer_model_large` (default `"claude-opus-5-5"`; the same name as `reviewer_model` turns it off) names it, and `reviewer_large_over_lines` (default 800, above zero) is the threshold on the round diff's changed lines. A risky job diff always uses it, a diff exactly at the threshold does not, and the choice is written to the ledger as `review model: <name> (<reason>)`. The weighted-token figure ignores the model, so a stronger model's extra cost does not show in `factory tokens`. Under Codex `reviewer_model_large` is unset unless you set it, so every review uses `reviewer_model`. When the job pins its own model (`--model`), the ledger says `review model: <name> (job model)`.

## Commits

- The builder never commits. The driver stages everything not ignored and makes the commits itself with git2. No agent writes a commit message.
- Signing: when the git setting `commit.gpgsign` is true (system, global or repository config), the driver signs its snapshot, sync and final merge commits the way `git commit` does, so GitHub shows them as verified. `gpg.format` picks OpenPGP (`gpg`), x509 (`gpgsm`) or SSH (`ssh-keygen`), and `gpg.program`, `gpg.openpgp.program`, `gpg.x509.program`, `gpg.ssh.program` and `user.signingkey` apply as in git. The signing program never prompts (`--pinentry-mode error` for gpg, `SSH_ASKPASS_REQUIRE=never` for ssh-keygen) and is killed after 30 s. When signing fails, the commit is made unsigned and the job goes on. The ledger gets `commit <sha7> is unsigned: <reason>`, stderr gets a warning, the events get a `commit.unsigned` event, and an unsigned final merge shows `<sha7>, unsigned: <reason>` in the `merge` gate detail.
- Commit note: every builder reply ends with a note between `---BEGIN COMMIT---` and `---END COMMIT---`: a subject of at most 50 characters in the imperative with no trailing period, a sentence on why, and one bullet per change. The note comes after the `Status:` line and any `Ruling:` lines. Example:

```
---BEGIN COMMIT---
Add a retry endpoint for parked jobs

The dashboard had no way to continue a parked job.

- Add POST /projects/{p}/jobs/{id}/retry with 409 refusals
- Refuse approve while a retry is queued
---END COMMIT---
```

- The driver saves each note in the job folder under `commit-notes/`: `task-<N>.md` for a plan task, `round-<R>.md` for a fix or rework round, `build.md` for a job with no task list. A missing or bad note (no markers, an empty or long subject, a trailing period, an empty body) uses a fallback note built from the task title and the changed paths, and the ledger gets one line naming the problem. It never parks the job.
- Snapshots: after each task the driver makes a `wip: task <N> <title>` commit on the job branch, after the plan builder of a job with no task list `wip: build the plan`, and after a fix round `wip: fix round <R>`. They are working commits on the job branch, carry no footer and are what review, the fix-round floor and `rework` build on.
- The merge into the base branch is a no-ff merge. Its message is written by the driver, in this layout:

```
feat(retry): add a retry endpoint

<the plan's Goal line, wrapped at 72>

Add a retry endpoint for parked jobs:
- Add POST /projects/{p}/jobs/{id}/retry with 409 refusals
- Refuse approve while a retry is queued

Review fixes:
- <bullets of each fix round note>

Fixes: #142
Factory-Job: 042
```

- Subject: the plan's `**Commit:**` line, else `feat: <plan title>`. Then the plan's Goal as a paragraph, one section per task note (its subject, then its bullets), `Review fixes:` with the bullets of the round notes (left out with no fix rounds), then the footers. A job from an older version with no notes takes its sections from the subjects of its non-`wip:` commits.
- Sync message: fixed text, `chore(factory): sync <base branch> into job <id> <slug>`, plus the same footers. If a merge message fails its checks, the fixed `chore(factory): merge job <id> <slug>` is used with the footers and a ledger line. Every merge and sync message ends with `Factory-Job: <id>`, also for a job with no issues.
- The plan's `**Commit:**` line sits in the plan header next to `**Goal:**`: `**Commit:** <type>(<scope>): <subject>` (or `<type>: <subject>`), a lowercase Conventional Commits type (`feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert`), a one-word scope, at most 50 characters, no trailing period. `factory new` refuses a plan whose line breaks the rule. With `--one-job` the spec carries one such line under its `# ` title, and a spec with no line gives `feat: <spec title>`.
- Parks: `builder committed on its own (the branch was at <sha7> before the run)` (in a task plan `builder committed on its own in task N (the branch was at <sha7> before the run)`), `builder made no changes` (only when the branch has no commits above the base, see "Builder recovery"), and `task N changed nothing` (same rule). A builder that moved HEAD off the job branch parks with `builder moved HEAD off the job branch in task N ...`. The reason names the detached sha when known and tells the person to note it (`git rev-parse HEAD`), run `git checkout <branch>` in the worktree (uncommitted changes stay), then `factory rework <id>`. To recover from the builder-committed parks, run `git reset --soft <sha7>` in the job worktree (the sha is in the park reason), then `factory rework <id>`.
- Floor: the job base in round 1, the previous head in a fix round. After a red-check round the floor is the unreviewed head, which is stricter and safe.
- Tokens: `factory tokens` shows the roles `committer` and `message` only for jobs of older versions.

### Issue references

- The board job has an `issues` field: the `#N` numbers found in the `factory new` feature text only (every job of a series gets the same list). `spec.md` is never read for issues, because a spec that merely cites an issue would otherwise close it with `Fixes:`. Without a number the field is empty and no issue footer is written.
- Footers the driver checks or adds:

| Commit | Footer lines |
|---|---|
| `wip:` snapshot commits on the job branch | none, they carry no footer |
| merge message of the last plan, or of a single-plan job | `Fixes: #N`, then `Factory-Job: <id>` |
| merge message of an earlier plan of a series | `Relates to: #N`, then `Factory-Job: <id>` |
| sync message | `Relates to: #N`, then `Factory-Job: <id>` |

- Sync messages and the merge messages of earlier plans stay `Relates to`; only the merge of the last plan (or of a single job) says `Fixes`, also when the spec lists follow-ups.
- `Fixes` closes the issue when the user pushes the merge to GitHub. A job that only helps an issue should be created without the number in its text, so the merge does not close it.

### Knowledge base (read side)

- `kb_path` in `config.toml` is the project's KB folder, an absolute path or one that starts with `~/` (expanded with `HOME` when the refresh runs). A relative path, a `..` component and any character other than ASCII letters, digits and `/ . _ - ~` (so no spaces, commas, parentheses, backslashes or glob characters) fail the config load. A symlinked folder is resolved to its real path, which must follow the same rule. Unset turns the KB off. Builders and the reviewer get the folder as `--add-dir`, because it is outside the job worktree.
- With it set, the builder (first, per-task, resume and fresh fix) and reviewer prompts get a line: read `README.md` in that folder first, then only the topic pages that apply, treat the KB as read-only, let the source and the spec win on a conflict, say in the reply when a page is stale, and skip it if the folder does not exist. The reviewer keeps `Read,Grep,Glob`.

Whether headless agents can read the KB through `--add-dir` has not been shown by a finished real run (see the status below).

### Knowledge base (refresh side)

- Config: `kb_path` (see above), `kb_model` (default `claude-sonnet-5-5`), `kb_budget_weighted` (default 150000 per run) and `kb_auto_commit`. `kb_auto_commit` makes no commits: the KB edits always stay uncommitted in the KB folder. On a Codex project, setting it only makes the KB repo's git folder writable for the refresher.
- After a merge lands, the driver runs the `factory-kb-refresher` agent. The post-merge refresh runs inside the merge step, so the merge call returns after it. The agent runs in the main checkout with the KB folder as `--add-dir`, runs the `kb-refresh` skill (its own subagents through the Agent tool, never `Workflow`) and replies with the pages changed, one per line, or `No relevant changes`.
- What it may touch: the run uses permission mode `dontAsk`, so the allowlist is binding. Edit and Write are scoped to the KB folder (`Edit(//<kb path>/**)`, `Write(//<kb path>/**)`). Bash is read-only git (`git log`, `diff`, `show`, `rev-parse`, `merge-base`, `status`), the same with `git -C <repo root>` tied to the main checkout path, plus `date` and `ls`. The guard below checks the main checkout fingerprint (HEAD, every ref, and the content of non-ignored changes). Ignored paths are not covered. The dangerous case is `.claude/worktrees/`, where live job worktrees sit, so a change there goes unnoticed.
- The guard: the driver takes a fingerprint of the main checkout before and after the run (tip, status entries and file contents). A difference is `Tampered`, whatever the reply says. The fingerprint can show a false `Tampered` if you edit the main checkout while a refresh runs. It fails closed, and a manual run clears it.
- The stop marker: `Tampered` writes `.superpowers/factory/kb-stopped`, and automatic refreshes are skipped while it exists (a ledger line says so). Clear it by running `factory kb-refresh --job <id>` after you looked, or delete the file. A false `Tampered` is cleared only by a manual run that ends `Refreshed` or `UpToDate`.
- Failures never park or unmerge a job. A failed, over-budget or blocked run writes a ledger line `kb refresh failed, <reason>`. Success writes `kb refreshed, pages: ...` or `kb up to date`.
- `factory kb-refresh --job <id>` reruns it by hand. `--job` is required, the command refuses unless the job is in stage `merged`, it takes the project lease like a run, and it refuses while another process holds the job's lease. Do not run it while a driver run is live.
- Tokens: the run is logged with role `kb`, so `factory tokens <id>` shows a `kb` row.
- Limits: the run stops after 80 turns, at `kb_budget_weighted`, or after `stall_minutes` without output.

Status (2026-11-22): the refresh has not finished in a real job yet. In the 19 jobs on this machine that ran it, ten runs ended with `BudgetStop` and nine replied `BLOCKED` because Bash was denied, so the measured `kb` cost is still open. `kb_path` is commented out in the rocket-factory project until that is fixed. When it is set again, run `factory kb-refresh --job <a merged job id>` once and check that the `kb-refresh` skill loads, the agent reads and writes the KB through `--add-dir`, the Agent fan-out works, the fingerprint guard stays quiet and `factory tokens` shows the `kb` cost; then adjust `kb_budget_weighted`.

## Ticket hygiene

The driver can file the leftover work of a ticket and close the ticket after the last plan of a job merges. It reaches GitHub only through `gh`, behind the `Tracker` port in `factory-engine/src/tracker.rs`. The command is `factory tickets <id> [--dry-run]`, and the config key `tickets_on_merge` (default `true`, `false` turns it off) runs the same step at the end of `merge_stage`. A project that never set the key now closes its tickets after a merge.

- The parent tickets come from the feature text given to `factory new`: `#N` and `https://github.com/<owner>/<repo>/issues/N` both count. A URL for a repository other than `origin` makes the step skip.
- The remaining work comes from the `## Follow-ups` section of the job's `spec.md`. One bullet is one ticket: the title is the first sentence (at most 100 characters), the body is the whole bullet. The new tickets get the parent's labels except `tracking`, the `intake_label` and any `factory:` label (any case), so a follow-up of an intake issue is never picked up by intake. A missing or empty section files nothing.
- The step needs a merged job that is the last plan of its series. It files the tickets, comments once on each parent (job id, `main` sha, "not yet pushed" when `origin/main` lacks it, links to the new tickets) and closes each parent that is still open.
- `tickets.json` in the job folder records every created ticket, comment and close, saved after each `gh` call. A re-run, also after a failure, never repeats a call. `--dry-run` makes only read calls and prints each action.
- A follow-up that was started but not recorded is found again by its title and the `Filed by factory job` line, so a rerun adopts that issue instead of filing a second one.
- A real `factory tickets` run takes the job lease, so a second run on the same job is refused. A dry run takes none.
- A follow-up body over 60,000 characters is cut, with a note that says so. A repeated follow-up title gets a ` (2)` suffix, ` (3)` for the third, and so on.
- A `gh` failure is written to the ledger and printed. It never changes the merge result, and `factory tickets` exits non-zero. A failed filing stops the step before the close and is a warning: the `Fixes` footer still closes the ticket on push, the follow-ups stay in the spec, and `factory tickets <id>` retries.

Waiver tickets: when you approve with `--waive "<reason>"` over open review blockers, the driver records them in `waivers.json` in the job folder. After the merge, `waiver_tickets_step` runs right after the ticket step and files one ticket per waived blocker, whether or not `tickets_on_merge` is set. `factory tickets <id>` retries it. An intake job files none.

Post-merge steps run in this order, each a soft failure that never changes the merge result: the KB refresh, the ticket step (when `tickets_on_merge`), waiver tickets, the minor ticket, the `costs.json` rebuild, then the cleanup of the job's worktree and branch (when `cleanup_on_merge`).

Minor tickets: after a merge, `merge_stage` runs `minor_tickets_step` once the waiver step is done. It files one ticket titled `Minor review findings from job <id>: <slug>` whose body is a `- [ ]` checklist of the open `[minor]` lines (not `[fixed]`) of the last review file, with no labels. It does not apply the series gate, runs whether or not `tickets_on_merge` is set, and is switched off with `minor_tickets = false` (default true). `minors.json` in the job folder holds `{"issue": <number>}` once filed; a failure is soft and `factory tickets <id>` retries it.
- Merge footer: `Fixes: #N` on the last plan of a job (and on a single-plan job), `Relates to: #N` on every earlier plan. The spec's follow-ups do not change it. The ticket step files them before it closes. Commit and sync footers stay `Relates to`.

Issue intake: with `intake = true`, `factoryd` queues one `Intake` request per project every `intake_poll_minutes` (default 15), never while that project has a request queued or running, and the child is `factory --project <p> intake --once`. Intake takes an open issue that carries `intake_label` (default `factory`), was labelled by a trusted user (`intake_trusted`, else the `gh` user) and was not edited, commented on or relabelled since. It picks `priority: P0` before `P1` before `P2`, then the oldest, and works one issue at a time. The issue gets `factory: running` and a comment with the job id. A drafter agent with no `Bash` and no network writes the spec and plans from an export of the tracked files of the base branch, then the series runs as usual. On a merge intake files the spec's follow-ups through its own call (`file_follow_ups`, once, after the last plan), sets `factory: done`, comments and closes the issue. The comment is `merged as job <ids>` and one fixed line: `follow-ups filed as #a, #b`, `no follow-ups` or `follow-ups not filed, see the spec of job <id>`. A filing failure never stops the relabel or the close. The follow-ups never carry `factory` or `factory: ...` labels; on a failure or a park it sets `factory: needs-human` and posts a fixed comment with a reason code. No agent text is ever posted. A pass refuses to start, with no label change, when the drafter agent file `factory-intake-drafter.md` is missing; `factory presets refresh` restores it. An intake job carries a marker, so `factory tickets <id>`, the minor ticket step and a waived approve never file, comment, close or relabel for it: the guarded tracker (`tracker_for`) skips them quietly, and only `intake.rs` uses the raw tracker.

## Logging

The driver and `factoryd` write one JSON line per event, next to `events.jsonl`, the board and the ledger, which are unchanged.

- `<state_dir>/log.jsonl` for the driver (scope `cli`), `<home>/factoryd.log.jsonl` for `factoryd` (scope `factoryd`).
- The driver takes its level from `log_level` in `project.toml` (default `info`). `factoryd` always uses `info`. `FACTORY_LOG` overrides either one and takes a level or comma-separated `target=level` directives. It changes only the file, never what stderr shows. An invalid `FACTORY_LOG` prints `note: FACTORY_LOG ignored: <why>`.
- stderr shows `warn` and above, plus `sup.notice` at `info`. A failure logs an `error` line and still prints a human line.
- A log failure never changes a stage, a merge or a command. At most one `note: log disabled: <why>` is printed at startup.

| Key | Note |
|---|---|
| `ts` | UTC, RFC 3339 with milliseconds |
| `level` | `error`, `warn`, `info`, `debug`, `trace` |
| `scope` | `cli` or `factoryd` |
| `project` | omitted when the checkout is not registered |
| `job_id`, `stage` | omitted outside a job |
| `event` | dotted name: `job.created`, `stage.change`, `agent.start`, `agent.end`, `check.run`, `git.op` (debug), `task.start`, `task.end`, `commit.done`, `operator.approve`, `operator.retry`, `operator.rework`, `operator.cancel`, `decide`, `merge.done`, `park`, `step.error`, `sup.start`, `sup.finish`, `sup.reject`, `sup.notice`, `sup.warn`, `sup.error`; also `builder.wait`, `builder.probe`, `builder.wait_end`, `builder.rerun`, `builder.restore_failed`, `builder.session_save_failed` (builder recovery), `cargo_check.store`, `cargo_check.ledger`, `cargo_check.cleanup` (the Cargo lock check), `intake.card`, `intake.fail`, `intake.skip`, `job.rollback`, `children.reap`, `park.error` and `park.record_error`; also `cleanup.done` and `cleanup.kept` (the cleanup after a merge) |
| `msg` | at most 300 characters |
| other | event fields, such as `from`, `to`, `sha`, `role` |

`agent.end` also carries `input`, `cache_creation`, `cache_read`, `output`, `turns`, `resumed`, `round`, `start_ctx`, `end_ctx` and `session_id`. `operator.approve` has `without_review` (and `waived` and `waive_reason` after `approve --waive`), `operator.retry` has `resume` (the stage word), `operator.rework` has `note` and `operator.cancel` has `reason`, `force` and, on a cascaded job, `cascade_from` (the id of the cancelled target); they are logged only after the command succeeds.

Rotation: a file over 10 MiB is renamed to `.1`, with `.1` and `.2` moved to `.2` and `.3`; the old `.3` is dropped. Writers append one line at a time, so concurrent processes lose no line.

Redaction: a field whose name contains `token`, `key`, `secret`, `password`, `passwd`, `passphrase`, `credential`, `auth`, `bearer` or `cookie` (any case), or has `pat` or `pass` as a whole part of its name (split on `-` and `_`), is written as `[redacted]`, except the count `weighted_tokens`. In a check command, the value of a long flag with such a name is redacted in both forms, `--name=value` and `--name value` (the word after a bare flag is hidden, even when it starts with `-`). Short flags are never redacted. Environment variables, prompts and command output are never logged.

`factory log [--job <id>] [--level <lvl>] [--event <name-or-prefix>] [--tail <n>]` prints the matching lines of the log and its rotated files, oldest first. `--level` keeps that level and above, `--event` keeps a line whose event equals the value or starts with the value and a dot (`agent` matches `agent.end`, `tas` matches nothing; an empty value is refused), `--tail` defaults to 100 (0 means all), and `--project <name>` reads that project's file. A missing file prints `no log yet`.

## Draft cards

A draft card tracks a spec that is still being written, before any job exists. It lives in the board's `drafts` list.

- `factory spec start <slug> '<feature>'` creates a draft at the `interviewing` stage. The slug goes through the same slugify as job names. A slug with no letters or digits is refused, and so is a slug of only digits (`a draft slug cannot be only digits`), because `factory cancel 12` means job 012.
- `factory spec stage <slug> <interviewing|writing-spec|writing-plans>` moves the card to another stage.
- `factory spec resume <slug>` prints the `draft`, `stage` and `file <spec path> (exists|missing)` lines.
- `factory cancel <slug>` removes the card and keeps the spec file. A numeric argument still means a job.
- `factory status` lists the cards in a `drafts:` block.
- `factory new` adopts the draft with the same slug and removes it. When no list item of the spec's `## Decisions` section says `Decided by: user`, or the spec has no `## Decisions` section at all, `factory new` refuses a spec whose plans touch risky paths, unless you pass `--accept-spec "<reason>"` (see "Spec check"). Otherwise it prints `warning: no decision in this spec was answered by the user` after `created job ...`, and the job still runs. A single plan file with no spec text gets no warning.
- The dashboard shows each draft in the Spec column. A card not updated for 24 hours gets a stale marker (dashboard `--draft-stale-hours`, factoryd `draft_stale_hours`).
- An older `factory` binary that rewrites the board drops `drafts`.

Plan lint: `factory new` lints every plan on the chain before any job exists. A `TBD`, `TODO`, `FIXME` or `???` outside backticks and code fences refuses the call and names the plan file and line, and so does a `**Commit:**` header line that breaks the subject rule (see "Commits"). Other findings only warn: a missing header field (Goal, Architecture, Tech Stack, Spec, Global Constraints, Review Focus), no `superpowers:subagent-driven-development` executor in the header, a task without `**Files:**` or `**Interfaces:**`, and a commit step. The warnings are appended to the first job's spec warning, after the spec's own warning, and print as `warning: ...` lines after `created job ...`. The job still runs. A plan whose header names the factory driver as its executor (`**Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review.`) is linted by a shorter rule: its header needs `Spec` and `File Structure`, it needs a `Review Focus` anywhere outside fences, each task needs `**Interfaces:**`, and the executor warning is skipped. With `--one-job`, a task body that names another task by its old number (`Task 2`) outside code and inline code warns, because the tasks are renumbered.

## One job per spec

The default is one job per plan. `factory new ... --plans <dir> --one-job` creates one job for the whole spec
instead, and the driver combines the plans. Plans are still written with at most 3 tasks each, but the driver does not
enforce that cap on this path. The 6 plan cap and the chain rules (`Next plan:` lines) still apply. The flag needs `--plans` and cannot be used with `--replaces`. With a
single plan it has no effect and `new` prints a `note:` line.

The estimate is `110K + 100K per task` weighted tokens, checked against `budget_weighted`. Over budget, `new`
creates nothing, prints a `needs consent:` line (estimate and budget) on stderr and exits non-zero. Re-run with
`--accept-over-budget` (it needs `--one-job`) or raise the budget. With `--accept-over-budget` the job is created and
`new` prints a `warning:` line with the estimate and the budget. At or under budget, `new` prints a `note:` line with
the same figures. The `/factory` command passes `--one-job` for a
feature that starts or ends with that token and asks the user before accepting the overrun.

## Checks

The builder runs only per-crate cargo commands (`cargo check -j4 -p <crate>`), never a bare `cargo check -j4` and never `--workspace` or `--all`: in a fresh job worktree a workspace-wide check builds everything and can stall the run. Plans follow the same rule, and the driver refuses a check command that runs `cargo` with `--workspace` or `--all`, or without `-j4` (`cargo fmt` excepted). The driver runs the checks itself. The commands come from the `[[checks]]` rules of the project's config or its preset (rust, node, go, ruby, csharp), chosen from the changed paths. The rust preset runs `cargo check -j4 -p {crate}`, `cargo test -j4 -p {crate}` and `cargo test -j4 -p {dependents}`, with `cargo fmt` as the format gate and `cargo clippy` as the lint gate. Rocket's own config also runs `yarn tsc --noEmit`, `yarn check` and `yarn test --run --maxWorkers=50%` (half the workers, so the full suite does not time out under its own load). A failure starts a fix round with the trimmed output: the builder resumes its session, or starts fresh when the round is escalated or the session is cold (see "Builder escalation and the fresh fix builder"). The reviewer never sees a red build. Results go to `checks.md`.

A `[[checks]]` rule can also list `format` and `lint` commands, planned after `commands` with the same templating. After each checks run, the job gets a `format` and a `lint` entry in its reviews: `pass` when every command of that gate passed, else `changes`. A gate with no command has no entry, and a new round clears both. `factory status` shows them next to the reviewers, for example `code:pass format:pass lint:pass security:pass`. A failed gate command is a failed check as before.

### No-check and no-test gates

- `no-checks`: a changed code path that no `[[checks]]` rule matches parks the job before the reviewer runs, even when other paths of the diff did run a check. The reason reads `no check covers N changed code paths: a, b (+k more)`. A path counts as covered only when a command was planned for it: a rule that matches but plans nothing (a `{crate}` rule for a path with no owning crate, or a `{dependents}` rule with no dependents under the cap) leaves the path uncovered, and the reason marks it `path (a rule matches but planned no command)`. A path is exempt when it matches `unchecked_paths`. The rust preset covers the root `Cargo.toml` and `Cargo.lock` with `cargo check -j4`. With no `code_paths` setting, a path ending in `.md` or under `docs/**` or `.claude/**` is not code, so a diff of only such paths is not parked. When `code_paths` is set, only the paths it matches count as code. A risky diff is gated too: it parks as `no-checks` before `decide()` can end it as `risky-diff`, and the park carries the review and risk parts. With no config, or a broken one, every path counts as code.
- `no-tests`: a code diff with no test path and no added test marker writes a ledger line and a note to the reviewer. With `require_tests = true` it parks as `no-tests` instead, with the reason `no test was added in <paths>`.
- Where they run: both gates run in the checks stage. The no-checks gate runs again in the final re-run of the checks on the merge head (before an auto-merge, and before a risky-diff park), so a fix round that leaves a path with no mapped check still parks `no-checks`. The no-tests gate does not run again. The final run of the commands is skipped when the checks stage passed on the same head, the worktree is clean and the planned commands are the same as in that run; the no-checks gate still runs. Both park through `park_with`. A `no-checks` park offers `retry` (after the check mapping is fixed; it resumes at review), `approve` (as `approve --without-review` when the review has not run) and `rework`. A `no-tests` park offers `approve` and `rework` only.
- The settings are `code_paths`, `test_paths`, `test_markers` and `require_tests`, see `README.md`.
- The ledger has one line `checks ran: <commands>` or `checks ran: none` before the `auto-merge approved` line. When the passing run of the checks stage is reused it reads `checks ran: <commands> (reused from the checks stage)`. After a sync park the checks always run again.

## Budget

Weighted tokens per job: input + cache creation + output + cache read / 10. Defaults: 1,000,000 per job (`DEFAULT_JOB_BUDGET_WEIGHTED`; a config that sets `budget_weighted` keeps its own value), warn at 80 percent, kill the agent and park the job at 100 percent. Diff cap 4,000 changed lines, 2 fix rounds, a stall limit without events.

## Merge rules

Auto-merge needs all of: no check failure, the latest reviewer verdict is exactly `VERDICT: APPROVE`, the diff is not risky, the diff is within the size cap, the budget is not at stop. `decide()` checks in this order:

1. Budget at stop: park `budget-stopped`, no fix round.
2. Diff over the cap: park `diff-too-large`, no fix round.
3. Checks failed: a fix round while rounds are left, else park `checks-failed`.
4. Approved and risky: park `risky-diff`. The risk gate comes last, so a risky diff still gets its fix rounds first.
5. Approved: auto-merge.
6. A reviewer run that did not finish: park `review-failed`, no fix round.
7. `CHANGES` or no verdict: a fix round while rounds are left, else park `changes-remain`.

Fix rounds are counted since the last rework, at most `fix_rounds` (default 2).

The merge step itself can park too: `nothing to merge, head already in <main>`, `merge conflict in <paths>` and `merge refused: <reason>`. A crash after the merge commit is found on the next run, because the base branch already holds the head, and the job is finished as merged. A cancel or a lost lease during the message build stops before the git merge.

Risky means the diff touches any `risk_paths` entry. The list comes from the project's config or its preset. The rust preset marks `**/Cargo.toml`, `**/Cargo.lock`, `build.rs` files, any path with `auth`, `secret` or `credential` in it, `.github/**`, `**/.cargo/**` and `**/rust-toolchain*`. Rocket's own config adds its trust gate, persistence formats, `src-tauri/`, `DomainEvent`, `package.json`, `tauri.conf.json`, CSP and the harness hooks. A missing or broken config makes every diff risky.

The Cargo lock check can clear a diff whose only risky paths are Cargo files. A path counts as a Cargo file only when every `risk_paths` glob that matches it names Cargo files alone (`Cargo.toml`, `Cargo.lock`). Such a diff counts as normal when all of these hold:

- The lock gains and loses no outside crate.
- `cargo metadata --locked` passes in a clean export of the head commit, made under the system temp folder and removed afterwards.
- No path crate or target leaves the project, and no build script changed.
- No proc-macro, `links`, `build`, `cargo-features`, `[patch]` or `[replace]` change came in.

Anything the check cannot settle stays risky. It only ever turns risky into normal, never the other way, and a risky path that is not a Cargo file (such as `*auth*`) keeps the diff risky without running cargo. The built-in list and the rust preset mark `**/.cargo/**` and `**/rust-toolchain*` risky, so those are never cleared. The ledger gets one line with the `Ruling: ` prefix each time the check runs, never from a stored verdict and never for a stop before the check runs (the park card shows those). A park repeats the note in its Risk part:

- `cargo lock check: no outside code changed, Cargo files are not risky`
- `cargo lock check: outside crates added: <name version, ...|none>; removed: <name version, ...|none>; source or checksum changed: <name version, ...|none>, diff stays risky`
- `cargo lock check: <reason>, diff stays risky`

The verdict is stored in `cargo-check.txt` in the job folder: line 1 is `key <head sha> cargo_lock_check=<bool>; risk_paths=<len>:<glob>...` with `<len>` the glob's byte length, line 2 is `same` or `risky`, line 3 is the ledger note. A stored verdict is reused only for the same head and the same `cargo_lock_check` and `risk_paths` values. Only a pass and verdicts the commit's own content caused are stored, so a cargo or disk failure runs the check again on a retry. The setting `cargo_lock_check` (default true) turns the check off; see `project-toml-reference.md`.

A risky diff gets the same fix rounds as any other (`risky_fix_rounds`, default `fix_rounds`; 0 parks at once). It never auto-merges: after an approve the final checks run on the exact head, and then the job parks as `risky-diff` for a human approve. The park evidence leads with the review. For `risky-diff` and `changes-remain` the parts are, in order:

- `Review`: `verdict <word>, round <N>`, then the open `[blocker]` lines (the whole reply when a CHANGES has none), or `findings file missing`.
- `Risk`: the risky paths, and `added by fix rounds: <paths>` for risky paths that were not in the first review (`risky-paths.txt`). Only on a risky diff.
- `Rounds`: one line per review round, `round N (<source>): <verdict>, <B> blockers`, read from the ledger.
- `Changed files`: the diff stat.

`nothing-changed` after a review fix round, and `no-checks` or `no-tests` parks, put `Review` (and `Risk` when risky) before their other parts. `park.json` also records `verdict` and `risky`. In round 2 or later, before the new review runs, the Review part is the previous round's review, and its text names that round. After a sync, a risky diff that needs a human runs the final checks again and parks with the same parts plus `Checks`. A failing final check parks as `checks-failed` without a fix round, and a diff that cannot be read gives a `Risk` part that starts with `unknown:`.

The driver merges the reviewed sha only: it checks the branch tip equals the recorded head, preflights main (on `main`, clean, not mid-merge), makes a no-ff merge commit (the driver-written message, or the fixed `chore(factory): merge job NNN <slug>` fallback), and verifies the second parent. If main moved it merges main into the job branch inside the worktree, re-runs checks and the reviewer on the sync diff (at most 2 syncs). A sync review is saved as `review-code-r<round>-sync<n>.md`, so the round review `review-code-r<round>.md` is never overwritten. It sets `last_review` with source `sync`, and the ledger line starts `sync review round <round> (sync)`. A conflict goes to `needs-human`. It never pushes and never rewrites history.

## Retry

`factory retry <id>` continues a parked job at the stage that parked it. Every resume stage except `build` runs without a builder. It needs a job in `needs-human` that is not cancelled and whose park has a resume stage. The board field `park_resume` and the `resume` field of `park.json` hold it (`build`, `review`, `rereview`, `merge` or `sync`), and both are cleared when the job leaves `needs-human`. `retry` holds the project and job leases like `run`, writes the ledger ruling `retry at <stage>`, logs `operator.retry`, drives the job on until it merges or parks, and continues a series like `run --series`. It exits 75 like `run` and `approve` when the project lease is held. If the cause is still there, the job parks again with the same reason.

| Park | Resume at | What retry does |
|---|---|---|
| `review-failed` (a reviewer run failed or stalled and no role asked for changes), security agent file not found, `no-checks` | `review` | Resets the reviews of the round and runs the checks and the reviewers on the same head. |
| Branch tip differs from the reviewed head, or a failed sync review | `rereview` | Moves `head` to the tip and runs a whole-branch review. |
| Main busy, merge lock busy | `merge` | Runs the merge step. An approval given before the park (also `--without-review` or `--waive`) carries over when the tip is unchanged. |
| Main moved again after the maximum number of syncs | `sync` | Allows one more automatic sync, then merges. |
| `machine-waiting`, `builder-stopped` after a Failed builder run | `build` | Starts the builder again. The preflight runs first, so a machine that is still down waits again. For `builder-stopped` the builder has already had its one rerun. A rework note saved before the park is kept and sent. |
| `series-waiting` | `build` | Continues at build; the guard checks the plan before it again; the series also continues it on its own once that plan merges. |

A park that is not in the table has no resume stage: other builder parks (`builder-stopped` from a budget stop, turn limit or stall takes `factory rework`), `no-tests`, budget stop, risky diff, changes remain, merge conflict, sync conflict, base not on main, scope violation, missing plan. `retry` refuses them and names their own next commands. After the stage runs, the normal rules apply: a Changes verdict or failing checks start a fix round.

## Rework

- A rework of a job that parked at a task boundary is the next round. It bumps `round`, sets `rework_base` to the new round minus one (a full new set of fix rounds) and writes `changes-r<N>.md` with the note first and then the open findings of the latest review. Earlier files (`review-code-r*.md`, `builder-r*.md`, `changes-r*.md`) are kept. It prints `job <id> back to build, round <N>`.
- `factory rework` also saves the note alone as `user-note-r<N>.md` in the job folder (the new round for a next-round rework, the current round for a mid-task one), and the reviewer of that round is told where it is.
- `rework` refuses a job whose park has a resume stage, except `machine-waiting` and `builder-stopped`, where a note helps the builder: `job <id> parked outside the builder (<reason>); run factory retry <id>, or factory rework <id> --force <note> to send it to the builder anyway`. `--force` keeps the behaviour below. A cancelled job is revived without `--force`.
- A job parked mid-task keeps the old path: `rework-note.md` is used, `round` and `rework_base` stay, and it prints `job <id> back to build, same task`.
- The next review reads `last_review.file` as the previous findings, which can be a sync review file.

## Builder recovery

The driver gives the builder a working machine and one more try. It covers the whole-plan builder, the per-task builders and the fix builders.

- Preflight: before a builder starts, the driver checks free disk space on the filesystem of the job worktree and on the one of the factory home. Each must have at least `min_free_disk_mb` (default 2048, `0` turns the check off, see `docs/reference/project-toml-reference.md`). The reading comes from `fs2::available_space`.
- Classes: a failed run is a machine failure when its error text says the disk is full, the network is down or the API key is bad (`disk`, `network`, `auth`). Claude's own `is_error` result text counts too. Any other failure is plain. A run that ends `blocked` or `needs-context` while the disk is short counts as a disk failure.
- Wait: while the machine is down the driver does not start a builder. It sets the board field `machine_wait` (`disk: 0.4 GB free`, `network` or `auth`), which the dashboard card shows, and writes the ledger line `waiting for the machine: <detail>`. A disk wait probes the disk. A network or auth wait probes with a tiny builder run, free at `turns <= 1`, `output_tokens < 2000` and `weighted < 20000`, so it does not count against the budget in a real way. The probes run at 1, 2, 4, 8 and 15 minutes, then every 15 minutes, and the wait ends at 25 minutes in total. The driver sleeps in steps of at most 5 seconds and stops at once on `cancel --force` or when it loses the lease. A ledger that cannot be written does not end the wait.
- Wait ends: when a probe finds the machine fine, the driver clears `machine_wait`, writes `machine ok after N min` and goes on. At the cap the job parks as `machine-waiting` with the reason `<detail>, waited 25 min` (the detail is cut to 200 characters). A budget stop parks first.
- Rerun: a failed builder run, or a disk-blocked report, gets one rerun, after any wait. The ledger line is `builder failed (<cause or error>), rerunning once`. The rerun carries a pending rework note and keeps the session when it can. A second failure parks as before (`builder-stopped` for a stopped run). A run that parks or reruns keeps the rework note, and it is removed only when the build pass advances.
- Events (see `factory log --event`): `builder.wait` when a wait starts, `builder.probe` per probe (with the state `fine`, `down`, `final` or `parked`), `builder.wait_end` with the outcome and the minutes waited, and `builder.rerun` when the rerun starts.
- Parks: `machine-waiting` resumes with `factory retry <id>` and still accepts `factory rework <id> <note>`. `builder-stopped` from a Failed run (after its one rerun) resumes at build too, with the reason `builder stopped: Failed: <error>` (`builder stopped in task N: Failed: <error>` per task) and the next commands `factory retry <id>` then `factory rework <id> <note>`. A `builder-stopped` park from a budget stop, turn limit or stall has no resume stage and takes `factory rework <id> <note>`. Error text is capped at 500 characters in the agent run and at 200 in park reasons and ledger lines.
- The nothing-changed rule: a builder round that changes nothing parks the job only when the branch has no commits above the base (`builder made no changes`, or `task N changed nothing` in a task plan). With commits on the branch the round advances. The ledger line is `builder changed nothing; it says the work is already on the branch` (`task N changed nothing; it says the work is already on the branch` for a task), the builder's reply is saved (`builder-r<N>.md`, appended per task), and the board field `no_change_round` marks the round. The reviewer then judges the whole branch, not a fix diff, and is told where the builder's reply is. The diff cap, the risky-diff park and `fix_rounds` still apply, so a disagreement ends with `changes-remain`. A fix round that started from failed checks and changed nothing keeps its own rule: the checks run again once.
- Version note: upgrade `factory` and `factoryd` together before setting `min_free_disk_mb`. Older binaries reject unknown config keys.

## Lease and children

- A live owner keeps the project lease past its expiry. An owner whose pid is gone, a zombie or reused is taken over at once, and an owner without a pid keeps the expiry rule.
- A corrupt project lease is refused and left on disk, and a failed run does not delete it. Deleting the file clears it. `factory cancel <id> --force` also removes it, but it gives up that job first.
- Agent and check children run in their own process group (`factory-procgroup`). The group ends on drop and on SIGTERM, SIGINT or SIGHUP of the driver.
- Each driver registers its children in `children-<owner>.json` in the state folder. After a SIGKILL, the next driver that takes the lease reaps the files of dead owners. It kills a group only when the pid runs, leads its group and has the recorded start time, and reports every entry as a ledger line.

## Monorepos

A folder inside a git repo can be its own project (`factory project add <repo>/apps/web`, stored as `subpath`). A repo holds one top-folder project or disjoint subfolder projects, never both. Their builds, checks and reviews run side by side. The builder, fix builder and reviewer start in the project folder of the job worktree, for `claude` and for `codex`. Risk paths and check mappings are relative to that folder, and check commands run in it.

- Scope: the driver lists the job's own changes (from the merge base with the base branch) after the builder and every fix round. Any path outside the folder, including either side of a cross-boundary rename, parks the job as `needs-human` with the paths named. It never auto-merges, `factory approve` refuses it, and the driver never reverts files. `factory rework` makes the builder remove them.
- Lock: merges into the base branch take a per-repo lock in the git common dir. A sync runs in the job worktree, outside the lock. The wait is 30 seconds, then the job parks with `another project is merging into this repository, run factory retry <id>`. Main is re-checked under the lock before a busy main parks the job. The OS releases the lock if its holder dies.
- Contention: while other projects merge, main moves more often, so a job can use more of the sync cap (`MAX_SYNCS`, 2) and park with `<main> moved again after the maximum number of syncs` (from the merge step) or `<main> moved again after N syncs` (from the sync stage). Run `factory retry <id>` for a sync park (one more automatic sync is allowed). `approve` does not reset the sync count, so it parks again at once.

## How to run

Build once from the repo root: `cargo build -j4 -p factory-cli`. Binary: `target/debug/factory`. `docs/guides/getting-started.md` covers PATH, install and registering a project.

| Command | Does |
|---|---|
| `/factory <feature> [--opus] [--one-job]` | Start a draft card, brainstorm and plan with the user, then `factory new` and `factory run <id> --series` in the background. |
| `/factory status` | `factory status`. |
| `/factory approve <id>` | Release a `needs-human` job to the merge step. Refused when the code review has not run, unless `--without-review` is given. While the last review asked for changes and a required role (code, or security when held) has no verdict, every approve is refused, `--waive` included (`Job::approve_refusal`); retry or rework reruns the review. |
| `/factory retry <id>` | Continue a `needs-human` job at the stage that parked it (see "Retry"), then run it on like `run`. When `factoryd` runs, the command queues it with `factoryd enqueue --retry`, so it waits for the daily budget. Refused for a park with no resume stage (the refusal names that park's next commands), and for a cancelled job. |
| `/factory rework <id> <note>` | Send a `needs-human` job back to the builder with a one-line note of at most 200 characters. It also revives a cancelled job that was not replaced. Refused for a park that `retry` can resume, unless `--force` is given. `factory rework` itself does not run the job; `/factory` starts the run, and by hand you run `factory run <id>` next. |
| `/factory tickets <id>` | `factory tickets <id>`: file the follow-ups and close the parent tickets of a merged job, and retry the waiver and minor tickets (see "Ticket hygiene"). |
| `/factory cancel <id>` | Stop and park a job. A running driver is not stopped: it ends at its next stage move as `needs-human: cancelled: <reason>`, and the job never reaches review, approve or merged. A cancel during the git merge itself (milliseconds) is a window it cannot stop: main gets the merge and the job stays cancelled. |
| `/factory cleanup <id>` | Remove the worktree of a finished job. A merged job is cleaned automatically right after the merge when `cleanup_on_merge` is on (the default), so the command is for worktrees that were kept (untracked or modified files, logged as `cleanup.kept` and in the ledger) and for cancelled jobs. It refuses a job another driver holds a live lease on. A cancelled job also loses its board card (not in a series with other active jobs, not while leased); the job folder stays. Once every member of a series is merged or cancelled, cleaning one cancelled member also cleans every other cancelled member, one line each (`job <id>: ...`). `--discard-commits` applies to all of them, and a member whose cleanup is refused keeps its card while the others go on. |

`factory sync <id>` is a CLI command with no slash form. It merges the base branch into a job in `build`, `review`, `approve` or `needs-human`, then reruns the checks and the code reviewer on the sync diff. It is refused for a merged or cancelled job, a dirty worktree, a pending batch commit and a live lease, with the same exit code and message form as `approve`, and it does not count against `MAX_SYNCS`. Only a job in `approve` whose branch tip equals the reviewed head keeps its floor and gets one code review of the sync diff. Every other case (a tip that is not the head, or a job in `build`, `review` or `needs-human` that is behind main) resets the floor and the review states in one board write, keeps `last_review`, merges the base branch if the branch is behind, and runs one whole-branch review by the code and security reviewers from the merge base, saved as `review-code-r<round>-rebase.md`. The raw files of a multi-role review get the run suffix: `review-<role>-r<round>-rebase.raw.md` and `review-<role>-r<round>-sync<n>.raw.md`. A run that stops after the reset is run again the same way, and `approve` refuses in between. A passing whole-branch review of a job that was in `needs-human` parks it again with `sync review passed, run factory approve <id>`. A park after a sync replaces the old `park.json`, and a non-risky decision keeps its kind and hints. A job whose tip is the head and that is not behind prints `job <id> is up to date with <base>` and changes nothing. `factory status` lists it as a next command for a `nothing-changed` park after a rewrite.

The command matches subcommands by exact shape, so feature text such as `next button for pagination` stays a feature. The session does not poll while the driver runs. The harness notifies it on exit.

`factory new` validates the spec and the plan chain, so both are written, approved and committed under `docs/superpowers/` first. The driver cannot ask questions during a run, so the spec and the plans must be complete. `/factory` runs `superpowers:brainstorming` 6.4.2 as it is, with a few overrides: every question carries a recommended answer and two or three alternatives, the work is always sized as a project with a written design, and a user who skips the questions gets the draft cancelled and nothing built. The session writes the spec at the path `factory spec start` printed as the interview goes, one `## Decisions` list item per answered question with the options and the choice, ending `Decided by: user` (or `Decided by: call` for the session's own choice). The session reviews the spec against the quality bar before the user sees it, and the spec is committed only after the user approves it. The driver refuses a spec without the Goal and the Acceptance criteria or with a placeholder, and only warns when no decision says `Decided by: user`. Plans come from `superpowers:writing-plans` in its 6.4.2 style, and each header says the driver runs it using `superpowers:subagent-driven-development`, one fresh builder per task. The spec and the plans are committed on the session's current branch, with a pathspec commit. Job branches start from `main`, so the docs reach `main` only when that branch merges. The driver resolves `Next plan:` paths against the checkout the plan lives in and stores absolute paths, so a session in a linked worktree works. The jobs themselves run from the copies in the job folders. Harness rules the driver keeps: every cargo call has `-j4`, no `--workspace`, no `CARGO_TARGET_DIR`, one implementer per worktree. The builder never commits. The driver removes `ANTHROPIC_API_KEY` and `ANTHROPIC_AUTH_TOKEN` from the environment of every `claude` it starts, so jobs use the subscription login and never bill an API account by accident. Only an explicit `FACTORY_ALLOW_API_KEY=1` in the shell that runs the driver turns that off. A project can decide for itself: `allow_api_key = true` passes the environment key to that project's agents (and fails the run when there is none), `allow_api_key = false` keeps it on the login, and `api_key_file` reads a key for that project only from a file that only you can read. A bad key file fails the run and never falls back to the login.

## Dashboard

`dashboard/` (own workspace, crate and binary `factory-server`, shipped in every release archive) reads `board.json` and `events.jsonl` and serves a local page on port 8765 by default. It never writes them, and the factory works the same when it is not running. It has two modes:

- Root mode, one checkout: `cargo run -j4 --manifest-path dashboard/Cargo.toml -- --root <main checkout>`. Data flow: `factory driver -> .superpowers/factory/{board.json,events.jsonl} -> factory-server -> dashboard.html`.
- Home mode, every project of a factory home: `cargo run -j4 --manifest-path dashboard/Cargo.toml -- --home ~/.factory --port 8765`. It needs `factoryd run`, takes each project's state folder from factoryd's API, holds the API token server-side and keeps it out of every reply, and can send writes (new, approve, retry, rework, cancel) through factoryd. It also shows the daily budget. `--home` and `--root` cannot be combined. See `dashboard/README.md`.

- The header reads "Rocket Factory" with a small version next to it from `/api/version`: factoryd's version in home mode, the dashboard's in root mode. Hovering lists every version, and the label turns amber when they differ.

- The cost panel below the board reads costs.json through /api/state (costs: groups and roles only, null when the file is missing or broken): a row per size group with jobs, median (~ when some jobs are approximate), range and notes, the exact-counts-only medians, and the role split as a stacked bar with a text legend. A card that is not merged shows its estimate; a merged card shows its cost against the estimate range (within, above or below).
- Agents are matched to a job only by the `job_id` on their events. An event without `job_id` shows under "Agents without a job". There is no matching by cwd or stage.
- The card shows the job id, a `plan k of N` chip (hidden for a single plan), feature, branch, builder round, review chips, time in stage (hidden once merged or needs-human), time per stage from `stage_history` when the board has it, weighted tokens against the budget with a bar (text level `warn` at 80 percent, `stop` at 100), and copy chips for `factory approve <id>`, `factory rework <id> <note>` and `factory cancel <id>`.
- A spec card is built for each series id shared by two or more plan jobs (`specs[]` in `/api/state`). It holds the spec title and goal from `spec.md` in plan 1's job folder, and one row per plan with its state (waiting, building, review, needs-human, merged, cancelled). The card sits in the spec column while any plan is open, in needs-human when a plan is stuck or cancelled, and in merged once every plan merged. Jobs without a series id get no card.
- A plan card shows a `spec S2` chip next to `plan k of N` (hover shows the spec title). A plan that has not started sits in the spec column with every task waiting, and its card leaves out stage times, the token bar, agent activity and the branch box until it runs. The spec card shows "Plans n/N done" as a list that reuses the task rows.
- The server reads the board's four token counters and weights them like the driver (cache reads count one tenth). New fields that are malformed are dropped, never a board error. The budget falls back to 3,000,000 weighted tokens when the board has no `budget_weighted` (boards made under the old default). The driver's own default is now 1,000,000. The card bar shows the board total, or the running total from the agent events when that is higher, so it moves while an agent works.
- Only the last 4 MiB of the log is read at startup. Agents idle for a day are dropped (cap 500). The registry resets when the log is truncated, replaced or deleted. An agent silent for 10 minutes shows `stale`.
- A needs-human card with details (`has_park` in `/api/state`) shows a "See what went wrong" button. It opens a native dialog that fetches `GET /api/jobs/<id>/park` (404 without a file, 422 for a broken file, text capped at 1 MiB, the path taken from the board and kept inside `.superpowers/sdd/`). The popup shows the kind, reason, each evidence part with the lines that matter marked and the first one scrolled into view, and the next commands as copy chips. The polled state never carries the evidence text.
- A job with a gate record shows a strip of 18 marks on its card, grouped by stage, with a summary at the right end (`stopped at <gate>`, a count such as `14 passed · 2 notes`, or `n / 18` while it runs) and a list of one line per gate under it. A job parked in needs-human opens the list. Once the user opens or closes a job's list, the page keeps that choice across redraws, as it does for the task list. Jobs without a record keep the review chips. See the Gate record section.
- The page patches only changed text. The status line is not a live region. One short announcement is made when the connection level changes. Bar widths are set through the CSSOM because the CSP forbids inline styles.

## Gate record

With `gate_record = true` in `project.toml` (default false, see `docs/reference/project-toml-reference.md`), the driver writes one JSON line per gate result into `gates.jsonl` in the job folder. The dashboard reads it to draw the gate strip on the card. Without the setting no file is written. Writing is best effort: a failed write logs the event `gates.record_error` and never changes what the job does.

Each line has these fields: `v` (line version, 1), `gate` (one of the 18 keys), `result`, `round`, `head` (short sha, left out when unknown), `at` (unix seconds) and `detail` (one line, newlines become spaces, cut to 300 characters with a final `…`). The `result` is one of `pass`, `fail`, `note`, `waived`, `skipped`, `running` or `waiting`. The round is the builder round of the job; a round of 0 is written as 1. The file is append-only, and a file that ends in a cut line gets a newline before the next line. The `format` and `lint` details list their commands through `redact_command`, the same rule the `check.run` log uses, so a sensitive flag shows `[redacted]`, in both the `--name=value` and the `--name value` form.

The 18 gates, in card order, with their stage group:

| Stage | Gates |
|---|---|
| spec | `spec-check`, `spec-review` |
| start | `series`, `machine` |
| build | `builder-guard`, `builder` |
| checks | `checks`, `format`, `lint`, `no-checks`, `no-tests` |
| review | `budget`, `diff-cap`, `review`, `risk` |
| merge | `final-gate`, `merge`, `approve` |

`spec-check`, `spec-review` and `series` run once per job. Every other gate runs once per round. The `builder` line comes before the diff cap check, so a `diff-too-large` park still shows the builder as passed. A checks run writes `format` and `lint` before `checks`, so a park on failed checks stops at `checks`. The `decision warnings` count in the `spec-check` detail is 0 for an intake spec, which gets no decision warning.

A park writes a fail line for the gate that follows from its kind. The `series-waiting` park writes `waiting` instead of `fail`. `review-failed` and `changes-remain` write none, because the review loop already wrote the review line. A park of kind `other` uses the stage the job was in: build gives `builder`, review gives `review`, approve gives `merge`, and any other stage gives no line. A park that offers `factory approve` also writes an `approve: waiting` line.

| Park kind | Gate |
|---|---|
| `builder-stopped`, `builder-blocked`, `builder-committed`, `nothing-changed`, `commit-failed` | `builder` |
| `builder-tampered` | `builder-guard` |
| `checks-failed` | `checks` |
| `no-checks` | `no-checks` |
| `no-tests` | `no-tests` |
| `diff-too-large` | `diff-cap` |
| `budget-stopped` | `budget` |
| `risky-diff` | `risk` |
| `machine-waiting` | `machine` |
| `series-waiting` | `series` (`waiting`) |

The dashboard shows the latest state of each gate for the job's current round (a board round of 0 counts as 1). A gate that runs once per job uses its last line of any round. Any other gate uses its last line of the current round, and `not-reached` when it has none. Lines from earlier rounds only feed a history text such as `fail r1`. A `running` result counts as `interrupted` when the job is merged, cancelled or parked, and a `waiting` result when the job ended or is working on a stage other than the start gates (`series`, `machine`). The gate the job `stopped_at` is the last `fail` line of the current round. Broken lines, lines of a newer version and lines with an unknown gate or result are skipped.

## Per-task builders

- The driver runs one fresh `factory-builder` agent per `### Task N:` heading of `plan.md`, one after another, each as its own `claude -p --agent factory-builder` run (`--max-turns 120`). A plan with no task headings keeps the old whole-plan builder.
- Each build call handles one task while the board field `task_index` is below the task count. The prompt names the plan, the spec, the base, the current tip, the earlier commit subjects and "Do only Task N". The builder leaves the commit to the driver (a `wip:` snapshot per task) and ends its reply with a commit note. A task that changed nothing advances when the branch already has commits above the base, with the ledger line `task N changed nothing; it says the work is already on the branch`. It parks the job with `task N changed nothing` only when the branch has no commits above the base. `Task N: implementer done (<sha>)` records the `wip:` snapshot sha.
- Builder excerpt (`builder_excerpt`, default on): the task prompt carries the plan header, the task's own `### Task N:` section and the spec goal, design, edge cases, decisions, scope and acceptance criteria (the goal and the whole acceptance criteria always, the other sections while they fit under 8192 bytes, with a marker line where one was cut), and still names the full `plan.md` and `spec.md` for reference. The builder agent reads those files only for something the excerpt refers to outside itself. A plan with no `### Task N:` heading for the task (for example `- [ ] Task N:` checkboxes) falls back to the full prompt. Whole-plan builders, fix resumes and rework resumes are unchanged. `builder_excerpt = false` restores the old prompt.
- Builder carry (`builder_carry_session`, default on, with `builder_carry_max_ctx`, default 45000): the builder of task N+1 resumes the session of task N when the previous builder ended less than the idle limit ago and its `end_ctx` is at most the cap. The idle limit follows `cache_ttl`: 3300 seconds for `"1h"`, 240 for `"5m"` or unset. Task 1, the whole-plan builder, a task with a rework note, a task with a saved crash session, a task whose previous session is not the one of task N-1, and every task of a Codex project start fresh, and a failed carry gets one fresh builder. The carried run keeps the agent, tools, permissions and turn limit. The ledger line of a task ends with `continued` or `fresh (<reason>)`; with the setting off it stays `fresh`. Fix rounds are unchanged. `factory tokens --cache` shows the mean end context per role (AVG-END).
- Ledger lines: `Ruling: per-task builders, <total> tasks` once, then `Task N: dispatched (BASE <sha>, <model>)` and `Task N: implementer done (<short sha>)` per task. `tick_plan` still writes `Task N: complete` after the merge.
- Board field `task_index` (number of finished tasks, each committed by the driver, skipped when 0): a budget or stall kill in task 2 leaves task 1 committed, and a `rework` resumes at `task_index`. After the last task the driver records `head`, checks the diff cap and moves to review once. Fix rounds and `rework` then resume the last task's session, or start a fresh fix builder (see "Builder escalation and the fresh fix builder"), and never restart the task loop. The reviewer diff `prev_head..head` is set once, after the last task.
- The dashboard reads the ledger marker `per-task builders` to show each task's true state (`review` after its implementer is done, `active` while dispatched, `pending` before). Jobs without the marker keep the stage-based display.
- The driver now passes `--agent factory-builder` for the whole-plan, task and fix builders. Before this it never did.
- Token model (modelled, not measured): a fresh agent for task k pays off when the context it would have inherited, P_k, exceeds `(9F + (n_k+9)S) / n_k` (F = fixed start of about 19K to 38K tokens, S = spec plus plan size, n_k = turns of the task). Estimated saving: about 11 percent on a 2-task run, about 17 percent on a 3-task run, slightly negative on very short plans. The main gain is accurate per-task state on the card.
- No checks run between tasks. The checks stage runs the mapped checks once over the whole branch after the last task and writes `checks.md`. Job folders from before this change may still hold `checks-task{n}.md` files and `task N checks` ledger lines; nothing reads them.

## Builder escalation and the fresh fix builder

- Escalation: once `escalate_after_fix_rounds` (default 1) fix rounds were spent without fixing the job, the next fix round runs the builder on `escalation_model` (default `claude-opus-5-5`). The job keeps the model for its later rounds and the ledger says `escalated the builder to <model> (fix round N, <reason>)`. Only the builder escalates; the reviewer keeps its model. A job's own `--model` never escalates. A Codex project without `escalation_model` does not escalate. To turn it off, set `escalate_after_fix_rounds` at or above `fix_rounds`.
- The first escalated round, and a `rework` of an escalated job, cannot resume a session from the cheaper model, so the builder starts fresh. Later rounds on the same model resume that session. A provider switch clears the escalated model and writes a ruling.
- Cold session: a fix round or `rework` that would resume a session that ended longer ago than the cache idle limit, with an end context above `fix_fresh_over_ctx` (default 60000; `0` means always fresh once idle), starts a fresh fix builder. The ledger shows `fresh fix builder (idle ...)`. Codex fix rounds always resume, because the idle limit is the Anthropic cache TTL.
- The fresh fix builder gets the branch, the job diff from base to head, the plan, the spec, `read_first`, `builder_rules`, the KB line and the findings (or the rework note). A round that changes nothing after a checks failure is handled like a resumed one.

## Builder permissions

Why: a headless `claude -p` run cannot answer a permission prompt, and Claude Code ignores repository settings in a folder it does not trust. So the driver gives every builder run its own permissions and no repository needs a settings file.

- Flags: each builder run gets `--permission-mode acceptEdits`, `--allowedTools` with the allowlist, `--disallowedTools` with the deny rules and `--add-dir <job folder>`. The flags are passed again on every resume, because a resumed session without them has its commands refused.
- Base list: `Read`, `Grep`, `Glob`, `Skill`, `Agent`, then `Bash(cd:*)`, `Bash(ls:*)`, `Bash(cat:*)`, `Bash(head:*)`, `Bash(tail:*)`, `Bash(wc:*)`, `Bash(grep:*)`, `Bash(find:*)`, `Bash(pwd:*)`, and the read-only git commands `Bash(git status:*)`, `Bash(git diff:*)`, `Bash(git log:*)`, `Bash(git show:*)`.
- Derived programs: the first word of each `[[checks]]` command and of each `check_hints` entry, after leading `NAME=value` words, becomes `Bash(<program>:*)`. The `rust` preset gives `cargo`, a Go config gives `go`. `git`, `rm`, `rmdir`, `curl`, `sudo`, the shells and the wrapper programs (`env`, `xargs`, `timeout`, `nice`, `nohup`, `time`, `stdbuf`, `setsid`, `eval`, `exec`, `command`) are never derived, because a wrapper runs any other command. A compound check command such as `cd web && bun test` derives only `cd`, its first word, which the base list already has. Then come the `builder_allow` entries; duplicates are removed. A fail-closed config gives only the base list.
- `builder_allow`: extra entries as `Bash(<command>:*)` or `Bash(<command>)`, where the command may hold arguments, such as `git log`. No shell operators, quotes, backslashes or wildcards. A bad entry makes the whole config fail closed. See `docs/reference/project-toml-reference.md`.
- Deny rules: `Edit(//<job folder>/**)` and `Write(//<job folder>/**)` with the canonical job folder path, and the same two rules for each knowledge base folder, so a builder cannot edit the knowledge base. A deny rule wins over any allow from a repository setting, and `Read` still works.
- Guard: the builder must not change `plan.md`, `spec.md` or `progress.md` in the job folder. The driver checks them after each builder run, restores them and parks the job with the kind `builder-tampered` and the reason `builder changed <file>`. This catches writes the deny rules cannot, such as a shell command. A failed restore shows in the park reason. The kind accepts `rework` and `cancel` only, never `approve`.
- Kill switch: `builder_permissions = false` gives the old builder arguments exactly. The reviewer and KB runs are not affected. The guard still runs.
- Known limit: the allowlist is not a sandbox. `find` (with `-exec` or `-delete`) and `git diff`, `git log` and `git show` with `--output` can act outside the worktree. The deny rules and the file guard cover only the job folder files named above and the knowledge base folders. The probes for `find`, `git diff --output`, `Bash(*)` and `acceptEdits` with `rm` have not been run against the real claude, so this has not been probed. Do not rely on the worktree to protect a builder from them.

Probes with the real claude 2.1.303 on haiku in a scratch folder, 2026-10-19:

| Question | Result |
|---|---|
| Compound command `cd sub && python3 ...` with only `Bash(python3:*)` allowed | Runs; `cd` is read-only and always allowed |
| The same with only `Bash(cd:*)` allowed | Denied: "The following part requires approval: python3 ..." (each part is checked) |
| `python3 ... \| tee out.txt` with only `python3` allowed | Denied on `tee` |
| `--add-dir` plus `acceptEdits`, no deny rule | An Edit of a file in the added folder succeeds |
| The same plus `--disallowedTools Edit(//dir/**),Write(//dir/**)` | Edit denied; Read still works |
| Deny rule plus an allow for the same path from `--settings` | The deny wins |
| Bash redirect `echo x > <denied folder>/plan.md` with `Bash(echo:*)` allowed | Denied |
| `--resume <session>` with the permission flags passed again | Works |
| `--resume <session>` without the flags | The command is refused: the flags must be passed on every run |

## Using Codex

Set `agent_cli = "codex"` in `project.toml`, or run `factory project add <path> --agent-cli codex`, to run every agent through `codex exec --json` instead of `claude -p`. Unset or `"claude"` changes nothing. Each usage line in `usage.jsonl` carries the provider, old lines without it read as `claude`, and `factory tokens`, `factory tokens --cache`, `factory costs` and the dashboard card show it.

Limits:

- `max_turns` does not apply. `builder_permissions` and `builder_allow` are ignored; the sandbox below replaces them.
- There is no mid-run budget warning or stop. Codex reports usage only at the end of a turn, so the run total is added to the job tally after the run and the job parks when it is over budget. `stall_minutes` still stops a silent run.
- `cache_ttl` does not apply and is not set. A builder never carries a session into the next task, whatever `builder_carry_session` says (the ledger shows `fresh (codex)`).
- There is no cold-start data, so Codex rows in the COLD STARTS tables show `-` for the start average and 0 for the first-turn figures.
- The prompt is a short preface, the Codex tool map (without its `### Model option` section), then the agent file text. The preface allows the skill catalog when the role has an allowed skill and forbids skills otherwise. A Codex run lists the skills in `~/.agents/skills`, `$CODEX_HOME/skills` (default `~/.codex/skills`) and `<worktree>/.agents/skills`; only the catalog (name, description, path) is in context. The factory sets `skills.bundled.enabled=false` and disables, by canonical path in `skills.config`, every skill the role is not allowed. The allowlist is the role's plugin list: an entry `superpowers` allows the skills in a `superpowers` subfolder, an entry `foo` allows a top-level `foo` folder, and an empty list, or one that matches no installed skill, gives `skills.include_instructions=false`, `skills.bundled.enabled=false` and every found skill disabled by path. The role plugin lists are the skill allowlist that replaced the planned `codex_builder_skills` setting. KB gets `kb-refresh`. Subagents stay off. When the skill a role names is not installed, the agent text keeps its short rules.
- Runs pass `--ignore-user-config`, so your own `~/.codex/config.toml` is not read. The login still works. `OPENAI_API_KEY` and `CODEX_API_KEY` are removed from the child unless `FACTORY_ALLOW_API_KEY=1`.
- Runs set the `skills.*` flags above (`skills.bundled.enabled`, `skills.config`, `skills.include_instructions`) as `-c` values.
- Runs set `features.apps=false` and `features.plugins=false`, so ChatGPT apps and Codex plugins add no tools. Codex has no switch that drops every MCP server, and `-c mcp_servers={}` merges instead of replacing.
- Runs set `approval_policy="on-request"` and `approvals_reviewer="auto_review"` explicitly, which enables automatic review for fresh and resumed sessions while keeping the configured role sandbox boundary.
- Switching a project from one CLI to the other clears the saved session ids of a running job once. One fresh builder runs next.

Sandbox by role:

| Role | Sandbox |
|---|---|
| builder, fix builder | `workspace-write`, network on |
| kb | `workspace-write` in the KB folder (plus the KB repo's git folder when `kb_auto_commit` is on), network off |
| reviewer | `read-only`, network off |

The real smoke test is `#[ignore]`; run it by hand with `cargo test -j4 -p factory-agent real_codex_smoke -- --ignored`.

## Checking what agents see

`factory probe` builds each role's real agent spec (builder, reviewer, spec-reviewer, kb), adds a unique marker to the agent text, and asks the agent in one turn, with every tool denied, what reached its context. It reports a pass or fail per channel and role:

| Channel | What a pass proves |
|---|---|
| `agent_text` | The agent file text arrived, because the agent echoes the marker. |
| `skills` | Every skill the role may use is visible. |
| `bootstrap` | The `You have superpowers` bootstrap text from the plugin start hook arrived. |
| `no_extra_skills` | The agent sees no skill outside the role's allowlist. |

A channel the role does not expect shows `-`, or `extra` when it is present anyway. `skills` is expected only when the role's allowlist is not empty. `bootstrap` is expected only on Claude, for a role whose plugin list has `superpowers`. `no_extra_skills` is expected for every Codex role, but on Claude only for the reviewer and spec reviewer. After the table, each role gets a `Skills seen (<role>):` line with the raw answer (`NONE` when empty), so the `--disable-slash-commands` result is readable, then a `Missing (<role>) <channel>: <detail>` line per failed channel and a `Not installed (<role>): ...` line when it applies. The report ends with `Report:` and `Transcripts:` paths.

```bash
factory [--project <name>] probe [--harness claude|codex] [--role <role>]... [--json] [--delivery agent|append|stdin]
```

The harness defaults to the project's `agent_cli`, the roles to all four. `--delivery` picks how the marker reaches Claude: an `agents.json` passed with `--agents` (`agent`), `--append-system-prompt` (`append`) or the start of stdin (`stdin`). The probe exits 1 when an expected channel is missing, 2 on bad flag values. Reports and per-role transcripts are written under `<state dir>/probes/`; `--json` prints the report and nothing else.

The probe runs in a scratch folder under the state dir, not in the repo or a job worktree, so project-level context (the repo's `CLAUDE.md`, `.claude/settings.json`, `.claude/skills`) does not reach it. A pass on `no_extra_skills` covers user and plugin skills only, and real runs may still see project skills. The probe agents file lives in the scratch folder, so it never touches the repo.

Each run costs one short turn per role with real credentials, so `cargo test` and `factory run` never start it. Run it by hand, and before each release (see `docs/operations/releasing.md`).

Findings (2026-11-22):

- Claude 2.1.330: agent text reaches every role. The builder gets the superpowers bootstrap and skills. The reviewer and spec reviewer list no skills, so `--disable-slash-commands` blocks skills. The builder and kb also list 21 user and plugin skills, which the Claude probe does not check.
- Codex 0.169.0 on `gpt-5.6-luna`: agent text reaches every role. The builder lists exactly the 14 superpowers skills, so the allowlist works with plugins and apps off. The other roles named a `factory` skill taken from the command-only `### Model option` section of the map, which run prompts now leave out. Their catalog is empty.
- Headless manual runs of `/factory status` and `$factory status` both worked. Claude refused two commands that held a shell variable (`B=${FACTORY_BIN:-factory}; ...`), which is why the command now calls `factory` literally.
- Under Codex, a role's named skill that is not installed (kb's `kb-refresh` lives in `~/.claude/skills` only) shows `Not installed (kb): kb-refresh, short rules used` and does not fail the probe. The probe checks a partly absent allowlist against the skills that are installed (`evaluate_with_absent`).

## Harness text

The `/factory` command body and the agent files name no harness tool. Tool names live in one mapping file per harness, `tool-map-claude.md` and `tool-map-codex.md` in `crates/factory-core/assets/`. The Claude map is also the tail of `global-command.md`, and a test pins the two equal. The Codex skill is the shared body with `/factory` changed to `$factory`, then the Codex map. The shared command body must end with the exact Claude map, and `factory install` refuses otherwise, so no Claude tool name reaches the Codex skill. Codex runtime agent text is the preface, then the Codex map without its `### Model option` section (`codex_runtime_tool_map`), then the agent body; the installed `$factory` skill keeps the whole map. Claude agent runs need no map. A third harness needs one map file and its own word list in `crates/factory-cli/tests/tool_map.rs`, next to `CLAUDE_WORDS` and `CODEX_WORDS`. The prompt strings in `specs.rs` and `kb.rs` still name the Read, Agent and Workflow tools and are out of scope here.

## Token comparison

| | Raw tokens | Weighted tokens | Status |
|---|---|---|---|
| Job 001 (v1, Opus builder, orchestrator session, 6 agents) | 12.75M | 2.17M weighted | measured baseline |
| New design, comparable job | not stated | about 0.9M weighted | estimate |

The baseline: the orchestrator was 53.6 percent of the raw tokens and commit helper agents about 7 percent. The estimate is arithmetic, not a measurement: it removes those two shares and the five extra reviewers (about 39 percent of the baseline remains, 2.17M x 0.39), and it ignores the interactive brainstorming and planning tokens, which the user spends in their own session. Real jobs have since replaced the estimate: `factory costs` prints the measured median per plan-size group.

## Token baseline

Measured on job 001 (v1, LLM orchestrator, full review set): about 12.75M raw tokens, about 2.17M weighted (input + cache creation + output + cache read / 10), orchestrator 53.6 percent of the total, commit helper agents about 7 percent, diff of 23 KB read by 5 reviewers.

`factory tokens <id>` prints the board totals, then raw and weighted totals per agent role and the percent of that weighted baseline. The per-role numbers come from `usage.jsonl` in the job folder, one line per agent run, written by the engine. Compare like with like:

| Measure | Job 001 (v1) | Lean job target |
|---|---|---|
| Weighted total for the whole job | about 2.17M | well below, aim under 1.1M for a comparable small job |
| Orchestrator share | 53.6 percent | not counted: the driver is Rust, the interactive session only plans |
| Reviewer runs per clean round | 5 | 1 |
| Commit helper agents | 2 or more per commit | 0 |

Since 2026-10-16 each run's output is the larger of the message sum and the result event, and each usage.jsonl line has ts, model, turns, resumed, stop, round, start_ctx, usage_v 2, cache_creation_1h (the 1 hour part of cache_creation, since 2026-10-18), secs (the run time, since 2026-10-20), start_cc (the cache creation of the first turn) and, for builder runs, tasks_done. The later fields are absent on older lines. Lines without usage_v are old and their output counts are under-counted, so factory tokens prints a note for such a job. factory tokens also prints one row per run. Jobs 001 to 011 are all old.

`factory tokens --cache [--days N]` is the project-wide cache report: prompt cache use per role, and every resumed run with its idle gap, creation, read, share and a rewrite flag (share of 25% or more). It reads the `usage.jsonl` of every board job, writes nothing, and needs the `secs` field (written since 2026-10-20) to show a gap, otherwise the gap is `?`.

Cost report: jobs are grouped by plan size, from the bytes of plan.md: small is under `size_small_max_bytes` (12288), medium is up to `size_medium_max_bytes` (25600), complex is above. Both are config keys and invalid values fail closed (small must be above zero and below medium). `factory costs` prints every job (stage, group, tasks, changed lines, files, weighted and raw tokens, runs; a ~ marks jobs whose usage.jsonl has lines without usage_v 2, so their output is under-counted), then a table per group (jobs, median, range, notes: `few jobs` under 3 jobs and the approximate count), a corrected section with the figures over exact jobs only, and the role split. It writes .superpowers/factory/costs.json with the group figures, the thresholds and the role split (not the per-job list). If the board cannot be loaded the command fails and leaves the existing costs.json untouched. The driver rebuilds costs.json after every merge, best effort. Only merged jobs with data feed the figures. `factory new` stores an estimate on every job it makes from a plan (group, median, low, high, n, n_approx) from the history at that moment, falling back to all sizes when the group has no jobs, and prints it.

`factory costs --trend` compares the cost per changed line around each marker from `[[settings.cost_markers]]`. Before is every eligible job below the cut, after runs to the next cut or the latest job. A side needs 3 jobs per size group, otherwise the group prints `not enough data` and no number. Per side it shows the median weighted tokens per changed line, the median turns, the resumed-run share, the rework rate (`round >= 2`) and the park rate (`needs-human` in `stage_history`). The verdict has a 10% dead band: `fell` at -10% or lower, `rose` at +10% or higher, else `flat`. `costs.json` carries the same report in the optional `trend` field, absent without markers.

The plan-time session (brainstorming and writing plans) is interactive and is not in `factory tokens`. Note it by hand from the session's usage line when comparing totals. The target is a guideline taken from the spec's success check ("well below 2.17M weighted"), not a gate.

## Test coverage of the removed v1 helper

The v1 Python helper and the event hook are deleted. Their behaviors are pinned by Rust tests:

- ids, slugs, board writes, stage moves: `factory-core` (`ids::tests`, `stage::tests`, `board::tests`, `fsutil::tests`).
- ledger lines and `Task N: complete` at merge: `factory-core` `ledger::tests`, `factory-engine` `tick::tests`.
- merged check and main root: `factory-git` `is_merged_*`, `open_reports_the_checkout_root_from_a_subdirectory`.
- review chips on the board: `factory-engine` `review_state_*` and `a_fix_round_resets_the_review_state`.
- events: `factory-agent` `sink_*` tests, and `tests/privacy.rs` for url query stripping, concurrent appenders and no tool output.
- job id on each event: `run_series_chains_plans_and_each_job_writes_its_own_job_id_into_events`.
- agent and command files, settings, cutover: `factory-cli` `tests/agent_lint.rs`.

## Open items

- The two-plan dry run below was written before the factory ran real jobs. Since then it has merged over 100 jobs on this machine, including series of up to four plans (jobs 009 to 012), so the recipe stays only as a manual check of a fresh install. Real `claude -p` and `codex exec` behaviour is also covered by `scripts/live-smoke.sh` (see `docs/operations/releasing.md`) and by `factory probe`.
- Light theme in a headless browser counts as checked only when `factory-shots` does not print the note that the light screenshot equals the dark one. In the plan 08 run, headless Chrome stayed on the light scheme for all three shots, so the dark theme is still unchecked by eye.
- `factory-shots` needs a fresh build of the dashboard server (`cargo build -j4 --manifest-path dashboard/Cargo.toml`) and `google-chrome`, `chromium`, `chromium-browser` or `firefox` on the PATH. It sends a `Host` header, because the server refuses a request without one.

## Rulings

- Ruling (2026-10-13): the dashboard was checked in a headless Google Chrome 155 with `factory-shots` at 1440 px and 400 px. Findings: five stage columns (the board now has six), summary strip, token bar with warn level, review chips with text, the needs-human card with its copy chips and the series chip all render. At 400 px the board scrolls inside its own container and the page does not scroll sideways. The dark and light themes were not told apart by the browser, so only the light look was seen. A human look in a real window is welcome but blocks nothing. The v1 minors are triaged in plan 08 of the v3 series: the Python ctl and hook minors were dropped with the deleted code.
- Ruling (2026-10-13): `factory-core` writes `stage_history` (`{stage, at}`, unix seconds) and `budget_weighted` to the board, because the dashboard reads both. The engine writes one `usage.jsonl` line per agent run in the job folder for `factory tokens`.
- Ruling (2026-10-13): the dashboard shows one spec card per series and one card per plan from the start (plans 01 to 03 of `docs/superpowers/plans/2026-10-13-factory-spec-cards/`). The visual check in a real window (dark, light and phone width) is left to the user, because agents cannot see a browser.
- Ruling (2026-10-16): probe of a real stream: message sum {input 6, cache creation 34108, cache read 34197, output 12}, result usage {input 4, cache creation 17207, cache read 34197, output 322}. The result event covers only the main thread (its cache creation equals the main-thread messages alone, the helper's messages add the rest to the sum), while its output count is the full total and the message snapshots hold partial output. `final_usage` takes the larger per field.

## Dry run recipe (run by the user)

Run this once on a fresh install. It uses real agents, so it costs real tokens. Pick a small feature that needs two plans, for example "Add a per-request timeout setting and show it in the request tab".

1. In a fresh Claude Code session on `main` with a clean tree, start the dashboard: `cargo run -j4 --manifest-path dashboard/Cargo.toml -- --root "$(pwd)" --port 8765`, and open http://127.0.0.1:8765/.
2. Run `/factory <the feature>`. In the session, brainstorm and approve a spec, then a two-plan series (plans end with `Next plan:` lines). Note the session usage line, this is the interactive cost.
3. When the command starts `factory run` in the background, leave the session idle and watch the dashboard. Expect, in order:
   - A job card in build with its worktree and the token counter rising.
   - A `checks.md` in the job folder (`ls .superpowers/sdd/*/checks.md`) with every check passing, or the builder resumed once after a failure.
   - One reviewer run, then the stage moves to approve and merged. A diff that touches a risk path (one of the project's `risk_paths`, for example `Cargo.toml` or an `auth` file) parks in needs-human instead. Run `factory approve <id>` after reading the diff.
   - The merge commit has the driver-written message, or the fixed fallback `chore(factory): merge job NNN <slug>`, and its second parent is the reviewed sha (`git log --merges -1 --format=%P`).
   - Plan 2 starts by itself as the next job and merges the same way.
4. Read the cost: `factory tokens 001` and `factory tokens 002`. Each prints runs, raw and weighted tokens per agent role, and the percent of the job 001 weighted baseline. Add the two jobs and the interactive session to compare with the 2.17M weighted of job 001. Check that the reviewer role shows one run per clean round, and that no commit helper agents appear.
5. Pass criteria (the spec's success check): the interactive session was the only place the user spent tokens by hand and sat idle while the driver worked, the driver built in a worktree, ran the checks, ran one reviewer, merged the reviewed sha and started plan 2, the dashboard showed live stages and per-job tokens, no stage needed manual repair, `git status` is clean in the main checkout, and the logged weighted total is well below 2.17M (aim under 1.1M for a comparable small feature).
6. Clean up with `factory cleanup <id>` for each job and confirm `git branch --list 'factory/*'` shows nothing left.

If any step needs a manual action, note the step and file a GitHub issue for it.
