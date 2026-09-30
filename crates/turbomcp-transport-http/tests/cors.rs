//! CORS follows the Origin policy: an allowed origin gets headers a browser
//! can use with bearer auth, anything else gets none.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use serde_json::json;
use tower::ServiceExt;
use turbomcp_core::{Implementation, McpResult};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, ListToolsContext, McpServerCore, MethodRouter, VersionDispatcher, WithTools,
};
use turbomcp_transport_http::{HttpConfig, router};

#[derive(Clone)]
struct Plain;

impl McpServerCore for Plain {
    fn server_info(&self) -> Implementation {
        Implementation::new("plain", "0.1.0")
    }
}

impl WithTools for Plain {
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

const APP: &str = "https://app.example.com";

fn app(config: HttpConfig) -> axum::Router {
    router(
        VersionDispatcher::new(Plain, MethodRouter::new().with_tools()),
        config,
    )
}

fn preflight(origin: &str) -> Request<Body> {
    Request::builder()
        .method("OPTIONS")
        .uri("/mcp")
        .header(header::ORIGIN, origin)
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
        .header(
            header::ACCESS_CONTROL_REQUEST_HEADERS,
            "authorization, content-type, mcp-protocol-version, mcp-method, mcp-param-region",
        )
        .body(Body::empty())
        .unwrap()
}

/// `Access-Control-Allow-Headers: *` does not cover `Authorization` (Fetch
/// standard), so the permissive layer failed every bearer-authenticated
/// preflight, and an origin allowed without `enable_cors` got no CORS headers
/// at all. An allowed origin's preflight now names what it asked for.
#[tokio::test]
async fn an_allowed_origin_preflights_with_authorization() {
    let resp = app(HttpConfig::new().allow_origin(APP))
        .oneshot(preflight(APP))
        .await
        .unwrap();
    assert!(resp.status().is_success(), "{}", resp.status());
    let h = resp.headers();
    assert_eq!(h[header::ACCESS_CONTROL_ALLOW_ORIGIN], APP);
    let allowed = h[header::ACCESS_CONTROL_ALLOW_HEADERS]
        .to_str()
        .unwrap()
        .to_ascii_lowercase();
    for name in ["authorization", "mcp-protocol-version", "mcp-param-region"] {
        assert!(allowed.contains(name), "{name} in {allowed}");
    }
    let methods = h[header::ACCESS_CONTROL_ALLOW_METHODS].to_str().unwrap();
    assert!(methods.contains("POST") && methods.contains("DELETE"));
}

#[tokio::test]
async fn another_origin_gets_no_cors_headers() {
    let resp = app(HttpConfig::new().allow_origin(APP))
        .oneshot(preflight("https://evil.example"))
        .await
        .unwrap();
    assert!(
        resp.headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none()
    );
}

/// A browser can only read the headers it is allowed to: without
/// `Mcp-Session-Id` exposed, a web client can't keep the session it opened.
#[tokio::test]
async fn a_response_exposes_the_session_header() {
    let init = json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "web", "version": "1" },
        }
    });
    let req = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header(header::ORIGIN, APP)
        .header("accept", "application/json, text/event-stream")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(init.to_string()))
        .unwrap();
    let resp = app(HttpConfig::new().allow_origin(APP))
        .oneshot(req)
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let exposed = resp.headers()[header::ACCESS_CONTROL_EXPOSE_HEADERS]
        .to_str()
        .unwrap()
        .to_ascii_lowercase();
    assert!(exposed.contains("mcp-session-id"), "{exposed}");
    assert!(exposed.contains("www-authenticate"), "{exposed}");
}

/// No allowed origin, no CORS: the default serves non-browser clients only.
#[tokio::test]
async fn the_default_answers_no_cors() {
    let resp = app(HttpConfig::new())
        .oneshot(preflight(APP))
        .await
        .unwrap();
    assert!(
        resp.headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none()
    );
}
