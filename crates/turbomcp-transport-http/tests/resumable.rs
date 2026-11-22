//! Resumable response streams on the session wires: with an event store, a
//! stream is primed and its events carry ids, and `GET` with
//! `Last-Event-ID` catches a client up after a lost connection, then follows
//! the call live to its response.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use turbomcp_core::{Implementation, JsonRpcMessage, McpResult};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, ListToolsContext, McpServerCore, MethodRouter, VersionDispatcher, WithTools,
};
use turbomcp_transport_http::{
    EventFuture, EventStore, EventStoreError, HttpConfig, InMemoryEventStore, StoredEvent, service,
};

/// `steps` reports progress twice, 150 ms apart, then answers; `slow` sends
/// nothing for six seconds, longer than the endpoint waits before it starts
/// a stream, then answers.
#[derive(Clone)]
struct Stepped;

impl McpServerCore for Stepped {
    fn server_info(&self) -> Implementation {
        Implementation::new("stepped", "0.1.0")
    }
}

impl WithTools for Stepped {
    async fn list_tools(
        &self,
        _ctx: &ListToolsContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        Ok(neutral::ListToolsResult::new(vec![
            neutral::Tool::new("steps", json!({"type":"object"})),
            neutral::Tool::new("slow", json!({"type":"object"})),
        ]))
    }

    async fn call_tool(
        &self,
        ctx: &CallToolContext,
        params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        if params.name == "slow" {
            tokio::time::sleep(Duration::from_secs(6)).await;
            return Ok(neutral::CallToolResult::text("late"));
        }
        ctx.progress.report(1.0, Some(2.0), None).await;
        tokio::time::sleep(Duration::from_millis(150)).await;
        ctx.progress.report(2.0, Some(2.0), None).await;
        tokio::time::sleep(Duration::from_millis(150)).await;
        Ok(neutral::CallToolResult::text("done"))
    }
}

fn app() -> turbomcp_transport_http::HttpService {
    let dispatcher = VersionDispatcher::new(Stepped, MethodRouter::new().with_tools());
    let terminator = dispatcher.session_terminator();
    service(
        dispatcher,
        HttpConfig::new()
            .with_session_terminator(Arc::new(terminator))
            .with_event_store(Arc::new(InMemoryEventStore::new())),
    )
}

async fn open_session(app: &turbomcp_transport_http::HttpService) -> String {
    let init = json!({
        "jsonrpc": "2.0", "id": 0, "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "resume", "version": "1" },
        }
    });
    let req = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("accept", "application/json, text/event-stream")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(init.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    resp.headers()["mcp-session-id"]
        .to_str()
        .unwrap()
        .to_owned()
}

fn call(session: &str) -> Request<Body> {
    call_tool(session, "steps")
}

