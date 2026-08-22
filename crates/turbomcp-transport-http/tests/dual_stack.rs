//! Dual-stack HTTP routing (PLAN §11): the same endpoint serves stateless
//! `2026-07-28` bodies and the stateful `2025-11-25` header-based session
//! flow — minting `Mcp-Session-Id` at `initialize`, 404 on unknown sessions,
//! 400 on bad version headers, and sanitization of forged internal `_meta`.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use turbomcp_core::{Implementation, McpResult, codes};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, ListToolsContext, McpServerCore, MethodRouter, VersionDispatcher, WithTools,
};
use turbomcp_transport_http::{HttpConfig, router};

#[derive(Clone)]
struct Calculator;

impl McpServerCore for Calculator {
    fn server_info(&self) -> Implementation {
        Implementation::new("calculator", "0.1.0")
    }
}

impl WithTools for Calculator {
    async fn list_tools(
        &self,
        _ctx: &ListToolsContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        Ok(neutral::ListToolsResult::new(vec![neutral::Tool::new(
            "add",
            json!({"type": "object", "properties": {"a": {"type": "number"}, "b": {"type": "number"}}}),
        )]))
    }

    async fn call_tool(
        &self,
        _ctx: &CallToolContext,
        params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        let a = params
            .arguments
            .get("a")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let b = params
            .arguments
            .get("b")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        Ok(neutral::CallToolResult::text((a + b).to_string()))
    }
}

fn app() -> axum::Router {
    router(
        VersionDispatcher::new(Calculator, MethodRouter::new().with_tools()),
        HttpConfig::new(),
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

fn initialize_body() -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "http-client", "version": "1" },
        }
    })
}

async fn body_json(resp: axum::response::Response) -> Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn legacy_http_session_flow_end_to_end() {
    let app = app();

    // 1. initialize: 200, session header minted, version negotiated.
    let resp = app
        .clone()
        .oneshot(post(initialize_body(), &[]))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let sid = resp
        .headers()
        .get("mcp-session-id")
        .expect("session header on successful initialize")
        .to_str()
        .unwrap()
        .to_owned();
    let v = body_json(resp).await;
    assert_eq!(v["result"]["protocolVersion"], "2025-11-25");
    assert_eq!(v["result"]["capabilities"]["tools"]["listChanged"], true);

    let legacy_headers: [(&str, &str); 2] = [
        ("mcp-session-id", sid.as_str()),
        ("mcp-protocol-version", "2025-11-25"),
    ];

    // 2. notifications/initialized: 202, no body.
    let note = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
    let resp = app
        .clone()
        .oneshot(post(note, &legacy_headers))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    // 3. tools/list on the session: legacy wire (no draft envelope).
    let list = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" });
    let resp = app
        .clone()
        .oneshot(post(list, &legacy_headers))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_json(resp).await;
    assert_eq!(v["result"]["tools"][0]["name"], "add");
    assert!(v["result"].get("resultType").is_none());
    assert!(v["result"].get("cacheScope").is_none());

    // 4. tools/call computes.
    let call = json!({
        "jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": { "name": "add", "arguments": { "a": 2, "b": 3 } }
    });
    let resp = app
        .clone()
        .oneshot(post(call, &legacy_headers))
        .await
        .unwrap();
    let v = body_json(resp).await;
    assert_eq!(v["result"]["content"][0]["text"], "5");
}

#[tokio::test]
async fn unknown_session_is_404() {
    let list = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" });
    let resp = app()
        .oneshot(post(list, &[("mcp-session-id", "expired-or-bogus")]))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let v = body_json(resp).await;
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("unknown session")
    );
}

