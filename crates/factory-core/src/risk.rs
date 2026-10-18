use crate::config::Config;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffClass {
    DocOnly,
    Normal,
    Risky,
}

/// Matches a `/`-separated path against a glob. `*` and `?` stay inside one segment, `**` spans zero or more segments.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    let p: Vec<&str> = pattern.split('/').collect();
    let s: Vec<&str> = path.split('/').collect();
    match_segments(&p, &s)
}

fn match_segments(p: &[&str], s: &[&str]) -> bool {
    match p.split_first() {
        None => s.is_empty(),
        // A trailing `**` needs at least one segment, so `a/**` does not match `a` itself.
        Some((seg, rest)) if *seg == "**" && rest.is_empty() => !s.is_empty(),
        Some((seg, rest)) if *seg == "**" => (0..=s.len()).any(|i| match_segments(rest, &s[i..])),
        Some((seg, rest)) => match s.split_first() {
            Some((first, tail)) => {
                let pc: Vec<char> = seg.chars().collect();
                let sc: Vec<char> = first.chars().collect();
                match_segment(&pc, &sc) && match_segments(rest, tail)
            }
            None => false,
        },
    }
}

fn match_segment(p: &[char], s: &[char]) -> bool {
    match p.split_first() {
        None => s.is_empty(),
        Some(('*', rest)) => (0..=s.len()).any(|i| match_segment(rest, &s[i..])),
        Some(('?', rest)) => !s.is_empty() && match_segment(rest, &s[1..]),
        Some((c, rest)) => s.first() == Some(c) && match_segment(rest, &s[1..]),
    }
}

pub(crate) fn normalize(path: &str) -> String {
    let mut p = path.trim();
    while let Some(rest) = p.strip_prefix("./") {
        p = rest;
    }
    p.to_string()
}

fn has_parent_segment(path: &str) -> bool {
    path.split('/').any(|seg| seg == "..")
}

pub(crate) fn is_doc_path(path: &str) -> bool {
    path.ends_with(".md") || path.starts_with("docs/") || path.starts_with(".claude/")
}

/// Risk is checked first and ignores case. A path with a `..` segment is treated as risky.
pub fn classify_paths<S: AsRef<str>>(paths: &[S], config: &Config) -> DiffClass {
    if paths.is_empty() {
        return DiffClass::Normal;
    }
    let patterns: Vec<String> = config.risk_paths.iter().map(|p| p.to_lowercase()).collect();
    let mut all_docs = true;
    for raw in paths {
        let path = normalize(raw.as_ref());
        let lower = path.to_lowercase();
        if has_parent_segment(&path) || patterns.iter().any(|p| glob_match(p, &lower)) {
            return DiffClass::Risky;
        }
        if !is_doc_path(&path) {
            all_docs = false;
        }
    }
    if all_docs {
        DiffClass::DocOnly
    } else {
        DiffClass::Normal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn class(paths: &[&str]) -> DiffClass {
        classify_paths(paths, &Config::default())
    }

    #[test]
    fn glob_basics() {
        assert!(glob_match("src-tauri/**", "src-tauri/src/lib.rs"));
        assert!(glob_match("**/Cargo.toml", "Cargo.toml"));
        assert!(glob_match("**/Cargo.toml", "crates/a/Cargo.toml"));
        assert!(glob_match("crates/*/**", "crates/rocket-http/src/lib.rs"));
        assert!(!glob_match("crates/*/**", "crates/lonely"));
        assert!(glob_match("tools/factory/**", "tools/factory/Cargo.toml"));
        assert!(!glob_match("tools/factory/**", "tools/factory-server/Cargo.toml"));
        assert!(glob_match("src/**/*.tsx", "src/a/b/C.tsx"));
        assert!(!glob_match("src/*.tsx", "src/a/C.tsx"));
        assert!(glob_match("a?c", "abc"));
    }

    #[test]
    fn doc_only_normal_and_risky() {
        assert_eq!(class(&["README.md", "docs/a.txt", ".claude/software-factory.md"]), DiffClass::DocOnly);
        assert_eq!(class(&["crates/rocket-http/src/executor.rs"]), DiffClass::Normal);
        assert_eq!(class(&["src/components/Foo.tsx", "docs/a.md"]), DiffClass::Normal);
        assert_eq!(class(&["src-tauri/src/lib.rs"]), DiffClass::Risky);
        assert_eq!(class(&["docs/a.md", "package.json"]), DiffClass::Risky);
        assert_eq!(class(&["tools/factory/crates/factory-core/Cargo.toml"]), DiffClass::Risky);
        assert_eq!(class(&[".claude/settings.json"]), DiffClass::Risky);
        assert_eq!(class(&[".claude/harness/hooks/no-unwrap.py"]), DiffClass::Risky);
        assert_eq!(class(&["src/lib/auth-utils.ts"]), DiffClass::Risky);
        assert_eq!(class(&["crates/rocket-infra/src/fs_collection.rs"]), DiffClass::Risky);
    }

    #[test]
    fn empty_diff_is_normal() {
        let none: [&str; 0] = [];
        assert_eq!(classify_paths(&none, &Config::default()), DiffClass::Normal);
    }

    #[test]
    fn spelling_tricks_are_still_risky() {
        assert_eq!(class(&["./src-tauri/src/lib.rs"]), DiffClass::Risky);
        assert_eq!(class(&["SRC-TAURI/src/lib.rs"]), DiffClass::Risky);
        assert_eq!(class(&["web/Cargo.TOML"]), DiffClass::Risky);
        assert_eq!(class(&["docs/../src-tauri/src/lib.rs"]), DiffClass::Risky);
        assert_eq!(class(&["docs/../outside.md"]), DiffClass::Risky);
    }

    #[test]
    fn a_degraded_config_makes_every_diff_risky() {
        let cfg = Config::fail_closed();
        assert_eq!(classify_paths(&["docs/a.md"], &cfg), DiffClass::Risky);
        assert_eq!(classify_paths(&["README.md"], &cfg), DiffClass::Risky);
    }
}
