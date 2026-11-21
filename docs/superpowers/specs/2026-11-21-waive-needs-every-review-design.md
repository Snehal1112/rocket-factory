# Waive needs every review (issue #117)

## Intent

`factory approve <id> --waive "<reason>"` lets a person merge over open code-review blockers. It must
never merge code that a required reviewer never judged. Today it can: when the code review asks for
changes and the security review fails, stalls or never runs, the missing-review rule is skipped
because changes are open (`Job::approve_refusal`, `crates/factory-core/src/board.rs`). The dashboard
then offers exactly that waive command.

## Goal

A job with open code-review blockers and a required role without a verdict (`missing_reviews()` is
not empty: pending, running, failed, or code absent) cannot be approved by any flag. The person
reruns the review (`factory retry`, or `rework` when the park has no resume stage) or cancels.

## Non-goals and anti-goals

- No override. `--waive` and `--without-review` stay mutually exclusive; the user chose the strict
  option (A) over a double-flag override (B). A broken security reviewer blocks the merge until a
  retry passes.
- No change to jobs without open changes: `--without-review` still merges a job whose review never
  ran, and a plain approve still merges a passed review.
- No change to how verdicts are stored or how retry reruns reviewers.

## Approaches

- **A. Strict refusal (chosen).** Refuse every approve while changes are open and a role has no
  verdict. Smallest change, no new way to skip security.
- **B. Strict plus `--waive --without-review` together.** Keeps an escape hatch for a broken
  reviewer, but is one more way to merge unreviewed code. Rejected by the user.

## Design

1. `Job::approve_refusal`: after the series-waiting check and before the existing `changes_open`
   checks, when `changes_open() && review_missing()`, return one refusal whatever the flags. It names
   the open blockers and the missing roles and offers `rework_hint()` and cancel, with no `--waive`
   and no `--without-review`. Shape: `job 001 has 3 open review blockers from round 2
   (review-code-r2.md) and no security review yet: use factory retry 001, or cancel`.
   CLI, the factoryd API (`api/write.rs`) and retry share this function, so all three follow.
2. Dashboard `approveCommand` (`dashboard/static/board.js`): when `openChanges(job) &&
   reviewMissing(job)`, return no approve command; the needs-human card then shows only the retry or
   rework chip and cancel. `reviewMissing` already matches the core roles (`code`, `security`).
3. Docs: the approve rules in `docs/reference/software-factory.md` and the user guide get one line.

## Tests

- core: changes open + security failed (and pending, running) refuses plain, `--waive` and
  `--without-review`, with the retry hint on a review-resume park and `rework` without one; the
  message has no `--waive`.
- core: changes open + every role with a verdict still allows `--waive` (existing tests stay green).
- CLI or supervisor: `approve --waive` on such a job exits non-zero and leaves the job unmerged.
- dashboard node test: such a job shows no approve chip.

## Left to the builder

Exact wording of the message within the shape above, and where the CLI/API test lives.
