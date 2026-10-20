# Upgrade and version check

## Goal

Finish ticket #1: show the factory version, add `factory upgrade` that updates the slash command and unedited presets and reports shadowing overrides, record a version marker in the factory home, report a `factoryd` and `factory` version mismatch, and document upgrading.

## Overview

A new release reaches a user by a rebuild, and then four manual steps are needed (`factory install --force`, `factory presets refresh`, a check of overrides, a `factoryd` restart). Nothing says which version last touched `~/.factory`, so an older binary meets newer state with a parse error. Nothing tells `factoryd` that its `factory` child is another version. Release pipeline, `--version` and `factory presets refresh` already shipped (v0.1.1, v0.1.2, commits `fbcfd8b`, `1f53c36`).

## Current state

| Area | Today | Where |
|---|---|---|
| Version | `version` in both clap derives; one workspace version `0.1.2`; no non-test use of `CARGO_PKG_VERSION` | `crates/factory-cli/src/cli.rs:5`, `crates/factory-supervisor/src/bin/factoryd.rs:21`, `Cargo.toml:15` |
| Preset refresh | An existing file differing from the embedded text is "unedited" only when it equals one of 4 hardcoded agent texts (`LEGACY`); no hash is recorded; `config.toml` and `commit-context.md` have no legacy path | `crates/factory-core/src/preset.rs:111-175` |
| Refresh output | `written\|updated\|unchanged\|refused: <path>` lines, a count line, a `--force` hint when files were refused; always exit 0 | `crates/factory-cli/src/presets.rs:14-40`, pinned by `crates/factory-cli/tests/presets_refresh.rs` |
| Slash command | `install` writes `<dir>/factory.md`; a different file is an error unless `--force`, which replaces it with no backup | `crates/factory-cli/src/install.rs:28-49` |
| Home files | `presets/`, `projects/`, `config.toml`, `api.token`, `queue/`, `results/`, `run/`, `logs/`; no version file; `HomeConfig` has `deny_unknown_fields` | `crates/factory-core/src/home.rs:15-50`, `homeconfig.rs:15` |
| Binary choice | `ProcessSpawner::from_env`: `FACTORY_BIN`, else `factory` next to `factoryd`; called once in `run` | `crates/factory-supervisor/src/spawn.rs:64-73`, `factoryd.rs:81` |
| Layers | Preset, project and repo agent files merge by name, later wins; the merge keeps no provenance | `crates/factory-core/src/resolve.rs:236-281` |
| Output | `project show` prints `project:`, `repo:`, `preset:` and more; `factoryd status` prints an optional budget line, then Queue, Running and Results | `crates/factory-cli/src/project.rs:179-188`, `factoryd.rs:139-205` |

Gaps: no way to tell an unedited file without a hand-kept list, no marker, no mismatch report, no shadow report, no single upgrade command, no README "Upgrading" section.

## Non-goals

- Automatic migration of board, ledger or leases, and rollback beyond a clear error.
- Upgrading the dashboard binary or restarting `factoryd` (the README tells the user to).
- A `[settings]` key shadow report, and a per-file version stamp inside preset files.
- A new `factory preset sync`: `factory presets refresh` already does that job.

## Design

Flow of `factory upgrade [--dry-run]`:

```
check marker (newer -> stop) -> slash command -> presets refresh -> shadow report -> write marker
```

### 1. Version marker

- File `<home>/VERSION` (`Home::version_file()`), one line, the version text such as `0.1.2`.
- New module `factory-core/src/version.rs`:
  - `pub const FACTORY_VERSION: &str = env!("CARGO_PKG_VERSION");`
  - `pub fn read_marker(home: &Home) -> Result<Option<String>, CoreError>`: `None` when the file is missing.
  - `pub fn check(home: &Home, current: &str) -> Result<(), CoreError>`: fails when the marker is newer than `current`, or when it is not a version. Missing or older is fine.
  - `pub fn stamp(home: &Home, current: &str) -> Result<bool, CoreError>`: writes atomically when the marker is missing, unparseable or older; returns whether it wrote. Never writes an older version over a newer one.
  - Comparison is on the numeric `major.minor.patch` triple. Text after `-` or `+` is ignored. Fewer than three numbers or a non-number means "not a version".
