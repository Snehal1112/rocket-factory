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
        Ok(run)
    }

    /// The job's weighted token tally from the board. A load error counts as zero, because `job()` already failed earlier in the same stage.
    pub(crate) fn job_weighted_tokens(&self) -> u64 {
        match Board::load(&self.board_path) {
            Ok(b) => b.find(&self.job_id).map(|j| j.tokens.weighted()).unwrap_or(0),
            Err(_) => 0,
        }
    }
}
