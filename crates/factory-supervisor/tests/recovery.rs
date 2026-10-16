//! Recovery after the supervisor stopped, with shell scripts as children.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use factory_core::{project, Home};
use factory_supervisor::request::{Action, Request};
use factory_supervisor::{pid_alive, queue, records, ChildRecord, Outcome, ProcessSpawner, QueueItem, Supervisor};

const TIMEOUT: Duration = Duration::from_secs(20);

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

struct Env {
    _tmp: tempfile::TempDir,
    home: Home,
    dir: PathBuf,
}

fn env() -> Env {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().join("scripts");
    std::fs::create_dir_all(&dir).expect("scripts dir");
    Env { home: Home::at(tmp.path().join("home")), dir, _tmp: tmp }
}

impl Env {
    fn supervisor(&self) -> Supervisor {
        let bin = self.dir.join("factory");
        std::fs::write(&bin, "#!/bin/sh\nexit 0\n").expect("script");
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let mut s = Supervisor::new(self.home.clone(), Box::new(ProcessSpawner::new(bin)), Box::new(now_ms));
        s.set_max_parallel(4);
        s
    }

    fn add_project(&self, name: &str) {
        let repo = self._tmp.path().join("repos").join(name);
        std::fs::create_dir_all(&repo).expect("repo");
        project::add(&self.home, name, &repo, "rust", None, &|_| Ok(())).expect("add project");
    }

    fn enqueue(&self, project: &str, job: &str) -> Request {
        let req = Request::new(project, Action::Run, job, false, now_ms()).expect("request");
        queue::enqueue(&self.home, &req).expect("enqueue");
        req
    }

    fn result(&self, id: &str) -> Option<Outcome> {
        records::read_results(&self.home).into_iter().flatten().find(|r| r.request_id == id).map(|r| r.outcome)
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

    /// Writes a child record for a request, with log files, and returns the record.
    fn child_record(&self, req: &Request, pid: u32, started_ms: u64, out: &str) -> ChildRecord {
        let logs = self.home.logs_dir(&req.project);
        std::fs::create_dir_all(&logs).expect("logs dir");
        let (o, e) = (logs.join(format!("{}.out", req.id)), logs.join(format!("{}.err", req.id)));
        std::fs::write(&o, out).expect("out");
        std::fs::write(&e, "").expect("err");
        let rec = ChildRecord { pid, project: req.project.clone(), job: req.job.clone(), request_id: req.id.clone(), started_ms, out: o, err: e };
        records::write_child(&self.home, &rec).expect("child record");
        rec
    }
}

fn sleeper() -> Child {
    Command::new("sleep").arg("60").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().expect("sleep")
}

fn wait_dead(pid: u32) {
    let end = Instant::now() + TIMEOUT;
    while pid_alive(pid) {
        assert!(Instant::now() < end, "pid {pid} stayed alive");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_recorded_pid_that_started_far_from_the_record_is_treated_as_gone() {
    for offset in [3_600_000i64, -3_600_000] {
        let e = env();
        e.add_project("p");
        let mut child = sleeper();
        let req = e.enqueue("p", "1");
        let started = (now_ms() as i64 + offset) as u64;
        e.child_record(&req, child.id(), started, "");
        let mut s = e.supervisor();
        let report = s.tick().expect("tick");
        assert_eq!(report.finished.len(), 1, "{report:?}");
        assert!(matches!(e.result(&req.id), Some(Outcome::Lost { .. })), "{:?}", e.result(&req.id));
        assert!(e.queue_ids().is_empty());
        assert!(records::read_children(&e.home).is_empty());
        // The stranger is left alone.
        assert!(pid_alive(child.id()));
        child.kill().expect("kill");
        child.wait().expect("wait");
    }
}

#[test]
fn a_recorded_pid_that_started_when_the_record_says_is_adopted() {
    let e = env();
    e.add_project("p");
    let mut child = sleeper();
    let req = e.enqueue("p", "1");
    e.child_record(&req, child.id(), now_ms(), "");
    let mut s = e.supervisor();
    let report = s.tick().expect("tick");
    assert!(report.finished.is_empty(), "{report:?}");
    assert_eq!(e.queue_ids(), vec![req.id.clone()]);
    child.kill().expect("kill");
    child.wait().expect("wait");
    wait_dead(child.id());
}
