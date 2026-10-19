// Copyright (c) 2026 Code Infinity
// SPDX-License-Identifier: MPL-2.0
//! `tina4 ai serve` - the local LLM gateway.
//!
//! dev-admin (and anything else) talks to THIS, never to a model directly. It is a
//! single, machine-wide service (like `ollama serve`) on :11435 that:
//!   * GATES on registration - a request is refused unless `tina4 login` has stored
//!     credentials, which only exist after a GitHub sign-in on profile.tina4.com;
//!   * PICKS a backend - the Tina4 ai-server (:11437) if up, else Ollama (:11434),
//!     overridable with TINA4_AI_BACKEND_URL - so falling back to Ollama is automatic;
//!   * FORWARDS `/api/chat` verbatim (base64 `images[]` ride along) and STREAMS the
//!     reply straight back, so `stream:true`, images and thinking all pass through.
//!
//! It is an idempotent singleton: starting a second one no-ops if :11435 answers.
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use axum::{
    body::{Body, Bytes},
    extract::{Path, Request, State},
    http::{header, HeaderName, HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};

use crate::auth;
use crate::console::{icon_fail, icon_info, icon_ok};

const DEFAULT_PORT: u16 = 11435;
// Backends tried in order when TINA4_AI_BACKEND_URL is not set.
const BACKENDS: &[(u16, &str)] = &[(11437, "tina4 ai-server"), (11434, "Ollama")];

#[derive(Clone)]
struct GwState {
    client: reqwest::Client,
}

/// Entry point for `tina4 ai serve`.
pub fn serve(port: Option<u16>, backend: Option<String>) {
    let port = port.unwrap_or(DEFAULT_PORT);
    if let Some(b) = backend {
        if !b.trim().is_empty() {
            std::env::set_var("TINA4_AI_BACKEND_URL", b.trim());
        }
    }

    // Idempotent singleton: if something already answers on the port, assume it is us.
    if port_open(port) {
        println!("{} AI gateway already running on http://127.0.0.1:{port}", icon_ok());
        return;
    }

    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("could not start the async runtime: {e}");
            std::process::exit(1);
        }
    };
    rt.block_on(async move {
        let client = reqwest::Client::new();

        // PRECONDITION 1 - the user must be signed in. No local creds means they
        // never completed the GitHub sign-in on profile.tina4.com.
        if auth::load_credentials().is_none() {
            eprintln!("{} not signed in - run `tina4 login` first (free profile at https://profile.tina4.com)", icon_fail());
            std::process::exit(1);
        }

        // PRECONDITION 2 - a model backend must be reachable AND answering HTTP.
        // Port-open alone is not enough; we GET it to confirm it is actually working.
        let backend = match resolve_backend() {
            Some(b) if backend_alive(&client, &b).await => b,
            _ => {
                eprintln!("{} no working local model - start Ollama (:11434) or the Tina4 model server (:11437), or set TINA4_AI_BACKEND_URL", icon_fail());
                std::process::exit(1);
            }
        };

        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("{} could not bind :{port}: {e}", icon_fail());
                std::process::exit(1);
            }
        };

        let state = GwState { client };
        let app = Router::new()
            .route("/health", get(|| async { "ok" }))
            // Proxy ANY Ollama-native model endpoint (/api/chat, /api/generate,
            // /api/embeddings, ...) so dev-admin never needs a path straight to a model.
            .route("/api/{*rest}", post(proxy))
            .with_state(state)
            // dev-admin runs in the browser on another local port (the framework
            // dev server, or the SPA dev port), so talking to :11435 is cross-origin.
            // This gateway only ever listens on loopback and gates on local creds,
            // so it reflects the caller's Origin and answers the preflight.
            .layer(middleware::from_fn(cors));

        println!("{} Tina4 AI gateway on http://127.0.0.1:{port}  (POST /api/chat)", icon_ok());
        println!("{} signed in, backend live: {backend}", icon_info());

        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("gateway stopped: {e}");
            std::process::exit(1);
        }
    });
}

/// Guard the proxied path before it is appended to the trusted backend base.
/// `rest` is caller-supplied (the `/api/{*rest}` wildcard), and it flows straight
/// into `format!("{backend}/api/{rest}")`, so it must be a PLAIN path: nothing
/// that could re-point the request at another host (a `://` scheme, an `@`
/// authority), climb out with `..`, or smuggle control characters / CRLF. The
/// backend's host is already pinned by `resolve_backend`; this is what keeps the
/// URL's host from being influenced by the path, and closes the SSRF surface.
fn safe_proxy_path(rest: &str) -> bool {
    if rest.is_empty() || rest.len() > 256 || rest.contains("://") || rest.contains("..") {
        return false;
    }
    rest.bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.'))
}

