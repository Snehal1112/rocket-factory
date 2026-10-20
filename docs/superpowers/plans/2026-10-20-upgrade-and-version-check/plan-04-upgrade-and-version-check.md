# Upgrade and version check, Plan 4: The upgrade command, the shadow report and the docs

> **Executor:** the factory driver runs this plan using superpowers:subagent-driven-development, with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.

**Goal:** Add `factory upgrade [--dry-run]`, which gates on the version marker, updates the `/factory` command with a `.bak` copy, refreshes unedited presets, reports project and repo agent files that shadow an updated preset file, and writes the marker. Document upgrading in the README.

**Architecture:** `factory-core` gains `preset::shadowed`, `ShadowLine` and `Layer`. It does no git work: the caller passes a `tip_for` closure. `factory-cli` gains `install::commands_folder` and `install::upgrade_into` (the `.bak` step), a shared `presets::report_lines`, and the new module `upgrade.rs` with `upgrade::run`. The closure in `upgrade.rs` wraps an owned `factory_git::Repo` in a small `OwnedTip` that delegates to the engine's `RepoTip`. No `factory-engine` logic changes, so no port is involved. No board, ledger or events field is added.

**Tech Stack:** Rust (clap, git2 in tests, tempfile), Markdown.

**Spec:** docs/superpowers/specs/2026-10-20-upgrade-and-version-check-design.md

## Global Constraints

- Always `-j4`; never `--workspace` or `--all`; never set `CARGO_TARGET_DIR`. Build and test per crate: `cargo check -j4 -p <crate>` and `cargo test -j4 -p <crate> <filter>`.
- Paths reach the driver through `Home` and `Layout` only (`crates/factory-cli/tests/no_hardcoded_paths.rs`). No string literal `".claude`, `".superpowers` or `"main"` in non-test code. The commands folder comes from `factory_core::claude_commands_dir`.
- `factory-core` stays free of I/O-heavy dependencies. `shadowed` does no git work.
- `factory presets refresh` output and the existing `install` messages keep their text.
- `upgrade` never overwrites an edited preset file: it calls `preset::refresh(presets_dir, false)` only, never with `force`.
- Tests use `.expect("...")` and `expect_err`, never the unwrap call.
- Each task lists the tests it adds. This plan touches no `factory-engine` logic (it only uses `factory_engine::RepoTip`), and adds no board, ledger or events field.
- Plans 1 to 3 have merged: `factory_core::version::{FACTORY_VERSION, read_marker, check, stamp}`, `Home::version_file()`, `preset::plan_refresh` and the `presets/.manifest` exist.

## Review Focus

1. A `.bak` that cannot be written (the path `<dir>/factory.md.bak` is a non-empty folder): upgrade returns an error, `factory.md` keeps its old bytes, and no preset file is written. Test `upgrade_stops_when_the_bak_cannot_be_written` (Task 2).
2. `$HOME` unset with no `--dir`: the binary exits 1 with the `install` error `$HOME is not set` and writes nothing into `$FACTORY_HOME` (no `VERSION`, no `presets/`). Test `upgrade_without_home_fails_before_any_change` (Task 2).
3. A registered project whose repo folder was deleted: one `skipped: <project> (<reason>)` line, and a second project is still checked. Test `upgrade_skips_a_project_whose_repo_is_gone` (Task 2), and in core `shadowed_skips_a_project_whose_tip_cannot_be_read` (Task 1).
4. A broken `project.toml`: `Skipped` with the load error as reason, the next project is still checked. Test `shadowed_skips_a_broken_project_and_checks_the_rest` (Task 1).
5. `--dry-run` shadow report uses the files that would be updated, and still writes nothing. Test `upgrade_dry_run_reports_shadowing_from_the_files_it_would_update` (Task 2).
6. Only agent files are checked, and only for projects on the same preset: an updated `config.toml` or a file of another preset gives no line and does not even open the project's tip. Tests `shadowed_ignores_non_agent_files_and_other_presets` and `shadowed_returns_nothing_when_no_agent_file_was_updated` (Task 1).
7. The refactor of `presets.rs` must not change `factory presets refresh` output by one byte. Golden test `the_refresh_output_is_byte_identical` (Task 2, written and passing before the refactor).
8. The upgrade's refused hint replaces, not adds to, the refresh hint: the upgrade output never holds `use --force to overwrite them`. Asserted in `upgrade_refuses_edited_presets_and_says_how_to_force` (Task 2).
9. `install` without `--force` on a differing file still refuses with its old text, and `install --force` still writes no `.bak`. Test `install_force_still_writes_no_bak` (Task 1); the old text stays pinned by `install_refuses_an_edited_file_unless_forced`.

