# Authoring journeybook steps

The data in `src/data/` is the whole product. The UI is a way to read it and to
tick steps off, and it is much less likely to need changing than the steps are.

Source of truth: `../docs/guides/factory-user-guide.md`. That guide was checked against
`factory --help`, `factoryd --help` and the source. Reference app for the data
style: `/home/numericlabs/data/rocket/rocketvault/journeybook/src/data/` (read
`journeys-v-w.ts` for a worked example of every field).

## Where things live

| File | Holds |
| --- | --- |
| `src/data/types.ts` | `Case`, `Suite`, `Phase`, `Surface` |
| `src/data/journeys-a-c.ts` | Journeys A, B, C |
| `src/data/journeys-d-e.ts` | Journeys D, E |
| `src/data/journeys-f-h.ts` | Journeys F, G, H |
| `src/data/reference.ts` | Glossary, pipeline stages, park reasons, flag traps, quick card |
| `src/data/index.ts` | Flattens the journeys and records which guide revision this was built from |
| `scripts/check-links.mjs` | Fails on a dangling `related` id or a claim with no `source` |

## The rules

1. **Transcribe, never invent.** Every `command` and `expected` comes from the
   guide. If the guide says something has no effect or does not exist (for
   example `factory new --model`, or `factoryd enqueue --without-review`), write a
   step that asserts the absence. Do not invent an invocation that looks right.
2. **Step ids are permanent.** Progress is keyed on `id` in `localStorage`. Never
   rename `B3`. Adding `B11` is free.
3. **One assertion per step.** `assert` is what a reader scans on the collapsed
   row to decide whether their run matched. If a step needs two `assert` lines,
   it is two steps.
4. **Expected output is verbatim**, including the exact message text. Where a
   message has placeholders, keep the placeholders as the guide writes them
   (`<id>`, `N`, `<reason>`).
5. **`phase` says where in the pipeline the step happens**, not what the command
   is called. Use `observe` for read-only commands.
6. **`flag` is for the findings, not decoration.** `trap` bites an unprepared
   reader, `divergence` is two docs or two modes disagreeing, `gap` is a flag or
   feature that exists but does nothing or does not exist. Most steps carry no flag.
7. **`notes` explains why the step exists, or what is easy to get wrong.** It
   renders a two-marker subset of markdown: `` `code` `` and `**bold**`. Nothing
   else.
8. **A claim needs a citation or it does not ship.** `why`, `verify` and `after`
   each require `source`: a guide section such as
   `factory-user-guide.md § 6.5 factory approve`, or a code location such as
   `crates/factory-cli/src/commands.rs:566`. A claim the guide does not support
   is omitted. Omission is a correct outcome.
9. **`verify.command` is transcribed from the guide or built only from flags that
   appear in the guide's flag tables.** Never inferred.
10. **No hedging.** No "should", "presumably", "likely", "appears to".
11. **`why` and `notes` do not overlap.** `why` is the factory's mechanism. `notes`
    is what the reader should do or watch for.
12. **Reader-safe.** The reader may be non-technical. Titles and `assert` use plain
    words. Commands use the placeholders `my-app`, `login-fix`, `028` consistently.
    Never include a real token, key or password. Never print the contents of
    `~/.factory/api.token`.

## Which steps are safe to run

Every command a step asks the reader to run must be safe to run on a scratch
project. Steps that change state say so in `after` (what it leaves behind, and
what to undo first). Steps that delete (`project remove`, `cleanup --discard-commits`)
carry `flag: "trap"` and a `precondition`.
