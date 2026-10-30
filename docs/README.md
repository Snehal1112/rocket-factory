# Docs index

Start here. Find the question you have, open that one doc. Paths are relative to `docs/`.

## Which doc answers what

| Question | Read |
|---|---|
| How do I install the factory and run my first feature? | `guides/getting-started.md` |
| What is every command, option and recipe, in plain words? | `guides/factory-user-guide.md` |
| How does the pipeline work: stages, job model, gates, merge rules, dashboard? | `reference/software-factory.md` |
| What can I set in `project.toml`, with defaults and validation? | `reference/project-toml-reference.md` |
| Which flags does each `factory` and `factoryd` command take? | `reference/factory-admin-manual.md` |
| How do I cut a release? | `operations/releasing.md` |
| How do I register Rocket or RocketVault on another machine? | `operations/rocket-registration.md`, `operations/rocketvault-registration.md` |
| What is planned next, and why? | `roadmap/roadmap-2026-10-18.md` |
| Why does factory output need human follow-up fixes? | `roadmap/quality-investigation-2026-10-29.md` |
| Why was a feature built this way? | `superpowers/README.md`, then the spec |

## Folders

- `guides/`: for people learning or using the factory. Task-first, with checks after each step.
- `reference/`: look-ups. Verified against the code, so re-check them after a change to a command, a setting or a gate.
- `operations/`: steps you run on a machine or at release time.
- `roadmap/`: research and ordering for future work. Leads, not proof; re-verify against source.
- `assets/`: `factory-workflow.html` (the five diagrams as one page) and `images/` (the same diagrams as SVG, used by the root `README.md`).
- `superpowers/`: specs and plans written by the brainstorming and planning skills. The paths are fixed by those skills and by `factory new`. See `superpowers/README.md` for the list.

## For Claude

- Read this file, then only the one doc the question needs. `reference/software-factory.md` and `guides/factory-user-guide.md` are large (50 KB or more); search them by heading instead of reading them whole.
- Docs you write go in the folder that matches their purpose, and get a row in the table above. A new spec or plan series gets a row in `superpowers/README.md`.
- Tests read some of these files by path (`crates/factory-cli/tests/agent_lint.rs`). If you move or rename a doc, update that test and run it.
