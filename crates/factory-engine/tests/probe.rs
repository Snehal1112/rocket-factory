mod common;

use common::Fixture;
use factory_agent::{AgentRun, AgentSpec, FakeRunner};
use factory_core::{AgentFile, Harness, ProbeReport, ProbeRole};
use factory_engine::{run_probe, Delivery};

const MARKER: &str = "FPROBE-test00000001";

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

fn fixture(harness: Harness) -> Fixture {
    let mut fx = Fixture::new();
    if harness == Harness::Codex {
        fx.config.agent_cli = Some("codex".to_string());
    }
    fx
}

fn answer(marker: &str) -> AgentRun {
    AgentRun::done(&format!(
        "MARKER: {marker}\nBOOTSTRAP: You have superpowers.\nTOOLS: Read\nSKILLS: superpowers:brainstorming"
    ))
}

fn work_dir(fx: &Fixture) -> std::path::PathBuf {
    let dir = fx.tmp.path().join("work");
    std::fs::create_dir_all(&dir).expect("work dir");
    dir
}

/// Runs one probe and returns the spec the runner saw.
fn spec_of(role: ProbeRole, harness: Harness, delivery: Delivery) -> (AgentSpec, Fixture) {
    let fx = fixture(harness);
    let runner = FakeRunner::new(vec![answer(MARKER)]);
    let dir = work_dir(&fx);
    fx.engine(&runner)
        .with_agent_files(files())
        .probe_role_with(role, harness, delivery, MARKER, &dir)
        .expect("probe");
    let spec = runner.calls().pop().expect("one run");
    (spec, fx)
}

#[test]
fn probe_spec_cannot_write() {
    for role in ProbeRole::ALL {
        let (spec, _fx) = spec_of(role, Harness::Claude, Delivery::Agent);
        assert!(spec.allowed_tools.is_empty(), "{role:?}");
        assert!(spec.disallowed_tools.is_empty(), "{role:?}");
        assert_eq!(spec.permission_mode.as_deref(), Some("dontAsk"), "{role:?}");
        assert_eq!(spec.max_turns, Some(1), "{role:?}");
        assert_eq!(spec.resume, None, "{role:?}");
        assert_eq!(spec.budget_weighted, Some(50_000), "{role:?}");
        let (spec, _fx) = spec_of(role, Harness::Codex, Delivery::Agent);
        assert_eq!(spec.sandbox.as_deref(), Some("read-only"), "{role:?}");
        assert!(!spec.network, "{role:?}");
        assert!(spec.writable_roots.is_empty(), "{role:?}");
    }
}

#[test]
fn prompt_is_replaced_and_marker_is_in_agent_text() {
    let (spec, fx) = spec_of(ProbeRole::Builder, Harness::Claude, Delivery::Agent);
    assert!(spec.prompt.starts_with("PROBE"), "{}", spec.prompt);
    assert_eq!(spec.agent.as_deref(), Some("factory-builder"));
    assert_eq!(spec.cwd, fx.tmp.path().join("work"));
    let file = spec.cwd.join(".claude/agents/factory-builder.md");
    let text = std::fs::read_to_string(file).expect("agent file");
    assert!(text.starts_with("---\nname: factory-builder"), "{text}");
    assert!(text.contains("BODY builder"), "{text}");
    assert!(text.contains(&format!("Probe marker: {MARKER}")), "{text}");
    assert!(!spec.prompt.contains(MARKER));
}

#[test]
fn append_delivery_puts_the_marker_in_agent_prompt() {
    for delivery in [Delivery::AppendSystemPrompt, Delivery::Stdin] {
        let (spec, _fx) = spec_of(ProbeRole::Committer, Harness::Claude, delivery);
        assert_eq!(spec.agent, None, "{delivery:?}");
        let text = spec.agent_prompt.expect("agent text");
        assert!(text.starts_with("---\nname: factory-committer"), "{text}");
        assert!(text.contains(&format!("Probe marker: {MARKER}")), "{text}");
    }
}

#[test]
fn codex_marker_rides_agent_prompt_after_the_preface() {
    let (spec, _fx) = spec_of(ProbeRole::Builder, Harness::Codex, Delivery::Agent);
    assert_eq!(spec.agent, None);
    let text = spec.agent_prompt.expect("agent text");
    assert!(text.starts_with("You run under Codex."), "{text}");
    let body = text.find("BODY builder").expect("body");
    let marker = text
        .find(&format!("Probe marker: {MARKER}"))
        .expect("marker");
    assert!(body < marker, "{text}");
    assert!(!text.contains("name: factory-builder"), "{text}");
}

