//! The presets that ship with the factory. `seed` copies them into the factory home.

use std::path::{Path, PathBuf};

use crate::resolve::{BaseTipFiles, AGENTS_DIR};
use crate::{project, write_atomic, CoreError, Home};

mod manifest;
pub use manifest::MANIFEST_FILE;
use manifest::{sha256_hex, Manifest};

pub const PRESET_NAMES: [&str; 5] = ["rust", "go", "ruby", "csharp", "node"];

/// One preset, embedded in the binary.
pub struct Preset {
    pub name: &'static str,
    pub config: &'static str,
    pub commit_context: &'static str,
    pub agents: &'static [(&'static str, &'static str)],
}

static RUST: Preset = Preset {
    name: "rust",
    config: include_str!("../presets/rust/config.toml"),
    commit_context: include_str!("../presets/rust/commit-context.md"),
    agents: &[
        (
            "factory-builder.md",
            include_str!("../presets/rust/agents/factory-builder.md"),
        ),
        (
            "factory-code-reviewer.md",
            include_str!("../presets/rust/agents/factory-code-reviewer.md"),
        ),
        (
            "factory-committer.md",
            include_str!("../presets/rust/agents/factory-committer.md"),
        ),
        (
            "factory-kb-refresher.md",
            include_str!("../presets/rust/agents/factory-kb-refresher.md"),
        ),
        (
            "factory-spec-reviewer.md",
            include_str!("../presets/rust/agents/factory-spec-reviewer.md"),
        ),
    ],
};

static GO: Preset = Preset {
    name: "go",
    config: include_str!("../presets/go/config.toml"),
    commit_context: include_str!("../presets/go/commit-context.md"),
    agents: &[
        (
            "factory-builder.md",
            include_str!("../presets/go/agents/factory-builder.md"),
        ),
        (
            "factory-code-reviewer.md",
            include_str!("../presets/go/agents/factory-code-reviewer.md"),
        ),
        (
            "factory-committer.md",
            include_str!("../presets/go/agents/factory-committer.md"),
        ),
        (
            "factory-kb-refresher.md",
            include_str!("../presets/go/agents/factory-kb-refresher.md"),
        ),
        (
            "factory-spec-reviewer.md",
            include_str!("../presets/go/agents/factory-spec-reviewer.md"),
        ),
    ],
};

static RUBY: Preset = Preset {
    name: "ruby",
    config: include_str!("../presets/ruby/config.toml"),
    commit_context: include_str!("../presets/ruby/commit-context.md"),
    agents: &[
        (
            "factory-builder.md",
            include_str!("../presets/ruby/agents/factory-builder.md"),
        ),
        (
            "factory-code-reviewer.md",
            include_str!("../presets/ruby/agents/factory-code-reviewer.md"),
        ),
        (
            "factory-committer.md",
            include_str!("../presets/ruby/agents/factory-committer.md"),
        ),
        (
            "factory-kb-refresher.md",
            include_str!("../presets/ruby/agents/factory-kb-refresher.md"),
        ),
        (
            "factory-spec-reviewer.md",
            include_str!("../presets/ruby/agents/factory-spec-reviewer.md"),
        ),
    ],
};

static CSHARP: Preset = Preset {
    name: "csharp",
    config: include_str!("../presets/csharp/config.toml"),
    commit_context: include_str!("../presets/csharp/commit-context.md"),
    agents: &[
        (
            "factory-builder.md",
            include_str!("../presets/csharp/agents/factory-builder.md"),
        ),
        (
            "factory-code-reviewer.md",
            include_str!("../presets/csharp/agents/factory-code-reviewer.md"),
        ),
        (
            "factory-committer.md",
            include_str!("../presets/csharp/agents/factory-committer.md"),
        ),
        (
            "factory-kb-refresher.md",
            include_str!("../presets/csharp/agents/factory-kb-refresher.md"),
        ),
        (
            "factory-spec-reviewer.md",
            include_str!("../presets/csharp/agents/factory-spec-reviewer.md"),
        ),
    ],
};

static NODE: Preset = Preset {
    name: "node",
    config: include_str!("../presets/node/config.toml"),
    commit_context: include_str!("../presets/node/commit-context.md"),
    agents: &[
        (
            "factory-builder.md",
            include_str!("../presets/node/agents/factory-builder.md"),
        ),
        (
            "factory-code-reviewer.md",
            include_str!("../presets/node/agents/factory-code-reviewer.md"),
        ),
        (
            "factory-committer.md",
            include_str!("../presets/node/agents/factory-committer.md"),
        ),
        (
            "factory-kb-refresher.md",
            include_str!("../presets/node/agents/factory-kb-refresher.md"),
        ),
        (
            "factory-spec-reviewer.md",
            include_str!("../presets/node/agents/factory-spec-reviewer.md"),
        ),
    ],
};

pub fn embedded(name: &str) -> Option<&'static Preset> {
    match name {
        "rust" => Some(&RUST),
        "go" => Some(&GO),
        "ruby" => Some(&RUBY),
        "csharp" => Some(&CSHARP),
        "node" => Some(&NODE),
        _ => None,
    }
}

/// The agent texts that earlier versions seeded: (preset, file, text).
///
/// The manifest comes first. These are the fallback for files without a manifest entry, and
/// `refresh` replaces them without `--force`.
static LEGACY: [(&str, &str, &str); 4] = [
    (
        "rust",
        "factory-builder.md",
        include_str!("../presets/legacy/rust-factory-builder.md"),
    ),
    (
        "rust",
        "factory-code-reviewer.md",
        include_str!("../presets/legacy/rust-factory-code-reviewer.md"),
    ),
    (
        "go",
        "factory-builder.md",
        include_str!("../presets/legacy/go-factory-builder.md"),
    ),
    (
        "go",
        "factory-code-reviewer.md",
        include_str!("../presets/legacy/go-factory-code-reviewer.md"),
    ),
];

