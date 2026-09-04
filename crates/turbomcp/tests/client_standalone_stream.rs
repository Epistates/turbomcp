//! The client must open the standalone server→client SSE stream (`GET` on the
//! MCP endpoint) and answer requests that arrive on it.
//!
//! Streamable HTTP lets a server deliver a message it originates in either of
//! two places: inline on the SSE stream of whichever POST it is answering, or
//! on the standalone stream the client opens with a `GET`. Nothing on the wire
//! says which a given server will use.
//!
//! TurboMCP's own server always answers inline, so every in-repo elicitation
//! test passed while the client never issued the `GET` at all — and against the
//! reference TypeScript SDK server, which uses the standalone stream, an
//! `elicitation/create` simply never arrived and the call hung until timeout.
//! Testing both halves of one SDK against each other cannot catch that; the
//! mock below deliberately behaves the way our server does *not*.

#![cfg(all(feature = "client", feature = "http"))]

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{Map, Value, json};
use tokio::sync::{Mutex, mpsc, oneshot};
use turbomcp::client::{ClientBuilder, ClientHandler, ConnectMode, async_trait, connect_http};
use turbomcp::neutral;

/// Answers every elicitation by accepting with a fixed marker value, so the
/// mock can prove the request reached a handler rather than merely being sent.
struct Confirming;

#[async_trait]
impl ClientHandler for Confirming {
    async fn elicit(&self, request: neutral::ElicitParams) -> neutral::ElicitOutcome {
        assert_eq!(request.message, "confirm?");
        let mut content = Map::new();
        content.insert("ok".into(), Value::Bool(true));
        neutral::ElicitOutcome::new(neutral::ElicitAction::Accept, content)
    }
}

/// What the mock server needs to hand between its two routes.
struct Mock {
    /// Frames to push down the standalone `GET` stream once it is opened.
    outbound: Mutex<Option<mpsc::Receiver<String>>>,
    /// Sends the client's elicitation *response* back to the test body.
    answered: Mutex<Option<oneshot::Sender<Value>>>,
}

/// `POST /mcp` — the handshake, plus whatever the client sends back.
async fn mcp_post(State(mock): State<Arc<Mock>>, Json(body): Json<Value>) -> Response {
    let id = body.get("id").cloned();
    let method = body.get("method").and_then(Value::as_str).unwrap_or("");

    match method {
        "initialize" => (
            [("mcp-session-id", "session-under-test")],
            Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2025-11-25",
                    "capabilities": {},
                    "serverInfo": { "name": "standalone-stream-mock", "version": "1.0.0" },
                },
            })),
        )
            .into_response(),

        // A notification: nothing to answer.
        "" if id.is_none() => StatusCode::ACCEPTED.into_response(),
        "notifications/initialized" => StatusCode::ACCEPTED.into_response(),

        // No method means this is a *response* — the client answering the
        // elicitation we pushed down the standalone stream. That arriving at
        // all is the whole point of the test.
        "" => {
            if let Some(tx) = mock.answered.lock().await.take() {
                let _ = tx.send(body);
            }
            StatusCode::ACCEPTED.into_response()
        }

        _ => Json(json!({ "jsonrpc": "2.0", "id": id, "result": {} })).into_response(),
    }
}

