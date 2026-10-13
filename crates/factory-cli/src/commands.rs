use std::path::PathBuf;
use std::sync::Arc;

use factory_agent::{AgentRunner, Caps, ClaudeRunner, EventSink};
use factory_core::{Config, Stage};
use factory_engine::{
    acquire_lease, create_job, lease_is_live, merge_stage, now_secs, release_lease, run_job, run_series, Driver,
    MergeResult, RunEnd, LEASE_TTL_SECS,
};

use crate::cli::{Cli, Command};
use crate::status::render_status;

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

/// What a command needs: the checkout, an optional injected runner (tests) and the lease owner name.
#[derive(Clone)]
pub struct CliCtx {
    pub root: PathBuf,
    pub runner: Option<Arc<dyn AgentRunner>>,
    pub owner: String,
}

impl CliCtx {
    /// A context with an injected runner, used by tests.
    pub fn new(root: PathBuf, runner: Arc<dyn AgentRunner>, owner: &str) -> CliCtx {
        CliCtx { root, runner: Some(runner), owner: owner.to_string() }
    }

    /// A context for the real binary. The root is the main checkout of the current directory.
    pub fn from_env(root: Option<PathBuf>) -> Result<CliCtx, CliError> {
        let root = match root {
            Some(r) => r,
            None => {
                let cwd = std::env::current_dir()?;
                factory_git::Repo::open(&cwd)?.root().to_path_buf()
            }
        };
        let owner = format!("driver-{}-{}", std::process::id(), now_secs() as u64);
        Ok(CliCtx { root, runner: None, owner })
    }

    /// The runner of one job. A real runner gets an event sink stamped with that job's id,
    /// so a series never writes the first job's id into later jobs' events.
    pub fn runner_for(&self, job_id: &str) -> Arc<dyn AgentRunner> {
        match &self.runner {
            Some(r) => r.clone(),
            None => {
                let sink = EventSink::new(self.root.join(".superpowers/factory/events.jsonl"), job_id);
                Arc::new(ClaudeRunner::new("claude", Caps::default(), sink))
            }
        }
    }

    fn driver(&self) -> Result<Driver, CliError> {
        let config = Config::load(&self.root.join(".claude/factory/config.toml"));
        let me = self.clone();
        let driver = Driver::new(&self.root, config, Box::new(move |id| me.runner_for(id)))?;
        Ok(driver.with_lease_owner(&self.owner))
    }
}

pub fn validate_rework_note(note: &str) -> Result<(), CliError> {
    if note.trim().is_empty() {
        return Err(CliError::Refused("the rework note is empty".into()));
    }
    if note.contains('\n') || note.contains('\r') {
        return Err(CliError::Refused("the rework note must be one line".into()));
    }
    if note.chars().count() > 200 {
        return Err(CliError::Refused("the rework note is longer than 200 characters".into()));
    }
    Ok(())
}

/// Runs one command and returns the text to print.
pub fn dispatch(cli: Cli, ctx: &CliCtx) -> Result<String, CliError> {
    match cli.command {
        Command::New { slug, feature, plans, model } => new_job(ctx, &slug, &feature, plans, model),
        Command::Run { id, series } => run(ctx, &norm(&id)?, series),
        Command::Status => {
            let d = ctx.driver()?;
            Ok(render_status(&d.store().load()?, d.config()))
        }
        Command::Show { id } => {
            let d = ctx.driver()?;
            Ok(serde_json::to_string_pretty(&d.job(&norm(&id)?)?)?)
        }
        Command::Approve { id } => approve(ctx, &norm(&id)?),
        Command::Rework { id, note } => rework(ctx, &norm(&id)?, &note),
        Command::Cancel { id, reason, force } => cancel(ctx, &norm(&id)?, &reason, force),
        Command::Cleanup { id, discard_commits } => cleanup(ctx, &norm(&id)?, discard_commits),
        Command::Tokens { id } => tokens(ctx, &norm(&id)?),
    }
}

fn norm(id: &str) -> Result<String, CliError> {
    Ok(factory_core::normalize_id(id)?)
}

fn new_job(
    ctx: &CliCtx,
    slug: &str,
    feature: &str,
    plans: Option<PathBuf>,
    model: Option<String>,
) -> Result<String, CliError> {
    let d = ctx.driver()?;
    let job = create_job(&d, slug, feature, plans.as_deref(), model.as_deref())?;
    Ok(format!("created job {} in {}", job.id, job.dir))
}

fn run(ctx: &CliCtx, id: &str, series: bool) -> Result<String, CliError> {
    let d = ctx.driver()?;
    // The lease check comes first, so a second driver is refused before any stage work.
    acquire_lease(d.store(), id, &ctx.owner, LEASE_TTL_SECS, now_secs())?;
    let end = if series { run_series(&d, id)? } else { run_job(&d, id)? };
    Ok(match end {
        RunEnd::Merged => "merged".to_string(),
        RunEnd::NeedsHuman(reason) => format!("needs-human: {reason}"),
    })
}

/// Releases the lease on every exit path. A crashed driver's lease expires by itself.
struct LeaseGuard<'a> {
    driver: &'a Driver,
    id: String,
    owner: String,
}

