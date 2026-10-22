//! Plan lint: checks a plan against the format the factory builders rely on.

use crate::runloop::find_placeholder;
use crate::tasks::{lines_with_fence, parse_tasks, plan_header, task_section};

/// What the lint found. A refusal stops `factory new`, a warning never does.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlanLint {
    pub refuse: Option<String>,
    pub warnings: Vec<String>,
}

/// The header fields every plan names.
const HEADER_FIELDS: [&str; 6] = [
    "Goal",
    "Architecture",
    "Tech Stack",
    "Spec",
    "Global Constraints",
    "Review Focus",
];

/// The executor the plan header must name.
const EXECUTOR: &str = "superpowers:subagent-driven-development";
const FACTORY_EXECUTOR: &str =
    "**Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review.";

/// True when a header line names the field as `**Field:**`, `Field:` or a heading.
fn names_field(line: &str, field: &str) -> bool {
    let t = line.trim().trim_start_matches('>').trim();
    let bare = t.trim_start_matches('#').trim_start_matches("**").trim();
    bare.strip_prefix(field).is_some_and(|rest| {
        let rest = rest.trim_start_matches("**");
        let heading = t.starts_with('#') && rest.trim().is_empty();
        heading || rest.starts_with(':')
    })
}

/// True when the text has the marker outside code fences.
fn has_outside_fence(text: &str, marker: &str) -> bool {
    lines_with_fence(text)
        .iter()
        .any(|(l, fenced)| !fenced && l.contains(marker))
}

/// True for a step line whose title is `Commit`: `### Step N: Commit` or `- [ ] **Step N: Commit**`.
/// Only the title counts, so an identifier such as `BuilderCommitted` in the step text is fine.
fn is_commit_step(line: &str) -> bool {
    let title = if let Some(rest) = line.strip_prefix('#') {
        rest.trim_start_matches('#').trim()
    } else if let Some(rest) = line
        .strip_prefix("- [ ] **")
        .or_else(|| line.strip_prefix("- [x] **"))
    {
        rest.split("**").next().unwrap_or("")
    } else {
        return false;
    };
    let Some(rest) = title.strip_prefix("Step ") else {
        return false;
    };
    let Some((_, name)) = rest.split_once(':') else {
        return false;
    };
    name.trim()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .next()
        == Some("Commit")
}

