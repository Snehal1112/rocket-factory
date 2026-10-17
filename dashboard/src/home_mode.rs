//! Home mode: the dashboard shows every project of a factory home and forwards writes to factoryd.
//!
//! The server reads the API token from `<home>/api.token` for each call and adds it to the request it sends
//! to factoryd. The token never reaches the browser: no reply, header or log line carries it.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::time::Duration;

use serde_json::{json, Value};

use crate::paths::Paths;
use crate::server::{api_park, json_reply, state_reply, HomeState, Project, Reply, Request};

const API_TIMEOUT: Duration = Duration::from_secs(5);
/// Most bytes of an API reply body that are read.
const MAX_REPLY: usize = 4 * 1024 * 1024;
/// Room for the status line and headers on top of the body cap.
const HEADER_ROOM: usize = 32 * 1024;
const TOKEN_FILE: &str = "api.token";

/// A reply from factoryd.
#[derive(Debug)]
pub struct ApiReply {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Only loopback hosts are accepted, because the token is sent to this address.
pub fn valid_api_addr(addr: &str) -> bool {
    let Some((host, port)) = addr.rsplit_once(':') else { return false };
    port.parse::<u16>().is_ok() && matches!(host, "127.0.0.1" | "localhost" | "[::1]")
}

/// Project names are used in API paths, so they stay to a small safe alphabet.
pub fn valid_project(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name != "."
        && name != ".."
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Job ids are digits only.
pub fn valid_job_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 12 && id.chars().all(|c| c.is_ascii_digit())
}

fn error_reply(status: u16, code: &str, message: &str) -> Reply {
    json_reply(status, &json!({ "error": { "code": code, "message": message } }))
}

/// Reads the token. A token with whitespace or control characters is refused, so it cannot split a header.
fn read_token(home: &Path) -> Result<String, Reply> {
    let missing = || {
        error_reply(
            502,
            "token_missing",
            "The factory API token could not be read. Start factoryd once so it creates api.token in the factory home.",
        )
    };
    let text = std::fs::read_to_string(home.join(TOKEN_FILE)).map_err(|_| missing())?;
    let token = text.trim();
    if token.is_empty() || !token.chars().all(|c| c.is_ascii_graphic()) {
        return Err(missing());
    }
    Ok(token.to_string())
}

fn find_header_end(data: &[u8]) -> Option<usize> {
    data.windows(4).position(|w| w == b"\r\n\r\n")
}

fn decode_chunked(mut data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let line_end = data.windows(2).position(|w| w == b"\r\n").ok_or("bad chunk")?;
        let size_text = std::str::from_utf8(&data[..line_end]).map_err(|_| "bad chunk size")?;
        let size_text = size_text.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_text, 16).map_err(|_| "bad chunk size")?;
        data = &data[line_end + 2..];
        if size == 0 {
            return Ok(out);
        }
        let end = size.checked_add(2).ok_or("bad chunk size")?;
        if data.len() < end {
            return Err("short chunk".to_string());
        }
        out.extend_from_slice(&data[..size]);
        data = &data[end..];
    }
}

/// Splits a raw HTTP/1.1 reply into status and body.
fn parse_response(raw: &[u8]) -> Result<ApiReply, String> {
    let end = find_header_end(raw).ok_or("the API reply has no header end")?;
    let head = std::str::from_utf8(&raw[..end]).map_err(|_| "the API reply head is not text")?;
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or("");
    let status = status_line
        .split(' ')
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or("the API reply has no status")?;
    let mut chunked = false;
    let mut length: Option<usize> = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else { continue };
        let value = value.trim();
        if name.eq_ignore_ascii_case("transfer-encoding") && value.to_ascii_lowercase().contains("chunked") {
            chunked = true;
        } else if name.eq_ignore_ascii_case("content-length") {
            length = value.parse().ok();
        }
    }
    let rest = &raw[end + 4..];
    let body = if chunked {
        decode_chunked(rest)?
    } else if let Some(n) = length {
        if n > rest.len() {
            return Err("the API reply is shorter than its Content-Length".to_string());
        }
        rest[..n].to_vec()
    } else {
        rest.to_vec()
    };
    if body.len() > MAX_REPLY {
        return Err("the API reply is too large".to_string());
    }
    Ok(ApiReply { status, body })
}

