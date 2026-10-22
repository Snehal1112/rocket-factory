//! Codex CLI support: the `codex exec --json` event stream.

use std::collections::BTreeSet;
use std::ffi::OsString;

use serde_json::Value;

use crate::child::{drive, AgentStream, ChildConfig};
use crate::claude::{AgentError, ALLOW_API_KEY_VAR};
use crate::events::EventSink;
use crate::run::AgentRun;
use crate::runner::AgentRunner;
use crate::spec::AgentSpec;
use crate::stream::{trim_chars, StreamEvent, MAX_COMMAND};
use crate::usage::Usage;

/// Variables that make `codex` bill an API account instead of the login.
const CODEX_API_VARS: [&str; 2] = ["OPENAI_API_KEY", "CODEX_API_KEY"];

/// Builds the `codex exec` argument list. The prompt goes to stdin, marked by the last `-`.
///
/// Approval flags: `approval_policy="on-request"` with `approvals_reviewer="auto_review"`
/// routes approval requests to automatic review, so a headless run never waits on a person.
/// Probed with codex-cli 0.163.0 (read-only sandbox, `curl -sI https://example.com`): exit 0
/// in 12s, no hang, the command was declined with a message the model saw.
pub fn codex_args(spec: &AgentSpec) -> Vec<String> {
    let mut a: Vec<String> = vec!["exec".to_string()];
    if spec.resume.is_some() {
        a.push("resume".to_string());
    }
    for flag in ["--json", "--ignore-user-config", "--skip-git-repo-check"] {
        a.push(flag.to_string());
    }
    if let Some(m) = &spec.model {
        a.push("-m".to_string());
        a.push(m.clone());
    }
    let mut config = |value: String| {
        a.push("-c".to_string());
        a.push(value);
    };
    config("approval_policy=\"on-request\"".to_string());
    config("approvals_reviewer=\"auto_review\"".to_string());
    let mode = spec.sandbox.as_deref().unwrap_or("read-only");
    config(format!("sandbox_mode=\"{mode}\""));
    if !spec.writable_roots.is_empty() {
        let roots: Vec<String> = spec
            .writable_roots
            .iter()
            .map(|r| format!("\"{}\"", r.display()))
            .collect();
        config(format!(
            "sandbox_workspace_write.writable_roots=[{}]",
            roots.join(",")
        ));
    }
    if spec.network {
        config("sandbox_workspace_write.network_access=true".to_string());
    }
    if let Some(id) = &spec.resume {
        a.push(id.clone());
    }
    a.push("-".to_string());
    a
}

/// Runs headless `codex exec --json` and reads its events.
#[derive(Debug)]
pub struct CodexRunner {
    program: OsString,
    sink: EventSink,
    allow_api_key: bool,
}

impl CodexRunner {
    pub fn new(program: impl Into<OsString>, sink: EventSink) -> CodexRunner {
        CodexRunner {
            program: program.into(),
            sink,
            allow_api_key: std::env::var(ALLOW_API_KEY_VAR).is_ok_and(|v| v == "1"),
        }
    }

    /// Overrides the `FACTORY_ALLOW_API_KEY` setting. Meant for tests.
    #[must_use]
    pub fn with_api_key_allowed(mut self, allowed: bool) -> CodexRunner {
        self.allow_api_key = allowed;
        self
    }

    fn run_inner(&self, spec: &AgentSpec) -> Result<AgentRun, AgentError> {
        let stdin = match &spec.agent_prompt {
            Some(text) => format!("{text}\n\n{}", spec.prompt),
            None => spec.prompt.clone(),
        };
        let cfg = ChildConfig {
            program: self.program.clone(),
            args: codex_args(spec),
            stdin,
            label: "codex".to_string(),
            remove_env: if self.allow_api_key {
                Vec::new()
            } else {
                CODEX_API_VARS.to_vec()
            },
            honor_max_turns: false,
            fail_on_exit_after_result: true,
            mid_run_budget: false,
        };
        drive(&cfg, spec, &self.sink, CodexStream::default())
    }
}

impl AgentRunner for CodexRunner {
    fn run(&self, spec: &AgentSpec) -> AgentRun {
        match self.run_inner(spec) {
            Ok(run) => run,
            Err(e) => AgentRun::failed(&e.to_string()),
        }
    }
}

/// Maps the usage of a `turn.completed` line to the factory usage fields.
pub fn codex_usage(turn_usage: &Value) -> Usage {
    let get = |k: &str| turn_usage.get(k).and_then(Value::as_u64).unwrap_or(0);
    let input = get("input_tokens");
    let cached = get("cached_input_tokens");
    Usage {
        input_tokens: input.saturating_sub(cached),
        cache_read_input_tokens: cached,
        cache_creation_input_tokens: get("cache_write_input_tokens"),
        output_tokens: get("output_tokens"),
        cache_creation_1h_input_tokens: 0,
    }
}

