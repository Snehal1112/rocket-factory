# Harness, CI, subfolder test and committer leftovers

**Commit:** fix(factory): harness, CI and subfolder leftovers

## Goal

Finish the leftovers that issue 132 lists for issues 44, 5, 12 and 122, plus the Codex probe model bug and the combine lint false positive, so a cross-harness probe runs, no test passes by skipping, CI covers macOS, subfolder projects have their missing tests, and no retired committer agent reaches a builder.

## Overview

Issue 132 keeps the open parts of issues 44 (harness hardening), 5 (CI), 12 (subfolder projects) and 122 (committer leftovers). A survey of main at `88c23cb` confirmed each item below is still open. Two more came up on 2026-11-22:
- the live probe `factory probe --harness codex` failed for every role, because it sent the Claude model `claude-sonnet-5-5` to Codex;
- job 100's `factory new` warned about `Task 1` inside a backtick span in a later plan.

Issue 12's "lock is free during the commit message run" item is obsolete. No commit-message agent runs any more: the driver writes the message (`no_message_agent_runs_for_a_merge_or_a_sync`, `crates/factory-engine/tests/merge.rs:789`).

## Current state

| Area | Today | Gap |
|---|---|---|
| Probe model | `Driver::probe` (`crates/factory-engine/src/compat.rs:611-646`) clones the config and sets only `agent_cli` to the harness. The model fields stay the project's. `CODEX_DEFAULT_MODEL = "gpt-5.6-luna"` (`crates/factory-core/src/project.rs:132`) is used only by `project add --agent-cli codex` (`:182`). | A cross-harness probe sends a model the other CLI refuses. |
| `shape_spec` | `Engine::shape_spec` (`crates/factory-engine/src/ports.rs:240`) branches on `self.config.agent_cli()`. The probe calls it at `crates/factory-engine/src/probe.rs:148`. | It ignores the probe's `harness` argument. Only the config override in `Driver::probe` hides that. |
| Silent skips | `common::fake_claude()` (`crates/factory-cli/tests/common/mod.rs:7-17`) prints a skip line and returns `None` when the binary is missing. Used at `tests/probe_cmd.rs:320-324` and `tests/plugin_install.rs:64,111`. | Three tests pass without testing anything. |
| Probe hint | `hide_claude_tools` (`probe.rs:200-208`) and `prepare_for_codex` (`codex_prep.rs:94-99`) fail with `agent file {name} not found`. | No hint when no project is selected. |
| Exec bits | Before `a5b9c06`: `scripts/live-smoke.sh` and `crates/factory-cli/tests/support/claude-shim.sh` are mode 100644. `docs/operations/releasing.md:30-32` runs `./scripts/live-smoke.sh`. | `./scripts/live-smoke.sh` gives "permission denied". |
| `--interactive` | `interactive_step` (`scripts/live-smoke.sh:296-320`). `tests/live_smoke_script.rs` covers only its argument checks. | The step itself is untested. |
| CI | Before `a5b9c06`: `.github/workflows/ci.yml`: jobs `check` and `dashboard`, both `ubuntu-latest`, no matrix. `PKGS` and `scripts/release.sh` `CRATES` (`:28-37`) list 8 crates. | No macOS. `factory-procgroup` is never fmt-checked, linted or tested in CI. |
| `/proc` | CLAUDE.md forbids reading `/proc`. No source does. `crates/factory-cli/tests/no_hardcoded_paths.rs` lints other paths. | Nothing stops a future `/proc` read. |
| Combine lint | `stale_mentions` (`crates/factory-engine/src/combine.rs:393`) skips fenced lines, not inline backtick spans. `strip_spans` (`crates/factory-core/src/commitmsg.rs:~203`) is private. | A quoted `### Task 1: first` in a later plan warns. |
| Subfolder cwd | Only the first builder (`builder_starts_in_the_project_folder`, `tests/agent_files.rs:120-143`) and the Codex builder check their cwd. | No test for the resumed, carried, per-task and fix builders, or for the reviewer. The reviewer prompt names the worktree root while the spec cwd is the project folder. |
| Overlap error | `check_overlap` (`crates/factory-core/src/project.rs:108-126`): "project {name} already covers {canon/new}". `add_refuses_overlap_and_root_after_subfolder` asserts only `contains("web")`. | A root project added after `web` reads "project web already covers /repo". |
| Disabled show | `show` (`crates/factory-cli/src/project.rs:239`) prints `disabled: {why}`. Only a missing repo is tested (`project_cmd.rs:592`). | No test for `project folder is missing`, and none for the `is not the top folder of a git repository` text (`crates/factory-git/src/error.rs:14`). |
| Scoped crash-resume | The merge path handles `BaseState::Merged`. | No test for a subfolder job. |
| Scoped mapping | `Repo::scoped_paths` (`crates/factory-git/src/repo.rs:115-126`) passes out-of-scope paths through unchanged. `checks_map_project_relative_paths` (`tests/gates.rs:354`) asserts only `Advanced(Build)`. | No test for a rename with both sides or an empty list, and no assert on `checks.md`. |
| Name collision | `create_worktree` (`crates/factory-git/src/worktree.rs:92-126`) refuses `{path} already exists`. `factory-a-001-001-x` is the name of both project `a-001` slug `x` and project `a` slug `001-x`. | Untested, and the error does not say why. |
| `worktree_root_of` | `repo.rs:96-100` is lexical. | No test or doc for a `..` path. |
| Committer | `RETIRED_AGENTS` (`crates/factory-core/src/resolve.rs:262`) is applied only to the preset layer (`:276`). Project-own (`:~282`) and base-tip (`:~296`) copies reach `agents.json`, and the builder has the `Agent` tool (`specs.rs:38`). | A builder can start a committer subagent. |
| Committer text | Before `b58ac2c`: `journeybook/src/data/journeys-f-h.ts:658`, `journeys-a-c.ts:603`, `journeys-d-e.ts:608` and `reference.ts:397-398` name a committer role. `docs/assets/images/factory-turns.svg` has no run counts. `no_preset_ships_the_committer_or_the_commit_context` (`preset.rs:984-994`) asserts with no messages. `every_intake_drafter_asks_for_the_commit_line` (`agent_lint.rs:1285`) pins only `**Commit:**` and `at most 50 characters`. | Stale text, and a weak pin. |

