//! The multi-job driver that bridges to plan 04's single-job `Engine`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use factory_agent::{AgentRunner, EventSink};
use factory_core::{Config, CoreError, Job, Layout, Ledger};
use factory_git::{GitError, Repo};

use crate::{
    BoardStore, Engine, EngineError, ReviewOutcome, StageOutcome, StoreError, DEFAULT_LOCK_WAIT,
};

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
        match e {
            crate::ProjectLeaseError::Held { .. } => EngineError::ProjectBusy(e.to_string()),
            other => EngineError::Msg(other.to_string()),
        }
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
    /// How long a merge waits for the repository merge lock.
    merge_lock_wait: Duration,
    agent_files: Vec<factory_core::AgentFile>,
    /// The user-wide folders Codex lists skills from.
    codex_skill_roots: Vec<PathBuf>,
    /// How many `ProjectGuard`s are alive. While one is, `run_job` keeps the project lease.
    project_holds: AtomicU32,
    /// How long a lease lasts without a renewal.
    lease_ttl: f64,
    /// How often the heartbeat renews the leases.
    heartbeat: Duration,
    /// The jobs the heartbeat renews. The heartbeat thread gets a clone of this, never the driver.
    shared: Arc<Mutex<Held>>,
    /// Set by the heartbeat when another owner holds a lease.
    lost: Arc<AtomicBool>,
    /// The running heartbeat, one for all nested guards.
    beat: Mutex<Option<Heartbeat>>,
    tracker: Box<dyn crate::Tracker>,
}

/// What the heartbeat renews right now.
#[derive(Default)]
struct Held {
    /// The job named in the project lease.
    project_job: String,
    /// The job whose board lease is renewed. None while no job runs.
    job_lease: Option<String>,
}

/// The thread that renews the leases while a guard is alive.
struct Heartbeat {
    stop: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

impl Heartbeat {
    /// Stops the thread and waits for it. It wakes at once, so this returns quickly.
    fn finish(self) {
        self.stop.store(true, Ordering::SeqCst);
        self.handle.thread().unpark();
        // A panic in the thread is not an error of the run.
        let _ = self.handle.join();
    }
}

fn lock_held(m: &Mutex<Held>) -> MutexGuard<'_, Held> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Holds the project lease until it is dropped. A driver with no lease owner gets a guard that does nothing.
#[must_use = "the project lease is released when the guard is dropped"]
pub struct ProjectGuard<'a> {
    driver: Option<&'a Driver>,
}

impl ProjectGuard<'_> {
    /// True when the heartbeat found another owner holding a lease. The run must stop.
    pub fn lost(&self) -> bool {
        self.driver.is_some_and(|d| d.lease_lost())
    }
}

impl Drop for ProjectGuard<'_> {
    fn drop(&mut self) {
        if let Some(driver) = self.driver {
            if driver.project_holds.fetch_sub(1, Ordering::SeqCst) == 1 {
                // The thread ends first, so it cannot write the lease again after the release.
                let beat = driver.beat.lock().unwrap_or_else(|e| e.into_inner()).take();
                if let Some(beat) = beat {
                    beat.finish();
                }
                driver.release_project();
            }
        }
    }
}

/// How long a lease lasts without a renewal.
pub const LEASE_TTL_SECS: f64 = 1800.0;

/// How often the heartbeat renews the leases by default.
pub const HEARTBEAT_SECS: u64 = 60;

/// The heartbeat loop. It uses only cloned values and its own board store.
struct BeatCtx {
    state_dir: PathBuf,
    board_file: PathBuf,
    owner: String,
    ttl: f64,
    wait: Duration,
    heartbeat: Duration,
    shared: Arc<Mutex<Held>>,
    lost: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}