/// Folds `codex exec --json` lines into run state.
#[derive(Debug, Default)]
pub struct CodexStream {
    session_id: Option<String>,
    result_text: Option<String>,
    result_is_error: bool,
    result_subtype: Option<String>,
    last_message: Option<String>,
    messages: u32,
    total: Usage,
    seen_tools: BTreeSet<String>,
    has_result: bool,
    last_error: Option<String>,
    pub skipped_lines: u64,
}

fn text_of(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .map_or(String::new(), str::to_string)
}

impl CodexStream {
    fn on_item_started(&mut self, item: &Value, out: &mut Vec<StreamEvent>) {
        if item.get("type").and_then(Value::as_str) != Some("command_execution") {
            return;
        }
        let id = text_of(item, "id");
        if !id.is_empty() && !self.seen_tools.insert(id) {
            return;
        }
        out.push(StreamEvent::Tool {
            name: "Bash".to_string(),
            target: trim_chars(&text_of(item, "command"), MAX_COMMAND),
        });
    }

    fn on_item_completed(&mut self, item: &Value) {
        if item.get("type").and_then(Value::as_str) == Some("agent_message") {
            self.last_message = Some(text_of(item, "text"));
            self.messages = self.messages.saturating_add(1);
        }
    }

    fn fail(&mut self, message: String, out: &mut Vec<StreamEvent>) {
        self.result_text = Some(message.clone());
        self.result_is_error = true;
        self.result_subtype = Some("turn_failed".to_string());
        self.has_result = true;
        out.push(StreamEvent::Result {
            text: message,
            is_error: true,
            subtype: "turn_failed".to_string(),
        });
    }
}

impl AgentStream for CodexStream {
    fn feed(&mut self, line: &str) -> Vec<StreamEvent> {
        let line = line.trim();
        if line.is_empty() {
            return Vec::new();
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            self.skipped_lines += 1;
            return Vec::new();
        };
        let mut out = Vec::new();
        match value.get("type").and_then(Value::as_str).unwrap_or("") {
            "thread.started" => {
                let id = text_of(&value, "thread_id");
                if !id.is_empty() {
                    self.session_id = Some(id.clone());
                    out.push(StreamEvent::Init { session_id: id });
                }
            }
            "item.started" => {
                if let Some(item) = value.get("item") {
                    self.on_item_started(item, &mut out);
                }
            }
            "item.completed" => {
                if let Some(item) = value.get("item") {
                    self.on_item_completed(item);
                }
            }
            "turn.completed" => {
                if let Some(u) = value.get("usage") {
                    self.total = self.total.add(&codex_usage(u));
                }
                if self.result_is_error {
                    return Vec::new();
                }
                let text = self.last_message.clone().unwrap_or_default();
                self.result_text = Some(text.clone());
                self.result_is_error = false;
                self.result_subtype = Some("success".to_string());
                self.has_result = true;
                out.push(StreamEvent::Result {
                    text,
                    is_error: false,
                    subtype: "success".to_string(),
                });
            }
            "turn.failed" => {
                let msg = value
                    .get("error")
                    .map_or(String::new(), |e| text_of(e, "message"));
                self.fail(msg, &mut out);
            }
            "error" => {
                // Non-fatal (retries, notices); only turn.failed ends a run in failure.
                self.last_error = Some(text_of(&value, "message"));
            }
            _ => {}
        }
        out
    }

