use crate::error::{GitError, Result};
use crate::worktree::JobRef;
use git2::{BranchType, Commit, Diff, ErrorCode, Repository};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The base branch of a repo opened without `with_base_branch`.
pub const DEFAULT_BASE_BRANCH: &str = "main";

/// Handle on the main checkout of a repository.
pub struct Repo {
    pub(crate) inner: Repository,
    pub(crate) root: PathBuf,
    pub(crate) base: String,
    pub(crate) worktrees: PathBuf,
    /// The subfolder project name, `None` for a top-folder project.
    pub(crate) project: Option<String>,
    /// The project folder relative to the repo root.
    pub(crate) subpath: Option<PathBuf>,
}

/// First 7 characters of a sha, or the whole string when it is shorter.
pub(crate) fn short(sha: &str) -> &str {
    sha.get(..7).map_or(sha, |s| s)
}

impl Repo {
    /// Opens the repository that contains `path`. From inside a linked worktree it
    /// resolves to the main checkout, like `git rev-parse --git-common-dir` did.
    pub fn open(path: &Path) -> Result<Repo> {
        let found = Repository::discover(path)?;
        let repo = if found.is_worktree() {
            // git2 0.19 has no commondir(). A linked worktree's git dir is
            // `<main>/.git/worktrees/<name>`, so three parents up is the checkout.
            let top = found
                .path()
                .parent()
                .and_then(Path::parent)
                .and_then(Path::parent)
                .ok_or_else(|| GitError::NotFound("main checkout of the worktree".into()))?;
            Repository::open(top)?
        } else {
            found
        };
        let root = repo
            .workdir()
            .ok_or_else(|| GitError::Refused("bare repositories are not supported".into()))?
            .to_path_buf();
        let worktrees = root.join(".claude").join("worktrees");
        Ok(Repo {
            inner: repo,
            root,
            base: DEFAULT_BASE_BRANCH.to_string(),
            worktrees,
            project: None,
            subpath: None,
        })
    }

    /// Like `open`, but refuses a path that is not the top folder of the repository.
    pub fn open_top(path: &Path) -> Result<Repo> {
        let repo = Repo::open(path)?;
        let same = std::fs::canonicalize(repo.root()).ok() == std::fs::canonicalize(path).ok();
        if !same {
            return Err(GitError::NotTopFolder(path.to_path_buf()));
        }
        Ok(repo)
    }

    /// The same repo scoped to a subfolder project.
    pub fn with_scope(mut self, project: &str, subpath: &Path) -> Repo {
        self.project = Some(project.to_string());
        self.subpath = Some(subpath.to_path_buf());
        self
    }

    /// The subfolder project name, if the repo is scoped.
    pub fn project(&self) -> Option<&str> {
        self.project.as_deref()
    }

    /// The project folder relative to the repo root, if the repo is scoped.
    pub fn subpath(&self) -> Option<&Path> {
        self.subpath.as_deref()
    }

    /// The project folder inside a job worktree, or the worktree itself without a scope.
    pub fn workdir_in(&self, worktree: &Path) -> PathBuf {
        match &self.subpath {
            Some(sub) => worktree.join(sub),
            None => worktree.to_path_buf(),
        }
    }

    /// The job worktree folder under `worktrees_dir()` that holds `path`.
    pub fn worktree_root_of(&self, path: &Path) -> Option<PathBuf> {
        let rest = path.strip_prefix(&self.worktrees).ok()?;
        let first = rest.components().next()?;
        Some(self.worktrees.join(first))
    }

    /// The paths that are not under the project folder. Empty without a scope.
    pub fn out_of_scope(&self, paths: &[String]) -> Vec<String> {
        match &self.subpath {
            Some(sub) => paths
                .iter()
                .filter(|p| !Path::new(p).starts_with(sub))
                .cloned()
                .collect(),
            None => Vec::new(),
        }
    }

    /// The paths with the project folder removed. Unchanged without a scope.
    pub fn scoped_paths(&self, paths: &[String]) -> Vec<String> {
        match &self.subpath {
            Some(sub) => paths
                .iter()
                .map(|p| match Path::new(p).strip_prefix(sub) {
                    Ok(rest) => rest.to_string_lossy().into_owned(),
                    Err(_) => p.clone(),
                })
                .collect(),
            None => paths.to_vec(),
        }
    }

    /// A job reference for this repo's scope.
    pub fn job_ref(&self, id: &str, slug: &str) -> JobRef {
        JobRef {
            id: id.to_string(),
            slug: slug.to_string(),
            project: self.project.clone(),
        }
    }

    /// The factory branches this repo's project owns, as `factory/<id>-<slug>`.
    /// With a scope only `factory/<project>/` branches count. Without one, branches
    /// of subfolder projects (`factory/<name>/...`) are dropped.
    pub fn own_branch_names(&self) -> Result<Vec<String>> {
        let all = self.local_branch_names()?;
        Ok(match &self.project {
            Some(p) => {
                let prefix = format!("factory/{p}/");
                all.iter()
                    .filter_map(|n| n.strip_prefix(&prefix))
                    .map(|rest| format!("factory/{rest}"))
                    .collect()
            }
            None => all
                .into_iter()
                .filter(|n| {
                    n.strip_prefix("factory/")
                        .is_none_or(|rest| !rest.contains('/'))
                })
                .collect(),
        })
    }

    /// The same repo with another base branch.
    pub fn with_base_branch(mut self, name: &str) -> Repo {
        self.base = name.to_string();
        self
    }

    /// The same repo with the job worktrees kept in `dir`.
    pub fn with_worktrees_dir(mut self, dir: PathBuf) -> Repo {
        self.worktrees = dir;
        self
    }

