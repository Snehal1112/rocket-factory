#![allow(dead_code)]

use git2::{BranchType, Commit, Oid, Repository, RepositoryInitOptions, Signature};
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// A temp repo with one commit on `main` and a file `base.txt`.
pub struct Fixture {
    pub dir: TempDir,
    pub path: PathBuf,
}

impl Fixture {
    pub fn new() -> Fixture {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("repo");
        std::fs::create_dir_all(&path).expect("mkdir");
        let mut opts = RepositoryInitOptions::new();
        opts.initial_head("main");
        let repo = Repository::init_opts(&path, &opts).expect("init");
        commit_files(&repo, &[("base.txt", "base\n")], "init");
        Fixture { dir, path }
    }

    pub fn git(&self) -> Repository {
        Repository::open(&self.path).expect("open git")
    }

    pub fn repo(&self) -> factory_git::Repo {
        factory_git::Repo::open(&self.path).expect("open repo")
    }

    pub fn main_sha(&self) -> String {
        branch_sha(&self.git(), "main")
    }

    /// Creates `branch` from main and commits `files` onto it. Returns (base, head).
    pub fn job_branch(&self, branch: &str, files: &[(&str, &str)]) -> (String, String) {
        let git = self.git();
        let base = self.main_sha();
        let main = git.find_commit(Oid::from_str(&base).expect("oid")).expect("commit");
        git.branch(branch, &main, false).expect("branch");
        let head = commit_to_branch(&git, branch, files, "job work");
        (base, head.to_string())
    }
}

pub fn sig() -> Signature<'static> {
    Signature::now("t", "t@t").expect("sig")
}

pub fn branch_sha(repo: &Repository, branch: &str) -> String {
    let b = repo.find_branch(branch, BranchType::Local).expect("branch");
    b.get().peel_to_commit().expect("commit").id().to_string()
}

/// Writes files into the workdir, stages them and commits on HEAD.
pub fn commit_files(repo: &Repository, files: &[(&str, &str)], msg: &str) -> Oid {
    let workdir = repo.workdir().expect("workdir");
    let mut index = repo.index().expect("index");
    for (p, c) in files {
        std::fs::write(workdir.join(p), c).expect("write");
        index.add_path(Path::new(p)).expect("add");
    }
    index.write().expect("write index");
    let tree = repo.find_tree(index.write_tree().expect("tree")).expect("find tree");
    let parents: Vec<Commit> = match repo.head() {
        Ok(h) => vec![h.peel_to_commit().expect("head commit")],
        Err(_) => vec![],
    };
    let refs: Vec<&Commit> = parents.iter().collect();
    repo.commit(Some("HEAD"), &sig(), &sig(), msg, &tree, &refs).expect("commit")
}

/// Commits flat files onto a branch without touching any working tree.
pub fn commit_to_branch(repo: &Repository, branch: &str, files: &[(&str, &str)], msg: &str) -> Oid {
    let b = repo.find_branch(branch, BranchType::Local).expect("branch");
    let parent = b.get().peel_to_commit().expect("parent");
    let mut tb = repo.treebuilder(Some(&parent.tree().expect("tree"))).expect("treebuilder");
    for (p, c) in files {
        let blob = repo.blob(c.as_bytes()).expect("blob");
        tb.insert(p, blob, 0o100644).expect("insert");
    }
    let tree = repo.find_tree(tb.write().expect("write tree")).expect("find tree");
    let name = format!("refs/heads/{branch}");
    repo.commit(Some(&name), &sig(), &sig(), msg, &tree, &[&parent]).expect("commit")
}

/// Moves a branch ref to a commit (a stand-in for a manual fast-forward merge).
pub fn point_branch_at(repo: &Repository, branch: &str, sha: &str) {
    let name = format!("refs/heads/{branch}");
    repo.reference(&name, Oid::from_str(sha).expect("oid"), true, "test").expect("reference");
}
