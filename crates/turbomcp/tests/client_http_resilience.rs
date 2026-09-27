//! The HTTP client against servers that do not behave like TurboMCP's own.
//!
//! Every case here passed CI before its fix, because the existing tests talk to
//! a server that never sends a plain-text 400, never expires a session under an
//! idle client, and never lets a response stream drop.
#![cfg(all(feature = "client", feature = "http"))]

use std::convert::Infallible;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use serde_json::{Map, Value, json};
use turbomcp::client::{ClientBuilder, ConnectMode, HttpClientLimits, HttpClientTransport};

/// One request as the server saw it.
#[derive(Clone, Debug)]
struct Seen {
    rpc: String,
    session: Option<String>,
    version: Option<String>,
}

type Script = dyn Fn(&Mock, &str, &Value, Option<&str>) -> Response + Send + Sync;

struct Mock {
    seen: Mutex<Vec<Seen>>,
    handshakes: AtomicUsize,
    expired: Mutex<Vec<String>>,
    script: Box<Script>,
}

impl Mock {
    fn new(
        script: impl Fn(&Mock, &str, &Value, Option<&str>) -> Response + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            seen: Mutex::new(Vec::new()),
            handshakes: AtomicUsize::new(0),
            expired: Mutex::new(Vec::new()),
            script: Box::new(script),
        })
    }

    fn seen(&self, rpc: &str) -> Vec<Seen> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.rpc == rpc)
            .cloned()
            .collect()
    }

    fn record(&self, rpc: &str, headers: &HeaderMap) -> Option<String> {
        let get = |name: &str| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        let session = get("mcp-session-id");
        self.seen.lock().unwrap().push(Seen {
            rpc: rpc.to_owned(),
            session: session.clone(),
            version: get("mcp-protocol-version"),
        });
        session
    }
}

fn result(id: &Value, result: Value) -> Response {
    Json(json!({ "jsonrpc": "2.0", "id": id, "result": result })).into_response()
}

/// An `initialize` answer minting `session-N`.
fn initialize(mock: &Mock, id: &Value) -> Response {
    let n = mock.handshakes.fetch_add(1, Ordering::SeqCst);
    let body = json!({ "jsonrpc": "2.0", "id": id, "result": {
        "protocolVersion": "2025-11-25",
        "capabilities": { "tools": {} },
        "serverInfo": { "name": "mock", "version": "1.0" }
    }});
    ([("mcp-session-id", format!("session-{n}"))], Json(body)).into_response()
}

/// A stream that stays open, sending nothing.
fn idle_stream() -> Response {
    let body = Body::from_stream(futures::stream::pending::<Result<String, Infallible>>());
    ([(header::CONTENT_TYPE, "text/event-stream")], body).into_response()
}

async fn serve(mock: Arc<Mock>) -> String {
    async fn on_post(
        State(mock): State<Arc<Mock>>,
        headers: HeaderMap,
        Json(msg): Json<Value>,
    ) -> Response {
        let rpc = msg["method"].as_str().unwrap_or("<response>").to_owned();
        let session = mock.record(&rpc, &headers);
        (mock.script)(&mock, &rpc, &msg, session.as_deref())
    }
    async fn on_get(State(mock): State<Arc<Mock>>, headers: HeaderMap) -> Response {
        let session = mock.record("<stream>", &headers);
        (mock.script)(&mock, "<stream>", &Value::Null, session.as_deref())
    }
    async fn on_delete(State(mock): State<Arc<Mock>>, headers: HeaderMap) -> Response {
        mock.record("<delete>", &headers);
        StatusCode::NO_CONTENT.into_response()
    }
    let app = Router::new()
        .route("/mcp", post(on_post).get(on_get).delete(on_delete))
        .with_state(mock);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}/mcp")
}

