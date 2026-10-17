use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::events::{pump, Registry, Tailer, LOG_WINDOW_BYTES};
use crate::home_mode;
use crate::park::{load_park, ParkReply};
use crate::paths::Paths;
use crate::state::{build_state, load_board};

const DASHBOARD_HTML: &str = include_str!("../static/dashboard.html");
const DASHBOARD_CSS: &str = include_str!("../static/dashboard.css");
const BOARD_JS: &str = include_str!("../static/board.js");
const PATCH_JS: &str = include_str!("../static/patch.js");
const PROJECTS_JS: &str = include_str!("../static/projects.js");
const APP_JS: &str = include_str!("../static/app.js");
const POLL_INTERVAL: Duration = Duration::from_millis(500);
/// The most bytes of a request body the server reads. The factory API has the same limit.
pub const MAX_BODY: usize = 64 * 1024;
/// The factory API address used when `--api` is absent.
pub const DEFAULT_API: &str = "127.0.0.1:8766";

pub struct Config {
    pub root: PathBuf,
    pub port: u16,
    /// Set by `--home`: the factory home to read the API token from.
    pub home: Option<PathBuf>,
    pub api: String,
}

/// One project's folders and the agent registry its event log feeds.
pub struct Project {
    pub paths: Paths,
    pub registry: Mutex<Registry>,
}

impl Project {
    pub fn new(paths: Paths) -> Project {
        Project { paths, registry: Mutex::new(Registry::new()) }
    }
}

/// Home mode state: the factory home, the API address and one project value per name.
pub struct HomeState {
    pub dir: PathBuf,
    pub api: String,
    pub timeouts: home_mode::Timeouts,
    projects: Mutex<HashMap<String, Arc<Project>>>,
}

impl HomeState {
    /// The project for `name`, made on first use. A project whose folders changed gets a new value.
    pub fn project(&self, name: &str, paths: Paths) -> Option<Arc<Project>> {
        let mut map = self.projects.lock().ok()?;
        if let Some(existing) = map.get(name) {
            if existing.paths == paths {
                return Some(Arc::clone(existing));
            }
        }
        let project = Arc::new(Project::new(paths));
        spawn_tailer(Arc::downgrade(&project));
        map.insert(name.to_string(), Arc::clone(&project));
        Some(project)
    }
}

pub enum Mode {
    Root(Arc<Project>),
    Home(HomeState),
}

pub struct App {
    pub mode: Mode,
}

impl App {
    pub fn for_root(root: &Path) -> App {
        App { mode: Mode::Root(Arc::new(Project::new(Paths::for_root(root)))) }
    }

    pub fn for_home(dir: PathBuf, api: String) -> App {
        App { mode: Mode::Home(HomeState { dir, api, timeouts: home_mode::Timeouts::default(), projects: Mutex::new(HashMap::new()) }) }
    }

    /// Shorter timeouts for tests.
    #[cfg(test)]
    pub fn with_timeouts(mut self, timeouts: home_mode::Timeouts) -> App {
        if let Mode::Home(home) = &mut self.mode {
            home.timeouts = timeouts;
        }
        self
    }

    /// The single project of `--root` mode.
    #[cfg(test)]
    pub fn root_project(&self) -> Option<&Arc<Project>> {
        match &self.mode {
            Mode::Root(project) => Some(project),
            Mode::Home(_) => None,
        }
    }
}

pub struct Reply {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

/// What `route_request` needs to know about one request.
pub struct Request<'a> {
    pub method: &'a str,
    pub url: &'a str,
    pub host: Option<&'a str>,
    pub origin: Option<&'a str>,
    pub content_type: Option<&'a str>,
    pub body: &'a [u8],
}

/// Return a warning line when the factory data folder is missing under the root.
pub fn missing_factory_dir_warning(root: &Path) -> Option<String> {
    let dir = root.join(".superpowers").join("factory");
    if dir.exists() {
        return None;
    }
    Some(format!(
        "factory-server: warning: {} does not exist yet; the board stays empty until a factory job or hook event creates it (is --root the main checkout?)",
        dir.display()
    ))
}

