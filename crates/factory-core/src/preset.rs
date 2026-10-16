//! The presets that ship with the factory. `seed` copies them into the factory home.

use std::path::{Path, PathBuf};

use crate::CoreError;

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
        ("factory-builder.md", include_str!("../presets/rust/agents/factory-builder.md")),
        ("factory-code-reviewer.md", include_str!("../presets/rust/agents/factory-code-reviewer.md")),
        ("factory-committer.md", include_str!("../presets/rust/agents/factory-committer.md")),
        ("factory-kb-refresher.md", include_str!("../presets/rust/agents/factory-kb-refresher.md")),
    ],
};

static GO: Preset = Preset {
    name: "go",
    config: include_str!("../presets/go/config.toml"),
    commit_context: include_str!("../presets/go/commit-context.md"),
    agents: &[
        ("factory-builder.md", include_str!("../presets/go/agents/factory-builder.md")),
        ("factory-code-reviewer.md", include_str!("../presets/go/agents/factory-code-reviewer.md")),
        ("factory-committer.md", include_str!("../presets/go/agents/factory-committer.md")),
        ("factory-kb-refresher.md", include_str!("../presets/go/agents/factory-kb-refresher.md")),
    ],
};

pub fn embedded(name: &str) -> Option<&'static Preset> {
    match name {
        "rust" => Some(&RUST),
        "go" => Some(&GO),
        _ => None,
    }
}

/// Writes every embedded preset file that is missing. Existing files are never touched.
pub fn seed(presets_dir: &Path) -> Result<Vec<PathBuf>, CoreError> {
    let mut wrote = Vec::new();
    for name in PRESET_NAMES {
        let Some(p) = embedded(name) else { continue };
        let base = presets_dir.join(name);
        let mut files: Vec<(PathBuf, &str)> =
            vec![(base.join("config.toml"), p.config), (base.join("commit-context.md"), p.commit_context)];
        for (file, text) in p.agents {
            files.push((base.join("agents").join(file), text));
        }
        for (path, text) in files {
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
            assert!(!cfg.builder_rules.is_empty() && !cfg.reviewer_rules.is_empty(), "{name} fills the prompt keys");
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
            assert_eq!(names, ["factory-builder.md", "factory-code-reviewer.md", "factory-committer.md", "factory-kb-refresher.md"]);
            for (file, text) in p.agents {
                assert!(text.starts_with("---\n"), "{name}/{file} has frontmatter");
                assert!(text.contains(&format!("name: {}", file.trim_end_matches(".md"))), "{name}/{file} names itself");
                for banned in ["Rocket", "src-tauri", "shadcn", "rocket-", "tools/factory", "yarn"] {
                    assert!(!text.contains(banned), "{name}/{file} must not mention {banned}");
                }
            }
            assert!(!p.commit_context.contains("Rocket"));
        }
    }

    #[test]
    fn rust_checks_use_cargo_with_j4_and_go_checks_use_go() {
        let rust = Config::parse(embedded("rust").expect("rust").config).expect("parse");
        let all: Vec<&String> = rust.checks.iter().flat_map(|c| &c.commands).collect();
        assert!(all.iter().all(|c| !c.starts_with("cargo") || c.contains("-j4")));
        let go = Config::parse(embedded("go").expect("go").config).expect("parse");
        assert!(go.checks.iter().flat_map(|c| &c.commands).any(|c| c.starts_with("go test")));
    }

    #[test]
    fn seed_writes_missing_files_and_never_overwrites() {
        let dir = tempfile::tempdir().expect("tmp");
        let wrote = seed(dir.path()).expect("seed");
        assert!(wrote.iter().any(|p| p.ends_with("rust/config.toml")));
        assert!(wrote.iter().any(|p| p.ends_with("go/agents/factory-builder.md")));
        let edited = dir.path().join("rust/config.toml");
        std::fs::write(&edited, "# mine\n").expect("edit");
        std::fs::remove_file(dir.path().join("go/commit-context.md")).expect("rm");
        let again = seed(dir.path()).expect("seed again");
        assert_eq!(again, vec![dir.path().join("go/commit-context.md")]);
        assert_eq!(std::fs::read_to_string(&edited).expect("read"), "# mine\n");
    }
}
