//! Resolves the settings of one project from three layers: preset, project and repo override.
//!
//! The repo override is read from the base branch tip only. It may add risk paths and checks,
//! nothing else. Any problem makes the config fail closed.

use std::collections::BTreeMap;
use std::path::Path;

use crate::layout::is_agent_file_name;
use crate::project::{self, ProjectFile};
use crate::{Config, CoreError, Home, Layout};

const OVERRIDE_FILE: &str = ".factory/override.toml";
pub(crate) const AGENTS_DIR: &str = ".factory/agents";
/// Always a risk path, so a repo cannot change its own factory settings without review.
const FACTORY_RISK: &str = ".factory/**";

/// Reads committed files from the tip of the base branch.
pub trait BaseTipFiles {
    /// The text of the file, or `None` when it is absent.
    fn read_file(&self, rel: &str) -> Result<Option<String>, String>;
    /// The regular files directly inside the folder as (name, text). An absent folder is empty.
    fn list_dir(&self, rel: &str) -> Result<Vec<(String, String)>, String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentFile {
    pub name: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    pub config: Config,
    pub layout: Layout,
    pub agents: Vec<AgentFile>,
    pub problems: Vec<String>,
    /// Things that were left out but do not fail the config closed, such as a stray agent file.
    pub notes: Vec<String>,
}

/// Only an unknown project is an `Err`. Every other problem gives a fail-closed config.
pub fn resolve(home: &Home, name: &str, tip: &dyn BaseTipFiles) -> Result<Resolved, CoreError> {
    let file = project::get(home, name)?;
    let mut problems = Vec::new();
    // A bad preset name is a problem, and the layout then never names the preset folder.
    let layout = match project::layout(home, name, &file) {
        Ok(l) => l,
        Err(_) => project::build_layout(home, name, &file, false),
    };
    let merged = merged_config(home, &file, tip, &mut problems);
    let mut notes = Vec::new();
    let agents = merged_agents(home, name, &file, tip, &mut problems, &mut notes);
    let config = match merged {
        Some(cfg) if problems.is_empty() => cfg,
        _ => Config {
            base_branch: safe_base_branch(&file),
            ..Config::fail_closed()
        },
    };
    Ok(Resolved {
        config,
        layout,
        agents,
        problems,
        notes,
    })
}

/// The project's base branch when it is valid, else the default one.
fn safe_base_branch(file: &ProjectFile) -> String {
    file.base_branch()
        .unwrap_or_else(|| Config::default().base_branch)
}

fn strings(value: Option<&toml::Value>, key: &str, problems: &mut Vec<String>) -> Vec<String> {
    match value {
        None => Vec::new(),
        Some(toml::Value::Array(items)) => {
            let mut out = Vec::new();
            for item in items {
                match item.as_str() {
                    Some(s) => out.push(s.to_string()),
                    None => problems.push(format!("{key} must be a list of strings")),
                }
            }
            out
        }
        Some(_) => {
            problems.push(format!("{key} must be a list of strings"));
            Vec::new()
        }
    }
}

fn tables(value: Option<&toml::Value>, key: &str, problems: &mut Vec<String>) -> Vec<toml::Value> {
    match value {
        None => Vec::new(),
        Some(toml::Value::Array(items)) if items.iter().all(toml::Value::is_table) => items.clone(),
        Some(_) => {
            problems.push(format!("{key} must be a list of tables"));
            Vec::new()
        }
    }
}

fn merged_config(
    home: &Home,
    file: &ProjectFile,
    tip: &dyn BaseTipFiles,
    problems: &mut Vec<String>,
) -> Option<Config> {
    if project::validate_name(&file.preset).is_err() {
        problems.push(format!("preset name {:?} is not valid", file.preset));
        return None;
    }
    let preset_path = home.presets_dir().join(&file.preset).join("config.toml");
    let text = match std::fs::read_to_string(&preset_path) {
        Ok(t) => t,
        Err(e) => {
            problems.push(format!("cannot read preset {}: {e}", preset_path.display()));
            return None;
        }
    };
    let mut table: toml::Table = match toml::from_str(&text) {
        Ok(t) => t,
        Err(e) => {
            problems.push(format!(
                "preset {} is not valid TOML: {e}",
                preset_path.display()
            ));
            return None;
        }
    };
    for (k, v) in &file.settings {
        table.insert(k.clone(), v.clone());
    }

    let mut risk = if table.contains_key("risk_paths") {
        strings(table.get("risk_paths"), "risk_paths", problems)
    } else {
        Config::default().risk_paths
    };
    let mut checks = tables(table.get("checks"), "checks", problems);

    match tip.read_file(OVERRIDE_FILE) {
        Err(e) => problems.push(format!(
            "cannot read {OVERRIDE_FILE} from the base branch: {e}"
        )),
        Ok(None) => {}
        Ok(Some(text)) => match toml::from_str::<toml::Table>(&text) {
            Err(e) => problems.push(format!("{OVERRIDE_FILE} is not valid TOML: {e}")),
            Ok(over) => {
                for key in over.keys() {
                    if key != "risk_paths" && key != "checks" {
                        problems.push(format!(
                            "{OVERRIDE_FILE} may only set risk_paths and checks, not {key:?}"
                        ));
                    }
                }
                risk.extend(strings(
                    over.get("risk_paths"),
                    "override risk_paths",
                    problems,
                ));
                checks.extend(tables(over.get("checks"), "override checks", problems));
            }
        },
    }

    risk.push(FACTORY_RISK.to_string());
    let mut seen = Vec::new();
    risk.retain(|r| {
        if seen.contains(r) {
            false
        } else {
            seen.push(r.clone());
            true
        }
    });
    table.insert(
        "risk_paths".into(),
        toml::Value::Array(risk.into_iter().map(toml::Value::String).collect()),
    );
    if !checks.is_empty() {
        table.insert("checks".into(), toml::Value::Array(checks));
    }

    if !problems.is_empty() {
        return None;
    }
    let text = match toml::to_string(&table) {
        Ok(t) => t,
        Err(e) => {
            problems.push(format!("cannot write the merged settings: {e}"));
            return None;
        }
    };
    match Config::parse(&text) {
        Ok(cfg) => Some(cfg),
        Err(e) => {
            problems.push(format!("the merged settings are not valid: {e}"));
            None
        }
    }
}

fn read_agent_dir(dir: &Path, problems: &mut Vec<String>) -> Vec<(String, String)> {
    if !dir.is_dir() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            problems.push(format!("cannot read {}: {e}", dir.display()));
            return out;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if !path.is_file() || !name.ends_with(".md") {
            continue;
        }
        match std::fs::read_to_string(&path) {
            Ok(text) => out.push((name, text)),
            Err(e) => problems.push(format!("cannot read {}: {e}", path.display())),
        }
    }
    out
}

