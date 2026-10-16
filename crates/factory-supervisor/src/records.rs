//! Runtime records: one per running child and one per finished request.

use std::io::Read;
use std::path::{Component, Path, PathBuf};

use factory_core::project::validate_name;
use factory_core::{write_atomic, CoreError, Home};
use serde::{Deserialize, Serialize};

use crate::request::{validate_id_chars, validate_job, RequestError};

/// Records are small. Anything larger is refused.
const RECORD_CAP: u64 = 64 * 1024;
const EXT: &str = "json";

#[derive(Debug, thiserror::Error)]
pub enum RecordError {
    #[error(transparent)]
    Request(#[from] RequestError),
    #[error("bad project: {0}")]
    Project(String),
    #[error("invalid record: {0}")]
    Invalid(String),
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

/// Reads a regular file of at most `cap` bytes. Symlinks, FIFOs, devices and folders are refused
/// without being opened, so a planted file cannot block or flood the supervisor.
pub(crate) fn read_regular(path: &Path, cap: u64) -> Result<String, String> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| format!("cannot read: {e}"))?;
    if !meta.file_type().is_file() {
        return Err("not a regular file".to_string());
    }
    if meta.len() > cap {
        return Err("file too large".to_string());
    }
    let file = std::fs::File::open(path).map_err(|e| format!("cannot read: {e}"))?;
    let mut text = String::new();
    file.take(cap + 1).read_to_string(&mut text).map_err(|e| format!("cannot read: {e}"))?;
    if text.len() as u64 > cap {
        return Err("file too large".to_string());
    }
    Ok(text)
}

fn check_project(project: &str) -> Result<(), String> {
    validate_name(project).map_err(|e| e.to_string())
}

fn check_job(job: &str) -> Result<(), String> {
    validate_job(job).map_err(|e| e.to_string())
}

fn check_id(id: &str) -> Result<(), String> {
    validate_id_chars(id).map_err(|e| e.to_string())
}

fn check_path(path: &Path, what: &str) -> Result<(), String> {
    if path.is_absolute() && !path.components().any(|c| matches!(c, Component::ParentDir)) {
        Ok(())
    } else {
        Err(format!("{what} must be an absolute path without .."))
    }
}

impl ChildRecord {
    pub fn validate(&self) -> Result<(), String> {
        check_project(&self.project)?;
        check_job(&self.job)?;
        check_id(&self.request_id)?;
        if self.pid == 0 || self.pid > i32::MAX as u32 {
            return Err(format!("bad pid {}", self.pid));
        }
        check_path(&self.out, "out")?;
        check_path(&self.err, "err")
    }
}

impl ResultRecord {
    pub fn validate(&self) -> Result<(), String> {
        check_id(&self.request_id)?;
        check_project(&self.project)?;
        check_job(&self.job)
    }
}

fn child_path(home: &Home, project: &str) -> Result<PathBuf, RecordError> {
    validate_name(project).map_err(|e| RecordError::Project(e.to_string()))?;
    Ok(home.children_dir().join(format!("{project}.{EXT}")))
}

pub fn write_child(home: &Home, rec: &ChildRecord) -> Result<(), RecordError> {
    let path = child_path(home, &rec.project)?;
    rec.validate().map_err(RecordError::Invalid)?;
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

/// Reads every `*.json` file of a folder, sorted by file name. A broken, odd or oversize file is
/// an `Err` entry. Files are judged by `lstat`, so a symlink is never followed.
fn read_folder<T: serde::de::DeserializeOwned>(
    dir: &Path,
    check: impl Fn(&str, &T) -> Result<(), String>,
) -> Vec<Result<T, String>> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut names: Vec<String> =
        entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| !n.starts_with('.')).collect();
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let Some(stem) = name.strip_suffix(&format!(".{EXT}")) else {
                return Err(format!("{name}: not a .json file"));
            };
            let text = read_regular(&dir.join(&name), RECORD_CAP).map_err(|e| format!("{name}: {e}"))?;
            let rec: T = serde_json::from_str(&text).map_err(|e| format!("{name}: {e}"))?;
            check(stem, &rec).map_err(|e| format!("{name}: {e}"))?;
            Ok(rec)
        })
        .collect()
}

pub fn read_children(home: &Home) -> Vec<Result<ChildRecord, String>> {
    read_folder(&home.children_dir(), |stem, rec: &ChildRecord| {
        if rec.project != stem {
            return Err("file name does not match the project".to_string());
        }
        rec.validate()
    })
}

