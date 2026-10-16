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
    auth: Option<String>,
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
            auth: get("authorization"),
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
        // Wait until the first call holds the slot. A fixed head start lost
        // the race on a loaded CI runner, and then the *first* call was the
        // one refused. Bounded, so a first call that never reaches the
        // server fails the test instead of hanging it.
        tokio::time::timeout(Duration::from_secs(10), async {
            while mock.seen("tools/call").is_empty() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the first call reached the server");
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

/// A slot is free again by the time its caller has the response: with one
/// slot, back-to-back calls never collide with the call before them.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sequential_calls_never_find_their_own_slot_taken() {
    let mock = Mock::new(|_, rpc, msg, _| match rpc {
        "server/discover" => discover(&msg["id"]),
        "tools/call" => result(
            &msg["id"],
            json!({ "resultType": "complete", "content": [], "isError": false }),
        ),
        _ => StatusCode::ACCEPTED.into_response(),
    });
    let url = serve(Arc::clone(&mock)).await;
    let mut limits = HttpClientLimits::default();
    limits.max_posts = 1;
    let client = ClientBuilder::new("sequential", "1.0.0")
        .with_connect_mode(ConnectMode::Modern)
        .connect(HttpClientTransport::new(&url).unwrap().with_limits(limits))
        .await
        .unwrap();
    for n in 0..300 {
        client
            .call_tool("t", Map::new())
            .await
            .unwrap_or_else(|e| panic!("call {n} was refused: {e}"));
    }
}

/// Concurrent calls meeting an expired session all recover. Recovery cleared
/// the session id before re-initializing, so a call that started during the
/// re-handshake went out with no session at all, got the server's `400`
/// "session required", and failed for good.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn calls_racing_a_session_recovery_never_go_out_sessionless() {
    let recovering = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = Arc::clone(&recovering);
    let mock = Mock::new(move |mock, rpc, msg, session| {
        if let Some(sid) = session
            && mock.expired.lock().unwrap().iter().any(|e| e == sid)
        {
            return StatusCode::NOT_FOUND.into_response();
        }
        match rpc {
            "initialize" => {
                // A slow re-handshake: the window the other calls race.
                if mock.handshakes.load(Ordering::SeqCst) > 0 {
                    flag.store(true, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(300));
                }
                initialize(mock, &msg["id"])
            }
            "<stream>" => StatusCode::METHOD_NOT_ALLOWED.into_response(),
            "tools/list" if session.is_none() => {
                (StatusCode::BAD_REQUEST, "session required").into_response()
            }
            "tools/list" => result(&msg["id"], json!({ "tools": [] })),
            _ => StatusCode::ACCEPTED.into_response(),
        }
    });
    let url = serve(Arc::clone(&mock)).await;
    let client = ClientBuilder::new("racer", "1.0.0")
        .with_connect_mode(ConnectMode::Legacy)
        .connect(HttpClientTransport::new(&url).unwrap())
        .await
        .expect("handshake");

    mock.expired.lock().unwrap().push("session-0".into());
    let first = tokio::spawn({
        let client = client.clone();
        async move { client.list_tools(None).await }
    });
    // The rest start once the recovery handshake is under way.
    while !recovering.load(Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let racing = (0..3).map(|_| {
        let client = client.clone();
        async move { client.list_tools(None).await }
    });
    for outcome in futures::future::join_all(racing).await {
        outcome.expect("a call racing the recovery recovers too");
    }
    first
        .await
        .unwrap()
        .expect("the call that found it expired recovers");
    assert!(
        mock.seen("tools/list").iter().all(|s| s.session.is_some()),
        "no call went out sessionless: {:?}",
        mock.seen("tools/list")
    );
    assert_eq!(mock.handshakes.load(Ordering::SeqCst), 2, "one recovery");
}

/// A JSON answer that isn't the request's response fails the request at
/// once; an id-less error answers it. Both used to leave the caller waiting
/// for an answer that POST would never carry.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_json_answer_that_is_not_the_response_fails_the_call() {
    let mock = Mock::new(|mock, rpc, msg, _| match rpc {
        "initialize" => initialize(mock, &msg["id"]),
        "<stream>" => StatusCode::METHOD_NOT_ALLOWED.into_response(),
        "tools/list" => result(&json!("someone-else"), json!({ "tools": [] })),
        "tools/call" => Json(json!({
            "jsonrpc": "2.0",
            "error": { "code": -32600, "message": "unreadable" },
        }))
        .into_response(),
        _ => StatusCode::ACCEPTED.into_response(),
    });
    let url = serve(Arc::clone(&mock)).await;
    let client = ClientBuilder::new("strict", "1.0.0")
        .with_connect_mode(ConnectMode::Legacy)
        .connect(HttpClientTransport::new(&url).unwrap())
        .await
        .expect("handshake");

    let wrong = tokio::time::timeout(Duration::from_secs(5), client.list_tools(None))
        .await
        .expect("answered, not left waiting");
    assert!(wrong.is_err());
    let idless = tokio::time::timeout(Duration::from_secs(5), client.call_tool("t", Map::new()))
        .await
        .expect("answered, not left waiting");
    let err = idless.expect_err("the id-less error is this request's");
    assert_eq!(err.as_rpc().map(|e| e.code), Some(-32600), "{err:?}");
}