## Non-goals

- No change to worktree or branch names.
- No fallback to the repo's tracked agents for a probe. That stays with `docs/superpowers/plans/2026-11-01-agent-delivery-fixes.md`.
- No change to the reviewer prompt wording, because golden prompt tests pin it. A test pins that it names the worktree root.
- No dashboard change.

## Design

```
probe --harness h ─ config.agent_cli = h ─ h ≠ project's cli? ─ yes → every model field = h's default
                                                              └ no  → project models
shape_spec(spec, harness)          fake_claude() missing → build it, panic only if the build fails
merged_agents: preset ∪ own ∪ tip, then drop RETIRED_AGENTS with a note
CI: check and dashboard × {ubuntu-latest, macos-latest}, fail-fast off, PKGS + factory-procgroup
```

### 1. Probe model and harness (issue 44)

- In `Driver::probe`: when `harness.as_str() != self.config.agent_cli()`, set `builder_model`, `reviewer_model`, `kb_model` and `escalation_model` to that harness's default, and `reviewer_model_large` the same way:
  - Codex: `factory_core::project::CODEX_DEFAULT_MODEL`. `reviewer_model_large` and `escalation_model` become `None`, as `Config::parse` does for Codex.
  - Claude: the field values of `Config::default()`.
- The probe report prints one line per role with the model used, `model: <id>`, after the existing role line. Its JSON gains an optional `model` field per role.
- `shape_spec` takes the harness: `fn shape_spec(&self, spec: &mut AgentSpec, harness: Harness) -> Result<(), String>`. `prepare_agent` passes `Harness::from_config(&self.config)`, and `probe_run` passes its `harness`.

### 2. Loud skips, hint and exec bits (issue 44)

- `common::fake_claude()` returns `PathBuf`. When the binary is missing it builds it with `cargo build -j4 -p factory-agent --bin fake-claude` (through `env!("CARGO")`, from the workspace root), and panics with "build fake-claude with `cargo build -j4 -p factory-agent` first" only when that build fails. The factory's own checks run `cargo test -p factory-cli` without building it, so a bare panic would fail every later job. Its three callers drop their early returns.
- On the probe path only, the "agent file {name} not found" error ends with "; pass --project <name>, or run inside a registered project". `prepare_for_codex` also runs in real jobs, so the hint is added where the probe maps the error (`probe_run` and `hide_claude_tools`), not in `prepare_for_codex`.
- `scripts/live-smoke.sh` and `claude-shim.sh` have mode 100755. The builder cannot run `chmod`, so this was set by hand on main before the job, and a test pins it.
- A new test runs `scripts/live-smoke.sh --no-probe --interactive` with a fake tmux, through `run_script`, with `TMUX_BIN` pointing at an executable fake and `HOME` at a temp home that holds `.claude/commands/factory.md`. The fake answers `capture-pane` with a pane holding `? for shortcuts`, `merged` and `001`, and every other subcommand with exit 0. The test expects exit 0, and `out/interactive/pane.txt` containing `001`.