/// One HTTP/1.1 call with `Connection: close`, 5 second timeouts and a 4 MiB body cap.
pub fn api_call(api: &str, method: &str, path: &str, token: &str, body: Option<&[u8]>) -> Result<ApiReply, String> {
    let addr = api
        .to_socket_addrs()
        .map_err(|_| "the API address cannot be resolved".to_string())?
        .next()
        .ok_or("the API address cannot be resolved")?;
    let mut stream = TcpStream::connect_timeout(&addr, API_TIMEOUT).map_err(|_| "the factory API is not reachable".to_string())?;
    let _ = stream.set_read_timeout(Some(API_TIMEOUT));
    let _ = stream.set_write_timeout(Some(API_TIMEOUT));
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {api}\r\nAuthorization: Bearer {token}\r\nAccept: application/json\r\nConnection: close\r\n"
    );
    if let Some(bytes) = body {
        head.push_str(&format!("Content-Type: application/json\r\nContent-Length: {}\r\n", bytes.len()));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).map_err(|_| "the factory API did not take the request".to_string())?;
    if let Some(bytes) = body {
        stream.write_all(bytes).map_err(|_| "the factory API did not take the request".to_string())?;
    }
    let mut raw = Vec::new();
    let limit = (MAX_REPLY + HEADER_ROOM + 1) as u64;
    let read = stream.take(limit).read_to_end(&mut raw);
    if raw.is_empty() {
        return Err(match read {
            Err(_) => "the factory API did not answer in time".to_string(),
            Ok(_) => "the factory API sent an empty reply".to_string(),
        });
    }
    if raw.len() > MAX_REPLY + HEADER_ROOM {
        return Err("the API reply is too large".to_string());
    }
    parse_response(&raw)
}

/// Turns a transport or token problem into the error the page shows.
fn call_failed(message: &str) -> Reply {
    error_reply(502, "api_unreachable", &format!("{message}. Check that factoryd is running."))
}

/// Maps an API reply to the dashboard reply. A 401 means the token on disk is not the one factoryd uses.
fn forward(reply: ApiReply) -> Reply {
    if reply.status == 401 {
        return error_reply(
            502,
            "token_rejected",
            "The factory API rejected the dashboard token. Restart factory-server so it reads the current api.token.",
        );
    }
    match serde_json::from_slice::<Value>(&reply.body) {
        Ok(value) => json_reply(reply.status, &value),
        Err(_) => error_reply(502, "api_error", &format!("The factory API answered {} with a body that is not JSON.", reply.status)),
    }
}

fn get_api(home: &HomeState, path: &str) -> Result<ApiReply, Reply> {
    let token = read_token(&home.dir)?;
    api_call(&home.api, "GET", path, &token, None).map_err(|m| call_failed(&m))
}

/// The API project list, or the reply to show instead.
fn fetch_projects(home: &HomeState) -> Result<Vec<Value>, Reply> {
    let reply = get_api(home, "/projects")?;
    if reply.status != 200 {
        return Err(forward(reply));
    }
    match serde_json::from_slice::<Value>(&reply.body) {
        Ok(Value::Array(list)) => Ok(list),
        _ => Err(error_reply(502, "api_error", "The factory API project list is not a list.")),
    }
}

fn str_field(entry: &Value, key: &str) -> Option<String> {
    entry.get(key).and_then(Value::as_str).map(str::to_string)
}

fn api_projects(home: &HomeState) -> Reply {
    let mut list = match fetch_projects(home) {
        Ok(list) => list,
        Err(reply) => return reply,
    };
    list.sort_by_key(|entry| str_field(entry, "name").unwrap_or_default());
    let view: Vec<Value> = list
        .iter()
        .map(|entry| {
            json!({
                "name": entry.get("name"),
                "repo": entry.get("repo"),
                "preset": entry.get("preset"),
                "baseBranch": entry.get("baseBranch"),
                "status": entry.get("status"),
                "reason": entry.get("reason"),
                "activeJob": entry.get("activeJob"),
            })
        })
        .collect();
    json_reply(200, &view)
}

