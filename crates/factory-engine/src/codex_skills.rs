//! Finds the skills Codex would list and turns a role allowlist into `-c` values.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// One skill found in a skill root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundSkill {
    /// The subfolder name, empty for a top-level skill.
    pub group: String,
    /// The skill folder name.
    pub name: String,
    /// The canonical path of its `SKILL.md`.
    pub path: PathBuf,
}

/// Finds the skills under each root, in order. A missing or unusable entry is skipped.
pub fn discover_skills(roots: &[PathBuf]) -> Vec<FoundSkill> {
    let mut found: Vec<FoundSkill> = Vec::new();
    for root in roots {
        for (name, dir) in visible_dirs(root) {
            if let Some(path) = skill_file(&dir) {
                push_new(&mut found, "", &name, path);
                continue;
            }
            for (child, child_dir) in visible_dirs(&dir) {
                if let Some(path) = skill_file(&child_dir) {
                    push_new(&mut found, &name, &child, path);
                }
            }
        }
    }
    found
}

fn push_new(found: &mut Vec<FoundSkill>, group: &str, name: &str, path: PathBuf) {
    if found.iter().all(|skill| skill.path != path) {
        found.push(FoundSkill {
            group: group.to_string(),
            name: name.to_string(),
            path,
        });
    }
}

/// The child directories of `dir` (symlinks followed), sorted by name, without dot names.
fn visible_dirs(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut dirs: Vec<(String, PathBuf)> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().into_string().ok()?;
            let path = entry.path();
            (!name.starts_with('.') && path.is_dir()).then_some((name, path))
        })
        .collect();
    dirs.sort();
    dirs
}

/// The canonical `SKILL.md` of a skill folder, when it has one.
fn skill_file(dir: &Path) -> Option<PathBuf> {
    fs::canonicalize(dir.join("SKILL.md")).ok()
}

/// True when the allowlist names the skill's group, or its own name for a top-level skill.
pub fn allowed(skill: &FoundSkill, allow: &[String]) -> bool {
    let wanted = if skill.group.is_empty() {
        &skill.name
    } else {
        &skill.group
    };
    allow
        .iter()
        .any(|entry| entry.split('@').next() == Some(wanted.as_str()))
}

/// The `-c` values that limit Codex to the allowed skills.
pub fn codex_skill_args(found: &[FoundSkill], allow: &[String]) -> Vec<String> {
    let allowed_paths: BTreeSet<&Path> = found
        .iter()
        .filter(|skill| allowed(skill, allow))
        .map(|skill| skill.path.as_path())
        .collect();
    if allowed_paths.is_empty() {
        return vec!["skills.include_instructions=false".to_string()];
    }
    let disabled: BTreeSet<String> = found
        .iter()
        .filter(|skill| !allowed_paths.contains(skill.path.as_path()))
        .map(|skill| {
            format!(
                "{{path={},enabled=false}}",
                toml_string(&skill.path.to_string_lossy())
            )
        })
        .collect();
    let mut args = vec!["skills.bundled.enabled=false".to_string()];
    if !disabled.is_empty() {
        let items: Vec<String> = disabled.into_iter().collect();
        args.push(format!("skills.config=[{}]", items.join(",")));
    }
    args
}

