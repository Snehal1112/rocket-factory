use crate::error::CoreError;
use crate::risk::glob_match;
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Default per-job budget in weighted tokens. A config file can set its own.
pub const DEFAULT_JOB_BUDGET_WEIGHTED: u64 = 1_000_000;

/// Path-based risk list from spec decision 6. Matching ignores case.
const DEFAULT_RISK: &[&str] = &[
    "src-tauri/**",
    "**/Cargo.toml",
    "**/Cargo.lock",
    "**/package.json",
    "**/tauri.conf.json",
    "**/*auth*",
    "**/*secret*",
    "**/*trust*",
    "**/*csp*",
    "crates/rocket-infra/**",
    "crates/rocket-shared/src/event*",
    "crates/rocket-shared/src/events/**",
    ".claude/settings.json",
    ".claude/settings.local.json",
    ".claude/harness/**",
    ".claude/factory/hooks/**",
    ".claude/factory/config.toml",
];

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckRule {
    pub paths: Vec<String>,
    pub commands: Vec<String>,
}

impl CheckRule {
    pub fn matches(&self, path: &str) -> bool {
        self.paths.iter().any(|p| glob_match(p, path))
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub risk_paths: Vec<String>,
    pub diff_cap_lines: u32,
    pub budget_weighted: u64,
    pub fix_rounds: u32,
    pub stall_minutes: u32,
    pub builder_model: String,
    pub reviewer_model: String,
    /// A stronger reviewer model for risky or large diffs. `None` turns the switch off.
    pub reviewer_model_large: Option<String>,
    /// A round diff with more changed lines than this uses `reviewer_model_large`.
    pub reviewer_large_over_lines: u32,
    pub commit_model: String,
    pub commit_subagents: bool,
    /// Per-run cap on weighted tokens for one committer run.
    pub commit_budget_weighted: u64,
    /// Run one committer per job after the last task instead of one per task.
    pub commit_batch: bool,
    /// Give each task builder only its own task instead of the whole plan and spec.
    pub builder_excerpt: bool,
    /// After a merge, file the follow-up tickets and close the parent tickets. Off by default.
    pub tickets_on_merge: bool,
    /// The Rocket knowledge base folder. `None` turns the KB read side and refresh off.
    pub kb_path: Option<String>,
    pub kb_model: String,
    /// Per-run cap on weighted tokens for one KB refresh run.
    pub kb_budget_weighted: u64,
    /// Whether the KB refresh commits its edits in the KB folder.
    pub kb_auto_commit: bool,
    /// A plan under this many bytes is a small job.
    pub size_small_max_bytes: u64,
    /// A plan up to this many bytes is a medium job, a bigger one is complex.
    pub size_medium_max_bytes: u64,
    /// The branch jobs start from and merge into.
    pub base_branch: String,
    /// Files or folders the builder reads before it starts, such as `CLAUDE.md`.
    pub read_first: Vec<String>,
    /// Rules added to the builder prompt, one sentence each.
    pub builder_rules: Vec<String>,
    /// Example check commands named to the builder, one command each.
    pub check_hints: Vec<String>,
    /// Rules added to the reviewer prompt, one sentence each.
    pub reviewer_rules: Vec<String>,
    /// Rules for the plans the `/factory` command writes. The driver never puts them in a prompt.
    pub plan_rules: Vec<String>,
    pub checks: Vec<CheckRule>,
    /// Globs of the paths that count as code. Empty means every path that is not a doc path.
    pub code_paths: Vec<String>,
    /// Globs of the test paths. Empty means the built-in list.
    pub test_paths: Vec<String>,
    /// Substrings of an added line that mark a test. Empty means the built-in list.
    pub test_markers: Vec<String>,
    /// Park a code diff that adds no test. Off by default.
    pub require_tests: bool,
    /// Prompt cache lifetime for every agent run, `5m` or `1h`. `None` leaves the choice to `claude`.
    pub cache_ttl: Option<String>,
    /// File log level: error, warn, info, debug or trace. `None` means info.
    pub log_level: Option<String>,
    /// True only for a fail-closed config. Never read from the file.
    #[serde(skip)]
    pub degraded: bool,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            risk_paths: DEFAULT_RISK.iter().map(|s| s.to_string()).collect(),
            diff_cap_lines: 4000,
            budget_weighted: DEFAULT_JOB_BUDGET_WEIGHTED,
            fix_rounds: 2,
            stall_minutes: 10,
            builder_model: "sonnet".to_string(),
            reviewer_model: "sonnet".to_string(),
            reviewer_model_large: None,
            reviewer_large_over_lines: 800,
            commit_model: "sonnet".to_string(),
            commit_subagents: false,
            commit_budget_weighted: 60_000,
            commit_batch: true,
            builder_excerpt: true,
            tickets_on_merge: false,
            kb_path: None,
            kb_model: "sonnet".to_string(),
            kb_budget_weighted: 150_000,
            kb_auto_commit: false,
            size_small_max_bytes: 12_288,
            size_medium_max_bytes: 25_600,
            base_branch: "main".to_string(),
            read_first: Vec::new(),
            builder_rules: Vec::new(),
            check_hints: Vec::new(),
            reviewer_rules: Vec::new(),
            plan_rules: Vec::new(),
            checks: Vec::new(),
            code_paths: Vec::new(),
            test_paths: Vec::new(),
            test_markers: Vec::new(),
            require_tests: false,
            cache_ttl: None,
            log_level: None,
            degraded: false,
        }
    }
}

