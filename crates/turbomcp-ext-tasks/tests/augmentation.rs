//! Phase 9b: `tools/call` task augmentation — `CreateTaskResult`
//! (`resultType: "task"`), polling to completion via `tasks/get`, and
//! cancellation — driven against a real [`VersionDispatcher`].

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tower::{Service, ServiceExt};
use turbomcp_core::{Implementation, JsonRpcMessage, JsonRpcRequest, McpError, McpResult};
use turbomcp_ext_tasks::{EXTENSION_ID, TasksExtension};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, ListToolsContext, McpServerCore, MethodRouter, VersionDispatcher, WithTools,
};

#[derive(Clone)]
struct Tools;

impl McpServerCore for Tools {
    fn server_info(&self) -> Implementation {
        Implementation::new("tools", "1.0.0")
    }
}

impl WithTools for Tools {
    async fn list_tools(
        &self,
        _ctx: &ListToolsContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        Ok(neutral::ListToolsResult::new(vec![
            neutral::Tool::new("echo", json!({"type": "object"})),
            neutral::Tool::new("slow", json!({"type": "object"})),
            neutral::Tool::new("boom", json!({"type": "object"})),
        ]))
    }

    async fn call_tool(
        &self,
        _ctx: &CallToolContext,
        params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        match params.name.as_str() {
            "echo" => Ok(neutral::CallToolResult::text("echoed")),
            "slow" => {
                tokio::time::sleep(Duration::from_secs(30)).await;
                Ok(neutral::CallToolResult::text("eventually"))
            }
            // A JSON-RPC protocol error during execution ⇒ `failed` task.
            "boom" => Err(McpError::internal("kaboom")),
            other => Ok(neutral::CallToolResult::error(format!("unknown: {other}"))),
        }
    }
}

fn dispatcher() -> VersionDispatcher<Tools> {
    VersionDispatcher::new(Tools, MethodRouter::new().with_tools()).with_extension(Arc::new(
        TasksExtension::new()
            .task_tools(["echo", "slow", "boom"])
            .poll_interval_ms(Some(10)),
    ))
}

fn draft_meta() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": { "extensions": { EXTENSION_ID: {} } },
    })
}

