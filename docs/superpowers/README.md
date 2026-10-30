# Specs and plans

The brainstorming and planning skills write here, so the folder names and paths stay fixed: `specs/<date>-<topic>-design.md` and `plans/<date>-<topic>/plan-NN-<topic>.md`. The driver reads these paths with `factory new --spec ... --plans ...`.

Everything here is a record of how a feature was designed and built. For how the factory works today, read `../reference/software-factory.md`. Specs and plans can be out of date and name old paths; trust the code and the reference docs over them.

This index lists each spec with its plans. It does not track whether a series merged; check `git log` or the factory board for that.

| Topic | Spec | Plans |
|---|---|---|
| Standalone factory (one factory, many projects) | `specs/2026-10-16-standalone-software-factory-design.md` | `plans/2026-10-16-standalone-software-factory/` (index: `00-plan-index.md`) |
| Dashboard React frontend | `specs/2026-10-18-dashboardv1-react-frontend-design.md` | none |
| Interview, spec cards, execution rework | `specs/2026-10-18-factory-interview-cards-and-execution-design.md` | `plans/2026-10-18-factory-interview-cards-index.md`, `plans/2026-10-18-factory-command-and-presets/`, `plans/2026-10-18-factory-draft-cards/`, `plans/2026-10-18-factory-execution-rework/` |
| No-checks and no-tests merge gate | `specs/2026-10-18-no-check-no-test-gate-design.md` | `plans/2026-10-18-no-check-no-test-gate/` |
| Builder permissions | `specs/2026-10-19-builder-permissions-design.md` | `plans/2026-10-19-builder-permissions/`, `plans/2026-10-19-builder-permissions-2/`, `plans/2026-10-19-builder-permissions-3/`, `plans/2026-10-20-builder-permissions-fixes/` |
| One job per spec | `specs/2026-10-19-one-job-per-spec-design.md` | `plans/2026-10-19-one-job-per-spec/` |
| Structured logging | `specs/2026-10-19-structured-logging-design.md` | `plans/2026-10-19-structured-logging/` |
| Builder task resume | `specs/2026-10-20-builder-task-resume-design.md` | `plans/2026-10-20-builder-task-resume/` |
| Cold-start savings | `specs/2026-10-20-cold-start-savings-design.md` | `plans/2026-10-20-cold-start-savings/` |
| Crash resume | `specs/2026-10-20-crash-resume-design.md` | `plans/2026-10-20-crash-resume/` |
| Merged column sort | `specs/2026-10-20-merged-column-sort-design.md` | `plans/2026-10-20-merged-column-sort/` |
| Per-role cache TTL | `specs/2026-10-20-per-role-cache-ttl-design.md` | `plans/2026-10-20-per-role-cache-ttl/` |
| Upgrade and version check | `specs/2026-10-20-upgrade-and-version-check-design.md` | `plans/2026-10-20-upgrade-and-version-check/` |
| Codex support | `specs/2026-10-21-codex-support-design.md` | `plans/2026-10-21-codex-support/` |
| Monorepo subfolder projects | `specs/2026-10-21-monorepo-subfolder-projects-design.md` | `plans/2026-10-21-monorepo-subfolder-projects.md` |
| Provider switch fix | `specs/2026-10-21-provider-switch-fix-design.md` | none |
| Cleanup of cancelled series | `specs/2026-10-22-cleanup-cancelled-series-design.md` | `plans/2026-10-22-cleanup-cancelled-series/` |
| Codex support audit | `specs/2026-10-22-codex-support-audit-design.md` | `plans/2026-10-22-codex-support-audit.md` |
| Structured logging gaps | `specs/2026-10-24-structured-logging-design.md` | `plans/2026-10-24-structured-logging/` |
| Act on review findings | `specs/2026-10-29-act-on-review-findings-design.md` | `plans/2026-10-29-act-on-review-findings/` |

Plans with no spec of their own:

- `plans/2026-10-23-code-review-fixes.md`
- `plans/2026-10-24-node-preset.md`
- `plans/2026-10-30-review-gate-follow-ups.md` (issues #28 to #31)

When you add a spec or a plan series, add a row here.
