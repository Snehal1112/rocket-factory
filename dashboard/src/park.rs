//! Reads the driver's `park.json` for the dashboard. The server never writes it.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::Board;
use crate::paths::Paths;

/// The file the driver writes into a job folder when it parks the job.
pub const PARK_FILE: &str = "park.json";

/// `<sdd>/<dir>/park.json`, or `None` when `dir` could point outside the sdd folder.
pub fn park_path(paths: &Paths, dir: &str) -> Option<PathBuf> {
    let bad = dir.is_empty() || dir.contains(['/', '\\', '\0']) || dir.contains("..");
    if bad {
        return None;
    }
    Some(paths.sdd.join(dir).join(PARK_FILE))
}

/// Most evidence text, in bytes, one reply carries.
pub const MAX_REPLY_TEXT: usize = 1024 * 1024;

/// One block of evidence as the dashboard gets it. Unknown fields in the file are dropped.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PartView {
    pub title: String,
    pub source: String,
    pub text: String,
    pub truncated: bool,
}

/// The park record as the dashboard gets it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ParkView {
    pub kind: String,
    pub reason: String,
    pub at: u64,
    pub round: u32,
    pub next: Vec<String>,
    pub parts: Vec<PartView>,
}

pub enum ParkReply {
    Found(ParkView),
    NotFound,
    Broken,
}

fn valid_id(id: &str) -> bool {
    (3..=6).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit())
}

/// Keeps the end of `text`, at most `max` bytes, cut on a character boundary.
fn tail(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut start = text.len() - max;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    text[start..].to_string()
}

/// Shares `MAX_REPLY_TEXT` over the parts in order. A part that does not fit is cut and marked truncated.
fn cap_parts(view: &mut ParkView) {
    let mut room = MAX_REPLY_TEXT;
    for part in &mut view.parts {
        if part.text.len() > room {
            part.text = tail(&part.text, room);
            part.truncated = true;
        }
        room -= part.text.len();
    }
}

