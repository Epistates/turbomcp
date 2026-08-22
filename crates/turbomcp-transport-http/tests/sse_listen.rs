//! `subscriptions/listen` over HTTP: the POST answers a long-lived SSE stream
//! — acknowledged event first, then opted-in notifications, keep-alive
//! comments between (the "survives a buffering proxy" exit piece), JSON error
//! bodies for rejected listens, and pruning when a stream is dropped.

use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use turbomcp_core::{Implementation, McpResult};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, ListToolsContext, McpServerCore, MethodRouter, ServerNotifier,
    VersionDispatcher, WithTools,
};
use turbomcp_transport_http::{HttpConfig, router};

#[derive(Clone)]
struct Watched;

impl McpServerCore for Watched {
    fn server_info(&self) -> Implementation {
        Implementation::new("watched", "0.1.0")
    }
}

impl WithTools for Watched {
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

/// One app + the notifier that publishes into it.
fn app(config: HttpConfig) -> (axum::Router, ServerNotifier) {
    let dispatcher = VersionDispatcher::new(Watched, MethodRouter::new().with_tools());
    let notifier = dispatcher.notifier();
    (router(dispatcher, config), notifier)
}

fn listen_request(id: i64, notifications: Value) -> Request<Body> {
    let body = json!({
        "jsonrpc": "2.0", "id": id, "method": "subscriptions/listen",
        "params": {
            "_meta": { "io.modelcontextprotocol/protocolVersion": "2026-07-28" },
            "notifications": notifications,
        }
    });
    Request::builder()
        .method("POST")
        .header("accept", "application/json, text/event-stream")
        .uri("/mcp")
        .header(header::CONTENT_TYPE, "application/json")
        // The draft envelope requires the mirrored request-metadata headers.
        .header("MCP-Protocol-Version", "2026-07-28")
        .header("Mcp-Method", "subscriptions/listen")
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// Pull raw chunks off the SSE body until one complete event (terminated by a
/// blank line) is buffered; returns it (comments included).
async fn next_sse_chunk(body: &mut Body, buffer: &mut String) -> String {
    loop {
        if let Some(end) = buffer.find("\n\n") {
            let event: String = buffer.drain(..end + 2).collect();
            return event;
        }
        let frame = tokio::time::timeout(Duration::from_secs(5), body.frame())
            .await
            .expect("an SSE frame should arrive")
            .expect("stream open")
            .expect("frame ok");
        if let Some(data) = frame.data_ref() {
            buffer.push_str(&String::from_utf8_lossy(data));
        }
    }
}

/// The `data:` payload of an SSE event, parsed as JSON.
fn event_json(event: &str) -> Value {
    let data: String = event
        .lines()
        .filter_map(|l| l.strip_prefix("data: ").or_else(|| l.strip_prefix("data:")))
        .collect();
    serde_json::from_str(&data).expect("SSE data is one JSON-RPC message")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn listen_answers_sse_with_ack_first_then_events() {
    let (app, notifier) = app(HttpConfig::new());
    let resp = app
        .oneshot(listen_request(7, json!({ "toolsListChanged": true })))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        resp.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/event-stream"),
        "listen answers an SSE stream"
    );
    assert_eq!(
        resp.headers()["x-accel-buffering"],
        "no",
        "proxies are told not to buffer"
    );

    let mut body = resp.into_body();
    let mut buffer = String::new();

    // First message on the stream: the acknowledgment.
    let ack = event_json(&next_sse_chunk(&mut body, &mut buffer).await);
    assert_eq!(ack["method"], "notifications/subscriptions/acknowledged");
    // Verbatim JSON-RPC id: a numeric listen id rides as a number.
    assert_eq!(
        ack["params"]["_meta"]["io.modelcontextprotocol/subscriptionId"],
        serde_json::json!(7)
    );
    assert_eq!(ack["params"]["notifications"]["toolsListChanged"], true);

    // A published change arrives as the next event.
    notifier.tools_list_changed();
    let event = event_json(&next_sse_chunk(&mut body, &mut buffer).await);
    assert_eq!(event["method"], "notifications/tools/list_changed");
    assert_eq!(
        event["params"]["_meta"]["io.modelcontextprotocol/subscriptionId"],
        serde_json::json!(7)
    );
}

/// Graceful teardown (frozen `2026-07-28`): the server answers the listen
/// request with a `SubscriptionsListenResult` envelope and then the stream
/// ends. The RC had no such envelope — closing was the only signal — and the
/// freeze restored it for server-initiated teardown.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_closes_the_listen_stream_with_a_result_envelope() {
    let dispatcher = VersionDispatcher::new(Watched, MethodRouter::new().with_tools());
    let closer = dispatcher.clone();
    let config = HttpConfig::new();
    let shutdown = config.shutdown_token();
    let app = router(dispatcher, config);

    let resp = app
        .oneshot(listen_request(7, json!({ "toolsListChanged": true })))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let mut body = resp.into_body();
    let mut buffer = String::new();

    let ack = event_json(&next_sse_chunk(&mut body, &mut buffer).await);
    assert_eq!(ack["method"], "notifications/subscriptions/acknowledged");

    closer.close_subscriptions().await;

    // The closing envelope arrives on the listen stream, addressed to the
    // listen request's own id.
    let closed = event_json(&next_sse_chunk(&mut body, &mut buffer).await);
    assert_eq!(closed["id"], 7);
    assert_eq!(closed["result"]["resultType"], "complete");
    assert_eq!(
        closed["result"]["_meta"]["io.modelcontextprotocol/subscriptionId"],
        7
    );

    // Then the transport tears the stream down off the same token.
    shutdown.cancel();
    let end = tokio::time::timeout(Duration::from_secs(5), body.frame()).await;
    assert!(
        matches!(end, Ok(None)),
        "stream should end after the closing envelope, got {end:?}"
    );
}

/// The proxy-survival exit piece: with no events flowing, keep-alive comments
/// keep the stream non-idle.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn idle_stream_emits_keepalive_comments() {
    let (app, _notifier) = app(HttpConfig::new().sse_keepalive(Duration::from_millis(50)));
    let resp = app
        .oneshot(listen_request(1, json!({ "toolsListChanged": true })))
        .await
        .unwrap();
    let mut body = resp.into_body();
    let mut buffer = String::new();

