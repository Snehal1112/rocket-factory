//! The multi-job driver that bridges to plan 04's single-job `Engine`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use factory_agent::{AgentRunner, EventSink};
use factory_core::{Config, CoreError, Job, Layout, Ledger};
use factory_git::{GitError, Repo};

use crate::{BoardStore, Engine, DEFAULT_LOCK_WAIT, EngineError, ReviewOutcome, StageOutcome, StoreError};

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

impl From<crate::ProjectLeaseError> for EngineError {
    fn from(e: crate::ProjectLeaseError) -> Self {
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
    layout: Layout,
    repo: Repo,
    config: Config,
    store: BoardStore,
    runner_for: RunnerFactory,
    lease_owner: Option<String>,
    lock_wait: Duration,
    agent_files: Vec<factory_core::AgentFile>,
    /// How many `ProjectGuard`s are alive. While one is, `run_job` keeps the project lease.
    project_holds: AtomicU32,
}

/// Holds the project lease until it is dropped. A driver with no lease owner gets a guard that does nothing.
#[must_use = "the project lease is released when the guard is dropped"]
pub struct ProjectGuard<'a> {
    driver: Option<&'a Driver>,
}

impl Drop for ProjectGuard<'_> {
    fn drop(&mut self) {
        if let Some(driver) = self.driver {
            if driver.project_holds.fetch_sub(1, Ordering::SeqCst) == 1 {
                driver.release_project();
            }
        }
    }
}

/// How long a lease lasts without a renewal.
pub const LEASE_TTL_SECS: f64 = 1800.0;

impl Driver {
    pub fn new(layout: Layout, config: Config, runner_for: RunnerFactory) -> Result<Driver, EngineError> {
        let repo = Repo::open(&layout.repo)?
            .with_base_branch(&config.base_branch)
            .with_worktrees_dir(layout.worktrees_dir.clone());
        let store = BoardStore::new(&layout.board_file(), DEFAULT_LOCK_WAIT);
        Ok(Driver { layout, repo, config, store, runner_for, lease_owner: None, lock_wait: DEFAULT_LOCK_WAIT, agent_files: Vec::new(), project_holds: AtomicU32::new(0) })
    }

    /// Changes how long board writes wait for the lock.
    pub fn with_lock_wait(mut self, wait: Duration) -> Driver {
        self.store = BoardStore::new(&self.layout.board_file(), wait);
        self.lock_wait = wait;
        self
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// Sets the agent files every engine of this driver writes into new job worktrees.
    pub fn with_agent_files(mut self, files: Vec<factory_core::AgentFile>) -> Driver {
        self.agent_files = files;
        self
    }

    /// Makes `run_job` take and renew a lease for `owner` on every job it drives.
    pub fn with_lease_owner(mut self, owner: &str) -> Driver {
        self.lease_owner = Some(owner.to_string());
        self
    }

    /// Takes or renews the project lease and the job lease when this driver has an owner.
    /// A live lease of another owner is an error. The project lease comes first, so a busy project
    /// leaves the job lease untouched.
    pub(crate) fn renew_lease(&self, id: &str) -> Result<(), EngineError> {
        if let Some(owner) = &self.lease_owner {
            self.take_project(id, owner)?;
            crate::acquire_lease(&self.store, id, owner, LEASE_TTL_SECS, crate::now_secs())?;
        }
        Ok(())
    }

    fn take_project(&self, id: &str, owner: &str) -> Result<crate::LeaseTake, EngineError> {
        Ok(crate::acquire_project_lease(
            &self.layout.state_dir,
            id,
            owner,
            LEASE_TTL_SECS,
            crate::now_secs(),
            self.lock_wait,
        )?)
    }

    fn release_project(&self) {
        if let Some(owner) = &self.lease_owner {
            // A failed release is not fatal, because the lease expires by itself.
            let _ = crate::release_project_lease(&self.layout.state_dir, owner, self.lock_wait);
        }
    }

    /// Takes the project lease for `job` and keeps it until the guard is dropped. Without a lease owner
    /// it takes nothing and returns a guard that does nothing.
    pub fn hold_project(&self, job: &str) -> Result<ProjectGuard<'_>, EngineError> {
        let Some(owner) = &self.lease_owner else {
            return Ok(ProjectGuard { driver: None });
        };
        self.take_project(job, owner)?;
        self.project_holds.fetch_add(1, Ordering::SeqCst);
        Ok(ProjectGuard { driver: Some(self) })
    }

    /// Drops this driver's lease. A failed release is not fatal, because a lease expires by itself.
    /// The project lease stays while a `ProjectGuard` is alive.
    pub(crate) fn release_lease(&self, id: &str) {
        if let Some(owner) = &self.lease_owner {
            let _ = crate::release_lease(&self.store, id, owner, crate::now_secs());
            if self.project_holds.load(Ordering::SeqCst) == 0 {
                self.release_project();
            }
        }
    }

    pub fn root(&self) -> &Path {
        &self.layout.repo
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
        self.layout.events_file()
    }

    pub fn job(&self, id: &str) -> Result<Job, EngineError> {
        let board = self.store.load()?;
        Ok(board.find(id)?.clone())
    }
    pub fn job_dir(&self, job: &Job) -> PathBuf {
        self.layout.job_dir(&job.dir)
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
        )?
        .with_lock_wait(self.lock_wait)
        .with_agent_files(self.agent_files.clone());
        let engine = if self.layout.commit_context_file.exists() {
            engine.with_commit_context(self.layout.commit_context_file.clone())
        } else {
            engine
        };
        f(&engine)
    }

    /// Asks the committer for the message of the merge (`Merge`) or the sync (`Sync`) commit of a job.
    /// Always returns a valid message, falling back to the fixed text.
    pub fn commit_message(&self, id: &str, kind: crate::MessageKind) -> Result<String, EngineError> {
        let job = self.job(id)?;
        let base = job.base.clone().unwrap_or_default();
        let tip = match kind {
            crate::MessageKind::Merge => job.head.clone().unwrap_or_default(),
            crate::MessageKind::Sync => self.repo.tip(self.repo.base_branch())?,
        };
        let cwd = job.worktree.as_ref().map(PathBuf::from).unwrap_or_else(|| self.layout.repo.clone());
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
        let marker = self.layout.kb_stop_file();
        let say = |text: String| -> Result<String, EngineError> {
            // The ledger takes one line, so the text is folded into a single line.
            let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
            ledger.ruling(&text)?;
            Ok(text)
        };
        if !manual && marker.exists() {
            return say(format!("kb refresh skipped, stopped by an earlier change to the main checkout (see {})", marker.display()));
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
