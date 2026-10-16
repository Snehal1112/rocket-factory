//! Reads committed files at the tip of a local branch. The working tree is never looked at.

use crate::error::{GitError, Result};
use crate::repo::Repo;
use git2::{BranchType, ErrorCode, ObjectType, Tree, TreeEntry};
use std::path::Path;

/// The tree entry at `rel`. Only a plain NotFound means absent. Any other error
/// (a missing or corrupt object, a path through a file) is an error, so the caller fails closed.
fn entry_at(tree: &Tree<'_>, rel: &str) -> Result<Option<TreeEntry<'static>>> {
    match tree.get_path(Path::new(rel)) {
        Ok(e) => Ok(Some(e)),
        Err(e) if e.code() == ErrorCode::NotFound && e.class() == git2::ErrorClass::Tree => Ok(None),
        Err(e) => Err(GitError::Git(e)),
    }
}

impl Repo {
    fn tree_at_branch(&self, branch: &str) -> Result<Tree<'_>> {
        let b = self
            .inner
            .find_branch(branch, BranchType::Local)
            .map_err(|_| GitError::NotFound(format!("branch {branch}")))?;
        let commit = b.get().peel_to_commit()?;
        Ok(commit.tree()?)
    }

    /// The text of the file `rel` at the tip of `branch`, or `None` when the file is absent.
    pub fn file_at_branch(&self, branch: &str, rel: &str) -> Result<Option<String>> {
        let tree = self.tree_at_branch(branch)?;
        let Some(entry) = entry_at(&tree, rel)? else {
            return Ok(None);
        };
        if entry.kind() != Some(ObjectType::Blob) {
            return Err(GitError::Refused(format!("{rel} is not a regular file")));
        }
        let blob = self.inner.find_blob(entry.id())?;
        let text = std::str::from_utf8(blob.content())
            .map_err(|_| GitError::BadInput(format!("{rel} is not valid UTF-8")))?;
        Ok(Some(text.to_string()))
    }

    /// The regular files directly inside the folder `rel` at the tip of `branch`, as (name, text).
    /// An absent folder gives an empty list.
    pub fn dir_at_branch(&self, branch: &str, rel: &str) -> Result<Vec<(String, String)>> {
        let tree = self.tree_at_branch(branch)?;
        let Some(entry) = entry_at(&tree, rel)? else {
            return Ok(Vec::new());
        };
        if entry.kind() != Some(ObjectType::Tree) {
            return Err(GitError::Refused(format!("{rel} is not a folder")));
        }
        let sub = self.inner.find_tree(entry.id())?;
        let mut out = Vec::new();
        for item in sub.iter() {
            if item.kind() != Some(ObjectType::Blob) {
                continue;
            }
            let name = item
                .name()
                .ok_or_else(|| GitError::BadInput(format!("a file name in {rel} is not valid UTF-8")))?;
            let blob = self.inner.find_blob(item.id())?;
            let text = std::str::from_utf8(blob.content())
                .map_err(|_| GitError::BadInput(format!("{rel}/{name} is not valid UTF-8")))?;
            out.push((name.to_string(), text.to_string()));
        }
        Ok(out)
    }
}
