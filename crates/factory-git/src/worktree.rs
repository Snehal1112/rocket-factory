use crate::error::{GitError, Result};
use crate::merge::{commit_merge, MergeOutcome};
use crate::repo::{Repo, MAIN_BRANCH};
use git2::{BranchType, Repository, StatusOptions, WorktreeAddOptions, WorktreePruneOptions};
use std::fs;
use std::path::{Path, PathBuf};

/// The two fields of a job that name its branch and worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRef {
    pub id: String,
    pub slug: String,
}

impl JobRef {
    /// `factory/NNN-<slug>`.
    pub fn branch_name(&self) -> String {
        format!("factory/{}-{}", self.id, self.slug)
    }

    /// `factory-NNN-<slug>`, the worktree directory and git worktree name.
    pub fn dir_name(&self) -> String {
        format!("factory-{}-{}", self.id, self.slug)
    }

    fn validate(&self) -> Result<()> {
        let id_ok = !self.id.is_empty() && self.id.chars().all(|c| c.is_ascii_digit());
        let slug_ok = !self.slug.is_empty()
            && self.slug.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if id_ok && slug_ok {
            Ok(())
        } else {
            Err(GitError::BadInput(format!(
                "job id {:?} must be digits and slug {:?} must be lower case letters, digits and dashes",
                self.id, self.slug
            )))
        }
    }
}

/// A linked worktree for one job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub name: String,
    pub path: PathBuf,
    pub branch: String,
    /// The main sha the branch was created from.
    pub base: String,
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

impl Repo {
    /// Creates branch `factory/NNN-<slug>` from main and a worktree for it at
    /// `.claude/worktrees/factory-NNN-<slug>`. The builder owns its cargo target dir.
    pub fn create_worktree(&self, job: &JobRef) -> Result<Worktree> {
        job.validate()?;
        let branch = job.branch_name();
        let name = job.dir_name();
        let parent = self.root.join(".claude").join("worktrees");
        let path = parent.join(&name);
        if path.exists() {
            return Err(GitError::Refused(format!("{} already exists", path.display())));
        }
        if self.inner.find_branch(&branch, BranchType::Local).is_ok() {
            return Err(GitError::Refused(format!("branch {branch} already exists")));
        }
        let main = self.branch_commit(MAIN_BRANCH)?;
        fs::create_dir_all(&parent)?;
        let mut created = self.inner.branch(&branch, &main, false)?;
        let added = {
            let mut opts = WorktreeAddOptions::new();
            opts.reference(Some(created.get()));
            self.inner.worktree(&name, &path, Some(&opts)).map(|_| ())
        };
        if let Err(e) = added {
            // Do not leave a stray branch behind.
            let _ = created.delete();
            return Err(e.into());
        }
        Ok(Worktree { name, path, branch, base: main.id().to_string() })
    }

    /// Removes a job worktree and its branch. Refuses a dirty worktree and
    /// unmerged commits unless `force`. Never removes main, a branch outside
    /// `factory/`, the main checkout, or the branch the main checkout is on.
    pub fn remove_worktree(&self, wt: &Worktree, force: bool) -> Result<()> {
        if wt.branch == MAIN_BRANCH || !wt.branch.starts_with("factory/") {
            return Err(GitError::Refused(format!("{} is not a factory job branch", wt.branch)));
        }
        if same_dir(&wt.path, &self.root) {
            return Err(GitError::Refused("the worktree is the main checkout".into()));
        }
        let head = self.inner.head()?;
        if head.shorthand() == Some(wt.branch.as_str()) {
            return Err(GitError::Refused("the main checkout is on the job branch".into()));
        }
        if !force {
            if wt.path.is_dir() {
                let inside = Repository::open(&wt.path)?;
                let mut opts = StatusOptions::new();
                opts.include_untracked(true).recurse_untracked_dirs(true).include_ignored(false);
                if !inside.statuses(Some(&mut opts))?.is_empty() {
                    return Err(GitError::Refused(format!(
                        "worktree {} has uncommitted or untracked work",
                        wt.path.display()
                    )));
                }
            }
            if let Ok(tip) = self.tip(&wt.branch) {
                let main = self.tip(MAIN_BRANCH)?;
                if !self.is_ancestor(&tip, &main)? {
                    return Err(GitError::Refused(format!(
                        "branch {} has commits that are not on main",
                        wt.branch
                    )));
                }
            }
        }
        if let Ok(found) = self.inner.find_worktree(&wt.name) {
            let mut opts = WorktreePruneOptions::new();
            opts.valid(true).working_tree(true);
            found.prune(Some(&mut opts))?;
        }
        if let Ok(mut branch) = self.inner.find_branch(&wt.branch, BranchType::Local) {
            branch.delete()?;
        }
        Ok(())
    }

    /// Merges main into the job branch inside its worktree. No rebase and no
    /// history rewrite, the branch only gains a merge commit. A conflict is
    /// returned as a value and the worktree is left as it was.
    pub fn merge_main_into(&self, wt: &Worktree, message: &str) -> Result<MergeOutcome> {
        let refuse = |reason: &str| Ok(MergeOutcome::Refused { reason: reason.to_string() });
        let inside = Repository::open(&wt.path)?;
        let head = inside.head()?;
        if wt.branch == MAIN_BRANCH || !head.is_branch() || head.shorthand() != Some(wt.branch.as_str()) {
            return refuse("the worktree is not on the job branch");
        }
        if inside.state() != git2::RepositoryState::Clean {
            return refuse("an operation is already in progress in the worktree");
        }
        let mut opts = StatusOptions::new();
        opts.include_untracked(false).include_ignored(false);
        if !inside.statuses(Some(&mut opts))?.is_empty() {
            return refuse("the worktree has tracked changes");
        }
        let ours = head.peel_to_commit()?;
        let theirs = inside.find_commit(self.branch_commit(MAIN_BRANCH)?.id())?;
        if ours.id() == theirs.id() || inside.graph_descendant_of(ours.id(), theirs.id())? {
            return Ok(MergeOutcome::UpToDate);
        }
        let branch_ref = format!("refs/heads/{}", wt.branch);
        commit_merge(&inside, &branch_ref, &ours, &theirs, message)
    }
}
