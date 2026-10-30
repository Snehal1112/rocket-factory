//! The live marker probe: builds each role's real spec, swaps the prompt, hides the tools and
//! reads back what the agent says reached its context.
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::path::{Path, PathBuf};

use factory_agent::AgentSpec;
use factory_core::{
    agent_files_rel_dir, evaluate, expected_channels, parse_probe_answer, probe_prompt,
    write_atomic, Harness, ProbeAnswer, ProbeReport, ProbeRole, RoleProbe,
};

use crate::ports::{core_err, io_err, JobView};
use crate::specs::spec_reviewer_agent;
use crate::{committer_spec, kb_spec, Engine, EngineError, ReviewerExtra};

/// Where the probe puts the marker so the agent can see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Delivery {
    /// A written agent file, passed with `--agent`.
    #[default]
    Agent,
    /// The `--append-system-prompt` flag.
    AppendSystemPrompt,
    /// The start of stdin. The caller builds the runner without the two flags.
    Stdin,
}

impl std::str::FromStr for Delivery {
    type Err = String;

    /// Reads the `--delivery` values `agent`, `append` and `stdin`.
    fn from_str(s: &str) -> Result<Delivery, String> {
        match s {
            "agent" => Ok(Delivery::Agent),
            "append" => Ok(Delivery::AppendSystemPrompt),
            "stdin" => Ok(Delivery::Stdin),
            other => Err(format!(
                "unknown delivery `{other}`, expected agent, append or stdin"
            )),
        }
    }
}

/// The weighted token cap of one probe run.
const PROBE_BUDGET: u64 = 50_000;

/// A fresh marker, `FPROBE-` and 12 hex digits.
fn fresh_marker() -> String {
    let bits = RandomState::new().build_hasher().finish();
    format!("FPROBE-{:012x}", bits & 0xffff_ffff_ffff)
}

fn probe_job_view() -> JobView {
    JobView {
        slug: "probe".to_string(),
        branch: "probe".to_string(),
        worktree: None,
        base: None,
        head: None,
        prev_head: None,
        session_id: None,
        round: 1,
        task_index: 0,
        issues: Vec::new(),
        batch_snapshot: None,
        fix_source: None,
        escalated_model: None,
        builder_session_model: None,
        review_missing: false,
        park_kind: None,
        task_session: None,
        builder_last: None,
        rework_base: 0,
        last_review: None,
    }
}