- Error text: `this home was last used by factory 0.2.0, this binary is 0.1.2: install a newer binary, or delete <home>/VERSION to ignore this` and `<path> does not hold a version (got "<text>"): delete it and run factory upgrade`.
- The marker is a high-water mark: the newest binary that touched the home. It does not say the presets are current, so `upgrade` never skips work because of it.
- `check` runs wherever a command opens the home: `factory project ...`, `factory presets ...`, `factory upgrade` and every command with `--project <name>` in `crates/factory-cli/src/main.rs`. Commands without `--project` use the in-repo layout (`CliCtx::from_env`), do not open the home and are not checked. In `factoryd` it runs for every command except `is-running`. `--version` and `factory install` never open the home.
- `stamp` runs in `factory upgrade`, after `factory project add` succeeds, and at the start of `factoryd run`.
- `factory upgrade` is gated like the others. Its own output reports the newer marker. Running it with an older binary would move unedited presets back to older text.

Example: a 0.1.2 binary on a home stamped `0.2.0` prints the first error text and exits 1.

### 2. Version display

- `factory project show <name>` adds a line `version: 0.1.2` right after the `project:` line, in the normal and the disabled variant.
- `factoryd status` appends a final line `factoryd 0.1.2`, and `warning: <mismatch text>` before it when design 3 found a mismatch. The line goes last so the text still starts with the budget line when there is one.

### 3. Binary mismatch check in `factoryd`

- `factory-supervisor/src/version_check.rs`: `pub fn binary_version(bin: &Path) -> Result<String, String>` runs `<bin> --version` with a 5 second timeout and reads the last word of the first line (clap prints `factory 0.1.2`). `pub fn mismatch(bin: &Path, own: &str) -> Option<String>` returns `factory binary <path> is 0.1.1, factoryd is 0.1.2` when they differ, and `cannot read the version of <path>: <why>` when it cannot run.
- `ProcessSpawner::bin()` exposes the chosen path.
- `factoryd run` calls `mismatch` once after `ProcessSpawner::from_env`. A result is printed as one `warning:` line on stderr and logged at warn. Work is never blocked.
- `factoryd status` calls it the same way, using `ProcessSpawner::from_env`. When that fails (no binary), the status shows no mismatch line.

### 4. Preset manifest

- File `<home>/presets/.manifest`, text lines `<sha256 hex>  <path relative to presets/>`, sorted by path. Written atomically.
- New dependency `sha2` (pinned) in `factory-core` for the hash. A std hasher is not stable across Rust versions, so it cannot be stored.
- `preset::seed` and `preset::refresh` record the hash of every file they write or find equal to the embedded text. A refused file keeps its old entry.
- Unedited rule in `refresh`, in order: the file equals the embedded text (`unchanged`); the file hash equals its manifest entry (`updated`); the file equals a `LEGACY` text for that agent (`updated`); else a hand edit: `refused`, or `updated` with `force`.
- A home with no manifest behaves as today for the first refresh, and then has one. An unedited `config.toml` or `commit-context.md` of such a home that differs from the embedded text is refused once. `--force` clears that.
- `refresh` gets a dry-run form: `preset::plan_refresh(presets_dir)` returns the same `RefreshReport` and writes nothing, and `refresh` applies it.
- The output of `factory presets refresh` does not change.

### 5. `factory upgrade [--dry-run]`

New `Upgrade { dry_run }` command in `cli.rs` and `crates/factory-cli/src/upgrade.rs`. Steps, one line each unless noted:

1. Slash command, as `install` finds it: absent, `installed <path>`; equal, `already installed: <path>`; different, the old file is copied to `<path>.bak` (replacing an earlier `.bak`), then `updated <path> (old copy saved as <path>.bak)`.
2. Presets: the lines and count of `factory presets refresh` without `--force`. When files are refused, a last line `refused files stay as they are: factory presets refresh --force overwrites them`.
3. Shadow report (design 6).
4. Marker: `version marker: written 0.1.2` or `version marker: 0.1.2`.

With `--dry-run` every line starts with `would ` where something would change, and nothing is written, not the `.bak`, the manifest or the marker. When `$HOME` is unset, step 1 fails with the `install` error before anything changes.

Example output:
```
updated /home/u/.claude/commands/factory.md (old copy saved as /home/u/.claude/commands/factory.md.bak)
updated: /home/u/.factory/presets/rust/agents/factory-builder.md
12 written, 1 updated, 11 unchanged, 0 refused
shadowed: rocket project-layer factory-builder.md
version marker: written 0.1.2
```

### 6. Shadow report

