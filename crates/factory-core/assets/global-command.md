---
description: Run the software factory on the current project (brainstorm, plan, then the driver builds, checks, reviews and merges)
argument-hint: "<feature> [--opus] | status | approve <id> | rework <id> <note> | cancel <id|slug> | cleanup <id> | tickets <id>"
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
- `approve <digits> --without-review`
- `cancel <digits>` (nothing after the digits)
- `cancel <slug>`: one kebab-case word, not only digits, that is the slug of a draft listed under `drafts:` by `<factory> --project <name> status` (run it to check). When no draft has that slug, the whole argument is a feature text.
- `cleanup <digits>` (nothing after the digits)
- `tickets <digits>` (nothing after the digits)
- `rework <digits> <note>` with a non-empty note

Anything else is a feature text, for example `next button for pagination`, `status page for jobs`, `approve button for forms` or `cancel dialog 12 layout`. A feature may start or end with the single token `--opus`. Remove that token from the text and pass `--model opus` to `<factory> --project <name> new`. If the argument is `rework <digits>` with no note, print `rework needs a note: /factory rework <id> <note>` and stop without calling `<factory>`. If the argument is empty, print the argument hint and stop.

## Other subcommands

Each is one call and a one-line report of the output, nothing more:

- `status`: `<factory> --project <name> status`
- `approve <id>`: `<factory> --project <name> approve <id>`. The driver refuses a `needs-human` job whose code review has not run (the checks never passed or the reviewer never ran) and says so. Report that refusal as it is and offer `/factory rework <id> <note>`, or `/factory approve <id> --without-review` to merge unreviewed, which the user must ask for.
- `approve <id> --without-review`: `<factory> --project <name> approve <id> --without-review`
- `rework <id> <note>`: `<factory> --project <name> rework <id> '<note>'` (the driver accepts a one-line note of at most 200 characters and refuses anything else)
- `cancel <id>`: `<factory> --project <name> cancel <id>`
- `cancel <slug>`: `<factory> --project <name> cancel <slug>` (it removes the draft card and keeps its spec file)
- `cleanup <id>`: `<factory> --project <name> cleanup <id>`
- `tickets <id>`: `<factory> --project <name> tickets <id>`

Always escape the note when you put it in the shell: wrap text in single quotes and write an embedded single quote as `'\''`.

## `<feature>`: new job

The driver validates the spec and the plans when it creates the job, so both must exist, and be committed, before `<factory> --project <name> new`. Run git and `<factory> --project <name> new` from the repository top, even when the session started in a subfolder, and write the spec and plan paths relative to that top. Pick one short kebab-case `<slug>` from the feature before step 1, never only digits (the driver refuses those), and use the same slug for `spec start`, `spec stage` and `<factory> --project <name> new`: `new` adopts the draft card by slug, and with a different slug it leaves the draft card behind. `<date>` (YYYY-MM-DD) is the date in the spec path that `spec start` prints. The spec is `docs/superpowers/specs/<date>-<slug>-design.md`. The plans are in `docs/superpowers/plans/<date>-<slug>/` as `plan-01-<slug>.md`, `plan-02-<slug>.md` and so on. There is one copy of each, and both are committed for future reference. Nothing is drafted in a gitignored folder. Keep a visible task list (TaskCreate and TaskUpdate) for steps 1 to 6 and update it at each milestone. The spec and the plans are committed on the branch this session is on. Job branches start from the project's base branch, and the driver copies the spec and the plans into the job folders, so the jobs run either way: the driver resolves `Next plan:` paths against the checkout the plan lives in and stores absolute paths, so a session in a linked worktree works. Tell the user which branch holds the docs, and that they reach the base branch only when that branch merges.

