//! Phase 4 exit criterion (HTTP half): the same hand-written
//! `McpServerCore + WithTools` server that stdio drives is reachable over the
//! Streamable HTTP endpoint — `server/discover`, `tools/list`, `tools/call` —
//! plus the transport's guards (Origin, body limit, 405 on GET, parse error).
//!
//! Tests drive the configured `axum::Router` via `tower::ServiceExt::oneshot`,
//! so no socket is bound.

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

const DRAFT_META: &str = r#"{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}"#;

fn app(config: HttpConfig) -> axum::Router {
    let dispatcher = VersionDispatcher::new(Calculator, MethodRouter::new().with_tools());
    router(dispatcher, config)
}

fn post(body: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .header("accept", "application/json, text/event-stream")
        .uri("/mcp")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_owned()))
        .unwrap()
}

/// A draft-enveloped request POST with its required mirrored headers
/// (`MCP-Protocol-Version`, `Mcp-Method`, and `Mcp-Name` when applicable).
fn draft_post(body: &str, method: &str, name: Option<&str>) -> Request<Body> {
    let mut req = Request::builder()
        .method("POST")
        .header("accept", "application/json, text/event-stream")
        .uri("/mcp")
        .header(header::CONTENT_TYPE, "application/json")
        .header("MCP-Protocol-Version", "2026-07-28")
        .header("Mcp-Method", method);
    if let Some(name) = name {
        req = req.header("Mcp-Name", name);
    }
    req.body(Body::from(body.to_owned())).unwrap()
}

async fn body_json(resp: axum::response::Response) -> Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn discover_list_and_call_over_http() {
    // discover (version-agnostic)
    let resp = app(HttpConfig::new())
        .oneshot(post(
            r#"{"jsonrpc":"2.0","id":1,"method":"server/discover"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/json"
    );
    let v = body_json(resp).await;
    // Identity is in the result's `_meta` on the frozen `2026-07-28`.
    assert_eq!(
        v["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "calculator"
    );

    // tools/list (modern, version in _meta + mirrored headers)
    let resp = app(HttpConfig::new())
        .oneshot(draft_post(
            &format!(
                r#"{{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{{"_meta":{DRAFT_META}}}}}"#
            ),
            "tools/list",
            None,
        ))
        .await
        .unwrap();
    let v = body_json(resp).await;
    assert_eq!(v["result"]["tools"][0]["name"], "add");

    // tools/call → 2 + 40 = 42
    let resp = app(HttpConfig::new())
        .oneshot(draft_post(
            &format!(
                r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"add","arguments":{{"a":2,"b":40}},"_meta":{DRAFT_META}}}}}"#
            ),
            "tools/call",
            Some("add"),
        ))
        .await
        .unwrap();
    let v = body_json(resp).await;
    assert_eq!(v["result"]["content"][0]["text"], "42");
    assert_eq!(v["result"]["isError"], false);
}

