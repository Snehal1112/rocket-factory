//! The builder allowlist and deny rules, built from the config and the job folder.

use std::path::Path;

use factory_core::{Config, NEVER_ALLOWED_PROGRAMS};

const BASE_TOOLS: [&str; 5] = ["Read", "Grep", "Glob", "Skill", "Agent"];
const BASE_PROGRAMS: [&str; 9] = [
    "cd", "ls", "cat", "head", "tail", "wc", "grep", "find", "pwd",
];
const BASE_GIT: [&str; 4] = ["status", "diff", "log", "show"];

/// The tools a builder may use: the fixed base, the check programs, then `builder_allow`.
pub fn builder_allowed_tools(config: &Config) -> Vec<String> {
    let mut tools: Vec<String> = BASE_TOOLS.iter().map(|t| t.to_string()).collect();
    tools.extend(BASE_PROGRAMS.iter().map(|p| format!("Bash({p}:*)")));
    tools.extend(BASE_GIT.iter().map(|s| format!("Bash(git {s}:*)")));
    for program in derived_programs(config) {
        tools.push(format!("Bash({program}:*)"));
    }
    tools.extend(config.builder_allow.iter().cloned());
    tools
}

/// The rules that stop a builder from editing the job folder.
pub fn builder_denied_tools(job_dir: &Path) -> Vec<String> {
    let dir = job_dir
        .canonicalize()
        .unwrap_or_else(|_| job_dir.to_path_buf());
    let dir = dir.display().to_string();
    let dir = dir.trim_start_matches('/');
    vec![format!("Edit(//{dir}/**)"), format!("Write(//{dir}/**)")]
}

/// The programs of the check commands and hints, in order of first use, without duplicates.
fn derived_programs(config: &Config) -> Vec<String> {
    let commands = config
        .checks
        .iter()
        .flat_map(|rule| rule.commands.iter())
        .chain(config.check_hints.iter());
    let mut out: Vec<String> = Vec::new();
    for command in commands {
        if let Some(program) = program_of(command) {
            if !out.contains(&program) {
                out.push(program);
            }
        }
    }
    out
}

/// True for a `NAME=value` word.
fn is_env_assignment(word: &str) -> bool {
    match word.split_once('=') {
        Some((name, _)) => {
            !name.is_empty()
                && !name.starts_with(|c: char| c.is_ascii_digit())
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        }
        None => false,
    }
}

