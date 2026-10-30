use std::fmt::Display;
use std::path::PathBuf;

use factory_agent::{AgentRun, AgentSpec};
use factory_core::{Board, CoreError, Job, Ledger, Stage, TaskState};

use crate::{Engine, EngineError};

pub(crate) fn core_err<E: Display>(e: E) -> EngineError {
    EngineError::Core(e.to_string())
}
pub(crate) fn git_err<E: Display>(e: E) -> EngineError {
    EngineError::Git(e.to_string())
}
pub(crate) fn io_err<E: Display>(e: E) -> EngineError {
    EngineError::Io(e.to_string())
}

/// Seconds since the Unix epoch, the unit of `Job::stage_since`.
pub(crate) fn now() -> f64 {
    crate::clock::now_secs()
}

/// The first 7 characters of a sha, or the whole string when it is shorter.
pub(crate) fn short_sha(sha: &str) -> &str {
    sha.get(..7).unwrap_or(sha)
}

/// Collapses whitespace and newlines so a log field holds one line.
pub(crate) fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Logs a park as one warning with a one-line reason.
pub(crate) fn log_park(job_id: &str, reason: &str) {
    let reason = one_line(reason);
    tracing::warn!(
        event = "park",
        job_id,
        reason = reason.as_str(),
        msg = reason.as_str()
    );
}

/// Moves a job to a stage and logs `stage.change` when the move worked and the stage changed.
/// A failed move logs nothing and returns the same error.
pub fn set_stage_logged(
    job: &mut Job,
    to: Stage,
    reason: Option<&str>,
    force: bool,
    now: f64,
) -> Result<(), CoreError> {
    let from = job.stage;
    job.set_stage(to, reason, force, now)?;
    if from != to {
        let (from, to_name) = (from.as_str(), to.as_str());
        let reason = reason.map(one_line);
        tracing::info!(
            event = "stage.change",
            job_id = job.id.as_str(),
            from,
            to = to_name,
            reason = reason.as_deref().unwrap_or(""),
            stage = to_name,
            msg = format!("stage {from} -> {to_name}").as_str()
        );
    }
    Ok(())
}

/// A copy of the board fields the stages read.
#[derive(Debug, Clone)]
pub(crate) struct JobView {
    pub slug: String,
    pub branch: String,
    pub worktree: Option<PathBuf>,
    pub base: Option<String>,
    pub head: Option<String>,
    pub prev_head: Option<String>,
    pub session_id: Option<String>,
    pub round: u32,
    pub task_index: u32,
    pub issues: Vec<u32>,
    pub batch_snapshot: Option<String>,
    pub fix_source: Option<String>,
    /// The stronger builder model the job escalated to, if any.
    pub escalated_model: Option<String>,
    /// The model the stored builder session ran on, if known.
    pub builder_session_model: Option<String>,
    pub review_missing: bool,
    /// The kind of the park the job is in, if any.
    pub park_kind: Option<String>,
    /// The saved builder session of the task that was running, if any.
    pub task_session: Option<factory_core::board::TaskSession>,
    /// The last finished builder, task or fix, if any.
    pub builder_last: Option<factory_core::board::BuilderLast>,
    /// The round count at the last rework.
    pub rework_base: u32,
    /// The last review summary, if any.
    pub last_review: Option<factory_core::LastReview>,
}

