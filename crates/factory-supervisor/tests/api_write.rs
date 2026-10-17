//! The write endpoints and the event stream over a real socket, with a fake `factory` command.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use factory_core::Home;
use factory_supervisor::api::{ApiDeps, ApiHandle, ApiServer};
use factory_supervisor::{token, CliOutput, CliSpawnError, FactoryCli, QueueItem};

#[derive(Default)]
struct RecordingCli {
    calls: Mutex<Vec<(String, Vec<String>)>>,
}

impl FactoryCli for RecordingCli {
    fn run(&self, project: &str, args: &[String]) -> Result<CliOutput, CliSpawnError> {
        let mut calls = self.calls.lock().expect("lock");
        calls.push((project.to_string(), args.to_vec()));
        Ok(CliOutput { code: 0, stdout: format!("created job {:03} in somewhere\n", calls.len()), stderr: String::new() })
    }
}

struct Server {
    _t: tempfile::TempDir,
    home: Home,
    handle: ApiHandle,
    token: String,
    cli: Arc<RecordingCli>,
}

fn add_project(t: &tempfile::TempDir, home: &Home, name: &str) {
    let repo = t.path().join(format!("repo-{name}"));
    std::fs::create_dir_all(&repo).expect("repo");
    git2::Repository::init(&repo).expect("init");
    let dir = home.project_dir(name);
    std::fs::create_dir_all(dir.join("state")).expect("project");
    std::fs::write(dir.join("project.toml"), format!("repo = {:?}\npreset = \"rust\"\n", repo.display().to_string())).expect("toml");
}

fn start() -> Server {
    let t = tempfile::tempdir().expect("tempdir");
    let home = Home::at(t.path().join("home"));
    add_project(&t, &home, "alpha");
    add_project(&t, &home, "beta");
    let token = token::ensure(&home).expect("token");
    let cli = Arc::new(RecordingCli::default());
    // Every call sees a later millisecond, because a request id is time, project and job.
    let clock = std::sync::atomic::AtomicU64::new(1_700_000_000_000);
    let deps = ApiDeps { now_ms: Arc::new(move || clock.fetch_add(1, std::sync::atomic::Ordering::SeqCst)), cli: cli.clone() };
    let handle = ApiServer::start(home.clone(), "127.0.0.1:0".parse().expect("addr"), deps).expect("start");
    Server { _t: t, home, handle, token, cli }
}

fn send(addr: SocketAddr, tok: &str, method: &str, path: &str, body: &str) -> (u16, serde_json::Value) {
    let mut s = TcpStream::connect(addr).expect("connect");
    write!(
        s,
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {tok}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        addr.port(),
        body.len()
    )
    .expect("write");
    let mut raw = String::new();
    s.read_to_string(&mut raw).expect("read");
    let status: u16 = raw.split_whitespace().nth(1).and_then(|c| c.parse().ok()).expect("status");
    let text = raw.split("\r\n\r\n").nth(1).unwrap_or("");
    (status, serde_json::from_str(text).expect("json body"))
}

fn queued(home: &Home) -> Vec<factory_supervisor::Request> {
    factory_supervisor::queue::list(home)
        .into_iter()
        .filter_map(|i| match i {
            QueueItem::Ok(r) => Some(r),
            QueueItem::Bad { .. } => None,
        })
        .collect()
}

fn write_board(home: &Home, project: &str, stage: &str) {
    let job = format!(
        r#"{{"id":"001","dir":"001-first","slug":"first","created":"2026-10-17","feature":"F","branch":"factory/001-first","stage":"{stage}","round":1,"stage_since":1.0,"reviews":{{}},"tokens":{{"input":0,"cache_creation":0,"output":0,"cache_read":0}}}}"#
    );
    std::fs::write(home.project_dir(project).join("state").join("board.json"), format!(r#"{{"jobs":[{job}]}}"#)).expect("board");
}

#[test]
fn create_and_approve_leave_the_expected_queue_files() {
    let s = start();
    let addr = s.handle.addr;
    let work = s._t.path().join("work");
    std::fs::create_dir_all(work.join("plans")).expect("plans");
    std::fs::write(work.join("spec.md"), "# spec").expect("spec");
    let body = serde_json::json!({"spec": work.join("spec.md"), "plans": work.join("plans")}).to_string();

    let (status, out) = send(addr, &s.token, "POST", "/projects/alpha/jobs", &body);
    assert_eq!(status, 201, "{out}");
    assert_eq!(out["job"], "001");
    let (status, out) = send(addr, &s.token, "POST", "/projects/beta/jobs", &body);
    assert_eq!(status, 201, "{out}");

    let mut runs = queued(&s.home);
    runs.sort_by(|a, b| a.project.cmp(&b.project));
    assert_eq!(runs.len(), 2);
    assert_eq!((runs[0].project.as_str(), runs[0].action), ("alpha", factory_supervisor::Action::Run));
    assert_eq!((runs[1].project.as_str(), runs[1].action), ("beta", factory_supervisor::Action::Run));
    let calls = s.cli.calls.lock().expect("lock").clone();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].1[0], "new");

    // Approve in one project does not touch the other.
    write_board(&s.home, "alpha", "needs-human");
    write_board(&s.home, "beta", "build");
    let (status, out) = send(addr, &s.token, "POST", "/projects/alpha/jobs/1/approve", "");
    assert_eq!(status, 202, "{out}");
    let (status, out) = send(addr, &s.token, "POST", "/projects/beta/jobs/1/approve", "");
    assert_eq!((status, out["error"]["code"].as_str()), (409, Some("conflict")));
    let approves: Vec<_> = queued(&s.home).into_iter().filter(|r| r.action == factory_supervisor::Action::Approve).collect();
    assert_eq!(approves.len(), 1);
    assert_eq!((approves[0].project.as_str(), approves[0].job.as_str()), ("alpha", "001"));

    // A request without the token runs nothing.
    let (status, _) = send(addr, "wrong", "POST", "/projects/alpha/jobs", &body);
    assert_eq!(status, 401);
    assert_eq!(s.cli.calls.lock().expect("lock").len(), 2);
    s.handle.stop();
}

