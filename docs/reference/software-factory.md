# Software Factory for Rocket

Status: v3, a Rust driver. Design: `docs/superpowers/specs/2026-10-13-software-factory-v3-rust-driver-design.md`. Plans: `docs/superpowers/plans/2026-10-13-software-factory-v3/`. v1 (LLM orchestrator, Python helper) and the v2 plan series are superseded.

The Rust driver does everything mechanical: state, checks, diffs, merge, sync, series, budget and events. The LLM is used only where judgment is needed: design and planning with the user, one builder per plan, one reviewer per diff.

## Pipeline

```
/factory <feature>  (interactive, with the user)
  factory spec start <slug> '<feature>' -> draft card in the Spec column (factory spec stage moves it)
  superpowers:brainstorming (full interview, builder check) -> docs/superpowers/specs/<date>-<slug>-design.md, approved, committed
  superpowers:writing-plans -> docs/superpowers/plans/<date>-<slug>/plan-01-<slug>.md ... (at most 3 tasks each, Next plan: line), approved, committed
  factory new <slug> '<feature>' --plans <plans folder> --spec <spec file>, then factory run <id> --series  (background Bash, session idle)

factory run (no LLM tokens):
  worktree -> builder (claude -p, Sonnet) -> checks -> reviewer (claude -p, Sonnet)
                 ^                              |  red: resume builder with trimmed output
                 |  CHANGES, at most 2 rounds   |
                 +------------------------------+
  main moved -> merge main in, re-run checks and a sync review (review-code-r<N>-sync<K>.md, at most 2) -> merge
  APPROVE + no risk + under size cap + budget ok -> merge reviewed sha to local main -> next plan
  otherwise                                      -> needs-human (factory approve | factory rework | factory cancel)
```

## Spec check

`factory new` checks the spec and its plans before it creates anything. Two checks run, and a refusal creates no job, board entry or worktree.

- The decisions check always runs. When the plans touch risky paths and no `## Decisions` item says `Decided by: user`, the call is refused.
- The spec review runs when `spec_review` is true (the default). A read-only reviewer (tools `Read, Grep, Glob`) reads the spec and the plans and answers APPROVE or CHANGES. A CHANGES verdict, or a missing verdict, refuses the call. The review is saved under `<state>/spec-reviews/` and the refusal names the file. When a preset is in use but has no spec reviewer agent file (a preset from before this check), the call is refused with `the spec reviewer agent is missing; run factory upgrade or factory presets refresh, or set spec_review = false`, and no agent runs.

A refusal ends with the hint to record the user's answer as a `Decided by: user` decision, or to pass `--accept-spec "<reason>"` (one line, not empty, at most 200 characters). The `/factory` command shows the findings to the user, fixes the spec with them and runs `factory new` again. The supervisor create endpoint answers a refusal with 409 `conflict` and the same text when it carries the `--accept-spec` override hint (a missing user decision, or a spec review that asked for changes or did not finish). Every other failure of `factory new` stays a 500. It takes `accept_spec` in its body, and waits up to the project's `stall_minutes` plus two minutes for the review.

## Job model

- One job is one feature. It holds a series of plans, each of at most 3 tasks, run one after another, never in parallel.
- Job id `NNN`, branch `factory/NNN-<slug>`, worktree `.claude/worktrees/factory-NNN-<slug>`. The driver creates the worktree with `git2` and starts the builder inside it.
- Job directory `.superpowers/sdd/YYYY-MM-DD-NNN-<slug>/` (gitignored): `spec.md`, `plan.md` (the current plan), `progress.md` (the ledger), `checks.md`, `review-*.diff`, review replies.
- `factory new` without `--plans` makes one job at `build` with no plan, so `plan.md` must be placed by hand in the job folder (`new` prints the path) before `run`. Without it the run parks the job at needs-human with `no plan.md in the job folder` and starts no agent.
- `factory new --plans` makes one job per plan at once. Plan 1 starts at `build`. Plans 2 to N wait at `spec` and share a series id `S<n>` (`series.id` on the board). The spec file given with `--spec` (or `spec.md` beside the plans when `--spec` is left out) is checked and copied into every job's folder as `spec.md`, so every builder and the reviewer can read it. The check needs a `## Goal` of at least 8 words, an `## Acceptance criteria` section with at least one list item, and no `TBD`, `TODO`, `FIXME` or `???` outside code, else `new` creates no job. A section ends only at a level-1 or level-2 heading outside code fences, and only an exact `## Goal` line names the Goal. `--spec` needs `--plans`. Paths in a committed series are repo-relative, and so are the `Next plan:` lines. `run --series` runs the next waiting job when the plan before it merges, and `run <id>` on a waiting job moves it to `build`. A series with one plan has no id. A series made before this change has no id either, and its next job is created after the previous one merges. Cancelling one plan stops the series and leaves the later plans waiting at `spec`, so cancel each of them too if the series is dropped. Plans are copied into their job folders when `factory new` runs, so edit a plan's `plan.md` in its job folder, not the committed plan file, and do not change a `Next plan:` line after `new`.
- `.superpowers/factory/board.json`: stage per job. Stages `spec`, `build`, `review`, `approve`, `merged`, `needs-human`. Same names and fields as v1, so the dashboard keeps working.
- `park.json` (job folder, written by the driver when it parks a job): `kind`, `reason`, `at`, `round`, `next` (the commands to run) and `parts` (title, source, text, truncated; each text capped at 200 KB, keeping the end). Kinds: `checks-failed`, `changes-remain`, `risky-diff`, `diff-too-large`, `budget-stopped`, `builder-stopped`, `builder-blocked`, `builder-committed`, `nothing-changed`, `commit-failed`, `other`. The board job has `park_kind` and `park_at`, cleared when the job leaves needs-human. `factory status` prints the reason, the first 12 lines of the first part and the next commands for each parked job.
- `.superpowers/factory/events.jsonl`: events written by the driver, each with the job id.
- Only the driver writes the ledger, board and events. Agents and the `/factory` command never do.
- Config: `.claude/factory/config.toml` (`risk_paths`, `diff_cap_lines`, `budget_weighted`, `fix_rounds`, `stall_minutes`, `builder_model`, `reviewer_model`, `reviewer_model_large`, `reviewer_large_over_lines`, `commit_model`, `commit_subagents`, `commit_budget_weighted`, `commit_batch`, `builder_excerpt`, `builder_permissions`, `builder_allow`, `tickets_on_merge`, `cache_ttl`, `kb_path`, `kb_model`, `kb_budget_weighted`, `kb_auto_commit`, `[[checks]]`).

## Modes

