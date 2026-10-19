use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::usage::Usage;

const MAX_COMMAND: usize = 80;
const MAX_TARGET: usize = 300;

/// Something the runner reacts to while reading the stream.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    Init {
        session_id: String,
    },
    Tool {
        name: String,
        target: String,
    },
    Result {
        text: String,
        is_error: bool,
        subtype: String,
    },
}

/// Folds `claude -p --output-format stream-json` lines into run state.
#[derive(Debug, Default)]
pub struct StreamState {
    pub session_id: Option<String>,
    pub result_text: Option<String>,
    pub result_is_error: bool,
    pub result_subtype: Option<String>,
    pub skipped_lines: u64,
    usage_by_message: BTreeMap<String, Usage>,
    seen_tools: BTreeSet<String>,
    result_usage: Option<Usage>,
    first_usage: Option<Usage>,
}

fn trim_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// Picks the same target the v1 hook picked: a path, pattern, url or a short command.
fn tool_target(input: &Value) -> String {
    for key in ["file_path", "path", "pattern", "url"] {
        if let Some(v) = input.get(key).and_then(Value::as_str) {
            let v = if key == "url" {
                v.split(['?', '#']).next().map_or(v, |p| p)
            } else {
                v
            };
            return trim_chars(v, MAX_TARGET);
        }
    }
    input
        .get("command")
        .and_then(Value::as_str)
        .map_or(String::new(), |c| trim_chars(c, MAX_COMMAND))
}

impl StreamState {
    /// Feeds one stdout line. Blank and unparsable lines are skipped and counted.
    pub fn feed(&mut self, line: &str) -> Vec<StreamEvent> {
        let line = line.trim();
        if line.is_empty() {
            return Vec::new();
        }
        let value: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => {
                self.skipped_lines += 1;
                return Vec::new();
            }
        };
        let mut out = Vec::new();
        let kind = value.get("type").and_then(Value::as_str).map_or("", |t| t);
        // The latest session id wins, because a resumed run may get a new one.
        if let Some(id) = value.get("session_id").and_then(Value::as_str) {
            self.session_id = Some(id.to_string());
            if kind == "system" && value.get("subtype").and_then(Value::as_str) == Some("init") {
                out.push(StreamEvent::Init {
                    session_id: id.to_string(),
                });
            }
        }
        match kind {
            "assistant" => self.on_assistant(&value, &mut out),
            "result" => self.on_result(&value, &mut out),
            _ => {}
        }
        out
    }

    fn on_assistant(&mut self, value: &Value, out: &mut Vec<StreamEvent>) {
        let Some(message) = value.get("message") else {
            return;
        };
        if let Some(id) = message.get("id").and_then(Value::as_str) {
            let seen = message
                .get("usage")
                .map_or(Usage::default(), Usage::from_value);
            let merged = match self.usage_by_message.get(id) {
                Some(old) => old.max_with(&seen),
                None => seen,
            };
            if self.first_usage.is_none() {
                self.first_usage = Some(seen);
            }
            self.usage_by_message.insert(id.to_string(), merged);
        }
        let Some(items) = message.get("content").and_then(Value::as_array) else {
            return;
        };
        for item in items {
            if item.get("type").and_then(Value::as_str) != Some("tool_use") {
                continue;
            }
            let tool_id = item
                .get("id")
                .and_then(Value::as_str)
                .map_or(String::new(), str::to_string);
            if !tool_id.is_empty() && !self.seen_tools.insert(tool_id) {
                continue;
            }
            let name = item
                .get("name")
                .and_then(Value::as_str)
                .map_or(String::new(), str::to_string);
            let target = item.get("input").map_or(String::new(), tool_target);
            out.push(StreamEvent::Tool { name, target });
        }
    }

    fn on_result(&mut self, value: &Value, out: &mut Vec<StreamEvent>) {
        let text = value
            .get("result")
            .and_then(Value::as_str)
            .map_or(String::new(), str::to_string);
        let is_error = value
            .get("is_error")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let subtype = value
            .get("subtype")
            .and_then(Value::as_str)
            .map_or(String::new(), str::to_string);
        self.result_usage = value.get("usage").map(Usage::from_value);
        self.result_text = Some(text.clone());
        self.result_is_error = is_error;
        self.result_subtype = Some(subtype.clone());
        out.push(StreamEvent::Result {
            text,
            is_error,
            subtype,
        });
    }

    /// Sum of the latest usage of each unique message id. Falls back to the result usage when
    /// no per-message usage was seen.
    pub fn usage(&self) -> Usage {
        let sum = self
            .usage_by_message
            .values()
            .fold(Usage::default(), |acc, u| acc.add(u));
        if sum.is_zero() {
            return self.result_usage.unwrap_or(sum);
        }
        sum
    }

    /// Usage of a finished run: per field, the larger of the message sum and the result event.
    /// The message snapshots carry partial output counts, the result event carries the run total.
    pub fn final_usage(&self) -> Usage {
        let sum = self.usage();
        match self.result_usage {
            Some(result) => sum.max_with(&result),
            None => sum,
        }
    }

    /// Context the run started with: cache creation plus cache read of the first message.
    pub fn start_ctx(&self) -> u64 {
        self.first_usage.map_or(0, |u| {
            u.cache_creation_input_tokens
                .saturating_add(u.cache_read_input_tokens)
        })
    }

    /// Number of unique assistant messages seen so far.
    pub fn turns(&self) -> u32 {
        u32::try_from(self.usage_by_message.len()).unwrap_or(u32::MAX)
    }

    pub fn has_result(&self) -> bool {
        self.result_text.is_some()
    }
}