---

## File Structure

- `crates/factory-core/src/preset.rs` (modify): `Layer`, `ShadowLine`, `shadowed`, unit tests.
- `crates/factory-core/src/resolve.rs` (modify): `AGENTS_DIR` becomes `pub(crate)` so `shadowed` lists the same folder `resolve` uses.
- `crates/factory-cli/src/install.rs` (modify): `commands_folder`, `upgrade_into`.
- `crates/factory-cli/src/presets.rs` (modify): `report_lines` shared by refresh and upgrade.
- `crates/factory-cli/src/upgrade.rs` (new): `run`, `OwnedTip`, the `tip_for` closure.
- `crates/factory-cli/src/cli.rs`, `lib.rs`, `main.rs`, `commands.rs` (modify): the `Upgrade { dry_run }` command and its wiring.
- `crates/factory-cli/tests/global_command.rs`, `presets_refresh.rs` (modify), `upgrade.rs` (new).
- `README.md`, `docs/getting-started.md`, `crates/factory-cli/tests/agent_lint.rs` (modify).

### Task 1: The shadow report and the `.bak` helper

**Files:**
- Modify: `crates/factory-core/src/preset.rs`, `crates/factory-core/src/resolve.rs`, `crates/factory-cli/src/install.rs`, `crates/factory-cli/tests/global_command.rs`

**Interfaces:**
- Consumes: `factory_core::resolve::BaseTipFiles` (`read_file`, `list_dir`); `factory_core::project::{list, ProjectEntry, ProjectFile}`; `Home::{presets_dir, project_dir}`; `factory_core::{write_atomic, claude_commands_dir}`; `install::GLOBAL_COMMAND`.
- Produces:
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Layer { Project, Repo }` with `pub fn label(self) -> &'static str` giving `project-layer` and `repo-layer`.
  - `#[derive(Debug, Clone, PartialEq, Eq)] pub enum ShadowLine { Shadow { project: String, layer: Layer, file: String }, Skipped { project: String, reason: String } }`.
  - `pub fn shadowed(home: &Home, updated: &[PathBuf], tip_for: &dyn Fn(&str) -> Result<Box<dyn BaseTipFiles>, String>) -> Vec<ShadowLine>` in `factory_core::preset`. The argument to `tip_for` is the project name.
  - `pub fn commands_folder(dir: Option<&Path>) -> Result<PathBuf, CliError>` in `factory_cli::install` (the folder choice now inside `run`, same error text).
  - `pub fn upgrade_into(folder: &Path, dry_run: bool) -> Result<String, CliError>` in `factory_cli::install`.

Rules for `shadowed`, from spec design 6:
- An updated path counts only when it is `home.presets_dir()/<preset>/agents/<file>`; any other path is ignored. When no updated path counts, return an empty list without reading a project or calling `tip_for`.
- Projects come from `project::list(home)` (sorted by name). An `Err` entry gives `Skipped { project, reason: <the load error> }`. If `project::list` itself fails, return one `Skipped { project: "*", reason }`.
- A loaded project with no counting file whose `<preset>` equals `ProjectFile.preset` gives nothing, and `tip_for` is not called for it.
- Otherwise call `tip_for(name)` once; an `Err(why)` gives `Skipped { project, reason: why }`. Then `tip.list_dir(AGENTS_DIR)` once; an `Err(why)` gives `Skipped { project, reason: format!("cannot read .factory/agents from the base branch: {why}") }`.
- For each counting file in `updated` order: `Shadow { Project }` when `home.project_dir(name).join("agents").join(file).is_file()`, then `Shadow { Repo }` when the listing holds an entry with that name.

Rules for `upgrade_into`, from spec design 5 step 1, with `target = folder.join("factory.md")` and `bak = folder.join("factory.md.bak")`:

| Existing target | Line | Writes (not with `dry_run`) |
|---|---|---|
| absent | `installed <target>` | the command |
| equal to `GLOBAL_COMMAND` | `already installed: <target>` | nothing |
| different | `updated <target> (old copy saved as <bak>)` | `write_atomic(bak, old bytes)`, then the command; a failed `.bak` write returns `CliError::Refused("cannot save <bak>: <why>")` before the command is touched |
| unreadable | the `install` error text `cannot read <target>: <why>` | nothing |

With `dry_run` a line that names a change (`installed`, `updated`) gets the prefix `would `; nothing is written.

- [ ] **Step 1: Read** spec sections 5 and 6, `crates/factory-core/src/preset.rs` (`RefreshReport`, the unit tests and their `tempfile` use), `resolve.rs` (`BaseTipFiles`, `AGENTS_DIR`, `merged_agents` and its test `Tip`), `project.rs` (`list`, `ProjectEntry`, `ProjectFile`, `PROJECT_FILE`), `crates/factory-cli/src/install.rs`, and the helpers `install_args`, `make_repo`, `s` and the install tests in `crates/factory-cli/tests/global_command.rs`.
- [ ] **Step 2: Write the failing tests.** In the `preset.rs` test module add two fakes and a helper:

```rust
struct ListTip(Vec<(String, String)>);
impl BaseTipFiles for ListTip {
    fn read_file(&self, _rel: &str) -> Result<Option<String>, String> { Ok(None) }
    fn list_dir(&self, rel: &str) -> Result<Vec<(String, String)>, String> {
        assert_eq!(rel, ".factory/agents");
        Ok(self.0.clone())
    }
}
struct FailingList;
impl BaseTipFiles for FailingList {
    fn read_file(&self, _rel: &str) -> Result<Option<String>, String> { Ok(None) }
    fn list_dir(&self, _rel: &str) -> Result<Vec<(String, String)>, String> { Err("bad tree".into()) }
}
/// Writes projects/<name>/project.toml by hand, with an absolute repo path.
fn register(home: &Home, name: &str, preset: &str) {
    let dir = home.project_dir(name);
    std::fs::create_dir_all(&dir).expect("mkdir");
    std::fs::write(
        dir.join("project.toml"),
        format!("repo = \"/nowhere/{name}\"\npreset = \"{preset}\"\n\n[settings]\nbase_branch = \"main\"\n"),
    )
    .expect("write project.toml");
}
```

Full test:

```rust
#[test]
fn shadowed_finds_project_and_repo_layer_files() {
    let tmp = tempfile::tempdir().expect("tmp");
    let home = Home::at(tmp.path().to_path_buf());
    register(&home, "alpha", "rust");
    let layer = home.project_dir("alpha").join("agents/factory-builder.md");
    std::fs::create_dir_all(layer.parent().expect("parent")).expect("mkdir");
    std::fs::write(&layer, "# project override\n").expect("write");
    let updated = vec![
        home.presets_dir().join("rust/agents/factory-builder.md"),
        home.presets_dir().join("rust/agents/factory-committer.md"),
    ];
    let tip_for = |name: &str| -> Result<Box<dyn BaseTipFiles>, String> {
        assert_eq!(name, "alpha");
        Ok(Box::new(ListTip(vec![
            ("factory-builder.md".into(), "# repo\n".into()),
            ("factory-kb-refresher.md".into(), "# repo\n".into()),
        ])))
    };
    let lines = shadowed(&home, &updated, &tip_for);
    assert_eq!(
        lines,
        vec![
            ShadowLine::Shadow { project: "alpha".into(), layer: Layer::Project, file: "factory-builder.md".into() },
            ShadowLine::Shadow { project: "alpha".into(), layer: Layer::Repo, file: "factory-builder.md".into() },
        ]
    );
    assert_eq!(Layer::Project.label(), "project-layer");
    assert_eq!(Layer::Repo.label(), "repo-layer");
}
```

Same shape, by name and inputs:

