//! Combines the ordered plan texts of one spec into one plan text. Pure: no I/O.

use std::collections::HashMap;

use crate::tasks::{
    lines_with_fence, parse_checkbox_tasks, parse_tasks, plan_header, plan_title, review_focus,
    task_section,
};

const CONSTRAINTS: &str = "## Global Constraints";
const FOCUS: &str = "## Review Focus";

/// One plan text made from the plans of a spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CombinedPlan {
    pub text: String,
    pub tasks: usize,
    pub warnings: Vec<String>,
}

/// One task section of a plan, with its old number.
struct Section {
    old: u32,
    text: String,
}

/// A plan cut into its header and its task sections.
struct Split {
    header: String,
    sections: Vec<Section>,
    /// The text that joins two sections: a blank line for headings, a line break for checkboxes.
    joint: &'static str,
    /// Task numbers whose `### Task N:` heading appears more than once. Only the first section counts.
    repeated: Vec<u32>,
}

/// The task form of a plan.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Form {
    Heading,
    Checkbox,
    None,
}

/// The task form a plan uses. `### Task N:` headings win over the checkbox form.
fn form_of(text: &str) -> Form {
    if !parse_tasks(text).is_empty() {
        Form::Heading
    } else if !parse_checkbox_tasks(text).is_empty() {
        Form::Checkbox
    } else {
        Form::None
    }
}