    // Ack first, then — with zero events published — comment frames.
    let ack = next_sse_chunk(&mut body, &mut buffer).await;
    assert!(ack.contains("subscriptions/acknowledged"));
    let comment = next_sse_chunk(&mut body, &mut buffer).await;
    assert!(
        comment.starts_with(':') && comment.contains("keep-alive"),
        "expected a keep-alive comment, got: {comment:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_listen_filter_answers_json_error() {
    let (app, _notifier) = app(HttpConfig::new());
    let body = json!({
        "jsonrpc": "2.0", "id": 1, "method": "subscriptions/listen",
        "params": { "_meta": { "io.modelcontextprotocol/protocolVersion": "2026-07-28" } }
    });
    let req = Request::builder()
        .method("POST")
        .header("accept", "application/json, text/event-stream")
        .uri("/mcp")
        .header(header::CONTENT_TYPE, "application/json")
        .header("MCP-Protocol-Version", "2026-07-28")
        .header("Mcp-Method", "subscriptions/listen")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        resp.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("application/json"),
        "rejected listens answer plain JSON, not a stream"
    );
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["error"]["code"], -32602);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropped_stream_is_pruned_and_later_streams_keep_working() {
    let (app, notifier) = app(HttpConfig::new());

    // Open a stream and immediately drop it (client disconnect).
    let dropped = app
        .clone()
        .oneshot(listen_request(1, json!({ "toolsListChanged": true })))
        .await
        .unwrap();
    drop(dropped);

    // A second subscription still gets exactly its own events.
    let resp = app
        .oneshot(listen_request(2, json!({ "toolsListChanged": true })))
        .await
        .unwrap();
    let mut body = resp.into_body();
    let mut buffer = String::new();
    let _ack = next_sse_chunk(&mut body, &mut buffer).await;

    notifier.tools_list_changed();
    let event = event_json(&next_sse_chunk(&mut body, &mut buffer).await);
    assert_eq!(event["method"], "notifications/tools/list_changed");
    assert_eq!(
        event["params"]["_meta"]["io.modelcontextprotocol/subscriptionId"],
        serde_json::json!(2)
    );
}

// ---- legacy (2025-11-25) GET stream --------------------------------------------

#[derive(Clone)]
struct Resourceful;

impl McpServerCore for Resourceful {
    fn server_info(&self) -> Implementation {
        Implementation::new("resourceful", "0.1.0")
    }
}

