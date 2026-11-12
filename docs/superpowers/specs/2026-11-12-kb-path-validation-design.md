# KB path validation design

## Goal

Validate and normalise `kb_path` so a bad value is refused with a clear message and the KB run's Edit and Write grants name exactly one real folder.

## Overview

GitHub issue #59 reports four defects in how `kb_path` reaches the KB refresh run. A code read in this session confirms all four.

- A. `Config::kb_dir` (`config.rs:548`) expands a leading `~` and nothing else. A relative path, a `..` component and glob characters pass through. The load check (`config.rs:419`) only rejects a blank value.
- B. `kb_allowed_tools` (`kb.rs:23`) builds `Edit(//<path>/**)` and `Write(//<path>/**)` from that path with the leading `/` trimmed. A relative path becomes the rule for an absolute folder of the same name, a `..` stays in the rule, and `*`, `?`, `[`, `{` widen the grant to sibling folders. The builder's deny rules canonicalise the folder (`permissions.rs:99-109`); the KB rule does not.
- C. With `HOME` unset or empty, `~/x` gives `kb_dir() == None`, and `kb_refresh` (`compat.rs:601`) and `kb_refresh_step` (`kb.rs:160`) both report "kb_path is not set" although it is set.
- D. `factory kb-refresh <id>` takes the project lease before it looks up the job (`commands.rs:1134-1135`), so an unknown id takes and releases the lease first.

The docs allow only an absolute path or one that starts with `~/` (`project-toml-reference.md:166`, `software-factory.md:94`), so rejecting the other forms fixes a gap and removes no documented behaviour.

## Current state

| Piece | Today | Gap |
|---|---|---|
| `Config::kb_dir` | `kb_path` through `expand_home`, `None` when unset or HOME missing (`config.rs:548-552`) | One `None` for two different causes |
| Load check | Blank `kb_path` is an error (`config.rs:419-423`) | No check of the text |
| `kb_allowed_tools` | Trims the leading `/` and builds the `//` rules (`kb.rs:23-37`) | No canonical folder, no character check |
| `kb_spec` | Takes the path for the allow rule and `add_dirs` (`kb.rs:70-90`) | Same string for both |
| `kb_refresh_step` | `is_dir()` check, then the run (`kb.rs:159-180`) | No canonicalise |
| `kb_refresh` | "kb_path is not set" when `kb_dir()` is `None` (`compat.rs:601-603`) | Wrong message for a missing HOME |
| `commands.rs` `kb_refresh` | `hold_project`, then `job(id)`, then the stage check (`commands.rs:1130-1154`) | Lease taken for an unknown id |
| Other `kb_dir()` callers | `specs.rs:75` and `:90`, `probe.rs:123`, `codex_prep.rs:110` | Read the configured path; no change needed |

Relative paths never worked: `is_dir()` tests against the process directory while the agent runs in the repo root (`kb_spec`), so the check and the run disagree.

## Non-goals

- Supporting a repo-relative `kb_path`. It is rejected; a follow-up can add it if asked.
- Changing the prompt text, `specs.rs`, `probe.rs` or `codex_prep.rs`. They keep `kb_dir()`.
- Escaping glob characters. They are rejected, because the permission rule syntax has no documented escape.
- Testing how the `claude` permission matcher treats `..` or symlinks. Rejecting `..` and canonicalising is correct either way.

## Design

Flow of a KB run:

```
load:        check_kb_path(raw)               -> text rules only (never reads HOME)
kb_refresh:  kb_dir_checked()                 -> Ok(None) "kb_path is not set"; Err -> reported by the step
step:        kb_dir_checked() -> is_dir -> canonicalize -> check chars again -> kb_spec(real folder)
```

### 1. The resolver (`factory-core`, `config.rs`)

```rust
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KbPathError {
    #[error("kb_path {path} starts with ~ but HOME is unset or empty")]
    HomeUnset { path: String },
    #[error("kb_path {path} must be absolute or start with ~/")]
    NotAbsolute { path: String },
    #[error("kb_path {path} must not contain a .. component")]
    ParentDir { path: String },
    #[error("kb_path {path} must not contain {ch:?}")]
    BadChar { path: String, ch: char },
}

/// The text rules. Needs no environment.
pub fn check_kb_path(raw: &str) -> Result<(), KbPathError>;
/// The text rules, then the `~` expansion with `home`.
pub fn resolve_kb_path(raw: &str, home: Option<&str>) -> Result<PathBuf, KbPathError>;
impl Config {
    pub fn kb_dir_checked(&self) -> Result<Option<PathBuf>, KbPathError>; // None when unset
    pub fn kb_dir(&self) -> Option<PathBuf>; // kb_dir_checked().ok().flatten()
}
```

