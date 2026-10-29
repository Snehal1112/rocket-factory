use crate::error::CoreError;
use crate::risk::glob_match;
use serde::{Deserialize, Serialize};
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

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
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

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
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
    /// The agent CLI for every run, `claude` or `codex`. `None` means `claude`.
    pub agent_cli: Option<String>,
    /// Let this project's Claude agents bill the API key from the environment. `Some(false)`
    /// always strips the key. `None` leaves the choice to `FACTORY_ALLOW_API_KEY`.
    pub allow_api_key: Option<bool>,
    /// A file that holds the Anthropic API key for this project only. Absolute or `~/`. The key
    /// itself is never stored in the config.
    pub api_key_file: Option<String>,
    /// File log level: error, warn, info, debug or trace. `None` means info.
    pub log_level: Option<String>,
    /// Give builder runs their own permission mode and tool allowlist. On by default.
    pub builder_permissions: bool,
    /// Extra `Bash(...)` allowlist entries for builder runs.
    pub builder_allow: Vec<String>,
    /// Resume the previous task's builder session for the next task of a plan. Off by default.
    pub builder_carry_session: bool,
    /// The largest end-of-task context, in tokens, that may be carried to the next task.
    pub builder_carry_max_ctx: u64,
    /// Claude Code plugins builder runs load, by name or `name@marketplace`. Every other plugin is off.
    pub builder_plugins: Vec<String>,
    /// Claude Code plugins reviewer runs load. Every other plugin is off.
    pub reviewer_plugins: Vec<String>,
    /// Claude Code plugins committer and message runs load. Every other plugin is off.
    pub commit_plugins: Vec<String>,
    /// Claude Code plugins KB refresh runs load. Every other plugin is off.
    pub kb_plugins: Vec<String>,
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
            agent_cli: None,
            allow_api_key: None,
            api_key_file: None,
            log_level: None,
            builder_permissions: true,
            builder_allow: Vec::new(),
            builder_carry_session: true,
            builder_carry_max_ctx: 45_000,
            builder_plugins: vec!["superpowers".to_string()],
            reviewer_plugins: Vec::new(),
            commit_plugins: vec!["dev-workflow-skills".to_string()],
            kb_plugins: Vec::new(),
            degraded: false,
        }
    }
}

impl Config {
    /// The agent CLI, `claude` when unset.
    pub fn agent_cli(&self) -> &str {
        self.agent_cli.as_deref().unwrap_or("claude")
    }

    /// The file log level, `info` when unset.
    pub fn log_level_or_default(&self) -> &str {
        self.log_level.as_deref().unwrap_or("info")
    }

    /// The plugins a run of `role` loads. A role without its own list loads none.
    pub fn plugins_for(&self, role: &str) -> &[String] {
        match role {
            "builder" => &self.builder_plugins,
            "reviewer" => &self.reviewer_plugins,
            "committer" | "message" => &self.commit_plugins,
            "kb" => &self.kb_plugins,
            _ => &[],
        }
    }

    /// Every path matches `**`, so every diff is risky and nothing auto-merges.
    pub fn fail_closed() -> Config {
        Config {
            risk_paths: vec!["**".to_string()],
            code_paths: vec!["**".to_string()],
            degraded: true,
            ..Config::default()
        }
    }

