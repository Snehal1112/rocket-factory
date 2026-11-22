use crate::error::{GitError, Result};
use crate::merge::signature;
use crate::repo::{paths_of, Repo};
use crate::sign::signer_for;
use crate::worktree::Worktree;
use git2::{Commit, Oid, Repository, Signature, Sort, Tree};

/// A commit the driver made: its sha, and why it is unsigned when
/// `commit.gpgsign` is on but signing failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Committed {
    /// The full commit sha.
    pub sha: String,
    /// The reason the commit is unsigned, or None.
    pub unsigned: Option<String>,
}

/// One commit above a floor: its sha, subject line and changed files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitInfo {
    /// The full commit sha.
    pub sha: String,
    /// The first line of the commit message.
    pub subject: String,
    /// The full commit message, subject and body.
    pub message: String,
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
            let diff = self
                .inner
                .diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)?;
            commits.push(CommitInfo {
                sha: commit.id().to_string(),
                subject: commit.summary().unwrap_or("").to_string(),
                message: commit.message().unwrap_or("").to_string(),
                files: paths_of(&diff),
            });
        }
        Ok(commits)
    }

    /// Commits the index of the job worktree on its current branch and returns the
    /// new sha. The driver uses it for its `wip:` snapshot commits. The commit is
    /// signed when `commit.gpgsign` is on. Refused when nothing is staged.
    pub fn commit_staged(&self, wt: &Worktree, message: &str) -> Result<Committed> {
        let inside = Repository::open(&wt.path)?;
        let mut index = inside.index()?;
        let tree = inside.find_tree(index.write_tree()?)?;
        let parent = inside.head()?.peel_to_commit()?;
        if parent.tree_id() == tree.id() {
            return Err(GitError::Refused("nothing is staged to commit".into()));
        }
        let sig = signature(&inside)?;
        let (id, unsigned) = write_commit(&inside, Some("HEAD"), &sig, message, &tree, &[&parent])?;
        Ok(Committed {
            sha: id.to_string(),
            unsigned,
        })
    }
}

/// Writes a commit, signed when the repository asks for it. Returns its id and
/// the reason it is unsigned, if signing was on but failed.
///
/// A signed commit is written without moving a ref, then `update_ref` is moved
/// only when it still points at the first parent, as `Repository::commit` does.
pub(crate) fn write_commit(
    repo: &Repository,
    update_ref: Option<&str>,
    sig: &Signature<'_>,
    message: &str,
    tree: &Tree<'_>,
    parents: &[&Commit<'_>],
) -> Result<(Oid, Option<String>)> {
    let reason = match signer_for(repo) {
        None => {
            let id = repo.commit(update_ref, sig, sig, message, tree, parents)?;
            return Ok((id, None));
        }
        Some(Err(reason)) => reason,
        Some(Ok(signer)) => {
            let buffer = repo.commit_create_buffer(sig, sig, message, tree, parents)?;
            let signed = buffer
                .as_str()
                .ok_or_else(|| "the commit is not valid utf-8".to_string())
                .and_then(|text| signer.sign(text).map(|signature| (text, signature)));
            match signed {
                Ok((text, signature)) => {
                    let id = repo.commit_signed(text, &signature, Some("gpgsig"))?;
                    if let Some(name) = update_ref {
                        move_ref(repo, name, id, parents, message)?;
                    }
                    return Ok((id, None));
                }
                Err(reason) => reason,
            }
        }
    };
    let id = repo.commit(update_ref, sig, sig, message, tree, parents)?;
    Ok((id, Some(reason)))
}

/// Moves `name` (or the branch `HEAD` points to) to `id`, expecting it to be at
/// the first parent.
fn move_ref(
    repo: &Repository,
    name: &str,
    id: Oid,
    parents: &[&Commit<'_>],
    message: &str,
) -> Result<()> {
    let expected = parents
        .first()
        .ok_or_else(|| GitError::Refused("a signed commit needs a parent".into()))?
        .id();
    let target = if name == "HEAD" {
        let head = repo.find_reference("HEAD")?;
        head.symbolic_target()
            .ok_or_else(|| GitError::Refused("HEAD is not on a branch".into()))?
            .to_string()
    } else {
        name.to_string()
    };
    let subject = message.lines().next().unwrap_or_default();
    repo.reference_matching(&target, id, true, expected, &format!("commit: {subject}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn a_signed_snapshot_refuses_a_moved_branch() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let repo = Repository::init(dir.path()).expect("init");
        let gpg = dir.path().join("fake-gpg");
        std::fs::write(
            &gpg,
            "#!/bin/sh\ncat > /dev/null\necho '[GNUPG:] SIG_CREATED D 22 8 00 1 ABC' >&2\necho SIG\n",
        )
        .expect("write script");
        std::fs::set_permissions(&gpg, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let mut cfg = repo.config().expect("config");
        cfg.set_bool("commit.gpgsign", true).expect("gpgsign");
        cfg.set_str("gpg.format", "openpgp").expect("format");
        cfg.set_str("gpg.program", gpg.to_str().expect("utf8"))
            .expect("program");
        cfg.set_str("user.signingkey", "ABC").expect("key");
        let sig = Signature::now("t", "t@t").expect("sig");
        let tree = repo
            .find_tree(repo.index().expect("index").write_tree().expect("tree"))
            .expect("find tree");
        let first = repo
            .commit(Some("HEAD"), &sig, &sig, "one", &tree, &[])
            .expect("one");
        let first = repo.find_commit(first).expect("first");
        let second = repo
            .commit(Some("HEAD"), &sig, &sig, "two", &tree, &[&first])
            .expect("two");

        let refused = write_commit(&repo, Some("HEAD"), &sig, "three", &tree, &[&first]);
        assert!(refused.is_err());
        let tip = repo
            .head()
            .expect("head")
            .peel_to_commit()
            .expect("tip")
            .id();
        assert_eq!(tip, second);
    }
}