pub fn parse_args(args: &[String]) -> Result<Config, String> {
    let mut root: Option<PathBuf> = None;
    let mut home: Option<PathBuf> = None;
    let mut api: Option<String> = None;
    let mut port: u16 = 8765;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--root" => {
                root = Some(PathBuf::from(iter.next().ok_or("--root needs a path")?));
            }
            "--home" => {
                home = Some(PathBuf::from(iter.next().ok_or("--home needs a path")?));
            }
            "--api" => {
                api = Some(iter.next().ok_or("--api needs host:port")?.clone());
            }
            "--port" => {
                let raw = iter.next().ok_or("--port needs a number")?;
                port = raw
                    .parse::<u16>()
                    .map_err(|_| format!("bad port {raw:?}"))?;
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    if home.is_some() && root.is_some() {
        return Err("--home and --root cannot be used together".to_string());
    }
    if api.is_some() && home.is_none() {
        return Err("--api only works with --home".to_string());
    }
    let api = api.unwrap_or_else(|| DEFAULT_API.to_string());
    if !home_mode::valid_api_addr(&api) {
        return Err(format!("--api {api:?} must be a loopback host:port"));
    }
    Ok(Config { root: root.unwrap_or_else(|| PathBuf::from(".")), port, home, api })
}

pub fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs_f64())
        .unwrap_or(0.0)
}

/// Only the loopback names with our own port are accepted, which blocks DNS-rebinding requests.
pub fn host_allowed(host: Option<&str>, port: u16) -> bool {
    match host {
        Some(value) => value == format!("127.0.0.1:{port}") || value == format!("localhost:{port}"),
        None => false,
    }
}

pub fn text(status: u16, body: &str) -> Reply {
    Reply {
        status,
        content_type: "text/plain; charset=utf-8",
        body: body.as_bytes().to_vec(),
    }
}

fn asset(content_type: &'static str, body: &str) -> Reply {
    Reply {
        status: 200,
        content_type,
        body: body.as_bytes().to_vec(),
    }
}

pub fn json_reply(status: u16, value: &impl serde::Serialize) -> Reply {
    match serde_json::to_vec(value) {
        Ok(body) => Reply { status, content_type: "application/json", body },
        Err(_) => text(500, "could not encode the reply"),
    }
}

/// A request without a body or an Origin, which is all most tests need.
#[cfg(test)]
pub fn route(app: &App, method: &str, url: &str, host: Option<&str>, port: u16, now: f64) -> Reply {
    let request = Request { method, url, host, origin: None, content_type: None, body: &[] };
    route_request(app, &request, port, now)
}

pub fn route_request(app: &App, request: &Request, port: u16, now: f64) -> Reply {
    if !host_allowed(request.host, port) {
        return text(403, "forbidden host");
    }
    let path = request.url.split('?').next().unwrap_or("/");
    if let Mode::Home(home) = &app.mode {
        if let Some(reply) = home_mode::route(home, request, path, now) {
            return reply;
        }
    }
    if request.method != "GET" {
        return text(405, "method not allowed");
    }
    if let Mode::Root(project) = &app.mode {
        if let Some(id) = path
            .strip_prefix("/api/jobs/")
            .and_then(|rest| rest.strip_suffix("/park"))
        {
            return api_park(&project.paths, id);
        }
    }
    match path {
        "/" | "/dashboard.html" => Reply {
            status: 200,
            content_type: "text/html; charset=utf-8",
            body: DASHBOARD_HTML.as_bytes().to_vec(),
        },
        "/dashboard.css" => asset("text/css; charset=utf-8", DASHBOARD_CSS),
        "/board.js" => asset("text/javascript; charset=utf-8", BOARD_JS),
        "/patch.js" => asset("text/javascript; charset=utf-8", PATCH_JS),
        "/projects.js" => asset("text/javascript; charset=utf-8", PROJECTS_JS),
        "/app.js" => asset("text/javascript; charset=utf-8", APP_JS),
        "/healthz" => text(200, "ok"),
        "/api/state" => match &app.mode {
            Mode::Root(project) => state_reply(project, now, None),
            Mode::Home(_) => text(404, "not found"),
        },
        _ => text(404, "not found"),
    }
}

/// The park details of one job under `paths`.
pub fn api_park(paths: &Paths, id: &str) -> Reply {
    let (board, _) = load_board(&paths.board);
    match load_park(paths, &board, id) {
        ParkReply::Found(view) => match serde_json::to_vec(&view) {
            Ok(body) => Reply {
                status: 200,
                content_type: "application/json",
                body,
            },
            Err(_) => text(500, "could not encode park details"),
        },
        ParkReply::NotFound => text(404, "no park details for this job"),
        ParkReply::Broken => text(422, "park details are not readable"),
    }
}

