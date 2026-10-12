use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::events::{Registry, Tailer};
use crate::state::{build_state, load_board};

const DASHBOARD_HTML: &str = include_str!("../static/dashboard.html");
const DASHBOARD_CSS: &str = include_str!("../static/dashboard.css");
const BOARD_JS: &str = include_str!("../static/board.js");
const APP_JS: &str = include_str!("../static/app.js");
const POLL_INTERVAL: Duration = Duration::from_millis(500);

pub struct Config {
    pub root: PathBuf,
    pub port: u16,
}

pub struct App {
    pub root: PathBuf,
    pub registry: Mutex<Registry>,
}

pub struct Reply {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

pub fn parse_args(args: &[String]) -> Result<Config, String> {
    let mut root = PathBuf::from(".");
    let mut port: u16 = 8765;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--root" => {
                root = PathBuf::from(iter.next().ok_or("--root needs a path")?);
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
    Ok(Config { root, port })
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

fn text(status: u16, body: &str) -> Reply {
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

pub fn route(app: &App, method: &str, url: &str, host: Option<&str>, port: u16, now: f64) -> Reply {
    if !host_allowed(host, port) {
        return text(403, "forbidden host");
    }
    if method != "GET" {
        return text(405, "method not allowed");
    }
    let path = url.split('?').next().unwrap_or("/");
    match path {
        "/" | "/dashboard.html" => Reply {
            status: 200,
            content_type: "text/html; charset=utf-8",
            body: DASHBOARD_HTML.as_bytes().to_vec(),
        },
        "/dashboard.css" => asset("text/css; charset=utf-8", DASHBOARD_CSS),
        "/board.js" => asset("text/javascript; charset=utf-8", BOARD_JS),
        "/app.js" => asset("text/javascript; charset=utf-8", APP_JS),
        "/healthz" => text(200, "ok"),
        "/api/state" => api_state(app, now),
        _ => text(404, "not found"),
    }
}

fn api_state(app: &App, now: f64) -> Reply {
    let board_path = app
        .root
        .join(".superpowers")
        .join("factory")
        .join("board.json");
    let (board, board_error) = load_board(&board_path);
    let view = match app.registry.lock() {
        Ok(registry) => build_state(&board, &registry, now, board_error),
        Err(_) => return text(500, "state lock poisoned"),
    };
    match serde_json::to_vec(&view) {
        Ok(body) => Reply {
            status: 200,
            content_type: "application/json",
            body,
        },
        Err(_) => text(500, "could not encode state"),
    }
}

/// Tails the event log in a background thread and feeds the registry.
pub fn spawn_tailer(app: Arc<App>) {
    let path = app
        .root
        .join(".superpowers")
        .join("factory")
        .join("events.jsonl");
    std::thread::spawn(move || {
        let mut tailer = Tailer::new(path);
        loop {
            let lines = tailer.poll();
            if !lines.is_empty() {
                if let Ok(mut registry) = app.registry.lock() {
                    for line in &lines {
                        registry.ingest_line(line);
                    }
                }
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    });
}

pub fn serve(server: tiny_http::Server, app: Arc<App>, port: u16) {
    for request in server.incoming_requests() {
        let host = request
            .headers()
            .iter()
            .find(|header| header.field.equiv("Host"))
            .map(|header| header.value.as_str().to_string());
        let reply = route(
            &app,
            request.method().as_str(),
            request.url(),
            host.as_deref(),
            port,
            now_secs(),
        );
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
                &b"default-src 'self'; frame-ancestors 'none'"[..],
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
        let app = Arc::new(App {
            root: dir.path().to_path_buf(),
            registry: Mutex::new(Registry::new()),
        });
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
        if let Ok(mut registry) = app.registry.lock() {
            registry.ingest_line(
                r#"{"ts":2.0,"hook_event_name":"SubagentStart","agent_id":"s1","agent_type":"factory-spec-writer","cwd":"/r"}"#,
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
        Ok(())
    }
}
