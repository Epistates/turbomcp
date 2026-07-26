//! Client HTTP transport failure semantics — the counterpart to
//! `client_robustness.rs`, which covers the same ground over stdio.
//!
//! The HTTP transport POSTs from a detached task, so a failed POST has no
//! caller to return an error to; it has to *manufacture* one addressed to the
//! waiting request. If that routing is wrong the symptom is not an error, it
//! is a caller that blocks until its request timeout on a server that already
//! said "no" — which is why these drive a deliberately misbehaving endpoint
//! rather than the real server.

#![cfg(all(feature = "client", feature = "http"))]

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use turbomcp::client::{Client, ClientBuilder, ConnectMode, connect_http};

/// The session id the fake endpoint hands out on the handshake.
const SESSION: &str = "sess-http-failures";

/// How the endpoint answers `tools/list`.
#[derive(Clone, Copy, PartialEq)]
enum OnList {
    /// A plain `application/json` frame.
    Json,
    /// An SSE stream carrying the spec's primer event (an id with empty
    /// `data`) and a keep-alive comment around the real frame.
    SseWithPrimer,
    /// Refuse outright.
    ServerError,
}

struct Endpoint {
    behavior: OnList,
    /// `DELETE`s seen, and the session id each carried.
    deletes: AtomicUsize,
    deleted_session: std::sync::Mutex<Option<String>>,
}

fn discover_result() -> Value {
    json!({
        "capabilities": { "tools": {} },
        "supportedVersions": ["2026-07-28"],
        "resultType": "complete", "cacheScope": "private", "ttlMs": 0,
    })
}

async fn handle_post(State(ep): State<Arc<Endpoint>>, body: String) -> Response {
    let frame: Value = serde_json::from_str(&body).expect("valid json from the client");
    let id = frame.get("id").cloned().unwrap_or(Value::Null);
    let method = frame
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();

    let session = [(header::HeaderName::from_static("mcp-session-id"), SESSION)];

    let result = match method.as_str() {
        "server/discover" => discover_result(),
        "tools/list" => match ep.behavior {
            OnList::ServerError => {
                return (StatusCode::INTERNAL_SERVER_ERROR, session, "upstream down")
                    .into_response();
            }
            OnList::Json | OnList::SseWithPrimer => json!({
                "tools": [], "resultType": "complete", "cacheScope": "private", "ttlMs": 0,
            }),
        },
        other => panic!("unexpected method from the client: {other}"),
    };
    let reply = json!({ "jsonrpc": "2.0", "id": id, "result": result });

    if ep.behavior == OnList::SseWithPrimer && method == "tools/list" {
        // The primer is the spec's SHOULD (an event id with empty `data`) so a
        // resuming client has a `Last-Event-ID` to resume from; the comment is
        // an ordinary keep-alive. Neither is a JSON-RPC frame, and neither may
        // disturb the response that follows.
        let stream = format!("id: primer-1\ndata: \n\n: keep-alive\n\ndata: {reply}\n\n");
        return (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "text/event-stream"),
                (header::HeaderName::from_static("mcp-session-id"), SESSION),
            ],
            stream,
        )
            .into_response();
    }
    (StatusCode::OK, session, axum::Json(reply)).into_response()
}

async fn handle_delete(State(ep): State<Arc<Endpoint>>, headers: HeaderMap) -> StatusCode {
    ep.deletes.fetch_add(1, Ordering::SeqCst);
    *ep.deleted_session.lock().unwrap() = headers
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    StatusCode::NO_CONTENT
}

/// Spawn the fake endpoint; returns its `/mcp` URL and the shared state.
async fn spawn(behavior: OnList) -> (String, Arc<Endpoint>) {
    let ep = Arc::new(Endpoint {
        behavior,
        deletes: AtomicUsize::new(0),
        deleted_session: std::sync::Mutex::new(None),
    });
    let app = axum::Router::new()
        .route(
            "/mcp",
            axum::routing::post(handle_post).delete(handle_delete),
        )
        .with_state(Arc::clone(&ep));
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}/mcp"), ep)
}

async fn connect(behavior: OnList) -> (Client, Arc<Endpoint>) {
    let (url, ep) = spawn(behavior).await;
    let client = connect_http(
        ClientBuilder::new("http-failures", "1.0.0")
            .with_connect_mode(ConnectMode::Modern)
            .with_timeout(Duration::from_secs(30)),
        url,
    )
    .await
    .expect("handshake succeeds");
    (client, ep)
}

/// The one that matters: a non-2xx response must reach the caller as an error,
/// promptly. The POST runs in a detached task, so nothing returns this failure
/// naturally — it is synthesized as a response addressed to the pending
/// request. Get that wrong and the caller waits out its full timeout (30s
/// here) on a server that already refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_post_fails_the_waiting_caller_promptly() {
    let (client, _ep) = connect(OnList::ServerError).await;

    let result = tokio::time::timeout(Duration::from_secs(2), client.list_tools(None))
        .await
        .expect("the failure arrives promptly, not after the 30s request timeout");

    let err = result.expect_err("a 500 is not a successful list");
    let msg = err.to_string();
    assert!(
        msg.contains("500"),
        "the error should name the status the server returned: {msg}"
    );
}

/// A server that refused one request has not invalidated the connection: the
/// error is scoped to its own caller, and the client keeps working.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_post_does_not_kill_the_connection() {
    let (client, _ep) = connect(OnList::ServerError).await;
    assert!(client.list_tools(None).await.is_err());
    assert!(
        client.list_tools(None).await.is_err(),
        "a second request must still be routed (and refused), not report a \
         dead connection"
    );
}

/// Nothing is listening on the port, so the handshake POST cannot connect.
/// That has to surface as a connect error rather than a client that appears to
/// come up and then hangs on its first call.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unreachable_endpoint_fails_the_handshake() {
    // Port 1 on loopback is reserved; nothing binds it.
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        connect_http(
            ClientBuilder::new("http-failures", "1.0.0")
                .with_connect_mode(ConnectMode::Modern)
                .with_timeout(Duration::from_secs(2)),
            "http://127.0.0.1:1/mcp",
        ),
    )
    .await
    .expect("the connect attempt gives up rather than hanging");
    assert!(
        result.is_err(),
        "an unreachable endpoint is not a connection"
    );
}

/// Non-frame SSE events — the primer (`id:` with empty `data`) and keep-alive
/// comments — must be skipped, not decoded. Decoding an empty `data` would
/// fail the whole request, and this server sends a primer on every draft
/// POST-response stream.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn primer_and_keep_alive_events_do_not_disturb_the_response() {
    let (client, _ep) = connect(OnList::SseWithPrimer).await;
    let tools = tokio::time::timeout(Duration::from_secs(2), client.list_tools(None))
        .await
        .expect("no hang")
        .expect("the frame after the primer is the response");
    assert!(tools.tools.is_empty());
}

/// Dropping the client terminates its session with the spec's explicit
/// `DELETE`, carrying the session id the server issued. Without it a
/// long-running server accumulates a session per client that ever connected.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_the_client_deletes_its_session() {
    let (client, ep) = connect(OnList::Json).await;
    client.list_tools(None).await.expect("list");
    drop(client);

    // The DELETE is issued by the transport as the connection actor unwinds.
    for _ in 0..50 {
        if ep.deletes.load(Ordering::SeqCst) > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        ep.deletes.load(Ordering::SeqCst),
        1,
        "a dropped client must terminate its session exactly once"
    );
    assert_eq!(
        ep.deleted_session.lock().unwrap().as_deref(),
        Some(SESSION),
        "the DELETE must name the session the server issued"
    );
}
