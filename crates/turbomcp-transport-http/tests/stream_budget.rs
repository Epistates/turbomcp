//! Long-lived streams have their own budget: they don't hold request slots,
//! one client can't take them all, a session has one `GET` stream at a time,
//! and a stream ends when its session or the server does.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use turbomcp_core::{Implementation, McpResult};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, ListToolsContext, McpServerCore, MethodRouter, VersionDispatcher, WithTools,
};
use turbomcp_service::CancellationToken;
use turbomcp_transport_http::{HttpConfig, router};

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

fn dispatcher() -> VersionDispatcher<Quiet> {
    VersionDispatcher::new(Quiet, MethodRouter::new().with_tools())
}

fn modern(id: i64, method: &str, params: Value) -> Request<Body> {
    let mut params = params;
    params["_meta"] = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
    });
    let body = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
    Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("accept", "application/json, text/event-stream")
        .header(header::CONTENT_TYPE, "application/json")
        .header("MCP-Protocol-Version", "2026-07-28")
        .header("Mcp-Method", method)
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn listen(id: i64) -> Request<Body> {
    modern(
        id,
        "subscriptions/listen",
        json!({ "notifications": { "toolsListChanged": true } }),
    )
}

/// `request` as if it came from `ip` over a real socket.
fn from(ip: [u8; 4], mut request: Request<Body>) -> Request<Body> {
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from((ip, 40_000))));
    request
}

async fn initialize(app: &axum::Router) -> String {
    let body = json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "legacy", "version": "1" },
        }
    });
    let req = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("accept", "application/json, text/event-stream")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    resp.headers()["mcp-session-id"]
        .to_str()
        .unwrap()
        .to_owned()
}

fn get(sid: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri("/mcp")
        .header(header::ACCEPT, "text/event-stream")
        .header("mcp-session-id", sid)
        .body(Body::empty())
        .unwrap()
}

/// Wait for the stream to end, skipping keep-alives and events.
async fn ends(mut body: Body) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(frame) = body.frame().await {
            frame.expect("frame ok");
        }
    })
    .await
    .expect("the stream should end");
}

/// Idle streams used to hold a request slot each, so a client that opened
/// `max_concurrent_requests` of them refused every request from everyone.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn open_streams_do_not_hold_request_slots() {
    let app = router(dispatcher(), HttpConfig::new().max_concurrent_requests(2));
    let mut streams = Vec::new();
    for id in 0..3 {
        let resp = app.clone().oneshot(listen(id)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        streams.push(resp.into_body());
    }
    let resp = app
        .clone()
        .oneshot(modern(9, "tools/list", json!({})))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_is_held_to_its_share_of_streams() {
    let app = router(dispatcher(), HttpConfig::new().max_streams_per_client(2));
    let mut streams = Vec::new();
    for id in 0..2 {
        let resp = app
            .clone()
            .oneshot(from([192, 0, 2, 1], listen(id)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        streams.push(resp.into_body());
    }
    let refused = app
        .clone()
        .oneshot(from([192, 0, 2, 1], listen(3)))
        .await
        .unwrap();
    assert_eq!(refused.status(), StatusCode::TOO_MANY_REQUESTS);

    let other = app
        .clone()
        .oneshot(from([192, 0, 2, 2], listen(4)))
        .await
        .unwrap();
    assert_eq!(other.status(), StatusCode::OK, "another client has its own");

    // A closed stream gives its share back.
    drop(streams.pop());
    let again = app
        .clone()
        .oneshot(from([192, 0, 2, 1], listen(5)))
        .await
        .unwrap();
    assert_eq!(again.status(), StatusCode::OK);
}

/// A full endpoint is `503`: `429` would tell this caller it had used up its
/// own quota.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_full_endpoint_refuses_streams_with_503() {
    let app = router(dispatcher(), HttpConfig::new().max_streams(1));
    let open = app.clone().oneshot(listen(1)).await.unwrap();
    assert_eq!(open.status(), StatusCode::OK);
    let refused = app.clone().oneshot(listen(2)).await.unwrap();
    assert_eq!(refused.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(refused.headers().contains_key(header::RETRY_AFTER));
    let body = refused.into_body().collect().await.unwrap().to_bytes();
    let error: Value = serde_json::from_slice(&body).unwrap();
    assert!(error["error"]["code"].is_i64());
}

/// A reconnecting `GET` replaces the session's stream, and the replaced one
/// ends: it used to stay open, keep-alives and all, for as long as the client
/// held it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_replaced_get_stream_ends() {
    let app = router(dispatcher(), HttpConfig::new());
    let sid = initialize(&app).await;
    let first = app.clone().oneshot(get(&sid)).await.unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    let second = app.clone().oneshot(get(&sid)).await.unwrap();
    assert_eq!(second.status(), StatusCode::OK);
    ends(first.into_body()).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deleting_a_session_ends_its_get_stream() {
    let dispatcher = dispatcher();
    let terminator = dispatcher.session_terminator();
    let app = router(
        dispatcher,
        HttpConfig::new().with_session_terminator(Arc::new(terminator)),
    );
    let sid = initialize(&app).await;
    let stream = app.clone().oneshot(get(&sid)).await.unwrap();
    assert_eq!(stream.status(), StatusCode::OK);
    let delete = Request::builder()
        .method("DELETE")
        .uri("/mcp")
        .header("mcp-session-id", &sid)
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(delete).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    ends(stream.into_body()).await;
}

/// A `GET` stream ends at shutdown. Only the client could end one before, so
/// any connected 2025-11-25 client held every deploy for the full drain
/// timeout.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_ends_get_streams() {
    let shutdown = CancellationToken::new();
    let app = router(
        dispatcher(),
        HttpConfig::new().with_shutdown(shutdown.clone()),
    );
    let sid = initialize(&app).await;
    let stream = app.clone().oneshot(get(&sid)).await.unwrap();
    assert_eq!(stream.status(), StatusCode::OK);
    shutdown.cancel();
    ends(stream.into_body()).await;
}

/// A session swept for idleness takes its `GET` stream with it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_expired_session_ends_its_get_stream() {
    let dispatcher = dispatcher().with_session_idle_timeout(Duration::from_millis(50));
    let app = router(dispatcher, HttpConfig::new());
    let sid = initialize(&app).await;
    let stream = app.clone().oneshot(get(&sid)).await.unwrap();
    assert_eq!(stream.status(), StatusCode::OK);
    tokio::time::sleep(Duration::from_millis(100)).await;
    // Sessions are swept where new ones are minted.
    initialize(&app).await;
    ends(stream.into_body()).await;
}
