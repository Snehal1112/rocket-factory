//! The instance lock. One supervisor per home: an exclusive file lock with the pid inside.

use std::io::{Read, Seek, Write};
use std::path::PathBuf;

use factory_core::Home;
use fs2::FileExt;

const LOCK_FILE: &str = "factoryd.lock";
const STOP_FILE: &str = "stop";

#[derive(Debug, thiserror::Error)]
pub enum LockError {
    #[error("factoryd is already running (pid {0})")]
    Held(String),
    #[error("instance lock: {0}")]
    Io(String),
}

/// Holds the lock until it is dropped.
#[derive(Debug)]
pub struct InstanceLock {
    _file: std::fs::File,
}

fn lock_path(home: &Home) -> PathBuf {
    home.run_dir().join(LOCK_FILE)
}

/// The stop file. Its existence asks a running supervisor to end its loop.
pub fn stop_file(home: &Home) -> PathBuf {
    home.run_dir().join(STOP_FILE)
}

fn open(home: &Home) -> Result<std::fs::File, LockError> {
    let dir = home.run_dir();
    std::fs::create_dir_all(&dir).map_err(|e| LockError::Io(format!("{}: {e}", dir.display())))?;
    let path = lock_path(home);
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|e| LockError::Io(format!("{}: {e}", path.display())))
}

fn read_pid(file: &mut std::fs::File) -> String {
    let mut text = String::new();
    let _ = file.seek(std::io::SeekFrom::Start(0));
    let _ = Read::by_ref(file).take(64).read_to_string(&mut text);
    let text = text.trim();
    if text.is_empty() { "unknown".to_string() } else { text.to_string() }
}

impl InstanceLock {
    /// Takes the lock or fails with a message that names the running pid.
    pub fn acquire(home: &Home) -> Result<InstanceLock, LockError> {
        let mut file = open(home)?;
        if file.try_lock_exclusive().is_err() {
            return Err(LockError::Held(read_pid(&mut file)));
        }
        let io = |e: std::io::Error| LockError::Io(e.to_string());
        file.set_len(0).map_err(io)?;
        file.seek(std::io::SeekFrom::Start(0)).map_err(io)?;
        file.write_all(format!("{}\n", std::process::id()).as_bytes()).map_err(io)?;
        file.flush().map_err(io)?;
        Ok(InstanceLock { _file: file })
    }

    /// The pid text of a running instance, or None when no instance holds the lock.
    pub fn running(home: &Home) -> Option<String> {
        let mut file = open(home).ok()?;
        if file.try_lock_exclusive().is_ok() {
            let _ = FileExt::unlock(&file);
            return None;
        }
        Some(read_pid(&mut file))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> (tempfile::TempDir, Home) {
        let t = tempfile::tempdir().expect("tempdir");
        let h = Home::at(t.path().to_path_buf());
        (t, h)
    }

    #[test]
    fn second_acquire_fails_naming_the_pid_and_drop_releases() {
        let (_t, h) = home();
        let first = InstanceLock::acquire(&h).expect("first");
        let err = InstanceLock::acquire(&h).expect_err("second").to_string();
        assert!(err.contains(&std::process::id().to_string()), "{err}");
        assert_eq!(InstanceLock::running(&h), Some(std::process::id().to_string()));
        drop(first);
        assert_eq!(InstanceLock::running(&h), None);
        InstanceLock::acquire(&h).expect("third");
    }
}