/// The standalone stream is always open, so it is usually the first thing to
/// meet an expired session. Its 404 used to clear the session id instead of
/// recovering: every later POST went out sessionless, got a 400 rather than the
/// 404 that triggers recovery, and the client stayed broken for good. The
/// recovery must also send `notifications/initialized` on the new session.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_the_stream_finds_expired_is_re_established() {
    let mock = Mock::new(|mock, rpc, msg, session| {
        if let Some(sid) = session
            && mock.expired.lock().unwrap().iter().any(|e| e == sid)
        {
            return StatusCode::NOT_FOUND.into_response();
        }
        match rpc {
            "initialize" => initialize(mock, &msg["id"]),
            // The first stream finds its session already forgotten.
            "<stream>" if session == Some("session-0") => {
                mock.expired.lock().unwrap().push("session-0".into());
                StatusCode::NOT_FOUND.into_response()
            }
            "<stream>" => idle_stream(),
            "tools/list" => result(&msg["id"], json!({ "tools": [] })),
            _ => StatusCode::ACCEPTED.into_response(),
        }
    });
    let url = serve(Arc::clone(&mock)).await;
    let client = ClientBuilder::new("idle", "1.0.0")
        .with_connect_mode(ConnectMode::Legacy)
        .connect(HttpClientTransport::new(&url).unwrap())
        .await
        .expect("handshake");

    // Give the stream time to meet the 404 and recover.
    tokio::time::sleep(Duration::from_millis(300)).await;
    client
        .list_tools(None)
        .await
        .expect("the client recovered the session the stream found expired");

    assert_eq!(
        mock.handshakes.load(Ordering::SeqCst),
        2,
        "one recovery handshake"
    );
    let lists = mock.seen("tools/list");
    assert_eq!(lists.last().unwrap().session.as_deref(), Some("session-1"));
    let initialized = mock.seen("notifications/initialized");
    assert!(
        initialized
            .iter()
            .any(|s| s.session.as_deref() == Some("session-1")),
        "the new session is confirmed with `initialized`: {initialized:?}"
    );
}

/// go-sdk answers an unrecognized `MCP-Protocol-Version` with a plain-text
/// 400. That is a legacy server; the probe used to give up on it. And the
/// fallback `initialize` used to carry the probe's `2026-07-28` header, which
/// the same server refuses again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auto_falls_back_past_a_plain_text_400_without_the_probe_version() {
    let mock = Mock::new(|mock, rpc, msg, _| match rpc {
        "server/discover" => (
            StatusCode::BAD_REQUEST,
            "Bad Request: unsupported protocol version",
        )
            .into_response(),
        "initialize" => initialize(mock, &msg["id"]),
        "<stream>" => StatusCode::METHOD_NOT_ALLOWED.into_response(),
        _ => StatusCode::ACCEPTED.into_response(),
    });
    let url = serve(Arc::clone(&mock)).await;
    let client = ClientBuilder::new("auto", "1.0.0")
        .connect(HttpClientTransport::new(&url).unwrap())
        .await
        .expect("a plain-text 400 to the probe means legacy");
    assert_eq!(client.protocol_version().as_str(), "2025-11-25");

    let init = mock.seen("initialize");
    assert_eq!(init.len(), 1);
    assert_eq!(
        init[0].version, None,
        "initialize negotiates; it claims no version"
    );
}

/// "The client MUST include the `MCP-Protocol-Version` header on all
/// subsequent requests" — including the closing `DELETE`, which a strict
/// server otherwise refuses, leaving the session behind.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_closing_delete_carries_the_protocol_version() {
    let mock = Mock::new(|mock, rpc, msg, _| match rpc {
        "initialize" => initialize(mock, &msg["id"]),
        "<stream>" => StatusCode::METHOD_NOT_ALLOWED.into_response(),
        _ => StatusCode::ACCEPTED.into_response(),
    });
    let url = serve(Arc::clone(&mock)).await;
    let client = ClientBuilder::new("closer", "1.0.0")
        .with_connect_mode(ConnectMode::Legacy)
        .connect(HttpClientTransport::new(&url).unwrap())
        .await
        .unwrap();
    client.close().await;

    let delete = mock.seen("<delete>");
    assert_eq!(delete.len(), 1);
    assert_eq!(delete[0].version.as_deref(), Some("2025-11-25"));
}

