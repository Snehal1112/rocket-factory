# Factory interview, cards and execution: plan index

Spec: `docs/superpowers/specs/2026-10-18-factory-interview-cards-and-execution-design.md`.

The driver allows at most 6 plans per chain and 3 tasks per plan, so the work is three series. Run them in this order, each with the same `--spec`. Every plan is executed by the factory driver using `superpowers:subagent-driven-development`.

| Series | Folder | Plans | What it delivers |
|---|---|---|---|
| 1. Execution rework | `plans/2026-10-18-factory-execution-rework/` | 6 | Real task headings only, builder status and park on `BLOCKED`, rulings and reviewer inputs, per-task checks and slim agent starts, stronger reviewer model and the 1M job budget, review skills in the reviewer and fix prompts |
| 2. Draft cards | `plans/2026-10-18-factory-draft-cards/` | 5 | Board `drafts[]` and spec path, `factory spec` commands and slug cancel, adoption and warning in `factory new`, dashboard state and factoryd, the draft card UI |
| 3. Command and presets | `plans/2026-10-18-factory-command-and-presets/` | 4 | Builder and reviewer text for `subagent-driven-development`, the 7.0.0 command text, `factory presets refresh`, plan lint and the skill drift test |

## Order and dependencies

- Series 2 and 3 assume series 1 is merged. Command plan 01 writes the `Status:` and `Ruling:` contract that execution plan 02 parses.
- Command plan 02 tells sessions to run `factory spec start`, which cards plan 02 adds. Command plan 04 reuses the spec warning field from cards plan 03.
- Execution plan 04 saves the old preset agent texts under `presets/legacy/` before any preset is edited; command plan 03 embeds them.

## Running a series

```
factory new <slug> '<feature>' --plans docs/superpowers/plans/<series folder> --spec docs/superpowers/specs/2026-10-18-factory-interview-cards-and-execution-design.md
```

Start the next series only after the previous one has merged.