/// A standalone stream answered `200` with no event stream is backed off
/// like a refusal; it was re-requested at the server's `retry` forever.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_200_that_is_not_a_stream_is_backed_off() {
    let mock = Mock::new(|mock, rpc, msg, _| match rpc {
        "initialize" => initialize(mock, &msg["id"]),
        "<stream>" => Json(json!({})).into_response(),
        _ => StatusCode::ACCEPTED.into_response(),
    });
    let url = serve(Arc::clone(&mock)).await;
    let _client = ClientBuilder::new("patient", "1.0.0")
        .with_connect_mode(ConnectMode::Legacy)
        .connect(HttpClientTransport::new(&url).unwrap())
        .await
        .expect("handshake");
    tokio::time::sleep(Duration::from_millis(2500)).await;
    let gets = mock.seen("<stream>").len();
    assert!(
        gets <= 2,
        "backed off (1s, then 2s, …): {gets} GETs in 2.5s"
    );
}

/// An `initialize` answer at `version`, minting `session-N`, that offers
/// resource subscriptions and logging.
fn initialize_offering(mock: &Mock, id: &Value, version: &str) -> Response {
    let n = mock.handshakes.fetch_add(1, Ordering::SeqCst);
    let body = json!({ "jsonrpc": "2.0", "id": id, "result": {
        "protocolVersion": version,
        "capabilities": { "resources": { "subscribe": true }, "logging": {}, "tools": {} },
        "serverInfo": { "name": "mock", "version": "1.0" }
    }});
    ([("mcp-session-id", format!("session-{n}"))], Json(body)).into_response()
}

/// A recovered session gets its subscriptions and log level back: the server
/// forgot them with the old session, and `resources/updated` and log
/// messages stopped without a word.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_recovered_session_gets_its_subscriptions_and_log_level_back() {
    let mock = Mock::new(|mock, rpc, msg, session| {
        if let Some(sid) = session
            && mock.expired.lock().unwrap().iter().any(|e| e == sid)
        {
            return StatusCode::NOT_FOUND.into_response();
        }
        match rpc {
            "initialize" => initialize_offering(mock, &msg["id"], "2025-11-25"),
            "<stream>" => StatusCode::METHOD_NOT_ALLOWED.into_response(),
            "tools/list" => result(&msg["id"], json!({ "tools": [] })),
            "resources/subscribe" | "logging/setLevel" => result(&msg["id"], json!({})),
            _ => StatusCode::ACCEPTED.into_response(),
        }
    });
    let url = serve(Arc::clone(&mock)).await;
    let client = ClientBuilder::new("keeper", "1.0.0")
        .with_connect_mode(ConnectMode::Legacy)
        .connect(HttpClientTransport::new(&url).unwrap())
        .await
        .expect("handshake");
    client.subscribe_resource("mem://watched").await.unwrap();
    client.set_level(turbomcp::LogLevel::Warning).await.unwrap();

    mock.expired.lock().unwrap().push("session-0".into());
    client.list_tools(None).await.expect("recovered");

    let on_new = |rpc: &str| {
        mock.seen(rpc)
            .iter()
            .any(|s| s.session.as_deref() == Some("session-1"))
    };
    assert!(on_new("resources/subscribe"), "the subscription is renewed");
    assert!(on_new("logging/setLevel"), "the log level is set again");
}

