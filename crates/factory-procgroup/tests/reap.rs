#![cfg(unix)]

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use factory_procgroup::{reap_leftovers, spawn, GroupChild};

fn gone_soon(pid: i32) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        // SAFETY: signal 0 only probes the pid.
        let rc = unsafe { libc::kill(pid, 0) };
        if rc == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

fn write_entry(dir: &Path, owner: &str, pid: u32) {
    let entries = serde_json::json!([{"pid": pid, "start_ms": 5000, "owner": owner}]);
    let file = dir.join(format!("children-{owner}.json"));
    std::fs::write(file, entries.to_string()).expect("write registry");
}

fn sleeper() -> GroupChild {
    spawn(Command::new("sleep").arg("60")).expect("spawn")
}

/// Kills the sleeper and reaps it, so the test leaves nothing behind.
fn finish(mut child: GroupChild) {
    child.kill().expect("kill");
    child.wait().expect("wait");
}

#[test]
fn reap_kills_a_matching_leftover() {
    let dir = tempfile::tempdir().expect("dir");
    let mut child = sleeper();
    write_entry(dir.path(), "dead", child.id());
    let lines = reap_leftovers(dir.path(), &|o| o == "dead", &|_| true, |_| Some(5000));
    assert_eq!(lines.len(), 1);
    assert!(lines[0].starts_with("killed"), "{lines:?}");
    child.wait().expect("wait");
    assert!(gone_soon(child.id() as i32));
    assert!(!dir.path().join("children-dead.json").exists());
}

#[test]
fn reap_leaves_a_start_time_mismatch_alone() {
    let dir = tempfile::tempdir().expect("dir");
    let child = sleeper();
    write_entry(dir.path(), "dead", child.id());
    let lines = reap_leftovers(dir.path(), &|_| true, &|_| true, |_| Some(9999));
    assert!(lines[0].starts_with("left alone"), "{lines:?}");
    assert_eq!(
        child.id() as i32,
        // SAFETY: getpgid only reads process facts.
        unsafe { libc::getpgid(child.id() as i32) }
    );
    finish(child);
}

#[test]
fn reap_kills_nothing_when_the_start_time_is_unknown() {
    let dir = tempfile::tempdir().expect("dir");
    let child = sleeper();
    write_entry(dir.path(), "dead", child.id());
    let lines = reap_leftovers(dir.path(), &|_| true, &|_| true, |_| None);
    assert!(lines[0].starts_with("left alone"), "{lines:?}");
    // SAFETY: signal 0 only probes the pid.
    assert_eq!(unsafe { libc::kill(child.id() as i32, 0) }, 0);
    finish(child);
}

#[test]
fn reap_leaves_a_non_leader_alone() {
    let dir = tempfile::tempdir().expect("dir");
    let mut shell = Command::new("sh")
        .arg("-c")
        .arg("sleep 60 & echo $!; wait")
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn");
    let stdout = shell.stdout.take().expect("stdout");
    let mut line = String::new();
    BufReader::new(stdout).read_line(&mut line).expect("line");
    let grandchild: u32 = line.trim().parse().expect("pid");
    write_entry(dir.path(), "dead", grandchild);
    let lines = reap_leftovers(dir.path(), &|_| true, &|_| true, |_| Some(5000));
    assert!(lines[0].starts_with("left alone"), "{lines:?}");
    // SAFETY: signal 0 only probes the pid.
    assert_eq!(unsafe { libc::kill(grandchild as i32, 0) }, 0);
    // SAFETY: plain kill of the child we started.
    unsafe { libc::kill(grandchild as i32, libc::SIGKILL) };
    shell.wait().expect("wait");
}

#[test]
fn reap_ignores_entries_of_other_owners() {
    let dir = tempfile::tempdir().expect("dir");
    let child = sleeper();
    write_entry(dir.path(), "live", child.id());
    let lines = reap_leftovers(dir.path(), &|o| o == "dead", &|_| true, |_| Some(5000));
    assert!(lines.is_empty(), "{lines:?}");
    assert!(dir.path().join("children-live.json").exists());
    // SAFETY: signal 0 only probes the pid.
    assert_eq!(unsafe { libc::kill(child.id() as i32, 0) }, 0);
    finish(child);
}

#[test]
fn reap_with_a_missing_or_corrupt_registry_kills_nothing() {
    let dir = tempfile::tempdir().expect("dir");
    let file = dir.path().join("children-dead.json");
    let absent = dir.path().join("absent");
    let lines = reap_leftovers(&absent, &|_| true, &|_| true, |_| Some(5000));
    assert_eq!(lines.len(), 1);
    assert!(lines[0].starts_with("left alone: registry"), "{lines:?}");
    std::fs::write(&file, "not json").expect("write");
    let lines = reap_leftovers(dir.path(), &|_| true, &|_| true, |_| Some(5000));
    assert_eq!(lines.len(), 1);
    assert!(lines[0].starts_with("left alone: registry"), "{lines:?}");
    assert!(file.exists());
}