impl Drop for LeaseGuard<'_> {
    fn drop(&mut self) {
        let _ = release_lease(self.driver.store(), &self.id, &self.owner, now_secs());
    }
}

fn approve(ctx: &CliCtx, id: &str) -> Result<String, CliError> {
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
    acquire_lease(d.store(), id, &ctx.owner, LEASE_TTL_SECS, now_secs())?;
    let _guard = LeaseGuard { driver: &d, id: id.to_string(), owner: ctx.owner.clone() };
    let now = now_secs();
    d.store().update_job(id, |j| j.set_stage(Stage::Approve, None, false, now))?;
    d.ledger(&job).ruling("approved by the user")?;
    Ok(match merge_stage(&d, id)? {
        MergeResult::Merged(sha) => format!("merged {sha}"),
        MergeResult::Parked(reason) => format!("parked: {reason}"),
    })
}

fn rework(ctx: &CliCtx, id: &str, note: &str) -> Result<String, CliError> {
    validate_rework_note(note)?;
    let d = ctx.driver()?;
    let job = d.job(id)?;
    if job.stage != Stage::NeedsHuman {
        return Err(CliError::Refused(format!("job {id} is in stage {}, rework needs needs-human", job.stage)));
    }
    // The note is stored verbatim. The builder gets exactly this text.
    std::fs::write(d.job_dir(&job).join("rework-note.md"), note)?;
    d.ledger(&job).ruling(&format!("rework note: {note}"))?;
    let now = now_secs();
    d.store().update_job(id, |j| {
        // Needs-human back to build is allowed by the stage guard, so no force.
        j.set_stage(Stage::Build, None, false, now)?;
        j.round = 0;
        Ok(())
    })?;
    Ok(format!("job {id} back to build, round reset to 0"))
}

fn cancel(ctx: &CliCtx, id: &str, reason: &str, force: bool) -> Result<String, CliError> {
    let d = ctx.driver()?;
    let job = d.job(id)?;
    if job.stage == Stage::Merged {
        return Err(CliError::Refused(format!("job {id} is already merged")));
    }
    if lease_is_live(&job, now_secs()) && !force {
        return Err(CliError::Refused("job has a live lease, use --force to cancel anyway".into()));
    }
    let text = format!("cancelled: {reason}");
    let now = now_secs();
    d.store().update_job(id, |j| {
        j.set_stage(Stage::NeedsHuman, Some(&text), false, now)?;
        j.cancelled = true;
        j.lease = None;
        Ok(())
    })?;
    // The reason is folded into one line, because the ledger takes one line.
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    d.ledger(&job).ruling(&one_line)?;
    Ok(format!("job {id} cancelled"))
}

fn cleanup(ctx: &CliCtx, id: &str, discard: bool) -> Result<String, CliError> {
    let d = ctx.driver()?;
    let job = d.job(id)?;
    if job.stage != Stage::Merged && !job.cancelled {
        return Err(CliError::Refused("cleanup only works on a merged or cancelled job".into()));
    }
    let repo = d.repo();
    let jr = factory_git::JobRef { id: job.id.clone(), slug: job.slug.clone() };
    let wt = factory_git::Worktree {
        name: jr.dir_name(),
        path: job
            .worktree
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| d.root().join(".claude/worktrees").join(jr.dir_name())),
        branch: job.branch.clone(),
        base: job.base.clone().unwrap_or_default(),
    };
    // A head that is gone or equals the base has no commits to protect, so an error counts as merged.
    let unmerged = match (job.base.as_deref(), job.head.as_deref()) {
        (Some(b), Some(h)) => repo.is_merged(b, h).map(|m| m.is_none()).unwrap_or(false),
        _ => false,
    };
    if unmerged && !discard {
        return Err(CliError::Refused("the branch has commits that are not in main, use --discard-commits".into()));
    }
    // remove_worktree also deletes the job branch. It refuses a dirty worktree and unmerged commits unless forced.
    repo.remove_worktree(&wt, discard)
        .map_err(|e| CliError::Refused(format!("{e}, use --discard-commits to drop it")))?;
    d.store().update_job(id, |j| {
        j.cleaned = true;
        Ok(())
    })?;
    Ok(format!("removed worktree and branch of job {id}"))
}

fn tokens(ctx: &CliCtx, id: &str) -> Result<String, CliError> {
    let d = ctx.driver()?;
    let job = d.job(id)?;
    let t = job.tokens;
    let weighted = t.weighted();
    let budget = d.config().budget_weighted;
    let pct = if budget > 0 { weighted * 100 / budget } else { 0 };
    let records = crate::tokens::load_usage(&d.job_dir(&job).join("usage.jsonl"), id)?;
    let report = crate::tokens::render(id, &crate::tokens::aggregate(&records));
    Ok(format!(
        "input {}\ncache_creation {}\noutput {}\ncache_read {}\nweighted {weighted} ({pct}% of {budget})\n{}",
        t.input, t.cache_creation, t.output, t.cache_read, report.trim_end()
    ))
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
