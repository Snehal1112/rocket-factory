//! The commented block in `project.toml` that lists every setting with the value it takes when its line stays commented.

use crate::config::Config;
use crate::error::CoreError;

/// The first line of the block. Its presence means the header is already in the file.
pub const BLOCK_HEADER: &str =
    "# Every setting, with the value this project gets while its line stays commented.";

const HEADER_REST: &str =
    "# Remove the \"# \" to set one. A value here replaces the preset's value, lists included.\n\
# Reference: docs/reference/project-toml-reference.md in the rocket-factory repo.\n";

/// A setting and, for one that is unset by default, an example value with a note.
type Key = (&'static str, Option<&'static str>);

/// The settings in the order of the reference doc, grouped under its headings.
const GROUPS: &[(&str, &[Key])] = &[
    (
        "Limits and rounds.",
        &[
            ("budget_weighted", None),
            ("diff_cap_lines", None),
            ("fix_rounds", None),
            (
                "risky_fix_rounds",
                Some("2  # Unset means the same as fix_rounds."),
            ),
            ("stall_minutes", None),
            ("dependents_cap", None),
            ("unchecked_paths", None),
        ],
    ),
    (
        "Models and billing.",
        &[
            ("agent_cli", Some("\"codex\"  # Unset means claude.")),
            (
                "allow_api_key",
                Some("true  # Unset follows FACTORY_ALLOW_API_KEY."),
            ),
            (
                "api_key_file",
                Some("\"~/.factory/keys/project.key\"  # Unset bills the subscription login."),
            ),
            ("builder_model", None),
            ("reviewer_model", None),
            ("reviewer_model_large", None),
            ("reviewer_large_over_lines", None),
            ("escalation_model", None),
            ("escalate_after_fix_rounds", None),
            ("commit_model", None),
            ("kb_model", None),
        ],
    ),
    (
        "Commits.",
        &[
            ("commit_budget_weighted", None),
            ("commit_subagents", None),
            ("commit_batch", None),
        ],
    ),
    (
        "Builder and reviewer prompts.",
        &[
            ("builder_excerpt", None),
            ("builder_simplify", None),
            ("builder_carry_session", None),
            ("builder_carry_max_ctx", None),
            ("fix_fresh_over_ctx", None),
            ("read_first", None),
            ("builder_rules", None),
            ("check_hints", None),
            ("builder_permissions", None),
            ("builder_allow", None),
            ("reviewer_rules", None),
            ("plan_rules", None),
        ],
    ),
    (
        "Merge, tickets and logs.",
        &[
            ("base_branch", None),
            ("tickets_on_merge", None),
            ("minor_tickets", None),
            ("spec_review", None),
            ("log_level", Some("\"debug\"  # Unset means info.")),
            ("risk_paths", None),
        ],
    ),
    (
        "Checks and gates.",
        &[
            ("checks", None),
            ("code_paths", None),
            ("test_paths", None),
            ("test_markers", None),
            ("require_tests", None),
        ],
    ),
    (
        "Knowledge base.",
        &[
            (
                "kb_path",
                Some("\"~/kb/project\"  # Unset turns the KB off."),
            ),
            ("kb_budget_weighted", None),
            ("kb_auto_commit", None),
        ],
    ),
    (
        "Plan size groups.",
        &[
            ("size_small_max_bytes", None),
            ("size_medium_max_bytes", None),
        ],
    ),
    (
        "Prompt cache.",
        &[("cache_ttl", Some("\"1h\"  # Unset lets claude choose."))],
    ),
    (
        "Plugins. No agent ever loads an MCP server.",
        &[
            ("builder_plugins", None),
            ("reviewer_plugins", None),
            ("commit_plugins", None),
            ("kb_plugins", None),
        ],
    ),
];

#[cfg(test)]
fn keys() -> impl Iterator<Item = Key> {
    GROUPS.iter().flat_map(|(_, keys)| keys.iter().copied())
}

fn config_err(e: impl std::fmt::Display) -> CoreError {
    CoreError::Config(e.to_string())
}

