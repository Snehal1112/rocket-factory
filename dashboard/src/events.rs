use std::collections::{BTreeMap, VecDeque};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

use crate::model::{AgentState, Event, RecentEvent};

const MAX_RECENT: usize = 40;

/// Folds hook events into per-agent state and a short live feed.
#[derive(Debug, Default)]
pub struct Registry {
    agents: BTreeMap<String, AgentState>,
    recent: VecDeque<RecentEvent>,
    pub skipped_lines: u64,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parses one JSONL line and applies it. Returns false for a line that is not a valid event.
    pub fn ingest_line(&mut self, line: &str) -> bool {
        match serde_json::from_str::<Event>(line) {
            Ok(event) => {
                self.ingest(&event);
                true
            }
            Err(_) => {
                self.skipped_lines += 1;
                false
            }
        }
    }

    pub fn ingest(&mut self, event: &Event) {
        match event.hook_event_name.as_str() {
            "SubagentStart" => self.on_start(event),
            "SubagentStop" => self.on_stop(event),
            "PostToolUse" => self.on_tool(event),
            _ => {}
        }
    }

    pub fn agents(&self) -> Vec<AgentState> {
        self.agents.values().cloned().collect()
    }

    /// Newest first.
    pub fn recent(&self) -> Vec<RecentEvent> {
        self.recent.iter().rev().cloned().collect()
    }

    fn entry(&mut self, event: &Event, agent_id: &str) -> &mut AgentState {
        self.agents
            .entry(agent_id.to_string())
            .or_insert_with(|| AgentState {
                agent_id: agent_id.to_string(),
                agent_type: event.agent_type.clone().unwrap_or_default(),
                status: "running".to_string(),
                started_ts: event.ts,
                ended_ts: None,
                last_tool: None,
                last_target: None,
                last_tool_ts: None,
                tool_count: 0,
                last_message: None,
                cwd: event.cwd.clone(),
            })
    }

    fn on_start(&mut self, event: &Event) {
        let Some(id) = event.agent_id.as_deref() else {
            return;
        };
        let agent = self.entry(event, id);
        if agent.status == "done" {
            agent.started_ts = event.ts;
        }
        agent.status = "running".to_string();
        agent.ended_ts = None;
        if let Some(kind) = event.agent_type.as_deref() {
            if !kind.is_empty() {
                agent.agent_type = kind.to_string();
            }
        }
        self.push_recent(event, "start");
    }

    fn on_stop(&mut self, event: &Event) {
        let Some(id) = event.agent_id.as_deref() else {
            return;
        };
        let agent = self.entry(event, id);
        agent.status = "done".to_string();
        agent.ended_ts = Some(event.ts);
        if let Some(message) = &event.last_message {
            agent.last_message = Some(message.clone());
        }
        self.push_recent(event, "stop");
    }

    fn on_tool(&mut self, event: &Event) {
        // Tool calls of the orchestrator itself carry no agent id and are not tracked.
        let Some(id) = event.agent_id.as_deref() else {
            return;
        };
        let agent = self.entry(event, id);
        agent.tool_count += 1;
        agent.last_tool = event.tool_name.clone();
        agent.last_target = event.tool_target.clone();
        agent.last_tool_ts = Some(event.ts);
        if agent.cwd.is_none() {
            agent.cwd = event.cwd.clone();
        }
        self.push_recent(event, "tool");
    }

    fn push_recent(&mut self, event: &Event, kind: &str) {
        self.recent.push_back(RecentEvent {
            ts: event.ts,
            kind: kind.to_string(),
            agent_id: event.agent_id.clone(),
            agent_type: event.agent_type.clone(),
            tool: event.tool_name.clone(),
            target: event.tool_target.clone(),
        });
        while self.recent.len() > MAX_RECENT {
            self.recent.pop_front();
        }
    }
}

/// Reads new complete lines from a growing file.
#[derive(Debug)]
pub struct Tailer {
    path: PathBuf,
    offset: u64,
    partial: String,
}

