use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use factory_core::{CheckRule, Config};

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
    /// Changed paths that a rule matched but no matching rule planned a command for.
    pub unplanned: Vec<String>,
    /// The gate of each planned `format` or `lint` command. Commands from `commands` have no entry.
    pub gates: BTreeMap<String, String>,
}

impl CheckPlan {
    /// `format` or `lint` for a command planned from those lists, `None` for any other command.
    pub fn gate_of(&self, command: &str) -> Option<&str> {
        self.gates.get(command).map(String::as_str)
    }
}

/// The command templates of a rule in plan order, each with its gate: `commands`, `format`, then `lint`.
fn templates(rule: &CheckRule) -> impl Iterator<Item = (&String, Option<&'static str>)> {
    let plain = rule.commands.iter().map(|t| (t, None));
    let format = rule.format.iter().map(|t| (t, Some("format")));
    let lint = rule.lint.iter().map(|t| (t, Some("lint")));
    plain.chain(format).chain(lint)
}

/// Rejects a cargo command that lacks `-j4` or uses `--workspace` or `--all`.
/// `cargo fmt` takes no job count, so it needs no `-j4`.
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
    if tokens.get(1) == Some(&"fmt") {
        return Ok(());
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
        for cmd in rule.all_commands() {
            if cmd.contains("{crate}") && cmd.contains("{dependents}") {
                return Err(CheckError::CrossProduct(cmd.clone()));
            }
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
    let mut gates = BTreeMap::new();
    let mut notes: Vec<String> = Vec::new();
    let mut matched_any: BTreeSet<&str> = BTreeSet::new();
    let mut covered: BTreeSet<&str> = BTreeSet::new();
    let owner_of = |p: &str| -> Option<String> {
        match ws {
            Some(ws) => ws.owner(p).map(str::to_string),
            None => crate_of(p),
        }
    };
    for rule in &cfg.checks {
        let matched: Vec<&String> = changed_paths.iter().filter(|p| rule.matches(p)).collect();
        if matched.is_empty() {
            continue;
        }
        let crates: BTreeSet<String> = matched.iter().filter_map(|p| owner_of(p)).collect();
        let uses_dependents = rule.all_commands().any(|c| c.contains("{dependents}"));
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
        for (template, gate) in templates(rule) {
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
            // A command that breaks the cargo rules never runs.
            let valid: Vec<String> = expanded
                .into_iter()
                .filter(|cmd| validate_command(cmd).is_ok())
                .collect();
            if !valid.is_empty() {
                for path in &matched {
                    // A `{crate}` template plans nothing for a path without an owning crate.
                    let planned = !template.contains("{crate}")
                        || owner_of(path).is_some_and(|c| {
                            validate_command(&template.replace("{crate}", &c)).is_ok()
                        });
                    if planned {
                        covered.insert(path.as_str());
                    }
                }
            }
            for cmd in valid {
                if seen.insert(cmd.clone()) {
                    if let Some(gate) = gate {
                        gates.insert(cmd.clone(), gate.to_string());
                    }
                    commands.push(cmd);
                }
            }
        }
        matched_any.extend(matched.iter().map(|p| p.as_str()));
    }
    let mut listed = BTreeSet::new();
    let unplanned = changed_paths
        .iter()
        .filter(|p| {
            matched_any.contains(p.as_str())
                && !covered.contains(p.as_str())
                && listed.insert(p.as_str())
        })
        .cloned()
        .collect();
    CheckPlan {
        commands,
        timeout: DEFAULT_TIMEOUT,
        notes,
        unplanned,
        gates,
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
                ..CheckRule::default()
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
    fn cargo_fmt_needs_no_job_count() {
        assert!(validate_command("cargo fmt -p core -- --check").is_ok());
        assert!(validate_command("cargo fmt -- --check").is_ok());
    }

    #[test]
    fn cargo_fmt_still_refuses_all_and_workspace() {
        assert!(matches!(
            validate_command("cargo fmt --all -- --check"),
            Err(crate::CheckError::Forbidden { flag: "--all", .. })
        ));
        assert!(matches!(
            validate_command("cargo fmt --workspace"),
            Err(crate::CheckError::Forbidden {
                flag: "--workspace",
                ..
            })
        ));
    }

    #[test]
    fn other_cargo_commands_still_need_j4() {
        assert!(matches!(
            validate_command("cargo check -p core"),
            Err(crate::CheckError::MissingJobs(_))
        ));
        assert!(matches!(
            validate_command("cargo clippy -p core"),
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

    #[test]
    fn a_crate_rule_for_a_root_file_plans_nothing_and_reports_the_path() {
        let c = cfg(vec![(&["**"], &["cargo test -j4 -p {crate}"])]);
        let plan = plan_checks_in(&paths(&["Cargo.toml"]), &c, None);
        assert!(plan.commands.is_empty());
        assert_eq!(plan.unplanned, paths(&["Cargo.toml"]));
    }

    #[test]
    fn dependents_over_the_cap_leave_the_paths_unplanned() {
        let mut c = cfg(vec![(&["crates/**"], &["cargo test -j4 -p {dependents}"])]);
        c.dependents_cap = 0;
        let plan = plan_checks_in(&paths(&["crates/core/src/a.rs"]), &c, Some(&ws()));
        assert!(plan.commands.is_empty());
        assert_eq!(plan.unplanned, paths(&["crates/core/src/a.rs"]));
    }

    #[test]
    fn a_rule_that_plans_for_one_path_leaves_an_unowned_path_unplanned() {
        let c = cfg(vec![(&["**"], &["cargo test -j4 -p {crate}"])]);
        let plan = plan_checks_in(&paths(&["crates/core/src/a.rs", "Cargo.toml"]), &c, None);
        assert_eq!(plan.commands, ["cargo test -j4 -p core"]);
        assert_eq!(plan.unplanned, paths(&["Cargo.toml"]));
    }

    #[test]
    fn validate_config_rejects_crate_with_dependents() {
        let c = cfg(vec![(
            &["crates/**"],
            &["cargo test -j4 -p {crate} -p {dependents}"],
        )]);
        let err = validate_config(&c).expect_err("cross product");
        assert!(matches!(err, crate::CheckError::CrossProduct(_)), "{err:?}");
        assert!(
            err.to_string().contains("{crate} and {dependents}"),
            "{err}"
        );
        assert!(
            err.to_string()
                .contains("cargo test -j4 -p {crate} -p {dependents}"),
            "{err}"
        );
    }

    #[test]
    fn a_path_with_a_planned_command_is_not_unplanned() {
        let c = cfg(vec![(&["crates/**"], &["cargo test -j4 -p {crate}"])]);
        let plan = plan_checks_in(&paths(&["crates/core/src/a.rs"]), &c, Some(&ws()));
        assert!(plan.unplanned.is_empty());
        assert_eq!(plan.commands, ["cargo test -j4 -p core"]);
    }

    #[test]
    fn a_second_rule_that_plans_a_command_covers_the_path() {
        let c = cfg(vec![
            (&["**"], &["cargo test -j4 -p {crate}"]),
            (&["Cargo.toml"], &["cargo check -j4"]),
        ]);
        let plan = plan_checks_in(&paths(&["Cargo.toml"]), &c, None);
        assert_eq!(plan.commands, ["cargo check -j4"]);
        assert!(plan.unplanned.is_empty());
    }

    #[test]
    fn a_duplicate_command_still_counts_as_planned() {
        let c = cfg(vec![
            (&["crates/**"], &["cargo check -j4"]),
            (&["crates/core/**"], &["cargo check -j4"]),
        ]);
        let plan = plan_checks_in(&paths(&["crates/core/a.rs"]), &c, None);
        assert_eq!(plan.commands, ["cargo check -j4"]);
        assert!(plan.unplanned.is_empty());
    }

    #[test]
    fn a_path_no_rule_matches_is_not_unplanned() {
        let c = cfg(vec![(&["docs/**"], &["yarn check"])]);
        let plan = plan_checks_in(&paths(&["src/a.ts"]), &c, None);
        assert!(plan.commands.is_empty());
        assert!(plan.unplanned.is_empty());
    }

    #[test]
    fn validate_config_accepts_crate_and_dependents_in_separate_commands() {
        let c = cfg(vec![(
            &["crates/**"],
            &[
                "cargo test -j4 -p {crate}",
                "cargo test -j4 -p {dependents}",
            ],
        )]);
        assert!(validate_config(&c).is_ok());
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
            ["bun run typecheck", "bun test", "bun run lint"]
        );
    }

    #[test]
    fn the_node_preset_runs_nothing_for_a_lockfile() {
        let plan = plan_checks(&paths(&["bun.lock"]), &node_preset());
        assert!(plan.commands.is_empty());
    }

    fn gate_rule(commands: &[&str], format: &[&str], lint: &[&str]) -> CheckRule {
        let own = |l: &[&str]| l.iter().map(|s| s.to_string()).collect();
        CheckRule {
            paths: vec!["crates/**".into()],
            commands: own(commands),
            format: own(format),
            lint: own(lint),
        }
    }

    fn cfg_of(rule: CheckRule) -> Config {
        Config {
            checks: vec![rule],
            ..Config::default()
        }
    }

    #[test]
    fn format_and_lint_commands_are_planned_after_commands() {
        let c = cfg_of(CheckRule {
            paths: vec!["crates/*/**".into()],
            commands: vec!["cargo check -j4 -p {crate}".into()],
            format: vec!["cargo fmt -p {crate} -- --check".into()],
            lint: vec!["cargo clippy -j4 -p {crate} --all-targets -- -D warnings".into()],
        });
        let plan = plan_checks_in(&paths(&["crates/foo/src/lib.rs"]), &c, None);
        assert_eq!(
            plan.commands,
            [
                "cargo check -j4 -p foo",
                "cargo fmt -p foo -- --check",
                "cargo clippy -j4 -p foo --all-targets -- -D warnings",
            ]
        );
        assert_eq!(plan.gate_of("cargo fmt -p foo -- --check"), Some("format"));
        assert_eq!(
            plan.gate_of("cargo clippy -j4 -p foo --all-targets -- -D warnings"),
            Some("lint")
        );
        assert_eq!(plan.gate_of("cargo check -j4 -p foo"), None);
    }

    #[test]
    fn a_path_is_covered_by_a_lint_only_rule() {
        let c = cfg_of(gate_rule(&[], &[], &["cargo clippy -j4 -p {crate}"]));
        let plan = plan_checks_in(&paths(&["crates/core/src/a.rs"]), &c, None);
        assert_eq!(plan.commands, ["cargo clippy -j4 -p core"]);
        assert!(plan.unplanned.is_empty());
    }

    #[test]
    fn gate_lists_expand_dependents() {
        let mut c = cfg_of(gate_rule(&[], &[], &["cargo test -j4 -p {dependents}"]));
        c.dependents_cap = 1;
        let plan = plan_checks_in(&paths(&["crates/core/src/a.rs"]), &c, Some(&ws()));
        assert_eq!(plan.commands, ["cargo test -j4 -p app"]);
        assert_eq!(plan.notes, ["SKIP dependents over the cap: rocket"]);
        assert_eq!(plan.gate_of("cargo test -j4 -p app"), Some("lint"));
    }

    #[test]
    fn an_invalid_cargo_command_in_a_gate_list_is_refused() {
        let c = cfg_of(gate_rule(&[], &["cargo fmt --workspace"], &[]));
        assert!(validate_config(&c).is_err());
        let c = cfg_of(gate_rule(&[], &[], &["cargo clippy -p x"]));
        assert!(validate_config(&c).is_err());
    }

    #[test]
    fn the_first_rule_that_plans_a_command_decides_its_gate() {
        let c = Config {
            checks: vec![
                gate_rule(&["cargo check -j4"], &[], &[]),
                gate_rule(&[], &[], &["cargo check -j4"]),
            ],
            ..Config::default()
        };
        let plan = plan_checks_in(&paths(&["crates/a.rs"]), &c, None);
        assert_eq!(plan.commands, ["cargo check -j4"]);
        assert_eq!(plan.gate_of("cargo check -j4"), None);
    }

    #[test]
    fn rust_preset_marks_fmt_and_clippy_as_gates() {
        let plan = plan_checks(&paths(&["crates/x/src/lib.rs"]), &rust_preset());
        assert_eq!(plan.gate_of("cargo fmt -p x -- --check"), Some("format"));
        assert_eq!(
            plan.gate_of("cargo clippy -j4 -p x --all-targets -- -D warnings"),
            Some("lint")
        );
        assert_eq!(plan.gate_of("cargo check -j4 -p x"), None);
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
                [
                    "cargo check -j4",
                    "cargo test -j4",
                    "cargo fmt -- --check",
                    "cargo clippy -j4 --all-targets -- -D warnings"
                ],
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
                "cargo test -j4 -p y",
                "cargo fmt -p x -- --check",
                "cargo fmt -p y -- --check",
                "cargo clippy -j4 -p x --all-targets -- -D warnings",
                "cargo clippy -j4 -p y --all-targets -- -D warnings"
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
    fn the_rust_preset_plans_no_dependents_test_without_dependents() {
        let json = r#"{"packages":[
          {"name":"core","manifest_path":"/r/crates/core/Cargo.toml","dependencies":[]},
          {"name":"app","manifest_path":"/r/crates/app/Cargo.toml","dependencies":[
            {"name":"core","path":"/r/crates/core"}]}]}"#;
        let w = Workspace::from_metadata(json, std::path::Path::new("/r")).expect("parse");
        let plan = plan_checks_in(&paths(&["crates/app/src/a.rs"]), &rust_preset(), Some(&w));
        assert!(plan
            .commands
            .contains(&"cargo fmt -p app -- --check".to_string()));
        assert!(plan
            .commands
            .iter()
            .all(|c| !c.contains("-p ") || c.contains("-p app")));
    }

    #[test]
    fn the_rust_preset_plans_clippy_for_the_changed_crate_only() {
        let plan = plan_checks_in(
            &paths(&["crates/app/src/a.rs"]),
            &rust_preset(),
            Some(&ws()),
        );
        let clippy: Vec<&String> = plan
            .commands
            .iter()
            .filter(|c| c.starts_with("cargo clippy"))
            .collect();
        assert_eq!(
            clippy,
            ["cargo clippy -j4 -p app --all-targets -- -D warnings"]
        );
    }

    #[test]
    fn the_rust_preset_commands_all_validate() {
        assert!(validate_config(&rust_preset()).is_ok());
    }

    #[test]
    fn the_rust_preset_dependents_test_respects_the_cap() {
        let mut c = rust_preset();
        c.dependents_cap = 0;
        let plan = plan_checks_in(&paths(&["crates/core/src/a.rs"]), &c, Some(&ws()));
        assert!(plan.commands.iter().all(|c| !c.contains("-p app")));
        assert!(
            plan.notes
                .iter()
                .any(|n| n.starts_with("SKIP dependents over the cap: app")),
            "{:?}",
            plan.notes
        );
    }

    #[test]
    fn a_preset_file_change_plans_the_cli_tests() {
        let json = r#"{"packages":[
          {"name":"factory-core","manifest_path":"/r/crates/factory-core/Cargo.toml","dependencies":[]},
          {"name":"factory-cli","manifest_path":"/r/crates/factory-cli/Cargo.toml","dependencies":[
            {"name":"factory-core","path":"/r/crates/factory-core"}]}]}"#;
        let w = Workspace::from_metadata(json, std::path::Path::new("/r")).expect("parse");
        let plan = plan_checks_in(
            &paths(&["crates/factory-core/presets/rust/agents/factory-builder.md"]),
            &rust_preset(),
            Some(&w),
        );
        assert!(plan
            .commands
            .contains(&"cargo test -j4 -p factory-cli".to_string()));
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
                "cargo fmt -p x -- --check",
                "cargo clippy -j4 -p x --all-targets -- -D warnings",
                "cargo check -j4",
                "cargo test -j4",
                "cargo fmt -- --check",
                "cargo clippy -j4 --all-targets -- -D warnings"
            ]
        );
    }
}