impl BeatCtx {
    fn run(self) {
        let store = BoardStore::new(&self.board_file, self.wait);
        loop {
            let deadline = Instant::now() + self.heartbeat;
            while !self.stop.load(Ordering::SeqCst) {
                let now = Instant::now();
                if now >= deadline {
                    break;
                }
                std::thread::park_timeout(deadline - now);
            }
            if self.stop.load(Ordering::SeqCst) {
                return;
            }
            // The lock keeps a release of the job lease from racing with a renewal.
            let held = lock_held(&self.shared);
            let at = crate::now_secs();
            let project = crate::acquire_project_lease(
                &self.state_dir,
                &held.project_job,
                &self.owner,
                self.ttl,
                at,
                self.wait,
            );
            if matches!(project, Err(crate::ProjectLeaseError::Held { .. })) {
                self.lost.store(true, Ordering::SeqCst);
                return;
            }
            if let Some(job) = &held.job_lease {
                let lease = crate::acquire_lease(&store, job, &self.owner, self.ttl, at);
                if matches!(lease, Err(crate::LeaseError::Held { .. })) {
                    self.lost.store(true, Ordering::SeqCst);
                    return;
                }
            }
            // Any other error is a busy lock or a full disk. The next beat tries again.
        }
    }
}

impl Driver {
    pub fn new(
        layout: Layout,
        config: Config,
        runner_for: RunnerFactory,
    ) -> Result<Driver, EngineError> {
        let mut repo = Repo::open(&layout.repo)?
            .with_base_branch(&config.base_branch)
            .with_worktrees_dir(layout.worktrees_dir.clone());
        if let Some(scope) = &layout.scope {
            repo = repo.with_scope(&scope.project, &scope.subpath);
        }
        let store = BoardStore::new(&layout.board_file(), DEFAULT_LOCK_WAIT);
        let tracker: Box<dyn crate::Tracker> = Box::new(crate::GhTracker::new(&layout.repo));
        Ok(Driver {
            layout,
            repo,
            config,
            store,
            runner_for,
            lease_owner: None,
            lock_wait: DEFAULT_LOCK_WAIT,
            merge_lock_wait: crate::MERGE_LOCK_WAIT,
            agent_files: Vec::new(),
            codex_skill_roots: Vec::new(),
            project_holds: AtomicU32::new(0),
            lease_ttl: LEASE_TTL_SECS,
            heartbeat: Duration::from_secs(HEARTBEAT_SECS),
            shared: Arc::new(Mutex::new(Held::default())),
            lost: Arc::new(AtomicBool::new(false)),
            beat: Mutex::new(None),
            tracker,
        })
    }

    /// Replaces the issue tracker. Tests pass a fake.
    pub fn with_tracker(mut self, tracker: Box<dyn crate::Tracker>) -> Driver {
        self.tracker = tracker;
        self
    }

    pub fn tracker(&self) -> &dyn crate::Tracker {
        self.tracker.as_ref()
    }

    /// Changes how long board writes wait for the lock.
    pub fn with_lock_wait(mut self, wait: Duration) -> Driver {
        self.store = BoardStore::new(&self.layout.board_file(), wait);
        self.lock_wait = wait;
        self
    }

    /// Changes how long a merge waits for the repository merge lock.
    pub fn set_merge_lock_wait(&mut self, wait: Duration) {
        self.merge_lock_wait = wait;
    }

