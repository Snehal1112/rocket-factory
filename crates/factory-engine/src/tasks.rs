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
}