fn call_tool(session: &str, tool: &str) -> Request<Body> {
    let body = json!({
        "jsonrpc": "2.0", "id": 9, "method": "tools/call",
        "params": { "name": tool, "arguments": {}, "_meta": { "progressToken": "p" } }
    });
    Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("accept", "application/json, text/event-stream")
        .header(header::CONTENT_TYPE, "application/json")
        .header("mcp-session-id", session)
        .header("MCP-Protocol-Version", "2025-11-25")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn resume(session: &str, last: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri("/mcp")
        .header(header::ACCEPT, "text/event-stream")
        .header("mcp-session-id", session)
        .header("MCP-Protocol-Version", "2025-11-25")
        .header("last-event-id", last)
        .body(Body::empty())
        .unwrap()
}

/// One SSE event: its id and its data (comments skipped).
#[derive(Debug)]
struct Sse {
    id: Option<String>,
    data: String,
}

async fn next_event(
    body: &mut turbomcp_transport_http::HttpBody,
    buffer: &mut String,
) -> Option<Sse> {
    loop {
        while let Some(end) = buffer.find("\n\n") {
            let raw: String = buffer.drain(..end + 2).collect();
            let mut id = None;
            let mut data = String::new();
            let mut is_event = false;
            for line in raw.lines() {
                if let Some(v) = line.strip_prefix("id:") {
                    id = Some(v.trim().to_owned());
                    is_event = true;
                } else if let Some(v) = line.strip_prefix("data:") {
                    data.push_str(v.trim_start());
                    is_event = true;
                }
            }
            if is_event {
                return Some(Sse { id, data });
            }
        }
        let frame = tokio::time::timeout(Duration::from_secs(5), body.frame())
            .await
            .expect("an event should arrive")?
            .expect("frame ok");
        if let Some(bytes) = frame.data_ref() {
            buffer.push_str(&String::from_utf8_lossy(bytes));
        }
    }
}

fn json_of(event: &Sse) -> Value {
    serde_json::from_str(&event.data).expect("JSON data")
}

/// A client that loses the connection after the first progress event picks
/// the stream back up where it left off: the second progress event and the
/// response, each once, numbered on from where it was.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_interrupted_stream_resumes_where_it_left_off() {
    let app = app();
    let sid = open_session(&app).await;
    let resp = app.clone().oneshot(call(&sid)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let mut body = resp.into_body();
    let mut buffer = String::new();

    // "The server SHOULD immediately send an SSE event consisting of an event
    // ID and an empty data field."
    let prime = next_event(&mut body, &mut buffer).await.unwrap();
    assert!(prime.data.is_empty());
    let stream = prime
        .id
        .as_deref()
        .unwrap()
        .rsplit_once(':')
        .unwrap()
        .0
        .to_owned();

    let first = next_event(&mut body, &mut buffer).await.unwrap();
    assert_eq!(json_of(&first)["params"]["progress"], 1.0);
    let last_seen = first.id.clone().unwrap();
    drop(body); // the connection drops

    let resumed = app.clone().oneshot(resume(&sid, &last_seen)).await.unwrap();
    assert_eq!(resumed.status(), StatusCode::OK);
    let mut body = resumed.into_body();
    let mut buffer = String::new();
    let second = next_event(&mut body, &mut buffer).await.unwrap();
    assert_eq!(json_of(&second)["params"]["progress"], 2.0);
    let done = next_event(&mut body, &mut buffer).await.unwrap();
    assert_eq!(json_of(&done)["id"], 9);
    assert_eq!(json_of(&done)["result"]["content"][0]["text"], "done");
    for event in [&second, &done] {
        assert!(
            event
                .id
                .as_deref()
                .unwrap()
                .starts_with(&format!("{stream}:"))
        );
    }
    assert!(
        next_event(&mut body, &mut buffer).await.is_none(),
        "the response ends the stream"
    );
}

/// A client that comes back after the call finished still gets its response.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_finished_stream_replays_through_its_response() {
    let app = app();
    let sid = open_session(&app).await;
    let resp = app.clone().oneshot(call(&sid)).await.unwrap();
    let mut body = resp.into_body();
    let mut buffer = String::new();
    let prime = next_event(&mut body, &mut buffer).await.unwrap();
    drop(body);
    tokio::time::sleep(Duration::from_millis(500)).await;

    let resumed = app
        .clone()
        .oneshot(resume(&sid, prime.id.as_deref().unwrap()))
        .await
        .unwrap();
    let mut body = resumed.into_body();
    let mut buffer = String::new();
    let mut methods = Vec::new();
    while let Some(event) = next_event(&mut body, &mut buffer).await {
        let v = json_of(&event);
        methods.push(v["method"].as_str().unwrap_or("response").to_owned());
    }
    assert_eq!(
        methods,
        [
            "notifications/progress",
            "notifications/progress",
            "response"
        ]
    );
}