/// The folders of a project that can take work, or `None` for a disabled one.
fn project_paths(entry: &Value) -> Option<Paths> {
    if entry.get("status").and_then(Value::as_str) == Some("disabled") {
        return None;
    }
    let state = str_field(entry, "stateDir")?;
    let jobs = str_field(entry, "jobsDir")?;
    Some(Paths::from_project(Path::new(&state), Path::new(&jobs)))
}

/// The list entry of `name`, or the reply to show instead.
fn find_project(home: &HomeState, name: &str) -> Result<Value, Reply> {
    let list = fetch_projects(home)?;
    list.into_iter()
        .find(|entry| entry.get("name").and_then(Value::as_str) == Some(name))
        .ok_or_else(|| error_reply(404, "not_found", "No such project."))
}

fn project_state(home: &HomeState, name: &str, now: f64) -> Reply {
    let entry = match find_project(home, name) {
        Ok(entry) => entry,
        Err(reply) => return reply,
    };
    match project_paths(&entry) {
        Some(paths) => match home.project(name, paths) {
            Some(project) => state_reply(&project, now, None),
            None => error_reply(500, "failed", "The project registry is not available."),
        },
        // A project that cannot take work shows an empty board. Its tab carries the reason.
        None => state_reply(&Project::new(Paths::nowhere()), now, None),
    }
}

fn project_park(home: &HomeState, name: &str, id: &str) -> Reply {
    let entry = match find_project(home, name) {
        Ok(entry) => entry,
        Err(reply) => return reply,
    };
    match project_paths(&entry) {
        Some(paths) => api_park(&paths, id),
        None => crate::server::text(404, "no park details for this job"),
    }
}

/// Checks Origin and Content-Type of a write. The Host was checked before.
fn check_write(request: &Request) -> Result<(), Reply> {
    let host = request.host.unwrap_or("");
    let expected = format!("http://{host}");
    if request.origin != Some(expected.as_str()) {
        return Err(error_reply(403, "forbidden", "A write must come from this page (Origin check)."));
    }
    let kind = request.content_type.unwrap_or("").split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    if kind != "application/json" {
        return Err(error_reply(415, "unsupported_media_type", "A write needs Content-Type: application/json."));
    }
    Ok(())
}

fn forward_write(home: &HomeState, request: &Request, api_path: &str) -> Reply {
    if let Err(reply) = check_write(request) {
        return reply;
    }
    let token = match read_token(&home.dir) {
        Ok(token) => token,
        Err(reply) => return reply,
    };
    match api_call(&home.api, "POST", api_path, &token, Some(request.body)) {
        Ok(reply) => forward(reply),
        Err(message) => call_failed(&message),
    }
}