/// A TOML basic string for `s`.
fn toml_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill_dir(root: &Path, rel: &str) {
        let dir = root.join(rel);
        fs::create_dir_all(&dir).expect("create skill dir");
        fs::write(dir.join("SKILL.md"), "---\nname: x\n---\n").expect("write skill");
    }

    fn found(group: &str, name: &str, path: &str) -> FoundSkill {
        FoundSkill {
            group: group.to_string(),
            name: name.to_string(),
            path: PathBuf::from(path),
        }
    }

    fn list(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn discovers_top_level_and_grouped_skills() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        skill_dir(root, "foo");
        skill_dir(root, "superpowers/brainstorming");
        skill_dir(root, ".system/imagegen");
        fs::create_dir_all(root.join("empty")).expect("create empty");
        let got = discover_skills(&[root.to_path_buf()]);
        let canon = |rel: &str| fs::canonicalize(root.join(rel)).expect("canonicalize");
        assert_eq!(
            got,
            vec![
                found("", "foo", canon("foo/SKILL.md").to_str().expect("utf8")),
                found(
                    "superpowers",
                    "brainstorming",
                    canon("superpowers/brainstorming/SKILL.md")
                        .to_str()
                        .expect("utf8")
                ),
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_roots_give_one_canonical_entry() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        skill_dir(root, "real/skills/brainstorming");
        skill_dir(root, "real/skills/debugging");
        for side in ["a", "b"] {
            fs::create_dir_all(root.join(side)).expect("create root");
            symlink(
                root.join("real/skills"),
                root.join(side).join("superpowers"),
            )
            .expect("symlink");
        }
        let got = discover_skills(&[root.join("a"), root.join("b")]);
        let names: Vec<&str> = got.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["brainstorming", "debugging"]);
        let expected = fs::canonicalize(root.join("real/skills/brainstorming/SKILL.md"))
            .expect("canonicalize");
        assert_eq!(got[0].path, expected);
        assert_eq!(got[0].group, "superpowers");
    }

    #[cfg(unix)]
    #[test]
    fn missing_root_and_broken_entries_are_skipped() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("root");
        fs::create_dir_all(root.join("no-skill-md")).expect("create folder");
        symlink(tmp.path().join("gone"), root.join("dangling")).expect("symlink");
        let got = discover_skills(&[tmp.path().join("missing"), root]);
        assert!(got.is_empty());
    }

    #[test]
    fn allowed_matches_group_or_name_and_drops_marketplace() {
        let grouped = found("superpowers", "brainstorming", "/p/b");
        let top = found("", "foo", "/p/f");
        assert!(allowed(&grouped, &list(&["superpowers@official"])));
        assert!(allowed(&top, &list(&["foo"])));
        assert!(!allowed(&grouped, &list(&["brainstorming"])));
        assert!(!allowed(&top, &list(&["superpowers"])));
    }

    #[test]
    fn empty_or_unmatched_allowlist_turns_instructions_off() {
        let skills = [found("", "foo", "/p/f")];
        let off = list(&["skills.include_instructions=false"]);
        assert_eq!(codex_skill_args(&skills, &[]), off);
        assert_eq!(codex_skill_args(&skills, &list(&["nothing"])), off);
    }

    #[test]
    fn allowlist_disables_every_other_canonical_path() {
        let skills = [
            found("", "foo", "/p/foo"),
            found("", "bar", "/p/bar"),
            found("superpowers", "x", "/p/x"),
        ];
        assert_eq!(
            codex_skill_args(&skills, &list(&["superpowers"])),
            list(&[
                "skills.bundled.enabled=false",
                "skills.config=[{path=\"/p/bar\",enabled=false},{path=\"/p/foo\",enabled=false}]"
            ])
        );
    }

    #[test]
    fn an_allowed_path_is_never_disabled() {
        let skills = [
            found("", "foo", "/p/same"),
            found("superpowers", "x", "/p/same"),
        ];
        assert_eq!(
            codex_skill_args(&skills, &list(&["superpowers"])),
            list(&["skills.bundled.enabled=false"])
        );
    }

    #[test]
    fn everything_allowed_has_no_skills_config() {
        let skills = [found("", "foo", "/p/foo")];
        assert_eq!(
            codex_skill_args(&skills, &list(&["foo"])),
            list(&["skills.bundled.enabled=false"])
        );
    }

    #[test]
    fn paths_are_toml_escaped() {
        for input in [
            "a\"b",
            "a\\b",
            "a\nb",
            "a\tb",
            "caf\u{e9}",
            "a\u{1}b",
            "a\rb",
        ] {
            let doc = format!("x = {}", toml_string(input));
            let table: toml::Table = doc.parse().expect("valid toml");
            assert_eq!(table["x"].as_str(), Some(input));
        }
    }
}
