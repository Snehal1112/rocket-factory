---
description: Run the software factory on the current project (brainstorm, plan, then the driver builds, checks, reviews and merges)
argument-hint: "<feature> [--opus] | status | approve <id> | rework <id> <note> | cancel <id> | cleanup <id> | tickets <id>"
---

# Factory

Arguments: $ARGUMENTS

You are a thin front end. The Rust driver does all mechanical work. Never edit the board, the ledger, the events log or any plan checkbox, and do not do the builder's or reviewer's work. The checks, the base branch and the risk rules come from the project's registry entry, not from this command.

## Find the project

1. Run `factory project which` from the current directory (use `$FACTORY_BIN` in place of `factory` when `FACTORY_BIN` is set). Its output is the project name, called `<name>` below.
2. If it fails, show the user its error line, then stop. When the line says `no registered project`, add that they can run `factory project add <path> --preset <name>` for this repository. When it says `ambiguous match`, say the registry holds two names for one repository and must be fixed. When the command is not found, say the factory binaries are not on PATH.

`<factory>` means `$FACTORY_BIN` when `FACTORY_BIN` is set, else `factory` on PATH. Pass `--project <name>` on every `<factory>` call, so each call is `<factory> --project <name> <subcommand> ...`.

## Pick the subcommand

Match by exact shape only, on the whole argument string:

- `status` alone
- `approve <digits>` (nothing after the digits)
- `cancel <digits>` (nothing after the digits)
- `cleanup <digits>` (nothing after the digits)
- `tickets <digits>` (nothing after the digits)
- `rework <digits> <note>` with a non-empty note

Anything else is a feature text, for example `next button for pagination`, `status page for jobs`, `approve button for forms` or `cancel dialog 12 layout`. A feature may start or end with the single token `--opus`. Remove that token from the text and pass `--model opus` to `<factory> --project <name> new`. If the argument is `rework <digits>` with no note, print `rework needs a note: /factory rework <id> <note>` and stop without calling `<factory>`. If the argument is empty, print the argument hint and stop.

## Other subcommands

Each is one call and a one-line report of the output, nothing more:

- `status`: `<factory> --project <name> status`
- `approve <id>`: `<factory> --project <name> approve <id>`
- `rework <id> <note>`: `<factory> --project <name> rework <id> '<note>'` (the driver accepts a one-line note of at most 200 characters and refuses anything else)
- `cancel <id>`: `<factory> --project <name> cancel <id>`
- `cleanup <id>`: `<factory> --project <name> cleanup <id>`
- `tickets <id>`: `<factory> --project <name> tickets <id>`

Always escape the note when you put it in the shell: wrap text in single quotes and write an embedded single quote as `'\''`.

## `<feature>`: new job

The driver validates the spec and the plans when it creates the job, so both must exist, and be committed, before `<factory> --project <name> new`. Run git and `<factory> --project <name> new` from the repository top, even when the session started in a subfolder, and write the spec and plan paths relative to that top. Pick a short kebab-case `<slug>` from the feature and a date `<date>` (YYYY-MM-DD). The spec is `docs/superpowers/specs/<date>-<slug>-design.md`. The plans are in `docs/superpowers/plans/<date>-<slug>/` as `plan-01-<slug>.md`, `plan-02-<slug>.md` and so on. There is one copy of each, and both are committed for future reference. Nothing is drafted in a gitignored folder. Keep a visible task list (TaskCreate and TaskUpdate) for steps 1 to 6 and update it at each milestone. The spec and the plans are committed on the branch this session is on. Job branches start from the project's base branch, and the driver copies the spec and the plans into the job folders, so the jobs run either way: the driver resolves `Next plan:` paths against the checkout the plan lives in and stores absolute paths, so a session in a linked worktree works. Tell the user which branch holds the docs, and that they reach the base branch only when that branch merges.

1. Invoke `superpowers:brainstorming` yourself, with the user, in this session, and follow it fully. Always take its full architectural path: never classify the feature as a spike or a bounded change, because both skip the spec and the plans that the driver needs. Announce the classification before the first question. Spec quality comes from the interview, so do not shorten it:
   - Read the code, the docs and the open tickets and branches that touch the area first. You may use read-only exploration subagents for the code survey. Do not use subagents for the interview, the design or the writing, and do not do the builder's or reviewer's work.
   - Ask clarifying questions one at a time, as many as it takes, until the user confirms your written-back understanding of the intended outcome. A ticket that looks clear is not a reason to skip questions.
   - A detailed ticket, or the user saying the details are in the ticket, does not skip the interview. Use the ticket and the code survey to ask sharper questions.
   - The questions must cover these topics: purpose and who benefits, constraints, success checks and how each is measured, edge cases and failure behavior, security and compatibility rules that must not weaken, scope against related tickets and branches, non-goals, and the alternatives that were rejected and why.
   - Coverage gate: ask at least one question per topic, or have the user say that the topic does not apply. Do not write the spec until every topic has an answer or an `n/a: <reason>` line from the user. The spec has a `## Decisions` section that lists each topic with the question asked and the answer, or the `n/a: <reason>` line.
   - Every entry of the spec's `## Decisions` section is a question actually asked in this session with the options considered and why the others were rejected. The spec also has a `## Rejected alternatives` section listing the alternatives that were considered and dropped, with the reason for each. If the user declines to answer a topic, record `n/a: <reason>` in their words.
   - Propose 2 to 3 approaches with trade-offs and a recommendation. Present the design in sections and get the user's approval after each section.
   - Save the spec as `docs/superpowers/specs/<date>-<slug>-design.md`. It must have a `## Goal` section of at least 8 words and a `## Acceptance criteria` section with one bullet per check, and no `TBD`, `TODO`, `FIXME` or `???` outside backticks and code fences, because the driver refuses a spec without them. Headings must be exact level-2 lines such as `## Goal`. The spec also has `## Decisions`, `## Rejected alternatives` and `## Non-goals`, and Behavior that must not change, Design and Tests and measurement sections as the topic needs.
   - The spec also has a `## Follow-ups` section that lists, one bullet per piece, the work of the ticket that this spec does not build. Leave the section out when the ticket is fully done. The factory files one ticket per bullet and closes the ticket after the last plan merges.
   - Run the skill's spec self-review (placeholders, contradictions, scope, ambiguity) and fix what it finds. Stage the spec path, check that `git diff --cached --name-only` lists only that path, and commit through the `dev-workflow-skills:1-git-commit` skill with a pathspec (`git commit --only -- <spec path>`), so files staged by anything else stay out. Then show the spec to the user and wait for an explicit approval. If the user asks for changes, edit the spec, run the self-review again, commit it the same way and ask for approval again. Do not write any plan before that approval.
