mod common;

use factory_core::{Job, Stage};
use factory_engine::{create_job, park, run_job, set_stage_logged};
use serde_json::Value;

fn events_named<'a>(lines: &'a [Value], name: &str) -> Vec<&'a Value> {
    lines.iter().filter(|l| l["event"] == name).collect()
}

fn build_job() -> Job {
    let mut job = Job::new("001", "2026-10-19-001-x", "x", "2026-10-19", "feature", 1.0);
    job.set_stage(Stage::Build, None, false, 2.0)
        .expect("to build");
    job
}

#[test]
fn stage_change_is_logged_with_from_and_to() {
    let mut job = build_job();
    let (res, lines) = factory_telemetry::capture("info", || {
        tracing::info_span!("job", job_id = "001")
            .in_scope(|| set_stage_logged(&mut job, Stage::Review, None, false, 3.0))
    });
    res.expect("move");
    let changes = events_named(&lines, "stage.change");
    assert_eq!(changes.len(), 1, "{lines:?}");
    assert_eq!(changes[0]["from"], "build");
    assert_eq!(changes[0]["to"], "review");
    assert_eq!(changes[0]["stage"], "review");
    assert_eq!(changes[0]["job_id"], "001");
}

#[test]
fn same_stage_is_not_logged() {
    let mut job = build_job();
    let (_, lines) = factory_telemetry::capture("info", || {
        let _ = set_stage_logged(&mut job, Stage::Build, None, false, 3.0);
    });
    assert!(events_named(&lines, "stage.change").is_empty(), "{lines:?}");
}

#[test]
fn illegal_stage_move_logs_nothing() {
    let mut job = build_job();
    job.set_stage(Stage::Merged, None, true, 3.0)
        .expect("force merged");
    let (res, lines) = factory_telemetry::capture("info", || {
        set_stage_logged(&mut job, Stage::Build, None, false, 4.0)
    });
    res.expect_err("illegal move");
    assert!(lines.is_empty(), "{lines:?}");
}

#[test]
fn park_logs_a_warning() {
    let fx = common::approved_job();
    let (res, lines) =
        factory_telemetry::capture("info", || park(&fx.driver, &fx.id, "risky diff"));
    res.expect("park");
    let parks = events_named(&lines, "park");
    assert_eq!(parks.len(), 1, "{lines:?}");
    assert_eq!(parks[0]["level"], "warn");
    assert_eq!(parks[0]["reason"], "risky diff");
}

#[test]
fn park_reason_is_one_line() {
    let fx = common::approved_job();
    let (res, lines) = factory_telemetry::capture("info", || park(&fx.driver, &fx.id, "a\nb"));
    res.expect("park");
    let parks = events_named(&lines, "park");
    assert_eq!(parks[0]["reason"], "a b");
}

#[test]
fn job_created_is_logged_with_slug() {
    let fx = common::SeriesFixture::new("fine\nVERDICT: APPROVE");
    let plan = fx.plan_file("plan-01-a.md", 1, "none");
    let (res, lines) = factory_telemetry::capture("info", || {
        create_job(&fx.driver, "hello", "say hello", Some(&plan), None)
    });
    res.expect("create");
    let created = events_named(&lines, "job.created");
    assert_eq!(created.len(), 1, "{lines:?}");
    assert_eq!(created[0]["slug"], "hello");
}

#[test]
fn run_job_lines_carry_the_job_id() {
    let fx = common::SeriesFixture::new("fine\nVERDICT: APPROVE");
    let plan = fx.plan_file("plan-01-a.md", 1, "none");
    let job = create_job(&fx.driver, "hello", "say hello", Some(&plan), None).expect("create");
    let (res, lines) = factory_telemetry::capture("info", || run_job(&fx.driver, &job.id));
    res.expect("run");
    assert!(!lines.is_empty());
    for l in &lines {
        assert!(
            l.get("job_id").is_some() || l["event"] == "job.created",
            "{l}"
        );
    }
}

fn builder_effects() -> Vec<common::Effect> {
    vec![Box::new(|spec| {
        std::fs::write(spec.cwd.join("a.txt"), "x\n").expect("write");
        "done".to_string()
    })]
}

