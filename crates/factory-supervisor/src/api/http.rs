//! The request router. It is pure: no socket, so tests call `route` directly.

use factory_core::Home;
use serde_json::{json, Value};

use crate::api::{read, usage, write, ApiDeps};
use crate::token;

/// The largest request body that is accepted.
pub const MAX_BODY: usize = 64 * 1024;

/// A status code and a JSON body.
#[derive(Debug, Clone, PartialEq)]
pub struct ApiResponse {
    pub status: u16,
    pub body: Value,
}

impl ApiResponse {
    pub fn ok(body: Value) -> ApiResponse {
        ApiResponse { status: 200, body }
    }
}

/// An error body: `{"error": {"code", "message"}}`.
pub fn error_response(status: u16, code: &str, message: &str) -> ApiResponse {
    ApiResponse {
        status,
        body: json!({ "error": { "code": code, "message": message } }),
    }
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// Only a loopback host name passes, with any port. This stops DNS rebinding from a web page.
fn host_ok(host: &str) -> bool {
    let name = if let Some(rest) = host.strip_prefix('[') {
        match rest.split_once(']') {
            Some((inner, tail)) if tail.is_empty() || tail.starts_with(':') => inner,
            _ => return false,
        }
    } else {
        host.split(':').next().unwrap_or("")
    };
    matches!(name, "127.0.0.1" | "localhost" | "::1")
}

fn bearer(headers: &[(String, String)]) -> &str {
    let value = header(headers, "authorization").unwrap_or("");
    match value.split_once(' ') {
        Some((scheme, rest)) if scheme.eq_ignore_ascii_case("bearer") => rest.trim(),
        _ => "",
    }
}

fn unauthorized() -> ApiResponse {
    error_response(401, "unauthorized", "a valid bearer token is required")
}

/// Checks the host, then the token. This runs before any body is read.
pub fn authorize(home: &Home, headers: &[(String, String)]) -> Result<(), ApiResponse> {
    if !header(headers, "host").is_some_and(host_ok) {
        return Err(error_response(
            400,
            "bad_request",
            "the Host header must be a loopback address",
        ));
    }
    // The detail stays out of the body, because the caller is not authenticated yet.
    let expected = token::read(home)
        .map_err(|_| error_response(500, "failed", "the server cannot check the token"))?;
    if token::matches(&expected, bearer(headers)) {
        Ok(())
    } else {
        Err(unauthorized())
    }
}

/// Checks the host, then the token, then dispatches.
pub fn route(
    home: &Home,
    deps: &ApiDeps,
    method: &str,
    path: &str,
    query: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> ApiResponse {
    if let Err(r) = authorize(home, headers) {
        return r;
    }
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let allowed: &[&str] = match segments.as_slice() {
        ["projects"] | ["usage"] => &["GET"],
        ["projects", _, "jobs"] => &["GET", "POST"],
        ["projects", _, "jobs", _, "approve" | "rework" | "cancel"] => &["POST"],
        _ => return error_response(404, "not_found", "no such endpoint"),
    };
    if !allowed.contains(&method) {
        return error_response(
            405,
            "method_not_allowed",
            &format!("allowed here: {}", allowed.join(", ")),
        );
    }
    match (method, segments.as_slice()) {
        (_, ["projects"]) => read::projects(home, deps),
        ("GET", ["projects", name, "jobs"]) => read::jobs(home, deps, name),
        (_, ["projects", name, "jobs"]) => write::create(home, deps, name, body),
        (_, ["projects", name, "jobs", id, "approve"]) => {
            write::approve(home, deps, name, id, body)
        }
        (_, ["projects", name, "jobs", id, "rework"]) => write::rework(home, deps, name, id, body),
        (_, ["projects", name, "jobs", id, _]) => write::cancel(home, deps, name, id, body),
        _ => usage::report(home, deps, query),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::testutil::*;

    fn get(
        home: &factory_core::Home,
        path: &str,
        query: &str,
        token: Option<&str>,
        host: &str,
    ) -> ApiResponse {
        route(
            home,
            &deps(),
            "GET",
            path,
            query,
            &headers(token, host),
            b"",
        )
    }

    #[test]
    fn bad_credentials_get_identical_401s() {
        let (_t, home) = fixture();
        let short = &TOKEN[..63];
        let cases = [None, Some(""), Some("wrong"), Some(short)];
        let mut bodies = Vec::new();
        for c in cases {
            let r = get(&home, "/projects", "", c, "127.0.0.1:8766");
            assert_eq!(r.status, 401, "{c:?}");
            assert_eq!(r.body["error"]["code"], "unauthorized");
            bodies.push(r.body);
        }
        assert!(bodies.windows(2).all(|w| w[0] == w[1]));
        assert_eq!(
            get(&home, "/projects", "", Some(TOKEN), "127.0.0.1:8766").status,
            200
        );
    }

    #[test]
    fn an_empty_bearer_header_is_unauthorized() {
        let (_t, home) = fixture();
        let h = vec![
            ("Host".to_string(), "localhost:1".to_string()),
            ("authorization".to_string(), "Bearer ".to_string()),
        ];
        let r = route(&home, &deps(), "GET", "/projects", "", &h, b"");
        assert_eq!(r.status, 401);
    }

    #[test]
    fn the_host_must_be_loopback() {
        let (_t, home) = fixture();
        let r = get(&home, "/projects", "", Some(TOKEN), "evil.example");
        assert_eq!(
            (r.status, r.body["error"]["code"].as_str()),
            (400, Some("bad_request"))
        );
        assert_eq!(
            get(&home, "/projects", "", Some(TOKEN), "127.0.0.1:4242").status,
            200
        );
        assert_eq!(
            get(&home, "/projects", "", Some(TOKEN), "localhost:4242").status,
            200
        );
        // The host check comes before the token check.
        assert_eq!(
            get(&home, "/projects", "", None, "evil.example").status,
            400
        );
    }

    #[test]
    fn unknown_paths_and_methods_are_refused() {
        let (_t, home) = fixture();
        let h = headers(Some(TOKEN), "localhost:1");
        let r = route(&home, &deps(), "DELETE", "/projects", "", &h, b"");
        assert_eq!(
            (r.status, r.body["error"]["code"].as_str()),
            (405, Some("method_not_allowed"))
        );
        let r = route(&home, &deps(), "GET", "/nope", "", &h, b"");
        assert_eq!(
            (r.status, r.body["error"]["code"].as_str()),
            (404, Some("not_found"))
        );
    }
}
