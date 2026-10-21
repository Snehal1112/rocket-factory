# Commit context

Rules for the committer of this project.

- Use Conventional Commits: `type(scope): description`, with a lowercase type such as `feat`, `fix`, `refactor`, `test`, `docs` or `chore`.
- Make one logical change per commit. Split unrelated changes into separate commits.
- Keep the subject at most 72 characters, in the imperative mood, with no trailing period.
- Explain why the change was made in the body, not only what changed.
- Never mention the factory, the agents or the plan in the message.

## Batch runs

For `Kind: batch` the staged change holds several tasks, listed under "Tasks in this change". Make focused commits, normally one per task in that order, never more than 4. The floor is the base of the job, so there is nothing above it to amend.
