//! The local JSON API of factoryd.

pub mod events;
pub mod http;
pub mod read;
#[cfg(test)]
pub(crate) mod testutil;
pub mod usage;
pub mod write;

use std::io::{Read, Write};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use factory_core::Home;

use self::http::{ApiResponse, MAX_BODY};
use crate::factory_cli::FactoryCli;

/// What the handlers need besides the home.
#[derive(Clone)]
pub struct ApiDeps {
    pub now_ms: Arc<dyn Fn() -> u64 + Send + Sync>,
    /// Runs the commands only factory-cli implements.
    pub cli: Arc<dyn FactoryCli>,
    /// Reads the version of the `factory` binary the jobs run, or why it cannot.
    pub factory_version: Arc<dyn Fn() -> Result<String, String> + Send + Sync>,
}

/// Handler threads that may run at once. A further request is answered with `conflict`.
const MAX_HANDLERS: usize = 16;
/// Open event streams that may run at once. They count against `MAX_HANDLERS` too.
const MAX_STREAMS: usize = 8;
const POLL: Duration = Duration::from_millis(100);

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("cannot listen on {addr}: {reason}")]
    Bind { addr: SocketAddr, reason: String },
    #[error("cannot start the API thread: {0}")]
    Thread(String),
}

/// The work of one request, run on its own thread.
type Job = Box<dyn FnOnce() + Send + 'static>;

/// Starts a named thread for a job. The seam lets tests force the OS refusal.
type Spawner = Arc<dyn Fn(&str, Job) -> std::io::Result<()> + Send + Sync>;

fn os_spawner() -> Spawner {
    Arc::new(|name, job| {
        std::thread::Builder::new()
            .name(name.to_string())
            .spawn(job)
            .map(|_| ())
    })
}

/// A running API server.
pub struct ApiHandle {
    pub addr: SocketAddr,
    stop: Arc<AtomicBool>,
    acceptor: Option<JoinHandle<()>>,
}

impl ApiHandle {
    /// Stops accepting and waits for the acceptor thread. Requests in flight finish on their own.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.acceptor.take() {
            let _ = t.join();
        }
    }
}

impl Drop for ApiHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub struct ApiServer;

fn respond(request: tiny_http::Request, response: ApiResponse) {
    let text = response.body.to_string();
    let header = tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]);
    let mut out = tiny_http::Response::from_string(text).with_status_code(response.status);
    if let Ok(h) = header {
        out = out.with_header(h);
    }
    let _ = request.respond(out);
}

