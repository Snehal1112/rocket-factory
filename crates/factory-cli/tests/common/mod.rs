#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use clap::Parser;
use factory_agent::{AgentRun, AgentRunner, AgentSpec};
use factory_cli::{dispatch, Cli, CliCtx, CliError};
use git2::{Repository, Signature};

/// Commits one file in the repository at `dir` and returns the new sha.
pub fn commit_file(dir: &Path, name: &str, content: &str) -> String {
    let repo = Repository::open(dir).expect("open repo");
    if let Some(parent) = dir.join(name).parent() {
        std::fs::create_dir_all(parent).expect("mkdir parent");
    }
    std::fs::write(dir.join(name), content).expect("write file");
    let mut index = repo.index().expect("index");
    index.add_path(Path::new(name)).expect("add");
    index.write().expect("write index");
    let tree = repo
        .find_tree(index.write_tree().expect("tree"))
        .expect("find tree");
    let sig = Signature::now("t", "t@example.com").expect("sig");
    let parents: Vec<git2::Commit> = match repo.head() {
        Ok(h) => vec![h.peel_to_commit().expect("commit")],
        Err(_) => vec![],
    };
    let refs: Vec<&git2::Commit> = parents.iter().collect();
    repo.commit(Some("HEAD"), &sig, &sig, "test commit", &tree, &refs)
        .expect("commit")
        .to_string()
}

/// Commits whatever is staged in `dir`, like a committer agent would.
pub fn commit_index(dir: &Path, message: &str) -> String {
    let repo = Repository::open(dir).expect("open repo");
    let mut index = repo.index().expect("index");
    let tree = repo
        .find_tree(index.write_tree().expect("tree"))
        .expect("find tree");
    let sig = Signature::now("t", "t@example.com").expect("sig");
    let parent = repo.head().expect("head").peel_to_commit().expect("commit");
    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&parent])
        .expect("commit")
        .to_string()
}

/// The builder commits a new file, the reviewer approves. It never runs the real `claude`.
pub struct RoleRunner {
    counter: AtomicUsize,
    /// Counts builder calls, shared with the fixture.
    builder_calls: Arc<AtomicUsize>,
}

impl RoleRunner {
    pub fn new() -> RoleRunner {
        RoleRunner::counting(Arc::new(AtomicUsize::new(0)))
    }

    pub fn counting(builder_calls: Arc<AtomicUsize>) -> RoleRunner {
        RoleRunner {
            counter: AtomicUsize::new(0),
            builder_calls,
        }
    }
}

impl AgentRunner for RoleRunner {
    fn run(&self, spec: &AgentSpec) -> AgentRun {
        let text = if spec.role == "builder" {
            self.builder_calls.fetch_add(1, Ordering::SeqCst);
            let n = self.counter.fetch_add(1, Ordering::SeqCst);
            let path = spec.cwd.join(format!("docs/{}-{n}.txt", spec.job_id));
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(path, "built\n").expect("write");
            "done".to_string()
        } else if spec.role == "committer" {
            commit_index(&spec.cwd, "feat(test): build step");
            "done".to_string()
        } else {
            "fine\nVERDICT: APPROVE".to_string()
        };
        AgentRun {
            session_id: Some("sess-1".to_string()),
            ..AgentRun::done(&text)
        }
    }
}

pub struct Fixture {
    pub tmp: tempfile::TempDir,
    pub root: PathBuf,
    pub ctx: CliCtx,
    pub builder_calls: Arc<AtomicUsize>,
}

impl Fixture {
    pub fn new() -> Fixture {
        Fixture::with_risk_path("src-tauri/**")
    }

    pub fn with_risk_path(risk: &str) -> Fixture {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("repo");
        std::fs::create_dir_all(&root).expect("mkdir");
        let mut opts = git2::RepositoryInitOptions::new();
        opts.initial_head("main");
        Repository::init_opts(&root, &opts).expect("init");
        commit_file(&root, "README.md", "hello\n");
        // The config stays untracked, which the merge preflight allows.
        std::fs::create_dir_all(root.join(".claude/factory")).expect("mkdir config");
        std::fs::write(
            root.join(".claude/factory/config.toml"),
            format!("risk_paths = [\"{risk}\"]\n"),
        )
        .expect("config");
        std::fs::write(
            root.join(".claude/factory/commit-context.md"),
            "TEST CONTEXT\n",
        )
        .expect("context");
        let builder_calls = Arc::new(AtomicUsize::new(0));
        let runner: Arc<dyn AgentRunner> = Arc::new(RoleRunner::counting(builder_calls.clone()));
        let ctx = CliCtx::new(root.clone(), runner, "tester");
        Fixture {
            tmp,
            root,
            ctx,
            builder_calls,
        }
    }

    pub fn write_plan(&self, name: &str, tasks: u32, next: &str) -> PathBuf {
        let dir = self.tmp.path().join("plans");
        std::fs::create_dir_all(&dir).expect("plans dir");
        let mut s = String::from("# Plan\n\n");
        for n in 1..=tasks {
            s.push_str(&format!("### Task {n}: t\n\n"));
        }
        s.push_str(&format!("Next plan: {next}\n"));
        let path = dir.join(name);
        std::fs::write(&path, s).expect("write plan");
        path
    }

    pub fn builder_calls(&self) -> usize {
        self.builder_calls.load(Ordering::SeqCst)
    }

    pub fn run(&self, args: &[&str]) -> Result<String, CliError> {
        let mut argv = vec!["factory"];
        argv.extend_from_slice(args);
        dispatch(Cli::parse_from(argv), &self.ctx)
    }

    fn main_commit(&self) -> git2::Oid {
        let repo = Repository::open(&self.root).expect("open");
        let id = repo
            .revparse_single("main")
            .expect("main")
            .peel_to_commit()
            .expect("commit")
            .id();
        id
    }

    pub fn main_tip(&self) -> String {
        self.main_commit().to_string()
    }

    pub fn main_message(&self) -> String {
        let repo = Repository::open(&self.root).expect("open");
        let msg = repo
            .find_commit(self.main_commit())
            .expect("commit")
            .message()
            .unwrap_or("")
            .to_string();
        msg
    }

    pub fn main_file(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    pub fn worktree_path(&self, id: u32) -> PathBuf {
        self.root
            .join(".claude/worktrees")
            .join(format!("factory-{id:03}-hello"))
    }

    /// Takes a live lease as another driver.
    pub fn hold_lease(&self, id: &str, owner: &str) {
        let store = factory_engine::BoardStore::new(
            &factory_core::Layout::in_repo(&self.root).board_file(),
            std::time::Duration::from_secs(15),
        );
        factory_engine::acquire_lease(&store, id, owner, 600.0, factory_engine::now_secs())
            .expect("lease");
    }
}