impl Engine<'_> {
    pub(crate) fn job(&self) -> Result<JobView, EngineError> {
        let board = Board::load(&self.board_path).map_err(core_err)?;
        let job = board.find(&self.job_id).map_err(core_err)?;
        Ok(JobView {
            slug: job.slug.clone(),
            branch: job.branch.clone(),
            worktree: job.worktree.as_ref().map(PathBuf::from),
            base: job.base.clone(),
            head: job.head.clone(),
            prev_head: job.prev_head.clone(),
            session_id: job.builder_id.clone(),
            // Plan 01 starts a job at round 0. The engine counts rounds from 1.
            round: job.round.max(1),
            task_index: job.task_index,
            issues: job.issues.clone(),
            batch_snapshot: job.batch_snapshot.clone(),
            fix_source: job.fix_source.clone(),
            escalated_model: job.escalated_model.clone(),
            builder_session_model: job.builder_session_model.clone(),
            review_missing: job.review_missing(),
            park_kind: job.park_kind.clone(),
            task_session: job.task_session.clone(),
            builder_last: job.builder_last.clone(),
            rework_base: job.rework_base,
            last_review: job.last_review.clone(),
        })
    }

    /// Loads the board, applies one change, and saves it atomically.
    pub(crate) fn update(
        &self,
        f: impl FnOnce(&mut Job) -> Result<(), CoreError>,
    ) -> Result<(), EngineError> {
        // The store takes the board lock, so every stage write is serialized with the other writers.
        crate::BoardStore::new(&self.board_path, self.lock_wait)
            .update_job(&self.job_id, f)
            .map_err(core_err)
    }

    /// Moves the job to a stage. Only a fix round (review back to build) passes `force`.
    pub(crate) fn set_stage(
        &self,
        stage: Stage,
        reason: Option<&str>,
        force: bool,
    ) -> Result<(), EngineError> {
        self.update(|j| set_stage_logged(j, stage, reason, force, now()))
    }

    fn ledger_file(&self) -> Ledger {
        Ledger::new(self.job_dir.join("progress.md"))
    }

    /// Appends a strict `Task N: <state> (detail)` line.
    pub(crate) fn ledger_task(
        &self,
        n: u32,
        state: TaskState,
        detail: &str,
    ) -> Result<(), EngineError> {
        self.ledger_file()
            .task(n, state, Some(detail))
            .map_err(core_err)
    }

    /// Appends a `Ruling:` line. The ledger takes one line, so whitespace and newlines fold into single spaces.
    pub(crate) fn ruling(&self, text: &str) -> Result<(), EngineError> {
        let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
        self.ledger_file().ruling(&one_line).map_err(core_err)
    }

    /// Parks the job for the human with a reason, and tells the caller.
    pub(crate) fn park(&self, reason: String) -> Result<crate::StageOutcome, EngineError> {
        self.set_stage(Stage::NeedsHuman, Some(&reason), false)?;
        log_park(&self.job_id, &reason);
        self.ruling(&format!("needs-human, {reason}"))?;
        Ok(crate::StageOutcome::NeedsHuman(reason))
    }

    /// The job's own model from `factory new --model`, when set and not blank. It replaces every role's model.
    pub(crate) fn job_model(&self) -> Option<String> {
        let board = Board::load(&self.board_path).ok()?;
        let model = board.find(&self.job_id).ok()?.model.as_deref()?.trim();
        (!model.is_empty()).then(|| model.to_string())
    }

    /// The builder model this job runs on: the job's own model, else the escalated one, else the configured one.
    pub(crate) fn builder_model(&self) -> String {
        self.job_model()
            .or_else(|| self.escalated_model())
            .unwrap_or_else(|| self.config.builder_model.clone())
    }

    /// The stronger builder model the job escalated to, when set and not blank.
    fn escalated_model(&self) -> Option<String> {
        let board = Board::load(&self.board_path).ok()?;
        let model = board
            .find(&self.job_id)
            .ok()?
            .escalated_model
            .as_deref()?
            .trim();
        (!model.is_empty()).then(|| model.to_string())
    }

    /// The provider-specific part of a run spec: Codex gets its agent text and sandbox, Claude gets
    /// the cache env, its plugins and the subagent model. The probe shapes its specs the same way.
    pub(crate) fn shape_spec(&self, spec: &mut AgentSpec) -> Result<(), String> {
        if self.config.agent_cli() == "codex" {
            return self.prepare_for_codex(spec).map_err(|e| e.to_string());
        }
        if let Some(var) = cache_ttl_var(self.config.cache_ttl.as_deref()) {
            spec.env.push((var.0.to_string(), var.1.to_string()));
        }
        // Each role loads only its own plugins, so other skills stay out of the prompt.
        spec.plugins = Some(self.config.plugins_for(&spec.role).to_vec());
        // Helper agents run on the run's own model, so a skill cannot pick another one.
        if let Some(model) = spec.model.clone() {
            spec.env
                .push(("CLAUDE_CODE_SUBAGENT_MODEL".to_string(), model));
        }
        Ok(())
    }

    /// Runs one agent and adds its token usage to the job tally. The runner never fails, a failure is a stop reason.
    pub(crate) fn run_agent(&self, mut spec: AgentSpec) -> Result<AgentRun, EngineError> {
        self.sync_provider()?;
        if let Some(model) = self.job_model() {
            spec.model = Some(model);
        } else if spec.role == "builder" {
            // Only the builder escalates. The job's own model wins over it.
            if let Some(model) = self.escalated_model() {
                spec.model = Some(model);
            }
        }
        if let Err(e) = self.shape_spec(&mut spec) {
            return Ok(AgentRun::failed(&e));
        }
        // Every agent that runs in a job worktree finds the resolved agent files as resolved.
        if !self.agent_files.is_empty() {
            // The agent may start in a project folder, but the files live at the worktree root.
            if let Some(root) = self.repo.worktree_root_of(&spec.cwd) {
                self.write_agent_files(&root)?;
            }
        }
        tracing::info!(event = "agent.start", role = %spec.role, model = spec.model.as_deref().unwrap_or(""));
        let started = std::time::Instant::now();
        let run = self.runner.run(&spec);
        tracing::info!(
            event = "agent.end",
            role = %spec.role,
            model = spec.model.as_deref().unwrap_or(""),
            stop_reason = ?run.stop_reason,
            weighted_tokens = run.weighted,
            duration_ms = started.elapsed().as_millis() as u64,
            input = run.usage.input_tokens,
            cache_creation = run.usage.cache_creation_input_tokens,
            cache_read = run.usage.cache_read_input_tokens,
            output = run.usage.output_tokens,
            turns = run.turns,
            resumed = spec.resume.is_some(),
            round = self.job().map_or(1, |j| j.round),
            start_ctx = run.start_ctx,
            end_ctx = run.end_ctx,
            session_id = run.session_id.as_deref().unwrap_or("")
        );
        let u = run.usage;
        self.update(|j| {
            j.tokens.add(
                u.input_tokens,
                u.cache_creation_input_tokens,
                u.output_tokens,
                u.cache_read_input_tokens,
            );
            Ok(())
        })?;
        self.log_usage(&spec, &run, started.elapsed().as_secs());
        Ok(run)
    }

    /// Clears every saved session id once when the project's provider differs from the job's.
    /// A job with no provider counts as `claude`.
    #[doc(hidden)]
    pub fn sync_provider(&self) -> Result<(), EngineError> {
        if self.provider_synced.get() {
            return Ok(());
        }
        let want = self.config.agent_cli().to_string();
        let board = Board::load(&self.board_path).map_err(core_err)?;
        let stored = board.find(&self.job_id).map_err(core_err)?.provider.clone();
        if stored.as_deref() == Some(want.as_str()) {
            self.provider_synced.set(true);
            return Ok(());
        }
        let from = stored.as_deref().unwrap_or("claude").to_string();
        let switched = from != want;
        let mut cleared = None;
        self.update(|j| {
            if switched {
                j.task_session = None;
                j.builder_id = None;
                j.builder_session_model = None;
                j.builder_last = None;
                // The escalated model names the old provider's model, so it cannot run on the new one.
                cleared = j.escalated_model.take();
            }
            j.provider = Some(want.clone());
            Ok(())
        })?;
        if let Some(model) = cleared {
            self.ruling(&format!(
                "provider switched from {from} to {want}, cleared the escalated builder model {model}"
            ))?;
        }
        self.provider_synced.set(true);
        Ok(())
    }

    /// Appends one usage line per agent run to `usage.jsonl` in the job folder, for `factory tokens`.
    /// A write error is ignored, because the report is informational.
    fn log_usage(&self, spec: &AgentSpec, run: &AgentRun, secs: u64) {
        use std::io::Write;
        // A board read error falls back to round 1 and zero tasks, the line is still worth writing.
        let (round, tasks_done) = self.job().map_or((1, 0), |j| (j.round, j.task_index));
        let line = usage_line(
            &self.job_id,
            spec,
            run,
            round,
            tasks_done,
            now().max(0.0) as u64,
            secs,
            self.config.agent_cli(),
        );
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.job_dir.join("usage.jsonl"));
        if let Ok(mut f) = file {
            let _ = writeln!(f, "{line}");
        }
    }

    /// The job's weighted token tally from the board. A load error counts as zero, because `job()` already failed earlier in the same stage.
    pub(crate) fn job_weighted_tokens(&self) -> u64 {
        match Board::load(&self.board_path) {
            Ok(b) => b
                .find(&self.job_id)
                .map(|j| j.tokens.weighted())
                .unwrap_or(0),
            Err(_) => 0,
        }
    }
}

