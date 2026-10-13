use std::collections::{BTreeMap, VecDeque};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::model::{AgentState, Event, RecentEvent};

const MAX_RECENT: usize = 40;
const REOPEN_GRACE_SECS: f64 = 2.0;
/// Agents with no event for this long are dropped from the map.
const PRUNE_AFTER_SECS: f64 = 86_400.0;
const MAX_AGENTS: usize = 500;
/// At startup only this much of the end of the log is read.
pub const LOG_WINDOW_BYTES: u64 = 4 * 1024 * 1024;

fn last_activity(agent: &AgentState) -> f64 {
    agent
        .ended_ts
        .unwrap_or(0.0)
        .max(agent.last_tool_ts.unwrap_or(0.0))
        .max(agent.started_ts)
}

/// Folds hook events into per-agent state and a short live feed.
#[derive(Debug, Default)]
pub struct Registry {
    agents: BTreeMap<String, AgentState>,
    recent: VecDeque<RecentEvent>,
    pub skipped_lines: u64,
}

impl Registry {
    /// Forgets everything, for a log that was replaced.
    pub fn reset(&mut self) {
        self.agents.clear();
        self.recent.clear();
        self.skipped_lines = 0;
    }

    /// Drops agents idle for a day, then the oldest ones beyond the cap.
    pub fn prune(&mut self, now: f64) {
        self.agents.retain(|_, agent| now - last_activity(agent) <= PRUNE_AFTER_SECS);
        if self.agents.len() <= MAX_AGENTS {
            return;
        }
        let mut order: Vec<(f64, String)> = self
            .agents
            .values()
            .map(|agent| (last_activity(agent), agent.agent_id.clone()))
            .collect();
        order.sort_by(|a, b| a.0.total_cmp(&b.0));
        let excess = self.agents.len() - MAX_AGENTS;
        for (_, id) in order.into_iter().take(excess) {
            self.agents.remove(&id);
        }
    }

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
                job_id: event.job_id.clone(),
                tokens_weighted: 0,
            })
    }

    fn on_start(&mut self, event: &Event) {
        let Some(id) = event.agent_id.as_deref() else {
            return;
        };
        let agent = self.entry(event, id);
        if agent.job_id.is_none() {
            agent.job_id = event.job_id.clone();
        }
        if agent.status == "done" {
            // A Start older than the recorded Stop arrived out of order.
            if agent.ended_ts.is_some_and(|ended| event.ts <= ended) {
                return;
            }
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
        // Claude Code also stops internal helpers that never started and carry no type, such as the
        // progress summarizer. They are not factory agents, so they must not get an entry.
        let untyped = event.agent_type.as_deref().is_none_or(str::is_empty);
        if untyped && !self.agents.contains_key(id) {
            return;
        }
        let agent = self.entry(event, id);
        if agent.job_id.is_none() {
            agent.job_id = event.job_id.clone();
        }
        if let Some(tokens) = &event.tokens {
            agent.tokens_weighted = tokens.weighted;
        }
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
        // A resumed agent may not fire a new start event. Late async lines and the handback
        // call right after the stop must not reopen it.
        let reopen = agent.status == "done"
            && event.tool_name.as_deref() != Some("SubagentHandback")
            && agent
                .ended_ts
                .is_some_and(|ended| event.ts > ended + REOPEN_GRACE_SECS);
        if reopen {
            agent.status = "running".to_string();
            agent.ended_ts = None;
            agent.started_ts = event.ts;
        }
        agent.tool_count += 1;
        agent.last_tool = event.tool_name.clone();
        agent.last_target = event.tool_target.clone();
        agent.last_tool_ts = Some(event.ts);
        if agent.cwd.is_none() {
            agent.cwd = event.cwd.clone();
        }
        if agent.job_id.is_none() {
            agent.job_id = event.job_id.clone();
        }
        if let Some(tokens) = &event.tokens {
            agent.tokens_weighted = tokens.weighted;
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
    /// Device and inode of the file read last, to notice a replaced file.
    ident: Option<(u64, u64)>,
    /// How much of the end of a file the first read may take.
    window: u64,
    /// True until the first read of a file, when the window applies.
    fresh: bool,
    /// True while the cut first line of a windowed read is still to be dropped.
    skip_partial: bool,
    rotated: bool,
}

impl Tailer {
    #[cfg(test)]
    pub fn new(path: PathBuf) -> Self {
        Self::with_window(path, u64::MAX)
    }

    pub fn with_window(path: PathBuf, window: u64) -> Self {
        Self {
            path,
            offset: 0,
            partial: String::new(),
            ident: None,
            window,
            fresh: true,
            skip_partial: false,
            rotated: false,
        }
    }

    /// True once after the file was truncated, replaced or deleted. The first read is not rotation.
    pub fn take_rotated(&mut self) -> bool {
        std::mem::take(&mut self.rotated)
    }

    fn restart(&mut self) {
        self.offset = 0;
        self.partial.clear();
        self.fresh = true;
        self.skip_partial = false;
    }

    /// Returns the complete lines added since the last call. A missing file gives no lines.
    /// A shorter file than before is treated as a new file and read from the start.
    pub fn poll(&mut self) -> Vec<String> {
        let Ok(mut file) = File::open(&self.path) else {
            if self.ident.is_some() {
                self.rotated = true;
            }
            self.restart();
            self.ident = None;
            return Vec::new();
        };
        let Ok(meta) = file.metadata() else {
            return Vec::new();
        };
        let ident = (meta.dev(), meta.ino());
        if self.ident.is_some_and(|old| old != ident) {
            self.rotated = true;
            self.restart();
        }
        self.ident = Some(ident);
        let len = meta.len();
        if len < self.offset {
            self.rotated = true;
            self.restart();
        }
        if self.fresh {
            self.fresh = false;
            if len > self.window {
                self.offset = len - self.window;
                self.skip_partial = true;
            }
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
        if self.skip_partial {
            match self.partial.find('\n') {
                Some(idx) => {
                    self.partial.drain(..=idx);
                    self.skip_partial = false;
                }
                None => {
                    self.partial.clear();
                    return Vec::new();
                }
            }
        }
        let Some(idx) = self.partial.rfind('\n') else {
            return Vec::new();
        };
        let rest = self.partial.split_off(idx + 1);
        let complete = std::mem::replace(&mut self.partial, rest);
        complete
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect()
    }
}

/// One tailer step: read new lines, reset the registry on rotation, ingest, then prune.
pub fn pump(tailer: &mut Tailer, registry: &Mutex<Registry>, now: f64) {
    let lines = tailer.poll();
    let rotated = tailer.take_rotated();
    if let Ok(mut guard) = registry.lock() {
        if rotated {
            guard.reset();
        }
        for line in &lines {
            guard.ingest_line(line);
        }
        guard.prune(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    const START: &str = r#"{"ts":100.0,"hook_event_name":"SubagentStart","agent_id":"a1","agent_type":"factory-builder","cwd":"/wt"}"#;
    const TOOL: &str = r#"{"ts":101.5,"hook_event_name":"PostToolUse","agent_id":"a1","tool_name":"Edit","tool_target":"src/App.tsx","cwd":"/wt"}"#;
    const STOP: &str =
        r#"{"ts":120.0,"hook_event_name":"SubagentStop","agent_id":"a1","last_message":"done"}"#;

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
    fn a_stop_from_an_untyped_agent_that_never_started_is_ignored() {
        let mut registry = Registry::new();
        registry.ingest_line(
            r#"{"ts":100.0,"hook_event_name":"SubagentStop","agent_id":"h1","agent_type":"","last_message":"Reading files"}"#,
        );
        assert!(registry.agents().is_empty());
        assert!(registry.recent().is_empty());
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

    #[test]
    fn tailer_reads_a_replaced_file_that_is_longer_than_the_old_offset() -> TestResult {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("events.jsonl");
        std::fs::write(&path, "{\"a\":1}\n")?;
        let mut tailer = Tailer::new(path.clone());
        assert_eq!(tailer.poll().len(), 1);
        let other = dir.path().join("new.jsonl");
        std::fs::write(&other, "{\"x\":10}\n{\"y\":20}\n{\"z\":30}\n")?;
        std::fs::rename(&other, &path)?;
        assert_eq!(
            tailer.poll(),
            vec![
                r#"{"x":10}"#.to_string(),
                r#"{"y":20}"#.to_string(),
                r#"{"z":30}"#.to_string()
            ]
        );
        Ok(())
    }

    #[test]
    fn tailer_reads_a_file_deleted_and_recreated_longer_than_the_old_offset() -> TestResult {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("events.jsonl");
        std::fs::write(&path, "{\"a\":1}\n")?;
        let mut tailer = Tailer::new(path.clone());
        assert_eq!(tailer.poll().len(), 1);
        std::fs::remove_file(&path)?;
        assert!(tailer.poll().is_empty());
        std::fs::write(&path, "{\"long\":\"line longer than before\"}\n")?;
        assert_eq!(
            tailer.poll(),
            vec![r#"{"long":"line longer than before"}"#.to_string()]
        );
        Ok(())
    }

    #[test]
    fn tailer_reads_a_large_log_in_one_poll() -> TestResult {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("events.jsonl");
        let mut text = String::new();
        for i in 0..200_000 {
            text.push_str(&format!("{{\"n\":{i}}}\n"));
        }
        std::fs::write(&path, text)?;
        let mut tailer = Tailer::new(path);
        assert_eq!(tailer.poll().len(), 200_000);
        Ok(())
    }

    const STOP_AT_120: &str = r#"{"ts":120.0,"hook_event_name":"SubagentStop","agent_id":"a1"}"#;

    fn tool_at(ts: f64, tool: &str) -> String {
        format!(
            r#"{{"ts":{ts},"hook_event_name":"PostToolUse","agent_id":"a1","tool_name":"{tool}"}}"#
        )
    }

    #[test]
    fn a_tool_event_well_after_the_stop_reopens_the_agent() {
        let mut registry = Registry::new();
        registry.ingest_line(START);
        registry.ingest_line(STOP_AT_120);
        registry.ingest_line(&tool_at(200.0, "Edit"));
        let agents = registry.agents();
        assert_eq!(agents[0].status, "running");
        assert_eq!(agents[0].ended_ts, None);
        assert_eq!(agents[0].started_ts, 200.0);
    }

    #[test]
    fn a_tool_event_one_second_after_the_stop_does_not_reopen() {
        let mut registry = Registry::new();
        registry.ingest_line(START);
        registry.ingest_line(STOP_AT_120);
        registry.ingest_line(&tool_at(121.0, "Edit"));
        assert_eq!(registry.agents()[0].status, "done");
    }

    #[test]
    fn a_handback_tool_event_after_the_stop_does_not_reopen() {
        let mut registry = Registry::new();
        registry.ingest_line(START);
        registry.ingest_line(STOP_AT_120);
        registry.ingest_line(&tool_at(300.0, "SubagentHandback"));
        assert_eq!(registry.agents()[0].status, "done");
    }

    #[test]
    fn an_out_of_order_start_does_not_reopen_a_stopped_agent() {
        let mut registry = Registry::new();
        registry.ingest_line(START);
        registry.ingest_line(STOP_AT_120);
        registry.ingest_line(
            r#"{"ts":110.0,"hook_event_name":"SubagentStart","agent_id":"a1","agent_type":"factory-builder"}"#,
        );
        let agents = registry.agents();
        assert_eq!(agents[0].status, "done");
        assert_eq!(agents[0].started_ts, 100.0);
    }

    #[test]
    fn job_id_and_tokens_are_read_and_a_bad_tokens_value_is_dropped() {
        let mut registry = Registry::new();
        registry.ingest_line(r#"{"ts":1.0,"hook_event_name":"SubagentStart","agent_id":"a","agent_type":"t","job_id":"001"}"#);
        registry.ingest_line(r#"{"ts":2.0,"hook_event_name":"PostToolUse","agent_id":"a","tool_name":"Edit","job_id":"001","tokens":{"weighted":30,"raw":300}}"#);
        registry.ingest_line(r#"{"ts":2.5,"hook_event_name":"PostToolUse","agent_id":"a","tool_name":"Edit","tokens":40}"#);
        registry.ingest_line(r#"{"ts":3.0,"hook_event_name":"PostToolUse","agent_id":"a","tool_name":"Edit","tokens":"bad"}"#);
        let agents = registry.agents();
        assert_eq!(agents[0].job_id.as_deref(), Some("001"));
        assert_eq!(agents[0].tokens_weighted, 40);
        assert_eq!(registry.skipped_lines, 0);
    }

    fn numbered_lines(count: usize) -> String {
        (0..count).map(|k| format!("{{\"n\":{}}}\n", 100 + k)).collect()
    }

    #[test]
    fn a_windowed_tailer_reads_only_the_end_and_drops_the_cut_line() -> TestResult {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("events.jsonl");
        std::fs::write(&path, numbered_lines(100))?;
        let mut tailer = Tailer::with_window(path, 55);
        let lines = tailer.poll();
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[0], "{\"n\":195}");
        assert_eq!(lines[4], "{\"n\":199}");
        Ok(())
    }

    #[test]
    fn a_windowed_read_does_not_count_the_cut_line_as_skipped() -> TestResult {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("events.jsonl");
        let mut text = String::new();
        for k in 0..200 {
            text.push_str(&format!(
                "{{\"ts\":{k}.0,\"hook_event_name\":\"SubagentStart\",\"agent_id\":\"a{k}\",\"agent_type\":\"t\"}}\n"
            ));
        }
        std::fs::write(&path, text)?;
        let registry = Mutex::new(Registry::new());
        let mut tailer = Tailer::with_window(path, 500);
        pump(&mut tailer, &registry, 200.0);
        let guard = registry.lock().map_err(|_| "poisoned")?;
        assert_eq!(guard.skipped_lines, 0);
        assert!(guard.agents().len() < 10);
        assert!(guard.agents().iter().any(|a| a.agent_id == "a199"));
        Ok(())
    }

    #[test]
    fn a_log_smaller_than_the_window_is_read_whole() -> TestResult {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("events.jsonl");
        std::fs::write(&path, numbered_lines(10))?;
        let mut tailer = Tailer::with_window(path, 1_000_000);
        assert_eq!(tailer.poll().len(), 10);
        Ok(())
    }

    #[test]
    fn rotation_is_flagged_for_a_truncated_or_deleted_file_but_not_at_startup() -> TestResult {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("events.jsonl");
        let mut tailer = Tailer::new(path.clone());
        tailer.poll();
        assert!(!tailer.take_rotated(), "a missing file at startup is not rotation");
        std::fs::write(&path, numbered_lines(10))?;
        tailer.poll();
        assert!(!tailer.take_rotated(), "the first read is not rotation");
        std::fs::write(&path, numbered_lines(2))?;
        tailer.poll();
        assert!(tailer.take_rotated(), "a shorter file is rotation");
        assert!(!tailer.take_rotated(), "the flag clears once taken");
        std::fs::remove_file(&path)?;
        tailer.poll();
        assert!(tailer.take_rotated(), "a deleted file is rotation");
        Ok(())
    }

    #[test]
    fn pump_resets_the_registry_when_the_log_is_replaced() -> TestResult {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("events.jsonl");
        std::fs::write(&path, format!("{START}\n"))?;
        let registry = Mutex::new(Registry::new());
        let mut tailer = Tailer::new(path.clone());
        pump(&mut tailer, &registry, 101.0);
        assert_eq!(registry.lock().map_err(|_| "poisoned")?.agents().len(), 1);
        std::fs::write(
            &path,
            "{\"ts\":1.0,\"hook_event_name\":\"SubagentStart\",\"agent_id\":\"z\",\"agent_type\":\"t\"}\n",
        )?;
        pump(&mut tailer, &registry, 2.0);
        let guard = registry.lock().map_err(|_| "poisoned")?;
        let ids: Vec<String> = guard.agents().iter().map(|a| a.agent_id.clone()).collect();
        assert_eq!(ids, vec!["z".to_string()]);
        Ok(())
    }

    #[test]
    fn prune_drops_agents_idle_for_a_day() {
        let mut registry = Registry::new();
        registry.ingest_line(START);
        registry.prune(100.0 + 86_000.0);
        assert_eq!(registry.agents().len(), 1);
        registry.prune(100.0 + 86_401.0);
        assert!(registry.agents().is_empty());
    }

    #[test]
    fn prune_caps_the_agent_map_and_drops_the_oldest_first() {
        let mut registry = Registry::new();
        for k in 0..600 {
            registry.ingest_line(&format!(
                "{{\"ts\":{k}.0,\"hook_event_name\":\"SubagentStart\",\"agent_id\":\"a{k}\",\"agent_type\":\"t\"}}"
            ));
        }
        registry.prune(700.0);
        let agents = registry.agents();
        assert_eq!(agents.len(), 500);
        assert!(agents.iter().all(|a| a.agent_id != "a0"));
        assert!(agents.iter().any(|a| a.agent_id == "a599"));
    }
}