- Default: builder and reviewer run on Sonnet.
- `/factory <feature> --opus`: this job uses Opus (`factory new --model claude-opus-5-5`). Every agent of the job runs on it: builders, fix rounds, the reviewer, the committer, the merge message and the KB refresher, and their helper agents. `factory new` refuses a blank or Fable model, and a Claude model on a Codex project. The model is never in the agent files, the driver passes it.

## Agents (`.claude/agents/`)

| Agent | Tools | Duties |
|---|---|---|
| `factory-builder` | Bash, Read, Edit, Write, Grep, Glob, Skill, Agent (`BUILDER_TOOLS`) | Runs headless in the worktree. Follows the implementer rules of `superpowers:subagent-driven-development` (its implementer-prompt.md), one fresh builder per task. It is the implementer, not the controller: no implementer or reviewer subagents and no per-task review (the driver does the ledger, commits and final review), with a `Rules in short` digest of TDD, systematic debugging and verification. Never commits, the driver does (see Commits). At most 2 helper agents per task, only to read or search code. The reply starts with `Status: DONE`, `DONE_WITH_CONCERNS`, `BLOCKED` or `NEEDS_CONTEXT` (a first-line `BLOCKED: <reason>` is still accepted), then at most five concern lines, then `Ruling: <what> — <why> — <cost if wrong>` lines. BLOCKED and NEEDS_CONTEXT park the job as `builder-blocked`. |
| `factory-committer` | all | Runs headless in the worktree and makes the commits through `dev-workflow-skills:1-git-commit` (see Commits). Also runs in message-only mode (role `message`, tools `Read,Skill`) to write merge and sync messages. |
| `factory-code-reviewer` | Read, Grep, Glob | Loaded by the driver with `--agent`. The prompt names the round diff, the plan, the spec and, from round 2, the earlier findings and the builder's reply to them. It also carries the plan's Review Focus, the builder's rulings and concerns recorded while building, and the plan outline. One combined review: correctness, DDD placement, serde, error handling, persistence compatibility, tests, and the hard rules (shadcn, lucide, SingleLineEditor vs Monaco, Zustand selectors, no unwrap, no git CLI). At most 60 lines, findings tagged `[blocker]` or `[minor]`, last line `VERDICT: APPROVE` or `VERDICT: CHANGES`. |

There are no spec-writer, security, ux, ui or approver agents. Risky diffs go to the human.

## Commits

- The builder never commits. The driver stages everything not ignored and runs the `factory-committer` agent (skill `dev-workflow-skills:1-git-commit`), once per job in a task plan (the batch committer, see below), once per round otherwise.
- Batch commit (`commit_batch`, default on): after each task the driver makes a `wip: task N <title>` snapshot commit with git2 and no agent run. After the last task it records the snapshot tip in the board field `batch_snapshot`, soft-resets the branch to `base` and runs one committer (`Kind: batch`, floor `base`, cap `2 * commit_budget_weighted` because it sees the whole job diff). A pending `batch_snapshot` marks an unfinished batch: the next build call (after `factory rework`) finishes it without new builders, and `factory approve` refuses the job. If the committer fails twice, the fallback makes one `chore:` commit per snapshot. With `commit_batch = false` the driver runs one committer per task as before.
- Floor: the job base in round 1, the previous head in a fix round. After a red-check round the floor is the unreviewed head, which is stricter and safe. The committer may amend or fixup above the floor and never at or below it.
- Checks after each run: the floor is an ancestor, the tree is clean, at most 4 net-new commits, the subject is valid, HEAD is on the job branch. A failed run (no commit made, a dirty tree, BLOCKED, an unavailable skill) gets one retry by resuming, then a `chore: <task title>` fallback commit. After the retry, a validator or commit-count failure keeps the commits and writes a ruling, with no fallback commit. A message problem never parks the job.
- Merge and sync messages come from the committer in message-only mode (role `message`, tools `Read,Skill`), with the fixed message as fallback (`chore(factory): merge job <id> <slug>`, `chore(factory): sync main into job <id> <slug>`). The fallback messages also get the driver footers, and `Factory-Job: <id>` is added even for a job with no issues.
- Parks: `builder committed on its own (the branch was at <sha7> before the run)` (in a task plan `builder committed on its own in task N (the branch was at <sha7> before the run)`), `builder made no changes`, `task N changed nothing`, rewriting the floor, `committer moved HEAD off the job branch`, `the job branch is gone`, and `the committer left nothing to commit: <why>`. With `commit_batch` on, a builder that moved HEAD off the job branch parks with `builder moved HEAD off the job branch in task N ...` before the snapshot. The reason names the detached sha when known and tells the person to note it (`git rev-parse HEAD`), run `git checkout <branch>` in the worktree (uncommitted changes stay), then `factory rework <id>`. A rework note still on disk when a batch finishes is copied into a ledger ruling and removed, so a later fix round uses its findings file. A batch that finds the branch moved parks with `the branch moved during the batch commit; run `git reset --soft <base7>` in the job worktree, then `factory rework <id>``. If the branch holds the snapshot work as non-`wip:` commits with the same tree, the driver instead treats the batch as done (a ruling `batch commit already done`). To recover from the builder-committed parks, run `git reset --soft <sha7>` in the job worktree (the sha is in the park reason), then `factory rework <id>`.
- Tokens: `factory tokens` shows the roles `committer` and `message`, each run capped by `commit_budget_weighted`.

### Issue references

- The board job has an `issues` field: the `#N` numbers found in the `factory new` feature text only (every job of a series gets the same list). `spec.md` is never read for issues, because a spec that merely cites an issue would otherwise close it with `Fixes:`. Without a number the field is empty and no issue footer is written.
- Footers the driver checks or adds:

| Commit | Footer lines |
|---|---|
| task, legacy and fix commits (written by the committer, checked by the driver) | `Relates to: #N` |
| merge message | `Fixes: #N`, then `Factory-Job: <id>` |
| sync message | `Relates to: #N`, then `Factory-Job: <id>` |

- A task or fix commit without the `Relates to` line is kept, with a ruling. The fallback commit carries it.
- `Fixes` closes the issue when the user pushes the merge to GitHub. A job that only helps an issue should be created without the number in its text, so the merge does not close it.

### Knowledge base (read side)

- `kb_path` in `config.toml` is the Rocket KB folder, an absolute path or one that starts with `~/` (expanded with `HOME`). Unset turns the KB off. Builders and the reviewer get the folder as `--add-dir`, because it is outside the job worktree.
- With it set, the builder (first, per-task, resume and fresh fix) and reviewer prompts get a line: read `README.md` in that folder first, then only the topic pages that apply, treat the KB as read-only, let the source and the spec win on a conflict, say in the reply when a page is stale, and skip it if the folder does not exist. The reviewer keeps `Read,Grep,Glob`. The committer prompt has no KB line.

