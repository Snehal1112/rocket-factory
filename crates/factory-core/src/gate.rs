//! Pure helpers for the no-checks and no-tests gates.
use crate::config::Config;
use crate::risk::{glob_match, is_doc_path, normalize};

const DEFAULT_TEST_PATHS: &[&str] = &[
    "**/tests/**",
    "**/__tests__/**",
    "**/*.test.ts",
    "**/*.test.tsx",
    "**/*.spec.ts",
    "**/*_test.rs",
    "**/*_test.go",
];

const DEFAULT_TEST_MARKERS: &[&str] = &[
    "#[test]",
    "#[tokio::test]",
    "#[cfg(test)]",
    "describe(",
    "it(",
    "test(",
    "func Test",
];

fn matches_any(patterns: &[String], defaults: &[&str], path: &str) -> bool {
    let lower = normalize(path).to_lowercase();
    if patterns.is_empty() {
        defaults.iter().any(|p| glob_match(p, &lower))
    } else {
        patterns
            .iter()
            .any(|p| glob_match(&p.to_lowercase(), &lower))
    }
}

/// The paths that count as code, in input order. An empty `code_paths` means every path that is not a doc path.
pub fn code_paths_of<S: AsRef<str>>(paths: &[S], config: &Config) -> Vec<String> {
    paths
        .iter()
        .map(|p| p.as_ref())
        .filter(|p| {
            if config.code_paths.is_empty() {
                !is_doc_path(&normalize(p))
            } else {
                matches_any(&config.code_paths, &[], p)
            }
        })
        .map(|p| p.to_string())
        .collect()
}

/// The code paths that no `[[checks]]` rule maps and that `unchecked_paths` does not exempt.
pub fn uncovered_code_paths(paths: &[String], config: &Config) -> Vec<String> {
    code_paths_of(paths, config)
        .into_iter()
        .filter(|p| !config.checks.iter().any(|r| r.matches(p)))
        .filter(|p| !matches_any(&config.unchecked_paths, &[], p))
        .collect()
}

/// True when one path is a test path. An empty `test_paths` means the built-in list.
pub fn has_test_path<S: AsRef<str>>(paths: &[S], config: &Config) -> bool {
    paths
        .iter()
        .any(|p| matches_any(&config.test_paths, DEFAULT_TEST_PATHS, p.as_ref()))
}