/// The state view of one project. `board_note` replaces a missing board error.
pub fn state_reply(project: &Project, now: f64, board_note: Option<String>) -> Reply {
    let (board, board_error) = load_board(&project.paths.board);
    let view = match project.registry.lock() {
        Ok(registry) => build_state(&board, &registry, now, board_error.or(board_note), &project.paths),
        Err(_) => return text(500, "state lock poisoned"),
    };
    json_reply(200, &view)
}

/// Content security policy for every reply.
pub const CSP: &str = "default-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'";

/// Tails the event log in a background thread and feeds the registry. It ends when the project is dropped.
pub fn spawn_tailer(project: Weak<Project>) {
    let Some(first) = project.upgrade() else { return };
    let path = first.paths.events.clone();
    drop(first);
    std::thread::spawn(move || {
        let mut tailer = Tailer::with_window(path, LOG_WINDOW_BYTES);
        loop {
            let Some(project) = project.upgrade() else { return };
            pump(&mut tailer, &project.registry, now_secs());
            drop(project);
            std::thread::sleep(POLL_INTERVAL);
        }
    });
}

fn header_value(request: &tiny_http::Request, name: &str) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|header| header.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|header| header.value.as_str().to_string())
}

pub fn serve(server: tiny_http::Server, app: Arc<App>, port: u16) {
    for mut request in server.incoming_requests() {
        let host = header_value(&request, "Host");
        let origin = header_value(&request, "Origin");
        let content_type = header_value(&request, "Content-Type");
        let method = request.method().as_str().to_string();
        let url = request.url().to_string();
        let mut body = Vec::new();
        let mut too_big = false;
        if method == "POST" {
            let limit = (MAX_BODY as u64) + 1;
            let _ = request.as_reader().take(limit).read_to_end(&mut body);
            too_big = body.len() > MAX_BODY;
        }
        let reply = if too_big {
            text(413, "request body too large")
        } else {
            let parsed = Request {
                method: &method,
                url: &url,
                host: host.as_deref(),
                origin: origin.as_deref(),
                content_type: content_type.as_deref(),
                body: &body,
            };
            route_request(&app, &parsed, port, now_secs())
        };
        let mut response =
            tiny_http::Response::from_data(reply.body).with_status_code(reply.status);
        if let Ok(header) =
            tiny_http::Header::from_bytes(&b"Content-Type"[..], reply.content_type.as_bytes())
        {
            response.add_header(header);
        }
        if let Ok(header) = tiny_http::Header::from_bytes(&b"Cache-Control"[..], &b"no-store"[..]) {
            response.add_header(header);
        }
        let security: [(&[u8], &[u8]); 3] = [
            (&b"X-Content-Type-Options"[..], &b"nosniff"[..]),
            (
                &b"Content-Security-Policy"[..],
                CSP.as_bytes(),
            ),
            (&b"Referrer-Policy"[..], &b"no-referrer"[..]),
        ];
        for (name, value) in security {
            if let Ok(header) = tiny_http::Header::from_bytes(name, value) {
                response.add_header(header);
            }
        }
        let _ = request.respond(response);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpStream;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn warns_only_when_the_factory_folder_is_missing() -> TestResult {
        let dir = tempfile::tempdir()?;
        let warning = missing_factory_dir_warning(dir.path());
        assert!(warning.is_some_and(|text| text.contains("does not exist yet")));
        std::fs::create_dir_all(dir.path().join(".superpowers").join("factory"))?;
        assert!(missing_factory_dir_warning(dir.path()).is_none());
        Ok(())
    }

    const JOB: &str = r#"{"jobs":[{"id":"001","dir":"d","slug":"s","created":"2026-10-13","feature":"f","branch":"b","stage":"spec","round":0,"stage_since":1.0,"reviews":{"security":"pending","ux":"pending","ui":"pending","code":"pending"}}]}"#;

    fn app_with_board(
        board: Option<&str>,
    ) -> Result<(tempfile::TempDir, Arc<App>), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let factory = dir.path().join(".superpowers").join("factory");
        std::fs::create_dir_all(&factory)?;
        if let Some(text) = board {
            std::fs::write(factory.join("board.json"), text)?;
        }
        let app = Arc::new(App::for_root(dir.path()));
        Ok((dir, app))
    }

    fn http_get(port: u16, path: &str, host: &str) -> Result<String, std::io::Error> {
        let mut stream = TcpStream::connect(("127.0.0.1", port))?;
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
        )?;
        let mut out = String::new();
        stream.read_to_string(&mut out)?;
        Ok(out)
    }

    #[test]
    fn parse_args_defaults_and_overrides() {
        let none: Vec<String> = Vec::new();
        let config = parse_args(&none).expect("defaults parse");
        assert_eq!(config.port, 8765);
        assert_eq!(config.root, PathBuf::from("."));
        let args: Vec<String> = ["--root", "/r", "--port", "9000"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let config = parse_args(&args).expect("overrides parse");
        assert_eq!(config.port, 9000);
        assert_eq!(config.root, PathBuf::from("/r"));
    }

    #[test]
    fn parse_args_rejects_bad_input() {
        let bad_port: Vec<String> = ["--port", "abc"].iter().map(|s| s.to_string()).collect();
        assert!(parse_args(&bad_port).is_err());
        let missing: Vec<String> = ["--root"].iter().map(|s| s.to_string()).collect();
        assert!(parse_args(&missing).is_err());
        let unknown: Vec<String> = ["--nope"].iter().map(|s| s.to_string()).collect();
        assert!(parse_args(&unknown).is_err());
    }

    #[test]
    fn only_local_hosts_with_the_right_port_are_allowed() {
        assert!(host_allowed(Some("127.0.0.1:8765"), 8765));
        assert!(host_allowed(Some("localhost:8765"), 8765));
        assert!(!host_allowed(Some("evil.example"), 8765));
        assert!(!host_allowed(Some("127.0.0.1:9999"), 8765));
        assert!(!host_allowed(None, 8765));
    }

    #[test]
    fn routes_answer_the_expected_status_codes() -> TestResult {
        let (_dir, app) = app_with_board(Some(JOB))?;
        let host = Some("127.0.0.1:1");
        assert_eq!(route(&app, "GET", "/healthz", host, 1, 5.0).status, 200);
        assert_eq!(route(&app, "GET", "/", host, 1, 5.0).status, 200);
        assert_eq!(
            route(&app, "GET", "/dashboard.html?x=1", host, 1, 5.0).status,
            200
        );
        assert_eq!(route(&app, "GET", "/nope", host, 1, 5.0).status, 404);
        assert_eq!(route(&app, "POST", "/api/state", host, 1, 5.0).status, 405);
        assert_eq!(
            route(&app, "GET", "/api/state", Some("evil.example"), 1, 5.0).status,
            403
        );
        Ok(())
    }

    #[test]
    fn api_state_joins_the_board_and_the_registry() -> TestResult {
        let (_dir, app) = app_with_board(Some(JOB))?;
        let project = app.root_project().ok_or("no root project")?;
        if let Ok(mut registry) = project.registry.lock() {
            registry.ingest_line(
                r#"{"ts":2.0,"hook_event_name":"SubagentStart","agent_id":"s1","agent_type":"factory-spec-writer","cwd":"/r","job_id":"001"}"#,
            );
        }
        let reply = route(&app, "GET", "/api/state", Some("127.0.0.1:1"), 1, 5.0);
        assert_eq!(reply.status, 200);
        let json: serde_json::Value = serde_json::from_slice(&reply.body)?;
        assert_eq!(json["jobs"][0]["id"], "001");
        assert_eq!(
            json["jobs"][0]["agents"][0]["agent_type"],
            "factory-spec-writer"
        );
        assert!(json["board_error"].is_null());
        Ok(())
    }

    #[test]
    fn api_state_without_a_board_file_has_no_jobs_and_no_error() -> TestResult {
        let (_dir, app) = app_with_board(None)?;
        let reply = route(&app, "GET", "/api/state", Some("127.0.0.1:1"), 1, 5.0);
        let json: serde_json::Value = serde_json::from_slice(&reply.body)?;
        assert_eq!(json["jobs"].as_array().map(|jobs| jobs.len()), Some(0));
        assert!(json["board_error"].is_null());
        Ok(())
    }

    #[test]
    fn api_state_reports_an_invalid_board_file() -> TestResult {
        let (_dir, app) = app_with_board(Some("{ nope"))?;
        let reply = route(&app, "GET", "/api/state", Some("127.0.0.1:1"), 1, 5.0);
        let json: serde_json::Value = serde_json::from_slice(&reply.body)?;
        assert!(json["board_error"]
            .as_str()
            .is_some_and(|text| text.contains("invalid")));
        Ok(())
    }

    #[test]
    fn serves_state_over_a_real_socket_and_rejects_a_foreign_host() -> TestResult {
        let (_dir, app) = app_with_board(Some(JOB))?;
        let server = tiny_http::Server::http("127.0.0.1:0").map_err(|e| e.to_string())?;
        let port = server.server_addr().to_ip().ok_or("no ip address")?.port();
        let served = Arc::clone(&app);
        std::thread::spawn(move || serve(server, served, port));
        let good = http_get(port, "/api/state", &format!("127.0.0.1:{port}"))?;
        assert!(good.starts_with("HTTP/1.1 200"), "got: {good}");
        assert!(good.contains(r#""id":"001""#));
        let bad = http_get(port, "/api/state", "evil.example")?;
        assert!(bad.starts_with("HTTP/1.1 403"), "got: {bad}");
        Ok(())
    }

    #[test]
    fn static_assets_are_served_with_the_right_content_type() -> TestResult {
        let (_dir, app) = app_with_board(None)?;
        let host = Some("127.0.0.1:1");
        let css = route(&app, "GET", "/dashboard.css", host, 1, 5.0);
        assert_eq!(css.status, 200);
        assert_eq!(css.content_type, "text/css; charset=utf-8");
        let board = route(&app, "GET", "/board.js", host, 1, 5.0);
        assert_eq!(board.status, 200);
        assert_eq!(board.content_type, "text/javascript; charset=utf-8");
        let patch = route(&app, "GET", "/patch.js", host, 1, 5.0);
        assert_eq!(patch.status, 200);
        assert_eq!(patch.content_type, "text/javascript; charset=utf-8");
        let app_js = route(&app, "GET", "/app.js", host, 1, 5.0);
        assert_eq!(app_js.status, 200);
        let page = route(&app, "GET", "/", host, 1, 5.0);
        assert!(String::from_utf8(page.body)?.contains("/board.js"));
        Ok(())
    }

    #[test]
    fn replies_carry_security_headers() -> TestResult {
        let (_dir, app) = app_with_board(Some(JOB))?;
        let server = tiny_http::Server::http("127.0.0.1:0").map_err(|e| e.to_string())?;
        let port = server.server_addr().to_ip().ok_or("no ip address")?.port();
        let served = Arc::clone(&app);
        std::thread::spawn(move || serve(server, served, port));
        let reply = http_get(port, "/", &format!("127.0.0.1:{port}"))?.to_lowercase();
        assert!(
            reply.contains("x-content-type-options: nosniff"),
            "got: {reply}"
        );
        assert!(reply.contains("frame-ancestors 'none'"), "got: {reply}");
        assert!(reply.contains("default-src 'self'"), "got: {reply}");
        assert!(reply.contains("base-uri 'none'"), "got: {reply}");
        assert!(reply.contains("form-action 'none'"), "got: {reply}");
        Ok(())
    }

    #[test]
    fn the_csp_blocks_base_tags_and_form_posts() {
        for part in ["default-src 'self'", "frame-ancestors 'none'", "base-uri 'none'", "form-action 'none'"] {
            assert!(CSP.contains(part), "missing {part}");
        }
    }

    #[test]
    fn the_park_route_answers_200_404_422_and_refuses_other_methods() -> TestResult {
        let (dir, app) = app_with_board(Some(JOB))?;
        let host = Some("127.0.0.1:1");
        // No file yet.
        assert_eq!(
            route(&app, "GET", "/api/jobs/001/park", host, 1, 5.0).status,
            404
        );
        let folder = dir.path().join(".superpowers/sdd/d");
        std::fs::create_dir_all(&folder)?;
        std::fs::write(
            folder.join("park.json"),
            r#"{"kind":"checks-failed","reason":"why","at":5,"round":1,"next":[],"parts":[]}"#,
        )?;
        let ok = route(&app, "GET", "/api/jobs/001/park?x=1", host, 1, 5.0);
        assert_eq!(ok.status, 200);
        assert_eq!(ok.content_type, "application/json");
        let json: serde_json::Value = serde_json::from_slice(&ok.body)?;
        assert_eq!(json["kind"], "checks-failed");
        std::fs::write(folder.join("park.json"), "not json")?;
        assert_eq!(
            route(&app, "GET", "/api/jobs/001/park", host, 1, 5.0).status,
            422
        );
        assert_eq!(
            route(&app, "POST", "/api/jobs/001/park", host, 1, 5.0).status,
            405
        );
        assert_eq!(
            route(
                &app,
                "GET",
                "/api/jobs/001/park",
                Some("evil.example"),
                1,
                5.0
            )
            .status,
            403
        );
        assert_eq!(
            route(&app, "GET", "/api/jobs/zzz/park", host, 1, 5.0).status,
            404
        );
        assert_eq!(
            route(&app, "GET", "/api/jobs/001", host, 1, 5.0).status,
            404
        );
        assert_eq!(
            route(&app, "GET", "/api/jobs/001/park/extra", host, 1, 5.0).status,
            404
        );
        Ok(())
    }

    #[test]
    fn the_page_carries_the_park_dialog_and_its_script_and_styles() -> TestResult {
        let (_dir, app) = app_with_board(Some(JOB))?;
        let host = Some("127.0.0.1:1");
        let page = route(&app, "GET", "/", host, 1, 5.0);
        let html = String::from_utf8(page.body)?;
        assert!(html.contains(r#"<dialog id="park-dialog""#), "dialog element");
        assert!(html.contains(r#"id="park-content""#), "content holder");
        let app_js = String::from_utf8(route(&app, "GET", "/app.js", host, 1, 5.0).body)?;
        assert!(app_js.contains("/park") && app_js.contains("showModal"), "controller");
        let css = String::from_utf8(route(&app, "GET", "/dashboard.css", host, 1, 5.0).body)?;
        assert!(css.contains(".park-dialog") && css.contains(".ln-hit"), "styles");
        Ok(())
    }

    #[test]
    fn the_page_carries_the_cost_panel_and_its_script_and_styles() -> TestResult {
        let (_dir, app) = app_with_board(Some(JOB))?;
        let host = Some("127.0.0.1:1");
        let html = String::from_utf8(route(&app, "GET", "/", host, 1, 5.0).body)?;
        assert!(html.contains(r#"id="costs""#), "costs section");
        let app_js = String::from_utf8(route(&app, "GET", "/app.js", host, 1, 5.0).body)?;
        assert!(app_js.contains("renderCostPanel") && app_js.contains(".role-seg"), "controller");
        let css = String::from_utf8(route(&app, "GET", "/dashboard.css", host, 1, 5.0).body)?;
        assert!(css.contains(".cost-table") && css.contains(".role-bar") && css.contains(".estimate"), "styles");
        Ok(())
    }

    #[test]
    fn home_and_root_cannot_be_combined() {
        let args = |list: &[&str]| -> Vec<String> { list.iter().map(|s| s.to_string()).collect() };
        assert!(parse_args(&args(&["--home", "/h", "--root", "/r"])).is_err());
        assert!(parse_args(&args(&["--root", "/r", "--home", "/h"])).is_err());
        let config = parse_args(&args(&["--home", "/h", "--api", "127.0.0.1:9000"])).expect("home parses");
        assert_eq!(config.home, Some(PathBuf::from("/h")));
        assert_eq!(config.api, "127.0.0.1:9000");
        assert_eq!(parse_args(&args(&["--home", "/h"])).expect("default api").api, DEFAULT_API);
        assert!(parse_args(&args(&["--home", "/h", "--api", "example.com:80"])).is_err());
        assert!(parse_args(&args(&["--api", "127.0.0.1:9000"])).is_err());
    }

    #[test]
    fn root_mode_has_no_project_routes_and_refuses_writes() -> TestResult {
        let (_dir, app) = app_with_board(Some(JOB))?;
        let host = Some("127.0.0.1:1");
        assert_eq!(route(&app, "GET", "/api/projects", host, 1, 5.0).status, 404);
        assert_eq!(route(&app, "GET", "/api/usage", host, 1, 5.0).status, 404);
        assert_eq!(route(&app, "POST", "/api/projects/x/jobs", host, 1, 5.0).status, 405);
        Ok(())
    }

    #[test]
    fn the_page_carries_the_home_mode_pieces() -> TestResult {
        let (_dir, app) = app_with_board(None)?;
        let host = Some("127.0.0.1:1");
        let html = String::from_utf8(route(&app, "GET", "/", host, 1, 5.0).body)?;
        for part in ["/projects.js", r#"id="project-tabs""#, r#"id="banner""#, r#"id="newjob""#, r#"id="nj-series""#, r#"id="act-dialog""#, r#"id="budget""#] {
            assert!(html.contains(part), "missing {part}");
        }
        let projects = route(&app, "GET", "/projects.js", host, 1, 5.0);
        assert_eq!(projects.status, 200);
        assert_eq!(projects.content_type, "text/javascript; charset=utf-8");
        Ok(())
    }
}
