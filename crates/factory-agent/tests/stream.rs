use factory_agent::{AgentEvent, AgentSpec, Caps, EventKind, EventSink, StopReason, StreamEvent, StreamState, Usage};
use std::path::PathBuf;

fn feed_file(name: &str) -> (StreamState, Vec<StreamEvent>) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    let text = std::fs::read_to_string(path).expect("fixture");
    let mut state = StreamState::default();
    let mut events = Vec::new();
    for line in text.lines() {
        events.extend(state.feed(line));
    }
    (state, events)
}

#[test]
fn stream_dedups_message_ids() {
    let (state, _) = feed_file("ok.jsonl");
    let u = state.usage();
    // msg_1 counts once with its latest values, plus msg_2.
    assert_eq!(u.input_tokens, 15);
    assert_eq!(u.cache_creation_input_tokens, 100);
    assert_eq!(u.cache_read_input_tokens, 2100);
    assert_eq!(u.output_tokens, 60);
    assert_eq!(u.weighted(), 15 + 100 + 60 + 210);
    assert_eq!(state.turns(), 2);
}

#[test]
fn stream_reads_session_result_and_tools() {
    let (state, events) = feed_file("ok.jsonl");
    assert_eq!(state.session_id.as_deref(), Some("sess-1"));
    assert_eq!(state.result_text.as_deref(), Some("All done."));
    assert!(state.has_result());
    assert!(!state.result_is_error);
    let tools: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::Tool { name, target } => Some((name.clone(), target.clone())),
            _ => None,
        })
        .collect();
    // The tool use repeats on streaming lines, but the tool id is reported once.
    assert_eq!(tools, vec![("Read".to_string(), "/w/a.rs".to_string())]);
}

#[test]
fn stream_skips_garbage_lines() {
    let (state, _) = feed_file("garbage.jsonl");
    assert_eq!(state.skipped_lines, 2);
    assert_eq!(state.session_id.as_deref(), Some("sess-g"));
    assert_eq!(state.usage().output_tokens, 2);
    assert!(state.has_result());
}

#[test]
fn stream_reports_error_result() {
    let (state, _) = feed_file("error_result.jsonl");
    assert!(state.result_is_error);
    assert_eq!(state.result_subtype.as_deref(), Some("error_max_turns"));
    // No per-message usage here, so the result usage is the fallback.
    assert_eq!(state.usage().output_tokens, 4);
}

#[test]
fn usage_add_and_max() {
    let a = Usage { input_tokens: 1, cache_creation_input_tokens: 2, cache_read_input_tokens: 3, output_tokens: 9, ..Usage::default() };
    let b = Usage { input_tokens: 4, cache_creation_input_tokens: 1, cache_read_input_tokens: 0, output_tokens: 5, ..Usage::default() };
    assert_eq!(a.add(&b).output_tokens, 14);
    assert_eq!(a.max_with(&b).input_tokens, 4);
    assert_eq!(a.max_with(&b).output_tokens, 9);
    assert!(Usage::default().is_zero());
}

#[test]
fn usage_reads_the_one_hour_share_of_cache_writes() {
    let v: serde_json::Value = serde_json::from_str(
        r#"{"cache_creation_input_tokens":25032,"cache_creation":{"ephemeral_1h_input_tokens":25000,"ephemeral_5m_input_tokens":32}}"#,
    )
    .expect("json");
    let u = Usage::from_value(&v);
    assert_eq!(u.cache_creation_input_tokens, 25032);
    assert_eq!(u.cache_creation_1h_input_tokens, 25000);
    // A usage object without the breakdown counts as zero.
    assert_eq!(Usage::from_value(&serde_json::json!({"output_tokens": 1})).cache_creation_1h_input_tokens, 0);
}

fn caps(agent: bool, max_turns: bool, append: bool) -> Caps {
    Caps { agent, max_turns, append_system_prompt: append }
}

#[test]
fn args_full_spec() {
    let mut spec = AgentSpec::new("001", "builder", "/w", "do it");
    spec.agent = Some("factory-builder".into());
    spec.model = Some("sonnet".into());
    spec.resume = Some("sess-1".into());
    spec.permission_mode = Some("acceptEdits".into());
    spec.allowed_tools = vec!["Read".into(), "Bash".into()];
    spec.max_turns = Some(40);
    let args = spec.args(&caps(true, true, true));
    let joined = args.join(" ");
    assert!(joined.starts_with("-p --output-format stream-json --verbose"));
    for want in ["--model sonnet", "--resume sess-1", "--permission-mode acceptEdits", "--allowedTools Read,Bash", "--agent factory-builder", "--max-turns 40"] {
        assert!(joined.contains(want), "missing {want} in {joined}");
    }
    assert_eq!(spec.stdin_prompt(&caps(true, true, true)), "do it");
}

