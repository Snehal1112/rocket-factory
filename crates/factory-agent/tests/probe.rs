use factory_agent::{AgentRunner, AgentSpec, Caps, ClaudeRunner, EventSink, StopReason};

fn runner(caps: Caps) -> ClaudeRunner {
    ClaudeRunner::new(
        env!("CARGO_BIN_EXE_fake-claude"),
        caps,
        EventSink::null("001"),
    )
}

fn marker_line(text: &str) -> String {
    text.lines().next().expect("first line").to_string()
}

#[test]
fn fake_claude_probe_reads_the_marker_from_each_delivery_path() {
    let tmp = tempfile::tempdir().expect("tmp");
    let agents = tmp.path().join(".claude/agents");
    std::fs::create_dir_all(&agents).expect("agents dir");
    std::fs::write(
        agents.join("factory-builder.md"),
        "---\nname: factory-builder\n---\nBody.\n\nProbe marker: FPROBE-t1\n",
    )
    .expect("agent file");
    let caps = Caps::default();

    // Agent file: the marker is only in the file, found through `--agent`.
    let mut by_file = AgentSpec::new("001", "builder", tmp.path(), "PROBE\nask");
    by_file.agent = Some("factory-builder".to_string());
    let run = runner(caps).run(&by_file);
    assert_eq!(run.stop_reason, StopReason::Done);
    assert_eq!(marker_line(&run.result_text), "MARKER: FPROBE-t1");
    assert!(run.result_text.contains("TOOLS: Read,Grep,Glob"));
    assert!(run.result_text.contains("BOOTSTRAP: NONE"));
    assert!(run.result_text.contains("SKILLS: NONE"));

    // Append system prompt: the marker rides in the flag value.
    let other = tempfile::tempdir().expect("tmp");
    let mut by_flag = AgentSpec::new("001", "builder", other.path(), "PROBE\nask");
    by_flag.agent_prompt = Some("Body.\n\nProbe marker: FPROBE-t1\n".to_string());
    let run = runner(caps).run(&by_flag);
    assert_eq!(marker_line(&run.result_text), "MARKER: FPROBE-t1");

    // Stdin: the runner prepends the agent text, so PROBE is no longer the first line.
    let no_flags = Caps {
        agent: false,
        max_turns: true,
        append_system_prompt: false,
    };
    let run = runner(no_flags).run(&by_flag);
    assert_eq!(marker_line(&run.result_text), "MARKER: FPROBE-t1");

    // No marker anywhere.
    let bare = AgentSpec::new("001", "builder", other.path(), "PROBE\nask");
    let run = runner(caps).run(&bare);
    assert_eq!(marker_line(&run.result_text), "MARKER: NONE");
}