/// Handles every `/api/projects` route. Other paths return `None` and fall through to the shared routes.
pub fn route(home: &HomeState, request: &Request, path: &str, now: f64) -> Option<Reply> {
    let rest = path.strip_prefix("/api/projects")?;
    let is_get = request.method == "GET";
    let is_post = request.method == "POST";
    let not_found = || error_reply(404, "not_found", "Not found.");
    let wrong_method = || error_reply(405, "method_not_allowed", "Method not allowed.");
    if rest.is_empty() {
        return Some(if is_get { api_projects(home) } else { wrong_method() });
    }
    let rest = rest.strip_prefix('/')?;
    let parts: Vec<&str> = rest.split('/').collect();
    let Some(name) = parts.first().copied().filter(|n| valid_project(n)) else { return Some(not_found()) };
    let reply = match parts.as_slice() {
        [_, "state"] => {
            if is_get { project_state(home, name, now) } else { wrong_method() }
        }
        [_, "jobs"] => {
            if is_post { forward_write(home, request, &format!("/projects/{name}/jobs")) } else { wrong_method() }
        }
        [_, "jobs", id, "park"] if valid_job_id(id) => {
            if is_get { project_park(home, name, id) } else { wrong_method() }
        }
        [_, "jobs", id, action @ ("approve" | "rework" | "cancel")] if valid_job_id(id) => {
            if is_post {
                forward_write(home, request, &format!("/projects/{name}/jobs/{id}/{action}"))
            } else {
                wrong_method()
            }
        }
        _ => not_found(),
    };
    Some(reply)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::{route_request, App, Request};
    use serde_json::{json, Value};
    use std::sync::{Arc, Mutex};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    const TOKEN: &str = "s3cr3t-token-value";

    #[derive(Debug, Clone)]
    struct Call {
        method: String,
        url: String,
        auth: Option<String>,
        body: String,
    }

    struct Fake {
        addr: String,
        calls: Arc<Mutex<Vec<Call>>>,
    }

    impl Fake {
        fn calls(&self) -> Vec<Call> {
            self.calls.lock().map(|c| c.clone()).unwrap_or_default()
        }
    }

    /// A stand-in for factoryd. `GET /projects` answers `projects`, a POST answers `post_status`.
    fn fake_api(projects: Value, post_status: u16, post_body: &str) -> Fake {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("bind fake api");
        let addr = format!("127.0.0.1:{}", server.server_addr().to_ip().expect("ip").port());
        let calls: Arc<Mutex<Vec<Call>>> = Arc::new(Mutex::new(Vec::new()));
        let record = Arc::clone(&calls);
        let post_body = post_body.to_string();
        std::thread::spawn(move || {
            for mut request in server.incoming_requests() {
                let auth = request
                    .headers()
                    .iter()
                    .find(|h| h.field.equiv("Authorization"))
                    .map(|h| h.value.as_str().to_string());
                let mut body = String::new();
                let _ = std::io::Read::read_to_string(request.as_reader(), &mut body);
                let method = request.method().as_str().to_string();
                let url = request.url().to_string();
                if let Ok(mut list) = record.lock() {
                    list.push(Call { method: method.clone(), url: url.clone(), auth: auth.clone(), body });
                }
                let authorized = auth.as_deref() == Some(&format!("Bearer {TOKEN}"));
                let (status, text) = if !authorized {
                    (401, r#"{"error":{"code":"unauthorized","message":"a valid bearer token is required"}}"#.to_string())
                } else if method == "GET" && url == "/projects" {
                    (200, projects.to_string())
                } else {
                    (post_status, post_body.clone())
                };
                let response = tiny_http::Response::from_string(text).with_status_code(status);
                let _ = request.respond(response);
            }
        });
        Fake { addr, calls }
    }

    struct Setup {
        _home: tempfile::TempDir,
        _repo: tempfile::TempDir,
        app: App,
        fake: Fake,
    }

    const JOB: &str = r#"{"jobs":[{"id":"001","dir":"2026-10-13-001-x","slug":"s","created":"2026-10-13","feature":"f","branch":"b","stage":"spec","round":0,"stage_since":1.0,"reviews":{}}]}"#;

    fn setup_with(token: &str, post_status: u16, post_body: &str) -> Result<Setup, Box<dyn std::error::Error>> {
        let home = tempfile::tempdir()?;
        std::fs::write(home.path().join("api.token"), format!("{token}\n"))?;
        let repo = tempfile::tempdir()?;
        let state = repo.path().join("state");
        std::fs::create_dir_all(&state)?;
        std::fs::write(state.join("board.json"), JOB)?;
        let plan = repo.path().join("jobs/2026-10-13-001-x");
        std::fs::create_dir_all(&plan)?;
        std::fs::write(plan.join("plan.md"), "# Plan: Alpha\n\n### Task 1: One\n")?;
        let projects = json!([
            {"name": "zeta", "repo": "/r/zeta", "preset": "rust", "baseBranch": "main", "status": "disabled",
             "reason": "project.toml is broken", "stateDir": null, "jobsDir": null, "activeJob": null},
            {"name": "alpha", "repo": "/r/alpha", "preset": "rust", "baseBranch": "main", "status": "ready",
             "reason": null, "stateDir": state, "jobsDir": repo.path().join("jobs"), "activeJob": null}
        ]);
        let fake = fake_api(projects, post_status, post_body);
        let app = App::for_home(home.path().to_path_buf(), fake.addr.clone());
        Ok(Setup { _home: home, _repo: repo, app, fake })
    }

    fn setup() -> Result<Setup, Box<dyn std::error::Error>> {
        setup_with(TOKEN, 201, r#"{"job":"004","requestId":null}"#)
    }

    const HOST: &str = "127.0.0.1:9";

    fn get(app: &App, url: &str) -> crate::server::Reply {
        let request = Request { method: "GET", url, host: Some(HOST), origin: None, content_type: None, body: &[] };
        route_request(app, &request, 9, 5.0)
    }

    fn post(app: &App, url: &str, host: Option<&str>, origin: Option<&str>, ctype: Option<&str>, body: &[u8]) -> crate::server::Reply {
        let request = Request { method: "POST", url, host, origin, content_type: ctype, body };
        route_request(app, &request, 9, 5.0)
    }

    fn good_post(app: &App, url: &str, body: &[u8]) -> crate::server::Reply {
        post(app, url, Some(HOST), Some("http://127.0.0.1:9"), Some("application/json"), body)
    }

    #[test]
    fn projects_merge_the_api_list() -> TestResult {
        let s = setup()?;
        let reply = get(&s.app, "/api/projects");
        assert_eq!(reply.status, 200);
        let json: Value = serde_json::from_slice(&reply.body)?;
        let names: Vec<&str> = json.as_array().ok_or("array")?.iter().filter_map(|p| p["name"].as_str()).collect();
        assert_eq!(names, vec!["alpha", "zeta"]);
        assert_eq!(json[1]["reason"], "project.toml is broken");
        assert_eq!(json[1]["status"], "disabled");
        Ok(())
    }

    #[test]
    fn a_disabled_project_has_an_empty_state() -> TestResult {
        let s = setup()?;
        let reply = get(&s.app, "/api/projects/zeta/state");
        assert_eq!(reply.status, 200);
        let json: Value = serde_json::from_slice(&reply.body)?;
        assert_eq!(json["jobs"].as_array().map(|j| j.len()), Some(0));
        assert!(json["board_error"].is_null());
        Ok(())
    }

    #[test]
    fn a_ready_project_state_reads_the_api_folders() -> TestResult {
        let s = setup()?;
        let reply = get(&s.app, "/api/projects/alpha/state");
        assert_eq!(reply.status, 200);
        let json: Value = serde_json::from_slice(&reply.body)?;
        assert_eq!(json["jobs"][0]["id"], "001");
        assert_eq!(json["jobs"][0]["plan"]["title"], "Alpha");
        Ok(())
    }

    #[test]
    fn an_unknown_or_malformed_project_is_not_found() -> TestResult {
        let s = setup()?;
        assert_eq!(get(&s.app, "/api/projects/nope/state").status, 404);
        assert_eq!(get(&s.app, "/api/projects/a%2Fb/state").status, 404);
        assert_eq!(get(&s.app, "/api/projects/../state").status, 404);
        assert_eq!(get(&s.app, "/api/projects/alpha/jobs/zz/park").status, 404);
        Ok(())
    }

    #[test]
    fn the_park_route_reads_the_job_folder() -> TestResult {
        let s = setup()?;
        assert_eq!(get(&s.app, "/api/projects/alpha/jobs/001/park").status, 404);
        Ok(())
    }

    #[test]
    fn the_token_goes_to_the_api_and_never_into_a_reply() -> TestResult {
        let s = setup()?;
        let replies = [
            get(&s.app, "/api/projects"),
            get(&s.app, "/api/projects/alpha/state"),
            get(&s.app, "/"),
            get(&s.app, "/app.js"),
            good_post(&s.app, "/api/projects/alpha/jobs", br#"{"spec":"/a","plans":"/b"}"#),
        ];
        let calls = s.fake.calls();
        assert!(!calls.is_empty());
        for call in &calls {
            assert_eq!(call.auth.as_deref(), Some(format!("Bearer {TOKEN}").as_str()));
        }
        for reply in replies {
            assert!(!String::from_utf8_lossy(&reply.body).contains(TOKEN));
            assert!(!reply.content_type.contains(TOKEN));
        }
        Ok(())
    }

    #[test]
    fn an_api_401_shows_a_token_rejected_error() -> TestResult {
        let s = setup_with("another-token", 201, "{}")?;
        let reply = get(&s.app, "/api/projects");
        assert_eq!(reply.status, 502);
        let json: Value = serde_json::from_slice(&reply.body)?;
        assert_eq!(json["error"]["code"], "token_rejected");
        assert!(!String::from_utf8_lossy(&reply.body).contains("another-token"));
        let write = good_post(&s.app, "/api/projects/alpha/jobs/001/approve", b"");
        let json: Value = serde_json::from_slice(&write.body)?;
        assert_eq!(json["error"]["code"], "token_rejected");
        Ok(())
    }

    #[test]
    fn a_missing_token_file_or_dead_api_gives_a_clear_error() -> TestResult {
        let home = tempfile::tempdir()?;
        let app = App::for_home(home.path().to_path_buf(), "127.0.0.1:1".to_string());
        let json: Value = serde_json::from_slice(&get(&app, "/api/projects").body)?;
        assert_eq!(json["error"]["code"], "token_missing");
        std::fs::write(home.path().join("api.token"), TOKEN)?;
        let reply = get(&app, "/api/projects");
        assert_eq!(reply.status, 502);
        let json: Value = serde_json::from_slice(&reply.body)?;
        assert_eq!(json["error"]["code"], "api_unreachable");
        Ok(())
    }

    #[test]
    fn refused_writes_never_reach_the_api() -> TestResult {
        let s = setup()?;
        let url = "/api/projects/alpha/jobs";
        let body = br#"{"spec":"/a","plans":"/b"}"#;
        let good_origin = Some("http://127.0.0.1:9");
        let json = Some("application/json");
        let refused = [
            post(&s.app, url, Some(HOST), None, json, body),
            post(&s.app, url, Some(HOST), Some("http://evil.example"), json, body),
            post(&s.app, url, Some(HOST), Some("http://localhost:9"), json, body),
            post(&s.app, url, Some(HOST), Some("null"), json, body),
            post(&s.app, url, Some(HOST), good_origin, None, body),
            post(&s.app, url, Some(HOST), good_origin, Some("text/plain"), body),
            post(&s.app, url, Some(HOST), good_origin, Some("application/x-www-form-urlencoded"), body),
            post(&s.app, url, Some("evil.example"), Some("http://evil.example"), json, body),
            post(&s.app, url, None, good_origin, json, body),
            post(&s.app, "/api/projects/alpha/jobs/001/approve", Some(HOST), None, json, b""),
        ];
        for reply in &refused {
            assert!(reply.status == 403 || reply.status == 415, "status {}", reply.status);
        }
        assert!(s.fake.calls().is_empty(), "calls: {:?}", s.fake.calls());
        Ok(())
    }

    #[test]
    fn a_valid_write_is_forwarded_unchanged() -> TestResult {
        let s = setup()?;
        let body = br#"{"spec":"/a/spec.md","plans":"/a/plans","run":true,"series":false}"#;
        let reply = post(&s.app, "/api/projects/alpha/jobs", Some(HOST), Some("http://127.0.0.1:9"), Some("application/json; charset=utf-8"), body);
        assert_eq!(reply.status, 201);
        let json: Value = serde_json::from_slice(&reply.body)?;
        assert_eq!(json["job"], "004");
        let calls = s.fake.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].method, "POST");
        assert_eq!(calls[0].url, "/projects/alpha/jobs");
        assert_eq!(calls[0].body.as_bytes(), body);
        for action in ["approve", "rework", "cancel"] {
            let url = format!("/api/projects/alpha/jobs/003/{action}");
            assert_eq!(good_post(&s.app, &url, br#"{"note":"x"}"#).status, 201);
        }
        let urls: Vec<String> = s.fake.calls().iter().map(|c| c.url.clone()).collect();
        assert!(urls.contains(&"/projects/alpha/jobs/003/rework".to_string()));
        Ok(())
    }

    #[test]
    fn api_errors_keep_their_code_and_status() -> TestResult {
        let s = setup_with(TOKEN, 409, r#"{"error":{"code":"project_disabled","message":"nope"}}"#)?;
        let reply = good_post(&s.app, "/api/projects/alpha/jobs", b"{}");
        assert_eq!(reply.status, 409);
        let json: Value = serde_json::from_slice(&reply.body)?;
        assert_eq!(json["error"]["code"], "project_disabled");
        Ok(())
    }

    #[test]
    fn writes_only_accept_known_routes_and_methods() -> TestResult {
        let s = setup()?;
        assert_eq!(good_post(&s.app, "/api/projects/alpha/jobs/abc/approve", b"").status, 404);
        assert_eq!(good_post(&s.app, "/api/projects/alpha/jobs/001/delete", b"").status, 404);
        assert_eq!(good_post(&s.app, "/api/projects", b"").status, 405);
        assert_eq!(good_post(&s.app, "/api/projects/alpha/state", b"").status, 405);
        let put = Request { method: "PUT", url: "/api/projects/alpha/jobs", host: Some(HOST), origin: None, content_type: None, body: &[] };
        assert_eq!(route_request(&s.app, &put, 9, 5.0).status, 405);
        assert!(s.fake.calls().is_empty());
        Ok(())
    }

    #[test]
    fn the_api_address_must_be_loopback() {
        assert!(valid_api_addr("127.0.0.1:8766"));
        assert!(valid_api_addr("localhost:8766"));
        assert!(valid_api_addr("[::1]:8766"));
        assert!(!valid_api_addr("example.com:8766"));
        assert!(!valid_api_addr("10.0.0.1:8766"));
        assert!(!valid_api_addr("127.0.0.1"));
        assert!(!valid_api_addr("127.0.0.1:x"));
    }

    #[test]
    fn the_project_and_job_names_are_checked() {
        assert!(valid_project("rocket-api_2.x"));
        assert!(!valid_project(""));
        assert!(!valid_project(".."));
        assert!(!valid_project("a/b"));
        assert!(!valid_project(&"a".repeat(65)));
        assert!(valid_job_id("001"));
        assert!(!valid_job_id(""));
        assert!(!valid_job_id("1a"));
        assert!(!valid_job_id("../1"));
    }

    #[test]
    fn replies_are_parsed_plain_and_chunked() -> TestResult {
        let plain = parse_response(b"HTTP/1.1 201 Created\r\nContent-Length: 2\r\n\r\n{}")?;
        assert_eq!((plain.status, plain.body.as_slice()), (201, &b"{}"[..]));
        let chunked = parse_response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n")?;
        assert_eq!(chunked.body, b"abcde");
        assert!(parse_response(b"garbage").is_err());
        // An oversized chunk size must be an error, never a panic.
        let huge = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nffffffffffffffff\r\nabc\r\n0\r\n\r\n";
        assert!(parse_response(huge).is_err());
        assert!(parse_response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n").is_err());
        let short = b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\n{}";
        assert!(parse_response(short).is_err());
        Ok(())
    }

    #[test]
    fn a_token_that_could_split_a_header_is_refused() -> TestResult {
        let home = tempfile::tempdir()?;
        std::fs::write(home.path().join("api.token"), "abc\r\nX-Evil: 1")?;
        assert!(read_token(home.path()).is_err());
        std::fs::write(home.path().join("api.token"), "")?;
        assert!(read_token(home.path()).is_err());
        std::fs::write(home.path().join("api.token"), "good-token\n")?;
        assert_eq!(read_token(home.path()).ok().as_deref(), Some("good-token"));
        Ok(())
    }

    #[test]
    fn the_raw_socket_reply_never_carries_the_token() -> TestResult {
        use std::io::{Read, Write};
        let s = setup()?;
        let server = tiny_http::Server::http("127.0.0.1:0").map_err(|e| e.to_string())?;
        let port = server.server_addr().to_ip().ok_or("ip")?.port();
        let app = Arc::new(s.app);
        let served = Arc::clone(&app);
        std::thread::spawn(move || crate::server::serve(server, served, port));
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port))?;
        write!(stream, "GET /api/projects HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n")?;
        let mut out = String::new();
        stream.read_to_string(&mut out)?;
        assert!(out.starts_with("HTTP/1.1 200"), "got: {out}");
        assert!(!out.contains(TOKEN));
        assert!(out.to_lowercase().contains("no-store"));
        let body = r#"{"spec":"/a","plans":"/b"}"#;
        let write = |origin: &str| -> Result<String, Box<dyn std::error::Error>> {
            let mut stream = std::net::TcpStream::connect(("127.0.0.1", port))?;
            write!(stream, "POST /api/projects/alpha/jobs HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: {origin}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())?;
            let mut out = String::new();
            stream.read_to_string(&mut out)?;
            Ok(out)
        };
        let before = s.fake.calls().len();
        assert!(write("http://evil.example")?.starts_with("HTTP/1.1 403"));
        assert_eq!(s.fake.calls().len(), before);
        assert!(write(&format!("http://127.0.0.1:{port}"))?.starts_with("HTTP/1.1 201"));
        assert_eq!(s.fake.calls().len(), before + 1);
        Ok(())
    }
}
