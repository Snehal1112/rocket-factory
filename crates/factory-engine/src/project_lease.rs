//! The project lease: at most one active job per project, held in a small file next to the board.

use std::path::{Path, PathBuf};
use std::time::Duration;

use factory_core::{write_atomic, PROJECT_LEASE_FILE};
use serde::{Deserialize, Serialize};

use crate::{lock_board, now_secs, StoreError};

/// The content of the project lease file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectLease {
    pub owner: String,
    pub job: String,
    pub expires: f64,
}

/// How a lease was taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaseTake {
    Fresh,
    Renewed,
    TookOverStale { owner: String, job: String },
}

#[derive(Debug, thiserror::Error)]
pub enum ProjectLeaseError {
    #[error(
        "project is busy: job {job} is running (leased by {owner} for another {secs_left} seconds). \
         If the process that held it is gone, `factory cancel {job} --force` clears it, or delete the project.lease file in the state folder"
    )]
    Held { owner: String, job: String, secs_left: u64 },
    #[error("project lease lock: {0}")]
    Lock(#[from] StoreError),
    #[error("project lease io: {0}")]
    Io(String),
}

/// What the lease file says right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectLeaseStatus {
    Free,
    Held { owner: String, job: String, secs_left: u64 },
    Stale { owner: String, job: String },
    Unreadable(String),
}

/// The pid in an owner of the form `driver-<pid>-<secs>`. Any other owner has none.
pub fn owner_pid(owner: &str) -> Option<u32> {
    let rest = owner.strip_prefix("driver-")?;
    let (pid, secs) = rest.split_once('-')?;
    secs.parse::<u64>().ok()?;
    pid.parse().ok()
}

/// True only when this machine says no process has `pid`. This process is never gone.
pub fn process_gone(pid: u32) -> bool {
    pid != std::process::id() && !crate::procinfo::pid_exists(pid)
}

/// A lease whose owner is a driver process that no longer runs on this machine.
fn owner_is_dead(owner: &str) -> bool {
    owner_pid(owner).is_some_and(process_gone)
}

fn lease_path(state_dir: &Path) -> PathBuf {
    state_dir.join(PROJECT_LEASE_FILE)
}

fn lock_path(state_dir: &Path) -> PathBuf {
    state_dir.join(format!("{PROJECT_LEASE_FILE}.lock"))
}

/// The lease in the file. None means no file. A file that cannot be read or parsed is an error text.
fn read_lease(path: &Path) -> Result<Option<ProjectLease>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str::<ProjectLease>(&text).map(Some).map_err(|e| format!("not valid lease JSON: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Takes or renews the lease. A lease that is free, expired, corrupt or ours can be taken.
pub fn acquire_project_lease(
    state_dir: &Path,
    job: &str,
    owner: &str,
    ttl: f64,
    now: f64,
    wait: Duration,
) -> Result<LeaseTake, ProjectLeaseError> {
    let _lock = lock_board(&lock_path(state_dir), wait)?;
    let path = lease_path(state_dir);
    let take = match read_lease(&path) {
        Ok(None) => LeaseTake::Fresh,
        Ok(Some(l)) if l.owner == owner => LeaseTake::Renewed,
        Ok(Some(l)) if l.expires <= now || owner_is_dead(&l.owner) => LeaseTake::TookOverStale { owner: l.owner, job: l.job },
        Ok(Some(l)) => {
            return Err(ProjectLeaseError::Held { owner: l.owner, job: l.job, secs_left: (l.expires - now) as u64 });
        }
        // A corrupt file must never block the project for good.
        Err(_) => LeaseTake::TookOverStale { owner: "unknown".into(), job: "unknown".into() },
    };
    let lease = ProjectLease { owner: owner.to_string(), job: job.to_string(), expires: now + ttl };
    let bytes = serde_json::to_vec_pretty(&lease).map_err(|e| ProjectLeaseError::Io(e.to_string()))?;
    write_atomic(&path, &bytes).map_err(|e| ProjectLeaseError::Io(e.to_string()))?;
    Ok(take)
}

/// Removes the lease when it is ours, expired or corrupt. A live lease of another owner is left alone.
pub fn release_project_lease(state_dir: &Path, owner: &str, wait: Duration) -> Result<(), ProjectLeaseError> {
    let _lock = lock_board(&lock_path(state_dir), wait)?;
    let path = lease_path(state_dir);
    let removable = match read_lease(&path) {
        Ok(None) => return Ok(()),
        Ok(Some(l)) => l.owner == owner || l.expires <= now_secs() || owner_is_dead(&l.owner),
        Err(_) => true,
    };
    if removable {
        remove_lease_file(&path)?;
    }
    Ok(())
}

/// Deletes the lease file. A file that is already gone counts as removed.
fn remove_lease_file(path: &Path) -> Result<(), ProjectLeaseError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(ProjectLeaseError::Io(e.to_string())),
    }
}

/// Removes the lease when it belongs to `job`, whoever the owner is. Used when a job is cancelled with
/// force, because the process that held the lease may be gone. Returns true when a lease was removed.
pub fn force_release_project_lease(state_dir: &Path, job: &str, wait: Duration) -> Result<bool, ProjectLeaseError> {
    let _lock = lock_board(&lock_path(state_dir), wait)?;
    let path = lease_path(state_dir);
    match read_lease(&path) {
        Ok(Some(l)) if l.job == job => {
            remove_lease_file(&path)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// Reads the lease without locking, for display.
pub fn project_lease_status(state_dir: &Path, now: f64) -> ProjectLeaseStatus {
    match read_lease(&lease_path(state_dir)) {
        Ok(None) => ProjectLeaseStatus::Free,
        Ok(Some(l)) if l.expires > now && !owner_is_dead(&l.owner) => {
            ProjectLeaseStatus::Held { owner: l.owner, job: l.job, secs_left: (l.expires - now) as u64 }
        }
        Ok(Some(l)) => ProjectLeaseStatus::Stale { owner: l.owner, job: l.job },
        Err(reason) => ProjectLeaseStatus::Unreadable(reason),
    }
}