/// Gives the handler slot back when the handler ends, also after a panic.
struct Slot(Arc<AtomicUsize>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Only these methods carry a body that is read.
fn carries_body(method: &str) -> bool {
    matches!(method, "POST" | "PUT" | "PATCH")
}

/// What every handler thread shares.
#[derive(Clone)]
struct Control {
    stop: Arc<AtomicBool>,
    streams: Arc<AtomicUsize>,
}

/// Serves `GET /events` on the raw socket, because the answer has no end and each piece is flushed at once.
fn serve_events(home: &Home, query: &str, ctl: &Control, request: tiny_http::Request) {
    if ctl.streams.fetch_add(1, Ordering::SeqCst) >= MAX_STREAMS {
        ctl.streams.fetch_sub(1, Ordering::SeqCst);
        respond(
            request,
            http::error_response(409, "conflict", "too many open event streams"),
        );
        return;
    }
    let _slot = Slot(ctl.streams.clone());
    let stream = match events::open(home, query, ctl.stop.clone(), events::Timing::default()) {
        Ok(s) => s,
        Err(refused) => {
            respond(request, refused);
            return;
        }
    };
    let mut out = request.into_writer();
    let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n";
    if out.write_all(head.as_bytes()).is_err() || out.flush().is_err() {
        return;
    }
    events::pump(stream, &mut out);
}

/// Serves one request. The body is read only after the Host and token checks pass, so an
/// unauthenticated client that declares a body and stalls never holds a thread on a read.
/// tiny_http offers no socket timeout, so an authenticated client that stalls inside its body
/// holds one of the handler slots until it disconnects. The read itself is capped at `MAX_BODY`.
fn handle(home: &Home, deps: &ApiDeps, ctl: &Control, mut request: tiny_http::Request) {
    let method = request.method().as_str().to_string();
    let url = request.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((url.as_str(), ""));
    let headers: Vec<(String, String)> = request
        .headers()
        .iter()
        .map(|h| {
            (
                h.field.as_str().as_str().to_string(),
                h.value.as_str().to_string(),
            )
        })
        .collect();
    if let Err(refused) = http::authorize(home, &headers) {
        respond(request, refused);
        return;
    }
    if method == "GET" && path == "/events" {
        serve_events(home, query, ctl, request);
        return;
    }
    let mut body = Vec::new();
    if carries_body(&method)
        && request
            .as_reader()
            .take(MAX_BODY as u64 + 1)
            .read_to_end(&mut body)
            .is_err()
    {
        respond(
            request,
            http::error_response(400, "bad_request", "the request body could not be read"),
        );
        return;
    }
    let response = http::route(home, deps, &method, path, query, &headers, &body);
    respond(request, response);
}

impl ApiServer {
    /// Binds `listen` and serves in the background until the handle is stopped.
    pub fn start(home: Home, listen: SocketAddr, deps: ApiDeps) -> Result<ApiHandle, ApiError> {
        Self::start_with(
            home,
            listen,
            deps,
            os_spawner(),
            Arc::new(AtomicUsize::new(0)),
        )
    }

    fn start_with(
        home: Home,
        listen: SocketAddr,
        deps: ApiDeps,
        spawner: Spawner,
        active: Arc<AtomicUsize>,
    ) -> Result<ApiHandle, ApiError> {
        let bind = |reason: String| ApiError::Bind {
            addr: listen,
            reason,
        };
        let server = tiny_http::Server::http(listen).map_err(|e| bind(e.to_string()))?;
        let addr = server
            .server_addr()
            .to_ip()
            .ok_or_else(|| bind("not an IP address".to_string()))?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let ctl = Control {
            stop: stop.clone(),
            streams: Arc::new(AtomicUsize::new(0)),
        };
        let handler_spawner = spawner.clone();
        let acceptor = std::thread::Builder::new()
            .name("factoryd-api-accept".to_string())
            .spawn(move || {
                while !flag.load(Ordering::SeqCst) {
                    let request = match server.recv_timeout(POLL) {
                        Ok(Some(r)) => r,
                        Ok(None) => continue,
                        Err(_) => break,
                    };
                    if active.fetch_add(1, Ordering::SeqCst) >= MAX_HANDLERS {
                        active.fetch_sub(1, Ordering::SeqCst);
                        respond(request, http::error_response(409, "conflict", "busy"));
                        continue;
                    }
                    // The request stays reachable here, so a refused thread can still be answered.
                    let cell = Arc::new(Mutex::new(Some(request)));
                    let (home, deps, slot_active, ctl, job_cell) = (
                        home.clone(),
                        deps.clone(),
                        active.clone(),
                        ctl.clone(),
                        cell.clone(),
                    );
                    let job: Job = Box::new(move || {
                        let _slot = Slot(slot_active);
                        let taken = job_cell.lock().ok().and_then(|mut c| c.take());
                        if let Some(request) = taken {
                            handle(&home, &deps, &ctl, request);
                        }
                    });
                    if handler_spawner("factoryd-api-handler", job).is_err() {
                        // The job never ran, so its slot is given back here.
                        active.fetch_sub(1, Ordering::SeqCst);
                        let taken = cell.lock().ok().and_then(|mut c| c.take());
                        if let Some(request) = taken {
                            respond(
                                request,
                                http::error_response(
                                    503,
                                    "unavailable",
                                    "the daemon cannot start a thread right now",
                                ),
                            );
                        }
                        std::thread::sleep(POLL);
                    }
                }
            })
            .map_err(|e| ApiError::Thread(e.to_string()))?;
        Ok(ApiHandle {
            addr,
            stop,
            acceptor: Some(acceptor),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::testutil::{deps, fixture, TOKEN};

    fn get(addr: SocketAddr, path: &str) -> (u16, serde_json::Value) {
        let mut s = std::net::TcpStream::connect(addr).expect("connect");
        let host = format!("127.0.0.1:{}", addr.port());
        write!(s, "GET {path} HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {TOKEN}\r\nConnection: close\r\n\r\n").expect("write");
        let mut raw = String::new();
        s.read_to_string(&mut raw).expect("read");
        let status = raw
            .split_whitespace()
            .nth(1)
            .and_then(|c| c.parse().ok())
            .expect("status");
        (
            status,
            serde_json::from_str(raw.split("\r\n\r\n").nth(1).unwrap_or("")).expect("json"),
        )
    }

    #[test]
    fn a_refused_handler_thread_gets_503_and_the_acceptor_keeps_serving() {
        let (_t, home) = fixture();
        let refuse = Arc::new(AtomicBool::new(true));
        let flag = refuse.clone();
        let spawner: Spawner = Arc::new(move |name, job| {
            if flag.load(Ordering::SeqCst) {
                return Err(std::io::Error::from_raw_os_error(11));
            }
            std::thread::Builder::new()
                .name(name.to_string())
                .spawn(job)
                .map(|_| ())
        });
        let active = Arc::new(AtomicUsize::new(0));
        let handle = ApiServer::start_with(
            home,
            "127.0.0.1:0".parse().expect("addr"),
            deps(),
            spawner,
            active.clone(),
        )
        .expect("start");
        let (status, body) = get(handle.addr, "/projects");
        assert_eq!(
            (status, body["error"]["code"].as_str()),
            (503, Some("unavailable"))
        );
        assert_eq!(
            body["error"]["message"],
            "the daemon cannot start a thread right now"
        );
        assert_eq!(active.load(Ordering::SeqCst), 0);
        refuse.store(false, Ordering::SeqCst);
        assert_eq!(get(handle.addr, "/projects").0, 200);
        // The slot is given back just after the answer is written, so wait for it.
        let began = std::time::Instant::now();
        while active.load(Ordering::SeqCst) != 0 {
            assert!(
                began.elapsed() < Duration::from_secs(5),
                "the handler slot was not given back"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        handle.stop();
    }
}
