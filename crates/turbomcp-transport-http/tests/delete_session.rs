//! Client-initiated session termination (`2025-11-25` §Session Management): a
//! `DELETE` with the session header ends the session (204), a second one is
//! 404, a subsequent request on the dead session is 404, and an endpoint with
//! no terminator configured refuses with 405.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt;
use turbomcp_core::{Implementation, McpResult};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, ListToolsContext, McpServerCore, MethodRouter, VersionDispatcher, WithTools,
};
use turbomcp_transport_http::{HttpConfig, service};

#[derive(Clone)]
struct Bare;

impl McpServerCore for Bare {
    fn server_info(&self) -> Implementation {
        Implementation::new("bare", "0.1.0")
    }
}

impl WithTools for Bare {
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

/// An endpoint that honors DELETE (terminator wired from the dispatcher).
fn app_with_termination() -> turbomcp_transport_http::HttpService {
    let dispatcher = VersionDispatcher::new(Bare, MethodRouter::new().with_tools());
    let terminator = dispatcher.session_terminator();
    service(
        dispatcher,
        HttpConfig::new().with_session_terminator(Arc::new(terminator)),
    )
}

fn post(body: Value, headers: &[(&str, &str)]) -> Request<Body> {
    let mut req = Request::builder()
        .method("POST")
        .header("accept", "application/json, text/event-stream")
        .uri("/mcp")
        .header(header::CONTENT_TYPE, "application/json");
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    req.body(Body::from(body.to_string())).unwrap()
}

fn delete(session: Option<&str>) -> Request<Body> {
    let mut req = Request::builder().method("DELETE").uri("/mcp");
    if let Some(sid) = session {
        req = req.header("mcp-session-id", sid);
    }
    req.body(Body::empty()).unwrap()
}

fn initialize_body() -> Value {
    json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "c", "version": "1" },
        }
    })
}

async fn init_session(app: &turbomcp_transport_http::HttpService) -> String {
    let resp = app
        .clone()
        .oneshot(post(initialize_body(), &[]))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    resp.headers()
        .get("mcp-session-id")
        .expect("session header")
        .to_str()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn delete_terminates_then_404s() {
    let app = app_with_termination();
    let sid = init_session(&app).await;

    // DELETE the live session → 204.
    let resp = app.clone().oneshot(delete(Some(&sid))).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // DELETE again → 404 (already gone).
    let resp = app.clone().oneshot(delete(Some(&sid))).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // A request on the terminated session → 404 (UnknownSession → re-initialize).
    let list = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" });
    let headers = [
        ("mcp-session-id", sid.as_str()),
        ("mcp-protocol-version", "2025-11-25"),
    ];
    let resp = app.clone().oneshot(post(list, &headers)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delete_without_session_header_is_400() {
    let app = app_with_termination();
    let resp = app.oneshot(delete(None)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn delete_unsupported_without_terminator_is_405() {
    // No terminator configured → the spec-permitted refusal.
    let app = service(
        VersionDispatcher::new(Bare, MethodRouter::new().with_tools()),
        HttpConfig::new(),
    );
    let resp = app.oneshot(delete(Some("whatever"))).await.unwrap();
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}
