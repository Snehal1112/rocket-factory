#![allow(dead_code)]

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use factory_agent::{AgentRun, AgentRunner, AgentSpec, EventSink, StopReason};
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

/// Commits whatever is staged in `dir`, like a committer agent would. Returns the new sha.
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

/// Amends HEAD with whatever is staged in `dir` and a new message, like `git commit --amend`.
pub fn amend_head(dir: &Path, message: &str) -> String {
    let repo = Repository::open(dir).expect("open repo");
    let mut index = repo.index().expect("index");
    let tree = repo
        .find_tree(index.write_tree().expect("tree"))
        .expect("find tree");
    let head = repo.head().expect("head").peel_to_commit().expect("commit");
    head.amend(Some("HEAD"), None, None, None, Some(message), Some(&tree))
        .expect("amend")
        .to_string()
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
        board.jobs.push(Job::new(
            "001",
            "2026-10-13-001-demo",
            "demo",
            "2026-10-13",
            "demo feature",
            0.0,
        ));
        board.save(&board_path).expect("save board");
        let config = Config {
            fix_rounds: 2,
            diff_cap_lines: 4000,
            checks: vec![CheckRule {
                paths: vec!["**".into()],
                commands: vec!["true".into()],
                ..CheckRule::default()
            }],
            // These tests cover fresh task builders. `task_carry.rs` turns the carry on itself.
            builder_carry_session: false,
            // A test machine with little free disk must never wait. `builder_recover.rs` turns it on itself.
            min_free_disk_mb: 0,
            ..Config::default()
        };
        let repo = Repo::open(&main).expect("open");
        let events = EventSink::new(job_dir.join("events.jsonl"), "001");
        Self {
            tmp,
            main,
            board_path,
            job_dir,
            plan_file,
            repo,
            config,
            events,
        }
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

    /// Moves the parked job back to build, as rework does, so the next build call can run.
    pub fn resume_to_build(&self) {
        factory_engine::BoardStore::new(&self.board_path, std::time::Duration::from_secs(5))
            .update_job("001", |j| j.resume(factory_core::Stage::Build, 1.0))
            .expect("resume to build");
    }

    /// Resets the job branch in its worktree back to the base, so the branch holds no commits above it.
    pub fn reset_branch_to_base(&self) {
        let job = self.job();
        let wt = Repository::open(job.worktree.expect("worktree")).expect("open worktree");
        let base = wt
            .revparse_single(&job.base.expect("base"))
            .expect("base commit");
        wt.reset(&base, git2::ResetType::Hard, None)
            .expect("reset to base");
    }
}

pub type Effect = Box<dyn Fn(&AgentSpec) -> String + Send + Sync>;

/// Plays one effect per call and records every spec it was given.
pub struct ScriptedRunner {
    effects: Mutex<VecDeque<Effect>>,
    /// Per-role queues from `scripted_roles`. They take the place of `effects` when set.
    role_effects: Option<Mutex<HashMap<String, VecDeque<Effect>>>>,
    calls: Mutex<Vec<AgentSpec>>,
}

impl ScriptedRunner {
    /// How many times the runner was called.
    pub fn call_count(&self) -> usize {
        self.calls.lock().expect("calls").len()
    }

    /// Every spec the runner was called with for one role, in order.
    pub fn specs_for(&self, role: &str) -> Vec<AgentSpec> {
        self.calls
            .lock()
            .expect("calls")
            .iter()
            .filter(|s| s.role == role)
            .cloned()
            .collect()
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
        let next = match &self.role_effects {
            Some(roles) => roles
                .lock()
                .expect("effects")
                .get_mut(&spec.role)
                .and_then(VecDeque::pop_front),
            None => self.effects.lock().expect("effects").pop_front(),
        };
        match next {
            Some(effect) => AgentRun {
                session_id: Some("sess-1".to_string()),
                ..AgentRun::done(&effect(spec))
            },
            None => AgentRun::failed("script exhausted"),
        }
    }
}

