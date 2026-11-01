# Factory quality fixes

An audit of the factory and its agents on 2026-11-01 (research over jobs 035 to 046, five read-only agents) found why factory-built code in this repo still needs human follow-up. Most of the causes are in how the builder and reviewer are set up, not in the model. This spec fixes the six causes that can be fixed in code and presets. Claims below were checked against source at the time of the audit; the plans re-verify them before changing anything.

## Goal

A factory job on any preset builds from the right instructions, is gated on formatting and on the tests of the crates that depend on what it changed, and is reviewed against the same information the builder had. No change may break Codex runs or the other presets.

## Evidence

| Finding | Where |
|---|---|
| The repo tracks Rocket's copies of `.claude/agents/factory-*.md`. `write_agent_files` leaves a file that is tracked on the base branch alone, so they shadow the preset. The builder file says "do not run clippy" and never run a bare `cargo check -j4`, and carries yarn and shadcn rules. The reviewer file judges Rocket's DDD and Tauri placement. 41 jobs carry the "repo agent files shadow" ruling. | `crates/factory-engine/src/build.rs` (`write_agent_files`), job ledgers |
| Codex agent text comes from the resolved agent file list, not from the tracked copies, so Claude and Codex builders get different text. | `crates/factory-engine/src/codex_prep.rs:101` |
| The rust preset's `risk_paths` has `**/build.rs`, which marks the ordinary source file `crates/factory-engine/src/build.rs` as risky. It parked job 046 and led to two rework rounds. | `crates/factory-core/presets/rust/config.toml` |
| `project.toml` tells builder, reviewer and plan authors to keep engine logic behind a port in `ports.rs`. `CLAUDE.md` says `ports.rs` holds no traits, and only `AgentRunner` and `Tracker` are traits. Its `read_first` names `docs/software-factory.md`, which does not exist. | `~/.factory/projects/rocket-factory/project.toml` |
| No gate runs `cargo fmt --check`. Tests run only for crates with a changed path, so jobs 044 and 046 never ran the `factory-cli` tests (`agent_lint`, `e2e`, `no_hardcoded_paths`). `{dependents}` is supported and unused. | `crates/factory-checks/src/plan.rs`, job `checks.md` files |
| `spec_excerpt` keeps only a goal line and the acceptance criteria. Design, edge cases, Decisions and Out of scope never reach the builder, and the preamble says the excerpt replaces reading the spec. | `crates/factory-engine/src/tasks.rs:262-298`, `specs.rs:244-246` |
| The reviewer has the tools Read, Grep and Glob and reads the diff. It caught 0 of the 5 bugs a human audit found in job 044, including tests that exercised a helper and not the real call site. It is not given the user's rework note. | reviewer agent file, `specs.rs` `reviewer_spec_with_model` |

## Design

The work is four plans, in this order. The first plan comes first so that later jobs that touch `factory-engine/src/build.rs` no longer park for a false risk.

### Plan 1: Agent files and the risk glob

- Delete the five tracked `.claude/agents/factory-*.md` files. Jobs then get the preset agents written into the worktree, as for any project that does not track its own.
- Retarget `agent_lint` from `.claude/agents` to `crates/factory-core/presets/<preset>/agents/`, over every name in `PRESET_NAMES` (rust, go, ruby, csharp, node). The repo root finder, the known-agents test and the per-agent text pins all read the preset files. A test fails when a preset has no agent file.
- Add an `agent_lint` test that no preset agent file names a Rocket-only term (`shadcn`, `lucide`, `zustand`, `rocket-infra`, `yarn`).
- Add one line to the builder file of every preset: match the conventions of the neighbouring code and of `CLAUDE.md`, and search for an existing helper before writing one.
- Bring the "Carried over from Rocket" paragraph of `CLAUDE.md` up to date.
- In the rust preset, replace `**/build.rs` in `risk_paths` with the three globs `build.rs`, `*/build.rs` and `*/*/build.rs`. Tests pin `build.rs`, `dashboard/build.rs` and `crates/x/build.rs` as risky and `crates/x/src/build.rs` as not risky, and the plan checks how `*` treats `/` in the glob matcher before relying on it. Residual risk, accepted: a build script nested deeper than two folders no longer forces review on its own. Its package's `Cargo.toml` stays risky, and the other two risk globs for Cargo files are unchanged.

### Plan 2: Check gates

- In the rust preset's check rules add `cargo fmt -p {crate} -- --check` to the crate rule and `cargo fmt -- --check` to the rule for the root `src/**` paths. The plan confirms that the command validator accepts a cargo command without `-j4` for `fmt`, which takes no job count, and adds a test.
- In the rust crate rule add `cargo test -j4 -p {dependents}` as its own command, never in the same command as `{crate}`, which the validator already rejects. It uses the existing `dependents_cap` setting, and `Workspace::dependents` returns direct dependents only.
- A change under `crates/factory-core/presets/**` belongs to `factory-core`, so its dependents, including `factory-cli` where `agent_lint` lives, are tested.
- No change for the go, ruby, csharp and node presets. Their formatters are chosen per project or are not verified here.

### Plan 3: Builder context