impl Config {
    /// The file log level, `info` when unset.
    pub fn log_level_or_default(&self) -> &str {
        self.log_level.as_deref().unwrap_or("info")
    }

    /// Every path matches `**`, so every diff is risky and nothing auto-merges.
    pub fn fail_closed() -> Config {
        Config { risk_paths: vec!["**".to_string()], code_paths: vec!["**".to_string()], degraded: true, ..Config::default() }
    }

    pub fn parse(text: &str) -> Result<Config, CoreError> {
        let cfg: Config = toml::from_str(text).map_err(|e| CoreError::Config(e.to_string()))?;
        if cfg.diff_cap_lines == 0 || cfg.budget_weighted == 0 {
            return Err(CoreError::Config("diff_cap_lines and budget_weighted must be above zero".into()));
        }
        if cfg.commit_budget_weighted == 0 || cfg.kb_budget_weighted == 0 {
            return Err(CoreError::Config("commit_budget_weighted and kb_budget_weighted must be above zero".into()));
        }
        let models = [&cfg.builder_model, &cfg.reviewer_model, &cfg.commit_model, &cfg.kb_model];
        if models.iter().any(|m| m.trim().is_empty()) {
            return Err(CoreError::Config("model names must not be empty".into()));
        }
        if cfg.reviewer_large_over_lines == 0 {
            return Err(CoreError::Config("reviewer_large_over_lines must be above zero".into()));
        }
        if cfg.reviewer_model_large.as_deref().is_some_and(|m| m.trim().is_empty()) {
            return Err(CoreError::Config("reviewer_model_large must not be empty when it is set".into()));
        }
        if cfg.size_small_max_bytes == 0 || cfg.size_small_max_bytes >= cfg.size_medium_max_bytes {
            return Err(CoreError::Config("size_small_max_bytes must be above zero and below size_medium_max_bytes".into()));
        }
        if cfg.kb_path.as_deref().is_some_and(|p| p.trim().is_empty()) {
            return Err(CoreError::Config("kb_path must not be empty when it is set".into()));
        }
        if cfg.cache_ttl.as_deref().is_some_and(|t| t != "5m" && t != "1h") {
            return Err(CoreError::Config("cache_ttl must be \"5m\" or \"1h\" when it is set".into()));
        }
        if cfg.log_level.as_deref().is_some_and(|l| !["error", "warn", "info", "debug", "trace"].contains(&l)) {
            return Err(CoreError::Config("log_level must be one of error, warn, info, debug, trace when it is set".into()));
        }
        if let Err(why) = check_branch_name(&cfg.base_branch) {
            return Err(CoreError::Config(format!("base_branch: {why}")));
        }
        for (key, lines) in [
            ("read_first", &cfg.read_first),
            ("builder_rules", &cfg.builder_rules),
            ("check_hints", &cfg.check_hints),
            ("reviewer_rules", &cfg.reviewer_rules),
            ("plan_rules", &cfg.plan_rules),
        ] {
            if lines.iter().any(|l| l.trim().is_empty() || l.contains('\n') || l.contains('\r')) {
                return Err(CoreError::Config(format!("{key}: every line must be one non-empty line")));
            }
        }
        Ok(cfg)
    }