#[test]
fn reviewer_spec_keeps_disable_slash_commands() {
    let (reviewer, _fx) = spec_of(ProbeRole::Reviewer, Harness::Claude, Delivery::Agent);
    assert!(reviewer.disable_slash_commands);
    let (spec_reviewer, _fx) = spec_of(ProbeRole::SpecReviewer, Harness::Claude, Delivery::Agent);
    assert!(spec_reviewer.disable_slash_commands);
    let (builder, _fx) = spec_of(ProbeRole::Builder, Harness::Claude, Delivery::Agent);
    assert!(!builder.disable_slash_commands);
}

#[test]
fn all_channels_present_when_the_answer_echoes_everything() {
    let fx = fixture(Harness::Claude);
    let runner = FakeRunner::new(vec![answer(MARKER)]);
    let dir = work_dir(&fx);
    let (probe, text) = fx
        .engine(&runner)
        .with_agent_files(files())
        .probe_role_with(
            ProbeRole::Builder,
            Harness::Claude,
            Delivery::Agent,
            MARKER,
            &dir,
        )
        .expect("probe");
    assert!(probe.ok(), "{probe:?}");
    assert!(text.contains(MARKER));
    assert_eq!(probe.skills, ["superpowers:brainstorming"]);
}

#[test]
fn a_stale_marker_does_not_pass() {
    let fx = fixture(Harness::Claude);
    let runner = FakeRunner::new(vec![answer("FPROBE-stale")]);
    let dir = work_dir(&fx);
    let (probe, _) = fx
        .engine(&runner)
        .with_agent_files(files())
        .probe_role_with(
            ProbeRole::Builder,
            Harness::Claude,
            Delivery::Agent,
            MARKER,
            &dir,
        )
        .expect("probe");
    assert!(!probe.ok());
}

#[test]
fn failed_run_marks_all_expected_channels_missing() {
    for run in [AgentRun::failed("boom"), AgentRun::done("  ")] {
        let fx = fixture(Harness::Claude);
        let want = run.error.clone().unwrap_or_else(|| "empty answer".into());
        let runner = FakeRunner::new(vec![run]);
        let dir = work_dir(&fx);
        let (probe, _) = fx
            .engine(&runner)
            .with_agent_files(files())
            .probe_role_with(
                ProbeRole::Builder,
                Harness::Claude,
                Delivery::Agent,
                MARKER,
                &dir,
            )
            .expect("probe");
        assert!(!probe.ok());
        let expected: Vec<_> = probe.channels.iter().filter(|c| c.expected).collect();
        assert_eq!(expected.len(), 3);
        for c in expected {
            assert!(!c.present, "{}", c.name);
            assert_eq!(c.detail, want, "{}", c.name);
        }
    }
}

#[test]
fn missing_agent_file_is_an_error() {
    for harness in [Harness::Claude, Harness::Codex] {
        let fx = fixture(harness);
        let runner = FakeRunner::new(vec![answer(MARKER)]);
        let dir = work_dir(&fx);
        let err = fx
            .engine(&runner)
            .probe_role_with(ProbeRole::Builder, harness, Delivery::Agent, MARKER, &dir)
            .expect_err("no agent files");
        assert!(err.to_string().contains("factory-builder"), "{err}");
        assert!(runner.calls().is_empty());
    }
}

#[test]
fn run_probe_writes_report_and_transcripts() {
    let fx = fixture(Harness::Claude);
    let none = "MARKER: NONE\nBOOTSTRAP: NONE\nTOOLS: NONE\nSKILLS: NONE";
    let runner = FakeRunner::new(vec![AgentRun::done(none), AgentRun::done(none)]);
    let probes = fx.tmp.path().join("probes");
    let stamp = "20261030T120000Z";
    let report = run_probe(
        &fx.engine(&runner).with_agent_files(files()),
        Harness::Claude,
        &[ProbeRole::Builder, ProbeRole::Kb],
        Delivery::Agent,
        "1.2.3",
        &probes,
        stamp,
    )
    .expect("probe");
    assert!(!report.ok());
    assert_eq!(report.cli_version, "1.2.3");
    assert_eq!(
        report.transcript_dir,
        probes.join(format!("{stamp}-claude"))
    );
    let json =
        std::fs::read_to_string(probes.join(format!("{stamp}-claude.json"))).expect("report");
    let back: ProbeReport = serde_json::from_str(&json).expect("parse");
    assert_eq!(back, report);
    for role in ["builder", "kb"] {
        let text = std::fs::read_to_string(report.transcript_dir.join(format!("{role}.txt")))
            .expect("transcript");
        assert!(text.contains("== prompt ==\nPROBE"), "{text}");
        assert!(text.contains("== answer ==\nMARKER: NONE"), "{text}");
    }
}
