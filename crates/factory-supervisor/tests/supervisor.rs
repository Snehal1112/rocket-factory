//! The supervisor against a shell script that stands in for `factory`.
//!
//! CI must not run these as root: the read-only folder tests skip themselves for root and then prove nothing.
#![cfg(unix)]

use std::cell::Cell;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use factory_core::{project, Home};
use factory_engine::{acquire_project_lease, release_project_lease};
use factory_supervisor::request::{Action, Request};
use factory_supervisor::{
    pid_alive, queue, records, Outcome, ProcessSpawner, QueueItem, Supervisor, TickReport,
};

const WAIT: Duration = Duration::from_secs(2);
const TIMEOUT: Duration = Duration::from_secs(20);

// The script reads a behavior file per project and job. A `hold` behavior runs until its gate file is deleted.
const SCRIPT: &str = r#"#!/bin/sh
d=$(dirname "$0")
echo x >> "$d/runs-$2-$4"
b=$(cat "$d/behavior-$2-$4")
case "$b" in
  merged*|needs-human*|parked*) echo "$b" ;;
  fail) echo "boom" >&2; exit 1 ;;
  hold)
    while [ -f "$d/gate-$2-$4" ]; do sleep 0.02; done
    echo merged ;;
esac
"#;

struct Env {
    _tmp: tempfile::TempDir,
    home: Home,
    dir: PathBuf,
    ts: Cell<u64>,
}

fn env() -> Env {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().join("scripts");
    std::fs::create_dir_all(&dir).expect("scripts dir");
    let bin = dir.join("factory");
    std::fs::write(&bin, SCRIPT).expect("script");
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let home = Home::at(tmp.path().join("home"));
    Env {
        _tmp: tmp,
        home,
        dir,
        ts: Cell::new(1),
    }
}

impl Env {
    fn supervisor(&self, max_parallel: u32) -> Supervisor {
        let mut s = Supervisor::new(
            self.home.clone(),
            Box::new(ProcessSpawner::new(self.dir.join("factory"))),
            Box::new(|| 1000),
        );
        s.set_max_parallel(max_parallel);
        s
    }

    fn add_project(&self, name: &str) -> PathBuf {
        let repo = self._tmp.path().join("repos").join(name);
        std::fs::create_dir_all(&repo).expect("repo");
        project::add(&self.home, name, &repo, "rust", None, &|_| Ok(()), None).expect("add project");
        repo
    }

    fn enqueue(&self, project: &str, job: &str, behavior: &str) -> String {
        self.enqueue_as(project, job, behavior, Action::Run)
    }

    fn runs(&self, project: &str, job: &str) -> usize {
        std::fs::read_to_string(self.dir.join(format!("runs-{project}-{job}")))
            .map(|t| t.lines().count())
            .unwrap_or(0)
    }

    fn enqueue_as(&self, project: &str, job: &str, behavior: &str, action: Action) -> String {
        std::fs::write(self.dir.join(format!("behavior-{project}-{job}")), behavior)
            .expect("behavior");
        if behavior == "hold" {
            self.hold(project, job);
        }
        let ts = self.ts.get();
        self.ts.set(ts + 1);
        let req = Request::new(project, action, job, false, ts).expect("request");
        queue::enqueue(&self.home, &req).expect("enqueue");
        req.id
    }

    fn hold(&self, project: &str, job: &str) {
        std::fs::write(self.dir.join(format!("gate-{project}-{job}")), "").expect("gate");
    }

    fn release(&self, project: &str, job: &str) {
        std::fs::remove_file(self.dir.join(format!("gate-{project}-{job}"))).expect("remove gate");
    }

    fn result(&self, id: &str) -> Outcome {
        records::read_results(&self.home)
            .into_iter()
            .flatten()
            .find(|r| r.request_id == id)
            .expect("result")
            .outcome
    }