Whether headless agents can read the KB through `--add-dir` is not proven yet. The first real run verifies it.

### Knowledge base (refresh side)

- Config: `kb_path` (see above), `kb_model` (default `claude-sonnet-5-5`), `kb_budget_weighted` (default 150000 per run) and `kb_auto_commit`. `kb_auto_commit` is parsed and reserved. Automatic KB commits are not built, so the KB edits always stay uncommitted in the KB folder.
- After a merge lands, the driver runs the `factory-kb-refresher` agent. The post-merge refresh runs inside the merge step, so the merge call returns after it. The agent runs in the main checkout with the KB folder as `--add-dir`, runs the `kb-refresh` skill (its own subagents through the Agent tool, never `Workflow`) and replies with the pages changed, one per line, or `No relevant changes`.
- What it may touch: the run uses permission mode `dontAsk`, so the allowlist is binding. Edit and Write are scoped to the KB folder (`Edit(//<kb path>/**)`, `Write(//<kb path>/**)`). Bash is read-only git (`git log`, `diff`, `show`, `rev-parse`, `merge-base`, `status`), the same with `git -C <repo root>` tied to the main checkout path, plus `date` and `ls`. The guard below checks the main checkout fingerprint (HEAD, every ref, and the content of non-ignored changes). Ignored paths are not covered. The dangerous case is `.claude/worktrees/`, where live job worktrees sit, so a change there goes unnoticed.
- The guard: the driver takes a fingerprint of the main checkout before and after the run (tip, status entries and file contents). A difference is `Tampered`, whatever the reply says. The fingerprint can show a false `Tampered` if you edit the main checkout while a refresh runs. It fails closed, and a manual run clears it.
- The stop marker: `Tampered` writes `.superpowers/factory/kb-stopped`, and automatic refreshes are skipped while it exists (a ledger line says so). Clear it by running `factory kb-refresh --job <id>` after you looked, or delete the file. A false `Tampered` is cleared only by a manual run that ends `Refreshed` or `UpToDate`.
- Failures never park or unmerge a job. A failed, over-budget or blocked run writes a ledger line `kb refresh failed, <reason>`. Success writes `kb refreshed, pages: ...` or `kb up to date`.
- `factory tickets <id> [--dry-run]`: file follow-up tickets and close the parent tickets of a merged job (see Ticket hygiene).
- `factory kb-refresh --job <id>` reruns it by hand. `--job` is required, the command refuses unless the job is in stage `merged`, and it refuses while the job's lease is live. Do not run it while a driver run is live.
- Tokens: the run is logged with role `kb`, so `factory tokens <id>` shows a `kb` row (and a `message` row for commit messages).

First real run checklist: after merge, set `kb_path = "~/data/rocket/Nl-knowledge-base/rocketapi"` and run `factory kb-refresh --job <a merged job id>` once. Check that:

1. the `kb-refresh` skill loads inside the headless run,
2. the agent can read and write the KB through `--add-dir`,
3. the Agent tool fan-out works,
4. the fingerprint guard stays quiet,
5. `factory tokens` shows the weighted cost of the `kb` role,
6. the refresher can read `projects.json` one folder above the KB folder (the kb-refresh skill step 1). If not, the prompt must say the project is already resolved (it does now, so check the run continues with the given paths).

Record that number here, then adjust `kb_budget_weighted`. Measured `kb` cost: not measured yet.

## Ticket hygiene

The reviewer can use a stronger model on risky or large diffs. `reviewer_model_large` (default `"claude-opus-5-5"`; the same name as `reviewer_model` turns it off) names it, and `reviewer_large_over_lines` (default 800, above zero) is the threshold on the round diff's changed lines. A risky job diff always uses it, a diff exactly at the threshold does not, and the choice is written to the ledger as `review model: <name> (<reason>)`. The weighted-token figure ignores the model, so a stronger model's extra cost does not show in `factory tokens`.

The driver can file the leftover work of a ticket and close the ticket after the last plan of a job merges. It reaches GitHub only through `gh`, behind the `Tracker` port in `factory-engine/src/tracker.rs`. The command is `factory tickets <id> [--dry-run]`, and the config key `tickets_on_merge` (default `false`) runs the same step at the end of `merge_stage`.

- The parent tickets come from the feature text given to `factory new`: `#N` and `https://github.com/<owner>/<repo>/issues/N` both count. A URL for a repository other than `origin` makes the step skip.
- The remaining work comes from the `## Follow-ups` section of the job's `spec.md`. One bullet is one ticket: the title is the first sentence (at most 100 characters), the body is the whole bullet. The new tickets get the parent's labels except `tracking`. A missing or empty section files nothing.
- The step needs a merged job that is the last plan of its series. It files the tickets, comments once on each parent (job id, `main` sha, "not yet pushed" when `origin/main` lacks it, links to the new tickets) and closes each parent that is still open.
- `tickets.json` in the job folder records every created ticket, comment and close, saved after each `gh` call. A re-run, also after a failure, never repeats a call. `--dry-run` makes only read calls and prints each action.
- A `gh` failure is written to the ledger and printed. It never changes the merge result, and `factory tickets` exits non-zero.

Minor tickets: after a merge, `merge_stage` runs `minor_tickets_step` once the waiver step is done. It files one ticket titled `Minor review findings from job <id>: <slug>` whose body is a `- [ ]` checklist of the open `[minor]` lines (not `[fixed]`) of the last review file, with no labels. It does not apply the series gate, runs whether or not `tickets_on_merge` is set, and is switched off with `minor_tickets = false` (default true). `minors.json` in the job folder holds `{"issue": <number>}` once filed; a failure is soft and `factory tickets <id>` retries it.
- Merge footer: `Relates to: #N` when the spec has follow-ups (the ticket is not finished), `Fixes: #N` otherwise. Commit and sync footers stay `Relates to`.

## Logging

The driver and `factoryd` write one JSON line per event, next to `events.jsonl`, the board and the ledger, which are unchanged.

- `<state_dir>/log.jsonl` for the driver (scope `cli`), `<home>/factoryd.log.jsonl` for `factoryd` (scope `factoryd`).
- The level comes from `log_level` in `project.toml` (default `info`). `FACTORY_LOG` overrides it. An invalid `FACTORY_LOG` prints `note: FACTORY_LOG ignored: <why>`.
- stderr shows `warn` and above, plus `sup.notice` at `info`. A failure logs an `error` line and still prints a human line.
- A log failure never changes a stage, a merge or a command. At most one `note: log disabled: <why>` is printed at startup.

