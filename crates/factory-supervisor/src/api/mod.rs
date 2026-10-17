//! The local JSON API of factoryd.

pub mod events;
pub mod http;
pub mod read;
pub mod usage;
pub mod write;
#[cfg(test)]
pub(crate) mod testutil;

use std::io::{Read, Write};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
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
        respond(request, http::error_response(409, "conflict", "too many open event streams"));
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
    let headers: Vec<(String, String)> =
        request.headers().iter().map(|h| (h.field.as_str().as_str().to_string(), h.value.as_str().to_string())).collect();
    if let Err(refused) = http::authorize(home, &headers) {
        respond(request, refused);
        return;
    }
    if method == "GET" && path == "/events" {
        serve_events(home, query, ctl, request);
        return;
    }
    let mut body = Vec::new();
    if carries_body(&method) && request.as_reader().take(MAX_BODY as u64 + 1).read_to_end(&mut body).is_err() {
        respond(request, http::error_response(400, "bad_request", "the request body could not be read"));
        return;
    }
    let response = http::route(home, deps, &method, path, query, &headers, &body);
    respond(request, response);
}

impl ApiServer {
    /// Binds `listen` and serves in the background until the handle is stopped.
    pub fn start(home: Home, listen: SocketAddr, deps: ApiDeps) -> Result<ApiHandle, ApiError> {
        let bind = |reason: String| ApiError::Bind { addr: listen, reason };
        let server = tiny_http::Server::http(listen).map_err(|e| bind(e.to_string()))?;
        let addr = server.server_addr().to_ip().ok_or_else(|| bind("not an IP address".to_string()))?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let active = Arc::new(AtomicUsize::new(0));
        let ctl = Control { stop: stop.clone(), streams: Arc::new(AtomicUsize::new(0)) };
        let acceptor = std::thread::spawn(move || {
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
                let (home, deps, active, ctl) = (home.clone(), deps.clone(), active.clone(), ctl.clone());
                std::thread::spawn(move || {
                    let _slot = Slot(active);
                    handle(&home, &deps, &ctl, request);
                });
            }
        });
        Ok(ApiHandle { addr, stop, acceptor: Some(acceptor) })
    }
}
