//! Checks that the installed superpowers skills still have what the factory commands rely on.
//! It passes on a machine with no plugin installed.

use std::path::{Path, PathBuf};

/// The superpowers version the commands were written against.
const SKILLS_TESTED_AGAINST: &str = "7.0.0";

/// Splits `7.0.0` into numbers so versions compare in order. Non-numeric parts count as 0.
fn version_key(name: &str) -> Vec<u64> {
    name.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

/// The newest `superpowers/<version>/skills` directory under the plugin cache, with its version.
fn newest_skills(home: &Path) -> Option<(String, PathBuf)> {
    let cache = home.join(".claude/plugins/cache");
    let mut found: Vec<(String, PathBuf)> = Vec::new();
    for vendor in std::fs::read_dir(cache).ok()?.flatten() {
        let Ok(versions) = std::fs::read_dir(vendor.path().join("superpowers")) else { continue };
        for version in versions.flatten() {
            let skills = version.path().join("skills");
            if skills.is_dir() {
                found.push((version.file_name().to_string_lossy().into_owned(), skills));
            }
        }
    }
    found.into_iter().max_by_key(|(v, _)| version_key(v))
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
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
    if version != SKILLS_TESTED_AGAINST {
        println!("note: the commands were written against superpowers {SKILLS_TESTED_AGAINST}, found {version}");
    }
    assert!(skills.join("brainstorming/builder-check-prompt.md").is_file(), "builder-check-prompt.md is gone");
    let plans = read(&skills.join("writing-plans/SKILL.md"));
    assert!(plans.contains("Interfaces:"), "writing-plans no longer names Interfaces:");
    assert!(plans.contains("Review Focus"), "writing-plans no longer names Review Focus");
    assert!(skills.join("subagent-driven-development/SKILL.md").is_file(), "subagent-driven-development is gone");
    let prompt = read(&skills.join("subagent-driven-development/implementer-prompt.md"));
    assert!(prompt.contains("Status"), "implementer-prompt.md no longer asks for a Status");
}