- `RefreshReport` already holds the path of every file per outcome (`crates/factory-core/src/preset.rs:130-135`), so `plan_refresh` and `refresh` both give the updated paths. A preset agent path has the shape `presets/<preset>/agents/<file>`.
- `preset::shadowed(home, updated: &[PathBuf], tip_for: &dyn Fn(&str) -> Result<Box<dyn BaseTipFiles>, String>) -> Vec<ShadowLine>` with `ShadowLine::Shadow { project, layer: Layer, file }`, `ShadowLine::Skipped { project, reason }` and `Layer::{Project, Repo}`. `factory-core` does no git work: `factory-cli` supplies `tip_for`, built on the engine's `RepoTip` (`crates/factory-engine/src/tipfiles.rs`), which implements `BaseTipFiles` (`resolve.rs:19`).
- A project uses the preset named in its `project.toml` (`ProjectFile.preset`, `project.rs:47`). For each registered project, each updated file whose `<preset>` part equals that name is checked by file name: `Project` when `<home>/projects/<name>/agents/<file>` exists, `Repo` when `.factory/agents/<file>` exists at the base branch tip (the same listing `resolve` uses).
- Only agent files the upgrade updated are checked. A project that cannot load, has no base branch, or whose repository cannot be opened is skipped with `skipped: <project> (<reason>)`. A dirty checkout does not matter, because the tip is a commit.
- Line shape: `shadowed: <project> <project-layer|repo-layer> <file>`. Nothing is edited. With `--dry-run` it uses the files that would be updated.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| Marker newer than the binary | Command stops with the design 1 text, exit 1; `--version` and `install` still work |
| Marker missing | Fine; `stamp` writes it at the next `upgrade`, `project add` or `factoryd run` |
| Marker holds text that is not a version | Stops with the second error text; `delete it and run factory upgrade` |
| `factory` binary cannot run `--version` | Warning line `cannot read the version of <path>: <why>`; work continues |
| `--version` output has no version word | Treated as cannot read |
| Manifest missing or unreadable line | Missing: legacy rule. A bad line is ignored and that file counts as unmanifested |
| Preset file deleted by the user | Written again as `written` |
| `.bak` cannot be written | Upgrade stops before replacing the command |
| `upgrade --dry-run` | Prints `would` lines, writes nothing |
| A project path is gone or its `project.toml` is broken | `skipped: <project> (<reason>)`, other projects still checked |
| Two `upgrade` runs in a row | The second prints `already installed`, `unchanged` lines and no `shadowed` lines |

## Testing

- `factory-core` `version.rs` unit tests: `a_missing_marker_passes_the_check`, `a_newer_marker_fails_with_both_versions_in_the_text`, `an_older_or_equal_marker_passes`, `a_marker_that_is_not_a_version_fails`, `a_pre_release_suffix_is_ignored_in_the_comparison`, `stamp_writes_when_missing_or_older_and_never_downgrades`.
- `factory-core` `preset.rs` unit tests: `seed_records_a_hash_for_every_file_it_writes`, `an_unedited_file_that_differs_from_the_embedded_text_is_updated_through_the_manifest`, `an_edited_file_with_a_manifest_entry_is_refused`, `a_home_without_a_manifest_uses_the_legacy_rule_and_then_gets_one`, `plan_refresh_writes_nothing`, `shadowed_finds_project_and_repo_layer_files`.
- `factory-cli`: `crates/factory-cli/tests/upgrade.rs` with `upgrade_updates_the_command_and_keeps_a_bak`, `upgrade_dry_run_changes_nothing`, `upgrade_refuses_edited_presets_and_says_how_to_force`, `upgrade_reports_shadowing_overrides`, `upgrade_stops_on_a_newer_marker`, `a_second_upgrade_changes_nothing`; in `project_cmd.rs` `project_show_prints_the_version`; in `presets_refresh.rs` the existing output asserts stay unchanged.
- `factory-supervisor`: `version_check.rs` tests built on `write_script` (`spawn.rs:200`): `a_matching_binary_has_no_mismatch`, `a_different_version_is_reported`, `a_binary_that_cannot_run_is_reported`; in `factoryd.rs` `status_ends_with_the_factoryd_version` and `status_warns_on_a_mismatch`; the existing budget test keeps `starts_with`.
- Commands: `cargo test -j4 -p factory-core`, `-p factory-cli`, `-p factory-supervisor`, then `cargo clippy -j4 -p factory-core -p factory-cli -p factory-supervisor --all-targets -- -D warnings` and `cargo fmt --check`.

## Files changed

