#![cfg(unix)]

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use factory_procgroup::{kill_all, set_registry, spawn};

/// The registry and the group set are process-wide, and `kill_all` ends every
/// group, so the tests of this file take turns.
fn serial() -> MutexGuard<'static, ()> {
    static TURN: Mutex<()> = Mutex::new(());
    TURN.lock().unwrap_or_else(|e| e.into_inner())
}

/// True once the pid no longer exists, polled for up to two seconds.
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

fn entries(dir: &Path, owner: &str) -> Vec<serde_json::Value> {
    let file = dir.join(format!("children-{owner}.json"));
    let text = std::fs::read_to_string(file).expect("registry file");
    serde_json::from_str(&text).expect("registry json")
}

#[test]
fn a_child_runs_in_its_own_group() {
    let _turn = serial();
    let mut child = spawn(Command::new("sleep").arg("30")).expect("spawn");
    let pid = child.id() as i32;
    // SAFETY: getpgid only reads process facts.
    unsafe {
        assert_eq!(libc::getpgid(pid), pid);
        assert_ne!(libc::getpgid(0), pid);
    }
    child.kill().expect("kill");
    child.wait().expect("wait");
}

#[test]
fn killing_a_group_child_ends_its_grandchild() {
    let _turn = serial();
    let mut child = spawn(
        Command::new("sh")
            .arg("-c")
            .arg("sleep 30 & echo $!; wait")
            .stdout(Stdio::piped()),
    )
    .expect("spawn");
    let stdout = child.stdout.take().expect("stdout");
    let mut line = String::new();
    BufReader::new(stdout).read_line(&mut line).expect("line");
    let grandchild: i32 = line.trim().parse().expect("pid");
    child.kill().expect("kill");
    child.wait().expect("wait");
    assert!(gone_soon(grandchild));
}

#[test]
fn dropping_a_group_child_kills_it() {
    let _turn = serial();
    let child = spawn(Command::new("sleep").arg("30")).expect("spawn");
    let pid = child.id() as i32;
    drop(child);
    assert!(gone_soon(pid));
}

#[test]
fn killing_an_exited_child_is_quiet() {
    let _turn = serial();
    let mut child = spawn(&mut Command::new("true")).expect("spawn");
    child.wait().expect("wait");
    child.kill().expect("quiet");
}

#[test]
fn the_registry_lists_a_spawned_child_and_drops_it_on_exit() {
    let _turn = serial();
    let dir = tempfile::tempdir().expect("dir");
    set_registry(dir.path().to_path_buf(), "driver-1-1".into(), |_| {
        Some(5000)
    });
    let mut child = spawn(Command::new("sleep").arg("30")).expect("spawn");
    let list = entries(dir.path(), "driver-1-1");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["pid"], child.id());
    assert_eq!(list[0]["start_ms"], 5000);
    child.kill().expect("kill");
    child.wait().expect("wait");
    assert!(!dir.path().join("children-driver-1-1.json").exists());
}

#[test]
fn an_owner_outside_the_allowed_set_keeps_no_registry() {
    let _turn = serial();
    let dir = tempfile::tempdir().expect("dir");
    set_registry(dir.path().to_path_buf(), "a/b".into(), |_| Some(1000));
    let mut child = spawn(Command::new("sleep").arg("30")).expect("spawn");
    let files = std::fs::read_dir(dir.path()).expect("read dir").count();
    assert_eq!(files, 0);
    child.kill().expect("kill");
    child.wait().expect("wait");
}

#[test]
fn two_threads_spawning_both_reach_the_registry() {
    let _turn = serial();
    let dir = tempfile::tempdir().expect("dir");
    set_registry(dir.path().to_path_buf(), "driver-2-2".into(), |_| {
        Some(1000)
    });
    let handles: Vec<_> = (0..2)
        .map(|_| std::thread::spawn(|| spawn(Command::new("sleep").arg("30")).expect("spawn")))
        .collect();
    let children: Vec<_> = handles
        .into_iter()
        .map(|h| h.join().expect("join"))
        .collect();
    assert_eq!(entries(dir.path(), "driver-2-2").len(), 2);
    drop(children);
}

#[test]
fn kill_all_ends_every_registered_group() {
    let _turn = serial();
    let dir = tempfile::tempdir().expect("dir");
    set_registry(dir.path().to_path_buf(), "driver-3-3".into(), |_| {
        Some(1000)
    });
    let mut a = spawn(Command::new("sleep").arg("30")).expect("spawn");
    let mut b = spawn(Command::new("sleep").arg("30")).expect("spawn");
    let (pa, pb) = (a.id() as i32, b.id() as i32);
    kill_all();
    // Reaping makes the pids count as gone.
    a.wait().expect("wait");
    b.wait().expect("wait");
    assert!(gone_soon(pa));
    assert!(gone_soon(pb));
}