/// Fails the first runs of one role with an environment error, then hands every run to `inner`.
pub struct RoleFailing {
    pub spec_role: &'static str,
    pub reason: StopReason,
    pub fails_left: AtomicU32,
    pub inner: Arc<ScriptedRunner>,
}

impl RoleFailing {
    pub fn new(
        spec_role: &'static str,
        reason: StopReason,
        fails: u32,
        inner: Arc<ScriptedRunner>,
    ) -> RoleFailing {
        RoleFailing {
            spec_role,
            reason,
            fails_left: AtomicU32::new(fails),
            inner,
        }
    }
}

impl AgentRunner for RoleFailing {
    fn run(&self, spec: &AgentSpec) -> AgentRun {
        if spec.role == self.spec_role {
            let took = self
                .fails_left
                .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1));
            if took.is_ok() {
                return AgentRun {
                    stop_reason: self.reason,
                    error: Some("claude reported success: API Error: 401 Invalid API key".into()),
                    ..AgentRun::done("")
                };
            }
        }
        self.inner.run(spec)
    }
}

/// A failed run with the given error, turns and token counts. The weighted total is the one given.
pub fn failed_run(error: &str, turns: u32, output: u64, weighted: u64) -> AgentRun {
    AgentRun {
        turns,
        weighted,
        usage: factory_agent::Usage {
            output_tokens: output,
            input_tokens: weighted.saturating_sub(output),
            ..factory_agent::Usage::default()
        },
        ..AgentRun::failed(error)
    }
}

/// One scripted builder answer. The first unused step whose `when` matches is used up.
pub struct Step {
    pub when: fn(&AgentSpec) -> bool,
    /// `Some` is returned as is, `None` hands the call to the inner runner.
    pub run: Option<AgentRun>,
}

pub fn any(_: &AgentSpec) -> bool {
    true
}

pub fn resumed(spec: &AgentSpec) -> bool {
    spec.resume.is_some()
}

pub fn fresh(spec: &AgentSpec) -> bool {
    spec.resume.is_none()
}

/// Answers builder calls from a list of steps and every other call from `inner`.
pub struct BuilderScript {
    inner: Arc<ScriptedRunner>,
    steps: Mutex<Vec<Option<Step>>>,
    clock: Option<Arc<dyn Fn() -> f64 + Send + Sync>>,
    times: Mutex<Vec<f64>>,
    seen: Mutex<Vec<AgentSpec>>,
}

impl BuilderScript {
    pub fn new(inner: Arc<ScriptedRunner>, steps: Vec<Step>) -> BuilderScript {
        BuilderScript {
            inner,
            steps: Mutex::new(steps.into_iter().map(Some).collect()),
            clock: None,
            times: Mutex::new(Vec::new()),
            seen: Mutex::new(Vec::new()),
        }
    }

    /// Records the clock at each builder call.
    pub fn with_clock(mut self, clock: Arc<dyn Fn() -> f64 + Send + Sync>) -> BuilderScript {
        self.clock = Some(clock);
        self
    }

    /// The clock reading of each builder call, in order.
    pub fn call_times(&self) -> Vec<f64> {
        self.times.lock().expect("times").clone()
    }

    /// Every builder spec the script was called with, in order.
    pub fn specs(&self) -> Vec<AgentSpec> {
        self.seen.lock().expect("seen").clone()
    }
}

impl AgentRunner for BuilderScript {
    fn run(&self, spec: &AgentSpec) -> AgentRun {
        if spec.role != "builder" {
            return self.inner.run(spec);
        }
        if let Some(clock) = &self.clock {
            self.times.lock().expect("times").push(clock());
        }
        self.seen.lock().expect("seen").push(spec.clone());
        let step = {
            let mut steps = self.steps.lock().expect("steps");
            let at = steps
                .iter()
                .position(|s| s.as_ref().is_some_and(|s| (s.when)(spec)));
            at.and_then(|i| steps[i].take())
        };
        match step {
            Some(Step { run: Some(run), .. }) => run,
            _ => self.inner.run(spec),
        }
    }
}

