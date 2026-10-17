//! The factory run loop stages: build, checks, review and decide.

mod build;
mod checks_stage;
mod decide;
mod park;
mod ports;
mod review;
mod lease;
mod lock;
mod project_lease;
mod runloop;
mod series;
mod specs;
mod clock;
mod commit;
mod compat;
mod costs;
mod kb;
mod merge;
mod message;
mod store;
mod sync;
mod tasks;
mod tick;
mod tipfiles;

use std::path::PathBuf;
use std::time::Duration;

use factory_agent::{AgentRunner, EventSink};
use factory_checks::validate_config;
use factory_core::{AgentFile, Config, Stage};
use factory_git::Repo;
use thiserror::Error;

pub use build::build_stage;
pub use clock::{date_from_days, now_secs, today};
pub use commit::{
    committer_prompt, committer_spec, CommitKind, CommitOutcome, CommitRequest, SnapshotRef, COMMITTER_AGENT, COMMITTER_MAX_TURNS, COMMITTER_TOOLS,
};
pub use kb::{kb_prompt, kb_spec, parse_kb_reply, KbOutcome, KB_AGENT};
pub use costs::{costs_path, estimate_for_plan, gather_costs, refresh_costs};
pub use compat::{Driver, ProjectGuard, RunnerFactory, HEARTBEAT_SECS, LEASE_TTL_SECS};
pub use message::{fixed_message, message_prompt, message_spec, MessageKind, MessageRequest, MESSAGE_TOOLS};
pub use merge::{merge_stage, park, MergeResult, MAX_SYNCS};
pub use lease::{acquire_lease, lease_is_live, release_lease, Lease, LeaseError};
pub use project_lease::{
    acquire_project_lease, force_release_project_lease, owner_pid, process_gone, project_lease_status, release_project_lease, LeaseTake, ProjectLease, ProjectLeaseError, ProjectLeaseStatus,
};
pub use lock::{lock_board, BoardLock};
pub use runloop::{create_job, create_job_with_spec, run_job, run_series, RunEnd};
pub use series::{parse_series, Series, SeriesError, SeriesPlan, MAX_PLANS};
pub use store::{next_id, BoardStore, StoreError, DEFAULT_LOCK_WAIT};
pub use sync::{sync_stage, SyncOutcome};
pub use tasks::{parse_tasks, plan_title, spec_goal, PlanTask};
pub use tick::tick_plan;
pub use tipfiles::RepoTip;
pub use checks_stage::checks_stage;
pub use decide::{decide, risk_summary, DecideInput, Decision};
pub use review::{apply_decision, final_checks, review_stage, ReviewOutcome};

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("core: {0}")]
    Core(String),
    #[error("git: {0}")]
    Git(String),
    #[error("agent: {0}")]
    Agent(String),
    #[error("checks: {0}")]
    Check(String),
    #[error("io: {0}")]
    Io(String),
    #[error("{0}")]
    Msg(String),
}

/// What a stage did to the job. The caller chains stages by the new board stage.
#[derive(Debug, Clone, PartialEq)]
pub enum StageOutcome {
    Advanced(Stage),
    NeedsHuman(String),
}

pub struct Engine<'a> {
    pub board_path: PathBuf,
    pub job_dir: PathBuf,
    pub job_id: String,
    pub plan_file: PathBuf,
    pub repo: &'a Repo,
    pub runner: &'a dyn AgentRunner,
    pub config: Config,
    pub events: &'a EventSink,
    pub lock_wait: Duration,
    pub commit_context: Option<PathBuf>,
    /// Agent files written into each new job worktree. Empty by default.
    pub agent_files: Vec<AgentFile>,
}

impl<'a> Engine<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        board_path: PathBuf,
        job_dir: PathBuf,
        job_id: String,
        plan_file: PathBuf,
        repo: &'a Repo,
        runner: &'a dyn AgentRunner,
        config: Config,
        events: &'a EventSink,
    ) -> Result<Self, EngineError> {
        validate_config(&config).map_err(|e| EngineError::Check(e.to_string()))?;
        Ok(Self {
            board_path,
            job_dir,
            job_id,
            plan_file,
            repo,
            runner,
            config,
            events,
            lock_wait: DEFAULT_LOCK_WAIT,
            commit_context: None,
            agent_files: Vec::new(),
        })
    }

    /// Changes how long board writes wait for the lock.
    pub fn with_lock_wait(mut self, wait: Duration) -> Self {
        self.lock_wait = wait;
        self
    }

    /// Sets the file the committer prompt text is read from. Without it the committer step is skipped.
    pub fn with_commit_context(mut self, path: PathBuf) -> Self {
        self.commit_context = Some(path);
        self
    }

    /// Sets the agent files written into each new job worktree.
    pub fn with_agent_files(mut self, files: Vec<AgentFile>) -> Self {
        self.agent_files = files;
        self
    }

    /// The commit context text, or the reason it cannot be read.
    pub(crate) fn commit_context_text(&self) -> Result<String, String> {
        match &self.commit_context {
            Some(path) => std::fs::read_to_string(path).map_err(|e| e.to_string()),
            None => Err("no commit context file is set".to_string()),
        }
    }
}
