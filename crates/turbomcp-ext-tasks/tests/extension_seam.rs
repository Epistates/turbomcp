//! Phase 9a: the [`Extension`] seam — `server/discover` advertisement, modern
//! method routing, and the SEP-2663 capability gate — exercised against a real
//! [`VersionDispatcher`] driving the [`TasksExtension`].

use std::sync::Arc;

use serde_json::{Value, json};
use tower::{Service, ServiceExt};
use turbomcp_core::{Implementation, JsonRpcMessage, JsonRpcRequest, McpResult};
use turbomcp_ext_tasks::{EXTENSION_ID, TasksExtension};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, ListToolsContext, McpServerCore, MethodRouter, VersionDispatcher, WithTools,
};

#[derive(Clone)]
struct Adder;

impl McpServerCore for Adder {
    fn server_info(&self) -> Implementation {
        Implementation::new("adder", "1.0.0")
    }
}

impl WithTools for Adder {
    async fn list_tools(
        &self,
        _ctx: &ListToolsContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        Ok(neutral::ListToolsResult::new(vec![neutral::Tool::new(
            "add",
            json!({"type": "object"}),
        )]))
    }

    async fn call_tool(
        &self,
        _ctx: &CallToolContext,
        _params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        Ok(neutral::CallToolResult::text("ok"))
    }
}

fn dispatcher() -> VersionDispatcher<Adder> {
    VersionDispatcher::new(Adder, MethodRouter::new().with_tools())
        .with_extension(Arc::new(TasksExtension::new()))
}

/// Draft `_meta` with the protocol version and, optionally, a declared tasks
/// extension capability.
fn draft_meta(declare_tasks: bool) -> Value {
    let mut meta = serde_json::Map::new();
    meta.insert(
        "io.modelcontextprotocol/protocolVersion".into(),
        json!("2026-07-28"),
    );
    // Required on this wire either way (SEP-2575); declaring the extension is
    // a matter of what goes *in* it, not whether it is present.
    meta.insert(
        "io.modelcontextprotocol/clientCapabilities".into(),
        if declare_tasks {
            json!({ "extensions": { EXTENSION_ID: {} } })
        } else {
            json!({})
        },
    );
    Value::Object(meta)
}

async fn call(svc: &mut VersionDispatcher<Adder>, req: JsonRpcRequest) -> Value {
    let JsonRpcMessage::Response(r) = svc
        .ready()
        .await
        .unwrap()
        .call(req.into())
        .await
        .unwrap()
        .expect("a response")
    else {
        panic!("expected a response")
    };
    json!({
        "result": r.result,
        "error": r.error.map(|e| json!({ "code": e.code, "message": e.message })),
    })
}

#[tokio::test]
async fn discover_advertises_the_tasks_extension() {
    let mut svc = dispatcher();
    let out = call(
        &mut svc,
        JsonRpcRequest::new(
            1,
            "server/discover",
            Some(json!({ "_meta": draft_meta(true) })),
        ),
    )
    .await;
    let exts = &out["result"]["capabilities"]["extensions"];
    assert!(
        exts.get(EXTENSION_ID).is_some(),
        "discover should advertise the extension, got {exts}"
    );
    assert!(out["error"].is_null());
}

#[tokio::test]
async fn declaring_client_reaches_the_extension_and_gets_task_not_found() {
    let mut svc = dispatcher();
    let req = JsonRpcRequest::new(
        2,
        "tasks/get",
        Some(json!({ "taskId": "nope", "_meta": draft_meta(true) })),
    );
    let out = call(&mut svc, req).await;
    // Routed to the extension; the empty registry → -32602 task-not-found.
    assert_eq!(out["error"]["code"], -32602);
    assert!(
        out["error"]["message"].as_str().unwrap().contains("nope"),
        "message should name the unknown task: {}",
        out["error"]["message"]
    );
}

#[tokio::test]
async fn non_declaring_client_gets_method_not_found() {
    let mut svc = dispatcher();
    // Same request, but the client did NOT declare the extension capability.
    let req = JsonRpcRequest::new(
        3,
        "tasks/get",
        Some(json!({ "taskId": "nope", "_meta": draft_meta(false) })),
    );
    let out = call(&mut svc, req).await;
    assert_eq!(
        out["error"]["code"], -32601,
        "SEP-2663: non-declaring clients get -32601 for tasks/*"
    );
}

#[tokio::test]
async fn missing_task_id_is_invalid_params() {
    let mut svc = dispatcher();
    let req = JsonRpcRequest::new(
        4,
        "tasks/cancel",
        Some(json!({ "_meta": draft_meta(true) })),
    );
    let out = call(&mut svc, req).await;
    assert_eq!(out["error"]["code"], -32602);
}

#[tokio::test]
async fn legacy_path_does_not_route_to_the_extension() {
    // On the legacy 2025-11-25 path, tasks/* are the core methods (without
    // `with_task_support` here they're method-not-found) — never the draft
    // extension. A legacy request lacks a session, so it's rejected upstream
    // (-32002) before any task handling; the point is it is NOT -32601 from the
    // extension gate, i.e. the extension never sees legacy traffic.
    let mut svc = dispatcher();
    let meta = json!({ "io.modelcontextprotocol/protocolVersion": "2025-11-25" });
    let req = JsonRpcRequest::new(5, "tasks/get", Some(json!({ "_meta": meta })));
    let out = call(&mut svc, req).await;
    assert_eq!(
        out["error"]["code"], -32002,
        "legacy tasks/get without a session is an uninitialized-session error"
    );
}
