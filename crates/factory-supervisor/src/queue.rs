//! The request queue: one JSON file per request in the queue folder.
//!
//! The folder is writable by the same user, so every file is validated before use, and file
//! names are derived from the validated id only.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use factory_core::{CoreError, Home};

use crate::records::read_regular;
use crate::request::{validate_id_chars, Request, RequestError};

const EXT: &str = "json";
/// A request is a few hundred bytes. Anything larger is refused.
const REQUEST_CAP: u64 = 16 * 1024;
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, thiserror::Error)]
pub enum QueueError {
    #[error(transparent)]
    Request(#[from] RequestError),
    #[error("request {0} is already queued")]
    Duplicate(String),
    #[error("no queued request {0}")]
    Unknown(String),
    #[error(transparent)]
    Core(#[from] CoreError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueItem {
    Ok(Request),
    Bad { file: String, reason: String },
}

fn path_of(home: &Home, id: &str) -> PathBuf {
    home.queue_dir().join(format!("{id}.{EXT}"))
}

/// Writes the request atomically. The same id twice is an error.
pub fn enqueue(home: &Home, req: &Request) -> Result<(), QueueError> {
    req.validate()?;
    let path = path_of(home, &req.id);
    let bytes = serde_json::to_vec_pretty(req).map_err(CoreError::from)?;
    claim(&path, &bytes)
}

/// Creates `path` with the content so that exactly one caller wins and nobody is overwritten.
/// The content goes to a temp file first, then a hard link claims the final name.
fn claim(path: &Path, bytes: &[u8]) -> Result<(), QueueError> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| CoreError::io(dir, e))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let n = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!(".{name}.{}.{n}.tmp", std::process::id()));
    let written = (|| -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(CoreError::io(&tmp, e).into());
    }
    let linked = std::fs::hard_link(&tmp, path);
    let _ = std::fs::remove_file(&tmp);
    match linked {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(QueueError::Duplicate(name)),
        // No hard links here: claim the name with create_new instead.
        Err(_) => {
            let mut f = match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
            {
                Ok(f) => f,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    return Err(QueueError::Duplicate(name))
                }
                Err(e) => return Err(CoreError::io(path, e).into()),
            };
            f.write_all(bytes)
                .and_then(|()| f.sync_all())
                .map_err(|e| CoreError::io(path, e))?;
            Ok(())
        }
    }
}

fn read_item(path: &std::path::Path, stem: &str) -> Result<Request, String> {
    let text = read_regular(path, REQUEST_CAP)?;
    let req: Request = serde_json::from_str(&text).map_err(|e| format!("invalid request: {e}"))?;
    req.validate().map_err(|e| e.to_string())?;
    if req.id != stem {
        return Err(format!("file name does not match request id {}", req.id));
    }
    Ok(req)
}

/// Every queued request sorted by `ts_ms` then id, then the bad files by name.
/// One bad file never fails the listing.
pub fn list(home: &Home) -> Vec<QueueItem> {
    let dir = home.queue_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut good = Vec::new();
    let mut bad = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let stem = name.strip_suffix(&format!(".{EXT}"));
        let result = match stem {
            Some(stem) => read_item(&path, stem),
            None => Err("not a .json file".to_string()),
        };
        match result {
            Ok(req) => good.push(req),
            Err(reason) => bad.push((name, reason)),
        }
    }
    good.sort_by(|a, b| a.ts_ms.cmp(&b.ts_ms).then_with(|| a.id.cmp(&b.id)));
    bad.sort();
    good.into_iter()
        .map(QueueItem::Ok)
        .chain(
            bad.into_iter()
                .map(|(file, reason)| QueueItem::Bad { file, reason }),
        )
        .collect()
}