/// CORS for the browser-based dev-admin SPA. The gateway binds loopback only and
/// gates every proxied call on local `tina4 login` credentials, so opening CORS
/// here does not widen the trust boundary - only a page already on this machine
/// can reach :11435. A preflight (OPTIONS) is answered here without routing; every
/// other response gets the headers added on the way out. The caller's Origin is
/// reflected (with `Vary: Origin`) and `Access-Control-Allow-Private-Network` is
/// set so Chrome's Private Network Access check passes when a localhost page calls
/// 127.0.0.1.
async fn cors(req: Request, next: Next) -> Response {
    let origin = req.headers().get(header::ORIGIN).cloned();
    let req_headers = req.headers().get(header::ACCESS_CONTROL_REQUEST_HEADERS).cloned();
    let is_preflight = req.method() == Method::OPTIONS;

    let mut resp = if is_preflight {
        StatusCode::NO_CONTENT.into_response()
    } else {
        next.run(req).await
    };

    let h = resp.headers_mut();
    h.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        origin.unwrap_or_else(|| HeaderValue::from_static("*")),
    );
    h.insert(header::VARY, HeaderValue::from_static("Origin"));
    h.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, POST, OPTIONS"),
    );
    h.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        req_headers.unwrap_or_else(|| HeaderValue::from_static("content-type, authorization")),
    );
    h.insert(
        HeaderName::from_static("access-control-allow-private-network"),
        HeaderValue::from_static("true"),
    );
    h.insert(header::ACCESS_CONTROL_MAX_AGE, HeaderValue::from_static("600"));
    resp
}

/// POST /api/<rest> - gate, pick a backend, forward verbatim, stream the reply back.
async fn proxy(State(state): State<GwState>, Path(rest): Path<String>, body: Bytes) -> Response {
    // Registration gate: no local credentials => not signed up on profile.
    if auth::load_credentials().is_none() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": "not signed in - run `tina4 login` (free profile at https://profile.tina4.com)"
            })),
        )
            .into_response();
    }

    // SSRF guard: a path that is not a plain path segment sequence is refused
    // before it can shape the upstream URL (see safe_proxy_path).
    if !safe_proxy_path(&rest) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "invalid API path" })),
        )
            .into_response();
    }

    let backend = match resolve_backend() {
        Some(b) => b,
        None => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({
                    "error": "no local model running - start the Tina4 ai-server (:11437) or Ollama (:11434)"
                })),
            )
                .into_response();
        }
    };

    let url = format!("{backend}/api/{rest}");
    let upstream = state
        .client
        .post(&url)
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await;

    let resp = match upstream {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": format!("backend unreachable: {e}") })),
            )
                .into_response();
        }
    };

    // Pass status + content-type through, and STREAM the body (so stream:true works).
    let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::OK);
    let ctype = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json")
        .to_string();

    let mut builder = Response::builder().status(status).header("content-type", ctype);
    // Mark which local backend answered - handy when switching Ollama <-> ai-server.
    builder = builder.header("x-tina4-ai-backend", backend);
    builder
        .body(Body::from_stream(resp.bytes_stream()))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// The backend base URL: an explicit override, else the first open local port.
fn resolve_backend() -> Option<String> {
    if let Ok(u) = std::env::var("TINA4_AI_BACKEND_URL") {
        let u = u.trim();
        if !u.is_empty() {
            return Some(u.trim_end_matches('/').to_string());
        }
    }
    for (port, _name) in BACKENDS {
        if port_open(*port) {
            return Some(format!("http://127.0.0.1:{port}"));
        }
    }
    None
}

/// Is a TCP listener accepting on 127.0.0.1:port? ("see if the port is open")
fn port_open(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_ok()
}