- Order of the text rules: the value must be `~`, start with `~/`, or start with `/`, else `NotAbsolute` (so `kb/x` and `~user/x` fail); then any `..` component (split on `/`) gives `ParentDir`; then the first character in `* ? [ ] { } !` or a control character gives `BadChar`.
- `resolve_kb_path` expands with `expand_home` and returns `HomeUnset` when `HOME` is missing or empty.
- `Config::parse` calls `check_kb_path` after the blank check and maps an error to `CoreError::Config(e.to_string())`. A bad value therefore gives the fail-closed config, as the project rule requires. `HomeUnset` is never a load error.

Example: `kb_path = "~/kb/rocketapi"` loads everywhere. With `HOME` unset, `kb_dir_checked()` is `Err(HomeUnset)`, and `kb_dir()` is `None`.

### 2. The KB refresh messages (`compat.rs`, `kb.rs`)

- `Driver::kb_refresh` says "kb_path is not set" only for `Ok(None)`. For `Err(_)` it continues, so `kb_refresh_step` reports the typed error through the usual `kb refresh failed, ...` ledger line.
- `kb_refresh_step` matches `kb_dir_checked()`: `Ok(None)` gives `Failed("kb_path is not set")`, `Err(e)` gives `Failed(e.to_string())`.

### 3. The canonical folder (`kb.rs`)

After the `is_dir()` check, `kb_refresh_step` calls `canonicalize()` and applies the character rules of Design 1 to the real path. A failed canonicalise or a failed character check gives `Failed("the KB folder <configured> resolves to <real>: <reason>")` and no run starts. The allow rules and `add_dirs` of `kb_spec` use the real folder. The prompt keeps the configured path.

### 4. The lease order (`factory-cli`, `commands.rs`)

In `kb_refresh`, load the job and check that it is merged before `hold_project`. The project lease is still held for the run, and the foreign job lease check stays after it.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| `kb_path` unset | `kb_dir_checked()` is `Ok(None)`; "kb_path is not set" |
| `kb_path = ""` | Existing load error, unchanged |
| `kb/x`, `./x`, `~user/x` | Load error `NotAbsolute` |
| `/a/../b`, `~/../x` | Load error `ParentDir` |
| `/a/*`, `/a/b[1]`, `/a/{x}`, `/a/b!`, a path with a tab or newline | Load error `BadChar` |
| `~/x` and HOME unset or empty | Loads; the KB run fails with the `HomeUnset` message |
| `/` or `~` alone | Accepted: the grant covers that whole folder, by the user's choice |
| Symlinked KB folder | Rules use the real folder |
| Symlink target contains a glob character | The run fails with "resolves to ... must not contain" |
| Folder does not exist | "the KB folder ... does not exist", as today |
| Unknown job id for `kb-refresh` | Refused for the unknown job without taking the project lease |
| Absolute or `~/` path as documented | Behaves as before |

## Testing

`cargo test -j4 -p factory-core` (`config.rs` tests):
- `check_kb_path_accepts_the_documented_forms`: `/kb/rocketapi`, `~/kb`, `~`.
- `kb_path_with_tilde_expands_with_home`: `resolve_kb_path("~/kb", Some("/home/u"))` is `/home/u/kb`.
- `kb_path_with_tilde_and_no_home_reports_home_unset`: `None` and `Some("")` give `HomeUnset`.
- `relative_kb_path_is_rejected_at_load`: `kb/x`, `./x`, `~user/x` make `Config::parse` fail with `must be absolute`.
- `kb_path_with_parent_dir_is_rejected_at_load`: `/a/../b` and `~/../x`.
- `kb_path_with_glob_characters_is_rejected_at_load`: one case per character `* ? [ ] { } !` and a tab.
- `absolute_kb_path_still_loads`, `tilde_kb_path_loads_without_home`.
- `kb_dir_hides_an_error_and_kb_dir_checked_shows_it`.