/// Renders the lines this project still lacks. Empty when nothing is missing.
pub fn render(
    present: &dyn Fn(&str) -> bool,
    with_header: bool,
    preset_name: &str,
    preset_config: &str,
) -> Result<String, CoreError> {
    let preset: toml::Table = toml::from_str(preset_config).map_err(config_err)?;
    let defaults = toml::Table::try_from(Config::default()).map_err(config_err)?;
    let mut lines = String::new();
    for (heading, keys) in GROUPS {
        let missing: Vec<&Key> = keys.iter().filter(|(k, _)| !present(k)).collect();
        if missing.is_empty() {
            continue;
        }
        lines.push_str(&format!("\n# {heading}\n"));
        for (key, example) in missing {
            if *key == "checks" {
                lines.push_str(&format!(
                    "# checks: the [[checks]] tables of the {preset_name} preset. Add [[settings.checks]] tables at the end of the file to replace them.\n"
                ));
                continue;
            }
            let value = match (example, preset.get(*key).or(defaults.get(*key))) {
                (Some(example), _) => example.to_string(),
                (None, Some(value)) => value.to_string(),
                (None, None) => {
                    return Err(CoreError::Config(format!("setting {key} has no value")));
                }
            };
            let from = if preset.contains_key(*key) {
                format!("  # From the {preset_name} preset.")
            } else {
                String::new()
            };
            lines.push_str(&format!("# {key} = {value}{from}\n"));
        }
    }
    if lines.is_empty() {
        return Ok(String::new());
    }
    if !with_header {
        return Ok(lines.trim_start_matches('\n').to_string());
    }
    Ok(format!("{BLOCK_HEADER}\n{HEADER_REST}{lines}"))
}

/// One line per setting, in the order of the block: the value the project runs with and where it comes from.
pub fn effective_lines(
    config: &Config,
    project: &toml::Table,
    preset_config: &str,
) -> Result<Vec<String>, CoreError> {
    let preset: toml::Table = toml::from_str(preset_config).map_err(config_err)?;
    let values = toml::Table::try_from(config).map_err(config_err)?;
    let mut out = Vec::new();
    for (key, _) in GROUPS.iter().flat_map(|(_, keys)| keys.iter()) {
        let source = if project.contains_key(*key) {
            "project"
        } else if preset.contains_key(*key) {
            "preset"
        } else {
            "default"
        };
        let shown = match values.get(*key) {
            None => "unset".to_string(),
            Some(toml::Value::Array(items)) if *key == "checks" => format!("{} rules", items.len()),
            // A long list shows its size, the reference doc and the file show its entries.
            Some(toml::Value::Array(items))
                if toml::Value::Array(items.clone()).to_string().len() > 60 =>
            {
                format!("{} entries", items.len())
            }
            Some(v) => v.to_string(),
        };
        out.push(format!("{key} = {shown} ({source})"));
    }
    Ok(out)
}

