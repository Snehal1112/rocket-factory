## Harness notes: Claude Code

### Task list

Keep the visible task list with `TaskCreate` and `TaskUpdate`.

### Shell and background runs

Run commands with the `Bash` tool. Run `<factory> --project <name> run <id> --series` as a `Bash` command with `run_in_background` set to true. The harness notifies you when the run ends.

### Skills

Invoke a skill with the `Skill` tool.

### Subagents

Start a subagent with the `Agent` tool.

### File tools

Read files with `Read`, change them with `Edit` and `Write`, and search with `Grep` and `Glob`.

### Interview

Invoke `superpowers:brainstorming` yourself, with the user, in this session, and follow it fully. Spec quality comes from the interview, so do not shorten it. These rules win over the skill's own question style and its choice of path or size. Every question, the first one included, carries your recommended answer and two or three alternatives, so the user can pick one or answer in their own words. The user asked for this because open questions were too hard to answer. The driver needs a written spec, so always take the path that writes a spec and then invokes `superpowers:writing-plans`. If the user skips the questions, follow the shared text. Here the commit after the spec approval comes first (the last bullet of the spec step), then the plans step. Do the interview and the writing yourself, and do not do the builder's or reviewer's work.

### Plan writing

Invoke `superpowers:writing-plans` yourself and write the plans in its style. The factory driver is the execution method: skip the skill's execution handoff question and do not offer to run the plans yourself. The plan header is the skill's template, with the repo-relative spec path (`docs/superpowers/specs/<date>-<slug>-design.md`) as its Spec line, and the header's line about who executes the plan is replaced by this executor line, whatever the skill's own wording is: `> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.` Write Review Focus as the skill defines it, as a numbered list. Run the skill's plan self-review and fix its findings.

### Commit

Commit through the `dev-workflow-skills:1-git-commit` skill with a pathspec.

### Model option

A feature may start or end with the single token `--opus`. Remove that token from the text and pass `--model claude-opus-5-5` to `<factory> --project <name> new`. It may likewise start or end with the single token `--one-job`, in any order together with `--opus` (either token may be at the start or the end), handled the same way: remove it from the feature text and from the slug, and pass `--one-job` to `<factory> --project <name> new`. When you add the job, add `--model claude-opus-5-5` when `--opus` was given and `--one-job` when `--one-job` was given.
