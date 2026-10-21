mod common;

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use clap::Parser;
use common::{commit_file, commit_index};
use factory_agent::{AgentRun, AgentRunner, AgentSpec};
use factory_cli::{dispatch, Cli, CliCtx, CliError};
use factory_core::{Layout, Scope};
use git2::Repository;

/// The first builder run edits the repo root README. Later builder runs put it back.
struct ScopeRunner {
    builds: AtomicUsize,
}

impl AgentRunner for ScopeRunner {
    fn run(&self, spec: &AgentSpec) -> AgentRun {
        let text = if spec.role == "builder" {
            let n = self.builds.fetch_add(1, Ordering::SeqCst);
            std::fs::create_dir_all(spec.cwd.join("docs")).expect("mkdir");
            std::fs::write(spec.cwd.join("docs/a.txt"), "built\n").expect("write");
            let readme = spec.cwd.join("../../README.md");
            let body = if n == 0 { "changed\n" } else { "hello\n" };
            std::fs::write(readme, body).expect("write readme");
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

struct Scoped {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    ctx: CliCtx,
}

fn scoped() -> Scoped {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("repo");
    std::fs::create_dir_all(&root).expect("mkdir");
    let mut opts = git2::RepositoryInitOptions::new();
    opts.initial_head("main");
    Repository::init_opts(&root, &opts).expect("init");
    commit_file(&root, "README.md", "hello\n");
    commit_file(&root, "apps/web/index.txt", "web\n");
    std::fs::create_dir_all(root.join(".claude/factory")).expect("mkdir config");
    // Every docs path is risky, so a clean job parks for a human instead of merging.
    std::fs::write(
        root.join(".claude/factory/config.toml"),
        "risk_paths = [\"docs/**\"]\n",
    )
    .expect("config");
    std::fs::write(
        root.join(".claude/factory/commit-context.md"),
        "TEST CONTEXT\n",
    )
    .expect("context");
    let mut layout = Layout::in_repo(&root);
    layout.scope = Some(Scope {
        project: "web".into(),
        subpath: PathBuf::from("apps/web"),
    });
    let runner: Arc<dyn AgentRunner> = Arc::new(ScopeRunner {
        builds: AtomicUsize::new(0),
    });
    let ctx = CliCtx::with_layout(layout, runner, "tester");
    Scoped {
        _tmp: tmp,
        root,
        ctx,
    }
}

fn run(fx: &Scoped, args: &[&str]) -> Result<String, CliError> {
    let mut argv = vec!["factory"];
    argv.extend_from_slice(args);
    dispatch(Cli::parse_from(argv), &fx.ctx)
}

fn main_tip(fx: &Scoped) -> String {
    let repo = Repository::open(&fx.root).expect("open");
    let id = repo
        .revparse_single("main")
        .expect("main")
        .peel_to_commit()
        .expect("commit")
        .id();
    id.to_string()
}

#[test]
fn approve_refuses_while_paths_remain() {
    let fx = scoped();
    let plan = fx._tmp.path().join("plan-01-hello.md");
    std::fs::write(&plan, "# Plan\n\n### Task 1: t\n\nNext plan: none\n").expect("plan");
    run(
        &fx,
        &[
            "new",
            "hello",
            "say hello",
            "--plans",
            plan.to_str().expect("utf8"),
        ],
    )
    .expect("new");
    let out = run(&fx, &["run", "1"]).expect("run");
    assert!(out.starts_with("needs-human"), "{out}");
    assert!(out.contains("README.md"), "{out}");
    let before = main_tip(&fx);
    // The scope blocker shows first, not the missing review.
    let err = run(&fx, &["approve", "1"]).expect_err("refused");
    assert!(matches!(err, CliError::Refused(_)), "{err:?}");
    assert!(err.to_string().contains("README.md"), "{err}");
    assert!(!err.to_string().contains("--without-review"), "{err}");
    // Skipping the review does not lift the scope refusal.
    let err = run(&fx, &["approve", "1", "--without-review"]).expect_err("refused");
    assert!(matches!(err, CliError::Refused(_)), "{err:?}");
    assert!(err.to_string().contains("README.md"), "{err}");
    assert_eq!(main_tip(&fx), before);
    // The builder puts the README back through the rework flow, then the check passes.
    let out = run(
        &fx,
        &[
            "rework",
            "1",
            "Revert the README.md change at the repo root.",
        ],
    )
    .expect("rework");
    assert!(out.contains("back to build"), "{out}");
    let out = run(&fx, &["run", "1"]).expect("run again");
    assert!(out.starts_with("needs-human"), "{out}");
    assert!(!out.contains("outside"), "{out}");
    let out = run(&fx, &["approve", "1"]).expect("approve");
    assert!(out.starts_with("merged"), "{out}");
}