impl Engine<'_> {
    /// The scratch folder of one role. It holds the written agent file of the `Agent` delivery.
    fn probe_scratch(&self, role: ProbeRole) -> PathBuf {
        self.job_dir.join("probe").join(role.as_str())
    }

    /// Probes one role with the agent file delivery and a fresh marker.
    pub fn probe_role(&self, role: ProbeRole, harness: Harness) -> Result<RoleProbe, EngineError> {
        let work_dir = self.probe_scratch(role);
        let marker = fresh_marker();
        let (probe, _) =
            self.probe_role_with(role, harness, Delivery::Agent, &marker, &work_dir)?;
        Ok(probe)
    }

    /// Probes one role. The second value is the raw answer text for the transcript.
    pub fn probe_role_with(
        &self,
        role: ProbeRole,
        harness: Harness,
        delivery: Delivery,
        marker: &str,
        work_dir: &Path,
    ) -> Result<(RoleProbe, String), EngineError> {
        let (probe, answer, _) = self.probe_run(role, harness, delivery, marker, work_dir)?;
        Ok((probe, answer))
    }

    /// The real spec of a role, shaped for the harness, with the prompt still the role's own.
    fn probe_base_spec(&self, role: ProbeRole, work_dir: &Path) -> AgentSpec {
        match role {
            ProbeRole::Builder => self.builder_first_spec(&probe_job_view(), work_dir),
            ProbeRole::Reviewer => self.reviewer_spec(
                work_dir,
                &work_dir.join("probe.diff"),
                &work_dir.join("probe.names"),
                1,
                &[],
                &ReviewerExtra::default(),
            ),
            ProbeRole::SpecReviewer => spec_reviewer_agent(
                self.repo,
                &self.config,
                self.repo.root(),
                &work_dir.join("spec.md"),
                &[work_dir.join("plan.md")],
            ),
            ProbeRole::Committer => committer_spec(&self.job_id, &self.config, work_dir, "probe"),
            ProbeRole::Kb => {
                let kb_dir = self
                    .config
                    .kb_dir()
                    .unwrap_or_else(|| work_dir.to_path_buf());
                kb_spec(
                    &self.job_id,
                    &self.config,
                    self.repo.root(),
                    &kb_dir,
                    "probe",
                )
            }
        }
    }

    fn probe_run(
        &self,
        role: ProbeRole,
        harness: Harness,
        delivery: Delivery,
        marker: &str,
        work_dir: &Path,
    ) -> Result<(RoleProbe, String, AgentSpec), EngineError> {
        let configured = Harness::from_config(&self.config);
        if configured != harness {
            return Err(EngineError::Msg(format!(
                "the probe harness {} does not match agent_cli {}",
                harness.as_str(),
                configured.as_str()
            )));
        }
        let mut spec = self.probe_base_spec(role, work_dir);
        self.shape_spec(&mut spec).map_err(EngineError::Msg)?;
        spec.prompt = probe_prompt(marker);
        spec.resume = None;
        spec.used_before = 0;
        spec.budget_weighted = Some(PROBE_BUDGET);
        spec.cwd = work_dir.to_path_buf();
        match harness {
            Harness::Claude => self.hide_claude_tools(&mut spec, delivery, marker, work_dir)?,
            Harness::Codex => hide_codex_tools(&mut spec, marker),
        }
        let run = self.runner.run(&spec);
        let failure = match (&run.stop_reason, run.result_text.trim().is_empty()) {
            (factory_agent::StopReason::Done, false) => None,
            (factory_agent::StopReason::Done, true) => Some("empty answer".to_string()),
            _ => Some(
                run.error
                    .clone()
                    .unwrap_or_else(|| format!("run ended with {:?}", run.stop_reason)),
            ),
        };
        let probe = match failure {
            None => {
                let answer = parse_probe_answer(&run.result_text, marker);
                evaluate(role, harness, &self.config, &answer)
            }
            Some(why) => {
                let mut probe = evaluate(role, harness, &self.config, &ProbeAnswer::default());
                let expected = expected_channels(role, harness, &self.config);
                for channel in &mut probe.channels {
                    if expected.contains(&channel.name.as_str()) {
                        channel.present = false;
                        channel.detail = why.clone();
                    }
                }
                probe
            }
        };
        Ok((probe, run.result_text, spec))
    }

    /// Claude: keep the role's tool and skill lists, but deny every tool call and stop after one turn.
    fn hide_claude_tools(
        &self,
        spec: &mut AgentSpec,
        delivery: Delivery,
        marker: &str,
        work_dir: &Path,
    ) -> Result<(), EngineError> {
        spec.allowed_tools.clear();
        spec.disallowed_tools.clear();
        spec.permission_mode = Some("dontAsk".to_string());
        spec.max_turns = Some(1);
        let name = spec
            .agent
            .clone()
            .ok_or_else(|| EngineError::Msg(format!("{} spec names no agent file", spec.role)))?;
        let file = self
            .agent_files
            .iter()
            .find(|f| f.name == name || f.name == format!("{name}.md"))
            .ok_or_else(|| EngineError::Msg(format!("agent file {name} not found")))?;
        let text = format!("{}\n\nProbe marker: {marker}\n", file.content);
        match delivery {
            Delivery::Agent => {
                let dir = work_dir.join(agent_files_rel_dir());
                std::fs::create_dir_all(&dir).map_err(io_err)?;
                write_atomic(&dir.join(format!("{name}.md")), text.as_bytes()).map_err(core_err)?;
            }
            Delivery::AppendSystemPrompt | Delivery::Stdin => {
                spec.agent = None;
                spec.agent_prompt = Some(text);
            }
        }
        Ok(())
    }
}

/// Codex has no tool switch, so the sandbox is read-only with no network and no writable folder.
/// The agent text already sits in `agent_prompt`, and the marker goes after it.
fn hide_codex_tools(spec: &mut AgentSpec, marker: &str) {
    spec.sandbox = Some("read-only".to_string());
    spec.network = false;
    spec.writable_roots.clear();
    let text = spec.agent_prompt.take().unwrap_or_default();
    spec.agent_prompt = Some(format!("{text}\n\nProbe marker: {marker}\n"));
}

fn transcript(spec: &AgentSpec, answer: &str) -> String {
    format!(
        "== prompt ==\n{}\n\n== spec ==\n{spec:#?}\n\n== answer ==\n{answer}\n",
        spec.prompt
    )
}

/// Probes the roles one after the other and writes the report and one transcript per role.
pub fn run_probe(
    engine: &Engine<'_>,
    harness: Harness,
    roles: &[ProbeRole],
    delivery: Delivery,
    cli_version: &str,
    probes_dir: &Path,
    stamp: &str,
) -> Result<ProbeReport, EngineError> {
    let transcript_dir = probes_dir.join(format!("{stamp}-{}", harness.as_str()));
    std::fs::create_dir_all(&transcript_dir).map_err(io_err)?;
    let mut probes = Vec::new();
    for &role in roles {
        let work_dir = engine.probe_scratch(role);
        std::fs::create_dir_all(&work_dir).map_err(io_err)?;
        let (probe, answer, spec) =
            engine.probe_run(role, harness, delivery, &fresh_marker(), &work_dir)?;
        std::fs::write(
            transcript_dir.join(format!("{}.txt", role.as_str())),
            transcript(&spec, &answer),
        )
        .map_err(io_err)?;
        probes.push(probe);
    }
    let report = ProbeReport {
        harness,
        cli_version: cli_version.to_string(),
        roles: probes,
        transcript_dir,
    };
    let json =
        serde_json::to_string_pretty(&report).map_err(|e| EngineError::Msg(e.to_string()))?;
    let file = probes_dir.join(format!("{stamp}-{}.json", harness.as_str()));
    write_atomic(&file, json.as_bytes()).map_err(core_err)?;
    Ok(report)
}
