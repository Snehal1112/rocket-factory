use std::time::{Duration, Instant};

use factory_agent::{AgentRun, AgentRunner, AgentSpec, Caps, ClaudeRunner, EventSink, FakeRunner, StopReason};

fn runner(sink: EventSink) -> ClaudeRunner {
    ClaudeRunner::new(env!("CARGO_BIN_EXE_fake-claude"), Caps { agent: true, max_turns: false, append_system_prompt: true }, sink)
}

fn spec(mode: &str, dir: &std::path::Path) -> AgentSpec {
    AgentSpec::new("001", "builder", dir, mode)
}

#[test]
fn ok_run_collects_session_usage_result_and_events() {
    let tmp = tempfile::tempdir().expect("tmp");
    let log = tmp.path().join("events.jsonl");
    let r = runner(EventSink::new(&log, "001"));
    let run = r.run(&spec("ok", tmp.path()));
    assert_eq!(run.stop_reason, StopReason::Done);
    assert_eq!(run.session_id.as_deref(), Some("fake-1"));
    assert_eq!(run.result_text, "all good");
    // m1 counts once at 50, m2 at 10.
    assert_eq!(run.usage.output_tokens, 60);
    assert_eq!(run.weighted, 60);
    assert_eq!(run.turns, 2);
    let lines: Vec<serde_json::Value> =
        std::fs::read_to_string(&log).expect("log").lines().map(|l| serde_json::from_str(l).expect("json")).collect();
    let names: Vec<&str> = lines.iter().map(|v| v["hook_event_name"].as_str().expect("name")).collect();
    assert_eq!(names, vec!["SubagentStart", "PostToolUse", "SubagentStop"]);
    assert_eq!(lines[1]["tool_name"], "Bash");
    assert_eq!(lines[1]["tool_target"], "echo hi");
    assert!(lines.iter().all(|v| v["job_id"] == "001" && v["agent_id"] == "001-builder"));
    assert_eq!(lines[2]["tokens"], 60);
}

#[test]
fn args_reach_the_child_and_prompt_goes_to_stdin() {
    let tmp = tempfile::tempdir().expect("tmp");
    let mut s = spec("args", tmp.path());
    s.model = Some("sonnet".into());
    s.resume = Some("sess-9".into());
    let run = runner(EventSink::null("001")).run(&s);
    assert_eq!(run.stop_reason, StopReason::Done);
    assert!(run.result_text.starts_with("-p --output-format stream-json --verbose"));
    assert!(run.result_text.contains("--model sonnet --resume sess-9"));
}

#[test]
fn error_result_is_failed() {
    let tmp = tempfile::tempdir().expect("tmp");
    let run = runner(EventSink::null("001")).run(&spec("error", tmp.path()));
    assert_eq!(run.stop_reason, StopReason::Failed);
    assert_eq!(run.result_text, "it broke");
}

#[test]
fn no_result_is_failed() {
    let tmp = tempfile::tempdir().expect("tmp");
    let run = runner(EventSink::null("001")).run(&spec("noresult", tmp.path()));
    assert_eq!(run.stop_reason, StopReason::Failed);
    assert_eq!(run.exit_code, Some(3));
    let err = run.error.expect("error text");
    assert!(err.contains("boom"), "stderr tail missing: {err}");
}

#[test]
fn missing_program_is_failed_not_a_panic() {
    let tmp = tempfile::tempdir().expect("tmp");
    let r = ClaudeRunner::new("/nonexistent/claude-binary", Caps::default(), EventSink::null("001"));
    let run = r.run(&spec("ok", tmp.path()));
    assert_eq!(run.stop_reason, StopReason::Failed);
    assert!(run.error.is_some());
}

#[test]
fn flood_does_not_deadlock() {
    let tmp = tempfile::tempdir().expect("tmp");
    let start = Instant::now();
    let run = runner(EventSink::null("001")).run(&spec("flood", tmp.path()));
    assert_eq!(run.stop_reason, StopReason::Done);
    assert_eq!(run.result_text, "flooded");
    assert!(start.elapsed() < Duration::from_secs(20));
}

