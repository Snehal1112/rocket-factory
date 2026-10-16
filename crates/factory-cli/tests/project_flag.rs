//! The global --project flag: a job runs against a registered project and leaves the repo alone.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clap::Parser;
use factory_agent::{AgentRun, AgentRunner, AgentSpec};
use factory_cli::{dispatch, project, Cli, CliCtx, CliError, Command};
use factory_core::Home;
use git2::{Repository, RepositoryInitOptions, Signature};

const BASE: &str = "v-4.0.0";

struct Roles {
    calls: AtomicUsize,
    builder_agent: Mutex<Option<String>>,
    reviewer_agent: Mutex<Option<String>>,
    /// An extra file the builder writes, relative to the worktree.
    also: Mutex<Option<&'static str>>,
}

impl AgentRunner for Roles {
    fn run(&self, spec: &AgentSpec) -> AgentRun {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let text = match spec.role.as_str() {
            "builder" => {
                *self.builder_agent.lock().expect("lock") =
                    std::fs::read_to_string(spec.cwd.join(".claude/agents/factory-builder.md")).ok();
                let path = spec.cwd.join("docs/x.txt");
                std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
                std::fs::write(path, "built\n").expect("write");
                if let Some(extra) = *self.also.lock().expect("lock") {
                    let p = spec.cwd.join(extra);
                    std::fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
                    std::fs::write(p, "extra\n").expect("write");
                }
                "done".to_string()
            }
            "committer" => {
                let repo = Repository::open(&spec.cwd).expect("open");
                let mut index = repo.index().expect("index");
                let tree = repo.find_tree(index.write_tree().expect("tree")).expect("tree");
                let sig = Signature::now("t", "t@t").expect("sig");
                let parent = repo.head().expect("head").peel_to_commit().expect("commit");
                repo.commit(Some("HEAD"), &sig, &sig, "feat(test): build step", &tree, &[&parent]).expect("commit");
                "done".to_string()
            }
            "reviewer" => {
                *self.reviewer_agent.lock().expect("lock") =
                    std::fs::read_to_string(spec.cwd.join(".claude/agents/factory-code-reviewer.md")).ok();
                "fine\nVERDICT: APPROVE".to_string()
            }
            _ => "fine\nVERDICT: APPROVE".to_string(),
        };
        AgentRun { session_id: Some("s".to_string()), ..AgentRun::done(&text) }
    }
}

struct Env {
    tmp: tempfile::TempDir,
    home: Home,
    repo: PathBuf,
    runner: Arc<Roles>,
}

fn commit_files(repo: &Repository, files: &[(&str, &str)], msg: &str) {
    let dir = repo.workdir().expect("workdir").to_path_buf();
    let mut index = repo.index().expect("index");
    for (name, content) in files {
        let p = dir.join(name);
        std::fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
        std::fs::write(&p, content).expect("write");
        index.add_path(Path::new(name)).expect("add");
    }
    index.write().expect("write index");
    let tree = repo.find_tree(index.write_tree().expect("tree")).expect("tree");
    let sig = Signature::now("t", "t@t").expect("sig");
    let parents: Vec<git2::Commit> = repo.head().ok().map(|h| h.peel_to_commit().expect("c")).into_iter().collect();
    let refs: Vec<&git2::Commit> = parents.iter().collect();
    repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &refs).expect("commit");
}

