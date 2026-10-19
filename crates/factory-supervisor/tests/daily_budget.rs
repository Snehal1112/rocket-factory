//! The daily budget hold, with a fake spawner and an injected clock.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;

use factory_core::{project, Home};
use factory_supervisor::request::{Action, Request};
use factory_supervisor::{queue, ChildPoll, QueueItem, SpawnError, Spawner, Supervisor};

const DAY: u64 = 19_675;
const DAY_MS: u64 = 86_400_000;
const NOON: u64 = DAY * DAY_MS + 12 * 3_600_000;

#[derive(Clone, Default)]
struct Fake {
    spawned: Rc<RefCell<Vec<(String, Action)>>>,
    polls: Rc<Cell<u32>>,
}

impl Spawner for Fake {
    fn spawn(&self, _: &Home, req: &Request, _: &Path, _: &Path) -> Result<u32, SpawnError> {
        self.spawned
            .borrow_mut()
            .push((req.project.clone(), req.action));
        Ok(std::process::id())
    }

    fn poll(&self, _: u32) -> ChildPoll {
        self.polls.set(self.polls.get() + 1);
        ChildPoll::Running
    }
}

struct Env {
    _t: tempfile::TempDir,
    home: Home,
    clock: Rc<Cell<u64>>,
    fake: Fake,
}

fn env(limit: Option<u64>) -> Env {
    let t = tempfile::tempdir().expect("tempdir");
    let home = Home::at(t.path().join("home"));
    for n in ["a", "b"] {
        let repo = t.path().join("repos").join(n);
        std::fs::create_dir_all(&repo).expect("repo");
        project::add(&home, n, &repo, "rust", None, &|_| Ok(())).expect("add");
    }
    let e = Env {
        _t: t,
        home,
        clock: Rc::new(Cell::new(NOON)),
        fake: Fake::default(),
    };
    e.set_limit(limit);
    e
}

impl Env {
    fn supervisor(&self) -> Supervisor {
        let clock = self.clock.clone();
        Supervisor::new(
            self.home.clone(),
            Box::new(self.fake.clone()),
            Box::new(move || clock.get()),
        )
    }

    fn set_limit(&self, limit: Option<u64>) {
        let text = limit
            .map(|l| format!("daily_budget_weighted = {l}\n"))
            .unwrap_or_default();
        std::fs::write(self.home.config_file(), text).expect("config");
    }

    fn use_tokens(&self, name: &str, tokens: u64, day: u64) {
        let file = project::get(&self.home, name).expect("project");
        let dir = project::layout(&self.home, name, &file)
            .expect("layout")
            .jobs_dir
            .join("001");
        std::fs::create_dir_all(&dir).expect("job dir");
        let line = format!("{{\"input\":{tokens},\"ts\":{}}}\n", day * 86_400 + 5);
        let path = dir.join("usage.jsonl");
        let old = std::fs::read_to_string(&path).unwrap_or_default();
        std::fs::write(path, old + &line).expect("usage");
    }

    fn enqueue(&self, project: &str, action: Action, ts: u64) -> String {
        let req = Request::new(project, action, "1", false, ts).expect("request");
        queue::enqueue(&self.home, &req).expect("enqueue");
        req.id
    }

    fn queue_files(&self) -> Vec<(String, String)> {
        queue::list(&self.home)
            .into_iter()
            .filter_map(|i| match i {
                QueueItem::Ok(r) => {
                    let text = std::fs::read_to_string(
                        self.home.queue_dir().join(format!("{}.json", r.id)),
                    )
                    .expect("read");
                    Some((r.id, text))
                }
                QueueItem::Bad { .. } => None,
            })
            .collect()
    }

    fn spawned(&self) -> Vec<(String, Action)> {
        self.fake.spawned.borrow().clone()
    }
}

#[test]
fn one_token_under_the_limit_starts_the_run() {
    let e = env(Some(100));
    e.use_tokens("a", 99, DAY);
    let id = e.enqueue("a", Action::Run, 1);
    let r = e.supervisor().tick().expect("tick");
    assert_eq!(r.started, vec![id]);
}

