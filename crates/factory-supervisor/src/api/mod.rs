//! The local JSON API of factoryd.

pub mod http;
pub mod read;
pub mod usage;
#[cfg(test)]
pub(crate) mod testutil;

use std::io::Read;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use factory_core::Home;

use self::http::ApiResponse;

/// What the handlers need besides the home.
#[derive(Clone)]
pub struct ApiDeps {
    pub now_ms: Arc<dyn Fn() -> u64 + Send + Sync>,
}

/// Handler threads that may run at once. A further request is answered with `conflict`.
const MAX_HANDLERS: usize = 16;
/// The largest request body that is read.
const MAX_BODY: usize = 64 * 1024;
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

fn handle(home: &Home, deps: &ApiDeps, mut request: tiny_http::Request) {
    let method = request.method().as_str().to_string();
    let url = request.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((url.as_str(), ""));
    let headers: Vec<(String, String)> =
        request.headers().iter().map(|h| (h.field.as_str().as_str().to_string(), h.value.as_str().to_string())).collect();
    let mut body = Vec::new();
    let _ = request.as_reader().take(MAX_BODY as u64).read_to_end(&mut body);
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
                let (home, deps, active) = (home.clone(), deps.clone(), active.clone());
                std::thread::spawn(move || {
                    handle(&home, &deps, request);
                    active.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });
        Ok(ApiHandle { addr, stop, acceptor: Some(acceptor) })
    }
}
