/// One task heading of a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanTask {
    pub n: u32,
    pub title: String,
}

/// Reads `### Task N: title` and `- [ ] Task N: title` lines outside code fences, in order. A repeated number counts once.
pub fn parse_checkbox_tasks(plan: &str) -> Vec<PlanTask> {
    collect_tasks(plan, |_| true)
}

/// Collects the tasks of the lines outside code fences that pass `accept`.
fn collect_tasks(plan: &str, accept: impl Fn(&str) -> bool) -> Vec<PlanTask> {
    let mut out: Vec<PlanTask> = Vec::new();
    for (line, fenced) in lines_with_fence(plan) {
        if fenced || !accept(line) {
            continue;
        }
        let t = line
            .trim_start_matches("- [ ] ")
            .trim_start_matches("- [x] ")
            .trim_start_matches("### ");
        let Some(rest) = t.strip_prefix("Task ") else {
            continue;
        };
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        let Ok(n) = digits.parse::<u32>() else {
            continue;
        };
        let Some(title) = rest[digits.len()..].strip_prefix(':') else {
            continue;
        };
        if out.iter().any(|t| t.n == n) {
            continue;
        }
        out.push(PlanTask {
            n,
            title: title.trim().to_string(),
        });
    }
    out
}

/// Reads `### Task N: title` headings outside code fences, in order. A repeated number counts once.
pub fn parse_tasks(plan: &str) -> Vec<PlanTask> {
    collect_tasks(plan, is_task_heading)
}

/// The text of the first `# ` heading of a plan, or `plan` when it has none.
pub fn plan_title(plan: &str) -> String {
    plan.lines()
        .find_map(|line| line.strip_prefix("# "))
        .map(|title| title.trim().to_string())
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| "plan".to_string())
}

/// The goal of a spec for the committer prompt: a `Goal:` line, or else the first line that is
/// neither blank nor a heading, cut to 200 characters. Empty when there is none.
pub fn spec_goal(spec: &str) -> String {
    let cut = |text: &str| text.chars().take(200).collect::<String>();
    let mut first_plain: Option<String> = None;
    for line in spec.lines() {
        let t = line.trim().trim_start_matches(['-', '*', ' ']);
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if t.len() >= 5 && t.is_char_boundary(5) && t[..5].eq_ignore_ascii_case("goal:") {
            let rest = t[5..].trim_start_matches(['*', ' ']).trim();
            if !rest.is_empty() {
                return cut(rest);
            }
        }
        if first_plain.is_none() {
            first_plain = Some(cut(t));
        }
    }
    first_plain.unwrap_or_default()
}

/// True for a `### Task N:` heading line.
fn is_task_heading(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("### Task ") else {
        return false;
    };
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    !digits.is_empty() && rest[digits.len()..].starts_with(':')
}

/// The lines of a text, each with whether it sits inside a fenced code block.
/// A fence line itself counts as the text around the block, not as inside it.
/// `str::lines` drops a trailing carriage return, so Windows line endings need no extra care.
pub(crate) fn lines_with_fence(text: &str) -> Vec<(&str, bool)> {
    let mut in_fence = false;
    text.lines()
        .map(|line| {
            if line.trim_start().starts_with("```") {
                in_fence = !in_fence;
                return (line, false);
            }
            (line, in_fence)
        })
        .collect()
}

