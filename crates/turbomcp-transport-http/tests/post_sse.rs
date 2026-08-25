//! The per-request lazy SSE upgrade on `POST`: a request whose handler emits
//! nothing answers plain JSON exactly as before; one that pushes mid-flight
//! messages answers a request-scoped `text/event-stream`; and dropping that
//! stream drops the in-flight call (HTTP's cancellation signal).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use turbomcp_core::{Implementation, JsonRpcMessage, JsonRpcNotification, McpResult};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, ListToolsContext, McpServerCore, MethodRouter, VersionDispatcher, WithTools,
};
use turbomcp_service::{ProtocolError, outbound};
use turbomcp_transport_http::{HttpConfig, router};

fn call_request(id: i64) -> Request<Body> {
    let body = json!({
        "jsonrpc": "2.0", "id": id, "method": "tools/call",
        "params": {
            "name": "echo",
            "arguments": {},
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientCapabilities": {},
            },
        }
    });
    Request::builder()
        .method("POST")
        .header("accept", "application/json, text/event-stream")
        .uri("/mcp")
        .header(header::CONTENT_TYPE, "application/json")
        // The draft envelope requires the mirrored request-metadata headers.
        .header("MCP-Protocol-Version", "2026-07-28")
        .header("Mcp-Method", "tools/call")
        .header("Mcp-Name", "echo")
        .body(Body::from(body.to_string()))
        .unwrap()
}

// ---- no mid-flight pushes → plain JSON, exactly as before ----------------------

#[derive(Clone)]
struct Quiet;

impl McpServerCore for Quiet {
    fn server_info(&self) -> Implementation {
        Implementation::new("quiet", "0.1.0")
    }
}

impl WithTools for Quiet {
    async fn list_tools(
        &self,
        _ctx: &ListToolsContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        Ok(neutral::ListToolsResult::new(vec![]))
    }

    async fn call_tool(
        &self,
        _ctx: &CallToolContext,
        _params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        Ok(neutral::CallToolResult::text("ok"))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn quiet_request_answers_plain_json() {
    let app = router(
        VersionDispatcher::new(Quiet, MethodRouter::new().with_tools()),
        HttpConfig::new(),
    );
    let resp = app.oneshot(call_request(1)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        resp.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("application/json"),
        "no mid-flight messages → no SSE upgrade"
    );
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["result"]["content"][0]["text"], "ok");
}

// ---- progress upgrades the POST to a request-scoped stream ---------------------

#[derive(Clone)]
struct Slow;

impl McpServerCore for Slow {
    fn server_info(&self) -> Implementation {
        Implementation::new("slow", "0.1.0")
    }
}

impl WithTools for Slow {
    async fn list_tools(
        &self,
        _ctx: &ListToolsContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        Ok(neutral::ListToolsResult::new(vec![]))
    }

