#![allow(dead_code)]

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use factory_agent::{AgentRun, AgentRunner, AgentSpec, EventSink};
use factory_core::{Board, CheckRule, Config, Job};
use factory_engine::Engine;
use factory_git::Repo;
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
    let tree = repo.find_tree(index.write_tree().expect("tree")).expect("find tree");
    let sig = Signature::now("t", "t@example.com").expect("sig");
    let parents: Vec<git2::Commit> = match repo.head() {
        Ok(h) => vec![h.peel_to_commit().expect("commit")],
        Err(_) => vec![],
    };
    let refs: Vec<&git2::Commit> = parents.iter().collect();
    repo.commit(Some("HEAD"), &sig, &sig, "test commit", &tree, &refs).expect("commit").to_string()
}

/// The subject the fake committer gives its commits.
pub const AUTO_COMMIT_MESSAGE: &str = "feat(test): build step";

/// Commits whatever is staged in `dir`, like a committer agent would. Returns the new sha.
pub fn commit_index(dir: &Path, message: &str) -> String {
    let repo = Repository::open(dir).expect("open repo");
    let mut index = repo.index().expect("index");
    let tree = repo.find_tree(index.write_tree().expect("tree")).expect("find tree");
    let sig = Signature::now("t", "t@example.com").expect("sig");
    let parent = repo.head().expect("head").peel_to_commit().expect("commit");
    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&parent]).expect("commit").to_string()
}

/// Amends HEAD with whatever is staged in `dir` and a new message, like `git commit --amend`.
pub fn amend_head(dir: &Path, message: &str) -> String {
    let repo = Repository::open(dir).expect("open repo");
    let mut index = repo.index().expect("index");
    let tree = repo.find_tree(index.write_tree().expect("tree")).expect("find tree");
    let head = repo.head().expect("head").peel_to_commit().expect("commit");
    head.amend(Some("HEAD"), None, None, None, Some(message), Some(&tree)).expect("amend").to_string()
}

/// Writes the committer prompt context into the main checkout, where the engine reads it.
pub fn write_commit_context(main: &Path) {
    let path = main.join(".claude/factory/commit-context.md");
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(path, "TEST CONTEXT\n").expect("context");
}

/// A fixture whose main checkout has the committer context file, so the engine runs the committer.
pub fn committing_fixture() -> Fixture {
    let fx = Fixture::new();
    write_commit_context(&fx.main);
    fx
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
        init_repo(&main);
        let job_dir = tmp.path().join("job");
        std::fs::create_dir_all(&job_dir).expect("mkdir job");
        let plan_file = job_dir.join("plan.md");
        std::fs::write(&plan_file, "- [ ] Task 1\n").expect("plan");
        let board_path = tmp.path().join("board.json");
        let mut board = Board::default();
        board.jobs.push(Job::new("001", "2026-10-13-001-demo", "demo", "2026-10-13", "demo feature", 0.0));
        board.save(&board_path).expect("save board");
        let config = Config {
            fix_rounds: 2,
            diff_cap_lines: 4000,
            checks: vec![CheckRule { paths: vec!["**".into()], commands: vec!["true".into()] }],
            ..Config::default()
        };
        let repo = Repo::open(&main).expect("open");
        let events = EventSink::new(job_dir.join("events.jsonl"), "001");
        Self { tmp, main, board_path, job_dir, plan_file, repo, config, events }
    }

    pub fn engine<'a>(&'a self, runner: &'a dyn AgentRunner) -> Engine<'a> {
        let engine = Engine::new(
            self.board_path.clone(),
            self.job_dir.clone(),
            "001".to_string(),
            self.plan_file.clone(),
            &self.repo,
            runner,
            self.config.clone(),
            &self.events,
        )
        .expect("engine");
        let context = self.main.join(".claude/factory/commit-context.md");
        if context.exists() {
            engine.with_commit_context(context)
        } else {
            engine
        }
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
    /// When true, every `committer` run commits the staged index and does not use up an effect.
    auto_committer: bool,
}

impl ScriptedRunner {
    /// Every spec the runner was called with for one role, in order.
    pub fn specs_for(&self, role: &str) -> Vec<AgentSpec> {
        self.calls.lock().expect("calls").iter().filter(|s| s.role == role).cloned().collect()
    }