/// `GET /mcp` — the standalone stream, carrying a server→client request.
async fn mcp_get(State(mock): State<Arc<Mock>>) -> Response {
    let Some(rx) = mock.outbound.lock().await.take() else {
        // Only the first GET carries the scripted traffic; a reconnect after it
        // gets an empty stream rather than a duplicate request.
        return StatusCode::NO_CONTENT.into_response();
    };

    let stream = futures::stream::unfold(rx, |mut rx| async move {
        let frame = rx.recv().await?;
        Ok::<_, std::io::Error>(format!("data: {frame}\n\n"))
            .map(|chunk| (chunk, rx))
            .ok()
    })
    .map(Ok::<_, std::io::Error>);

    (
        [
            (header::CONTENT_TYPE, "text/event-stream"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        Body::from_stream(stream),
    )
        .into_response()
}

use futures::StreamExt as _;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_server_request_on_the_standalone_stream_is_answered() {
    let (frames_tx, frames_rx) = mpsc::channel::<String>(4);
    let (answered_tx, answered_rx) = oneshot::channel::<Value>();
    let mock = Arc::new(Mock {
        outbound: Mutex::new(Some(frames_rx)),
        answered: Mutex::new(Some(answered_tx)),
    });

    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("bind ephemeral port");
    let addr: SocketAddr = listener.local_addr().unwrap();
    let app = Router::new()
        .route("/mcp", post(mcp_post).get(mcp_get))
        .with_state(Arc::clone(&mock));
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let client = connect_http(
        ClientBuilder::new("standalone-stream-test", "1.0.0")
            .with_handler(Confirming)
            .with_connect_mode(ConnectMode::Legacy)
            .with_capabilities(json!({ "elicitation": { "formats": ["form"] } })),
        format!("http://{addr}/mcp"),
    )
    .await
    .expect("handshake");

    // Push the request only once the stream can exist: the client opens it off
    // the back of the handshake, so a frame queued earlier would just sit in
    // the channel until then anyway — this only keeps the failure legible.
    frames_tx
        .send(
            json!({
                "jsonrpc": "2.0",
                "id": "elicit-1",
                "method": "elicitation/create",
                "params": {
                    "message": "confirm?",
                    "requestedSchema": {
                        "type": "object",
                        "properties": { "ok": { "type": "boolean" } },
                    },
                },
            })
            .to_string(),
        )
        .await
        .expect("queue server request");

    let answer = tokio::time::timeout(Duration::from_secs(10), answered_rx)
        .await
        .expect(
            "client never answered a request delivered on the standalone GET stream — \
             it is not opening one, so any server that pushes there is unreachable",
        )
        .expect("answer channel dropped");

    assert_eq!(answer["id"], "elicit-1");
    assert_eq!(answer["result"]["action"], "accept");
    assert_eq!(answer["result"]["content"]["ok"], true);

    drop(client);
    server.abort();
}

/// A server with nothing to push answers `405`, and that is a complete answer —
/// the client must not sit in a reconnect loop against it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_405_on_the_standalone_stream_is_not_retried_forever() {
    let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("bind ephemeral port");
    let addr: SocketAddr = listener.local_addr().unwrap();

    let counted = Arc::clone(&hits);
    let app = Router::new()
        .route(
            "/mcp",
            post(|Json(body): Json<Value>| async move {
                let id = body.get("id").cloned();
                match body.get("method").and_then(Value::as_str).unwrap_or("") {
                    "initialize" => Json(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "protocolVersion": "2025-11-25",
                            "capabilities": {},
                            "serverInfo": { "name": "no-stream-mock", "version": "1.0.0" },
                        },
                    }))
                    .into_response(),
                    _ if id.is_none() => StatusCode::ACCEPTED.into_response(),
                    _ => Json(json!({ "jsonrpc": "2.0", "id": id, "result": {} })).into_response(),
                }
            })
            .get(move |_: HeaderMap| {
                let counted = Arc::clone(&counted);
                async move {
                    counted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    StatusCode::METHOD_NOT_ALLOWED
                }
            }),
        )
        .into_make_service();

    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let client = connect_http(
        ClientBuilder::new("no-stream-test", "1.0.0")
            .with_handler(Confirming)
            .with_connect_mode(ConnectMode::Legacy),
        format!("http://{addr}/mcp"),
    )
    .await
    .expect("handshake");

    // Comfortably longer than the 1s default reconnect delay: a client that
    // treated 405 as retryable would be several attempts in by now.
    tokio::time::sleep(Duration::from_secs(3)).await;
    let attempts = hits.load(std::sync::atomic::Ordering::Relaxed);
    // Both bounds matter. Zero would mean this test is vacuous — it would pass
    // just as happily against the bug it exists to guard, a client that never
    // opens the stream at all.
    assert_eq!(
        attempts, 1,
        "expected exactly one standalone GET against a server answering 405; got {attempts}. \
         Zero means the client never opens the stream; more than one means it treats a \
         refusal to offer it as a transient error rather than a final answer",
    );

    drop(client);
    server.abort();
}