    fn usage(&self) -> Usage {
        self.total
    }
    fn final_usage(&self) -> Usage {
        self.total
    }
    fn turns(&self) -> u32 {
        self.messages
    }
    fn diagnostic(&self) -> Option<String> {
        self.last_error.clone()
    }
    fn has_result(&self) -> bool {
        self.has_result
    }
    fn result_text(&self) -> Option<String> {
        self.result_text.clone()
    }
    fn result_is_error(&self) -> bool {
        self.result_is_error
    }
    fn result_subtype(&self) -> Option<String> {
        self.result_subtype.clone()
    }
    fn session_id(&self) -> Option<String> {
        self.session_id.clone()
    }
    fn start_ctx(&self) -> u64 {
        0
    }
    fn end_ctx(&self) -> u64 {
        0
    }
    fn start_cache(&self) -> (u64, u64) {
        (0, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::AgentSpec;
    use crate::stream::StreamEvent;

    fn args_spec(sandbox: &str) -> AgentSpec {
        let mut s = AgentSpec::new("001", "builder", "/w", "p");
        s.sandbox = Some(sandbox.to_string());
        s
    }

    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn fresh_args_have_exec_json_ignore_user_config_model_sandbox_and_stdin_dash() {
        let mut s = args_spec("workspace-write");
        s.model = Some("gpt-5.6-luna".to_string());
        assert_eq!(
            codex_args(&s),
            strs(&[
                "exec",
                "--json",
                "--ignore-user-config",
                "--skip-git-repo-check",
                "-m",
                "gpt-5.6-luna",
                "-c",
                "approval_policy=\"on-request\"",
                "-c",
                "approvals_reviewer=\"auto_review\"",
                "-c",
                "sandbox_mode=\"workspace-write\"",
                "-"
            ])
        );
    }

    #[test]
    fn codex_args_set_the_approval_policy() {
        for sandbox in ["workspace-write", "read-only"] {
            let a = codex_args(&args_spec(sandbox));
            for pair in [
                "approval_policy=\"on-request\"",
                "approvals_reviewer=\"auto_review\"",
            ] {
                let i = a.iter().position(|x| x == pair).expect(pair);
                assert_eq!(a[i - 1], "-c");
            }
        }
    }

    #[test]
    fn resume_args_use_exec_resume_and_the_thread_id() {
        let mut s = args_spec("read-only");
        s.resume = Some("abc".to_string());
        let a = codex_args(&s);
        assert_eq!(
            &a[..4],
            &strs(&["exec", "resume", "--json", "--ignore-user-config"])[..]
        );
        assert_eq!(a[a.len() - 2], "abc");
        assert!(a.contains(&"approvals_reviewer=\"auto_review\"".to_string()));
        assert_eq!(a.last().map(String::as_str), Some("-"));
    }

    #[test]
    fn reviewer_args_are_read_only() {
        let a = codex_args(&args_spec("read-only"));
        assert!(a.contains(&"sandbox_mode=\"read-only\"".to_string()));
        assert!(!a.iter().any(|x| x.contains("network_access")));
    }

    #[test]
    fn committer_args_add_the_git_dir_root() {
        let mut s = args_spec("workspace-write");
        s.writable_roots = vec!["/repo/.git".into()];
        let a = codex_args(&s);
        assert!(a.contains(&"sandbox_workspace_write.writable_roots=[\"/repo/.git\"]".to_string()));
    }

    #[test]
    fn builder_args_turn_network_on() {
        let mut s = args_spec("workspace-write");
        s.network = true;
        let a = codex_args(&s);
        assert!(a.contains(&"sandbox_workspace_write.network_access=true".to_string()));
    }

    #[test]
    fn unset_sandbox_means_read_only() {
        let s = AgentSpec::new("001", "builder", "/w", "p");
        assert!(codex_args(&s).contains(&"sandbox_mode=\"read-only\"".to_string()));
    }

    const SESSION: &str = include_str!("../tests/fixtures/codex-session.jsonl");
    const FAILED: &str = include_str!("../tests/fixtures/codex-failed.jsonl");

    fn feed_all(s: &mut CodexStream, text: &str) -> Vec<StreamEvent> {
        text.lines().flat_map(|l| s.feed(l)).collect()
    }

    #[test]
    fn codex_usage_splits_cached_input_and_keeps_cache_write() {
        let v = serde_json::json!({"input_tokens":43851,"cached_input_tokens":38656,
            "cache_write_input_tokens":0,"output_tokens":443,"reasoning_output_tokens":199});
        let u = codex_usage(&v);
        assert_eq!((u.input_tokens, u.cache_read_input_tokens), (5195, 38656));
        assert_eq!((u.cache_creation_input_tokens, u.output_tokens), (0, 443));
        assert_eq!(u.weighted(), 9503);
    }

    #[test]
    fn codex_stream_reads_thread_id_tools_and_usage() {
        let mut s = CodexStream::default();
        let events = feed_all(&mut s, SESSION);
        let inits: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, StreamEvent::Init { .. }))
            .collect();
        assert_eq!(
            inits,
            vec![&StreamEvent::Init {
                session_id: "01a15eac-9d1e-73f0-98dd-ce2929f333e6".to_string()
            }]
        );
        let tools: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                StreamEvent::Tool { name, target } => Some((name.clone(), target.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].0, "Bash");
        assert!(tools[0].1.contains("echo hi"));
        assert_eq!(s.result_text(), Some("OK".to_string()));
        assert!(s.has_result());
        assert_eq!(s.final_usage().weighted(), 9503);
        assert_eq!(
            s.session_id().as_deref(),
            Some("01a15eac-9d1e-73f0-98dd-ce2929f333e6")
        );
    }