/// Loads the park record of one job. The path comes from the board, never from the request.
pub fn load_park(paths: &Paths, board: &Board, id: &str) -> ParkReply {
    if !valid_id(id) {
        return ParkReply::NotFound;
    }
    let Some(job) = board.jobs.iter().find(|j| j.id == id) else {
        return ParkReply::NotFound;
    };
    let Some(path) = park_path(paths, &job.dir) else {
        return ParkReply::NotFound;
    };
    // A symlinked job folder must not lead outside the sdd folder.
    let (Ok(real), Ok(real_sdd)) = (path.canonicalize(), paths.sdd.canonicalize()) else {
        return ParkReply::NotFound;
    };
    if !real.starts_with(&real_sdd) {
        return ParkReply::NotFound;
    }
    let Ok(text) = std::fs::read_to_string(&real) else {
        return ParkReply::NotFound;
    };
    match serde_json::from_str::<ParkView>(&text) {
        Ok(mut view) => {
            cap_parts(&mut view);
            ParkReply::Found(view)
        }
        Err(_) => ParkReply::Broken,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn park_path_joins_the_job_folder_under_the_sdd_root() {
        let p = park_path(&Paths::for_root(Path::new("/r")), "2026-10-13-001-x");
        assert_eq!(
            p,
            Some(PathBuf::from(
                "/r/.superpowers/sdd/2026-10-13-001-x/park.json"
            ))
        );
    }

    #[test]
    fn park_path_refuses_anything_that_could_leave_the_folder() {
        for bad in ["", "..", "../x", "a/b", "a\\b", "a/../b", "x\0y", "/abs"] {
            assert_eq!(
                park_path(&Paths::for_root(Path::new("/r")), bad),
                None,
                "{bad:?}"
            );
        }
    }

    use crate::model::Board;
    use crate::paths::Paths;

    fn board_with(dir: &str) -> Board {
        let text = format!(
            r#"{{"jobs":[{{"id":"001","dir":"{dir}","slug":"s","created":"c","feature":"f","branch":"b","stage":"needs-human","round":1,"stage_since":1.0,"reviews":{{}}}}]}}"#
        );
        serde_json::from_str(&text).expect("board")
    }

    fn write_park(root: &Path, dir: &str, json: &str) {
        let folder = root.join(".superpowers/sdd").join(dir);
        std::fs::create_dir_all(&folder).expect("dir");
        std::fs::write(folder.join(PARK_FILE), json).expect("park.json");
    }

    #[test]
    fn a_valid_file_is_returned_without_unknown_fields() {
        let root = tempfile::tempdir().expect("tempdir");
        write_park(
            root.path(),
            "d",
            r#"{"kind":"checks-failed","reason":"why","at":5,"round":2,"next":["factory rework 001 <note>"],
                "secret":"x","parts":[{"title":"T","source":"s","text":"body","truncated":false,"extra":1}]}"#,
        );
        match load_park(&Paths::for_root(root.path()), &board_with("d"), "001") {
            ParkReply::Found(v) => {
                assert_eq!(v.kind, "checks-failed");
                assert_eq!(v.parts[0].text, "body");
                let json = serde_json::to_string(&v).expect("json");
                assert!(
                    !json.contains("secret") && !json.contains("extra"),
                    "{json}"
                );
            }
            _ => panic!("expected Found"),
        }
    }

    #[test]
    fn missing_file_unknown_job_and_bad_ids_are_not_found() {
        let root = tempfile::tempdir().expect("tempdir");
        let board = board_with("d");
        assert!(matches!(
            load_park(&Paths::for_root(root.path()), &board, "001"),
            ParkReply::NotFound
        ));
        write_park(root.path(), "d", "{}");
        assert!(matches!(
            load_park(&Paths::for_root(root.path()), &board, "002"),
            ParkReply::NotFound
        ));
        for bad in ["", "1", "../1", "00a", "0000000"] {
            assert!(
                matches!(
                    load_park(&Paths::for_root(root.path()), &board, bad),
                    ParkReply::NotFound
                ),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn a_board_dir_that_leaves_the_sdd_folder_is_not_found() {
        let root = tempfile::tempdir().expect("tempdir");
        write_park(root.path(), "d", "{}");
        for dir in ["..", "../d", "d/..", "a/b"] {
            assert!(
                matches!(
                    load_park(&Paths::for_root(root.path()), &board_with(dir), "001"),
                    ParkReply::NotFound
                ),
                "{dir:?}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_job_folder_that_points_outside_is_not_found() {
        let root = tempfile::tempdir().expect("tempdir");
        let outside = tempfile::tempdir().expect("outside");
        std::fs::write(
            outside.path().join(PARK_FILE),
            r#"{"kind":"other","reason":"leak"}"#,
        )
        .expect("file");
        let sdd = root.path().join(".superpowers/sdd");
        std::fs::create_dir_all(&sdd).expect("sdd");
        std::os::unix::fs::symlink(outside.path(), sdd.join("d")).expect("symlink");
        assert!(matches!(
            load_park(&Paths::for_root(root.path()), &board_with("d"), "001"),
            ParkReply::NotFound
        ));
    }

    #[test]
    fn a_broken_file_is_reported_as_broken() {
        let root = tempfile::tempdir().expect("tempdir");
        write_park(root.path(), "d", "not json");
        assert!(matches!(
            load_park(&Paths::for_root(root.path()), &board_with("d"), "001"),
            ParkReply::Broken
        ));
        write_park(root.path(), "d", r#"{"parts":"not a list"}"#);
        assert!(matches!(
            load_park(&Paths::for_root(root.path()), &board_with("d"), "001"),
            ParkReply::Broken
        ));
    }

    #[test]
    fn the_reply_text_is_capped_and_keeps_the_end() {
        let root = tempfile::tempdir().expect("tempdir");
        let big = format!("{}END", "x".repeat(MAX_REPLY_TEXT + 500));
        let json = serde_json::json!({
            "kind": "other", "reason": "r", "at": 1, "round": 1, "next": [],
            "parts": [{"title": "Big", "source": "s", "text": big, "truncated": false},
                      {"title": "Second", "source": "s", "text": "later", "truncated": false}]
        });
        write_park(root.path(), "d", &json.to_string());
        match load_park(&Paths::for_root(root.path()), &board_with("d"), "001") {
            ParkReply::Found(v) => {
                let total: usize = v.parts.iter().map(|p| p.text.len()).sum();
                assert!(total <= MAX_REPLY_TEXT, "{total}");
                assert!(v.parts[0].truncated);
                assert!(v.parts[0].text.ends_with("END"));
                assert!(v.parts[1].truncated, "the second part had no room left");
            }
            _ => panic!("expected Found"),
        }
    }
}
