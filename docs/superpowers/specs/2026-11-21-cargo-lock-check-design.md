# Cargo diffs with no new outside code merge without a park

Date: 2026-11-21. Item 4 of the 2026-11-19 efficiency adviser brief (the risky-diff park). Revised after three adviser reviews and a builder check.

## Intent

`risk_paths` lists `**/Cargo.toml` and `**/Cargo.lock`, so every diff that touches a Cargo file is risky. A risky diff gets its review and fix rounds, then parks for the human to approve. Most of these diffs bring in no outside code:

- **The parks:** rocket-factory jobs 025, 036 (three parks), 086 and 088, and Rocket jobs 033 and 034.
- **What the Cargo lines did:**
  - `serde_json.workspace = true`, `sha2.workspace = true`, `toml.workspace = true`;
  - a new `[[bin]]`;
  - a `path` dependency on an internal crate;
  - `libc` pinned to the version already in the lock.
- **The one exception:** only 086 brought in an outside crate (`signal-hook`). It also added a new internal crate, `factory-procgroup`.

`Cargo.lock` records every outside crate with its name, version, source and checksum. When that set doesn't change, and the lock is proven current for the manifests at head, no outside code came in. The park then buys nothing.

Success: a Cargo diff that brings in no outside code and no new build-time code merges after review like a normal diff. A diff that adds, removes or changes an outside crate still parks, and so does one the check can't settle.

## Goals

1. A diff counts as normal, not risky, when all of these hold:
   - every path that makes it risky is a Cargo file that matched only Cargo-only globs (section 1);
   - for every workspace the diff touches, cargo proves the head lock current (section 3);
   - the lock check gives Same for each of those locks (section 2);
   - the structural check gives Same (section 4);
   - `cargo_lock_check` is on.
2. Every place in the engine that asks whether a whole job diff is risky gets the same answer.
3. Anything the checks can't settle counts as risky.

## Non-goals

- **Other manifests:** `package.json`, `go.mod`, `*.csproj` and `Gemfile` stay path-based.
- **The plan-time check** in `spec_check.rs` (`risky_without_user_decision`). It has no diff, so a plan that names `Cargo.toml` still needs a user decision in the spec.
- **Judging an outside crate.** An outside change parks, as today.
- **Feature flags and dependency specs on crates already in the lock.** A change such as `serde = { workspace = true, features = ["derive"] }`, a moved line, a renamed key (`package = "..."`) or a looser version requirement is accepted when cargo proves the lock current and the external set is the same. A feature can switch on code paths of a crate that is already there, but it brings in no new crate. Where features matter for security (the Tauri app in Rocket), the manifest sits under a broader risk glob (`src-tauri/**`) and stays risky by section 1.
- **Build-time code in a diff with no Cargo file.** A `build.rs` deeper than the preset globs reach stays as risky (or not) as it is today when no Cargo file changes. Section 4 only stops such a file from riding along with a cleared Cargo diff.
- **Making today's other risk answers stricter.** A diff that `classify_paths` calls normal stays normal; the check only ever turns `Risky` into `Normal`.

## Anti-goals

- **Outside code merging without a human look.** That covers:
  - a new or removed outside crate, a version change or a changed checksum;
  - a git source or a new registry;
  - `[patch]`, `[replace]` or source replacement;
  - a manifest that asks for something the committed lock doesn't have yet (a stale lock), which cargo would fetch on the next build because the checks run `cargo check` without `--locked`;
  - a path crate, target file or workspace member that leaves the repo or sits in a git submodule.
- **New build-time code merging unseen:** a changed build script at any depth, a `build = "..."` change, a new proc-macro crate, a `links` change or `cargo-features`.
- **A parse error, a missing file, a missing `cargo` or a cargo error turning a diff normal.**

## Approaches

- **A+. Compare the external crate set in `Cargo.lock`, and have cargo prove that lock current (picked).** The lock check is a pure function in `factory-core`. In a clean checkout of the head commit, `cargo locate-project` finds each touched workspace's real root and lock, and one `cargo metadata --locked` proves that the head manifests need no lock change and lists every path crate and target with its file. The manifest rules shrink to a short structural list the lock can't show.
- **A. Lock and manifest parsing only.** No cargo call. A manifest edit can bring in outside code while the committed lock looks the same (`package = "evil"`, a switch to `git = ...`, `libc = "0.3"` against a lock that pins 0.2, a feature that needs an optional crate, `foo.workspace = true` for an unused root entry). The rules must pin every dependency key exactly, so any version or source change re-parks and the list keeps growing with Cargo.
- **C. Run `cargo metadata` at base and head and compare the graphs.** It needs a base checkout, and it is two runs for what the lock already records.

