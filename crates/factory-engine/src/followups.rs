//! The `## Follow-ups` section of a spec: one item per bullet, the pieces of a ticket that are not built yet.

use crate::runloop::{is_list_item, section_body};

/// Longest ticket title, in characters.
const MAX_TITLE_CHARS: usize = 100;

/// One remaining piece of work. The title is the first sentence of the bullet, the body is the whole bullet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FollowUp {
    pub title: String,
    pub body: String,
}

/// The bullet text without its marker (`- `, `* ` or `1. `).
fn strip_marker(line: &str) -> &str {
    let t = line.trim_start();
    if let Some(rest) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
        return rest.trim();
    }
    let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
    t[digits..].strip_prefix(". ").unwrap_or(t).trim()
}

/// The first sentence of a bullet without its closing full stop, cut at 100 characters.
fn title_of(body: &str) -> String {
    let first = body.split_once(". ").map_or(body, |(head, _)| head);
    let first = first.trim().trim_end_matches('.').trim();
    first.chars().take(MAX_TITLE_CHARS).collect::<String>().trim_end().to_string()
}

/// The bullets under `## Follow-ups`. A continuation line joins the bullet above it, a blank line ends
/// the bullet. A missing or empty section gives an empty list.
pub fn follow_ups(spec: &str) -> Vec<FollowUp> {
    let Some(section) = section_body(spec, "Follow-ups") else {
        return Vec::new();
    };
    let mut bodies: Vec<String> = Vec::new();
    let mut open = false;
    for line in section.lines() {
        if line.trim().is_empty() {
            open = false;
        } else if is_list_item(line) {
            bodies.push(strip_marker(line).to_string());
            open = true;
        } else if open {
            if let Some(last) = bodies.last_mut() {
                last.push(' ');
                last.push_str(line.trim());
            }
        }
    }
    bodies
        .into_iter()
        .filter(|b| !b.is_empty())
        .map(|body| FollowUp { title: title_of(&body), body })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_bullet() {
        let spec = "## Goal\nx\n\n## Follow-ups\n\n- Update the tier tracker tickets.\n\n## Acceptance criteria\n- a\n";
        let items = follow_ups(spec);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Update the tier tracker tickets");
        assert_eq!(items[0].body, "Update the tier tracker tickets.");
    }

    #[test]
    fn several_bullets_with_different_markers() {
        let spec = "## Follow-ups\n- First thing. More detail.\n* Second thing\n1. Third thing\n";
        let titles: Vec<String> = follow_ups(spec).into_iter().map(|f| f.title).collect();
        assert_eq!(titles, vec!["First thing", "Second thing", "Third thing"]);
    }

    #[test]
    fn multi_line_bullet_is_one_item() {
        let spec = "## Follow-ups\n- Merge QueryParam and Header\n  into one helper for both types.\n- Other\n";
        let items = follow_ups(spec);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "Merge QueryParam and Header into one helper for both types");
        assert_eq!(items[0].body, "Merge QueryParam and Header into one helper for both types.");
    }

    #[test]
    fn blank_line_ends_a_bullet() {
        let spec = "## Follow-ups\n- One\n\nstray paragraph\n- Two\n";
        let items = follow_ups(spec);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].body, "One");
    }

    #[test]
    fn no_section_and_empty_section_give_nothing() {
        assert!(follow_ups("## Goal\nx\n").is_empty());
        assert!(follow_ups("## Follow-ups\n\n## Acceptance criteria\n- a\n").is_empty());
    }

    #[test]
    fn bullets_inside_a_code_fence_are_ignored_by_the_section_reader() {
        let spec = "## Goal\n```\n## Follow-ups\n- fake\n```\n";
        assert!(follow_ups(spec).is_empty());
    }

    #[test]
    fn long_title_is_cut_at_100_chars() {
        let long = "\u{e9}".repeat(150);
        let spec = format!("## Follow-ups\n- {long}\n");
        let items = follow_ups(&spec);
        assert_eq!(items[0].title.chars().count(), 100);
        assert_eq!(items[0].body.chars().count(), 150);
    }
}
