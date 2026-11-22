//! The checks a request must pass before it is queued. The HTTP API and `factoryd enqueue`
//! share them, because the queued child would refuse a bad request and nobody reads that refusal.

use std::fmt;

use factory_core::project::{self, ProjectFile};
use factory_core::{normalize_id, Board, Home, Layout, ResumeAt, Stage};

use crate::request::{validate_job, Action, Request, INTAKE_JOB};
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
    let id = normalize_id(raw).map_err(|e| Refusal::BadRequest(e.to_string()))?;
    if id == INTAKE_JOB {
        return Err(Refusal::BadRequest(format!(
            "job {INTAKE_JOB} is not a job, ids start at 001"
        )));
    }
    Ok(id)
}

/// True when a queued or running request of one of `actions` exists for the padded job `id`.
/// An old entry stored with an unpadded job id matches too.
fn is_queued(home: &Home, name: &str, id: &str, actions: &[Action]) -> bool {
    queue::list(home).iter().any(|item| {
        matches!(item, queue::QueueItem::Ok(r)
            if r.project == name
                && actions.contains(&r.action)
                && normalize_id(&r.job).is_ok_and(|j| j == id))
    })
}

/// Refuses when an approve or retry request for the job is already queued or running.
pub fn refuse_if_queued(home: &Home, name: &str, id: &str) -> Result<(), Refusal> {
    // The queue entry stays until the child finishes, so this also covers a running request.
    if is_queued(home, name, id, &[Action::Approve, Action::Retry]) {
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

/// Queues a run request for job `raw_id` of project `name`. A run blocks only another run, and the
/// queue entry stays while the child runs, so a running run blocks too. The board is not read: the
/// child reports a job that does not exist. Returns the request id.
pub fn queue_run(
    home: &Home,
    name: &str,
    raw_id: &str,
    series: bool,
    now_ms: u64,
) -> Result<String, Refusal> {
    project_layout(home, name)?;
    let id = job_id(raw_id)?;
    if is_queued(home, name, &id, &[Action::Run]) {
        return Err(Refusal::Conflict(format!(
            "job {id} already has a run queued"
        )));
    }
    let req = Request::new(name, Action::Run, &id, series, now_ms)
        .map_err(|e| Refusal::BadRequest(e.to_string()))?;
    enqueue(home, &req)?;
    Ok(req.id)
}

/// Checks that job `raw_id` of project `name` can be approved, then queues an approve request.
/// The child would refuse a bad approve and nobody reads that refusal, so the checks run here.
/// Returns the request id.
pub fn queue_approve(
    home: &Home,
    name: &str,
    raw_id: &str,
    without_review: bool,
    waive_reason: Option<&str>,
    now_ms: u64,
) -> Result<String, Refusal> {
    let layout = project_layout(home, name)?;
    let id = job_id(raw_id)?;
    let board = Board::load(&layout.board_file())
        .map_err(|e| Refusal::Failed(format!("the board of this project cannot be read: {e}")))?;
    let job = board
        .jobs
        .iter()
        .find(|j| j.id == id)
        .ok_or_else(|| Refusal::NotFound(format!("no job {id} in project {name}")))?;
    if job.cancelled {
        return Err(Refusal::Conflict(format!("job {id} was cancelled")));
    }
    if job.stage != Stage::NeedsHuman && job.stage != Stage::Approve {
        return Err(Refusal::Conflict(format!(
            "job {id} is in stage {}, approve needs needs-human or approve",
            job.stage
        )));
    }
    // The review rules inside apply to needs-human only, the open-changes rule to both stages.
    if let Some(why) = job.approve_refusal(without_review, waive_reason.is_some()) {
        return Err(Refusal::Conflict(why));
    }
    refuse_if_queued(home, name, &id)?;
    let bad = |e: crate::request::RequestError| Refusal::BadRequest(e.to_string());
    let mut req = Request::new(name, Action::Approve, &id, false, now_ms).map_err(bad)?;
    if without_review {
        req = req.without_review().map_err(bad)?;
    }
    if let Some(reason) = waive_reason {
        req = req.waive(reason).map_err(bad)?;
    }
    enqueue(home, &req)?;
    Ok(req.id)
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
    // A machine park comes before the batch recovery, so its retry reruns the gate and then the recovery.
    if job.batch_snapshot.is_some() && job.park_kind.as_deref() != Some("machine-waiting") {
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

    fn jobs(home: &Home) -> Vec<(Action, String)> {
        queue::list(home)
            .into_iter()
            .map(|item| match item {
                queue::QueueItem::Ok(r) => (r.action, r.job.clone()),
                other => panic!("{other:?}"),
            })
            .collect()
    }

    fn board(home: &Home, stage: &str, cancelled: bool, review: &str) {
        let job = job_json("001", "001-first")
            .replace(
                r#""stage":"build""#,
                &format!(r#""stage":"{stage}","cancelled":{cancelled}"#),
            )
            .replace(
                r#""reviews":{}"#,
                &format!(r#""reviews":{{"code":"{review}"}}"#),
            );
        write_board(home, &format!(r#"{{"jobs":[{job}]}}"#));
    }

    const TWICE: &str = "job 001 already has an approve or retry queued";

    #[test]
    fn queue_approve_pads_and_refuses_a_second_approve() {
        let (_t, home) = fixture();
        board(&home, "approve", false, "pass");
        queue_approve(&home, "good", "1", false, None, 1).expect("queued");
        let err = queue_approve(&home, "good", "001", false, None, 2).expect_err("refused");
        assert_eq!(err, Refusal::Conflict(TWICE.into()));
        assert_eq!(jobs(&home), vec![(Action::Approve, "001".to_string())]);
    }

    #[test]
    fn queue_retry_refuses_after_an_unpadded_approve() {
        let (_t, home) = fixture();
        parked(&home, RESUME_MERGE);
        queue_approve(&home, "good", "1", true, None, 1).expect("queued");
        let err = queue_retry(&home, "good", "1", 2).expect_err("refused");
        assert_eq!(err, Refusal::Conflict(TWICE.into()));
        assert_eq!(queue::list(&home).len(), 1);
    }

    #[test]
    fn queue_approve_refuses_an_approve_after_a_retry() {
        let (_t, home) = fixture();
        parked(&home, RESUME_MERGE);
        queue_retry(&home, "good", "1", 1).expect("queued");
        let err = queue_approve(&home, "good", "1", true, None, 2).expect_err("refused");
        assert_eq!(err, Refusal::Conflict(TWICE.into()));
        assert_eq!(queue::list(&home).len(), 1);
    }

    #[test]
    fn queue_run_and_approve_do_not_block_each_other() {
        let (_t, home) = fixture();
        board(&home, "approve", false, "pass");
        queue_run(&home, "good", "1", false, 1).expect("queued");
        queue_approve(&home, "good", "1", false, None, 2).expect("queued");
        assert_eq!(queue::list(&home).len(), 2);

        let (_t2, home2) = fixture();
        board(&home2, "approve", false, "pass");
        queue_approve(&home2, "good", "1", false, None, 1).expect("queued");
        queue_run(&home2, "good", "1", false, 2).expect("queued");
        assert_eq!(queue::list(&home2).len(), 2);
    }

    #[test]
    fn queue_approve_checks_the_board() {
        let (_t, home) = fixture();
        board(&home, "build", false, "pass");
        assert_eq!(
            queue_approve(&home, "good", "1", false, None, 1),
            Err(Refusal::Conflict(
                "job 001 is in stage build, approve needs needs-human or approve".into()
            ))
        );
        board(&home, "approve", true, "pass");
        assert_eq!(
            queue_approve(&home, "good", "1", false, None, 1),
            Err(Refusal::Conflict("job 001 was cancelled".into()))
        );
        board(&home, "approve", false, "pass");
        assert_eq!(
            queue_approve(&home, "good", "2", false, None, 1),
            Err(Refusal::NotFound("no job 002 in project good".into()))
        );
        board(&home, "needs-human", false, "pending");
        let layout = project_layout(&home, "good").expect("layout");
        let loaded = Board::load(&layout.board_file()).expect("board");
        let why = loaded.jobs[0]
            .approve_refusal(false, false)
            .expect("refused");
        assert_eq!(
            queue_approve(&home, "good", "1", false, None, 1),
            Err(Refusal::Conflict(why))
        );
        assert!(queue::list(&home).is_empty());
    }

    #[test]
    fn queue_approve_passes_the_flags() {
        let (_t, home) = fixture();
        board(&home, "needs-human", false, "pending");
        queue_approve(&home, "good", "1", true, None, 1).expect("queued");
        match &queue::list(&home)[0] {
            queue::QueueItem::Ok(r) => assert!(r.without_review),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn queue_run_refuses_a_second_run() {
        let (_t, home) = fixture();
        queue_run(&home, "good", "1", false, 1).expect("queued");
        let err = queue_run(&home, "good", "001", false, 2).expect_err("refused");
        assert_eq!(
            err,
            Refusal::Conflict("job 001 already has a run queued".into())
        );
        assert_eq!(jobs(&home), vec![(Action::Run, "001".to_string())]);
    }

    #[test]
    fn job_id_refuses_job_zero() {
        for raw in ["0", "00", "000"] {
            assert_eq!(
                job_id(raw).expect_err("refused"),
                Refusal::BadRequest("job 000 is not a job, ids start at 001".into())
            );
        }
        assert_eq!(job_id("1").expect("ok"), "001");
    }

    #[test]
    fn refuse_if_queued_matches_an_unpadded_entry() {
        let (_t, home) = fixture();
        parked(&home, RESUME_MERGE);
        queue::enqueue(
            &home,
            &Request::new("good", Action::Approve, "1", false, 1).expect("req"),
        )
        .expect("enqueue");
        let err = queue_retry(&home, "good", "1", 2).expect_err("refused");
        assert_eq!(
            err,
            Refusal::Conflict("job 001 already has an approve or retry queued".into())
        );
        assert_eq!(queue::list(&home).len(), 1);
    }

    #[test]
    fn queue_run_pads_the_job_id() {
        let (_t, home) = fixture();
        queue_run(&home, "good", "0001", true, 1).expect("queued");
        assert_eq!(jobs(&home), vec![(Action::Run, "001".to_string())]);
        match &queue::list(&home)[0] {
            queue::QueueItem::Ok(r) => assert!(r.series),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn queue_run_does_not_block_an_approve_or_retry() {
        let (_t, home) = fixture();
        parked(&home, RESUME_MERGE);
        queue_run(&home, "good", "1", false, 1).expect("queued");
        queue_retry(&home, "good", "1", 2).expect("queued");
        assert_eq!(queue::list(&home).len(), 2);
    }

    #[test]
    fn queue_run_refuses_an_unknown_project_and_a_bad_id() {
        let (_t, home) = fixture();
        assert_eq!(
            queue_run(&home, "nope", "1", false, 1),
            Err(Refusal::NotFound("no project named nope".into()))
        );
        for bad in ["abc", "", "1234567"] {
            assert_eq!(
                queue_run(&home, "good", bad, false, 1),
                Err(Refusal::BadRequest(
                    validate_job(bad).expect_err("bad").to_string()
                ))
            );
        }
        assert!(queue::list(&home).is_empty());
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
