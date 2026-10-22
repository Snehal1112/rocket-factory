use std::sync::{Arc, Mutex};
use std::time::Duration;

use factory_agent::{AgentRunner, AgentSpec, CodexRunner, EventSink, SessionHook, StopReason};

fn runner(sink: EventSink) -> CodexRunner {
    CodexRunner::new(env!("CARGO_BIN_EXE_fake-codex"), sink).with_api_key_allowed(false)
}

fn spec(mode: &str, dir: &std::path::Path) -> AgentSpec {
    AgentSpec::new("001", "builder", dir, mode)
}

#[test]
fn ok_run_collects_session_usage_and_result() {
    let tmp = tempfile::tempdir().expect("tmp");
    let run = runner(EventSink::null("001")).run(&spec("ok", tmp.path()));
    assert_eq!(run.stop_reason, StopReason::Done);
    assert_eq!(
        run.session_id.as_deref(),
        Some("01a15eac-9d1e-73f0-98dd-ce2929f333e6")
    );
    assert_eq!(run.result_text, "OK");
    assert_eq!(run.weighted, 9503);
}

#[test]
fn completed_turn_with_nonzero_exit_is_failed_and_keeps_diagnostics() {
    let tmp = tempfile::tempdir().expect("tmp");
    let run = runner(EventSink::null("001")).run(&spec("completed-nonzero", tmp.path()));
    assert_eq!(run.stop_reason, StopReason::Failed);
    assert_eq!(run.result_text, "OK");
    assert_eq!(run.exit_code, Some(7));
    let err = run.error.expect("error");
    assert!(err.contains("exit Some(7)"), "{err}");
    assert!(
        err.contains("codex transport failed after completion"),
        "{err}"
    );
}

#[test]
fn turn_failed_ends_failed_with_the_message() {
    let tmp = tempfile::tempdir().expect("tmp");
    let run = runner(EventSink::null("001")).run(&spec("failed", tmp.path()));
    assert_eq!(run.stop_reason, StopReason::Failed);
    let err = run.error.expect("error text");
    assert!(err.contains("invalid_request_error"), "{err}");
}

#[test]
fn no_turn_completed_is_failed() {
    let tmp = tempfile::tempdir().expect("tmp");
    let run = runner(EventSink::null("001")).run(&spec("noresult", tmp.path()));
    assert_eq!(run.stop_reason, StopReason::Failed);
    let err = run.error.expect("error text");
    assert!(err.contains("codex exited without a result"), "{err}");
}

#[test]
fn spawn_error_names_the_program() {
    let tmp = tempfile::tempdir().expect("tmp");
    let run =
        CodexRunner::new("no-such-codex", EventSink::null("001")).run(&spec("ok", tmp.path()));
    assert_eq!(run.stop_reason, StopReason::Failed);
    let err = run.error.expect("error text");
    assert!(err.contains("cannot start no-such-codex"), "{err}");
}

#[test]
fn codex_stall_kills_the_child() {
    let tmp = tempfile::tempdir().expect("tmp");
    let mut s = spec("silent", tmp.path());
    s.stall_after = Some(Duration::from_secs(1));
    let run = runner(EventSink::null("001")).run(&s);
    assert_eq!(run.stop_reason, StopReason::Stalled);
}

#[test]
fn no_mid_run_budget_stop_on_codex() {
    let tmp = tempfile::tempdir().expect("tmp");
    let mut s = spec("ok", tmp.path());
    s.budget_weighted = Some(1);
    let run = runner(EventSink::null("001")).run(&s);
    assert_eq!(run.stop_reason, StopReason::Done);
    assert_eq!(run.weighted, 9503);
}

#[test]
fn api_key_vars_are_removed_unless_allowed() {
    let tmp = tempfile::tempdir().expect("tmp");
    let mut s = spec("env\nOPENAI_API_KEY", tmp.path());
    s.env
        .push(("OPENAI_API_KEY".to_string(), "sk-test".to_string()));
    let run = runner(EventSink::null("001")).run(&s);
    assert_eq!(run.result_text, "<unset>");
    let run = runner(EventSink::null("001"))
        .with_api_key_allowed(true)
        .run(&s);
    assert_eq!(run.result_text, "sk-test");
}

#[test]
fn a_session_event_is_emitted_for_the_thread_id() {
    let tmp = tempfile::tempdir().expect("tmp");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let into = Arc::clone(&seen);
    let mut s = spec("ok", tmp.path());
    s.on_session = Some(SessionHook::new(move |id| {
        into.lock().expect("seen").push(id.to_string());
    }));
    runner(EventSink::null("001")).run(&s);
    assert_eq!(
        *seen.lock().expect("seen"),
        vec!["01a15eac-9d1e-73f0-98dd-ce2929f333e6".to_string()]
    );
}
