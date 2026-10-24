# Rocket Factory Journeybook

A single-file, offline HTML page that walks a new user through the Rocket
Factory CLI, from a fresh install to a merged feature. 8 journeys, each a list
of steps with the command to run, the output to expect and a one-line assertion
to tick off. Progress is stored only in the reader's own browser.

Content is transcribed from [`docs/guides/factory-user-guide.md`](../docs/guides/factory-user-guide.md).
The look and feel follows the RocketVault journeybook.

## Commands

```bash
bun install
bun run dev            # Vite dev server on http://127.0.0.1:5175
bun run build          # emits dist/index.html and dist/journeybook.html
bun run typecheck
bun run lint
bun run check:links    # dangling `related` ids, claims with no `source`
bun run format
bun scripts/check-contrast.mjs   # after touching any colour token
```

`dist/journeybook.html` is the file to share. It has no external assets.

## Editing the steps

Read [.claude/authoring-steps.md](.claude/authoring-steps.md) first. Step ids
are permanent because progress is keyed on them.