/// A call that sends nothing before its response, but outlasts the wait
/// before the endpoint starts a stream: the stream opens with only its
/// priming event, and a client that reconnects with that id after the call
/// finished still gets the response. The events reach the store from the
/// moment the stream opens, not only from its first event.
#[tokio::test(start_paused = true)]
async fn a_stream_opened_on_a_silent_call_replays_its_response() {
    let app = app();
    let sid = open_session(&app).await;
    let resp = app.clone().oneshot(call_tool(&sid, "slow")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let mut body = resp.into_body();
    let mut buffer = String::new();
    let prime = next_event(&mut body, &mut buffer).await.unwrap();
    assert!(prime.data.is_empty());
    drop(body); // the connection drops before the call answers

    tokio::time::sleep(Duration::from_secs(5)).await;
    let resumed = app
        .clone()
        .oneshot(resume(&sid, prime.id.as_deref().unwrap()))
        .await
        .unwrap();
    assert_eq!(resumed.status(), StatusCode::OK);
    let mut body = resumed.into_body();
    let mut buffer = String::new();
    let done = next_event(&mut body, &mut buffer).await.unwrap();
    assert_eq!(json_of(&done)["id"], 9);
    assert_eq!(json_of(&done)["result"]["content"][0]["text"], "late");
    assert!(next_event(&mut body, &mut buffer).await.is_none());
}

/// Counts what reaches the store it wraps.
struct Counting {
    inner: InMemoryEventStore,
    appends: AtomicUsize,
}

impl EventStore for Counting {
    fn append<'a>(
        &'a self,
        session: &'a str,
        stream: &'a str,
        seq: u64,
        message: &'a JsonRpcMessage,
    ) -> EventFuture<'a, Result<(), EventStoreError>> {
        self.appends.fetch_add(1, Ordering::Relaxed);
        self.inner.append(session, stream, seq, message)
    }

    fn replay<'a>(
        &'a self,
        session: &'a str,
        stream: &'a str,
        after: u64,
        before: u64,
    ) -> EventFuture<'a, Result<Option<Vec<StoredEvent>>, EventStoreError>> {
        self.inner.replay(session, stream, after, before)
    }

    fn complete<'a>(
        &'a self,
        session: &'a str,
        stream: &'a str,
    ) -> EventFuture<'a, Result<(), EventStoreError>> {
        self.inner.complete(session, stream)
    }

    fn forget_session<'a>(
        &'a self,
        session: &'a str,
    ) -> EventFuture<'a, Result<(), EventStoreError>> {
        self.inner.forget_session(session)
    }
}

/// A call that answers with plain JSON gave its client no stream id, so
/// nothing about it is recorded; a call answered with a stream records every
/// event.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn only_a_resumable_stream_reaches_the_store() {
    let store = Arc::new(Counting {
        inner: InMemoryEventStore::new(),
        appends: AtomicUsize::new(0),
    });
    let dispatcher = VersionDispatcher::new(Stepped, MethodRouter::new().with_tools());
    let terminator = dispatcher.session_terminator();
    let app = service(
        dispatcher,
        HttpConfig::new()
            .with_session_terminator(Arc::new(terminator))
            .with_event_store(store.clone()),
    );
    let sid = open_session(&app).await;

    let list = json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/list" });
    let req = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("accept", "application/json, text/event-stream")
        .header(header::CONTENT_TYPE, "application/json")
        .header("mcp-session-id", &sid)
        .header("MCP-Protocol-Version", "2025-11-25")
        .body(Body::from(list.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let content_type = resp.headers()[header::CONTENT_TYPE].to_str().unwrap();
    assert!(
        content_type.starts_with("application/json"),
        "{content_type}"
    );
    let _ = resp.into_body().collect().await.unwrap();
    assert_eq!(store.appends.load(Ordering::Relaxed), 0);

    let resp = app.clone().oneshot(call(&sid)).await.unwrap();
    let mut body = resp.into_body();
    let mut buffer = String::new();
    while next_event(&mut body, &mut buffer).await.is_some() {}
    assert_eq!(
        store.appends.load(Ordering::Relaxed),
        3,
        "two progress notifications and the response"
    );
}

/// An id the endpoint can't replay from is refused: serving the live rest
/// instead would leave a silent gap. With `400`, not `404`, which a client
/// must take to mean its session is gone.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unknown_event_id_is_refused() {
    let app = app();
    let sid = open_session(&app).await;
    for id in ["nosuchstream:3", "garbage"] {
        let resp = app.clone().oneshot(resume(&sid, id)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "{id}");
    }
}

/// Another session's events are out of reach: the stream is looked up under
/// the session asking.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stream_cannot_be_resumed_from_another_session() {
    let app = app();
    let owner = open_session(&app).await;
    let other = open_session(&app).await;
    let resp = app.clone().oneshot(call(&owner)).await.unwrap();
    let mut body = resp.into_body();
    let mut buffer = String::new();
    let prime = next_event(&mut body, &mut buffer).await.unwrap();
    let stolen = app
        .clone()
        .oneshot(resume(&other, prime.id.as_deref().unwrap()))
        .await
        .unwrap();
    assert_eq!(stolen.status(), StatusCode::BAD_REQUEST);
}

