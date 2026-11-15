//! The project lease: at most one active job per project, held in a small file next to the board.

use std::path::{Path, PathBuf};
use std::time::Duration;

use factory_core::{write_atomic, PROJECT_LEASE_FILE};
use serde::{Deserialize, Serialize};

use crate::procinfo::ProcState;
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
    Held {
        owner: String,
        job: String,
        secs_left: u64,
    },
    #[error(
        "project lease file {path} is unreadable ({reason}). Nothing was changed. \
         `factory cancel <id> --force` removes it and gives up that job, or delete the file to clear it"
    )]
    Corrupt { path: String, reason: String },
    #[error("project lease lock: {0}")]
    Lock(#[from] StoreError),
    #[error("project lease io: {0}")]
    Io(String),
}

/// What the lease file says right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectLeaseStatus {
    Free,
    Held {
        owner: String,
        job: String,
        secs_left: u64,
    },
    Stale {
        owner: String,
        job: String,
    },
    Unreadable(String),
}

/// The pid in an owner of the form `driver-<pid>-<secs>`. Any other owner has none.
pub fn owner_pid(owner: &str) -> Option<u32> {
    owner_parts(owner).map(|(pid, _)| pid)
}

fn owner_parts(owner: &str) -> Option<(u32, u64)> {
    let rest = owner.strip_prefix("driver-")?;
    let (pid, secs) = rest.split_once('-')?;
    Some((pid.parse().ok()?, secs.parse().ok()?))
}

/// How far the start time of a pid may differ from the seconds in the owner name.
pub const START_SLACK_SECS: u64 = 10;

/// What this machine says about the process behind a lease owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerState {
    /// The driver process runs and started when its name says.
    Alive,
    /// The pid is gone, a zombie, or reused by another process.
    Dead,
    /// The owner has no pid, or this machine cannot say when the pid started.
    Unknown,
}

/// The state of the owner of a lease. Only an owner of the form `driver-<pid>-<secs>` can be judged.
pub fn owner_state(owner: &str) -> OwnerState {
    let Some((pid, secs)) = owner_parts(owner) else {
        return OwnerState::Unknown;
    };
    if pid == std::process::id() {
        return OwnerState::Alive;
    }
    match crate::procinfo::process_state(pid) {
        ProcState::Gone | ProcState::Zombie => OwnerState::Dead,
        ProcState::Running => match process_start_secs(pid) {
            None => OwnerState::Unknown,
            Some(start) if start.abs_diff(secs) <= START_SLACK_SECS => OwnerState::Alive,
            Some(_) => OwnerState::Dead,
        },
    }
}

/// True when a lease is still held: a live owner holds it past expiry, a dead one never, and an owner
/// that cannot be judged holds it until it expires.
pub fn lease_held(owner: &str, expires: f64, now: f64) -> bool {
    match owner_state(owner) {
        OwnerState::Alive => true,
        OwnerState::Dead => false,
        OwnerState::Unknown => expires > now,
    }
}

/// When the pid started, in whole seconds since the epoch. None when this machine does not say.
pub fn process_start_secs(pid: u32) -> Option<u64> {
    crate::procinfo::process_start_ms(pid).map(|ms| ms / 1000)
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
        Ok(text) => serde_json::from_str::<ProjectLease>(&text)
            .map(Some)
            .map_err(|e| format!("not valid lease JSON: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Takes or renews the lease. A lease that is free, ours, or no longer held can be taken. A corrupt file is refused.
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
        Ok(Some(l)) if !lease_held(&l.owner, l.expires, now) => LeaseTake::TookOverStale {
            owner: l.owner,
            job: l.job,
        },
        Ok(Some(l)) => {
            return Err(ProjectLeaseError::Held {
                owner: l.owner,
                job: l.job,
                secs_left: (l.expires - now) as u64,
            });
        }
        // A corrupt file is never replaced silently: a person decides who held it.
        Err(reason) => {
            return Err(ProjectLeaseError::Corrupt {
                path: path.display().to_string(),
                reason,
            });
        }
    };
    let lease = ProjectLease {
        owner: owner.to_string(),
        job: job.to_string(),
        expires: now + ttl,
    };
    let bytes =
        serde_json::to_vec_pretty(&lease).map_err(|e| ProjectLeaseError::Io(e.to_string()))?;
    write_atomic(&path, &bytes).map_err(|e| ProjectLeaseError::Io(e.to_string()))?;
    Ok(take)
}

/// Removes the lease when it is ours, expired or its owner is dead. A live lease of another owner is left
/// alone, and so is an unreadable file: only a forced cancel or a person removes that.
pub fn release_project_lease(
    state_dir: &Path,
    owner: &str,
    wait: Duration,
) -> Result<(), ProjectLeaseError> {
    let _lock = lock_board(&lock_path(state_dir), wait)?;
    let path = lease_path(state_dir);
    let removable = match read_lease(&path) {
        Ok(None) => return Ok(()),
        Ok(Some(l)) => l.owner == owner || !lease_held(&l.owner, l.expires, now_secs()),
        Err(_) => false,
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

/// Removes the lease when it belongs to `job`, whoever the owner is, and removes an unreadable file too.
/// Used when a job is cancelled with force, because the process that held the lease may be gone.
/// Returns true when a lease was removed.
pub fn force_release_project_lease(
    state_dir: &Path,
    job: &str,
    wait: Duration,
) -> Result<bool, ProjectLeaseError> {
    let _lock = lock_board(&lock_path(state_dir), wait)?;
    let path = lease_path(state_dir);
    match read_lease(&path) {
        Ok(None) => Ok(false),
        Ok(Some(l)) if l.job != job => Ok(false),
        Ok(Some(_)) | Err(_) => {
            remove_lease_file(&path)?;
            Ok(true)
        }
    }
}

/// Reads the lease without locking, for display.
pub fn project_lease_status(state_dir: &Path, now: f64) -> ProjectLeaseStatus {
    match read_lease(&lease_path(state_dir)) {
        Ok(None) => ProjectLeaseStatus::Free,
        Ok(Some(l)) if lease_held(&l.owner, l.expires, now) => ProjectLeaseStatus::Held {
            owner: l.owner,
            job: l.job,
            secs_left: (l.expires - now) as u64,
        },
        Ok(Some(l)) => ProjectLeaseStatus::Stale {
            owner: l.owner,
            job: l.job,
        },
        Err(reason) => ProjectLeaseStatus::Unreadable(reason),
    }
}
