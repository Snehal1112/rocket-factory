//! Starting `factory` as a child process.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

use factory_core::Home;

use crate::request::{Action, Request};

const BIN_NAME: &str = "factory";
const BIN_ENV: &str = "FACTORY_BIN";
const HOME_ENV: &str = "FACTORY_HOME";
const TEXT_FILE_BUSY: i32 = 26;

#[derive(Debug, thiserror::Error)]
pub enum SpawnError {
    #[error("factory binary {0} not found")]
    Missing(PathBuf),
    #[error("cannot find the factory binary: {0}")]
    NoBin(String),
    #[error("cannot start {path}: {reason}")]
    Start { path: PathBuf, reason: String },
    #[error("process control is not supported on this platform")]
    Unsupported,
}

/// What a spawner knows about one pid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildPoll {
    /// This spawner did not start the pid, so it cannot tell.
    Unknown,
    Running,
    /// Ended. No code when a signal ended it.
    Exited(Option<i32>),
}

pub trait Spawner {
    fn spawn(&self, home: &Home, req: &Request, out: &Path, err: &Path) -> Result<u32, SpawnError>;

    /// The state of a child this spawner started.
    fn poll(&self, _pid: u32) -> ChildPoll {
        ChildPoll::Unknown
    }
}

/// Starts the real `factory` binary. It keeps the child handles so it can read exit codes.
#[derive(Debug)]
pub struct ProcessSpawner {
    pub bin: PathBuf,
    children: Mutex<HashMap<u32, Child>>,
}

impl ProcessSpawner {
    pub fn new(bin: PathBuf) -> ProcessSpawner {
        ProcessSpawner { bin, children: Mutex::new(HashMap::new()) }
    }

    /// `FACTORY_BIN`, else a `factory` file next to the running executable.
    pub fn from_env() -> Result<ProcessSpawner, SpawnError> {
        if let Some(v) = std::env::var_os(BIN_ENV).filter(|v| !v.is_empty()) {
            return Ok(ProcessSpawner::new(PathBuf::from(v)));
        }
        let exe = std::env::current_exe().map_err(|e| SpawnError::NoBin(e.to_string()))?;
        let dir = exe.parent().ok_or_else(|| SpawnError::NoBin("the executable has no folder".to_string()))?;
        Ok(ProcessSpawner::new(dir.join(BIN_NAME)))
    }
}

fn args_of(req: &Request) -> Vec<String> {
    let mut args = vec!["--project".to_string(), req.project.clone()];
    match req.action {
        Action::Run => {
            args.push("run".to_string());
            args.push(req.job.clone());
            if req.series {
                args.push("--series".to_string());
            }
        }
        Action::Approve => {
            args.push("approve".to_string());
            args.push(req.job.clone());
        }
    }
    args
}

#[cfg(unix)]
fn start(cmd: &mut Command) -> std::io::Result<Child> {
    use std::os::unix::process::CommandExt;
    // Its own process group, so stopping the supervisor never stops the child.
    cmd.process_group(0);
    // A script written just before it runs can briefly be busy. Retry a few times.
    let mut tries = 0;
    loop {
        match cmd.spawn() {
            Err(e) if e.raw_os_error() == Some(TEXT_FILE_BUSY) && tries < 20 => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            other => return other,
        }
    }
}

#[cfg(not(unix))]
fn start(_cmd: &mut Command) -> std::io::Result<Child> {
    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "unsupported"))
}

