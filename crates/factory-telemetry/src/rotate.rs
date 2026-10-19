use fs2::FileExt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Production cap for one log file.
pub const LOG_CAP_BYTES: u64 = 10 * 1024 * 1024;

/// An append-only log file that rotates to `.1` to `.3`.
#[derive(Debug)]
pub struct RotatingFile {
    path: PathBuf,
    cap_bytes: u64,
    failures: AtomicU64,
}

fn suffixed(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

impl RotatingFile {
    pub fn new(path: impl Into<PathBuf>, cap_bytes: u64) -> RotatingFile {
        RotatingFile {
            path: path.into(),
            cap_bytes,
            failures: AtomicU64::new(0),
        }
    }

    /// Number of failed writes so far.
    pub fn failures(&self) -> u64 {
        self.failures.load(Ordering::Relaxed)
    }

    /// Append one line plus newline. Never returns an error.
    pub fn write_line(&self, line: &str) {
        if self.try_write(line).is_err() {
            self.failures.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn try_write(&self, line: &str) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let mut buf = String::with_capacity(line.len() + 1);
        buf.push_str(line);
        buf.push('\n');
        file.write_all(buf.as_bytes())?;
        let len = file.metadata()?.len();
        drop(file);
        if len > self.cap_bytes {
            self.rotate()?;
        }
        Ok(())
    }

    fn rotate(&self) -> std::io::Result<()> {
        let lock = OpenOptions::new()
            .create(true)
            .append(true)
            .open(suffixed(&self.path, ".lock"))?;
        lock.lock_exclusive()?;
        let result = (|| {
            let len = fs::metadata(&self.path)?.len();
            if len <= self.cap_bytes {
                return Ok(());
            }
            let p = |n: &str| suffixed(&self.path, n);
            if p(".2").exists() {
                fs::rename(p(".2"), p(".3"))?;
            }
            if p(".1").exists() {
                fs::rename(p(".1"), p(".2"))?;
            }
            fs::rename(&self.path, p(".1"))
        })();
        let _ = FileExt::unlock(&lock);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_write_creates_the_folder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("a/b/log.jsonl");
        let f = RotatingFile::new(&path, 1000);
        f.write_line("{}");
        assert!(path.exists());
        assert_eq!(f.failures(), 0);
    }

    #[test]
    fn write_appends_lines_with_newlines() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("log.jsonl");
        let f = RotatingFile::new(&path, 1000);
        f.write_line("one");
        f.write_line("two");
        assert_eq!(fs::read_to_string(&path).expect("read"), "one\ntwo\n");
    }

    #[test]
    fn rotation_moves_files_to_1_2_3() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("log.jsonl");
        let f = RotatingFile::new(&path, 50);
        for i in 0..40 {
            f.write_line(&format!("line number {i:04} padded"));
        }
        for n in ["1", "2", "3"] {
            assert!(suffixed(&path, &format!(".{n}")).exists(), ".{n}");
        }
        let live = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        assert!(live <= 50 + 25, "live {live}");
    }

    #[test]
    fn oldest_file_is_dropped() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("log.jsonl");
        let f = RotatingFile::new(&path, 50);
        for i in 0..100 {
            f.write_line(&format!("line number {i:04} padded"));
        }
        assert!(!suffixed(&path, ".4").exists());
    }

    #[test]
    fn concurrent_writers_lose_no_line() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("log.jsonl");
        let f = std::sync::Arc::new(RotatingFile::new(&path, 4096));
        let mut handles = Vec::new();
        for t in 0..8 {
            let f = f.clone();
            handles.push(std::thread::spawn(move || {
                for i in 0..200 {
                    let n = t * 1000 + i;
                    f.write_line(&format!("{:<40}", format!("{{\"n\":{n}}}")));
                }
            }));
        }
        for h in handles {
            h.join().expect("join");
        }
        let mut seen = std::collections::HashSet::new();
        for suffix in ["", ".1", ".2", ".3"] {
            let Ok(text) = fs::read_to_string(suffixed(&path, suffix)) else {
                continue;
            };
            for l in text.lines() {
                let v: serde_json::Value = serde_json::from_str(l).expect("json line");
                let n = v["n"].as_i64().expect("n");
                assert!(seen.insert(n), "duplicate {n}");
            }
        }
        assert!(!seen.is_empty());
    }

    #[test]
    fn write_error_is_counted_not_returned() {
        let dir = tempfile::tempdir().expect("tempdir");
        let f = RotatingFile::new(dir.path(), 1000);
        f.write_line("x");
        assert_eq!(f.failures(), 1);
    }
}
