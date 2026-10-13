use std::collections::BTreeSet;
use std::time::Duration;

use factory_core::Config;

use crate::CheckError;

/// Default time limit for one check command.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(900);

/// The ordered, de-duplicated commands to run for one diff.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckPlan {
    pub commands: Vec<String>,
    pub timeout: Duration,
}

/// Rejects a cargo command that lacks `-j4` or uses `--workspace` or `--all`.
pub fn validate_command(cmd: &str) -> Result<(), CheckError> {
    let tokens: Vec<&str> = cmd.split_whitespace().collect();
    let Some(first) = tokens.first() else {
        return Err(CheckError::Empty);
    };
    if *first != "cargo" {
        return Ok(());
    }
    for flag in ["--workspace", "--all"] {
        if tokens.contains(&flag) {
            return Err(CheckError::Forbidden { flag, cmd: cmd.to_string() });
        }
    }
    let has_j4 = tokens.contains(&"-j4") || tokens.windows(2).any(|w| w[0] == "-j" && w[1] == "4");
    if has_j4 {
        Ok(())
    } else {
        Err(CheckError::MissingJobs(cmd.to_string()))
    }
}

/// Validates every command of every `[[checks]]` rule. The engine calls this at start up.
pub fn validate_config(cfg: &Config) -> Result<(), CheckError> {
    for rule in &cfg.checks {
        for cmd in &rule.commands {
            validate_command(cmd)?;
        }
    }
    Ok(())
}

/// Returns the directory name after `crates/` in a path, if there is one.
fn crate_of(path: &str) -> Option<String> {
    let mut parts = path.split('/');
    while let Some(part) = parts.next() {
        if part == "crates" {
            return parts.next().filter(|n| !n.is_empty()).map(|n| n.to_string());
        }
    }
    None
}

/// Chooses the check commands for a set of changed paths.
pub fn plan_checks(changed_paths: &[String], cfg: &Config) -> CheckPlan {
    let mut seen = BTreeSet::new();
    let mut commands = Vec::new();
    for rule in &cfg.checks {
        let matched: Vec<&String> = changed_paths
            .iter()
            .filter(|p| rule.matches(p))
            .collect();
        if matched.is_empty() {
            continue;
        }
        let crates: BTreeSet<String> = matched.iter().filter_map(|p| crate_of(p)).collect();
        for template in &rule.commands {
            let expanded: Vec<String> = if template.contains("{crate}") {
                crates.iter().map(|c| template.replace("{crate}", c)).collect()
            } else {
                vec![template.clone()]
            };
            for cmd in expanded {
                // A command that breaks the cargo rules never runs.
                if validate_command(&cmd).is_ok() && seen.insert(cmd.clone()) {
                    commands.push(cmd);
                }
            }
        }
    }
    CheckPlan { commands, timeout: DEFAULT_TIMEOUT }
}

#[cfg(test)]
mod tests {
    use super::*;
    use factory_core::{CheckRule, Config};

    fn cfg(rules: Vec<(&[&str], &[&str])>) -> Config {
        let mut c = Config::default();
        c.checks = rules
            .into_iter()
            .map(|(p, cmds)| CheckRule {
                paths: p.iter().map(|s| s.to_string()).collect(),
                commands: cmds.iter().map(|s| s.to_string()).collect(),
            })
            .collect();
        c
    }

    fn paths(p: &[&str]) -> Vec<String> {
        p.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn rejects_cargo_without_j4() {
        assert!(matches!(validate_command("cargo check"), Err(crate::CheckError::MissingJobs(_))));
        assert!(matches!(validate_command("cargo test -j 3 -p a"), Err(crate::CheckError::MissingJobs(_))));
    }

    #[test]
    fn rejects_workspace_and_all() {
        assert!(matches!(validate_command("cargo test -j4 --workspace"), Err(crate::CheckError::Forbidden { .. })));
        assert!(matches!(validate_command("cargo check -j4 --all"), Err(crate::CheckError::Forbidden { .. })));
    }

    #[test]
    fn accepts_good_commands() {
        assert!(validate_command("cargo check -j4 --all-targets").is_ok());
        assert!(validate_command("cargo test -j 4 -p rocket-http").is_ok());
        assert!(validate_command("yarn tsc --noEmit").is_ok());
        assert!(matches!(validate_command("  "), Err(crate::CheckError::Empty)));
    }

    #[test]
    fn config_with_a_bad_command_is_rejected() {
        let c = cfg(vec![(&["**"], &["cargo test --workspace -j4"])]);
        assert!(validate_config(&c).is_err());
    }

    #[test]
    fn plan_is_ordered_and_deduplicated() {
        let c = cfg(vec![
            (&["crates/**"], &["cargo check -j4", "cargo test -j4 -p {crate}"]),
            (&["src-tauri/**"], &["cargo check -j4"]),
            (&["src/**"], &["yarn tsc --noEmit"]),
        ]);
        let plan = plan_checks(
            &paths(&["crates/rocket-http/src/a.rs", "crates/rocket-git/src/b.rs", "src-tauri/src/x.rs", "src/app.ts"]),
            &c,
        );
        assert_eq!(
            plan.commands,
            vec![
                "cargo check -j4",
                "cargo test -j4 -p rocket-git",
                "cargo test -j4 -p rocket-http",
                "yarn tsc --noEmit",
            ]
        );
    }

    #[test]
    fn plan_skips_unmatched_rules_and_unsafe_commands() {
        let c = cfg(vec![
            (&["docs/**"], &["yarn check"]),
            (&["**/*.md"], &["cargo test --workspace -j4"]),
        ]);
        assert!(plan_checks(&paths(&["src/a.ts"]), &c).commands.is_empty());
        assert!(plan_checks(&paths(&["README.md"]), &c).commands.is_empty());
    }

    #[test]
    fn crate_placeholder_without_a_crate_path_is_skipped() {
        let c = cfg(vec![(&["**"], &["cargo test -j4 -p {crate}"])]);
        assert!(plan_checks(&paths(&["README.md"]), &c).commands.is_empty());
    }
}
