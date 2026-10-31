# Check coverage defaults for the rust preset

## Goal

The rust preset covers the root `Cargo.toml` and `Cargo.lock` with a check and exempts common non-code files, so jobs that touch them stop parking as `no-checks`. The upgrade steps are written down and added to the release checklist.

## Overview

The `no-checks` gate parks a job when a changed code path matches no `[[checks]]` rule and no `unchecked_paths` glob (GitHub issue 32). Any path that is not a doc path counts as code (`crates/factory-core/src/gate.rs:37`). The rust preset has two rules, `crates/*/**` and `src/**`-style paths, so a root manifest, a lockfile or a `LICENSE` edit parks a job that nothing is wrong with. The preset also sets no `unchecked_paths`, so every rust project meets this on its first release or license change.

## Current state

| Piece | Today | Where |
|---|---|---|
| Preset check rules | `crates/*/**` (per-crate check and test), `src/**`, `tests/**`, `benches/**`, `examples/**`, `build.rs` (bare `cargo check -j4`, `cargo test -j4`) | `crates/factory-core/presets/rust/config.toml:21-28` |
| Preset `unchecked_paths` | not set | same file |
| Preset comment | says a virtual workspace "relies on the crates/*/** rule above" | same file, line 25 |
| Gate | `uncovered_code_paths` = code paths minus paths matched by a `[[checks]]` rule minus paths matched by `unchecked_paths`; matching of `unchecked_paths` ignores case | `gate.rs:52-58`, `gate.rs:25-33` |
| Glob | `**/x` matches `x` at the root; `a/**` does not match `a` | `crates/factory-core/src/risk.rs:11-32` |
| Duplicate commands | run once | `crates/factory-checks/src/plan.rs:122` |
| Project override | `[[settings.checks]]` and `unchecked_paths` in `project.toml` replace the preset value, lists included | `docs/reference/project-toml-reference.md` |
| Upgrade | `factory upgrade` refreshes unedited preset files and never touches project settings | `README.md` (Upgrading), `docs/guides/getting-started.md` section 15 |

Gaps: root `Cargo.toml` and `Cargo.lock` match no rule. `LICENSE`, `.gitignore` and similar match nothing. A project with its own `[[settings.checks]]` (rocket-factory, rocket, rocketvault) does not inherit new preset rules and must add them itself.

## Non-goals

- No change to the gate, to `Config` or to `factory upgrade` code. The upgrade does not warn about uncovered paths.
- No change to the go, node, ruby or csharp presets.
- No edit of any file under `~/.factory`. Those registry files are outside the repo.
- No change to `cliff.toml`.

## Design

Flow: the preset text changes, `factory upgrade` copies it into `~/.factory/presets/rust/` when that file is unedited, and projects without their own `[[settings.checks]]` pick it up on their next run.

### 1. Root manifest rule

A third `[[checks]]` table in `crates/factory-core/presets/rust/config.toml`:

```toml
# Root manifest and lockfile. A change here can break any member, so check the whole workspace.
[[checks]]
paths = ["Cargo.toml", "Cargo.lock"]
commands = ["cargo check -j4"]
```

- The command has no `{crate}` and no `--workspace`, so it passes the validator in `project-toml-reference.md`.
- It runs once when a diff also hits `build.rs` or `src/**`, because identical commands run once.
- Only the root files match. `crates/*/Cargo.toml` stays under the per-crate rule, and `dashboard/Cargo.toml` stays uncovered in the preset on purpose, as it is a separate workspace.
- It runs `cargo check` only, with no tests. A dependency bump that breaks a member's tests is found when that member next changes, or by CI.

### 2. Generic `unchecked_paths`

A top-level key in the same file:

```toml
unchecked_paths = ["LICENSE", "LICENSE-*", "LICENSE.txt", "COPYING", "COPYING.*", ".gitignore", ".gitattributes", ".editorconfig"]
```

- The names are exact and have no `**/` prefix, so they match only at the repo root. Matching ignores case and `*` matches any characters inside one path segment, so a pattern such as `**/LICENSE*` would also exempt code files like `xtask/src/license.rs` or `tools/licenses.rs` from the `no-checks` gate. These patterns do not.
- `rustfmt.toml`, `clippy.toml` and `rust-toolchain.toml` are not listed, because they change what the checks do.
- `scripts/**`, `cliff.toml` and `*.pub` are not listed, because they are repo-specific.

### 3. Preset comment

The comment above the single-crate rule keeps its text, with the sentence "A virtual workspace ... relies on the crates/*/** rule above" replaced by a pointer to the root rule: a virtual workspace's root manifest is covered by the rule below.

### 4. Docs

- `docs/reference/project-toml-reference.md`: in the `unchecked_paths` row and the checks section, say the rust preset now sets a generic list and a root manifest rule, and that a project override replaces the list, so a project that sets either must copy the preset entries it still wants. Show the snippet for a project with its own `[[settings.checks]]`.
- `README.md` (Upgrading) and `docs/guides/getting-started.md` (section 15): a "Check coverage" note after the upgrade steps. It says: run `factory upgrade`; a project with `[[settings.checks]]` or `unchecked_paths` in `project.toml` keeps its own values, so add the root rule and the exempt paths there; use `factory --project <name> status` to see a `no-checks` park.
- `docs/reference/software-factory.md`: one sentence in the `no-checks` gate bullet that the rust preset covers the root manifest.
- `docs/operations/releasing.md`: a step in "Cutting a release" to paste the "Check coverage" note into the draft release, next to the probe tables. It also says a `chore:` job commit is not in the notes, so the note must be pasted by hand.

## Error handling and edge cases