/// Gives each builder call its own session id and a fixed end context.
pub struct Tagged {
    pub inner: ScriptedRunner,
    pub end_ctx: u64,
    pub calls: Mutex<u32>,
    pub no_session: bool,
}

impl Tagged {
    pub fn new(inner: ScriptedRunner, end_ctx: u64) -> Tagged {
        Tagged {
            inner,
            end_ctx,
            calls: Mutex::new(0),
            no_session: false,
        }
    }
}

impl AgentRunner for Tagged {
    fn run(&self, spec: &AgentSpec) -> AgentRun {
        let mut run = self.inner.run(spec);
        if spec.role == "builder" {
            let mut n = self.calls.lock().expect("calls");
            *n += 1;
            run.session_id = if self.no_session {
                None
            } else {
                Some(format!("sess-{n}"))
            };
            run.end_ctx = self.end_ctx;
        }
        run
    }
}

pub fn scripted(effects: Vec<Effect>) -> ScriptedRunner {
    ScriptedRunner {
        effects: Mutex::new(effects.into()),
        role_effects: None,
        calls: Mutex::new(Vec::new()),
    }
}

/// Like `scripted`, with the effects queued per spec role, so the order of concurrent calls does not matter.
pub fn scripted_roles(effects: Vec<(&'static str, Effect)>) -> ScriptedRunner {
    let mut roles: HashMap<String, VecDeque<Effect>> = HashMap::new();
    for (role, effect) in effects {
        roles.entry(role.to_string()).or_default().push_back(effect);
    }
    ScriptedRunner {
        effects: Mutex::new(VecDeque::new()),
        role_effects: Some(Mutex::new(roles)),
        calls: Mutex::new(Vec::new()),
    }
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
    let effects: Vec<Effect> = (0..n)
        .map(|_| Box::new(|_: &AgentSpec| "ok\nVERDICT: APPROVE".to_string()) as Effect)
        .collect();
    Arc::new(scripted(effects))
}

pub fn init_repo(main: &Path) {
    std::fs::create_dir_all(main).expect("mkdir");
    let mut opts = git2::RepositoryInitOptions::new();
    opts.initial_head("main");
    let git = Repository::init_opts(main, &opts).expect("init");
    // The user's global git config may turn signing on.
    git.config()
        .expect("config")
        .set_bool("commit.gpgsign", false)
        .expect("gpgsign");
    commit_file(main, "README.md", "hello\n");
}

/// The config a test driver gets. A test machine with little free disk must never wait, so the disk check
/// is off unless the test set its own limit. The cleanup after a merge is off, because many tests read a
/// merged job's worktree or branch.
pub fn test_config(mut config: Config) -> Config {
    if config.min_free_disk_mb == Config::default().min_free_disk_mb {
        config.min_free_disk_mb = 0;
    }
    config.cleanup_on_merge = false;
    config
}

/// A driver whose runner factory hands every job the same shared scripted runner.
pub fn driver_for(
    main: &Path,
    runner: Arc<ScriptedRunner>,
    config: Config,
) -> factory_engine::Driver {
    let config = test_config(config);
    let factory: factory_engine::RunnerFactory = Box::new(move |_job_id| runner.clone());
    factory_engine::Driver::new(factory_core::Layout::in_repo(main), config, factory)
        .expect("driver")
}

/// Creates a repo, a job branch with one commit adding `docs/hello.txt`, a plan file and a board entry at `approve`.
pub fn approved_job() -> MergeFixture {
    approved_job_with(approving_runner(3))
}

/// Like `approved_job`, with a chosen runner.
pub fn approved_job_with(runner: Arc<ScriptedRunner>) -> MergeFixture {
    approved_job_config(runner, Config::default())
}

/// Like `approved_job_with`, with a chosen config.
pub fn approved_job_config(runner: Arc<ScriptedRunner>, config: Config) -> MergeFixture {
    approved_job_dyn(runner.clone(), runner, config)
}

/// Like `approved_job_config`, with the driver's runner apart from the scripted runner the fixture exposes.
/// A test wraps the scripted runner in `RoleFailing` and passes the wrapper as `runner`.
pub fn approved_job_dyn(
    runner: Arc<dyn AgentRunner>,
    scripted: Arc<ScriptedRunner>,
    config: Config,
) -> MergeFixture {
    approved_job_built(runner, scripted, config, false)
}

/// Like `approved_job`, with the cleanup after a merge turned on.
pub fn approved_job_with_cleanup() -> MergeFixture {
    let runner = approving_runner(3);
    approved_job_built(runner.clone(), runner, Config::default(), true)
}

fn approved_job_built(
    runner: Arc<dyn AgentRunner>,
    scripted: Arc<ScriptedRunner>,
    mut config: Config,
    cleanup_on_merge: bool,
) -> MergeFixture {
    let tmp = tempfile::tempdir().expect("tempdir");
    let main = tmp.path().join("repo");
    init_repo(&main);
    if config.base_branch != "main" {
        // A non-main base branch starts at the first commit.
        let repo = Repository::open(&main).expect("open");
        let head = repo.head().expect("head").peel_to_commit().expect("commit");
        repo.branch(&config.base_branch, &head, false)
            .expect("base branch");
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
    let factory: factory_engine::RunnerFactory = Box::new(move |_job_id| runner.clone());
    let mut config = test_config(config);
    config.cleanup_on_merge = cleanup_on_merge;
    let driver = factory_engine::Driver::new(factory_core::Layout::in_repo(&main), config, factory)
        .expect("driver");
    let wt = driver
        .repo()
        .create_worktree(&factory_git::JobRef {
            id: "001".into(),
            slug: "hello".into(),
            project: None,
        })
        .expect("worktree");
    let head = commit_file(&wt.path, "docs/hello.txt", "hello\n");
    let mut job = Job::new(
        "001",
        "2026-10-13-001-hello",
        "hello",
        "2026-10-13",
        "say hello",
        0.0,
    );
    job.worktree = Some(wt.path.to_string_lossy().to_string());
    job.branch = wt.branch.clone();
    job.base = Some(wt.base.clone());
    job.head = Some(head.clone());
    job.set_stage(factory_core::Stage::Approve, None, false, 1.0)
        .expect("stage");
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
    MergeFixture {
        tmp,
        main,
        driver,
        runner: scripted,
        id: "001".into(),
        head,
    }
}

/// Like `approved_job`, for the `web` project in `apps/web`. The job branch commits each of `files`, repo-relative.
pub fn approved_scoped_job(files: &[&str]) -> MergeFixture {
    approved_scoped_job_with(files, Config::default())
}

/// Like `approved_scoped_job`, with the given config.
pub fn approved_scoped_job_with(files: &[&str], config: Config) -> MergeFixture {
    let tmp = tempfile::tempdir().expect("tempdir");
    let main = tmp.path().join("repo");
    init_repo(&main);
    commit_file(&main, "apps/web/index.txt", "web\n");
    let mut layout = factory_core::Layout::in_repo(&main);
    layout.scope = Some(factory_core::Scope {
        project: "web".into(),
        subpath: PathBuf::from("apps/web"),
    });
    let runner = approving_runner(3);
    let rf = runner.clone();
    let factory: factory_engine::RunnerFactory = Box::new(move |_job_id| rf.clone());
    let driver = factory_engine::Driver::new(layout, test_config(config), factory).expect("driver");
    let wt = driver
        .repo()
        .create_worktree(&factory_git::JobRef {
            id: "001".into(),
            slug: "hello".into(),
            project: Some("web".into()),
        })
        .expect("worktree");
    let mut head = String::new();
    for file in files {
        head = commit_file(&wt.path, file, "x\n");
    }
    let mut job = Job::new(
        "001",
        "2026-10-13-001-hello",
        "hello",
        "2026-10-13",
        "say hello",
        0.0,
    );
    job.worktree = Some(wt.path.to_string_lossy().to_string());
    job.branch = wt.branch.clone();
    job.base = Some(wt.base.clone());
    job.head = Some(head.clone());
    job.set_stage(factory_core::Stage::Approve, None, false, 1.0)
        .expect("stage");
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
    MergeFixture {
        tmp,
        main,
        driver,
        runner,
        id: "001".into(),
        head,
    }
}

/// The job worktree path from the board.
pub fn worktree_of(fx: &MergeFixture) -> PathBuf {
    PathBuf::from(
        fx.driver
            .job(&fx.id)
            .expect("job")
            .worktree
            .expect("worktree"),
    )
}

/// Turns signing on in the repo at `main`, with a signer script made from `body` in `dir`.
pub fn signer_on(main: &Path, dir: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let script = dir.join("fake-signer");
    std::fs::write(&script, format!("#!/bin/sh\n{body}\n")).expect("script");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let repo = Repository::open(main).expect("open");
    let mut cfg = repo.config().expect("config");
    cfg.set_bool("commit.gpgsign", true).expect("gpgsign");
    cfg.set_str("gpg.program", script.to_str().expect("utf8"))
        .expect("program");
    cfg.set_str("user.signingkey", "ABC").expect("key");
}

/// A signer that fails at once, as gpg does when the passphrase is not cached.
pub fn failing_signer(main: &Path, dir: &Path) {
    signer_on(
        main,
        dir,
        "echo 'gpg: signing failed: No pinentry' >&2\nexit 2",
    );
}

/// A signer that makes a signature, as a working gpg does.
pub fn working_signer(main: &Path, dir: &Path) {
    signer_on(
        main,
        dir,
        "cat > /dev/null\necho '[GNUPG:] SIG_CREATED D 22 8 00 1 ABC' >&2\nprintf -- '-----BEGIN PGP SIGNATURE-----\\nfake\\n-----END PGP SIGNATURE-----\\n'",
    );
}

pub fn main_tip(dir: &Path) -> String {
    let repo = Repository::open(dir).expect("open");
    let id = repo
        .revparse_single("main")
        .expect("main")
        .peel_to_commit()
        .expect("commit")
        .id();
    id.to_string()
}

pub fn branch_tip(fx: &MergeFixture) -> String {
    let repo = Repository::open(&fx.main).expect("open");
    let name = fx.driver.job(&fx.id).expect("job").branch;
    let id = repo
        .revparse_single(&name)
        .expect("branch")
        .peel_to_commit()
        .expect("commit")
        .id();
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
            let n = self
                .counter
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let path = spec.cwd.join(format!("docs/{}-{n}.txt", spec.job_id));
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(path, "built\n").expect("write");
            "done".to_string()
        } else if spec.role == "spec-reviewer" {
            "VERDICT: APPROVE".to_string()
        } else {
            self.verdict.clone()
        };
        AgentRun {
            session_id: Some("sess-1".to_string()),
            ..AgentRun::done(&text)
        }
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

    /// A repo and a driver whose runner for each job is a `RoleRunner`
    /// with `verdict`, passed through `wrap`.
    pub fn build(
        verdict: &str,
        config: Config,
        wrap: impl Fn(RoleRunner) -> Arc<dyn AgentRunner> + 'static,
    ) -> SeriesFixture {
        let tmp = tempfile::tempdir().expect("tempdir");
        let main = tmp.path().join("repo");
        init_repo(&main);
        let events = main.join(".superpowers/factory/events.jsonl");
        let verdict = verdict.to_string();
        let factory: factory_engine::RunnerFactory = Box::new(move |job_id| {
            wrap(RoleRunner {
                sink: EventSink::new(events.clone(), job_id),
                verdict: verdict.clone(),
                counter: std::sync::atomic::AtomicUsize::new(0),
            })
        });
        let driver = factory_engine::Driver::new(
            factory_core::Layout::in_repo(&main),
            test_config(config),
            factory,
        )
        .expect("driver");
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
        let next = if next == "none" {
            next.to_string()
        } else {
            dir.join(next).to_string_lossy().into_owned()
        };
        s.push_str(&format!("Next plan: {next}\n"));
        let path = dir.join(name);
        std::fs::write(&path, s).expect("write plan");
        path
    }
}

/// A call hook that runs before every agent run of an owned series fixture.
pub type RunHook = Arc<dyn Fn(&AgentSpec) + Send + Sync>;

pub struct HookRunner {
    pub inner: RoleRunner,
    pub hook: RunHook,
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
        let mut fx =
            SeriesFixture::build("fine\nVERDICT: APPROVE", Config::default(), move |inner| {
                Arc::new(HookRunner {
                    inner,
                    hook: hook.clone(),
                })
            });
        let driver = fx.driver;
        fx.driver = driver.with_lease_owner(owner);
        fx
    }

    /// Like `owned`, with the lease ttl and the heartbeat interval set.
    pub fn owned_timed(
        owner: &str,
        hook: RunHook,
        ttl: f64,
        beat: std::time::Duration,
    ) -> SeriesFixture {
        let mut fx = SeriesFixture::owned(owner, hook);
        let tmp_driver = fx.driver;
        fx.driver = tmp_driver.with_lease_timing(ttl, beat);
        fx
    }

    /// A second driver on the same layout with another lease owner. Its runner must never be used.
    pub fn other_driver(&self, owner: &str) -> factory_engine::Driver {
        let factory: factory_engine::RunnerFactory =
            Box::new(|_| panic!("the second driver must not run an agent"));
        factory_engine::Driver::new(
            factory_core::Layout::in_repo(&self.main),
            test_config(Config::default()),
            factory,
        )
        .expect("driver")
        .with_lease_owner(owner)
    }
}