#[test]
fn agent_run_logs_start_and_end() {
    let fx = common::Fixture::new();
    let runner = common::scripted_auto(builder_effects());
    let (res, lines) = factory_telemetry::capture("info", || fx.engine(&runner).build_stage());
    res.expect("build");
    let starts = events_named(&lines, "agent.start");
    let ends = events_named(&lines, "agent.end");
    assert!(!starts.is_empty(), "{lines:?}");
    assert_eq!(starts.len(), ends.len(), "{lines:?}");
    assert_eq!(starts[0]["role"], "builder");
    assert_eq!(ends[0]["weighted_tokens"], 0);
    assert_eq!(ends[0]["stop_reason"], "Done");
    assert!(ends[0].get("duration_ms").is_some());
    let pos = |name: &str| {
        lines
            .iter()
            .position(|l| l["event"] == name)
            .expect("event")
    };
    assert!(pos("agent.start") < pos("agent.end"));
}

struct UsageRunner(common::ScriptedRunner, bool);

impl factory_agent::AgentRunner for UsageRunner {
    fn run(&self, spec: &factory_agent::AgentSpec) -> factory_agent::AgentRun {
        let mut run = self.0.run(spec);
        if spec.role == "builder" {
            run.usage = factory_agent::Usage {
                input_tokens: 10,
                cache_creation_input_tokens: 34_419,
                cache_read_input_tokens: 151_613,
                output_tokens: 1_790,
                cache_creation_1h_input_tokens: 0,
            };
            run.turns = 5;
            run.start_ctx = 30_748;
            run.end_ctx = 42_784;
            if self.1 {
                run.session_id = None;
            }
        }
        run
    }
}

#[test]
fn agent_end_carries_usage_fields() {
    let fx = common::Fixture::new();
    // The engine counts rounds from 1, a new job starts at 0.
    let round = fx.job().round.max(1);
    let runner = UsageRunner(common::scripted_auto(builder_effects()), false);
    let (res, lines) = factory_telemetry::capture("info", || fx.engine(&runner).build_stage());
    res.expect("build");
    let ends = events_named(&lines, "agent.end");
    let end = ends
        .iter()
        .find(|l| l["role"] == "builder")
        .expect("builder end");
    assert_eq!(end["input"], 10);
    assert_eq!(end["cache_creation"], 34_419);
    assert_eq!(end["cache_read"], 151_613);
    assert_eq!(end["output"], 1_790);
    assert_eq!(end["turns"], 5);
    assert_eq!(end["resumed"], false);
    assert_eq!(end["start_ctx"], 30_748);
    assert_eq!(end["end_ctx"], 42_784);
    assert_eq!(end["session_id"], "sess-1");
    assert_eq!(end["round"], round);
    let redacted = end
        .as_object()
        .expect("object")
        .values()
        .any(|v| v == "[redacted]");
    assert!(!redacted, "{end:?}");
}

#[test]
fn agent_end_session_id_is_empty_without_a_session() {
    let fx = common::Fixture::new();
    let runner = UsageRunner(common::scripted_auto(builder_effects()), true);
    let (res, lines) = factory_telemetry::capture("info", || fx.engine(&runner).build_stage());
    res.expect("build");
    let ends = events_named(&lines, "agent.end");
    let end = ends
        .iter()
        .find(|l| l["role"] == "builder")
        .expect("builder end");
    assert!(end.get("session_id").is_some(), "{end:?}");
    assert_eq!(end["session_id"], "");
}

#[test]
fn failed_agent_run_logs_agent_end() {
    let fx = common::Fixture::new();
    let runner = common::scripted(vec![]);
    let (_, lines) = factory_telemetry::capture("info", || fx.engine(&runner).build_stage());
    let ends = events_named(&lines, "agent.end");
    assert!(!ends.is_empty(), "{lines:?}");
    assert_eq!(ends[0]["stop_reason"], "Failed");
}

fn checks_run(fx: &common::Fixture, level: &str) -> Vec<Value> {
    let runner = common::scripted_auto(builder_effects());
    fx.engine(&runner).build_stage().expect("build");
    let (res, lines) = factory_telemetry::capture(level, || fx.engine(&runner).checks_stage());
    res.expect("checks");
    lines
}

#[test]
fn check_run_is_logged_per_command() {
    let mut fx = common::Fixture::new();
    fx.config.checks[0].commands = vec!["true".into(), "false".into()];
    let lines = checks_run(&fx, "info");
    let runs = events_named(&lines, "check.run");
    assert_eq!(runs.len(), 2, "{lines:?}");
    assert_eq!(runs[0]["passed"], true);
    assert_eq!(runs[1]["passed"], false);
}

