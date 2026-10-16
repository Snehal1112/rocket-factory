use std::fmt::Display;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

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
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
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
        })
    }

    /// Loads the board, applies one change, and saves it atomically.
    pub(crate) fn update(&self, f: impl FnOnce(&mut Job) -> Result<(), CoreError>) -> Result<(), EngineError> {
        // The store takes the board lock, so every stage write is serialized with the other writers.
        crate::BoardStore::at(&self.board_path).update_job(&self.job_id, f).map_err(core_err)
    }

    /// Moves the job to a stage. Only a fix round (review back to build) passes `force`.
    pub(crate) fn set_stage(&self, stage: Stage, reason: Option<&str>, force: bool) -> Result<(), EngineError> {
        self.update(|j| j.set_stage(stage, reason, force, now()))
    }

    fn ledger_file(&self) -> Ledger {
        Ledger::new(self.job_dir.join("progress.md"))
    }

    /// Appends a strict `Task N: <state> (detail)` line.
    pub(crate) fn ledger_task(&self, n: u32, state: TaskState, detail: &str) -> Result<(), EngineError> {
        self.ledger_file().task(n, state, Some(detail)).map_err(core_err)
    }

    /// Appends a `Ruling:` line. The ledger takes one line, so whitespace and newlines fold into single spaces.
    pub(crate) fn ruling(&self, text: &str) -> Result<(), EngineError> {
        let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
        self.ledger_file().ruling(&one_line).map_err(core_err)
    }

    /// Parks the job for the human with a reason, and tells the caller.
    pub(crate) fn park(&self, reason: String) -> Result<crate::StageOutcome, EngineError> {
        self.set_stage(Stage::NeedsHuman, Some(&reason), false)?;
        self.ruling(&format!("needs-human, {reason}"))?;
        Ok(crate::StageOutcome::NeedsHuman(reason))
    }

    /// Runs one agent and adds its token usage to the job tally. The runner never fails, a failure is a stop reason.
    pub(crate) fn run_agent(&self, spec: AgentSpec) -> Result<AgentRun, EngineError> {
        let run = self.runner.run(&spec);
        let u = run.usage;
        self.update(|j| {
            j.tokens.add(u.input_tokens, u.cache_creation_input_tokens, u.output_tokens, u.cache_read_input_tokens);
            Ok(())
        })?;
        self.log_usage(&spec, &run);
        Ok(run)
    }

    /// Appends one usage line per agent run to `usage.jsonl` in the job folder, for `factory tokens`.
    /// A write error is ignored, because the report is informational.
    fn log_usage(&self, spec: &AgentSpec, run: &AgentRun) {
        use std::io::Write;
        // A board read error falls back to round 1 and zero tasks, the line is still worth writing.
        let (round, tasks_done) = self.job().map_or((1, 0), |j| (j.round, j.task_index));
        let line = usage_line(&self.job_id, spec, run, round, tasks_done, now().max(0.0) as u64);
        let file = std::fs::OpenOptions::new().create(true).append(true).open(self.job_dir.join("usage.jsonl"));
        if let Ok(mut f) = file {
            let _ = writeln!(f, "{line}");
        }
    }

    /// The job's weighted token tally from the board. A load error counts as zero, because `job()` already failed earlier in the same stage.
    pub(crate) fn job_weighted_tokens(&self) -> u64 {
        match Board::load(&self.board_path) {
            Ok(b) => b.find(&self.job_id).map(|j| j.tokens.weighted()).unwrap_or(0),
            Err(_) => 0,
        }
    }
}

/// Builds one `usage.jsonl` line. `usage_v` 2 marks lines with the corrected output count.
pub(crate) fn usage_line(job_id: &str, spec: &AgentSpec, run: &AgentRun, round: u32, tasks_done: u32, ts: u64) -> serde_json::Value {
    let u = &run.usage;
    let mut line = serde_json::json!({
        "job": job_id,
        "role": spec.role,
        "input": u.input_tokens,
        "cache_creation": u.cache_creation_input_tokens,
        "output": u.output_tokens,
        "cache_read": u.cache_read_input_tokens,
        "ts": ts,
        "model": spec.model,
        "turns": run.turns,
        "resumed": spec.resume.is_some(),
        "stop": run.stop_reason.as_str(),
        "round": round,
        "start_ctx": run.start_ctx,
        "usage_v": 2,
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
            usage: Usage { input_tokens: 1, cache_creation_input_tokens: 2, cache_read_input_tokens: 3, output_tokens: 4 },
            turns: 7,
            start_ctx: 55,
            stop_reason: stop,
            ..AgentRun::done("x")
        }
    }

    #[test]
    fn usage_line_has_the_counters_and_the_run_details() {
        let mut spec = AgentSpec::new("001", "builder", "/w", "p");
        spec.model = Some("sonnet".to_string());
        spec.resume = Some("sess".to_string());
        let line = usage_line("001", &spec, &run_with(StopReason::Done), 2, 1, 1_700_000_000);
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
        assert_eq!(line["usage_v"], 2);
        assert_eq!(line["tasks_done"], 1);
    }

    #[test]
    fn tasks_done_is_written_for_builder_lines_only() {
        let spec = AgentSpec::new("001", "reviewer", "/w", "p");
        let line = usage_line("001", &spec, &run_with(StopReason::Done), 1, 3, 1);
        assert!(line.get("tasks_done").is_none(), "{line}");
        assert!(line["model"].is_null());
        assert_eq!(line["resumed"], false);
    }

    #[test]
    fn a_killed_run_names_its_stop_reason() {
        let spec = AgentSpec::new("001", "builder", "/w", "p");
        let line = usage_line("001", &spec, &run_with(StopReason::BudgetStop), 1, 0, 1);
        assert_eq!(line["stop"], "budget-stop");
    }

    #[test]
    fn a_failed_run_still_gets_a_line_with_zero_counters() {
        let spec = AgentSpec::new("001", "builder", "/w", "p");
        let line = usage_line("001", &spec, &AgentRun::failed("boom"), 1, 0, 1);
        assert_eq!(line["stop"], "failed");
        assert_eq!(line["input"], 0);
        assert_eq!(line["output"], 0);
        assert_eq!(line["usage_v"], 2);
    }
}
