//! Recovery after the supervisor stopped, with shell scripts as children.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use factory_core::{project, Home};
use factory_engine::{acquire_project_lease, project_lease_status, ProjectLeaseStatus};
use factory_supervisor::request::{Action, Request};
use factory_supervisor::{
    pid_alive, queue, records, ChildRecord, Outcome, ProcessSpawner, QueueItem, Supervisor,
};

const TIMEOUT: Duration = Duration::from_secs(20);

// The script counts its starts per project and job. A `hold` behavior runs until its gate file is deleted.
const SCRIPT: &str = r#"#!/bin/sh
d=$(dirname "$0")
echo x >> "$d/runs-$2-$4"
b=$(cat "$d/behavior-$2-$4")
case "$b" in
  merged*) echo "$b" ;;
  spoof) echo merged; exit 1 ;;
  busy-once)
    if [ "$(wc -l < "$d/runs-$2-$4")" -le 1 ]; then exit 75; fi
    echo merged ;;
  busy-always) exit 75 ;;
  hold)
    while [ -f "$d/gate-$2-$4" ]; do sleep 0.02; done
    echo merged ;;
esac
"#;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
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
    Env {
        home: Home::at(tmp.path().join("home")),
        dir,
        _tmp: tmp,
    }
}

impl Env {
    fn supervisor(&self) -> Supervisor {
        let bin = self.dir.join("factory");
        std::fs::write(&bin, SCRIPT).expect("script");
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let mut s = Supervisor::new(
            self.home.clone(),
            Box::new(ProcessSpawner::new(bin)),
            Box::new(now_ms),
        );
        s.set_max_parallel(4);
        s
    }

    fn add_project(&self, name: &str) {
        let repo = self._tmp.path().join("repos").join(name);
        std::fs::create_dir_all(&repo).expect("repo");
        project::add(
            &self.home,
            name,
            &repo,
            None,
            "rust",
            None,
            &|_| Ok(()),
            None,
        )
        .expect("add project");
    }

    /// Queues a request. The script behavior is `merged` or `hold`, and `hold` creates its gate file.
    fn enqueue_with(&self, project: &str, job: &str, behavior: &str) -> Request {
        std::fs::write(self.dir.join(format!("behavior-{project}-{job}")), behavior)
            .expect("behavior");
        if behavior == "hold" {
            std::fs::write(self.dir.join(format!("gate-{project}-{job}")), "").expect("gate");
        }
        self.enqueue(project, job)
    }

    fn release(&self, project: &str, job: &str) {
        std::fs::remove_file(self.dir.join(format!("gate-{project}-{job}"))).expect("remove gate");
    }

    fn runs(&self, project: &str, job: &str) -> usize {
        std::fs::read_to_string(self.dir.join(format!("runs-{project}-{job}")))
            .map(|t| t.lines().count())
            .unwrap_or(0)
    }