#[test]
fn check_command_secret_is_redacted() {
    let mut fx = common::Fixture::new();
    fx.config.checks[0].commands = vec!["echo --token=abc".into()];
    let lines = checks_run(&fx, "debug");
    let runs = events_named(&lines, "check.run");
    assert_eq!(runs[0]["command"], "echo --token=[redacted]", "{lines:?}");
    for l in &lines {
        assert!(!l.to_string().contains("abc"), "{l}");
    }
}

#[test]
fn decide_logs_the_choice() {
    let fx = common::committing_fixture();
    let runner = common::scripted_auto(builder_effects());
    fx.engine(&runner).build_stage().expect("build");
    fx.engine(&runner).checks_stage().expect("checks");
    let rr = common::scripted(vec![Box::new(|_| "fine\nVERDICT: APPROVE".to_string())]);
    let (res, lines) = factory_telemetry::capture("info", || fx.engine(&rr).review_stage());
    res.expect("review");
    let decides = events_named(&lines, "decide");
    assert_eq!(decides.len(), 1, "{lines:?}");
    assert_eq!(decides[0]["choice"], "merge");
    assert_eq!(decides[0]["round"], 1);
}

fn error_step<'a>(lines: &'a [Value], step: &str) -> Vec<&'a Value> {
    lines
        .iter()
        .filter(|l| l["level"] == "error" && l["step"] == step)
        .collect()
}

#[test]
fn merge_logs_sha() {
    let fx = common::approved_job();
    let (res, lines) =
        factory_telemetry::capture("info", || factory_engine::merge_stage(&fx.driver, &fx.id));
    let sha = match res.expect("merge") {
        factory_engine::MergeResult::Merged(sha) => sha,
        other => panic!("expected a merge, got {other:?}"),
    };
    let done = events_named(&lines, "merge.done");
    assert_eq!(done.len(), 1, "{lines:?}");
    assert_eq!(done[0]["sha"], sha.as_str());
}

#[test]
fn kb_failure_logs_step_error_and_merge_result_is_unchanged() {
    let config = factory_core::Config {
        kb_path: Some("kb/rocketapi".to_string()),
        ..factory_core::Config::default()
    };
    let fx =
        common::approved_job_config(std::sync::Arc::new(common::scripted(vec![])), false, config);
    let (res, lines) =
        factory_telemetry::capture("info", || factory_engine::merge_stage(&fx.driver, &fx.id));
    assert!(matches!(
        res.expect("merge"),
        factory_engine::MergeResult::Merged(_)
    ));
    let errs = error_step(&lines, "kb_refresh");
    assert_eq!(errs.len(), 1, "{lines:?}");
    assert!(
        errs[0]["msg"]
            .as_str()
            .expect("msg")
            .starts_with("kb refresh error for job"),
        "{}",
        errs[0]
    );
}

#[test]
fn tickets_failure_logs_step_error() {
    let t = common::FakeTracker::new("o/r");
    t.fail_on("repo_name");
    let config = factory_core::Config {
        tickets_on_merge: true,
        ..factory_core::Config::default()
    };
    let fx = common::with_tracker(
        common::approved_job_config(common::approving_runner(3), false, config),
        t,
    );
    common::set_feature(&fx, "dedup cleanups #76", Some("## Goal\nsomething with enough words to pass the check here\n\n## Acceptance criteria\n- a\n"));
    let (res, lines) =
        factory_telemetry::capture("info", || factory_engine::merge_stage(&fx.driver, &fx.id));
    assert!(matches!(
        res.expect("merge"),
        factory_engine::MergeResult::Merged(_)
    ));
    assert!(!error_step(&lines, "tickets").is_empty(), "{lines:?}");
}

#[test]
fn git_ops_log_at_debug_only() {
    let run = |level: &str| {
        let fx = common::approved_job();
        let (res, lines) =
            factory_telemetry::capture(level, || factory_engine::merge_stage(&fx.driver, &fx.id));
        res.expect("merge");
        lines
    };
    let debug = run("debug");
    assert!(!events_named(&debug, "git.op").is_empty(), "{debug:?}");
    let info = run("info");
    assert!(events_named(&info, "git.op").is_empty(), "{info:?}");
}