| Key | Note |
|---|---|
| `ts` | UTC, RFC 3339 with milliseconds |
| `level` | `error`, `warn`, `info`, `debug`, `trace` |
| `scope` | `cli` or `factoryd` |
| `project` | omitted when the checkout is not registered |
| `job_id`, `stage` | omitted outside a job |
| `event` | dotted name: `job.created`, `stage.change`, `agent.start`, `agent.end`, `check.run`, `git.op` (debug), `task.start`, `task.end`, `commit.done`, `operator.approve`, `operator.rework`, `operator.cancel`, `decide`, `merge.done`, `park`, `step.error`, `sup.start`, `sup.finish`, `sup.reject`, `sup.notice`, `sup.warn`, `sup.error` |
| `msg` | at most 300 characters |
| other | event fields, such as `from`, `to`, `sha`, `role` |

`agent.end` also carries `input`, `cache_creation`, `cache_read`, `output`, `turns`, `resumed`, `round`, `start_ctx`, `end_ctx` and `session_id`. `operator.approve` has `without_review`, `operator.rework` has `note` and `operator.cancel` has `reason` and `force`; they are logged only after the command succeeds.

Rotation: a file over 10 MiB is renamed to `.1`, with `.1` and `.2` moved to `.2` and `.3`; the old `.3` is dropped. Writers append one line at a time, so concurrent processes lose no line.

Redaction: a field whose name contains `token`, `key`, `secret` or `password` (any case) is written as `[redacted]`, except the count `weighted_tokens`. In a check command, the value of a `--name=value` flag with such a name is redacted. Environment variables, prompts and command output are never logged.

`factory log [--job <id>] [--level <lvl>] [--event <name-or-prefix>] [--tail <n>]` prints the matching lines of the log and its rotated files, oldest first. `--level` keeps that level and above, `--event` keeps a line whose event equals the value or starts with the value and a dot (`agent` matches `agent.end`, `tas` matches nothing; an empty value is refused), `--tail` defaults to 100 (0 means all), and `--project <name>` reads that project's file. A missing file prints `no log yet`.

## Draft cards

A draft card tracks a spec that is still being written, before any job exists. It lives in the board's `drafts` list.

- `factory spec start <slug> '<feature>'` creates a draft at the `interviewing` stage. The slug goes through the same slugify as job names. A slug with no letters or digits is refused, and so is a slug of only digits (`a draft slug cannot be only digits`), because `factory cancel 12` means job 012.
- `factory spec stage <slug> <interviewing|writing-spec|writing-plans>` moves the card to another stage.
- `factory spec resume <slug>` prints the `draft`, `stage` and `file <spec path> (exists|missing)` lines.
- `factory cancel <slug>` removes the card and keeps the spec file. A numeric argument still means a job.
- `factory status` lists the cards in a `drafts:` block.
- `factory new` adopts the draft with the same slug and removes it. When no list item of the spec's `## Decisions` section says `Decided by: user`, or the spec has no `## Decisions` section at all, it prints `warning: no decision in this spec was answered by the user` after `created job ...`. The job still runs. A single plan file with no spec text gets no warning.
- The dashboard shows each draft in the Spec column. A card not updated for 24 hours gets a stale marker (dashboard `--draft-stale-hours`, factoryd `draft_stale_hours`).
- An older `factory` binary that rewrites the board drops `drafts`.

Plan lint: `factory new` lints every plan on the chain before any job exists. A `TBD`, `TODO`, `FIXME` or `???` outside backticks and code fences refuses the call and names the plan file and line. Other findings only warn: a missing header field (Goal, Architecture, Tech Stack, Spec, Global Constraints, Review Focus), no `superpowers:subagent-driven-development` executor in the header, a task without `**Files:**` or `**Interfaces:**`, and a commit step. The warnings are appended to the first job's spec warning, after the spec's own warning, and print as `warning: ...` lines after `created job ...`. The job still runs.

## One job per spec

The default is one job per plan. `factory new ... --plans <dir> --one-job` creates one job for the whole spec
instead, and the driver combines the plans. Plans are still written with at most 3 tasks each, but the driver does not
enforce that cap on this path. The 6 plan cap and the chain rules (`Next plan:` lines) still apply. The flag needs `--plans`. With a
single plan it has no effect and `new` prints a `note:` line.

The estimate is `110K + 100K per task` weighted tokens, checked against `budget_weighted`. Over budget, `new`
creates nothing, prints a `needs consent:` line (estimate and budget) on stderr and exits non-zero. Re-run with
`--accept-over-budget` (it needs `--one-job`) or raise the budget. With `--accept-over-budget` the job is created and
`new` prints a `warning:` line with the estimate and the budget. At or under budget, `new` prints a `note:` line with
the same figures. The `/factory` command passes `--one-job` for a
feature that starts or ends with that token and asks the user before accepting the overrun.

## Checks

The builder runs only per-crate cargo commands (`cargo check -j4 -p <crate>`), never a bare `cargo check -j4`: in a fresh job worktree a workspace-wide check builds the whole app and V8, takes more than ten minutes and stalls the run (seen in job 012). Plans follow the same rule. The driver runs the checks itself, chosen from the changed paths by `config.toml`: `cargo check -j4`, targeted `cargo test -j4 -p <crate>`, `yarn tsc --noEmit`, `yarn check`, `yarn test --run --maxWorkers=50%` (half the workers, so the full suite does not time out under its own load), and the factory crate checks. A failure starts a fix round with the trimmed output: the builder resumes its session, or starts fresh when the round is escalated or the session is cold (see "Builder escalation and the fresh fix builder"). The reviewer never sees a red build. Results go to `checks.md`.

### No-check and no-test gates

- `no-checks`: a changed code path that no `[[checks]]` rule matches parks the job before the reviewer runs, even when other paths of the diff did run a check. The reason reads `no check covers N changed code paths: a, b (+k more)`. A path is exempt when it matches `unchecked_paths`. A diff of only `*.md`, `docs/**` or `.claude/**` paths is not code and is not parked. A risky diff is gated too: it parks as `no-checks` before `decide()` can end it as `risky-diff`, and the park carries the review and risk parts. With no config, or a broken one, every path counts as code.
- `no-tests`: a code diff with no test path and no added test marker writes a ledger line and a note to the reviewer. With `require_tests = true` it parks as `no-tests` instead.
- Where they run: in the checks stage, and again in the final re-run of the checks on the merge head (the `AutoMerge` arm), so a fix round that leaves a path with no mapped check still parks `no-checks`. Both park through `park_with`. A parked job offers `approve` (as `approve --without-review` when the review has not run) and `rework`.
- The settings are `code_paths`, `test_paths`, `test_markers` and `require_tests`, see `README.md`.
- The ledger has one line `checks ran: <commands>` or `checks ran: none` before the `auto-merge approved` line.

