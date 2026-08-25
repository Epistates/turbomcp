//! Core Tasks (2025-11-25) integration: task-augmented `tools/call`,
//! `tasks/get|list|cancel|result`, capability advertisement, and the
//! disabled-Tasks fallback — through the [`LegacySessionAdapter`] exactly as a
//! byte-pipe client would drive it.

use std::sync::Arc;

use serde_json::{Value, json};
use tokio::sync::Semaphore;
use tower::{Service, ServiceExt};
use turbomcp_core::{Implementation, JsonRpcMessage, JsonRpcRequest, McpResult};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, LegacySessionAdapter, ListToolsContext, McpServerCore, ServerBuilder,
    VersionDispatcher, WithTools,
};

/// A server whose `gated` tool blocks until the test releases a permit —
/// letting tests observe tasks in `working` status deterministically.
#[derive(Clone)]
struct Gated {
    gate: Arc<Semaphore>,
}

impl Gated {
    fn new() -> Self {
        Self {
            gate: Arc::new(Semaphore::new(0)),
        }
    }
}

impl McpServerCore for Gated {
    fn server_info(&self) -> Implementation {
        Implementation::new("gated", "1.0.0")
    }
}

impl WithTools for Gated {
    async fn list_tools(
        &self,
        _ctx: &ListToolsContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        Ok(neutral::ListToolsResult::new(vec![neutral::Tool::new(
            "gated",
            json!({"type": "object", "properties": {}}),
        )]))
    }

    async fn call_tool(
        &self,
        _ctx: &CallToolContext,
        params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        if params.name == "fails" {
            return Err(turbomcp_core::McpError::internal("tool exploded"));
        }
        let _permit = self.gate.acquire().await.expect("gate open");
        Ok(neutral::CallToolResult::text("gate passed"))
    }
}

type Svc = LegacySessionAdapter<VersionDispatcher<Gated>>;

fn tasked(server: &Gated) -> Svc {
    LegacySessionAdapter::new(
        ServerBuilder::new(server.clone())
            .with_tools()
            .with_tasks()
            .build(),
    )
}

async fn raw(svc: &mut Svc, req: JsonRpcRequest) -> turbomcp_core::JsonRpcResponse {
    let out = svc
        .ready()
        .await
        .expect("ready")
        .call(req.into())
        .await
        .expect("call");
    match out {
        Some(JsonRpcMessage::Response(r)) => r,
        other => panic!("expected response, got {other:?}"),
    }
}

async fn ok(svc: &mut Svc, req: JsonRpcRequest) -> Value {
    let r = raw(svc, req).await;
    assert!(r.error.is_none(), "unexpected error: {:?}", r.error);
    r.result.expect("result")
}

async fn initialize(svc: &mut Svc) -> Value {
    ok(
        svc,
        JsonRpcRequest::new(
            0,
            "initialize",
            Some(json!({
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": "task-client", "version": "1" },
            })),
        ),
    )
    .await
}

#[tokio::test]
async fn initialize_advertises_tasks_capability_and_tools_list_marks_optional() {
    let server = Gated::new();
    let mut svc = tasked(&server);
    let init = initialize(&mut svc).await;
    assert_eq!(init["capabilities"]["tasks"]["list"], json!({}));
    assert_eq!(init["capabilities"]["tasks"]["cancel"], json!({}));
    assert_eq!(
        init["capabilities"]["tasks"]["requests"]["tools"]["call"],
        json!({})
    );

    let list = ok(&mut svc, JsonRpcRequest::new(1, "tools/list", None)).await;
    assert_eq!(list["tools"][0]["execution"]["taskSupport"], "optional");
}

#[tokio::test]
async fn task_augmented_call_polls_to_completion() {
    let server = Gated::new();
    let mut svc = tasked(&server);
    let _ = initialize(&mut svc).await;

    // Augmented call answers immediately with a working task.
    let created = ok(
        &mut svc,
        JsonRpcRequest::new(
            1,
            "tools/call",
            Some(json!({ "name": "gated", "task": { "ttl": 60000 } })),
        ),
    )
    .await;
    let task_id = created["task"]["taskId"]
        .as_str()
        .expect("taskId")
        .to_owned();
    assert_eq!(created["task"]["status"], "working");
    assert_eq!(created["task"]["ttl"], 60000);
    assert!(created["task"]["createdAt"].as_str().unwrap().contains('T'));

    // Still working while the gate is shut.
    let got = ok(
        &mut svc,
        JsonRpcRequest::new(2, "tasks/get", Some(json!({ "taskId": task_id }))),
    )
    .await;
    assert_eq!(got["status"], "working");
    assert!(got["pollInterval"].as_i64().unwrap() > 0);

    // tasks/list sees it too.
    let listed = ok(&mut svc, JsonRpcRequest::new(3, "tasks/list", None)).await;
    assert_eq!(listed["tasks"][0]["taskId"], task_id.as_str());

    // tasks/result blocks until the tool finishes.
    let waiter = {
        let mut svc = svc.clone();
        let task_id = task_id.clone();
        tokio::spawn(async move {
            ok(
                &mut svc,
                JsonRpcRequest::new(4, "tasks/result", Some(json!({ "taskId": task_id }))),
            )
            .await
        })
    };
    tokio::task::yield_now().await;
    server.gate.add_permits(1);

    let result = waiter.await.expect("waiter");
    assert_eq!(result["content"][0]["text"], "gate passed");
    assert_eq!(result["isError"], false);

    let done = ok(
        &mut svc,
        JsonRpcRequest::new(5, "tasks/get", Some(json!({ "taskId": task_id }))),
    )
    .await;
    assert_eq!(done["status"], "completed");
}