The user first picked A, then picked A+ after the second adviser review found the stale-lock gap. A third review moved cargo from the job worktree into a clean checkout of head, because the builder controls the worktree.

- **Cost:** a checkout plus a second or two of cargo, once per head of a Cargo diff.
- **Exposure:** `cargo metadata` runs `rustc -vV` through the checkout's committed cargo config. The checks stage already runs `cargo check`, build scripts and `cargo metadata --no-deps` (`factory-checks/src/workspace.rs`) on the same code, so the call adds no new exposure.

The user also decided that a new internal crate (a new `[workspace] members` entry inside the repo) counts as safe when the checks give Same. Its code is reviewed in full, and anything outside that it pulls in changes the lock.

## Design

### 1. Which risky paths may be cleared

- **Paths are the scoped ones.** Classification works on `job_changed_paths()` (scoped to the project's `subpath`), as today, because `risk_paths` globs are relative to the project. Files are read at the repo-relative path `subpath/` plus the scoped path, and cargo runs on the `subpath/` folder of the clean checkout (section 3).
- **Only some Cargo files qualify.** A risky path may be cleared only when its file name is `Cargo.toml` or `Cargo.lock` (case-insensitive) and every `risk_paths` glob that matches it is Cargo-only.
- **A Cargo-only glob is one whose last segment is `Cargo.toml` or `Cargo.lock`,** such as `**/Cargo.toml`, `Cargo.lock` or `crates/*/Cargo.toml`.
- **Under a broader glob, the file stays risky.** `src-tauri/Cargo.toml` in Rocket also matches `src-tauri/**`, so it stays risky.
- **Any other risky path in the diff keeps the whole diff risky** and the cargo checks don't run. That covers `build.rs`, `*auth*`, `.cargo/**` and the rest.

### 2. The lock check (factory-core, pure)

This lives in a new module, `crates/factory-core/src/cargo_lock.rs`, and parses with the `toml` crate already in factory-core.

- **External entries:** every `[[package]]` that has a `source` key (`registry+...` or `git+...`). Each is compared as the tuple (name, version, source, checksum), and a missing checksum counts as `None`. Entries with no `source` are workspace or path crates and are ignored.
- **The result is one of:**
  - `Same`;
  - `Changed { added, removed }`, where `added` and `removed` are `name version` strings;
  - `Unknown(reason)`.
- **`Unknown` when:** a text doesn't parse, it has no `[[package]]` array, an entry lacks `name` or `version`, or the lock is format v1. Format v1 is a lock with a `[metadata]` table holding any key that starts with `checksum `.
- **The lock's own `version = N` line** (format 3 or 4) is not compared.
- **A lock missing at base** (a new workspace) gives `Unknown`.

### 3. The cargo check (factory-checks, runs cargo)

- **Cargo runs on the commit that merges, not on the builder's worktree.** The builder controls the job worktree. An untracked or ignored `.cargo/config.toml` there (`[patch.crates-io] foo = { path = "vendor/foo" }`), or a dirty manifest that isn't in the diff, could make `--locked` pass for a lock that is stale on main. So the engine makes a clean, detached checkout of the head tree through `factory_git`, in a temporary folder under the job folder. Cargo runs there, and the folder is removed afterwards, also on error. A checkout that fails gives `Unknown`. The checks stage's `cargo check` no longer matters either: whatever it rewrote in the job worktree is not in the clean checkout.
- **The steps, per touched workspace, in this order:**
  1. **Find the workspace root.** Run `cargo locate-project --workspace --message-format plain --manifest-path <manifest>`. It resolves nothing and needs no network. The root's folder must lie inside the project folder, else `Unknown`. A subfolder project whose workspace root sits above its `subpath` therefore gives `Unknown`, which is accepted. This finds the real root for nested and excluded workspaces, where the nearest lock above a manifest can be the wrong one.
  2. **Lock check** (section 2) on `<root>/Cargo.lock` at base and head. Anything but `Same` stops here, so cargo never downloads a crate the job added.
  3. **Prove the lock current.** Run `cargo metadata --locked --format-version 1 --manifest-path <root manifest>`. From the JSON it returns, for every package whose `source` is null, its `manifest_path` and each target's `kind` and `src_path`.
- **What `--locked` proves:** cargo fails when the head manifests would need any change to the lock. That catches every stale-lock case in the anti-goals without the factory parsing a dependency table. `--locked` never writes the lock.
- **Why the network is allowed:** full metadata needs every package in the resolve on disk, for every platform. `cargo check` on Linux never downloads Windows-only crates (`windows-sys`), so an `--offline` call would fail on most real workspaces and the feature would never fire. Step 2 has already shown that the external set is the one on main, so cargo can only fetch crates main already uses, each verified against its lock checksum. A failed download gives `Unknown`.
- **Which manifests:** the touched workspaces come from every changed `Cargo.toml` present at head, and from the folder of every changed `Cargo.lock`. A `Cargo.toml` must exist in that folder at head, else `Unknown`. The calls are deduplicated by root. Every changed `Cargo.lock` must be the lock of one of these roots, else `Unknown`.
- **Errors:** a start failure (no `cargo` on `PATH`), a non-zero exit or a JSON error is an error. The engine maps it to `Unknown` with cargo's first stderr line.
- **Where the code lives:** the two cargo calls go in a new function next to `Workspace::load` in `crates/factory-checks/src/workspace.rs`. No `--no-deps`, since `--locked` must resolve the whole graph, and no `-j`, since nothing is built.

### 4. The structural check (factory-core pure, fed by section 3)

These are what the lock can't show. Any of them gives `Changed(reason)`:

- **A path that leaves the project or sits in a submodule.** For every package with a null `source` in the head metadata, its `manifest_path` and every target `src_path`, canonicalised, must lie inside the canonicalised project folder of the clean checkout. That covers path dependencies, workspace members, `package.workspace`, `[patch]` to a path, and target `path` keys (`[lib]`, `[[bin]]`, `[[test]]`, `[[example]]`, `[[bench]]`), symlinks included. When `.gitmodules` exists at head, the whole cargo check gives `Unknown` instead.
- **Build-time code.** These give `Changed`:
  - any target of kind `custom-build` whose `src_path` is a file the diff changed (a `build.rs` at any depth, or a custom `build = "..."` file);
  - any package with a `proc-macro` target whose manifest is new in the diff (from the metadata's target kind, so `proc-macro` and `proc_macro` are both caught);
  - a changed manifest that differs between base and head in any of `package.build`, `package.links`, `lib.proc-macro`, `lib.proc_macro` or `cargo-features`.

  A new manifest is compared against an empty one. A new crate with only `[lib]` and `[[bin]]` targets doesn't park.
- **What stays as today:** a file that a build script pulls in with `mod` or `include!`, and an edit to the source of an existing proc-macro crate, are reviewed like any code. They don't park today either.
- **A build-dependency on a crate already in the lock** is accepted. It only runs through a build script, and a changed build script parks.
- **Overrides.** Any difference in `[patch.*]` or `[replace]` in a changed manifest. The lock and the path rule would catch most of these; the rule stays because it is cheap and patches are rare.

Parsed TOML values are compared, so formatting is ignored. A manifest that doesn't parse at base or head gives `Unknown`. Dependency tables, `[[bin]]`, `[features]`, `[package]` metadata, `[profile.*]`, `[lints]` and the rest change freely; sections 2 and 3 cover them.

### 5. Where the engine applies it

- **The new function:** `Engine::diff_class(&self, paths: &[String]) -> DiffClass`, on scoped paths.
  - It reads base files at the job's range start (`range_start(base, head)`, as the review's whole diff) and head files at head.
  - It runs `classify_paths`. When the answer is `Risky`, `cargo_lock_check` is on, and every risky path qualifies under section 1, it runs sections 2 to 4. All `Same` gives `Normal`; a diff with a Cargo file is never docs-only. Anything else stays `Risky`.
  - Section 1 needs to know which globs match a path. factory-core gains a pure helper that gives the matching `risk_paths` globs for one path, taken from the effective config the engine already holds (the project's list when it sets one, else the preset's).
- **A per-path variant** `Engine::path_is_risky(&self, path, verdict)`. Each risky-path list uses it, so a cleared Cargo file is never listed as risky.
- **The verdict is stored once per head and config.** The first caller runs the check and writes the result to `cargo-check.txt` in the job folder, next to `risky-paths.txt`. The result is keyed by the head sha plus a hash of `risk_paths` and `cargo_lock_check`, and written atomically (temporary file, then rename; the last writer wins).
  - **Later callers** with the same key read the stored result, so park cards and the dashboard never run cargo.
  - **A different key** (a new head or a changed config) makes the check run again. It needs only the repo, not the job worktree, because section 3 makes its own checkout.
  - **Which callers can come first:** the review stage, the checks stage's `fix_or_park`, `park_after_sync` (park.rs ~166) and any other park that builds a card.
- **Every whole-diff risk call goes through these.** The list, from the adviser reviews:
  - `review.rs`: the class at ~100, the first-risky file at ~104, the large reviewer model at ~119 (`specs.rs` ~561), `security_skippable` at ~163, `risk_summary` at ~309, and `fix_rounds_for` at ~314;
  - `decide.rs`: `risk_summary` at ~91, which gets the cleared list;
  - `park.rs`: `is_risky` ~318, `risk_part` ~360 (with its own per-path filter for "added by fix rounds") and `record.risky` ~219, which the dashboard reads in `dashboard/src/park.rs` ~50;
  - `lead_parts` ~324, reached from `checks_stage.rs` ~215 (`park_gate`) and ~292;
  - `lead_parts_for_card` ~331, reached from `park_after_sync` (park.rs ~166), `decision_parts` (park.rs ~255) and `nothing_changed_parts` (build.rs ~355);
  - `checks_stage.rs`: `fix_or_park` ~283.

  The plan greps for any caller of `classify_paths` or `DiffClass::Risky` missed here. `merge.rs` and `approve` (merge.rs ~270) do no risk check. `spec_check.rs` stays on `classify_paths`.
- **Which base:** the range start is the merge base with the base branch (checks_stage.rs ~187), as for the review's whole diff. Cargo changes that a sync brings in from main are not part of the job's diff.
- **An unreadable diff.** `is_risky` treats a diff it can't read as not risky, and `lead_parts_for_card` gives a "risk unknown" part; both stay as they are, since the cargo check never runs without paths. Once paths are read, a check that can't settle the answer gives `Risky`.
- **The ledger gets one line per review stage, and only when the check ran:**
  - `cargo lock check: no outside code changed, Cargo files are not risky`;
  - `cargo lock check: outside crates added: signal-hook 0.3.17; removed: none, diff stays risky`;
  - `cargo lock check: <reason>, diff stays risky`.

  The risk part of a park repeats the last two.
- **The class can change mid-job.** A Same verdict in round 1 can become Changed in a fix round, or the other way round. It then works like today's path-based flips: `fix_rounds_for` switches to `risky_fix_rounds` (config.rs ~385), which can park at once in `fix_or_park`. This is accepted.
  - `risky-paths.txt` is written only at the first review that finds the diff risky (review.rs ~103-113), and it uses `path_is_risky`. So when round 1 was cleared and round 2 turns risky, the file is written in round 2 with the Cargo files in it, and the park shows no "added by fix rounds" line. When round 1 was risky and a later round is cleared, the diff is normal and no risk part shows.

### 6. factory-git

- **New helper:** `Repo::file_at(&self, commit: &str, rel: &str) -> Result<Option<String>>`, in `factory-git/src/tip.rs` next to `file_at_branch`. It reuses `entry_at` and `is_regular`. It gives `None` when the file is absent, and an error when the path isn't a regular file or isn't UTF-8. The engine maps any error to `Unknown`.
- **A clean checkout of one commit:** a detached, temporary checkout of a commit's tree into a given folder, plus its removal. If the existing worktree code can't do this, the plan adds a helper. git stays inside `factory_git`.

### 7. Config and presets

- **New setting:** `cargo_lock_check: bool`, default `true`. It goes in `Config`, in the settings block next to `risk_paths`, and in `docs/reference/project-toml-reference.md`. When it is off, Cargo files stay risky by path and cargo is not called. A broken config already makes every diff risky, and the check runs only on a valid one.
- **The rust preset** (`crates/factory-core/presets/rust/config.toml`) gains `**/.cargo/**` (source replacement, registries, `rustc-wrapper`, runners) and `**/rust-toolchain*`. These leave the lock alone, and the Cargo park used to hide them. Its build-script globs (`build.rs`, `*/build.rs`, `*/*/build.rs`) stay as they are; `**/build.rs` would mark `crates/factory-engine/src/build.rs` as risky (`preset.rs` ~633-637). Section 4 covers a deeper build script that rides along with a Cargo diff. The installed copy updates through `factory presets refresh`.
- **Rocket's `~/.factory/projects/rocket/project.toml`** sets its own `risk_paths`, which replace the preset's. It gains the same `.cargo` and toolchain globs. Build scripts there are covered by section 4 when they ride with a Cargo diff, so the build globs are optional; the plan adds `build.rs`, `*/build.rs` and `*/*/build.rs` too, so a build script alone stays risky as in the rust preset. This file is the user's, outside the repo, so the user approves it with this design and the plan edits it as its own step.
- **This repo's `.claude/factory/config.toml`** is Rocket's carried-over copy and a test fixture (CLAUDE.md). It is not changed. rocket-factory's jobs use the rust preset.

### 8. Docs

- `docs/reference/software-factory.md`: the risk rule and the Cargo exception, with the three checks.
- `docs/reference/project-toml-reference.md`: the setting.
- The user guide's risky-diff section.
- The rust preset comment: why `.cargo` and the toolchain files are risky.

## Constraints

- No new external crate. `serde_json` for the metadata JSON is already used by `factory-checks`.
- `factory-core` stays pure. Reads go through `factory_git` in the engine, git goes only through `factory_git`, and cargo runs only from `factory-checks`.
- Board JSON is unchanged.
- Paths reach the driver only through `Layout`.
- cargo in tests and docs: `-j4`, never `--workspace`, never set `CARGO_TARGET_DIR`.

## Tests the plan must include

- **Lock check:**
  - reordering gives `Same`;
  - a new registry crate gives `Changed` with it in `added`;
  - a version bump gives `Changed` with both versions;
  - a changed checksum gives `Changed`;
  - a new git source gives `Changed`;
  - a new path or workspace package gives `Same`;
  - garbage, no `[[package]]`, or a v1 lock with `checksum ` keys under `[metadata]` gives `Unknown`;
  - a format `version` bump alone gives `Same`.
- **Cargo check, on fixture workspaces with only path crates (so no test needs the network):**
  - a current lock succeeds and gives the right `workspace_root`, path packages and targets;
  - a manifest that adds a dependency the lock lacks fails under `--locked`;
  - a nested workspace (a member with its own `[workspace]`) gives its own root;
  - a missing `cargo` on `PATH` gives an error, not a pass.
- **Structural check:**
  - a path package or target `src_path` outside the project, also through a symlink, gives `Changed`;
  - `.gitmodules` at head gives `Unknown`;
  - a changed `crates/a/b/c/build.rs` that is a `custom-build` target gives `Changed`;
  - `build = "src/gen.rs"`, `proc-macro = true`, `proc_macro = true`, `links` or `cargo-features` gives `Changed`;
  - any `[patch.crates-io]` or `[replace]` change gives `Changed`;
  - `[[bin]]`, `[features]`, `[profile]` edits and reformatting give `Same`;
  - a new crate manifest is compared against an empty one.
- **Engine, through the review stage and decide on a real repo:**
  - **Merges with no `risky-diff` park:** a job that adds a path dependency on an internal crate (the lock's internal `dependencies` list changes, the external set doesn't).
  - **Parks `risky-diff`:**
    - a job that adds `evil = "1"` to a manifest without touching the lock (cargo `--locked` fails, ledger line names it);
    - a job whose job worktree holds an untracked `.cargo/config.toml` with a path `[patch]` that makes a stale lock look current (the clean checkout ignores it);
    - a job that adds a registry crate to the lock, with the risk part naming the crate;
    - a job that touches `Cargo.toml` and an `*auth*` path;
    - a job with an unparsable lock at head, with the ledger line naming the reason;
    - a job that touches `.cargo/config.toml` (preset glob);
    - with `cargo_lock_check = false`, a Cargo diff that the check would clear.
  - **A new internal crate** (a new member, a path dependency, its own `Cargo.toml`) with no outside change merges. With a `build.rs` in it, it parks.
  - **Under a broader glob:** in a config with `src-tauri/**` and `**/Cargo.toml`, a change to `src-tauri/Cargo.toml` stays risky.
  - **A subfolder project:** a job with `subpath` set classifies scoped paths, reads `subpath/` files and runs cargo on its own workspace, not the root one.
  - **A mid-job flip:** Same in round 1, then a fix round that adds an outside crate, ends with the risky cap; `risky-paths.txt` is written in that round and the park shows no "added by fix rounds" line.
  - **The stored verdict:** a park card for a stored key reads `cargo-check.txt` and runs no cargo; a changed `risk_paths` makes the check run again; the clean checkout folder is gone after both a pass and a cargo error.
- **Config:** `cargo_lock_check` defaults to true and parses.
- **Presets:** the rust preset classifies `.cargo/config.toml` and `rust-toolchain.toml` as risky, and `crates/factory-engine/src/build.rs` stays normal.

## Left to the builder

- The exact struct and enum shapes, and the format of `cargo-check.txt`.
- Where the shared path helpers live, as long as the pure parts are in factory-core.
- How engine tests keep cargo off the network: fixture workspaces with path crates only, or a pre-filled cache. Tests must not need the network.