    async fn call_tool(
        &self,
        ctx: &CallToolContext,
        _params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        ctx.progress.report(1.0, Some(2.0), Some("halfway")).await;
        ctx.progress.report(2.0, Some(2.0), None).await;
        Ok(neutral::CallToolResult::text("done"))
    }
}

fn call_with_token(id: i64) -> Request<Body> {
    let body = json!({
        "jsonrpc": "2.0", "id": id, "method": "tools/call",
        "params": {
            "name": "slow",
            "arguments": {},
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientCapabilities": {},
                "progressToken": "tok-9",
            },
        }
    });
    Request::builder()
        .method("POST")
        .header("accept", "application/json, text/event-stream")
        .uri("/mcp")
        .header(header::CONTENT_TYPE, "application/json")
        // The draft envelope requires the mirrored request-metadata headers.
        .header("MCP-Protocol-Version", "2026-07-28")
        .header("Mcp-Method", "tools/call")
        .header("Mcp-Name", "slow")
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// Collect every `data:` event until the stream ends.
async fn all_sse_events(body: Body) -> Vec<Value> {
    let bytes = tokio::time::timeout(Duration::from_secs(5), body.collect())
        .await
        .expect("the stream should terminate")
        .unwrap()
        .to_bytes();
    String::from_utf8_lossy(&bytes)
        .lines()
        .filter_map(|l| l.strip_prefix("data: ").or_else(|| l.strip_prefix("data:")))
        .map(|d| serde_json::from_str(d).expect("SSE data is JSON"))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn progress_notifications_ride_the_posts_own_stream() {
    let app = router(
        VersionDispatcher::new(Slow, MethodRouter::new().with_tools()),
        HttpConfig::new(),
    );
    let resp = app.oneshot(call_with_token(5)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        resp.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/event-stream"),
        "progress upgrades the response to SSE"
    );

    let events = all_sse_events(resp.into_body()).await;
    assert_eq!(events.len(), 3, "two progress events + the final response");
    assert_eq!(events[0]["method"], "notifications/progress");
    assert_eq!(events[0]["params"]["progressToken"], "tok-9");
    assert_eq!(events[0]["params"]["progress"], 1.0);
    assert_eq!(events[1]["params"]["progress"], 2.0);
    assert_eq!(events[2]["id"], 5);
    assert_eq!(events[2]["result"]["content"][0]["text"], "done");
}

// ---- the 2025-11-25 primer event ------------------------------------------------

fn legacy_initialize() -> Request<Body> {
    let body = json!({
        "jsonrpc": "2.0", "id": 0, "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "primer-test", "version": "0.0.0" },
        }
    });
    Request::builder()
        .method("POST")
        .header("accept", "application/json, text/event-stream")
        .uri("/mcp")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn legacy_call_with_token(id: i64, session: &str) -> Request<Body> {
    let body = json!({
        "jsonrpc": "2.0", "id": id, "method": "tools/call",
        "params": {
            "name": "slow",
            "arguments": {},
            "_meta": { "progressToken": "tok-legacy" },
        }
    });
    Request::builder()
        .method("POST")
        .header("accept", "application/json, text/event-stream")
        .uri("/mcp")
        .header(header::CONTENT_TYPE, "application/json")
        .header("mcp-session-id", session)
        .header("MCP-Protocol-Version", "2025-11-25")
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// `2025-11-25` SHOULD-requires priming a POST-response SSE stream so the
/// client always holds a `Last-Event-ID` (transports spec §Sending Messages).
/// The primer is the stream's first event: an `id:` field (globally unique,
/// stream-identifying, per §Resumability) with no message payload.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn legacy_streams_are_primed_with_an_event_id() {
    let app = router(
        VersionDispatcher::new(Slow, MethodRouter::new().with_tools()),
        HttpConfig::new(),
    );

    let init = app.clone().oneshot(legacy_initialize()).await.unwrap();
    assert_eq!(init.status(), StatusCode::OK);
    let session = init.headers()["mcp-session-id"]
        .to_str()
        .unwrap()
        .to_owned();

    let resp = app
        .oneshot(legacy_call_with_token(7, &session))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        resp.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/event-stream"),
        "progress upgrades the response to SSE"
    );

    let bytes = tokio::time::timeout(Duration::from_secs(5), resp.into_body().collect())
        .await
        .expect("the stream should terminate")
        .unwrap()
        .to_bytes();
    let text = String::from_utf8_lossy(&bytes);

    // The primer is its own first event block: an event ID, no payload.
    let first_event = text.split("\n\n").next().expect("at least one event");
    let id_line = first_event
        .lines()
        .find_map(|l| l.strip_prefix("id: ").or_else(|| l.strip_prefix("id:")))
        .expect("the first event carries the primer's event ID");
    assert!(
        id_line.starts_with("http-post-") && id_line.ends_with("-0"),
        "the id encodes the originating stream and a cursor: {id_line}"
    );
    assert!(
        first_event
            .lines()
            .all(|l| l.strip_prefix("data:").is_none_or(|d| d.trim().is_empty())),
        "the primer carries no message payload: {first_event:?}"
    );

    // The JSON-RPC frames follow, untouched.
    let frames: Vec<Value> = text
        .lines()
        .filter_map(|l| l.strip_prefix("data: ").or_else(|| l.strip_prefix("data:")))
        .filter(|d| !d.is_empty())
        .map(|d| serde_json::from_str(d).expect("SSE data is JSON"))
        .collect();
    assert_eq!(frames.len(), 3, "two progress events + the final response");
    assert_eq!(frames[0]["method"], "notifications/progress");
    assert_eq!(frames[2]["id"], 7);
    assert_eq!(frames[2]["result"]["content"][0]["text"], "done");
}

/// The draft dropped SSE resumability ("Resumable SSE streams via
/// `Last-Event-ID` are not supported"), so draft streams carry no primer and
/// no event IDs at all.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn draft_streams_are_not_primed() {
    let app = router(
        VersionDispatcher::new(Slow, MethodRouter::new().with_tools()),
        HttpConfig::new(),
    );
    let resp = app.oneshot(call_with_token(6)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = tokio::time::timeout(Duration::from_secs(5), resp.into_body().collect())
        .await
        .expect("the stream should terminate")
        .unwrap()
        .to_bytes();
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.lines().all(|l| !l.starts_with("id:")),
        "no event IDs on a draft stream: {text:?}"
    );
}

