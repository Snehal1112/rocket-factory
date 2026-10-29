use std::collections::BTreeSet;
use std::time::Duration;

use factory_core::Config;

use crate::workspace::Workspace;
use crate::CheckError;

/// Default time limit for one check command.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(900);

/// The ordered, de-duplicated commands to run for one diff.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckPlan {
    pub commands: Vec<String>,
    pub timeout: Duration,
    /// Lines for `checks.md` that explain what the plan left out.
    pub notes: Vec<String>,
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
            return Err(CheckError::Forbidden {
                flag,
                cmd: cmd.to_string(),
            });
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
            return parts
                .next()
                .filter(|n| !n.is_empty())
                .map(|n| n.to_string());
        }
    }
    None
}

/// Chooses the check commands for a set of changed paths.
pub fn plan_checks(changed_paths: &[String], cfg: &Config) -> CheckPlan {
    plan_checks_in(changed_paths, cfg, None)
}

/// Chooses the check commands, taking `{crate}` and `{dependents}` from the workspace map when there is one.
pub fn plan_checks_in(changed_paths: &[String], cfg: &Config, ws: Option<&Workspace>) -> CheckPlan {
    let mut seen = BTreeSet::new();
    let mut commands = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    for rule in &cfg.checks {
        let matched: Vec<&String> = changed_paths.iter().filter(|p| rule.matches(p)).collect();
        if matched.is_empty() {
            continue;
        }
        let crates: BTreeSet<String> = match ws {
            Some(ws) => matched
                .iter()
                .filter_map(|p| ws.owner(p))
                .map(str::to_string)
                .collect(),
            None => matched.iter().filter_map(|p| crate_of(p)).collect(),
        };
        let uses_dependents = rule.commands.iter().any(|c| c.contains("{dependents}"));
        let mut dependents = match ws {
            Some(ws) if uses_dependents => ws.dependents(&crates),
            _ => Vec::new(),
        };
        let cap = cfg.dependents_cap as usize;
        if dependents.len() > cap {
            let skipped = dependents.split_off(cap);
            let note = format!("SKIP dependents over the cap: {}", skipped.join(", "));
            if !notes.contains(&note) {
                notes.push(note);
            }
        }
        for template in &rule.commands {
            let mut expanded: Vec<String> = vec![template.clone()];
            if template.contains("{crate}") {
                expanded = crates
                    .iter()
                    .map(|c| template.replace("{crate}", c))
                    .collect();
            }
            if template.contains("{dependents}") {
                expanded = expanded
                    .iter()
                    .flat_map(|t| dependents.iter().map(|d| t.replace("{dependents}", d)))
                    .collect();
            }
            for cmd in expanded {
                // A command that breaks the cargo rules never runs.
                if validate_command(&cmd).is_ok() && seen.insert(cmd.clone()) {
                    commands.push(cmd);
                }
            }
        }
    }
    CheckPlan {
        commands,
        timeout: DEFAULT_TIMEOUT,
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use factory_core::{CheckRule, Config};

    fn cfg(rules: Vec<(&[&str], &[&str])>) -> Config {
        let checks = rules
            .into_iter()
            .map(|(p, cmds)| CheckRule {
                paths: p.iter().map(|s| s.to_string()).collect(),
                commands: cmds.iter().map(|s| s.to_string()).collect(),
            })
            .collect();
        Config {
            checks,
            ..Config::default()
        }
    }

    fn paths(p: &[&str]) -> Vec<String> {
        p.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn rejects_cargo_without_j4() {
        assert!(matches!(
            validate_command("cargo check"),
            Err(crate::CheckError::MissingJobs(_))
        ));
        assert!(matches!(
            validate_command("cargo test -j 3 -p a"),
            Err(crate::CheckError::MissingJobs(_))
        ));
    }

    #[test]
    fn rejects_workspace_and_all() {
        assert!(matches!(
            validate_command("cargo test -j4 --workspace"),
            Err(crate::CheckError::Forbidden { .. })
        ));
        assert!(matches!(
            validate_command("cargo check -j4 --all"),
            Err(crate::CheckError::Forbidden { .. })
        ));
    }

    #[test]
    fn accepts_good_commands() {
        assert!(validate_command("cargo check -j4 --all-targets").is_ok());
        assert!(validate_command("cargo test -j 4 -p rocket-http").is_ok());
        assert!(validate_command("yarn tsc --noEmit").is_ok());
        assert!(matches!(
            validate_command("  "),
            Err(crate::CheckError::Empty)
        ));
    }

    #[test]
    fn config_with_a_bad_command_is_rejected() {
        let c = cfg(vec![(&["**"], &["cargo test --workspace -j4"])]);
        assert!(validate_config(&c).is_err());
    }

    #[test]
    fn plan_is_ordered_and_deduplicated() {
        let c = cfg(vec![
            (
                &["crates/**"],
                &["cargo check -j4", "cargo test -j4 -p {crate}"],
            ),
            (&["src-tauri/**"], &["cargo check -j4"]),
            (&["src/**"], &["yarn tsc --noEmit"]),
        ]);
        let plan = plan_checks(
            &paths(&[
                "crates/rocket-http/src/a.rs",
                "crates/rocket-git/src/b.rs",
                "src-tauri/src/x.rs",
                "src/app.ts",
            ]),
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

    fn ws() -> Workspace {
        let json = r#"{"packages":[
          {"name":"core","manifest_path":"/r/crates/core/Cargo.toml","dependencies":[]},
          {"name":"app","manifest_path":"/r/crates/app/Cargo.toml","dependencies":[
            {"name":"core","path":"/r/crates/core"}]},
          {"name":"rocket","manifest_path":"/r/src-tauri/Cargo.toml","dependencies":[
            {"name":"app","path":"/r/crates/app"},{"name":"core","path":"/r/crates/core"}]}]}"#;
        Workspace::from_metadata(json, std::path::Path::new("/r")).expect("parse")
    }

    #[test]
    fn crate_follows_the_package() {
        let c = cfg(vec![(&["src-tauri/**"], &["cargo test -j4 -p {crate}"])]);
        let plan = plan_checks_in(&paths(&["src-tauri/src/lib.rs"]), &c, Some(&ws()));
        assert_eq!(plan.commands, ["cargo test -j4 -p rocket"]);
    }

    #[test]
    fn dependents_expand_and_cap() {
        let mut c = cfg(vec![(&["crates/**"], &["cargo test -j4 -p {dependents}"])]);
        c.dependents_cap = 1;
        let plan = plan_checks_in(&paths(&["crates/core/src/a.rs"]), &c, Some(&ws()));
        assert_eq!(plan.commands, ["cargo test -j4 -p app"]);
        assert_eq!(plan.notes, ["SKIP dependents over the cap: rocket"]);
    }

    #[test]
    fn a_crate_only_rule_never_notes_the_dependents_cap() {
        let mut c = cfg(vec![(&["crates/**"], &["cargo test -j4 -p {crate}"])]);
        c.dependents_cap = 0;
        let plan = plan_checks_in(&paths(&["crates/core/src/a.rs"]), &c, Some(&ws()));
        assert_eq!(plan.commands, ["cargo test -j4 -p core"]);
        assert!(plan.notes.is_empty(), "{:?}", plan.notes);
    }

    #[test]
    fn root_path_gives_no_crate_command() {
        let c = cfg(vec![(&["Cargo.lock"], &["cargo test -j4 -p {crate}"])]);
        let plan = plan_checks_in(&paths(&["Cargo.lock"]), &c, Some(&ws()));
        assert!(plan.commands.is_empty());
    }

    #[test]
    fn no_cargo_falls_back() {
        let c = cfg(vec![(&["crates/**"], &["cargo test -j4 -p {crate}"])]);
        let plan = plan_checks_in(&paths(&["crates/core/src/a.rs"]), &c, None);
        assert_eq!(plan.commands, ["cargo test -j4 -p core"]);
        assert!(plan.notes.is_empty());
    }

    fn rust_preset() -> Config {
        let text = factory_core::preset::embedded("rust")
            .expect("rust preset")
            .config;
        Config::parse(text).expect("preset parses")
    }

    fn node_preset() -> Config {
        let text = factory_core::preset::embedded("node")
            .expect("node preset")
            .config;
        Config::parse(text).expect("preset parses")
    }

    #[test]
    fn the_node_preset_runs_three_scripts_for_a_tsx_change() {
        let plan = plan_checks(&paths(&["src/App.tsx", "src/App.test.tsx"]), &node_preset());
        assert_eq!(
            plan.commands,
            ["bun run typecheck", "bun run lint", "bun test"]
        );
    }

    #[test]
    fn the_node_preset_runs_nothing_for_a_lockfile() {
        let plan = plan_checks(&paths(&["bun.lock"]), &node_preset());
        assert!(plan.commands.is_empty());
    }

    #[test]
    fn the_rust_preset_checks_a_single_crate_repo() {
        let c = rust_preset();
        for changed in [
            "src/x.rs",
            "tests/t.rs",
            "benches/b.rs",
            "examples/e.rs",
            "build.rs",
        ] {
            let plan = plan_checks(&paths(&[changed]), &c);
            assert_eq!(
                plan.commands,
                ["cargo check -j4", "cargo test -j4"],
                "{changed}"
            );
        }
    }

    #[test]
    fn the_rust_preset_keeps_the_crate_commands_for_a_workspace_member() {
        let c = rust_preset();
        let plan = plan_checks(&paths(&["crates/x/src/lib.rs", "crates/y/tests/t.rs"]), &c);
        assert_eq!(
            plan.commands,
            [
                "cargo check -j4 -p x",
                "cargo check -j4 -p y",
                "cargo test -j4 -p x",
                "cargo test -j4 -p y"
            ]
        );
        // The root rule must not match a member's own src folder.
        assert!(!factory_core::glob_match("src/**", "crates/x/src/lib.rs"));
        for pat in [
            "src/**",
            "tests/**",
            "benches/**",
            "examples/**",
            "build.rs",
        ] {
            assert!(
                !factory_core::glob_match(pat, "crates/x/src/lib.rs"),
                "{pat}"
            );
            assert!(!factory_core::glob_match(pat, "crates/x/build.rs"), "{pat}");
        }
    }

    #[test]
    fn a_mixed_diff_runs_the_crate_and_root_commands_once_each() {
        let c = rust_preset();
        let plan = plan_checks(&paths(&["crates/x/src/lib.rs", "src/main.rs"]), &c);
        assert_eq!(
            plan.commands,
            [
                "cargo check -j4 -p x",
                "cargo test -j4 -p x",
                "cargo check -j4",
                "cargo test -j4"
            ]
        );
    }
}
