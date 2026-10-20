//! The builder allowlist and deny rules, built from the config and the job folder.

use std::path::{Path, PathBuf};

use factory_agent::{AgentRun, AgentSpec};
#[cfg(test)]
use factory_core::NEVER_ALLOWED_PROGRAMS;
use factory_core::{builder_program_ok, is_env_assignment, Config, ParkKind};

use crate::ports::one_line;
use crate::{Engine, EngineError, StageOutcome};

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
    let mut seen = std::collections::HashSet::new();
    tools.retain(|t| seen.insert(t.clone()));
    tools
}

const GUARDED_FILES: [&str; 3] = ["plan.md", "spec.md", "progress.md"];

impl Engine<'_> {
    /// Runs a builder and restores `plan.md`, `spec.md` and `progress.md` if it changed them.
    ///
    /// `Ok(Err(outcome))` is the parked outcome the caller returns.
    pub(crate) fn run_builder_guarded(
        &self,
        spec: AgentSpec,
    ) -> Result<Result<AgentRun, StageOutcome>, EngineError> {
        let before: Vec<Option<Vec<u8>>> = GUARDED_FILES
            .iter()
            .map(|name| std::fs::read(self.job_dir.join(name)).ok())
            .collect();
        let earlier_session = self.job().ok().and_then(|j| j.session_id.clone());
        let run = self.run_agent(spec);
        let mut first_changed: Option<&str> = None;
        let mut failed: Vec<String> = Vec::new();
        for (name, old) in GUARDED_FILES.iter().zip(before) {
            let path = self.job_dir.join(name);
            let now = std::fs::read(&path).ok();
            if now == old {
                continue;
            }
            if let Err(e) = restore_file(&path, old.as_deref()) {
                tracing::warn!(event = "builder.restore_failed", file = *name, msg = %one_line(&e));
                failed.push(e);
            }
            first_changed.get_or_insert(name);
        }
        let run = run?;
        match first_changed {
            None => Ok(Ok(run)),
            Some(name) => {
                let session = run.session_id.clone().or(earlier_session);
                self.update(|j| {
                    j.builder_id = session;
                    Ok(())
                })?;
                let reason = match failed.first() {
                    None => format!("builder changed {name}"),
                    Some(e) => format!("builder changed {name}; it could not be restored: {e}"),
                };
                let parked = self.park_with(ParkKind::BuilderTampered, reason, Vec::new())?;
                Ok(Err(parked))
            }
        }
    }
}

/// Writes `old` back to `path`, or removes the file when `old` is `None`.
pub fn restore_file(path: &Path, old: Option<&[u8]>) -> Result<(), String> {
    match old {
        Some(bytes) => std::fs::write(path, bytes),
        None => std::fs::remove_file(path),
    }
    .map_err(|e| format!("{}: {e}", path.display()))
}

/// The rules that stop a builder from editing the job folder and the read-only folders.
pub fn builder_denied_tools(job_dir: &Path, read_only_dirs: &[PathBuf]) -> Vec<String> {
    let mut rules = Vec::new();
    for dir in std::iter::once(job_dir).chain(read_only_dirs.iter().map(PathBuf::as_path)) {
        let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        let dir = dir.display().to_string();
        let dir = dir.trim_start_matches('/');
        rules.push(format!("Edit(//{dir}/**)"));
        rules.push(format!("Write(//{dir}/**)"));
    }
    rules
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

/// The program of a command line, or None when it is unsafe to allow.
fn program_of(command: &str) -> Option<String> {
    let word = command.split_whitespace().find(|w| !is_env_assignment(w))?;
    if !builder_program_ok(word) {
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
    fn wrapper_and_shell_programs_are_never_derived() {
        let cfg = with_commands(&[
            "timeout 600 cargo test",
            "nice cargo test",
            ". venv/bin/activate",
            "source x",
            "eval x",
            "exec x",
            "command x",
            "zsh -c x",
            "nohup x",
            "time cargo t",
            "stdbuf -o0 x",
            "setsid x",
            "busybox sh",
        ]);
        assert_eq!(builder_allowed_tools(&cfg), base());
    }

    #[test]
    fn a_check_program_already_in_the_base_is_not_repeated() {
        let cfg = with_commands(&["grep -r x", "ls"]);
        assert_eq!(builder_allowed_tools(&cfg), base());
    }

    #[test]
    fn duplicate_builder_allow_entries_are_removed() {
        let cfg = Config {
            builder_allow: vec![
                "Bash(make:*)".into(),
                "Bash(make:*)".into(),
                "Bash(cd:*)".into(),
            ],
            ..Config::default()
        };
        assert_eq!(builder_allowed_tools(&cfg), plus(&["Bash(make:*)"]));
    }

    #[test]
    fn fail_closed_allowlist_is_the_base_list() {
        assert_eq!(builder_allowed_tools(&Config::fail_closed()), base());
    }

    #[test]
    fn allowlist_never_contains_env_xargs_rmdir_or_shells() {
        for name in factory_core::preset::PRESET_NAMES {
            let p = factory_core::preset::embedded(name).expect("preset");
            let cfg = Config::parse(p.config).expect("preset parses");
            for tool in builder_allowed_tools(&cfg) {
                let Some(inner) = tool.strip_prefix("Bash(") else {
                    continue;
                };
                let command = inner.trim_end_matches(')').trim_end_matches(":*");
                if BASE_GIT
                    .iter()
                    .any(|s| command == format!("git {s}").as_str())
                {
                    continue;
                }
                let program = command.split_whitespace().next().unwrap_or("");
                let base_name = program.rsplit('/').next().unwrap_or(program);
                assert!(
                    !NEVER_ALLOWED_PROGRAMS.contains(&base_name.to_lowercase().as_str()),
                    "{name}: {tool}"
                );
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
            builder_denied_tools(dir.path(), &[]),
            vec![format!("Edit(//{p}/**)"), format!("Write(//{p}/**)")]
        );
    }

    #[test]
    fn denied_tools_list_each_read_only_folder_after_the_job_folder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let job = dir.path().join("job");
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        for d in [&job, &a, &b] {
            std::fs::create_dir(d).expect("mkdir");
        }
        let rule = |d: &std::path::Path| {
            let c = d.canonicalize().expect("canonical").display().to_string();
            c.trim_start_matches('/').to_string()
        };
        let (j, pa, pb) = (rule(&job), rule(&a), rule(&b));
        assert_eq!(
            builder_denied_tools(&job, &[a, b]),
            vec![
                format!("Edit(//{j}/**)"),
                format!("Write(//{j}/**)"),
                format!("Edit(//{pa}/**)"),
                format!("Write(//{pa}/**)"),
                format!("Edit(//{pb}/**)"),
                format!("Write(//{pb}/**)"),
            ]
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
            builder_denied_tools(&link, &[]),
            vec![format!("Edit(//{p}/**)"), format!("Write(//{p}/**)")]
        );
    }
}