2. Invoke `superpowers:writing-plans` yourself. The factory driver is the execution method: skip the skill's execution handoff question and do not offer to run the plans yourself. Save every plan in `docs/superpowers/plans/<date>-<slug>/`. Before the per-plan rules, run `<factory> --project <name> project show <name>` and apply every line of its `plan rules:` block to every plan, as if listed with the rules below. When the output has no `plan rules:` block, there is nothing extra to apply. Rules for each plan:
   - the header has Goal, Architecture, Tech Stack, the repo-relative path of the spec file (`docs/superpowers/specs/<date>-<slug>-design.md`), Global Constraints and Review Focus, as `superpowers:writing-plans` describes
   - no commit steps: the driver commits, so a plan that still has them is harmless but wasteful, because the builder is told to skip every commit step
   - at most 3 tasks, each ending with an independently testable deliverable, each task heading written as `### Task N: <title>`
   - each task lists exact files and its steps are 2 to 5 minute actions with exact code and exact commands; no placeholders such as TBD, "add error handling" or "similar to Task N"
   - the header also says the plan is executed by a `factory-builder` subagent through `superpowers:executing-plans`, skipping that skill's final whole-branch review, ledger scripts and progress file
   - every acceptance criterion of the spec is covered by a task, and the Review Focus lines name the tests that pin them
   - the last line of the file is `Next plan: docs/superpowers/plans/<date>-<slug>/plan-NN-<slug>.md` (the repo-relative path of the next plan file), or exactly `Next plan: none` in the last plan
3. Always validate before creating the job: every plan has at most 3 tasks (count the `### Task` headings), each plan's last line starts with `Next plan:`, the chain of paths leads from `plan-01-<slug>.md` to the final `Next plan: none`, every plan file in the folder is on that chain, and the spec file exists with a non-empty `## Goal` and `## Acceptance criteria` and no placeholder. If something is wrong, fix the plan with the user. Do not run an invalid series. Then list the plans for the user and wait for an explicit approval of the plans. After that approval stage the plan paths, check that `git diff --cached --name-only` lists only them, and commit through the `dev-workflow-skills:1-git-commit` skill with a pathspec (`git commit --only -- <plan paths>`). If the user asks for plan changes, edit the plans, commit them the same way and ask for approval again. Do not run `<factory> --project <name> new` before that approval.
4. Run `<factory> --project <name> new <slug> '<feature>' --plans docs/superpowers/plans/<date>-<slug> --spec docs/superpowers/specs/<date>-<slug>-design.md`, adding `--model opus` when `--opus` was given. Escape the feature text as above, so quotes, `$`, backticks and newlines stay literal. Its output is `created job <id> in <folder>`. Read the job id and the folder name from it. The driver checks the spec and copies it into every job's folder. When the work belongs to a GitHub ticket, put it in the feature text as `#N` or as the issue URL, so the driver can write the footer and file the follow-ups.
5. Start the run. First run `factoryd is-running`. If it exits 0, a supervisor is running: run `factoryd enqueue --project <name> --job <id> --series`, which prints a request id, then run `factoryd status` once. If the first line of that output contains `reached`, tell the user in one line that the run is queued as that request id and waits for the daily budget, giving the reset time from that line; otherwise tell the user in one line that the factory is running, with the request id. For any other exit code of `factoryd is-running` (including command not found) treat it as not running: run `<factory> --project <name> run <id> --series` as a Bash command with `run_in_background` set to true, and tell the user in one line that the factory is running. Then stop. Do not poll, do not sleep, do not read logs. The supervisor or the harness reports the end.
6. When the run ends (the harness notifies you for a background run; with the supervisor the user asks, or the dashboard shows it), run `<factory> --project <name> status` once and report the line for this job. If it is `needs-human`, add that the user can run `/factory approve <id>` or `/factory rework <id> <note>`.

The driver never asks questions during a run, so the plans must be complete before step 5. A blocker found by the builder moves the job to `needs-human`.
