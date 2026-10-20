use factory_core::Job;

use crate::lease_owner_dead;
use crate::{BoardStore, StoreError};

pub use factory_core::Lease;

#[derive(Debug, thiserror::Error)]
pub enum LeaseError {
    #[error("job is leased by {owner} for another {secs_left} seconds")]
    Held { owner: String, secs_left: u64 },
    #[error("{0}")]
    Store(#[from] StoreError),
}

pub fn lease_is_live(job: &Job, now: f64) -> bool {
    job.lease
        .as_ref()
        .is_some_and(|l| l.expires > now && !lease_owner_dead(&l.owner))
}

fn check_free(job: &Job, owner: &str, now: f64) -> Result<(), LeaseError> {
    match &job.lease {
        Some(l) if lease_is_live(job, now) && l.owner != owner => Err(LeaseError::Held {
            owner: l.owner.clone(),
            secs_left: (l.expires - now) as u64,
        }),
        _ => Ok(()),
    }
}

/// Takes or renews the lease. A lease that is expired, or whose driver process is gone, can be taken over by anyone.
pub fn acquire_lease(
    store: &BoardStore,
    id: &str,
    owner: &str,
    ttl: f64,
    now: f64,
) -> Result<(), LeaseError> {
    // The closure's own Result carries the lease verdict, so a refused lease still saves an unchanged board.
    store.update_job(id, |job| {
        Ok(check_free(job, owner, now).map(|()| {
            job.lease = Some(Lease {
                owner: owner.to_string(),
                expires: now + ttl,
            });
        }))
    })?
}

/// Drops the lease. Another owner's live lease is left alone and reported; a dead owner's lease is not live.
pub fn release_lease(
    store: &BoardStore,
    id: &str,
    owner: &str,
    now: f64,
) -> Result<(), LeaseError> {
    store.update_job(id, |job| {
        Ok(check_free(job, owner, now).map(|()| {
            job.lease = None;
        }))
    })?
}