/// `plans` holds `(path, text)` in chain order. Pure: no I/O.
/// When the plans mix the heading and the checkbox form, checkbox tasks are written as headings.
pub fn combine_plans(plans: &[(String, String)]) -> CombinedPlan {
    let mut warnings = Vec::new();
    let mut constraints: Vec<String> = Vec::new();
    let mut focus: Vec<String> = Vec::new();
    let mut goals: Vec<String> = Vec::new();
    let mut task_blocks: Vec<String> = Vec::new();
    let mut first_parts: Option<HeaderParts> = None;
    let mut next = 1u32;
    let forms: Vec<Form> = plans.iter().map(|(_, text)| form_of(text)).collect();
    let mixed = forms.contains(&Form::Heading) && forms.contains(&Form::Checkbox);
    for (i, (path, text)) in plans.iter().enumerate() {
        let as_headings = mixed && forms[i] == Form::Checkbox;
        if as_headings {
            warnings.push(format!(
                "{path}: its checkbox tasks are written as `### Task N:` headings, because the plans mix both task forms"
            ));
        }
        let split = split_plan(text, as_headings);
        for n in &split.repeated {
            warnings.push(format!(
                "{path}: the heading of Task {n} appears more than once, only the first section is kept"
            ));
        }
        let parts = header_parts(&split.header);
        for item in section_items(parts.section(CONSTRAINTS)) {
            if constraints.contains(&item) {
                continue;
            }
            if names_a_plan(&item) {
                let first = item.lines().next().unwrap_or_default();
                warnings.push(format!("{path}: a Global Constraints bullet names a plan by number, kept as written: {first}"));
            }
            constraints.push(item);
        }
        let numbers: HashMap<u32, u32> = split
            .sections
            .iter()
            .enumerate()
            .map(|(k, s)| (s.old, next + k as u32))
            .collect();
        if let Some(body) = review_focus(text) {
            let body = without_rules(&body);
            if i > 0 {
                for m in stale_mentions(&body, &numbers) {
                    warnings.push(format!(
                        "{path}: its Review Focus mentions \"Task {m}\", which keeps its old number and now points at another task"
                    ));
                }
            }
            if plans.len() > 1 {
                focus.push(format!(
                    "From plan {} ({}):\n\n{body}",
                    i + 1,
                    task_range(next, split.sections.len())
                ));
            } else {
                focus.push(body);
            }
        }
        if i > 0 {
            let goal = goal_line(&split.header).unwrap_or_else(|| plan_title(text));
            goals.push(format!("Plan {} goal: {goal}", i + 1));
        }
        let mut block: Vec<String> = Vec::new();
        for (k, section) in split.sections.iter().enumerate() {
            let new = next + k as u32;
            let body = section.text.lines().skip(1).collect::<Vec<_>>().join("\n");
            for m in if i > 0 {
                stale_mentions(&body, &numbers)
            } else {
                Vec::new()
            } {
                warnings.push(format!(
                    "{path}: Task {} (now Task {new}) mentions \"Task {m}\", which keeps its old number",
                    section.old
                ));
            }
            let mut lines = section.text.lines();
            let heading = renumber(lines.next().unwrap_or_default(), new);
            block.push(
                std::iter::once(heading.as_str())
                    .chain(lines)
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        }
        next += split.sections.len() as u32;
        if !block.is_empty() {
            task_blocks.push(block.join(split.joint).trim().to_string());
        }
        if i == 0 {
            first_parts = Some(parts);
        }
    }
    let first_text = plans.first().map(|(_, t)| t.as_str()).unwrap_or_default();
    let sources: Vec<&str> = plans.iter().map(|(p, _)| p.as_str()).collect();
    let mut out: Vec<String> = vec![
        format!("# {}", plan_title(first_text)),
        format!("<!-- Combined from: {} -->", sources.join(", ")),
    ];
    let first_parts = first_parts.unwrap_or_default();
    out.push(first_parts.preamble.clone());
    out.push(goals.join("\n"));
    if !constraints.is_empty() {
        out.push(format!("{CONSTRAINTS}\n\n{}", constraints.join("\n")));
    }
    if !focus.is_empty() {
        out.push(format!("{FOCUS}\n\n{}", focus.join("\n\n")));
    }
    for (heading, body) in &first_parts.sections {
        if heading.trim_end() != CONSTRAINTS && heading.trim_end() != FOCUS {
            out.push(format!("{heading}\n\n{body}").trim().to_string());
        }
    }
    if !task_blocks.is_empty() {
        out.push("---".to_string());
        out.extend(task_blocks);
    }
    let text = out
        .into_iter()
        .filter(|p| !p.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
        + "\n";
    // The board joins warnings with "; " and the CLI splits them on it, so no warning may hold it.
    let warnings = warnings
        .into_iter()
        .map(|w| w.replace("; ", ", "))
        .collect();
    CombinedPlan {
        text,
        tasks: (next - 1) as usize,
        warnings,
    }
}

/// The new task numbers of a plan with `count` tasks starting at `first`, as `now Tasks A to B`.
fn task_range(first: u32, count: usize) -> String {
    match count {
        0 => "no tasks".to_string(),
        1 => format!("now Task {first}"),
        _ => format!("now Tasks {first} to {}", first + count as u32 - 1),
    }
}

/// Cuts a plan into its header and task sections. `### Task N:` headings win over the checkbox form.
/// With `as_headings`, checkbox task lines are written as `### Task N:` headings.
/// `Next plan:` lines outside code fences are dropped.
fn split_plan(text: &str, as_headings: bool) -> Split {
    let headings = parse_tasks(text);
    if !headings.is_empty() {
        let sections = headings
            .iter()
            .filter_map(|t| task_section(text, t.n).map(|text| Section { old: t.n, text }))
            .collect();
        let mut seen: Vec<u32> = Vec::new();
        let mut repeated: Vec<u32> = Vec::new();
        for (line, _) in lines_with_fence(text)
            .into_iter()
            .filter(|(_, fenced)| !fenced)
        {
            let Some(n) = parse_tasks(line).first().map(|t| t.n) else {
                continue;
            };
            if seen.contains(&n) && !repeated.contains(&n) {
                repeated.push(n);
            }
            seen.push(n);
        }
        return Split {
            header: drop_next_plan(&plan_header(text)),
            sections,
            joint: "\n\n",
            repeated,
        };
    }
    let lines = lines_with_fence(text);
    let task_of = |line: &str, fenced: bool| {
        (!fenced)
            .then(|| parse_checkbox_tasks(line).first().map(|t| t.n))
            .flatten()
    };
    let start = lines
        .iter()
        .position(|(l, fenced)| task_of(l, *fenced).is_some())
        .unwrap_or(lines.len());
    let header = lines[..start]
        .iter()
        .map(|(l, _)| *l)
        .collect::<Vec<_>>()
        .join("\n");
    let mut sections: Vec<(u32, Vec<&str>)> = Vec::new();
    for (line, fenced) in &lines[start..] {
        if !fenced && line.starts_with("Next plan:") {
            continue;
        }
        match task_of(line, *fenced).filter(|n| sections.iter().all(|(old, _)| old != n)) {
            Some(n) => sections.push((n, vec![*line])),
            None => {
                if let Some((_, body)) = sections.last_mut() {
                    body.push(line);
                }
            }
        }
    }
    if as_headings {
        let sections = sections
            .into_iter()
            .map(|(old, lines)| {
                let first = lines
                    .first()
                    .map(|l| l.trim_start_matches("- [ ] ").trim_start_matches("- [x] "))
                    .unwrap_or_default();
                let text = std::iter::once(format!("### {first}"))
                    .chain(lines.iter().skip(1).map(|l| l.to_string()));
                Section {
                    old,
                    text: text.collect::<Vec<_>>().join("\n").trim().to_string(),
                }
            })
            .collect();
        return Split {
            header: drop_next_plan(&header),
            sections,
            joint: "\n\n",
            repeated: Vec::new(),
        };
    }
    let sections = sections
        .into_iter()
        .map(|(old, lines)| Section {
            old,
            text: lines.join("\n"),
        })
        .collect();
    Split {
        header: drop_next_plan(&header),
        sections,
        joint: "\n",
        repeated: Vec::new(),
    }
}

/// The text without its `Next plan:` lines outside code fences, trimmed.
fn drop_next_plan(text: &str) -> String {
    let kept: Vec<&str> = lines_with_fence(text)
        .into_iter()
        .filter(|(l, fenced)| *fenced || !l.starts_with("Next plan:"))
        .map(|(l, _)| l)
        .collect();
    kept.join("\n").trim().to_string()
}

/// A task line with its number set to `n`. The rest of the line is kept.
fn renumber(line: &str, n: u32) -> String {
    let Some(at) = line.find("Task ").map(|i| i + "Task ".len()) else {
        return line.to_string();
    };
    let digits = line[at..].chars().take_while(char::is_ascii_digit).count();
    format!("{}{n}{}", &line[..at], &line[at + digits..])
}

/// The numbers of `Task N` mentions in a text that point at another task once renumbered.
/// Lines inside code fences are skipped. Each number is listed once.
fn stale_mentions(text: &str, numbers: &HashMap<u32, u32>) -> Vec<u32> {
    let mut out = Vec::new();
    for (line, _) in lines_with_fence(text)
        .into_iter()
        .filter(|(_, fenced)| !fenced)
    {
        for (at, _) in line.match_indices("Task ") {
            if line[..at]
                .chars()
                .next_back()
                .is_some_and(char::is_alphanumeric)
            {
                continue;
            }
            let digits: String = line[at + "Task ".len()..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            let Ok(m) = digits.parse::<u32>() else {
                continue;
            };
            if numbers.get(&m) != Some(&m) && !out.contains(&m) {
                out.push(m);
            }
        }
    }
    out
}

/// True when the text names a plan by number, such as `from plan 1`.
fn names_a_plan(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.match_indices("plan ").any(|(at, _)| {
        let before = lower[..at].chars().next_back();
        let after = lower[at + "plan ".len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && after.is_some_and(|c| c.is_ascii_digit())
    })
}

/// The text of the header's `Goal:` line, with any bold marks removed.
fn goal_line(header: &str) -> Option<String> {
    lines_with_fence(header)
        .into_iter()
        .filter(|(_, fenced)| !fenced)
        .find_map(|(line, _)| {
            let t = line.trim().trim_start_matches(['-', '*', ' ']);
            let head = t.get(..5).filter(|h| h.eq_ignore_ascii_case("goal:"))?;
            let rest = t[head.len()..].trim_start_matches(['*', ' ']).trim();
            (!rest.is_empty()).then(|| rest.to_string())
        })
}

/// The header of a plan without its title, cut at its `# ` and `## ` headings.
#[derive(Default)]
struct HeaderParts {
    preamble: String,
    sections: Vec<(String, String)>,
}

impl HeaderParts {
    /// The body of the section with this heading, empty when there is none.
    fn section(&self, heading: &str) -> &str {
        self.sections
            .iter()
            .find(|(h, _)| h.trim_end() == heading)
            .map(|(_, b)| b.as_str())
            .unwrap_or_default()
    }
}

/// Cuts a header into the text before its first section and its sections. The first `# ` line is the
/// title and is left out. A trailing `---` rule is dropped from every part.
fn header_parts(header: &str) -> HeaderParts {
    let mut preamble: Vec<&str> = Vec::new();
    let mut sections: Vec<(&str, Vec<&str>)> = Vec::new();
    let mut titled = false;
    for (line, fenced) in lines_with_fence(header) {
        if !fenced && !titled && line.starts_with("# ") {
            titled = true;
        } else if !fenced && (line.starts_with("# ") || line.starts_with("## ")) {
            sections.push((line, Vec::new()));
        } else if let Some((_, body)) = sections.last_mut() {
            body.push(line);
        } else {
            preamble.push(line);
        }
    }
    HeaderParts {
        preamble: without_rules(&preamble.join("\n")),
        sections: sections
            .into_iter()
            .map(|(h, body)| (h.to_string(), without_rules(&body.join("\n"))))
            .collect(),
    }
}

/// The text trimmed, without trailing `---` rule lines.
fn without_rules(text: &str) -> String {
    let mut t = text.trim();
    while let Some(rest) = t.strip_suffix("---") {
        if !rest.is_empty() && !rest.ends_with('\n') {
            break;
        }
        t = rest.trim();
    }
    t.to_string()
}

/// The items of a constraints section: each bullet with its continuation lines, or a paragraph.
fn section_items(body: &str) -> Vec<String> {
    let mut items: Vec<Vec<&str>> = Vec::new();
    let mut open = false;
    for (line, fenced) in lines_with_fence(body) {
        if !fenced && line.trim().is_empty() {
            open = false;
            continue;
        }
        let starts = !fenced && (line.starts_with("- ") || line.starts_with("* "));
        match items.last_mut() {
            Some(item) if open && !starts => item.push(line.trim_end()),
            _ => items.push(vec![line.trim_end()]),
        }
        open = true;
    }
    items.into_iter().map(|lines| lines.join("\n")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a plan text in the shape the planner writes.
    fn plan(
        title: &str,
        goal: &str,
        constraints: &[&str],
        focus: &[&str],
        tasks: &[&str],
    ) -> String {
        let mut out = format!("# {title}\n\n**Goal:** {goal}\n\n**Spec:** `docs/spec.md`\n\n");
        if !constraints.is_empty() {
            out.push_str("## Global Constraints\n\n");
            for c in constraints {
                out.push_str(&format!("- {c}\n"));
            }
            out.push('\n');
        }
        if !focus.is_empty() {
            out.push_str("## Review Focus\n\n");
            for (i, f) in focus.iter().enumerate() {
                out.push_str(&format!("{}. {f}\n", i + 1));
            }
            out.push('\n');
        }
        out.push_str("---\n\n");
        for (i, t) in tasks.iter().enumerate() {
            out.push_str(&format!(
                "### Task {}: {t}\n\nBody of {t}.\n\n```rust\nfn {}() {{}}\n```\n\n",
                i + 1,
                t.replace(' ', "_")
            ));
        }
        out.push_str("Next plan: none\n");
        out
    }

    fn two(a: &str, b: &str) -> Vec<(String, String)> {
        vec![
            ("plans/plan-01.md".to_string(), a.to_string()),
            ("plans/plan-02.md".to_string(), b.to_string()),
        ]
    }

    #[test]
    fn repeated_task_numbers_across_plans_are_renumbered() {
        let a = plan(
            "Core",
            "build the core",
            &[],
            &[],
            &["a one", "a two", "a three"],
        );
        let b = plan(
            "Engine",
            "wire the engine",
            &[],
            &[],
            &["b one", "b two", "b three"],
        );
        let got = combine_plans(&two(&a, &b));
        assert_eq!(got.tasks, 6);
        let tasks = parse_tasks(&got.text);
        assert_eq!(
            tasks.iter().map(|t| t.n).collect::<Vec<_>>(),
            (1..=6).collect::<Vec<_>>()
        );
        let titles: Vec<&str> = tasks.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(
            titles,
            vec!["a one", "a two", "a three", "b one", "b two", "b three"]
        );
        assert!(
            got.text.contains("Body of b three.") && got.text.contains("fn b_three() {}"),
            "{}",
            got.text
        );
    }

    #[test]
    fn checkbox_form_tasks_are_renumbered() {
        let a = "# Old A\n\n**Goal:** old a.\n\n- [ ] Task 1: first\n- [ ] Task 2: second\n\nNext plan: plan-02.md\n";
        let b = "# Old B\n\n**Goal:** old b.\n\n- [ ] Task 1: third\n- [x] Task 2: fourth\n\nNext plan: none\n";
        let got = combine_plans(&two(a, b));
        assert_eq!(got.tasks, 4);
        let tasks = parse_checkbox_tasks(&got.text);
        assert_eq!(
            tasks.iter().map(|t| t.n).collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
        assert_eq!(
            tasks.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(),
            vec!["first", "second", "third", "fourth"]
        );
        assert!(got.text.contains("- [x] Task 4: fourth"), "{}", got.text);
    }

    #[test]
    fn the_text_has_one_header_and_no_next_plan_line() {
        let a = plan(
            "Core",
            "build the core",
            &["Use -j4."],
            &["Check one."],
            &["a one"],
        )
        .replace("Next plan: none", "Next plan: plan-02.md");
        let b = plan(
            "Engine",
            "wire the engine",
            &["Use -j4."],
            &["Check two."],
            &["b one"],
        );
        let got = combine_plans(&two(&a, &b));
        let titles: Vec<&str> = got.text.lines().filter(|l| l.starts_with("# ")).collect();
        assert_eq!(titles, vec!["# Core"], "{}", got.text);
        let mut lines = got
            .text
            .lines()
            .skip_while(|l| !l.starts_with("# "))
            .skip(1)
            .filter(|l| !l.trim().is_empty());
        assert_eq!(
            lines.next(),
            Some("<!-- Combined from: plans/plan-01.md, plans/plan-02.md -->")
        );
        let header = plan_header(&got.text);
        assert!(header.contains("Plan 2 goal: wire the engine"), "{header}");
        assert!(header.contains("**Goal:** build the core"), "{header}");
        assert!(
            !got.text.lines().any(|l| l.starts_with("Next plan:")),
            "{}",
            got.text
        );
    }

    #[test]
    fn global_constraints_are_merged_without_duplicates() {
        let a = plan("Core", "g", &["Use -j4.", "Only in one."], &[], &["a one"]);
        let b = plan(
            "Engine",
            "g",
            &["Use -j4.", "Only in two."],
            &[],
            &["b one"],
        );
        let got = combine_plans(&two(&a, &b));
        assert_eq!(got.text.matches("- Use -j4.").count(), 1, "{}", got.text);
        assert_eq!(
            got.text.matches("## Global Constraints").count(),
            1,
            "{}",
            got.text
        );
        let one = got.text.find("- Only in one.").expect("plan 1 bullet");
        let two = got.text.find("- Only in two.").expect("plan 2 bullet");
        let first = got.text.find("- Use -j4.").expect("shared bullet");
        assert!(first < one && one < two, "{}", got.text);
        assert!(got.warnings.is_empty(), "{:?}", got.warnings);
    }

    #[test]
    fn review_focus_sections_are_merged() {
        let a = plan(
            "Core",
            "g",
            &[],
            &["Check the lease.", "Check the ledger."],
            &["a one"],
        );
        let b = plan("Engine", "g", &[], &["Check the board."], &["b one"]);
        let got = combine_plans(&two(&a, &b));
        assert_eq!(
            got.text.matches("## Review Focus").count(),
            1,
            "{}",
            got.text
        );
        let focus = review_focus(&got.text).expect("focus");
        for line in [
            "1. Check the lease.",
            "2. Check the ledger.",
            "1. Check the board.",
        ] {
            assert!(focus.contains(line), "{focus}");
        }
        let first = focus
            .find("From plan 1 (now Task 1):")
            .expect("plan 1 label");
        let second = focus
            .find("From plan 2 (now Task 2):")
            .expect("plan 2 label");
        let lease = focus.find("1. Check the lease.").expect("plan 1 item");
        let board = focus.find("1. Check the board.").expect("plan 2 item");
        assert!(first < lease && lease < second && second < board, "{focus}");
    }

    #[test]
    fn late_review_focus_survives_combining_two_plans() {
        let a = plan("Core", "g", &[], &[], &["a one"]).replace(
            "Next plan: none",
            "## Review Focus\n\n1. Check the lease after a retry.\n\nNext plan: plan-02.md",
        );
        let b = plan("Engine", "g", &[], &[], &["b one"]).replace(
            "Next plan: none",
            "## Review Focus\n\n1. Check the board after merge.\n\nNext plan: none",
        );
        let got = combine_plans(&two(&a, &b));
        let focus = review_focus(&got.text).expect("combined review focus");
        assert!(focus.contains("Check the lease after a retry."), "{focus}");
        assert!(focus.contains("Check the board after merge."), "{focus}");
    }

    #[test]
    fn focus_labels_name_the_task_range_of_each_plan() {
        let a = plan("Core", "g", &[], &["Check the lease."], &["a one", "a two"]);
        let b = plan("Engine", "g", &[], &[], &["b one"]);
        let c = plan(
            "Cli",
            "g",
            &[],
            &["Check the cli."],
            &["c one", "c two", "c three"],
        );
        let plans = vec![
            ("plans/plan-01.md".to_string(), a),
            ("plans/plan-02.md".to_string(), b),
            ("plans/plan-03.md".to_string(), c),
        ];
        let got = combine_plans(&plans);
        let focus = review_focus(&got.text).expect("focus");
        assert!(
            focus.starts_with("From plan 1 (now Tasks 1 to 2):\n\n1. Check the lease."),
            "{focus}"
        );
        assert!(
            focus.contains("From plan 3 (now Tasks 4 to 6):\n\n1. Check the cli."),
            "{focus}"
        );
        assert!(!focus.contains("From plan 2"), "{focus}");
    }

    #[test]
    fn a_focus_mention_of_a_renumbered_task_warns() {
        let a = plan(
            "Core",
            "g",
            &[],
            &["Lease. Pinned in Task 1 (`lease`)."],
            &["a one", "a two"],
        );
        let b = plan(
            "Engine",
            "g",
            &[],
            &["Board. Pinned in Task 1 (`board`)."],
            &["b one"],
        );
        let got = combine_plans(&two(&a, &b));
        assert_eq!(got.warnings.len(), 1, "{:?}", got.warnings);
        let w = &got.warnings[0];
        assert!(
            w.contains("plans/plan-02.md")
                && w.contains("Review Focus")
                && w.contains("\"Task 1\""),
            "{w}"
        );
        assert!(
            got.text.contains("1. Board. Pinned in Task 1 (`board`)."),
            "{}",
            got.text
        );
    }

    /// Asserts the combined text has `### Task` headings numbered exactly `1..=tasks`.
    fn assert_headings_cover(got: &CombinedPlan) {
        let numbers: Vec<u32> = parse_tasks(&got.text).iter().map(|t| t.n).collect();
        assert_eq!(
            numbers,
            (1..=got.tasks as u32).collect::<Vec<_>>(),
            "{}",
            got.text
        );
    }

    const CHECKBOX: &str = "# Old\n\n**Goal:** old.\n\n- [ ] Task 1: first\n  Indented note.\n- [x] Task 2: second\n\nNext plan: none\n";

    #[test]
    fn checkbox_then_heading_plans_keep_every_task() {
        let b = plan("Engine", "g", &[], &[], &["third", "fourth"]);
        let got = combine_plans(&two(CHECKBOX, &b));
        assert_eq!(got.tasks, 4);
        assert_headings_cover(&got);
        let titles: Vec<String> = parse_tasks(&got.text)
            .into_iter()
            .map(|t| t.title)
            .collect();
        assert_eq!(titles, vec!["first", "second", "third", "fourth"]);
        assert!(
            task_section(&got.text, 1)
                .expect("task 1")
                .contains("Indented note."),
            "{}",
            got.text
        );
        assert!(
            task_section(&got.text, 4)
                .expect("task 4")
                .contains("Body of fourth."),
            "{}",
            got.text
        );
        assert_eq!(got.warnings.len(), 1, "{:?}", got.warnings);
        assert!(
            got.warnings[0].contains("plans/plan-01.md") && got.warnings[0].contains("### Task"),
            "{:?}",
            got.warnings
        );
    }

    #[test]
    fn heading_then_checkbox_plans_keep_every_task() {
        let a = plan("Core", "g", &[], &[], &["first", "second"]);
        let got = combine_plans(&two(&a, CHECKBOX));
        assert_eq!(got.tasks, 4);
        assert_headings_cover(&got);
        let titles: Vec<String> = parse_tasks(&got.text)
            .into_iter()
            .map(|t| t.title)
            .collect();
        assert_eq!(titles, vec!["first", "second", "first", "second"]);
        let two_text = task_section(&got.text, 2).expect("task 2");
        assert!(
            !two_text.contains("Task 3") && !two_text.contains("Task 4"),
            "{two_text}"
        );
        assert_eq!(got.warnings.len(), 1, "{:?}", got.warnings);
        assert!(
            got.warnings[0].contains("plans/plan-02.md"),
            "{:?}",
            got.warnings
        );
    }

    #[test]
    fn no_warning_contains_a_semicolon_joint() {
        let a = plan("Core", "g", &[], &[], &["a one"]);
        let b = plan(
            "Engine",
            "g",
            &["Use the fields from plan 1; do not redefine them."],
            &[],
            &["b one"],
        )
        .replace("Body of b one.", "Body of b one, see Task 1.");
        let plans = vec![
            ("plans/a; b.md".to_string(), a),
            ("plans/c; d.md".to_string(), b),
        ];
        let got = combine_plans(&plans);
        assert_eq!(got.warnings.len(), 2, "{:?}", got.warnings);
        assert!(
            got.warnings.iter().all(|w| !w.contains("; ")),
            "{:?}",
            got.warnings
        );
        assert!(
            got.text
                .contains("- Use the fields from plan 1; do not redefine them."),
            "{}",
            got.text
        );
    }

    #[test]
    fn a_repeated_heading_number_in_one_plan_warns() {
        let a = plan("Core", "g", &[], &[], &["a one", "a two", "a three"])
            .replace("### Task 3: a three", "### Task 2: a three");
        let b = plan("Engine", "g", &[], &[], &["b one"]);
        let got = combine_plans(&two(&a, &b));
        assert_eq!(got.tasks, 3);
        assert_headings_cover(&got);
        assert_eq!(got.warnings.len(), 1, "{:?}", got.warnings);
        let w = &got.warnings[0];
        assert!(
            w.contains("plans/plan-01.md") && w.contains("Task 2"),
            "{w}"
        );
    }

    #[test]
    fn fenced_lines_are_left_alone() {
        let a = plan("Core", "g", &[], &[], &["a one", "a two"]);
        let fence = "Body of b one.\n\n```text\n### Task 9: not a task\nNext plan: not a link\nsee Task 2\n```";
        let b = plan("Engine", "g", &[], &[], &["b one", "b two"]).replace("Body of b one.", fence);
        let got = combine_plans(&two(&a, &b));
        assert_eq!(got.tasks, 4);
        assert_headings_cover(&got);
        let three = task_section(&got.text, 3).expect("task 3");
        assert!(
            three.contains("### Task 9: not a task") && three.contains("Next plan: not a link"),
            "{three}"
        );
        assert!(got.warnings.is_empty(), "{:?}", got.warnings);
    }

    #[test]
    fn plan_one_mentions_never_warn() {
        let a = plan(
            "Core",
            "g",
            &[],
            &["Pinned in Task 7."],
            &["a one", "a two"],
        )
        .replace("Body of a one.", "Body of a one, see Task 7.");
        let b = plan("Engine", "g", &[], &[], &["b one"]);
        let got = combine_plans(&two(&a, &b));
        assert!(got.warnings.is_empty(), "{:?}", got.warnings);
    }

    #[test]
    fn plans_without_focus_or_constraints_combine() {
        let a = plan("Core", "g", &[], &[], &["a one"]);
        let b = plan("Engine", "g", &[], &[], &["b one", "b two"]);
        let got = combine_plans(&two(&a, &b));
        assert_eq!(got.tasks, 3);
        assert_headings_cover(&got);
        assert!(
            !got.text.contains(FOCUS) && !got.text.contains(CONSTRAINTS),
            "{}",
            got.text
        );
        assert_eq!(review_focus(&got.text), None);
        assert!(got.warnings.is_empty(), "{:?}", got.warnings);
    }

    #[test]
    fn windows_line_endings_combine() {
        let a = plan(
            "Core",
            "build the core",
            &["Use -j4."],
            &["Check one."],
            &["a one", "a two"],
        )
        .replace('\n', "\r\n");
        let b = plan(
            "Engine",
            "wire the engine",
            &["Use -j4."],
            &["Check two."],
            &["b one"],
        )
        .replace('\n', "\r\n");
        let got = combine_plans(&two(&a, &b));
        assert_eq!(got.tasks, 3);
        assert_headings_cover(&got);
        assert!(!got.text.contains('\r'), "{:?}", got.text);
        assert_eq!(got.text.matches("- Use -j4.").count(), 1, "{}", got.text);
        assert!(
            got.text.contains("Plan 2 goal: wire the engine"),
            "{}",
            got.text
        );
        assert!(
            !got.text.lines().any(|l| l.starts_with("Next plan:")),
            "{}",
            got.text
        );
        assert!(got.warnings.is_empty(), "{:?}", got.warnings);
    }

    #[test]
    fn an_empty_plan_list_gives_an_empty_plan() {
        let got = combine_plans(&[]);
        assert_eq!(got.tasks, 0);
        assert!(got.warnings.is_empty(), "{:?}", got.warnings);
        assert!(parse_tasks(&got.text).is_empty(), "{}", got.text);
    }

    #[test]
    fn a_body_mention_of_another_task_warns() {
        let a = plan("Core", "g", &[], &[], &["a one", "a two", "a three"]);
        let b = plan("Engine", "g", &[], &[], &["b one", "b two"])
            .replace("Body of b one.", "Body of b one, see Task 2.");
        let got = combine_plans(&two(&a, &b));
        assert_eq!(got.warnings.len(), 1, "{:?}", got.warnings);
        let w = &got.warnings[0];
        assert!(
            w.contains("Task 2")
                && w.contains("plans/plan-02.md")
                && w.contains("Task 1")
                && w.contains("Task 4"),
            "{w}"
        );
        assert!(
            got.text.contains("Body of b one, see Task 2."),
            "{}",
            got.text
        );
    }

    #[test]
    fn a_mention_that_keeps_its_meaning_does_not_warn() {
        let a = plan("Core", "g", &[], &[], &["a one", "a two"])
            .replace("Body of a two.", "Body of a two, after Task 1.");
        let b = plan("Engine", "g", &[], &[], &["b one"]);
        let got = combine_plans(&two(&a, &b));
        assert!(got.warnings.is_empty(), "{:?}", got.warnings);
    }

    #[test]
    fn a_constraint_that_names_another_plan_warns() {
        let a = plan("Core", "g", &["Use -j4."], &[], &["a one"]);
        let b = plan(
            "Engine",
            "g",
            &[
                "Use -j4.",
                "Use the `Config` fields from plan 1, do not redefine them.",
            ],
            &[],
            &["b one"],
        );
        let got = combine_plans(&two(&a, &b));
        assert_eq!(got.warnings.len(), 1, "{:?}", got.warnings);
        assert!(
            got.warnings[0].contains("from plan 1"),
            "{:?}",
            got.warnings
        );
        assert!(
            got.text
                .contains("- Use the `Config` fields from plan 1, do not redefine them."),
            "{}",
            got.text
        );
    }

    #[test]
    fn one_plan_combines_to_itself() {
        let a = plan(
            "Core",
            "build the core",
            &["Use -j4."],
            &["Check one."],
            &["a one", "a two"],
        );
        let got = combine_plans(&[("plans/plan-01.md".to_string(), a.clone())]);
        assert_eq!(got.tasks, 2);
        assert_eq!(parse_tasks(&got.text), parse_tasks(&a));
        assert!(!got.text.contains("Plan 2 goal:"), "{}", got.text);
        assert!(got.warnings.is_empty(), "{:?}", got.warnings);
        assert_eq!(review_focus(&got.text), review_focus(&a));
        assert!(
            got.text.contains("- Use -j4.") && got.text.contains("Body of a two."),
            "{}",
            got.text
        );
    }
}