impl Spawner for ProcessSpawner {
    fn spawn(&self, home: &Home, req: &Request, out: &Path, err: &Path) -> Result<u32, SpawnError> {
        if cfg!(not(unix)) {
            return Err(SpawnError::Unsupported);
        }
        if !self.bin.is_file() {
            return Err(SpawnError::Missing(self.bin.clone()));
        }
        let fail = |reason: String| SpawnError::Start { path: self.bin.clone(), reason };
        for p in [out, err] {
            if let Some(dir) = p.parent() {
                std::fs::create_dir_all(dir).map_err(|e| fail(format!("{}: {e}", dir.display())))?;
            }
        }
        let out_file = std::fs::File::create(out).map_err(|e| fail(format!("{}: {e}", out.display())))?;
        let err_file = std::fs::File::create(err).map_err(|e| fail(format!("{}: {e}", err.display())))?;
        let mut cmd = Command::new(&self.bin);
        cmd.args(args_of(req))
            .env(HOME_ENV, &home.dir)
            .stdin(Stdio::null())
            .stdout(out_file)
            .stderr(err_file);
        let child = start(&mut cmd).map_err(|e| fail(e.to_string()))?;
        let pid = child.id();
        match self.children.lock() {
            Ok(mut m) => {
                m.insert(pid, child);
            }
            Err(p) => {
                p.into_inner().insert(pid, child);
            }
        }
        Ok(pid)
    }

    fn poll(&self, pid: u32) -> ChildPoll {
        let mut guard = match self.children.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let Some(child) = guard.get_mut(&pid) else { return ChildPoll::Unknown };
        match child.try_wait() {
            Ok(None) => ChildPoll::Running,
            Ok(Some(status)) => {
                guard.remove(&pid);
                ChildPoll::Exited(status.code())
            }
            Err(_) => ChildPoll::Running,
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};

    pub(crate) fn write_script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write script");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        path
    }

    fn wait_exit(sp: &ProcessSpawner, pid: u32) -> ChildPoll {
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            let p = sp.poll(pid);
            if !matches!(p, ChildPoll::Running) || Instant::now() > end {
                return p;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn setup() -> (tempfile::TempDir, Home, ProcessSpawner) {
        let t = tempfile::tempdir().expect("tempdir");
        let home = Home::at(t.path().join("home"));
        let bin = write_script(t.path(), "fake-factory", "echo \"$@\"\necho \"home=$FACTORY_HOME\"\nexit 0");
        (t, home, ProcessSpawner::new(bin))
    }

    fn req(action: Action, series: bool) -> Request {
        Request::new("p", action, "7", series, 1).expect("request")
    }

    #[test]
    fn runs_with_arguments_home_and_its_own_process_group() {
        let (t, home, sp) = setup();
        let (out, err) = (t.path().join("o.txt"), t.path().join("e.txt"));
        let pid = sp.spawn(&home, &req(Action::Run, false), &out, &err).expect("spawn");
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).expect("stat");
        let after = stat.rsplit_once(')').expect("comm").1;
        let pgrp: u32 = after.split_whitespace().nth(2).expect("pgrp").parse().expect("number");
        assert_eq!(pgrp, pid);
        assert_eq!(wait_exit(&sp, pid), ChildPoll::Exited(Some(0)));
        let text = std::fs::read_to_string(&out).expect("out");
        assert!(text.contains("--project p run 7\n"), "{text}");
        assert!(text.contains(&format!("home={}", home.dir.display())), "{text}");
    }

    #[test]
    fn series_and_approve_shapes() {
        let (t, home, sp) = setup();
        let (out, err) = (t.path().join("o.txt"), t.path().join("e.txt"));
        let pid = sp.spawn(&home, &req(Action::Run, true), &out, &err).expect("spawn");
        wait_exit(&sp, pid);
        assert!(std::fs::read_to_string(&out).expect("out").contains("--project p run 7 --series\n"));
        let pid = sp.spawn(&home, &req(Action::Approve, false), &out, &err).expect("spawn");
        wait_exit(&sp, pid);
        assert!(std::fs::read_to_string(&out).expect("out").contains("--project p approve 7\n"));
    }

    #[test]
    fn missing_binary_names_the_path() {
        let t = tempfile::tempdir().expect("tempdir");
        let home = Home::at(t.path().to_path_buf());
        let sp = ProcessSpawner::new(t.path().join("nope"));
        let e = sp.spawn(&home, &req(Action::Run, false), &t.path().join("o"), &t.path().join("e")).expect_err("missing");
        assert!(e.to_string().contains("nope"), "{e}");
    }
}
