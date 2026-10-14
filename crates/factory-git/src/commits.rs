use crate::error::{GitError, Result};
use crate::merge::signature;
use crate::repo::{paths_of, Repo};
use crate::worktree::Worktree;
use git2::{Repository, Sort};

/// One commit above a floor: its sha, subject line and changed files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitInfo {
    /// The full commit sha.
    pub sha: String,
    /// The first line of the commit message.
    pub subject: String,
    /// The paths changed against the first parent, sorted.
    pub files: Vec<String>,
}

impl Repo {
    /// The commits in `floor..tip`, oldest first, each with the files it changed
    /// against its first parent. Empty when `floor` is `tip` or one of its
    /// descendants. Follows first parents only and skips merge commits, so a sync
    /// of main into the job branch adds nothing to the list.
    pub fn commits_above(&self, floor: &str, tip: &str) -> Result<Vec<CommitInfo>> {
        let mut walk = self.inner.revwalk()?;
        walk.push(self.commit_of(tip)?.id())?;
        walk.hide(self.commit_of(floor)?.id())?;
        walk.simplify_first_parent()?;
        walk.set_sorting(Sort::TOPOLOGICAL | Sort::REVERSE)?;
        let mut commits = Vec::new();
        for oid in walk {
            let commit = self.inner.find_commit(oid?)?;
            if commit.parent_count() > 1 {
                continue;
            }
            let tree = commit.tree()?;
            let parent_tree = if commit.parent_count() == 0 {
                None
            } else {
                Some(commit.parent(0)?.tree()?)
            };
            let diff = self.inner.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)?;
            commits.push(CommitInfo {
                sha: commit.id().to_string(),
                subject: commit.summary().unwrap_or("").to_string(),
                files: paths_of(&diff),
            });
        }
        Ok(commits)
    }

    /// Commits the index of the job worktree on its current branch and returns the
    /// new sha. This is the plain `git2` fallback commit, used when the committer
    /// agent fails. Refused when nothing is staged.
    pub fn commit_staged(&self, wt: &Worktree, message: &str) -> Result<String> {
        let inside = Repository::open(&wt.path)?;
        let mut index = inside.index()?;
        let tree = inside.find_tree(index.write_tree()?)?;
        let parent = inside.head()?.peel_to_commit()?;
        if parent.tree_id() == tree.id() {
            return Err(GitError::Refused("nothing is staged to commit".into()));
        }
        let sig = signature(&inside)?;
        let id = inside.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&parent])?;
        Ok(id.to_string())
    }
}
