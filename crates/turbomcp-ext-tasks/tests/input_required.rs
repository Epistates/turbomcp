//! Phase 12b: in-execution `input_required` (SEP-2663 §Task Update Requests).
//!
//! A tool running *as a task* elicits mid-execution: the task flips to
//! `input_required` and surfaces the request via `tasks/get` `inputRequests`;
//! the client answers with `tasks/update` `inputResponses`; the handler
//! resumes and drives the task to `completed`. Cancellation while awaiting
//! input unwinds the handler instead of hanging it.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tower::{Service, ServiceExt};
use turbomcp_core::{Implementation, JsonRpcMessage, JsonRpcRequest, McpResult};
use turbomcp_ext_tasks::{EXTENSION_ID, TasksExtension};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, ListToolsContext, McpServerCore, MethodRouter, VersionDispatcher, WithTools,
};

#[derive(Clone)]
struct Guarded;

impl McpServerCore for Guarded {
    fn server_info(&self) -> Implementation {
        Implementation::new("guarded", "1.0.0")
    }
}

impl WithTools for Guarded {
    async fn list_tools(
        &self,
        _ctx: &ListToolsContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        Ok(neutral::ListToolsResult::new(vec![neutral::Tool::new(
            "guarded",
            json!({"type": "object"}),
        )]))
    }

    async fn call_tool(
        &self,
        ctx: &CallToolContext,
        _params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        // Mid-task client input: this blocks until `tasks/update` answers
        // (or the task is cancelled).
        let outcome = ctx
            .client
            .elicit(
                "confirm",
                neutral::ElicitParams::new(
                    "Proceed?",
                    json!({ "type": "object", "properties": { "ok": { "type": "boolean" } } }),
                ),
            )
            .await?;
        Ok(neutral::CallToolResult::text(if outcome.accepted() {
            "proceeded"
        } else {
            "aborted"
        }))
    }
}

fn dispatcher(extension: TasksExtension) -> VersionDispatcher<Guarded> {
    VersionDispatcher::with_config(
        Guarded,
        MethodRouter::new().with_tools(),
        turbomcp_server::ServerConfig::new()
            .with_extension(Arc::new(extension.task_tools(["guarded"]))),
    )
}

/// The client declares the tasks extension AND the elicitation capability —
/// the input-request capability gate (SEP-2322 MUST) applies mid-task too.
fn draft_meta(with_elicitation: bool) -> Value {
    let mut caps = json!({ "extensions": { EXTENSION_ID: {} } });
    if with_elicitation {
        caps["elicitation"] = json!({});
    }
    json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": caps,
    })
}

async fn call(svc: &mut VersionDispatcher<Guarded>, req: JsonRpcRequest) -> Value {
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

async fn get_task(svc: &mut VersionDispatcher<Guarded>, id: i64, task_id: &str) -> Value {
    call(
        svc,
        JsonRpcRequest::new(
            id,
            "tasks/get",
            Some(json!({ "taskId": task_id, "_meta": draft_meta(true) })),
        ),
    )
    .await
}

/// Poll until the task reports `status` (or give up).
async fn poll_until(svc: &mut VersionDispatcher<Guarded>, task_id: &str, status: &str) -> Value {
    for i in 0..200 {
        let got = get_task(svc, 1000 + i, task_id).await;
        if got["result"]["status"] == status {
            return got;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("task {task_id} never reached {status}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mid_task_elicitation_round_trips_via_tasks_update() {
    let mut svc = dispatcher(TasksExtension::new().poll_interval_ms(Some(10)));

    // 1. The call is taskified immediately (input comes later, mid-task).
    let created = call(
        &mut svc,
        JsonRpcRequest::new(
            1,
            "tools/call",
            Some(json!({ "name": "guarded", "arguments": {}, "_meta": draft_meta(true) })),
        ),
    )
    .await;
    assert_eq!(created["result"]["resultType"], "task");
    let task_id = created["result"]["taskId"].as_str().unwrap().to_owned();

    // 2. The handler hits `elicit` → the task flips to `input_required` and
    //    tasks/get surfaces the outstanding wire request.
    let waiting = poll_until(&mut svc, &task_id, "input_required").await;
    let request = &waiting["result"]["inputRequests"]["confirm"];
    assert_eq!(request["method"], "elicitation/create");
    assert_eq!(request["params"]["message"], "Proceed?");

    // 3. The client answers via tasks/update → empty `complete` ack.
    let ack = call(
        &mut svc,
        JsonRpcRequest::new(
            2,
            "tasks/update",
            Some(json!({
                "taskId": task_id,
                "inputResponses": {
                    "confirm": { "action": "accept", "content": { "ok": true } }
                },
                "_meta": draft_meta(true),
            })),
        ),
    )
    .await;
    assert_eq!(ack["result"]["resultType"], "complete");
    assert!(ack["error"].is_null());

    // 4. The handler resumed with the answer and completed the task.
    let done = poll_until(&mut svc, &task_id, "completed").await;
    assert_eq!(done["result"]["result"]["content"][0]["text"], "proceeded");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_while_awaiting_input_unwinds_the_handler() {
    let mut svc = dispatcher(TasksExtension::new().poll_interval_ms(Some(10)));

    let created = call(
        &mut svc,
        JsonRpcRequest::new(
            1,
            "tools/call",
            Some(json!({ "name": "guarded", "arguments": {}, "_meta": draft_meta(true) })),
        ),
    )
    .await;
    let task_id = created["result"]["taskId"].as_str().unwrap().to_owned();
    poll_until(&mut svc, &task_id, "input_required").await;

    let ack = call(
        &mut svc,
        JsonRpcRequest::new(
            2,
            "tasks/cancel",
            Some(json!({ "taskId": task_id, "_meta": draft_meta(true) })),
        ),
    )
    .await;
    assert_eq!(ack["result"]["resultType"], "complete");

    // The task is cancelled, stops advertising input requests, and the
    // handler unwound (a late completion never flips the status).
    let got = poll_until(&mut svc, &task_id, "cancelled").await;
    assert!(got["result"].get("inputRequests").is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn undeclared_elicitation_capability_fails_the_task() {
    let mut svc = dispatcher(TasksExtension::new().poll_interval_ms(Some(10)));

    // The client declared the tasks extension but NOT elicitation — the
    // capability gate (SEP-2322 MUST NOT) rejects the handler's elicit, which
    // surfaces as a `failed` task.
    let created = call(
        &mut svc,
        JsonRpcRequest::new(
            1,
            "tools/call",
            Some(json!({ "name": "guarded", "arguments": {}, "_meta": draft_meta(false) })),
        ),
    )
    .await;
    let task_id = created["result"]["taskId"].as_str().unwrap().to_owned();

    let failed = poll_until(&mut svc, &task_id, "failed").await;
    assert!(
        failed["result"]["error"]["message"]
            .as_str()
            .unwrap()
            .contains("elicitation"),
    );
}