/// Moves main to `head` by hand, as a person merging outside the driver would.
pub fn merge_by_hand(fx: &MergeFixture) -> String {
    let repo = Repository::open(&fx.main).expect("open");
    move_main_to(&repo, git2::Oid::from_str(&fx.head).expect("oid"));
    fx.head.clone()
}

/// Points main at `oid` and checks it out, so the index and the folder follow.
fn move_main_to(repo: &Repository, oid: git2::Oid) {
    repo.reference("refs/heads/main", oid, true, "hand merge")
        .expect("move main");
    repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
        .expect("checkout");
}

/// Merges the job head into main with a merge commit, as a person would. Returns its sha.
pub fn merge_by_hand_no_ff(fx: &MergeFixture) -> String {
    let repo = Repository::open(&fx.main).expect("open");
    let head = repo
        .find_commit(git2::Oid::from_str(&fx.head).expect("oid"))
        .expect("head commit");
    let tip = repo
        .revparse_single("main")
        .expect("main")
        .peel_to_commit()
        .expect("main commit");
    let sig = Signature::now("t", "t@example.com").expect("sig");
    let oid = repo
        .commit(
            None,
            &sig,
            &sig,
            "hand merge",
            &head.tree().expect("tree"),
            &[&tip, &head],
        )
        .expect("merge commit");
    move_main_to(&repo, oid);
    oid.to_string()
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

use std::collections::HashSet;

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
    pub found: HashMap<String, u32>,
    pub on_create: Option<Arc<dyn Fn() + Send + Sync>>,
    pub issues: Vec<factory_engine::IssueInfo>,
    pub actors: HashMap<(u32, String), factory_engine::LabelEvent>,
    pub user: String,
    pub label_edits: Vec<(u32, Vec<String>, Vec<String>)>,
}

