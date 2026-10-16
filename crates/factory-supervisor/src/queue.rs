//! The request queue: one JSON file per request in the queue folder.
//!
//! The folder is writable by the same user, so every file is validated before use, and file
//! names are derived from the validated id only.

use std::path::PathBuf;

use factory_core::{write_atomic, CoreError, Home};

use crate::request::{validate_id_chars, Request, RequestError};

const EXT: &str = "json";

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
    if path.exists() {
        return Err(QueueError::Duplicate(req.id.clone()));
    }
    let bytes = serde_json::to_vec_pretty(req).map_err(CoreError::from)?;
    write_atomic(&path, &bytes)?;
    Ok(())
}

fn read_item(path: &std::path::Path, stem: &str) -> Result<Request, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read: {e}"))?;
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
    let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut good = Vec::new();
    let mut bad = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || path.is_dir() {
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
        .chain(bad.into_iter().map(|(file, reason)| QueueItem::Bad { file, reason }))
        .collect()
}

/// Deletes a queued request by id. An unknown id is an error.
pub fn remove(home: &Home, id: &str) -> Result<(), QueueError> {
    validate_id_chars(id)?;
    let path = path_of(home, id);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(QueueError::Unknown(id.to_string())),
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
        assert_eq!(bad, ["0000000000002-p-1.json", "0000000000009-b-1.json", "junk.json", "notes.txt"]);
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
}