/// A repo on `v-4.0.0` with a local `main` at the first commit, added as project `p`.
fn setup(extra: &[(&str, &str)]) -> Env {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo_path = tmp.path().join("target");
    std::fs::create_dir_all(&repo_path).expect("mkdir");
    let mut opts = RepositoryInitOptions::new();
    opts.initial_head(BASE);
    let repo = Repository::init_opts(&repo_path, &opts).expect("init");
    commit_files(&repo, &[("Cargo.toml", "[package]\nname = \"t\"\n"), ("src/lib.rs", "")], "init");
    let first = repo.head().expect("head").peel_to_commit().expect("commit");
    repo.branch("main", &first, false).expect("main branch");
    if !extra.is_empty() {
        commit_files(&repo, extra, "add extra files");
    }
    let home = Home::at(tmp.path().join("home"));
    let cli = Cli::try_parse_from([
        "factory", "project", "add", repo_path.to_str().expect("utf8"), "--preset", "rust", "--name", "p", "--base", BASE,
    ])
    .expect("parse");
    match cli.command {
        Command::Project { cmd } => project::run(&home, cmd).expect("add"),
        _ => unreachable!(),
    };
    let runner = Arc::new(Roles { calls: AtomicUsize::new(0), builder_agent: Mutex::new(None), reviewer_agent: Mutex::new(None), also: Mutex::new(None) });
    Env { tmp, home, repo: repo_path, runner }
}

impl Env {
    fn ctx(&self) -> Result<CliCtx, CliError> {
        let r: Arc<dyn AgentRunner> = self.runner.clone();
        CliCtx::for_project(&self.home, "p", Some(r), "tester", Duration::from_secs(5))
    }

    fn run(&self, ctx: &CliCtx, args: &[&str]) -> Result<String, CliError> {
        let mut argv = vec!["factory", "--project", "p"];
        argv.extend_from_slice(args);
        dispatch(Cli::parse_from(argv), ctx)
    }

    fn new_job(&self, ctx: &CliCtx) {
        let dir = self.tmp.path().join("plans");
        std::fs::create_dir_all(&dir).expect("plans dir");
        let plan = dir.join("plan-01-hello.md");
        std::fs::write(&plan, "# Plan\n\n### Task 1: t\n\nNext plan: none\n").expect("plan");
        self.run(ctx, &["new", "hello", "say hello", "--plans", plan.to_str().expect("utf8")]).expect("new");
    }

    fn tip(&self, branch: &str) -> String {
        let repo = Repository::open(&self.repo).expect("open");
        let id = repo.revparse_single(branch).expect("branch").peel_to_commit().expect("commit").id();
        id.to_string()
    }
}

#[test]
fn project_conflicts_with_root() {
    assert!(Cli::try_parse_from(["factory", "--project", "p", "--root", "/x", "status"]).is_err());
    Cli::try_parse_from(["factory", "--project", "p", "status"]).expect("parses");
}

#[test]
fn a_full_job_runs_through_the_project_and_leaves_the_repo_clean() {
    // The repo ignores .claude/, as real repos do, so the agent files and worktrees cannot dirty it.
    let e = setup(&[(".gitignore", ".claude/\n")]);
    let ctx = e.ctx().expect("ctx");
    let main_before = e.tip("main");
    let base_before = e.tip(BASE);
    e.new_job(&ctx);
    assert_eq!(e.run(&ctx, &["run", "1"]).expect("run"), "merged");
    assert_ne!(e.tip(BASE), base_before, "the base branch moved");
    assert_eq!(e.tip("main"), main_before, "main is untouched");
    let pdir = e.home.project_dir("p");
    assert!(pdir.join("state/board.json").exists());
    let shown = e.run(&ctx, &["show", "1"]).expect("show");
    let job: serde_json::Value = serde_json::from_str(&shown).expect("show prints the job as json");
    let wt = job["worktree"].as_str().expect("the job records its worktree");
    assert!(std::path::Path::new(wt).starts_with(pdir.join("worktrees")), "{wt}");
    assert!(e.runner.builder_agent.lock().expect("lock").as_deref().is_some_and(|t| !t.is_empty()), "builder saw its agent file");
    assert!(e.runner.reviewer_agent.lock().expect("lock").as_deref().is_some_and(|t| !t.is_empty()), "reviewer saw its agent file");
    for dir in [".superpowers", ".claude", ".factory"] {
        assert!(!e.repo.join(dir).exists(), "{dir} must not exist in the target repo");
    }
    let repo = Repository::open(&e.repo).expect("open");
    let statuses = repo.statuses(None).expect("statuses");
    assert!(statuses.is_empty(), "the main checkout is clean");
}