/// A tracker that records its calls. Clones share one state, so a test keeps a handle after the driver owns the box.
#[derive(Clone)]
pub struct FakeTracker {
    pub state: Arc<Mutex<FakeState>>,
}

impl FakeTracker {
    pub fn new(repo: &str) -> FakeTracker {
        let state = FakeState {
            repo: repo.to_string(),
            next: 200,
            ..FakeState::default()
        };
        FakeTracker {
            state: Arc::new(Mutex::new(state)),
        }
    }
    pub fn with_labels(self, issue: u32, labels: &[&str]) -> FakeTracker {
        self.state
            .lock()
            .expect("lock")
            .labels
            .insert(issue, labels.iter().map(|s| s.to_string()).collect());
        self
    }
    pub fn with_found(self, title: &str, number: u32) -> FakeTracker {
        self.state
            .lock()
            .expect("lock")
            .found
            .insert(title.to_string(), number);
        self
    }
    /// Runs `hook` just before a successful `create` returns.
    pub fn with_on_create(self, hook: impl Fn() + Send + Sync + 'static) -> FakeTracker {
        self.state.lock().expect("lock").on_create = Some(Arc::new(hook));
        self
    }
    /// The open issues `list_labelled` filters by label.
    pub fn with_issues(self, issues: Vec<factory_engine::IssueInfo>) -> FakeTracker {
        self.state.lock().expect("lock").issues = issues;
        self
    }
    /// Who `labelled_by` names for `label` on `issue`. The event is dated far in the future.
    pub fn with_label_actor(self, issue: u32, label: &str, login: &str) -> FakeTracker {
        self.state.lock().expect("lock").actors.insert(
            (issue, label.to_string()),
            factory_engine::LabelEvent {
                actor: login.to_string(),
                at: "9999-12-31T23:59:59Z".to_string(),
            },
        );
        self
    }
    pub fn with_user(self, login: &str) -> FakeTracker {
        self.state.lock().expect("lock").user = login.to_string();
        self
    }
    pub fn fail_on(&self, method: &str) {
        self.state
            .lock()
            .expect("lock")
            .fail
            .insert(method.to_string());
    }
    pub fn clear_failures(&self) {
        self.state.lock().expect("lock").fail.clear();
    }
    pub fn calls(&self) -> Vec<String> {
        self.state.lock().expect("lock").calls.clone()
    }
    pub fn count(&self, method: &str) -> usize {
        self.calls()
            .iter()
            .filter(|c| c.starts_with(method))
            .count()
    }
    fn enter(
        &self,
        call: String,
        method: &str,
    ) -> Result<std::sync::MutexGuard<'_, FakeState>, String> {
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
        Ok(self
            .enter(format!("labels_of {issue}"), "labels_of")?
            .labels
            .get(&issue)
            .cloned()
            .unwrap_or_default())
    }
    fn is_closed(&self, issue: u32) -> Result<bool, String> {
        Ok(self
            .enter(format!("is_closed {issue}"), "is_closed")?
            .closed
            .contains(&issue))
    }
    fn create(&self, title: &str, body: &str, labels: &[String]) -> Result<u32, String> {
        let mut s = self.enter(format!("create {title}"), "create")?;
        s.next += 1;
        let number = s.next;
        s.created
            .push((title.to_string(), body.to_string(), labels.to_vec()));
        let hook = s.on_create.clone();
        drop(s);
        if let Some(hook) = hook {
            hook();
        }
        Ok(number)
    }
    fn find_created(&self, title: &str, _footer: &str) -> Result<Option<u32>, String> {
        Ok(self
            .enter(format!("find_created {title}"), "find_created")?
            .found
            .get(title)
            .copied())
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
    fn list_labelled(&self, label: &str) -> Result<Vec<factory_engine::IssueInfo>, String> {
        let s = self.enter(format!("list_labelled {label}"), "list_labelled")?;
        Ok(s.issues
            .iter()
            .filter(|i| i.labels.iter().any(|l| l == label))
            .cloned()
            .collect())
    }
    fn labelled_by(
        &self,
        issue: u32,
        label: &str,
    ) -> Result<Option<factory_engine::LabelEvent>, String> {
        let s = self.enter(format!("labelled_by {issue} {label}"), "labelled_by")?;
        Ok(s.actors.get(&(issue, label.to_string())).cloned())
    }
    fn edit_labels(&self, issue: u32, add: &[String], remove: &[String]) -> Result<(), String> {
        let mut s = self.enter(format!("edit_labels {issue}"), "edit_labels")?;
        s.label_edits.push((issue, add.to_vec(), remove.to_vec()));
        Ok(())
    }
    fn current_user(&self) -> Result<String, String> {
        Ok(self
            .enter("current_user".into(), "current_user")?
            .user
            .clone())
    }
}