| Case | Behaviour |
|---|---|
| Diff touches only root `Cargo.lock` | runs `cargo check -j4` and passes the gate; the diff is still risky by `risk_paths` |
| Diff touches `Cargo.toml` and `build.rs` | `cargo check -j4` runs once |
| Project overrides `[[settings.checks]]` | preset rules are not applied; project must add the root rule itself |
| Project overrides `unchecked_paths` | the preset list is replaced; project must copy the entries it wants |
| `LICENSE-MIT`, `LICENSE.txt`, `COPYING.txt` | exempt at the repo root through `LICENSE-*`, `LICENSE.txt` and `COPYING.*` (`LICENSE.md` is already a doc path) |
| `xtask/src/license.rs`, `tools/licenses.rs` | stay uncovered, so they park as `no-checks` |
| `vendor/x/LICENSE`, `sub/.gitattributes` | stay uncovered, the exemptions match the repo root only |
| `license` in lower case | exempt, matching ignores case |
| Root `Cargo.toml` in a large workspace | the bare `cargo check -j4` builds every member; the known cost of option "check only" |
| `Cargo.toml` inside `dashboard/` | still uncovered by the preset |
| Edited preset file in `~/.factory/presets/rust/` | `factory upgrade` lists it as `refused` and leaves it; `factory presets refresh --force` replaces it |

## Testing

In `crates/factory-core/src/preset.rs` (module `tests`), parsing the embedded rust preset with `Config::parse`:

- `rust_preset_covers_the_root_manifest_and_lockfile`: `uncovered_code_paths(["Cargo.toml", "Cargo.lock"], &config)` is empty; the rule's command is `cargo check -j4`.
- `rust_preset_root_rule_does_not_cover_other_manifests`: `dashboard/Cargo.toml` is still uncovered.
- `rust_preset_exempts_license_and_dotfiles`: `LICENSE`, `LICENSE-MIT`, `license`, `LICENSE.txt`, `COPYING`, `COPYING.txt`, `.gitignore`, `.gitattributes` and `.editorconfig` at the root are not uncovered.
- `rust_preset_does_not_exempt_tooling_files`: `rustfmt.toml`, `rust-toolchain.toml`, `scripts/release.sh`, `cliff.toml`, `xtask/src/license.rs`, `tools/licenses.rs`, `vendor/x/LICENSE` and `sub/.gitattributes` are uncovered.
- The existing `rust_checks_use_cargo_with_j4_and_go_checks_use_go` keeps passing and covers the new command.

Commands: `cargo test -j4 -p factory-core rust_preset`, then `cargo test -j4 -p factory-core`, then `cargo clippy -j4 -p factory-core --all-targets -- -D warnings`.

## Files changed

- `crates/factory-core/presets/rust/config.toml`
- `crates/factory-core/src/preset.rs` (tests only)
- `docs/reference/project-toml-reference.md`
- `docs/reference/software-factory.md`
- `docs/guides/getting-started.md`
- `README.md`
- `docs/operations/releasing.md`

## Constraints

- No `--workspace` or `--all` in any command, and `-j4` on every cargo command.
- No new `Config` field. The preset uses `unchecked_paths` and `[[checks]]`, which exist.
- The preset must still parse with `Config::parse` and must not mention Rocket (the test at `preset.rs:560-565` bans it).

## Acceptance criteria

- The embedded rust preset has a rule for `Cargo.toml` and `Cargo.lock` that runs `cargo check -j4`.
- A diff of only the root `Cargo.toml` and `Cargo.lock` has no uncovered code paths under the rust preset.
- The embedded rust preset sets `unchecked_paths` to the eight root-only names, and `LICENSE`, `.gitignore` and the other listed files are not uncovered.
- `dashboard/Cargo.toml`, `rustfmt.toml`, `rust-toolchain.toml`, `scripts/release.sh` and `cliff.toml` stay uncovered under the preset.
- The reference doc says a project override replaces the preset's checks and `unchecked_paths`, with a snippet.
- `README.md` and `getting-started.md` carry a "Check coverage" note after the upgrade steps.
- `releasing.md` has a step to paste that note into the draft release.
- `cargo test -j4 -p factory-core` and `cargo clippy -j4 -p factory-core --all-targets -- -D warnings` pass.

## Out of scope

- Editing `~/.factory/projects/rocket-factory/project.toml` and `~/.factory/projects/rocket/project.toml` to add the root rule and the repo-specific paths (`scripts/**`, `cliff.toml`, `*.pub`). The maintainer does this by hand, outside the job.
- Running `factory upgrade` or `factory presets refresh` on the maintainer's machine so the spec-reviewer preset and the new rules exist.
- A `factory upgrade` warning for projects that shadow the preset checks.

## Follow-ups

None.

## Decisions

- What should the change deliver, with the registry files edited by hand? Options: preset fix plus docs, docs only, preset fix plus an upgrade warning. Chosen: preset fix plus docs. Decided by: user
- Which command covers the root manifest? Options: `cargo check -j4` only, check and test, `cargo metadata --no-deps`. Chosen: `cargo check -j4` only. Decided by: user
- Which paths does the preset exempt? Options: generic list in the preset with repo-specific paths in the registry, everything in the preset, none. Chosen: generic list in the preset. Decided by: user
- Where do the upgrade steps go? Options: docs plus a release checklist step, docs plus a cliff footer, docs only. Chosen: docs plus a release checklist step. Decided by: user
- How narrow is the exempt list, given that `**/LICENSE*` would also exempt `xtask/src/license.rs`? Options: exact names at the repo root, exact names in any folder, only the dotfiles. Chosen: exact names at the repo root, no `**/` prefix and no trailing `*` after the name. Decided by: user
