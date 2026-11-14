//! Checks that the installed superpowers has the skills the factory commands name.
//! It reads no skill text and passes on a machine with no plugin installed.

use std::path::{Path, PathBuf};

/// The skills the factory commands invoke.
const REQUIRED_SKILLS: [&str; 3] = [
    "brainstorming",
    "writing-plans",
    "subagent-driven-development",
];

/// The required skills whose `<name>/SKILL.md` is not a file.
fn missing_skills(skills: &Path) -> Vec<&'static str> {
    REQUIRED_SKILLS
        .into_iter()
        .filter(|name| !skills.join(name).join("SKILL.md").is_file())
        .collect()
}

/// Splits `6.4.2` into numbers so versions compare in order. Non-numeric parts count as 0.
fn version_key(name: &str) -> Vec<u64> {
    name.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

/// The newest `superpowers/<version>/skills` directory under the plugin cache, with its version.
fn newest_skills(home: &Path) -> Option<(String, PathBuf)> {
    let cache = home.join(".claude/plugins/cache");
    let mut found: Vec<(String, PathBuf)> = Vec::new();
    for vendor in std::fs::read_dir(cache).ok()?.flatten() {
        let Ok(versions) = std::fs::read_dir(vendor.path().join("superpowers")) else {
            continue;
        };
        for version in versions.flatten() {
            let skills = version.path().join("skills");
            if skills.is_dir() {
                found.push((version.file_name().to_string_lossy().into_owned(), skills));
            }
        }
    }
    found.into_iter().max_by_key(|(v, _)| version_key(v))
}

/// Creates `<skills>/<name>/SKILL.md`.
fn add_skill(skills: &Path, name: &str) {
    let dir = skills.join(name);
    std::fs::create_dir(&dir).expect("skill dir");
    std::fs::write(dir.join("SKILL.md"), "x").expect("skill file");
}

#[test]
fn skill_names_check_passes_with_no_plugin_installed() {
    let home = tempfile::tempdir().expect("temp home");
    assert!(newest_skills(home.path()).is_none());
}

#[test]
fn a_missing_skill_folder_is_named() {
    let skills = tempfile::tempdir().expect("temp skills");
    for name in ["brainstorming", "writing-plans"] {
        add_skill(skills.path(), name);
    }
    assert_eq!(
        missing_skills(skills.path()),
        vec!["subagent-driven-development"]
    );
}

#[test]
fn all_three_skill_folders_pass() {
    let skills = tempfile::tempdir().expect("temp skills");
    for name in REQUIRED_SKILLS {
        add_skill(skills.path(), name);
    }
    assert!(missing_skills(skills.path()).is_empty());
}

#[test]
fn installed_skills_still_have_what_the_commands_rely_on() {
    let Some(home) = std::env::var_os("HOME") else {
        println!("skill drift: HOME is not set, skipped");
        return;
    };
    let Some((version, skills)) = newest_skills(Path::new(&home)) else {
        println!("skill drift: no superpowers install found, skipped");
        return;
    };
    println!("skill drift: checked superpowers {version}");
    let missing = missing_skills(&skills);
    assert!(
        missing.is_empty(),
        "superpowers {version} lacks skills the commands name: {missing:?}"
    );
}
