use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Map, Value};

const MAX_MESSAGE: usize = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Start,
    Tool,
    Stop,
    BudgetWarn,
    /// A driver note, such as a fallback the driver took. Not an agent action.
    Notice,
    /// The session id of a run, reported while the run goes on.
    Session,
}

impl EventKind {
    /// The `hook_event_name` the dashboard server understands.
    pub fn hook_name(self) -> &'static str {
        match self {
            EventKind::Start => "SubagentStart",
            EventKind::Tool => "PostToolUse",
            EventKind::Stop => "SubagentStop",
            EventKind::BudgetWarn => "BudgetWarn",
            EventKind::Notice => "FactoryNotice",
            EventKind::Session => "FactorySession",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgentEvent {
    pub kind: EventKind,
    pub agent_id: String,
    pub agent_type: String,
    pub tool_name: Option<String>,
    pub tool_target: Option<String>,
    pub cwd: Option<String>,
    pub last_message: Option<String>,
    /// Weighted tokens of the job so far.
    pub tokens: u64,
}

/// Appends events to `events.jsonl`. A write error is counted and never returned.
#[derive(Debug)]
pub struct EventSink {
    path: Option<PathBuf>,
    job_id: String,
    failures: AtomicU64,
}

impl EventSink {
    pub fn new(path: impl Into<PathBuf>, job_id: &str) -> EventSink {
        EventSink {
            path: Some(path.into()),
            job_id: job_id.to_string(),
            failures: AtomicU64::new(0),
        }
    }

    /// A sink that writes nothing. Used in tests.
    pub fn null(job_id: &str) -> EventSink {
        EventSink {
            path: None,
            job_id: job_id.to_string(),
            failures: AtomicU64::new(0),
        }
    }

    pub fn failures(&self) -> u64 {
        self.failures.load(Ordering::Relaxed)
    }

    /// Renders one JSON line without the trailing newline.
    pub fn line(&self, ev: &AgentEvent, ts: f64) -> String {
        let mut m = Map::new();
        m.insert("ts".into(), json!(ts));
        m.insert("hook_event_name".into(), json!(ev.kind.hook_name()));
        m.insert("agent_id".into(), json!(ev.agent_id));
        m.insert("agent_type".into(), json!(ev.agent_type));
        if let Some(v) = &ev.tool_name {
            m.insert("tool_name".into(), json!(v));
        }
        if let Some(v) = &ev.tool_target {
            m.insert("tool_target".into(), json!(v));
        }
        if let Some(v) = &ev.cwd {
            m.insert("cwd".into(), json!(v));
        }
        if let Some(v) = &ev.last_message {
            let skip = v.chars().count().saturating_sub(MAX_MESSAGE);
            let tail: String = v.chars().skip(skip).collect();
            m.insert("last_message".into(), json!(tail));
        }
        m.insert("job_id".into(), json!(self.job_id));
        m.insert("tokens".into(), json!(ev.tokens));
        Value::Object(m).to_string()
    }

    /// Appends one event. Never fails the caller.
    pub fn emit(&self, ev: &AgentEvent) {
        let Some(path) = &self.path else {
            return;
        };
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64());
        let mut line = self.line(ev, ts);
        line.push('\n');
        let result = (|| -> std::io::Result<()> {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut f = OpenOptions::new().create(true).append(true).open(path)?;
            // One write call keeps concurrent appenders from interleaving a line.
            f.write_all(line.as_bytes())
        })();
        if result.is_err() {
            self.failures.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_kind_is_the_factory_session_hook() {
        assert_eq!(EventKind::Session.hook_name(), "FactorySession");
        for other in [
            EventKind::Start,
            EventKind::Tool,
            EventKind::Stop,
            EventKind::BudgetWarn,
            EventKind::Notice,
        ] {
            assert_ne!(other.hook_name(), "FactorySession");
        }
    }

    #[test]
    fn a_session_line_carries_the_id_as_last_message() {
        let ev = AgentEvent {
            kind: EventKind::Session,
            agent_id: "001-builder".into(),
            agent_type: "factory-builder".into(),
            tool_name: None,
            tool_target: None,
            cwd: None,
            last_message: Some("sess-1".into()),
            tokens: 0,
        };
        let line = EventSink::null("001").line(&ev, 1.0);
        let v: Value = serde_json::from_str(&line).expect("json");
        assert_eq!(v["hook_event_name"], "FactorySession");
        assert_eq!(v["last_message"], "sess-1");
        assert_eq!(v["job_id"], "001");
        assert!(v.get("tool_name").is_none());
    }
}