async fn call(svc: &mut VersionDispatcher<Tools>, req: JsonRpcRequest) -> Value {
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

async fn call_tool(svc: &mut VersionDispatcher<Tools>, id: i64, name: &str) -> Value {
    call(
        svc,
        JsonRpcRequest::new(
            id,
            "tools/call",
            Some(json!({ "name": name, "arguments": {}, "_meta": draft_meta() })),
        ),
    )
    .await
}

async fn get_task(svc: &mut VersionDispatcher<Tools>, id: i64, task_id: &str) -> Value {
    call(
        svc,
        JsonRpcRequest::new(
            id,
            "tasks/get",
            Some(json!({ "taskId": task_id, "_meta": draft_meta() })),
        ),
    )
    .await
}

/// Poll `tasks/get` until the task reaches a terminal status (or give up).
async fn poll_until_terminal(svc: &mut VersionDispatcher<Tools>, task_id: &str) -> Value {
    for i in 0..200 {
        let got = get_task(svc, 1000 + i, task_id).await;
        let status = got["result"]["status"].as_str().unwrap_or("");
        if matches!(status, "completed" | "failed" | "cancelled") {
            return got;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("task {task_id} never reached a terminal status");
}

#[tokio::test]
async fn tools_call_becomes_a_task_and_completes() {
    let mut svc = dispatcher();
    let created = call_tool(&mut svc, 1, "echo").await;
    let result = &created["result"];
    assert_eq!(result["resultType"], "task", "got {result}");
    assert_eq!(result["status"], "working");
    assert_eq!(result["pollIntervalMs"], 10);
    let task_id = result["taskId"].as_str().expect("taskId").to_owned();

    let terminal = poll_until_terminal(&mut svc, &task_id).await;
    assert_eq!(terminal["result"]["resultType"], "complete");
    assert_eq!(terminal["result"]["status"], "completed");
    assert_eq!(terminal["result"]["result"]["content"][0]["text"], "echoed");
    assert_eq!(terminal["result"]["result"]["isError"], false);
}

#[tokio::test]
async fn protocol_error_during_execution_is_a_failed_task() {
    let mut svc = dispatcher();
    let created = call_tool(&mut svc, 1, "boom").await;
    let task_id = created["result"]["taskId"]
        .as_str()
        .expect("taskId")
        .to_owned();

    let terminal = poll_until_terminal(&mut svc, &task_id).await;
    assert_eq!(terminal["result"]["status"], "failed");
    assert!(terminal["result"]["error"]["message"].as_str().is_some());
    assert!(terminal["result"].get("result").is_none());
}

#[tokio::test]
async fn cancel_transitions_to_cancelled() {
    let mut svc = dispatcher();
    let created = call_tool(&mut svc, 1, "slow").await;
    let task_id = created["result"]["taskId"]
        .as_str()
        .expect("taskId")
        .to_owned();

    // The task is still working (the handler sleeps 30s).
    let working = get_task(&mut svc, 2, &task_id).await;
    assert_eq!(working["result"]["status"], "working");

    // Cancel acks with an empty `complete` result, then the task reads back
    // `cancelled` immediately (cooperative; the spawned handler unwinds later).
    let cancelled_ack = call(
        &mut svc,
        JsonRpcRequest::new(
            3,
            "tasks/cancel",
            Some(json!({ "taskId": task_id, "_meta": draft_meta() })),
        ),
    )
    .await;
    assert_eq!(cancelled_ack["result"]["resultType"], "complete");
    assert!(cancelled_ack["error"].is_null());

    let after = get_task(&mut svc, 4, &task_id).await;
    assert_eq!(after["result"]["status"], "cancelled");
}

#[tokio::test]
async fn untasked_tool_runs_synchronously() {
    // Only `echo`/`slow`/`boom` are taskified; an unlisted tool runs normally.
    let mut svc = VersionDispatcher::new(Tools, MethodRouter::new().with_tools())
        .with_extension(Arc::new(TasksExtension::new().task_tools(["slow"])));
    let out = call_tool(&mut svc, 1, "echo").await;
    // A normal synchronous CallToolResult — not a task handle.
    assert_eq!(out["result"]["resultType"], "complete");
    assert_eq!(out["result"]["content"][0]["text"], "echoed");
    assert!(out["result"].get("taskId").is_none());
}

#[tokio::test]
async fn non_declaring_client_never_gets_a_task() {
    let mut svc = dispatcher();
    // No clientCapabilities ⇒ the server must run synchronously (SEP-2663).
    let meta = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
    });
    let out = call(
        &mut svc,
        JsonRpcRequest::new(
            1,
            "tools/call",
            Some(json!({ "name": "echo", "arguments": {}, "_meta": meta })),
        ),
    )
    .await;
    assert_eq!(out["result"]["resultType"], "complete");
    assert!(out["result"].get("taskId").is_none());
}

/// SEP-2663 graceful degradation at the RPC seam: with the registry at
/// capacity, an eligible `tools/call` declines taskification and runs
/// synchronously — no error, no task handle — until a slot frees up.
#[tokio::test]
async fn at_capacity_the_augmented_call_runs_synchronously() {
    let mut svc =
        VersionDispatcher::new(Tools, MethodRouter::new().with_tools()).with_extension(Arc::new(
            TasksExtension::new()
                .task_tools(["echo", "slow"])
                .capacity(1)
                .poll_interval_ms(Some(10)),
        ));

    // Fill the single slot with a long-running task…
    let created = call_tool(&mut svc, 1, "slow").await;
    assert_eq!(created["result"]["resultType"], "task", "got {created}");

    // …then the next eligible call answers inline.
    let out = call_tool(&mut svc, 2, "echo").await;
    assert_eq!(out["result"]["resultType"], "complete", "got {out}");
    assert_eq!(out["result"]["content"][0]["text"], "echoed");
    assert!(out["result"].get("taskId").is_none());
}