### 3. CI on macOS and the `/proc` lint (issue 5)

- Both jobs in `ci.yml` run with `strategy: { fail-fast: false, matrix: { os: [ubuntu-latest, macos-latest] } }` and `runs-on: ${{ matrix.os }}`. `ci.yml` is a risky path with no check, so this edit was made by hand on main before the job. The job adds a test that pins it.
- `PKGS` and `release.sh` `CRATES` include `factory-procgroup`, also by hand before the job. A test checks that both list every workspace member.
- Any test that is Linux-only gets `#[cfg(target_os = "linux")]` with a comment saying why. `reap_on_takeover.rs` already has one.
- `scripts/live-smoke.sh` runs on macOS: its GNU-only `sed -i` and `\|` uses were replaced by hand before the job (`awk` into a new file, and `sed -E`). It uses no bash 4 features.
- `no_hardcoded_paths.rs` gains `no_source_reads_proc`. It covers every crate in the workspace, `factory-procgroup` and `factory-telemetry` included. It skips `#[cfg(test)]` tails and `//` lines, as the existing test does, and fails on a line containing `"/proc`.

### 4. Combine lint skips inline code

- `strip_spans` in `commitmsg.rs` becomes `pub` and is exported from `factory_core`.
- `stale_mentions` runs each unfenced line through `strip_spans` before it looks for `Task `.
- A mention outside the backticks on the same line still warns.

### 5. Subfolder project tests (issue 12)

- Cwd tests on `scoped_fixture()`:
  - the task, task resume and task carry builders;
  - the resumed and fresh fix builders;
  - the reviewer, whose prompt names the worktree root while `spec.cwd` is `<root>/apps/web`.
- `check_overlap` keeps "project {name} already covers {canon/new}" when the new folder sits inside the other. When the other folder sits strictly inside the new one, it says "project {name} already covers {canon/other}, inside {canon/new}", where an empty `new` prints `canon` itself, with no trailing slash. The CLI shows it after `config error: `. The test asserts the full texts with `ends_with`.
- CLI tests:
  - `project show` of a subfolder project whose folder is gone prints `disabled: project folder is missing`;
  - a project whose repo path is a subfolder of a git repo prints `is not the top folder of a git repository`.
- A scoped job whose base is already merged resumes without a scope failure.
- `scoped_paths`:
  - unit tests for a rename's two sides, an empty list, and an out-of-scope path passed through;
  - a doc comment that says out-of-scope paths pass through;
  - `checks_map_project_relative_paths` also asserts that `checks.md` names the `false` command.
- `create_worktree` says "{path} already exists; another job or project uses the worktree name {name}", and a test pins it for the `a-001`/`001-x` pair.
- `worktree_root_of` gets a doc line saying the match is lexical, and a test that `<worktrees>/a/../b` gives `<worktrees>/a`.

### 6. Retired committer and its text (issue 122)

- `merged_agents` applies the `RETIRED_AGENTS` filter after all three layers are merged.
- Each project-own or tip copy it drops adds a note: "ignored retired agent {name} from {layer}; delete it". `{layer}` is `the project folder` or `the base branch`.
- Tests that use `factory-committer.md` as a sample name move to `factory-kb-refresher.md` in `resolve.rs`, and to `factory-intake-drafter.md` in `factory-engine/tests/agent_files.rs`, whose list already holds kb-refresher.
- The journeybook lines drop the committer. They were edited by hand before the job, because no check covers `journeybook/`, and the job adds a test that pins them:
  - `--role` lists `builder`, `reviewer`, `spec-reviewer` and `kb`;
  - the model line names the builder, its fix rounds, the reviewer, the KB refresher and their helpers;
  - the tokens line names the builder, reviewer and kb;
  - the reference correction keeps its old `claimed` text, and its `actual` says the driver writes commits and merge messages with no agent, and the spec reviewer and the KB refresher spend tokens too.
- The SVG note gains, by hand before the job and pinned by a test: "At most N + 2 builder runs, 3 runs per review role and 1 spec review, with fix_rounds = 2."
- The two preset asserts get messages `"{name} ships factory-committer.md"` and `"{name} ships commit-context.md"`.
- `every_intake_drafter_asks_for_the_commit_line` also pins:
  - `` `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert` ``;
  - `the scope is one word with no spaces`;
  - `no trailing period`;
  - `next to `**Goal:**``.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| `--harness` equals the project's CLI | The project models are kept. |
