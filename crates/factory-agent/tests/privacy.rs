//! Pins what the event sink and the stream parser keep: no url secrets, no tool output, valid lines.

use factory_agent::{AgentEvent, EventKind, EventSink, StreamEvent, StreamState};

fn event(n: usize) -> AgentEvent {
    AgentEvent {
        kind: EventKind::Tool,
        agent_id: format!("agent-{n}"),
        agent_type: "builder".to_string(),
        tool_name: Some("Bash".to_string()),
        tool_target: Some("cargo check -j4".to_string()),
        cwd: None,
        last_message: None,
        tokens: n as u64,
    }
}

#[test]
fn tool_target_drops_url_query_and_fragment() {
    let line = r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"t1","name":"WebFetch","input":{"url":"https://example.com/a/b?token=secret#frag"}}]}}"#;
    let mut state = StreamState::default();
    let events = state.feed(line);
    let target = events.iter().find_map(|e| match e {
        StreamEvent::Tool { target, .. } => Some(target.clone()),
        _ => None,
    });
    assert_eq!(target.as_deref(), Some("https://example.com/a/b"));
    let all = format!("{events:?}");
    assert!(!all.contains("secret") && !all.contains("frag"), "{all}");
}

#[test]
fn concurrent_sinks_write_whole_valid_lines() -> Result<(), String> {
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let path = dir.path().join("events.jsonl");
    let threads = 8;
    let per_thread = 200;
    let mut handles = Vec::new();
    for t in 0..threads {
        let path = path.clone();
        handles.push(std::thread::spawn(move || {
            let sink = EventSink::new(path, "001");
            for i in 0..per_thread {
                sink.emit(&event(t * per_thread + i));
            }
            sink.failures()
        }));
    }
    for h in handles {
        let failures = h.join().map_err(|_| "writer thread panicked".to_string())?;
        assert_eq!(failures, 0);
    }
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut count = 0;
    for line in text.lines() {
        serde_json::from_str::<serde_json::Value>(line).map_err(|e| format!("bad line {line}: {e}"))?;
        count += 1;
    }
    assert_eq!(count, threads * per_thread);
    Ok(())
}

#[test]
fn events_never_carry_tool_output() -> Result<(), String> {
    let sink = EventSink::null("001");
    let line = sink.line(&event(1), 1.0);
    let value: serde_json::Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
    let obj = value.as_object().ok_or("event is not an object")?;
    for key in ["output", "content", "tool_output", "tool_result", "stdout"] {
        assert!(!obj.contains_key(key), "event has a {key} field");
    }
    // A tool_result line from the stream makes no event and its text goes nowhere.
    let result_line = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"TOP-SECRET-OUTPUT"}]}}"#;
    let mut state = StreamState::default();
    let events = state.feed(result_line);
    assert!(events.is_empty(), "{events:?}");
    assert!(!format!("{state:?}").contains("TOP-SECRET-OUTPUT"));
    Ok(())
}
