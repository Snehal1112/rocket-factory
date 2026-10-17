# Factory committer context

You are committing the work of a job in Rocket's software factory. The driver prepared everything below and checks the result again when you finish. Nobody answers questions during a run.

## Commit style

The project uses Conventional Commits with a lowercase type: `type(scope): description`, as in `CLAUDE.md`. Allowed types are feat, fix, docs, style, refactor, perf, test, build, ci, chore and revert. The scope is the crate or area, for example `infra`, `http`, `flow`, `factory` or `frontend`. Keep the subject at 72 characters or fewer, with no trailing period, and put a blank line between the subject and any body. Use the body to say why, in short sentences.

## Standing answers

The commit skill asks questions. Answer them like this:

| The skill asks | Answer |
|---|---|
| Step 1 verdict is `split` and asks to confirm the split | Confirm. Commit each group, at most 4 commits per run. If there are more groups, merge the smallest. |
| One amend candidate matches | Amend it only if it is above the floor and not pushed. Otherwise make a new commit. |
| Two or more candidates match | Take the most recent one above the floor. If none is above the floor, make a new commit. |
| Amend target is HEAD | `git commit --amend`. |
| Amend target is not HEAD | `git commit --fixup` plus `GIT_SEQUENCE_EDITOR=true git rebase -i --autosquash <target>~1`. On a conflict: `git rebase --abort` and make a new commit. |
| Force-push after amending a pushed commit | Never. A pushed commit is not amended in the first place. |
| The prompt names an issue footer | Add it as the last paragraph of every commit message, exactly as given. |
| The prompt says `Issue footer: none` | Add no issue footer. |
| Anything else | Pick the option that makes a new commit and changes no existing commit. |

## Batch runs

For `Kind: batch` the staged change holds several tasks, listed under "Tasks in this change". Make focused commits, normally one per task in that order, never more than 4. The floor is the base of the job, so there is nothing above it to amend.

## Rules that do not change

- The floor is the commit named in "This run". Never rewrite it or any commit below it.
- Never push, merge, stash, or use `git add -A`, `--all` or `.`.
- Stage only paths of the staged change you were given. The driver already staged everything that belongs to this task.