/// One embedded preset file: path, embedded text, legacy agent key, manifest key.
type PresetFile<'a> = (PathBuf, &'a str, Option<&'a str>, String);

/// Every file of a preset, with the `/`-separated path under `presets_dir` as the manifest key.
fn files_of<'a>(presets_dir: &Path, p: &'a Preset) -> Vec<PresetFile<'a>> {
    let base = presets_dir.join(p.name);
    let mut files = vec![
        (
            base.join("config.toml"),
            p.config,
            None,
            format!("{}/config.toml", p.name),
        ),
        (
            base.join("commit-context.md"),
            p.commit_context,
            None,
            format!("{}/commit-context.md", p.name),
        ),
    ];
    for (file, text) in p.agents {
        files.push((
            base.join("agents").join(file),
            *text,
            Some(*file),
            format!("{}/agents/{file}", p.name),
        ));
    }
    files
}

fn read_existing(path: &Path) -> Result<Option<Vec<u8>>, CoreError> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(CoreError::io(path, e)),
    }
}

/// Writes every embedded preset file that is missing and records hashes in the manifest.
///
/// Existing files are never touched. The manifest holds the hash of every file written or found
/// equal to the embedded text, and the old entry of any other existing file.
pub fn seed(presets_dir: &Path) -> Result<Vec<PathBuf>, CoreError> {
    let old = Manifest::read(presets_dir)?;
    let mut new = Manifest::default();
    let mut wrote = Vec::new();
    for name in PRESET_NAMES {
        let Some(p) = embedded(name) else { continue };
        for (path, text, _, rel) in files_of(presets_dir, p) {
            match read_existing(&path)? {
                None => {
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent).map_err(|e| CoreError::io(parent, e))?;
                    }
                    std::fs::write(&path, text).map_err(|e| CoreError::io(&path, e))?;
                    new.set(rel, sha256_hex(text.as_bytes()));
                    wrote.push(path);
                }
                Some(bytes) if bytes == text.as_bytes() => {
                    new.set(rel, sha256_hex(text.as_bytes()));
                }
                Some(_) => {
                    if let Some(h) = old.get(&rel) {
                        new.set(rel, h.to_string());
                    }
                }
            }
        }
    }
    new.write(presets_dir)?;
    Ok(wrote)
}

/// What `refresh` did with each embedded preset file.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RefreshReport {
    pub written: Vec<PathBuf>,
    pub updated: Vec<PathBuf>,
    pub skipped: Vec<PathBuf>,
    pub refused: Vec<PathBuf>,
}

/// Brings the seeded presets up to the embedded text. It only touches files under `presets_dir`.
///
/// An absent file is written and an equal one is skipped. A file whose hash matches its manifest
/// entry is unedited and is updated. A file without an entry that holds a `LEGACY` text is updated
/// too. Any other difference is a hand edit: it is refused, unless `force` overwrites it. The
/// manifest is read before any file is touched and written once at the end.
pub fn refresh(presets_dir: &Path, force: bool) -> Result<RefreshReport, CoreError> {
    let (report, manifest, writes) = plan(presets_dir, force)?;
    for (path, text) in writes {
        write_atomic(&path, text.as_bytes())?;
    }
    manifest.write(presets_dir)?;
    Ok(report)
}

/// Reports what refresh without force would do. It writes nothing.
pub fn plan_refresh(presets_dir: &Path) -> Result<RefreshReport, CoreError> {
    plan(presets_dir, false).map(|(report, _, _)| report)
}

/// The files `plan` wants written, each with its embedded text.
type Writes = Vec<(PathBuf, &'static str)>;

/// Decides the outcome of every embedded file without touching the disk.
fn plan(presets_dir: &Path, force: bool) -> Result<(RefreshReport, Manifest, Writes), CoreError> {
    let old = Manifest::read(presets_dir)?;
    let mut new = Manifest::default();
    let mut report = RefreshReport::default();
    let mut writes: Writes = Vec::new();
    for name in PRESET_NAMES {
        let Some(p) = embedded(name) else { continue };
        for (path, text, agent, rel) in files_of(presets_dir, p) {
            let embedded_hash = sha256_hex(text.as_bytes());
            let Some(existing) = read_existing(&path)? else {
                writes.push((path.clone(), text));
                new.set(rel, embedded_hash);
                report.written.push(path);
                continue;
            };
            if existing == text.as_bytes() {
                new.set(rel, embedded_hash);
                report.skipped.push(path);
                continue;
            }
            let unedited = old.get(&rel) == Some(sha256_hex(&existing).as_str());
            let is_legacy = agent.is_some_and(|file| {
                LEGACY
                    .iter()
                    .any(|(n, f, o)| *n == name && *f == file && o.as_bytes() == existing)
            });
            if unedited || is_legacy || force {
                writes.push((path.clone(), text));
                new.set(rel, embedded_hash);
                report.updated.push(path);
            } else {
                if let Some(h) = old.get(&rel) {
                    new.set(rel, h.to_string());
                }
                report.refused.push(path);
            }
        }
    }
    Ok((report, new, writes))
}

/// Where an agent file that shadows an updated preset file lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    /// `projects/<name>/agents/` in the factory home.
    Project,
    /// `.factory/agents/` on the tip of the base branch.
    Repo,
}

impl Layer {
    /// The word the upgrade report prints for the layer.
    pub fn label(self) -> &'static str {
        match self {
            Layer::Project => "project-layer",
            Layer::Repo => "repo-layer",
        }
    }
}

/// One line of the shadow report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShadowLine {
    /// An agent file in `layer` hides the updated preset file `file`.
    Shadow {
        project: String,
        layer: Layer,
        file: String,
    },
    /// The project could not be checked.
    Skipped { project: String, reason: String },
}

/// Opens the base tip of a project by name.
type TipFor<'a> = &'a dyn Fn(&str) -> Result<Box<dyn BaseTipFiles>, String>;