- `crates/factory-core/src/version.rs` (new), `home.rs`, `lib.rs`, `preset.rs`, `Cargo.toml` (sha2).
- `crates/factory-cli/src/cli.rs`, `main.rs`, `upgrade.rs` (new), `project.rs`, `lib.rs`; tests as listed.
- `crates/factory-supervisor/src/version_check.rs` (new), `spawn.rs`, `lib.rs`, `src/bin/factoryd.rs`.
- `README.md` (an "Upgrading" section), `docs/software-factory.md` if it names the commands.

## Constraints

- Always `-j4`; never `--workspace` or `--all`; never set `CARGO_TARGET_DIR`.
- Paths reach the driver through `Home` and `Layout` only (`no_hardcoded_paths.rs`).
- `factory-core` stays free of I/O-heavy dependencies; `sha2` is pure.
- `factory presets refresh` output, and the existing `install` messages, keep their text.
- `upgrade` never overwrites an edited preset file.

## Acceptance criteria

- `factory project show <name>` prints `version: <workspace version>`.
- `factoryd status` ends with `factoryd <version>` and starts with the budget line when a budget is set.
- A binary older than `<home>/VERSION` stops with the message naming both versions; `--version` and `install` still work.
- `<home>/VERSION` is written by `factory upgrade`, `factory project add` and `factoryd run` only when missing, unparseable or older.
- `factoryd run` and `factoryd status` report a version mismatch with its `factory` binary and never block work.
- Preset seed and refresh record file hashes in `presets/.manifest`, and an unedited file that differs from the embedded text is updated without a hand-kept legacy entry.
- `factory upgrade` updates the slash command with a `.bak`, refreshes unedited presets, lists refused files, reports shadowing overrides, writes the marker, and `--dry-run` changes nothing.
- The README has an "Upgrading" section with the steps, the no-running-jobs advice and the layer note.

## Out of scope

- Rollback, migration of board and ledger, dashboard upgrade, `[settings]` shadow report.

## Decisions

- How does `factory upgrade` tell an edited preset file from an unedited one? Options: a hash manifest with the legacy list as a fallback, the legacy list only, a version stamp inside each file. Chosen: a hash manifest in `presets/` written at seed and refresh time, with today's embedded and legacy comparison as the fallback for homes that have no manifest yet. Decided by: user
- When is the version marker written and checked? Options: write on upgrade, project add and factoryd start and check on every open; write on upgrade only; write on every state change. Chosen: a `<home>/VERSION` file, written by `factory upgrade`, `factory project add` and `factoryd run` when it is missing or older, and checked whenever a command opens the home. A newer marker stops the command with a clear message. A missing or older marker is fine. `--version`, `install` and `upgrade` skip the check. Decided by: user
- What does `factoryd` do when its `factory` binary reports another version? Options: warn and keep going, refuse to start, refuse to spawn. Chosen: warn at startup (one line and a log entry) and show the mismatch in `factoryd status` while it lasts, and never block work. Decided by: user
- What does `factory upgrade` do with the installed `/factory` command when it differs from the shipped one? Options: replace it and keep a backup, refuse unless `--force`, replace with no backup. Chosen: replace it and save the old file as `factory.md.bak` next to it; a later upgrade overwrites the `.bak`. Decided by: user
- What does `factory upgrade` report about overrides that shadow new preset text? Options: agent files only and only those it changed, agent files and `[settings]` keys, every shadowing file. Chosen: agent files only, for every registered project, in the project layer and the repo layer, only where the name matches a preset agent file the upgrade updated. It reports and never edits. Decided by: user
- Which options does `factory upgrade` have? Options: `--dry-run` only, `--dry-run` and `--force`, none. Chosen: `--dry-run` only. `upgrade` never overwrites an edited preset file; it lists it as refused and the user decides with `factory presets refresh --force`. Decided by: user
- How is the manifest stored and the version compared? Chosen: text file `presets/.manifest` with `sha2` hashes, and a numeric `major.minor.patch` comparison with no new crate. Decided by: call
- Is `upgrade` gated by a newer marker? Chosen: yes, so it differs from the earlier answer that `upgrade` skips the check. The recovery for a newer marker is a newer binary, and an older `upgrade` would move unedited presets back to older text. Decided by: user (approved with the spec as written)
- Where do the new output lines go? Chosen: `version:` after `project:` in `project show`, and the `factoryd` line last in `factoryd status` so the budget line stays first. Decided by: call
- Delivery? Chosen: one series of four plans: marker and display, mismatch check, manifest and refresh, upgrade command with docs. Decided by: call