/// Adds an agent drift note when an override lacks parts of the preset file it replaces.
fn note_drift(
    preset_files: &BTreeMap<String, String>,
    name: &str,
    own: &str,
    preset: &str,
    notes: &mut Vec<String>,
) {
    let Some(base) = preset_files.get(name) else {
        return;
    };
    let mut items = crate::agent_drift::agent_drift(base, own);
    let Some(last) = items.pop() else {
        return;
    };
    let list = if items.is_empty() {
        last
    } else {
        format!("{} and {last}", items.join(", "))
    };
    notes.push(format!(
        "agent drift: {name} lacks {list}; compare it with the {preset} preset or remove the override"
    ));
}

fn merged_agents(
    home: &Home,
    name: &str,
    file: &ProjectFile,
    tip: &dyn BaseTipFiles,
    problems: &mut Vec<String>,
    notes: &mut Vec<String>,
) -> Vec<AgentFile> {
    let mut by_name: BTreeMap<String, String> = BTreeMap::new();
    if project::validate_name(&file.preset).is_ok() {
        let preset_agents = home.presets_dir().join(&file.preset).join("agents");
        by_name.extend(read_agent_dir(&preset_agents, problems));
    }
    let preset_files = by_name.clone();
    let own_files = read_agent_dir(&home.project_dir(name).join("agents"), problems);
    for (n, content) in &own_files {
        note_drift(&preset_files, n, content, &file.preset, notes);
    }
    by_name.extend(own_files);
    match tip.list_dir(AGENTS_DIR) {
        Err(e) => problems.push(format!(
            "cannot read {AGENTS_DIR} from the base branch: {e}"
        )),
        Ok(files) => {
            for (n, content) in files {
                if n.contains('/') || n.contains('\\') || n.contains("..") {
                    problems.push(format!(
                        "agent file name {n:?} in {AGENTS_DIR} is not valid"
                    ));
                } else if n.ends_with(".md") {
                    note_drift(&preset_files, &n, &content, &file.preset, notes);
                    by_name.insert(n, content);
                }
            }
        }
    }
    // The engine only writes factory-<name>.md. A stray file is dropped with a note, it does not degrade the config.
    let mut out = Vec::new();
    for (name, content) in by_name {
        if is_agent_file_name(&name) {
            out.push(AgentFile { name, content });
        } else if !name.contains('/') && !name.contains('\\') && !name.contains("..") {
            notes.push(format!(
                "ignored agent file {name}: names must be factory-<name>.md"
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[derive(Default)]
    struct Tip {
        files: HashMap<String, String>,
        dirs: HashMap<String, Vec<(String, String)>>,
        fail: bool,
    }

    impl Tip {
        fn with_override(text: &str) -> Tip {
            let mut t = Tip::default();
            t.files.insert(OVERRIDE_FILE.to_string(), text.to_string());
            t
        }
    }

    impl BaseTipFiles for Tip {
        fn read_file(&self, rel: &str) -> Result<Option<String>, String> {
            if self.fail {
                return Err("no such branch".into());
            }
            Ok(self.files.get(rel).cloned())
        }
        fn list_dir(&self, rel: &str) -> Result<Vec<(String, String)>, String> {
            if self.fail {
                return Err("no such branch".into());
            }
            Ok(self.dirs.get(rel).cloned().unwrap_or_default())
        }
    }

    struct Env {
        _t: TempDir,
        home: Home,
    }

    fn env_with(base: Option<&str>, preset: &str) -> Env {
        let t = TempDir::new().expect("tempdir");
        let repo: PathBuf = t.path().join("repo");
        std::fs::create_dir_all(&repo).expect("mkdir");
        let home = Home::at(t.path().join("home"));
        project::add(&home, "p", &repo, None, preset, base, &|_| Ok(()), None).expect("add");
        Env { _t: t, home }
    }

    fn env() -> Env {
        env_with(None, "rust")
    }

    fn set_settings(e: &Env, extra: &str) {
        let mut file = project::get(&e.home, "p").expect("get");
        let table: toml::Table = toml::from_str(extra).expect("settings toml");
        file.settings.extend(table);
        let text = toml::to_string(&file).expect("serialize");
        std::fs::write(e.home.project_dir("p").join("project.toml"), text).expect("write");
    }

    fn preset_config(e: &Env) -> Config {
        Config::parse(
            &std::fs::read_to_string(e.home.presets_dir().join("rust/config.toml")).expect("read"),
        )
        .expect("parse")
    }

    fn assert_fail_closed(r: &Resolved) {
        assert!(!r.problems.is_empty());
        assert!(r.config.degraded);
        assert_eq!(r.config.risk_paths, vec!["**".to_string()]);
    }

    #[test]
    fn no_override_gives_the_preset_with_the_project_base_branch_and_default_cache_ttl() {
        let e = env_with(Some("v-4.0.0"), "rust");
        let r = resolve(&e.home, "p", &Tip::default()).expect("resolve");
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        let mut want = preset_config(&e);
        want.base_branch = "v-4.0.0".into();
        want.cache_ttl = Some("5m".into());
        want.risk_paths.push(FACTORY_RISK.into());
        assert_eq!(r.config, want);
        let names: Vec<&str> = r.agents.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "factory-builder.md",
                "factory-code-reviewer.md",
                "factory-committer.md",
                "factory-kb-refresher.md",
                "factory-spec-reviewer.md"
            ]
        );
    }

    #[test]
    fn project_settings_replace_preset_keys() {
        let e = env();
        set_settings(&e, "diff_cap_lines = 123\nrisk_paths = [\"only/**\"]\n");
        let r = resolve(&e.home, "p", &Tip::default()).expect("resolve");
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        assert_eq!(r.config.diff_cap_lines, 123);
        assert_eq!(r.config.risk_paths, ["only/**", FACTORY_RISK]);
    }

    #[test]
    fn tickets_on_merge_is_a_valid_project_setting() {
        let e = env();
        assert!(
            !resolve(&e.home, "p", &Tip::default())
                .expect("resolve")
                .config
                .tickets_on_merge
        );
        set_settings(&e, "tickets_on_merge = true\n");
        let r = resolve(&e.home, "p", &Tip::default()).expect("resolve");
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        assert!(r.config.tickets_on_merge && !r.config.degraded);
    }

    #[test]
    fn override_risk_paths_are_added_never_removed() {
        let e = env();
        let base = resolve(&e.home, "p", &Tip::default())
            .expect("resolve")
            .config
            .risk_paths;
        let r = resolve(
            &e.home,
            "p",
            &Tip::with_override("risk_paths = [\"extra/**\"]\n"),
        )
        .expect("resolve");
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        assert!(r.config.risk_paths.len() > base.len());
        assert!(r.config.risk_paths.contains(&"extra/**".to_string()));
        for p in &base {
            assert!(r.config.risk_paths.contains(p), "{p} was dropped");
        }
    }

    #[test]
    fn an_empty_override_cannot_remove_risk_paths() {
        let e = env();
        let base = resolve(&e.home, "p", &Tip::default())
            .expect("resolve")
            .config
            .risk_paths;
        let r = resolve(&e.home, "p", &Tip::with_override("risk_paths = []\n")).expect("resolve");
        assert_eq!(r.config.risk_paths, base);
    }

    #[test]
    fn factory_dir_is_always_a_risk_path_exactly_once() {
        let e = env();
        let count = |r: &Resolved| {
            r.config
                .risk_paths
                .iter()
                .filter(|p| *p == FACTORY_RISK)
                .count()
        };
        assert_eq!(
            count(&resolve(&e.home, "p", &Tip::default()).expect("r")),
            1
        );
        let listed = Tip::with_override("risk_paths = [\".factory/**\", \"x/**\"]\n");
        assert_eq!(count(&resolve(&e.home, "p", &listed).expect("r")), 1);
        set_settings(&e, "risk_paths = [\".factory/**\"]\n");
        assert_eq!(
            count(&resolve(&e.home, "p", &Tip::default()).expect("r")),
            1
        );
        // A degraded config matches everything, so it covers the folder too.
        let bad = resolve(&e.home, "p", &Tip::with_override("diff_cap_lines = 1\n")).expect("r");
        assert_eq!(bad.config.risk_paths, ["**"]);
    }

    #[test]
    fn override_checks_follow_the_preset_checks() {
        let e = env();
        let preset_checks = preset_config(&e).checks;
        let over = "[[checks]]\npaths = [\"web/**\"]\ncommands = [\"yarn test\"]\n";
        let r = resolve(&e.home, "p", &Tip::with_override(over)).expect("resolve");
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        assert_eq!(r.config.checks.len(), preset_checks.len() + 1);
        assert_eq!(&r.config.checks[..preset_checks.len()], &preset_checks[..]);
        assert_eq!(
            r.config.checks.last().map(|c| c.commands.clone()),
            Some(vec!["yarn test".to_string()])
        );
    }

    #[test]
    fn an_override_with_empty_checks_leaves_the_preset_checks_intact() {
        let e = env();
        let preset_checks = preset_config(&e).checks;
        assert!(!preset_checks.is_empty());
        let r = resolve(&e.home, "p", &Tip::with_override("checks = []\n")).expect("resolve");
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        assert_eq!(r.config.checks, preset_checks);
    }

    #[test]
    fn plan_rules_come_from_project_settings_and_the_preset_has_none() {
        let e = env();
        let r = resolve(&e.home, "p", &Tip::default()).expect("resolve");
        assert!(r.config.plan_rules.is_empty());
        set_settings(&e, "plan_rules = [\"one\", \"two\"]\n");
        let r = resolve(&e.home, "p", &Tip::default()).expect("resolve");
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        assert!(!r.config.degraded);
        assert_eq!(r.config.plan_rules, ["one", "two"]);
    }

    #[test]
    fn any_other_override_key_fails_closed() {
        for key in [
            "plan_rules = [\"x\"]",
            "diff_cap_lines = 10",
            "base_branch = \"x\"",
            "budget_weighted = 5",
            "risk_pathz = [\"a\"]",
        ] {
            let e = env_with(Some("v-4.0.0"), "rust");
            let r =
                resolve(&e.home, "p", &Tip::with_override(&format!("{key}\n"))).expect("resolve");
            assert_fail_closed(&r);
            assert_eq!(r.config.base_branch, "v-4.0.0", "{key}");
        }
    }

    #[test]
    fn an_override_with_good_and_bad_keys_still_fails_closed() {
        let e = env();
        let r = resolve(
            &e.home,
            "p",
            &Tip::with_override("risk_paths = [\"a/**\"]\nfix_rounds = 9\n"),
        )
        .expect("r");
        assert_fail_closed(&r);
    }

    #[test]
    fn malformed_override_values_fail_closed() {
        let e = env();
        for text in [
            "risk_paths = \"a\"",
            "risk_paths = [1]",
            "checks = [1]",
            "checks = \"x\"",
        ] {
            assert_fail_closed(&resolve(&e.home, "p", &Tip::with_override(text)).expect("r"));
        }
        // A check rule with a typo is caught by Config::parse.
        let typo = "[[checks]]\npaths = [\"a\"]\ncommandz = [\"x\"]\n";
        assert_fail_closed(&resolve(&e.home, "p", &Tip::with_override(typo)).expect("r"));
    }

    #[test]
    fn an_unreadable_override_fails_closed_instead_of_meaning_no_override() {
        let e = env_with(Some("v-4.0.0"), "rust");
        let tip = Tip {
            fail: true,
            ..Tip::default()
        };
        let r = resolve(&e.home, "p", &tip).expect("resolve");
        assert_fail_closed(&r);
        assert!(
            r.problems.iter().any(|p| p.contains(OVERRIDE_FILE)),
            "{:?}",
            r.problems
        );
        assert_eq!(r.config.base_branch, "v-4.0.0");
    }

    #[test]
    fn broken_inputs_give_a_degraded_config_and_a_problem() {
        let e = env();
        assert_fail_closed(&resolve(&e.home, "p", &Tip::with_override("not = = toml")).expect("r"));
        let tip_fail = Tip {
            fail: true,
            ..Tip::default()
        };
        assert_fail_closed(&resolve(&e.home, "p", &tip_fail).expect("r"));

        let preset_file = e.home.presets_dir().join("rust/config.toml");
        let good = std::fs::read_to_string(&preset_file).expect("read");
        std::fs::write(&preset_file, "this is = = broken").expect("write");
        assert_fail_closed(&resolve(&e.home, "p", &Tip::default()).expect("r"));
        std::fs::write(&preset_file, &good).expect("write");

        set_settings(&e, "diff_cap_lines = 0\n");
        assert_fail_closed(&resolve(&e.home, "p", &Tip::default()).expect("r"));
    }

    #[test]
    fn a_missing_preset_directory_fails_closed_and_keeps_the_base_branch() {
        let e = env_with(Some("v-4.0.0"), "rust");
        std::fs::remove_dir_all(e.home.presets_dir().join("rust")).expect("rm");
        let r = resolve(&e.home, "p", &Tip::default()).expect("resolve");
        assert_fail_closed(&r);
        assert_eq!(r.config.base_branch, "v-4.0.0");
    }

    #[test]
    fn an_invalid_project_base_branch_falls_back_to_the_default() {
        let e = env();
        set_settings(&e, "base_branch = \"bad branch\"\n");
        let r = resolve(&e.home, "p", &Tip::default()).expect("resolve");
        assert_fail_closed(&r);
        assert_eq!(r.config.base_branch, Config::default().base_branch);
    }

    #[test]
    fn an_old_override_gives_a_drift_note() {
        let e = env();
        let own = e.home.project_dir("p").join("agents");
        std::fs::write(own.join("factory-code-reviewer.md"), "old reviewer\n").expect("write");
        let r = resolve(&e.home, "p", &Tip::default()).expect("resolve");
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        assert!(
            r.notes
                .iter()
                .any(|n| n
                    .starts_with("agent drift: factory-code-reviewer.md lacks the rules marker")),
            "{:?}",
            r.notes
        );
    }

    const OLD_REVIEWER_NOTE: &str = "agent drift: factory-code-reviewer.md lacks the rules marker, the section Stack, the section How to read, the section Check, the section Judging and the section Report; compare it with the rust preset or remove the override";

    #[test]
    fn an_old_override_lists_every_missing_item() {
        let e = env();
        let own = e.home.project_dir("p").join("agents");
        std::fs::write(own.join("factory-code-reviewer.md"), "old reviewer\n").expect("write");
        let r = resolve(&e.home, "p", &Tip::default()).expect("resolve");
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        let drift: Vec<&String> = r
            .notes
            .iter()
            .filter(|n| n.starts_with("agent drift"))
            .collect();
        assert_eq!(drift, [OLD_REVIEWER_NOTE]);
    }

    #[test]
    fn a_base_tip_override_gives_a_drift_note() {
        let e = env();
        let mut tip = Tip::default();
        tip.dirs.insert(
            AGENTS_DIR.to_string(),
            vec![("factory-code-reviewer.md".into(), "old\n".into())],
        );
        let r = resolve(&e.home, "p", &tip).expect("resolve");
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        assert!(
            r.notes.iter().any(|n| n == OLD_REVIEWER_NOTE),
            "{:?}",
            r.notes
        );
    }

    #[test]
    fn no_override_no_drift() {
        let e = env();
        let r = resolve(&e.home, "p", &Tip::default()).expect("resolve");
        assert!(
            !r.notes.iter().any(|n| n.starts_with("agent drift")),
            "{:?}",
            r.notes
        );
    }

    #[test]
    fn agent_layers_replace_by_name_and_the_result_is_sorted() {
        let e = env();
        let own = e.home.project_dir("p").join("agents");
        std::fs::write(own.join("factory-builder.md"), "project builder").expect("write");
        std::fs::write(own.join("factory-committer.md"), "project committer").expect("write");
        std::fs::write(own.join("factory-zzz-new.md"), "project new").expect("write");
        std::fs::write(own.join("notes.txt"), "ignored").expect("write");
        let mut tip = Tip::default();
        tip.dirs.insert(
            AGENTS_DIR.to_string(),
            vec![
                (
                    "factory-committer.md".to_string(),
                    "repo committer".to_string(),
                ),
                ("factory-aaa-repo.md".to_string(), "repo new".to_string()),
                ("readme.txt".to_string(), "ignored".to_string()),
            ],
        );
        let r = resolve(&e.home, "p", &tip).expect("resolve");
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        let got: Vec<(&str, &str)> = r
            .agents
            .iter()
            .map(|a| (a.name.as_str(), a.content.as_str()))
            .collect();
        let preset_reviewer = std::fs::read_to_string(
            e.home
                .presets_dir()
                .join("rust/agents/factory-code-reviewer.md"),
        )
        .expect("read");
        assert_eq!(
            got.iter().map(|g| g.0).collect::<Vec<_>>(),
            [
                "factory-aaa-repo.md",
                "factory-builder.md",
                "factory-code-reviewer.md",
                "factory-committer.md",
                "factory-kb-refresher.md",
                "factory-spec-reviewer.md",
                "factory-zzz-new.md"
            ]
        );
        assert_eq!(got[0].1, "repo new");
        assert_eq!(got[1].1, "project builder");
        assert_eq!(got[2].1, preset_reviewer);
        assert_eq!(got[3].1, "repo committer");
        assert_eq!(got[6].1, "project new");
    }

    #[test]
    fn stray_agent_files_are_dropped_with_a_note_and_do_not_degrade() {
        let e = env();
        std::fs::write(e.home.project_dir("p").join("agents/README.md"), "stray").expect("write");
        let mut tip = Tip::default();
        tip.dirs.insert(
            AGENTS_DIR.to_string(),
            vec![
                ("README.md".to_string(), "stray".to_string()),
                ("factory-ok.md".to_string(), "ok".to_string()),
            ],
        );
        let r = resolve(&e.home, "p", &tip).expect("resolve");
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        assert!(!r.config.degraded);
        assert!(r.agents.iter().all(|a| a.name != "README.md"));
        assert!(r.agents.iter().any(|a| a.name == "factory-ok.md"));
        assert_eq!(
            r.notes.iter().filter(|n| n.contains("README.md")).count(),
            1,
            "{:?}",
            r.notes
        );
        assert!(
            r.notes[0].starts_with("ignored agent file README.md: names must be factory-<name>.md")
        );
    }

    #[test]
    fn an_agent_name_with_a_path_separator_is_a_problem() {
        let e = env();
        let mut tip = Tip::default();
        tip.dirs.insert(
            AGENTS_DIR.to_string(),
            vec![("../evil.md".to_string(), "x".to_string())],
        );
        let r = resolve(&e.home, "p", &tip).expect("resolve");
        assert_fail_closed(&r);
        assert!(r.agents.iter().all(|a| a.name != "../evil.md"));
    }

    #[test]
    fn an_unknown_or_unsafe_project_is_an_error() {
        let e = env();
        assert!(resolve(&e.home, "nope", &Tip::default()).is_err());
        assert!(resolve(&e.home, "../p", &Tip::default()).is_err());
    }

    #[test]
    fn the_layout_matches_project_layout() {
        let e = env();
        let r = resolve(&e.home, "p", &Tip::default()).expect("resolve");
        let file = project::get(&e.home, "p").expect("get");
        assert_eq!(
            r.layout,
            project::layout(&e.home, "p", &file).expect("layout")
        );
    }

    #[test]
    fn a_preset_name_with_a_separator_is_a_problem_and_the_layout_stays_in_the_home() {
        let e = env();
        let mut file = project::get(&e.home, "p").expect("get");
        file.preset = "../../escape".to_string();
        std::fs::write(
            e.home.project_dir("p").join("project.toml"),
            toml::to_string(&file).expect("ser"),
        )
        .expect("write");
        let r = resolve(&e.home, "p", &Tip::default()).expect("resolve");
        assert!(
            r.problems.iter().any(|p| p.contains("preset name")),
            "{:?}",
            r.problems
        );
        assert!(r.layout.commit_context_file.starts_with(&e.home.dir));
        assert!(!r
            .layout
            .commit_context_file
            .to_string_lossy()
            .contains(".."));
        assert!(r.config.degraded);
    }
}