    pub(crate) fn merge_lock_wait(&self) -> Duration {
        self.merge_lock_wait
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// Sets the agent files every engine of this driver writes into new job worktrees.
    pub fn with_agent_files(mut self, files: Vec<factory_core::AgentFile>) -> Driver {
        self.agent_files = files;
        self
    }

    /// Sets the user-wide skill folders every engine of this driver passes to Codex runs.
    pub fn with_codex_skill_roots(mut self, roots: Vec<PathBuf>) -> Driver {
        self.codex_skill_roots = roots;
        self
    }

    /// Makes `run_job` take and renew a lease for `owner` on every job it drives.
    pub fn with_lease_owner(mut self, owner: &str) -> Driver {
        self.lease_owner = Some(owner.to_string());
        self
    }

    /// Sets how long a lease lasts and how often the heartbeat renews it. Meant for tests.
    pub fn with_lease_timing(mut self, ttl: f64, heartbeat: Duration) -> Driver {
        self.lease_ttl = ttl;
        self.heartbeat = heartbeat;
        self
    }

    /// True when the heartbeat found another owner holding a lease.
    pub fn lease_lost(&self) -> bool {
        self.lost.load(Ordering::SeqCst)
    }

    /// Takes or renews the project lease and the job lease when this driver has an owner.
    /// A live lease of another owner is an error. The project lease comes first, so a busy project
    /// leaves the job lease untouched.
    pub fn renew_lease(&self, id: &str) -> Result<(), EngineError> {
        if let Some(owner) = &self.lease_owner {
            let mut held = lock_held(&self.shared);
            self.take_project(id, owner)?;
            held.project_job = id.to_string();
            crate::acquire_lease(&self.store, id, owner, self.lease_ttl, crate::now_secs())?;
            held.job_lease = Some(id.to_string());
        }
        Ok(())
    }

    fn take_project(&self, id: &str, owner: &str) -> Result<crate::LeaseTake, EngineError> {
        Ok(crate::acquire_project_lease(
            &self.layout.state_dir,
            id,
            owner,
            self.lease_ttl,
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
        // The mutex covers the take and the job name, so the heartbeat never rewrites the lease with an old job.
        let mut held = lock_held(&self.shared);
        self.take_project(job, owner)?;
        held.project_job = job.to_string();
        drop(held);
        if self.project_holds.fetch_add(1, Ordering::SeqCst) == 0 {
            self.lost.store(false, Ordering::SeqCst);
            if let Err(e) = self.start_heartbeat(owner) {
                // No guard exists yet, so the hold is undone here.
                self.project_holds.fetch_sub(1, Ordering::SeqCst);
                self.release_project();
                return Err(e);
            }
        }
        Ok(ProjectGuard { driver: Some(self) })
    }

    fn start_heartbeat(&self, owner: &str) -> Result<(), EngineError> {
        let stop = Arc::new(AtomicBool::new(false));
        let ctx = BeatCtx {
            state_dir: self.layout.state_dir.clone(),
            board_file: self.layout.board_file(),
            owner: owner.to_string(),
            ttl: self.lease_ttl,
            wait: self.lock_wait,
            heartbeat: self.heartbeat,
            shared: self.shared.clone(),
            lost: self.lost.clone(),
            stop: stop.clone(),
        };
        // Without the thread the lease would expire during a long step, so a failed start is an error.
        let handle = std::thread::Builder::new()
            .name("lease-heartbeat".into())
            .spawn(move || ctx.run())
            .map_err(|e| EngineError::Msg(format!("cannot start the lease heartbeat: {e}")))?;
        *self.beat.lock().unwrap_or_else(|e| e.into_inner()) = Some(Heartbeat { stop, handle });
        Ok(())
    }

    /// Drops this driver's lease. A failed release is not fatal, because a lease expires by itself.
    /// The project lease stays while a `ProjectGuard` is alive.
    pub fn release_lease(&self, id: &str) {
        if let Some(owner) = &self.lease_owner {
            let mut held = lock_held(&self.shared);
            held.job_lease = None;
            let _ = crate::release_lease(&self.store, id, owner, crate::now_secs());
            drop(held);
            if self.project_holds.load(Ordering::SeqCst) == 0 {
                self.release_project();
            }
        }
    }

    pub fn root(&self) -> &Path {
        &self.layout.repo
    }
    /// The agent files the driver writes into checkouts.
    pub(crate) fn agent_files(&self) -> &[factory_core::AgentFile] {
        &self.agent_files
    }
    /// Runs one agent that belongs to no job yet, through the runner made for `id`.
    pub(crate) fn run_agent_for(
        &self,
        id: &str,
        spec: &factory_agent::AgentSpec,
    ) -> factory_agent::AgentRun {
        (self.runner_for)(id).run(spec)
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

    /// Runs the live marker probe with `runner` and saves the report under the probes folder.
    /// The probe has no job, so its engine works in a scratch folder next to the reports.
    pub fn probe(
        &self,
        runner: &dyn AgentRunner,
        harness: factory_core::Harness,
        roles: &[factory_core::ProbeRole],
        delivery: crate::Delivery,
        cli_version: &str,
        stamp: &str,
    ) -> Result<factory_core::ProbeReport, EngineError> {
        let probes_dir = self.layout.probes_dir();
        let scratch = probes_dir.join("work");
        let events = EventSink::null("probe");
        // The flag overrides the project's agent CLI for this run only.
        let mut config = self.config.clone();
        config.agent_cli = Some(harness.as_str().to_string());
        let engine = Engine::new(
            self.store.board_path().to_path_buf(),
            scratch.clone(),
            "probe".to_string(),
            scratch.join("plan.md"),
            &self.repo,
            runner,
            config,
            &events,
        )?
        .with_agent_files(self.agent_files.clone())
        .with_codex_skill_roots(self.codex_skill_roots.clone());
        crate::run_probe(
            &engine,
            harness,
            roles,
            delivery,
            cli_version,
            &probes_dir,
            stamp,
        )
    }

    /// Builds the runner and event sink for this job and runs `f` on a per-job engine.
    pub(crate) fn with_engine<R>(
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
        .with_agent_files(self.agent_files.clone())
        .with_codex_skill_roots(self.codex_skill_roots.clone());
        let engine = if self.layout.commit_context_file.exists() {
            engine.with_commit_context(self.layout.commit_context_file.clone())
        } else {
            engine
        };
        f(&engine)
    }

    /// Asks the committer for the message of the merge (`Merge`) or the sync (`Sync`) commit of a job.
    /// Always returns a valid message, falling back to the fixed text.
    pub fn commit_message(
        &self,
        id: &str,
        kind: crate::MessageKind,
    ) -> Result<String, EngineError> {
        let job = self.job(id)?;
        let base = job.base.clone().unwrap_or_default();
        let tip = match kind {
            crate::MessageKind::Merge => job.head.clone().unwrap_or_default(),
            crate::MessageKind::Sync => self.repo.tip(self.repo.base_branch())?,
        };
        let cwd = job
            .worktree
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| self.layout.repo.clone());
        self.with_engine(id, |e| {
            e.message_step(
                &cwd,
                &crate::MessageRequest {
                    kind,
                    job_id: &job.id,
                    slug: &job.slug,
                    title: &job.feature,
                    base: &base,
                    tip: &tip,
                    issues: &job.issues,
                    partial: crate::message::spec_has_follow_ups(&self.job_dir(&job)),
                },
            )
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
            return say(format!(
                "kb refresh skipped, stopped by an earlier change to the main checkout (see {})",
                marker.display()
            ));
        }
        let outcome = match self.with_engine(id, |e| e.kb_refresh_step(Some((&job.id, &job.slug))))
        {
            Ok(o) => o,
            Err(e) => crate::KbOutcome::Failed(e.to_string()),
        };
        if manual
            && matches!(
                outcome,
                crate::KbOutcome::Refreshed { .. } | crate::KbOutcome::UpToDate
            )
        {
            // A missing marker is fine, so the result is ignored.
            let _ = std::fs::remove_file(&marker);
        }
        match outcome {
            crate::KbOutcome::Refreshed { pages } => {
                say(format!("kb refreshed, pages: {}", pages.join(", ")))
            }
            crate::KbOutcome::UpToDate => say("kb up to date".to_string()),
            crate::KbOutcome::Failed(r) => say(format!("kb refresh failed, {r}")),
            crate::KbOutcome::Tampered(r) => {
                let written = marker
                    .parent()
                    .map_or(Ok(()), std::fs::create_dir_all)
                    .and_then(|()| std::fs::write(&marker, format!("{r}\n")));
                let mut text =
                    format!("kb refresh changed the main checkout, further refreshes stopped: {r}");
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
    pub fn review_sync(&self, id: &str) -> Result<ReviewOutcome, EngineError> {
        self.with_engine(id, |e| e.review_stage_as(crate::ReviewRun::Sync))
    }
    pub fn apply(&self, id: &str, outcome: &ReviewOutcome) -> Result<StageOutcome, EngineError> {
        self.with_engine(id, |e| e.apply_decision(outcome))
    }
}