    fn queue_ids(&self) -> Vec<String> {
        queue::list(&self.home)
            .into_iter()
            .filter_map(|i| match i {
                QueueItem::Ok(r) => Some(r.id),
                QueueItem::Bad { .. } => None,
            })
            .collect()
    }

    fn children(&self) -> Vec<records::ChildRecord> {
        records::read_children(&self.home)
            .into_iter()
            .flatten()
            .collect()
    }

    fn state_dir(&self, name: &str) -> PathBuf {
        let file = project::get(&self.home, name).expect("project");
        project::layout(&self.home, name, &file)
            .expect("layout")
            .state_dir
    }
}

/// Ticks until the request id has ended, and returns every report on the way.
fn tick_until_finished(s: &mut Supervisor, id: &str) -> Vec<TickReport> {
    let end = Instant::now() + TIMEOUT;
    let mut reports = Vec::new();
    loop {
        let r = s.tick().expect("tick");
        let done = r.finished.iter().any(|(i, _)| i == id);
        reports.push(r);
        if done {
            return reports;
        }
        assert!(Instant::now() < end, "request {id} did not finish");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn first_tick_with_start(s: &mut Supervisor, id: &str) -> TickReport {
    let end = Instant::now() + TIMEOUT;
    loop {
        let r = s.tick().expect("tick");
        if r.started.iter().any(|i| i == id) {
            return r;
        }
        assert!(Instant::now() < end, "request {id} did not start");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_request_runs_and_merges() {
    let e = env();
    e.add_project("p");
    let id = e.enqueue("p", "1", "merged");
    let mut s = e.supervisor(2);
    let first = s.tick().expect("tick");
    assert_eq!(first.started, vec![id.clone()]);
    tick_until_finished(&mut s, &id);
    assert_eq!(e.result(&id), Outcome::Merged);
    assert!(e.queue_ids().is_empty());
    assert!(e.children().is_empty());
}

#[test]
fn needs_human_and_failure_are_classified() {
    let e = env();
    e.add_project("a");
    e.add_project("b");
    let a = e.enqueue("a", "1", "needs-human: risky");
    let b = e.enqueue("b", "1", "fail");
    let mut s = e.supervisor(2);
    let end = Instant::now() + TIMEOUT;
    while !e.queue_ids().is_empty() {
        s.tick().expect("tick");
        assert!(Instant::now() < end, "requests did not finish");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        e.result(&a),
        Outcome::NeedsHuman {
            reason: "risky".into()
        }
    );
    assert_eq!(
        e.result(&b),
        Outcome::Failed {
            code: Some(1),
            stderr_tail: "boom".into()
        }
    );
}

#[test]
fn the_parallel_limit_holds_and_a_project_never_runs_twice() {
    let e = env();
    for p in ["a", "b", "c"] {
        e.add_project(p);
    }
    let ids: Vec<String> = ["a", "b", "c"]
        .iter()
        .map(|p| e.enqueue(p, "1", "hold"))
        .collect();
    let mut s = e.supervisor(2);
    let r = s.tick().expect("tick");
    assert_eq!(r.started, vec![ids[0].clone(), ids[1].clone()]);
    for _ in 0..3 {
        assert!(s.tick().expect("tick").started.is_empty());
    }
    e.release("a", "1");
    let reports = tick_until_finished(&mut s, &ids[0]);
    // The freed slot is used in the same tick that saw the child end.
    assert!(reports.last().expect("report").started.contains(&ids[2]));
    for (p, id) in [("b", &ids[1]), ("c", &ids[2])] {
        e.release(p, "1");
        tick_until_finished(&mut s, id);
    }

    // Two requests of one project run one after the other, even with free slots.
    e.add_project("d");
    let first = e.enqueue("d", "1", "hold");
    let second = e.enqueue("d", "2", "hold");
    let mut s = e.supervisor(8);
    assert_eq!(s.tick().expect("tick").started, vec![first.clone()]);
    for _ in 0..3 {
        assert!(s.tick().expect("tick").started.is_empty());
        assert_eq!(e.children().len(), 1);
    }
    e.release("d", "1");
    let reports = tick_until_finished(&mut s, &first);
    assert!(reports.last().expect("report").started.contains(&second));
    e.release("d", "2");
    tick_until_finished(&mut s, &second);
}

#[test]
fn a_project_with_a_missing_repo_is_rejected_and_others_start() {
    let e = env();
    let gone = e.add_project("gone");
    e.add_project("ok");
    std::fs::remove_dir_all(&gone).expect("remove repo");
    let bad = e.enqueue("gone", "1", "merged");
    let good = e.enqueue("ok", "1", "merged");
    let mut s = e.supervisor(2);
    let r = s.tick().expect("tick");
    assert_eq!(r.started, vec![good.clone()]);
    assert_eq!(r.rejected.len(), 1);
    assert_eq!(r.rejected[0].0, bad);
    assert!(matches!(e.result(&bad), Outcome::Rejected { .. }));
    assert!(!e.queue_ids().contains(&bad));
    tick_until_finished(&mut s, &good);
}

#[test]
fn a_broken_project_file_is_rejected() {
    let e = env();
    e.add_project("broken");
    std::fs::write(
        e.home.project_dir("broken").join("project.toml"),
        "not = [valid",
    )
    .expect("break");
    let id = e.enqueue("broken", "1", "merged");
    let mut s = e.supervisor(2);
    let r = s.tick().expect("tick");
    assert_eq!(r.rejected.len(), 1);
    assert!(matches!(e.result(&id), Outcome::Rejected { .. }));
}

#[test]
fn a_bad_request_file_is_skipped_and_reported_by_list() {
    let e = env();
    e.add_project("p");
    std::fs::create_dir_all(e.home.queue_dir()).expect("queue dir");
    std::fs::write(e.home.queue_dir().join("garbage.json"), "{nope").expect("garbage");
    let id = e.enqueue("p", "1", "merged");
    let mut s = e.supervisor(2);
    assert_eq!(s.tick().expect("tick").started, vec![id.clone()]);
    tick_until_finished(&mut s, &id);
    assert!(queue::list(&e.home)
        .iter()
        .any(|i| matches!(i, QueueItem::Bad { file, .. } if file == "garbage.json")));
}

#[test]
fn a_human_lease_makes_the_request_wait() {
    let e = env();
    e.add_project("p");
    let state = e.state_dir("p");
    std::fs::create_dir_all(&state).expect("state dir");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time")
        .as_secs_f64();
    let owner = format!("driver-{}-1", std::process::id());
    acquire_project_lease(&state, "001", &owner, 100_000.0, now, WAIT).expect("lease");
    let id = e.enqueue("p", "1", "merged");
    let mut s = e.supervisor(2);
    for _ in 0..3 {
        let r = s.tick().expect("tick");
        assert!(r.started.is_empty() && r.rejected.is_empty());
    }
    assert_eq!(e.queue_ids(), vec![id.clone()]);
    release_project_lease(&state, &owner, WAIT).expect("release");
    first_tick_with_start(&mut s, &id);
    tick_until_finished(&mut s, &id);
    assert_eq!(e.result(&id), Outcome::Merged);
}

#[test]
fn the_loop_ends_without_killing_a_running_child() {
    let e = env();
    e.add_project("p");
    let id = e.enqueue("p", "1", "hold");
    let mut s = e.supervisor(2);
    let calls = Cell::new(0u32);
    s.run_loop(Duration::from_millis(5), &|| {
        calls.set(calls.get() + 1);
        calls.get() >= 3
    });
    let children = e.children();
    assert_eq!(children.len(), 1);
    assert!(pid_alive(children[0].pid));
    e.release("p", "1");
    tick_until_finished(&mut s, &id);
    assert_eq!(e.result(&id), Outcome::Merged);
}

#[test]
fn a_spawn_failure_is_a_failed_result_and_the_loop_continues() {
    let e = env();
    e.add_project("a");
    e.add_project("b");
    let a = e.enqueue("a", "1", "merged");
    let b = e.enqueue("b", "1", "merged");
    let missing: &Path = &e.dir.join("no-such-factory");
    let mut s = Supervisor::new(
        e.home.clone(),
        Box::new(ProcessSpawner::new(missing.to_path_buf())),
        Box::new(|| 1000),
    );
    let r = s.tick().expect("tick");
    assert!(r.started.is_empty());
    assert_eq!(r.finished.len(), 2);
    for id in [&a, &b] {
        match e.result(id) {
            Outcome::Failed {
                code: None,
                stderr_tail,
            } => assert!(stderr_tail.contains("no-such-factory"), "{stderr_tail}"),
            other => panic!("unexpected {other:?}"),
        }
    }
    assert!(e.queue_ids().is_empty());
}

#[test]
fn approve_output_shapes_are_recognised() {
    let e = env();
    e.add_project("a");
    e.add_project("b");
    let a = e.enqueue_as("a", "1", "merged abc1234", Action::Approve);
    let b = e.enqueue_as("b", "1", "parked: x", Action::Approve);
    let mut s = e.supervisor(2);
    wait_queue_empty(&mut s, &e);
    assert_eq!(e.result(&a), Outcome::Merged);
    assert_eq!(e.result(&b), Outcome::NeedsHuman { reason: "x".into() });
}

fn wait_queue_empty(s: &mut Supervisor, e: &Env) {
    let end = Instant::now() + TIMEOUT;
    while !e.queue_ids().is_empty() {
        s.tick().expect("tick");
        assert!(Instant::now() < end, "requests did not finish");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_request_with_a_result_is_dropped_and_not_started() {
    let e = env();
    e.add_project("p");
    let id = e.enqueue("p", "1", "merged");
    let rec = records::ResultRecord {
        request_id: id.clone(),
        project: "p".into(),
        job: "1".into(),
        outcome: Outcome::Merged,
        finished_ms: 5,
    };
    records::write_result(&e.home, &rec).expect("result");
    let mut s = e.supervisor(2);
    let r = s.tick().expect("tick");
    assert!(r.started.is_empty());
    assert!(e.queue_ids().is_empty());
    assert_eq!(e.runs("p", "1"), 0);
}

#[test]
fn a_corrupt_child_record_blocks_only_its_project() {
    let e = env();
    e.add_project("a");
    e.add_project("b");
    std::fs::create_dir_all(e.home.children_dir()).expect("children dir");
    std::fs::write(e.home.children_dir().join("a.json"), "{broken").expect("corrupt");
    let a = e.enqueue("a", "1", "merged");
    let b = e.enqueue("b", "1", "merged");
    let mut s = e.supervisor(4);
    for _ in 0..3 {
        let r = s.tick().expect("tick");
        assert_eq!(r.bad_children.len(), 1);
        assert!(!r.started.contains(&a));
    }
    assert!(e.queue_ids().contains(&a));
    assert_eq!(e.runs("a", "1"), 0);
    let end = Instant::now() + TIMEOUT;
    while !e.home.results_dir().join(format!("{b}.json")).exists() {
        s.tick().expect("tick");
        assert!(Instant::now() < end, "b did not finish");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(e.result(&b), Outcome::Merged);
}

/// Makes a folder read-only and restores it on drop, also when a test panics.
struct ReadOnly(PathBuf);

impl ReadOnly {
    fn new(p: &Path) -> ReadOnly {
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o555)).expect("chmod");
        ReadOnly(p.to_path_buf())
    }
}

impl Drop for ReadOnly {
    fn drop(&mut self) {
        let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
    }
}

/// True, with a loud message, when the user ignores folder permissions (root).
fn permissions_ignored(dir: &Path) -> bool {
    let probe = dir.join(".probe");
    if std::fs::write(&probe, "").is_ok() {
        let _ = std::fs::remove_file(&probe);
        eprintln!("SKIPPED (running as root): folder permissions are not enforced for this user");
        return true;
    }
    false
}

#[test]
fn a_failing_queue_removal_never_restarts_the_job() {
    let e = env();
    e.add_project("p");
    let id = e.enqueue("p", "1", "merged");
    let mut s = e.supervisor(2);
    assert_eq!(s.tick().expect("tick").started, vec![id.clone()]);
    {
        let _guard = ReadOnly::new(&e.home.queue_dir());
        if permissions_ignored(&e.home.queue_dir()) {
            return;
        }
        let end = Instant::now() + Duration::from_secs(3);
        let mut saw_error = false;
        while Instant::now() < end {
            let r = s.tick().expect("tick");
            saw_error |= !r.errors.is_empty();
            assert!(r.started.is_empty());
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(saw_error, "the removal failure was never reported");
        assert_eq!(e.runs("p", "1"), 1);
    }
    // Once the folder is writable again, the job ends and the queue entry goes.
    tick_until_finished(&mut s, &id);
    assert_eq!(e.runs("p", "1"), 1);
    assert!(e.queue_ids().is_empty());
    assert_eq!(e.result(&id), Outcome::Merged);
}

/// Runs one tick of a fresh supervisor on a thread and fails when it does not return in time.
fn tick_within(home: Home, bin: PathBuf, secs: u64) -> TickReport {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut s = Supervisor::new(home, Box::new(ProcessSpawner::new(bin)), Box::new(|| 1000));
        let _ = tx.send(s.tick().expect("tick"));
    });
    rx.recv_timeout(Duration::from_secs(secs))
        .expect("tick did not return in time")
}

#[test]
fn a_fifo_or_symlink_as_a_log_path_never_blocks() {
    for kind in ["fifo", "symlink"] {
        let e = env();
        e.add_project("p");
        let id = e.enqueue("p", "1", "merged");
        let mut s = e.supervisor(2);
        s.tick().expect("tick");
        let child = e.children().remove(0);
        // Wait for the script to end, then replace its out file.
        let end = Instant::now() + TIMEOUT;
        while pid_alive(child.pid) && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(10));
        }
        std::fs::remove_file(&child.out).expect("remove out");
        if kind == "fifo" {
            let made = std::process::Command::new("mkfifo")
                .arg(&child.out)
                .status();
            if !made.map(|st| st.success()).unwrap_or(false) {
                eprintln!("skipped: mkfifo is not available");
                continue;
            }
        } else {
            std::os::unix::fs::symlink(e.dir.join("behavior-p-1"), &child.out).expect("symlink");
        }
        let r = tick_within(e.home.clone(), e.dir.join("factory"), 10);
        assert_eq!(r.finished.len(), 1, "{kind}");
        match e.result(&id) {
            Outcome::Failed { stderr_tail, .. } => assert!(
                stderr_tail.contains("log not read"),
                "{kind}: {stderr_tail}"
            ),
            Outcome::Lost { reason } => {
                assert!(reason.contains("log not read"), "{kind}: {reason}")
            }
            other => panic!("{kind}: unexpected {other:?}"),
        }
    }
}

#[test]
fn the_loop_never_ticks_faster_than_the_floor() {
    let e = env();
    let mut s = e.supervisor(2);
    let calls = Cell::new(0u32);
    let start = Instant::now();
    s.run_loop(Duration::ZERO, &|| {
        calls.set(calls.get() + 1);
        calls.get() >= 3
    });
    assert!(start.elapsed() >= factory_supervisor::supervisor::MIN_INTERVAL);
}

#[test]
fn spawn_ok_with_children_and_queue_both_failing_starts_exactly_once() {
    let e = env();
    e.add_project("p");
    let id = e.enqueue("p", "1", "merged");
    std::fs::create_dir_all(e.home.children_dir()).expect("children dir");
    let _children = ReadOnly::new(&e.home.children_dir());
    let _queue = ReadOnly::new(&e.home.queue_dir());
    if permissions_ignored(&e.home.queue_dir()) {
        return;
    }
    let mut s = e.supervisor(2);
    let first = s.tick().expect("tick");
    assert!(first.started.is_empty());
    assert!(!first.errors.is_empty());
    // Many ticks, long enough for the child to end and the project to look free.
    let end = Instant::now() + Duration::from_secs(2);
    while Instant::now() < end {
        let r = s.tick().expect("tick");
        assert!(r.started.is_empty());
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(e.runs("p", "1"), 1);
    assert!(matches!(e.result(&id), Outcome::Failed { .. }));
}

/// A spawner that always reports a started pid and counts its calls.
struct CountingSpawner(std::rc::Rc<Cell<u32>>);

impl factory_supervisor::Spawner for CountingSpawner {
    fn spawn(
        &self,
        _home: &Home,
        _req: &Request,
        _out: &Path,
        _err: &Path,
    ) -> Result<u32, factory_supervisor::SpawnError> {
        self.0.set(self.0.get() + 1);
        Ok(std::process::id())
    }
}

#[test]
fn the_in_memory_guard_blocks_a_second_start_when_nothing_can_be_written() {
    let e = env();
    e.add_project("p");
    let id = e.enqueue("p", "1", "merged");
    // A file where a folder is needed makes the child record and the result both unwritable, even for root.
    std::fs::create_dir_all(e.home.run_dir()).expect("run dir");
    std::fs::write(e.home.children_dir(), "").expect("block children");
    std::fs::write(e.home.results_dir(), "").expect("block results");
    let count = std::rc::Rc::new(Cell::new(0));
    let mut s = Supervisor::new(
        e.home.clone(),
        Box::new(CountingSpawner(count.clone())),
        Box::new(|| 1000),
    );
    let first = s.tick().expect("tick");
    assert!(
        first.errors.iter().any(|m| m.contains("blocked in memory")),
        "{:?}",
        first.errors
    );
    for _ in 0..5 {
        let r = s.tick().expect("tick");
        assert!(r.started.is_empty());
        assert!(r.errors.iter().any(|m| m.contains(&id)));
    }
    assert_eq!(count.get(), 1);
    assert_eq!(e.queue_ids(), vec![id]);
}

#[test]
fn a_failing_decision_does_not_stop_the_others() {
    let e = env();
    e.add_project("a");
    e.add_project("b");
    // The first request fails to spawn, and its result cannot be written because the folder is read-only.
    let a = e.enqueue("a", "1", "merged");
    let b = e.enqueue("b", "1", "merged");
    std::fs::create_dir_all(e.home.results_dir()).expect("results dir");
    let _results = ReadOnly::new(&e.home.results_dir());
    if permissions_ignored(&e.home.results_dir()) {
        return;
    }
    struct FailFirst(PathBuf);
    impl factory_supervisor::Spawner for FailFirst {
        fn spawn(
            &self,
            home: &Home,
            req: &Request,
            out: &Path,
            err: &Path,
        ) -> Result<u32, factory_supervisor::SpawnError> {
            if req.project == "a" {
                return Err(factory_supervisor::SpawnError::Missing(PathBuf::from(
                    "a-fails",
                )));
            }
            ProcessSpawner::new(self.0.clone()).spawn(home, req, out, err)
        }
    }
    let mut s = Supervisor::new(
        e.home.clone(),
        Box::new(FailFirst(e.dir.join("factory"))),
        Box::new(|| 1000),
    );
    let r = s.tick().expect("tick");
    assert!(r.errors.iter().any(|m| m.contains(&a)), "{:?}", r.errors);
    assert_eq!(r.started, vec![b]);
}