#[test]
fn exactly_at_the_limit_holds_the_run_and_the_file_stays_unchanged() {
    let e = env(Some(100));
    e.use_tokens("a", 60, DAY);
    e.use_tokens("b", 40, DAY);
    e.enqueue("a", Action::Run, 1);
    let before = e.queue_files();
    let mut s = e.supervisor();
    for _ in 0..3 {
        let r = s.tick().expect("tick");
        assert!(r.started.is_empty() && r.rejected.is_empty() && r.finished.is_empty());
    }
    assert_eq!(e.queue_files(), before);
    assert!(e.spawned().is_empty());
}

#[test]
fn a_notice_is_logged_when_the_state_flips_not_every_tick() {
    let e = env(Some(100));
    e.use_tokens("a", 100, DAY);
    e.enqueue("a", Action::Run, 1);
    let mut s = e.supervisor();
    assert_eq!(s.tick().expect("tick").notices.len(), 1);
    assert!(s.tick().expect("tick").notices.is_empty());
    e.clock.set((DAY + 1) * DAY_MS);
    assert_eq!(s.tick().expect("tick").notices.len(), 1);
    assert!(s.tick().expect("tick").notices.is_empty());
}

#[test]
fn raising_the_budget_in_config_starts_the_held_run_on_the_next_tick() {
    let e = env(Some(100));
    e.use_tokens("a", 100, DAY);
    let id = e.enqueue("a", Action::Run, 1);
    let mut s = e.supervisor();
    assert!(s.tick().expect("tick").started.is_empty());
    e.set_limit(Some(101));
    assert_eq!(s.tick().expect("tick").started, vec![id]);
}

#[test]
fn removing_the_budget_starts_the_held_run() {
    let e = env(Some(100));
    e.use_tokens("a", 100, DAY);
    let id = e.enqueue("a", Action::Run, 1);
    let mut s = e.supervisor();
    assert!(s.tick().expect("tick").started.is_empty());
    e.set_limit(None);
    assert_eq!(s.tick().expect("tick").started, vec![id]);
}

#[test]
fn moving_past_utc_midnight_starts_the_held_run() {
    let e = env(Some(100));
    e.use_tokens("a", 100, DAY);
    let id = e.enqueue("a", Action::Run, 1);
    let mut s = e.supervisor();
    assert!(s.tick().expect("tick").started.is_empty());
    e.clock.set((DAY + 1) * DAY_MS);
    assert_eq!(s.tick().expect("tick").started, vec![id]);
}

#[test]
fn a_running_child_is_never_touched_when_the_budget_is_crossed() {
    let e = env(Some(100));
    let first = e.enqueue("a", Action::Run, 1);
    let mut s = e.supervisor();
    assert_eq!(s.tick().expect("tick").started, vec![first]);
    e.use_tokens("a", 500, DAY);
    e.enqueue("b", Action::Run, 2);
    let r = s.tick().expect("tick");
    assert!(r.started.is_empty() && r.finished.is_empty());
    assert_eq!(e.spawned(), vec![("a".to_string(), Action::Run)]);
    assert!(e.fake.polls.get() >= 1);
}

#[test]
fn an_approve_starts_while_the_budget_is_exhausted_behind_held_runs() {
    let e = env(Some(100));
    e.use_tokens("a", 100, DAY);
    e.enqueue("a", Action::Run, 1);
    e.enqueue("b", Action::Run, 2);
    let approve = e.enqueue("b", Action::Approve, 3);
    let r = e.supervisor().tick().expect("tick");
    assert_eq!(r.started, vec![approve]);
    assert_eq!(e.spawned(), vec![("b".to_string(), Action::Approve)]);
    assert_eq!(e.queue_files().len(), 3);
}

#[test]
fn an_invalid_config_mid_run_keeps_the_last_valid_budget() {
    let e = env(Some(100));
    e.use_tokens("a", 100, DAY);
    e.enqueue("a", Action::Run, 1);
    let mut s = e.supervisor();
    assert!(s.tick().expect("tick").errors.is_empty());
    std::fs::write(e.home.config_file(), "daily_budget_weighted = 0\n").expect("config");
    let r = s.tick().expect("tick");
    assert!(r.started.is_empty(), "the old limit still holds the run");
    assert_eq!(r.errors.len(), 1);
}