/// Finds project and repo agent files that shadow an updated preset agent file.
/// Only projects on the same preset are checked. `tip_for` opens a project's base tip.
pub fn shadowed(home: &Home, updated: &[PathBuf], tip_for: TipFor<'_>) -> Vec<ShadowLine> {
    let presets = home.presets_dir();
    // (preset, file name) for each updated agent file, in order.
    let mut counting: Vec<(String, String)> = Vec::new();
    for path in updated {
        let Ok(rel) = path.strip_prefix(&presets) else {
            continue;
        };
        let parts: Vec<&str> = rel.iter().filter_map(|p| p.to_str()).collect();
        if let [preset, "agents", file] = parts.as_slice() {
            counting.push(((*preset).to_string(), (*file).to_string()));
        }
    }
    if counting.is_empty() {
        return Vec::new();
    }
    let entries = match project::list(home) {
        Ok(e) => e,
        Err(e) => {
            return vec![ShadowLine::Skipped {
                project: "*".into(),
                reason: e.to_string(),
            }]
        }
    };
    let mut out = Vec::new();
    for entry in entries {
        let file = match entry.file {
            Ok(f) => f,
            Err(reason) => {
                out.push(ShadowLine::Skipped {
                    project: entry.name,
                    reason,
                });
                continue;
            }
        };
        let files: Vec<&str> = counting
            .iter()
            .filter(|(p, _)| *p == file.preset)
            .map(|(_, f)| f.as_str())
            .collect();
        if files.is_empty() {
            continue;
        }
        let tip = match tip_for(&entry.name) {
            Ok(t) => t,
            Err(reason) => {
                out.push(ShadowLine::Skipped {
                    project: entry.name,
                    reason,
                });
                continue;
            }
        };
        let listing = match tip.list_dir(AGENTS_DIR) {
            Ok(l) => l,
            Err(why) => {
                out.push(ShadowLine::Skipped {
                    project: entry.name,
                    reason: format!("cannot read .factory/agents from the base branch: {why}"),
                });
                continue;
            }
        };
        for f in files {
            if home
                .project_dir(&entry.name)
                .join("agents")
                .join(f)
                .is_file()
            {
                out.push(ShadowLine::Shadow {
                    project: entry.name.clone(),
                    layer: Layer::Project,
                    file: f.to_string(),
                });
            }
            if listing.iter().any(|(n, _)| n == f) {
                out.push(ShadowLine::Shadow {
                    project: entry.name.clone(),
                    layer: Layer::Repo,
                    file: f.to_string(),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::manifest::{sha256_hex, Manifest};
    use super::*;
    use crate::Config;

    #[test]
    fn the_node_preset_is_bun_and_script_driven() {
        let p = embedded("node").expect("node preset");
        let cfg = Config::parse(p.config).expect("parses");
        assert!(!cfg.degraded);
        let commands: Vec<&str> = cfg
            .checks
            .iter()
            .flat_map(|c| c.commands.iter().map(String::as_str))
            .collect();
        assert_eq!(commands, ["bun run typecheck", "bun run lint", "bun test"]);
        let risky = |path: &str| {
            cfg.risk_paths
                .iter()
                .any(|g| crate::risk::glob_match(g, path))
        };
        for path in [
            "package.json",
            "apps/web/vite.config.ts",
            ".env.local",
            "tsconfig.base.json",
        ] {
            assert!(risky(path), "{path} is risky");
        }
        assert!(!risky("bun.lock"));
        assert!(!risky("bun.lockb"));
        for forbidden in ["npm ", "yarn", "pnpm"] {
            assert!(!p.config.contains(forbidden), "{forbidden} in node config");
        }
    }

    #[test]
    fn every_preset_exists_and_parses_as_config() {
        for name in PRESET_NAMES {
            let p = embedded(name).unwrap_or_else(|| panic!("{name} missing"));
            assert_eq!(p.name, name);
            let cfg = Config::parse(p.config).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(!cfg.degraded);
            assert!(!cfg.risk_paths.is_empty(), "{name} sets risk_paths");
            assert!(
                !cfg.builder_rules.is_empty() && !cfg.reviewer_rules.is_empty(),
                "{name} fills the prompt keys"
            );
            assert!(!cfg.checks.is_empty(), "{name} has checks");
        }
        assert!(embedded("cobol").is_none());
    }

    #[test]
    fn presets_ship_the_five_agents_and_name_no_rocket_file() {
        for name in PRESET_NAMES {
            let p = embedded(name).expect("preset");
            let mut names: Vec<&str> = p.agents.iter().map(|(n, _)| *n).collect();
            names.sort();
            assert_eq!(
                names,
                [
                    "factory-builder.md",
                    "factory-code-reviewer.md",
                    "factory-committer.md",
                    "factory-kb-refresher.md",
                    "factory-spec-reviewer.md"
                ]
            );
            for (file, text) in p.agents {
                assert!(text.starts_with("---\n"), "{name}/{file} has frontmatter");
                assert!(
                    text.contains(&format!("name: {}", file.trim_end_matches(".md"))),
                    "{name}/{file} names itself"
                );
                for banned in [
                    "Rocket",
                    "src-tauri",
                    "shadcn",
                    "rocket-",
                    "tools/factory",
                    "yarn",
                ] {
                    assert!(
                        !text.contains(banned),
                        "{name}/{file} must not mention {banned}"
                    );
                }
            }
            assert!(!p.commit_context.contains("Rocket"));
        }
    }

    fn rust_config() -> Config {
        Config::parse(embedded("rust").expect("rust").config).expect("parse")
    }

    fn rust_uncovered(paths: &[&str]) -> Vec<String> {
        let owned: Vec<String> = paths.iter().map(|p| p.to_string()).collect();
        crate::gate::uncovered_code_paths(&owned, &rust_config())
    }

    #[test]
    fn rust_preset_covers_the_root_manifest_and_lockfile() {
        assert!(rust_uncovered(&["Cargo.toml", "Cargo.lock"]).is_empty());
        let config = rust_config();
        let rule = config
            .checks
            .iter()
            .find(|r| r.matches("Cargo.toml"))
            .expect("rule for Cargo.toml");
        assert_eq!(rule.commands, vec!["cargo check -j4".to_string()]);
    }

    #[test]
    fn rust_preset_root_rule_does_not_cover_other_manifests() {
        assert_eq!(
            rust_uncovered(&["dashboard/Cargo.toml"]),
            vec!["dashboard/Cargo.toml".to_string()]
        );
    }

    #[test]
    fn rust_preset_exempts_license_and_dotfiles() {
        let paths = [
            "LICENSE",
            "LICENSE-MIT",
            "license",
            "LICENSE.txt",
            "COPYING",
            "COPYING.txt",
            ".gitignore",
            ".gitattributes",
            ".editorconfig",
        ];
        assert!(rust_uncovered(&paths).is_empty());
    }

    #[test]
    fn rust_preset_does_not_exempt_tooling_files() {
        let paths = [
            "rustfmt.toml",
            "rust-toolchain.toml",
            "scripts/release.sh",
            "cliff.toml",
            "xtask/src/license.rs",
            "tools/licenses.rs",
            "vendor/x/LICENSE",
            "sub/.gitattributes",
        ];
        let expected: Vec<String> = paths.iter().map(|p| p.to_string()).collect();
        assert_eq!(rust_uncovered(&paths), expected);
    }

    #[test]
    fn rust_preset_nested_crate_manifest_is_covered() {
        assert!(rust_uncovered(&["crates/factory-core/Cargo.toml"]).is_empty());
    }

    #[test]
    fn rust_preset_root_command_equals_the_src_rule_command() {
        let config = rust_config();
        let src = config
            .checks
            .iter()
            .find(|r| r.matches("src/main.rs"))
            .expect("src rule");
        let root = config
            .checks
            .iter()
            .find(|r| r.matches("Cargo.toml"))
            .expect("root rule");
        assert_eq!(src.commands[0], "cargo check -j4");
        assert_eq!(root.commands[0], src.commands[0]);
    }

    #[test]
    fn rust_checks_use_cargo_with_j4_and_go_checks_use_go() {
        let rust = Config::parse(embedded("rust").expect("rust").config).expect("parse");
        let all: Vec<&String> = rust.checks.iter().flat_map(|c| &c.commands).collect();
        assert!(all
            .iter()
            .all(|c| !c.starts_with("cargo") || c.contains("-j4")));
        let go = Config::parse(embedded("go").expect("go").config).expect("parse");
        assert!(go
            .checks
            .iter()
            .flat_map(|c| &c.commands)
            .any(|c| c.starts_with("go test")));
    }

    #[test]
    fn seed_writes_missing_files_and_never_overwrites() {
        let dir = tempfile::tempdir().expect("tmp");
        let wrote = seed(dir.path()).expect("seed");
        assert!(wrote.iter().any(|p| p.ends_with("rust/config.toml")));
        assert!(wrote
            .iter()
            .any(|p| p.ends_with("go/agents/factory-builder.md")));
        let edited = dir.path().join("rust/config.toml");
        std::fs::write(&edited, "# mine\n").expect("edit");
        std::fs::remove_file(dir.path().join("go/commit-context.md")).expect("rm");
        let again = seed(dir.path()).expect("seed again");
        assert_eq!(again, vec![dir.path().join("go/commit-context.md")]);
        assert_eq!(std::fs::read_to_string(&edited).expect("read"), "# mine\n");
    }

    fn legacy_builder() -> &'static str {
        LEGACY[0].2
    }

    #[test]
    fn refresh_writes_absent_files() {
        let dir = tempfile::tempdir().expect("tmp");
        let r = refresh(dir.path(), false).expect("refresh");
        assert_eq!(r.written.len(), 35);
        assert!(r.updated.is_empty() && r.skipped.is_empty() && r.refused.is_empty());
        assert!(dir.path().join("go/agents/factory-builder.md").exists());
    }

    #[test]
    fn refresh_skips_equal_files() {
        let dir = tempfile::tempdir().expect("tmp");
        seed(dir.path()).expect("seed");
        let r = refresh(dir.path(), false).expect("refresh");
        assert_eq!(r.skipped.len(), 35);
        assert!(r.written.is_empty() && r.updated.is_empty() && r.refused.is_empty());
    }

    #[test]
    fn refresh_updates_a_known_legacy_text() {
        let dir = tempfile::tempdir().expect("tmp");
        seed(dir.path()).expect("seed");
        let path = dir.path().join("rust/agents/factory-builder.md");
        std::fs::write(&path, legacy_builder()).expect("old");
        let r = refresh(dir.path(), false).expect("refresh");
        assert_eq!(r.updated, vec![path.clone()]);
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            embedded("rust").expect("rust").agents[0].1
        );
    }

    fn shipped(preset: &str, file: &str) -> &'static str {
        embedded(preset)
            .unwrap_or_else(|| panic!("{preset} missing"))
            .agents
            .iter()
            .find(|(f, _)| *f == file)
            .unwrap_or_else(|| panic!("{preset} ships no {file}"))
            .1
    }

    fn other_preset(name: &str) -> &'static str {
        if name == "rust" {
            "go"
        } else {
            "rust"
        }
    }

    #[test]
    fn every_legacy_entry_names_a_shipped_agent_and_is_an_older_text() {
        let mut seen = std::collections::BTreeSet::new();
        for (name, file, old) in LEGACY.iter() {
            assert!(seen.insert((*name, *file)), "{name}/{file} appears twice");
            assert!(!old.trim().is_empty(), "{name}/{file} legacy text is empty");
            assert_ne!(
                *old,
                shipped(name, file),
                "{name}/{file} legacy text equals the shipped text"
            );
        }
    }

    #[test]
    fn refresh_updates_every_legacy_entry_and_nothing_else() {
        for (name, file, old) in LEGACY.iter() {
            let dir = tempfile::tempdir().expect("tmp");
            seed(dir.path()).expect("seed");
            let path = dir.path().join(name).join("agents").join(file);
            std::fs::write(&path, old).expect("old");
            let r = refresh(dir.path(), false).expect("refresh");
            assert_eq!(r.updated, vec![path.clone()], "{name}/{file}");
            assert!(
                r.refused.is_empty() && r.written.is_empty(),
                "{name}/{file}"
            );
            assert_eq!(r.skipped.len(), 34, "{name}/{file}");
            assert_eq!(
                std::fs::read_to_string(&path).expect("read"),
                shipped(name, file),
                "{name}/{file}"
            );
        }
    }

    #[test]
    fn refresh_refuses_a_legacy_text_that_is_not_for_that_file() {
        for (name, file, old) in LEGACY.iter() {
            let dir = tempfile::tempdir().expect("tmp");
            seed(dir.path()).expect("seed");
            let other = other_preset(name);
            let wrong_preset = dir.path().join(other).join("agents").join(file);
            std::fs::write(&wrong_preset, old).expect("write");
            let r = refresh(dir.path(), false).expect("refresh");
            assert_eq!(
                r.refused,
                vec![wrong_preset.clone()],
                "{name}/{file} in {other}"
            );
            assert!(r.updated.is_empty(), "{name}/{file} in {other}");
            assert_eq!(std::fs::read_to_string(&wrong_preset).expect("read"), *old);
        }
    }

    #[test]
    fn refresh_refuses_a_legacy_text_with_a_hand_edit() {
        for (name, file, old) in LEGACY.iter() {
            let dir = tempfile::tempdir().expect("tmp");
            seed(dir.path()).expect("seed");
            let path = dir.path().join(name).join("agents").join(file);
            let edited = format!("{old}\n# my own rule\n");
            std::fs::write(&path, &edited).expect("edit");
            let r = refresh(dir.path(), false).expect("refresh");
            assert_eq!(r.refused, vec![path.clone()], "{name}/{file}");
            assert!(r.updated.is_empty(), "{name}/{file}");
            assert_eq!(
                std::fs::read_to_string(&path).expect("read"),
                edited,
                "{name}/{file}"
            );
        }
    }

    #[test]
    fn refresh_refuses_a_hand_edited_file_and_leaves_it_byte_identical() {
        let dir = tempfile::tempdir().expect("tmp");
        seed(dir.path()).expect("seed");
        let path = dir.path().join("rust/config.toml");
        std::fs::write(&path, "# mine\n").expect("edit");
        let r = refresh(dir.path(), false).expect("refresh");
        assert_eq!(r.refused, vec![path.clone()]);
        assert_eq!(std::fs::read(&path).expect("read"), b"# mine\n");
    }

    #[test]
    fn refresh_force_overwrites_a_hand_edited_file() {
        let dir = tempfile::tempdir().expect("tmp");
        seed(dir.path()).expect("seed");
        let path = dir.path().join("go/agents/factory-code-reviewer.md");
        std::fs::write(&path, "# mine\n").expect("edit");
        let r = refresh(dir.path(), true).expect("refresh");
        assert_eq!(r.updated, vec![path.clone()]);
        assert!(r.refused.is_empty());
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            embedded("go").expect("go").agents[1].1
        );
    }

    fn all_rels() -> Vec<String> {
        let mut v = Vec::new();
        for name in PRESET_NAMES {
            v.push(format!("{name}/config.toml"));
            v.push(format!("{name}/commit-context.md"));
            for (f, _) in embedded(name).expect("preset").agents {
                v.push(format!("{name}/agents/{f}"));
            }
        }
        v
    }

    fn embedded_text(rel: &str) -> &'static str {
        let mut parts = rel.splitn(2, '/');
        let name = parts.next().expect("preset");
        let rest = parts.next().expect("rest");
        let p = embedded(name).expect("preset");
        match rest {
            "config.toml" => p.config,
            "commit-context.md" => p.commit_context,
            _ => shipped(name, rest.trim_start_matches("agents/")),
        }
    }

    #[test]
    fn an_unedited_file_that_differs_from_the_embedded_text_is_updated_through_the_manifest() {
        let dir = tempfile::tempdir().expect("tmp");
        seed(dir.path()).expect("seed");
        let rel = "rust/agents/factory-committer.md";
        let path = dir.path().join(rel);
        let older = "---\nname: factory-committer\n---\nAn older shipped text.\n";
        std::fs::write(&path, older).expect("old");
        let mut m = Manifest::read(dir.path()).expect("read");
        m.set(rel.to_string(), sha256_hex(older.as_bytes()));
        m.write(dir.path()).expect("write");

        let r = refresh(dir.path(), false).expect("refresh");
        assert_eq!(r.updated, vec![path.clone()]);
        assert!(r.refused.is_empty() && r.written.is_empty());
        assert_eq!(r.skipped.len(), 34);
        let shipped_text = shipped("rust", "factory-committer.md");
        assert_eq!(std::fs::read_to_string(&path).expect("read"), shipped_text);
        let after = Manifest::read(dir.path()).expect("read again");
        assert_eq!(
            after.get(rel),
            Some(sha256_hex(shipped_text.as_bytes()).as_str())
        );
    }

    #[test]
    fn seed_records_a_hash_for_every_file_it_writes() {
        let dir = tempfile::tempdir().expect("tmp");
        let wrote = seed(dir.path()).expect("seed");
        assert_eq!(wrote.len(), 35);
        assert!(wrote.iter().all(|p| !p.ends_with(MANIFEST_FILE)));
        let m = Manifest::read(dir.path()).expect("read");
        for rel in all_rels() {
            let bytes = std::fs::read(dir.path().join(&rel)).expect("file");
            assert_eq!(m.get(&rel), Some(sha256_hex(&bytes).as_str()), "{rel}");
        }
        assert_eq!(m.render().lines().count(), 35);
    }

    #[test]
    fn seed_keeps_the_entry_of_an_existing_edited_file() {
        let dir = tempfile::tempdir().expect("tmp");
        seed(dir.path()).expect("seed");
        std::fs::write(dir.path().join("rust/config.toml"), "# mine\n").expect("edit");
        let again = seed(dir.path()).expect("seed again");
        assert!(again.is_empty());
        let m = Manifest::read(dir.path()).expect("read");
        let want = sha256_hex(embedded("rust").expect("rust").config.as_bytes());
        assert_eq!(m.get("rust/config.toml"), Some(want.as_str()));
    }

    #[test]
    fn seed_records_files_equal_to_the_embedded_text() {
        let dir = tempfile::tempdir().expect("tmp");
        seed(dir.path()).expect("seed");
        std::fs::remove_file(dir.path().join(MANIFEST_FILE)).expect("rm");
        std::fs::write(dir.path().join("rust/config.toml"), "# mine\n").expect("edit");
        seed(dir.path()).expect("seed again");
        let m = Manifest::read(dir.path()).expect("read");
        assert_eq!(m.render().lines().count(), 34);
        assert!(m.get("rust/config.toml").is_none());
    }

    fn edit_reviewer(dir: &Path) -> PathBuf {
        let path = dir.join("go/agents/factory-code-reviewer.md");
        std::fs::write(&path, "# mine\n").expect("edit");
        path
    }

    #[test]
    fn an_edited_file_with_a_manifest_entry_is_refused() {
        let dir = tempfile::tempdir().expect("tmp");
        seed(dir.path()).expect("seed");
        let before = std::fs::read_to_string(dir.path().join(MANIFEST_FILE)).expect("manifest");
        let path = edit_reviewer(dir.path());
        let r = refresh(dir.path(), false).expect("refresh");
        assert_eq!(r.refused, vec![path.clone()]);
        assert_eq!(std::fs::read(&path).expect("read"), b"# mine\n");
        let after = std::fs::read_to_string(dir.path().join(MANIFEST_FILE)).expect("manifest");
        assert_eq!(after, before);
    }

    #[test]
    fn refresh_force_records_the_new_hash() {
        let dir = tempfile::tempdir().expect("tmp");
        seed(dir.path()).expect("seed");
        let path = edit_reviewer(dir.path());
        let r = refresh(dir.path(), true).expect("refresh");
        assert_eq!(r.updated, vec![path]);
        let m = Manifest::read(dir.path()).expect("read");
        let want = sha256_hex(shipped("go", "factory-code-reviewer.md").as_bytes());
        assert_eq!(
            m.get("go/agents/factory-code-reviewer.md"),
            Some(want.as_str())
        );
    }

    #[test]
    fn a_home_without_a_manifest_uses_the_legacy_rule_and_then_gets_one() {
        let dir = tempfile::tempdir().expect("tmp");
        seed(dir.path()).expect("seed");
        std::fs::remove_file(dir.path().join(MANIFEST_FILE)).expect("rm");
        let path = dir.path().join("rust/agents/factory-builder.md");
        std::fs::write(&path, legacy_builder()).expect("old");
        let r = refresh(dir.path(), false).expect("refresh");
        assert_eq!(r.updated, vec![path]);
        let m = Manifest::read(dir.path()).expect("read");
        assert_eq!(m.render().lines().count(), 35);
        for rel in all_rels() {
            let want = sha256_hex(embedded_text(&rel).as_bytes());
            assert_eq!(m.get(&rel), Some(want.as_str()), "{rel}");
        }
    }

    #[test]
    fn a_home_without_a_manifest_refuses_a_changed_config_until_force() {
        let dir = tempfile::tempdir().expect("tmp");
        seed(dir.path()).expect("seed");
        std::fs::remove_file(dir.path().join(MANIFEST_FILE)).expect("rm");
        let path = dir.path().join("go/config.toml");
        std::fs::write(&path, "# an older shipped config\n").expect("old");
        for _ in 0..2 {
            let r = refresh(dir.path(), false).expect("refresh");
            assert_eq!(r.refused, vec![path.clone()]);
            let m = Manifest::read(dir.path()).expect("read");
            assert!(m.get("go/config.toml").is_none());
        }
        let r = refresh(dir.path(), true).expect("refresh");
        assert_eq!(r.updated, vec![path]);
        let m = Manifest::read(dir.path()).expect("read");
        let want = sha256_hex(embedded("go").expect("go").config.as_bytes());
        assert_eq!(m.get("go/config.toml"), Some(want.as_str()));
    }

    #[test]
    fn a_bad_manifest_line_makes_that_file_unmanifested() {
        let dir = tempfile::tempdir().expect("tmp");
        seed(dir.path()).expect("seed");
        let rel = "rust/agents/factory-committer.md";
        let path = dir.path().join(rel);
        std::fs::write(&path, "An older text.\n").expect("old");
        let mpath = dir.path().join(MANIFEST_FILE);
        let text = std::fs::read_to_string(&mpath).expect("manifest");
        let rewritten: String = text
            .lines()
            .map(|l| {
                if l.ends_with(rel) {
                    format!("nothex  {rel}\n")
                } else {
                    format!("{l}\n")
                }
            })
            .collect();
        std::fs::write(&mpath, rewritten).expect("write");
        let r = refresh(dir.path(), false).expect("refresh");
        assert_eq!(r.refused, vec![path]);
    }

    #[test]
    fn a_deleted_preset_file_is_written_again_and_recorded() {
        let dir = tempfile::tempdir().expect("tmp");
        seed(dir.path()).expect("seed");
        let path = dir.path().join("go/commit-context.md");
        std::fs::remove_file(&path).expect("rm");
        let r = refresh(dir.path(), false).expect("refresh");
        assert_eq!(r.written, vec![path]);
        let m = Manifest::read(dir.path()).expect("read");
        let want = sha256_hex(embedded("go").expect("go").commit_context.as_bytes());
        assert_eq!(m.get("go/commit-context.md"), Some(want.as_str()));
    }

    #[test]
    fn unknown_manifest_entries_are_dropped_on_write() {
        let dir = tempfile::tempdir().expect("tmp");
        seed(dir.path()).expect("seed");
        let mpath = dir.path().join(MANIFEST_FILE);
        let mut text = std::fs::read_to_string(&mpath).expect("manifest");
        text.push_str(&format!("{}  rust/agents/retired.md\n", sha256_hex(b"x")));
        std::fs::write(&mpath, text).expect("write");
        let retired = dir.path().join("rust/agents/retired.md");
        std::fs::write(&retired, "x").expect("retired");
        refresh(dir.path(), false).expect("refresh");
        let m = Manifest::read(dir.path()).expect("read");
        assert!(m.get("rust/agents/retired.md").is_none());
        assert_eq!(std::fs::read_to_string(&retired).expect("read"), "x");
    }

    #[test]
    fn refresh_fails_before_writing_when_the_manifest_cannot_be_read() {
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::create_dir(dir.path().join(MANIFEST_FILE)).expect("mkdir");
        let err = refresh(dir.path(), false).expect_err("manifest is a directory");
        assert!(matches!(err, CoreError::Io { .. }), "{err:?}");
        assert!(!dir.path().join("rust").exists());
    }

    #[test]
    fn refresh_never_touches_project_or_repo_layers() {
        let home = tempfile::tempdir().expect("tmp");
        let presets = home.path().join("presets");
        let layer = home.path().join("projects/p/agents/factory-builder.md");
        std::fs::create_dir_all(layer.parent().expect("parent")).expect("mkdir");
        std::fs::write(&layer, "# project override\n").expect("write");
        let repo_layer = home.path().join("repo/.factory/agents/factory-builder.md");
        std::fs::create_dir_all(repo_layer.parent().expect("parent")).expect("mkdir");
        std::fs::write(&repo_layer, "# repo override\n").expect("write");
        refresh(&presets, true).expect("refresh");
        assert_eq!(
            std::fs::read_to_string(&layer).expect("read"),
            "# project override\n"
        );
        assert_eq!(
            std::fs::read_to_string(&repo_layer).expect("read"),
            "# repo override\n"
        );
    }

    fn snapshot(dir: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
        fn walk(dir: &Path, out: &mut std::collections::BTreeMap<PathBuf, Vec<u8>>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for e in entries {
                let p = e.expect("entry").path();
                if p.is_dir() {
                    walk(&p, out);
                } else {
                    out.insert(p.clone(), std::fs::read(&p).expect("read"));
                }
            }
        }
        let mut out = std::collections::BTreeMap::new();
        walk(dir, &mut out);
        out
    }

    fn messy_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tmp");
        seed(dir.path()).expect("seed");
        std::fs::remove_file(dir.path().join("go/commit-context.md")).expect("rm");
        std::fs::write(dir.path().join("rust/config.toml"), "# mine\n").expect("edit");
        std::fs::write(
            dir.path().join("rust/agents/factory-builder.md"),
            legacy_builder(),
        )
        .expect("old");
        dir
    }

    #[test]
    fn plan_refresh_writes_nothing() {
        let dir = messy_dir();
        let before = snapshot(dir.path());

        let plan = plan_refresh(dir.path()).expect("plan");
        assert_eq!(snapshot(dir.path()), before);
        assert_eq!(plan.written, vec![dir.path().join("go/commit-context.md")]);
        assert_eq!(
            plan.updated,
            vec![dir.path().join("rust/agents/factory-builder.md")]
        );
        assert_eq!(plan.refused, vec![dir.path().join("rust/config.toml")]);
        assert_eq!(plan.skipped.len(), 32);
    }

    #[test]
    fn plan_refresh_reports_what_refresh_then_does() {
        let dir = messy_dir();
        let rel = "go/agents/factory-committer.md";
        let older = "---\nname: factory-committer\n---\nAn older shipped text.\n";
        std::fs::write(dir.path().join(rel), older).expect("old");
        let mut m = Manifest::read(dir.path()).expect("read");
        m.set(rel.to_string(), sha256_hex(older.as_bytes()));
        m.write(dir.path()).expect("write");

        let plan = plan_refresh(dir.path()).expect("plan");
        let done = refresh(dir.path(), false).expect("refresh");
        assert_eq!(plan, done);
        assert!(done.updated.contains(&dir.path().join(rel)));
    }

    #[test]
    fn plan_refresh_on_a_missing_dir_creates_nothing() {
        let dir = tempfile::tempdir().expect("tmp");
        let presets = dir.path().join("presets");
        let plan = plan_refresh(&presets).expect("plan");
        assert_eq!(plan.written.len(), 35);
        assert!(plan.updated.is_empty() && plan.skipped.is_empty() && plan.refused.is_empty());
        assert!(!presets.exists());
    }

    #[test]
    fn plan_refresh_never_forces() {
        let dir = tempfile::tempdir().expect("tmp");
        seed(dir.path()).expect("seed");
        let path = dir.path().join("go/config.toml");
        std::fs::write(&path, "# mine\n").expect("edit");
        let plan = plan_refresh(dir.path()).expect("plan");
        assert_eq!(plan.refused, vec![path]);
        assert!(plan.updated.is_empty());
    }

    #[test]
    fn plan_refresh_fails_when_the_manifest_cannot_be_read() {
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::create_dir(dir.path().join(MANIFEST_FILE)).expect("mkdir");
        let err = plan_refresh(dir.path()).expect_err("manifest is a directory");
        assert!(matches!(err, CoreError::Io { .. }), "{err:?}");
    }

    struct ListTip(Vec<(String, String)>);
    impl BaseTipFiles for ListTip {
        fn read_file(&self, _rel: &str) -> Result<Option<String>, String> {
            Ok(None)
        }
        fn list_dir(&self, rel: &str) -> Result<Vec<(String, String)>, String> {
            assert_eq!(rel, ".factory/agents");
            Ok(self.0.clone())
        }
    }
    struct FailingList;
    impl BaseTipFiles for FailingList {
        fn read_file(&self, _rel: &str) -> Result<Option<String>, String> {
            Ok(None)
        }
        fn list_dir(&self, _rel: &str) -> Result<Vec<(String, String)>, String> {
            Err("bad tree".into())
        }
    }
    /// Writes projects/<name>/project.toml by hand, with an absolute repo path.
    fn register(home: &Home, name: &str, preset: &str) {
        let dir = home.project_dir(name);
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(
            dir.join("project.toml"),
            format!(
                "repo = \"/nowhere/{name}\"\npreset = \"{preset}\"\n\n[settings]\nbase_branch = \"main\"\n"
            ),
        )
        .expect("write project.toml");
    }
    fn builder_tip() -> Result<Box<dyn BaseTipFiles>, String> {
        Ok(Box::new(ListTip(vec![(
            "factory-builder.md".into(),
            "# repo\n".into(),
        )])))
    }
    fn never(_: &str) -> Result<Box<dyn BaseTipFiles>, String> {
        panic!("must not be called")
    }

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
                ShadowLine::Shadow {
                    project: "alpha".into(),
                    layer: Layer::Project,
                    file: "factory-builder.md".into()
                },
                ShadowLine::Shadow {
                    project: "alpha".into(),
                    layer: Layer::Repo,
                    file: "factory-builder.md".into()
                },
            ]
        );
        assert_eq!(Layer::Project.label(), "project-layer");
        assert_eq!(Layer::Repo.label(), "repo-layer");
    }

    #[test]
    fn shadowed_returns_nothing_when_no_agent_file_was_updated() {
        let tmp = tempfile::tempdir().expect("tmp");
        let home = Home::at(tmp.path().to_path_buf());
        register(&home, "alpha", "rust");
        let updated = vec![
            home.presets_dir().join("rust/config.toml"),
            home.presets_dir().join("rust/commit-context.md"),
        ];
        assert_eq!(shadowed(&home, &updated, &never), vec![]);
    }

    #[test]
    fn shadowed_ignores_non_agent_files_and_other_presets() {
        let tmp = tempfile::tempdir().expect("tmp");
        let home = Home::at(tmp.path().to_path_buf());
        register(&home, "alpha", "go");
        let layer = home.project_dir("alpha").join("agents/factory-builder.md");
        std::fs::create_dir_all(layer.parent().expect("parent")).expect("mkdir");
        std::fs::write(&layer, "# project override\n").expect("write");
        let updated = vec![
            home.presets_dir().join("rust/agents/factory-builder.md"),
            home.presets_dir().join("rust/config.toml"),
        ];
        assert_eq!(shadowed(&home, &updated, &never), vec![]);
    }

    #[test]
    fn shadowed_skips_a_broken_project_and_checks_the_rest() {
        let tmp = tempfile::tempdir().expect("tmp");
        let home = Home::at(tmp.path().to_path_buf());
        let bad = home.project_dir("bad");
        std::fs::create_dir_all(&bad).expect("mkdir");
        std::fs::write(bad.join("project.toml"), "not toml [").expect("write");
        register(&home, "good", "rust");
        let updated = vec![home.presets_dir().join("rust/agents/factory-builder.md")];
        let tip_for = |_: &str| builder_tip();
        let lines = shadowed(&home, &updated, &tip_for);
        assert_eq!(lines.len(), 2, "{lines:?}");
        match &lines[0] {
            ShadowLine::Skipped { project, reason } => {
                assert_eq!(project, "bad");
                assert!(reason.contains("project.toml"), "{reason}");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(
            lines[1],
            ShadowLine::Shadow {
                project: "good".into(),
                layer: Layer::Repo,
                file: "factory-builder.md".into()
            }
        );
    }

    #[test]
    fn shadowed_skips_a_project_whose_tip_cannot_be_read() {
        let tmp = tempfile::tempdir().expect("tmp");
        let home = Home::at(tmp.path().to_path_buf());
        register(&home, "alpha", "rust");
        register(&home, "beta", "rust");
        let updated = vec![home.presets_dir().join("rust/agents/factory-builder.md")];
        let tip_for = |name: &str| -> Result<Box<dyn BaseTipFiles>, String> {
            if name == "alpha" {
                Err("not a git repository: gone".into())
            } else {
                builder_tip()
            }
        };
        assert_eq!(
            shadowed(&home, &updated, &tip_for),
            vec![
                ShadowLine::Skipped {
                    project: "alpha".into(),
                    reason: "not a git repository: gone".into()
                },
                ShadowLine::Shadow {
                    project: "beta".into(),
                    layer: Layer::Repo,
                    file: "factory-builder.md".into()
                },
            ]
        );
    }

    #[test]
    fn shadowed_skips_when_the_tip_listing_fails() {
        let tmp = tempfile::tempdir().expect("tmp");
        let home = Home::at(tmp.path().to_path_buf());
        register(&home, "alpha", "rust");
        let updated = vec![home.presets_dir().join("rust/agents/factory-builder.md")];
        let tip_for =
            |_: &str| -> Result<Box<dyn BaseTipFiles>, String> { Ok(Box::new(FailingList)) };
        let lines = shadowed(&home, &updated, &tip_for);
        assert_eq!(lines.len(), 1, "{lines:?}");
        match &lines[0] {
            ShadowLine::Skipped { project, reason } => {
                assert_eq!(project, "alpha");
                assert!(reason.contains("bad tree"), "{reason}");
                assert!(reason.contains(".factory/agents"), "{reason}");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn shadowed_reports_nothing_without_overrides() {
        let tmp = tempfile::tempdir().expect("tmp");
        let home = Home::at(tmp.path().to_path_buf());
        register(&home, "alpha", "rust");
        let updated = vec![home.presets_dir().join("rust/agents/factory-builder.md")];
        let tip_for =
            |_: &str| -> Result<Box<dyn BaseTipFiles>, String> { Ok(Box::new(ListTip(vec![]))) };
        assert_eq!(shadowed(&home, &updated, &tip_for), vec![]);
    }
}
