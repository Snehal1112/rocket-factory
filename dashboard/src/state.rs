use std::path::Path;

use serde::Serialize;

use crate::events::Registry;
use crate::model::{AgentState, Board, Draft, Job, StageEntry};
use crate::paths::Paths;
use crate::plan::{load_plan, PlanView};
use crate::specs::{build_spec_cards, SpecCard};

/// A draft is stale when it has not changed for longer than this, unless `--draft-stale-hours` says otherwise.
pub const DEFAULT_DRAFT_STALE_SECS: f64 = 86_400.0;
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
    pub has_park: bool,
}

/// One draft card: the board entry plus its age and a stale flag.
#[derive(Debug, Serialize, PartialEq)]
pub struct DraftCard {
    #[serde(flatten)]
    pub draft: Draft,
    pub seconds_since_update: f64,
    pub stale: bool,
}

/// Cards for the drafts. A draft is stale one second past the threshold, not at it. A clock that
/// runs behind the board gives age 0.
pub fn build_draft_cards(drafts: &[Draft], now: f64, stale_after_secs: f64) -> Vec<DraftCard> {
    drafts
        .iter()
        .map(|draft| {
            let seconds_since_update = (now - draft.updated_at).max(0.0);
            DraftCard {
                draft: draft.clone(),
                seconds_since_update,
                stale: seconds_since_update > stale_after_secs,
            }
        })
        .collect()
}

#[derive(Debug, Serialize)]
pub struct StateView {
    pub generated_at: f64,
    pub board_error: Option<String>,
    pub jobs: Vec<JobView>,
    pub specs: Vec<SpecCard>,
    /// Built with the default threshold here. `state_reply` rebuilds it with the configured one.
    pub drafts: Vec<DraftCard>,
    pub unassigned_agents: Vec<AgentState>,
    pub skipped_lines: u64,
    pub costs: Option<crate::costs::CostsView>,
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
    /// True when the total comes from a running agent's events and is ahead of the board.
    pub live: bool,
}

fn is_terminal(stage: &str) -> bool {
    matches!(stage, "merged" | "needs-human")
}

