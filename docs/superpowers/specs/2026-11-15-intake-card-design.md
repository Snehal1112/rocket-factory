# Intake card on the dashboard: short design

Status: for approval. Size: a small change (one engine file, one core struct, one dashboard file).

## Why

`factory intake` claims a labelled issue and then drafts its spec for minutes. No job exists
until the spec is valid (`intake.rs` `draft_jobs`, then `create_job_with_spec_accepting`), so the
board shows nothing for that issue. A failed draft (`draft-invalid`, `drafter-failed`, ...) never
produces a card at all. After the job exists, its card does not say it came from an issue:
`job.intake_issue` is stored and the dashboard never reads it.

## Goals

- From the claim on, the issue has a card in the Spec column.
- A failed draft keeps a visible card with the reason code.
- Once the job exists, the normal job card replaces it and names the issue.

## Non-goals

- No full job card before a job exists (no spec or plans to show).
- The page stays read-only. No new buttons.
- No change to labels, comments or the job pipeline.

## Approaches

- A. Draft card at the claim, then the normal job card (chosen by the user).
- B. A real job at the claim: rejected, every reader of the spec and plans would need an empty-job case.
- C. The dashboard reads the intake folder and issue labels: rejected, a second source of truth.

## Design

1. `Draft` (core) gets two optional fields, `issue: Option<u32>` and `note: Option<String>`, both
   skipped when `None`, so old boards and `factory-server` still load.
2. Two new draft stages for intake: `drafting-from-issue` and `draft-failed`. `DRAFT_STAGES`
   grows to five. `spec stage` accepts them, which is harmless. (my call)
3. `run_intake`, right after the claim: remove any old draft `issue-<n>`, then add a draft with
   slug `issue-<n>`, the feature text `<title>, #<n>`, stage `drafting-from-issue`, `issue = n`.
   (my call: remove first, because a person who relabels a failed issue must get a fresh card.)
4. `settle_failure`: set the draft to `draft-failed` with `note` = the reason code. When a job
   already exists (`build-failed`), remove the draft, because the job card shows the failure.
5. Success needs no code: `create_job_with_spec_accepting` already calls `adopt_draft` by slug.
   `mark_series` already sets `intake_issue` on the job.
6. Every draft write in intake is best effort: a failure goes to the log and never changes the
   pass result, like `adopt_draft`.
7. Dashboard `board.js`: the draft card shows `issue #n`, the stage label ("Drafting from issue",
   "Draft failed") and the note. For intake drafts it hides the `spec resume` chip and keeps
   `factory cancel issue-<n>`. The job card gets an `issue #n` chip from `intake_issue`.
   `dashboard/src/model.rs` carries the new fields through.

## Edge cases

| Case | Behaviour |
|---|---|
| Old board without the new fields | loads, card shows as before |
| Redraft attempt inside one pass | the same draft card stays, no new write |
| Driver crashes while drafting | card stays `drafting-from-issue`, goes stale after 24h, `factory cancel issue-<n>` removes it |
| Person relabels a failed issue | next pass replaces the failed card |
| Draft write fails | logged, intake goes on |

## Testing

- core: `draft_new_fields_are_optional_and_absent_from_old_boards`.
- engine `tests/intake.rs`: card exists during drafting (fake drafter inspects the board), card
  becomes a job on success, `draft-failed` with the reason code on a failed draft, removed on
  `build-failed`, replaced on a second pass.
- dashboard: `state.rs` JSON carries `issue` and `note`. `board.js` has no test harness today, so
  I will check it in a browser against a sample board.
- Commands: `cargo test -j4 -p factory-core`, `-p factory-engine`, `--manifest-path dashboard/Cargo.toml`.

## Acceptance criteria

- While the drafter runs, `state.drafts` holds `issue-<n>` at `drafting-from-issue`.
- After a failed draft the card shows `draft-failed` and the reason code.
- After the job is created the draft is gone and the job card shows `issue #n`.
- An old board loads unchanged.

## Left to the builder

CSS for the new chip, and the wording of the stage labels.
