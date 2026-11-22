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
        Fixture::on_branch("main")
    }

    /// A temp repo with one commit on `base` and a file `base.txt`.
    pub fn on_branch(base: &str) -> Fixture {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("repo");
        std::fs::create_dir_all(&path).expect("mkdir");
        let mut opts = RepositoryInitOptions::new();
        opts.initial_head(base);
        let repo = Repository::init_opts(&path, &opts).expect("init");
        // The user's global git config may turn signing on.
        repo.config()
            .expect("config")
            .set_bool("commit.gpgsign", false)
            .expect("gpgsign");
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

    pub fn branch_tip(&self, branch: &str) -> String {
        branch_sha(&self.git(), branch)
    }

    /// Creates `branch` from main and commits `files` onto it. Returns (base, head).
    pub fn job_branch(&self, branch: &str, files: &[(&str, &str)]) -> (String, String) {
        let git = self.git();
        let base = self.main_sha();
        let main = git
            .find_commit(Oid::from_str(&base).expect("oid"))
            .expect("commit");
        git.branch(branch, &main, false).expect("branch");
        let head = commit_to_branch(&git, branch, files, "job work");
        (base, head.to_string())
    }
}

/// Writes an executable `/bin/sh` script into `dir` and returns its path.
pub fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write script");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    path
}

/// A fake gpg that records its arguments in `args` beside it and makes a fixed signature.
pub fn fake_gpg(dir: &Path) -> PathBuf {
    script(
        dir,
        "fake-gpg",
        r#"echo "$@" > "$(dirname "$0")/args"
cat > /dev/null
echo "[GNUPG:] SIG_CREATED D 22 8 00 1 ABC" >&2
printf -- '-----BEGIN PGP SIGNATURE-----\nfake\n-----END PGP SIGNATURE-----\n'"#,
    )
}

/// The `-bsau` arguments the fake gpg was run with.
pub fn recorded_args(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("args"))
        .expect("args file")
        .trim_end()
        .to_string()
}

/// Turns signing on in the repo config with the given extra settings.
pub fn signing_on(repo: &Repository, settings: &[(&str, &str)]) {
    let mut cfg = repo.config().expect("config");
    cfg.set_bool("commit.gpgsign", true).expect("gpgsign");
    cfg.set_str("gpg.format", "openpgp").expect("format");
    for (name, value) in settings {
        cfg.set_str(name, value).expect("setting");
    }
}

pub const FAKE_SIGNATURE: &str = "-----BEGIN PGP SIGNATURE-----\nfake\n-----END PGP SIGNATURE-----";

/// The `gpgsig` text of a commit, or None when it is unsigned.
pub fn signature_of(repo: &Repository, sha: &str) -> Option<String> {
    let oid = Oid::from_str(sha).expect("oid");
    let (sig, _) = repo.extract_signature(&oid, None).ok()?;
    Some(sig.as_str().expect("utf8").to_string())
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
    let tree = repo
        .find_tree(index.write_tree().expect("tree"))
        .expect("find tree");
    let parents: Vec<Commit> = match repo.head() {
        Ok(h) => vec![h.peel_to_commit().expect("head commit")],
        Err(_) => vec![],
    };
    let refs: Vec<&Commit> = parents.iter().collect();
    repo.commit(Some("HEAD"), &sig(), &sig(), msg, &tree, &refs)
        .expect("commit")
}

/// Commits flat files onto a branch without touching any working tree.
pub fn commit_to_branch(repo: &Repository, branch: &str, files: &[(&str, &str)], msg: &str) -> Oid {
    let b = repo.find_branch(branch, BranchType::Local).expect("branch");
    let parent = b.get().peel_to_commit().expect("parent");
    let mut tb = repo
        .treebuilder(Some(&parent.tree().expect("tree")))
        .expect("treebuilder");
    for (p, c) in files {
        let blob = repo.blob(c.as_bytes()).expect("blob");
        tb.insert(p, blob, 0o100644).expect("insert");
    }
    let tree = repo
        .find_tree(tb.write().expect("write tree"))
        .expect("find tree");
    let name = format!("refs/heads/{branch}");
    repo.commit(Some(&name), &sig(), &sig(), msg, &tree, &[&parent])
        .expect("commit")
}

/// Moves a branch ref to a commit (a stand-in for a manual fast-forward merge).
pub fn point_branch_at(repo: &Repository, branch: &str, sha: &str) {
    let name = format!("refs/heads/{branch}");
    repo.reference(&name, Oid::from_str(sha).expect("oid"), true, "test")
        .expect("reference");
}