#[test]
fn a_repo_override_adds_risk_paths() {
    let with = setup(&[(".factory/override.toml", "risk_paths = [\"docs/**\"]\n")]);
    let ctx = with.ctx().expect("ctx");
    with.new_job(&ctx);
    let out = with.run(&ctx, &["run", "1"]).expect("run");
    assert!(out.starts_with("needs-human"), "{out}");

    let without = setup(&[]);
    let ctx = without.ctx().expect("ctx");
    without.new_job(&ctx);
    assert_eq!(without.run(&ctx, &["run", "1"]).expect("run"), "merged");
}

#[test]
fn a_job_touching_the_factory_folder_parks_for_a_human() {
    let e = setup(&[(".gitignore", ".claude/\n")]);
    *e.runner.also.lock().expect("lock") = Some(".factory/note.txt");
    let ctx = e.ctx().expect("ctx");
    e.new_job(&ctx);
    let out = e.run(&ctx, &["run", "1"]).expect("run");
    assert!(out.starts_with("needs-human"), "{out}");
    assert!(out.contains("risky paths") && out.contains(".factory/note.txt"), "{out}");
    assert!(e.run(&ctx, &["show", "1"]).expect("show").contains("\"stage\": \"needs-human\""));
}

fn assert_blocked(e: &Env, reason: &str) {
    let ctx = e.ctx().expect("ctx");
    assert!(ctx.blocked.as_deref().is_some_and(|b| b.contains(reason)), "{:?}", ctx.blocked);
    // Job 1 exists, so `show` really reads it.
    e.new_job(&ctx);
    let calls = e.runner.calls.load(Ordering::SeqCst);
    for cmd in [["run", "1"], ["approve", "1"]] {
        let err = e.run(&ctx, &cmd).expect_err("blocked").to_string();
        assert!(err.contains(reason), "{err}");
    }
    assert_eq!(e.runner.calls.load(Ordering::SeqCst), calls, "no agent started");
    e.run(&ctx, &["status"]).expect("status works");
    e.run(&ctx, &["show", "1"]).expect("show is not blocked");
}

#[test]
fn a_broken_override_blocks_run_and_approve() {
    let e = setup(&[(".factory/override.toml", "this is not toml [[[\n")]);
    assert_blocked(&e, "override.toml");
}

#[test]
fn a_rejected_check_command_blocks_run_and_approve() {
    let e = setup(&[(
        ".factory/override.toml",
        "[[checks]]\npaths = [\"src/**\"]\ncommands = [\"cargo test -j4 --workspace\"]\n",
    )]);
    assert_blocked(&e, "a check command would be rejected when a job starts");
}

#[test]
fn a_project_without_a_base_branch_setting_is_blocked() {
    let e = setup(&[]);
    let file = e.home.project_dir("p").join("project.toml");
    let text = std::fs::read_to_string(&file).expect("read");
    let kept: Vec<&str> = text.lines().filter(|l| !l.trim_start().starts_with("base_branch")).collect();
    std::fs::write(&file, kept.join("\n")).expect("write");
    let ctx = e.ctx().expect("ctx");
    assert!(ctx.blocked.as_deref().is_some_and(|b| b.contains("base_branch")), "{:?}", ctx.blocked);
    let err = e.run(&ctx, &["run", "1"]).expect_err("blocked").to_string();
    assert!(err.contains("base_branch"), "{err}");
}

#[test]
fn unknown_and_missing_projects_are_refused() {
    let e = setup(&[]);
    let r: Arc<dyn AgentRunner> = e.runner.clone();
    let err = CliCtx::for_project(&e.home, "nope", Some(r), "t", Duration::from_secs(1)).err().expect("error");
    assert!(err.to_string().contains("no project named nope"), "{err}");
    std::fs::remove_dir_all(&e.repo).expect("remove repo");
    let err = e.ctx().err().expect("refused").to_string();
    assert!(err.contains("repo path is missing"), "{err}");
}