- `spec_excerpt` always keeps the goal line and the whole acceptance criteria section, as today. With the room left under a total cap of 8192 bytes it adds, in spec order, `## Design`, the edge-case section (a heading that contains `edge case`, ignoring case), `## Decisions` and `## Out of scope`. A section that does not fit whole is cut at a line boundary, and the later ones are left out. When anything was left out or cut, the excerpt ends with one line that names the spec file and says the rest was left out. The goal and acceptance criteria are never cut, even when they alone pass the cap.
- The excerpt preamble says the excerpt covers the goal, design, edge cases, decisions, scope and acceptance criteria, and that the full spec is for anything the excerpt refers to. The text is shared by both harnesses and names no harness tool.

### Plan 4: Reviewer input

- `factory rework` also writes the user's note, alone and verbatim, to `user-note-r{N}.md` in the job folder, where N is the new round for a next-round rework and the current round for a mid-task rework. The builder's own copies stay as they are: `rework-note.md` is deleted once the builder has read it, and `changes-r{N}.md` mixes the note with the findings. When `user-note-r{round}.md` exists, the reviewer prompt names it and says it is the user's instruction for this round. Both harnesses get the same line. The path is inside the job folder, which the read-only Codex reviewer already reads for the spec and plan.
- Add an acceptance step to the reviewer file of every preset: for each acceptance criterion, name the test that reaches the real call site and not only a helper, and grade a criterion with no such test as a blocker.

## Compatibility

- **Codex:** every new line of shared text passes the neutrality tests in `crates/factory-cli/tests/tool_map.rs`. `codex_prep` tests cover the reviewer prompt with a rework note, and the builder prompt with a widened excerpt. A live `factory probe --harness codex` run remains a maintainer step.
- **Presets:** agent text edits land in all five presets. Config edits land only in the rust preset. `{dependents}` needs the Cargo workspace map and no other preset uses it, so their behaviour is unchanged. Every preset config still has to load, which the existing preset tests check.
- **Existing projects:** an agent file in a project's own `agents` folder, or tracked in its repo, still wins. They keep their text and do not get the new lines. The release notes say so.
- **No new required setting.** The byte cap is a constant.

## Rollout

`seed` never touches existing files, so preset changes reach a factory home only through `factory presets refresh`. After the merge run it on each machine, then run `factory probe` to confirm the builder and reviewer see the preset text. A project's own `[[settings.checks]]` replaces the preset list, so this repo's `project.toml` needs the new check commands by hand.

## Changes outside the jobs

A factory job cannot edit `~/.factory/projects/rocket-factory/project.toml`. After approval it is edited by hand:

- Replace the "port in `ports.rs`" rule in the builder, reviewer and plan rules with the real one: only `AgentRunner` and `Tracker` are traits, and git and checks are called directly.
- Change `read_first` to `docs/reference/software-factory.md`.
- Add `cargo fmt -p {crate} -- --check`, the `{dependents}` test command and the dashboard `cargo fmt --manifest-path dashboard/Cargo.toml -- --check` to the project's check rules.
- Drop the Rocket-only leftovers from its rules.

## Non-goals

- Making per-task checks blocking.
- Giving a fresh task-2+ builder the earlier tasks' diff.
- Giving the reviewer a Bash tool or mutation runs.
- Format checks for the go, ruby, csharp and node presets (filed as an issue).
- Changes to the repo-level `.claude/factory/config.toml`.

## Decisions

- Stop the shadowing by deleting the tracked agent copies, and not by keeping identical copies or writing repo-specific agents. Side effect: the factory agents leave interactive Claude sessions in this repo. Decided by: user
- Fix the `**/build.rs` false positive in the rust preset, not by an override in one project. Decided by: user
- The design as a whole, including the Compatibility section: no breakage for Codex or the other presets. Decided by: user
- Three-glob form for the build script risk paths. The accepted residual risk is that a Cargo build script nested deeper than two folders no longer forces human review on its own, while its package's `Cargo.toml` stays risky. Decided by: user
- `{dependents}` runs direct dependents only, under the existing cap. Decided by: call
- The excerpt byte cap is 8192. Decided by: call

## Acceptance criteria

- The repo tracks no `.claude/agents/factory-*.md`, and `agent_lint` reads the agent files of all five presets and fails when one is missing.
- No preset agent file names a Rocket-only term, and every preset builder file carries the match-conventions line.
- `crates/factory-engine/src/build.rs` is not risky under the rust preset, and `build.rs`, `dashboard/build.rs` and `crates/x/build.rs` are.
- The rust preset plans a `cargo fmt --check` command and a `{dependents}` test command for a changed crate path, and the validator accepts both.
- A diff under `crates/factory-core/presets/` plans tests for `factory-cli`.
- The builder excerpt of a spec always holds its goal and its whole acceptance criteria section, and adds its Design, edge-case, Decisions and Out of scope sections while they fit under 8192 bytes, and says so when it leaves one out.
- `factory rework` writes `user-note-r{N}.md` in both rework kinds, the reviewer prompt names it for that round when it exists, for Claude and for Codex, and every preset reviewer file carries the acceptance step.
- The go, ruby, csharp and node preset configs are unchanged, and all five presets still load.
- fmt and clippy `-D warnings` pass for every crate a plan touches, and `cargo test -j4 -p factory-cli --test agent_lint` passes.