1. Interview the user and write the spec, in this order:
   - Survey the code and the ticket first, so your recommended answers rest on what is there. Reading is allowed before approval, and you need not offer it first.
   - Run `<factory> --project <name> status`. If a draft under `drafts:` looks like this feature, ask the user whether it is the same. If it is, use its slug, run `<factory> --project <name> spec resume <slug>`, read the file it names and continue the interview from the first open question.
   - Otherwise run `<factory> --project <name> spec start <slug> '<feature>'`, escaping the feature text as above. It puts a draft card in the Spec column and prints `draft <slug>`, `stage interviewing` and `file <repo-relative path> (exists|missing)`. Write the spec at that path. When the `file` line says `exists`, read the file and continue from it, never overwrite it. If it refuses because a draft with that slug already exists, ask the user whether it is the same feature: if it is, resume it as above; if not, pick another slug.
   - Invoke `superpowers:brainstorming` yourself, with the user, in this session, and follow it fully. Spec quality comes from the interview, so do not shorten it. A ticket that looks clear is not a reason to skip questions. A detailed ticket, or the user saying the details are in the ticket, does not skip the interview. Use the ticket and the code survey to ask sharper questions.
   - These overrides win where the skill says otherwise:
     - Every question, the first one included, carries your recommended answer and two or three alternatives, so the user can pick one or answer in their own words. The user asked for this because open questions were too hard to answer. It replaces the skill's open first question, its "a single sentence with no list of example answers", its "Offer a short menu only when they're stuck" and its Red Flags row "Options steer".
     - Always size it as a project with a written design. There is no quick-task, small-change or spike path, because the driver needs a spec.
     - If the user takes the skill's opt-out and skips the questions, tell them that `<factory> --project <name> new` needs a spec. If they still decline, run `<factory> --project <name> cancel <slug>` to remove the draft card and stop without building anything.
     - The skill goes from approval straight to `superpowers:writing-plans`. Here the commit after the spec approval comes first (the last bullet of this step), then step 2.
   - Write the spec file as the interview goes, so a later session can resume it. Each time the user answers a question, add one list-item line to its `## Decisions` section with the question, the options and the choice, ending with `Decided by: user`, for example `- Where does the draft live? Options: the spec file, a state folder. Chosen: the spec file. Decided by: user`. A choice you make on your own ends with `Decided by: call` instead. The driver counts only list-item lines that contain `Decided by: user`, and warns when there are none.
   - When you start writing the document itself, run `<factory> --project <name> spec stage <slug> writing-spec`. The spec must have a `## Goal` section of at least 8 words and a `## Acceptance criteria` section with one bullet per check, and no `TBD`, `TODO`, `FIXME` or `???` outside backticks and code fences, because the driver refuses a spec without them. Headings must be exact level-2 lines such as `## Goal`.
   - The spec also has a `## Follow-ups` section that lists, one bullet per piece, the work of the ticket that this spec does not build. Leave the section out when the ticket is fully done. The factory files one ticket per bullet and closes the ticket after the last plan merges.
   - The skill's builder check reads the spec before the user sees it. It is the one subagent this step uses: do the interview and the writing yourself, and do not do the builder's or reviewer's work.
   - Then show the spec to the user and wait for an explicit approval. If the user asks for changes, edit the spec and ask for approval again. Do not write any plan before that approval. After it, stage the spec path, check that `git diff --cached --name-only` lists only that path, and commit through the `dev-workflow-skills:1-git-commit` skill with a pathspec (`git commit --only -- <spec path>`), so files staged by anything else stay out. Then run `<factory> --project <name> spec stage <slug> writing-plans`.