    pub fn last_spec_for(&self, role: &str) -> Option<AgentSpec> {
        self.specs_for(role).pop()
    }

    pub fn last_spec(&self) -> Option<AgentSpec> {
        self.calls.lock().expect("calls").last().cloned()
    }
}

impl AgentRunner for ScriptedRunner {
    fn run(&self, spec: &AgentSpec) -> AgentRun {
        self.calls.lock().expect("calls").push(spec.clone());
        if self.auto_committer && spec.role == "committer" {
            commit_index(&spec.cwd, AUTO_COMMIT_MESSAGE);
            return AgentRun { session_id: Some("sess-c".to_string()), ..AgentRun::done("done") };
        }
        let next = self.effects.lock().expect("effects").pop_front();
        match next {
            Some(effect) => AgentRun { session_id: Some("sess-1".to_string()), ..AgentRun::done(&effect(spec)) },
            None => AgentRun::failed("script exhausted"),
        }
    }
}

pub fn scripted(effects: Vec<Effect>) -> ScriptedRunner {
    ScriptedRunner { effects: Mutex::new(effects.into()), calls: Mutex::new(Vec::new()), auto_committer: false }
}

/// Like `scripted`, but committer runs are answered by committing the staged index.
pub fn scripted_auto(effects: Vec<Effect>) -> ScriptedRunner {
    ScriptedRunner { effects: Mutex::new(effects.into()), calls: Mutex::new(Vec::new()), auto_committer: true }
}

/// A merge fixture: a repo, a driver and a job `001` at stage `approve` with one reviewed commit.
pub struct MergeFixture {
    pub tmp: tempfile::TempDir,
    pub main: PathBuf,
    pub driver: factory_engine::Driver,
    pub runner: Arc<ScriptedRunner>,
    pub id: String,
    pub head: String,
}

/// A runner whose first `n` calls are reviewer replies of `VERDICT: APPROVE`.
pub fn approving_runner(n: usize) -> Arc<ScriptedRunner> {
    let effects: Vec<Effect> =
        (0..n).map(|_| Box::new(|_: &AgentSpec| "ok\nVERDICT: APPROVE".to_string()) as Effect).collect();
    Arc::new(scripted(effects))
}

pub fn init_repo(main: &Path) {
    std::fs::create_dir_all(main).expect("mkdir");
    let mut opts = git2::RepositoryInitOptions::new();
    opts.initial_head("main");
    Repository::init_opts(main, &opts).expect("init");
    commit_file(main, "README.md", "hello\n");
}

/// A driver whose runner factory hands every job the same shared scripted runner.
pub fn driver_for(main: &Path, runner: Arc<ScriptedRunner>, config: Config) -> factory_engine::Driver {
    let factory: factory_engine::RunnerFactory = Box::new(move |_job_id| runner.clone());
    factory_engine::Driver::new(factory_core::Layout::in_repo(main), config, factory).expect("driver")
}

/// Creates a repo, a job branch with one commit adding `docs/hello.txt`, a plan file and a board entry at `approve`.
pub fn approved_job() -> MergeFixture {
    approved_job_with(approving_runner(3), false)
}

/// Like `approved_job`, with a chosen runner. `with_context` commits the committer context file to main first.
pub fn approved_job_with(runner: Arc<ScriptedRunner>, with_context: bool) -> MergeFixture {
    approved_job_config(runner, with_context, Config::default())
}

