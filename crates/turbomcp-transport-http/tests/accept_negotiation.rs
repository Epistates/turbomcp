//! Transports spec `Accept` requirements: a POST client MUST list both
//! `application/json` and `text/event-stream`; a GET client MUST list
//! `text/event-stream`. Violations answer `406` with a JSON-RPC error body;
//! matching follows RFC 9110 media ranges (wildcards and `q=` parameters).

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;
use turbomcp_core::{Implementation, McpResult};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, ListToolsContext, McpServerCore, MethodRouter, VersionDispatcher, WithTools,
};
use turbomcp_transport_http::{HttpConfig, router};

#[derive(Clone)]
struct Echo;

impl McpServerCore for Echo {
    fn server_info(&self) -> Implementation {
        Implementation::new("echo", "0.1.0")
    }
}

impl WithTools for Echo {
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

fn app() -> axum::Router {
    router(
        VersionDispatcher::new(Echo, MethodRouter::new().with_tools()),
        HttpConfig::new(),
    )
}

const DISCOVER: &str = r#"{"jsonrpc":"2.0","id":1,"method":"server/discover","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}"#;

fn post(accept: Option<&str>) -> Request<Body> {
    let mut req = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header(header::CONTENT_TYPE, "application/json")
        // The stateless wire requires these mirrors on every request; this
        // test is about `Accept`, so everything else has to be valid.
        .header("MCP-Protocol-Version", "2026-07-28")
        .header("Mcp-Method", "server/discover");
    if let Some(accept) = accept {
        req = req.header(header::ACCEPT, accept);
    }
    req.body(Body::from(DISCOVER)).unwrap()
}

fn get(accept: Option<&str>) -> Request<Body> {
    let mut req = Request::builder().method("GET").uri("/mcp");
    if let Some(accept) = accept {
        req = req.header(header::ACCEPT, accept);
    }
    req.body(Body::empty()).unwrap()
}

async fn body_json(resp: axum::response::Response) -> Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

/// POST without both required types — missing header, one type only, or an
/// unrelated type — is `406` with a JSON-RPC error body.
#[tokio::test]
async fn post_without_both_accept_types_is_406() {
    for accept in [
        None,
        Some("application/json"),
        Some("text/event-stream"),
        Some("text/html"),
        Some("application/json, text/html"),
    ] {
        let resp = app().oneshot(post(accept)).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::NOT_ACCEPTABLE,
            "accept: {accept:?}"
        );
        let v = body_json(resp).await;
        assert_eq!(v["error"]["code"], -32000, "accept {accept:?}: {v}");
        assert!(
            v["error"]["message"]
                .as_str()
                .unwrap()
                .contains("text/event-stream"),
            "names the missing type: {v}"
        );
    }
}

/// The compliant header — plus RFC 9110 media-range equivalents (wildcards,
/// `q=` parameters, reordering) — is accepted.
#[tokio::test]
async fn post_with_covering_media_ranges_is_served() {
    for accept in [
        "application/json, text/event-stream",
        "text/event-stream, application/json",
        "*/*",
        "application/*, text/*",
        "application/json;q=0.9, text/event-stream;q=0.5",
    ] {
        let resp = app().oneshot(post(Some(accept))).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "accept: {accept}");
        let v = body_json(resp).await;
        assert!(v.get("result").is_some(), "accept {accept}: {v}");
    }
}

/// GET must list `text/event-stream`; `application/json` alone (or nothing)
/// is `406`. A covering range passes the accept gate and reaches the next
/// guard (405: no session, so no GET stream to open).
#[tokio::test]
async fn get_requires_text_event_stream() {
    for accept in [None, Some("application/json")] {
        let resp = app().oneshot(get(accept)).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::NOT_ACCEPTABLE,
            "accept: {accept:?}"
        );
    }
    for accept in ["text/event-stream", "*/*", "text/*"] {
        let resp = app().oneshot(get(Some(accept))).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "accept gate passed, next guard answers: {accept}"
        );
    }
}

/// DELETE has no `Accept` requirement in the spec — a header-less DELETE
/// reaches session handling (405 here: no terminator configured).
#[tokio::test]
async fn delete_is_exempt_from_accept_validation() {
    let req = Request::builder()
        .method("DELETE")
        .uri("/mcp")
        .header("mcp-session-id", "s-1")
        .body(Body::empty())
        .unwrap();
    let resp = app().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}
