//! Resumable response streams on the session wires: with an event store, a
//! stream is primed and its events carry ids, and `GET` with
//! `Last-Event-ID` catches a client up after a lost connection, then follows
//! the call live to its response.

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use turbomcp_core::{Implementation, McpResult};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, ListToolsContext, McpServerCore, MethodRouter, VersionDispatcher, WithTools,
};
use turbomcp_transport_http::{HttpConfig, InMemoryEventStore, router};

/// Reports progress twice, 150 ms apart, then answers.
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
        Ok(neutral::ListToolsResult::new(vec![neutral::Tool::new(
            "steps",
            json!({"type":"object"}),
        )]))
    }

    async fn call_tool(
        &self,
        ctx: &CallToolContext,
        _params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        ctx.progress.report(1.0, Some(2.0), None).await;
        tokio::time::sleep(Duration::from_millis(150)).await;
        ctx.progress.report(2.0, Some(2.0), None).await;
        tokio::time::sleep(Duration::from_millis(150)).await;
        Ok(neutral::CallToolResult::text("done"))
    }
}

fn app() -> axum::Router {
    let dispatcher = VersionDispatcher::new(Stepped, MethodRouter::new().with_tools());
    let terminator = dispatcher.session_terminator();
    router(
        dispatcher,
        HttpConfig::new()
            .with_session_terminator(Arc::new(terminator))
            .with_event_store(Arc::new(InMemoryEventStore::new())),
    )
}

async fn open_session(app: &axum::Router) -> String {
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
    let body = json!({
        "jsonrpc": "2.0", "id": 9, "method": "tools/call",
        "params": { "name": "steps", "arguments": {}, "_meta": { "progressToken": "p" } }
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

async fn next_event(body: &mut Body, buffer: &mut String) -> Option<Sse> {
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

/// An id the endpoint can't replay from is refused: serving the live rest
/// instead would leave a silent gap.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unknown_event_id_is_refused() {
    let app = app();
    let sid = open_session(&app).await;
    for id in ["nosuchstream:3", "garbage"] {
        let resp = app.clone().oneshot(resume(&sid, id)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "{id}");
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
    assert_eq!(stolen.status(), StatusCode::NOT_FOUND);
}
