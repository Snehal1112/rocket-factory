
use serde::Serialize;

use crate::paths::Paths;

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
/// The driver runs one builder for a whole plan, so it cannot say which task the builder is on.
/// The stage of the job stands in for it: while the builder runs every open task is active, and
/// once the build is over the open tasks wait for the review or for the human. A task never moves
/// backwards, so a ledger line that already marked it done or in review wins.
fn settle_by_stage(tasks: &mut [TaskView], stage: &str, reviewed: bool) {
    let status = match stage {
        "build" => "active",
        "review" | "approve" => "review",
        "needs-human" if reviewed => "review",
        "needs-human" => "active",
        _ => return,
    };
    for task in tasks.iter_mut() {
        if rank(status) > rank(&task.status) {
            task.status = status.to_string();
        }
    }
}

/// `stage` is the job's stage. `reviewed` is true once any review has a verdict or is running.
pub fn load_plan(paths: &Paths, dir: &str, stage: &str, reviewed: bool) -> Option<PlanView> {
    if !safe_dir_name(dir) {
        return None;
    }
    let base = paths.sdd.join(dir);
    let plan = std::fs::read_to_string(base.join("plan.md")).ok();
    let spec = std::fs::read_to_string(base.join("spec.md")).ok();
    if plan.is_none() && spec.is_none() {
        return None;
    }
    let mut tasks = plan.as_deref().map(parse_tasks).unwrap_or_default();
    let mut per_task = false;
    if let Ok(progress) = std::fs::read_to_string(base.join("progress.md")) {
        apply_ledger(&mut tasks, &progress);
        per_task = progress.contains("per-task builders");
    }
    // With one builder per task the ledger knows each task's state while the build runs or is parked.
    let ledger_decides = per_task && (stage == "build" || (stage == "needs-human" && !reviewed));
    if !ledger_decides {
        settle_by_stage(&mut tasks, stage, reviewed);
    }
    if stage == "merged" {
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

/// What a spec file says about itself.
#[derive(Debug, Clone, PartialEq)]
pub struct SpecInfo {
    pub title: Option<String>,
    pub goal: Option<String>,
}

/// Reads `spec.md` of one job folder. None when the folder name is unsafe or the file is missing.
pub fn load_spec(paths: &Paths, dir: &str) -> Option<SpecInfo> {
    if !safe_dir_name(dir) {
        return None;
    }
    let path = paths.sdd.join(dir).join("spec.md");
    let text = std::fs::read_to_string(path).ok()?;
    Some(SpecInfo { title: first_heading(&text, "Spec:"), goal: goal(&text) })
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
    fn a_running_build_marks_every_open_task_active() {
        let mut tasks = parse_tasks(PLAN_V3);
        settle_by_stage(&mut tasks, "build", false);
        assert!(tasks.iter().all(|t| t.status == "active"));
    }

    #[test]
    fn a_finished_build_waiting_for_review_or_a_human_shows_in_review() {
        for (stage, reviewed) in [("review", false), ("approve", false), ("needs-human", true)] {
            let mut tasks = parse_tasks(PLAN_V3);
            apply_ledger(&mut tasks, "Task 1: dispatched (BASE abc, sonnet)\n");
            settle_by_stage(&mut tasks, stage, reviewed);
            assert!(tasks.iter().all(|t| t.status == "review"), "{stage}");
        }
    }

    #[test]
    fn a_parked_job_without_a_review_is_still_active_and_done_tasks_stay_done() {
        let mut tasks = parse_tasks(PLAN_V3);
        apply_ledger(&mut tasks, "Task 1: complete (merge abc)\n");
        settle_by_stage(&mut tasks, "needs-human", false);
        assert_eq!(tasks[0].status, "done");
        assert_eq!(tasks[1].status, "active");
        let mut planning = parse_tasks(PLAN_V3);
        settle_by_stage(&mut planning, "spec", false);
        assert!(planning.iter().all(|t| t.status == "pending"));
    }

    #[test]
    fn load_plan_settles_tasks_by_the_stage_of_the_job() -> TestResult {
        let root = tempfile::tempdir()?;
        let dir = root.path().join(".superpowers/sdd/2026-10-13-001-x");
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("plan.md"), PLAN_V3)?;
        std::fs::write(dir.join("progress.md"), "Task 1: dispatched (BASE abc, sonnet)\n")?;
        let in_review = load_plan(&Paths::for_root(root.path()), "2026-10-13-001-x", "review", false).ok_or("no plan")?;
        assert!(in_review.tasks.iter().all(|t| t.status == "review"));
        let building = load_plan(&Paths::for_root(root.path()), "2026-10-13-001-x", "build", false).ok_or("no plan")?;
        assert!(building.tasks.iter().all(|t| t.status == "active"));
        Ok(())
    }

    #[test]
    fn per_task_ledger_shows_the_true_state_of_each_task() -> TestResult {
        let root = tempfile::tempdir()?;
        let dir = root.path().join(".superpowers/sdd/2026-10-13-001-x");
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("plan.md"), "### Task 1: a\n\n### Task 2: b\n\n### Task 3: c\n")?;
        let ledger = "Ruling: per-task builders, 3 tasks\nTask 1: dispatched (BASE abc, sonnet)\n\
                      Task 1: implementer done (abc)\nTask 2: dispatched (BASE abc, sonnet)\n";
        std::fs::write(dir.join("progress.md"), ledger)?;
        let building = load_plan(&Paths::for_root(root.path()), "2026-10-13-001-x", "build", false).ok_or("no plan")?;
        let states: Vec<&str> = building.tasks.iter().map(|t| t.status.as_str()).collect();
        assert_eq!(states, vec!["review", "active", "pending"]);
        let in_review = load_plan(&Paths::for_root(root.path()), "2026-10-13-001-x", "review", false).ok_or("no plan")?;
        assert!(in_review.tasks.iter().all(|t| t.status == "review"));
        Ok(())
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
        // While the plan is still being written the tasks wait, whatever the ledger says.
        let open = load_plan(&Paths::for_root(root.path()), "2026-10-13-001-x", "spec", false)
            .ok_or("plan should load")?;
        assert_eq!(open.tasks[2].status, "pending");
        let merged = load_plan(&Paths::for_root(root.path()), "2026-10-13-001-x", "merged", false)
            .ok_or("plan should load")?;
        assert!(merged.tasks.iter().all(|task| task.status == "done"));
        Ok(())
    }

    #[test]
    fn a_missing_folder_or_unsafe_name_gives_no_plan() -> TestResult {
        let root = tempfile::tempdir()?;
        assert!(load_plan(&Paths::for_root(root.path()), "nope", "build", false).is_none());
        assert!(load_plan(&Paths::for_root(root.path()), "../x", "build", false).is_none());
        assert!(load_plan(&Paths::for_root(root.path()), "", "build", false).is_none());
        Ok(())
    }

    #[test]
    fn load_spec_reads_title_and_goal_and_refuses_unsafe_folders() -> TestResult {
        let root = tempfile::tempdir()?;
        let dir = root.path().join(".superpowers/sdd/2026-10-13-001-x");
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("spec.md"), "# Spec: Demo spec\n\n## Goal\n\nShow the plans.\n")?;
        let info = load_spec(&Paths::for_root(root.path()), "2026-10-13-001-x").ok_or("no spec")?;
        assert_eq!(info.title.as_deref(), Some("Demo spec"));
        assert_eq!(info.goal.as_deref(), Some("Show the plans."));
        assert!(load_spec(&Paths::for_root(root.path()), "../x").is_none());
        assert!(load_spec(&Paths::for_root(root.path()), "missing-folder").is_none());
        Ok(())
    }
}
