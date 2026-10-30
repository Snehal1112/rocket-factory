//! Test fixtures: a temp main repo, a two-plan series and a scripted fake runner.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use clap::Parser;
use factory_agent::{AgentRun, AgentRunner, AgentSpec, Usage};
use factory_cli::{dispatch, Cli, CliCtx, CliError};
use factory_core::{Board, Job};
use git2::{Repository, Signature};

pub type TestResult = Result<(), Box<dyn std::error::Error>>;

pub const APPROVE: &str = "Looks right.\nVERDICT: APPROVE";
pub const CHANGES: &str = "The helper is wrong.\nVERDICT: CHANGES";

/// A hook that runs inside the fake reviewer, before it answers.
pub type Hook = Box<dyn Fn(&Path) + Send + Sync>;

/// One scripted answer for the fake runner.
pub enum Step {
    /// Builder: write these files in the worktree, commit them, report this many input tokens.
    Build {
        files: Vec<(&'static str, &'static str)>,
        input_tokens: u64,
    },
    /// Reviewer: answer with this text. The last line is the verdict. The hook gets the main checkout.
    Review {
        text: &'static str,
        before: Option<Hook>,
    },
}

pub fn review(text: &'static str) -> Step {
    Step::Review { text, before: None }
}

pub fn build(files: Vec<(&'static str, &'static str)>, input_tokens: u64) -> Step {
    Step::Build {
        files,
        input_tokens,
    }
}