#[test]
fn a_body_over_the_cap_is_refused_before_anything_runs() {
    let s = start();
    let big = format!(r#"{{"spec":"/a","plans":"/b","feature":"{}"}}"#, "x".repeat(80 * 1024));
    let (status, out) = send(s.handle.addr, &s.token, "POST", "/projects/alpha/jobs", &big);
    assert_eq!((status, out["error"]["code"].as_str()), (400, Some("bad_request")));
    assert!(s.cli.calls.lock().expect("lock").is_empty());
    s.handle.stop();
}

fn open_stream(addr: SocketAddr, tok: &str, query: &str) -> BufReader<TcpStream> {
    let mut s = TcpStream::connect(addr).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(10))).expect("timeout");
    write!(s, "GET /events{query} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {tok}\r\nConnection: close\r\n\r\n", addr.port()).expect("write");
    BufReader::new(s)
}

fn skip_head(r: &mut BufReader<TcpStream>) -> String {
    let mut first = String::new();
    r.read_line(&mut first).expect("status line");
    loop {
        let mut line = String::new();
        r.read_line(&mut line).expect("header");
        if line == "\r\n" {
            return first;
        }
    }
}

fn next_data(r: &mut BufReader<TcpStream>) -> String {
    loop {
        let mut line = String::new();
        assert!(r.read_line(&mut line).expect("line") > 0, "the stream ended");
        if let Some(d) = line.strip_prefix("data: ") {
            return d.trim_end().to_string();
        }
    }
}

#[test]
fn the_event_stream_replays_then_follows_and_the_server_stops_it() {
    let s = start();
    let addr = s.handle.addr;
    let events = s.home.project_dir("alpha").join("state").join("events.jsonl");
    let lines: String = (1..=60).map(|i| format!("{{\"n\":{i}}}\n")).collect();
    std::fs::write(&events, lines).expect("events");

    let mut r = open_stream(addr, &s.token, "?project=alpha");
    assert!(skip_head(&mut r).starts_with("HTTP/1.1 200"));
    // Only the last 50 lines are replayed.
    assert_eq!(next_data(&mut r), r#"{"n":11,"project":"alpha"}"#);
    for _ in 12..=60 {
        next_data(&mut r);
    }
    std::fs::OpenOptions::new().append(true).open(&events).expect("open").write_all(b"{\"n\":61}\n").expect("append");
    assert_eq!(next_data(&mut r), r#"{"n":61,"project":"alpha"}"#);

    // The all-projects stream sees the other project too.
    let other = s.home.project_dir("beta").join("state").join("events.jsonl");
    std::fs::write(&other, "{\"b\":1}\n").expect("beta events");
    let mut all = open_stream(addr, &s.token, "");
    skip_head(&mut all);
    let mut seen = Vec::new();
    while !seen.iter().any(|l: &String| l.contains(r#""project":"beta""#)) {
        seen.push(next_data(&mut all));
        assert!(seen.len() <= 60, "no beta event");
    }

    // Errors on the stream endpoint are normal JSON answers.
    let (status, out) = send(addr, &s.token, "GET", "/events?project=ghost", "");
    assert_eq!((status, out["error"]["code"].as_str()), (404, Some("not_found")));
    let (status, _) = send(addr, "wrong", "GET", "/events", "");
    assert_eq!(status, 401);

    // Stopping the server ends open streams: the client sees the end of the answer.
    let began = Instant::now();
    s.handle.stop();
    let mut rest = String::new();
    while r.read_line(&mut rest).expect("read until the end") > 0 {
        rest.clear();
    }
    assert!(began.elapsed() < Duration::from_secs(10));
    drop(all);
}

#[test]
fn at_most_eight_streams_are_open_at_once() {
    let s = start();
    let addr = s.handle.addr;
    let mut open = Vec::new();
    for _ in 0..8 {
        let mut r = open_stream(addr, &s.token, "?project=alpha");
        assert!(skip_head(&mut r).starts_with("HTTP/1.1 200"));
        open.push(r);
    }
    let (status, out) = send(addr, &s.token, "GET", "/events", "");
    assert_eq!((status, out["error"]["code"].as_str()), (409, Some("conflict")));
    drop(open);
    s.handle.stop();
}