/// Time spent per stage, summed over repeats. A terminal stage that is still current shows
/// nothing, because its time would only grow.
pub fn stage_durations(job: &Job, now: f64) -> Vec<StageDuration> {
    let history: Vec<StageEntry> = if job.stage_history.is_empty() {
        vec![StageEntry {
            stage: job.stage.clone(),
            ts: job.stage_since,
        }]
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
            None => out.push(StageDuration {
                stage: entry.stage.clone(),
                seconds,
                current,
            }),
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
    Some(TokenView {
        weighted: tokens.weighted,
        raw: tokens.raw,
        budget,
        fraction,
        live: false,
    })
}

/// The card bar figures: the board total, or the highest running total from the job's agent events
/// when that is ahead. The board is written only when an agent run ends, so a running agent is ahead of it.
pub fn live_token_view(job: &Job, agents: &[AgentState]) -> Option<TokenView> {
    let board = token_view(job);
    let live = agents.iter().map(|a| a.tokens_weighted).max().unwrap_or(0);
    if live == 0 || live <= board.as_ref().map_or(0, |v| v.weighted) {
        return board;
    }
    let budget = job.budget_weighted.unwrap_or(DEFAULT_BUDGET_WEIGHTED);
    let fraction = if budget == 0 {
        0.0
    } else {
        (live as f64 / budget as f64).clamp(0.0, 1.0)
    };
    Some(TokenView {
        weighted: live,
        raw: board.map_or(0, |v| v.raw),
        budget,
        fraction,
        live: true,
    })
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

/// True when the board names a park kind and the evidence file is in the job folder.
fn has_park(paths: &Paths, job: &Job) -> bool {
    job.park_kind.is_some() && crate::park::park_path(paths, &job.dir).is_some_and(|p| p.is_file())
}

/// True when one warning part is a finding the old full-header rules raise on a compact plan.
fn is_old_finding(part: &str) -> bool {
    let Some((_, finding)) = part.split_once(".md ") else {
        return false;
    };
    let old_header = [
        "header has no Goal",
        "header has no Architecture",
        "header has no Tech Stack",
        "header has no Spec",
        "header has no Global Constraints",
        "header has no Review Focus",
        "does not name superpowers:subagent-driven-development as its executor",
    ]
    .contains(&finding);
    let old_files = finding
        .strip_prefix("task ")
        .and_then(|rest| rest.split_once(" has no **Files:**"))
        .is_some_and(|(number, tail)| tail.is_empty() && number.parse::<u32>().is_ok());
    old_header || old_files
}

/// True when any part of the warning is an old-header or old-files finding. Lint texts never
/// contain "; ", so the split is safe.
fn has_old_finding(warning: &str) -> bool {
    warning.split("; ").any(is_old_finding)
}

/// Old jobs retain the warnings from their creation. The compact factory plan format is now
/// accepted, so hide only the findings caused by applying the old full-header rules to it.
fn visible_spec_warning(paths: &Paths, job: &Job) -> Option<String> {
    let warning = job.spec_warning.as_ref()?;
    if !has_old_finding(warning) {
        return Some(warning.clone());
    }
    let plan = std::fs::read_to_string(paths.sdd.join(&job.dir).join("plan.md")).ok();
    let compact = plan.as_deref().is_some_and(|text| {
        // Only the part before the first task counts as the header.
        let end = [text.find("### Task "), text.find("- [ ]")]
            .into_iter()
            .flatten()
            .min()
            .unwrap_or(text.len());
        let header = &text[..end];
        header.contains("**Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review.")
            && header.contains("> **Spec:**")
            && header.contains("## File Structure")
            && text.contains("**Interfaces:**")
            && text.contains("## Review Focus")
    });
    if !compact {
        return Some(warning.clone());
    }
    let remaining: Vec<&str> = warning
        .split("; ")
        .filter(|part| !is_old_finding(part))
        .collect();
    (!remaining.is_empty()).then(|| remaining.join("; "))
}

pub fn build_state(
    board: &Board,
    registry: &Registry,
    now: f64,
    board_error: Option<String>,
    paths: &Paths,
) -> StateView {
    let mut jobs: Vec<JobView> = board
        .jobs
        .iter()
        .map(|job| JobView {
            job: {
                let mut visible = job.clone();
                visible.spec_warning = visible_spec_warning(paths, job);
                visible
            },
            agents: Vec::new(),
            seconds_in_stage: (now - job.stage_since).max(0.0),
            plan: load_plan(
                paths,
                &job.dir,
                &job.stage,
                job.reviews
                    .values()
                    .any(|v| matches!(v.as_str(), "pass" | "changes" | "running")),
            ),
            stage_durations: stage_durations(job, now),
            token_view: token_view(job),
            has_park: has_park(paths, job),
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
        view.token_view = live_token_view(&view.job, &view.agents);
    }
    unassigned.sort_by(|a, b| a.started_ts.total_cmp(&b.started_ts));
    let specs = build_spec_cards(&jobs, paths);
    StateView {
        generated_at: now,
        board_error,
        jobs,
        specs,
        drafts: build_draft_cards(&board.drafts, now, DEFAULT_DRAFT_STALE_SECS),
        unassigned_agents: unassigned,
        skipped_lines: registry.skipped_lines,
        costs: crate::costs::load_costs(paths),
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
            cancelled: None,
            park_kind: None,
            park_at: None,
            estimate: None,
            spec_warning: None,
            provider: None,
            last_review: None,
        }
    }

    fn start_for(registry: &mut Registry, ts: f64, id: &str, kind: &str, job_id: Option<&str>) {
        let job = job_id.map_or(String::new(), |j| format!(r#","job_id":"{j}""#));
        registry.ingest_line(&format!(
            r#"{{"ts":{ts},"hook_event_name":"SubagentStart","agent_id":"{id}","agent_type":"{kind}"{job}}}"#
        ));
    }

    #[test]
    fn has_park_needs_both_the_board_kind_and_the_file() -> TestResult {
        let root = tempfile::tempdir()?;
        let dir = root.path().join(".superpowers/sdd/2026-10-13-001-x");
        std::fs::create_dir_all(&dir)?;
        let mut parked = job("001", "needs-human", 0.0);
        parked.park_kind = Some("checks-failed".to_string());
        let board = Board {
            jobs: vec![parked.clone()],
            ..Board::default()
        };
        // The kind is on the board but the file is missing.
        let view = build_state(
            &board,
            &Registry::new(),
            10.0,
            None,
            &Paths::for_root(root.path()),
        );
        assert!(!view.jobs[0].has_park);
        std::fs::write(dir.join("park.json"), "{}")?;
        let view = build_state(
            &board,
            &Registry::new(),
            10.0,
            None,
            &Paths::for_root(root.path()),
        );
        assert!(view.jobs[0].has_park);
        // The file exists but the board has no kind, as for an older job.
        let mut old = parked;
        old.park_kind = None;
        let view = build_state(
            &Board {
                jobs: vec![old],
                ..Board::default()
            },
            &Registry::new(),
            10.0,
            None,
            &Paths::for_root(root.path()),
        );
        assert!(!view.jobs[0].has_park);
        Ok(())
    }

    #[test]
    fn the_state_json_carries_has_park_but_no_evidence_text() -> TestResult {
        let root = tempfile::tempdir()?;
        let dir = root.path().join(".superpowers/sdd/2026-10-13-001-x");
        std::fs::create_dir_all(&dir)?;
        std::fs::write(
            dir.join("park.json"),
            r#"{"parts":[{"text":"SECRET-EVIDENCE"}]}"#,
        )?;
        let mut parked = job("001", "needs-human", 0.0);
        parked.park_kind = Some("other".to_string());
        let view = build_state(
            &Board {
                jobs: vec![parked],
                ..Board::default()
            },
            &Registry::new(),
            10.0,
            None,
            &Paths::for_root(root.path()),
        );
        let json = serde_json::to_string(&view)?;
        assert!(json.contains("\"has_park\":true"), "{json}");
        assert!(json.contains("\"park_kind\":\"other\""), "{json}");
        assert!(!json.contains("SECRET-EVIDENCE"), "{json}");
        Ok(())
    }

    #[test]
    fn the_state_has_no_recent_activity_feed() -> TestResult {
        let view = build_state(
            &Board::default(),
            &Registry::new(),
            10.0,
            None,
            &Paths::nowhere(),
        );
        let json = serde_json::to_value(&view)?;
        assert!(json.get("recent").is_none(), "the recent feed was removed");
        assert!(
            json.get("skipped_lines").is_some(),
            "the skipped-lines count stays"
        );
        assert_eq!(
            json.get("specs").and_then(|v| v.as_array()).map(Vec::len),
            Some(0),
            "specs is an empty list"
        );
        Ok(())
    }

    fn series_job(id: &str, stage: &str, index: u32, series_id: Option<&str>) -> serde_json::Value {
        serde_json::json!({
            "id": id, "dir": format!("2026-10-13-{id}-x"), "slug": "x", "created": "2026-10-13",
            "feature": "f", "branch": format!("factory/{id}-x"), "stage": stage, "round": 0,
            "stage_since": 0.0, "reviews": {},
            "series": { "plan_file": "p.md", "index": index, "total": 2, "id": series_id },
        })
    }

    #[test]
    fn the_state_has_one_spec_card_per_series_and_none_for_old_jobs() -> TestResult {
        let root = tempfile::tempdir()?;
        let dir = root.path().join(".superpowers/sdd/2026-10-13-001-x");
        std::fs::create_dir_all(&dir)?;
        std::fs::write(
            dir.join("spec.md"),
            "# Spec: Demo spec\n\n## Goal\n\nShow the plans.\n",
        )?;
        let board: Board = serde_json::from_value(serde_json::json!({ "jobs": [
            series_job("001", "build", 1, Some("S1")),
            series_job("002", "spec", 2, Some("S1")),
            series_job("003", "merged", 1, None),
        ]}))?;
        let view = build_state(
            &board,
            &Registry::new(),
            10.0,
            None,
            &Paths::for_root(root.path()),
        );
        assert_eq!(view.specs.len(), 1);
        assert_eq!(view.specs[0].series_id, "S1");
        assert_eq!(view.specs[0].title.as_deref(), Some("Demo spec"));
        let states: Vec<_> = view.specs[0]
            .plans
            .iter()
            .map(|r| r.state.as_str())
            .collect();
        assert_eq!(states, vec!["building", "waiting"]);
        let json = serde_json::to_value(&view)?;
        assert_eq!(json["specs"][0]["plans"][1]["job_id"], "002");
        assert!(json["specs"][0]["stage_since"].is_number());
        assert_eq!(
            json["jobs"].as_array().map(Vec::len),
            Some(3),
            "plan jobs stay in jobs[]"
        );
        Ok(())
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
        let fresh = build_state(&board, &registry, 100.0, None, &Paths::nowhere());
        assert_eq!(fresh.jobs[0].agents[0].status, "running");
        let old = build_state(&board, &registry, 20.0 + 601.0, None, &Paths::nowhere());
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
        let view = build_state(&board, &registry, 1300.0, None, &Paths::nowhere());
        assert_eq!(view.jobs[0].agents[0].status, "running");
    }

    #[test]
    fn a_recent_done_unassigned_agent_is_kept() {
        let mut registry = Registry::new();
        start_for(&mut registry, 100.0, "u1", "other", None);
        registry.ingest_line(r#"{"ts":150.0,"hook_event_name":"SubagentStop","agent_id":"u1"}"#);
        let view = build_state(&Board::default(), &registry, 200.0, None, &Paths::nowhere());
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
            job_id: None,
            tokens_weighted: 0,
        };
        assert!(!keep_unassigned(&agent, 200.0));
    }

    #[test]
    fn a_running_unassigned_agent_with_recent_activity_is_kept() {
        let mut registry = Registry::new();
        start_for(&mut registry, 100.0, "u1", "other", None);
        let view = build_state(&Board::default(), &registry, 200.0, None, &Paths::nowhere());
        assert_eq!(view.unassigned_agents.len(), 1);
    }

    #[test]
    fn a_stale_unassigned_agent_older_than_two_hours_is_dropped() {
        let mut registry = Registry::new();
        start_for(&mut registry, 100.0, "u1", "other", None);
        let view = build_state(
            &Board::default(),
            &registry,
            100.0 + 7201.0,
            None,
            &Paths::nowhere(),
        );
        assert!(view.unassigned_agents.is_empty());
        let view = build_state(
            &Board::default(),
            &registry,
            100.0 + 3000.0,
            None,
            &Paths::nowhere(),
        );
        assert_eq!(view.unassigned_agents.len(), 1);
        assert_eq!(view.unassigned_agents[0].status, "stale");
    }

    #[test]
    fn seconds_in_stage_and_the_board_error_are_reported() {
        let mut board = Board::default();
        board.jobs.push(job("001", "build", 100.0));
        let view = build_state(
            &board,
            &Registry::new(),
            160.0,
            Some("oops".to_string()),
            &Paths::nowhere(),
        );
        assert_eq!(view.jobs[0].seconds_in_stage, 60.0);
        assert_eq!(view.board_error.as_deref(), Some("oops"));
        assert_eq!(view.generated_at, 160.0);
    }

    #[test]
    fn the_state_serializes_with_flattened_job_fields() -> TestResult {
        let mut board = Board::default();
        board.jobs.push(job("001", "build", 100.0));
        let view = build_state(&board, &Registry::new(), 160.0, None, &Paths::nowhere());
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
    fn a_board_written_by_the_driver_parses_history_and_budget() -> TestResult {
        // The same shape factory-core writes: history entries use `at`, plus a budget.
        let board = board_from(
            r#","stage_history":[{"stage":"spec","at":100.0},{"stage":"build","at":160.5}],"budget_weighted":3000000,"tokens":{"input":1,"cache_creation":2,"output":3,"cache_read":40}"#,
        )?;
        let item = &board.jobs[0];
        assert_eq!(item.stage_history.len(), 2);
        assert_eq!(item.stage_history[1].stage, "build");
        assert_eq!(item.stage_history[1].ts, 160.5);
        assert_eq!(item.budget_weighted, Some(3_000_000));
        Ok(())
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
        assert_eq!(
            item.tokens.as_ref().map(|t| (t.weighted, t.raw)),
            Some((500, 4000))
        );
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
        assert_eq!(
            item.tokens.as_ref().map(|t| (t.weighted, t.raw)),
            Some((700, 1600))
        );
        Ok(())
    }

    fn entry(stage: &str, ts: f64) -> StageEntry {
        StageEntry {
            stage: stage.to_string(),
            ts,
        }
    }

    #[test]
    fn durations_sum_repeated_stages_and_run_the_current_one_to_now() {
        let mut item = job("001", "review", 400.0);
        item.stage_history = vec![
            entry("spec", 0.0),
            entry("build", 100.0),
            entry("review", 300.0),
            entry("build", 350.0),
            entry("review", 400.0),
        ];
        let durations = stage_durations(&item, 460.0);
        let get = |name: &str| durations.iter().find(|d| d.stage == name);
        assert_eq!(get("spec").map(|d| d.seconds), Some(100.0));
        assert_eq!(get("build").map(|d| d.seconds), Some(250.0));
        assert_eq!(
            get("review").map(|d| (d.seconds, d.current)),
            Some((110.0, true))
        );
    }

    #[test]
    fn merged_and_needs_human_have_no_growing_duration() {
        for last in ["merged", "needs-human"] {
            let mut item = job("001", last, 300.0);
            item.stage_history = vec![
                entry("spec", 0.0),
                entry("build", 100.0),
                entry(last, 300.0),
            ];
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
        assert!(stage_durations(&item, 150.0)
            .iter()
            .all(|d| d.seconds >= 0.0));
    }

    #[test]
    fn the_token_view_has_a_fraction_between_zero_and_one() -> TestResult {
        let mut item = job("001", "build", 1.0);
        assert!(token_view(&item).is_none());
        item.tokens = Some(Tokens {
            weighted: 750_000,
            raw: 5_000_000,
        });
        let view = token_view(&item).ok_or("none")?;
        assert_eq!(view.budget, 3_000_000);
        assert_eq!(view.fraction, 0.25);
        item.budget_weighted = Some(500_000);
        item.tokens = Some(Tokens {
            weighted: 900_000,
            raw: 0,
        });
        assert_eq!(token_view(&item).ok_or("none")?.fraction, 1.0);
        item.budget_weighted = Some(0);
        assert_eq!(token_view(&item).ok_or("none")?.fraction, 0.0);
        Ok(())
    }

    fn sample_agent() -> AgentState {
        AgentState {
            agent_id: "a1".to_string(),
            agent_type: "other".to_string(),
            status: "running".to_string(),
            started_ts: 100.0,
            ended_ts: None,
            last_tool: None,
            last_target: None,
            last_tool_ts: None,
            tool_count: 0,
            last_message: None,
            cwd: None,
            job_id: None,
            tokens_weighted: 0,
        }
    }

    fn agent_with(tokens_weighted: u64) -> AgentState {
        let mut agent = sample_agent();
        agent.tokens_weighted = tokens_weighted;
        agent
    }

    #[test]
    fn a_running_agent_moves_the_bar_ahead_of_the_board() {
        let mut item = job("001", "build", 1.0);
        item.tokens = Some(Tokens {
            weighted: 100_000,
            raw: 800_000,
        });
        let view = live_token_view(&item, &[agent_with(250_000)]).expect("view");
        assert_eq!(view.weighted, 250_000);
        assert_eq!(view.raw, 800_000);
        assert!(view.live);
        assert!((view.fraction - 250_000.0 / DEFAULT_BUDGET_WEIGHTED as f64).abs() < 1e-9);
    }

    #[test]
    fn a_job_without_board_tokens_gets_a_live_bar_from_its_agent() {
        let item = job("001", "build", 1.0);
        let view = live_token_view(&item, &[agent_with(60_000)]).expect("view");
        assert_eq!((view.weighted, view.raw, view.live), (60_000, 0, true));
    }

    #[test]
    fn a_finished_job_keeps_the_board_figure() {
        let mut item = job("001", "merged", 1.0);
        item.tokens = Some(Tokens {
            weighted: 300_000,
            raw: 2_000_000,
        });
        let view =
            live_token_view(&item, &[agent_with(300_000), agent_with(40_000)]).expect("view");
        assert_eq!(
            (view.weighted, view.raw, view.live),
            (300_000, 2_000_000, false)
        );
    }

    #[test]
    fn the_board_wins_when_every_agent_total_is_lower() {
        let mut item = job("001", "build", 1.0);
        item.tokens = Some(Tokens {
            weighted: 90_000,
            raw: 500_000,
        });
        let view = live_token_view(&item, &[agent_with(20_000)]).expect("view");
        assert_eq!((view.weighted, view.live), (90_000, false));
    }

    #[test]
    fn no_board_tokens_and_no_agent_tokens_means_no_view() {
        let item = job("001", "build", 1.0);
        assert!(live_token_view(&item, &[]).is_none());
        assert!(live_token_view(&item, &[agent_with(0)]).is_none());
    }

    #[test]
    fn the_live_fraction_is_clamped_and_a_zero_budget_gives_zero() {
        let mut item = job("001", "build", 1.0);
        item.budget_weighted = Some(1_000);
        let view = live_token_view(&item, &[agent_with(5_000)]).expect("view");
        assert_eq!(view.fraction, 1.0);
        item.budget_weighted = Some(0);
        let view = live_token_view(&item, &[agent_with(5_000)]).expect("view");
        assert_eq!(view.fraction, 0.0);
    }

    #[test]
    fn agents_attach_only_by_job_id() {
        let mut board = Board::default();
        board.jobs.push(job("001", "review", 10.0));
        board.jobs.push(job("002", "review", 10.0));
        let mut registry = Registry::new();
        start_for(
            &mut registry,
            20.0,
            "r1",
            "factory-code-reviewer",
            Some("002"),
        );
        start_for(&mut registry, 21.0, "r2", "factory-code-reviewer", None);
        start_for(&mut registry, 22.0, "r3", "factory-builder", Some("999"));
        let view = build_state(&board, &registry, 30.0, None, &Paths::nowhere());
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
        let json =
            serde_json::to_value(build_state(&board, &registry, 5.0, None, &Paths::nowhere()))?;
        assert_eq!(json["skipped_lines"], 2);
        assert!(json["jobs"][0]["stage_durations"].is_array());
        assert!(json["jobs"][0]["token_view"].is_null());
        Ok(())
    }

    #[test]
    fn the_state_carries_the_group_summary_but_never_a_per_job_list() -> TestResult {
        let root = tempfile::tempdir()?;
        let dir = root.path().join(".superpowers/factory");
        std::fs::create_dir_all(&dir)?;
        std::fs::write(
            dir.join("costs.json"),
            r#"{"generated_at":1.0,"groups":{"small":{"n":1,"n_approx":0,"median":9,"low":9,"high":9,"roles":{},"corrected":null}},"roles":{"builder":100},"jobs":[{"id":"SECRET-JOB"}]}"#,
        )?;
        let view = build_state(
            &Board::default(),
            &Registry::new(),
            10.0,
            None,
            &Paths::for_root(root.path()),
        );
        let json = serde_json::to_string(&view)?;
        assert!(json.contains("\"costs\":{"), "{json}");
        assert!(json.contains("\"median\":9"), "{json}");
        assert!(!json.contains("SECRET-JOB"), "{json}");
        let none = build_state(
            &Board::default(),
            &Registry::new(),
            10.0,
            None,
            &Paths::nowhere(),
        );
        assert!(serde_json::to_value(&none)?["costs"].is_null());
        Ok(())
    }

    #[test]
    fn a_job_estimate_passes_through_and_a_malformed_one_is_dropped() -> TestResult {
        let good: Board = serde_json::from_str(
            r#"{"jobs":[{"id":"001","dir":"d","slug":"s","created":"c","feature":"f","branch":"b","stage":"build","round":0,"stage_since":1.0,"reviews":{},
                "estimate":{"group":"small","median":250000,"low":220000,"high":310000,"n":4,"n_approx":2}}]}"#,
        )?;
        let view = build_state(&good, &Registry::new(), 10.0, None, &Paths::nowhere());
        let json = serde_json::to_value(&view)?;
        assert_eq!(json["jobs"][0]["estimate"]["median"], 250_000);
        let bad: Board = serde_json::from_str(
            r#"{"jobs":[{"id":"001","dir":"d","slug":"s","created":"c","feature":"f","branch":"b","stage":"build","round":0,"stage_since":1.0,"reviews":{},"estimate":"lots"}]}"#,
        )?;
        assert!(bad.jobs[0].estimate.is_none());
        Ok(())
    }

    fn draft_json(slug: &str, updated_at: f64) -> serde_json::Value {
        serde_json::json!({
            "slug": slug, "feature": "f", "stage": "interview", "spec_file": null,
            "created": "2026-10-18", "updated_at": updated_at,
        })
    }

    #[test]
    fn an_old_board_without_drafts_still_loads() -> TestResult {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("board.json");
        std::fs::write(
            &path,
            r#"{"jobs":[{"id":"001","dir":"d","slug":"s","created":"2026-10-13","feature":"f","branch":"b","stage":"build","round":1,"stage_since":5.0,"reviews":{}}]}"#,
        )?;
        let (board, error) = load_board(&path);
        assert!(error.is_none());
        assert!(board.drafts.is_empty());
        assert_eq!(board.jobs[0].spec_warning, None);
        Ok(())
    }

    #[test]
    fn a_malformed_draft_is_dropped_not_fatal() -> TestResult {
        let board: Board = serde_json::from_value(serde_json::json!({
            "jobs": [],
            "drafts": [draft_json("good", 1.0), {"slug": 5}, "junk", draft_json("also-good", 2.0)],
        }))?;
        let slugs: Vec<_> = board.drafts.iter().map(|d| d.slug.as_str()).collect();
        assert_eq!(slugs, vec!["good", "also-good"]);
        let not_a_list: Board = serde_json::from_value(serde_json::json!({ "drafts": "nope" }))?;
        assert!(not_a_list.drafts.is_empty());
        Ok(())
    }

    #[test]
    fn a_draft_is_stale_one_second_past_the_threshold_and_not_at_it() -> TestResult {
        let board: Board =
            serde_json::from_value(serde_json::json!({ "drafts": [draft_json("d", 100.0)] }))?;
        let at = build_draft_cards(&board.drafts, 100.0 + 86_400.0, DEFAULT_DRAFT_STALE_SECS);
        assert!(!at[0].stale);
        assert_eq!(at[0].seconds_since_update, 86_400.0);
        let past = build_draft_cards(&board.drafts, 100.0 + 86_401.0, DEFAULT_DRAFT_STALE_SECS);
        assert!(past[0].stale);
        let behind = build_draft_cards(&board.drafts, 50.0, DEFAULT_DRAFT_STALE_SECS);
        assert_eq!(behind[0].seconds_since_update, 0.0);
        assert!(!behind[0].stale);
        Ok(())
    }

    #[test]
    fn the_state_json_has_a_drafts_array() -> TestResult {
        let empty = serde_json::to_value(build_state(
            &Board::default(),
            &Registry::new(),
            10.0,
            None,
            &Paths::nowhere(),
        ))?;
        assert_eq!(empty["drafts"].as_array().map(Vec::len), Some(0));
        let board: Board =
            serde_json::from_value(serde_json::json!({ "drafts": [draft_json("d", 4.0)] }))?;
        let json = serde_json::to_value(build_state(
            &board,
            &Registry::new(),
            10.0,
            None,
            &Paths::nowhere(),
        ))?;
        assert_eq!(json["drafts"][0]["slug"], "d");
        assert_eq!(json["drafts"][0]["stage"], "interview");
        assert_eq!(json["drafts"][0]["seconds_since_update"], 6.0);
        assert_eq!(json["drafts"][0]["stale"], false);
        Ok(())
    }

    #[test]
    fn a_job_spec_warning_is_passed_through() -> TestResult {
        let mut value = series_job("001", "build", 1, None);
        value["spec_warning"] = serde_json::json!("no decision");
        let mut bad = series_job("002", "build", 1, None);
        bad["spec_warning"] = serde_json::json!(7);
        let board: Board = serde_json::from_value(serde_json::json!({ "jobs": [value, bad] }))?;
        let view = build_state(&board, &Registry::new(), 10.0, None, &Paths::nowhere());
        let json = serde_json::to_value(&view)?;
        assert_eq!(json["jobs"][0]["spec_warning"], "no decision");
        assert!(
            json["jobs"][1].get("spec_warning").is_none(),
            "a bad value is dropped"
        );
        Ok(())
    }

    #[test]
    fn an_existing_compact_plan_drops_obsolete_lint_warnings_from_its_card() -> TestResult {
        let root = tempfile::tempdir()?;
        let paths = Paths::for_root(root.path());
        let dir = "2026-10-22-002-codex-factory-smoke-test";
        std::fs::create_dir_all(paths.sdd.join(dir))?;
        std::fs::write(
            paths.sdd.join(dir).join("plan.md"),
            "# Plan\n\n> **Spec:** docs/superpowers/specs/smoke-design.md\n> **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review.\n\n## File Structure\n- `spec/health_spec.rb` — tests.\n\n### Task 1: health\n**Interfaces:**\n- GET /\n\n## Review Focus\n1. Root route remains public.\nNext plan: none\n",
        )?;
        let mut old = job("002", "build", 0.0);
        old.dir = dir.to_string();
        old.spec_warning = Some("plan-01-smoke.md header has no Goal; plan-01-smoke.md header has no Architecture; plan-01-smoke.md header has no Tech Stack; plan-01-smoke.md header has no Spec; plan-01-smoke.md header has no Global Constraints; plan-01-smoke.md header has no Review Focus; plan-01-smoke.md does not name superpowers:subagent-driven-development as its executor; plan-01-smoke.md task 1 has no **Files:**; no decision was answered by the user".to_string());
        let board = Board {
            jobs: vec![old.clone()],
            ..Board::default()
        };
        let view = build_state(&board, &Registry::new(), 10.0, None, &paths);
        assert_eq!(
            view.jobs[0].job.spec_warning.as_deref(),
            Some("no decision was answered by the user")
        );
        assert_eq!(
            board.jobs[0].spec_warning, old.spec_warning,
            "board data stays intact"
        );
        old.spec_warning = Some(
            old.spec_warning
                .expect("warning")
                .replace("; no decision was answered by the user", ""),
        );
        let board = Board {
            jobs: vec![old],
            ..Board::default()
        };
        let view = build_state(&board, &Registry::new(), 10.0, None, &paths);
        assert_eq!(view.jobs[0].job.spec_warning, None);
        Ok(())
    }

    #[test]
    fn a_prose_mention_does_not_make_a_plan_compact() -> TestResult {
        let root = tempfile::tempdir()?;
        let paths = Paths::for_root(root.path());
        let dir = "2026-10-22-003-prose";
        std::fs::create_dir_all(paths.sdd.join(dir))?;
        std::fs::write(
            paths.sdd.join(dir).join("plan.md"),
            "# Plan\n\n### Task 1: quote\n**Interfaces:**\n- x\n\nThe text says > **Spec:** and **Executor:** the factory driver runs this plan with one fresh factory-builder implementer per task and no per-task review.\n\n## File Structure\n## Review Focus\n",
        )?;
        let mut old = job("003", "build", 0.0);
        old.dir = dir.to_string();
        old.spec_warning = Some("x.md header has no Goal".to_string());
        let board = Board {
            jobs: vec![old],
            ..Board::default()
        };
        let view = build_state(&board, &Registry::new(), 10.0, None, &paths);
        assert_eq!(
            view.jobs[0].job.spec_warning.as_deref(),
            Some("x.md header has no Goal")
        );
        Ok(())
    }

    #[test]
    fn no_plan_file_is_read_without_an_old_header_finding() {
        assert!(!has_old_finding("x.md has no Review Focus"));
        assert!(!has_old_finding("no decision was answered by the user"));
        assert!(!has_old_finding("x.md task 1 has no **Interfaces:**"));
        assert!(has_old_finding("x.md header has no Goal"));
        assert!(has_old_finding(
            "no decision; x.md task 12 has no **Files:**"
        ));
    }
}