fn discover(id: &Value) -> Response {
    result(
        id,
        json!({
            "resultType": "complete", "ttlMs": 0, "cacheScope": "private",
            "supportedVersions": ["2026-07-28"],
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "mock", "version": "1.0" }
        }),
    )
}

/// A response stream that ends before the response used to leave the caller
/// waiting out the whole timeout, then report it as a timeout. 2026-07-28:
/// "A broken response stream loses the in-flight request; clients MUST
/// re-issue it as a new request with a new request ID". A list is re-issued; a
/// tool call is reported, since running it again could repeat its effect.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_lost_response_stream_fails_fast_and_reads_are_re_issued() {
    let lists = Arc::new(AtomicUsize::new(0));
    let mock = {
        let lists = Arc::clone(&lists);
        Mock::new(move |_, rpc, msg, _| match rpc {
            "server/discover" => discover(&msg["id"]),
            // The first `tools/list` stream closes with nothing on it.
            "tools/list" if lists.fetch_add(1, Ordering::SeqCst) == 0 => {
                ([(header::CONTENT_TYPE, "text/event-stream")], "").into_response()
            }
            "tools/list" => result(
                &msg["id"],
                json!({ "resultType": "complete", "ttlMs": 0, "cacheScope": "private", "tools": [] }),
            ),
            "tools/call" => ([(header::CONTENT_TYPE, "text/event-stream")], "").into_response(),
            _ => StatusCode::ACCEPTED.into_response(),
        })
    };
    let url = serve(Arc::clone(&mock)).await;
    let client = ClientBuilder::new("lossy", "1.0.0")
        .with_connect_mode(ConnectMode::Modern)
        .with_timeout(Duration::from_secs(20))
        .connect(HttpClientTransport::new(&url).unwrap())
        .await
        .unwrap();

    client.list_tools(None).await.expect("re-issued once");
    assert_eq!(
        lists.load(Ordering::SeqCst),
        2,
        "the lost request and its re-issue"
    );

    let started = Instant::now();
    let err = client
        .call_tool("t", Map::new())
        .await
        .expect_err("a tool call is not re-run");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "failed fast: {:?}",
        started.elapsed()
    );
    assert!(err.to_string().contains("stream"), "{err}");
    assert_eq!(mock.seen("tools/call").len(), 1);
}

/// Running out of POST slots used to be treated as the transport failing, which
/// ended the client and every call in flight. Only the call that did not fit
/// fails now.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn running_out_of_post_slots_fails_one_call_not_the_client() {
    let mock = Mock::new(|_, rpc, msg, _| match rpc {
        "server/discover" => discover(&msg["id"]),
        "tools/call" => {
            // Hold the slot long enough for the second call to arrive: the
            // body arrives late, and the client holds the POST until it does.
            let body = json!({ "jsonrpc": "2.0", "id": msg["id"], "result": {
                "resultType": "complete", "content": [], "isError": false
            }})
            .to_string();
            let late = futures::stream::once(async move {
                tokio::time::sleep(Duration::from_millis(300)).await;
                Ok::<_, Infallible>(body)
            });
            (
                [(header::CONTENT_TYPE, "application/json")],
                Body::from_stream(late),
            )
                .into_response()
        }
        _ => StatusCode::ACCEPTED.into_response(),
    });
    let url = serve(Arc::clone(&mock)).await;
    let mut limits = HttpClientLimits::default();
    limits.max_posts = 1;
    let transport = HttpClientTransport::new(&url).unwrap().with_limits(limits);
    let client = ClientBuilder::new("tight", "1.0.0")
        .with_connect_mode(ConnectMode::Modern)
        .connect(transport)
        .await
        .unwrap();

    let (first, second) = tokio::join!(client.call_tool("a", Map::new()), async {
        tokio::time::sleep(Duration::from_millis(50)).await;
        client.call_tool("b", Map::new()).await
    });
    assert!(first.is_ok(), "{first:?}");
    let refused = second.expect_err("no slot for the second call");
    assert!(refused.to_string().contains("max_posts"), "{refused}");

    client
        .call_tool("c", Map::new())
        .await
        .expect("the client is still connected");
}