/// A replacement session that negotiates another revision is refused: the
/// client was told the first answer and speaks its shape.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_replacement_session_on_another_revision_is_refused() {
    let mock = Mock::new(|mock, rpc, msg, session| {
        if let Some(sid) = session
            && mock.expired.lock().unwrap().iter().any(|e| e == sid)
        {
            return StatusCode::NOT_FOUND.into_response();
        }
        match rpc {
            "initialize" if mock.handshakes.load(Ordering::SeqCst) == 0 => {
                initialize_offering(mock, &msg["id"], "2025-11-25")
            }
            "initialize" => initialize_offering(mock, &msg["id"], "2025-06-18"),
            "<stream>" => StatusCode::METHOD_NOT_ALLOWED.into_response(),
            "tools/list" => result(&msg["id"], json!({ "tools": [] })),
            _ => StatusCode::ACCEPTED.into_response(),
        }
    });
    let url = serve(Arc::clone(&mock)).await;
    let client = ClientBuilder::new("strict", "1.0.0")
        .with_connect_mode(ConnectMode::Legacy)
        .connect(HttpClientTransport::new(&url).unwrap())
        .await
        .expect("handshake");
    mock.expired.lock().unwrap().push("session-0".into());
    let err = client.list_tools(None).await.expect_err("refused");
    assert!(err.to_string().contains("negotiated 2025-06-18"), "{err}");
}

/// Swaps its token once the server challenges the old one.
struct Rotating {
    token: Mutex<String>,
}

#[turbomcp::client::async_trait]
impl turbomcp::client::BearerSource for Rotating {
    async fn bearer(&self) -> Option<turbomcp::client::Zeroizing<String>> {
        Some(turbomcp::client::Zeroizing::new(
            self.token.lock().unwrap().clone(),
        ))
    }

    async fn on_challenge(
        &self,
        status: u16,
        _header: Option<&str>,
        _rejected: Option<&str>,
    ) -> Result<bool, String> {
        if status == 401 {
            *self.token.lock().unwrap() = "fresh".into();
            return Ok(true);
        }
        Ok(false)
    }
}

/// The standalone stream answers a `401` through the bearer source, as a
/// POST does: it used to retry the refused token forever.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_standalone_stream_answers_an_auth_challenge() {
    let mock = Mock::new(|mock, rpc, msg, _| match rpc {
        "initialize" => initialize(mock, &msg["id"]),
        "<stream>" => {
            let fresh = mock
                .seen("<stream>")
                .last()
                .is_some_and(|s| s.auth.as_deref() == Some("Bearer fresh"));
            if fresh {
                idle_stream()
            } else {
                StatusCode::UNAUTHORIZED.into_response()
            }
        }
        _ => StatusCode::ACCEPTED.into_response(),
    });
    let url = serve(Arc::clone(&mock)).await;
    let source = Arc::new(Rotating {
        token: Mutex::new("stale".into()),
    });
    let _client = ClientBuilder::new("rotating", "1.0.0")
        .with_connect_mode(ConnectMode::Legacy)
        .connect(
            HttpClientTransport::new(&url)
                .unwrap()
                .with_bearer_source(source),
        )
        .await
        .expect("handshake");
    tokio::time::sleep(Duration::from_millis(300)).await;
    let gets = mock.seen("<stream>");
    assert!(
        gets.iter()
            .any(|s| s.auth.as_deref() == Some("Bearer fresh")),
        "the stream came back with the refreshed token: {gets:?}"
    );
}
