//! The checks a request must pass before it is queued. The HTTP API and `factoryd enqueue --retry`
//! share them, because the queued child would refuse a bad request and nobody reads that refusal.

use std::fmt;

use factory_core::project::{self, ProjectFile};
use factory_core::{normalize_id, Board, Home, Layout, ResumeAt, Stage};

use crate::request::{validate_job, Action, Request};
use crate::{queue, QueueError};

/// Why a request was not queued. The API maps each kind to its status code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The input is malformed (400 `bad_request`).
    BadRequest(String),
    /// The project or job does not exist (404 `not_found`).
    NotFound(String),
    /// The project cannot take work (409 `project_disabled`).
    Disabled(String),
    /// The job is in a state that does not allow the request (409 `conflict`).
    Conflict(String),
    /// Something broke while checking or queueing (500 `failed`).
    Failed(String),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::BadRequest(m)
            | Refusal::NotFound(m)
            | Refusal::Disabled(m)
            | Refusal::Conflict(m)
            | Refusal::Failed(m) => f.write_str(m),
        }
    }
}

/// The layout of a project that can take work. A missing project is `NotFound`, a broken one `Disabled`.
pub fn project_layout(home: &Home, name: &str) -> Result<Layout, Refusal> {
    project::validate_name(name).map_err(|e| Refusal::BadRequest(e.to_string()))?;
    if !home.project_dir(name).is_dir() {
        return Err(Refusal::NotFound(format!("no project named {name}")));
    }
    let disabled = |why: String| Refusal::Disabled(why);
    let file: ProjectFile = project::get(home, name).map_err(|e| disabled(e.to_string()))?;
    let layout = project::layout(home, name, &file).map_err(|e| disabled(e.to_string()))?;
    if !file.repo.is_dir() {
        return Err(disabled("repo path is missing".to_string()));
    }
    Ok(layout)
}

/// A job id checked and padded, such as `1` to `001`.
pub fn job_id(raw: &str) -> Result<String, Refusal> {
    validate_job(raw).map_err(|e| Refusal::BadRequest(e.to_string()))?;
    normalize_id(raw).map_err(|e| Refusal::BadRequest(e.to_string()))
}

/// Refuses when an approve or retry request for the job is already queued or running.
pub fn refuse_if_queued(home: &Home, name: &str, id: &str) -> Result<(), Refusal> {
    // The queue entry stays until the child finishes, so this also covers a running request.
    let twice = queue::list(home).iter().any(|item| {
        matches!(item, queue::QueueItem::Ok(r)
            if r.project == name && r.job == id && matches!(r.action, Action::Approve | Action::Retry))
    });
    if twice {
        return Err(Refusal::Conflict(format!(
            "job {id} already has an approve or retry queued"
        )));
    }
    Ok(())
}

/// Adds a request to the queue. A duplicate request id is a `Conflict`.
pub fn enqueue(home: &Home, req: &Request) -> Result<(), Refusal> {
    queue::enqueue(home, req).map_err(|e| match e {
        QueueError::Duplicate(id) => Refusal::Conflict(format!("request {id} is already queued")),
        other => Refusal::Failed(other.to_string()),
    })
}

/// Checks that job `raw_id` of project `name` can be retried, then queues a retry request.
/// Returns the request id.
pub fn queue_retry(home: &Home, name: &str, raw_id: &str, now_ms: u64) -> Result<String, Refusal> {
    let layout = project_layout(home, name)?;
    let id = job_id(raw_id)?;
    let board = Board::load(&layout.board_file())
        .map_err(|e| Refusal::Failed(format!("the board of this project cannot be read: {e}")))?;
    let job = board
        .jobs
        .iter()
        .find(|j| j.id == id)
        .ok_or_else(|| Refusal::NotFound(format!("no job {id} in project {name}")))?;
    let conflict = |why: String| Refusal::Conflict(why);
    if job.cancelled {
        return Err(conflict(format!("job {id} was cancelled")));
    }
    if job.stage != Stage::NeedsHuman {
        return Err(conflict(format!(
            "job {id} is in stage {}, retry needs needs-human",
            job.stage
        )));
    }
    if job.batch_snapshot.is_some() {
        return Err(conflict(format!(
            "job {id} has a pending batch commit, use rework to finish it"
        )));
    }
    let Some(resume) = job.park_resume else {
        return Err(conflict(format!(
            "job {id} has no resume stage, see its park for the next command"
        )));
    };
    // An approval given before the park carries over, as in the engine.
    if matches!(resume, ResumeAt::Merge | ResumeAt::Sync)
        && job.stage_before_park() != Some(Stage::Approve)
    {
        if let Some(why) = job.approve_refusal(false, false) {
            return Err(conflict(why));
        }
    }
    refuse_if_queued(home, name, &id)?;
    let req = Request::new(name, Action::Retry, &id, false, now_ms)
        .map_err(|e| Refusal::BadRequest(e.to_string()))?;
    enqueue(home, &req)?;
    Ok(req.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::testutil::{fixture, job_json, write_board};

    const RESUME_MERGE: &str = r#""park_resume":"merge","park_kind":"other","stage_history":[{"stage":"approve","at":1.0},{"stage":"needs-human","at":2.0}]"#;

    fn parked(home: &Home, extra: &str) {
        let job = job_json("001", "001-first").replace(
            r#""stage":"build""#,
            &format!(r#""stage":"needs-human",{extra}"#),
        );
        write_board(home, &format!(r#"{{"jobs":[{job}]}}"#));
    }

    #[test]
    fn queue_retry_queues_one_retry_and_refuses_a_second() {
        let (_t, home) = fixture();
        parked(&home, RESUME_MERGE);
        let id = queue_retry(&home, "good", "1", 1).expect("queued");
        let items = queue::list(&home);
        assert_eq!(items.len(), 1);
        match &items[0] {
            queue::QueueItem::Ok(req) => {
                assert_eq!(
                    (req.action, req.job.as_str(), req.id.as_str()),
                    (Action::Retry, "001", id.as_str())
                );
            }
            other => panic!("{other:?}"),
        }
        let again = queue_retry(&home, "good", "001", 2).expect_err("refused");
        assert_eq!(
            again,
            Refusal::Conflict("job 001 already has an approve or retry queued".into())
        );
        assert_eq!(queue::list(&home).len(), 1);
    }

    #[test]
    fn queue_retry_refuses_a_park_without_resume() {
        let (_t, home) = fixture();
        parked(&home, r#""park_kind":"other""#);
        let err = queue_retry(&home, "good", "1", 1).expect_err("refused");
        assert_eq!(
            err,
            Refusal::Conflict(
                "job 001 has no resume stage, see its park for the next command".into()
            )
        );
        assert!(queue::list(&home).is_empty());
    }

    #[test]
    fn queue_retry_refuses_an_unknown_project_and_job() {
        let (_t, home) = fixture();
        parked(&home, RESUME_MERGE);
        assert_eq!(
            queue_retry(&home, "nope", "1", 1),
            Err(Refusal::NotFound("no project named nope".into()))
        );
        assert_eq!(
            queue_retry(&home, "good", "2", 1),
            Err(Refusal::NotFound("no job 002 in project good".into()))
        );
        assert!(matches!(
            queue_retry(&home, "broken", "1", 1),
            Err(Refusal::Disabled(_))
        ));
        assert!(queue::list(&home).is_empty());
    }
}
