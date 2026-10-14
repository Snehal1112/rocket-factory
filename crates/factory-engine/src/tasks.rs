/// One task heading of a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanTask {
    pub n: u32,
    pub title: String,
}

/// Reads `### Task N: title` and `- [ ] Task N: title` lines, in order. A repeated number counts once.
pub fn parse_tasks(plan: &str) -> Vec<PlanTask> {
    let mut out: Vec<PlanTask> = Vec::new();
    for line in plan.lines() {
        let t = line.trim_start_matches("- [ ] ").trim_start_matches("- [x] ").trim_start_matches("### ");
        let Some(rest) = t.strip_prefix("Task ") else { continue };
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        let Ok(n) = digits.parse::<u32>() else { continue };
        let Some(title) = rest[digits.len()..].strip_prefix(':') else { continue };
        if out.iter().any(|t| t.n == n) {
            continue;
        }
        out.push(PlanTask { n, title: title.trim().to_string() });
    }
    out
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_heading_tasks_in_order() {
        let got = parse_tasks("# P\n### Task 1: first\ntext\n### Task 2:  second \n");
        assert_eq!(
            got,
            vec![PlanTask { n: 1, title: "first".into() }, PlanTask { n: 2, title: "second".into() }]
        );
    }

    #[test]
    fn ignores_template_headings_and_accepts_checkboxes() {
        let got = parse_tasks("### Task N: x\n- [ ] Task 3: third\n- [x] Task 4: fourth\n");
        assert_eq!(got.iter().map(|t| t.n).collect::<Vec<_>>(), vec![3, 4]);
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
        assert_eq!(spec_goal("# T\n\nGoal: add the thing\n\nmore"), "add the thing");
        assert_eq!(spec_goal("**Goal:** add it\n"), "add it");
        assert_eq!(spec_goal("- goal: fix it\n"), "fix it");
        assert_eq!(spec_goal("# T\n\nFirst line.\nGoal: later goal\n"), "later goal");
    }

    #[test]
    fn spec_goal_falls_back_to_the_first_plain_line() {
        assert_eq!(spec_goal("# Title\n\nFirst paragraph line.\nSecond.\n"), "First paragraph line.");
        assert_eq!(spec_goal(""), "");
        assert_eq!(spec_goal("# Only a heading\n\n"), "");
        assert_eq!(spec_goal("Goal:\nnext line\n"), "Goal:");
        let long = "x".repeat(300);
        assert_eq!(spec_goal(&long).chars().count(), 200);
    }
}