#[tokio::test]
async fn cancel_transitions_and_terminal_cancel_is_rejected() {
    let server = Gated::new();
    let mut svc = tasked(&server);
    let _ = initialize(&mut svc).await;

    let created = ok(
        &mut svc,
        JsonRpcRequest::new(
            1,
            "tools/call",
            Some(json!({ "name": "gated", "task": {} })),
        ),
    )
    .await;
    let task_id = created["task"]["taskId"].as_str().unwrap().to_owned();

    let cancelled = ok(
        &mut svc,
        JsonRpcRequest::new(2, "tasks/cancel", Some(json!({ "taskId": task_id }))),
    )
    .await;
    assert_eq!(cancelled["status"], "cancelled");

    // tasks/result on a cancelled task: the underlying request never finished.
    let r = raw(
        &mut svc,
        JsonRpcRequest::new(3, "tasks/result", Some(json!({ "taskId": task_id }))),
    )
    .await;
    assert_eq!(r.error.expect("cancelled outcome").code, -32010);

    // Cancelling a terminal task → -32602 (spec §Error Handling).
    let r = raw(
        &mut svc,
        JsonRpcRequest::new(4, "tasks/cancel", Some(json!({ "taskId": task_id }))),
    )
    .await;
    assert_eq!(r.error.expect("terminal cancel").code, -32602);

    // Late completion must not resurrect the task.
    server.gate.add_permits(1);
    tokio::task::yield_now().await;
    let got = ok(
        &mut svc,
        JsonRpcRequest::new(5, "tasks/get", Some(json!({ "taskId": task_id }))),
    )
    .await;
    assert_eq!(got["status"], "cancelled");
}

#[tokio::test]
async fn failed_tool_reports_failed_status_and_error_result() {
    let server = Gated::new();
    let mut svc = tasked(&server);
    let _ = initialize(&mut svc).await;

    let created = ok(
        &mut svc,
        JsonRpcRequest::new(
            1,
            "tools/call",
            Some(json!({ "name": "fails", "task": {} })),
        ),
    )
    .await;
    let task_id = created["task"]["taskId"].as_str().unwrap().to_owned();

    // The result is the JSON-RPC error the underlying call would have raised.
    let r = raw(
        &mut svc,
        JsonRpcRequest::new(2, "tasks/result", Some(json!({ "taskId": task_id }))),
    )
    .await;
    let err = r.error.expect("failed task error");
    assert!(err.message.contains("tool exploded"));

    let got = ok(
        &mut svc,
        JsonRpcRequest::new(3, "tasks/get", Some(json!({ "taskId": task_id }))),
    )
    .await;
    assert_eq!(got["status"], "failed");
    assert!(
        got["statusMessage"]
            .as_str()
            .unwrap()
            .contains("tool exploded")
    );
}

#[tokio::test]
async fn unknown_task_id_is_invalid_params() {
    let server = Gated::new();
    let mut svc = tasked(&server);
    let _ = initialize(&mut svc).await;
    for method in ["tasks/get", "tasks/cancel", "tasks/result"] {
        let r = raw(
            &mut svc,
            JsonRpcRequest::new(1, method, Some(json!({ "taskId": "no-such-task" }))),
        )
        .await;
        assert_eq!(r.error.expect("unknown id").code, -32602, "{method}");
    }
}

#[tokio::test]
async fn tasks_disabled_ignores_augmentation_and_hides_methods() {
    let server = Gated::new();
    // No `.with_tasks()`.
    let mut svc =
        LegacySessionAdapter::new(ServerBuilder::new(server.clone()).with_tools().build());
    let init = initialize(&mut svc).await;
    assert!(init["capabilities"].get("tasks").is_none());

    // Augmentation is ignored; the call processes normally (spec §Task
    // Support and Handling). Open the gate first since the call is now
    // synchronous.
    server.gate.add_permits(1);
    let result = ok(
        &mut svc,
        JsonRpcRequest::new(
            1,
            "tools/call",
            Some(json!({ "name": "gated", "task": {} })),
        ),
    )
    .await;
    assert_eq!(result["content"][0]["text"], "gate passed");
    assert!(result.get("task").is_none());

    // tasks/* methods don't exist on this server.
    let r = raw(&mut svc, JsonRpcRequest::new(2, "tasks/list", None)).await;
    assert_eq!(r.error.expect("no tasks").code, -32601);
}

#[tokio::test]
async fn tasks_methods_absent_on_the_modern_path() {
    let server = Gated::new();
    let mut svc = tasked(&server);
    // Draft request, no initialize: tasks are an extension there (Phase 8).
    let r = raw(
        &mut svc,
        JsonRpcRequest::new(
            1,
            "tasks/list",
            Some(json!({
                "_meta": {
                    "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                    "io.modelcontextprotocol/clientCapabilities": {},
                }
            })),
        ),
    )
    .await;
    assert_eq!(r.error.expect("modern tasks hidden").code, -32601);
}

#[tokio::test]
async fn malformed_task_augmentation_is_invalid_params() {
    let server = Gated::new();
    let mut svc = tasked(&server);
    let _ = initialize(&mut svc).await;
    let r = raw(
        &mut svc,
        JsonRpcRequest::new(
            1,
            "tools/call",
            Some(json!({ "name": "gated", "task": "not-an-object" })),
        ),
    )
    .await;
    assert_eq!(r.error.expect("bad augmentation").code, -32602);
}