/// The key a line sets or comments out, such as `fix_rounds` for `# fix_rounds = 3`.
fn commented_key(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix('#')?.trim_start();
    let end = rest
        .find(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'))
        .unwrap_or(rest.len());
    let after = rest[end..].trim_start();
    (end > 0 && (after.starts_with('=') || after.starts_with(':'))).then_some(&rest[..end])
}

/// Adds the missing lines of the block to `text`, inside `[settings]`. `None` when nothing is missing.
pub fn with_block(
    text: &str,
    preset_name: &str,
    preset_config: &str,
) -> Result<Option<String>, CoreError> {
    let before: toml::Table = toml::from_str(text).map_err(config_err)?;
    let set = before.get("settings").and_then(toml::Value::as_table);
    let commented: Vec<&str> = text.lines().filter_map(commented_key).collect();
    let present = |k: &str| set.is_some_and(|s| s.contains_key(k)) || commented.contains(&k);
    let block = render(
        &present,
        !text.contains(BLOCK_HEADER),
        preset_name,
        preset_config,
    )?;
    if block.is_empty() {
        return Ok(None);
    }
    let lines: Vec<&str> = text.lines().collect();
    let header = lines.iter().position(|l| l.trim() == "[settings]");
    // The block goes before the next table, and before the comments and blank lines that lead into it.
    let from = header.map_or(0, |h| h + 1);
    let mut at = lines[from..]
        .iter()
        .position(|l| l.trim_start().starts_with('['))
        .map_or(lines.len(), |i| from + i);
    if at < lines.len() {
        while at > from
            && (lines[at - 1].trim().is_empty() || lines[at - 1].trim_start().starts_with('#'))
        {
            at -= 1;
        }
    }
    let table = if header.is_none() { "[settings]\n" } else { "" };
    let mut out = lines[..at].join("\n");
    if !out.is_empty() {
        out.push('\n');
        if !out.ends_with("\n\n") {
            out.push('\n');
        }
    }
    out.push_str(table);
    out.push_str(&block);
    if at < lines.len() {
        // One blank line separates the block from what follows.
        if !lines[at].trim().is_empty() {
            out.push('\n');
        }
        out.push_str(&lines[at..].join("\n"));
        out.push('\n');
    }
    // The block holds only comments, so the parsed file must not change. An empty table counts as none.
    let without_empty = |mut t: toml::Table| {
        if t.get("settings")
            .and_then(toml::Value::as_table)
            .is_some_and(toml::Table::is_empty)
        {
            t.remove("settings");
        }
        t
    };
    let after: toml::Table = toml::from_str(&out).map_err(config_err)?;
    if without_empty(after) != without_empty(before) {
        return Err(CoreError::Config(
            "adding the settings block would change the settings".to_string(),
        ));
    }
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::preset;

    fn config_keys() -> Vec<String> {
        let v = serde_json::to_value(Config::default()).expect("json");
        v.as_object().expect("object").keys().cloned().collect()
    }

    /// Removes the `# ` of every setting line that has a real value, so the block can be parsed.
    fn uncommented(block: &str) -> String {
        block
            .lines()
            .filter_map(|l| {
                let rest = l.strip_prefix("# ")?;
                let key = rest.split([' ', '=']).next()?;
                let shown = keys().any(|(k, ex)| k == key && ex.is_none());
                (shown && key != "checks").then(|| rest.to_string())
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn every_config_key_is_listed_once() {
        let mut listed: Vec<String> = keys().map(|(k, _)| k.to_string()).collect();
        listed.sort();
        let before = listed.len();
        listed.dedup();
        assert_eq!(before, listed.len(), "a key is listed twice");
        let mut keys = config_keys();
        keys.sort();
        assert_eq!(listed, keys);
    }

    #[test]
    fn the_block_shows_the_preset_values_and_the_built_in_defaults() {
        for name in preset::PRESET_NAMES {
            let p = preset::embedded(name).expect("preset");
            let block = render(&|_| false, true, name, p.config).expect("render");
            assert!(block.starts_with(BLOCK_HEADER), "{block}");
            assert!(block.contains(&format!("the {name} preset")), "{block}");
            let shown = Config::parse(&uncommented(&block)).expect("the shown values parse");
            let expected = Config::parse(p.config).expect("preset parses");
            let mut a = serde_json::to_value(&shown).expect("json");
            let mut b = serde_json::to_value(&expected).expect("json");
            for v in [&mut a, &mut b] {
                v.as_object_mut().expect("object").remove("checks");
            }
            assert_eq!(a, b, "{name}");
        }
    }

    #[test]
    fn the_unset_examples_are_valid_values() {
        for (key, example) in keys() {
            if let Some(example) = example {
                let line = format!("{key} = {example}");
                Config::parse(&line).unwrap_or_else(|e| panic!("{line}: {e}"));
            }
        }
    }

    const FILE: &str = "repo = \"/r\"\npreset = \"rust\"\n\n[settings]\nbase_branch = \"main\"\nbudget_weighted = 5\n# fix_rounds = 3\n\n# My checks.\n[[settings.checks]]\npaths = [\"a/**\"]\ncommands = [\"true\"]\n";

    fn rust() -> &'static str {
        preset::embedded("rust").expect("rust").config
    }

    #[test]
    fn the_block_goes_inside_settings_and_skips_what_the_file_has() {
        let out = with_block(FILE, "rust", rust())
            .expect("ok")
            .expect("changed");
        for line in FILE.lines() {
            assert!(out.contains(line), "lost {line:?}");
        }
        assert!(!out.contains("# budget_weighted"), "{out}");
        let fix_rounds_lines = out
            .lines()
            .filter(|l| {
                l.trim_start()
                    .trim_start_matches('#')
                    .trim_start()
                    .starts_with("fix_rounds")
            })
            .count();
        assert_eq!(fix_rounds_lines, 1, "{out}");
        assert!(!out.contains("# checks"), "{out}");
        let block_at = out.find(BLOCK_HEADER).expect("header");
        assert!(
            block_at < out.find("# My checks.").expect("comment"),
            "{out}"
        );
        assert!(out.contains("# builder_carry_max_ctx = 45000"), "{out}");
        let before: toml::Table = toml::from_str(FILE).expect("toml");
        let after: toml::Table = toml::from_str(&out).expect("toml");
        assert_eq!(before, after);
    }

    #[test]
    fn a_second_run_changes_nothing() {
        let once = with_block(FILE, "rust", rust())
            .expect("ok")
            .expect("changed");
        assert_eq!(with_block(&once, "rust", rust()).expect("ok"), None);
    }

    #[test]
    fn a_file_without_settings_gets_the_table() {
        let out = with_block("repo = \"/r\"\npreset = \"rust\"\n", "rust", rust())
            .expect("ok")
            .expect("changed");
        assert!(out.contains("[settings]\n"), "{out}");
        let t: toml::Table = toml::from_str(&out).expect("toml");
        assert!(t["settings"].as_table().expect("table").is_empty());
    }
}
