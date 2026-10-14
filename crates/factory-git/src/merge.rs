use crate::error::Result;
use crate::repo::{short, Repo, MAIN_BRANCH};
use crate::state::Preflight;
use git2::build::CheckoutBuilder;
use git2::{Commit, ErrorClass, ErrorCode, Repository, Signature};
use std::collections::BTreeSet;

/// What a merge did. Conflicts and refusals are values, not errors, so the
/// driver can park the job with a reason instead of crashing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeOutcome {
    /// A new two-parent merge commit. For a sync this is the new job head.
    Merged { merge_sha: String },
    /// Nothing to merge, the target already contains the other side.
    UpToDate,
    /// The merge conflicts. Nothing was changed on disk or in any ref.
    Conflict { paths: Vec<String> },
    /// A safety check failed. Nothing was changed.
    Refused { reason: String },
}

pub(crate) fn signature(repo: &Repository) -> Result<Signature<'static>> {
    match repo.signature() {
        Ok(sig) => Ok(sig),
        Err(_) => Ok(Signature::now("factory", "factory@localhost")?),
    }
}

/// Puts the working tree and index back on `ours` after a failed checkout.
fn restore(repo: &Repository, ours: &Commit<'_>) {
    let mut cb = CheckoutBuilder::new();
    cb.force();
    // Best effort. The original error is the one the caller sees.
    let _ = repo.checkout_tree(ours.as_object(), Some(&mut cb));
}

/// Merges `theirs` into `ours` and moves `branch_ref` to the result.
///
/// The merge is computed in memory and the commit is created without touching
/// any ref. Parents are verified, then a safe checkout updates the working
/// tree (it refuses to overwrite untracked or modified files), and only then
/// the branch ref moves. So every failure before the last step leaves the
/// branch where it was. `repo` must have `branch_ref` checked out.
pub(crate) fn commit_merge(
    repo: &Repository,
    branch_ref: &str,
    ours: &Commit<'_>,
    theirs: &Commit<'_>,
    message: &str,
) -> Result<MergeOutcome> {
    let mut index = repo.merge_commits(ours, theirs, None)?;
    if index.has_conflicts() {
        let mut paths = BTreeSet::new();
        for conflict in index.conflicts()? {
            let conflict = conflict?;
            for entry in [conflict.our, conflict.their, conflict.ancestor].into_iter().flatten() {
                paths.insert(String::from_utf8_lossy(&entry.path).into_owned());
            }
        }
        return Ok(MergeOutcome::Conflict { paths: paths.into_iter().collect() });
    }
    let tree = repo.find_tree(index.write_tree_to(repo)?)?;
    let sig = signature(repo)?;
    let new_id = repo.commit(None, &sig, &sig, message, &tree, &[ours, theirs])?;
    let created = repo.find_commit(new_id)?;
    let parents_ok = created.parent_count() == 2
        && created.parent_id(0)? == ours.id()
        && created.parent_id(1)? == theirs.id();
    if !parents_ok {
        return Ok(MergeOutcome::Refused {
            reason: "the merge commit has the wrong parents, nothing was changed".into(),
        });
    }

    let mut cb = CheckoutBuilder::new();
    cb.safe();
    if let Err(e) = repo.checkout_tree(created.as_object(), Some(&mut cb)) {
        if e.code() == ErrorCode::Conflict || e.class() == ErrorClass::Checkout {
            return Ok(MergeOutcome::Refused {
                reason: format!("local files would be overwritten: {}", e.message()),
            });
        }
        restore(repo, ours);
        return Err(e.into());
    }
    let moved = repo
        .find_reference(branch_ref)
        .and_then(|mut r| r.set_target(new_id, "factory: merge").map(|_| ()));
    if let Err(e) = moved {
        restore(repo, ours);
        return Err(e.into());
    }
    Ok(MergeOutcome::Merged { merge_sha: new_id.to_string() })
}

impl Repo {
    /// Merges the reviewed head of `branch` into main with a no-ff merge commit.
    ///
    /// Refuses when the branch tip is not exactly `head_sha`, when main is not
    /// ready (see `preflight_main`) or when the head is already on main. Call
    /// `base_state` first and merge only on `Current`.
    pub fn merge_no_ff(&self, branch: &str, head_sha: &str, message: &str) -> Result<MergeOutcome> {
        let tip = self.tip(branch)?;
        let head = self.commit_of(head_sha)?;
        let head_id = head.id().to_string();
        if tip != head_id {
            return Ok(MergeOutcome::Refused {
                reason: format!(
                    "branch {branch} is at {} but the reviewed head is {}",
                    short(&tip),
                    short(&head_id)
                ),
            });
        }
        if let Preflight::Busy { reason } = self.preflight_main()? {
            return Ok(MergeOutcome::Refused { reason });
        }
        let main = self.branch_commit(MAIN_BRANCH)?;
        if self.is_ancestor(&head_id, &main.id().to_string())? {
            return Ok(MergeOutcome::Refused { reason: "the head is already on main".into() });
        }
        let branch_ref = format!("refs/heads/{MAIN_BRANCH}");
        commit_merge(&self.inner, &branch_ref, &main, &head, message)
    }
}
