//! `factory probe`: ask each role's agent what reaches its context and print the answer as a table.
use std::sync::Arc;

use factory_agent::{AgentRunner, Caps, ClaudeRunner, CodexRunner, EventSink};
use factory_core::{Channel, Harness, ProbeReport, ProbeRole};
use factory_engine::{utc_stamp, Delivery};

use crate::commands::{api_key_source, CliCtx, CliError};

/// The first line of `<program> --version`, or `unknown` when it cannot run.
fn cli_version(program: &str) -> String {
    std::process::Command::new(program)
        .arg("--version")
        .output()
        .ok()
        .and_then(|out| {
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .next()
                .map(|l| l.trim().to_string())
        })
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

/// The Claude flags the delivery path can rely on. Stdin means neither flag exists.
fn caps_for(delivery: Delivery) -> Caps {
    match delivery {
        Delivery::Agent => Caps::default(),
        Delivery::AppendSystemPrompt => Caps {
            agent: false,
            ..Caps::default()
        },
        Delivery::Stdin => Caps {
            agent: false,
            append_system_prompt: false,
            ..Caps::default()
        },
    }
}

/// Runs the probe and returns the table, or the JSON report with `json`. A failing report is an error.
pub fn run_probe_command(
    ctx: &CliCtx,
    harness: Option<Harness>,
    roles: &[ProbeRole],
    json: bool,
    delivery: Delivery,
) -> Result<String, CliError> {
    ctx.runnable()?;
    let d = ctx.driver()?;
    let harness = harness.unwrap_or_else(|| Harness::from_config(d.config()));
    let roles = if roles.is_empty() {
        ProbeRole::ALL.to_vec()
    } else {
        roles.to_vec()
    };
    let stamp = utc_stamp();
    let (runner, version): (Arc<dyn AgentRunner>, String) = match &ctx.runner {
        Some(r) => (r.clone(), "unknown".to_string()),
        None => {
            // The sink writes next to the transcripts, so a probe never touches events.jsonl.
            let folder = d
                .layout()
                .probes_dir()
                .join(format!("{stamp}-{}", harness.as_str()));
            let sink = EventSink::new(folder.join("events.jsonl"), "probe");
            match harness {
                Harness::Codex => (
                    Arc::new(CodexRunner::new("codex", sink)),
                    cli_version("codex"),
                ),
                Harness::Claude => (
                    Arc::new(
                        ClaudeRunner::new("claude", caps_for(delivery), sink)
                            .with_api_key_source(api_key_source(Some(d.config()))),
                    ),
                    cli_version("claude"),
                ),
            }
        }
    };
    let report = d.probe(runner.as_ref(), harness, &roles, delivery, &version, &stamp)?;
    let text = if json {
        serde_json::to_string_pretty(&report)?
    } else {
        render_probe(&report)
    };
    if report.ok() {
        Ok(text)
    } else {
        Err(CliError::ProbeFailed(text))
    }
}

fn cell(channel: &Channel) -> &'static str {
    match (channel.expected, channel.present) {
        (true, true) => "ok",
        (true, false) => "MISSING",
        (false, false) => "-",
        (false, true) => "extra",
    }
}

/// The table of a report: one row per role, one column per channel, then the skills each role saw.
pub fn render_probe(report: &ProbeReport) -> String {
    let names: Vec<&str> = report
        .roles
        .first()
        .map(|r| r.channels.iter().map(|c| c.name.as_str()).collect())
        .unwrap_or_default();
    let role_width = report
        .roles
        .iter()
        .map(|r| r.role.as_str().len())
        .chain(["role".len()])
        .max()
        .unwrap_or(4);
    let mut out = format!(
        "harness {}  cli {}\n\n",
        report.harness.as_str(),
        report.cli_version
    );
    out.push_str(&format!("{:<role_width$}", "role"));
    for name in &names {
        out.push_str(&format!("  {:<width$}", name, width = name.len().max(7)));
    }
    out.push('\n');
    for role in &report.roles {
        out.push_str(&format!("{:<role_width$}", role.role.as_str()));
        for name in &names {
            let shown = role
                .channels
                .iter()
                .find(|c| c.name == *name)
                .map_or("-", cell);
            out.push_str(&format!("  {:<width$}", shown, width = name.len().max(7)));
        }
        out.push('\n');
    }
    out.push('\n');
    for role in &report.roles {
        let skills = if role.skills.is_empty() {
            "NONE".to_string()
        } else {
            role.skills.join(", ")
        };
        out.push_str(&format!("Skills seen ({}): {skills}\n", role.role.as_str()));
        for channel in role.channels.iter().filter(|c| c.expected && !c.present) {
            out.push_str(&format!(
                "Missing ({}) {}: {}\n",
                role.role.as_str(),
                channel.name,
                channel.detail
            ));
        }
    }
    out.push_str(&format!(
        "\nReport: {}.json\nTranscripts: {}\n",
        report.transcript_dir.display(),
        report.transcript_dir.display()
    ));
    out
}