/// Lints a plan. Warning texts never contain `; ` because `factory new` splits on it.
pub fn lint_plan(plan: &str, name: &str) -> PlanLint {
    let mut lint = PlanLint::default();
    let name = &name.replace("; ", ", ");
    if let Some((line, word)) = find_placeholder(plan) {
        lint.refuse = Some(format!(
            "{name} line {line} still has `{word}`, finish it or remove it"
        ));
        return lint;
    }
    let header = plan_header(plan);
    let header_lines = lines_with_fence(&header);
    let compact = header.contains(FACTORY_EXECUTOR);
    let required: &[&str] = if compact {
        &["Spec", "File Structure"]
    } else {
        &HEADER_FIELDS
    };
    for field in required {
        if !header_lines
            .iter()
            .any(|(l, fenced)| !fenced && names_field(l, field))
        {
            lint.warnings.push(format!("{name} header has no {field}"));
        }
    }
    if compact
        && !lines_with_fence(plan)
            .iter()
            .any(|(l, fenced)| !fenced && names_field(l, "Review Focus"))
    {
        lint.warnings.push(format!("{name} has no Review Focus"));
    }
    if !header.contains(EXECUTOR) && !compact {
        lint.warnings
            .push(format!("{name} does not name {EXECUTOR} as its executor"));
    }
    for task in parse_tasks(plan) {
        let section = task_section(plan, task.n).unwrap_or_default();
        let markers: &[&str] = if compact {
            &["**Interfaces:**"]
        } else {
            &["**Files:**", "**Interfaces:**"]
        };
        for marker in markers {
            if !has_outside_fence(&section, marker) {
                lint.warnings
                    .push(format!("{name} task {} has no {marker}", task.n));
            }
        }
    }
    let commit = lines_with_fence(plan).into_iter().find(|(l, fenced)| {
        let t = l.trim_start();
        !fenced && (t.starts_with("git commit") || is_commit_step(t))
    });
    if let Some((line, _)) = commit {
        lint.warnings.push(format!(
            "{name} has a commit step (`{}`) but the driver commits",
            line.trim()
        ));
    }
    // `factory new` splits the stored warnings on `; `, so no warning may contain it.
    for warning in &mut lint.warnings {
        *warning = warning.replace("; ", ", ");
    }
    lint
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = include_str!("../../../docs/superpowers/plans/2026-10-18-factory-execution-rework/plan-01-count-real-task-headings.md");

    fn task(n: u32, files: bool, interfaces: bool) -> String {
        let mut s = format!("### Task {n}: t\n\n");
        if files {
            s.push_str("**Files:**\n- Modify: `a.rs`\n\n");
        }
        if interfaces {
            s.push_str("**Interfaces:**\n- Produces: x\n\n");
        }
        s
    }

    #[test]
    fn a_placeholder_outside_code_refuses() {
        let lint = lint_plan("# P\n\nTBD later\n", "plan-01.md");
        let reason = lint.refuse.expect("refusal");
        assert!(
            reason.contains("plan-01.md") && reason.contains("TBD"),
            "{reason}"
        );
    }

    #[test]
    fn a_placeholder_in_a_fence_or_backticks_is_fine() {
        let plan =
            format!("{GOOD}\n```\nTODO inside a fence\n```\n\nThe word `FIXME` is quoted.\n");
        assert_eq!(lint_plan(&plan, "p.md").refuse, None);
    }

    #[test]
    fn a_complete_plan_in_this_repos_format_has_no_warnings() {
        assert_eq!(lint_plan(GOOD, "p.md"), PlanLint::default());
    }

    #[test]
    fn a_codex_factory_plan_with_a_file_map_and_late_review_focus_has_no_warnings() {
        let plan = "# Plan 01: smoke test\n\n> **Spec:** docs/superpowers/specs/smoke-design.md\n> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review. The driver keeps the ledger, makes the commits and runs the final review.\n\n## File Structure\n\n- `spec/health_spec.rb` — request coverage.\n\n### Task 1: health route\n\n**Interfaces:**\n\n- Request: `GET /`.\n\n## Review Focus\n\n1. Root requests return HTTP 200.\n\nNext plan: none\n";
        assert_eq!(lint_plan(plan, "plan-01-smoke.md"), PlanLint::default());
    }

    #[test]
    fn each_missing_header_field_is_one_warning() {
        let plan = format!(
            "# P\n\n> executor {EXECUTOR}\n\n**Goal:** g\n\n{}",
            task(1, true, true)
        );
        let lint = lint_plan(&plan, "p.md");
        assert_eq!(lint.warnings.len(), 5, "{:?}", lint.warnings);
        assert!(lint.warnings.iter().any(|w| w.contains("Review Focus")));
        assert!(!lint.warnings.iter().any(|w| w.contains("no Goal")));
        assert!(lint.warnings.iter().all(|w| !w.contains("; ")));
    }

    #[test]
    fn a_missing_executor_line_warns() {
        let plan = GOOD.replace(EXECUTOR, "something else");
        let lint = lint_plan(&plan, "p.md");
        assert_eq!(lint.warnings.len(), 1, "{:?}", lint.warnings);
        assert!(lint.warnings[0].contains(EXECUTOR));
    }

    #[test]
    fn a_task_without_files_or_interfaces_warns() {
        let header = GOOD.split("### Task 1").next().expect("header");
        let plan = format!(
            "{header}{}{}\nNext plan: none\n",
            task(1, false, true),
            task(2, true, false)
        );
        let lint = lint_plan(&plan, "p.md");
        assert_eq!(lint.warnings.len(), 2, "{:?}", lint.warnings);
        assert!(lint.warnings[0].contains("task 1") && lint.warnings[0].contains("Files"));
        assert!(lint.warnings[1].contains("task 2") && lint.warnings[1].contains("Interfaces"));
    }

    #[test]
    fn a_commit_word_inside_an_identifier_or_task_title_is_fine() {
        let plan = format!(
            "{GOOD}\n- [ ] **Step 4: Implement** the `BuilderCommitted` flag.\n\n### Task 3: Commit message parsing\n"
        );
        assert_eq!(
            lint_plan(&plan, "p.md")
                .warnings
                .iter()
                .filter(|w| w.contains("commit step"))
                .count(),
            0
        );
        let plan = format!("{GOOD}\n- [ ] **Step 5: Commit**\n");
        assert_eq!(lint_plan(&plan, "p.md").warnings.len(), 1);
    }

    #[test]
    fn no_warning_contains_the_split_separator() {
        let plan = format!("{GOOD}\ngit commit -m \"a; b\"\n");
        let lint = lint_plan(&plan, "my; plan.md");
        assert!(!lint.warnings.is_empty());
        assert!(
            lint.warnings.iter().all(|w| !w.contains("; ")),
            "{:?}",
            lint.warnings
        );
        let lint = lint_plan("# P\n", "my; plan.md");
        assert!(
            lint.warnings.iter().all(|w| !w.contains("; ")),
            "{:?}",
            lint.warnings
        );
    }

    #[test]
    fn a_second_real_plan_has_no_warnings() {
        let plan = include_str!("../../../docs/superpowers/plans/2026-10-18-factory-execution-rework/plan-04-per-task-checks-and-slim-agents.md");
        assert_eq!(lint_plan(plan, "p.md"), PlanLint::default());
        let plan = include_str!("../../../docs/superpowers/plans/2026-10-18-factory-execution-rework/plan-02-builder-status-and-park.md");
        assert_eq!(lint_plan(plan, "p.md"), PlanLint::default());
    }

    #[test]
    fn a_commit_step_warns_but_a_fenced_one_does_not() {
        let plan = format!("{GOOD}\n- [ ] **Step 9: Commit**\n");
        assert_eq!(lint_plan(&plan, "p.md").warnings.len(), 1);
        let plan = format!("{GOOD}\ngit commit -m x\n");
        assert_eq!(lint_plan(&plan, "p.md").warnings.len(), 1);
        let fenced = format!("{GOOD}\n```\ngit commit -m x\n### Task 7: x\n```\n");
        assert_eq!(lint_plan(&fenced, "p.md"), PlanLint::default());
    }

    #[test]
    fn lint_texts_the_dashboard_filters_on() {
        // dashboard/src/state.rs filters on these texts, update both together.
        let lint = lint_plan("# P\n\n### Task 1: t\n\nbody\n", "x.md");
        let mut expected: Vec<String> = HEADER_FIELDS
            .iter()
            .map(|f| format!("x.md header has no {f}"))
            .collect();
        expected.push(format!("x.md does not name {EXECUTOR} as its executor"));
        expected.push("x.md task 1 has no **Files:**".to_string());
        expected.push("x.md task 1 has no **Interfaces:**".to_string());
        assert_eq!(lint.warnings, expected);
    }
}