/// With polling on, the server closes the connection soon after priming it
/// (with a `retry`), and the client polls the stream back: nothing the call
/// sent in between is lost.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_polled_stream_closes_its_connection_and_resumes() {
    let dispatcher = VersionDispatcher::new(Stepped, MethodRouter::new().with_tools());
    let terminator = dispatcher.session_terminator();
    let app = service(
        dispatcher,
        HttpConfig::new()
            .with_session_terminator(Arc::new(terminator))
            .with_event_store(Arc::new(InMemoryEventStore::new()))
            .with_sse_polling(turbomcp_transport_http::SsePolling::new(
                Duration::from_millis(50),
                Duration::from_millis(10),
            )),
    );
    let sid = open_session(&app).await;
    let resp = app.clone().oneshot(call(&sid)).await.unwrap();
    // The whole connection, which the server ends itself.
    let first = String::from_utf8(
        resp.into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    assert!(
        first.contains("retry: 10"),
        "the close says when to come back: {first}"
    );
    let mut seen = events_of(&first);
    let mut last = seen.last().unwrap().0.clone();

    // Poll until the response arrives; each connection is cut short again.
    while !seen.iter().any(|(_, v)| v.get("result").is_some()) {
        let resumed = app.clone().oneshot(resume(&sid, &last)).await.unwrap();
        assert_eq!(resumed.status(), StatusCode::OK);
        let text = String::from_utf8(
            resumed
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec(),
        )
        .unwrap();
        let more = events_of(&text);
        if let Some((id, _)) = more.last() {
            last = id.clone();
        }
        seen.extend(more);
    }
    let methods: Vec<&str> = seen
        .iter()
        .filter_map(|(_, v)| v.get("method").and_then(Value::as_str))
        .collect();
    assert_eq!(
        methods,
        ["notifications/progress", "notifications/progress"],
        "each event once, none lost"
    );
}

/// The `(id, data)` of each event in `text` that carries data.
fn events_of(text: &str) -> Vec<(String, Value)> {
    text.split("\n\n")
        .filter_map(|raw| {
            let id = raw
                .lines()
                .find_map(|l| l.strip_prefix("id:"))?
                .trim()
                .to_owned();
            let data = raw.lines().find_map(|l| l.strip_prefix("data:"))?.trim();
            (!data.is_empty()).then(|| (id, serde_json::from_str(data).expect("JSON data")))
        })
        .collect()
}

fn listen(session: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri("/mcp")
        .header(header::ACCEPT, "text/event-stream")
        .header("mcp-session-id", session)
        .header("MCP-Protocol-Version", "2025-11-25")
        .body(Body::empty())
        .unwrap()
}

/// The session's `GET` stream is resumable too: a message sent while no
/// connection carries it is recorded, and a `GET` with `Last-Event-ID`
/// delivers it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_get_stream_keeps_what_it_missed() {
    let dispatcher = VersionDispatcher::new(Stepped, MethodRouter::new().with_tools());
    let terminator = dispatcher.session_terminator();
    let notifier = dispatcher.notifier();
    let app = service(
        dispatcher,
        HttpConfig::new()
            .with_session_terminator(Arc::new(terminator))
            .with_event_store(Arc::new(InMemoryEventStore::new())),
    );
    let sid = open_session(&app).await;
    let resp = app.clone().oneshot(listen(&sid)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let mut body = resp.into_body();
    let mut buffer = String::new();
    let prime = next_event(&mut body, &mut buffer).await.unwrap();
    assert!(prime.data.is_empty());

    notifier.tools_list_changed();
    let first = next_event(&mut body, &mut buffer).await.unwrap();
    assert_eq!(
        json_of(&first)["method"],
        "notifications/tools/list_changed"
    );
    drop(body); // the connection drops; the stream does not

    tokio::time::sleep(Duration::from_millis(100)).await;
    notifier.tools_list_changed();
    tokio::time::sleep(Duration::from_millis(200)).await;

    let resumed = app
        .clone()
        .oneshot(resume(&sid, first.id.as_deref().unwrap()))
        .await
        .unwrap();
    assert_eq!(resumed.status(), StatusCode::OK);
    let mut body = resumed.into_body();
    let mut buffer = String::new();
    let missed = next_event(&mut body, &mut buffer).await.unwrap();
    assert_eq!(
        json_of(&missed)["method"],
        "notifications/tools/list_changed"
    );
    assert_ne!(missed.id, first.id);
}