/// Like `approved_job_with`, with a chosen config.
pub fn approved_job_config(runner: Arc<ScriptedRunner>, with_context: bool, mut config: Config) -> MergeFixture {
    let tmp = tempfile::tempdir().expect("tempdir");
    let main = tmp.path().join("repo");
    init_repo(&main);
    if config.base_branch != "main" {
        // A non-main base branch starts at the first commit.
        let repo = Repository::open(&main).expect("open");
        let head = repo.head().expect("head").peel_to_commit().expect("commit");
        repo.branch(&config.base_branch, &head, false).expect("base branch");
    }
    if with_context {
        commit_file(&main, ".claude/factory/commit-context.md", "TEST CONTEXT\n");
    }
    // A kb_path set to a relative name points at a folder inside the temp dir.
    if let Some(name) = config.kb_path.clone() {
        let kb = tmp.path().join(name);
        std::fs::create_dir_all(&kb).expect("mkdir kb");
        config.kb_path = Some(kb.to_string_lossy().into_owned());
    }
    if config.kb_path.is_some() {
        // The real repo ignores `.superpowers/`, and the KB run's tamper check relies on it.
        commit_file(&main, ".gitignore", ".superpowers/\n");
    }
    let driver = driver_for(&main, runner.clone(), config);
    let wt = driver
        .repo()
        .create_worktree(&factory_git::JobRef { id: "001".into(), slug: "hello".into() })
        .expect("worktree");
    let head = commit_file(&wt.path, "docs/hello.txt", "hello\n");
    let mut job = Job::new("001", "2026-10-13-001-hello", "hello", "2026-10-13", "say hello", 0.0);
    job.worktree = Some(wt.path.to_string_lossy().to_string());
    job.branch = wt.branch.clone();
    job.base = Some(wt.base.clone());
    job.head = Some(head.clone());
    job.set_stage(factory_core::Stage::Approve, None, false, 1.0).expect("stage");
    let dir = driver.job_dir(&job);
    std::fs::create_dir_all(&dir).expect("job dir");
    std::fs::write(dir.join("plan.md"), "- [ ] Task 1: hello\n").expect("plan");
    driver.ledger(&job).create("# SDD ledger").expect("ledger");
    driver
        .store()
        .update(|b| {
            b.jobs.push(job);
            Ok(())
        })
        .expect("board");
    MergeFixture { tmp, main, driver, runner, id: "001".into(), head }
}

/// The job worktree path from the board.
pub fn worktree_of(fx: &MergeFixture) -> PathBuf {
    PathBuf::from(fx.driver.job(&fx.id).expect("job").worktree.expect("worktree"))
}

pub fn main_tip(dir: &Path) -> String {
    let repo = Repository::open(dir).expect("open");
    let id = repo.revparse_single("main").expect("main").peel_to_commit().expect("commit").id();
    id.to_string()
}

pub fn branch_tip(fx: &MergeFixture) -> String {
    let repo = Repository::open(&fx.main).expect("open");
    let name = fx.driver.job(&fx.id).expect("job").branch;
    let id = repo.revparse_single(&name).expect("branch").peel_to_commit().expect("commit").id();
    id.to_string()
}

/// A runner that answers by role: the builder commits a new file, the reviewer replies with `verdict`.
/// Every run emits one event through the sink that the runner factory made for the job.
pub struct RoleRunner {
    sink: EventSink,
    verdict: String,
    counter: std::sync::atomic::AtomicUsize,
}

impl AgentRunner for RoleRunner {
    fn run(&self, spec: &AgentSpec) -> AgentRun {
        self.sink.emit(&factory_agent::AgentEvent {
            kind: factory_agent::EventKind::Start,
            agent_id: spec.agent_id(),
            agent_type: spec.role.clone(),
            tool_name: None,
            tool_target: None,
            cwd: None,
            last_message: None,
            tokens: 0,
        });
        let text = if spec.role == "builder" {
            let n = self.counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let path = spec.cwd.join(format!("docs/{}-{n}.txt", spec.job_id));
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(path, "built\n").expect("write");
            "done".to_string()
        } else if spec.role == "committer" {
            commit_index(&spec.cwd, AUTO_COMMIT_MESSAGE);
            "done".to_string()
        } else {
            self.verdict.clone()
        };
        AgentRun { session_id: Some("sess-1".to_string()), ..AgentRun::done(&text) }
    }
}

/// A repo plus a driver whose factory builds one `RoleRunner` and one event sink per job.
pub struct SeriesFixture {
    pub tmp: tempfile::TempDir,
    pub main: PathBuf,
    pub driver: factory_engine::Driver,
}

impl SeriesFixture {
    pub fn new(verdict: &str) -> SeriesFixture {
        SeriesFixture::with_config(verdict, Config::default())
    }

    pub fn with_config(verdict: &str, config: Config) -> SeriesFixture {
        SeriesFixture::build(verdict, config, |runner| Arc::new(runner))
    }