    pub fn try_load(path: &Path) -> Result<Config, CoreError> {
        let text = std::fs::read_to_string(path).map_err(|e| CoreError::io(path, e))?;
        Config::parse(&text)
    }

    /// The KB folder with a leading `~` expanded. `None` when `kb_path` is unset, or when `~` is used and `HOME` is unset or empty.
    /// It does not check that the folder exists.
    pub fn kb_dir(&self) -> Option<PathBuf> {
        let path = self.kb_path.as_deref()?;
        expand_home(path, std::env::var("HOME").ok().as_deref())
    }

    /// Never fails. A missing or invalid file yields the fail-closed config.
    pub fn load(path: &Path) -> Config {
        match Config::try_load(path) {
            Ok(cfg) => cfg,
            Err(_) => Config::fail_closed(),
        }
    }
}

/// Expands a leading `~/` or a lone `~` with `home`. Any other `~` stays as it is.
fn expand_home(path: &str, home: Option<&str>) -> Option<PathBuf> {
    let rest = if path == "~" {
        ""
    } else if let Some(rest) = path.strip_prefix("~/") {
        rest
    } else {
        return Some(PathBuf::from(path));
    };
    let home = home.filter(|h| !h.is_empty())?;
    Some(if rest.is_empty() { PathBuf::from(home) } else { Path::new(home).join(rest) })
}