impl Tailer {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            offset: 0,
            partial: String::new(),
        }
    }

    /// Returns the complete lines added since the last call. A missing file gives no lines.
    /// A shorter file than before is treated as a new file and read from the start.
    pub fn poll(&mut self) -> Vec<String> {
        let Ok(mut file) = File::open(&self.path) else {
            return Vec::new();
        };
        let Ok(meta) = file.metadata() else {
            return Vec::new();
        };
        let len = meta.len();
        if len < self.offset {
            self.offset = 0;
            self.partial.clear();
        }
        if len == self.offset {
            return Vec::new();
        }
        if file.seek(SeekFrom::Start(self.offset)).is_err() {
            return Vec::new();
        }
        let mut bytes = Vec::new();
        if file.read_to_end(&mut bytes).is_err() {
            return Vec::new();
        }
        self.offset += bytes.len() as u64;
        self.partial.push_str(&String::from_utf8_lossy(&bytes));
        let mut lines = Vec::new();
        while let Some(pos) = self.partial.find('\n') {
            let line: String = self.partial.drain(..=pos).collect();
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                lines.push(trimmed.to_string());
            }
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    const START: &str = r#"{"ts":100.0,"hook_event_name":"SubagentStart","agent_id":"a1","agent_type":"factory-builder","cwd":"/wt"}"#;
    const TOOL: &str = r#"{"ts":101.5,"hook_event_name":"PostToolUse","agent_id":"a1","tool_name":"Edit","tool_target":"src/App.tsx","cwd":"/wt"}"#;
    const STOP: &str = r#"{"ts":120.0,"hook_event_name":"SubagentStop","agent_id":"a1","last_message":"done"}"#;

    #[test]
    fn start_tool_stop_updates_one_agent() {
        let mut registry = Registry::new();
        assert!(registry.ingest_line(START));
        assert!(registry.ingest_line(TOOL));
        let agents = registry.agents();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].status, "running");
        assert_eq!(agents[0].agent_type, "factory-builder");
        assert_eq!(agents[0].last_tool.as_deref(), Some("Edit"));
        assert_eq!(agents[0].last_target.as_deref(), Some("src/App.tsx"));
        assert_eq!(agents[0].tool_count, 1);
        assert!(registry.ingest_line(STOP));
        let agents = registry.agents();
        assert_eq!(agents[0].status, "done");
        assert_eq!(agents[0].ended_ts, Some(120.0));
        assert_eq!(agents[0].last_message.as_deref(), Some("done"));
    }

    #[test]
    fn a_restart_after_stop_runs_again_and_resets_the_start_time() {
        let mut registry = Registry::new();
        registry.ingest_line(START);
        registry.ingest_line(STOP);
        registry.ingest_line(
            r#"{"ts":200.0,"hook_event_name":"SubagentStart","agent_id":"a1","agent_type":"factory-builder"}"#,
        );
        let agents = registry.agents();
        assert_eq!(agents[0].status, "running");
        assert_eq!(agents[0].started_ts, 200.0);
        assert_eq!(agents[0].ended_ts, None);
    }

    #[test]
    fn a_tool_event_without_an_agent_id_is_ignored() {
        let mut registry = Registry::new();
        registry.ingest_line(r#"{"ts":1.0,"hook_event_name":"PostToolUse","tool_name":"Bash"}"#);
        assert!(registry.agents().is_empty());
        assert!(registry.recent().is_empty());
    }

    #[test]
    fn garbage_lines_are_skipped_and_counted() {
        let mut registry = Registry::new();
        assert!(!registry.ingest_line("not json"));
        assert!(!registry.ingest_line(r#"{"hook_event_name":"Stop"}"#));
        assert_eq!(registry.skipped_lines, 2);
        assert!(registry.agents().is_empty());
    }

    #[test]
    fn recent_is_newest_first_and_capped() {
        let mut registry = Registry::new();
        for i in 0..60 {
            registry.ingest_line(&format!(
                r#"{{"ts":{i}.0,"hook_event_name":"PostToolUse","agent_id":"a1","tool_name":"Read"}}"#
            ));
        }
        let recent = registry.recent();
        assert_eq!(recent.len(), 40);
        assert_eq!(recent[0].ts, 59.0);
    }

    #[test]
    fn tailer_holds_a_partial_line_until_its_newline() -> TestResult {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("events.jsonl");
        let mut tailer = Tailer::new(path.clone());
        assert!(tailer.poll().is_empty(), "a missing file gives no lines");
        let mut file = std::fs::File::create(&path)?;
        file.write_all(b"{\"a\":1}\n{\"b\":")?;
        assert_eq!(tailer.poll(), vec![r#"{"a":1}"#.to_string()]);
        file.write_all(b"2}\n")?;
        assert_eq!(tailer.poll(), vec![r#"{"b":2}"#.to_string()]);
        assert!(tailer.poll().is_empty());
        Ok(())
    }

    #[test]
    fn tailer_starts_over_when_the_file_is_truncated() -> TestResult {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("events.jsonl");
        std::fs::write(&path, "{\"a\":1}\n{\"b\":2}\n")?;
        let mut tailer = Tailer::new(path.clone());
        assert_eq!(tailer.poll().len(), 2);
        std::fs::write(&path, "{\"c\":3}\n")?;
        assert_eq!(tailer.poll(), vec![r#"{"c":3}"#.to_string()]);
        Ok(())
    }
}
