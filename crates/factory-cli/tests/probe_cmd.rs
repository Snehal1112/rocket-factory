mod common;

use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use common::Fixture;
use factory_agent::{AgentRun, AgentRunner, AgentSpec, Caps, ClaudeRunner, EventSink, FakeRunner};
use factory_cli::{dispatch, Cli, CliCtx, CliError};
use factory_core::{agent_files_rel_dir, AgentFile, Layout, ProbeReport};

/// Answers every probe. `marker_line` decides what the MARKER line says, given the real marker.
struct ProbeRunner {
    marker_line: fn(&str) -> String,
}

/// The marker sits in the agent text, or in the agent file the spec names.
fn marker_of(spec: &AgentSpec) -> String {
    let text = match &spec.agent_prompt {
        Some(t) => t.clone(),
        None => {
            let name = spec.agent.as_deref().expect("agent name");
            let file = spec
                .cwd
                .join(agent_files_rel_dir())
                .join(format!("{name}.md"));
            std::fs::read_to_string(file).expect("agent file")
        }
    };
    text.split("Probe marker: ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .expect("marker in the agent text")
        .to_string()
}

impl AgentRunner for ProbeRunner {
    fn run(&self, spec: &AgentSpec) -> AgentRun {
        let marker = marker_of(spec);
        let (bootstrap, skills) = match spec.role.as_str() {
            "builder" => ("You have superpowers.", "superpowers:brainstorming"),
            "committer" | "message" => ("NONE", "dev-workflow-skills:1-git-commit"),
            "kb" => ("NONE", "kb-refresh"),
            _ => ("NONE", "NONE"),
        };
        AgentRun::done(&format!(
            "{}\nBOOTSTRAP: {bootstrap}\nTOOLS: NONE\nSKILLS: {skills}",
            (self.marker_line)(&marker)
        ))
    }
}

fn echo(marker: &str) -> String {
    format!("MARKER: {marker}")
}

fn none(_: &str) -> String {
    "MARKER: NONE".to_string()
}

fn files() -> Vec<AgentFile> {
    [
        "builder",
        "code-reviewer",
        "spec-reviewer",
        "committer",
        "kb-refresher",
    ]
    .iter()
    .map(|n| AgentFile {
        name: format!("factory-{n}.md"),
        content: format!("---\nname: factory-{n}\n---\nBODY {n}\n"),
    })
    .collect()
}

fn ctx_with(fx: &Fixture, runner: Arc<dyn AgentRunner>) -> CliCtx {
    let mut ctx = CliCtx::with_layout(Layout::in_repo(&fx.root), runner, "tester");
    ctx.agents = files();
    ctx
}

fn probe(ctx: &CliCtx, args: &[&str]) -> Result<String, CliError> {
    let mut argv = vec!["factory", "probe"];
    argv.extend_from_slice(args);
    dispatch(Cli::parse_from(argv), ctx)
}

fn scripted(fx: &Fixture) -> CliCtx {
    ctx_with(fx, Arc::new(ProbeRunner { marker_line: echo }))
}

#[test]
fn probe_prints_a_table_and_exits_zero() {
    let fx = Fixture::new();
    let text = probe(&scripted(&fx), &[]).expect("probe passes");
    assert!(text.starts_with("harness claude  cli unknown"), "{text}");
    for role in ["builder", "reviewer", "spec-reviewer", "committer", "kb"] {
        assert!(text.contains(role), "{role}: {text}");
    }
    assert!(text.contains("agent_text"), "{text}");
    assert!(text.contains("ok"), "{text}");
    assert!(text.contains("Skills seen (builder): superpowers:brainstorming"));
    assert!(text.contains("Skills seen (reviewer): NONE"));
    assert!(!text.contains("MISSING"), "{text}");
}

#[test]
fn probe_reports_missing_channel_and_fails() {
    let fx = Fixture::new();
    let ctx = ctx_with(&fx, Arc::new(ProbeRunner { marker_line: none }));
    match probe(&ctx, &["--role", "builder"]) {
        Err(CliError::ProbeFailed(text)) => {
            assert!(text.contains("MISSING"), "{text}");
            assert!(text.contains("Missing (builder) agent_text"), "{text}");
        }
        other => panic!("expected ProbeFailed, got {other:?}"),
    }
}

#[test]
fn probe_json_is_one_parseable_report() {
    let fx = Fixture::new();
    let text = probe(&scripted(&fx), &["--json", "--role", "kb"]).expect("probe passes");
    let report: ProbeReport = serde_json::from_str(&text).expect("the whole output is one report");
    assert_eq!(report.roles.len(), 1);
    assert!(report.ok());
}

#[test]
fn probe_role_filter_limits_rows() {
    let fx = Fixture::new();
    let text = probe(&scripted(&fx), &["--role", "kb", "--role", "committer"]).expect("passes");
    assert!(text.contains("Skills seen (kb)"), "{text}");
    assert!(text.contains("Skills seen (committer)"), "{text}");
    assert!(!text.contains("Skills seen (builder)"), "{text}");
    assert!(!text.contains("Skills seen (reviewer)"), "{text}");
}

#[test]
fn probe_saves_the_report_under_probes_dir() {
    let fx = Fixture::new();
    probe(&scripted(&fx), &["--role", "kb"]).expect("passes");
    let dir = Layout::in_repo(&fx.root).probes_dir();
    let saved: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("probes dir")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    assert_eq!(saved.len(), 1, "{saved:?}");
    let report: ProbeReport =
        serde_json::from_str(&std::fs::read_to_string(&saved[0]).expect("read")).expect("parse");
    assert!(report.transcript_dir.join("kb.txt").is_file());
}

#[test]
fn harness_flag_overrides_the_project_agent_cli() {
    let fx = Fixture::new();
    let runner = Arc::new(FakeRunner::new(vec![AgentRun::done("MARKER: NONE")]));
    let ctx = ctx_with(&fx, runner.clone());
    // The fixture project runs Claude, and the flag asks for Codex.
    let result = probe(&ctx, &["--harness", "codex", "--role", "kb"]);
    assert!(
        matches!(result, Err(CliError::ProbeFailed(_))),
        "{result:?}"
    );
    let spec = runner.calls().pop().expect("one run");
    assert_eq!(spec.sandbox.as_deref(), Some("read-only"));
    assert_eq!(spec.agent, None);
    assert!(spec.agent_prompt.is_some());
}

#[test]
fn probe_through_fake_claude_finds_the_marker() {
    let Some(program) = common::fake_claude() else {
        return;
    };
    let fx = Fixture::new();
    let cases = [
        ("agent", Caps::default()),
        (
            "append",
            Caps {
                agent: false,
                ..Caps::default()
            },
        ),
        (
            "stdin",
            Caps {
                agent: false,
                append_system_prompt: false,
                ..Caps::default()
            },
        ),
    ];
    for (delivery, caps) in cases {
        let runner = ClaudeRunner::new(&program, caps, EventSink::null("probe"));
        let ctx = ctx_with(&fx, Arc::new(runner));
        let text = probe(
            &ctx,
            &["--json", "--role", "reviewer", "--delivery", delivery],
        )
        .unwrap_or_else(|e| panic!("{delivery}: {e}"));
        let report: ProbeReport = serde_json::from_str(&text).expect("report");
        let channel = report.roles[0]
            .channels
            .iter()
            .find(|c| c.name == "agent_text")
            .expect("agent_text channel");
        assert!(channel.present, "{delivery}: {}", channel.detail);
    }
}