/// A tracker that panics in one method, or in every method when `panic_on` is `None`, and forwards the rest.
/// It wraps a `FakeTracker` so that a panic never poisons the fake's mutex.
pub struct PanicTracker {
    pub inner: FakeTracker,
    pub panic_on: Option<&'static str>,
}

impl PanicTracker {
    fn check(&self, method: &str) {
        if self.panic_on.is_none_or(|m| m == method) {
            panic!("tracker panic in {method}");
        }
    }
}

impl factory_engine::Tracker for PanicTracker {
    fn repo_name(&self) -> Result<String, String> {
        self.check("repo_name");
        self.inner.repo_name()
    }
    fn labels_of(&self, issue: u32) -> Result<Vec<String>, String> {
        self.check("labels_of");
        self.inner.labels_of(issue)
    }
    fn is_closed(&self, issue: u32) -> Result<bool, String> {
        self.check("is_closed");
        self.inner.is_closed(issue)
    }
    fn create(&self, title: &str, body: &str, labels: &[String]) -> Result<u32, String> {
        self.check("create");
        self.inner.create(title, body, labels)
    }
    fn find_created(&self, title: &str, footer: &str) -> Result<Option<u32>, String> {
        self.check("find_created");
        self.inner.find_created(title, footer)
    }
    fn comment(&self, issue: u32, body: &str) -> Result<(), String> {
        self.check("comment");
        self.inner.comment(issue, body)
    }
    fn close(&self, issue: u32) -> Result<(), String> {
        self.check("close");
        self.inner.close(issue)
    }
    fn list_labelled(&self, label: &str) -> Result<Vec<factory_engine::IssueInfo>, String> {
        self.check("list_labelled");
        self.inner.list_labelled(label)
    }
    fn labelled_by(
        &self,
        issue: u32,
        label: &str,
    ) -> Result<Option<factory_engine::LabelEvent>, String> {
        self.check("labelled_by");
        self.inner.labelled_by(issue, label)
    }
    fn edit_labels(&self, issue: u32, add: &[String], remove: &[String]) -> Result<(), String> {
        self.check("edit_labels");
        self.inner.edit_labels(issue, add, remove)
    }
    fn current_user(&self) -> Result<String, String> {
        self.check("current_user");
        self.inner.current_user()
    }
}

/// Gives the fixture's driver a boxed tracker, keeping everything else.
pub fn with_boxed_tracker(
    fx: MergeFixture,
    tracker: Box<dyn factory_engine::Tracker>,
) -> MergeFixture {
    let MergeFixture {
        tmp,
        main,
        driver,
        runner,
        id,
        head,
    } = fx;
    MergeFixture {
        tmp,
        main,
        driver: driver.with_tracker(tracker),
        runner,
        id,
        head,
    }
}

/// Gives the fixture's driver a tracker, keeping everything else.
pub fn with_tracker(fx: MergeFixture, tracker: FakeTracker) -> MergeFixture {
    let MergeFixture {
        tmp,
        main,
        driver,
        runner,
        id,
        head,
    } = fx;
    MergeFixture {
        tmp,
        main,
        driver: driver.with_tracker(Box::new(tracker)),
        runner,
        id,
        head,
    }
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