#[test]
fn args_fall_back_without_agent_or_max_turns() {
    let mut spec = AgentSpec::new("001", "builder", "/w", "do it");
    spec.agent = Some("factory-builder".into());
    spec.agent_prompt = Some("You are the builder.".into());
    spec.max_turns = Some(40);
    let joined = spec.args(&caps(false, false, true)).join(" ");
    assert!(!joined.contains("--agent"));
    assert!(!joined.contains("--max-turns"));
    assert!(joined.contains("--append-system-prompt You are the builder."));
    // With no append support the agent text moves into the user prompt.
    let no_append = caps(false, false, false);
    assert!(!spec.args(&no_append).join(" ").contains("--append-system-prompt"));
    assert_eq!(spec.stdin_prompt(&no_append), "You are the builder.\n\ndo it");
}

#[test]
fn args_omit_empty_options() {
    let spec = AgentSpec::new("001", "reviewer", "/w", "review");
    let joined = spec.args(&caps(true, true, true)).join(" ");
    assert!(!joined.contains("--resume"));
    assert!(!joined.contains("--allowedTools"));
    assert!(!joined.contains("--model"));
    assert_eq!(spec.agent_id(), "001-reviewer");
}

#[test]
fn args_carry_strict_mcp_and_tools_as_single_arguments() {
    let mut spec = AgentSpec::new("001", "committer", "/w", "commit");
    spec.strict_mcp = true;
    spec.tools = Some("Bash,Read,Skill".into());
    let args = spec.args(&caps(true, true, true));
    assert!(args.iter().any(|a| a == "--strict-mcp-config"), "args: {args:?}");
    let at = args.iter().position(|a| a == "--tools").expect("--tools present");
    assert_eq!(args[at + 1], "Bash,Read,Skill");
}

#[test]
fn args_omit_strict_mcp_and_tools_by_default() {
    let spec = AgentSpec::new("001", "builder", "/w", "build");
    let joined = spec.args(&caps(true, true, true)).join(" ");
    assert!(!joined.contains("--strict-mcp-config"));
    assert!(!joined.contains("--tools"));
    assert!(spec.env.is_empty());
}

#[test]
fn args_carry_one_add_dir_flag_per_entry_and_none_by_default() {
    let mut spec = AgentSpec::new("001", "builder", "/w", "build");
    assert!(!spec.args(&caps(true, true, true)).iter().any(|a| a == "--add-dir"));
    spec.add_dirs = vec!["/kb/one".into(), "/kb/two".into()];
    let args = spec.args(&caps(true, true, true));
    let at = args.iter().position(|a| a == "--add-dir").expect("--add-dir present");
    assert_eq!(args[at + 1], "/kb/one");
    assert_eq!(args.iter().filter(|a| *a == "--add-dir").count(), 2);
    assert!(args.iter().any(|a| a == "/kb/two"));
}

fn sample_event() -> AgentEvent {
    AgentEvent {
        kind: EventKind::Tool,
        agent_id: "001-builder".into(),
        agent_type: "factory-builder".into(),
        tool_name: Some("Read".into()),
        tool_target: Some("/w/a.rs".into()),
        cwd: Some("/w".into()),
        last_message: None,
        tokens: 385,
    }
}

#[test]
fn sink_line_has_server_fields_and_new_ones() {
    let sink = EventSink::null("001");
    let v: serde_json::Value = serde_json::from_str(&sink.line(&sample_event(), 12.5)).expect("json");
    assert_eq!(v["hook_event_name"], "PostToolUse");
    assert_eq!(v["agent_id"], "001-builder");
    assert_eq!(v["agent_type"], "factory-builder");
    assert_eq!(v["tool_name"], "Read");
    assert_eq!(v["tool_target"], "/w/a.rs");
    assert_eq!(v["cwd"], "/w");
    assert_eq!(v["ts"], 12.5);
    assert_eq!(v["job_id"], "001");
    assert_eq!(v["tokens"], 385);
    assert!(v.get("last_message").is_none());
}

#[test]
fn sink_trims_long_last_message_on_a_char_boundary() {
    let sink = EventSink::null("001");
    let mut ev = sample_event();
    ev.kind = EventKind::Stop;
    ev.last_message = Some("é".repeat(500));
    let v: serde_json::Value = serde_json::from_str(&sink.line(&ev, 1.0)).expect("json");
    assert_eq!(v["hook_event_name"], "SubagentStop");
    assert_eq!(v["last_message"].as_str().expect("str").chars().count(), 300);
}

