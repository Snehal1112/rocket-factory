//! Builds the JSON that `claude --agents` takes from the agent files.

use serde_json::{json, Map, Value};

use crate::resolve::AgentFile;

/// Removes a leading `---` block, such as the frontmatter of an agent file.
pub fn strip_frontmatter(text: &str) -> &str {
    let Some(rest) = text.strip_prefix("---\n") else {
        return text;
    };
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        offset += line.len();
        if line.trim_end() == "---" {
            return rest[offset..].trim_start_matches('\n');
        }
    }
    text
}

/// The value of a `key:` line in the leading frontmatter block, trimmed.
fn frontmatter_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let rest = text.strip_prefix("---\n")?;
    rest.lines()
        .take_while(|line| line.trim_end() != "---")
        .find_map(|line| line.strip_prefix(key)?.strip_prefix(':'))
        .map(str::trim)
}

/// The `--agents` JSON: one entry per file, keyed by the file name without `.md`.
pub fn agents_json(files: &[AgentFile]) -> String {
    let mut agents = Map::new();
    for file in files {
        let name = file.name.strip_suffix(".md").unwrap_or(&file.name);
        let description = frontmatter_value(&file.content, "description").unwrap_or(name);
        let mut entry = json!({
            "description": description,
            "prompt": strip_frontmatter(&file.content),
        });
        if let Some(tools) = frontmatter_value(&file.content, "tools") {
            let tools: Vec<&str> = tools.split(',').map(str::trim).collect();
            entry["tools"] = Value::from(tools);
        }
        agents.insert(name.to_string(), entry);
    }
    Value::Object(agents).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset::{embedded, PRESET_NAMES};

    fn file(name: &str, content: &str) -> AgentFile {
        AgentFile {
            name: name.to_string(),
            content: content.to_string(),
        }
    }

    fn parse(files: &[AgentFile]) -> serde_json::Value {
        serde_json::from_str(&agents_json(files)).expect("agents_json is valid JSON")
    }

    #[test]
    fn tools_line_becomes_a_tools_list() {
        let text = "---\nname: factory-x\ndescription: d\ntools: Read, Grep, Glob\n---\nBody\n";
        let json = parse(&[file("factory-x.md", text)]);
        assert_eq!(
            json["factory-x"]["tools"],
            serde_json::json!(["Read", "Grep", "Glob"])
        );
    }

    #[test]
    fn a_frontmatter_without_tools_gives_no_tools_key() {
        let text = include_str!("../presets/rust/agents/factory-builder.md");
        let json = parse(&[file("factory-builder.md", text)]);
        assert!(json["factory-builder"].get("tools").is_none());
        let prompt = json["factory-builder"]["prompt"]
            .as_str()
            .expect("prompt is a string");
        assert!(prompt.starts_with("<!-- factory-rules: 1 -->"), "{prompt}");
    }

    #[test]
    fn name_description_and_prompt_come_from_the_file() {
        let text = include_str!("../presets/rust/agents/factory-builder.md");
        let json = parse(&[file("factory-builder.md", text)]);
        let entry = &json["factory-builder"];
        let description = text
            .lines()
            .find_map(|l| l.strip_prefix("description: "))
            .expect("the file has a description");
        assert_eq!(entry["description"], description);
        assert_eq!(entry["prompt"], strip_frontmatter(text));
    }

    #[test]
    fn a_file_without_frontmatter_uses_its_name_as_the_description() {
        let json = parse(&[file("factory-y.md", "Just text\n")]);
        assert_eq!(json["factory-y"]["description"], "factory-y");
        assert_eq!(json["factory-y"]["prompt"], "Just text\n");
        assert!(json["factory-y"].get("tools").is_none());
    }

    #[test]
    fn all_five_preset_agents_become_one_json_object() {
        for name in PRESET_NAMES {
            let preset = embedded(name).expect("the preset is embedded");
            let files: Vec<AgentFile> = preset.agents.iter().map(|(n, c)| file(n, c)).collect();
            let json = parse(&files);
            assert_eq!(json.as_object().expect("an object").len(), 5, "{name}");
        }
    }

    #[test]
    fn strip_frontmatter_removes_only_a_leading_block() {
        assert_eq!(
            strip_frontmatter("---\nname: x\n---\nBody\n---\nmore"),
            "Body\n---\nmore"
        );
        assert_eq!(
            strip_frontmatter("Body\n---\nx\n---\n"),
            "Body\n---\nx\n---\n"
        );
        assert_eq!(strip_frontmatter("---\nno end"), "---\nno end");
    }
}