| A Codex project probed with `--harness claude` | The Claude defaults are used. |
| fake-claude not built | The helper builds it; the test panics with the build command only when that build fails. |
| A probe without `--project` | The error names `--project`. |
| One OS fails in CI | The other OS still reports. |
| A `/proc` mention in a comment or test | Allowed. |
| `` see `Task 2` and Task 3 `` | Only `Task 3` can warn. |
| A committer file in all three layers | It is resolved nowhere, with one note per own or tip copy. |
| The `a-001`/`001-x` pair | The second worktree is refused with a clear error, and no stray branch is left. |

## Testing

- factory-engine:
  - `tests/probe.rs`: `shape_spec_follows_the_probe_harness`, `a_probe_without_agents_names_the_project_flag`.
  - `tests/agent_files.rs`: `every_builder_and_the_reviewer_start_in_the_project_folder`.
  - `tests/gates.rs`: the stronger `checks_map_project_relative_paths`.
  - `tests/merge.rs`: `a_scoped_job_with_a_merged_base_resumes`.
  - `src/combine.rs`: `an_inline_code_mention_is_left_alone`, `a_mention_outside_the_code_span_still_warns`, `a_lone_backtick_does_not_hide_a_mention`.
- factory-cli:
  - `tests/probe_cmd.rs`: `a_cross_harness_probe_uses_the_harness_default_model`, `a_same_harness_probe_keeps_the_project_models`, `the_probe_report_names_the_model`, and `harness_flag_overrides_the_project_agent_cli` also asserts the model.
  - `tests/live_smoke_script.rs`: `interactive_runs_with_a_fake_tmux`, `the_scripts_are_executable`.
  - `tests/probe_cmd.rs` also: `fake_claude_builds_the_binary_when_it_is_missing`.
  - `tests/no_hardcoded_paths.rs`: `no_source_reads_proc`, `the_proc_lint_catches_a_proc_literal`, `ci_and_release_list_every_workspace_crate`, `ci_runs_on_linux_and_macos`.
  - `tests/project_cmd.rs`: `show_of_a_subfolder_project_with_a_missing_folder_is_disabled`, `a_registered_subfolder_repo_path_names_the_top_folder_rule`, and the full overlap texts.
  - `tests/agent_lint.rs`: the stronger Commit-line pin, `the_turns_svg_names_the_worst_case_counts`, `the_journeybook_names_no_committer_role`.
- factory-core:
  - `src/resolve.rs`: `a_retired_agent_is_dropped_from_every_layer_with_a_note`, and the sample name repointed;
  - `src/project.rs`: `overlap_names_the_inner_folder`;
  - `src/preset.rs`: the assert messages.
- factory-git: `tests/repo_basics.rs` gets `scoped_paths_maps_both_sides_of_a_rename`, `scoped_paths_of_nothing_is_nothing`, `out_of_scope_paths_pass_through_scoped_paths` and `worktree_root_of_is_lexical`. `tests/worktree.rs` gets `a_worktree_name_clash_across_projects_fails_cleanly`.
- Commands: `cargo test -j4` per crate for factory-core, factory-git, factory-engine, factory-cli and factory-procgroup, then `node --test dashboard/tests/*.test.js`.

## Files changed

- `crates/factory-engine/src/`: `compat.rs`, `ports.rs`, `probe.rs`, `combine.rs`
- `crates/factory-core/src/`: `commitmsg.rs`, `lib.rs`, `probe.rs`, `resolve.rs`, `project.rs`, `preset.rs`
- `crates/factory-cli/src/`: `probe.rs`
- `crates/factory-git/src/`: `repo.rs`, `worktree.rs`
- Tests: the files named in Testing, and `crates/factory-cli/tests/common/mod.rs`, `tests/plugin_install.rs`
- Already changed on main by hand, in commits `a5b9c06` and `b58ac2c`, so the job adds only the tests that pin them: `.github/workflows/ci.yml`, `scripts/release.sh`, `scripts/live-smoke.sh`, the modes of `scripts/live-smoke.sh` and `crates/factory-cli/tests/support/claude-shim.sh`, the four journeybook data files and `docs/assets/images/factory-turns.svg`

## Constraints

