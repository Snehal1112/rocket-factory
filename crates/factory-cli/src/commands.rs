use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use factory_agent::{AgentRunner, Caps, ClaudeRunner, EventSink};
use factory_core::{
    outcome_lines, resolve, AgentFile, Config, CostsFile, Home, Layout, ParkRecord, Stage,
};
use factory_engine::{
    costs_path, create_job_with_spec, create_one_job, find_draft, force_release_project_lease,
    gather_costs, lease_is_live, merge_stage, now_secs, project_lease_status, remove_draft,
    run_job, run_series, set_draft_stage, start_draft, today, Driver, MergeResult, RepoTip, RunEnd,
    DEFAULT_LOCK_WAIT,
};

use crate::cli::{Cli, Command, SpecCmd};
use crate::costs::{estimate_lines, render_costs};
use crate::status::{render_parked, render_project_line, render_status};

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("{0}")]
    Refused(String),
    #[error("{0}")]
    Engine(#[from] factory_engine::EngineError),
    #[error("{0}")]
    Store(#[from] factory_engine::StoreError),
    #[error("{0}")]
    Lease(#[from] factory_engine::LeaseError),
    #[error("{0}")]
    Git(#[from] factory_git::GitError),
    #[error("{0}")]
    Core(#[from] factory_core::CoreError),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// What a command needs: the folder layout, an optional injected runner (tests), the lease owner name
/// and how long board writes wait for the lock.
#[derive(Clone)]
pub struct CliCtx {
    pub layout: Layout,
    pub runner: Option<Arc<dyn AgentRunner>>,
    pub owner: String,
    pub lock_wait: Duration,
    /// The resolved settings of a project. `None` means the settings come from the layout's config file.
    pub config: Option<Config>,
    /// The agent files of a project, written into each job worktree.
    pub agents: Vec<AgentFile>,
    /// Why a project may not start agents. Read-only commands still work.
    pub blocked: Option<String>,
    /// Things the resolver left out without failing, printed by the binary at start-up.
    pub notes: Vec<String>,
    /// An injected issue tracker. `None` means the real `gh` one. Tests use it.
    pub tracker: Option<Arc<dyn factory_engine::Tracker>>,
}

impl CliCtx {
    /// A context with an injected runner and the default layout inside `root`, used by tests.
    pub fn new(root: PathBuf, runner: Arc<dyn AgentRunner>, owner: &str) -> CliCtx {
        CliCtx::with_layout(Layout::in_repo(&root), runner, owner)
    }

    /// A context with an injected runner and an explicit layout.
    pub fn with_layout(layout: Layout, runner: Arc<dyn AgentRunner>, owner: &str) -> CliCtx {
        CliCtx {
            layout,
            runner: Some(runner),
            owner: owner.to_string(),
            lock_wait: DEFAULT_LOCK_WAIT,
            config: None,
            agents: Vec::new(),
            blocked: None,
            notes: Vec::new(),
            tracker: None,
        }
    }

    /// A context whose driver uses this tracker instead of `gh`. Tests use it.
    pub fn with_tracker(mut self, tracker: Arc<dyn factory_engine::Tracker>) -> CliCtx {
        self.tracker = Some(tracker);
        self
    }

    /// The main checkout.
    pub fn root(&self) -> &std::path::Path {
        &self.layout.repo
    }

    /// A context for the real binary. The root is the main checkout of the current directory.
    /// `FACTORY_LOCK_WAIT` (seconds) is read here, once.
    pub fn from_env(root: Option<PathBuf>) -> Result<CliCtx, CliError> {
        let root = match root {
            Some(r) => r,
            None => {
                let cwd = std::env::current_dir()?;
                factory_git::Repo::open(&cwd)?.root().to_path_buf()
            }
        };
        Ok(CliCtx {
            layout: Layout::in_repo(&root),
            runner: None,
            owner: env_owner(),
            lock_wait: env_lock_wait(),
            config: None,
            agents: Vec::new(),
            blocked: None,
            notes: Vec::new(),
            tracker: None,
        })
    }

    /// A context for a registered project. The settings come from `resolve`, never from the layout's
    /// config file (for a project that file is `project.toml`), which this function never reads.
    pub fn for_project(
        home: &Home,
        name: &str,
        runner: Option<Arc<dyn AgentRunner>>,
        owner: &str,
        lock_wait: Duration,
    ) -> Result<CliCtx, CliError> {
        let file = factory_core::project::get(home, name)?;
        if !file.repo.is_dir() {
            return Err(CliError::Refused(format!(
                "repo path is missing: {}",
                file.repo.display()
            )));
        }
        let strict = crate::project::base_of_strict(&file);
        let base = strict
            .clone()
            .unwrap_or_else(|| crate::project::base_of(&file));
        let repo = crate::project::open_repo(&file.repo, &base).map_err(CliError::Refused)?;
        let resolved = resolve(home, name, &RepoTip(&repo))?;
        let mut blocked = None;
        if resolved.config.degraded {
            let first = resolved
                .problems
                .first()
                .cloned()
                .unwrap_or_else(|| "the settings failed closed".to_string());
            blocked = Some(match resolved.problems.len() {
                0 | 1 => first,
                n => format!("{first} (and {} more)", n - 1),
            });
        } else if let Err(e) = factory_checks::validate_config(&resolved.config) {
            blocked = Some(format!(
                "a check command would be rejected when a job starts: {e}"
            ));
        }
        if blocked.is_none() && strict.is_none() {
            blocked = Some(format!(
                "project {name} has no valid base_branch setting in project.toml"
            ));
        }
        let ctx = CliCtx {
            layout: resolved.layout,
            runner,
            owner: owner.to_string(),
            lock_wait,
            config: Some(resolved.config),
            agents: resolved.agents,
            blocked,
            notes: resolved.notes,
            tracker: None,
        };
        Ok(ctx)
    }

    /// The file log level: the resolved project level, else the layout config file, else `info`.
    pub fn log_level(&self) -> String {
        match &self.config {
            Some(c) => c.log_level_or_default().to_string(),
            None => std::fs::read_to_string(&self.layout.config_file)
                .ok()
                .and_then(|text| Config::parse(&text).ok())
                .map(|c| c.log_level_or_default().to_string())
                .unwrap_or_else(|| "info".to_string()),
        }
    }

    /// Where the CLI writes its log.
    pub fn log_target(&self, project: Option<&str>) -> factory_telemetry::LogTarget {
        factory_telemetry::LogTarget {
            path: self.layout.log_file(),
            project: project.map(str::to_string),
            scope: factory_telemetry::Scope::Cli,
        }
    }

    /// Refuses commands that start agents or merge when the project is blocked.
    pub fn runnable(&self) -> Result<(), CliError> {
        match &self.blocked {
            Some(reason) => Err(CliError::Refused(format!("project is blocked: {reason}"))),
            None => Ok(()),
        }
    }

    /// The runner of one job. A real runner gets an event sink stamped with that job's id,
    /// so a series never writes the first job's id into later jobs' events.
    pub fn runner_for(&self, job_id: &str) -> Arc<dyn AgentRunner> {
        match &self.runner {
            Some(r) => r.clone(),
            None => {
                let sink = EventSink::new(self.layout.events_file(), job_id);
                Arc::new(ClaudeRunner::new("claude", Caps::default(), sink))
            }
        }
    }

    fn driver(&self) -> Result<Driver, CliError> {
        let config = match &self.config {
            Some(c) => c.clone(),
            None => Config::load(&self.layout.config_file),
        };
        let me = self.clone();
        let driver = Driver::new(
            self.layout.clone(),
            config,
            Box::new(move |id| me.runner_for(id)),
        )?;
        let driver = driver
            .with_lock_wait(self.lock_wait)
            .with_lease_owner(&self.owner);
        let driver = if self.agents.is_empty() {
            driver
        } else {
            driver.with_agent_files(self.agents.clone())
        };
        Ok(match &self.tracker {
            Some(t) => driver.with_tracker(Box::new(t.clone())),
            None => driver,
        })
    }
}

/// A lease owner name that is unique per process start.
pub fn env_owner() -> String {
    format!("driver-{}-{}", std::process::id(), now_secs() as u64)
}

/// `FACTORY_LOCK_WAIT` (seconds), or the default.
pub fn env_lock_wait() -> Duration {
    std::env::var("FACTORY_LOCK_WAIT")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_LOCK_WAIT)
}

pub fn validate_rework_note(note: &str) -> Result<(), CliError> {
    if note.trim().is_empty() {
        return Err(CliError::Refused("the rework note is empty".into()));
    }
    if note.contains('\n') || note.contains('\r') {
        return Err(CliError::Refused("the rework note must be one line".into()));
    }
    if note.chars().count() > 200 {
        return Err(CliError::Refused(
            "the rework note is longer than 200 characters".into(),
        ));
    }
    Ok(())
}

/// Stops when the home was last used by a newer factory than this binary.
pub fn check_home(home: &factory_core::Home) -> Result<(), CliError> {
    factory_core::version::check(home, factory_core::version::FACTORY_VERSION)?;
    Ok(())
}

/// Opens the factory home from the environment and checks its version marker.
pub fn open_home() -> Result<factory_core::Home, CliError> {
    let home = factory_core::Home::from_env()?;
    check_home(&home)?;
    Ok(home)
}

/// Runs one command and returns the text to print.
pub fn dispatch(cli: Cli, ctx: &CliCtx) -> Result<String, CliError> {
    match cli.command {
        Command::New {
            slug,
            feature,
            plans,
            spec,
            model,
            one_job,
            accept_over_budget,
        } => new_job(
            ctx,
            &slug,
            &feature,
            NewArgs {
                plans,
                spec,
                model,
                one_job,
                accept_over_budget,
            },
        ),
        Command::Run { id, series } => run(ctx, &norm(&id)?, series),
        Command::Status => {
            let d = ctx.driver()?;
            let board = d.store().load()?;
            let lease = project_lease_status(&d.layout().state_dir, now_secs());
            let mut out = render_project_line(&lease);
            out.push_str(&render_status(&board, d.config()));
            out.push_str(&render_parked(&board, |j| ParkRecord::read(&d.job_dir(j))));
            Ok(out)
        }
        Command::Show { id } => {
            let d = ctx.driver()?;
            Ok(serde_json::to_string_pretty(&d.job(&norm(&id)?)?)?)
        }
        Command::Approve { id, without_review } => approve(ctx, &norm(&id)?, without_review),
        Command::Rework { id, note } => rework(ctx, &norm(&id)?, &note),
        Command::Cancel { id, reason, force } => match factory_core::normalize_id(&id) {
            Ok(id) => cancel(ctx, &id, &reason, force),
            Err(_) => cancel_draft(ctx, &id),
        },
        Command::Spec { cmd } => spec(ctx, cmd),
        Command::Cleanup {
            id,
            discard_commits,
        } => cleanup(ctx, &norm(&id)?, discard_commits),
        Command::Tokens { id } => tokens(ctx, &norm(&id)?),
        Command::Costs => costs(ctx),
        Command::Project { .. } => Err(CliError::Refused(
            "project commands do not need a repository, run them through project::run".into(),
        )),
        Command::Presets { .. } => Err(CliError::Refused(
            "presets commands do not need a repository, run them through presets::run".into(),
        )),
        Command::Install { .. } => Err(CliError::Refused(
            "install needs no repository, run it through install::run".into(),
        )),
        Command::Upgrade { .. } => Err(CliError::Refused(
            "upgrade needs no repository, run it through upgrade::run".into(),
        )),
        Command::Log { job, level, tail } => {
            crate::log::run_log(ctx, job.as_deref(), level.as_deref(), tail)
        }
        Command::Tickets { id, dry_run } => tickets(ctx, &norm(&id)?, dry_run),
        Command::KbRefresh { job } => kb_refresh(ctx, &norm(&job)?),
    }
}

fn norm(id: &str) -> Result<String, CliError> {
    Ok(factory_core::normalize_id(id)?)
}

/// The plan related arguments of `factory new`.
struct NewArgs {
    plans: Option<PathBuf>,
    spec: Option<PathBuf>,
    model: Option<String>,
    one_job: bool,
    accept_over_budget: bool,
}

fn new_job(ctx: &CliCtx, slug: &str, feature: &str, args: NewArgs) -> Result<String, CliError> {
    let NewArgs {
        plans,
        spec,
        model,
        one_job,
        accept_over_budget,
    } = args;
    let d = ctx.driver()?;
    let (job, notes) = match (one_job, plans.as_deref()) {
        (true, Some(plan)) => {
            let made = create_one_job(
                &d,
                slug,
                feature,
                plan,
                spec.as_deref(),
                model.as_deref(),
                accept_over_budget,
            )?;
            (made.job, made.notes)
        }
        _ => (
            create_job_with_spec(
                &d,
                slug,
                feature,
                plans.as_deref(),
                spec.as_deref(),
                model.as_deref(),
            )?,
            Vec::new(),
        ),
    };
    let board = d.store().load()?;
    let mut out = format!(
        "created job {} in {}{}",
        job.id,
        job.dir,
        estimate_lines(&board, &job)
    );
    // The created line stays first because factoryd parses it. Notes and warnings follow, one per line.
    for note in &notes {
        out.push_str(&format!("\nnote: {note}"));
    }
    for warning in job.spec_warning.iter().flat_map(|w| w.split("; ")) {
        out.push_str(&format!("\nwarning: {warning}"));
    }
    if plans.is_none() {
        let dir = d.job_dir(&job);
        out.push_str(&format!(
            "\nno plan yet: put plan.md at {}/plan.md before running it",
            dir.display()
        ));
    }
    Ok(out)
}

/// The three lines `spec start` and `spec resume` print.
fn draft_lines(d: &Driver, draft: &factory_core::Draft) -> String {
    let file = draft.spec_file.clone().unwrap_or_else(|| {
        let full = d.layout().draft_spec_file(&draft.created, &draft.slug);
        full.strip_prefix(&d.layout().repo)
            .unwrap_or(&full)
            .to_string_lossy()
            .into_owned()
    });
    let state = if d.layout().repo.join(&file).is_file() {
        "exists"
    } else {
        "missing"
    };
    format!(
        "draft {}\nstage {}\nfile {file} ({state})",
        draft.slug, draft.stage
    )
}

/// The `spec` commands only touch draft cards, so they never start agents and skip `runnable`.
fn spec(ctx: &CliCtx, cmd: SpecCmd) -> Result<String, CliError> {
    let d = ctx.driver()?;
    let draft = match cmd {
        SpecCmd::Start { slug, feature } => start_draft(&d, &slug, &feature, &today(), now_secs())?,
        SpecCmd::Stage { slug, stage } => {
            set_draft_stage(&d, &slug, &stage, now_secs())?;
            return Ok(format!(
                "draft {} moved to {stage}",
                factory_core::slugify(&slug)
            ));
        }
        SpecCmd::Resume { slug } => find_draft(&d, &slug)?.ok_or_else(|| {
            CliError::Refused(format!("no draft named {}", factory_core::slugify(&slug)))
        })?,
    };
    Ok(draft_lines(&d, &draft))
}

/// Removes a draft card. The spec file stays.
fn cancel_draft(ctx: &CliCtx, name: &str) -> Result<String, CliError> {
    let d = ctx.driver()?;
    if find_draft(&d, name)?.is_none() {
        return Err(CliError::Refused(format!("no job or draft named {name}")));
    }
    let draft = remove_draft(&d, name)?;
    Ok(format!(
        "draft {} cancelled, its spec file was kept",
        draft.slug
    ))
}

fn costs(ctx: &CliCtx) -> Result<String, CliError> {
    let d = ctx.driver()?;
    let jobs = gather_costs(&d)?;
    let file = CostsFile::build(&jobs, d.config(), now_secs());
    let mut out = render_costs(&jobs, &file.groups);
    if let Err(e) = file.write(&costs_path(d.layout())) {
        out.push_str(&format!("\nwarning: could not write costs.json: {e}\n"));
    }
    Ok(out)
}

fn run(ctx: &CliCtx, id: &str, series: bool) -> Result<String, CliError> {
    ctx.runnable()?;
    let d = ctx.driver()?;
    // The project lease comes first, so a second driver is refused before any stage work.
    // The guard lives until the command returns, a series included.
    let _project = d.hold_project(id)?;
    // The driver takes the job lease and its heartbeat renews it until the guard below releases it.
    d.renew_lease(id)?;
    // The job lease is released on every exit path, an error from the run included.
    let _guard = LeaseGuard {
        driver: &d,
        id: id.to_string(),
    };
    let end = if series {
        run_series(&d, id)?
    } else {
        run_job(&d, id)?
    };
    Ok(match end {
        RunEnd::Merged => outcome_lines::merged_line(None),
        RunEnd::NeedsHuman(reason) => outcome_lines::needs_human_line(&reason),
    })
}

/// Releases the lease on every exit path. A crashed driver's lease is free once its process is gone, or when it expires.
struct LeaseGuard<'a> {
    driver: &'a Driver,
    id: String,
}

impl Drop for LeaseGuard<'_> {
    fn drop(&mut self) {
        self.driver.release_lease(&self.id);
    }
}

fn approve(ctx: &CliCtx, id: &str, without_review: bool) -> Result<String, CliError> {
    ctx.runnable()?;
    let _span = factory_engine::job_span(id).entered();
    let d = ctx.driver()?;
    let job = d.job(id)?;
    if job.cancelled {
        return Err(CliError::Refused(format!("job {id} was cancelled")));
    }
    if job.stage != Stage::NeedsHuman && job.stage != Stage::Approve {
        return Err(CliError::Refused(format!(
            "job {id} is in stage {}, approve needs needs-human or approve",
            job.stage
        )));
    }
    if job.batch_snapshot.is_some() {
        return Err(CliError::Refused(format!(
            "job {id} has a pending batch commit, use rework to finish it"
        )));
    }
    // A parked job whose review never ran must not merge unseen, unless the user says so.
    if job.stage == Stage::NeedsHuman && job.review_missing() && !without_review {
        return Err(CliError::Refused(job.review_missing_message()));
    }
    // The project lease is held through the merge and released on every exit path.
    let _project = d.hold_project(id)?;
    // The driver takes the job lease and its heartbeat renews it until the guard below releases it.
    d.renew_lease(id)?;
    let _guard = LeaseGuard {
        driver: &d,
        id: id.to_string(),
    };
    let now = now_secs();
    d.store().update_job(id, |j| {
        factory_engine::set_stage_logged(j, Stage::Approve, None, false, now)
    })?;
    let ruling = if without_review && job.stage == Stage::NeedsHuman && job.review_missing() {
        "approved by the user without a review"
    } else {
        "approved by the user"
    };
    d.ledger(&job).ruling(ruling)?;
    Ok(match merge_stage(&d, id)? {
        MergeResult::Merged(sha) => outcome_lines::merged_line(Some(&sha)),
        MergeResult::Parked(reason) => outcome_lines::parked_line(&reason),
    })
}

fn rework(ctx: &CliCtx, id: &str, note: &str) -> Result<String, CliError> {
    validate_rework_note(note)?;
    let _span = factory_engine::job_span(id).entered();
    let d = ctx.driver()?;
    let job = d.job(id)?;
    if job.stage != Stage::NeedsHuman {
        return Err(CliError::Refused(format!(
            "job {id} is in stage {}, rework needs needs-human",
            job.stage
        )));
    }
    // The note is stored verbatim. The builder gets exactly this text.
    std::fs::write(d.job_dir(&job).join("rework-note.md"), note)?;
    d.ledger(&job).ruling(&format!("rework note: {note}"))?;
    let now = now_secs();
    d.store().update_job(id, |j| {
        // Needs-human back to build is allowed by the stage guard, so no force.
        factory_engine::set_stage_logged(j, Stage::Build, None, false, now)?;
        j.round = 0;
        // The note is the findings now, so a no-change round is not a checks case.
        j.fix_source = None;
        // A rework revives a cancelled job, so approve does not refuse it later.
        j.cancelled = false;
        Ok(())
    })?;
    Ok(format!("job {id} back to build, round reset to 0"))
}

fn cancel(ctx: &CliCtx, id: &str, reason: &str, force: bool) -> Result<String, CliError> {
    let _span = factory_engine::job_span(id).entered();
    let d = ctx.driver()?;
    let job = d.job(id)?;
    if job.stage == Stage::Merged {
        return Err(CliError::Refused(format!("job {id} is already merged")));
    }
    if lease_is_live(&job, now_secs()) && !force {
        return Err(CliError::Refused(
            "job has a live lease, use --force to cancel anyway".into(),
        ));
    }
    let text = format!("cancelled: {reason}");
    let now = now_secs();
    d.store().update_job(id, |j| {
        factory_engine::set_stage_logged(j, Stage::NeedsHuman, Some(&text), false, now)?;
        j.cancelled = true;
        j.lease = None;
        Ok(())
    })?;
    // The reason is folded into one line, because the ledger takes one line.
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    d.ledger(&job).ruling(&one_line)?;
    let mut out = format!("job {id} cancelled");
    // The process that held the project lease may be gone, so a forced cancel clears it for this job.
    if force {
        let cleared = force_release_project_lease(&d.layout().state_dir, &job.id, ctx.lock_wait)
            .map_err(|e| {
                CliError::Refused(format!(
                    "cancelled, but the project lease could not be cleared: {e}"
                ))
            })?;
        if cleared {
            out.push_str("\nproject lease released");
        }
    }
    Ok(out)
}

/// Removes the worktree, branch and leftover folder, then the card of a cancelled job. A merged job keeps its card.
fn cleanup(ctx: &CliCtx, id: &str, discard: bool) -> Result<String, CliError> {
    let d = ctx.driver()?;
    let job = d.job(id)?;
    if job.stage != Stage::Merged && !job.cancelled {
        return Err(CliError::Refused(
            "cleanup only works on a merged or cancelled job".into(),
        ));
    }
    let mut message = cleanup_checkout(ctx, &job, discard)?;
    d.store().update_job(id, |j| {
        j.cleaned = true;
        Ok(())
    })?;
    if job.cancelled && job.stage != Stage::Merged {
        match remove_cancelled_card(ctx, &job.id)? {
            None => message.push_str("; card removed from the board"),
            Some(why) => message.push_str(&format!("; card kept on the board: {why}")),
        }
    }
    Ok(message)
}

/// Takes the card of a cancelled job off the board. The job folder stays, because the next id and the daily
/// usage read it. Returns why the card stays, or None when it was removed.
fn remove_cancelled_card(ctx: &CliCtx, id: &str) -> Result<Option<String>, CliError> {
    let d = ctx.driver()?;
    let state_dir = &d.layout().state_dir;
    match project_lease_status(state_dir, now_secs()) {
        factory_engine::ProjectLeaseStatus::Held { job, .. } if job == id => {
            return Ok(Some(format!("the project lease is held by job {id}")));
        }
        factory_engine::ProjectLeaseStatus::Stale { job, .. } if job == id => {
            force_release_project_lease(state_dir, id, ctx.lock_wait).map_err(|e| {
                CliError::Refused(format!("the project lease could not be cleared: {e}"))
            })?;
        }
        _ => {}
    }
    let kept = d.store().update(|board| {
        let job = board.find(id)?;
        if lease_is_live(job, now_secs()) {
            return Ok(Some(format!("job {id} has a live lease")));
        }
        if let Some(sid) = board.series_with_siblings(&job.id) {
            return Ok(Some(format!("job is part of series {sid}")));
        }
        board.remove_job(id)?;
        Ok(None)
    })?;
    Ok(kept)
}

/// Removes the worktree, the branch and any leftover folder. Returns what was done, in words.
fn cleanup_checkout(
    ctx: &CliCtx,
    job: &factory_core::Job,
    discard: bool,
) -> Result<String, CliError> {
    let d = ctx.driver()?;
    let id = job.id.as_str();
    let repo = d.repo();
    let jr = factory_git::JobRef {
        id: job.id.clone(),
        slug: job.slug.clone(),
    };
    let wt = factory_git::Worktree {
        name: jr.dir_name(),
        path: job
            .worktree
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| d.repo().worktrees_dir().join(jr.dir_name())),
        branch: job.branch.clone(),
        base: job.base.clone().unwrap_or_default(),
    };
    let branch_exists = repo.branch_exists(&wt.branch);
    // A folder without a `.git` is not a checkout, it is debris, for example agent files written after the worktree was removed.
    let debris = wt.path.is_dir() && !wt.path.join(".git").exists();
    if debris && !wt.path.starts_with(repo.worktrees_dir()) {
        return Err(CliError::Refused(format!(
            "{} is not a git checkout and lies outside {}, remove it by hand",
            wt.path.display(),
            repo.worktrees_dir().display()
        )));
    }
    if !branch_exists && (debris || !wt.path.exists()) {
        let message = if debris {
            std::fs::remove_dir_all(&wt.path).map_err(|e| {
                CliError::Refused(format!(
                    "could not remove leftover folder {}: {e}",
                    wt.path.display()
                ))
            })?;
            format!(
                "removed leftover folder for job {id}; the worktree and branch were already gone"
            )
        } else {
            format!("nothing to remove for job {id}, the worktree and branch are already gone")
        };
        return Ok(message);
    }
    // A head that is gone or equals the base has no commits to protect, so an error counts as merged.
    let unmerged = match (job.base.as_deref(), job.head.as_deref()) {
        (Some(b), Some(h)) => repo.is_merged(b, h).map(|m| m.is_none()).unwrap_or(false),
        _ => false,
    };
    if unmerged && !discard {
        return Err(CliError::Refused(format!(
            "the branch has commits that are not in {}, use --discard-commits",
            repo.base_branch()
        )));
    }
    if debris {
        // The unmerged check above has passed, so the leftover folder goes first and remove_worktree then drops the branch.
        std::fs::remove_dir_all(&wt.path).map_err(|e| {
            CliError::Refused(format!(
                "could not remove leftover folder {}: {e}",
                wt.path.display()
            ))
        })?;
    }
    // remove_worktree also deletes the job branch. It refuses a dirty worktree and unmerged commits unless forced.
    repo.remove_worktree(&wt, discard)
        .map_err(|e| CliError::Refused(format!("{e}, use --discard-commits to drop it")))?;
    Ok(format!("removed worktree and branch of job {id}"))
}

/// Takes no lease, it only calls the tracker and writes the job folder. Refuses while another driver holds a live lease on the job.
fn tickets(ctx: &CliCtx, id: &str, dry_run: bool) -> Result<String, CliError> {
    let d = ctx.driver()?;
    let job = d.job(id)?;
    if let Some(lease) = job
        .lease
        .as_ref()
        .filter(|_| lease_is_live(&job, now_secs()))
    {
        if lease.owner != ctx.owner {
            return Err(CliError::Refused(format!(
                "job {id} is leased by {}",
                lease.owner
            )));
        }
    }
    render_tickets(factory_engine::tickets_step(&d, id, dry_run)?, dry_run)
}

/// The text of a tickets report. A failed report is an error, so the exit code is non-zero.
fn render_tickets(
    report: factory_engine::TicketsReport,
    dry_run: bool,
) -> Result<String, CliError> {
    let body = report.lines.join("\n");
    if report.failed {
        return Err(CliError::Refused(body));
    }
    if dry_run && !report.skipped {
        return Ok(format!("dry run, nothing was changed\n{body}"));
    }
    Ok(body)
}

/// Takes the project lease. Refuses unless the job is merged, and refuses while another driver holds a live lease on the job.
fn kb_refresh(ctx: &CliCtx, id: &str) -> Result<String, CliError> {
    ctx.runnable()?;
    let d = ctx.driver()?;
    // The refresh writes into the main checkout, so it takes the project lease like a run.
    let _project = d.hold_project(id)?;
    let job = d.job(id)?;
    if job.stage != Stage::Merged {
        return Err(CliError::Refused(format!(
            "job {id} is not merged, the KB refresh needs a merged job"
        )));
    }
    if let Some(lease) = job
        .lease
        .as_ref()
        .filter(|_| lease_is_live(&job, now_secs()))
    {
        if lease.owner != ctx.owner {
            return Err(CliError::Refused(format!(
                "job {id} is leased by {}",
                lease.owner
            )));
        }
    }
    Ok(d.kb_refresh(id, true)?)
}

fn tokens(ctx: &CliCtx, id: &str) -> Result<String, CliError> {
    let d = ctx.driver()?;
    let job = d.job(id)?;
    let t = job.tokens;
    let weighted = t.weighted();
    let budget = d.config().budget_weighted;
    let pct = (weighted * 100).checked_div(budget).unwrap_or(0);
    let records = crate::tokens::load_usage(&d.job_dir(&job).join("usage.jsonl"), id)?;
    let report = crate::tokens::render(id, &crate::tokens::aggregate(&records));
    let mut text = format!(
        "input {}\ncache_creation {}\noutput {}\ncache_read {}\nweighted {weighted} ({pct}% of {budget})\n{}",
        t.input, t.cache_creation, t.output, t.cache_read, report.trim_end()
    );
    let detail = crate::tokens::render_detail(&records);
    if !detail.is_empty() {
        text.push('\n');
        text.push_str(detail.trim_end());
    }
    if crate::tokens::under_counted(&records) {
        text.push_str("\n  note: output under-counted, some lines predate the corrected count");
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rework_notes_are_one_short_line() {
        assert!(validate_rework_note("all green").is_ok());
        assert!(validate_rework_note("").is_err());
        assert!(validate_rework_note("two\nlines").is_err());
        assert!(validate_rework_note(&"x".repeat(201)).is_err());
        assert!(validate_rework_note(&"x".repeat(200)).is_ok());
    }
}

#[cfg(test)]
mod tickets_tests {
    use super::*;
    use factory_engine::TicketsReport;

    fn report(lines: &[&str], failed: bool, skipped: bool) -> TicketsReport {
        TicketsReport {
            lines: lines.iter().map(|s| s.to_string()).collect(),
            failed,
            skipped,
        }
    }

    #[test]
    fn a_good_report_prints_its_lines() {
        let text = render_tickets(
            report(&["created #201: A", "closed #76"], false, false),
            false,
        )
        .expect("ok");
        assert_eq!(text, "created #201: A\nclosed #76");
    }

    #[test]
    fn a_dry_run_says_so() {
        let text = render_tickets(report(&["would close #76"], false, false), true).expect("ok");
        assert!(text.starts_with("dry run, nothing was changed"), "{text}");
        assert!(text.contains("would close #76"));
    }

    #[test]
    fn a_skipped_report_is_not_an_error() {
        let text = render_tickets(
            report(&["skipped: job 001 is not merged"], false, true),
            false,
        )
        .expect("ok");
        assert!(text.contains("skipped"));
    }

    #[test]
    fn a_failed_report_is_an_error_with_all_lines() {
        let err = render_tickets(
            report(
                &["created #201: A", "failed: comment on #76: boom"],
                true,
                false,
            ),
            false,
        )
        .expect_err("failed");
        let message = err.to_string();
        assert!(
            message.contains("created #201: A") && message.contains("comment on #76: boom"),
            "{message}"
        );
    }
}
