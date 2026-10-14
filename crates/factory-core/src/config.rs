use crate::error::CoreError;
use crate::risk::glob_match;
use serde::Deserialize;
use std::path::Path;

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
    pub commit_model: String,
    pub commit_subagents: bool,
    /// Per-run cap on weighted tokens for one committer run.
    pub commit_budget_weighted: u64,
    /// The Rocket knowledge base folder. `None` turns the KB read side and refresh off.
    pub kb_path: Option<String>,
    pub kb_model: String,
    /// Per-run cap on weighted tokens for one KB refresh run.
    pub kb_budget_weighted: u64,
    /// Whether the KB refresh commits its edits in the KB folder.
    pub kb_auto_commit: bool,
    pub checks: Vec<CheckRule>,
    /// True only for a fail-closed config. Never read from the file.
    #[serde(skip)]
    pub degraded: bool,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            risk_paths: DEFAULT_RISK.iter().map(|s| s.to_string()).collect(),
            diff_cap_lines: 4000,
            budget_weighted: 3_000_000,
            fix_rounds: 2,
            stall_minutes: 10,
            builder_model: "sonnet".to_string(),
            reviewer_model: "sonnet".to_string(),
            commit_model: "sonnet".to_string(),
            commit_subagents: false,
            commit_budget_weighted: 60_000,
            kb_path: None,
            kb_model: "sonnet".to_string(),
            kb_budget_weighted: 150_000,
            kb_auto_commit: false,
            checks: Vec::new(),
            degraded: false,
        }
    }
}

impl Config {
    /// Every path matches `**`, so every diff is risky and nothing auto-merges.
    pub fn fail_closed() -> Config {
        Config { risk_paths: vec!["**".to_string()], degraded: true, ..Config::default() }
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
        if cfg.kb_path.as_deref().is_some_and(|p| p.trim().is_empty()) {
            return Err(CoreError::Config("kb_path must not be empty when it is set".into()));
        }
        Ok(cfg)
    }

    pub fn try_load(path: &Path) -> Result<Config, CoreError> {
        let text = std::fs::read_to_string(path).map_err(|e| CoreError::io(path, e))?;
        Config::parse(&text)
    }

    /// Never fails. A missing or invalid file yields the fail-closed config.
    pub fn load(path: &Path) -> Config {
        match Config::try_load(path) {
            Ok(cfg) => cfg,
            Err(_) => Config::fail_closed(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_gives_defaults_and_is_not_degraded() {
        let cfg = Config::parse("").expect("empty is valid");
        assert!(!cfg.degraded);
        assert_eq!(cfg.diff_cap_lines, 4000);
        assert_eq!(cfg.budget_weighted, 3_000_000);
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
        let good = dir.path().join("good.toml");
        std::fs::write(&good, "fix_rounds = 3").expect("write");
        let cfg = Config::load(&good);
        assert!(!cfg.degraded);
        assert_eq!(cfg.fix_rounds, 3);
    }

    #[test]
    fn shipped_config_loads_and_maps_the_expected_paths() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../../.claude/factory/config.toml");
        let cfg = Config::try_load(std::path::Path::new(path)).expect("shipped config is valid");
        assert!(!cfg.degraded);
        let cmds = |p: &str| -> Vec<String> {
            cfg.checks.iter().filter(|c| c.matches(p)).flat_map(|c| c.commands.clone()).collect()
        };
        assert!(cmds("crates/rocket-http/src/lib.rs").contains(&"cargo check -j4 -p {crate}".to_string()));
        assert!(cmds("crates/rocket-http/src/lib.rs").contains(&"cargo test -j4 -p {crate}".to_string()));
        assert!(cmds("src/App.tsx").contains(&"yarn tsc --noEmit".to_string()));
        assert!(cmds("src/App.tsx").contains(&"yarn check".to_string()));
        assert!(cmds("src/App.tsx").contains(&"yarn test".to_string()));
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
