//! A request to run or approve a job. Request files are same-user writable, so nothing here is trusted.

use factory_core::project::validate_name;
use serde::{Deserialize, Serialize};

const MAX_JOB_DIGITS: usize = 6;
const MAX_ID: usize = 120;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RequestError {
    #[error("bad project: {0}")]
    Project(String),
    #[error("job must be 1 to {MAX_JOB_DIGITS} ASCII digits, got {0:?}")]
    Job(String),
    #[error("series is only valid for a run request")]
    SeriesNotAllowed,
    #[error("bad request id {0:?}")]
    Id(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Run,
    Approve,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub id: String,
    pub project: String,
    pub action: Action,
    pub job: String,
    pub series: bool,
    pub ts_ms: u64,
}

/// The id of a request: `<ts_ms>-<project>-<job>` with a 13 digit timestamp.
pub fn make_id(ts_ms: u64, project: &str, job: &str) -> String {
    format!("{ts_ms:013}-{project}-{job}")
}

/// Checks that an id can be used as a file name: lowercase letters, digits and `-` only.
pub fn validate_id_chars(id: &str) -> Result<(), RequestError> {
    let ok = !id.is_empty()
        && id.len() <= MAX_ID
        && id.chars().next().is_some_and(|c| c.is_ascii_digit())
        && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if ok {
        Ok(())
    } else {
        Err(RequestError::Id(id.to_string()))
    }
}

pub fn validate_job(job: &str) -> Result<(), RequestError> {
    if !job.is_empty() && job.len() <= MAX_JOB_DIGITS && job.chars().all(|c| c.is_ascii_digit()) {
        Ok(())
    } else {
        Err(RequestError::Job(job.to_string()))
    }
}

impl Request {
    pub fn new(project: &str, action: Action, job: &str, series: bool, now_ms: u64) -> Result<Request, RequestError> {
        let req = Request {
            id: make_id(now_ms, project, job),
            project: project.to_string(),
            action,
            job: job.to_string(),
            series,
            ts_ms: now_ms,
        };
        req.validate()?;
        Ok(req)
    }

    pub fn validate(&self) -> Result<(), RequestError> {
        validate_name(&self.project).map_err(|e| RequestError::Project(e.to_string()))?;
        validate_job(&self.job)?;
        if self.series && self.action != Action::Run {
            return Err(RequestError::SeriesNotAllowed);
        }
        validate_id_chars(&self.id)?;
        if self.id != make_id(self.ts_ms, &self.project, &self.job) {
            return Err(RequestError::Id(self.id.clone()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn good_requests_are_accepted() {
        let r = Request::new("rocket", Action::Run, "1", true, 5).expect("ok");
        assert_eq!(r.id, "0000000000005-rocket-1");
        assert!(Request::new("rocket-vault", Action::Approve, "003", false, 99).is_ok());
    }

    #[test]
    fn bad_projects_jobs_and_series_are_refused() {
        for p in ["", "A", "../x", "a/b"] {
            assert!(Request::new(p, Action::Run, "1", false, 1).is_err(), "{p:?}");
        }
        for j in ["", "abc", "1/2", "1234567", "-1"] {
            assert!(Request::new("p", Action::Run, j, false, 1).is_err(), "{j:?}");
        }
        assert_eq!(Request::new("p", Action::Approve, "1", true, 1), Err(RequestError::SeriesNotAllowed));
    }

    #[test]
    fn a_hand_made_id_must_match_and_be_path_safe() {
        let good = Request::new("p", Action::Run, "1", false, 1).expect("ok");
        for id in ["", "x", "a/b", "../x", "0000000000001-p-1/..", "0000000000002-p-1", "0000000000001-p-1.json"] {
            let r = Request { id: id.to_string(), ..good.clone() };
            assert!(r.validate().is_err(), "{id:?}");
        }
        let r = Request { id: "a/b".into(), ..good };
        assert!(validate_id_chars(&r.id).is_err());
    }

    const GOOD_JSON: &str =
        r#"{"id":"0000000000001-p-1","project":"p","action":"run","job":"1","series":false,"ts_ms":1}"#;

    #[test]
    fn json_with_unknown_or_missing_fields_is_refused() {
        let ok: Request = serde_json::from_str(GOOD_JSON).expect("parse");
        assert!(ok.validate().is_ok());
        let extra = GOOD_JSON.replace("\"ts_ms\":1", "\"ts_ms\":1,\"cmd\":\"x\"");
        assert!(serde_json::from_str::<Request>(&extra).is_err());
        let missing = GOOD_JSON.replace(",\"series\":false", "");
        assert!(serde_json::from_str::<Request>(&missing).is_err());
    }
}