pub fn write_result(home: &Home, rec: &ResultRecord) -> Result<(), RecordError> {
    validate_id_chars(&rec.request_id)?;
    rec.validate().map_err(RecordError::Invalid)?;
    let path = home.results_dir().join(format!("{}.{EXT}", rec.request_id));
    let bytes = serde_json::to_vec_pretty(rec).map_err(CoreError::from)?;
    // A result that is larger than the read cap could never be read back, so it is refused.
    if bytes.len() as u64 > RECORD_CAP {
        return Err(RecordError::Invalid(format!("result is {} bytes, the cap is {RECORD_CAP}", bytes.len())));
    }
    write_atomic(&path, &bytes)?;
    Ok(())
}

pub fn read_results(home: &Home) -> Vec<Result<ResultRecord, String>> {
    read_folder(&home.results_dir(), |stem, rec: &ResultRecord| {
        if rec.request_id != stem {
            return Err("file name does not match the request id".to_string());
        }
        rec.validate()
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
        let dir = h.children_dir();
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

    #[test]
    fn child_validation_refuses_each_bad_field() {
        let ok = child("a");
        assert!(ok.validate().is_ok());
        assert!(ChildRecord { pid: 1, ..ok.clone() }.validate().is_ok());
        let bad = [
            ChildRecord { pid: 0, ..ok.clone() },
            ChildRecord { pid: i32::MAX as u32 + 1, ..ok.clone() },
            ChildRecord { project: "../x".into(), ..ok.clone() },
            ChildRecord { job: "x".into(), ..ok.clone() },
            ChildRecord { request_id: "a/b".into(), ..ok.clone() },
            ChildRecord { out: PathBuf::from("rel/out.log"), ..ok.clone() },
            ChildRecord { err: PathBuf::from("/l/../etc/err.log"), ..ok.clone() },
        ];
        for b in bad {
            assert!(b.validate().is_err(), "{b:?}");
            assert!(write_child(&Home::at(PathBuf::from("/nonexistent-never-written")), &b).is_err());
        }
    }

    #[test]
    fn result_validation_refuses_each_bad_field_on_write_and_read() {
        let (_t, h) = home();
        let ok = ResultRecord {
            request_id: "0000000000001-p-1".into(),
            project: "p".into(),
            job: "1".into(),
            outcome: Outcome::Merged,
            finished_ms: 1,
        };
        assert!(ok.validate().is_ok());
        for b in [
            ResultRecord { project: "P".into(), ..ok.clone() },
            ResultRecord { job: "1234567".into(), ..ok.clone() },
            ResultRecord { request_id: "../x".into(), ..ok.clone() },
        ] {
            assert!(write_result(&h, &b).is_err(), "{b:?}");
        }
        // A bad record planted by hand is an Err entry.
        std::fs::create_dir_all(h.results_dir()).expect("mkdir");
        let planted = ResultRecord { project: "../x".into(), ..ok };
        let text = serde_json::to_string(&planted).expect("json");
        std::fs::write(h.results_dir().join("0000000000001-p-1.json"), text).expect("write");
        assert!(read_results(&h)[0].is_err());
        // The same for a child record with pid 0.
        std::fs::create_dir_all(h.children_dir()).expect("mkdir");
        let text = serde_json::to_string(&ChildRecord { pid: 0, ..child("a") }).expect("json");
        std::fs::write(h.children_dir().join("a.json"), text).expect("write");
        assert!(read_children(&h)[0].is_err());
    }

    #[cfg(unix)]
    #[test]
    fn odd_and_oversize_files_are_err_entries_and_listing_returns() {
        let (t, h) = home();
        write_child(&h, &child("a")).expect("write");
        let dir = h.children_dir();
        let target = t.path().join("elsewhere.json");
        std::fs::write(&target, serde_json::to_string(&child("b")).expect("json")).expect("write");
        std::os::unix::fs::symlink(&target, dir.join("b.json")).expect("symlink");
        std::os::unix::fs::symlink("/dev/zero", dir.join("c.json")).expect("symlink");
        std::fs::write(dir.join("d.json"), vec![b' '; 70 * 1024]).expect("write");
        match std::process::Command::new("mkfifo").arg(dir.join("e.json")).status() {
            Ok(s) if s.success() => {}
            _ => eprintln!("mkfifo is missing, the FIFO case is skipped"),
        }
        let got = read_children(&h);
        assert!(got[0].is_ok());
        assert!(got[1..].iter().all(|r| r.is_err()), "{got:?}");
        assert!(got.len() >= 4);
    }
}
