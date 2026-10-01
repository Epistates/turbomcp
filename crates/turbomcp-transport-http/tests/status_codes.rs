//! What the endpoint answers when it refuses, and on which wire: a 2026-07-28
//! request ignores a session header, an endpoint without sessions answers
//! `GET`/`DELETE` with `405`, a message with neither a session nor the
//! stateless envelope is a `400`, and every refusal carries a JSON-RPC body a
//! client can classify.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use turbomcp_core::{Implementation, McpResult, ProtocolVersion, codes};
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

fn app(config: HttpConfig) -> axum::Router {
    let dispatcher = VersionDispatcher::new(Plain, MethodRouter::new().with_tools());
    let terminator = dispatcher.session_terminator();
    router(
        dispatcher,
        config.with_session_terminator(Arc::new(terminator)),
    )
}

fn post(body: Value, headers: &[(&str, &str)]) -> Request<Body> {
    let mut req = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("accept", "application/json, text/event-stream")
        .header(header::CONTENT_TYPE, "application/json");
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    req.body(Body::from(body.to_string())).unwrap()
}

fn modern_list(id: i64) -> Value {
    json!({
        "jsonrpc": "2.0", "id": id, "method": "tools/list",
        "params": { "_meta": {
            "io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientCapabilities": {},
        }}
    })
}

const MODERN: [(&str, &str); 2] = [
    ("MCP-Protocol-Version", "2026-07-28"),
    ("Mcp-Method", "tools/list"),
];

async fn json_body(resp: axum::response::Response) -> Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).expect("a JSON-RPC body")
}

async fn legacy_session(app: &axum::Router) -> String {
    let init = json!({
        "jsonrpc": "2.0", "id": 0, "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "t", "version": "1" },
        }
    });
    let resp = app.clone().oneshot(post(init, &[])).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    resp.headers()["mcp-session-id"]
        .to_str()
        .unwrap()
        .to_owned()
}

/// "An `Mcp-Session-Id` header on a request: ignore it." A dual-era client
/// (or a gateway replaying a sticky header) that sends a stale one with a
/// 2026-07-28 request used to get a bodiless `404`, which the spec's own
/// fallback reads as a legacy HTTP+SSE server.
#[tokio::test]
async fn a_modern_request_ignores_a_stale_session_header() {
    let mut headers = MODERN.to_vec();
    headers.push(("mcp-session-id", "long-gone"));
    let resp = app(HttpConfig::new())
        .oneshot(post(modern_list(1), &headers))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = json_body(resp).await;
    assert!(v["result"]["tools"].is_array(), "{v}");
}

/// A server that serves only 2026-07-28 "SHOULD respond ... HTTP GET or
/// DELETE to the MCP endpoint: respond with `405 Method Not Allowed`", with a
/// body a modern client recognizes.
#[tokio::test]
async fn an_endpoint_without_sessions_refuses_get_and_delete_with_405() {
    let app = app(HttpConfig::new().with_supported_versions(vec![ProtocolVersion::V2026_07_28]));
    for method in ["GET", "DELETE"] {
        let req = Request::builder()
            .method(method)
            .uri("/mcp")
            .header(header::ACCEPT, "text/event-stream")
            .header("mcp-session-id", "anything")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED, "{method}");
        assert_eq!(resp.headers()[header::ALLOW], "POST");
        let v = json_body(resp).await;
        assert!(v["error"]["code"].is_i64(), "{method}: {v}");
    }
}

/// Neither a session nor the stateless envelope: `400`, echoing the id. It
/// used to be dispatched and answered `200` with an in-band error, which a
/// probe or a gateway counts as a working call.
#[tokio::test]
async fn a_message_with_neither_a_session_nor_the_envelope_is_400() {
    let bare = json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/list", "params": {} });
    let resp = app(HttpConfig::new())
        .oneshot(post(bare, &[]))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let v = json_body(resp).await;
    assert_eq!(v["id"], 4);
    assert_eq!(v["error"]["code"], codes::NO_ACTIVE_SESSION);
}

/// `server/discover` is how a client finds out which wire it is on, so it
/// needs neither a session nor the header.
#[tokio::test]
async fn discover_needs_neither_a_session_nor_the_header() {
    let discover = json!({ "jsonrpc": "2.0", "id": 5, "method": "server/discover" });
    let resp = app(HttpConfig::new())
        .oneshot(post(discover, &[]))
        .await
        .unwrap();
    assert_ne!(resp.status(), StatusCode::BAD_REQUEST);
}

/// `UnsupportedProtocolVersionError`: "For HTTP, the response status code
/// MUST be `400 Bad Request`", on the session wire too.
#[tokio::test]
async fn an_unsupported_body_version_is_400_on_the_session_wire() {
    let app = app(HttpConfig::new());
    let sid = legacy_session(&app).await;
    let call = json!({
        "jsonrpc": "2.0", "id": 6, "method": "tools/list",
        "params": { "_meta": { "io.modelcontextprotocol/protocolVersion": "1999-01-01" } }
    });
    let resp = app
        .oneshot(post(call, &[("mcp-session-id", &sid)]))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let v = json_body(resp).await;
    assert_eq!(v["error"]["code"], codes::UNSUPPORTED_PROTOCOL_VERSION);
}

/// An unknown session is `404` with a JSON-RPC body naming the request, not
/// an empty one.
#[tokio::test]
async fn an_unknown_session_is_404_with_a_body() {
    let call = json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/list", "params": {} });
    let resp = app(HttpConfig::new())
        .oneshot(post(
            call,
            &[
                ("mcp-session-id", "never-minted"),
                ("MCP-Protocol-Version", "2025-11-25"),
            ],
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let v = json_body(resp).await;
    assert_eq!(v["id"], 7);
    assert!(v["error"]["code"].is_i64());
}

/// "The HTTP response body MAY comprise a JSON-RPC error response that has no
/// `id`": a refused Origin says why in a body a client can parse.
#[tokio::test]
async fn a_refused_origin_answers_a_json_rpc_body() {
    let mut req = post(modern_list(8), &MODERN);
    req.headers_mut()
        .insert(header::ORIGIN, "https://evil.example".parse().unwrap());
    let resp = app(HttpConfig::new()).oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let v = json_body(resp).await;
    assert!(v["error"]["code"].is_i64());
    assert!(v["id"].is_null());
}

/// A full session table refuses a newcomer with `503` + `Retry-After`, and
/// the session it already holds keeps working. It used to evict the least
/// recently used session instead, so an anonymous `initialize` flood pushed
/// out every real one.
#[tokio::test]
async fn a_full_session_table_refuses_initialize_and_keeps_its_sessions() {
    let dispatcher = VersionDispatcher::new(Plain, MethodRouter::new().with_tools())
        .with_session_backend(Arc::new(turbomcp_server::SessionStore::with_capacity(1)));
    let terminator = dispatcher.session_terminator();
    let app = router(
        dispatcher,
        HttpConfig::new().with_session_terminator(Arc::new(terminator)),
    );
    let sid = legacy_session(&app).await;

    let init = json!({
        "jsonrpc": "2.0", "id": 0, "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "flood", "version": "1" },
        }
    });
    let refused = app.clone().oneshot(post(init, &[])).await.unwrap();
    assert_eq!(refused.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(refused.headers().contains_key(header::RETRY_AFTER));
    assert!(refused.headers().get("mcp-session-id").is_none());

    let list = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {} });
    let resp = app
        .oneshot(post(list, &[("mcp-session-id", &sid)]))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}
