use std::path::Path;

use serde::Serialize;

use crate::events::Registry;
use crate::model::{AgentState, Board, Job, RecentEvent};
use crate::plan::{load_plan, PlanView};

const STALE_SECS: f64 = 1800.0;
const KEEP_DONE_SECS: f64 = 600.0;
const UNASSIGNED_MAX_AGE_SECS: f64 = 7200.0;

#[derive(Debug, Serialize)]
pub struct JobView {
    #[serde(flatten)]
    pub job: Job,
    pub agents: Vec<AgentState>,
    pub seconds_in_stage: f64,
    pub plan: Option<PlanView>,
}

#[derive(Debug, Serialize)]
pub struct StateView {
    pub generated_at: f64,
    pub board_error: Option<String>,
    pub jobs: Vec<JobView>,
    pub unassigned_agents: Vec<AgentState>,
    pub recent: Vec<RecentEvent>,
}

/// Reads `board.json`. A missing file means no jobs. A bad file gives no jobs and an error text.
pub fn load_board(path: &Path) -> (Board, Option<String>) {
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<Board>(&text) {
            Ok(board) => (board, None),
            Err(err) => (
                Board::default(),
                Some(format!("board.json is invalid: {err}")),
            ),
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => (Board::default(), None),
        Err(err) => (
            Board::default(),
            Some(format!("board.json is unreadable: {err}")),
        ),
    }
}

fn stage_for_agent_type(agent_type: &str) -> Option<&'static str> {
    match agent_type {
        "factory-spec-writer" => Some("spec"),
        "factory-builder" => Some("build"),
        "factory-security-reviewer"
        | "factory-ux-reviewer"
        | "factory-ui-reviewer"
        | "factory-code-reviewer" => Some("review"),
        "factory-approver" => Some("approve"),
        _ => None,
    }
}

fn with_status(mut agent: AgentState, now: f64) -> AgentState {
    if agent.status == "running" {
        let last = agent
            .last_tool_ts
            .unwrap_or(agent.started_ts)
            .max(agent.started_ts);
        if now - last > STALE_SECS {
            agent.status = "stale".to_string();
        }
    }
    agent
}

fn owns_agent(job: &Job, agent: &AgentState) -> bool {
    if job.builder_id.as_deref() == Some(agent.agent_id.as_str()) {
        return true;
    }
    match (&job.worktree, &agent.cwd) {
        (Some(worktree), Some(cwd)) if !worktree.is_empty() => Path::new(cwd).starts_with(worktree),
        _ => false,
    }
}

fn fits_stage(job: &Job, agent: &AgentState) -> bool {
    stage_for_agent_type(&agent.agent_type) == Some(job.stage.as_str())
        && agent.started_ts >= job.stage_since
}

fn keep_unassigned(agent: &AgentState, now: f64) -> bool {
    match agent.status.as_str() {
        "done" => agent
            .ended_ts
            .is_some_and(|ended| now - ended <= KEEP_DONE_SECS),
        _ => {
            let last = agent
                .last_tool_ts
                .unwrap_or(agent.started_ts)
                .max(agent.started_ts);
            now - last <= UNASSIGNED_MAX_AGE_SECS
        }
    }
}

