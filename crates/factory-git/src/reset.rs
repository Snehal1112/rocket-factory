use crate::error::Result;
use crate::repo::Repo;
use crate::worktree::Worktree;
use git2::build::CheckoutBuilder;
use git2::{Repository, ResetType};

impl Repo {
    /// Moves the job branch to `rev` and keeps the index and the files, so the work above `rev` stays staged.
    pub fn reset_soft(&self, wt: &Worktree, rev: &str) -> Result<()> {
        let inside = Repository::open(&wt.path)?;
        let target = inside.revparse_single(rev)?;
        inside.reset(&target, ResetType::Soft, None)?;
        Ok(())
    }

    /// Moves the job branch to `rev` and makes the index and the files match it.
    pub fn reset_hard(&self, wt: &Worktree, rev: &str) -> Result<()> {
        let inside = Repository::open(&wt.path)?;
        let target = inside.revparse_single(rev)?;
        inside.reset(&target, ResetType::Hard, None)?;
        Ok(())
    }

    /// Makes the index and the files match the tree of `rev` and leaves the branch where it is.
    pub fn restore_tree(&self, wt: &Worktree, rev: &str) -> Result<()> {
        let inside = Repository::open(&wt.path)?;
        let commit = inside.revparse_single(rev)?.peel_to_commit()?;
        let mut opts = CheckoutBuilder::new();
        opts.force();
        inside.checkout_tree(commit.tree()?.as_object(), Some(&mut opts))?;
        Ok(())
    }
}
