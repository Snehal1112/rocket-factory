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

impl From<crate::LeaseError> for EngineError {
    fn from(e: crate::LeaseError) -> Self {
        EngineError::Msg(e.to_string())
    }
}

impl From<crate::SeriesError> for EngineError {
    fn from(e: crate::SeriesError) -> Self {
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
    lease_owner: Option<String>,
}

/// The marker that stops automatic KB refreshes, relative to the repo root.
pub const KB_STOP_FILE: &str = ".superpowers/factory/kb-stopped";

/// How long a lease lasts without a renewal.
pub const LEASE_TTL_SECS: f64 = 1800.0;

impl Driver {
    pub fn new(root: &Path, config: Config, runner_for: RunnerFactory) -> Result<Driver, EngineError> {
        let repo = Repo::open(root)?;
        Ok(Driver {
            root: root.to_path_buf(),
            repo,
            config,
            store: BoardStore::new(root),
            runner_for,
            lease_owner: None,
        })
    }

    /// Makes `run_job` take and renew a lease for `owner` on every job it drives.
    pub fn with_lease_owner(mut self, owner: &str) -> Driver {
        self.lease_owner = Some(owner.to_string());
        self
    }

    /// Takes or renews the lease when this driver has an owner. A live lease of another owner is an error.
    pub(crate) fn renew_lease(&self, id: &str) -> Result<(), EngineError> {
        if let Some(owner) = &self.lease_owner {
            crate::acquire_lease(&self.store, id, owner, LEASE_TTL_SECS, crate::now_secs())?;
        }
        Ok(())
    }

    /// Drops this driver's lease. A failed release is not fatal, because a lease expires by itself.
    pub(crate) fn release_lease(&self, id: &str) {
        if let Some(owner) = &self.lease_owner {
            let _ = crate::release_lease(&self.store, id, owner, crate::now_secs());
        }
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

    /// Asks the committer for the message of the merge (`Merge`) or the sync (`Sync`) commit of a job.
    /// Always returns a valid message, falling back to the fixed text.
    pub fn commit_message(&self, id: &str, kind: crate::MessageKind) -> Result<String, EngineError> {
        let job = self.job(id)?;
        let base = job.base.clone().unwrap_or_default();
        let tip = match kind {
            crate::MessageKind::Merge => job.head.clone().unwrap_or_default(),
            crate::MessageKind::Sync => self.repo.tip("main")?,
        };
        let cwd = job.worktree.as_ref().map(PathBuf::from).unwrap_or_else(|| self.root.clone());
        self.with_engine(id, |e| {
            e.message_step(&cwd, &crate::MessageRequest {
                kind,
                job_id: &job.id,
                slug: &job.slug,
                title: &job.feature,
                base: &base,
                tip: &tip,
                issues: &job.issues,
            })
        })
    }

    /// Refreshes the KB and writes the outcome to the job ledger. Returns a one-line summary.
    /// A manual run ignores the stop marker and clears it only when the run refreshed or was up to date.
    pub fn kb_refresh(&self, id: &str, manual: bool) -> Result<String, EngineError> {
        if self.config.kb_dir().is_none() {
            return Ok("kb_path is not set".to_string());
        }
        let job = self.job(id)?;
        let ledger = self.ledger(&job);
        let marker = self.root.join(KB_STOP_FILE);
        let say = |text: String| -> Result<String, EngineError> {
            // The ledger takes one line, so the text is folded into a single line.
            let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
            ledger.ruling(&text)?;
            Ok(text)
        };
        if !manual && marker.exists() {
            return say(format!("kb refresh skipped, stopped by an earlier change to the main checkout (see {KB_STOP_FILE})"));
        }
        let outcome = match self.with_engine(id, |e| e.kb_refresh_step(Some((&job.id, &job.slug)))) {
            Ok(o) => o,
            Err(e) => crate::KbOutcome::Failed(e.to_string()),
        };
        if manual && matches!(outcome, crate::KbOutcome::Refreshed { .. } | crate::KbOutcome::UpToDate) {
            // A missing marker is fine, so the result is ignored.
            let _ = std::fs::remove_file(&marker);
        }
        match outcome {
            crate::KbOutcome::Refreshed { pages } => say(format!("kb refreshed, pages: {}", pages.join(", "))),
            crate::KbOutcome::UpToDate => say("kb up to date".to_string()),
            crate::KbOutcome::Failed(r) => say(format!("kb refresh failed, {r}")),
            crate::KbOutcome::Tampered(r) => {
                let written = marker
                    .parent()
                    .map_or(Ok(()), std::fs::create_dir_all)
                    .and_then(|()| std::fs::write(&marker, format!("{r}\n")));
                let mut text = format!("kb refresh changed the main checkout, further refreshes stopped: {r}");
                if let Err(e) = written {
                    text.push_str(&format!(" (marker not written: {e})"));
                }
                say(text)
            }
        }
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