/// The text of a plan before its first `### Task` heading, trimmed. The whole plan when it has no such heading.
/// Headings inside fenced code blocks are ignored.
pub fn plan_header(plan: &str) -> String {
    let lines = lines_with_fence(plan);
    let end = lines
        .iter()
        .position(|(l, fenced)| !fenced && is_task_heading(l))
        .unwrap_or(lines.len());
    lines[..end]
        .iter()
        .map(|(l, _)| *l)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// True for the `## Review Focus` heading line.
fn is_focus_heading(line: &str) -> bool {
    line.trim_end() == "## Review Focus"
}

/// True for a `# ` or `## ` heading line, which ends a header section.
fn ends_section(line: &str) -> bool {
    line.starts_with("# ") || line.starts_with("## ")
}

/// The body of the plan's `## Review Focus` section, up to the next top-level heading or
/// `Next plan:` line outside a fenced block. `None` when the section is absent or empty.
pub fn review_focus(plan: &str) -> Option<String> {
    let lines = lines_with_fence(plan);
    let start = lines
        .iter()
        .position(|(l, fenced)| !fenced && is_focus_heading(l))?;
    let body: Vec<&str> = lines[start + 1..]
        .iter()
        .take_while(|(l, fenced)| {
            *fenced || (!ends_section(l) && !is_task_heading(l) && !l.starts_with("Next plan:"))
        })
        .map(|(l, _)| *l)
        .collect();
    let body = body.join("\n").trim().to_string();
    (!body.is_empty()).then_some(body)
}

#[cfg(test)]
mod late_review_focus_tests {
    use super::*;

    #[test]
    fn review_focus_after_the_last_task_reaches_the_reviewer() {
        let plan = "# Plan\n\n### Task 1: work\n\n**Interfaces:**\n- Request: GET /.\n\n## Review Focus\n\n1. Root requests return HTTP 200.\n\nNext plan: none\n";
        assert_eq!(
            review_focus(plan).as_deref(),
            Some("1. Root requests return HTTP 200.")
        );
        assert!(!task_section(plan, 1)
            .expect("task")
            .contains("Review Focus"));
    }

    #[test]
    fn review_focus_before_tasks_stops_at_the_first_task() {
        let plan = "# Plan\n\n## Review Focus\n\n1. Root requests return HTTP 200.\n\n### Task 1: work\n\n**Interfaces:**\n- Request: GET /.\n";
        assert_eq!(
            review_focus(plan).as_deref(),
            Some("1. Root requests return HTTP 200.")
        );
    }
}

/// The plan header without its `## Review Focus` section, then each `### Task N:` heading line.
/// The reviewer gets the focus separately and looks a step up in the plan file when it needs one.
pub fn plan_outline(plan: &str) -> String {
    let mut header: Vec<&str> = Vec::new();
    let mut headings: Vec<&str> = Vec::new();
    let mut in_focus = false;
    for (line, fenced) in lines_with_fence(plan) {
        if !fenced && is_task_heading(line) {
            headings.push(line);
        } else if headings.is_empty() {
            if !fenced && is_focus_heading(line) {
                in_focus = true;
            } else if !fenced && ends_section(line) {
                in_focus = false;
            }
            if !in_focus {
                header.push(line);
            }
        }
    }
    let header = header.join("\n").trim().to_string();
    if headings.is_empty() {
        return header;
    }
    format!("{header}\n\n{}", headings.join("\n"))
        .trim()
        .to_string()
}

/// The heading and body of `### Task N:`, up to the next task heading, a `Next plan:` line or the end.
/// `None` when the plan has no such heading. Inner headings such as `### Step` stay in the section.
/// Headings and `Next plan:` lines inside fenced code blocks are ignored.
pub fn task_section(plan: &str, n: u32) -> Option<String> {
    let prefix = format!("{n}:");
    let lines = lines_with_fence(plan);
    let start = lines.iter().position(|(l, fenced)| {
        !fenced
            && is_task_heading(l)
            && l.strip_prefix("### Task ")
                .is_some_and(|r| r.starts_with(&prefix))
    })?;
    let mut out = vec![lines[start].0];
    for (line, fenced) in &lines[start + 1..] {
        if !fenced
            && (is_task_heading(line) || is_focus_heading(line) || line.starts_with("Next plan:"))
        {
            break;
        }
        out.push(line);
    }
    Some(out.join("\n").trim().to_string())
}

/// The body of a `## Goal` section as `## Goal` plus the body, up to the next `# ` or `## ` heading
/// outside a fenced block. `None` when there is no such heading or its body is empty.
fn goal_section(spec: &str) -> Option<String> {
    let lines = lines_with_fence(spec);
    let start = lines.iter().position(|(l, fenced)| {
        !fenced && l.starts_with("## ") && l[3..].trim().eq_ignore_ascii_case("goal")
    })?;
    let mut body = Vec::new();
    for (line, fenced) in &lines[start + 1..] {
        if !fenced && (line.starts_with("# ") || line.starts_with("## ")) {
            break;
        }
        body.push(*line);
    }
    let body = body.join("\n").trim().to_string();
    (!body.is_empty()).then(|| format!("## Goal\n\n{body}"))
}

/// The goal of a spec plus its `Acceptance criteria` section (a `## ` heading containing those words,
/// up to the next `## ` heading outside a fenced block). The goal is a `Goal:` line, else a `## Goal`
/// section, else the first plain line. Empty for an empty spec.
pub fn spec_excerpt(spec: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    let goal_line = spec
        .lines()
        .map(str::trim)
        .find(|l| l.len() >= 5 && l.is_char_boundary(5) && l[..5].eq_ignore_ascii_case("goal:"));
    if let Some(line) = goal_line {
        parts.push(line.to_string());
    } else if let Some(section) = goal_section(spec) {
        parts.push(section);
    } else {
        let goal = spec_goal(spec);
        if !goal.is_empty() {
            parts.push(format!("Goal: {goal}"));
        }
    }
    let lines = lines_with_fence(spec);
    let start = lines.iter().position(|(l, fenced)| {
        !fenced && l.starts_with("## ") && l.to_ascii_lowercase().contains("acceptance criteria")
    });
    if let Some(start) = start {
        let mut section = vec![lines[start].0];
        for (line, fenced) in &lines[start + 1..] {
            if !fenced && line.starts_with("## ") {
                break;
            }
            section.push(line);
        }
        parts.push(section.join("\n").trim().to_string());
    }
    parts.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_heading_tasks_in_order() {
        let got = parse_tasks("# P\n### Task 1: first\ntext\n### Task 2:  second \n");
        assert_eq!(
            got,
            vec![
                PlanTask {
                    n: 1,
                    title: "first".into()
                },
                PlanTask {
                    n: 2,
                    title: "second".into()
                }
            ]
        );
    }

    #[test]
    fn ignores_template_headings() {
        let got = parse_tasks("### Task N: x\n### Task 3: third\n### Task 4: fourth\n");
        assert_eq!(got.iter().map(|t| t.n).collect::<Vec<_>>(), vec![3, 4]);
    }

    #[test]
    fn parse_tasks_ignores_task_headings_inside_a_fence() {
        let plan = "### Task 1: a\n### Task 2: b\n### Task 3: c\n```md\n### Task 4: quoted\n```\n";
        let got = parse_tasks(plan);
        assert_eq!(got.iter().map(|t| t.n).collect::<Vec<_>>(), vec![1, 2, 3]);
    }

    #[test]
    fn parse_tasks_ignores_checkbox_lines() {
        assert!(parse_tasks("- [ ] Task 1: x\n").is_empty());
    }

    #[test]
    fn parse_checkbox_tasks_keeps_the_old_behavior() {
        let plan = "### Task 1: a\n- [ ] Task 2: b\n- [x] Task 3: c\n- [ ] Task 1: dup\n```\n- [ ] Task 9: fenced\n```\n";
        let got = parse_checkbox_tasks(plan);
        assert_eq!(got.iter().map(|t| t.n).collect::<Vec<_>>(), vec![1, 2, 3]);
    }

    #[test]
    fn no_tasks_gives_an_empty_list() {
        assert!(parse_tasks("just text\nTask list\n").is_empty());
    }

    #[test]
    fn plan_title_is_the_first_top_level_heading() {
        assert_eq!(plan_title("# Plan A\n\n### Task 1: x\n"), "Plan A");
        assert_eq!(plan_title("intro\n# Late Title\n"), "Late Title");
        assert_eq!(plan_title("## Not top level\n"), "plan");
        assert_eq!(plan_title("#NoSpace\n"), "plan");
        assert_eq!(plan_title(""), "plan");
        assert_eq!(plan_title("#  \n"), "plan");
    }

    #[test]
    fn spec_goal_prefers_a_goal_line() {
        assert_eq!(
            spec_goal("# T\n\nGoal: add the thing\n\nmore"),
            "add the thing"
        );
        assert_eq!(spec_goal("**Goal:** add it\n"), "add it");
        assert_eq!(spec_goal("- goal: fix it\n"), "fix it");
        assert_eq!(
            spec_goal("# T\n\nFirst line.\nGoal: later goal\n"),
            "later goal"
        );
    }

    #[test]
    fn spec_goal_falls_back_to_the_first_plain_line() {
        assert_eq!(
            spec_goal("# Title\n\nFirst paragraph line.\nSecond.\n"),
            "First paragraph line."
        );
        assert_eq!(spec_goal(""), "");
        assert_eq!(spec_goal("# Only a heading\n\n"), "");
        assert_eq!(spec_goal("Goal:\nnext line\n"), "Goal:");
        let long = "x".repeat(300);
        assert_eq!(spec_goal(&long).chars().count(), 200);
    }

    const PLAN: &str =
        "# Demo Plan\n\n**Goal:** make it.\n\n## Global Constraints\n\n- Use -j4.\n\n---\n\n\
### Task 1: first thing\n\nbody one\n\n### Step 2: not a task\n\nstill one\n\n\
### Task 2: second thing\n\nbody two\n\nNext plan: none\n";

    #[test]
    fn the_header_is_everything_before_the_first_task() {
        let header = plan_header(PLAN);
        assert!(header.starts_with("# Demo Plan"), "{header}");
        assert!(header.contains("Global Constraints") && header.contains("Use -j4."));
        assert!(!header.contains("Task 1"));
    }

    #[test]
    fn a_task_section_stops_at_the_next_task_and_keeps_inner_headings() {
        let one = task_section(PLAN, 1).expect("task 1");
        assert!(one.starts_with("### Task 1: first thing"), "{one}");
        assert!(
            one.contains("body one")
                && one.contains("### Step 2: not a task")
                && one.contains("still one")
        );
        assert!(!one.contains("body two") && !one.contains("Task 2"));
    }

    #[test]
    fn the_last_task_section_drops_the_next_plan_pointer() {
        let two = task_section(PLAN, 2).expect("task 2");
        assert!(
            two.contains("body two") && !two.contains("Next plan"),
            "{two}"
        );
    }

    #[test]
    fn task_numbers_need_not_be_consecutive_and_missing_ones_give_none() {
        let plan = "# P\n\n### Task 3: three\n\nc\n\n### Task 5: five\n\ne\n";
        assert!(task_section(plan, 5).expect("five").contains('e'));
        assert!(!task_section(plan, 3).expect("three").contains("five"));
        assert!(task_section(plan, 4).is_none());
        assert!(task_section("- [ ] Task 1: checkbox only\n", 1).is_none());
    }

    #[test]
    fn a_missing_task_heading_gives_none_and_the_whole_plan_as_header() {
        let plan = "# P\n\nJust prose.\n";
        assert!(task_section(plan, 1).is_none());
        assert_eq!(plan_header(plan), "# P\n\nJust prose.");
    }

    #[test]
    fn a_task_past_the_last_one_gives_none() {
        assert!(task_section(PLAN, 3).is_none());
        assert!(task_section(PLAN, 0).is_none());
        assert!(task_section("", 1).is_none());
    }

    #[test]
    fn a_task_number_is_matched_whole_not_as_a_prefix() {
        let plan = "# P\n\n### Task 12: twelve\n\ntwelve body\n";
        assert!(task_section(plan, 1).is_none());
        assert!(task_section(plan, 12).is_some());
    }

    #[test]
    fn a_single_task_plan_gives_that_task_and_its_header() {
        let plan = "# P\n\nHeader line.\n\n### Task 1: only\n\nonly body\n\nNext plan: later\n";
        assert_eq!(plan_header(plan), "# P\n\nHeader line.");
        assert_eq!(
            task_section(plan, 1).expect("task 1"),
            "### Task 1: only\n\nonly body"
        );
    }

    #[test]
    fn windows_line_endings_cut_the_same_sections_without_carriage_returns() {
        let plan = PLAN.replace('\n', "\r\n");
        let one = task_section(&plan, 1).expect("task 1");
        assert!(
            one.contains("body one") && !one.contains("body two"),
            "{one:?}"
        );
        assert!(!one.contains('\r'), "{one:?}");
        let two = task_section(&plan, 2).expect("task 2");
        assert!(
            two.contains("body two") && !two.contains("Next plan"),
            "{two:?}"
        );
        let header = plan_header(&plan);
        assert!(
            header.contains("Use -j4.") && !header.contains("Task 1"),
            "{header:?}"
        );
        let spec = "# S\r\n\r\nGoal: crlf goal here.\r\n\r\n## Acceptance criteria\r\n\r\n- ok\r\n\r\n## Testing\r\n\r\nnope\r\n";
        let ex = spec_excerpt(spec);
        assert!(
            ex.contains("Goal: crlf goal here.") && ex.contains("- ok") && !ex.contains("nope"),
            "{ex:?}"
        );
    }

    #[test]
    fn fenced_headings_and_pointers_stay_inside_their_task() {
        let plan = "# P\n\n### Task 1: one\n\nbefore\n\n```text\n### Task 9: fake\nNext plan: x\nin fence\n```\n\nafter fence\n\n\
### Task 2: two\n\nbody two\n";
        let one = task_section(plan, 1).expect("task 1");
        assert!(
            one.contains("### Task 9: fake") && one.contains("Next plan: x"),
            "{one}"
        );
        assert!(
            one.contains("in fence") && one.contains("after fence"),
            "{one}"
        );
        assert!(!one.contains("body two"));
        assert!(task_section(plan, 9).is_none());
    }

    #[test]
    fn a_fenced_task_heading_does_not_end_the_header() {
        let plan =
            "# P\n\n```text\n### Task 5: fake\n```\n\nmore header\n\n### Task 1: real\n\nbody\n";
        let header = plan_header(plan);
        assert!(
            header.contains("### Task 5: fake") && header.contains("more header"),
            "{header}"
        );
        assert!(!header.contains("Task 1: real"));
    }

    #[test]
    fn a_next_plan_line_must_start_at_column_zero() {
        let plan = "# P\n\n### Task 1: one\n\n  Next plan: indented\n\nstill one\n";
        assert!(task_section(plan, 1).expect("task 1").contains("still one"));
    }

    #[test]
    fn the_spec_excerpt_has_the_goal_and_the_acceptance_section_only() {
        let spec = "# Spec\n\nGoal: ship the thing quickly.\n\n## Background\n\nlong text\n\n\
## Acceptance criteria\n\n- it works\n- it is fast\n\n## Testing\n\nmore text\n";
        let ex = spec_excerpt(spec);
        assert!(ex.contains("Goal: ship the thing quickly."), "{ex}");
        assert!(ex.contains("Acceptance criteria") && ex.contains("- it is fast"));
        assert!(!ex.contains("long text") && !ex.contains("more text"));
    }

    #[test]
    fn a_fenced_h2_does_not_end_the_acceptance_section() {
        let spec = "Goal: g here ok.\n\n## Acceptance criteria\n\n```sh\n## comment\n```\n\n- last item\n\n## Testing\n\nnope\n";
        let ex = spec_excerpt(spec);
        assert!(
            ex.contains("## comment") && ex.contains("- last item"),
            "{ex}"
        );
        assert!(!ex.contains("nope"));
    }

    #[test]
    fn a_goal_section_wins_over_an_intro_paragraph() {
        let spec = "# Spec\n\nFollow-up to something else.\n\n## Goal\n\nCut the cost.\n\n```\n## Not a heading\n```\n\n## Background\n\nlong text\n";
        let ex = spec_excerpt(spec);
        assert!(ex.starts_with("## Goal\n\nCut the cost."), "{ex}");
        assert!(ex.contains("## Not a heading"), "{ex}");
        assert!(
            !ex.contains("Follow-up") && !ex.contains("long text"),
            "{ex}"
        );
    }

    #[test]
    fn a_goal_line_still_beats_a_goal_section() {
        let ex = spec_excerpt("Goal: the line.\n\n## Goal\n\nthe section\n");
        assert_eq!(ex, "Goal: the line.");
    }

    #[test]
    fn a_spec_without_acceptance_criteria_gives_the_goal_and_an_empty_spec_gives_nothing() {
        assert_eq!(
            spec_excerpt("Goal: only this goal here.\n"),
            "Goal: only this goal here."
        );
        assert_eq!(spec_excerpt(""), "");
    }

    #[test]
    fn review_focus_returns_the_section_body_and_none_when_missing() {
        let plan = "# P\n\nIntro.\n\n## Review Focus\n\nCheck the lease.\nAnd the ledger.\n\n## Other\n\nx\n\n### Task 1: a\n\nbody\n";
        assert_eq!(
            review_focus(plan),
            Some("Check the lease.\nAnd the ledger.".to_string())
        );
        assert_eq!(review_focus("# P\n\nIntro.\n\n### Task 1: a\n"), None);
        assert_eq!(
            review_focus("# P\n\n## Review Focus\n\n## Other\n\n### Task 1: a\n"),
            None
        );
        // Compact factory plans place Review Focus after their tasks.
        assert_eq!(
            review_focus("# P\n\n### Task 1: a\n\n## Review Focus\n\nlate\n"),
            Some("late".to_string())
        );
    }

    #[test]
    fn review_focus_ignores_a_fenced_heading() {
        let plan = "# P\n\n```md\n## Review Focus\n\nfake\n```\n\n### Task 1: a\n";
        assert_eq!(review_focus(plan), None);
        let plan = "# P\n\n## Review Focus\n\n```\n## not a heading\n```\nlast\n\n### Task 1: a\n";
        let focus = review_focus(plan).expect("focus");
        assert!(
            focus.contains("## not a heading") && focus.contains("last"),
            "{focus}"
        );
    }

    #[test]
    fn plan_outline_drops_the_focus_and_keeps_task_headings_only() {
        let plan = "# P\n\nIntro.\n\n## Review Focus\n\nCheck the lease.\n\n## Other\n\nkept\n\n### Task 1: a\n\nbody one\n\n### Task 2: b\n\nbody two\n";
        let out = plan_outline(plan);
        assert!(
            out.contains("Intro.") && out.contains("## Other") && out.contains("kept"),
            "{out}"
        );
        assert!(
            !out.contains("Review Focus") && !out.contains("Check the lease."),
            "{out}"
        );
        assert!(
            out.contains("### Task 1: a") && out.contains("### Task 2: b"),
            "{out}"
        );
        assert!(
            !out.contains("body one") && !out.contains("body two"),
            "{out}"
        );
    }
}
