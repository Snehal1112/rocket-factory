//! The read endpoints: the project list and the jobs of one project.

use factory_core::project::{self, ProjectFile};
use factory_core::{resolve, Board, Config, CoreError, Home, Layout};
use factory_engine::{project_lease_status, ProjectLeaseStatus, RepoTip};
use factory_git::Repo;
use serde_json::{json, Value};

use crate::api::http::{error_response, ApiResponse};
use crate::api::ApiDeps;
use crate::{queue, records, QueueItem};

const LAST_RESULTS: usize = 10;

fn base_of(file: &ProjectFile) -> String {
    file.base_branch().unwrap_or_else(|| Config::default().base_branch)
}

/// Opens the repo at exactly `path`, like `factory project list` does.
fn open_repo(path: &std::path::Path, base: &str) -> Result<Repo, String> {
    let repo = Repo::open(path).map_err(|e| format!("not a git repository: {e}"))?;
    if std::fs::canonicalize(repo.root()).ok() != std::fs::canonicalize(path).ok() {
        return Err(format!("{} is not the top folder of a git repository", path.display()));
    }
    Ok(repo.with_base_branch(base))
}

fn disabled(name: &str, reason: &str) -> Value {
    json!({
        "name": name, "repo": null, "preset": null, "baseBranch": null, "status": "disabled",
        "reason": reason, "stateDir": null, "jobsDir": null, "activeJob": null,
    })
}

fn describe(home: &Home, name: &str, file: &ProjectFile, now_secs: f64) -> Value {
    let base = base_of(file);
    let layout: Layout = match project::layout(home, name, file) {
        Ok(l) => l,
        Err(e) => return disabled(name, &e.to_string()),
    };
    let mut v = json!({
        "name": name,
        "repo": file.repo,
        "preset": file.preset,
        "baseBranch": base,
        "status": "ready",
        "reason": null,
        "stateDir": layout.state_dir,
        "jobsDir": layout.jobs_dir,
        "activeJob": null,
    });
    let set = |v: &mut Value, status: &str, reason: String| {
        v["status"] = json!(status);
        v["reason"] = json!(reason);
    };
    if !file.repo.is_dir() {
        set(&mut v, "disabled", "repo path is missing".to_string());
        return v;
    }
    let repo = match open_repo(&file.repo, &base) {
        Ok(r) => r,
        Err(why) => {
            set(&mut v, "disabled", why);
            return v;
        }
    };
    if let Ok(resolved) = resolve(home, name, &RepoTip(&repo)) {
        if resolved.config.degraded {
            let why = resolved.problems.first().cloned().unwrap_or_else(|| "the settings failed closed".to_string());
            set(&mut v, "ready", format!("degraded: {why}"));
        }
    }
    if let ProjectLeaseStatus::Held { owner, job, .. } = project_lease_status(&layout.state_dir, now_secs) {
        set(&mut v, "busy", format!("job {job} is running (leased by {owner})"));
        v["activeJob"] = json!(job);
    }
    v
}

/// `GET /projects`.
pub fn projects(home: &Home, deps: &ApiDeps) -> ApiResponse {
    let entries = match project::list(home) {
        Ok(e) => e,
        Err(e) => return error_response(500, "failed", &e.to_string()),
    };
    let now_secs = (deps.now_ms)() as f64 / 1000.0;
    let list: Vec<Value> = entries
        .iter()
        .map(|e| match &e.file {
            Err(why) => disabled(&e.name, why),
            Ok(f) => describe(home, &e.name, f, now_secs),
        })
        .collect();
    ApiResponse::ok(Value::Array(list))
}

/// `GET /projects/{name}/jobs`.
pub fn jobs(home: &Home, name: &str) -> ApiResponse {
    if let Err(e) = project::validate_name(name) {
        return error_response(400, "bad_request", &e.to_string());
    }
    if !home.project_dir(name).is_dir() {
        return error_response(404, "not_found", &format!("no project named {name}"));
    }
    let file = match project::get(home, name) {
        Ok(f) => f,
        Err(e) => return error_response(500, "failed", &e.to_string()),
    };
    let layout = match project::layout(home, name, &file) {
        Ok(l) => l,
        Err(e) => return error_response(500, "failed", &e.to_string()),
    };
    let board = match Board::load(&layout.board_file()) {
        Ok(b) => b,
        Err(e) => return error_response(500, "failed", &board_message(&e)),
    };
    let jobs: Vec<Value> = board
        .jobs
        .iter()
        .map(|j| {
            json!({
                "id": j.id, "slug": j.slug, "feature": j.feature, "stage": j.stage, "branch": j.branch,
                "tokens": j.tokens.weighted(),
                "series": j.series.as_ref().map(|s| json!({"id": s.id, "index": s.index, "total": s.total})),
                "cancelled": j.cancelled,
            })
        })
        .collect();
    let queued: Vec<Value> = queue::list(home)
        .into_iter()
        .filter_map(|item| match item {
            QueueItem::Ok(r) if r.project == name => {
                Some(json!({"id": r.id, "job": r.job, "action": r.action, "series": r.series}))
            }
            _ => None,
        })
        .collect();
    let mut results: Vec<_> = records::read_results(home).into_iter().flatten().filter(|r| r.project == name).collect();
    results.sort_by(|a, b| b.finished_ms.cmp(&a.finished_ms).then_with(|| b.request_id.cmp(&a.request_id)));
    let last: Vec<Value> = results
        .into_iter()
        .take(LAST_RESULTS)
        .map(|r| json!({"requestId": r.request_id, "job": r.job, "outcome": r.outcome, "finishedMs": r.finished_ms}))
        .collect();
    ApiResponse::ok(json!({"jobs": jobs, "queued": queued, "lastResults": last}))
}