    /// A repo with the committer context, and a driver whose runner for each job is a `RoleRunner`
    /// with `verdict`, passed through `wrap`.
    fn build(verdict: &str, config: Config, wrap: impl Fn(RoleRunner) -> Arc<dyn AgentRunner> + 'static) -> SeriesFixture {
        let tmp = tempfile::tempdir().expect("tempdir");
        let main = tmp.path().join("repo");
        init_repo(&main);
        write_commit_context(&main);
        let events = main.join(".superpowers/factory/events.jsonl");
        let verdict = verdict.to_string();
        let factory: factory_engine::RunnerFactory = Box::new(move |job_id| {
            wrap(RoleRunner {
                sink: EventSink::new(events.clone(), job_id),
                verdict: verdict.clone(),
                counter: std::sync::atomic::AtomicUsize::new(0),
            })
        });
        let driver = factory_engine::Driver::new(factory_core::Layout::in_repo(&main), config, factory).expect("driver");
        SeriesFixture { tmp, main, driver }
    }

    /// Writes a valid plan file into a `plans/` folder of the temp dir and returns its path.
    pub fn plan_file(&self, name: &str, tasks: u32, next: &str) -> PathBuf {
        let dir = self.tmp.path().join("plans");
        std::fs::create_dir_all(&dir).expect("plans dir");
        let mut s = String::from("# Plan\n\n");
        for n in 1..=tasks {
            s.push_str(&format!("### Task {n}: t\n\n"));
        }
        let next = if next == "none" { next.to_string() } else { dir.join(next).to_string_lossy().into_owned() };
        s.push_str(&format!("Next plan: {next}\n"));
        let path = dir.join(name);
        std::fs::write(&path, s).expect("write plan");
        path
    }
}

/// A call hook that runs before every agent run of an owned series fixture.
pub type RunHook = Arc<dyn Fn(&AgentSpec) + Send + Sync>;

struct HookRunner {
    inner: RoleRunner,
    hook: RunHook,
}

impl AgentRunner for HookRunner {
    fn run(&self, spec: &AgentSpec) -> AgentRun {
        (self.hook)(spec);
        self.inner.run(spec)
    }
}

impl SeriesFixture {
    /// Like `new`, with a lease owner on the driver and a hook that runs before every agent run.
    pub fn owned(owner: &str, hook: RunHook) -> SeriesFixture {
        let mut fx = SeriesFixture::build("fine\nVERDICT: APPROVE", Config::default(), move |inner| {
            Arc::new(HookRunner { inner, hook: hook.clone() })
        });
        let driver = fx.driver;
        fx.driver = driver.with_lease_owner(owner);
        fx
    }

    /// Like `owned`, with the lease ttl and the heartbeat interval set.
    pub fn owned_timed(owner: &str, hook: RunHook, ttl: f64, beat: std::time::Duration) -> SeriesFixture {
        let mut fx = SeriesFixture::owned(owner, hook);
        let tmp_driver = fx.driver;
        fx.driver = tmp_driver.with_lease_timing(ttl, beat);
        fx
    }

    /// A second driver on the same layout with another lease owner. Its runner must never be used.
    pub fn other_driver(&self, owner: &str) -> factory_engine::Driver {
        let factory: factory_engine::RunnerFactory = Box::new(|_| panic!("the second driver must not run an agent"));
        factory_engine::Driver::new(factory_core::Layout::in_repo(&self.main), Config::default(), factory)
            .expect("driver")
            .with_lease_owner(owner)
    }
}

/// Moves main to `head` by hand, as a person merging outside the driver would.
pub fn merge_by_hand(fx: &MergeFixture) -> String {
    let repo = Repository::open(&fx.main).expect("open");
    let oid = git2::Oid::from_str(&fx.head).expect("oid");
    repo.reference("refs/heads/main", oid, true, "hand merge").expect("move main");
    repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force())).expect("checkout");
    fx.head.clone()
}

/// Records `sha` as the reviewed head.
pub fn record_head(fx: &MergeFixture, sha: &str) {
    let sha = sha.to_string();
    fx.driver
        .store()
        .update_job(&fx.id, |j| {
            j.head = Some(sha);
            Ok(())
        })
        .expect("record head");
}

use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub struct FakeState {
    pub repo: String,
    pub labels: HashMap<u32, Vec<String>>,
    pub closed: HashSet<u32>,
    pub next: u32,
    pub calls: Vec<String>,
    pub created: Vec<(String, String, Vec<String>)>,
    pub comments: Vec<(u32, String)>,
    pub closes: Vec<u32>,
    pub fail: HashSet<String>,
}

/// A tracker that records its calls. Clones share one state, so a test keeps a handle after the driver owns the box.
#[derive(Clone)]
pub struct FakeTracker {
    pub state: Arc<Mutex<FakeState>>,
}

