//! The Rocket registration must resolve like Rocket's old `config.toml`, minus the factory's own folders.

use factory_core::{preset, resolve, BaseTipFiles, CheckRule, Config, Home, Resolved};
use tempfile::TempDir;

const OLD_CONFIG: &str = include_str!("fixtures/rocket-config.toml");
const REGISTRATION: &str = include_str!("fixtures/rocket-project.toml");
const NAME: &str = "rocket";

/// A base branch tip with no committed files.
struct EmptyTip;

impl BaseTipFiles for EmptyTip {
    fn read_file(&self, _rel: &str) -> Result<Option<String>, String> {
        Ok(None)
    }
    fn list_dir(&self, _rel: &str) -> Result<Vec<(String, String)>, String> {
        Ok(Vec::new())
    }
}

fn resolved() -> (TempDir, Resolved) {
    let t = TempDir::new().expect("tempdir");
    let home = Home::at(t.path().join("home"));
    preset::seed(&home.presets_dir()).expect("seed presets");
    let dir = home.project_dir(NAME);
    std::fs::create_dir_all(&dir).expect("mkdir project");
    std::fs::write(dir.join("project.toml"), REGISTRATION).expect("write project.toml");
    let r = resolve(&home, NAME, &EmptyTip).expect("resolve");
    (t, r)
}

/// True for a rule or path that belongs to the old in-repo factory.
fn is_factory_own(text: &str) -> bool {
    text.starts_with("tools/factory") || text.starts_with(".claude/factory")
}

fn old() -> Config {
    Config::parse(OLD_CONFIG).expect("old config parses")
}

#[test]
fn registration_does_not_fail_closed() {
    let (_t, r) = resolved();
    assert!(r.problems.is_empty(), "problems: {:?}", r.problems);
    assert!(!r.config.degraded);
    assert_ne!(r.config.risk_paths, vec!["**".to_string()]);
}

#[test]
fn settings_resolve_to_the_old_values() {
    let (_t, r) = resolved();
    let (new, old) = (r.config, old());
    assert_eq!(new.diff_cap_lines, old.diff_cap_lines);
    assert_eq!(new.budget_weighted, old.budget_weighted);
    assert_eq!(new.fix_rounds, old.fix_rounds);
    assert_eq!(new.stall_minutes, old.stall_minutes);
    assert_eq!(new.builder_model, old.builder_model);
    assert_eq!(new.reviewer_model, old.reviewer_model);
    assert_eq!(new.commit_model, old.commit_model);
    assert_eq!(new.commit_subagents, old.commit_subagents);
    assert_eq!(new.commit_budget_weighted, old.commit_budget_weighted);
    assert_eq!(new.commit_batch, old.commit_batch);
    assert_eq!(new.builder_excerpt, old.builder_excerpt);
    assert_eq!(new.tickets_on_merge, old.tickets_on_merge);
    assert_eq!(new.kb_path, old.kb_path);
    assert_eq!(new.kb_model, old.kb_model);
    assert_eq!(new.kb_budget_weighted, old.kb_budget_weighted);
    assert_eq!(new.kb_auto_commit, old.kb_auto_commit);
    assert_eq!(new.size_small_max_bytes, old.size_small_max_bytes);
    assert_eq!(new.size_medium_max_bytes, old.size_medium_max_bytes);
    assert_eq!(new.base_branch, old.base_branch);
}

#[test]
fn risk_paths_and_checks_match_minus_the_factory_folders() {
    let (_t, r) = resolved();
    let (new, old) = (r.config, old());
    let want: Vec<String> = old.risk_paths.iter().filter(|p| !is_factory_own(p)).cloned().collect();
    // The resolver always adds its own `.factory/**` entry. Everything else must match in order.
    let got: Vec<String> = new.risk_paths.iter().filter(|p| p.as_str() != ".factory/**").cloned().collect();
    assert_eq!(got, want);
    let want_checks: Vec<&CheckRule> =
        old.checks.iter().filter(|c| !c.paths.iter().any(|p| is_factory_own(p))).collect();
    let got_checks: Vec<&CheckRule> = new.checks.iter().collect();
    assert_eq!(got_checks, want_checks);
    for text in new.risk_paths.iter().chain(new.checks.iter().flat_map(|c| c.paths.iter())) {
        assert!(!is_factory_own(text), "factory folder left in: {text}");
    }
}

#[test]
fn rules_are_rockets_own_and_name_no_workspace_command() {
    let (_t, r) = resolved();
    let c = r.config;
    assert!(c.read_first.len() >= 2, "read_first: {:?}", c.read_first);
    assert!(c.read_first.iter().any(|f| f == "CLAUDE.md"));
    assert!(c.read_first.iter().any(|f| f.ends_with("rules/00-shortcuts.md")));
    let all: Vec<&String> =
        c.builder_rules.iter().chain(&c.check_hints).chain(&c.reviewer_rules).collect();
    for line in &all {
        // Only a command in backticks counts. A rule may say "never use --workspace".
        for cmd in line.split('`').skip(1).step_by(2) {
            assert!(!cmd.contains("--workspace") && !cmd.contains("--all"), "workspace command in: {line}");
        }
    }
    let joined = all.iter().map(|s| s.to_lowercase()).collect::<Vec<_>>().join("\n");
    for needle in [
        "shadcn", "lucide-react", "singlelineeditor", "monaco", "zustand", "unwrap", "git2", "camelcase",
        "yarn tsc --noemit", "yarn check", "yarn test", "cargo check -j4", "cargo test -j4 -p",
    ] {
        assert!(joined.contains(needle), "missing {needle}");
    }
}

#[test]
fn plan_rules_carry_the_two_rocket_rules_and_the_old_config_has_none() {
    let (_t, r) = resolved();
    let rules = &r.config.plan_rules;
    assert_eq!(rules.len(), 2, "{rules:?}");
    assert!(rules[0].contains("cargo check -j4 -p <crate>") && rules[0].contains("never a bare `cargo check -j4`"), "{rules:?}");
    assert!(rules[0].contains("stalls the builder"), "{rules:?}");
    assert!(rules[1].contains("docs/superpowers/specs/opencollection-spec-reference.md"), "{rules:?}");
    assert!(!OLD_CONFIG.contains("plan_rules"));
}

#[test]
fn the_src_tauri_rule_runs_the_tests() {
    let commands = |cfg: &Config| -> Vec<String> {
        cfg.checks
            .iter()
            .filter(|c| c.matches("src-tauri/src/lib.rs"))
            .flat_map(|c| c.commands.clone())
            .collect()
    };
    let (_t, r) = resolved();
    for cmds in [commands(&r.config), commands(&old())] {
        assert!(cmds.contains(&"cargo check -j4 -p rocket".to_string()), "{cmds:?}");
        assert!(cmds.contains(&"cargo test -j4 -p rocket".to_string()), "{cmds:?}");
        assert!(!cmds.contains(&"cargo check -j4".to_string()), "a bare workspace check: {cmds:?}");
    }
}
