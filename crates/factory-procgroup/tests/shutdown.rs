#![cfg(unix)]

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use factory_procgroup::reap_leftovers;

const DEMO: &str = env!("CARGO_BIN_EXE_shutdown-demo");

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

/// Starts the demo and returns it with the pid of its child.
fn start_demo(dir: &Path, owner: &str) -> (Child, i32) {
    let mut demo = Command::new(DEMO)
        .arg(dir)
        .arg(owner)
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn demo");
    let stdout = demo.stdout.take().expect("stdout");
    let mut line = String::new();
    BufReader::new(stdout).read_line(&mut line).expect("line");
    (demo, line.trim().parse().expect("child pid"))
}

fn signal(pid: u32, sig: i32) {
    // SAFETY: plain kill of a process the test started.
    unsafe { libc::kill(pid as i32, sig) };
}

fn listed_pids(dir: &Path, owner: &str) -> Vec<i64> {
    let text =
        std::fs::read_to_string(dir.join(format!("children-{owner}.json"))).expect("registry file");
    let list: Vec<serde_json::Value> = serde_json::from_str(&text).expect("json");
    list.iter().filter_map(|e| e["pid"].as_i64()).collect()
}

#[test]
fn sigterm_to_the_driver_ends_its_child() {
    let dir = tempfile::tempdir().expect("dir");
    let (mut demo, child) = start_demo(dir.path(), "driver-9-9");
    signal(demo.id(), libc::SIGTERM);
    let status = demo.wait().expect("wait");
    assert_eq!(status.code(), Some(143));
    assert!(gone_soon(child));
}

#[test]
fn two_processes_registering_at_once_both_survive() {
    let dir = tempfile::tempdir().expect("dir");
    let (mut a, child_a) = start_demo(dir.path(), "driver-1-1");
    let (mut b, child_b) = start_demo(dir.path(), "driver-2-2");
    assert_eq!(listed_pids(dir.path(), "driver-1-1"), [i64::from(child_a)]);
    assert_eq!(listed_pids(dir.path(), "driver-2-2"), [i64::from(child_b)]);
    signal(a.id(), libc::SIGKILL);
    signal(b.id(), libc::SIGKILL);
    a.wait().expect("wait");
    b.wait().expect("wait");
    let lines = reap_leftovers(dir.path(), &|_| true, &|_| true, |_| Some(7000));
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(gone_soon(child_a));
    assert!(gone_soon(child_b));
}

#[test]
fn sigkill_then_reap_ends_the_child() {
    let dir = tempfile::tempdir().expect("dir");
    let owner = "driver-5-5";
    let (mut demo, child) = start_demo(dir.path(), owner);
    signal(demo.id(), libc::SIGKILL);
    demo.wait().expect("wait");
    // SAFETY: signal 0 only probes the pid.
    assert_eq!(unsafe { libc::kill(child, 0) }, 0);
    let lines = reap_leftovers(dir.path(), &|o| o == owner, &|_| true, |_| Some(7000));
    assert!(lines[0].starts_with("killed"), "{lines:?}");
    assert!(gone_soon(child));
}
