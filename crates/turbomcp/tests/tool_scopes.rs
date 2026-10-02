//! Bucket-A A6 (part 2): `#[tool(scopes(…))]` denies a call unless the caller's
//! identity holds every required OAuth scope. The handler raises
//! `McpError::InsufficientScope` naming them; through the dispatcher a client
//! sees a tool error (and over HTTP, a step-up challenge: `tool_step_up.rs`).

use serde_json::{Map, Value, json};
use tower::{Service, ServiceExt};
use turbomcp::neutral::{CallToolParams, CallToolResult};
use turbomcp::prelude::*;
use turbomcp::{
    CallToolContext, Claims, Identity, JsonRpcMessage, JsonRpcRequest, McpRequest, ProtocolVersion,
    RequestContext, WithTools,
};

#[derive(Clone)]
struct Guarded;

#[server(name = "guarded", version = "1.0.0")]
impl Guarded {
    /// Requires the `admin` scope.
    #[tool(description = "Admin only", scopes("admin"))]
    async fn secret(&self) -> String {
        "top secret".into()
    }

    /// No scope requirement.
    #[tool(description = "Public")]
    async fn open(&self) -> String {
        "public".into()
    }
}

fn ctx(scope: Option<&str>) -> CallToolContext {
    let identity = match scope {
        Some(s) => {
            let mut claims = Claims::new();
            claims.insert("scope".into(), json!(s));
            Identity::Bearer {
                sub: "u".into(),
                claims,
            }
        }
        None => Identity::Anonymous,
    };
    CallToolContext::new(RequestContext::new(ProtocolVersion::LATEST).with_identity(identity))
}

async fn call(name: &str, ctx: &CallToolContext) -> McpResult<CallToolResult> {
    Guarded
        .call_tool(ctx, CallToolParams::new(name, Map::new()))
        .await
}

fn text(r: &CallToolResult) -> String {
    match &r.content[0] {
        turbomcp::neutral::Content::Text { text, .. } => text.clone(),
        other => panic!("expected text, got {other:?}"),
    }
}

#[tokio::test]
async fn scoped_tool_allows_caller_with_scope() {
    let r = call("secret", &ctx(Some("read admin write")))
        .await
        .unwrap();
    assert!(!r.is_error, "should allow: {r:?}");
    assert_eq!(text(&r), "top secret");
}

#[tokio::test]
async fn scoped_tool_denies_caller_without_scope() {
    match call("secret", &ctx(Some("read write"))).await {
        Err(McpError::InsufficientScope(scopes)) => assert_eq!(scopes, ["admin"]),
        other => panic!("should deny naming the scope, got {other:?}"),
    }
}

#[tokio::test]
async fn scoped_tool_denies_anonymous() {
    assert!(matches!(
        call("secret", &ctx(None)).await,
        Err(McpError::InsufficientScope(_))
    ));
}

/// Through the dispatcher, the denial is a tool error the model can read.
#[tokio::test]
async fn through_the_dispatcher_a_denial_is_a_tool_error() {
    let mut svc = Guarded.into_server().build();
    let request = McpRequest::new(JsonRpcRequest::new(
        1,
        "tools/call",
        Some(json!({
            "name": "secret",
            "arguments": {},
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientCapabilities": {},
            },
        })),
    ))
    .with(Identity::Bearer {
        sub: "u".into(),
        claims: json!({ "scope": "read" }).as_object().unwrap().clone(),
    });
    let Some(JsonRpcMessage::Response(r)) = svc.ready().await.unwrap().call(request).await.unwrap()
    else {
        panic!("expected a response");
    };
    let result: Value = r.result.expect("a tool result, not a protocol error");
    assert_eq!(result["isError"], true);
    assert_eq!(
        result["content"][0]["text"],
        "insufficient scope: requires admin"
    );
}

#[tokio::test]
async fn unscoped_tool_allows_anyone() {
    let r = call("open", &ctx(None)).await.unwrap();
    assert!(!r.is_error);
    assert_eq!(text(&r), "public");
}
