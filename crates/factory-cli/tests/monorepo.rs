//! Two subfolder projects of one repo each build and merge in their own folder, with their own branches
//! and board, and the scope rule parks stray edits. The runs are sequential. Real lock contention is
//! covered by `crates/factory-engine/tests/merge.rs`.

mod common;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clap::Parser;
use common::{commit_file, commit_index};
use factory_agent::{AgentRun, AgentRunner, AgentSpec};
use factory_cli::{dispatch, project, Cli, CliCtx, CliError, Command};
use factory_core::Home;
use git2::{Repository, RepositoryInitOptions};

/// The builder writes a file in its folder, and one more file when `also` is set.
struct Roles {
    /// An extra file the builder writes, relative to its folder.
    also: Mutex<Option<&'static str>>,
}

impl AgentRunner for Roles {
    fn run(&self, spec: &AgentSpec) -> AgentRun {
        let text = match spec.role.as_str() {
            "builder" => {
                let write = |rel: &str| {
                    let p = spec.cwd.join(rel);
                    std::fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
                    std::fs::write(p, "built\n").expect("write");
                };
                write("docs/built.txt");
                if let Some(extra) = *self.also.lock().expect("lock") {
                    write(extra);
                }
                "done".to_string()
            }
            "committer" => {
                commit_index(&spec.cwd, "feat(test): build step");
                "done".to_string()
            }
            _ => "fine\nVERDICT: APPROVE".to_string(),
        };
        AgentRun {
            session_id: Some("s".to_string()),
            ..AgentRun::done(&text)
        }
    }
}

struct Env {
    tmp: tempfile::TempDir,
    home: Home,
    repo: PathBuf,
    runner: Arc<Roles>,
}

/// A repo on `main` with `apps/web` and `apps/api`, both added as projects.
fn setup() -> Env {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("mono");
    std::fs::create_dir_all(&repo).expect("mkdir");
    let mut opts = RepositoryInitOptions::new();
    opts.initial_head("main");
    Repository::init_opts(&repo, &opts).expect("init");
    commit_file(&repo, "README.md", "hello\n");
    commit_file(&repo, "apps/web/index.txt", "web\n");
    commit_file(&repo, "apps/api/index.txt", "api\n");
    let home = Home::at(tmp.path().join("home"));
    for name in ["web", "api"] {
        let folder = repo.join("apps").join(name);
        let cli = Cli::try_parse_from([
            "factory",
            "project",
            "add",
            folder.to_str().expect("utf8"),
            "--preset",
            "rust",
        ])
        .expect("parse");
        match cli.command {
            Command::Project { cmd } => project::run(&home, cmd).expect("add"),
            _ => unreachable!(),
        };
    }
    Env {
        tmp,
        home,
        repo,
        runner: Arc::new(Roles {
            also: Mutex::new(None),
        }),
    }
}

impl Env {
    fn ctx(&self, name: &str) -> CliCtx {
        let r: Arc<dyn AgentRunner> = self.runner.clone();
        CliCtx::for_project(&self.home, name, Some(r), "tester", Duration::from_secs(5))
            .expect("ctx")
    }

    fn run(&self, name: &str, ctx: &CliCtx, args: &[&str]) -> Result<String, CliError> {
        let mut argv = vec!["factory", "--project", name];
        argv.extend_from_slice(args);
        dispatch(Cli::parse_from(argv), ctx)
    }

    fn new_job(&self, name: &str, ctx: &CliCtx, n: u32) {
        let dir = self.tmp.path().join("plans").join(name);
        std::fs::create_dir_all(&dir).expect("plans dir");
        let plan = dir.join(format!("plan-{n:02}-hello.md"));
        std::fs::write(&plan, "# Plan\n\n### Task 1: t\n\nNext plan: none\n").expect("plan");
        self.run(
            name,
            ctx,
            &[
                "new",
                &format!("{name}job"),
                "say hello",
                "--plans",
                plan.to_str().expect("utf8"),
            ],
        )
        .expect("new");
    }

    fn branches(&self) -> Vec<String> {
        let repo = Repository::open(&self.repo).expect("open");
        let found: Vec<String> = repo
            .branches(Some(git2::BranchType::Local))
            .expect("branches")
            .filter_map(|b| b.ok())
            .filter_map(|(b, _)| b.name().ok().flatten().map(str::to_string))
            .collect();
        found
    }

    fn main_tip(&self) -> String {
        let repo = Repository::open(&self.repo).expect("open");
        let id = repo
            .revparse_single("main")
            .expect("main")
            .peel_to_commit()
            .expect("commit")
            .id();
        id.to_string()
    }
}

#[test]
fn two_subfolder_projects_run_side_by_side() {
    let e = setup();
    let web = e.ctx("web");
    let api = e.ctx("api");
    e.new_job("web", &web, 1);
    e.new_job("api", &api, 1);
    assert_eq!(
        e.run("web", &web, &["run", "1"]).expect("run web"),
        "merged"
    );
    assert_eq!(
        e.run("api", &api, &["run", "1"]).expect("run api"),
        "merged"
    );
    // Main holds both files, each inside its own folder.
    assert!(e.repo.join("apps/web/docs/built.txt").exists());
    assert!(e.repo.join("apps/api/docs/built.txt").exists());
    // Each project has its own id counter and its own branch.
    let branches = e.branches();
    for prefix in ["factory/web/001-", "factory/api/001-"] {
        assert!(
            branches.iter().any(|b| b.starts_with(prefix)),
            "{prefix} in {branches:?}"
        );
    }
    // A project sees only its own job.
    let status = e.run("web", &web, &["status"]).expect("status");
    assert!(status.contains("webjob"), "{status}");
    assert!(!status.contains("apijob"), "{status}");
    let repo = Repository::open(&e.repo).expect("open");
    assert!(repo.statuses(None).expect("statuses").is_empty());
}

#[test]
fn out_of_scope_job_is_parked_end_to_end() {
    let e = setup();
    let web = e.ctx("web");
    e.new_job("web", &web, 1);
    // The builder also writes into the sibling folder.
    *e.runner.also.lock().expect("lock") = Some("../api/stray.txt");
    let before = e.main_tip();
    let out = e.run("web", &web, &["run", "1"]).expect("run");
    assert!(out.starts_with("needs-human"), "{out}");
    assert!(out.contains("apps/api/stray.txt"), "{out}");
    let err = e.run("web", &web, &["approve", "1"]).expect_err("refused");
    assert!(matches!(err, CliError::Refused(_)), "{err:?}");
    assert!(err.to_string().contains("apps/api/stray.txt"), "{err}");
    assert_eq!(e.main_tip(), before, "nothing merged");
    assert!(!e.repo.join("apps/api/stray.txt").exists());
}