#[test]
fn silent_child_is_stalled_and_killed() {
    let tmp = tempfile::tempdir().expect("tmp");
    let mut s = spec("silent", tmp.path());
    s.stall_after = Some(Duration::from_millis(400));
    let start = Instant::now();
    let run = runner(EventSink::null("001")).run(&s);
    assert_eq!(run.stop_reason, StopReason::Stalled);
    // The child sleeps 30 seconds, so a quick return proves it was killed.
    assert!(start.elapsed() < Duration::from_secs(10));
}

#[test]
fn budget_stop_kills_the_child() {
    let tmp = tempfile::tempdir().expect("tmp");
    let mut s = spec("budget", tmp.path());
    s.budget_weighted = Some(5000);
    let start = Instant::now();
    let run = runner(EventSink::null("001")).run(&s);
    assert_eq!(run.stop_reason, StopReason::BudgetStop);
    assert!(run.weighted >= 5000 && run.weighted < 20000, "weighted {}", run.weighted);
    assert!(start.elapsed() < Duration::from_secs(10));
}

#[test]
fn used_before_counts_toward_the_budget() {
    let tmp = tempfile::tempdir().expect("tmp");
    let mut s = spec("budget", tmp.path());
    s.budget_weighted = Some(5000);
    s.used_before = 4000;
    let run = runner(EventSink::null("001")).run(&s);
    assert_eq!(run.stop_reason, StopReason::BudgetStop);
    // Only 1000 more weighted tokens were needed, so well under 5000 came from this run.
    assert!(run.weighted < 5000, "weighted {}", run.weighted);
}

#[test]
fn turn_limit_is_enforced_by_the_driver() {
    let tmp = tempfile::tempdir().expect("tmp");
    let mut s = spec("turns", tmp.path());
    s.max_turns = Some(3);
    let run = runner(EventSink::null("001")).run(&s);
    assert_eq!(run.stop_reason, StopReason::TurnLimit);
    assert!(run.turns >= 4 && run.turns < 50, "turns {}", run.turns);
}

#[test]
fn event_log_failure_does_not_fail_the_run() {
    let tmp = tempfile::tempdir().expect("tmp");
    let blocker = tmp.path().join("file");
    std::fs::write(&blocker, "x").expect("write");
    let sink = EventSink::new(blocker.join("events.jsonl"), "001");
    let r = runner(sink);
    let run = r.run(&spec("ok", tmp.path()));
    assert_eq!(run.stop_reason, StopReason::Done);
}

#[test]
fn fake_runner_replays_script_and_records_calls() {
    let fake = FakeRunner::new(vec![AgentRun::done("one"), AgentRun::failed("two")]);
    let tmp = tempfile::tempdir().expect("tmp");
    let a = fake.run(&spec("a", tmp.path()));
    let b = fake.run(&spec("b", tmp.path()));
    let c = fake.run(&spec("c", tmp.path()));
    assert_eq!(a.result_text, "one");
    assert_eq!(b.stop_reason, StopReason::Failed);
    assert_eq!(c.error.as_deref(), Some("fake script exhausted"));
    let prompts: Vec<String> = fake.calls().into_iter().map(|s| s.prompt).collect();
    assert_eq!(prompts, vec!["a", "b", "c"]);
}

#[test]
fn env_reaches_the_child() {
    let tmp = tempfile::tempdir().expect("tmp");
    let mut s = spec("env\nFACTORY_TEST_ENV", tmp.path());
    s.env = vec![("FACTORY_TEST_ENV".into(), "from-spec".into())];
    let run = runner(EventSink::null("001")).run(&s);
    assert_eq!(run.stop_reason, StopReason::Done);
    assert_eq!(run.result_text, "from-spec");
}

#[test]
fn env_is_unset_when_the_spec_does_not_set_it() {
    let tmp = tempfile::tempdir().expect("tmp");
    let run = runner(EventSink::null("001")).run(&spec("env\nFACTORY_TEST_ENV", tmp.path()));
    assert_eq!(run.result_text, "<unset>");
}