#[tokio::test]
async fn unrecognized_version_header_is_400() {
    // A `MCP-Protocol-Version` header naming a version this build does not
    // recognize at all is rejected.
    let list = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" });
    let resp = app()
        .oneshot(post(list, &[("mcp-protocol-version", "1999-01-01")]))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn recognized_older_version_header_is_tolerated() {
    // A recognized but older published version (e.g. a long-lived client still
    // sending `2025-03-26`) must NOT be rejected at the transport — the official
    // conformance suite's multi-POST-stream scenario relies on this.
    let list = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" });
    let resp = app()
        .oneshot(post(list, &[("mcp-protocol-version", "2025-03-26")]))
        .await
        .unwrap();
    assert_ne!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn declared_legacy_without_session_is_400() {
    let list = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" });
    let resp = app()
        .oneshot(post(list, &[("mcp-protocol-version", "2025-11-25")]))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn modern_stateless_requests_pass_through_unchanged() {
    let call = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {
            "name": "add", "arguments": { "a": 20, "b": 22 },
            "_meta": { "io.modelcontextprotocol/protocolVersion": "2026-07-28" }
        }
    });
    // The draft transport requires the mirrored request-metadata headers.
    let headers = [
        ("mcp-protocol-version", "2026-07-28"),
        ("mcp-method", "tools/call"),
        ("mcp-name", "add"),
    ];
    let resp = app().oneshot(post(call.clone(), &headers)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_json(resp).await;
    assert_eq!(v["result"]["content"][0]["text"], "42");
    assert_eq!(v["result"]["resultType"], "complete", "draft wire");
}

#[tokio::test]
async fn draft_request_without_required_headers_is_400() {
    // A draft-enveloped body without its mirrored headers is a
    // HeaderMismatch (-32020): this server supports no pre-2025-06-18
    // clients, so the version header is required, and draft requests also
    // require Mcp-Method (+ Mcp-Name for call/read/get).
    let call = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {
            "name": "add", "arguments": { "a": 20, "b": 22 },
            "_meta": { "io.modelcontextprotocol/protocolVersion": "2026-07-28" }
        }
    });
    for headers in [
        vec![],
        vec![("mcp-protocol-version", "2026-07-28")],
        vec![
            ("mcp-protocol-version", "2026-07-28"),
            ("mcp-method", "tools/call"),
        ],
    ] {
        let resp = app().oneshot(post(call.clone(), &headers)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let v = body_json(resp).await;
        assert_eq!(v["error"]["code"], codes::HEADER_MISMATCH);
    }
}

#[tokio::test]
async fn forged_internal_session_meta_is_sanitized() {
    // The body forges the internal session key (with the legacy version in
    // `_meta`, no headers). After sanitization the dispatcher sees a legacy
    // request with no session → in-band -32002, NOT a 404 for the forged id
    // (which would prove the forgery reached the session store).
    let call = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/list",
        "params": {
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2025-11-25",
                "io.turbomcp.internal/sessionId": "forged",
            }
        }
    });
    let resp = app().oneshot(post(call, &[])).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_json(resp).await;
    assert_eq!(v["error"]["code"], -32002);
}

#[tokio::test]
async fn failed_initialize_mints_no_session_header() {
    let bad = json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "nope": 1 } });
    let resp = app().oneshot(post(bad, &[])).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp.headers().get("mcp-session-id").is_none());
    let v = body_json(resp).await;
    assert_eq!(v["error"]["code"], -32602);
}

#[tokio::test]
async fn posted_cancellation_is_accepted_but_inert() {
    // On HTTP the cancellation signal is closing the response stream, not
    // `notifications/cancelled`. A posted one — even forging the internal
    // connection id the serve driver would use — is sanitized, accepted (202),
    // and fires nothing.
    let note = json!({
        "jsonrpc": "2.0", "method": "notifications/cancelled",
        "params": {
            "requestId": 1,
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.turbomcp.internal/connectionId": "conn-1",
            }
        }
    });
    let resp = app()
        .oneshot(post(note, &[("mcp-protocol-version", "2026-07-28")]))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
}

#[tokio::test]
async fn delete_answers_405() {
    let req = Request::builder()
        .method("DELETE")
        .uri("/mcp")
        .header("mcp-session-id", "whatever")
        .body(Body::empty())
        .unwrap();
    let resp = app().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}