    pub fn parse(text: &str) -> Result<Config, CoreError> {
        let cfg: Config = toml::from_str(text).map_err(|e| CoreError::Config(e.to_string()))?;
        if cfg.diff_cap_lines == 0 || cfg.budget_weighted == 0 {
            return Err(CoreError::Config(
                "diff_cap_lines and budget_weighted must be above zero".into(),
            ));
        }
        if cfg.commit_budget_weighted == 0 || cfg.kb_budget_weighted == 0 {
            return Err(CoreError::Config(
                "commit_budget_weighted and kb_budget_weighted must be above zero".into(),
            ));
        }
        let models = [
            &cfg.builder_model,
            &cfg.reviewer_model,
            &cfg.commit_model,
            &cfg.kb_model,
        ];
        if models.iter().any(|m| m.trim().is_empty()) {
            return Err(CoreError::Config("model names must not be empty".into()));
        }
        if cfg.reviewer_large_over_lines == 0 {
            return Err(CoreError::Config(
                "reviewer_large_over_lines must be above zero".into(),
            ));
        }
        if cfg
            .reviewer_model_large
            .as_deref()
            .is_some_and(|m| m.trim().is_empty())
        {
            return Err(CoreError::Config(
                "reviewer_model_large must not be empty when it is set".into(),
            ));
        }
        if cfg.size_small_max_bytes == 0 || cfg.size_small_max_bytes >= cfg.size_medium_max_bytes {
            return Err(CoreError::Config(
                "size_small_max_bytes must be above zero and below size_medium_max_bytes".into(),
            ));
        }
        if cfg.kb_path.as_deref().is_some_and(|p| p.trim().is_empty()) {
            return Err(CoreError::Config(
                "kb_path must not be empty when it is set".into(),
            ));
        }
        if cfg.builder_carry_max_ctx == 0 {
            return Err(CoreError::Config(
                "builder_carry_max_ctx must be above 0".into(),
            ));
        }
        if cfg
            .cache_ttl
            .as_deref()
            .is_some_and(|t| t != "5m" && t != "1h")
        {
            return Err(CoreError::Config(
                "cache_ttl must be \"5m\" or \"1h\" when it is set".into(),
            ));
        }
        if cfg
            .agent_cli
            .as_deref()
            .is_some_and(|c| c != "claude" && c != "codex")
        {
            return Err(CoreError::Config(
                "agent_cli must be \"claude\" or \"codex\" when it is set".into(),
            ));
        }
        if let Some(file) = cfg.api_key_file.as_deref() {
            if !(file.starts_with("~/") || Path::new(file).is_absolute()) {
                return Err(CoreError::Config(
                    "api_key_file must be an absolute path or start with ~/".into(),
                ));
            }
            if cfg.allow_api_key == Some(false) {
                return Err(CoreError::Config(
                    "api_key_file cannot be set while allow_api_key is false".into(),
                ));
            }
        }
        if cfg.agent_cli() == "codex"
            && (cfg.allow_api_key == Some(true) || cfg.api_key_file.is_some())
        {
            return Err(CoreError::Config(
                "allow_api_key and api_key_file need agent_cli = \"claude\"".into(),
            ));
        }
        if cfg
            .log_level
            .as_deref()
            .is_some_and(|l| !["error", "warn", "info", "debug", "trace"].contains(&l))
        {
            return Err(CoreError::Config(
                "log_level must be one of error, warn, info, debug, trace when it is set".into(),
            ));
        }
        if let Err(why) = check_branch_name(&cfg.base_branch) {
            return Err(CoreError::Config(format!("base_branch: {why}")));
        }
        for entry in &cfg.builder_allow {
            if let Err(why) = check_builder_allow(entry) {
                return Err(CoreError::Config(format!("builder_allow: {why}")));
            }
        }
        for (key, lines) in [
            ("read_first", &cfg.read_first),
            ("builder_rules", &cfg.builder_rules),
            ("check_hints", &cfg.check_hints),
            ("reviewer_rules", &cfg.reviewer_rules),
            ("plan_rules", &cfg.plan_rules),
        ] {
            if lines
                .iter()
                .any(|l| l.trim().is_empty() || l.contains('\n') || l.contains('\r'))
            {
                return Err(CoreError::Config(format!(
                    "{key}: every line must be one non-empty line"
                )));
            }
        }
        for (key, names) in [
            ("builder_plugins", &cfg.builder_plugins),
            ("reviewer_plugins", &cfg.reviewer_plugins),
            ("commit_plugins", &cfg.commit_plugins),
            ("kb_plugins", &cfg.kb_plugins),
        ] {
            if names
                .iter()
                .any(|n| n.is_empty() || n.contains(char::is_whitespace))
            {
                return Err(CoreError::Config(format!(
                    "{key}: every plugin name must be one non-empty word"
                )));
            }
        }
        Ok(cfg)
    }