- Cargo commands are per crate with `-j4`, never `--workspace` or `--all`, and never set `CARGO_TARGET_DIR`.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- Board JSON is unchanged. The probe JSON `model` field is optional.
- Git stays behind `factory_git::Repo`.
- The job does not edit `.github/workflows/ci.yml`, `scripts/release.sh`, `scripts/live-smoke.sh`, the journeybook data or the SVG, and does not change file modes. Those were done by hand before the job, and the job only adds the tests that pin them.
- Process checks stay in `procinfo.rs`, and no code reads `/proc`.
- The macOS CI run happens on the first push. A failure there is fixed before this work counts as done.

## Acceptance criteria

- `factory probe --harness codex` on a Claude project sends `gpt-5.6-luna` for every role, and the report names it.
- `shape_spec` follows the probe harness.
- A missing fake-claude binary is built by the helper, and the three tests panic with the build command only when that build fails.
- The no-agent probe error names `--project`.
- Both shell scripts are executable, and the `--interactive` step has a test.
- CI runs both jobs on Linux and macOS with fail-fast off, and `factory-procgroup` is in `PKGS` and `CRATES`.
- `no_source_reads_proc` guards every crate.
- A `Task N` inside backticks in a later plan does not warn.
- Every builder kind and the reviewer are shown to start in the project folder.
- The overlap, disabled-show and top-folder texts are pinned.
- Scoped crash-resume, rename and empty mapping, the `checks.md` content, the name clash and `worktree_root_of` are tested.
- No retired agent is resolved from any layer, and each dropped copy is noted.
- The journeybook, SVG, preset asserts and the Commit-line pin are updated.
- clippy finishes with no warnings, and `cargo fmt` changes nothing.

## Out of scope

- Issue 94 and the doc text for issues 36 and 37, which the area 4 spec covers.
- Making subfolder runs load the repo's tracked agents.

## Decisions

- A leftover `factory-committer.md` in the project's own agents folder or the repo's agents folder? Options: drop it in every layer with a `note:` naming the ignored file, drop it silently, keep it and document. Chosen: drop it in every layer with a note. Decided by: user
- The combine lint false positive (a `Task N` mention inside inline backticks in a later plan): fix the lint, not the plan, in this area. Options: fix the lint here, reword job 100's plan only, cancel and recreate job 100. Chosen: fix the lint, here in area 3. Decided by: user
- How much of CI runs on macOS? Options: the full fmt, clippy and test matrix on ubuntu-latest and macos-latest with fail-fast off, tests only on macOS, the process crates only. Chosen: the full matrix, fail-fast off, with a commented cfg gate on any test that is Linux-only. Decided by: user
- What model does `factory probe --harness <h>` use when `<h>` differs from the project's `agent_cli`? Options: that harness's default model for every role (Codex `CODEX_DEFAULT_MODEL`), a new `--model` flag, refuse with a hint. Chosen: the harness default model, named in the report, and `shape_spec` follows the harness argument. Decided by: user
- How loud is a missing fake-claude binary in the CLI tests? Options: build it and panic only if that fails, panic and add a build step to the check rule, panic only in CI. Chosen: the helper builds it and panics with the build hint only when the build fails, because the factory's checks never build it. This replaces the first answer, a bare panic. Decided by: user
- How are the edits the gates cannot pass made (`ci.yml` is a risky path; `scripts/`, `journeybook/` and the SVG have no check; the builder cannot `chmod`)? Options: by hand on main before the job, in the job with a manual approve, new check rules. Chosen: by hand before the job; the job adds only Rust code and the tests that pin those edits. Decided by: user
- The `a-001`/`001-x` worktree name collision? Options: pin the clean failure with a test and a clear error, change the separator, refuse such project names. Chosen: pin the clean failure and make the error name the clashing folder; names do not change. Decided by: user
- A probe outside a registered project with no agent files? Options: add a hint to the error, add the hint plus a fallback to the repo's tracked agents. Chosen: the hint only; the fallback stays with `docs/superpowers/plans/2026-11-01-agent-delivery-fixes.md`. Decided by: user
- Issue 12's message-run lock item is dropped as obsolete, because no commit-message agent runs any more. Decided by: call
- The reviewer prompt keeps naming the worktree root, because golden prompt tests pin it. A test pins it next to the project-folder cwd. Decided by: call
- The root-after-subfolder overlap error names the inner folder. Decided by: call
- Out-of-scope paths keep passing through `scoped_paths`, now documented and tested. `worktree_root_of` stays lexical, documented and tested. Decided by: call
- `factory-procgroup` joins `PKGS` and `CRATES`, because CI never checked it. Decided by: call
- The dashboard CI job runs on macOS too, under the same full matrix. Decided by: call
- The SVG gets one worst-case count line, derived from `fix_rounds = 2`. Decided by: call