## Budget

Weighted tokens per job: input + cache creation + output + cache read / 10. Defaults: 1,000,000 per job (`DEFAULT_JOB_BUDGET_WEIGHTED`; a config that sets `budget_weighted` keeps its own value), warn at 80 percent, kill the agent and park the job at 100 percent. Diff cap 4,000 changed lines, 2 fix rounds, a stall limit without events.

## Merge rules

Auto-merge needs all of: no check failure, the latest reviewer verdict is exactly `VERDICT: APPROVE`, the diff is not risky, the diff is within the size cap, the budget is not at stop. Otherwise a fix round (at most 2) or `needs-human`.

Risky means the diff touches any `risk_paths` entry: the collection trust gate, auth, secrets or `{{process.env}}`, persistence formats, `src-tauri/`, `DomainEvent`, `Cargo.toml`, `Cargo.lock`, `package.json`, `tauri.conf.json`, CSP or the harness hooks. A missing or broken config makes every diff risky.

A risky diff gets the same fix rounds as any other (`risky_fix_rounds`, default `fix_rounds`; 0 parks at once). It never auto-merges: after an approve the final checks run on the exact head, and then the job parks as `risky-diff` for a human approve. The park evidence leads with the review. For `risky-diff` and `changes-remain` the parts are, in order:

- `Review`: `verdict <word>, round <N>`, then the open `[blocker]` lines (the whole reply when a CHANGES has none), or `findings file missing`.
- `Risk`: the risky paths, and `added by fix rounds: <paths>` for risky paths that were not in the first review (`risky-paths.txt`). Only on a risky diff.
- `Rounds`: one line per review round, `round N (<source>): <verdict>, <B> blockers`, read from the ledger.
- `Changed files`: the diff stat.

`nothing-changed` after a review fix round, and `no-checks` or `no-tests` parks, put `Review` (and `Risk` when risky) before their other parts. `park.json` also records `verdict` and `risky`. In round 2 or later, before the new review runs, the Review part is the previous round's review, and its text names that round.

