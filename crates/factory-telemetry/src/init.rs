use crate::layer::{JsonLayer, LineSink, Scope, StderrLayer};
use crate::rotate::{RotatingFile, LOG_CAP_BYTES};
use crate::TelemetryError;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Layer, Registry};

/// Where and as whom the log is written.
#[derive(Debug, Clone)]
pub struct LogTarget {
    pub path: PathBuf,
    pub project: Option<String>,
    pub scope: Scope,
}

/// Keeps the log file handle; reports a note and the failed write count.
pub struct Guard {
    file: Arc<RotatingFile>,
    note: Option<String>,
}

impl Guard {
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }
    pub fn failures(&self) -> u64 {
        self.file.failures()
    }
}

/// Pick the file filter: a valid `env` value, else `level`.
/// An invalid `env` value is ignored and the reason returned.
pub fn resolve_filter(level: &str, env: Option<&str>) -> (String, Option<String>) {
    let mut note = None;
    if let Some(e) = env.map(str::trim).filter(|e| !e.is_empty()) {
        match validate_directives(e) {
            Ok(()) => return (e.to_string(), None),
            Err(why) => {
                note = Some(format!("FACTORY_LOG value {e:?} ignored: {why}"));
            }
        }
    }
    (level.to_string(), note)
}

/// Accept `level` or `target=level`, comma separated.
fn validate_directives(spec: &str) -> Result<(), String> {
    const LEVELS: [&str; 6] = ["error", "warn", "info", "debug", "trace", "off"];
    for d in spec.split(',') {
        let d = d.trim();
        let (target, lvl) = match d.split_once('=') {
            Some((t, l)) => (Some(t.trim()), l.trim()),
            None => (None, d),
        };
        if !LEVELS.contains(&lvl.to_ascii_lowercase().as_str()) {
            return Err(format!("{lvl:?} is not a level"));
        }
        if let Some(t) = target {
            if t.is_empty() || !t.chars().all(|c| c.is_alphanumeric() || c == '_' || c == ':') {
                return Err(format!("{t:?} is not a target"));
            }
        }
    }
    EnvFilter::try_new(spec).map(|_| ()).map_err(|e| e.to_string())
}

fn filter_for(spec: &str) -> EnvFilter {
    EnvFilter::try_new(spec).unwrap_or_else(|_| EnvFilter::new("info"))
}

/// Install the global subscriber: JSON file layer plus the stderr layer.
pub fn init(target: LogTarget, level: &str, env: Option<&str>) -> Result<Guard, TelemetryError> {
    let (spec, note) = resolve_filter(level, env);
    let file = Arc::new(RotatingFile::new(target.path, LOG_CAP_BYTES));
    let json = JsonLayer::new(file.clone(), target.project, target.scope)
        .with_filter(filter_for(&spec));
    let subscriber = Registry::default().with(json).with(StderrLayer);
    tracing::subscriber::set_global_default(subscriber)
        .map_err(|_| TelemetryError::AlreadyInstalled)?;
    Ok(Guard { file, note })
}

#[derive(Default)]
struct Memory(Mutex<Vec<String>>);

impl LineSink for Memory {
    fn write_line(&self, line: &str) {
        if let Ok(mut v) = self.0.lock() {
            v.push(line.to_string());
        }
    }
}

/// Run `f` under a thread-local JSON layer writing to memory.
pub fn capture<R>(level: &str, f: impl FnOnce() -> R) -> (R, Vec<serde_json::Value>) {
    let mem = Arc::new(Memory::default());
    let layer = JsonLayer::new(mem.clone(), None, Scope::Cli).with_filter(filter_for(level));
    let subscriber = Registry::default().with(layer);
    let out = tracing::subscriber::with_default(subscriber, f);
    let lines = mem.0.lock().map(|v| v.clone()).unwrap_or_default();
    let values = lines
        .iter()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    (out, values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_has_ts_level_scope_event() {
        let (_, lines) = capture("info", || tracing::info!(event = "x.y", msg = "hi"));
        assert_eq!(lines.len(), 1);
        let l = &lines[0];
        assert!(l["ts"].is_string());
        assert_eq!(l["level"], "info");
        assert_eq!(l["scope"], "cli");
        assert_eq!(l["event"], "x.y");
        assert_eq!(l["msg"], "hi");
    }

    #[test]
    fn plain_message_gets_event_log() {
        let (_, lines) = capture("info", || tracing::info!("text"));
        assert_eq!(lines[0]["event"], "log");
        assert_eq!(lines[0]["msg"], "text");
    }

    #[test]
    fn span_fields_reach_the_line() {
        let (_, lines) = capture("info", || {
            tracing::info_span!("job", job_id = "003")
                .in_scope(|| tracing::info!(event = "a", msg = "m"))
        });
        assert_eq!(lines[0]["job_id"], "003");
    }

    #[test]
    fn event_field_beats_span_field() {
        let (_, lines) = capture("info", || {
            tracing::info_span!("job", stage = "build")
                .in_scope(|| tracing::info!(event = "a", stage = "review"))
        });
        assert_eq!(lines[0]["stage"], "review");
    }

    #[test]
    fn level_filter_drops_debug_at_info() {
        let (_, at_info) = capture("info", || tracing::debug!(event = "d"));
        assert!(at_info.is_empty());
        let (_, at_debug) = capture("debug", || tracing::debug!(event = "d"));
        assert_eq!(at_debug.len(), 1);
    }

    #[test]
    fn sensitive_event_field_is_redacted_in_the_line() {
        let (_, lines) = capture("info", || tracing::info!(event = "a", api_token = "abc"));
        assert_eq!(lines[0]["api_token"], "[redacted]");
    }

    #[test]
    fn planted_token_never_reaches_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("log.jsonl");
        let file = Arc::new(RotatingFile::new(&path, LOG_CAP_BYTES));
        let layer = JsonLayer::new(file, None, Scope::Cli);
        let sub = Registry::default().with(layer);
        tracing::subscriber::with_default(sub, || {
            tracing::info!(event = "a", api_token = "sekret-123", msg = "no secrets here");
        });
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(!text.contains("sekret-123"));
        assert!(text.contains("[redacted]"));
    }

    #[test]
    fn invalid_env_filter_falls_back_with_a_note() {
        let (spec, note) = resolve_filter("info", Some("not a filter!!"));
        assert_eq!(spec, "info");
        assert!(note.is_some());
    }

    #[test]
    fn env_filter_beats_log_level() {
        let (spec, note) = resolve_filter("info", Some("debug"));
        assert_eq!(spec, "debug");
        assert!(note.is_none());
    }
}