    /// Waits until the script wrote its first start, so a later count of 1 is not a race with its start.
    fn wait_started(&self, project: &str, job: &str) {
        let end = Instant::now() + TIMEOUT;
        while self.runs(project, job) == 0 {
            assert!(Instant::now() < end, "the script did not start");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn state_dir(&self, name: &str) -> PathBuf {
        let file = project::get(&self.home, name).expect("project");
        project::layout(&self.home, name, &file)
            .expect("layout")
            .state_dir
    }

    fn enqueue(&self, project: &str, job: &str) -> Request {
        let req = Request::new(project, Action::Run, job, false, now_ms()).expect("request");
        queue::enqueue(&self.home, &req).expect("enqueue");
        req
    }

    fn result(&self, id: &str) -> Option<Outcome> {
        records::read_results(&self.home)
            .into_iter()
            .flatten()
            .find(|r| r.request_id == id)
            .map(|r| r.outcome)
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
        let (o, e) = (
            logs.join(format!("{}.out", req.id)),
            logs.join(format!("{}.err", req.id)),
        );
        std::fs::write(&o, out).expect("out");
        std::fs::write(&e, "").expect("err");
        let rec = ChildRecord {
            pid,
            project: req.project.clone(),
            job: req.job.clone(),
            request_id: req.id.clone(),
            started_ms,
            out: o,
            err: e,
        };
        records::write_child(&self.home, &rec).expect("child record");
        rec
    }
}

fn sleeper() -> Child {
    Command::new("sleep")
        .arg("60")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("sleep")
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
        assert!(
            matches!(e.result(&req.id), Some(Outcome::Lost { .. })),
            "{:?}",
            e.result(&req.id)
        );
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

/// A pid that ran and was reaped, so no process has it.
fn dead_pid() -> u32 {
    let mut c = Command::new("sh")
        .arg("-c")
        .arg("exit 0")
        .spawn()
        .expect("sh");
    let pid = c.id();
    c.wait().expect("wait");
    pid
}

fn tick_until_finished(s: &mut Supervisor, id: &str) {
    let end = Instant::now() + TIMEOUT;
    loop {
        let r = s.tick().expect("tick");
        if r.finished.iter().any(|(i, _)| i == id) {
            return;
        }
        assert!(Instant::now() < end, "request {id} did not finish");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The process group of a pid, from `ps` (Linux and macOS both know `-o pgid=`).
fn pgrp(pid: u32) -> u32 {
    let out = Command::new("ps")
        .env("LC_ALL", "C")
        .args(["-o", "pgid=", "-p", &pid.to_string()])
        .output()
        .expect("ps");
    assert!(out.status.success(), "ps found no pid {pid}");
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .expect("pgid")
}

#[test]
fn a_new_supervisor_adopts_a_running_child_and_finishes_it() {
    let e = env();
    e.add_project("p");
    let id = e.enqueue_with("p", "1", "hold").id;
    let mut first = e.supervisor();
    assert_eq!(first.tick().expect("tick").started, vec![id.clone()]);
    let pid = records::read_children(&e.home)
        .into_iter()
        .flatten()
        .next()
        .expect("record")
        .pid;
    // The supervisor stops, the child does not.
    drop(first);
    e.wait_started("p", "1");
    assert!(pid_alive(pid));
    assert_ne!(
        pgrp(pid),
        pgrp(std::process::id()),
        "the child has its own process group"
    );
    let mut second = e.supervisor();
    let rec = second.recover().expect("recover");
    assert_eq!(rec.adopted, vec![id.clone()]);
    assert!(rec.report.finished.is_empty());
    for _ in 0..3 {
        assert!(second.tick().expect("tick").started.is_empty());
    }
    assert_eq!(e.runs("p", "1"), 1, "no second spawn");
    assert_eq!(e.queue_ids(), vec![id.clone()]);
    e.release("p", "1");
    tick_until_finished(&mut second, &id);
    assert_eq!(e.result(&id), Some(Outcome::Merged));
    assert!(records::read_children(&e.home).is_empty());
    assert!(e.queue_ids().is_empty());
}

#[test]
fn a_child_killed_while_no_supervisor_runs_is_lost_and_never_restarted() {
    let e = env();
    e.add_project("p");
    let id = e.enqueue_with("p", "1", "hold").id;
    let mut first = e.supervisor();
    first.tick().expect("tick");
    let pid = records::read_children(&e.home)
        .into_iter()
        .flatten()
        .next()
        .expect("record")
        .pid;
    drop(first);
    // Kill only after the script has started, so the start counter is written.
    e.wait_started("p", "1");
    assert!(Command::new("kill")
        .arg("-9")
        .arg(pid.to_string())
        .status()
        .expect("kill")
        .success());
    wait_dead(pid);
    let mut second = e.supervisor();
    let rec = second.recover().expect("recover");
    assert_eq!(rec.report.finished.len(), 1);
    assert!(
        matches!(e.result(&id), Some(Outcome::Lost { .. })),
        "{:?}",
        e.result(&id)
    );
    assert!(records::read_children(&e.home).is_empty());
    assert!(e.queue_ids().is_empty());
    // The project is free for a later request, and the lost job did not run again.
    let next = e.enqueue_with("p", "2", "merged").id;
    tick_until_finished(&mut second, &next);
    assert_eq!(e.result(&next), Some(Outcome::Merged));
    assert_eq!(e.runs("p", "1"), 1);
}

#[test]
fn a_child_record_without_a_queue_file_still_gets_a_result() {
    let e = env();
    e.add_project("p");
    let req = Request::new("p", Action::Run, "1", false, now_ms()).expect("request");
    e.child_record(&req, dead_pid(), now_ms(), "merged\n");
    let mut s = e.supervisor();
    let rec = s.recover().expect("recover");
    assert_eq!(rec.report.finished.len(), 1);
    assert_eq!(e.result(&req.id), Some(Outcome::Merged));
    assert!(records::read_children(&e.home).is_empty());
}

#[test]
fn a_corrupt_child_record_is_reported_and_the_others_are_handled() {
    let e = env();
    e.add_project("p");
    e.add_project("q");
    let req = Request::new("p", Action::Run, "1", false, now_ms()).expect("request");
    e.child_record(&req, dead_pid(), now_ms(), "merged\n");
    std::fs::write(e.home.children_dir().join("q.json"), "{nope").expect("corrupt");
    let blocked = e.enqueue_with("q", "1", "merged").id;
    let mut s = e.supervisor();
    let rec = s.recover().expect("recover");
    assert_eq!(rec.report.bad_children.len(), 1, "{rec:?}");
    assert_eq!(e.result(&req.id), Some(Outcome::Merged));
    // The project with the unreadable record stays busy.
    assert!(s.tick().expect("tick").started.is_empty());
    assert_eq!(e.runs("q", "1"), 0);
    assert_eq!(e.queue_ids(), vec![blocked]);
}

#[test]
fn two_supervisors_on_one_home_never_start_a_request_twice() {
    let e = env();
    e.add_project("p");
    let id = e.enqueue_with("p", "1", "hold").id;
    let mut a = e.supervisor();
    let mut b = e.supervisor();
    assert_eq!(a.tick().expect("tick").started, vec![id.clone()]);
    e.wait_started("p", "1");
    for _ in 0..3 {
        assert!(b.tick().expect("tick").started.is_empty());
        assert!(a.tick().expect("tick").started.is_empty());
    }
    assert_eq!(e.runs("p", "1"), 1);
    e.release("p", "1");
    tick_until_finished(&mut b, &id);
    assert_eq!(e.result(&id), Some(Outcome::Merged));
    assert_eq!(e.runs("p", "1"), 1);
}

#[test]
fn a_started_request_without_a_child_record_is_lost_and_not_run_again() {
    let e = env();
    e.add_project("p");
    let id = e.enqueue_with("p", "1", "merged").id;
    // The log files exist because the spawn step creates them before the child starts.
    let logs = e.home.logs_dir("p");
    std::fs::create_dir_all(&logs).expect("logs");
    std::fs::write(logs.join(format!("{id}.err")), "").expect("err log");
    let mut s = e.supervisor();
    let rec = s.recover().expect("recover");
    assert_eq!(rec.report.finished.len(), 1);
    assert!(
        rec.report
            .errors
            .iter()
            .any(|m| m.contains(&id) && m.contains(".err")),
        "{:?}",
        rec.report.errors
    );
    assert!(matches!(e.result(&id), Some(Outcome::Lost { .. })));
    assert!(e.queue_ids().is_empty());
    assert!(s.tick().expect("tick").started.is_empty());
    assert_eq!(e.runs("p", "1"), 0);
}

#[test]
fn a_live_lease_owner_without_a_child_record_is_an_orphan_until_it_ends() {
    let e = env();
    e.add_project("p");
    let mut stranger = sleeper();
    let state = e.state_dir("p");
    std::fs::create_dir_all(&state).expect("state");
    let owner = format!("driver-{}-1", stranger.id());
    let now = now_ms() as f64 / 1000.0;
    acquire_project_lease(
        &state,
        "001",
        &owner,
        100_000.0,
        now,
        Duration::from_secs(2),
    )
    .expect("lease");
    let id = e.enqueue_with("p", "1", "merged").id;
    let mut s = e.supervisor();
    let rec = s.recover().expect("recover");
    assert_eq!(rec.report.orphans, vec!["p".to_string()]);
    for _ in 0..3 {
        let r = s.tick().expect("tick");
        assert_eq!(r.orphans, vec!["p".to_string()]);
        assert!(r.started.is_empty());
    }
    assert_eq!(e.runs("p", "1"), 0);
    stranger.kill().expect("kill");
    stranger.wait().expect("wait");
    assert!(matches!(
        project_lease_status(&state, now),
        ProjectLeaseStatus::Stale { .. }
    ));
    tick_until_finished(&mut s, &id);
    assert_eq!(e.result(&id), Some(Outcome::Merged));
}

#[test]
fn a_second_locked_supervisor_is_refused_until_the_first_is_dropped() {
    let e = env();
    let spawner = || Box::new(ProcessSpawner::new(e.dir.join("factory")));
    let first = Supervisor::new_locked(e.home.clone(), spawner(), Box::new(now_ms)).expect("first");
    let err = Supervisor::new_locked(e.home.clone(), spawner(), Box::new(now_ms))
        .err()
        .expect("refused");
    assert!(
        err.to_string().contains(&std::process::id().to_string()),
        "{err}"
    );
    drop(first);
    // A child of a parallel test can hold a copy of the lock's file for a moment between fork and exec.
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        match Supervisor::new_locked(e.home.clone(), spawner(), Box::new(now_ms)) {
            Ok(_) => break,
            Err(err) => {
                assert!(Instant::now() < end, "still locked: {err}");
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
}

#[test]
fn a_merged_line_with_a_failing_exit_code_is_a_failure() {
    let e = env();
    e.add_project("p");
    let mut s = e.supervisor();
    let id = e.enqueue_with("p", "1", "spoof").id;
    tick_until_finished(&mut s, &id);
    assert!(matches!(
        e.result(&id),
        Some(Outcome::Failed { code: Some(1), .. })
    ));
}

#[test]
fn a_child_that_found_the_project_busy_is_started_again() {
    let e = env();
    e.add_project("p");
    let mut s = e.supervisor();
    let id = e.enqueue_with("p", "1", "busy-once").id;
    tick_until_finished(&mut s, &id);
    assert_eq!(e.result(&id), Some(Outcome::Merged));
    assert_eq!(e.runs("p", "1"), 2);
}

#[test]
fn a_request_that_always_finds_the_project_busy_fails_after_the_retries() {
    let e = env();
    e.add_project("p");
    let mut s = e.supervisor();
    let id = e.enqueue_with("p", "1", "busy-always").id;
    tick_until_finished(&mut s, &id);
    assert!(matches!(
        e.result(&id),
        Some(Outcome::Failed { code: Some(75), .. })
    ));
    assert_eq!(e.runs("p", "1"), 21);
}