| Test | Setup and `updated` | `tip_for` | Expected |
|---|---|---|---|
| `shadowed_returns_nothing_when_no_agent_file_was_updated` | `alpha` (rust); `[rust/config.toml, rust/commit-context.md]` | panics with `"must not be called"` | `vec![]` |
| `shadowed_ignores_non_agent_files_and_other_presets` | `alpha` (go) with a project-layer `factory-builder.md`; `[rust/agents/factory-builder.md, rust/config.toml]` | panics with `"must not be called"` | `vec![]` |
| `shadowed_skips_a_broken_project_and_checks_the_rest` | `bad` whose `project.toml` is `"not toml ["`, `good` (rust); `[rust/agents/factory-builder.md]` | `ListTip` with `factory-builder.md` | `[Skipped { project: "bad", reason }, Shadow { "good", Repo, "factory-builder.md" }]`, `reason` contains `project.toml` |
| `shadowed_skips_a_project_whose_tip_cannot_be_read` | `alpha` (rust), `beta` (rust); `[rust/agents/factory-builder.md]` | `Err("not a git repository: gone")` for `alpha`, `ListTip` with `factory-builder.md` for `beta` | `[Skipped { "alpha", "not a git repository: gone" }, Shadow { "beta", Repo, "factory-builder.md" }]` |
| `shadowed_skips_when_the_tip_listing_fails` | `alpha` (rust); `[rust/agents/factory-builder.md]` | `Ok(Box::new(FailingList))` | one `Skipped { "alpha", reason }`, `reason` contains `bad tree` and `.factory/agents` |
| `shadowed_reports_nothing_without_overrides` | `alpha` (rust), no project-layer file; `[rust/agents/factory-builder.md]` | `ListTip(vec![])` | `vec![]` |

(Paths in the table are under `home.presets_dir()`.)

In `crates/factory-cli/tests/global_command.rs` add, using the existing helpers and `tempfile`:

```rust
#[test]
fn upgrade_into_replaces_a_different_command_and_keeps_a_bak() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().join("commands");
    std::fs::create_dir_all(&dir).expect("mkdir");
    let target = dir.join("factory.md");
    std::fs::write(&target, "# old\n").expect("write");
    let bak = dir.join("factory.md.bak");
    std::fs::write(&bak, "# older bak\n").expect("write bak");
    let line = install::upgrade_into(&dir, false).expect("upgrade");
    assert_eq!(
        line,
        format!("updated {} (old copy saved as {})", target.display(), bak.display())
    );
    assert_eq!(std::fs::read_to_string(&bak).expect("read bak"), "# old\n");
    assert_eq!(std::fs::read_to_string(&target).expect("read"), GLOBAL_COMMAND);
}
```

| Test | Setup | Call | Expected |
|---|---|---|---|
| `upgrade_into_installs_an_absent_command` | empty `dir` | `upgrade_into(&dir, false)` | `installed <target>`; file equals `GLOBAL_COMMAND`; no `.bak` |
| `upgrade_into_leaves_an_equal_command` | `target` holds `GLOBAL_COMMAND` | `upgrade_into(&dir, false)` | `already installed: <target>`; no `.bak` |
| `upgrade_into_dry_run_writes_nothing` | `target` holds `"# old\n"` | `upgrade_into(&dir, true)` | starts with `would updated `; `target` still `"# old\n"`; `bak` absent. Second case: empty `dir` gives `would installed <target>` and no file |
| `upgrade_into_stops_when_the_bak_cannot_be_written` | `target` holds `"# old\n"`; `factory.md.bak/keep` is a file, so `.bak` is a non-empty folder | `upgrade_into(&dir, false)` | `expect_err` text contains `cannot save`; `target` still `"# old\n"` |
| `install_force_still_writes_no_bak` | `target` holds `"# old\n"` | `install_args(&["--dir", s(&dir), "--force"])` | `installed <target>`; `factory.md.bak` absent |