The driver merges the reviewed sha only: it checks the branch tip equals the recorded head, preflights main (on `main`, clean, not mid-merge), makes a no-ff merge commit (the committer's message, or the fixed `chore(factory): merge job NNN <slug>` fallback), and verifies the second parent. If main moved it merges main into the job branch inside the worktree, re-runs checks and the reviewer on the sync diff (at most 2 syncs). A sync review is saved as `review-code-r<round>-sync<n>.md`, so the round review `review-code-r<round>.md` is never overwritten. It sets `last_review` with source `sync`, and the ledger line starts `sync review round <round> (sync)`. A conflict goes to `needs-human`. It never pushes and never rewrites history.

## Rework

- A rework of a job that parked at a task boundary is the next round. It bumps `round`, sets `rework_base` to the new round minus one (a full new set of fix rounds) and writes `changes-r<N>.md` with the note first and then the open findings of the latest review. Earlier files (`review-code-r*.md`, `builder-r*.md`, `changes-r*.md`) are kept. It prints `job <id> back to build, round <N>`.
- A job parked mid-task keeps the old path: `rework-note.md` is used, `round` and `rework_base` stay, and it prints `job <id> back to build, same task`.
- The next review reads `last_review.file` as the previous findings, which can be a sync review file.

## Monorepos

A folder inside a git repo can be its own project (`factory project add <repo>/apps/web`, stored as `subpath`). A repo holds one top-folder project or disjoint subfolder projects, never both. Their builds, checks and reviews run side by side. The builder, fix builder and reviewer start in the project folder of the job worktree, for `claude` and for `codex`. Risk paths and check mappings are relative to that folder, and check commands run in it.

- Scope: the driver lists the job's own changes (from the merge base with the base branch) after the builder and every fix round. Any path outside the folder, including either side of a cross-boundary rename, parks the job as `needs-human` with the paths named. It never auto-merges, `factory approve` refuses it, and the driver never reverts files. `factory rework` makes the builder remove them.
- Lock: merges into the base branch take a per-repo lock in the git common dir. A sync runs in the job worktree, outside the lock. The wait is 30 seconds, then the job parks with `another project is merging into this repository, approve again to retry`. Main is re-checked under the lock before a busy main parks the job. The OS releases the lock if its holder dies.
- Contention: while other projects merge, main moves more often, so a job can use more of the sync cap (`MAX_SYNCS`, 2) and park with `<main> moved again after the maximum number of syncs` (from the merge step) or `<main> moved again after N syncs` (from the sync stage). Approve it again.

## How to run

Build once: `cargo build -j4 --manifest-path tools/factory/Cargo.toml -p factory-cli`. Binary: `tools/factory/target/debug/factory`.

| Command | Does |
|---|---|
| `/factory <feature> [--opus]` | Start a draft card, brainstorm and plan with the user, then `factory new` and `factory run <id> --series` in the background. |
| `/factory status` | `factory status`. |
| `/factory approve <id>` | Release a `needs-human` job to the merge step. Refused when the code review has not run, unless `--without-review` is given. |
| `/factory rework <id> <note>` | Send a `needs-human` job back to the builder with a one-line note. |
| `/factory cancel <id>` | Stop and park a job. |
| `/factory cleanup <id>` | Remove the worktree of a finished job. A cancelled job also loses its board card (not in a series with other jobs, not while leased); the job folder stays. |

The command matches subcommands by exact shape, so feature text such as `next button for pagination` stays a feature. The session does not poll while the driver runs. The harness notifies it on exit.

`factory new` validates the spec and the plan chain, so both are written, approved and committed under `docs/superpowers/` first. The driver cannot ask questions during a run, so the spec and the plans must be complete. `/factory` runs `superpowers:brainstorming` 7.0.0 as it is, with a few overrides: every question carries a recommended answer and two or three alternatives, the work is always sized as a project with a written design, and a user who opts out of the questions gets the draft cancelled and nothing built. The session writes the spec at the path `factory spec start` printed as the interview goes, one `## Decisions` list item per answered question with the options and the choice, ending `Decided by: user` (or `Decided by: call` for the session's own choice). The skill's builder check reads the spec before the user sees it, and the spec is committed only after the user approves it. The driver refuses a spec without the Goal and the Acceptance criteria or with a placeholder, and only warns when no decision says `Decided by: user`. Plans come from `superpowers:writing-plans` in its 7.0.0 style, and each header says the driver runs it using `superpowers:subagent-driven-development`, one fresh builder per task. The spec and the plans are committed on the session's current branch, with a pathspec commit. Job branches start from `main`, so the docs reach `main` only when that branch merges. The driver resolves `Next plan:` paths against the checkout the plan lives in and stores absolute paths, so a session in a linked worktree works. The jobs themselves run from the copies in the job folders. Harness rules the driver keeps: every cargo call has `-j4`, no `--workspace`, no `CARGO_TARGET_DIR`, one implementer per worktree. The builder never commits. The driver removes `ANTHROPIC_API_KEY` and `ANTHROPIC_AUTH_TOKEN` from the environment of every `claude` it starts, so jobs use the subscription login and never bill an API account by accident. Only an explicit `FACTORY_ALLOW_API_KEY=1` in the shell that runs the driver turns that off. A project can decide for itself: `allow_api_key = true` passes the environment key to that project's agents (and fails the run when there is none), `allow_api_key = false` keeps it on the login, and `api_key_file` reads a key for that project only from a file that only you can read. A bad key file fails the run and never falls back to the login.

## Dashboard

`tools/factory-server/` (own workspace, not shipped) reads `board.json` and `events.jsonl` and serves a local page. It never writes them, and the factory works the same when it is not running. Run it with `cargo run -j4 --manifest-path tools/factory-server/Cargo.toml -- --root <main checkout>`.

Data flow: `factory driver -> .superpowers/factory/{board.json,events.jsonl} -> factory-server -> dashboard.html`.

- The cost panel below the board reads costs.json through /api/state (costs: groups and roles only, null when the file is missing or broken): a row per size group with jobs, median (~ when some jobs are approximate), range and notes, the exact-counts-only medians, and the role split as a stacked bar with a text legend. A card that is not merged shows its estimate; a merged card shows its cost against the estimate range (within, above or below).
- Agents are matched to a job only by the `job_id` on their events. An event without `job_id` shows under "Agents without a job". There is no matching by cwd or stage.
- The card shows the job id, a `plan k of N` chip (hidden for a single plan), feature, branch, builder round, review chips, time in stage (hidden once merged or needs-human), time per stage from `stage_history` when the board has it, weighted tokens against the budget with a bar (text level `warn` at 80 percent, `stop` at 100), and copy chips for `factory approve <id>`, `factory rework <id> <note>` and `factory cancel <id>`.
- A spec card is built for each series id shared by two or more plan jobs (`specs[]` in `/api/state`). It holds the spec title and goal from `spec.md` in plan 1's job folder, and one row per plan with its state (waiting, building, review, needs-human, merged, cancelled). The card sits in the spec column while any plan is open, in needs-human when a plan is stuck or cancelled, and in merged once every plan merged. Jobs without a series id get no card.
- A plan card shows a `spec S2` chip next to `plan k of N` (hover shows the spec title). A plan that has not started sits in the spec column with every task waiting, and its card leaves out stage times, the token bar, agent activity and the branch box until it runs. The spec card shows "Plans n/N done" as a list that reuses the task rows.
- The server reads the board's four token counters and weights them like the driver (cache reads count one tenth). New fields that are malformed are dropped, never a board error. The budget falls back to 3,000,000 weighted tokens when the board has no `budget_weighted` (boards made under the old default). The driver's own default is now 1,000,000. The card bar shows the board total, or the running total from the agent events when that is higher, so it moves while an agent works.
- Only the last 4 MiB of the log is read at startup. Agents idle for a day are dropped (cap 500). The registry resets when the log is truncated, replaced or deleted. An agent silent for 10 minutes shows `stale`.
- A needs-human card with details (`has_park` in `/api/state`) shows a "See what went wrong" button. It opens a native dialog that fetches `GET /api/jobs/<id>/park` (404 without a file, 422 for a broken file, text capped at 1 MiB, the path taken from the board and kept inside `.superpowers/sdd/`). The popup shows the kind, reason, each evidence part with the lines that matter marked and the first one scrolled into view, and the next commands as copy chips. The polled state never carries the evidence text.
- The page patches only changed text. The status line is not a live region. One short announcement is made when the connection level changes. Bar widths are set through the CSSOM because the CSP forbids inline styles.

## Per-task builders

- The driver runs one fresh `factory-builder` agent per `### Task N:` heading of `plan.md`, one after another, each as its own `claude -p --agent factory-builder` run (`--max-turns 120`). A plan with no task headings keeps the old whole-plan builder.
- Each build call handles one task while the board field `task_index` is below the task count. The prompt names the plan, the spec, the base, the current tip, the earlier commit subjects and "Do only Task N". The builder leaves the commit to the driver (a `wip:` snapshot per task, then one batch committer after the last task when `commit_batch` is on, otherwise one committer per task). A task that changed nothing parks the job with `task N changed nothing`. With `commit_batch` on, `Task N: implementer done (<sha>)` records the `wip:` snapshot sha, which the batch commit replaces.
- Builder excerpt (`builder_excerpt`, default on): the task prompt carries the plan header, the task's own `### Task N:` section and the spec goal and acceptance criteria, and still names the full `plan.md` and `spec.md` for reference. The builder agent reads those files only for something the excerpt refers to outside itself. A plan with no `### Task N:` heading for the task (for example `- [ ] Task N:` checkboxes) falls back to the full prompt. Whole-plan builders, fix resumes and rework resumes are unchanged. `builder_excerpt = false` restores the old prompt.
- Builder carry (`builder_carry_session`, default on, with `builder_carry_max_ctx`, default 45000): the builder of task N+1 resumes the session of task N when the previous builder ended less than the idle limit ago and its `end_ctx` is at most the cap. The idle limit follows `cache_ttl`: 3300 seconds for `"1h"`, 240 for `"5m"` or unset. Task 1, the whole-plan builder, a task with a rework note and a task with a saved crash session start fresh, and a failed carry gets one fresh builder. The carried run keeps the agent, tools, permissions and turn limit. The ledger line of a task ends with `continued` or `fresh (<reason>)`; with the setting off it stays `fresh`. Fix rounds are unchanged. `factory tokens --cache` shows the mean end context per role (AVG-END).
- Ledger lines: `Ruling: per-task builders, <total> tasks` once, then `Task N: dispatched (BASE <sha>, <model>)` and `Task N: implementer done (<short sha>)` per task. `tick_plan` still writes `Task N: complete` after the merge.
- Board field `task_index` (number of finished tasks, each committed by the driver, skipped when 0): a budget or stall kill in task 2 leaves task 1 committed, and a `rework` resumes at `task_index`. After the last task the driver records `head`, checks the diff cap and moves to review once. Fix rounds and `rework` then resume the last task's session, or start a fresh fix builder (see "Builder escalation and the fresh fix builder"), and never restart the task loop. The reviewer diff `prev_head..head` is set once, after the last task.
- The dashboard reads the ledger marker `per-task builders` to show each task's true state (`review` after its implementer is done, `active` while dispatched, `pending` before). Jobs without the marker keep the stage-based display.
- The driver now passes `--agent factory-builder` for the whole-plan, task and fix builders. Before this it never did.
- Token model (modelled, not measured): a fresh agent for task k pays off when the context it would have inherited, P_k, exceeds `(9F + (n_k+9)S) / n_k` (F = fixed start of about 19K to 38K tokens, S = spec plus plan size, n_k = turns of the task). Estimated saving: about 11 percent on a 2-task run, about 17 percent on a 3-task run, slightly negative on very short plans. The main gain is accurate per-task state on the card.

## Builder escalation and the fresh fix builder

- Escalation: once `escalate_after_fix_rounds` (default 1) fix rounds were spent without fixing the job, the next fix round runs the builder on `escalation_model` (default `claude-opus-5-5`). The job keeps the model for its later rounds and the ledger says `escalated the builder to <model> (fix round N, <reason>)`. Only the builder escalates; the reviewer and the committer keep their models. A job's own `--model` never escalates. A Codex project without `escalation_model` does not escalate. To turn it off, set `escalate_after_fix_rounds` at or above `fix_rounds`.
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
- Kill switch: `builder_permissions = false` gives the old builder arguments exactly. The reviewer, committer, message and KB runs are not affected. The guard still runs.
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
- `cache_ttl` does not apply and is not set. `builder_carry_session` is off.
- There is no cold-start data, so the COLD STARTS tables show `n/a` for Codex.
- The agent file text and a short preface go into the prompt. No skill is loaded.
- Runs pass `--ignore-user-config`, so your own `~/.codex/config.toml` is not read. The login still works. `OPENAI_API_KEY` and `CODEX_API_KEY` are removed from the child unless `FACTORY_ALLOW_API_KEY=1`.
- Runs set `features.apps=false` and `features.plugins=false`, so ChatGPT apps and Codex plugins add no tools. Codex has no switch that drops every MCP server, and `-c mcp_servers={}` merges instead of replacing.
- Runs set `approval_policy="on-request"` and `approvals_reviewer="auto_review"` explicitly, which enables automatic review for fresh and resumed sessions while keeping the configured role sandbox boundary.
- Switching a project from one CLI to the other clears the saved session ids of a running job once. One fresh builder runs next.

Sandbox by role:

| Role | Sandbox |
|---|---|
| builder, fix builder | `workspace-write`, network on |
| committer, message | `workspace-write`, the repo's `.git` folder writable, network off |
| kb | `workspace-write` in the KB folder, network off |
| reviewer | `read-only`, network off |

A failed Codex commit falls back to the driver commit. The real smoke test is `#[ignore]`; run it by hand with `cargo test -j4 -p factory-agent real_codex_smoke -- --ignored`.

## Checking what agents see

`factory probe` builds each role's real agent spec (builder, reviewer, spec-reviewer, committer, kb), adds a unique marker to the agent text, and asks the agent in one turn, with every tool denied, what reached its context. It reports a pass or fail per channel and role:

| Channel | What a pass proves |
|---|---|
| `agent_text` | The agent file text arrived, because the agent echoes the marker. |
| `skills` | Every skill the role may use is visible. |
| `bootstrap` | The `You have superpowers` bootstrap text from the plugin start hook arrived. |
| `no_extra_skills` | The agent sees no skill outside the role's allowlist. |

A channel the role does not expect shows `-`, or `extra` when it is present anyway. A `Skills seen:` line per role shows the raw answer, so the `--disable-slash-commands` result is readable.

```bash
factory [--project <name>] probe [--harness claude|codex] [--role <role>]... [--json] [--delivery agent|append|stdin]
```

The harness defaults to the project's `agent_cli`, the roles to all five. `--delivery` picks how the marker reaches Claude: a written agent file (`agent`), `--append-system-prompt` (`append`) or the start of stdin (`stdin`). The probe exits 1 when an expected channel is missing, 2 on bad flag values. Reports and per-role transcripts are written under `<state dir>/probes/`; `--json` prints the report and nothing else.

The probe runs in a scratch folder under the state dir, not in the repo or a job worktree, so project-level context (the repo's `CLAUDE.md`, `.claude/settings.json`, `.claude/skills`) does not reach it. A pass on `no_extra_skills` covers user and plugin skills only, and real runs may still see project skills. Writing the probe agent file into the repo would risk replacing the real one.

Each run costs one short turn per role with real credentials, so `cargo test` and `factory run` never start it. Run it by hand, and before each release (see `docs/operations/releasing.md`).

Findings: the `--disable-slash-commands` and bootstrap answers are recorded here after the first live run.

## Token comparison

| | Raw tokens | Weighted tokens | Status |
|---|---|---|---|
| Job 001 (v1, Opus builder, orchestrator session, 6 agents) | 12.75M | 2.2M weighted | measured baseline |
| New design, comparable job | not stated | about 0.9M weighted | estimate |

The baseline: the orchestrator was 53.6 percent of the raw tokens and commit helper agents about 7 percent. The estimate is arithmetic, not a measurement: it removes those two shares and the five extra reviewers (about 39 percent of the baseline remains, 2.2M x 0.39), and it ignores the interactive brainstorming and planning tokens, which the user spends in their own session. Plan 08 measures a real job, and that number replaces the estimate.

## Token baseline

Measured on job 001 (v1, LLM orchestrator, full review set): about 12.75M raw tokens, about 2.17M weighted (input + cache creation + output + cache read / 10), orchestrator 53.6 percent of the total, commit helper agents about 7 percent, diff of 23 KB read by 5 reviewers.

`factory tokens <id>` prints the board totals, then raw and weighted totals per agent role and the percent of that weighted baseline. The per-role numbers come from `usage.jsonl` in the job folder, one line per agent run, written by the engine. Compare like with like:

| Measure | Job 001 (v1) | Lean job target |
|---|---|---|
| Weighted total for the whole job | about 2.17M | well below, aim under 1.1M for a comparable small job |
| Orchestrator share | 53.6 percent | not counted: the driver is Rust, the interactive session only plans |
| Reviewer runs per clean round | 5 | 1 |
| Commit helper agents | 2 or more per commit | 0 |

Since 2026-10-16 each run's output is the larger of the message sum and the result event, and each usage.jsonl line has ts, model, turns, resumed, stop, round, start_ctx, usage_v 2, cache_creation_1h (the 1 hour part of cache_creation, since 2026-10-18) and, for builder runs, tasks_done. Lines without usage_v are old and their output counts are under-counted, so factory tokens prints a note for such a job. factory tokens also prints one row per run. Jobs 001 to 011 are all old.

`factory tokens --cache [--days N]` is the project-wide cache report: prompt cache use per role, and every resumed run with its idle gap, creation, read, share and a rewrite flag (share of 25% or more). It reads the `usage.jsonl` of every board job, writes nothing, and needs the `secs` field (written since 2026-10-20) to show a gap, otherwise the gap is `?`.

Cost report: jobs are grouped by plan size, from the bytes of plan.md: small is under `size_small_max_bytes` (12288), medium is up to `size_medium_max_bytes` (25600), complex is above. Both are config keys and invalid values fail closed (small must be above zero and below medium). `factory costs` prints every job (stage, group, tasks, changed lines, files, weighted and raw tokens, runs; a ~ marks jobs whose usage.jsonl has lines without usage_v 2, so their output is under-counted), then a table per group (jobs, median, range, notes: `few jobs` under 3 jobs and the approximate count), a corrected section with the figures over exact jobs only, and the role split. It writes .superpowers/factory/costs.json with the group figures, the thresholds and the role split (not the per-job list). The driver rebuilds costs.json after every merge, best effort. Only merged jobs with data feed the figures. `factory new` stores an estimate on every job it makes from a plan (group, median, low, high, n, n_approx) from the history at that moment, falling back to all sizes when the group has no jobs, and prints it.

The plan-time session (brainstorming and writing plans) is interactive and is not in `factory tokens`. Note it by hand from the session's usage line when comparing totals. The target is a guideline taken from the spec's success check ("well below 2.2M weighted"), not a gate.

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

- Dry run of a two-plan feature through `/factory` has not been run yet. The user runs it, see "Dry run recipe" below.
- Real-agent behavior (the actual `claude -p` flags, resume, permission mode) is only proven by that dry run. The scenario tests use `FakeRunner`-style scripted runners.
- Light theme in a headless browser counts as checked only if the screenshot differs from the dark one. In the plan 08 run, headless Chrome stayed on the light scheme for all three shots, so the dark theme is still unchecked by eye. `factory-shots` names the shots dark and light by intent, not by result.
- `factory-shots` needs a fresh `cargo build` of the server and `google-chrome`, `chromium` or `firefox` on the PATH. It sends a `Host` header, because the server refuses a request without one.
- `smoke.sh` and `seed-demo.sh` were not reworked in plan 08.

## Rulings

- Ruling (2026-10-13): the dashboard was checked in a headless Google Chrome 155 with `factory-shots` at 1440 px and 400 px. Findings: five stage columns, summary strip, token bar with warn level, review chips with text, the needs-human card with its copy chips and the series chip all render. At 400 px the board scrolls inside its own container and the page does not scroll sideways. The dark and light themes were not told apart by the browser, so only the light look was seen. A human look in a real window is welcome but blocks nothing. The v1 minors are triaged in plan 08 of the v3 series: the Python ctl and hook minors were dropped with the deleted code.
- Ruling (2026-10-13): `factory-core` writes `stage_history` (`{stage, at}`, unix seconds) and `budget_weighted` to the board, because the dashboard reads both. The engine writes one `usage.jsonl` line per agent run in the job folder for `factory tokens`.
- Ruling (2026-10-13): the dashboard shows one spec card per series and one card per plan from the start (plans 01 to 03 of `docs/superpowers/plans/2026-10-13-factory-spec-cards/`). The visual check in a real window (dark, light and phone width) is left to the user, because agents cannot see a browser.
- Ruling (2026-10-16): probe of a real stream: message sum {input 6, cache creation 34108, cache read 34197, output 12}, result usage {input 4, cache creation 17207, cache read 34197, output 322}. The result event covers only the main thread (its cache creation equals the main-thread messages alone, the helper's messages add the rest to the sum), while its output count is the full total and the message snapshots hold partial output. `final_usage` takes the larger per field.

## Dry run recipe (run by the user)

Run this once after plans 01 to 08 are merged. It uses real agents, so it costs real tokens. Pick a small feature that needs two plans, for example "Add a per-request timeout setting and show it in the request tab".

1. In a fresh Claude Code session on `main` with a clean tree, start the dashboard: `cargo run -j4 --manifest-path tools/factory-server/Cargo.toml -- --root "$(pwd)" --port 8765`, and open http://127.0.0.1:8765/.
2. Run `/factory <the feature>`. In the session, brainstorm and approve a spec, then a two-plan series (plans end with `Next plan:` lines). Note the session usage line, this is the interactive cost.
3. When the command starts `factory run` in the background, leave the session idle and watch the dashboard. Expect, in order:
   - A job card in build with its worktree and the token counter rising.
   - A `checks.md` in the job folder (`ls .superpowers/sdd/*/checks.md`) with every check passing, or the builder resumed once after a failure.
   - One reviewer run, then the stage moves to approve and merged. A diff that touches a risk path (auth, `src-tauri/`, `Cargo.toml` and so on) parks in needs-human instead. Run `factory approve <id>` after reading the diff.
   - The merge commit has the committer's message, or the fixed fallback `chore(factory): merge job NNN <slug>`, and its second parent is the reviewed sha (`git log --merges -1 --format=%P`).
   - Plan 2 starts by itself as the next job and merges the same way.
4. Read the cost: `factory tokens 001` and `factory tokens 002`. Each prints runs, raw and weighted tokens per agent role, and the percent of the job 001 weighted baseline. Add the two jobs and the interactive session to compare with the 2.17M weighted of job 001. Check that the reviewer role shows one run per clean round, that no commit helper agents appear, and that the roles `committer` and `message` are listed.
5. Pass criteria (the spec's success check): the interactive session was the only place the user spent tokens by hand and sat idle while the driver worked, the driver built in a worktree, ran the checks, ran one reviewer, merged the reviewed sha and started plan 2, the dashboard showed live stages and per-job tokens, no stage needed manual repair, `git status` is clean in the main checkout, and the logged weighted total is well below 2.17M (aim under 1.1M for a comparable small feature).
6. Clean up with `factory cleanup <id>` for each job and confirm `git branch --list 'factory/*'` shows nothing left.

If any step needs a manual action, note the step and add a follow-up row to `.claude/roadmap.md`.
