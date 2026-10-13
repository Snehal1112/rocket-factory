use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};

/// One line of `events.jsonl`, as written by the factory driver.
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
    #[serde(default)]
    pub job_id: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub tokens: Option<Tokens>,
}

/// Weighted and raw token usage.
///
/// The driver writes a plain number on events (weighted tokens of the job so far) and four
/// counters on the board. Both shapes and the `{weighted, raw}` form are read.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Tokens {
    pub weighted: u64,
    pub raw: u64,
}

impl<'de> Deserialize<'de> for Tokens {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(d)?;
        let bad = || serde::de::Error::custom("bad tokens value");
        if let Some(n) = value.as_u64() {
            return Ok(Tokens { weighted: n, raw: 0 });
        }
        let map = value.as_object().ok_or_else(bad)?;
        let num = |key: &str| map.get(key).and_then(serde_json::Value::as_u64);
        if let Some(weighted) = num("weighted") {
            return Ok(Tokens { weighted, raw: num("raw").unwrap_or(0) });
        }
        let (input, creation, output, read) = (num("input"), num("cache_creation"), num("output"), num("cache_read"));
        if input.is_none() && creation.is_none() && output.is_none() && read.is_none() {
            return Err(bad());
        }
        let (input, creation, output, read) =
            (input.unwrap_or(0), creation.unwrap_or(0), output.unwrap_or(0), read.unwrap_or(0));
        let raw = input.saturating_add(creation).saturating_add(output).saturating_add(read);
        // Same formula as factory-core: cache reads count one tenth.
        let weighted = input
            .saturating_add(creation)
            .saturating_add(output)
            .saturating_add(read / 10);
        Ok(Tokens { weighted, raw })
    }
}

/// One stage change in a job's history.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct StageEntry {
    pub stage: String,
    pub ts: f64,
}

/// Where a job sits in a multi-plan series. The index is 1-based.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Series {
    pub index: u32,
    pub total: u32,
    #[serde(default)]
    pub id: serde_json::Value,
    #[serde(default, alias = "plan_file")]
    pub next_plan: serde_json::Value,
}

/// A bad optional value becomes none, so it never makes the whole board unreadable.
fn lenient<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let value = Option::<serde_json::Value>::deserialize(d)?;
    Ok(value.and_then(|v| serde_json::from_value(v).ok()))
}

/// Bad history entries are dropped one by one, and a non-array value gives an empty list.
fn lenient_history<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<StageEntry>, D::Error> {
    let value = serde_json::Value::deserialize(d)?;
    Ok(value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|v| serde_json::from_value(v.clone()).ok())
                .collect()
        })
        .unwrap_or_default())
}

/// One job in `board.json`, as written by the factory driver. Unknown fields are ignored.
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
    #[serde(default, deserialize_with = "lenient_history")]
    pub stage_history: Vec<StageEntry>,
    #[serde(default, deserialize_with = "lenient")]
    pub series: Option<Series>,
    #[serde(default, deserialize_with = "lenient")]
    pub tokens: Option<Tokens>,
    #[serde(default, deserialize_with = "lenient")]
    pub budget_weighted: Option<u64>,
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
    pub job_id: Option<String>,
    pub tokens_weighted: u64,
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
