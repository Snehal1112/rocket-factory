//! The multi-job driver that bridges to plan 04's single-job `Engine`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use factory_agent::{AgentRunner, EventSink};
use factory_core::{Config, CoreError, Job, Ledger};
use factory_git::{GitError, Repo};

use crate::{BoardStore, Engine, EngineError, ReviewOutcome, StageOutcome, StoreError};

impl From<CoreError> for EngineError {
    fn from(e: CoreError) -> Self {
        EngineError::Core(e.to_string())
    }
}
impl From<GitError> for EngineError {
    fn from(e: GitError) -> Self {
        EngineError::Git(e.to_string())
    }
}
impl From<std::io::Error> for EngineError {
    fn from(e: std::io::Error) -> Self {
        EngineError::Io(e.to_string())
    }
}
impl From<StoreError> for EngineError {
    fn from(e: StoreError) -> Self {
        EngineError::Msg(e.to_string())
    }
}

/// Builds the runner of one job. It is called with the job id every time a job starts a stage,
/// so a runner (and its event sink) never carries another job's id.
pub type RunnerFactory = Box<dyn Fn(&str) -> Arc<dyn AgentRunner>>;

/// Owns the repo, config and board store, and builds a plan 04 `Engine` per job.
pub struct Driver {
    root: PathBuf,
    repo: Repo,
    config: Config,
    store: BoardStore,
    runner_for: RunnerFactory,
}

impl Driver {
    pub fn new(root: &Path, config: Config, runner_for: RunnerFactory) -> Result<Driver, EngineError> {
        let repo = Repo::open(root)?;
        Ok(Driver { root: root.to_path_buf(), repo, config, store: BoardStore::new(root), runner_for })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn config(&self) -> &Config {
        &self.config
    }
    pub fn repo(&self) -> &Repo {
        &self.repo
    }
    pub fn store(&self) -> &BoardStore {
        &self.store
    }
    /// The path of `events.jsonl`, next to the board.
    pub fn events_path(&self) -> PathBuf {
        self.root.join(".superpowers/factory/events.jsonl")
    }

    pub fn job(&self, id: &str) -> Result<Job, EngineError> {
        let board = self.store.load()?;
        Ok(board.find(id)?.clone())
    }
    pub fn job_dir(&self, job: &Job) -> PathBuf {
        self.root.join(".superpowers/sdd").join(&job.dir)
    }
    pub fn ledger(&self, job: &Job) -> Ledger {
        Ledger::new(self.job_dir(job).join("progress.md"))
    }

    /// Builds the runner and event sink for this job and runs `f` on a per-job engine.
    fn with_engine<R>(
        &self,
        id: &str,
        f: impl FnOnce(&Engine) -> Result<R, EngineError>,
    ) -> Result<R, EngineError> {
        let job = self.job(id)?;
        let runner = (self.runner_for)(&job.id);
        let events = EventSink::new(self.events_path(), &job.id);
        let job_dir = self.job_dir(&job);
        let engine = Engine::new(
            self.store.board_path().to_path_buf(),
            job_dir.clone(),
            job.id.clone(),
            job_dir.join("plan.md"),
            &self.repo,
            runner.as_ref(),
            self.config.clone(),
            &events,
        )?;
        f(&engine)
    }

    pub fn build(&self, id: &str) -> Result<StageOutcome, EngineError> {
        self.with_engine(id, |e| e.build_stage())
    }
    pub fn checks(&self, id: &str) -> Result<StageOutcome, EngineError> {
        self.with_engine(id, |e| e.checks_stage())
    }
    pub fn review(&self, id: &str) -> Result<ReviewOutcome, EngineError> {
        self.with_engine(id, |e| e.review_stage())
    }
    pub fn apply(&self, id: &str, outcome: &ReviewOutcome) -> Result<StageOutcome, EngineError> {
        self.with_engine(id, |e| e.apply_decision(outcome))
    }
}