- [ ] **Step 3: Run** `cargo test -j4 -p factory-core shadowed` and expect FAIL with `cannot find function 'shadowed'` and `cannot find type 'ShadowLine'`. Run `cargo test -j4 -p factory-cli --test global_command upgrade_into` and expect FAIL with `cannot find function 'upgrade_into' in module 'install'`.
- [ ] **Step 4: Implement.** Make `AGENTS_DIR` in `resolve.rs` `pub(crate)` and use it in `shadowed`. Add `Layer`, `ShadowLine`, `Layer::label` and `shadowed` to `preset.rs` with a doc comment each, following the rules above. In `install.rs`, move the folder choice of `run` into `commands_folder` (same `$HOME is not set, pass --dir` text) and have `run` call it; add `upgrade_into` following the table. Keep `install_into` and its messages as they are.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-core preset`, `cargo test -j4 -p factory-cli --test global_command` and `cargo test -j4 -p factory-cli --test no_hardcoded_paths`. Expect PASS, including the existing `install_refuses_an_edited_file_unless_forced`, `install_without_home_and_dir_is_an_error` and `install_defaults_to_the_claude_commands_folder_under_home`.

### Task 2: The `factory upgrade` command

**Files:**
- Create: `crates/factory-cli/src/upgrade.rs`, `crates/factory-cli/tests/upgrade.rs`
- Modify: `crates/factory-cli/src/cli.rs`, `lib.rs`, `main.rs`, `commands.rs`, `presets.rs`, `crates/factory-cli/tests/presets_refresh.rs`

**Interfaces:**
- Consumes: `factory_core::version::{FACTORY_VERSION, read_marker, check, stamp}` and `Home::version_file()` (plan 1); `factory_core::preset::{plan_refresh, refresh, RefreshReport, seed}` (plan 3); `preset::{shadowed, ShadowLine, Layer}` and `install::{commands_folder, upgrade_into}` (Task 1); `factory_engine::RepoTip`; `factory_git::Repo`; `crate::project::{base_of_strict, open_repo}`.
- Produces:
  - `Command::Upgrade { #[arg(long)] dry_run: bool }` in `cli.rs`, doc comment `/// Update the /factory command and the unedited presets, report overrides that shadow them, and write the version marker.`
  - `pub(crate) fn report_lines(report: &RefreshReport, dry_run: bool) -> Vec<String>` in `presets.rs`: the outcome lines and the count line that `refresh` prints today; with `dry_run`, `written:` and `updated:` lines get the prefix `would `.
  - `pub fn run(home: &Home, commands_dir: &Path, dry_run: bool) -> Result<String, CliError>` in `factory_cli::upgrade`; `pub mod upgrade;` in `lib.rs`.
  - `struct OwnedTip(Repo)` implementing `BaseTipFiles` by delegating to `RepoTip(&self.0)`, and `fn tip_for(home: &Home) -> impl Fn(&str) -> Result<Box<dyn BaseTipFiles>, String> + '_` (private). The closure reads `project::get(home, name)`, takes `base_of_strict` (`None` gives `Err("no base branch in project.toml")`) and opens the repo with `open_repo`.

Order inside `run`, each step's lines in turn, joined with `\n`:
1. `version::check(home, FACTORY_VERSION)?`; a newer or unreadable marker stops before any line.
2. `install::upgrade_into(commands_dir, dry_run)?`.
3. `preset::plan_refresh(&home.presets_dir())` with `dry_run`, else `preset::refresh(&home.presets_dir(), false)`; `report_lines(&report, dry_run)`; when `report.refused` is not empty, then `refused files stay as they are: factory presets refresh --force overwrites them`.
4. `preset::shadowed(home, &report.updated, &tip_for(home))`, printed as `shadowed: <project> <layer.label()> <file>` and `skipped: <project> (<reason>)`.
5. Marker. Without `dry_run`: `stamp(home, FACTORY_VERSION)?` gives `version marker: written <v>` when it wrote, else `version marker: <v>`. With `dry_run`: `read_marker(home)?` is `None` or a text other than `FACTORY_VERSION` (after trim) gives `would version marker: written <v>`, else `version marker: <v>`.

`main.rs`: a new branch before the `--project` handling, like the `Presets` one: `Home::from_env()`, then `install::commands_folder(None)`, then `upgrade::run`, print the text or `factory: <e>` and exit 1. Both lookups happen before `run`, so an unset `$HOME` changes nothing. `commands.rs` `dispatch` gets the arm `Command::Upgrade { .. } => Err(CliError::Refused("upgrade needs no repository, run it through upgrade::run".into()))`.

- [ ] **Step 1: Read** spec sections 1 (gate and stamp), 5 and 6 and the edge-case table, `crates/factory-cli/src/presets.rs`, `main.rs`, `commands.rs` (`dispatch`), `project.rs` (`base_of_strict`, `open_repo`), `crates/factory-engine/src/tipfiles.rs`, `factory_core::version` from plan 1, `preset::plan_refresh` from plan 3, and the helpers in `crates/factory-cli/tests/presets_refresh.rs`, `project_cmd.rs` (`make_repo`, `run`, `s`) and `tests/common/mod.rs` (`commit_file`).
- [ ] **Step 2: Write the golden test first** in `presets_refresh.rs`, and run it before touching `presets.rs`:

```rust
#[test]
fn the_refresh_output_is_byte_identical() {
    let tmp = tempfile::tempdir().expect("tmp");
    let home = Home::at(tmp.path().join("home"));
    preset::seed(&home.presets_dir()).expect("seed");
    let p = home.presets_dir();
    std::fs::write(p.join("rust/config.toml"), "# mine\n").expect("edit");
    std::fs::remove_file(p.join("go/commit-context.md")).expect("rm");
    let agents = ["factory-builder.md", "factory-code-reviewer.md", "factory-committer.md", "factory-kb-refresher.md"];
    let mut expected = vec![format!("written: {}", p.join("go/commit-context.md").display())];
    expected.push(format!("unchanged: {}", p.join("rust/commit-context.md").display()));
    for a in agents { expected.push(format!("unchanged: {}", p.join("rust/agents").join(a).display())); }
    expected.push(format!("unchanged: {}", p.join("go/config.toml").display()));
    for a in agents { expected.push(format!("unchanged: {}", p.join("go/agents").join(a).display())); }
    expected.push(format!("refused: {}", p.join("rust/config.toml").display()));
    expected.push("1 written, 0 updated, 10 unchanged, 1 refused".to_string());
    expected.push("refused files differ from every known text, use --force to overwrite them".to_string());
    assert_eq!(run(&home, &["refresh"]), expected.join("\n"));
}
```

Run `cargo test -j4 -p factory-cli --test presets_refresh` and expect PASS on the current code.
- [ ] **Step 3: Write the failing tests.** In `cli.rs` tests add `upgrade_parses_dry_run`: `["factory", "upgrade", "--dry-run"]` matches `Command::Upgrade { dry_run: true }` and `["factory", "upgrade"]` matches `Command::Upgrade { dry_run: false }`. Create `crates/factory-cli/tests/upgrade.rs` with `mod common;`, a `make_repo(parent, folder)` copied from `project_cmd.rs` on branch `main`, a `project_run(home, args)` copied from `global_command.rs`, `s(p)`, and:

```rust
const LEGACY_BUILDER: &str =
    include_str!("../../factory-core/presets/legacy/rust-factory-builder.md");

struct Env { tmp: tempfile::TempDir, home: Home, commands: PathBuf }

fn env() -> Env {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = Home::at(tmp.path().join("home"));
    let commands = tmp.path().join("commands");
    Env { tmp, home, commands }
}

/// Registers `demo` (rust) on a repo that commits a repo-layer builder, and adds a project-layer builder.
fn demo_with_overrides(e: &Env) -> PathBuf {
    let repo = make_repo(e.tmp.path(), "demo-repo");
    common::commit_file(&repo, ".factory/agents/factory-builder.md", "# repo override\n");
    project_run(&e.home, &["add", s(&repo), "--preset", "rust", "--name", "demo", "--base", "main"]).expect("add");
    let layer = e.home.project_dir("demo").join("agents/factory-builder.md");
    std::fs::create_dir_all(layer.parent().expect("parent")).expect("mkdir");
    std::fs::write(&layer, "# project override\n").expect("write");
    repo
}

fn builder(e: &Env) -> PathBuf { e.home.presets_dir().join("rust/agents/factory-builder.md") }
```

Full test:

```rust
#[test]
fn upgrade_reports_shadowing_overrides() {
    let e = env();
    demo_with_overrides(&e);
    std::fs::write(builder(&e), LEGACY_BUILDER).expect("old text");
    let out = upgrade::run(&e.home, &e.commands, false).expect("upgrade");
    assert!(out.contains(&format!("updated: {}", builder(&e).display())), "{out}");
    assert!(out.contains("shadowed: demo project-layer factory-builder.md"), "{out}");
    assert!(out.contains("shadowed: demo repo-layer factory-builder.md"), "{out}");
    assert_eq!(
        out.lines().last().expect("last"),
        format!("version marker: written {FACTORY_VERSION}")
    );
    assert_eq!(
        std::fs::read_to_string(e.home.project_dir("demo").join("agents/factory-builder.md")).expect("read"),
        "# project override\n"
    );
}
```

Same shape, by name and inputs:

| Test | Setup | Call | Assertions |
|---|---|---|---|
| `upgrade_updates_the_command_and_keeps_a_bak` | `commands/factory.md` holds `"# old\n"`; empty home | `run(.., false)` | first line `updated <cmd> (old copy saved as <cmd>.bak)`; `.bak` is `"# old\n"`; `factory.md` equals `GLOBAL_COMMAND`; contains `12 written, 0 updated, 0 unchanged, 0 refused`; `read_marker(&home)` is `Some(FACTORY_VERSION)` |
| `upgrade_dry_run_changes_nothing` | as above, plus `preset::seed` and `LEGACY_BUILDER` in `builder(&e)`; record the bytes of `presets/.manifest` (`std::fs::read(..).ok()`) | `run(.., true)` | first line starts `would updated `; contains `would updated: <builder>`; last line `would version marker: written <v>`; `factory.md` still `"# old\n"`; no `.bak`; builder still `LEGACY_BUILDER`; manifest bytes unchanged; `home.version_file()` absent |
| `upgrade_refuses_edited_presets_and_says_how_to_force` | `preset::seed`; `rust/config.toml` holds `"# mine\n"` | `run(.., false)` | contains `refused: <config>` and `refused files stay as they are: factory presets refresh --force overwrites them`; does not contain `use --force to overwrite them`; config still `"# mine\n"` |
| `upgrade_stops_on_a_newer_marker` | `home.version_file()` holds `"999.0.0\n"` (create the home folder) | `run(.., false)` | `expect_err` text contains `999.0.0` and `FACTORY_VERSION`; `commands/factory.md` absent; `presets_dir()` absent; marker still `"999.0.0\n"` |
| `a_second_upgrade_changes_nothing` | `demo_with_overrides`; `LEGACY_BUILDER` in `builder(&e)`; one `run(.., false)` | second `run(.., false)` | first line `already installed: <cmd>`; contains `0 written, 0 updated, 12 unchanged, 0 refused`; no `shadowed:` and no `would `; last line `version marker: <v>` |
| `upgrade_stops_when_the_bak_cannot_be_written` | `commands/factory.md` holds `"# old\n"`; `commands/factory.md.bak/keep` is a file | `run(.., false)` | `expect_err` text contains `cannot save`; `factory.md` still `"# old\n"`; `presets_dir()` absent; `version_file()` absent |
| `upgrade_dry_run_reports_shadowing_from_the_files_it_would_update` | `demo_with_overrides`; `LEGACY_BUILDER` in `builder(&e)` | `run(.., true)` | contains both `shadowed: demo ...` lines; builder still `LEGACY_BUILDER` |
| `upgrade_skips_a_project_whose_repo_is_gone` | `demo_with_overrides`; a second project `other` (rust) on `other-repo` that commits the same repo-layer file; `LEGACY_BUILDER` in `builder(&e)`; then `remove_dir_all(demo-repo)` | `run(.., false)` | exactly one line starts `skipped: demo (`; no line starts `shadowed: demo `; contains `shadowed: other repo-layer factory-builder.md` |
| `upgrade_without_home_fails_before_any_change` | `std::process::Command::new(env!("CARGO_BIN_EXE_factory")).arg("upgrade").env_remove("HOME").env("FACTORY_HOME", home dir)` | output | exit code 1; stderr contains `$HOME is not set`; `version_file()` and `presets_dir()` absent |

- [ ] **Step 4: Run** `cargo test -j4 -p factory-cli --test upgrade` and expect FAIL with `could not find 'upgrade' in 'factory_cli'`; run `cargo test -j4 -p factory-cli upgrade_parses_dry_run` and expect FAIL with `no variant named 'Upgrade'`.
- [ ] **Step 5: Implement** `Command::Upgrade`, the `dispatch` arm, the `main.rs` branch, `report_lines` (and make `presets::refresh` call it, then append its own hint), `OwnedTip`, `tip_for` and `upgrade::run` from the signatures and the order above. Each new item gets a short doc comment.
- [ ] **Step 6: Run** `cargo test -j4 -p factory-cli --test upgrade`, `cargo test -j4 -p factory-cli --test presets_refresh`, `cargo test -j4 -p factory-cli --test no_hardcoded_paths`, `cargo test -j4 -p factory-cli --lib cli` and `cargo clippy -j4 -p factory-cli --all-targets -- -D warnings`. Expect PASS, with `the_refresh_output_is_byte_identical` and `the_command_reports_each_outcome_and_refused_paths` unchanged.

### Task 3: README "Upgrading" section and doc test

**Files:**
- Modify: `README.md`, `docs/getting-started.md`, `crates/factory-cli/tests/agent_lint.rs`

**Interfaces:**
- Consumes: `read`, `require_all`, `forbid_all` and `TestResult` in `agent_lint.rs`; the command output from Task 2.
- Produces: tests `readme_documents_upgrading` and `getting_started_upgrades_with_one_command` in `agent_lint.rs`.

- [ ] **Step 1: Read** the README sections `### Refreshing seeded presets`, `### The global /factory command`, `### factoryd`, `#### The dashboard across projects` and `### Layers`, section 15 of `docs/getting-started.md`, spec sections 1, 5 and 6 and the acceptance criteria, and the doc tests `readme_points_to_builder_permissions` and `software_factory_doc_describes_builder_permissions` in `agent_lint.rs` for the shape.
- [ ] **Step 2: Write the failing tests:**

```rust
#[test]
fn readme_documents_upgrading() -> TestResult {
    let doc = read("README.md")?;
    require_all(
        &doc,
        "README.md",
        &[
            "### Upgrading",
            "factory upgrade --dry-run",
            "no running jobs",
            "factory.md.bak",
            "refused files stay as they are",
            "factory presets refresh --force",
            "shadowed: <project> <project-layer|repo-layer> <file>",
            "do not follow preset changes",
            "factoryd stop",
            "cargo build -j4 --manifest-path dashboard/Cargo.toml",
            "<home>/VERSION",
            "version: ",
        ],
    )?;
    forbid_all(&doc, "README.md", &["After an upgrade also run `factory install --force`"])
}

#[test]
fn getting_started_upgrades_with_one_command() -> TestResult {
    let doc = read("docs/getting-started.md")?;
    require_all(&doc, "getting-started.md", &["factory upgrade", "factoryd stop"])?;
    forbid_all(&doc, "getting-started.md", &["do all four steps"])
}
```

- [ ] **Step 3: Run** `cargo test -j4 -p factory-cli --test agent_lint upgrad` and expect FAIL listing the missing phrases (`### Upgrading` and the rest) and the forbidden phrases found.
- [ ] **Step 4: Write the docs.** README: a `### Upgrading` section under `## Projects and presets`, before `### Refreshing seeded presets`, in short plain prose: the steps (pull, `cargo build -j4 -p factory-cli -p factory-supervisor`, upgrade with no running jobs, `factory upgrade --dry-run` to preview, `factory upgrade`, `factoryd stop` then start `factoryd run` again, and rebuild the dashboard with `cargo build -j4 --manifest-path dashboard/Cargo.toml` because `factory upgrade` does not touch it); what `factory upgrade` prints in order (command line with `factory.md.bak`, preset lines and the `refused files stay as they are` line, `shadowed:` and `skipped:` lines, the marker line); that project-layer and repo-layer agent files do not follow preset changes, so a `shadowed:` line means the override still holds the old text and needs a hand merge; the `<home>/VERSION` marker, the newer-marker error and that `--version` and `install` still work; that `factory project show` prints `version: ` and `factoryd status` ends with `factoryd <version>` and a `warning:` line on a mismatch. Replace the last paragraph of `### Refreshing seeded presets` with one that points to `factory upgrade`. `docs/getting-started.md` section 15: replace the four manual steps with `factory upgrade` (after the two builds) and the `factoryd stop` restart, and keep the note on refused files. `docs/software-factory.md` names none of these commands, so it stays as it is.
- [ ] **Step 5: Run** `cargo test -j4 -p factory-cli --test agent_lint` and `cargo test -j4 -p factory-cli --test skill_drift`. Expect PASS.

Next plan: none
