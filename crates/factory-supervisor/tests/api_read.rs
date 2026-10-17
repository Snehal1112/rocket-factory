//! The read API over a real socket.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;

use factory_core::Home;
use factory_supervisor::api::{ApiDeps, ApiServer};
use factory_supervisor::token;

fn request(addr: SocketAddr, path: &str, host: &str, token: Option<&str>) -> (u16, serde_json::Value) {
    let mut s = TcpStream::connect(addr).expect("connect");
    let auth = token.map(|t| format!("Authorization: Bearer {t}\r\n")).unwrap_or_default();
    write!(s, "GET {path} HTTP/1.1\r\nHost: {host}\r\n{auth}Connection: close\r\n\r\n").expect("write");
    let mut raw = String::new();
    s.read_to_string(&mut raw).expect("read");
    let status: u16 = raw.split_whitespace().nth(1).and_then(|c| c.parse().ok()).expect("status");
    let body = raw.split("\r\n\r\n").nth(1).unwrap_or("");
    (status, serde_json::from_str(body).expect("json body"))
}

#[test]
fn the_server_checks_token_and_host_and_lists_projects() {
    let t = tempfile::tempdir().expect("tempdir");
    let home = Home::at(t.path().join("home"));
    let repo = t.path().join("repo");
    std::fs::create_dir_all(&repo).expect("repo");
    git2::Repository::init(&repo).expect("init");
    let dir = home.project_dir("demo");
    std::fs::create_dir_all(&dir).expect("project");
    std::fs::write(dir.join("project.toml"), format!("repo = {:?}\npreset = \"rust\"\n", repo.display().to_string())).expect("toml");
    let tok = token::ensure(&home).expect("token");

    let deps = ApiDeps { now_ms: Arc::new(|| 1_700_000_000_000) };
    let handle = ApiServer::start(home, "127.0.0.1:0".parse().expect("addr"), deps).expect("start");
    let addr = handle.addr;
    let host = format!("127.0.0.1:{}", addr.port());

    let (status, body) = request(addr, "/projects", &host, None);
    assert_eq!((status, body["error"]["code"].as_str()), (401, Some("unauthorized")));

    let (status, body) = request(addr, "/projects", "evil.example", Some(&tok));
    assert_eq!((status, body["error"]["code"].as_str()), (400, Some("bad_request")));

    let (status, body) = request(addr, "/projects", &host, Some(&tok));
    assert_eq!(status, 200);
    let first = &body[0];
    assert_eq!(first["name"], "demo");
    assert_eq!(first["status"], "ready");
    for key in ["repo", "preset", "baseBranch", "stateDir", "jobsDir", "activeJob", "reason"] {
        assert!(first.get(key).is_some(), "missing {key}");
    }

    handle.stop();
}
