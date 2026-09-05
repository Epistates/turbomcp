//! The client's HTTP transport presents a bearer token on every request it
//! makes — POST, the standalone GET stream, and the session-terminating DELETE.
//!
//! `turbomcp-auth` has implemented the OAuth 2.1 client flow (discovery, DCR,
//! auth-code + PKCE, refresh) since 13a, but the client transport had no way to
//! *send* the resulting token, so the two halves were never actually connected:
//! you could obtain an access token and then had to write your own transport to
//! use it. `with_bearer` / `with_bearer_source` is that seam.
//!
//! All three request kinds matter. Missing it on the GET leaves every
//! server→client request undeliverable on an authenticated server; missing it
//! on the DELETE turns session termination into a 401, and the session leaks.

#![cfg(all(feature = "client", feature = "http"))]

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{Value, json};
use turbomcp::client::{BearerSource, ClientBuilder, ClientHandler, ConnectMode, async_trait};
use turbomcp::neutral;

/// Every `Authorization` header the mock saw, tagged with the HTTP method.
type Seen = Arc<Mutex<Vec<(String, Option<String>)>>>;

struct Silent;

#[async_trait]
impl ClientHandler for Silent {
    async fn elicit(&self, _request: neutral::ElicitParams) -> neutral::ElicitOutcome {
        neutral::ElicitOutcome::new(neutral::ElicitAction::Decline, serde_json::Map::new())
    }
}

fn record(seen: &Seen, method: &str, headers: &HeaderMap) {
    let value = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(std::string::ToString::to_string);
    seen.lock().unwrap().push((method.to_string(), value));
}

async fn mock_post(
    State(seen): State<Seen>,
    headers: HeaderMap,
    body: String,
) -> impl IntoResponse {
    record(&seen, "POST", &headers);
    let body: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    let id = body.get("id").cloned();
    if body.get("method").and_then(Value::as_str) == Some("initialize") {
        return (
            [("mcp-session-id", "sess-1")],
            Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2025-11-25",
                    "capabilities": {},
                    "serverInfo": { "name": "bearer-mock", "version": "1.0.0" },
                },
            })),
        )
            .into_response();
    }
    if id.is_none() {
        return StatusCode::ACCEPTED.into_response();
    }
    Json(json!({ "jsonrpc": "2.0", "id": id, "result": { "tools": [] } })).into_response()
}

async fn mock_get(State(seen): State<Seen>, headers: HeaderMap) -> impl IntoResponse {
    record(&seen, "GET", &headers);
    // 405 keeps the listener from reconnecting, so the recording stays one
    // entry rather than growing for the life of the test.
    StatusCode::METHOD_NOT_ALLOWED
}

async fn mock_delete(State(seen): State<Seen>, headers: HeaderMap) -> impl IntoResponse {
    record(&seen, "DELETE", &headers);
    StatusCode::NO_CONTENT
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_request_kind_carries_the_bearer_token() {
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));

    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("bind ephemeral port");
    let addr: SocketAddr = listener.local_addr().unwrap();
    let app = Router::new()
        .route("/mcp", post(mock_post).get(mock_get).delete(mock_delete))
        .with_state(Arc::clone(&seen));
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let transport = turbomcp::client::HttpClientTransport::new(format!("http://{addr}/mcp"))
        .expect("build transport")
        .with_bearer("token-abc");
    let client = ClientBuilder::new("bearer-test", "1.0.0")
        .with_handler(Silent)
        .with_connect_mode(ConnectMode::Legacy)
        .connect(transport)
        .await
        .expect("handshake");

    let _ = client.list_tools(None).await;

    // The DELETE goes out when the connection actor winds down, which happens
    // after the last clone drops rather than inside it.
    drop(client);
    tokio::time::sleep(Duration::from_millis(500)).await;

    let seen = seen.lock().unwrap().clone();
    for method in ["POST", "GET", "DELETE"] {
        let observed: Vec<&Option<String>> = seen
            .iter()
            .filter(|(m, _)| m == method)
            .map(|(_, v)| v)
            .collect();
        assert!(
            !observed.is_empty(),
            "no {method} reached the server; saw {seen:?}",
        );
        assert!(
            observed
                .iter()
                .all(|v| v.as_deref() == Some("Bearer token-abc")),
            "a {method} went out without the bearer token: {observed:?}",
        );
    }

    server.abort();
}

/// A source is consulted per request, so a refreshed token applies to the next
/// one without reconnecting — the reason this is a trait and not a `String`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_refreshed_token_applies_without_reconnecting() {
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));

    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("bind ephemeral port");
    let addr: SocketAddr = listener.local_addr().unwrap();
    let app = Router::new()
        .route("/mcp", post(mock_post).get(mock_get).delete(mock_delete))
        .with_state(Arc::clone(&seen));
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let rotating = Arc::new(Mutex::new(Some("first".to_string())));
    let transport = turbomcp::client::HttpClientTransport::new(format!("http://{addr}/mcp"))
        .expect("build transport")
        .with_bearer_source(Arc::clone(&rotating) as Arc<dyn BearerSource>);
    let client = ClientBuilder::new("refresh-test", "1.0.0")
        .with_handler(Silent)
        .with_connect_mode(ConnectMode::Legacy)
        .connect(transport)
        .await
        .expect("handshake");

    let _ = client.list_tools(None).await;
    *rotating.lock().unwrap() = Some("second".to_string());
    let _ = client.list_tools(None).await;

    let posts: Vec<Option<String>> = seen
        .lock()
        .unwrap()
        .iter()
        .filter(|(m, _)| m == "POST")
        .map(|(_, v)| v.clone())
        .collect();
    assert!(
        posts.iter().any(|v| v.as_deref() == Some("Bearer first")),
        "no request used the original token: {posts:?}",
    );
    assert!(
        posts.iter().any(|v| v.as_deref() == Some("Bearer second")),
        "the rotated token never reached the wire, so the source is being \
         captured once instead of consulted per request: {posts:?}",
    );

    drop(client);
    server.abort();
}