pub fn build_state(
    board: &Board,
    registry: &Registry,
    now: f64,
    board_error: Option<String>,
    root: &Path,
) -> StateView {
    let mut jobs: Vec<JobView> = board
        .jobs
        .iter()
        .map(|job| JobView {
            job: job.clone(),
            agents: Vec::new(),
            seconds_in_stage: (now - job.stage_since).max(0.0),
            plan: load_plan(root, &job.dir, job.stage == "merged"),
        })
        .collect();
    let mut unassigned = Vec::new();
    for agent in registry.agents() {
        let agent = with_status(agent, now);
        let index = jobs
            .iter()
            .position(|view| owns_agent(&view.job, &agent))
            .or_else(|| jobs.iter().position(|view| fits_stage(&view.job, &agent)));
        match index {
            Some(i) => jobs[i].agents.push(agent),
            None => {
                if keep_unassigned(&agent, now) {
                    unassigned.push(agent);
                }
            }
        }
    }
    for view in &mut jobs {
        view.agents
            .sort_by(|a, b| a.started_ts.total_cmp(&b.started_ts));
    }
    unassigned.sort_by(|a, b| a.started_ts.total_cmp(&b.started_ts));
    StateView {
        generated_at: now,
        board_error,
        jobs,
        unassigned_agents: unassigned,
        recent: registry.recent(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::Registry;
    use crate::model::Job;
    use std::collections::BTreeMap;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn job(id: &str, stage: &str, stage_since: f64) -> Job {
        let mut reviews = BTreeMap::new();
        for role in ["security", "ux", "ui", "code"] {
            reviews.insert(role.to_string(), "pending".to_string());
        }
        Job {
            id: id.to_string(),
            dir: format!("2026-10-13-{id}-x"),
            slug: "x".to_string(),
            created: "2026-10-13".to_string(),
            feature: "feature".to_string(),
            branch: format!("factory/{id}-x"),
            worktree: None,
            builder_id: None,
            stage: stage.to_string(),
            round: 0,
            reason: None,
            stage_since,
            reviews,
        }
    }

    fn start(registry: &mut Registry, ts: f64, id: &str, kind: &str, cwd: &str) {
        registry.ingest_line(&format!(
            r#"{{"ts":{ts},"hook_event_name":"SubagentStart","agent_id":"{id}","agent_type":"{kind}","cwd":"{cwd}"}}"#
        ));
    }

    #[test]
    fn a_missing_board_file_means_no_jobs() -> TestResult {
        let dir = tempfile::tempdir()?;
        let (board, error) = load_board(&dir.path().join("board.json"));
        assert!(board.jobs.is_empty());
        assert!(error.is_none());
        Ok(())
    }

    #[test]
    fn an_invalid_board_file_reports_an_error() -> TestResult {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("board.json");
        std::fs::write(&path, "{ nope")?;
        let (board, error) = load_board(&path);
        assert!(board.jobs.is_empty());
        assert!(error.is_some_and(|text| text.contains("invalid")));
        Ok(())
    }

    #[test]
    fn a_valid_board_file_loads() -> TestResult {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("board.json");
        std::fs::write(
            &path,
            r#"{"jobs":[{"id":"001","dir":"d","slug":"s","created":"2026-10-13","feature":"f","branch":"b","stage":"build","round":1,"stage_since":5.0,"reviews":{"security":"pass"},"base":"abc"}]}"#,
        )?;
        let (board, error) = load_board(&path);
        assert!(error.is_none());
        assert_eq!(board.jobs[0].round, 1);
        Ok(())
    }

    #[test]
    fn the_builder_id_wins_over_the_stage_rule() {
        let mut board = Board::default();
        let mut first = job("001", "review", 10.0);
        first.builder_id = Some("b1".to_string());
        board.jobs.push(first);
        board.jobs.push(job("002", "build", 10.0));
        let mut registry = Registry::new();
        start(&mut registry, 20.0, "b1", "factory-builder", "/elsewhere");
        let view = build_state(&board, &registry, 30.0, None, Path::new("/nonexistent"));
        assert_eq!(view.jobs[0].agents.len(), 1);
        assert!(view.jobs[1].agents.is_empty());
    }

    #[test]
    fn a_worktree_path_attaches_the_agent() {
        let mut board = Board::default();
        board.jobs.push(job("001", "approve", 10.0));
        let mut second = job("002", "approve", 10.0);
        second.worktree = Some("/repo/.claude/worktrees/factory-002".to_string());
        board.jobs.push(second);
        let mut registry = Registry::new();
        start(
            &mut registry,
            20.0,
            "x1",
            "other",
            "/repo/.claude/worktrees/factory-002/src",
        );
        let view = build_state(&board, &registry, 30.0, None, Path::new("/nonexistent"));
        assert_eq!(view.jobs[1].agents.len(), 1);
    }

    #[test]
    fn the_stage_rule_attaches_all_reviewers_to_the_review_job() {
        let mut board = Board::default();
        board.jobs.push(job("001", "review", 10.0));
        let mut registry = Registry::new();
        for (i, kind) in [
            "factory-security-reviewer",
            "factory-ux-reviewer",
            "factory-ui-reviewer",
            "factory-code-reviewer",
        ]
        .iter()
        .enumerate()
        {
            start(
                &mut registry,
                20.0 + i as f64,
                &format!("r{i}"),
                kind,
                "/repo",
            );
        }
        let view = build_state(&board, &registry, 30.0, None, Path::new("/nonexistent"));
        assert_eq!(view.jobs[0].agents.len(), 4);
        assert!(view.unassigned_agents.is_empty());
    }

    #[test]
    fn an_agent_from_an_earlier_stage_does_not_attach_to_a_later_job() {
        let mut board = Board::default();
        board.jobs.push(job("002", "spec", 500.0));
        let mut registry = Registry::new();
        start(&mut registry, 100.0, "s1", "factory-spec-writer", "/repo");
        registry.ingest_line(r#"{"ts":150.0,"hook_event_name":"SubagentStop","agent_id":"s1"}"#);
        let view = build_state(&board, &registry, 800.0, None, Path::new("/nonexistent"));
        assert!(view.jobs[0].agents.is_empty());
        assert!(
            view.unassigned_agents.is_empty(),
            "ended more than 10 minutes ago"
        );
    }

    #[test]
    fn a_silent_running_agent_becomes_stale() {
        let mut board = Board::default();
        board.jobs.push(job("001", "build", 10.0));
        let mut registry = Registry::new();
        start(&mut registry, 20.0, "b1", "factory-builder", "/repo");
        let fresh = build_state(&board, &registry, 100.0, None, Path::new("/nonexistent"));
        assert_eq!(fresh.jobs[0].agents[0].status, "running");
        let old = build_state(&board, &registry, 20.0 + 1801.0, None, Path::new("/nonexistent"));
        assert_eq!(old.jobs[0].agents[0].status, "stale");
    }

    #[test]
    fn a_tool_event_keeps_an_agent_running() {
        let mut board = Board::default();
        board.jobs.push(job("001", "build", 10.0));
        let mut registry = Registry::new();
        start(&mut registry, 20.0, "b1", "factory-builder", "/repo");
        registry.ingest_line(
            r#"{"ts":1000.0,"hook_event_name":"PostToolUse","agent_id":"b1","tool_name":"Edit"}"#,
        );
        let view = build_state(&board, &registry, 2000.0, None, Path::new("/nonexistent"));
        assert_eq!(view.jobs[0].agents[0].status, "running");
    }

    #[test]
    fn a_recent_done_unassigned_agent_is_kept() {
        let mut registry = Registry::new();
        start(&mut registry, 100.0, "u1", "other", "/x");
        registry.ingest_line(r#"{"ts":150.0,"hook_event_name":"SubagentStop","agent_id":"u1"}"#);
        let view = build_state(&Board::default(), &registry, 200.0, None, Path::new("/nonexistent"));
        assert_eq!(view.unassigned_agents.len(), 1);
    }

    #[test]
    fn a_done_agent_without_an_end_time_is_dropped() {
        let agent = AgentState {
            agent_id: "u1".to_string(),
            agent_type: "other".to_string(),
            status: "done".to_string(),
            started_ts: 100.0,
            ended_ts: None,
            last_tool: None,
            last_target: None,
            last_tool_ts: None,
            tool_count: 0,
            last_message: None,
            cwd: None,
        };
        assert!(!keep_unassigned(&agent, 200.0));
    }

    #[test]
    fn a_running_unassigned_agent_with_recent_activity_is_kept() {
        let mut registry = Registry::new();
        start(&mut registry, 100.0, "u1", "other", "/x");
        let view = build_state(&Board::default(), &registry, 200.0, None, Path::new("/nonexistent"));
        assert_eq!(view.unassigned_agents.len(), 1);
    }

    #[test]
    fn a_stale_unassigned_agent_older_than_two_hours_is_dropped() {
        let mut registry = Registry::new();
        start(&mut registry, 100.0, "u1", "other", "/x");
        let view = build_state(&Board::default(), &registry, 100.0 + 7201.0, None, Path::new("/nonexistent"));
        assert!(view.unassigned_agents.is_empty());
        let view = build_state(&Board::default(), &registry, 100.0 + 3000.0, None, Path::new("/nonexistent"));
        assert_eq!(view.unassigned_agents.len(), 1);
        assert_eq!(view.unassigned_agents[0].status, "stale");
    }

    #[test]
    fn an_empty_worktree_does_not_attach_an_arbitrary_agent() {
        let mut board = Board::default();
        let mut only = job("001", "approve", 10.0);
        only.worktree = Some(String::new());
        board.jobs.push(only);
        let mut registry = Registry::new();
        start(&mut registry, 20.0, "x1", "other", "/repo/src");
        let view = build_state(&board, &registry, 30.0, None, Path::new("/nonexistent"));
        assert!(view.jobs[0].agents.is_empty());
        assert_eq!(view.unassigned_agents.len(), 1);
    }

    #[test]
    fn seconds_in_stage_and_the_board_error_are_reported() {
        let mut board = Board::default();
        board.jobs.push(job("001", "build", 100.0));
        let view = build_state(&board, &Registry::new(), 160.0, Some("oops".to_string()), Path::new("/nonexistent"));
        assert_eq!(view.jobs[0].seconds_in_stage, 60.0);
        assert_eq!(view.board_error.as_deref(), Some("oops"));
        assert_eq!(view.generated_at, 160.0);
    }

    #[test]
    fn the_state_serializes_with_flattened_job_fields() -> TestResult {
        let mut board = Board::default();
        board.jobs.push(job("001", "build", 100.0));
        let view = build_state(&board, &Registry::new(), 160.0, None, Path::new("/nonexistent"));
        let json = serde_json::to_value(&view)?;
        assert_eq!(json["jobs"][0]["id"], "001");
        assert_eq!(json["jobs"][0]["stage"], "build");
        assert!(json["jobs"][0]["agents"].is_array());
        Ok(())
    }
}
