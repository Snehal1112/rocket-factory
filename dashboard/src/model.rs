use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// One line of `events.jsonl`, as written by the log-event hook.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Event {
    pub ts: f64,
    pub hook_event_name: String,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub agent_type: Option<String>,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub tool_target: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub last_message: Option<String>,
}

/// One job in `board.json`, as written by `factory-ctl`. Unknown fields are ignored.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Job {
    pub id: String,
    pub dir: String,
    pub slug: String,
    pub created: String,
    pub feature: String,
    pub branch: String,
    #[serde(default)]
    pub worktree: Option<String>,
    #[serde(default)]
    pub builder_id: Option<String>,
    pub stage: String,
    pub round: u32,
    #[serde(default)]
    pub reason: Option<String>,
    pub stage_since: f64,
    pub reviews: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Board {
    #[serde(default)]
    pub jobs: Vec<Job>,
}

/// What the registry knows about one subagent.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AgentState {
    pub agent_id: String,
    pub agent_type: String,
    /// "running" or "done". The state builder may report "stale" instead of "running".
    pub status: String,
    pub started_ts: f64,
    pub ended_ts: Option<f64>,
    pub last_tool: Option<String>,
    pub last_target: Option<String>,
    pub last_tool_ts: Option<f64>,
    pub tool_count: u32,
    pub last_message: Option<String>,
    pub cwd: Option<String>,
}

/// One entry of the live feed.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RecentEvent {
    pub ts: f64,
    /// "start", "stop" or "tool".
    pub kind: String,
    pub agent_id: Option<String>,
    pub agent_type: Option<String>,
    pub tool: Option<String>,
    pub target: Option<String>,
}