#[test]
fn sink_appends_lines_and_creates_parent_dirs() {
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("deep/events.jsonl");
    let sink = EventSink::new(&path, "001");
    sink.emit(&sample_event());
    sink.emit(&sample_event());
    let text = std::fs::read_to_string(&path).expect("read");
    assert_eq!(text.lines().count(), 2);
    assert_eq!(sink.failures(), 0);
}

#[test]
fn sink_survives_write_errors() {
    let dir = tempfile::tempdir().expect("tmp");
    let blocker = dir.path().join("file");
    std::fs::write(&blocker, "x").expect("write");
    // The parent of the log is a regular file, so every write fails.
    let sink = EventSink::new(blocker.join("events.jsonl"), "001");
    sink.emit(&sample_event());
    sink.emit(&sample_event());
    assert_eq!(sink.failures(), 2);
}

#[test]
fn stop_reason_is_comparable() {
    assert_ne!(StopReason::Done, StopReason::Failed);
}

fn feed_lines(lines: &[&str]) -> StreamState {
    let mut state = StreamState::default();
    for line in lines {
        state.feed(line);
    }
    state
}

const MSG_1: &str = r#"{"type":"assistant","message":{"id":"msg_1","role":"assistant","content":[],"usage":{"input_tokens":10,"cache_creation_input_tokens":100,"cache_read_input_tokens":1000,"output_tokens":1}},"session_id":"s"}"#;
const MSG_2: &str = r#"{"type":"assistant","message":{"id":"msg_2","role":"assistant","content":[],"usage":{"input_tokens":5,"cache_creation_input_tokens":0,"cache_read_input_tokens":1100,"output_tokens":2}},"session_id":"s"}"#;

#[test]
fn final_usage_takes_the_larger_result_output() {
    let result = r#"{"type":"result","subtype":"success","is_error":false,"result":"ok","session_id":"s","usage":{"input_tokens":15,"cache_creation_input_tokens":100,"cache_read_input_tokens":2100,"output_tokens":500}}"#;
    let state = feed_lines(&[MSG_1, MSG_2, result]);
    // The live sum keeps the partial snapshots, the final usage uses the result event.
    assert_eq!(state.usage().output_tokens, 3);
    assert_eq!(state.final_usage().output_tokens, 500);
    assert_eq!(state.final_usage().cache_read_input_tokens, 2100);
}

#[test]
fn final_usage_never_drops_below_the_message_sum() {
    let result = r#"{"type":"result","subtype":"success","is_error":false,"result":"ok","session_id":"s","usage":{"input_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":0}}"#;
    let state = feed_lines(&[MSG_1, MSG_2, result]);
    assert_eq!(state.final_usage(), state.usage());
}

#[test]
fn final_usage_without_a_result_event_is_the_message_sum() {
    let state = feed_lines(&[MSG_1, MSG_2]);
    assert!(!state.has_result());
    assert_eq!(state.final_usage(), state.usage());
    assert_eq!(state.final_usage().output_tokens, 3);
}

#[test]
fn final_usage_takes_each_field_independently() {
    // The result is larger in cache read only, the message sum is larger in input.
    let result = r#"{"type":"result","subtype":"success","is_error":false,"result":"ok","session_id":"s","usage":{"input_tokens":1,"cache_creation_input_tokens":100,"cache_read_input_tokens":9000,"output_tokens":3}}"#;
    let state = feed_lines(&[MSG_1, MSG_2, result]);
    let u = state.final_usage();
    assert_eq!(u.input_tokens, 15);
    assert_eq!(u.cache_read_input_tokens, 9000);
}

#[test]
fn start_ctx_is_the_first_messages_cache_creation_plus_read() {
    let state = feed_lines(&[MSG_1, MSG_2]);
    assert_eq!(state.start_ctx(), 100 + 1000);
}

#[test]
fn start_ctx_is_zero_without_messages() {
    assert_eq!(StreamState::default().start_ctx(), 0);
}

#[test]
fn start_ctx_does_not_overflow() {
    let big = format!(
        r#"{{"type":"assistant","message":{{"id":"m","role":"assistant","content":[],"usage":{{"input_tokens":0,"cache_creation_input_tokens":{max},"cache_read_input_tokens":{max},"output_tokens":0}}}}}}"#,
        max = u64::MAX
    );
    let state = feed_lines(&[&big]);
    assert_eq!(state.start_ctx(), u64::MAX);
}

#[test]
fn stop_reason_names_are_stable() {
    use factory_agent::StopReason;
    assert_eq!(StopReason::Done.as_str(), "done");
    assert_eq!(StopReason::BudgetStop.as_str(), "budget-stop");
    assert_eq!(StopReason::Stalled.as_str(), "stalled");
    assert_eq!(StopReason::TurnLimit.as_str(), "turn-limit");
    assert_eq!(StopReason::Failed.as_str(), "failed");
}