    pub fn try_load(path: &Path) -> Result<Config, CoreError> {
        let text = std::fs::read_to_string(path).map_err(|e| CoreError::io(path, e))?;
        Config::parse(&text)
    }

    /// The API key file with a leading `~` expanded. `None` when `api_key_file` is unset, or when `~` is used and `HOME` is unset or empty.
    /// It does not check that the file exists.
    pub fn api_key_path(&self) -> Option<PathBuf> {
        let path = self.api_key_file.as_deref()?;
        expand_home(path, std::env::var("HOME").ok().as_deref())
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
    Some(if rest.is_empty() {
        PathBuf::from(home)
    } else {
        Path::new(home).join(rest)
    })
}

/// Programs that no builder allowlist entry may name, and that are never derived.
pub const NEVER_ALLOWED_PROGRAMS: [&str; 25] = [
    "git", "rm", "rmdir", "curl", "sudo", "sh", "bash", "env", "xargs", "zsh", "dash", "ksh",
    "fish", "busybox", "source", ".", "eval", "exec", "command", "timeout", "nice", "nohup",
    "time", "stdbuf", "setsid",
];

/// The only git subcommands a `builder_allow` entry may name.
pub const READ_ONLY_GIT: [&str; 5] = ["status", "diff", "log", "show", "rev-parse"];

/// One `builder_allow` entry: `Bash(<command>:*)` or `Bash(<command>)`, with a safe program.
fn check_builder_allow(entry: &str) -> Result<(), String> {
    if entry.trim().is_empty() {
        return Err("must not be empty".into());
    }
    if entry.contains('\n') || entry.contains('\r') {
        return Err("must be one line".into());
    }
    if entry.chars().count() > 200 {
        return Err("must be at most 200 characters".into());
    }
    let form = "must be Bash(<command>:*) or Bash(<command>)";
    let inner = entry
        .strip_prefix("Bash(")
        .and_then(|rest| rest.strip_suffix(')'))
        .ok_or_else(|| form.to_string())?;
    let command = inner.strip_suffix(":*").unwrap_or(inner);
    if ["&", "|", ";", ">", "<", "`", "$("]
        .iter()
        .any(|op| command.contains(op))
    {
        return Err("must be one plain command, without shell operators".into());
    }
    if command.contains(['"', '\'', '\\', '*', '?']) {
        return Err("must not contain quotes, backslashes or wildcards".into());
    }
    let mut tokens: Vec<&str> = Vec::new();
    let mut words = command.split_whitespace();
    while let Some(word) = words.next() {
        if word == "-C" {
            words.next();
        } else if tokens.is_empty() && is_env_assignment(word) {
            continue;
        } else {
            tokens.push(word);
        }
    }
    let Some(first) = tokens.first() else {
        return Err("needs a command".into());
    };
    let program = first.rsplit('/').next().unwrap_or(first).to_lowercase();
    if program == "git" {
        let read_only = tokens
            .get(1)
            .is_some_and(|w| READ_ONLY_GIT.contains(&w.to_lowercase().as_str()));
        if !read_only {
            return Err("git is allowed only for status, diff, log, show and rev-parse".into());
        }
    } else if NEVER_ALLOWED_PROGRAMS.contains(&program.as_str()) || !builder_program_ok(first) {
        return Err(format!("{program} is not allowed"));
    }
    Ok(())
}

/// True when `word` is a program a builder may be granted: plain characters, and a basename
/// (lowercased) that is not in the never list.
pub fn builder_program_ok(word: &str) -> bool {
    if word.is_empty()
        || !word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/'))
    {
        return false;
    }
    let base = word.rsplit('/').next().unwrap_or(word).to_lowercase();
    !base.is_empty() && !NEVER_ALLOWED_PROGRAMS.contains(&base.as_str())
}

/// True for a `NAME=value` word, where NAME is letters, digits and `_` and does not start with
/// a digit.
pub fn is_env_assignment(word: &str) -> bool {
    match word.split_once('=') {
        Some((name, _)) => {
            !name.is_empty()
                && !name.starts_with(|c: char| c.is_ascii_digit())
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        }
        None => false,
    }
}

/// A branch name git accepts, and not one of the factory's own job branches.
fn check_branch_name(name: &str) -> Result<(), &'static str> {
    if name.trim().is_empty() {
        return Err("must not be empty");
    }
    if name.starts_with('-')
        || name.starts_with('/')
        || name.ends_with('/')
        || name.ends_with(".lock")
        || name == "@"
    {
        return Err("is not a valid branch name");
    }
    if name.contains("..") || name.contains("@{") {
        return Err("is not a valid branch name");
    }
    if name
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || "~^:?*[\\".contains(c))
    {
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
    fn builder_allow_rejects_forbidden_and_malformed_entries() {
        for entry in [
            "Bash(git:*)",
            "Bash(git  commit:*)",
            "Bash(git -C x commit:*)",
            "Bash(/bin/rm:*)",
            "Bash(env sh:*)",
            "Bash(xargs rm:*)",
            "Bash(rm:*)",
            "Bash(rmdir x)",
            "Bash(curl:*)",
            "Bash(sudo:*)",
            "Bash(sh:*)",
            "Bash(bash -c x)",
            "Bash()",
            "Bash(:*)",
            "",
            "  ",
            "make",
            "Bash(make:*",
            "Bash(make:*)\nBash(ls:*)",
        ] {
            let body = format!("builder_allow = [{entry:?}]\n");
            let err = Config::parse(&body).expect_err(entry);
            assert!(err.to_string().contains("builder_allow:"), "{entry}: {err}");
        }
        let long = format!("Bash({}:*)", "a".repeat(200));
        let body = format!("builder_allow = [{long:?}]\n");
        Config::parse(&body).expect_err("201 or more characters");
    }

    #[test]
    fn builder_allow_rejects_shell_operators() {
        for entry in [
            "Bash(make && rm -rf x:*)",
            "Bash(make || true:*)",
            "Bash(make; ls:*)",
            "Bash(make | tee f:*)",
            "Bash(make > f:*)",
            "Bash(make < f:*)",
            "Bash(make `id`:*)",
            "Bash(make $(id):*)",
        ] {
            let body = format!("builder_allow = [{entry:?}]\n");
            let err = Config::parse(&body).expect_err(entry);
            let text = err.to_string();
            assert!(text.contains("builder_allow:"), "{entry}: {text}");
            assert!(text.contains("shell operators"), "{entry}: {text}");
        }
    }

    #[test]
    fn builder_allow_still_accepts_plain_arguments() {
        let rules = [
            "Bash(make test:*)",
            "Bash(git log --format=%h)",
            "Bash(cargo test -j4 -p x:*)",
        ];
        let body = format!("builder_allow = {rules:?}\n");
        Config::parse(&body).expect("parses");
    }

    #[test]
    fn builder_allow_accepts_a_make_rule() {
        let rules = [
            "Bash(make:*)",
            "Bash(shellcheck:*)",
            "Bash(git log:*)",
            "Bash(git -C x log)",
            "Bash(./scripts/test.sh:*)",
        ];
        let body = format!("builder_allow = {rules:?}\n");
        let cfg = Config::parse(&body).expect("parses");
        let want: Vec<String> = rules.iter().map(|s| s.to_string()).collect();
        assert_eq!(cfg.builder_allow, want);
    }

    #[test]
    fn a_bad_builder_allow_loads_as_fail_closed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bad_allow.toml");
        std::fs::write(&path, "builder_allow = [\"Bash(rm:*)\"]").expect("write");
        let cfg = Config::load(&path);
        assert!(cfg.degraded);
        assert!(cfg.builder_allow.is_empty());
        assert!(cfg.builder_permissions);
    }

    #[test]
    fn builder_allow_rejects_the_bypass_forms() {
        for entry in [
            "Bash(*)",
            "Bash(* :*)",
            "Bash(g* push:*)",
            "Bash(FOO=1 rm:*)",
            "Bash(GIT_DIR=x git push:*)",
            "Bash(\"rm\":*)",
            "Bash('git' push)",
            "Bash(\\rm:*)",
            "Bash(RM:*)",
            "Bash(Git push:*)",
            "Bash(make & rm -rf x:*)",
            "Bash(timeout 5 make:*)",
            "Bash(Timeout 5 make:*)",
            "Bash(zsh:*)",
            "Bash(.:*)",
            "Bash(source x:*)",
            "Bash(eval x:*)",
            "Bash(exec x:*)",
            "Bash(command x:*)",
            "Bash(nohup x:*)",
        ] {
            let err = Config::parse(&format!("builder_allow = [{entry:?}]\n")).expect_err(entry);
            assert!(err.to_string().contains("builder_allow:"), "{entry}: {err}");
        }
    }

    #[test]
    fn builder_allow_still_accepts_plain_commands() {
        for entry in [
            "Bash(make:*)",
            "Bash(Make:*)",
            "Bash(FOO=1 make test:*)",
            "Bash(git log:*)",
            "Bash(git -C x log)",
            "Bash(./scripts/test.sh:*)",
            "Bash(cargo test -j4 -p x:*)",
            "Bash(shellcheck:*)",
        ] {
            Config::parse(&format!("builder_allow = [{entry:?}]\n")).expect(entry);
        }
    }

    #[test]
    fn builder_program_ok_checks_chars_and_the_lowercased_basename() {
        assert!(builder_program_ok("cargo"));
        assert!(builder_program_ok("./scripts/t.sh"));
        for word in ["", "a*b", "a b", "a(b)", "RM", "/bin/Rm", ".", "timeout"] {
            assert!(!builder_program_ok(word), "{word}");
        }
    }

    #[test]
    fn never_allowed_and_read_only_lists_are_exact() {
        assert_eq!(
            NEVER_ALLOWED_PROGRAMS,
            [
                "git", "rm", "rmdir", "curl", "sudo", "sh", "bash", "env", "xargs", "zsh", "dash",
                "ksh", "fish", "busybox", "source", ".", "eval", "exec", "command", "timeout",
                "nice", "nohup", "time", "stdbuf", "setsid"
            ]
        );
        assert_eq!(
            READ_ONLY_GIT,
            ["status", "diff", "log", "show", "rev-parse"]
        );
    }

    #[test]
    fn a_config_without_the_new_keys_parses_with_defaults() {
        let cfg = Config::parse("diff_cap_lines = 100\n").expect("parses");
        assert!(
            cfg.code_paths.is_empty() && cfg.test_paths.is_empty() && cfg.test_markers.is_empty()
        );
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
    fn builder_permissions_defaults_to_true_and_allow_to_empty() {
        assert!(Config::default().builder_permissions);
        assert!(Config::default().builder_allow.is_empty());
        let cfg = Config::parse("diff_cap_lines = 100\n").expect("parses");
        assert!(cfg.builder_permissions);
        assert!(cfg.builder_allow.is_empty());
        let off = Config::parse("builder_permissions = false\n").expect("parses");
        assert!(!off.builder_permissions);
    }

    #[test]
    fn fail_closed_config_keeps_the_defaults() {
        let cfg = Config::fail_closed();
        assert!(cfg.builder_permissions);
        assert!(cfg.builder_allow.is_empty());
    }

    #[test]
    fn fail_closed_makes_every_path_code() {
        let cfg = Config::fail_closed();
        assert_eq!(cfg.code_paths, vec!["**".to_string()]);
        assert!(cfg.test_paths.is_empty() && !cfg.require_tests);
    }

    use super::*;

    #[test]
    fn builder_carry_settings_have_on_and_45000_defaults() {
        let c = Config::default();
        assert!(c.builder_carry_session);
        assert_eq!(c.builder_carry_max_ctx, 45_000);
        let parsed = Config::parse("builder_carry_session = false\nbuilder_carry_max_ctx = 60000")
            .expect("parse");
        assert!(!parsed.builder_carry_session);
        assert_eq!(parsed.builder_carry_max_ctx, 60_000);
    }

    #[test]
    fn builder_carry_max_ctx_zero_is_refused() {
        let err = Config::parse("builder_carry_max_ctx = 0").expect_err("zero is refused");
        assert!(err
            .to_string()
            .contains("builder_carry_max_ctx must be above 0"));
    }

    #[test]
    fn builder_carry_session_must_be_a_bool() {
        Config::parse("builder_carry_session = \"yes\"").expect_err("not a bool");
    }

    #[test]
    fn agent_cli_is_unset_by_default_and_takes_claude_or_codex() {
        assert_eq!(Config::default().agent_cli, None);
        assert_eq!(Config::default().agent_cli(), "claude");
        let cfg = Config::parse("agent_cli = \"codex\"").expect("codex parses");
        assert_eq!(cfg.agent_cli(), "codex");
        let cfg = Config::parse("agent_cli = \"claude\"").expect("claude parses");
        assert_eq!(cfg.agent_cli(), "claude");
    }

    #[test]
    fn agent_cli_rejects_other_values() {
        for text in ["agent_cli = \"gpt\"", "agent_cli = \"\""] {
            let err = Config::parse(text).expect_err("must fail");
            assert!(err.to_string().contains("agent_cli must be"), "{err}");
        }
    }

    #[test]
    fn api_key_settings_are_unset_by_default() {
        let cfg = Config::default();
        assert_eq!(cfg.allow_api_key, None);
        assert_eq!(cfg.api_key_file, None);
        assert_eq!(cfg.api_key_path(), None);
    }

    #[test]
    fn api_key_settings_parse() {
        let cfg = Config::parse("allow_api_key = true").expect("flag parses");
        assert_eq!(cfg.allow_api_key, Some(true));
        let cfg = Config::parse("allow_api_key = false").expect("flag parses");
        assert_eq!(cfg.allow_api_key, Some(false));
        let cfg = Config::parse("api_key_file = \"/keys/rocket.key\"").expect("file parses");
        assert_eq!(cfg.api_key_path(), Some(PathBuf::from("/keys/rocket.key")));
        let cfg = Config::parse("api_key_file = \"~/keys/rocket.key\"").expect("tilde parses");
        assert!(cfg.api_key_file.is_some());
    }

    #[test]
    fn api_key_file_must_be_an_absolute_or_home_path() {
        for text in [
            "api_key_file = \"\"",
            "api_key_file = \"  \"",
            "api_key_file = \"keys/rocket.key\"",
        ] {
            let err = Config::parse(text).expect_err("must fail");
            assert!(err.to_string().contains("api_key_file must be"), "{err}");
        }
    }

    #[test]
    fn api_key_file_conflicts_with_a_denied_key() {
        let err = Config::parse("allow_api_key = false\napi_key_file = \"/k\"")
            .expect_err("contradiction");
        assert!(err.to_string().contains("api_key_file"), "{err}");
        Config::parse("allow_api_key = true\napi_key_file = \"/k\"").expect("agrees");
    }

    #[test]
    fn api_key_settings_are_claude_only() {
        for text in [
            "agent_cli = \"codex\"\nallow_api_key = true",
            "agent_cli = \"codex\"\napi_key_file = \"/k\"",
        ] {
            let err = Config::parse(text).expect_err("codex has no Anthropic key");
            assert!(err.to_string().contains("agent_cli = \"claude\""), "{err}");
        }
        Config::parse("agent_cli = \"codex\"\nallow_api_key = false").expect("denying is fine");
        Config::parse("agent_cli = \"claude\"\nallow_api_key = true").expect("claude is fine");
    }

    #[test]
    fn cache_ttl_is_unset_by_default_and_takes_only_5m_or_1h() {
        assert_eq!(Config::default().cache_ttl, None);
        assert_eq!(
            Config::parse("cache_ttl = \"5m\"")
                .expect("parse")
                .cache_ttl
                .as_deref(),
            Some("5m")
        );
        assert_eq!(
            Config::parse("cache_ttl = \"1h\"")
                .expect("parse")
                .cache_ttl
                .as_deref(),
            Some("1h")
        );
        assert!(Config::parse("cache_ttl = \"10m\"").is_err());
        assert!(Config::parse("cache_ttl = \"\"").is_err());
        assert!(Config::parse("cache_ttl = 5").is_err());
    }

    #[test]
    fn log_level_unset_by_default_and_takes_only_known_values() {
        assert_eq!(Config::default().log_level, None);
        assert_eq!(Config::default().log_level_or_default(), "info");
        assert_eq!(
            Config::parse("log_level = \"debug\"")
                .expect("parse")
                .log_level_or_default(),
            "debug"
        );
        let err = Config::parse("log_level = \"loud\"").expect_err("bad level");
        assert!(err
            .to_string()
            .contains("log_level must be one of error, warn, info, debug, trace"));
    }

    #[test]
    fn tickets_on_merge_is_off_by_default_and_can_be_turned_on() {
        assert!(!Config::default().tickets_on_merge);
        assert!(
            Config::parse("tickets_on_merge = true")
                .expect("parse")
                .tickets_on_merge
        );
        assert!(Config::parse("tickets_on_merge = \"yes\"").is_err());
    }

    #[test]
    fn each_role_loads_only_the_plugins_it_uses_by_default() {
        let cfg = Config::default();
        assert_eq!(cfg.plugins_for("builder"), ["superpowers"]);
        assert_eq!(cfg.plugins_for("committer"), ["dev-workflow-skills"]);
        assert_eq!(cfg.plugins_for("message"), ["dev-workflow-skills"]);
        assert!(cfg.plugins_for("reviewer").is_empty());
        assert!(cfg.plugins_for("kb").is_empty());
        assert!(cfg.plugins_for("someone-else").is_empty());
    }

    #[test]
    fn plugin_lists_can_be_set_per_role() {
        let cfg = Config::parse(
            "builder_plugins = [\"superpowers\", \"rust-analyzer-lsp\"]\nreviewer_plugins = [\"x@market\"]\ncommit_plugins = []\nkb_plugins = [\"y\"]",
        )
        .expect("parse");
        assert_eq!(
            cfg.plugins_for("builder"),
            ["superpowers", "rust-analyzer-lsp"]
        );
        assert_eq!(cfg.plugins_for("reviewer"), ["x@market"]);
        assert!(cfg.plugins_for("committer").is_empty());
        assert_eq!(cfg.plugins_for("kb"), ["y"]);
    }

    #[test]
    fn a_plugin_name_must_be_one_word() {
        for bad in ["builder_plugins = [\"\"]", "kb_plugins = [\"a b\"]"] {
            assert!(Config::parse(bad).is_err(), "{bad}");
        }
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
        let cfg = Config::parse("reviewer_model_large = \"opus\"\nreviewer_large_over_lines = 500")
            .expect("parse");
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
        for body in [
            "builder_rules = [\"\"]",
            "reviewer_rules = [\"a\\nb\"]",
            "read_first = [\"  \"]",
            "check_hints = [\"x\\ry\"]",
        ] {
            assert!(
                Config::parse(&format!("{body}\n")).is_err(),
                "{body} must be refused"
            );
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
            let cfg = Config::parse(&format!("base_branch = \"{name}\"\n"))
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(cfg.base_branch, name);
        }
    }

    #[test]
    fn base_branch_rejects_unsafe_names() {
        for name in [
            "",
            "  ",
            "a b",
            "a..b",
            "-x",
            "x/",
            "/x",
            "x.lock",
            "a\\tb",
            "factory/001-x",
            "a~b",
            "a^b",
            "a:b",
            "a?b",
            "a*b",
            "a[b",
            "@",
            "a@{b",
        ] {
            assert!(
                Config::parse(&format!("base_branch = \"{name}\"\n")).is_err(),
                "{name:?} must be refused"
            );
        }
    }

    #[test]
    fn size_thresholds_default_and_validate() {
        let cfg = Config::default();
        assert_eq!(cfg.size_small_max_bytes, 12_288);
        assert_eq!(cfg.size_medium_max_bytes, 25_600);
        assert!(Config::parse("size_small_max_bytes = 100\nsize_medium_max_bytes = 200\n").is_ok());
        assert!(
            Config::parse("size_small_max_bytes = 200\nsize_medium_max_bytes = 200\n").is_err()
        );
        assert!(
            Config::parse("size_small_max_bytes = 300\nsize_medium_max_bytes = 200\n").is_err()
        );
        assert!(Config::parse("size_small_max_bytes = 0\n").is_err());
        assert!(Config::parse("size_smal_max_bytes = 1\n").is_err());
    }

    #[test]
    fn kb_dir_is_none_when_unset() {
        assert_eq!(Config::default().kb_dir(), None);
    }

    #[test]
    fn an_absolute_kb_path_is_kept() {
        let cfg = Config {
            kb_path: Some("/kb/rocketapi".into()),
            ..Config::default()
        };
        assert_eq!(cfg.kb_dir(), Some(PathBuf::from("/kb/rocketapi")));
    }

    #[test]
    fn expand_home_handles_tilde_forms() {
        assert_eq!(
            expand_home("~/kb", Some("/home/u")),
            Some(PathBuf::from("/home/u/kb"))
        );
        assert_eq!(
            expand_home("~", Some("/home/u")),
            Some(PathBuf::from("/home/u"))
        );
        assert_eq!(expand_home("~/kb", None), None);
        assert_eq!(expand_home("~/kb", Some("")), None);
        assert_eq!(expand_home("~", None), None);
        assert_eq!(expand_home("/a/~b", None), Some(PathBuf::from("/a/~b")));
        assert_eq!(
            expand_home("~b/kb", Some("/home/u")),
            Some(PathBuf::from("~b/kb"))
        );
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
        assert_eq!(
            Config::default().budget_weighted,
            DEFAULT_JOB_BUDGET_WEIGHTED
        );
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
        assert!(
            text.contains("tickets_on_merge = true") && text.contains("commit_budget_weighted")
        );
        let cfg = Config::parse(text).expect("rocket config parses");
        assert!(cfg.tickets_on_merge && !cfg.commit_subagents && !cfg.degraded);
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, text).expect("write");
        let loaded = Config::load(&path);
        assert!(!loaded.degraded, "{loaded:?}");
        assert!(loaded.tickets_on_merge);
        assert!(
            Config::parse(&format!("{text}\nnot_a_key = 1\n")).is_err(),
            "an unknown key still fails"
        );
    }

    #[test]
    fn shipped_config_loads_and_maps_the_expected_paths() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../.claude/factory/config.toml"
        );
        let cfg = Config::try_load(std::path::Path::new(path)).expect("shipped config is valid");
        assert!(!cfg.degraded);
        let cmds = |p: &str| -> Vec<String> {
            cfg.checks
                .iter()
                .filter(|c| c.matches(p))
                .flat_map(|c| c.commands.clone())
                .collect()
        };
        assert!(cmds("crates/rocket-http/src/lib.rs")
            .contains(&"cargo check -j4 -p {crate}".to_string()));
        assert!(cmds("crates/rocket-http/src/lib.rs")
            .contains(&"cargo test -j4 -p {crate}".to_string()));
        assert!(cmds("src/App.tsx").contains(&"yarn tsc --noEmit".to_string()));
        assert!(cmds("src/App.tsx").contains(&"yarn check".to_string()));
        assert!(cmds("src/App.tsx").contains(&"yarn test --run --maxWorkers=50%".to_string()));
        assert!(cmds("tools/factory/crates/factory-core/src/lib.rs")
            .contains(&"cargo check -j4 --manifest-path tools/factory/Cargo.toml".to_string()));
        assert!(cmds("tools/factory-server/src/main.rs").contains(
            &"cargo check -j4 --manifest-path tools/factory-server/Cargo.toml".to_string()
        ));
        assert!(cmds("docs/a.md").is_empty());
        assert_eq!(cfg.commit_model, "sonnet");
        assert!(!cfg.commit_subagents);
        assert_eq!(cfg.commit_budget_weighted, 60_000);
        assert_eq!(cfg.kb_path, None);
    }
}