/// The `claude` environment variable for a `cache_ttl` setting. `FORCE_PROMPT_CACHING_5M` beats every other TTL setting.
fn cache_ttl_var(ttl: Option<&str>) -> Option<(&'static str, &'static str)> {
    match ttl {
        Some("5m") => Some(("FORCE_PROMPT_CACHING_5M", "1")),
        Some("1h") => Some(("ENABLE_PROMPT_CACHING_1H", "1")),
        _ => None,
    }
}

/// Builds one `usage.jsonl` line. `usage_v` 3 marks lines with the first-turn cache fields,
/// `usage_v` 4 marks lines with `end_ctx`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn usage_line(
    job_id: &str,
    spec: &AgentSpec,
    run: &AgentRun,
    round: u32,
    tasks_done: u32,
    ts: u64,
    secs: u64,
    provider: &str,
) -> serde_json::Value {
    let u = &run.usage;
    let mut line = serde_json::json!({
        "job": job_id,
        "role": spec.role,
        "input": u.input_tokens,
        "cache_creation": u.cache_creation_input_tokens,
        "output": u.output_tokens,
        "cache_read": u.cache_read_input_tokens,
        "cache_creation_1h": u.cache_creation_1h_input_tokens,
        "ts": ts,
        "secs": secs,
        "model": spec.model,
        "turns": run.turns,
        "resumed": spec.resume.is_some(),
        "stop": run.stop_reason.as_str(),
        "round": round,
        "start_ctx": run.start_ctx,
        "start_cc": run.start_cc,
        "start_cr": run.start_cr,
        "end_ctx": run.end_ctx,
        "usage_v": 4,
        "provider": provider,
    });
    if spec.role == "builder" {
        if let Some(obj) = line.as_object_mut() {
            obj.insert("tasks_done".to_string(), serde_json::json!(tasks_done));
        }
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use factory_agent::{AgentRun, AgentSpec, StopReason, Usage};

    fn run_with(stop: StopReason) -> AgentRun {
        AgentRun {
            usage: Usage {
                input_tokens: 1,
                cache_creation_input_tokens: 2,
                cache_read_input_tokens: 3,
                output_tokens: 4,
                ..Usage::default()
            },
            turns: 7,
            start_ctx: 55,
            start_cc: 40,
            start_cr: 15,
            end_ctx: 70,
            stop_reason: stop,
            ..AgentRun::done("x")
        }
    }

    #[test]
    fn usage_line_has_the_counters_and_the_run_details() {
        let mut spec = AgentSpec::new("001", "builder", "/w", "p");
        spec.model = Some("sonnet".to_string());
        spec.resume = Some("sess".to_string());
        let line = usage_line(
            "001",
            &spec,
            &run_with(StopReason::Done),
            2,
            1,
            1_700_000_000,
            0,
            "claude",
        );
        assert_eq!(line["job"], "001");
        assert_eq!(line["role"], "builder");
        assert_eq!(line["input"], 1);
        assert_eq!(line["cache_creation"], 2);
        assert_eq!(line["cache_read"], 3);
        assert_eq!(line["output"], 4);
        assert_eq!(line["ts"], 1_700_000_000u64);
        assert_eq!(line["model"], "sonnet");
        assert_eq!(line["turns"], 7);
        assert_eq!(line["resumed"], true);
        assert_eq!(line["stop"], "done");
        assert_eq!(line["round"], 2);
        assert_eq!(line["start_ctx"], 55);
        assert_eq!(line["usage_v"], 4);
        assert_eq!(line["tasks_done"], 1);
    }

    #[test]
    fn usage_line_carries_the_provider() {
        let spec = AgentSpec::new("001", "builder", "/w", "p");
        let run = run_with(StopReason::Done);
        assert_eq!(
            usage_line("001", &spec, &run, 1, 0, 1, 0, "claude")["provider"],
            "claude"
        );
        assert_eq!(
            usage_line("001", &spec, &run, 1, 0, 1, 0, "codex")["provider"],
            "codex"
        );
    }

    #[test]
    fn cache_ttl_picks_the_claude_variable() {
        assert_eq!(
            cache_ttl_var(Some("5m")),
            Some(("FORCE_PROMPT_CACHING_5M", "1"))
        );
        assert_eq!(
            cache_ttl_var(Some("1h")),
            Some(("ENABLE_PROMPT_CACHING_1H", "1"))
        );
        assert_eq!(cache_ttl_var(None), None);
    }

    #[test]
    fn the_one_hour_share_of_cache_writes_is_logged() {
        let mut run = run_with(StopReason::Done);
        run.usage.cache_creation_1h_input_tokens = 2;
        let spec = AgentSpec::new("001", "builder", "/w", "p");
        let line = usage_line("001", &spec, &run, 1, 0, 1, 0, "claude");
        assert_eq!(line["cache_creation_1h"], 2);
    }

    #[test]
    fn usage_line_has_the_run_duration() {
        let spec = AgentSpec::new("001", "builder", "/w", "p");
        let line = usage_line(
            "001",
            &spec,
            &run_with(StopReason::Done),
            1,
            0,
            1,
            42,
            "claude",
        );
        assert_eq!(line["secs"], 42);
        assert_eq!(line["role"], "builder");
    }

    #[test]
    fn tasks_done_is_written_for_builder_lines_only() {
        let spec = AgentSpec::new("001", "reviewer", "/w", "p");
        let line = usage_line(
            "001",
            &spec,
            &run_with(StopReason::Done),
            1,
            3,
            1,
            0,
            "claude",
        );
        assert!(line.get("tasks_done").is_none(), "{line}");
        assert!(line["model"].is_null());
        assert_eq!(line["resumed"], false);
    }

    #[test]
    fn a_killed_run_names_its_stop_reason() {
        let spec = AgentSpec::new("001", "builder", "/w", "p");
        let line = usage_line(
            "001",
            &spec,
            &run_with(StopReason::BudgetStop),
            1,
            0,
            1,
            0,
            "claude",
        );
        assert_eq!(line["stop"], "budget-stop");
    }

    #[test]
    fn a_failed_run_still_gets_a_line_with_zero_counters() {
        let spec = AgentSpec::new("001", "builder", "/w", "p");
        let line = usage_line(
            "001",
            &spec,
            &AgentRun::failed("boom"),
            1,
            0,
            1,
            0,
            "claude",
        );
        assert_eq!(line["stop"], "failed");
        assert_eq!(line["input"], 0);
        assert_eq!(line["output"], 0);
        assert_eq!(line["usage_v"], 4);
    }

    #[test]
    fn usage_line_carries_start_cc_and_start_cr() {
        let spec = AgentSpec::new("001", "builder", "/w", "p");
        let line = usage_line(
            "001",
            &spec,
            &run_with(StopReason::Done),
            1,
            0,
            10,
            5,
            "claude",
        );
        assert_eq!(line["start_cc"], 40);
        assert_eq!(line["start_cr"], 15);
        assert_eq!(line["usage_v"], 4);
    }

    #[test]
    fn usage_line_carries_end_ctx() {
        let spec = AgentSpec::new("001", "builder", "/w", "p");
        let line = usage_line(
            "001",
            &spec,
            &run_with(StopReason::Done),
            1,
            0,
            10,
            5,
            "claude",
        );
        assert_eq!(line["end_ctx"], 70);
        assert_eq!(line["usage_v"], 4);
    }

    #[test]
    fn a_failed_run_writes_zero_end_ctx() {
        let spec = AgentSpec::new("001", "builder", "/w", "p");
        let line = usage_line("001", &spec, &AgentRun::failed("x"), 1, 0, 1, 0, "claude");
        assert_eq!(line["end_ctx"], 0);
    }

    #[test]
    fn a_failed_run_still_writes_zero_start_fields() {
        let spec = AgentSpec::new("001", "builder", "/w", "p");
        let line = usage_line("001", &spec, &AgentRun::failed("x"), 1, 0, 1, 0, "claude");
        assert_eq!(line["start_cc"], 0);
        assert_eq!(line["start_cr"], 0);
    }
}
