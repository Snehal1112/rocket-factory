use crate::redact::{cap_message, redact_field};
use crate::rotate::RotatingFile;
use crate::time::rfc3339_utc;
use serde_json::{Map, Value};
use std::sync::Arc;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Record};
use tracing::{Event, Id, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

/// Which process writes the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Cli,
    Factoryd,
}

impl Scope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Scope::Cli => "cli",
            Scope::Factoryd => "factoryd",
        }
    }
}

/// Where a line goes. Writing never fails the caller.
pub trait LineSink: Send + Sync {
    fn write_line(&self, line: &str);
}

impl LineSink for RotatingFile {
    fn write_line(&self, line: &str) {
        RotatingFile::write_line(self, line)
    }
}

#[derive(Default)]
struct Fields(Map<String, Value>);

impl Visit for Fields {
    fn record_str(&mut self, f: &Field, v: &str) {
        self.0
            .insert(f.name().to_string(), Value::String(v.to_string()));
    }
    fn record_i64(&mut self, f: &Field, v: i64) {
        self.0.insert(f.name().to_string(), Value::from(v));
    }
    fn record_u64(&mut self, f: &Field, v: u64) {
        self.0.insert(f.name().to_string(), Value::from(v));
    }
    fn record_f64(&mut self, f: &Field, v: f64) {
        self.0.insert(f.name().to_string(), Value::from(v));
    }
    fn record_bool(&mut self, f: &Field, v: bool) {
        self.0.insert(f.name().to_string(), Value::Bool(v));
    }
    fn record_debug(&mut self, f: &Field, v: &dyn std::fmt::Debug) {
        self.0
            .insert(f.name().to_string(), Value::String(format!("{v:?}")));
    }
}

struct SpanFields(Map<String, Value>);

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn take_string(map: &mut Map<String, Value>, key: &str) -> Option<String> {
    map.remove(key).map(|v| match v {
        Value::String(s) => s,
        other => other.to_string(),
    })
}

/// Writes one JSON line per event.
pub struct JsonLayer {
    sink: Arc<dyn LineSink>,
    project: Option<String>,
    scope: Scope,
}

impl JsonLayer {
    pub fn new(sink: Arc<dyn LineSink>, project: Option<String>, scope: Scope) -> JsonLayer {
        JsonLayer {
            sink,
            project,
            scope,
        }
    }
}

impl<S> Layer<S> for JsonLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id) {
            let mut v = Fields::default();
            attrs.record(&mut v);
            span.extensions_mut().insert(SpanFields(v.0));
        }
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id) {
            let mut ext = span.extensions_mut();
            if let Some(SpanFields(map)) = ext.get_mut::<SpanFields>() {
                let mut v = Fields(std::mem::take(map));
                values.record(&mut v);
                *map = v.0;
            }
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut merged = Map::new();
        if let Some(scope) = ctx.event_scope(event) {
            let spans: Vec<_> = scope.collect();
            for span in spans.into_iter().rev() {
                if let Some(SpanFields(map)) = span.extensions().get::<SpanFields>() {
                    for (k, v) in map {
                        merged.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        let mut own = Fields::default();
        event.record(&mut own);
        for (k, v) in own.0 {
            merged.insert(k, v);
        }
        let name = take_string(&mut merged, "event").unwrap_or_else(|| "log".to_string());
        let message = take_string(&mut merged, "message");
        let msg = take_string(&mut merged, "msg")
            .or(message)
            .unwrap_or_default();

        let mut line = Map::new();
        for (k, v) in merged {
            let v = redact_field(&k, v);
            line.insert(k, v);
        }
        line.insert("ts".into(), Value::String(rfc3339_utc(now_ms())));
        line.insert(
            "level".into(),
            Value::String(event.metadata().level().as_str().to_ascii_lowercase()),
        );
        line.insert("scope".into(), Value::String(self.scope.as_str().into()));
        if let Some(p) = &self.project {
            line.insert("project".into(), Value::String(p.clone()));
        }
        line.insert("event".into(), Value::String(name));
        line.insert("msg".into(), Value::String(cap_message(&msg)));
        if let Ok(text) = serde_json::to_string(&Value::Object(line)) {
            self.sink.write_line(&text);
        }
    }
}

/// Prints `<level> <event>: <msg>` to stderr for warn and above,
/// and for info events named `sup.notice`.
pub struct StderrLayer;

impl<S: Subscriber> Layer<S> for StderrLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut f = Fields::default();
        event.record(&mut f);
        let mut map = f.0;
        let name = take_string(&mut map, "event").unwrap_or_else(|| "log".to_string());
        let level = *event.metadata().level();
        let shown = level <= tracing::Level::WARN
            || (level == tracing::Level::INFO && name == "sup.notice");
        if !shown {
            return;
        }
        let message = take_string(&mut map, "message");
        let msg = take_string(&mut map, "msg").or(message).unwrap_or_default();
        eprintln!(
            "{} {}: {}",
            level.as_str().to_ascii_lowercase(),
            name,
            cap_message(&msg)
        );
    }
}
