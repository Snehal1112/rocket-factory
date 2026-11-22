use serde_json::Value;
use std::path::{Path, PathBuf};

const HEADER: &str = "| Key | Type | Default | Effect |\n|---|---|---|---|\n";
const HEADER_LINE: &str = "| Key | Type | Default | Effect |";
const IGNORED_KEYS: [&str; 5] = [
    "commit_model",
    "commit_subagents",
    "commit_budget_weighted",
    "commit_batch",
    "commit_plugins",
];

struct Row {
    key: String,
    default: String,
    line: usize,
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Splits a table row into at most four cells, on ` | ` outside backticks.
fn split_cells(row: &str) -> Vec<String> {
    let inner = row.trim().trim_start_matches('|').trim_end_matches('|');
    let chars: Vec<char> = inner.chars().collect();
    let mut cells = Vec::new();
    let mut current = String::new();
    let mut in_code = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '`' {
            in_code = !in_code;
        }
        let at_separator = !in_code
            && cells.len() < 3
            && c == ' '
            && chars.get(i + 1) == Some(&'|')
            && chars.get(i + 2) == Some(&' ');
        if at_separator {
            cells.push(current.trim().to_string());
            current.clear();
            i += 3;
            continue;
        }
        current.push(c);
        i += 1;
    }
    cells.push(current.trim().to_string());
    cells
}

/// Reads the rows of every table whose header line is the key table header.
fn doc_rows(doc: &str) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut in_table = false;
    for (index, line) in doc.lines().enumerate() {
        if line.trim() == HEADER_LINE {
            in_table = true;
            continue;
        }
        if !in_table {
            continue;
        }
        if !line.starts_with('|') {
            in_table = false;
            continue;
        }
        if line.starts_with("|---") {
            continue;
        }
        let cells = split_cells(line);
        if cells.len() < 3 {
            continue;
        }
        let Some(key) = cells[0]
            .strip_prefix('`')
            .and_then(|rest| rest.strip_suffix('`'))
        else {
            continue;
        };
        rows.push(Row {
            key: key.to_string(),
            default: cells[2].clone(),
            line: index + 1,
        });
    }
    rows
}

fn strip_marks(cell: &str) -> String {
    cell.chars().filter(|c| *c != '`' && *c != '"').collect()
}