/// Commits several files in a repository and returns the new sha.
pub fn commit_files(
    dir: &Path,
    files: &[(&str, &str)],
    message: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let repo = Repository::open(dir)?;
    let mut index = repo.index()?;
    for (name, content) in files {
        // An empty content deletes the file.
        if content.is_empty() {
            std::fs::remove_file(dir.join(name))?;
            index.remove_path(Path::new(name))?;
            continue;
        }
        if let Some(parent) = dir.join(name).parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(dir.join(name), content)?;
        index.add_path(Path::new(name))?;
    }
    index.write()?;
    let tree = repo.find_tree(index.write_tree()?)?;
    let sig = Signature::now("t", "t@example.com")?;
    let parents: Vec<git2::Commit> = match repo.head() {
        Ok(h) => vec![h.peel_to_commit()?],
        Err(_) => vec![],
    };
    let refs: Vec<&git2::Commit> = parents.iter().collect();
    Ok(repo
        .commit(Some("HEAD"), &sig, &sig, message, &tree, &refs)?
        .to_string())
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

/// Plays the script in order and records the role of every call. It never runs the real `claude`.
pub struct ScriptRunner {
    steps: Mutex<VecDeque<Step>>,
    roles: Mutex<Vec<String>>,
    main: PathBuf,
}

impl ScriptRunner {
    pub fn roles(&self) -> Vec<String> {
        self.roles.lock().map(|r| r.clone()).unwrap_or_default()
    }
}

impl AgentRunner for ScriptRunner {
    fn run(&self, spec: &AgentSpec) -> AgentRun {
        if let Ok(mut roles) = self.roles.lock() {
            roles.push(spec.role.clone());
        }
        if spec.role == "committer" {
            commit_index(&spec.cwd, "feat(test): build step");
            return AgentRun {
                session_id: Some("sess-c".to_string()),
                ..AgentRun::done("done")
            };
        }
        // A message run never uses up a script step. Its reply has no message, so the fixed one is used.
        if spec.role == "message" {
            return AgentRun {
                session_id: Some("sess-m".to_string()),
                ..AgentRun::done("done")
            };
        }
        // A KB run never uses up a script step. It reports one page changed.
        if spec.role == "kb" {
            return AgentRun {
                session_id: Some("sess-k".to_string()),
                usage: Usage {
                    input_tokens: 50,
                    ..Usage::default()
                },
                weighted: 50,
                ..AgentRun::done("topic.md")
            };
        }
        let step = self.steps.lock().ok().and_then(|mut s| s.pop_front());
        let (text, input) = match step {
            Some(Step::Build {
                files,
                input_tokens,
            }) if spec.role == "builder" => {
                for (name, content) in &files {
                    // An empty content deletes the file, which the driver stages as a deletion.
                    if content.is_empty() {
                        let _ = std::fs::remove_file(spec.cwd.join(name));
                        continue;
                    }
                    if let Some(parent) = spec.cwd.join(name).parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    if let Err(e) = std::fs::write(spec.cwd.join(name), content) {
                        return AgentRun::failed(&format!("fake builder failed: {e}"));
                    }
                }
                ("done".to_string(), input_tokens)
            }
            Some(Step::Review { text, before }) if spec.role == "reviewer" => {
                if let Some(hook) = before {
                    hook(&self.main);
                }
                (text.to_string(), 100)
            }
            Some(_) => {
                return AgentRun::failed(&format!(
                    "script step does not match the role {}",
                    spec.role
                ))
            }
            None => return AgentRun::failed("script exhausted"),
        };
        let usage = Usage {
            input_tokens: input,
            ..Usage::default()
        };
        AgentRun {
            session_id: Some("sess-1".to_string()),
            usage,
            weighted: input,
            ..AgentRun::done(&text)
        }
    }
}

pub struct Scenario {
    pub tmp: tempfile::TempDir,
    pub root: PathBuf,
    pub plan_a: PathBuf,
    pub plan_b: PathBuf,
    pub runner: Arc<ScriptRunner>,
    ctx: CliCtx,
}

impl Scenario {
    /// `extra_config` is appended to the config file. The one check fails while `bad.flag` exists,
    /// so a builder can fail it on purpose and a resume can delete the flag.
    pub fn new(
        extra_config: &str,
        steps: Vec<Step>,
    ) -> Result<Scenario, Box<dyn std::error::Error>> {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path().join("repo");
        std::fs::create_dir_all(&root)?;
        let mut opts = git2::RepositoryInitOptions::new();
        opts.initial_head("main");
        Repository::init_opts(&root, &opts)?;
        commit_files(&root, &[("README.md", "demo\n")], "init")?;
        // The config stays untracked, which the merge preflight allows.
        std::fs::create_dir_all(root.join(".claude/factory"))?;
        let config = format!(
            "risk_paths = [\"secrets/**\"]\n{extra_config}\n[[checks]]\npaths = [\"**\"]\ncommands = [\"test ! -f bad.flag\"]\n"
        );
        std::fs::write(root.join(".claude/factory/config.toml"), config)?;
        std::fs::write(
            root.join(".claude/factory/commit-context.md"),
            "TEST CONTEXT\n",
        )?;
        let plans = tmp.path().join("plans");
        std::fs::create_dir_all(&plans)?;
        let plan_b = plans.join("plan-02-b.md");
        let plan_a = plans.join("plan-01-a.md");
        std::fs::write(&plan_b, "# B\n\n### Task 1: b\n\nNext plan: none\n")?;
        std::fs::write(
            &plan_a,
            format!("# A\n\n### Task 1: a\n\nNext plan: {}\n", plan_b.display()),
        )?;
        let runner = Arc::new(ScriptRunner {
            steps: Mutex::new(steps.into()),
            roles: Mutex::new(Vec::new()),
            main: root.clone(),
        });
        let dyn_runner: Arc<dyn AgentRunner> = runner.clone();
        let ctx = CliCtx::new(root.clone(), dyn_runner, "tester");
        Ok(Scenario {
            tmp,
            root,
            plan_a,
            plan_b,
            runner,
            ctx,
        })
    }

    /// The same scenario with an injected issue tracker instead of `gh`.
    pub fn with_tracker(mut self, tracker: Arc<dyn factory_engine::Tracker>) -> Scenario {
        self.ctx = self.ctx.clone().with_tracker(tracker);
        self
    }

    pub fn run(&self, args: &[&str]) -> Result<String, CliError> {
        let mut argv = vec!["factory"];
        argv.extend_from_slice(args);
        dispatch(Cli::parse_from(argv), &self.ctx)
    }

    /// Creates a job for a plan file and runs it, with `--series` when asked.
    pub fn start(&self, plan: &Path, series: bool) -> Result<String, CliError> {
        let plan = plan.to_string_lossy().to_string();
        self.run(&["new", "hello", "say hello", "--plans", &plan])?;
        if series {
            self.run(&["run", "1", "--series"])
        } else {
            self.run(&["run", "1"])
        }
    }

    pub fn job(&self, id: &str) -> Result<Job, Box<dyn std::error::Error>> {
        let board = Board::load(&self.root.join(".superpowers/factory/board.json"))?;
        Ok(board.find(id)?.clone())
    }

    pub fn stage(&self, id: &str) -> Result<String, Box<dyn std::error::Error>> {
        Ok(self.job(id)?.stage.to_string())
    }

    pub fn main_tip(&self) -> Result<String, Box<dyn std::error::Error>> {
        let repo = Repository::open(&self.root)?;
        let id = repo
            .revparse_single("main")?
            .peel_to_commit()?
            .id()
            .to_string();
        Ok(id)
    }

    pub fn merges_on_main(&self) -> Result<usize, Box<dyn std::error::Error>> {
        let repo = Repository::open(&self.root)?;
        let mut walk = repo.revwalk()?;
        walk.push_ref("refs/heads/main")?;
        let mut merges = 0;
        for oid in walk {
            if repo.find_commit(oid?)?.parent_count() > 1 {
                merges += 1;
            }
        }
        Ok(merges)
    }

    pub fn last_merge_subject(&self) -> Result<String, Box<dyn std::error::Error>> {
        let repo = Repository::open(&self.root)?;
        let commit = repo.revparse_single("main")?.peel_to_commit()?;
        let subject = commit.summary().unwrap_or("").to_string();
        Ok(subject)
    }

    pub fn second_parent_of_last_merge(&self) -> Result<String, Box<dyn std::error::Error>> {
        let repo = Repository::open(&self.root)?;
        let commit = repo.revparse_single("main")?.peel_to_commit()?;
        Ok(commit.parent_id(1)?.to_string())
    }
}