    #[test]
    fn tool_target_is_cut_at_80_characters() {
        let mut s = CodexStream::default();
        let cmd = "x".repeat(200);
        let line = serde_json::json!({"type":"item.started","item":{"id":"i1",
            "type":"command_execution","command":cmd}})
        .to_string();
        let events = s.feed(&line);
        match &events[0] {
            StreamEvent::Tool { target, .. } => assert_eq!(target.chars().count(), 80),
            other => panic!("unexpected event {other:?}"),
        }
    }

    #[test]
    fn cached_above_input_does_not_underflow() {
        let v = serde_json::json!({"input_tokens":10,"cached_input_tokens":50,
            "cache_write_input_tokens":0,"output_tokens":1});
        let u = codex_usage(&v);
        assert_eq!(u.input_tokens, 0);
        assert_eq!(u.cache_read_input_tokens, 50);
    }

    #[test]
    fn config_notice_errors_are_ignored() {
        let mut s = CodexStream::default();
        let line = FAILED.lines().next().expect("first line");
        assert!(s.feed(line).is_empty());
        assert!(!s.has_result());
        assert!(!s.result_is_error());
    }

    #[test]
    fn codex_stream_ignores_reasoning_and_file_change_items() {
        let mut s = CodexStream::default();
        for ty in ["reasoning", "file_change"] {
            let line =
                serde_json::json!({"type":"item.completed","item":{"id":"a","type":ty,"text":"t"}})
                    .to_string();
            assert!(s.feed(&line).is_empty());
        }
    }

    #[test]
    fn codex_turns_count_agent_messages() {
        let mut s = CodexStream::default();
        for (id, text) in [("a", "one"), ("b", "two")] {
            let line = serde_json::json!({"type":"item.completed","item":{"id":id,
                "type":"agent_message","text":text}})
            .to_string();
            s.feed(&line);
        }
        assert_eq!(s.turns(), 2);
    }

    #[test]
    fn turn_failed_marks_the_result_as_an_error() {
        let mut s = CodexStream::default();
        feed_all(&mut s, FAILED);
        assert!(s.has_result());
        assert!(s.result_is_error());
        assert_eq!(s.result_subtype().as_deref(), Some("turn_failed"));
        assert!(s.result_text().expect("text").contains("unsupported model"));
    }

    #[test]
    fn turn_failed_then_completed_stays_failed() {
        let mut s = CodexStream::default();
        s.feed(r#"{"type":"turn.failed","error":{"message":"turn rejected"}}"#);
        s.feed(r#"{"type":"turn.completed","usage":{"input_tokens":2,"output_tokens":3}}"#);
        assert!(s.result_is_error());
        assert_eq!(s.result_subtype().as_deref(), Some("turn_failed"));
        assert!(s.result_text().unwrap().contains("turn rejected"));
        assert_eq!(s.final_usage().input_tokens, 2);
    }

    #[test]
    fn error_event_then_completed_is_a_success() {
        let mut s = CodexStream::default();
        s.feed(r#"{"type":"error","message":"Reconnecting... 2/5"}"#);
        s.feed(r#"{"type":"turn.completed","usage":{"input_tokens":2,"output_tokens":3}}"#);
        assert!(!s.result_is_error());
        assert_eq!(s.result_subtype().as_deref(), Some("success"));
    }

    #[test]
    fn error_event_then_turn_failed_is_an_error_with_the_failure_message() {
        let mut s = CodexStream::default();
        s.feed(r#"{"type":"error","message":"Reconnecting... 2/5"}"#);
        s.feed(r#"{"type":"turn.failed","error":{"message":"turn rejected"}}"#);
        assert!(s.has_result());
        assert!(s.result_is_error());
        assert!(s.result_text().unwrap().contains("turn rejected"));
    }

    #[test]
    fn error_event_alone_is_not_a_result() {
        let mut s = CodexStream::default();
        s.feed(r#"{"type":"error","message":"provider unavailable"}"#);
        assert!(!s.has_result());
    }

    #[test]
    fn non_json_lines_are_skipped() {
        let mut s = CodexStream::default();
        assert!(s.feed("garbage").is_empty());
        assert!(!s.has_result());
    }

    #[test]
    fn usage_is_added_only_after_turn_completed() {
        let mut s = CodexStream::default();
        for line in SESSION.lines().take(4) {
            s.feed(line);
        }
        assert!(s.usage().is_zero());
    }
}
