//! The write endpoints: create a job, approve it, send a rework note, cancel it.
//!
//! A request body is untrusted. Everything is checked before a process starts or a request is queued.
//! The stage rules stay in factory-cli, which runs as a child. Only the approve pre-check reads the board.

use std::path::{Component, Path, PathBuf};

use factory_core::project::{self, ProjectFile};
use factory_core::{normalize_id, slugify, Board, Home, Layout, Stage};
use serde::Deserialize;
use serde_json::json;

use crate::api::http::{error_response, ApiResponse, MAX_BODY};
use crate::api::ApiDeps;
use crate::factory_cli::CliOutput;
use crate::request::{validate_job, Action, Request};
use crate::{queue, QueueError};

const MAX_NOTE: usize = 200;
const MAX_PATH: usize = 4096;
const TAIL_CHARS: usize = 800;
const DEFAULT_REASON: &str = "cancelled by user";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateBody {
    spec: String,
    plans: String,
    model: Option<String>,
    feature: Option<String>,
    run: Option<bool>,
    series: Option<bool>,
    slug: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReworkBody {
    note: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct CancelBody {
    reason: Option<String>,
    force: Option<bool>,
}

fn bad(message: &str) -> ApiResponse {
    error_response(400, "bad_request", message)
}

fn parse<T: for<'de> Deserialize<'de>>(body: &[u8]) -> Result<T, ApiResponse> {
    if body.len() > MAX_BODY {
        return Err(bad("the request body is too large"));
    }
    serde_json::from_slice(body).map_err(|e| bad(&format!("the request body is not valid: {e}")))
}

/// The layout of a project that can take work. A missing project is `not_found`, a broken one `project_disabled`.
fn enabled(home: &Home, name: &str) -> Result<Layout, ApiResponse> {
    project::validate_name(name).map_err(|e| bad(&e.to_string()))?;
    if !home.project_dir(name).is_dir() {
        return Err(error_response(404, "not_found", &format!("no project named {name}")));
    }
    let disabled = |why: String| error_response(409, "project_disabled", &why);
    let file: ProjectFile = project::get(home, name).map_err(|e| disabled(e.to_string()))?;
    let layout = project::layout(home, name, &file).map_err(|e| disabled(e.to_string()))?;
    if !file.repo.is_dir() {
        return Err(disabled("repo path is missing".to_string()));
    }
    Ok(layout)
}

fn job_id(raw: &str) -> Result<String, ApiResponse> {
    validate_job(raw).map_err(|e| bad(&e.to_string()))?;
    normalize_id(raw).map_err(|e| bad(&e.to_string()))
}

/// One non-empty line of at most 200 characters.
fn one_line(text: &str, what: &str) -> Result<(), ApiResponse> {
    if text.trim().is_empty() {
        return Err(bad(&format!("the {what} is empty")));
    }
    if text.contains('\n') || text.contains('\r') {
        return Err(bad(&format!("the {what} must be one line")));
    }
    if text.chars().count() > MAX_NOTE {
        return Err(bad(&format!("the {what} is longer than {MAX_NOTE} characters")));
    }
    if text.contains('\0') {
        return Err(bad(&format!("the {what} has a NUL character")));
    }
    Ok(())
}

/// `[a-z0-9][a-z0-9-]{0,63}`.
fn valid_slug(slug: &str) -> bool {
    let mut chars = slug.chars();
    slug.len() <= 64
        && chars.next().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The plans folder name without a leading `YYYY-MM-DD-`, as a slug. Nothing usable gives `None`.
fn derive_slug(plans: &Path) -> Option<String> {
    let name = plans.file_name()?.to_string_lossy().into_owned();
    let b = name.as_bytes();
    let dated = b.len() > 11
        && b[10] == b'-'
        && b[..10].iter().enumerate().all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() });
    let rest = if dated { &name[11..] } else { name.as_str() };
    if !rest.chars().any(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some(slugify(rest)).filter(|s| valid_slug(s))
}

fn valid_model(model: &str) -> bool {
    !model.is_empty()
        && model.len() <= 64
        && model.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

/// An absolute path without `..` that exists, as a canonical path.
fn existing_path(raw: &str, what: &str, want_dir: bool) -> Result<PathBuf, ApiResponse> {
    if raw.is_empty() || raw.len() > MAX_PATH || raw.contains('\0') {
        return Err(bad(&format!("the {what} path is not valid")));
    }
    let path = Path::new(raw);
    if !path.is_absolute() {
        return Err(bad(&format!("the {what} path must be absolute")));
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(bad(&format!("the {what} path must not contain ..")));
    }
    let canonical = std::fs::canonicalize(path).map_err(|_| bad(&format!("the {what} path does not exist")))?;
    let ok = if want_dir { canonical.is_dir() } else { canonical.is_file() };
    if !ok {
        return Err(bad(&format!("the {what} path must be {}", if want_dir { "a folder" } else { "a file" })));
    }
    Ok(canonical)
}

/// The last lines of what a command printed, cut to a size that fits an error message.
fn tail(text: &str) -> String {
    let text = text.trim();
    let count = text.chars().count();
    if count <= TAIL_CHARS {
        return text.to_string();
    }
    text.chars().skip(count - TAIL_CHARS).collect()
}

/// Maps a finished command. Exit code 1 is a refusal by the command, other codes are failures.
fn finished(out: Result<CliOutput, crate::factory_cli::CliSpawnError>, refusal_is_conflict: bool) -> Result<CliOutput, ApiResponse> {
    match out {
        Err(e) => Err(error_response(500, "failed", &e.to_string())),
        Ok(o) if o.code == 0 => Ok(o),
        Ok(o) => {
            let why = if o.stderr.trim().is_empty() { tail(&o.stdout) } else { tail(&o.stderr) };
            if refusal_is_conflict && o.code == 1 {
                Err(error_response(409, "conflict", &why))
            } else {
                Err(error_response(500, "failed", &why))
            }
        }
    }
}

fn queue_error(e: QueueError) -> ApiResponse {
    match e {
        QueueError::Duplicate(id) => error_response(409, "conflict", &format!("request {id} is already queued")),
        other => error_response(500, "failed", &other.to_string()),
    }
}

fn enqueue(home: &Home, deps: &ApiDeps, name: &str, action: Action, job: &str, series: bool) -> Result<String, ApiResponse> {
    let req = Request::new(name, action, job, series, (deps.now_ms)()).map_err(|e| bad(&e.to_string()))?;
    queue::enqueue(home, &req).map_err(queue_error)?;
    Ok(req.id)
}

/// `created job 001 in <dir>` gives `001`.
fn created_id(stdout: &str) -> Option<String> {
    let rest = stdout.lines().find_map(|l| l.trim().strip_prefix("created job "))?;
    let id = rest.split_whitespace().next()?;
    validate_job(id).ok()?;
    Some(id.to_string())
}

/// `POST /projects/{p}/jobs`.
pub fn create(home: &Home, deps: &ApiDeps, name: &str, body: &[u8]) -> ApiResponse {
    match create_inner(home, deps, name, body) {
        Ok(r) | Err(r) => r,
    }
}

fn create_inner(home: &Home, deps: &ApiDeps, name: &str, body: &[u8]) -> Result<ApiResponse, ApiResponse> {
    enabled(home, name)?;
    let b: CreateBody = parse(body)?;
    let spec = existing_path(&b.spec, "spec", false)?;
    let plans = existing_path(&b.plans, "plans", true)?;
    if let Some(m) = &b.model {
        if !valid_model(m) {
            return Err(bad("the model may use letters, digits, . _ - and at most 64 characters"));
        }
    }
    let slug = match &b.slug {
        Some(s) if valid_slug(s) => s.clone(),
        Some(_) => return Err(bad("the slug must match [a-z0-9][a-z0-9-]{0,63}")),
        None => derive_slug(&plans).ok_or_else(|| bad("no slug can be derived from the plans folder name, send a slug"))?,
    };
    let feature = b.feature.clone().unwrap_or_else(|| slug.clone());
    one_line(&feature, "feature")?;
    // Options go first and values are joined to their flag, so no value can pass for an option.
    let mut args = vec![
        "new".to_string(),
        format!("--plans={}", plans.display()),
        format!("--spec={}", spec.display()),
    ];
    if let Some(m) = &b.model {
        args.push(format!("--model={m}"));
    }
    args.extend(["--".to_string(), slug, feature]);
    let out = finished(deps.cli.run(name, &args), false)?;
    let job = created_id(&out.stdout)
        .ok_or_else(|| error_response(500, "failed", &format!("the job was created but its id is unknown: {}", tail(&out.stdout))))?;
    let request_id = if b.run.unwrap_or(true) {
        Some(enqueue(home, deps, name, Action::Run, &job, b.series.unwrap_or(true)).map_err(|e| {
            let why = e.body["error"]["message"].as_str().unwrap_or("").to_string();
            error_response(e.status, e.body["error"]["code"].as_str().unwrap_or("failed"), &format!("job {job} was created but not queued: {why}"))
        })?)
    } else {
        None
    };
    Ok(ApiResponse { status: 201, body: json!({"job": job, "requestId": request_id}) })
}

/// `POST /projects/{p}/jobs/{id}/approve`.
pub fn approve(home: &Home, deps: &ApiDeps, name: &str, id: &str) -> ApiResponse {
    match approve_inner(home, deps, name, id) {
        Ok(r) | Err(r) => r,
    }
}

fn approve_inner(home: &Home, deps: &ApiDeps, name: &str, id: &str) -> Result<ApiResponse, ApiResponse> {
    let layout = enabled(home, name)?;
    let id = job_id(id)?;
    let board = Board::load(&layout.board_file())
        .map_err(|e| error_response(500, "failed", &format!("the board of this project cannot be read: {e}")))?;
    let job = board
        .jobs
        .iter()
        .find(|j| j.id == id)
        .ok_or_else(|| error_response(404, "not_found", &format!("no job {id} in project {name}")))?;
    if job.cancelled {
        return Err(error_response(409, "conflict", &format!("job {id} was cancelled")));
    }
    if job.stage != Stage::NeedsHuman && job.stage != Stage::Approve {
        return Err(error_response(
            409,
            "conflict",
            &format!("job {id} is in stage {}, approve needs needs-human or approve", job.stage),
        ));
    }
    let request_id = enqueue(home, deps, name, Action::Approve, &id, false)?;
    Ok(ApiResponse { status: 202, body: json!({"requestId": request_id}) })
}

/// `POST /projects/{p}/jobs/{id}/rework`.
pub fn rework(home: &Home, deps: &ApiDeps, name: &str, id: &str, body: &[u8]) -> ApiResponse {
    match rework_inner(home, deps, name, id, body) {
        Ok(r) | Err(r) => r,
    }
}

fn rework_inner(home: &Home, deps: &ApiDeps, name: &str, id: &str, body: &[u8]) -> Result<ApiResponse, ApiResponse> {
    enabled(home, name)?;
    let id = job_id(id)?;
    let b: ReworkBody = parse(body)?;
    one_line(&b.note, "rework note")?;
    // After `--` the note cannot pass for an option, even when it starts with a dash.
    let args = vec!["rework".to_string(), "--".to_string(), id, b.note];
    let out = finished(deps.cli.run(name, &args), true)?;
    Ok(ApiResponse::ok(json!({"output": out.stdout})))
}

/// `POST /projects/{p}/jobs/{id}/cancel`.
pub fn cancel(home: &Home, deps: &ApiDeps, name: &str, id: &str, body: &[u8]) -> ApiResponse {
    match cancel_inner(home, deps, name, id, body) {
        Ok(r) | Err(r) => r,
    }
}

fn cancel_inner(home: &Home, deps: &ApiDeps, name: &str, id: &str, body: &[u8]) -> Result<ApiResponse, ApiResponse> {
    enabled(home, name)?;
    let id = job_id(id)?;
    // No body at all means the defaults.
    let b: CancelBody = if body.is_empty() { CancelBody::default() } else { parse(body)? };
    let reason = b.reason.unwrap_or_else(|| DEFAULT_REASON.to_string());
    one_line(&reason, "reason")?;
    let mut args = vec!["cancel".to_string(), format!("--reason={reason}")];
    if b.force.unwrap_or(false) {
        args.push("--force".to_string());
    }
    args.extend(["--".to_string(), id]);
    let out = finished(deps.cli.run(name, &args), true)?;
    Ok(ApiResponse::ok(json!({"output": out.stdout})))
}

#[cfg(test)]
mod tests {
    use crate::api::http::{route, ApiResponse};
    use crate::api::testutil::*;
    use crate::request::Action;
    use serde_json::json;
    use std::sync::Arc;

    struct Fx {
        _t: tempfile::TempDir,
        home: factory_core::Home,
        cli: Arc<FakeCli>,
        spec: std::path::PathBuf,
        plans: std::path::PathBuf,
    }

    fn fx() -> Fx {
        let (t, home) = fixture();
        let plans = t.path().join("work").join("plans");
        std::fs::create_dir_all(&plans).expect("plans");
        let spec = t.path().join("work").join("My Spec.md");
        std::fs::write(&spec, "# spec").expect("spec");
        Fx { _t: t, home, cli: Arc::new(FakeCli::default()), spec, plans }
    }

    fn post(f: &Fx, path: &str, body: &[u8]) -> ApiResponse {
        route(&f.home, &deps_with(f.cli.clone()), "POST", path, "", &headers(Some(TOKEN), "localhost:1"), body)
    }

    fn create_body(f: &Fx) -> serde_json::Value {
        json!({"spec": f.spec, "plans": f.plans})
    }

    fn code(r: &ApiResponse) -> (u16, String) {
        (r.status, r.body["error"]["code"].as_str().unwrap_or("").to_string())
    }

    fn queued(f: &Fx) -> usize {
        crate::queue::list(&f.home).len()
    }

    #[test]
    fn create_runs_new_once_with_canonical_paths_then_queues_a_run() {
        let f = fx();
        f.cli.reply_stdout("created job 007 in 2026-10-17-007-work\n");
        let r = post(&f, "/projects/good/jobs", create_body(&f).to_string().as_bytes());
        assert_eq!(r.status, 201, "{}", r.body);
        assert_eq!(r.body["job"], "007");
        let calls = f.cli.calls();
        assert_eq!(calls.len(), 1);
        let canon = |p: &std::path::Path| std::fs::canonicalize(p).expect("canon").display().to_string();
        assert_eq!(calls[0].0, "good");
        assert_eq!(
            calls[0].1,
            vec![
                "new".to_string(),
                format!("--plans={}", canon(&f.plans)),
                format!("--spec={}", canon(&f.spec)),
                "--".to_string(),
                "plans".to_string(),
                "plans".to_string()
            ]
        );
        let items = crate::queue::list(&f.home);
        assert_eq!(items.len(), 1);
        match &items[0] {
            crate::QueueItem::Ok(req) => {
                assert_eq!((req.action, req.job.as_str(), req.project.as_str()), (Action::Run, "007", "good"));
                assert!(req.series, "series defaults to true");
                assert_eq!(r.body["requestId"], req.id.as_str());
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn create_passes_model_and_feature_and_run_false_queues_nothing() {
        let f = fx();
        f.cli.reply_stdout("created job 001 in x\n");
        let mut body = create_body(&f);
        body["model"] = json!("claude-opus-4.1_x");
        body["feature"] = json!("--evil feature");
        body["run"] = json!(false);
        let r = post(&f, "/projects/good/jobs", body.to_string().as_bytes());
        assert_eq!(r.status, 201, "{}", r.body);
        assert!(r.body["requestId"].is_null());
        assert_eq!(queued(&f), 0);
        let args = &f.cli.calls()[0].1;
        assert!(args.contains(&"--model=claude-opus-4.1_x".to_string()));
        assert_eq!(args[args.len() - 1], "--evil feature");
        assert_eq!(args[args.len() - 3], "--");
    }

    #[test]
    fn a_failing_new_returns_failed_with_the_stderr_tail_and_queues_nothing() {
        let f = fx();
        f.cli.reply(1, "", "factory: the plans folder has no spec.md\n");
        let r = post(&f, "/projects/good/jobs", create_body(&f).to_string().as_bytes());
        assert_eq!(code(&r), (500, "failed".to_string()));
        assert!(r.body["error"]["message"].as_str().expect("msg").contains("no spec.md"));
        assert_eq!(f.cli.calls().len(), 1);
        assert_eq!(queued(&f), 0);
    }

    #[test]
    fn create_refusals_run_nothing_and_queue_nothing() {
        let f = fx();
        let dir = f._t.path().join("work");
        let file_as_plans = f.spec.clone();
        let cases: Vec<(&str, serde_json::Value)> = vec![
            ("relative spec", json!({"spec": "rel/spec.md", "plans": f.plans})),
            ("missing spec", json!({"spec": dir.join("none.md"), "plans": f.plans})),
            ("plans is a file", json!({"spec": f.spec, "plans": file_as_plans})),
            ("dotdot", json!({"spec": format!("{}/plans/../My Spec.md", dir.display()), "plans": f.plans})),
            ("extra field", json!({"spec": f.spec, "plans": f.plans, "cmd": "x"})),
            ("bad model", json!({"spec": f.spec, "plans": f.plans, "model": "a b"})),
            ("empty model", json!({"spec": f.spec, "plans": f.plans, "model": ""})),
            ("long model", json!({"spec": f.spec, "plans": f.plans, "model": "m".repeat(65)})),
            ("multiline feature", json!({"spec": f.spec, "plans": f.plans, "feature": "a\nb"})),
            ("run is a string", json!({"spec": f.spec, "plans": f.plans, "run": "yes"})),
        ];
        for (name, body) in cases {
            let r = post(&f, "/projects/good/jobs", body.to_string().as_bytes());
            assert_eq!(code(&r), (400, "bad_request".to_string()), "{name}: {}", r.body);
        }
        let huge = format!(r#"{{"spec":"/x","plans":"/y","feature":"{}"}}"#, "a".repeat(70 * 1024));
        assert_eq!(code(&post(&f, "/projects/good/jobs", huge.as_bytes())).1, "bad_request");
        assert_eq!(code(&post(&f, "/projects/good/jobs", b"{not json")).1, "bad_request");
        assert_eq!(code(&post(&f, "/projects/good/jobs", b"")).1, "bad_request");
        let r = post(&f, "/projects/broken/jobs", create_body(&f).to_string().as_bytes());
        assert_eq!(code(&r), (409, "project_disabled".to_string()));
        assert_eq!(code(&post(&f, "/projects/ghost/jobs", b"{}")), (404, "not_found".to_string()));
        assert_eq!(code(&post(&f, "/projects/Bad..Name/jobs", b"{}")).1, "bad_request");
        assert!(f.cli.calls().is_empty());
        assert_eq!(queued(&f), 0);
    }

    #[test]
    fn series_is_optional_boolean_and_slug_is_derived_or_given() {
        let f = fx();
        f.cli.reply_stdout("created job 001 in x\n");
        let mut body = create_body(&f);
        body["series"] = json!(false);
        assert_eq!(post(&f, "/projects/good/jobs", body.to_string().as_bytes()).status, 201);
        match &crate::queue::list(&f.home)[0] {
            crate::QueueItem::Ok(req) => assert!(!req.series),
            other => panic!("{other:?}"),
        }
        let n = f.cli.calls().len();
        for bad in [json!("yes"), json!(1)] {
            let mut b = create_body(&f);
            b["series"] = bad;
            let r = post(&f, "/projects/good/jobs", b.to_string().as_bytes());
            assert_eq!(code(&r), (400, "bad_request".to_string()));
        }
        assert_eq!(f.cli.calls().len(), n);

        // Derivation from a dated plans folder, and the override.
        let dated = f._t.path().join("2026-10-16-Standalone Software_Factory");
        std::fs::create_dir_all(&dated).expect("dated");
        let b = json!({"spec": f.spec, "plans": dated, "run": false});
        assert_eq!(post(&f, "/projects/good/jobs", b.to_string().as_bytes()).status, 201);
        let args = &f.cli.calls()[n].1;
        assert_eq!(args[args.len() - 2], "standalone-software-factory");
        let b = json!({"spec": f.spec, "plans": dated, "run": false, "slug": "my-slug1"});
        assert_eq!(post(&f, "/projects/good/jobs", b.to_string().as_bytes()).status, 201);
        assert_eq!(f.cli.calls()[n + 1].1[f.cli.calls()[n + 1].1.len() - 2], "my-slug1");

        // Invalid slugs and an underivable name run nothing.
        let calls = f.cli.calls().len();
        for s in ["", "-a", "A", "a b", "a/b", &"a".repeat(65)] {
            let b = json!({"spec": f.spec, "plans": f.plans, "slug": s});
            assert_eq!(code(&post(&f, "/projects/good/jobs", b.to_string().as_bytes())).1, "bad_request", "{s:?}");
        }
        let odd = f._t.path().join("2026-10-16-___");
        std::fs::create_dir_all(&odd).expect("odd");
        let b = json!({"spec": f.spec, "plans": odd});
        assert_eq!(code(&post(&f, "/projects/good/jobs", b.to_string().as_bytes())).1, "bad_request");
        assert_eq!(f.cli.calls().len(), calls);
    }

    fn board_with(f: &Fx, stage: &str, cancelled: bool) {
        let job = job_json("001", "001-first").replace(r#""stage":"build""#, &format!(r#""stage":"{stage}","cancelled":{cancelled}"#));
        write_board(&f.home, &format!(r#"{{"jobs":[{job}]}}"#));
    }

    #[test]
    fn approve_queues_only_for_needs_human_or_approve() {
        let f = fx();
        board_with(&f, "build", false);
        let r = post(&f, "/projects/good/jobs/1/approve", b"");
        assert_eq!(code(&r), (409, "conflict".to_string()));
        assert_eq!(queued(&f), 0);
        board_with(&f, "needs-human", true);
        assert_eq!(code(&post(&f, "/projects/good/jobs/1/approve", b"")), (409, "conflict".to_string()));
        board_with(&f, "needs-human", false);
        let r = post(&f, "/projects/good/jobs/1/approve", b"");
        assert_eq!(r.status, 202, "{}", r.body);
        assert_eq!(queued(&f), 1);
        match &crate::queue::list(&f.home)[0] {
            crate::QueueItem::Ok(req) => {
                assert_eq!((req.action, req.job.as_str()), (Action::Approve, "001"));
                assert_eq!(r.body["requestId"], req.id.as_str());
            }
            other => panic!("{other:?}"),
        }
        // The same second and job again is the same request id.
        assert_eq!(code(&post(&f, "/projects/good/jobs/001/approve", b"")), (409, "conflict".to_string()));
        board_with(&f, "approve", false);
        assert!(f.cli.calls().is_empty());
    }

    #[test]
    fn approve_checks_the_job_id() {
        let f = fx();
        board_with(&f, "approve", false);
        assert_eq!(code(&post(&f, "/projects/good/jobs/9/approve", b"")), (404, "not_found".to_string()));
        for id in ["1%2F2", "1x", "1234567", "-1"] {
            assert_eq!(code(&post(&f, &format!("/projects/good/jobs/{id}/approve"), b"")).1, "bad_request", "{id}");
        }
        // A slash makes another path, which does not exist.
        assert_eq!(code(&post(&f, "/projects/good/jobs/1/2/approve", b"")).1, "not_found");
        assert_eq!(queued(&f), 0);
    }

    #[test]
    fn rework_sends_the_note_as_one_argument() {
        let f = fx();
        f.cli.reply_stdout("job 001 back to build, round reset to 0\n");
        let note = "fix it; rm -rf / $(x) --force";
        let r = post(&f, "/projects/good/jobs/1/rework", json!({"note": note}).to_string().as_bytes());
        assert_eq!(r.status, 200, "{}", r.body);
        assert_eq!(r.body["output"], "job 001 back to build, round reset to 0\n");
        let calls = f.cli.calls();
        assert_eq!(calls[0].1, vec!["rework".to_string(), "--".to_string(), "001".to_string(), note.to_string()]);
    }

    #[test]
    fn rework_notes_are_one_short_line() {
        let f = fx();
        for note in ["two\nlines", "", "   ", &"x".repeat(201)] {
            let r = post(&f, "/projects/good/jobs/1/rework", json!({"note": note}).to_string().as_bytes());
            assert_eq!(code(&r), (400, "bad_request".to_string()), "{note:?}");
        }
        let r = post(&f, "/projects/good/jobs/1/rework", json!({"note": "x".repeat(200)}).to_string().as_bytes());
        assert_eq!(r.status, 200);
        assert_eq!(code(&post(&f, "/projects/good/jobs/1/rework", b"{}")).1, "bad_request");
        assert_eq!(f.cli.calls().len(), 1);
    }

    #[test]
    fn cancel_checks_force_and_surfaces_a_cli_refusal_as_conflict() {
        let f = fx();
        let r = post(&f, "/projects/good/jobs/1/cancel", br#"{"force":"yes"}"#);
        assert_eq!(code(&r), (400, "bad_request".to_string()));
        let r = post(&f, "/projects/good/jobs/1/cancel", br#"{"reason":"a\nb"}"#);
        assert_eq!(code(&r), (400, "bad_request".to_string()));
        assert!(f.cli.calls().is_empty());
        f.cli.reply(1, "", "factory: job has a live lease, use --force to cancel anyway\n");
        let r = post(&f, "/projects/good/jobs/1/cancel", br#"{"reason":"stuck","force":false}"#);
        assert_eq!(code(&r), (409, "conflict".to_string()));
        assert!(r.body["error"]["message"].as_str().expect("msg").contains("live lease"));
        assert_eq!(f.cli.calls()[0].1, vec!["cancel".to_string(), "--reason=stuck".to_string(), "--".to_string(), "001".to_string()]);
        f.cli.reply_stdout("job 001 cancelled\n");
        let r = post(&f, "/projects/good/jobs/1/cancel", br#"{"force":true}"#);
        assert_eq!(r.status, 200);
        assert_eq!(
            f.cli.calls()[1].1,
            vec![
                "cancel".to_string(),
                "--reason=cancelled by user".to_string(),
                "--force".to_string(),
                "--".to_string(),
                "001".to_string()
            ]
        );
        // An empty body is the defaults.
        assert_eq!(post(&f, "/projects/good/jobs/1/cancel", b"").status, 200);
    }

    #[test]
    fn a_cli_that_cannot_start_is_failed_and_other_exit_codes_are_failed() {
        let f = fx();
        f.cli.reply(2, "", "factory: cannot load the project\n");
        let r = post(&f, "/projects/good/jobs/1/cancel", b"{}");
        assert_eq!(code(&r), (500, "failed".to_string()));
    }

    #[test]
    fn methods_are_checked_per_path() {
        let f = fx();
        let h = headers(Some(TOKEN), "localhost:1");
        let d = deps_with(f.cli.clone());
        for (m, p) in [("GET", "/projects/good/jobs/1/approve"), ("PUT", "/projects/good/jobs"), ("DELETE", "/projects/good/jobs/1/cancel")] {
            let r = route(&f.home, &d, m, p, "", &h, b"");
            assert_eq!(code(&r), (405, "method_not_allowed".to_string()), "{m} {p}");
        }
        assert_eq!(code(&route(&f.home, &d, "POST", "/projects/good/jobs/1/other", "", &h, b"")).1, "not_found");
        assert_eq!(code(&route(&f.home, &d, "POST", "/projects", "", &h, b"")).1, "method_not_allowed");
    }

    #[test]
    fn unauthorized_writes_run_nothing() {
        let f = fx();
        let r = route(&f.home, &deps_with(f.cli.clone()), "POST", "/projects/good/jobs", "", &headers(None, "localhost:1"), b"{}");
        assert_eq!(code(&r).1, "unauthorized");
        assert!(f.cli.calls().is_empty());
    }
}
