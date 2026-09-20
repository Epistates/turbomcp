//! An expired legacy session must be re-established, not surfaced as a failure.
//!
//! "When a client receives HTTP 404 in response to a request containing an
//! `Mcp-Session-Id`, it MUST start a new session by sending a new
//! `InitializeRequest` without a session ID attached" (`2025-11-25` and
//! `2025-06-18` §Session Management).
//!
//! Before this the client had no 404 branch at all: the failure reached the
//! caller and the dead session id was replayed on every subsequent POST
//! forever, against a server that answers 404 to exactly that.
#![cfg(all(feature = "client", feature = "http"))]

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use turbomcp::client::{ClientBuilder, ConnectMode, HttpClientTransport};

/// Mints a session, then declares the *first* one expired the next time it is
/// used — the shape of a server restart or an idle eviction.
#[derive(Default)]
struct Mock {
    minted: Mutex<Vec<String>>,
    /// Session ids the server no longer knows.
    expired: Mutex<Vec<String>>,
    handshakes: AtomicUsize,
    tools_calls: AtomicUsize,
}

async fn handle_post(
    State(s): State<Arc<Mock>>,
    headers: HeaderMap,
    Json(msg): Json<Value>,
) -> Response {
    let method = msg["method"].as_str().unwrap_or_default();
    let sent = headers
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    if method == "initialize" {
        let n = s.handshakes.fetch_add(1, Ordering::SeqCst);
        let sid = format!("session-{n}");
        s.minted.lock().unwrap().push(sid.clone());
        // The first session is dead from the moment the second is requested.
        if n > 0 {
            s.expired.lock().unwrap().push("session-0".to_owned());
        }
        let body = json!({
            "jsonrpc": "2.0",
            "id": msg["id"],
            "result": {
                "protocolVersion": "2025-11-25",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "mock", "version": "1.0" }
            }
        });
        return ([("mcp-session-id", sid)], Json(body)).into_response();
    }

    // Any session-bearing request naming a session the server has forgotten is
    // a 404, which is the whole point of the exercise.
    if let Some(sid) = &sent
        && s.expired.lock().unwrap().contains(sid)
    {
        return StatusCode::NOT_FOUND.into_response();
    }
    // The first `tools/list` after the handshake is answered against a session
    // the server then forgets.
    if method == "tools/list" {
        let n = s.tools_calls.fetch_add(1, Ordering::SeqCst);
        if n == 0 {
            // Forget the session this very request rode in on, so the *next*
            // one 404s.
            if let Some(sid) = sent {
                s.expired.lock().unwrap().push(sid);
            }
        }
        let body = json!({
            "jsonrpc": "2.0",
            "id": msg["id"],
            "result": { "tools": [{ "name": "noop", "inputSchema": { "type": "object" } }] }
        });
        return Json(body).into_response();
    }
    StatusCode::ACCEPTED.into_response()
}

async fn serve(mock: Arc<Mock>) -> String {
    let app = Router::new()
        .route("/mcp", post(handle_post))
        .with_state(mock);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}/mcp")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_expired_session_is_re_established_rather_than_failing_the_call() {
    let mock = Arc::new(Mock::default());
    let url = serve(Arc::clone(&mock)).await;

    let transport = HttpClientTransport::new(&url).expect("transport");
    let client = ClientBuilder::new("expiring", "1.0.0")
        .with_connect_mode(ConnectMode::Legacy)
        .connect(transport)
        .await
        .expect("handshake");

    // Succeeds on the original session, which the server then forgets.
    client.list_tools(None).await.expect("first call");

    // This one meets the 404. The client must re-handshake and retry rather
    // than handing the caller an error.
    let tools = client
        .list_tools(None)
        .await
        .expect("an expired session must be re-established, not surfaced");
    assert_eq!(tools.tools.len(), 1);

    assert_eq!(
        mock.handshakes.load(Ordering::SeqCst),
        2,
        "the recovery must be a real second `initialize`"
    );
    let minted = mock.minted.lock().unwrap().clone();
    assert_eq!(minted, ["session-0", "session-1"]);
}
