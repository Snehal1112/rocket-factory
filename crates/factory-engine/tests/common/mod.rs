#![allow(dead_code)]

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use factory_agent::{AgentRun, AgentRunner, AgentSpec, EventSink};
use factory_core::{Board, CheckRule, Config, Job};
use factory_engine::Engine;
use factory_git::Repo;
use git2::{Repository, Signature};

/// Commits one file in the repository at `dir` and returns the new sha.
pub fn commit_file(dir: &Path, name: &str, content: &str) -> String {
    let repo = Repository::open(dir).expect("open repo");
    std::fs::write(dir.join(name), content).expect("write file");
    let mut index = repo.index().expect("index");
    index.add_path(Path::new(name)).expect("add");
    index.write().expect("write index");
    let tree = repo.find_tree(index.write_tree().expect("tree")).expect("find tree");
    let sig = Signature::now("t", "t@example.com").expect("sig");
    let parents: Vec<git2::Commit> = match repo.head() {
        Ok(h) => vec![h.peel_to_commit().expect("commit")],
        Err(_) => vec![],
    };
    let refs: Vec<&git2::Commit> = parents.iter().collect();
    repo.commit(Some("HEAD"), &sig, &sig, "test commit", &tree, &refs).expect("commit").to_string()
}

pub struct Fixture {
    pub tmp: tempfile::TempDir,
    pub main: PathBuf,
    pub board_path: PathBuf,
    pub job_dir: PathBuf,
    pub plan_file: PathBuf,
    pub repo: Repo,
    pub config: Config,
    pub events: EventSink,
}

impl Fixture {
    /// A temp repo on `main` with one commit, plus a board holding job `001`.
    pub fn new() -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let main = tmp.path().join("main");
        std::fs::create_dir_all(&main).expect("mkdir");
        let mut opts = git2::RepositoryInitOptions::new();
        opts.initial_head("main");
        Repository::init_opts(&main, &opts).expect("init");
        commit_file(&main, "README.md", "hello\n");
        let job_dir = tmp.path().join("job");
        std::fs::create_dir_all(&job_dir).expect("mkdir job");
        let plan_file = job_dir.join("plan.md");
        std::fs::write(&plan_file, "- [ ] Task 1\n").expect("plan");
        let board_path = tmp.path().join("board.json");
        let mut board = Board::default();
        board.jobs.push(Job::new("001", "2026-10-13-001-demo", "demo", "2026-10-13", "demo feature", 0.0));
        board.save(&board_path).expect("save board");
        let mut config = Config::default();
        config.fix_rounds = 2;
        config.diff_cap_lines = 4000;
        config.checks = vec![CheckRule { paths: vec!["**".into()], commands: vec!["true".into()] }];
        let repo = Repo::open(&main).expect("open");
        let events = EventSink::new(job_dir.join("events.jsonl"), "001");
        Self { tmp, main, board_path, job_dir, plan_file, repo, config, events }
    }

    pub fn engine<'a>(&'a self, runner: &'a dyn AgentRunner) -> Engine<'a> {
        Engine::new(
            self.board_path.clone(),
            self.job_dir.clone(),
            "001".to_string(),
            self.plan_file.clone(),
            &self.repo,
            runner,
            self.config.clone(),
            &self.events,
        )
        .expect("engine")
    }

    pub fn job(&self) -> Job {
        let board = Board::load(&self.board_path).expect("load");
        board.find("001").expect("job").clone()
    }
}

pub type Effect = Box<dyn Fn(&AgentSpec) -> String + Send + Sync>;

/// Plays one effect per call and records every spec it was given.
pub struct ScriptedRunner {
    effects: Mutex<VecDeque<Effect>>,
    calls: Mutex<Vec<AgentSpec>>,
}

impl ScriptedRunner {
    pub fn last_spec(&self) -> Option<AgentSpec> {
        self.calls.lock().expect("calls").last().cloned()
    }
}

impl AgentRunner for ScriptedRunner {
    fn run(&self, spec: &AgentSpec) -> AgentRun {
        self.calls.lock().expect("calls").push(spec.clone());
        let next = self.effects.lock().expect("effects").pop_front();
        match next {
            Some(effect) => AgentRun { session_id: Some("sess-1".to_string()), ..AgentRun::done(&effect(spec)) },
            None => AgentRun::failed("script exhausted"),
        }
    }
}

pub fn scripted(effects: Vec<Effect>) -> ScriptedRunner {
    ScriptedRunner { effects: Mutex::new(effects.into()), calls: Mutex::new(Vec::new()) }
}