`cargo test -j4 -p factory-engine`:
- `tests/kb.rs`: `kb_allow_rule_uses_the_canonical_folder` and `a_symlink_to_a_glob_path_fails_the_run` (both Unix only: a symlink in the temp dir).
- `tests/kb_home_unset.rs`: `an_unset_home_with_tilde_kb_path_does_not_say_not_set` (one test in its own file, so no other test reads `HOME`).
- `an_unset_kb_path_writes_nothing` stays and still expects "kb_path is not set".

`cargo test -j4 -p factory-cli`:
- `tests/project_lease_cli.rs`: `kb_refresh_for_an_unknown_job_is_not_blocked_by_the_project_lease`.
- `--test no_hardcoded_paths` keeps passing.

Run `cargo clippy -j4 -p factory-core -p factory-engine -p factory-cli`.

## Files changed

| File | Change |
|---|---|
| `crates/factory-core/src/config.rs` | `KbPathError`, `check_kb_path`, `resolve_kb_path`, `kb_dir_checked`, the load check, tests |
| `crates/factory-core/src/lib.rs` | Export the new items |
| `crates/factory-engine/src/kb.rs` | Typed errors, canonical folder, character re-check |
| `crates/factory-engine/src/compat.rs` | The "not set" message only for `None` |
| `crates/factory-cli/src/commands.rs` | Job and stage check before `hold_project` |
| `crates/factory-engine/tests/kb.rs`, `kb_home_unset.rs`, `crates/factory-cli/tests/project_lease_cli.rs` | New tests |
| `docs/reference/project-toml-reference.md`, `docs/reference/software-factory.md` | State the rules |

## Constraints

- A bad `kb_path` gives the fail-closed config; the new rules never default to permissive.
- Paths reach the driver through `Layout`; `kb_path` is user config, so the resolver lives with `Config`. `no_hardcoded_paths.rs` must pass.
- No `git` shell-outs. No `Cargo.toml` change; `thiserror` is already used in `factory-core`.
- Commands use `-j4`, never `--workspace` or `--all`.
- The touched files match no `risk_paths` glob, but the change scopes Edit and Write grants, so it gets a careful review.

## Acceptance criteria

- A relative, `..`, glob-character or control-character `kb_path` fails the config load with a message naming the cause.
- A `~/` path with `HOME` unset loads, and the KB refresh fails with a message that names `HOME`, not "kb_path is not set".
- `factory kb-refresh` says "kb_path is not set" only when `kb_path` is unset.
- The Edit and Write rules and `add_dirs` of a KB run use the canonical absolute folder.
- A symlink target with a glob character fails the run before it starts.
- `factory kb-refresh` for an unknown job does not take the project lease.
- Absolute and `~/` configs behave as before, and the Rocket fixture config still loads.
- The two reference docs state the rules.

## Out of scope

- A repo-relative `kb_path`.
- Changing which tools or folders a KB run may use beyond the Edit and Write scope.

## Follow-ups

- `codex_prep.rs:110` sets the KB run's working directory from the configured path, not the canonical one. Make it use the same real folder.

## Decisions

- Which checks fail the config at load? Options: text rules at load and `HOME` at run; everything at load; nothing at load. Chosen: the text rules at load and `HOME` at run, so a missing `HOME` never makes the config fail closed. Decided by: user
- How is the resolver exposed? Options: a new `kb_dir_checked` next to `kb_dir`; change `kb_dir` to a `Result`; resolve once into `Config`. Chosen: `kb_dir_checked`, with `kb_dir` kept as a thin wrapper. Decided by: user
- A symlink may resolve to a path with glob characters. Options: re-check and fail the run; use the canonical path as is; keep the configured path for `add_dirs`. Chosen: re-check the real path and fail the run, and use the real folder for the allow rule and `add_dirs`. Decided by: user
- Defect 4, the lease order. Options: move the job and stage check before `hold_project`; note only; also move the lease owner check. Chosen: move the job and stage check. Decided by: user
- Glob characters are rejected, not escaped, because the rule syntax has no documented escape. Decided by: call
- A repo-relative `kb_path` is rejected, as the docs never allowed it. Decided by: call
- The `HomeUnset` test sits in its own file, because it clears `HOME` and must not race other tests. Decided by: call
- Should `kb_path` be allowed to name `/` or the whole home folder? Options: reject both; reject only `/`; allow both. Chosen: allow both; the grant is the user's own choice, and a KB folder inside home is the normal case. Decided by: user