impl FakeTracker {
    pub fn new(repo: &str) -> FakeTracker {
        let state = FakeState { repo: repo.to_string(), next: 200, ..FakeState::default() };
        FakeTracker { state: Arc::new(Mutex::new(state)) }
    }
    pub fn with_labels(self, issue: u32, labels: &[&str]) -> FakeTracker {
        self.state.lock().expect("lock").labels.insert(issue, labels.iter().map(|s| s.to_string()).collect());
        self
    }
    pub fn fail_on(&self, method: &str) {
        self.state.lock().expect("lock").fail.insert(method.to_string());
    }
    pub fn clear_failures(&self) {
        self.state.lock().expect("lock").fail.clear();
    }
    pub fn calls(&self) -> Vec<String> {
        self.state.lock().expect("lock").calls.clone()
    }
    pub fn count(&self, method: &str) -> usize {
        self.calls().iter().filter(|c| c.starts_with(method)).count()
    }
    fn enter(&self, call: String, method: &str) -> Result<std::sync::MutexGuard<'_, FakeState>, String> {
        let mut s = self.state.lock().expect("lock");
        s.calls.push(call);
        if s.fail.contains(method) {
            return Err(format!("fake {method} failure"));
        }
        Ok(s)
    }
}

impl factory_engine::Tracker for FakeTracker {
    fn repo_name(&self) -> Result<String, String> {
        Ok(self.enter("repo_name".into(), "repo_name")?.repo.clone())
    }
    fn labels_of(&self, issue: u32) -> Result<Vec<String>, String> {
        Ok(self.enter(format!("labels_of {issue}"), "labels_of")?.labels.get(&issue).cloned().unwrap_or_default())
    }
    fn is_closed(&self, issue: u32) -> Result<bool, String> {
        Ok(self.enter(format!("is_closed {issue}"), "is_closed")?.closed.contains(&issue))
    }
    fn create(&self, title: &str, body: &str, labels: &[String]) -> Result<u32, String> {
        let mut s = self.enter(format!("create {title}"), "create")?;
        s.next += 1;
        let number = s.next;
        s.created.push((title.to_string(), body.to_string(), labels.to_vec()));
        Ok(number)
    }
    fn comment(&self, issue: u32, body: &str) -> Result<(), String> {
        let mut s = self.enter(format!("comment {issue}"), "comment")?;
        s.comments.push((issue, body.to_string()));
        Ok(())
    }
    fn close(&self, issue: u32) -> Result<(), String> {
        let mut s = self.enter(format!("close {issue}"), "close")?;
        s.closes.push(issue);
        s.closed.insert(issue);
        Ok(())
    }
}

/// Gives the fixture's driver a tracker, keeping everything else.
pub fn with_tracker(fx: MergeFixture, tracker: FakeTracker) -> MergeFixture {
    let MergeFixture { tmp, main, driver, runner, id, head } = fx;
    MergeFixture { tmp, main, driver: driver.with_tracker(Box::new(tracker)), runner, id, head }
}

/// Sets the feature text of a job on the board, and writes `spec.md` into its folder when given.
pub fn set_feature(fx: &MergeFixture, feature: &str, spec: Option<&str>) {
    let text = feature.to_string();
    fx.driver
        .store()
        .update_job(&fx.id, move |j| {
            j.feature = text;
            Ok(())
        })
        .expect("feature");
    if let Some(spec) = spec {
        let job = fx.driver.job(&fx.id).expect("job");
        std::fs::write(fx.driver.job_dir(&job).join("spec.md"), spec).expect("spec");
    }
}

/// An agent effect that replies with `text` and touches nothing.
pub fn reply(text: &'static str) -> Effect {
    Box::new(move |_: &AgentSpec| text.to_string())
}

impl Fixture {
    /// The job ledger, empty when nothing was written yet.
    pub fn ledger(&self) -> String {
        std::fs::read_to_string(self.job_dir.join("progress.md")).unwrap_or_default()
    }
}

impl MergeFixture {
    /// The ledger of the merge job.
    pub fn ledger(&self) -> String {
        let job = self.driver.job(&self.id).expect("job");
        std::fs::read_to_string(self.driver.job_dir(&job).join("progress.md")).expect("ledger")
    }
}
