//! Runtime records: one per running child and one per finished request.

use std::path::{Path, PathBuf};

use factory_core::project::validate_name;
use factory_core::{write_atomic, CoreError, Home};
use serde::{Deserialize, Serialize};

use crate::request::{validate_id_chars, RequestError};

const CHILDREN_DIR: &str = "children";
const EXT: &str = "json";

#[derive(Debug, thiserror::Error)]
pub enum RecordError {
    #[error(transparent)]
    Request(#[from] RequestError),
    #[error("bad project: {0}")]
    Project(String),
    #[error(transparent)]
    Core(#[from] CoreError),
}

/// A child process the supervisor started. Stored at `run/children/<project>.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChildRecord {
    pub pid: u32,
    pub project: String,
    pub job: String,
    pub request_id: String,
    pub started_ms: u64,
    pub out: PathBuf,
    pub err: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Outcome {
    Merged,
    NeedsHuman { reason: String },
    Failed { code: Option<i32>, stderr_tail: String },
    Lost { reason: String },
    Rejected { reason: String },
}

/// How a request ended. Stored at `results/<request id>.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResultRecord {
    pub request_id: String,
    pub project: String,
    pub job: String,
    pub outcome: Outcome,
    pub finished_ms: u64,
}

fn children_dir(home: &Home) -> PathBuf {
    home.run_dir().join(CHILDREN_DIR)
}

fn child_path(home: &Home, project: &str) -> Result<PathBuf, RecordError> {
    validate_name(project).map_err(|e| RecordError::Project(e.to_string()))?;
    Ok(children_dir(home).join(format!("{project}.{EXT}")))
}

pub fn write_child(home: &Home, rec: &ChildRecord) -> Result<(), RecordError> {
    let path = child_path(home, &rec.project)?;
    let bytes = serde_json::to_vec_pretty(rec).map_err(CoreError::from)?;
    write_atomic(&path, &bytes)?;
    Ok(())
}

pub fn remove_child(home: &Home, project: &str) -> Result<(), RecordError> {
    let path = child_path(home, project)?;
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(CoreError::io(&path, e).into()),
    }
}

/// Reads every `*.json` file of a folder, sorted by file name. A broken file is an `Err` entry.
fn read_folder<T: serde::de::DeserializeOwned>(
    dir: &Path,
    check: impl Fn(&str, &T) -> Result<(), String>,
) -> Vec<Result<T, String>> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('.'))
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let Some(stem) = name.strip_suffix(&format!(".{EXT}")) else {
                return Err(format!("{name}: not a .json file"));
            };
            let text = std::fs::read_to_string(dir.join(&name)).map_err(|e| format!("{name}: cannot read: {e}"))?;
            let rec: T = serde_json::from_str(&text).map_err(|e| format!("{name}: {e}"))?;
            check(stem, &rec).map_err(|e| format!("{name}: {e}"))?;
            Ok(rec)
        })
        .collect()
}

pub fn read_children(home: &Home) -> Vec<Result<ChildRecord, String>> {
    read_folder(&children_dir(home), |stem, rec: &ChildRecord| {
        if rec.project == stem {
            Ok(())
        } else {
            Err("file name does not match the project".to_string())
        }
    })
}

pub fn write_result(home: &Home, rec: &ResultRecord) -> Result<(), RecordError> {
    validate_id_chars(&rec.request_id)?;
    let path = home.results_dir().join(format!("{}.{EXT}", rec.request_id));
    let bytes = serde_json::to_vec_pretty(rec).map_err(CoreError::from)?;
    write_atomic(&path, &bytes)?;
    Ok(())
}

pub fn read_results(home: &Home) -> Vec<Result<ResultRecord, String>> {
    read_folder(&home.results_dir(), |stem, rec: &ResultRecord| {
        if rec.request_id == stem {
            Ok(())
        } else {
            Err("file name does not match the request id".to_string())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> (tempfile::TempDir, Home) {
        let t = tempfile::tempdir().expect("tempdir");
        let h = Home::at(t.path().join("home"));
        (t, h)
    }

    fn child(project: &str) -> ChildRecord {
        ChildRecord {
            pid: 42,
            project: project.into(),
            job: "1".into(),
            request_id: format!("0000000000001-{project}-1"),
            started_ms: 7,
            out: PathBuf::from("/l/out.log"),
            err: PathBuf::from("/l/err.log"),
        }
    }

    #[test]
    fn child_records_round_trip_and_are_removed() {
        let (_t, h) = home();
        write_child(&h, &child("b")).expect("write");
        write_child(&h, &child("a")).expect("write");
        let got: Vec<ChildRecord> = read_children(&h).into_iter().map(|r| r.expect("ok")).collect();
        assert_eq!(got, vec![child("a"), child("b")]);
        assert!(h.run_dir().join("children/a.json").is_file());
        remove_child(&h, "a").expect("remove");
        remove_child(&h, "a").expect("removing twice is fine");
        assert_eq!(read_children(&h).len(), 1);
        assert!(write_child(&h, &child("../x")).is_err());
        assert!(remove_child(&h, "../x").is_err());
    }

    #[test]
    fn a_broken_child_file_is_an_err_entry_not_a_failure() {
        let (_t, h) = home();
        write_child(&h, &child("a")).expect("write");
        let dir = h.run_dir().join("children");
        std::fs::write(dir.join("junk.json"), "nope").expect("write");
        let mismatch = serde_json::to_string(&child("zed")).expect("json");
        std::fs::write(dir.join("other.json"), mismatch).expect("write");
        let got = read_children(&h);
        assert_eq!(got.len(), 3);
        assert_eq!(got.iter().filter(|r| r.is_ok()).count(), 1);
        assert_eq!(got.iter().filter(|r| r.is_err()).count(), 2);
    }

    #[test]
    fn results_round_trip_for_every_outcome_and_use_the_request_id_as_name() {
        let (_t, h) = home();
        let outcomes = [
            Outcome::Merged,
            Outcome::NeedsHuman { reason: "r".into() },
            Outcome::Failed { code: Some(2), stderr_tail: "t".into() },
            Outcome::Failed { code: None, stderr_tail: String::new() },
            Outcome::Lost { reason: "gone".into() },
            Outcome::Rejected { reason: "no".into() },
        ];
        for (i, outcome) in outcomes.iter().enumerate() {
            let rec = ResultRecord {
                request_id: format!("{i:013}-p-1"),
                project: "p".into(),
                job: "1".into(),
                outcome: outcome.clone(),
                finished_ms: 9,
            };
            write_result(&h, &rec).expect("write");
            assert!(h.results_dir().join(format!("{}.json", rec.request_id)).is_file());
        }
        let got: Vec<ResultRecord> = read_results(&h).into_iter().map(|r| r.expect("ok")).collect();
        let back: Vec<Outcome> = got.into_iter().map(|r| r.outcome).collect();
        assert_eq!(back, outcomes);
    }

    #[test]
    fn an_unsafe_result_id_is_refused() {
        let (_t, h) = home();
        let rec = ResultRecord {
            request_id: "../x".into(),
            project: "p".into(),
            job: "1".into(),
            outcome: Outcome::Merged,
            finished_ms: 1,
        };
        assert!(write_result(&h, &rec).is_err());
    }
}
