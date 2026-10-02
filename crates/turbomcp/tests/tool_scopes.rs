//! Bucket-A A6 (part 2): `#[tool(scopes(…))]` denies a call unless the caller's
//! identity holds every required OAuth scope. The handler raises
//! `McpError::InsufficientScope` naming them; through the dispatcher a client
//! sees a tool error (and over HTTP, a step-up challenge: `tool_step_up.rs`).

use serde_json::{Map, Value, json};
use tower::{Service, ServiceExt};
use turbomcp::neutral::{CallToolParams, CallToolResult};
use turbomcp::prelude::*;
use turbomcp::{
    CallToolContext, Claims, Identity, JsonRpcMessage, JsonRpcRequest, LegacySessionAdapter,
    McpRequest, ProtocolVersion, RequestContext, WithTools,
};
use turbomcp_service::ScopeChallenge;

#[derive(Clone)]
struct Guarded;

#[server(name = "guarded", version = "1.0.0")]
impl Guarded {
    /// Requires the `admin` scope.
    #[tool(description = "Admin only", scopes("admin"))]
    async fn secret(&self) -> String {
        "top secret".into()
    }

    /// Requires `admin`, and runs only as a task.
    #[tool(description = "Admin batch", task = "required", scopes("admin"))]
    async fn batch(&self) -> String {
        "batched".into()
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

fn reader() -> Identity {
    Identity::Bearer {
        sub: "u".into(),
        claims: json!({ "scope": "read" }).as_object().unwrap().clone(),
    }
}

/// The refusal a task-augmented call to a scoped tool must get: the tool
/// error and the step-up challenge, never a task that fails later (whose
/// caller would have no challenge to step up on).
fn assert_refused(r: &turbomcp::JsonRpcResponse, challenge: &ScopeChallenge) {
    let result = r.result.as_ref().expect("a tool result");
    assert_eq!(result["isError"], true, "{result}");
    assert!(
        result.get("task").is_none() && result.get("taskId").is_none(),
        "{result}"
    );
    assert_eq!(challenge.demanded(), Some(&["admin".to_owned()][..]));
}

#[tokio::test]
async fn a_scoped_task_call_on_2025_11_25_is_refused_before_it_becomes_a_task() {
    let mut svc = LegacySessionAdapter::new(Guarded.into_server().with_tasks().build());
    let init = JsonRpcRequest::new(
        0,
        "initialize",
        Some(json!({
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "c", "version": "1" },
        })),
    );
    // A session belongs to the principal that opened it.
    let init = McpRequest::new(init).with(reader());
    svc.ready().await.unwrap().call(init).await.unwrap();
    let challenge = ScopeChallenge::default();
    let call = McpRequest::new(JsonRpcRequest::new(
        1,
        "tools/call",
        Some(json!({ "name": "batch", "arguments": {}, "task": { "ttl": 60000 } })),
    ))
    .with(reader())
    .with(challenge.clone());
    let Some(JsonRpcMessage::Response(r)) = svc.ready().await.unwrap().call(call).await.unwrap()
    else {
        panic!("expected a response");
    };
    assert_refused(&r, &challenge);
}

#[cfg(feature = "ext-tasks")]
#[tokio::test]
async fn a_scoped_task_call_on_2026_07_28_is_refused_before_it_becomes_a_task() {
    let mut svc = Guarded
        .into_server()
        .with_extension(std::sync::Arc::new(
            turbomcp::ext_tasks::TasksExtension::new(),
        ))
        .build();
    let challenge = ScopeChallenge::default();
    let call = McpRequest::new(JsonRpcRequest::new(
        1,
        "tools/call",
        Some(json!({
            "name": "batch",
            "arguments": {},
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientCapabilities": {
                    "extensions": { "io.modelcontextprotocol/tasks": {} },
                },
            },
        })),
    ))
    .with(reader())
    .with(challenge.clone());
    let Some(JsonRpcMessage::Response(r)) = svc.ready().await.unwrap().call(call).await.unwrap()
    else {
        panic!("expected a response");
    };
    assert_refused(&r, &challenge);
}