2. Invoke `superpowers:writing-plans` yourself and write the plans in its style. The factory driver is the execution method: skip the skill's execution handoff question and do not offer to run the plans yourself. Save every plan in `docs/superpowers/plans/<date>-<slug>/`. Before the per-plan rules, run `<factory> --project <name> project show <name>` and apply every line of its `plan rules:` block to every plan, as if listed with the rules below. When the output has no `plan rules:` block, there is nothing extra to apply. Rules for each plan:
   - the header is the skill's template, with the repo-relative spec path (`docs/superpowers/specs/<date>-<slug>-design.md`) as its Spec line and its line for agentic workers, which offers a choice of executor, replaced by this executor line: `> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.`
   - a File Structure map before the tasks, an `**Interfaces:**` block in every task, and steps that give signatures and named test assertions instead of function bodies; no placeholders such as TBD, "add error handling" or "similar to Task N"
   - Review Focus as the skill defines it: the input classes and failure modes the spec implies that no task's tests exercise, each with its test added to the task that owns the code
   - no commit steps: the driver commits, so a plan that still has them is harmless but wasteful, because the builder is told to skip every commit step
   - at most 3 tasks, each ending with an independently testable deliverable, each task heading written as `### Task N: <title>`
   - the skill's plan self-review is run and its findings fixed
   - the last line of the file is `Next plan: docs/superpowers/plans/<date>-<slug>/plan-NN-<slug>.md` (the repo-relative path of the next plan file), or exactly `Next plan: none` in the last plan

   A chain holds at most 6 plans, and the driver refuses more. When a feature needs more, split it into more than one series. The first uses `<slug>`, later ones `<slug>-2`, `<slug>-3` and so on, each with its own plans folder (`docs/superpowers/plans/<date>-<slug>-2/`, plan files named with that series slug) and the same `--spec`. Take each series through steps 3 to 6 in turn. Only the series created with `<slug>` adopts the draft card. Only the last series carries `#N` or the issue URL in its feature text; earlier series name the ticket without `#`, because the driver closes the ticket and files the follow-ups when a series' last plan merges.
3. Always validate before creating the job: every plan has at most 3 tasks (count the `### Task` headings), each plan's last line starts with `Next plan:`, the chain of paths leads from `plan-01-<slug>.md` to the final `Next plan: none`, every plan file in the folder is on that chain, and the spec file exists with a non-empty `## Goal` and `## Acceptance criteria` and no placeholder. `<factory> --project <name> new` refuses a plan with `TBD`, `TODO`, `FIXME` or `???` outside backticks and code fences, the same rule as for the spec. If something is wrong, fix the plan with the user. Do not run an invalid series. Then list the plans for the user and wait for an explicit approval of the plans. After that approval stage the plan paths, check that `git diff --cached --name-only` lists only them, and commit through the `dev-workflow-skills:1-git-commit` skill with a pathspec (`git commit --only -- <plan paths>`). If the user asks for plan changes, edit the plans, commit them the same way and ask for approval again. Do not run `<factory> --project <name> new` before that approval.
4. Run `<factory> --project <name> new <slug> '<feature>' --plans docs/superpowers/plans/<date>-<slug> --spec docs/superpowers/specs/<date>-<slug>-design.md`, adding `--model opus` when `--opus` was given. Escape the feature text as above, so quotes, `$`, backticks and newlines stay literal. Its output is `created job <id> in <folder>`. Read the job id and the folder name from it. Report every `warning:` line printed after it to the user (plan-lint findings and the warning that no decision was answered by the user); the job runs anyway. The driver checks the spec and copies it into every job's folder. When the work belongs to a GitHub ticket, put it in the feature text as `#N` or as the issue URL (in a split feature only in the last series, see step 2), so the driver can write the footer and file the follow-ups.
5. Start the run. First run `factoryd is-running`. If it exits 0, a supervisor is running: run `factoryd enqueue --project <name> --job <id> --series`, which prints a request id, then run `factoryd status` once. If the first line of that output contains `reached`, tell the user in one line that the run is queued as that request id and waits for the daily budget, giving the reset time from that line; otherwise tell the user in one line that the factory is running, with the request id. For any other exit code of `factoryd is-running` (including command not found) treat it as not running: run `<factory> --project <name> run <id> --series` as a Bash command with `run_in_background` set to true, and tell the user in one line that the factory is running. Then stop. Do not poll, do not sleep, do not read logs. The supervisor or the harness reports the end.
6. When the run ends (the harness notifies you for a background run; with the supervisor the user asks, or the dashboard shows it), run `<factory> --project <name> status` once and report the line for this job. If it is `needs-human`, add that the user can run `/factory approve <id>` or `/factory rework <id> <note>`. When the status line says the review has not run, the plain approve is refused, so name `/factory approve <id> --without-review` instead.

The driver never asks questions during a run, so the plans must be complete before step 5. A blocker found by the builder moves the job to `needs-human`.
