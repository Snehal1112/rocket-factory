# Codex KB canonical folder design

## Goal

Make the Codex KB run use the same real KB folder as the Claude KB run, so a symlinked `kb_path` behaves the same on both harnesses.

## Overview

GitHub issue #65 follows #59. `kb_refresh_step` (`kb.rs:160`) resolves `kb_path`, canonicalizes it, re-checks the characters of the real path and puts the real folder in `spec.add_dirs` (`kb.rs:99`). The Claude run uses that folder for its Edit and Write grants. The Codex run does not: `prepare_for_codex` (`codex_prep.rs:109-118`) calls `self.config.kb_dir()` again and uses the configured string for two things.

- `spec.cwd = kb_dir` (`codex_prep.rs:116`), the folder Codex works in and the root of its `workspace-write` sandbox.
- `Repo::open(&kb_dir)` for `kb_auto_commit` (`codex_prep.rs:112`), which decides the extra writable git dir.

With a `kb_path` that is a symlink, Codex therefore works through the link while Claude works in the target. The configured path also skips the character check on the real path. A second gap: `kb_dir()` hides an invalid path as `None`, and then `prepare_for_codex` keeps the run in the main checkout (`kb_spec` sets `cwd` to the repo root, `kb.rs:76`) with a `workspace-write` sandbox.

## Current state

| Piece | Today | Gap |
|---|---|---|
| `kb_spec` (`kb.rs:70`) | `cwd` is the repo root, `add_dirs` holds the real KB folder | Codex ignores `add_dirs` |
| `prepare_for_codex` (`codex_prep.rs:77`) | For role `kb`, `cwd` and the git-dir lookup come from `config.kb_dir()` | Configured string, not the real folder |
| `kb_refresh_step` | Resolves, canonicalizes and checks before `run_agent` | Result is not passed to the Codex step |
| Probe `Kb` role (`probe.rs:121`) | `kb_dir()` or the probe work dir is the `kb_dir` given to `kb_spec` | Its `add_dirs` is already right |
| `kb_runs_in_the_kb_folder_under_codex` (`tests/codex_prep.rs:333`) | Asserts `cwd == config.kb_dir()` | Must compare with the canonical folder |

## Non-goals

- A second resolve or character check inside `prepare_for_codex`. The KB step already did it.
- Warning when the KB repo cannot be opened for `kb_auto_commit`. A separate draft covers that.
- Changing the Claude run, `kb_spec` or `kb_allowed_tools`.

## Design

```
kb_refresh_step: resolve, canonicalize, check -> kb_spec(..., real) -> add_dirs = [real]
prepare_for_codex (role kb): folder = spec.add_dirs.first() -> cwd, git dir
```

### 1. Take the folder from `spec.add_dirs`

In `prepare_for_codex`, for a role that has `kb`, let `kb_dir = spec.add_dirs.first().cloned()`. When it is `Some`: `spec.cwd = kb_dir`, and when `config.kb_auto_commit` is true and `Repo::open(&kb_dir)` succeeds, push `kb_repo.common_git_dir()` into `writable_roots`. When it is `None`, `spec.cwd` and `writable_roots` stay as they are. The `config.kb_dir()` call is removed from this function.

Example: `kb_path = "/home/u/kb-link"`, a symlink to `/data/kb/rocketapi`. Before: Codex `cwd` is `/home/u/kb-link`. After: `/data/kb/rocketapi`.

### 2. Effect on the probe

The probe `Kb` role passes the KB folder, or its work folder when `kb_path` is unset, to `kb_spec`, so `add_dirs` holds it. Under Codex the probe now runs in that folder, where it ran in the repo root before. This matches the probe's intent. `tests/probe.rs:72` (writable roots empty) is unaffected because the probe clears them in its own shaping (`probe.rs:232`).

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| `kb_path` is a symlink | Codex `cwd` is the target folder |
| `kb_path` is a plain real folder | Same `cwd` as before |
| `kb_auto_commit` true, KB folder is a git repo | Its common git dir is a writable root |
| `kb_auto_commit` true, folder is not a repo | No extra root, as today |
| `kb_auto_commit` false | No extra root |
| `add_dirs` empty for a `kb` role | `cwd` and roots untouched |
| Other roles | Unchanged |

## Testing

- `crates/factory-engine/tests/codex_prep.rs`:
  - update `kb_runs_in_the_kb_folder_under_codex` to expect the canonicalized KB folder;
  - `kb_under_codex_runs_in_the_target_of_a_symlinked_kb_path` (a symlink to the KB folder as `kb_path`; `cwd` equals the target);
  - `kb_under_codex_gets_the_kb_git_dir_when_auto_commit_is_on` (the KB folder is a git repo reached through a symlink, `kb_auto_commit` true; `writable_roots` holds that repo's common git dir);
  - `kb_under_codex_has_no_extra_root_without_auto_commit`.
- `crates/factory-engine/src/codex_prep.rs` unit test `a_kb_spec_without_add_dirs_keeps_its_cwd`.
- Commands: `cargo test -j4 -p factory-engine`, then `cargo clippy -j4 -p factory-engine`.

## Files changed

- `crates/factory-engine/src/codex_prep.rs`
- `crates/factory-engine/tests/codex_prep.rs`

## Constraints

- Claude runs and `kb_spec` do not change. No `Cargo.toml` change, no `git` shell-outs, no `/proc` reads.
- Engine logic stays behind the existing `AgentRunner` port; no new trait.
- Use `-j4`. Comments are short full sentences.

## Acceptance criteria

- Under Codex, the KB run's `cwd` is the real, canonical KB folder, also when `kb_path` is a symlink.
- Under Codex with `kb_auto_commit` on, the KB repo's common git dir is a writable root, found through the real folder.
- `prepare_for_codex` no longer calls `kb_dir()`.
- A `kb` spec without `add_dirs` keeps its `cwd` and has no extra writable root.
- Claude KB runs and all other Codex roles behave as before.

## Out of scope

- The warning when the KB repo cannot be opened.
- A Codex-only second validation of the KB folder.

## Follow-ups

None.

## Decisions

- Where does the Codex KB run get its folder? Options: from `spec.add_dirs[0]`, resolve again in codex prep, a shared helper on `Engine`. Chosen: from `spec.add_dirs[0]`. Decided by: user
- Should the probe's Codex KB run move to its KB folder too? Options: yes, since `add_dirs` already holds it; keep the repo root for the probe. Chosen: yes. Decided by: call
