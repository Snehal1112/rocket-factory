//! Removes the checkout of a finished job: its worktree, its branch and any leftover folder.

use crate::merge::{run_soft_step, write_step_ledger};
use crate::{Driver, EngineError};
use factory_core::Job;
use std::path::PathBuf;

/// Removes the worktree, the branch and any leftover folder of a job. Returns what was done, in words.
/// Every refusal is `EngineError::Refused`.
pub fn cleanup_checkout(driver: &Driver, job: &Job, discard: bool) -> Result<String, EngineError> {
    let id = job.id.as_str();
    let repo = driver.repo();
    let jr = repo.job_ref(&job.id, &job.slug);
    let wt = factory_git::Worktree {
        name: jr.dir_name(),
        path: job
            .worktree
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| repo.worktrees_dir().join(jr.dir_name())),
        branch: job.branch.clone(),
        base: job.base.clone().unwrap_or_default(),
    };
    let branch_exists = repo.branch_exists(&wt.branch);
    // A folder without a `.git` is not a checkout, it is debris, for example agent files written after the worktree was removed.
    let debris = wt.path.is_dir() && !wt.path.join(".git").exists();
    if debris && !wt.path.starts_with(repo.worktrees_dir()) {
        return Err(EngineError::Refused(format!(
            "{} is not a git checkout and lies outside {}, remove it by hand",
            wt.path.display(),
            repo.worktrees_dir().display()
        )));
    }
    if !branch_exists && (debris || !wt.path.exists()) {
        let message = if debris {
            remove_leftover_folder(&wt.path)?;
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
        return Err(EngineError::Refused(format!(
            "the branch has commits that are not in {}, use --discard-commits",
            repo.base_branch()
        )));
    }
    if debris {
        // The unmerged check above has passed, so the leftover folder goes first and remove_worktree then drops the branch.
        remove_leftover_folder(&wt.path)?;
    }
    // remove_worktree also deletes the job branch. It refuses a dirty worktree and unmerged commits unless forced.
    repo.remove_worktree(&wt, discard)
        .map_err(|e| EngineError::Refused(format!("{e}, use --discard-commits to drop it")))?;
    Ok(format!("removed worktree and branch of job {id}"))
}

/// Runs the cleanup after a merge as a soft step: marks the card cleaned, or reports why the worktree was kept.
pub(crate) fn cleanup_after_merge(driver: &Driver, id: &str) {
    let result = run_soft_step(|| {
        let job = driver.job(id)?;
        let text = cleanup_checkout(driver, &job, false)?;
        driver.store().update_job(id, |j| {
            j.cleaned = true;
            Ok(())
        })?;
        Ok(text)
    });
    match result {
        Ok(text) => tracing::info!(event = "cleanup.done", job = id, msg = %text),
        Err(failure) => {
            let reason = failure.to_string();
            let line = format!("cleanup kept the worktree: {reason}");
            write_step_ledger(driver, id, "cleanup", &line);
            eprintln!("warning: job {id}: {line}");
            tracing::warn!(event = "cleanup.kept", job = id, reason = %reason);
        }
    }
}

fn remove_leftover_folder(path: &std::path::Path) -> Result<(), EngineError> {
    std::fs::remove_dir_all(path).map_err(|e| {
        EngineError::Refused(format!(
            "could not remove leftover folder {}: {e}",
            path.display()
        ))
    })
}