/// True when an added diff line holds a test marker. File headers, removed and context lines are ignored.
pub fn has_test_marker(diff_text: &str, config: &Config) -> bool {
    diff_text
        .lines()
        .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
        .any(|l| {
            if config.test_markers.is_empty() {
                DEFAULT_TEST_MARKERS.iter().any(|m| l.contains(m))
            } else {
                config.test_markers.iter().any(|m| l.contains(m.as_str()))
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CheckRule;

    fn cfg() -> Config {
        Config::default()
    }

    #[test]
    fn docs_only_paths_are_not_code_by_default() {
        let paths = ["README.md", "docs/a.md", ".claude/x.toml"];
        assert!(code_paths_of(&paths, &cfg()).is_empty());
    }

    #[test]
    fn a_source_file_is_code_by_default() {
        let paths = ["crates/a/src/lib.rs", "docs/a.md"];
        assert_eq!(
            code_paths_of(&paths, &cfg()),
            vec!["crates/a/src/lib.rs".to_string()]
        );
    }

    #[test]
    fn code_paths_from_config_replace_the_default() {
        let mut c = cfg();
        c.code_paths = vec!["src/**".into()];
        let paths = ["docs/a.md", "build.rs", "src/a.ts"];
        assert_eq!(code_paths_of(&paths, &c), vec!["src/a.ts".to_string()]);
    }

    #[test]
    fn an_empty_code_paths_list_means_the_default() {
        let mut c = cfg();
        c.code_paths = Vec::new();
        assert_eq!(code_paths_of(&["a.rs"], &c), vec!["a.rs".to_string()]);
    }

    #[test]
    fn a_degraded_config_counts_every_path_as_code() {
        let c = Config::fail_closed();
        assert_eq!(
            code_paths_of(&["docs/a.md"], &c),
            vec!["docs/a.md".to_string()]
        );
        assert!(!c.require_tests);
    }

    #[test]
    fn test_paths_default_covers_the_common_layouts() {
        for p in [
            "crates/a/tests/x.rs",
            "src/a/__tests__/b.ts",
            "src/a.test.tsx",
            "src/a.spec.ts",
            "crates/a/src/x_test.rs",
            "pkg/x_test.go",
        ] {
            assert!(has_test_path(&[p], &cfg()), "{p}");
        }
        assert!(!has_test_path(&["crates/a/src/lib.rs"], &cfg()));
    }

    #[test]
    fn an_empty_test_paths_list_means_the_default() {
        let mut c = cfg();
        c.test_paths = Vec::new();
        assert!(has_test_path(&["a/tests/b.rs"], &c));
    }

    #[test]
    fn test_paths_from_config_replace_the_default() {
        let mut c = cfg();
        c.test_paths = vec!["spec/**".into()];
        assert!(has_test_path(&["spec/a.rb"], &c));
        assert!(!has_test_path(&["crates/a/tests/x.rs"], &c));
    }

    #[test]
    fn paths_match_ignoring_case_and_a_leading_dot_slash() {
        assert!(has_test_path(&["./Crates/A/Tests/X.rs"], &cfg()));
        assert_eq!(
            code_paths_of(&["./Src/A.RS"], &cfg()),
            vec!["./Src/A.RS".to_string()]
        );
    }

    #[test]
    fn an_added_test_marker_line_counts_only_when_added() {
        let added = "+++ b/src/a.rs\n@@ -1,1 +1,3 @@\n+    #[test]\n+    fn x() {}\n";
        assert!(has_test_marker(added, &cfg()));
        assert!(has_test_marker("+  it(\"works\", () => {});\n", &cfg()));
        assert!(has_test_marker("+func TestThing(t *testing.T) {\n", &cfg()));
        assert!(!has_test_marker("-    #[test]\n", &cfg()), "a removed line");
        assert!(!has_test_marker(" #[test]\n", &cfg()), "a context line");
        assert!(
            !has_test_marker("+++ b/tests/it(.rs\n", &cfg()),
            "a file header"
        );
    }

    #[test]
    fn an_empty_test_markers_list_means_the_default() {
        let mut c = cfg();
        c.test_markers = Vec::new();
        assert!(has_test_marker("+#[cfg(test)]\n", &c));
    }

    #[test]
    fn test_markers_from_config_replace_the_default() {
        let mut c = cfg();
        c.test_markers = vec!["@Test".into()];
        assert!(has_test_marker("+@Test\n", &c));
        assert!(!has_test_marker("+#[test]\n", &c));
    }

    fn rule(glob: &str) -> CheckRule {
        CheckRule {
            paths: vec![glob.into()],
            commands: vec!["true".into()],
        }
    }

    fn strings(p: &[&str]) -> Vec<String> {
        p.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn uncovered_paths_skip_rule_matches() {
        let mut c = cfg();
        c.checks = vec![rule("src/**")];
        let got = uncovered_code_paths(&strings(&["src/a.rs", "Cargo.lock"]), &c);
        assert_eq!(got, strings(&["Cargo.lock"]));
    }

    #[test]
    fn unchecked_paths_are_exempt() {
        let mut c = cfg();
        c.unchecked_paths = vec!["LICENSE".into(), ".gitignore".into()];
        let got = uncovered_code_paths(&strings(&["LICENSE", ".gitignore", "a.rs"]), &c);
        assert_eq!(got, strings(&["a.rs"]));
    }

    #[test]
    fn doc_paths_are_never_uncovered() {
        let got = uncovered_code_paths(&strings(&["README.md", "docs/a.txt"]), &cfg());
        assert!(got.is_empty(), "{got:?}");
    }
}