/// The program of a command line, or None when it is unsafe to allow.
fn program_of(command: &str) -> Option<String> {
    let word = command.split_whitespace().find(|w| !is_env_assignment(w))?;
    if !word
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/'))
    {
        return None;
    }
    let basename = word.rsplit('/').next().unwrap_or(word);
    if basename.is_empty() || NEVER_ALLOWED_PROGRAMS.contains(&basename) {
        return None;
    }
    Some(word.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use factory_core::CheckRule;

    fn base() -> Vec<String> {
        [
            "Read",
            "Grep",
            "Glob",
            "Skill",
            "Agent",
            "Bash(cd:*)",
            "Bash(ls:*)",
            "Bash(cat:*)",
            "Bash(head:*)",
            "Bash(tail:*)",
            "Bash(wc:*)",
            "Bash(grep:*)",
            "Bash(find:*)",
            "Bash(pwd:*)",
            "Bash(git status:*)",
            "Bash(git diff:*)",
            "Bash(git log:*)",
            "Bash(git show:*)",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    fn with_commands(commands: &[&str]) -> Config {
        Config {
            checks: vec![CheckRule {
                paths: vec!["**".into()],
                commands: commands.iter().map(|s| s.to_string()).collect(),
            }],
            ..Config::default()
        }
    }

    fn plus(extra: &[&str]) -> Vec<String> {
        let mut v = base();
        v.extend(extra.iter().map(|s| s.to_string()));
        v
    }

    #[test]
    fn base_list_is_fixed_and_ordered() {
        assert_eq!(builder_allowed_tools(&Config::default()), base());
    }

    #[test]
    fn rust_preset_derives_cargo() {
        let cfg = with_commands(&["cargo test -j4 -p {crate}"]);
        let tools = builder_allowed_tools(&cfg);
        assert_eq!(tools.last().map(String::as_str), Some("Bash(cargo:*)"));
        assert_eq!(tools.iter().filter(|t| *t == "Bash(cargo:*)").count(), 1);
    }

    #[test]
    fn go_style_commands_derive_go() {
        let cfg = with_commands(&["go vet ./...", "go test ./..."]);
        assert_eq!(builder_allowed_tools(&cfg), plus(&["Bash(go:*)"]));
    }

    #[test]
    fn leading_env_assignment_is_skipped() {
        let cfg = with_commands(&["FOO=1 BAR=2 cargo test"]);
        assert_eq!(builder_allowed_tools(&cfg), plus(&["Bash(cargo:*)"]));
    }

    #[test]
    fn git_rm_rmdir_curl_sudo_sh_bash_env_xargs_are_never_derived() {
        let cfg = with_commands(&[
            "git diff --check",
            "rm -rf x",
            "rmdir x",
            "curl u",
            "sudo x",
            "sh -c x",
            "bash -c x",
            "env A=1 x",
            "xargs x",
            "/bin/rm x",
        ]);
        assert_eq!(builder_allowed_tools(&cfg), base());
    }

    #[test]
    fn prose_check_hint_adds_only_a_harmless_rule() {
        let cfg = Config {
            check_hints: vec!["run the tests".into()],
            ..Config::default()
        };
        assert_eq!(builder_allowed_tools(&cfg), plus(&["Bash(run:*)"]));
    }

    #[test]
    fn wildcard_program_is_skipped() {
        let cfg = with_commands(&["a*b x", "a(b) x", "a:b x"]);
        assert_eq!(builder_allowed_tools(&cfg), base());
    }

    #[test]
    fn duplicates_collapse_to_first_position() {
        let mut cfg = with_commands(&["cargo a", "make b", "cargo c"]);
        cfg.check_hints = vec!["make d".into()];
        assert_eq!(
            builder_allowed_tools(&cfg),
            plus(&["Bash(cargo:*)", "Bash(make:*)"])
        );
    }

    #[test]
    fn builder_allow_comes_last() {
        let mut cfg = with_commands(&["cargo t"]);
        cfg.builder_allow = vec!["Bash(make:*)".into()];
        assert_eq!(
            builder_allowed_tools(&cfg),
            plus(&["Bash(cargo:*)", "Bash(make:*)"])
        );
    }

    #[test]
    fn script_path_program_keeps_its_path() {
        let cfg = with_commands(&["./scripts/test.sh x"]);
        assert_eq!(
            builder_allowed_tools(&cfg),
            plus(&["Bash(./scripts/test.sh:*)"])
        );
    }

    #[test]
    fn allowlist_never_contains_commit_add_push_rm_curl() {
        for name in factory_core::preset::PRESET_NAMES {
            let p = factory_core::preset::embedded(name).expect("preset");
            let cfg = Config::parse(p.config).expect("preset parses");
            for tool in builder_allowed_tools(&cfg) {
                for bad in [
                    "commit",
                    "add",
                    "push",
                    "reset",
                    "rm",
                    "curl",
                    "sudo",
                    "Bash(sh",
                    "Bash(bash",
                ] {
                    assert!(!tool.contains(bad), "{name}: {tool} contains {bad}");
                }
            }
        }
    }

    #[test]
    fn denied_tools_cover_the_job_folder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let real = dir.path().canonicalize().expect("canonical");
        let p = real.display().to_string();
        let p = p.trim_start_matches('/');
        assert_eq!(
            builder_denied_tools(dir.path()),
            vec![format!("Edit(//{p}/**)"), format!("Write(//{p}/**)")]
        );
    }

    #[test]
    fn job_dir_is_canonicalized_in_the_deny_rules() {
        let dir = tempfile::tempdir().expect("tempdir");
        let real = dir.path().join("real");
        std::fs::create_dir(&real).expect("mkdir");
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");
        let canon = real.canonicalize().expect("canonical");
        let p = canon.display().to_string();
        let p = p.trim_start_matches('/');
        assert_eq!(
            builder_denied_tools(&link),
            vec![format!("Edit(//{p}/**)"), format!("Write(//{p}/**)")]
        );
    }
}
