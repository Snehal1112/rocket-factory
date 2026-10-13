use std::path::Path;

use serde::Serialize;

const GOAL_MAX_CHARS: usize = 240;

/// One task of a job's plan, with the state the ledger reports for it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TaskView {
    pub number: u32,
    pub title: String,
    /// "pending", "active", "review" or "done".
    pub status: String,
}

/// The plan of one job: what it is for and how its tasks stand.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PlanView {
    pub title: Option<String>,
    pub goal: Option<String>,
    pub tasks: Vec<TaskView>,
}

/// Highest progress a ledger line reported for a task.
fn rank(status: &str) -> u8 {
    match status {
        "active" => 1,
        "review" => 2,
        "done" => 3,
        _ => 0,
    }
}

fn first_heading(text: &str, prefix: &str) -> Option<String> {
    let line = text.lines().find(|line| line.starts_with("# "))?;
    let title = line.trim_start_matches("# ").trim();
    let title = title.strip_prefix(prefix).unwrap_or(title).trim();
    (!title.is_empty()).then(|| title.to_string())
}

fn goal(spec: &str) -> Option<String> {
    let mut lines = spec.lines().skip_while(|line| line.trim() != "## Goal");
    lines.next()?;
    let text = lines
        .skip_while(|line| line.trim().is_empty())
        .take_while(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if text.is_empty() {
        return None;
    }
    if text.chars().count() <= GOAL_MAX_CHARS {
        return Some(text);
    }
    let cut: String = text.chars().take(GOAL_MAX_CHARS - 1).collect();
    Some(format!("{}…", cut.trim_end()))
}

/// Parses `- [ ] Task N: title` checkbox lines (a ticked box counts as done) and the
/// `### Task N: title` headings that plans written by the v3 flow use.
fn parse_tasks(plan: &str) -> Vec<TaskView> {
    plan.lines().filter_map(parse_task_line).collect()
}

fn parse_task_line(line: &str) -> Option<TaskView> {
    let line = line.trim_start();
    if let Some(rest) = line.strip_prefix("### Task ") {
        let (number, title) = rest.split_once(':')?;
        return Some(TaskView {
            number: number.trim().parse().ok()?,
            title: title.trim().to_string(),
            status: "pending".to_string(),
        });
    }
    let rest = line.strip_prefix("- [")?;
    let mark = rest.chars().next()?;
    let rest = rest.get(mark.len_utf8()..)?.strip_prefix("] Task ")?;
    let (number, title) = rest.split_once(':')?;
    let number: u32 = number.trim().parse().ok()?;
    let status = if mark == 'x' || mark == 'X' {
        "done"
    } else {
        "pending"
    };
    Some(TaskView {
        number,
        title: title.trim().to_string(),
        status: status.to_string(),
    })
}

/// Reads ledger lines such as `Task 2: complete` and `Task 1-2: dispatched`.
fn apply_ledger(tasks: &mut [TaskView], progress: &str) {
    for line in progress.lines() {
        let Some(rest) = line.trim().strip_prefix("Task ") else {
            continue;
        };
        let Some((range, state)) = rest.split_once(':') else {
            continue;
        };
        let (first, last) = match range.split_once('-') {
            Some((a, b)) => (a.trim().parse::<u32>(), b.trim().parse::<u32>()),
            None => (range.trim().parse::<u32>(), range.trim().parse::<u32>()),
        };
        let (Ok(first), Ok(last)) = (first, last) else {
            continue;
        };
        let state = state.trim();
        let status = if state.starts_with("complete") {
            "done"
        } else if state.starts_with("implementer done") {
            "review"
        } else if state.starts_with("dispatched") {
            "active"
        } else {
            continue;
        };
        for task in tasks
            .iter_mut()
            .filter(|task| task.number >= first && task.number <= last)
        {
            if rank(status) > rank(&task.status) {
                task.status = status.to_string();
            }
        }
    }
}

fn safe_dir_name(dir: &str) -> bool {
    !dir.is_empty() && !dir.contains(['/', '\\']) && dir != "." && dir != ".."
}

/// Loads the plan of one job from `.superpowers/sdd/<dir>`. A merged job shows every task done.
pub fn load_plan(root: &Path, dir: &str, merged: bool) -> Option<PlanView> {
    if !safe_dir_name(dir) {
        return None;
    }
    let base = root.join(".superpowers").join("sdd").join(dir);
    let plan = std::fs::read_to_string(base.join("plan.md")).ok();
    let spec = std::fs::read_to_string(base.join("spec.md")).ok();
    if plan.is_none() && spec.is_none() {
        return None;
    }
    let mut tasks = plan.as_deref().map(parse_tasks).unwrap_or_default();
    if let Ok(progress) = std::fs::read_to_string(base.join("progress.md")) {
        apply_ledger(&mut tasks, &progress);
    }
    if merged {
        for task in &mut tasks {
            task.status = "done".to_string();
        }
    }
    Some(PlanView {
        title: plan.as_deref().and_then(|text| first_heading(text, "Plan:")),
        goal: spec.as_deref().and_then(goal),
        tasks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    const PLAN: &str = "# Plan: Prove mTLS\n\nIntro.\n\n- [x] Task 1: First thing\n\nBefore starting.\n\n- [ ] Task 2: Second thing\n- [ ] Task 3: Third thing\n";

    #[test]
    fn tasks_and_title_are_parsed() {
        let tasks = parse_tasks(PLAN);
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks[0].status, "done");
        assert_eq!(tasks[1].title, "Second thing");
        assert_eq!(first_heading(PLAN, "Plan:").as_deref(), Some("Prove mTLS"));
    }

    // A plan written by the v3 flow: tasks are `### Task N: title` headings, not checkbox lines.
    const PLAN_V3: &str = "# Baseline Plan 1\n\n> header\n\n## Review Focus\n\n- not a task\n\n---\n\n### Task 1: criterion and the resolve bench\n\n**Files:**\n- Create: `a.rs`\n\n- [ ] **Step 1: Write the failing test**\n\n### Task 2: fixture generator\n\n- [ ] **Step 1: Write it**\n\n### Task N: template heading is ignored\n\nNext plan: none\n";

    #[test]
    fn heading_tasks_from_v3_plans_are_parsed() {
        let tasks = parse_tasks(PLAN_V3);
        let numbers: Vec<u32> = tasks.iter().map(|t| t.number).collect();
        assert_eq!(numbers, vec![1, 2]);
        assert_eq!(tasks[0].title, "criterion and the resolve bench");
        assert_eq!(tasks[1].status, "pending");
    }

    #[test]
    fn heading_tasks_follow_the_driver_ledger() {
        let mut tasks = parse_tasks(PLAN_V3);
        apply_ledger(&mut tasks, "Task 1: dispatched (BASE abc, sonnet)\n");
        assert_eq!(tasks[0].status, "active");
        assert_eq!(tasks[1].status, "pending");
        apply_ledger(&mut tasks, "Task 1: complete (merge abc)\nTask 2: complete (merge abc)\n");
        assert!(tasks.iter().all(|t| t.status == "done"));
    }

    #[test]
    fn the_ledger_moves_tasks_forward_and_never_back() {
        let mut tasks = parse_tasks(PLAN);
        apply_ledger(
            &mut tasks,
            "Task 1-3: dispatched (BASE abc)\nTask 2: implementer done\nTask 1: dispatched\nTask 3: junk\n",
        );
        assert_eq!(tasks[0].status, "done");
        assert_eq!(tasks[1].status, "review");
        assert_eq!(tasks[2].status, "active");
    }

    #[test]
    fn the_goal_is_the_first_paragraph_after_the_goal_heading() {
        let spec = "# Spec: x\n\n## Goal\n\nClose it with\nevidence.\n\n## Scope\n";
        assert_eq!(goal(spec).as_deref(), Some("Close it with evidence."));
        assert!(goal("# Spec\n").is_none());
    }

    #[test]
    fn a_long_goal_is_cut() {
        let spec = format!("## Goal\n\n{}\n", "word ".repeat(100));
        let text = goal(&spec).unwrap_or_default();
        assert!(text.chars().count() <= GOAL_MAX_CHARS);
        assert!(text.ends_with('…'));
    }

    #[test]
    fn load_plan_reads_the_job_folder_and_forces_merged_done() -> TestResult {
        let root = tempfile::tempdir()?;
        let dir = root.path().join(".superpowers/sdd/2026-10-13-001-x");
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("plan.md"), PLAN)?;
        let open = load_plan(root.path(), "2026-10-13-001-x", false)
            .ok_or("plan should load")?;
        assert_eq!(open.tasks[2].status, "pending");
        let merged = load_plan(root.path(), "2026-10-13-001-x", true)
            .ok_or("plan should load")?;
        assert!(merged.tasks.iter().all(|task| task.status == "done"));
        Ok(())
    }

    #[test]
    fn a_missing_folder_or_unsafe_name_gives_no_plan() -> TestResult {
        let root = tempfile::tempdir()?;
        assert!(load_plan(root.path(), "nope", false).is_none());
        assert!(load_plan(root.path(), "../x", false).is_none());
        assert!(load_plan(root.path(), "", false).is_none());
        Ok(())
    }
}