/// Returns one failure line per problem between the doc and the config.
fn check(doc: &str, config: &Value) -> Vec<String> {
    let rows = doc_rows(doc);
    let Some(map) = config.as_object() else {
        return vec!["the config is not a JSON object".to_string()];
    };
    let mut failures = Vec::new();
    for (key, value) in map {
        if key == "cost_markers" {
            if !doc.lines().any(|l| l.trim() == "### Cost markers") {
                failures.push("`cost_markers` has no `### Cost markers` heading".to_string());
            }
            continue;
        }
        let Some(row) = rows.iter().find(|r| &r.key == key) else {
            failures.push(format!("`{key}` has no row in a key table"));
            continue;
        };
        let cell = strip_marks(&row.default);
        match value {
            Value::Null => {
                if !cell.starts_with("unset") {
                    failures.push(format!(
                        "line {}: `{key}` has no default, so the cell must start with `unset`, found `{}`",
                        row.line, row.default
                    ));
                }
            }
            Value::Bool(_) | Value::Number(_) | Value::String(_) => {
                let expected = match value {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                let one_token = !cell.is_empty() && !cell.contains(char::is_whitespace);
                if one_token && cell != expected {
                    failures.push(format!(
                        "line {}: `{key}` documents default `{cell}`, the code has `{expected}`",
                        row.line
                    ));
                }
            }
            _ => {}
        }
    }
    failures
}

/// The ignored keys are skipped by serde, so the doc section must name them.
fn check_ignored(doc: &str) -> Vec<String> {
    let mut section = String::new();
    let mut inside = false;
    for line in doc.lines() {
        if line.trim() == "### Ignored settings" {
            inside = true;
            continue;
        }
        if inside && line.starts_with('#') {
            break;
        }
        if inside {
            section.push_str(line);
            section.push('\n');
        }
    }
    IGNORED_KEYS
        .iter()
        .filter(|key| !section.contains(&format!("`{key}`")))
        .map(|key| format!("`{key}` is not named in the `### Ignored settings` section"))
        .collect()
}

#[test]
fn every_config_key_is_documented() {
    let doc = std::fs::read_to_string(repo_root().join("docs/reference/project-toml-reference.md"))
        .expect("doc");
    let config = serde_json::to_value(factory_core::Config::default()).expect("config json");
    let mut failures = check(&doc, &config);
    failures.extend(check_ignored(&doc));
    assert!(
        failures.is_empty(),
        "fix docs/reference/project-toml-reference.md:\n{}",
        failures.join("\n")
    );
}

#[test]
fn a_wrong_default_names_the_line() {
    let doc = format!("### A\n\n{HEADER}| `fix_rounds` | int | `7` | x |\n");
    let config = serde_json::json!({ "fix_rounds": 2 });
    let failures = check(&doc, &config);
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(
        failures[0].contains("fix_rounds") && failures[0].contains("line 5"),
        "{}",
        failures[0]
    );
    assert!(
        failures[0].contains('7') && failures[0].contains('2'),
        "{}",
        failures[0]
    );
}

#[test]
fn a_missing_key_fails() {
    let failures = check(HEADER, &serde_json::json!({ "fix_rounds": 2 }));
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(failures[0].contains("fix_rounds"), "{}", failures[0]);
}

#[test]
fn a_none_default_must_start_with_unset() {
    let doc = format!("{HEADER}| `risky_fix_rounds` | int | same as x | y |\n");
    let failures = check(&doc, &serde_json::json!({ "risky_fix_rounds": null }));
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(failures[0].contains("risky_fix_rounds"), "{}", failures[0]);
}

#[test]
fn a_prose_default_is_not_compared() {
    let doc = format!("{HEADER}| `risk_paths` | list | 19 built-in globs | y |\n");
    let failures = check(&doc, &serde_json::json!({ "risk_paths": ["a"] }));
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn a_pipe_in_the_effect_cell_keeps_the_default() {
    let doc = format!("{HEADER}| `fix_rounds` | int | `2` | a `x \\| y` b |\n");
    let failures = check(&doc, &serde_json::json!({ "fix_rounds": 2 }));
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn a_bare_pipe_in_backticks_keeps_the_default() {
    let doc = format!("{HEADER}| `fix_rounds` | int | `2` | a `x | y` b |\n");
    let failures = check(&doc, &serde_json::json!({ "fix_rounds": 2 }));
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn only_key_tables_are_read() {
    let doc = "| name | job |\n|---|---|\n| `fix_rounds` | z | `2` | x |\n\nno key table\n";
    let failures = check(doc, &serde_json::json!({ "fix_rounds": 2 }));
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(failures[0].contains("fix_rounds"), "{}", failures[0]);
}

#[test]
fn quoted_string_defaults_match() {
    let doc = format!("{HEADER}| `builder_model` | string | `\"m\"` | x |\n");
    let failures = check(&doc, &serde_json::json!({ "builder_model": "m" }));
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn cost_markers_need_their_heading() {
    let config = serde_json::json!({ "cost_markers": [] });
    assert_eq!(check("", &config).len(), 1);
    assert!(check("### Cost markers\n", &config).is_empty());
}

#[test]
fn ignored_keys_must_be_named_in_their_section() {
    let all = IGNORED_KEYS.map(|k| format!("`{k}`")).join(" ");
    assert!(check_ignored(&format!("### Ignored settings\n\n{all}\n\n### Next\n")).is_empty());
    let outside = format!("### Ignored settings\n\nnone\n\n### Next\n\n{all}\n");
    assert_eq!(check_ignored(&outside).len(), 5);
}
