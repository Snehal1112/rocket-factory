//! The presets that ship with the factory. `seed` copies them into the factory home.

use std::path::{Path, PathBuf};

use crate::{write_atomic, CoreError};

pub const PRESET_NAMES: [&str; 2] = ["rust", "go"];

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
    ],
};

pub fn embedded(name: &str) -> Option<&'static Preset> {
    match name {
        "rust" => Some(&RUST),
        "go" => Some(&GO),
        _ => None,
    }
}

/// The agent texts that earlier versions seeded: (preset, file, text). `refresh` replaces these without `--force`.
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

/// Every file of a preset as (path under `presets_dir`, embedded text, legacy file key).
fn files_of<'a>(presets_dir: &Path, p: &'a Preset) -> Vec<(PathBuf, &'a str, Option<&'a str>)> {
    let base = presets_dir.join(p.name);
    let mut files = vec![
        (base.join("config.toml"), p.config, None),
        (base.join("commit-context.md"), p.commit_context, None),
    ];
    for (file, text) in p.agents {
        files.push((base.join("agents").join(file), *text, Some(*file)));
    }
    files
}

/// Writes every embedded preset file that is missing. Existing files are never touched.
pub fn seed(presets_dir: &Path) -> Result<Vec<PathBuf>, CoreError> {
    let mut wrote = Vec::new();
    for name in PRESET_NAMES {
        let Some(p) = embedded(name) else { continue };
        for (path, text, _) in files_of(presets_dir, p) {
            if path.exists() {
                continue;
            }
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| CoreError::io(parent, e))?;
            }
            std::fs::write(&path, text).map_err(|e| CoreError::io(&path, e))?;
            wrote.push(path);
        }
    }
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
/// An absent file is written and an equal one is skipped. A file that holds a known older text is
/// updated. Any other difference is a hand edit: it is refused, unless `force` overwrites it.
pub fn refresh(presets_dir: &Path, force: bool) -> Result<RefreshReport, CoreError> {
    let mut report = RefreshReport::default();
    for name in PRESET_NAMES {
        let Some(p) = embedded(name) else { continue };
        for (path, text, agent) in files_of(presets_dir, p) {
            let existing = match std::fs::read(&path) {
                Ok(bytes) => Some(bytes),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => return Err(CoreError::io(&path, e)),
            };
            let Some(existing) = existing else {
                write_atomic(&path, text.as_bytes())?;
                report.written.push(path);
                continue;
            };
            if existing == text.as_bytes() {
                report.skipped.push(path);
                continue;
            }
            let is_legacy = agent.is_some_and(|file| {
                LEGACY
                    .iter()
                    .any(|(n, f, old)| *n == name && *f == file && old.as_bytes() == existing)
            });
            if is_legacy || force {
                write_atomic(&path, text.as_bytes())?;
                report.updated.push(path);
            } else {
                report.refused.push(path);
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;

    #[test]
    fn both_presets_exist_and_parse_as_config() {
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
    fn presets_ship_the_four_agents_and_name_no_rocket_file() {
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
                    "factory-kb-refresher.md"
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
        assert_eq!(r.written.len(), 12);
        assert!(r.updated.is_empty() && r.skipped.is_empty() && r.refused.is_empty());
        assert!(dir.path().join("go/agents/factory-builder.md").exists());
    }

    #[test]
    fn refresh_skips_equal_files() {
        let dir = tempfile::tempdir().expect("tmp");
        seed(dir.path()).expect("seed");
        let r = refresh(dir.path(), false).expect("refresh");
        assert_eq!(r.skipped.len(), 12);
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
            assert_eq!(r.skipped.len(), 11, "{name}/{file}");
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
}
