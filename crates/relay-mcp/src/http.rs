//! The localhost HTTP server.
//!
//! - `POST/GET /mcp`: the MCP endpoint (Streamable HTTP). Needs
//!   `Authorization: Bearer <token>` from the config file.
//! - `GET /files/<token>`: the bytes of one approved file. The token works
//!   once and expires after a minute, so it needs no other auth.
//! - `GET /bridge`: WebSocket for the native messaging host (the link to
//!   the Chrome extension). Needs the same bearer token.
//! - `GET /health`: says this is an AI File Relay hub (JSON), so a
//!   `relay-mcp` started by an AI app can find it and forward to it.
//!
//! The server only listens on 127.0.0.1, and rmcp rejects requests whose
//! `Host` header is not localhost (protection against DNS rebinding).

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use futures_util::{SinkExt, StreamExt};
use relay_protocol::ExtensionToHub;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use tokio_util::io::ReaderStream;
use tokio_util::sync::CancellationToken;

use crate::extension::ExtensionBridge;
use crate::proxy::HubIdentity;
use crate::relay::{Relay, guess_mime};
use crate::server::RelayServer;

/// Build the whole router.
pub fn router(relay: Arc<Relay>, shutdown: CancellationToken) -> Router {
    let mcp_relay = relay.clone();
    let mcp: StreamableHttpService<RelayServer, LocalSessionManager> = StreamableHttpService::new(
        move || Ok(RelayServer::new(mcp_relay.clone())),
        Default::default(),
        StreamableHttpServerConfig::default().with_cancellation_token(shutdown.child_token()),
    );

    let token = Arc::new(relay.config.token.clone());
    let private_routes = Router::new()
        .route("/bridge", get(bridge_socket))
        .with_state((relay.clone(), shutdown.clone()))
        .nest_service("/mcp", mcp)
        .layer(middleware::from_fn_with_state(token, require_bearer));

    Router::new()
        .route("/health", get(health))
        .route("/files/{token}", get(serve_file))
        .with_state(relay)
        .merge(private_routes)
}

async fn health(State(relay): State<Arc<Relay>>) -> axum::Json<HubIdentity> {
    axum::Json(HubIdentity::new(&relay.hub_role))
}

/// The native host connects here.
async fn bridge_socket(
    State((relay, shutdown)): State<(Arc<Relay>, CancellationToken)>,
    ws: WebSocketUpgrade,
) -> Response {
    let Some(bridge) = relay.extension.clone() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "this hub has no extension link",
        )
            .into_response();
    };
    ws.on_upgrade(move |socket| run_bridge(bridge, socket, shutdown))
}

/// Pass messages between the WebSocket and the bridge until either side
/// closes or the hub shuts down.
async fn run_bridge(bridge: ExtensionBridge, socket: WebSocket, shutdown: CancellationToken) {
    let (id, mut outgoing) = bridge.attach();
    let (mut sink, mut stream) = socket.split();
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => {
                let _ = sink.send(Message::Close(None)).await;
                break;
            }
            out = outgoing.recv() => {
                let Some(msg) = out else { break };
                let Ok(text) = serde_json::to_string(&msg) else { continue };
                if sink.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
            incoming = stream.next() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<ExtensionToHub>(&text) {
                            Ok(msg) => {
                                if let Some(reply) = bridge.handle_incoming(id, msg) {
                                    let Ok(text) = serde_json::to_string(&reply) else { continue };
                                    if sink.send(Message::Text(text.into())).await.is_err() {
                                        break;
                                    }
                                }
                            }
                            Err(e) => tracing::warn!("bad message from the extension: {e}"),
                        }
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(_)) => {}
                }
            }
        }
    }
    bridge.detach(id);
}

