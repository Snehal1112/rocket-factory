//! Compares an overriding agent file with the preset file it replaces.

/// The number of the first line that is exactly `<!-- factory-rules: N -->`.
pub fn rules_marker(text: &str) -> Option<u32> {
    text.lines().find_map(|line| {
        let n = line
            .strip_prefix("<!-- factory-rules: ")?
            .strip_suffix(" -->")?;
        n.parse::<u32>().ok()
    })
}

/// What `own` lacks from `preset`: the rules marker, then each `## ` section line, in preset order.
pub fn agent_drift(preset: &str, own: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(want) = rules_marker(preset) {
        if rules_marker(own).is_none_or(|have| have < want) {
            out.push("the rules marker".to_string());
        }
    }
    for line in preset.lines().filter(|l| l.starts_with("## ")) {
        if !own.lines().any(|l| l == line) {
            out.push(format!("the section {}", &line[3..]));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRESET: &str = "<!-- factory-rules: 1 -->\n## Stack\nx\n## Judging\ny\n";

    #[test]
    fn rules_marker_reads_only_exact_lines() {
        assert_eq!(
            rules_marker("---\n---\n<!-- factory-rules: 3 -->\n"),
            Some(3)
        );
        assert_eq!(rules_marker("x <!-- factory-rules: 3 -->"), None);
        assert_eq!(rules_marker("<!-- factory-rules: x -->"), None);
        assert_eq!(rules_marker(""), None);
    }

    #[test]
    fn drift_lists_the_marker_and_missing_sections() {
        assert_eq!(
            agent_drift(PRESET, "own text\n"),
            [
                "the rules marker",
                "the section Stack",
                "the section Judging"
            ]
        );
    }

    #[test]
    fn same_file_has_no_drift() {
        assert!(agent_drift(PRESET, PRESET).is_empty());
    }

    #[test]
    fn a_newer_marker_is_not_drift() {
        let own = PRESET.replace("rules: 1", "rules: 2");
        assert!(agent_drift(PRESET, &own).is_empty());
    }
}