/// A backend "works" if it answers an HTTP GET at all (any status). Port-open
/// alone can be a half-open socket or a non-HTTP process; this proves it speaks HTTP.
async fn backend_alive(client: &reqwest::Client, base: &str) -> bool {
    client.get(base).timeout(Duration::from_secs(3)).send().await.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener as StdTcpListener;

    #[test]
    fn port_open_true_on_a_real_listener_false_on_a_free_port() {
        // Real TCP, no mock. A bound listener reads as open; a just-freed port does not.
        let l = StdTcpListener::bind("127.0.0.1:0").expect("bind listener");
        let open_port = l.local_addr().unwrap().port();
        assert!(port_open(open_port), "a real bound listener must read as open");

        let l2 = StdTcpListener::bind("127.0.0.1:0").expect("bind listener");
        let free_port = l2.local_addr().unwrap().port();
        drop(l2); // release it so nothing is listening there
        assert!(
            !port_open(free_port),
            "a port with no listener must read as closed"
        );
    }

    #[test]
    fn safe_proxy_path_allows_plain_paths_and_rejects_ssrf_shapes() {
        // Real Ollama-native endpoints pass.
        assert!(safe_proxy_path("chat"));
        assert!(safe_proxy_path("generate"));
        assert!(safe_proxy_path("embeddings"));
        assert!(safe_proxy_path("some/nested/path-1_2"));
        // Anything that could re-point the host, climb out, or smuggle control
        // chars / a query is refused.
        assert!(!safe_proxy_path(""));
        assert!(!safe_proxy_path("http://evil.example/x"));
        assert!(!safe_proxy_path("chat@evil.example"));
        assert!(!safe_proxy_path("../../etc/passwd"));
        assert!(!safe_proxy_path("chat\r\nHost: evil"));
        assert!(!safe_proxy_path("chat?x=1"));
        assert!(!safe_proxy_path(&"a".repeat(257)));
    }

    #[test]
    fn resolve_backend_honours_the_override_and_trims_it() {
        // The override wins, and leading/trailing whitespace and the trailing slash go.
        std::env::set_var("TINA4_AI_BACKEND_URL", "http://10.0.0.9:1234/");
        assert_eq!(resolve_backend().as_deref(), Some("http://10.0.0.9:1234"));
        std::env::set_var("TINA4_AI_BACKEND_URL", "  http://10.0.0.9:1234  ");
        assert_eq!(resolve_backend().as_deref(), Some("http://10.0.0.9:1234"));
        std::env::remove_var("TINA4_AI_BACKEND_URL");
    }

    #[test]
    fn backend_alive_true_against_a_real_server_false_against_a_dead_port() {
        // No mock: stand up a REAL HTTP server and probe it; then probe a dead port.
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let app = Router::new().route("/", get(|| async { "ok" }));
            tokio::spawn(async move {
                let _ = axum::serve(listener, app).await;
            });
            tokio::time::sleep(Duration::from_millis(150)).await;

            let client = reqwest::Client::new();
            let live = format!("http://127.0.0.1:{}", addr.port());
            assert!(
                backend_alive(&client, &live).await,
                "a live HTTP server must be reported alive"
            );

            // Bind then drop to guarantee nothing is listening on this port.
            let dead = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let dead_port = dead.local_addr().unwrap().port();
            drop(dead);
            let dead_url = format!("http://127.0.0.1:{dead_port}");
            assert!(
                !backend_alive(&client, &dead_url).await,
                "a port with no HTTP server must not be reported alive"
            );
        });
    }

    #[test]
    fn cors_answers_preflight_and_tags_normal_responses() {
        // No mock: a REAL server carrying the exact `cors` layer the gateway uses.
        // The browser dev-admin SPA on another local port must get a preflight
        // answer and CORS headers on real responses, or it can never call :11435.
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let app = Router::new()
                .route("/health", get(|| async { "ok" }))
                .route("/api/{*rest}", post(|| async { "never reached in preflight" }))
                .layer(middleware::from_fn(cors));
            tokio::spawn(async move {
                let _ = axum::serve(listener, app).await;
            });
            tokio::time::sleep(Duration::from_millis(150)).await;
            let client = reqwest::Client::new();
            let origin = "http://localhost:7146";

            // 1) Preflight OPTIONS is answered here (204) WITHOUT routing, with the
            //    Origin reflected, the requested headers echoed, and the
            //    Private-Network-Access header Chrome needs for a localhost->127.0.0.1 call.
            let pre = client
                .request(reqwest::Method::OPTIONS, format!("http://127.0.0.1:{}/api/chat", addr.port()))
                .header("origin", origin)
                .header("access-control-request-method", "POST")
                .header("access-control-request-headers", "content-type")
                .send()
                .await
                .expect("preflight request");
            assert_eq!(pre.status().as_u16(), 204, "preflight must be 204 No Content");
            let hz = pre.headers();
            assert_eq!(hz.get("access-control-allow-origin").unwrap(), origin);
            assert_eq!(hz.get("access-control-allow-private-network").unwrap(), "true");
            assert!(hz.get("access-control-allow-methods").is_some());
            assert_eq!(hz.get("access-control-allow-headers").unwrap(), "content-type");

            // 2) A normal (non-preflight) response also carries the CORS headers.
            let ok = client
                .get(format!("http://127.0.0.1:{}/health", addr.port()))
                .header("origin", origin)
                .send()
                .await
                .expect("health request");
            assert_eq!(ok.status().as_u16(), 200);
            assert_eq!(
                ok.headers().get("access-control-allow-origin").unwrap(),
                origin,
                "normal responses must carry Access-Control-Allow-Origin"
            );
        });
    }
}