// ---- a pushing handler upgrades; dropping the stream cancels it ----------------

/// Sets the flag when the in-flight call future is dropped before completing.
struct DropFlag(Arc<AtomicBool>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// A raw service (below the dispatcher) that pushes one notification into its
/// request's channel and then parks forever — the only way it ends is by
/// being dropped.
#[derive(Clone)]
struct Parked {
    dropped: Arc<AtomicBool>,
}

impl tower::Service<JsonRpcMessage> for Parked {
    type Response = Option<JsonRpcMessage>;
    type Error = ProtocolError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, msg: JsonRpcMessage) -> Self::Future {
        let dropped = self.dropped.clone();
        Box::pin(async move {
            let JsonRpcMessage::Request(req) = &msg else {
                return Ok(None);
            };
            let conn = req
                .params
                .as_ref()
                .and_then(|p| p.get("_meta"))
                .and_then(|m| m.get("io.turbomcp.internal/connectionId"))
                .and_then(Value::as_str)
                .expect("the endpoint injects a per-request connection id")
                .to_owned();
            let writer = outbound::writer(&conn).expect("per-request writer registered");
            writer
                .send(JsonRpcNotification::new("notifications/test/started", None).into())
                .await
                .expect("channel open");

            let _flag = DropFlag(dropped);
            std::future::pending::<()>().await;
            unreachable!("pending() never completes")
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_the_upgraded_stream_drops_the_call() {
    let dropped = Arc::new(AtomicBool::new(false));
    let app = router(
        Parked {
            dropped: dropped.clone(),
        },
        HttpConfig::new(),
    );

    let resp = app.oneshot(call_request(1)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        resp.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/event-stream"),
        "the pushed notification upgrades the response to SSE"
    );

    // The pushed notification is the stream's first event.
    let mut body = resp.into_body();
    let frame = tokio::time::timeout(Duration::from_secs(5), body.frame())
        .await
        .expect("an SSE frame should arrive")
        .expect("stream open")
        .expect("frame ok");
    let text = String::from_utf8_lossy(frame.data_ref().expect("data frame")).into_owned();
    assert!(text.contains("notifications/test/started"));
    assert!(!dropped.load(Ordering::SeqCst), "call is still in flight");

    // Client disconnect: dropping the body drops the stream state, which owns
    // the in-flight call — HTTP's cancellation signal.
    drop(body);
    for _ in 0..50 {
        if dropped.load(Ordering::SeqCst) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("dropping the SSE stream should drop the in-flight call");
}
