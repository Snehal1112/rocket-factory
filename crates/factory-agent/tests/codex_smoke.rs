use factory_agent::{AgentRunner, AgentSpec, CodexRunner, EventSink, StopReason};

/// Needs a logged-in `codex` on PATH. Run with `--ignored`.
#[test]
#[ignore]
fn real_codex_smoke() {
    let tmp = tempfile::tempdir().expect("tmp");
    let mut spec = AgentSpec::new("001", "reviewer", tmp.path(), "Reply with the single word OK.");
    spec.model = Some("gpt-5.6-luna".to_string());
    spec.sandbox = Some("read-only".to_string());
    let run = CodexRunner::new("codex", EventSink::null("001")).run(&spec);
    assert_eq!(run.stop_reason, StopReason::Done, "{:?}", run.error);
    assert!(run.session_id.is_some_and(|s| !s.is_empty()));
}
