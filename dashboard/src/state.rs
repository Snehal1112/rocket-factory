use std::path::Path;

use serde::Serialize;

use crate::events::Registry;
use crate::model::{AgentState, Board, Job, RecentEvent, StageEntry};
use crate::plan::{load_plan, PlanView};

const STALE_SECS: f64 = 600.0;
const DEFAULT_BUDGET_WEIGHTED: u64 = 3_000_000;
const KEEP_DONE_SECS: f64 = 600.0;
const UNASSIGNED_MAX_AGE_SECS: f64 = 7200.0;

#[derive(Debug, Serialize)]
pub struct JobView {
    #[serde(flatten)]
    pub job: Job,
    pub agents: Vec<AgentState>,
    pub seconds_in_stage: f64,
    pub plan: Option<PlanView>,
    pub stage_durations: Vec<StageDuration>,
    pub token_view: Option<TokenView>,
}

#[derive(Debug, Serialize)]
pub struct StateView {
    pub generated_at: f64,
    pub board_error: Option<String>,
    pub jobs: Vec<JobView>,
    pub unassigned_agents: Vec<AgentState>,
    pub recent: Vec<RecentEvent>,
    pub skipped_lines: u64,
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

#[derive(Debug, Serialize, PartialEq)]
pub struct StageDuration {
    pub stage: String,
    pub seconds: f64,
    /// True for the stage the job is in now.
    pub current: bool,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct TokenView {
    pub weighted: u64,
    pub raw: u64,
    pub budget: u64,
    /// Weighted over budget, always between 0 and 1.
    pub fraction: f64,
}

fn is_terminal(stage: &str) -> bool {
    matches!(stage, "merged" | "needs-human")
}

/// Time spent per stage, summed over repeats. A terminal stage that is still current shows
/// nothing, because its time would only grow.
pub fn stage_durations(job: &Job, now: f64) -> Vec<StageDuration> {
    let history: Vec<StageEntry> = if job.stage_history.is_empty() {
        vec![StageEntry { stage: job.stage.clone(), ts: job.stage_since }]
    } else {
        job.stage_history.clone()
    };
    let mut out: Vec<StageDuration> = Vec::new();
    for (i, entry) in history.iter().enumerate() {
        let current = i + 1 == history.len();
        if current && is_terminal(&entry.stage) {
            continue;
        }
        let end = if current { now } else { history[i + 1].ts };
        let seconds = (end - entry.ts).max(0.0);
        match out.iter_mut().find(|d| d.stage == entry.stage) {
            Some(existing) => {
                existing.seconds += seconds;
                existing.current |= current;
            }
            None => out.push(StageDuration { stage: entry.stage.clone(), seconds, current }),
        }
    }
    out
}

/// Token totals and the budget fraction for the card bar.
pub fn token_view(job: &Job) -> Option<TokenView> {
    let tokens = job.tokens.as_ref()?;
    let budget = job.budget_weighted.unwrap_or(DEFAULT_BUDGET_WEIGHTED);
    let fraction = if budget == 0 {
        0.0
    } else {
        (tokens.weighted as f64 / budget as f64).clamp(0.0, 1.0)
    };
    Some(TokenView { weighted: tokens.weighted, raw: tokens.raw, budget, fraction })
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
            stage_durations: stage_durations(job, now),
            token_view: token_view(job),
        })
        .collect();
    let mut unassigned = Vec::new();
    for agent in registry.agents() {
        let agent = with_status(agent, now);
        let index = agent
            .job_id
            .as_deref()
            .and_then(|id| jobs.iter().position(|view| view.job.id == id));
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
        skipped_lines: registry.skipped_lines,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::Registry;
    use crate::model::{Job, Tokens};
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
            stage_history: Vec::new(),
            series: None,
            tokens: None,
            budget_weighted: None,
        }
    }

    fn start_for(registry: &mut Registry, ts: f64, id: &str, kind: &str, job_id: Option<&str>) {
        let job = job_id.map_or(String::new(), |j| format!(r#","job_id":"{j}""#));
        registry.ingest_line(&format!(
            r#"{{"ts":{ts},"hook_event_name":"SubagentStart","agent_id":"{id}","agent_type":"{kind}"{job}}}"#
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
    fn a_silent_running_agent_becomes_stale() {
        let mut board = Board::default();
        board.jobs.push(job("001", "build", 10.0));
        let mut registry = Registry::new();
        start_for(&mut registry, 20.0, "b1", "factory-builder", Some("001"));
        let fresh = build_state(&board, &registry, 100.0, None, Path::new("/nonexistent"));
        assert_eq!(fresh.jobs[0].agents[0].status, "running");
        let old = build_state(&board, &registry, 20.0 + 601.0, None, Path::new("/nonexistent"));
        assert_eq!(old.jobs[0].agents[0].status, "stale");
    }

    #[test]
    fn a_tool_event_keeps_an_agent_running() {
        let mut board = Board::default();
        board.jobs.push(job("001", "build", 10.0));
        let mut registry = Registry::new();
        start_for(&mut registry, 20.0, "b1", "factory-builder", Some("001"));
        registry.ingest_line(
            r#"{"ts":1000.0,"hook_event_name":"PostToolUse","agent_id":"b1","tool_name":"Edit"}"#,
        );
        let view = build_state(&board, &registry, 1300.0, None, Path::new("/nonexistent"));
        assert_eq!(view.jobs[0].agents[0].status, "running");
    }

    #[test]
    fn a_recent_done_unassigned_agent_is_kept() {
        let mut registry = Registry::new();
        start_for(&mut registry, 100.0, "u1", "other", None);
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
            cwd: None, job_id: None, tokens_weighted: 0,
        };
        assert!(!keep_unassigned(&agent, 200.0));
    }

    #[test]
    fn a_running_unassigned_agent_with_recent_activity_is_kept() {
        let mut registry = Registry::new();
        start_for(&mut registry, 100.0, "u1", "other", None);
        let view = build_state(&Board::default(), &registry, 200.0, None, Path::new("/nonexistent"));
        assert_eq!(view.unassigned_agents.len(), 1);
    }

    #[test]
    fn a_stale_unassigned_agent_older_than_two_hours_is_dropped() {
        let mut registry = Registry::new();
        start_for(&mut registry, 100.0, "u1", "other", None);
        let view = build_state(&Board::default(), &registry, 100.0 + 7201.0, None, Path::new("/nonexistent"));
        assert!(view.unassigned_agents.is_empty());
        let view = build_state(&Board::default(), &registry, 100.0 + 3000.0, None, Path::new("/nonexistent"));
        assert_eq!(view.unassigned_agents.len(), 1);
        assert_eq!(view.unassigned_agents[0].status, "stale");
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

    fn board_from(jobs_json: &str) -> Result<Board, Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("board.json");
        let base = r#"{"id":"001","dir":"d","slug":"s","created":"2026-10-13","feature":"f","branch":"b","stage":"build","round":0,"stage_since":5.0,"reviews":{}"#;
        std::fs::write(&path, format!(r#"{{"jobs":[{base}{jobs_json}}}]}}"#))?;
        let (board, error) = load_board(&path);
        assert!(error.is_none(), "got: {error:?}");
        Ok(board)
    }

    #[test]
    fn an_old_board_without_the_new_fields_still_loads() -> TestResult {
        let board = board_from("")?;
        let item = &board.jobs[0];
        assert!(item.stage_history.is_empty());
        assert!(item.series.is_none() && item.tokens.is_none() && item.budget_weighted.is_none());
        Ok(())
    }

    #[test]
    fn malformed_new_fields_are_dropped_not_fatal() -> TestResult {
        let board = board_from(
            r#","series":{"index":"two"},"tokens":"lots","budget_weighted":"x","stage_history":[{"stage":"spec","ts":1.0},"junk",{"stage":"build"}]"#,
        )?;
        let item = &board.jobs[0];
        assert!(item.series.is_none() && item.tokens.is_none() && item.budget_weighted.is_none());
        assert_eq!(item.stage_history.len(), 1);
        Ok(())
    }

    #[test]
    fn valid_new_fields_load() -> TestResult {
        let board = board_from(
            r#","series":{"id":"s1","index":2,"total":4,"next_plan":"p.md"},"tokens":{"weighted":500,"raw":4000},"budget_weighted":1000"#,
        )?;
        let item = &board.jobs[0];
        let series = item.series.as_ref().ok_or("no series")?;
        assert_eq!((series.index, series.total), (2, 4));
        assert_eq!(item.tokens.as_ref().map(|t| (t.weighted, t.raw)), Some((500, 4000)));
        assert_eq!(item.budget_weighted, Some(1000));
        Ok(())
    }

    #[test]
    fn the_driver_board_shape_loads() -> TestResult {
        let board = board_from(
            r#","series":{"plan_file":"p.md","index":1,"total":3},"tokens":{"input":100,"cache_creation":200,"output":300,"cache_read":1000}"#,
        )?;
        let item = &board.jobs[0];
        assert_eq!(item.series.as_ref().map(|s| s.total), Some(3));
        assert_eq!(item.tokens.as_ref().map(|t| (t.weighted, t.raw)), Some((700, 1600)));
        Ok(())
    }

    fn entry(stage: &str, ts: f64) -> StageEntry {
        StageEntry { stage: stage.to_string(), ts }
    }

    #[test]
    fn durations_sum_repeated_stages_and_run_the_current_one_to_now() {
        let mut item = job("001", "review", 400.0);
        item.stage_history = vec![
            entry("spec", 0.0), entry("build", 100.0), entry("review", 300.0),
            entry("build", 350.0), entry("review", 400.0),
        ];
        let durations = stage_durations(&item, 460.0);
        let get = |name: &str| durations.iter().find(|d| d.stage == name);
        assert_eq!(get("spec").map(|d| d.seconds), Some(100.0));
        assert_eq!(get("build").map(|d| d.seconds), Some(250.0));
        assert_eq!(get("review").map(|d| (d.seconds, d.current)), Some((110.0, true)));
    }

    #[test]
    fn merged_and_needs_human_have_no_growing_duration() {
        for last in ["merged", "needs-human"] {
            let mut item = job("001", last, 300.0);
            item.stage_history = vec![entry("spec", 0.0), entry("build", 100.0), entry(last, 300.0)];
            let durations = stage_durations(&item, 99_999.0);
            assert!(durations.iter().all(|d| d.stage != last), "{last}");
            assert_eq!(durations.len(), 2);
        }
    }

    #[test]
    fn a_job_without_history_falls_back_to_stage_since_and_a_clock_step_is_not_negative() {
        let durations = stage_durations(&job("001", "build", 100.0), 160.0);
        assert_eq!((durations.len(), durations[0].seconds), (1, 60.0));
        let mut item = job("001", "build", 100.0);
        item.stage_history = vec![entry("spec", 200.0), entry("build", 100.0)];
        assert!(stage_durations(&item, 150.0).iter().all(|d| d.seconds >= 0.0));
    }

    #[test]
    fn the_token_view_has_a_fraction_between_zero_and_one() -> TestResult {
        let mut item = job("001", "build", 1.0);
        assert!(token_view(&item).is_none());
        item.tokens = Some(Tokens { weighted: 750_000, raw: 5_000_000 });
        let view = token_view(&item).ok_or("none")?;
        assert_eq!(view.budget, 3_000_000);
        assert_eq!(view.fraction, 0.25);
        item.budget_weighted = Some(500_000);
        item.tokens = Some(Tokens { weighted: 900_000, raw: 0 });
        assert_eq!(token_view(&item).ok_or("none")?.fraction, 1.0);
        item.budget_weighted = Some(0);
        assert_eq!(token_view(&item).ok_or("none")?.fraction, 0.0);
        Ok(())
    }

    #[test]
    fn agents_attach_only_by_job_id() {
        let mut board = Board::default();
        board.jobs.push(job("001", "review", 10.0));
        board.jobs.push(job("002", "review", 10.0));
        let mut registry = Registry::new();
        start_for(&mut registry, 20.0, "r1", "factory-code-reviewer", Some("002"));
        start_for(&mut registry, 21.0, "r2", "factory-code-reviewer", None);
        start_for(&mut registry, 22.0, "r3", "factory-builder", Some("999"));
        let view = build_state(&board, &registry, 30.0, None, Path::new("/nonexistent"));
        assert!(view.jobs[0].agents.is_empty());
        assert_eq!(view.jobs[1].agents.len(), 1);
        assert_eq!(view.unassigned_agents.len(), 2);
    }

    #[test]
    fn skipped_lines_and_stage_durations_are_in_the_serialized_state() -> TestResult {
        let mut registry = Registry::new();
        registry.ingest_line("garbage");
        registry.ingest_line("more garbage");
        let mut board = Board::default();
        board.jobs.push(job("001", "build", 1.0));
        let json = serde_json::to_value(build_state(&board, &registry, 5.0, None, Path::new("/nonexistent")))?;
        assert_eq!(json["skipped_lines"], 2);
        assert!(json["jobs"][0]["stage_durations"].is_array());
        assert!(json["jobs"][0]["token_view"].is_null());
        Ok(())
    }
}
