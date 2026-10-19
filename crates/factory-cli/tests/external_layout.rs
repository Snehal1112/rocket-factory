mod common;

use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use common::{commit_file, RoleRunner};
use factory_agent::AgentRunner;
use factory_cli::{dispatch, Cli, CliCtx};
use factory_core::Layout;
use git2::{Repository, RepositoryInitOptions};

struct External {
    tmp: tempfile::TempDir,
    repo: PathBuf,
    layout: Layout,
    ctx: CliCtx,
}

impl External {
    fn new() -> External {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).expect("mkdir");
        let mut opts = RepositoryInitOptions::new();
        opts.initial_head("v-4.0.0");
        Repository::init_opts(&repo, &opts).expect("init");
        commit_file(&repo, "README.md", "hello\n");
        let git = Repository::open(&repo).expect("open");
        let first = git.head().expect("head").peel_to_commit().expect("commit");
        git.branch("main", &first, false).expect("main branch");
        let home = tmp.path().join("factory-home");
        let layout = Layout {
            repo: repo.clone(),
            state_dir: home.join("state"),
            jobs_dir: home.join("state/jobs"),
            worktrees_dir: home.join("worktrees"),
            config_file: home.join("project.toml"),
            commit_context_file: home.join("commit-context.md"),
        };
        std::fs::create_dir_all(&home).expect("home");
        std::fs::write(
            &layout.config_file,
            "base_branch = \"v-4.0.0\"\nrisk_paths = [\"src-tauri/**\"]\n",
        )
        .expect("config");
        std::fs::write(&layout.commit_context_file, "TEST CONTEXT\n").expect("context");
        let runner: Arc<dyn AgentRunner> = Arc::new(RoleRunner::new());
        let ctx = CliCtx::with_layout(layout.clone(), runner, "tester");
        External {
            tmp,
            repo,
            layout,
            ctx,
        }
    }

    fn run(&self, args: &[&str]) -> Result<String, factory_cli::CliError> {
        let mut argv = vec!["factory"];
        argv.extend_from_slice(args);
        dispatch(Cli::parse_from(argv), &self.ctx)
    }

    fn tip(&self, branch: &str) -> String {
        let git = Repository::open(&self.repo).expect("open");
        let id = git
            .revparse_single(branch)
            .expect("branch")
            .peel_to_commit()
            .expect("commit")
            .id();
        id.to_string()
    }
}

#[test]
fn a_job_merges_into_a_non_main_base_with_state_kept_outside_the_repo() {
    let fx = External::new();
    let plan_dir = fx.tmp.path().join("plans");
    std::fs::create_dir_all(&plan_dir).expect("plans");
    let plan = plan_dir.join("plan-01-hello.md");
    std::fs::write(&plan, "# Plan\n\n### Task 1: t\n\nNext plan: none\n").expect("plan");
    fx.run(&[
        "new",
        "hello",
        "say hello",
        "--plans",
        plan.to_str().expect("utf8"),
    ])
    .expect("new");
    let base_before = fx.tip("v-4.0.0");
    let main_before = fx.tip("main");
    assert_eq!(fx.run(&["run", "1"]).expect("run"), "merged");
    assert_ne!(fx.tip("v-4.0.0"), base_before, "the base branch advanced");
    assert_eq!(
        fx.tip("main"),
        main_before,
        "the other branch main is untouched"
    );
    assert!(
        fx.layout.board_file().exists(),
        "board kept in the state folder"
    );
    assert!(
        fx.layout.worktrees_dir.join("factory-001-hello").exists(),
        "worktree kept in the worktrees folder"
    );
    assert!(
        !fx.repo.join(".superpowers").exists(),
        "nothing written to .superpowers in the repo"
    );
    assert!(
        !fx.repo.join(".claude").exists(),
        "nothing written to .claude in the repo"
    );
    fx.run(&["cleanup", "1"]).expect("cleanup");
    assert!(!fx.layout.worktrees_dir.join("factory-001-hello").exists());
}