fn board_message(e: &CoreError) -> String {
    format!("the board of this project cannot be read: {e}")
}

#[cfg(test)]
mod tests {
    use crate::api::http::route;
    use crate::api::testutil::*;

    fn get(home: &factory_core::Home, path: &str) -> crate::api::http::ApiResponse {
        route(home, &deps(), "GET", path, "", &headers(Some(TOKEN), "localhost:1"), b"")
    }

    #[test]
    fn projects_lists_good_and_disabled() {
        let (_t, home) = fixture();
        let r = get(&home, "/projects");
        assert_eq!(r.status, 200);
        let list = r.body.as_array().expect("array");
        assert_eq!(list.len(), 2);
        let broken = &list[0];
        assert_eq!(broken["name"], "broken");
        assert_eq!(broken["status"], "disabled");
        assert!(broken["reason"].is_string());
        let good = &list[1];
        assert_eq!(good["name"], "good");
        assert_eq!(good["status"], "ready");
        assert_eq!(good["baseBranch"], "trunk");
        assert_eq!(good["preset"], "rust");
        assert!(good["stateDir"].as_str().expect("state").ends_with("projects/good/state"));
        assert!(good["jobsDir"].as_str().expect("jobs").ends_with("projects/good/state/jobs"));
        assert!(good["activeJob"].is_null());
    }

    #[test]
    fn job_names_are_checked() {
        let (_t, home) = fixture();
        assert_eq!(get(&home, "/projects/ghost/jobs").body["error"]["code"], "not_found");
        assert_eq!(get(&home, "/projects/Bad..Name/jobs").body["error"]["code"], "bad_request");
    }

    #[test]
    fn jobs_come_from_the_board_queue_and_results() {
        let (_t, home) = fixture();
        write_board(&home, &format!(r#"{{"jobs":[{}]}}"#, job_json("001", "001-first")));
        let req = crate::request::Request::new("good", crate::request::Action::Run, "002", false, 5).expect("req");
        crate::queue::enqueue(&home, &req).expect("enqueue");
        let done = crate::records::ResultRecord {
            request_id: crate::request::make_id(9, "good", "001"),
            project: "good".into(),
            job: "001".into(),
            outcome: crate::records::Outcome::Merged,
            finished_ms: 9,
        };
        crate::records::write_result(&home, &done).expect("result");
        let r = get(&home, "/projects/good/jobs");
        assert_eq!(r.status, 200, "{}", r.body);
        let job = &r.body["jobs"][0];
        assert_eq!(job["id"], "001");
        assert_eq!(job["stage"], "build");
        assert_eq!(job["branch"], "factory/001-first");
        assert_eq!(job["tokens"], 25);
        assert_eq!(job["cancelled"], false);
        assert!(job["series"].is_null());
        assert_eq!(r.body["queued"][0]["job"], "002");
        assert_eq!(r.body["lastResults"][0]["job"], "001");
        assert_eq!(r.body["lastResults"][0]["outcome"]["kind"], "merged");
    }

    #[test]
    fn a_corrupt_board_breaks_only_that_projects_jobs() {
        let (_t, home) = fixture();
        write_board(&home, "{ not json");
        let r = get(&home, "/projects/good/jobs");
        assert_eq!(r.status, 500);
        assert_eq!(r.body["error"]["code"], "failed");
        assert_eq!(get(&home, "/projects").status, 200);
    }

    #[test]
    fn usage_sums_weighted_tokens_per_utc_day() {
        let (_t, home) = fixture();
        // NOW_MS is 2023-11-14 22:13:20 UTC, so the window of 2 days is 2023-11-13 and 2023-11-14.
        let today = NOW_MS / 1000;
        let yesterday = today - 86_400;
        let old = today - 5 * 86_400;
        let text = format!(
            "{{\"job\":\"001\",\"input\":100,\"cache_creation\":0,\"output\":0,\"cache_read\":1000,\"ts\":{today}}}\n\
             bad line\n\
             {{\"job\":\"001\",\"input\":7,\"cache_creation\":0,\"output\":0,\"cache_read\":0,\"ts\":{yesterday}}}\n\
             {{\"job\":\"001\",\"input\":50,\"cache_creation\":0,\"output\":0,\"cache_read\":0,\"ts\":{old}}}\n"
        );
        write_usage(&home, "001-first", &text);
        let r = route(&home, &deps(), "GET", "/usage", "days=2", &headers(Some(TOKEN), "localhost:1"), b"");
        assert_eq!(r.status, 200, "{}", r.body);
        assert_eq!(r.body["days"], 2);
        let good = r.body["projects"].as_array().expect("array").iter().find(|p| p["name"] == "good").expect("good");
        assert_eq!(good["total"], 207);
        assert_eq!(good["perDay"][0], serde_json::json!({"day": "2023-11-13", "tokens": 7}));
        assert_eq!(good["perDay"][1], serde_json::json!({"day": "2023-11-14", "tokens": 200}));
    }

    #[test]
    fn bad_days_are_refused() {
        let (_t, home) = fixture();
        for q in ["days=0", "days=91", "days=x"] {
            let r = route(&home, &deps(), "GET", "/usage", q, &headers(Some(TOKEN), "localhost:1"), b"");
            assert_eq!((r.status, r.body["error"]["code"].as_str()), (400, Some("bad_request")), "{q}");
        }
    }
}