/// Reject requests without the right bearer token.
async fn require_bearer(State(expected): State<Arc<String>>, req: Request, next: Next) -> Response {
    let given = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if !expected.is_empty() && constant_time_eq(given.as_bytes(), expected.as_bytes()) {
        next.run(req).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Bearer")],
            "missing or wrong bearer token (see `relay-mcp config`)",
        )
            .into_response()
    }
}

/// Compare without leaking how many leading bytes matched.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Send one approved file, once.
async fn serve_file(State(relay): State<Arc<Relay>>, Path(token): Path<String>) -> Response {
    let Some(path) = relay.tokens.redeem(&token) else {
        return (StatusCode::NOT_FOUND, "unknown, used or expired link").into_response();
    };
    let file = match tokio::fs::File::open(&path).await {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("could not open {}: {e}", path.display());
            return (StatusCode::GONE, "file is no longer available").into_response();
        }
    };
    let len = file.metadata().await.map(|m| m.len()).ok();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());

    let mut res = Body::from_stream(ReaderStream::new(file)).into_response();
    let headers = res.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(guess_mime(&name)),
    );
    if let Some(len) = len {
        headers.insert(header::CONTENT_LENGTH, HeaderValue::from(len));
    }
    // Keep only safe characters in the header; the extension gets the real
    // name from the upload job anyway.
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() && c != '"' && c != '\\' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename=\"{safe}\"")) {
        headers.insert(header::CONTENT_DISPOSITION, v);
    }
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;
    use relay_core::Config;
    use tower::ServiceExt;

    fn relay_with_file() -> (tempfile::TempDir, Arc<Relay>, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("hello.txt");
        std::fs::write(&file, "hello bytes").unwrap();
        let config = Config {
            token: "secret-token".into(),
            ..Config::default()
        };
        (dir, Arc::new(Relay::from_config(config)), file)
    }

    async fn get(app: Router, uri: &str, bearer: Option<&str>) -> (StatusCode, String) {
        let mut req = Request::builder()
            .uri(uri)
            .header(header::HOST, "127.0.0.1");
        if let Some(b) = bearer {
            req = req.header(header::AUTHORIZATION, format!("Bearer {b}"));
        }
        let res = app.oneshot(req.body(Body::empty()).unwrap()).await.unwrap();
        let status = res.status();
        let body = res.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    #[tokio::test]
    async fn file_link_works_once() {
        let (_dir, relay, file) = relay_with_file();
        let token = relay
            .tokens
            .issue(&file, std::time::Duration::from_secs(60));
        let app = router(relay, CancellationToken::new());

        let (status, body) = get(app.clone(), &format!("/files/{token}"), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, "hello bytes");

        let (status, _) = get(app, &format!("/files/{token}"), None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn unknown_file_link_is_404() {
        let (_dir, relay, _file) = relay_with_file();
        let (status, _) = get(router(relay, CancellationToken::new()), "/files/abc", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn mcp_needs_the_bearer_token() {
        let (_dir, relay, _file) = relay_with_file();
        let app = router(relay, CancellationToken::new());
        let (status, _) = get(app.clone(), "/mcp", None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, _) = get(app.clone(), "/mcp", Some("wrong")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        // With the right token the request reaches rmcp (which then has its
        // own rules about method and headers), so it is no longer a 401.
        let (status, _) = get(app, "/mcp", Some("secret-token")).await;
        assert_ne!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn bridge_needs_the_bearer_token() {
        let (_dir, relay, _file) = relay_with_file();
        let (status, _) = get(router(relay, CancellationToken::new()), "/bridge", None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn health_is_open() {
        let (_dir, relay, _file) = relay_with_file();
        let (status, body) = get(router(relay, CancellationToken::new()), "/health", None).await;
        assert_eq!(status, StatusCode::OK);
        let id: HubIdentity = serde_json::from_str(&body).unwrap();
        assert_eq!(id.app, "ai-file-relay");
        assert_eq!(id.role, "serve");
    }

    #[test]
    fn constant_time_eq_works() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
    }
}