/// A branch name git accepts, and not one of the factory's own job branches.
fn check_branch_name(name: &str) -> Result<(), &'static str> {
    if name.trim().is_empty() {
        return Err("must not be empty");
    }
    if name.starts_with('-') || name.starts_with('/') || name.ends_with('/') || name.ends_with(".lock") || name == "@" {
        return Err("is not a valid branch name");
    }
    if name.contains("..") || name.contains("@{") {
        return Err("is not a valid branch name");
    }
    if name.chars().any(|c| c.is_whitespace() || c.is_control() || "~^:?*[\\".contains(c)) {
        return Err("is not a valid branch name");
    }
    if name.starts_with("factory/") {
        return Err("must not start with factory/, that is the job branch prefix");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_config_without_the_new_keys_parses_with_defaults() {
        let cfg = Config::parse("diff_cap_lines = 100\n").expect("parses");
        assert!(cfg.code_paths.is_empty() && cfg.test_paths.is_empty() && cfg.test_markers.is_empty());
        assert!(!cfg.require_tests);
    }

    #[test]
    fn the_new_keys_parse() {
        let cfg = Config::parse(
            "code_paths = [\"src/**\"]\ntest_paths = [\"spec/**\"]\ntest_markers = [\"@Test\"]\nrequire_tests = true\n",
        )
        .expect("parses");
        assert_eq!(cfg.code_paths, vec!["src/**".to_string()]);
        assert_eq!(cfg.test_paths, vec!["spec/**".to_string()]);
        assert_eq!(cfg.test_markers, vec!["@Test".to_string()]);
        assert!(cfg.require_tests);
    }

    #[test]
    fn fail_closed_makes_every_path_code() {
        let cfg = Config::fail_closed();
        assert_eq!(cfg.code_paths, vec!["**".to_string()]);
        assert!(cfg.test_paths.is_empty() && !cfg.require_tests);
    }

    use super::*;

    #[test]
    fn cache_ttl_is_unset_by_default_and_takes_only_5m_or_1h() {
        assert_eq!(Config::default().cache_ttl, None);
        assert_eq!(Config::parse("cache_ttl = \"5m\"").expect("parse").cache_ttl.as_deref(), Some("5m"));
        assert_eq!(Config::parse("cache_ttl = \"1h\"").expect("parse").cache_ttl.as_deref(), Some("1h"));
        assert!(Config::parse("cache_ttl = \"10m\"").is_err());
        assert!(Config::parse("cache_ttl = \"\"").is_err());
        assert!(Config::parse("cache_ttl = 5").is_err());
    }

    #[test]
    fn log_level_unset_by_default_and_takes_only_known_values() {
        assert_eq!(Config::default().log_level, None);
        assert_eq!(Config::default().log_level_or_default(), "info");
        assert_eq!(Config::parse("log_level = \"debug\"").expect("parse").log_level_or_default(), "debug");
        let err = Config::parse("log_level = \"loud\"").expect_err("bad level");
        assert!(err.to_string().contains("log_level must be one of error, warn, info, debug, trace"));
    }

    #[test]
    fn tickets_on_merge_is_off_by_default_and_can_be_turned_on() {
        assert!(!Config::default().tickets_on_merge);
        assert!(Config::parse("tickets_on_merge = true").expect("parse").tickets_on_merge);
        assert!(Config::parse("tickets_on_merge = \"yes\"").is_err());
    }

    #[test]
    fn the_large_reviewer_model_is_off_by_default() {
        let cfg = Config::default();
        assert_eq!(cfg.reviewer_model_large, None);
        assert_eq!(cfg.reviewer_large_over_lines, 800);
        assert_eq!(Config::parse("").expect("parse").reviewer_model_large, None);
    }

    #[test]
    fn reviewer_large_over_lines_rejects_zero() {
        assert!(Config::parse("reviewer_large_over_lines = 0").is_err());
        assert!(Config::parse("reviewer_large_over_lines = -1").is_err());
        assert!(Config::parse("reviewer_large_over_lines = 1").is_ok());
        assert!(Config::parse("reviewer_model_large = \" \"").is_err());
    }

    #[test]
    fn a_config_can_set_both_keys() {
        let cfg = Config::parse("reviewer_model_large = \"opus\"\nreviewer_large_over_lines = 500").expect("parse");
        assert_eq!(cfg.reviewer_model_large.as_deref(), Some("opus"));
        assert_eq!(cfg.reviewer_large_over_lines, 500);
    }

    #[test]
    fn cost_switches_default_on_and_can_be_turned_off() {
        let cfg = Config::default();
        assert!(cfg.commit_batch && cfg.builder_excerpt);
        let off = Config::parse("commit_batch = false\nbuilder_excerpt = false").expect("parse");
        assert!(!off.commit_batch && !off.builder_excerpt);
        assert!(Config::parse("commit_batch = \"yes\"").is_err());
        assert!(Config::parse("commit_bach = true").is_err());
    }

    #[test]
    fn prompt_keys_default_to_empty_and_parse() {
        let cfg = Config::default();
        assert!(cfg.read_first.is_empty() && cfg.builder_rules.is_empty());
        assert!(cfg.check_hints.is_empty() && cfg.reviewer_rules.is_empty());
        let cfg = Config::parse(
            "read_first = [\"CLAUDE.md\"]\nbuilder_rules = [\"a\", \"b\"]\ncheck_hints = [\"make test\"]\nreviewer_rules = [\"c\"]\n",
        )
        .expect("parses");
        assert_eq!(cfg.read_first, vec!["CLAUDE.md".to_string()]);
        assert_eq!(cfg.builder_rules.len(), 2);
        assert_eq!(cfg.check_hints, vec!["make test".to_string()]);
        assert_eq!(cfg.reviewer_rules, vec!["c".to_string()]);
    }

    #[test]
    fn plan_rules_default_to_empty_and_parse() {
        assert!(Config::default().plan_rules.is_empty());
        assert!(Config::parse("").expect("empty").plan_rules.is_empty());
        let cfg = Config::parse("plan_rules = [\"a\", \"b\"]\n").expect("parses");
        assert_eq!(cfg.plan_rules, vec!["a".to_string(), "b".to_string()]);
        assert!(!cfg.degraded);
        assert!(Config::parse("plan_rules = [\"a\\nb\"]\n").is_err());
        assert!(Config::parse("plan_rules = [\"\"]\n").is_err());
    }

    #[test]
    fn prompt_lines_must_be_one_line_and_not_empty() {
        for body in ["builder_rules = [\"\"]", "reviewer_rules = [\"a\\nb\"]", "read_first = [\"  \"]", "check_hints = [\"x\\ry\"]"] {
            assert!(Config::parse(&format!("{body}\n")).is_err(), "{body} must be refused");
        }
    }

    #[test]
    fn base_branch_defaults_to_main_and_survives_fail_closed() {
        assert_eq!(Config::default().base_branch, "main");
        assert_eq!(Config::parse("").expect("empty").base_branch, "main");
        assert_eq!(Config::fail_closed().base_branch, "main");
    }

    #[test]
    fn base_branch_accepts_ordinary_names() {
        for name in ["main", "master", "v-4.0.0", "release/2026.10", "dev_1"] {
            let cfg = Config::parse(&format!("base_branch = \"{name}\"\n")).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(cfg.base_branch, name);
        }
    }

    #[test]
    fn base_branch_rejects_unsafe_names() {
        for name in ["", "  ", "a b", "a..b", "-x", "x/", "/x", "x.lock", "a\\tb", "factory/001-x", "a~b", "a^b", "a:b", "a?b", "a*b", "a[b", "@", "a@{b"] {
            assert!(Config::parse(&format!("base_branch = \"{name}\"\n")).is_err(), "{name:?} must be refused");
        }
    }

    #[test]
    fn size_thresholds_default_and_validate() {
        let cfg = Config::default();
        assert_eq!(cfg.size_small_max_bytes, 12_288);
        assert_eq!(cfg.size_medium_max_bytes, 25_600);
        assert!(Config::parse("size_small_max_bytes = 100\nsize_medium_max_bytes = 200\n").is_ok());
        assert!(Config::parse("size_small_max_bytes = 200\nsize_medium_max_bytes = 200\n").is_err());
        assert!(Config::parse("size_small_max_bytes = 300\nsize_medium_max_bytes = 200\n").is_err());
        assert!(Config::parse("size_small_max_bytes = 0\n").is_err());
        assert!(Config::parse("size_smal_max_bytes = 1\n").is_err());
    }

    #[test]
    fn kb_dir_is_none_when_unset() {
        assert_eq!(Config::default().kb_dir(), None);
    }

    #[test]
    fn an_absolute_kb_path_is_kept() {
        let cfg = Config { kb_path: Some("/kb/rocketapi".into()), ..Config::default() };
        assert_eq!(cfg.kb_dir(), Some(PathBuf::from("/kb/rocketapi")));
    }

    #[test]
    fn expand_home_handles_tilde_forms() {
        assert_eq!(expand_home("~/kb", Some("/home/u")), Some(PathBuf::from("/home/u/kb")));
        assert_eq!(expand_home("~", Some("/home/u")), Some(PathBuf::from("/home/u")));
        assert_eq!(expand_home("~/kb", None), None);
        assert_eq!(expand_home("~/kb", Some("")), None);
        assert_eq!(expand_home("~", None), None);
        assert_eq!(expand_home("/a/~b", None), Some(PathBuf::from("/a/~b")));
        assert_eq!(expand_home("~b/kb", Some("/home/u")), Some(PathBuf::from("~b/kb")));
    }

    #[test]
    fn empty_file_gives_defaults_and_is_not_degraded() {
        let cfg = Config::parse("").expect("empty is valid");
        assert!(!cfg.degraded);
        assert_eq!(cfg.diff_cap_lines, 4000);
        assert_eq!(cfg.budget_weighted, 1_000_000);
        assert_eq!(cfg.fix_rounds, 2);
        assert_eq!(cfg.stall_minutes, 10);
        assert_eq!(cfg.builder_model, "sonnet");
        assert!(cfg.risk_paths.iter().any(|p| p == "src-tauri/**"));
        assert_eq!(cfg.commit_model, "sonnet");
        assert!(!cfg.commit_subagents);
        assert_eq!(cfg.commit_budget_weighted, 60_000);
        assert_eq!(cfg.kb_path, None);
        assert_eq!(cfg.kb_model, "sonnet");
        assert_eq!(cfg.kb_budget_weighted, 150_000);
        assert!(!cfg.kb_auto_commit);
    }

    #[test]
    fn keys_override_defaults_and_checks_parse() {
        let cfg = Config::parse(
            r#"
            diff_cap_lines = 100
            builder_model = "opus"
            risk_paths = ["x/**"]
            [[checks]]
            paths = ["src/**"]
            commands = ["yarn tsc --noEmit"]
            "#,
        )
        .expect("valid");
        assert_eq!(cfg.diff_cap_lines, 100);
        assert_eq!(cfg.builder_model, "opus");
        assert_eq!(cfg.risk_paths, vec!["x/**".to_string()]);
        assert_eq!(cfg.checks.len(), 1);
        assert!(cfg.checks[0].matches("src/a/b.ts"));
        assert!(!cfg.checks[0].matches("crates/a.rs"));
    }

    #[test]
    fn the_default_job_budget_is_one_million() {
        assert_eq!(DEFAULT_JOB_BUDGET_WEIGHTED, 1_000_000);
        assert_eq!(Config::default().budget_weighted, DEFAULT_JOB_BUDGET_WEIGHTED);
    }

    #[test]
    fn an_explicit_budget_is_kept() {
        let cfg = Config::parse("budget_weighted = 3000000").expect("valid");
        assert_eq!(cfg.budget_weighted, 3_000_000);
    }

    #[test]
    fn typos_and_bad_values_are_errors() {
        assert!(Config::parse("risk_path = [\"x\"]").is_err());
        assert!(Config::parse("diff_cap_lines = 0").is_err());
        assert!(Config::parse("budget_weighted = 0").is_err());
        assert!(Config::parse("builder_model = \"\"").is_err());
        assert!(Config::parse("this is = = not toml").is_err());
    }

    #[test]
    fn commit_and_kb_keys_override_defaults() {
        let cfg = Config::parse(
            r#"
            commit_model = "opus"
            commit_subagents = true
            commit_budget_weighted = 90000
            kb_path = "/kb/rocketapi"
            kb_model = "haiku"
            kb_budget_weighted = 40000
            kb_auto_commit = true
            "#,
        )
        .expect("valid");
        assert_eq!(cfg.commit_model, "opus");
        assert!(cfg.commit_subagents);
        assert_eq!(cfg.commit_budget_weighted, 90_000);
        assert_eq!(cfg.kb_path.as_deref(), Some("/kb/rocketapi"));
        assert_eq!(cfg.kb_model, "haiku");
        assert_eq!(cfg.kb_budget_weighted, 40_000);
        assert!(cfg.kb_auto_commit);
    }

    #[test]
    fn bad_commit_and_kb_values_are_errors() {
        assert!(Config::parse("commit_budget_weighted = 0").is_err());
        assert!(Config::parse("kb_budget_weighted = 0").is_err());
        assert!(Config::parse("commit_model = \"\"").is_err());
        assert!(Config::parse("kb_model = \"  \"").is_err());
        assert!(Config::parse("kb_path = \"\"").is_err());
        assert!(Config::parse("kb_pth = \"/x\"").is_err());
    }

    #[test]
    fn load_fails_closed_for_missing_and_invalid_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = Config::load(&dir.path().join("none.toml"));
        assert!(missing.degraded);
        let bad = dir.path().join("bad.toml");
        std::fs::write(&bad, "risk_path = 1").expect("write");
        assert!(Config::load(&bad).degraded);
        let bad_base = dir.path().join("bad_base.toml");
        std::fs::write(&bad_base, "base_branch = \"a b\"").expect("write");
        let degraded = Config::load(&bad_base);
        assert!(degraded.degraded);
        assert_eq!(degraded.risk_paths, vec!["**".to_string()]);
        let good = dir.path().join("good.toml");
        std::fs::write(&good, "fix_rounds = 3").expect("write");
        let cfg = Config::load(&good);
        assert!(!cfg.degraded);
        assert_eq!(cfg.fix_rounds, 3);
    }

    #[test]
    fn rockets_current_config_loads_with_no_problems() {
        // The fixture is a copy of Rocket's `.claude/factory/config.toml`, including `tickets_on_merge = true`.
        let text = include_str!("../tests/fixtures/rocket-config.toml");
        assert!(text.contains("tickets_on_merge = true") && text.contains("commit_budget_weighted"));
        let cfg = Config::parse(text).expect("rocket config parses");
        assert!(cfg.tickets_on_merge && !cfg.commit_subagents && !cfg.degraded);
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, text).expect("write");
        let loaded = Config::load(&path);
        assert!(!loaded.degraded, "{loaded:?}");
        assert!(loaded.tickets_on_merge);
        assert!(Config::parse(&format!("{text}\nnot_a_key = 1\n")).is_err(), "an unknown key still fails");
    }

    #[test]
    fn shipped_config_loads_and_maps_the_expected_paths() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../.claude/factory/config.toml");
        let cfg = Config::try_load(std::path::Path::new(path)).expect("shipped config is valid");
        assert!(!cfg.degraded);
        let cmds = |p: &str| -> Vec<String> {
            cfg.checks.iter().filter(|c| c.matches(p)).flat_map(|c| c.commands.clone()).collect()
        };
        assert!(cmds("crates/rocket-http/src/lib.rs").contains(&"cargo check -j4 -p {crate}".to_string()));
        assert!(cmds("crates/rocket-http/src/lib.rs").contains(&"cargo test -j4 -p {crate}".to_string()));
        assert!(cmds("src/App.tsx").contains(&"yarn tsc --noEmit".to_string()));
        assert!(cmds("src/App.tsx").contains(&"yarn check".to_string()));
        assert!(cmds("src/App.tsx").contains(&"yarn test --run --maxWorkers=50%".to_string()));
        assert!(cmds("tools/factory/crates/factory-core/src/lib.rs")
            .contains(&"cargo check -j4 --manifest-path tools/factory/Cargo.toml".to_string()));
        assert!(cmds("tools/factory-server/src/main.rs")
            .contains(&"cargo check -j4 --manifest-path tools/factory-server/Cargo.toml".to_string()));
        assert!(cmds("docs/a.md").is_empty());
        assert_eq!(cfg.commit_model, "sonnet");
        assert!(!cfg.commit_subagents);
        assert_eq!(cfg.commit_budget_weighted, 60_000);
        assert_eq!(cfg.kb_path, None);
    }
}