    /// The branch jobs start from and merge into.
    pub fn base_branch(&self) -> &str {
        &self.base
    }

    /// The folder that holds the job worktrees.
    pub fn worktrees_dir(&self) -> &Path {
        &self.worktrees
    }

    /// The working directory of the main checkout.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The common git folder of the repository, the main `.git` even when opened from a worktree.
    pub fn common_git_dir(&self) -> PathBuf {
        self.inner.path().to_path_buf()
    }

    /// Resolves a sha or other revision to a commit.
    pub(crate) fn commit_of(&self, rev: &str) -> Result<Commit<'_>> {
        let obj = self.inner.revparse_single(rev).map_err(|e| {
            if e.code() == ErrorCode::NotFound {
                GitError::NotFound(format!("revision {rev}"))
            } else {
                GitError::Git(e)
            }
        })?;
        Ok(obj.peel_to_commit()?)
    }

    /// The commit a local branch points at.
    pub(crate) fn branch_commit(&self, branch: &str) -> Result<Commit<'_>> {
        let found = self
            .inner
            .find_branch(branch, BranchType::Local)
            .map_err(|e| {
                if e.code() == ErrorCode::NotFound {
                    GitError::NotFound(format!("branch {branch}"))
                } else {
                    GitError::Git(e)
                }
            })?;
        Ok(found.get().peel_to_commit()?)
    }

    /// The short name of the branch HEAD is on, or `None` when HEAD is detached or unborn.
    pub fn current_branch(&self) -> Option<String> {
        let head = self.inner.head().ok()?;
        if !head.is_branch() {
            return None;
        }
        head.shorthand().map(str::to_string)
    }

    /// The names of all local branches, sorted.
    pub fn local_branch_names(&self) -> Result<Vec<String>> {
        let mut names = Vec::new();
        for entry in self.inner.branches(Some(BranchType::Local))? {
            let (branch, _) = entry?;
            if let Some(name) = branch.name()? {
                names.push(name.to_string());
            }
        }
        names.sort();
        Ok(names)
    }

    /// True when a local branch with this name exists.
    pub fn branch_exists(&self, name: &str) -> bool {
        self.inner.find_branch(name, BranchType::Local).is_ok()
    }

    /// Full sha of a local branch tip.
    pub fn tip(&self, branch: &str) -> Result<String> {
        Ok(self.branch_commit(branch)?.id().to_string())
    }

    /// The commits in `base..head` as `<short sha> <subject>` lines, oldest first.
    pub fn log_oneline(&self, base: &str, head: &str) -> Result<Vec<String>> {
        let mut walk = self.inner.revwalk()?;
        walk.push(self.commit_of(head)?.id())?;
        walk.hide(self.commit_of(base)?.id())?;
        walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::REVERSE)?;
        let mut lines = Vec::new();
        for oid in walk {
            let commit = self.inner.find_commit(oid?)?;
            let id = commit.id().to_string();
            lines.push(format!("{} {}", short(&id), commit.summary().unwrap_or("")));
        }
        Ok(lines)
    }

    /// Tree-to-tree diff, the same as `git diff base..head`.
    pub(crate) fn tree_diff(&self, base: &str, head: &str) -> Result<Diff<'_>> {
        let b = self.commit_of(base)?.tree()?;
        let h = self.commit_of(head)?.tree()?;
        Ok(self.inner.diff_tree_to_tree(Some(&b), Some(&h), None)?)
    }

    /// Paths changed between two commits. Renames list both paths on purpose,
    /// so risk classification sees the old location too.
    pub fn changed_paths(&self, base: &str, head: &str) -> Result<Vec<String>> {
        Ok(paths_of(&self.tree_diff(base, head)?))
    }

    /// The best common ancestor of two revisions, as a sha.
    pub fn merge_base(&self, a: &str, b: &str) -> Result<String> {
        let a = self.commit_of(a)?.id();
        let b = self.commit_of(b)?.id();
        Ok(self.inner.merge_base(a, b)?.to_string())
    }

    /// True when `older` is `newer` or one of its ancestors.
    pub fn is_ancestor(&self, older: &str, newer: &str) -> Result<bool> {
        let a = self.commit_of(older)?.id();
        let b = self.commit_of(newer)?.id();
        if a == b {
            return Ok(true);
        }
        Ok(self.inner.graph_descendant_of(b, a)?)
    }

    /// True when two commits have the same tree, whatever their history.
    pub fn same_tree(&self, a: &str, b: &str) -> Result<bool> {
        Ok(self.commit_of(a)?.tree_id() == self.commit_of(b)?.tree_id())
    }

    /// `Some(base sha)` when `head` is already on the base branch, `None` when it is not.
    /// Works after a manual merge and after the branch was deleted, because it
    /// takes shas. A job with no commits is refused, as `ctl is-merged` did.
    pub fn is_merged(&self, base: &str, head: &str) -> Result<Option<String>> {
        if head.trim().is_empty() {
            return Err(GitError::Refused("job has no recorded head".into()));
        }
        let head_id = self.commit_of(head)?.id();
        if head_id == self.commit_of(base)?.id() {
            return Err(GitError::Refused(
                "job has no commits, head equals base".into(),
            ));
        }
        let main = self.tip(&self.base)?;
        if self.is_ancestor(head, &main)? {
            Ok(Some(main))
        } else {
            Ok(None)
        }
    }
}

/// Sorted, deduplicated paths of a diff.
pub(crate) fn paths_of(diff: &Diff<'_>) -> Vec<String> {
    let mut set = BTreeSet::new();
    for delta in diff.deltas() {
        for file in [delta.old_file(), delta.new_file()] {
            if let Some(p) = file.path() {
                set.insert(p.to_string_lossy().into_owned());
            }
        }
    }
    set.into_iter().collect()
}