#[tokio::test]
async fn notification_yields_202_no_body() {
    let resp = app(HttpConfig::new())
        .oneshot(post(
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    assert!(bytes.is_empty());
}

#[tokio::test]
async fn malformed_body_is_400_parse_error() {
    let resp = app(HttpConfig::new())
        .oneshot(post("{not json}"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let v = body_json(resp).await;
    assert_eq!(v["error"]["code"], -32700);
    assert!(v["id"].is_null());
}

/// Draft transport rule: an unimplemented RPC method answers HTTP 404 with the
/// JSON-RPC `-32601` body (not a 200), so plain-HTTP callers see the miss.
#[tokio::test]
async fn draft_unknown_method_is_http_404_with_32601_body() {
    let body = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"does/not/exist","params":{{"_meta":{DRAFT_META}}}}}"#
    );
    let resp = app(HttpConfig::new())
        .oneshot(draft_post(&body, "does/not/exist", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["error"]["code"], -32601);

    // The same miss on the legacy wire stays HTTP 200 (errors ride the body).
    let legacy = r#"{"jsonrpc":"2.0","id":2,"method":"does/not/exist","params":{}}"#;
    let resp = app(HttpConfig::new()).oneshot(post(legacy)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["error"]["code"], -32601);
}

#[tokio::test]
async fn get_is_405_until_subscriptions() {
    let req = Request::builder()
        .method("GET")
        .header("accept", "text/event-stream")
        .uri("/mcp")
        .body(Body::empty())
        .unwrap();
    let resp = app(HttpConfig::new()).oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(resp.headers().get(header::ALLOW).unwrap(), "POST");
}

#[tokio::test]
async fn disallowed_origin_is_rejected() {
    // Default policy: an Origin that isn't allowlisted is forbidden.
    let req = Request::builder()
        .method("POST")
        .header("accept", "application/json, text/event-stream")
        .uri("/mcp")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "https://evil.example.com")
        .body(Body::from(
            r#"{"jsonrpc":"2.0","id":1,"method":"server/discover"}"#,
        ))
        .unwrap();
    let resp = app(HttpConfig::new()).oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn allowlisted_origin_passes() {
    let req = Request::builder()
        .method("POST")
        .header("accept", "application/json, text/event-stream")
        .uri("/mcp")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "https://app.example.com")
        .body(Body::from(
            r#"{"jsonrpc":"2.0","id":1,"method":"server/discover"}"#,
        ))
        .unwrap();
    let config = HttpConfig::new().allow_origin("https://app.example.com");
    let resp = app(config).oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn disallowed_host_is_rejected() {
    // With a Host allowlist, a spoofed Host is forbidden — DNS-rebinding defense
    // in depth (covers non-browser clients that don't send an Origin).
    let req = Request::builder()
        .method("POST")
        .header("accept", "application/json, text/event-stream")
        .uri("/mcp")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::HOST, "evil.example.com")
        .body(Body::from(
            r#"{"jsonrpc":"2.0","id":1,"method":"server/discover"}"#,
        ))
        .unwrap();
    let config = HttpConfig::new().allow_host("mcp.example.com");
    let resp = app(config).oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn allowlisted_host_passes() {
    let req = Request::builder()
        .method("POST")
        .header("accept", "application/json, text/event-stream")
        .uri("/mcp")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::HOST, "mcp.example.com")
        .body(Body::from(
            r#"{"jsonrpc":"2.0","id":1,"method":"server/discover"}"#,
        ))
        .unwrap();
    let config = HttpConfig::new().allow_host("mcp.example.com");
    let resp = app(config).oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn oversized_body_is_413() {
    let big = "x".repeat(2048);
    let body = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"server/discover","params":{{"pad":"{big}"}}}}"#
    );
    let config = HttpConfig::new().max_body_bytes(512);
    let resp = app(config).oneshot(post(&body)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

/// A `#[tool]` body that panics must still answer its POST with `-32603`
/// rather than tearing down the connection with no response. Covers the
/// lazy-upgrade dispatch path (`tools/call`), where the call future is polled
/// from both the inline arm and the upgraded SSE stream.
#[tokio::test]
async fn a_panicking_tool_answers_with_an_internal_error() {
    #[derive(Clone)]
    struct Exploding;

    impl McpServerCore for Exploding {
        fn server_info(&self) -> Implementation {
            Implementation::new("exploding", "0.1.0")
        }
    }

    impl WithTools for Exploding {
        async fn list_tools(
            &self,
            _ctx: &ListToolsContext,
            _params: neutral::ListParams,
        ) -> McpResult<neutral::ListToolsResult> {
            Ok(neutral::ListToolsResult::new(vec![neutral::Tool::new(
                "boom",
                json!({"type": "object"}),
            )]))
        }

        async fn call_tool(
            &self,
            _ctx: &CallToolContext,
            _params: neutral::CallToolParams,
        ) -> McpResult<neutral::CallToolResult> {
            panic!("tool exploded")
        }
    }

    let app = || {
        let dispatcher = VersionDispatcher::new(Exploding, MethodRouter::new().with_tools());
        router(dispatcher, HttpConfig::new())
    };

    let resp = app()
        .oneshot(draft_post(
            &format!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"boom","arguments":{{}},"_meta":{DRAFT_META}}}}}"#
            ),
            "tools/call",
            Some("boom"),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_json(resp).await;
    assert_eq!(v["id"], 1);
    assert_eq!(v["error"]["code"], -32603);
    // The panic payload belongs in the log, not on the wire.
    assert!(
        !v["error"]["message"].as_str().unwrap().contains("exploded"),
        "leaked panic payload: {v}"
    );

    // The endpoint keeps serving afterwards.
    let resp = app()
        .oneshot(draft_post(
            &format!(r#"{{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{{"_meta":{DRAFT_META}}}}}"#),
            "tools/list",
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_json(resp).await;
    assert_eq!(v["result"]["tools"][0]["name"], "boom");
}