/// Like `list`, but reports queue directory and entry read errors. A missing
/// queue directory is an empty queue. Bad files stay `QueueItem::Bad`.
pub fn try_list(home: &Home) -> Result<Vec<QueueItem>, QueueError> {
    let dir = home.queue_dir();
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(CoreError::io(&dir, e).into()),
    };
    let mut good = Vec::new();
    let mut bad = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| CoreError::io(&dir, e))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let result = match name.strip_suffix(&format!(".{EXT}")) {
            Some(stem) => read_item(&path, stem),
            None => Err("not a .json file".to_string()),
        };
        match result {
            Ok(req) => good.push(req),
            Err(reason) => bad.push((name, reason)),
        }
    }
    good.sort_by(|a, b| a.ts_ms.cmp(&b.ts_ms).then_with(|| a.id.cmp(&b.id)));
    bad.sort();
    Ok(good
        .into_iter()
        .map(QueueItem::Ok)
        .chain(
            bad.into_iter()
                .map(|(file, reason)| QueueItem::Bad { file, reason }),
        )
        .collect())
}

/// Deletes a queued request by id. An unknown id is an error.
pub fn remove(home: &Home, id: &str) -> Result<(), QueueError> {
    validate_id_chars(id)?;
    let path = path_of(home, id);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(QueueError::Unknown(id.to_string()))
        }
        Err(e) => Err(CoreError::io(&path, e).into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::Action;

    fn home() -> (tempfile::TempDir, Home) {
        let t = tempfile::tempdir().expect("tempdir");
        let h = Home::at(t.path().join("home"));
        (t, h)
    }

    fn req(p: &str, ts: u64) -> Request {
        Request::new(p, Action::Run, "1", false, ts).expect("request")
    }

    #[test]
    fn try_list_returns_empty_for_missing_queue_directory() {
        let (_tmp, home) = home();
        assert_eq!(try_list(&home).expect("missing queue is empty"), Vec::new());
    }

    #[test]
    fn try_list_returns_bad_items_for_malformed_requests() {
        let (_tmp, home) = home();
        let good = req("a", 1);
        enqueue(&home, &good).expect("enqueue valid request");
        std::fs::write(home.queue_dir().join("junk.json"), "{not json").expect("write bad request");

        let items = try_list(&home).expect("scan queue");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0], QueueItem::Ok(good));
        match &items[1] {
            QueueItem::Bad { file, reason } => {
                assert_eq!(file, "junk.json");
                assert!(!reason.is_empty());
            }
            QueueItem::Ok(request) => panic!("unexpected valid request: {request:?}"),
        }
    }

    #[test]
    fn try_list_reports_queue_directory_read_errors() {
        let (_tmp, home) = home();
        std::fs::create_dir_all(&home.dir).expect("create home");
        std::fs::write(home.queue_dir(), "not a directory").expect("make queue path a file");
        assert!(matches!(try_list(&home), Err(QueueError::Core(_))));
    }

    #[test]
    fn enqueue_writes_a_file_named_after_the_id_and_refuses_a_duplicate() {
        let (_t, h) = home();
        let r = req("a", 1);
        enqueue(&h, &r).expect("enqueue");
        assert!(h.queue_dir().join(format!("{}.json", r.id)).is_file());
        assert!(matches!(enqueue(&h, &r), Err(QueueError::Duplicate(_))));
        assert_eq!(list(&h), vec![QueueItem::Ok(r)]);
    }

    #[test]
    fn list_is_sorted_by_time_then_id() {
        let (_t, h) = home();
        for (p, ts) in [("c", 3), ("b", 2), ("z", 1), ("a", 1)] {
            enqueue(&h, &req(p, ts)).expect("enqueue");
        }
        let got: Vec<String> = list(&h)
            .into_iter()
            .map(|i| match i {
                QueueItem::Ok(r) => r.project,
                QueueItem::Bad { file, .. } => file,
            })
            .collect();
        assert_eq!(got, ["a", "z", "b", "c"]);
    }

    #[test]
    fn list_of_a_missing_folder_is_empty() {
        let (_t, h) = home();
        assert!(list(&h).is_empty());
    }

    #[test]
    fn bad_files_are_reported_and_good_ones_still_listed() {
        let (_t, h) = home();
        let good = req("a", 1);
        enqueue(&h, &good).expect("enqueue");
        let q = h.queue_dir();
        std::fs::write(q.join("junk.json"), "{not json").expect("write");
        // Valid JSON that fails validation: a path in the project.
        let evil = r#"{"id":"0000000000002-p-1","project":"../x","action":"run","job":"1","series":false,"ts_ms":2}"#;
        std::fs::write(q.join("0000000000002-p-1.json"), evil).expect("write");
        // A valid request under the wrong file name.
        let other = serde_json::to_string(&req("b", 3)).expect("json");
        std::fs::write(q.join("0000000000009-b-1.json"), other).expect("write");
        std::fs::write(q.join("notes.txt"), "x").expect("write");
        let items = list(&h);
        assert_eq!(items.len(), 5);
        assert_eq!(items[0], QueueItem::Ok(good));
        let bad: Vec<&str> = items[1..]
            .iter()
            .map(|i| match i {
                QueueItem::Bad { file, reason } => {
                    assert!(!reason.is_empty());
                    file.as_str()
                }
                QueueItem::Ok(_) => panic!("unexpected good item"),
            })
            .collect();
        assert_eq!(
            bad,
            [
                "0000000000002-p-1.json",
                "0000000000009-b-1.json",
                "junk.json",
                "notes.txt"
            ]
        );
    }

    #[test]
    fn remove_deletes_by_id_and_refuses_unknown_or_unsafe_ids() {
        let (_t, h) = home();
        let r = req("a", 1);
        enqueue(&h, &r).expect("enqueue");
        remove(&h, &r.id).expect("remove");
        assert!(list(&h).is_empty());
        assert!(matches!(remove(&h, &r.id), Err(QueueError::Unknown(_))));
        assert!(matches!(remove(&h, "../x"), Err(QueueError::Request(_))));
    }

    #[test]
    fn two_writers_of_one_id_leave_exactly_one_winner() {
        for _ in 0..20 {
            let (_t, h) = home();
            let gate = std::sync::Arc::new(std::sync::Barrier::new(2));
            let run = |series: bool| {
                let (h, gate) = (h.clone(), gate.clone());
                std::thread::spawn(move || {
                    let r = Request::new("a", Action::Run, "1", series, 1).expect("request");
                    gate.wait();
                    enqueue(&h, &r).map(|()| r)
                })
            };
            let (x, y) = (run(false), run(true));
            let results = [x.join().expect("x"), y.join().expect("y")];
            let wins: Vec<&Request> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
            assert_eq!(wins.len(), 1);
            assert!(results
                .iter()
                .any(|r| matches!(r, Err(QueueError::Duplicate(_)))));
            assert_eq!(list(&h), vec![QueueItem::Ok(wins[0].clone())]);
            let leftovers = std::fs::read_dir(h.queue_dir()).expect("list").count();
            assert_eq!(leftovers, 1, "temp files must be gone");
        }
    }

    #[cfg(unix)]
    #[test]
    fn odd_and_oversize_files_are_bad_and_listing_returns() {
        let (t, h) = home();
        let good = req("a", 1);
        enqueue(&h, &good).expect("enqueue");
        let q = h.queue_dir();
        let elsewhere = t.path().join("real.json");
        std::fs::write(
            &elsewhere,
            serde_json::to_string(&req("b", 2)).expect("json"),
        )
        .expect("write");
        std::os::unix::fs::symlink(&elsewhere, q.join("0000000000002-b-1.json")).expect("symlink");
        std::os::unix::fs::symlink("/dev/zero", q.join("zero.json")).expect("symlink");
        std::fs::write(q.join("big.json"), vec![b' '; 20 * 1024]).expect("write");
        std::fs::create_dir(q.join("dir.json")).expect("mkdir");
        let mut expect_bad = 4;
        match std::process::Command::new("mkfifo")
            .arg(q.join("fifo.json"))
            .status()
        {
            Ok(s) if s.success() => expect_bad += 1,
            _ => eprintln!("mkfifo is missing, the FIFO case is skipped"),
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let h2 = h.clone();
        std::thread::spawn(move || {
            let _ = tx.send(list(&h2));
        });
        let items = rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("list must return promptly");
        assert_eq!(items[0], QueueItem::Ok(good));
        assert_eq!(items.len(), 1 + expect_bad);
        for i in &items[1..] {
            match i {
                QueueItem::Bad { reason, .. } => assert!(
                    reason == "not a regular file" || reason == "file too large",
                    "{reason}"
                ),
                QueueItem::Ok(_) => panic!("unexpected good item"),
            }
        }
    }
}
