use crate::error::Result;
use crate::repo::Repo;
use crate::worktree::Worktree;
use git2::{IndexAddOption, Repository, Status, StatusOptions};

/// Options that see untracked files (but not ignored ones), like `git status`.
fn status_options() -> StatusOptions {
    let mut opts = StatusOptions::new();
    opts.include_untracked(true).recurse_untracked_dirs(true).include_ignored(false);
    opts
}

/// Label and path of every staged change, sorted by path.
pub(crate) fn staged_entries(inside: &Repository) -> Result<Vec<(&'static str, String)>> {
    let mut entries = Vec::new();
    for entry in inside.statuses(Some(&mut status_options()))?.iter() {
        let status = entry.status();
        let label = if status.contains(Status::INDEX_NEW) {
            "new file:"
        } else if status.contains(Status::INDEX_DELETED) {
            "deleted:"
        } else if status.contains(Status::INDEX_RENAMED) {
            "renamed:"
        } else if status.contains(Status::INDEX_TYPECHANGE) {
            "typechange:"
        } else if status.contains(Status::INDEX_MODIFIED) {
            "modified:"
        } else {
            continue;
        };
        if let Some(path) = entry.path() {
            entries.push((label, path.to_string()));
        }
    }
    entries.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(entries)
}

impl Repo {
    /// Stages every change in the job worktree that is not ignored: new, modified
    /// and deleted files. Returns the sorted staged paths. The worktree belongs to
    /// one job and one implementer, so every change belongs to the current task.
    pub fn stage_all(&self, wt: &Worktree) -> Result<Vec<String>> {
        let inside = Repository::open(&wt.path)?;
        let mut index = inside.index()?;
        index.add_all(["*"], IndexAddOption::DEFAULT, None)?;
        index.update_all(["*"], None)?;
        index.write()?;
        Ok(staged_entries(&inside)?.into_iter().map(|(_, path)| path).collect())
    }

    /// True when the worktree has no staged, unstaged or untracked (not ignored)
    /// changes.
    pub fn worktree_is_clean(&self, wt: &Worktree) -> Result<bool> {
        let inside = Repository::open(&wt.path)?;
        let clean = inside.statuses(Some(&mut status_options()))?.is_empty();
        Ok(clean)
    }
}