impl turbomcp_server::WithResources for Resourceful {
    async fn list_resources(
        &self,
        _ctx: &turbomcp_server::ListResourcesContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListResourcesResult> {
        Ok(neutral::ListResourcesResult::new(vec![]))
    }

    async fn read_resource(
        &self,
        _ctx: &turbomcp_server::ReadResourceContext,
        params: neutral::ReadResourceParams,
    ) -> McpResult<neutral::ReadResourceResult> {
        Ok(neutral::ReadResourceResult::text(params.uri, "x"))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn legacy_get_stream_delivers_subscribed_resource_updates() {
    let dispatcher = VersionDispatcher::new(Resourceful, MethodRouter::new().with_resources());
    let notifier = dispatcher.notifier();
    let app = router(dispatcher, HttpConfig::new());

    // 1. initialize mints the session.
    let init = json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "legacy-http", "version": "1" },
        }
    });
    let req = Request::builder()
        .method("POST")
        .header("accept", "application/json, text/event-stream")
        .uri("/mcp")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(init.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let sid = resp.headers()["mcp-session-id"]
        .to_str()
        .unwrap()
        .to_owned();

    // 2. resources/subscribe over POST with the session header.
    let sub = json!({
        "jsonrpc": "2.0", "id": 2, "method": "resources/subscribe",
        "params": { "uri": "file://a" }
    });
    let req = Request::builder()
        .method("POST")
        .header("accept", "application/json, text/event-stream")
        .uri("/mcp")
        .header(header::CONTENT_TYPE, "application/json")
        .header("mcp-session-id", &sid)
        .body(Body::from(sub.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 3. GET with the session header opens the session's SSE stream.
    let req = Request::builder()
        .method("GET")
        .uri("/mcp")
        .header(header::ACCEPT, "text/event-stream")
        .header("mcp-session-id", &sid)
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        resp.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/event-stream")
    );
    let mut body = resp.into_body();
    let mut buffer = String::new();

    // 4. A publish lands on the GET stream in legacy wire shape (no _meta).
    notifier.resource_updated("file://a").await;
    let event = event_json(&next_sse_chunk(&mut body, &mut buffer).await);
    assert_eq!(event["method"], "notifications/resources/updated");
    assert_eq!(event["params"]["uri"], "file://a");
    assert!(event["params"].get("_meta").is_none());
}

/// Two concurrent legacy sessions are isolated: each GET stream sees only its
/// own session's subscribed updates, with no cross-talk.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_legacy_sessions_get_only_their_own_events() {
    let dispatcher = VersionDispatcher::new(Resourceful, MethodRouter::new().with_resources());
    let notifier = dispatcher.notifier();
    let app = router(dispatcher, HttpConfig::new());

    let initialize = |name: &str| {
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": name, "version": "1" },
            }
        })
    };
    let mut sids = Vec::new();
    for name in ["client-a", "client-b"] {
        let req = Request::builder()
            .method("POST")
            .header("accept", "application/json, text/event-stream")
            .uri("/mcp")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(initialize(name).to_string()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        sids.push(
            resp.headers()["mcp-session-id"]
                .to_str()
                .unwrap()
                .to_owned(),
        );
    }
    assert_ne!(sids[0], sids[1], "each initialize mints its own session");

    // A subscribes to file://a, B to file://b.
    for (sid, uri) in [(&sids[0], "file://a"), (&sids[1], "file://b")] {
        let sub = json!({
            "jsonrpc": "2.0", "id": 2, "method": "resources/subscribe",
            "params": { "uri": uri }
        });
        let req = Request::builder()
            .method("POST")
            .header("accept", "application/json, text/event-stream")
            .uri("/mcp")
            .header(header::CONTENT_TYPE, "application/json")
            .header("mcp-session-id", sid)
            .body(Body::from(sub.to_string()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // Open both GET streams.
    let mut streams = Vec::new();
    for sid in &sids {
        let req = Request::builder()
            .method("GET")
            .uri("/mcp")
            .header(header::ACCEPT, "text/event-stream")
            .header("mcp-session-id", sid)
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        streams.push((resp.into_body(), String::new()));
    }

    // Publish both updates; each stream's FIRST event must be its own uri —
    // if file://a had leaked onto B's stream it would arrive there first.
    notifier.resource_updated("file://a").await;
    notifier.resource_updated("file://b").await;

    let (body_a, buf_a) = &mut streams[0];
    let event_a = event_json(&next_sse_chunk(body_a, buf_a).await);
    assert_eq!(event_a["method"], "notifications/resources/updated");
    assert_eq!(event_a["params"]["uri"], "file://a");

    let (body_b, buf_b) = &mut streams[1];
    let event_b = event_json(&next_sse_chunk(body_b, buf_b).await);
    assert_eq!(event_b["method"], "notifications/resources/updated");
    assert_eq!(event_b["params"]["uri"], "file://b");
}

#[tokio::test]
async fn get_without_session_header_stays_405() {
    let (app, _notifier) = app(HttpConfig::new());
    let req = Request::builder()
        .method("GET")
        .uri("/mcp")
        .header(header::ACCEPT, "text/event-stream")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}
