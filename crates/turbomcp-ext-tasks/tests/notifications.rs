//! Phase 9c: `notifications/tasks` over `subscriptions/listen` (`taskIds`).
//!
//! Drives the dispatcher directly with a manually-registered `outbound` writer
//! (the same pattern the core subscription unit tests use) — no serve driver
//! needed to prove the push mechanism and the `-32021` capability gate.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::mpsc;
use tower::{Service, ServiceExt};
use turbomcp_core::{
    Implementation, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest, McpResult,
};
use turbomcp_ext_tasks::{EXTENSION_ID, TasksExtension};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, ListToolsContext, McpServerCore, MethodRouter, VersionDispatcher, WithTools,
};

#[derive(Clone)]
struct Slow;

impl McpServerCore for Slow {
    fn server_info(&self) -> Implementation {
        Implementation::new("slow", "1.0.0")
    }
}

impl WithTools for Slow {
    async fn list_tools(
        &self,
        _ctx: &ListToolsContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        Ok(neutral::ListToolsResult::new(vec![neutral::Tool::new(
            "slow",
            json!({"type": "object"}),
        )]))
    }

    async fn call_tool(
        &self,
        _ctx: &CallToolContext,
        _params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        tokio::time::sleep(Duration::from_secs(30)).await;
        Ok(neutral::CallToolResult::text("done"))
    }
}

fn dispatcher() -> VersionDispatcher<Slow> {
    VersionDispatcher::new(Slow, MethodRouter::new().with_tools())
        .with_extension(Arc::new(TasksExtension::new().task_tools(["slow"])))
}

/// Draft `_meta`: protocol version + connection id, optionally declaring the
/// tasks extension capability.
fn meta(conn: &str, declare: bool) -> Value {
    let mut m = serde_json::Map::new();
    m.insert(
        "io.modelcontextprotocol/protocolVersion".into(),
        json!("2026-07-28"),
    );
    m.insert("io.turbomcp.internal/connectionId".into(), json!(conn));
    if declare {
        m.insert(
            "io.modelcontextprotocol/clientCapabilities".into(),
            json!({ "extensions": { EXTENSION_ID: {} } }),
        );
    }
    Value::Object(m)
}

async fn call_some(svc: &mut VersionDispatcher<Slow>, req: JsonRpcRequest) -> JsonRpcMessage {
    svc.ready()
        .await
        .unwrap()
        .call(req.into())
        .await
        .unwrap()
        .expect("a response")
}

async fn call_none(
    svc: &mut VersionDispatcher<Slow>,
    req: JsonRpcRequest,
) -> Option<JsonRpcMessage> {
    svc.ready().await.unwrap().call(req.into()).await.unwrap()
}

fn as_notification(msg: JsonRpcMessage) -> JsonRpcNotification {
    match msg {
        JsonRpcMessage::Notification(n) => n,
        other => panic!("expected a notification, got {other:?}"),
    }
}

#[tokio::test]
async fn listen_then_cancel_pushes_notifications_tasks() {
    let conn = "notif-test-listen-conn";
    let mut svc = dispatcher();
    let (tx, mut rx) = mpsc::channel(16);
    let _guard = turbomcp_service::outbound::register(conn, tx);

    // 1. Create a (slow) task.
    let created = call_some(
        &mut svc,
        JsonRpcRequest::new(
            1,
            "tools/call",
            Some(json!({ "name": "slow", "arguments": {}, "_meta": meta(conn, true) })),
        ),
    )
    .await;
    let JsonRpcMessage::Response(r) = created else {
        panic!("expected response")
    };
    let task_id = r.result.unwrap()["taskId"]
        .as_str()
        .expect("taskId")
        .to_owned();

    // 2. Subscribe to the task's status via subscriptions/listen.
    let listen = call_none(
        &mut svc,
        JsonRpcRequest::new(
            2,
            "subscriptions/listen",
            Some(json!({
                "_meta": meta(conn, true),
                "notifications": { "taskIds": [task_id] },
            })),
        ),
    )
    .await;
    assert!(
        listen.is_none(),
        "subscriptions/listen has no JSON-RPC response"
    );

    // The first stream message is the acknowledgement, echoing our taskIds.
    let ack = as_notification(rx.recv().await.expect("ack"));
    assert_eq!(ack.method, "notifications/subscriptions/acknowledged");
    assert_eq!(
        ack.params.as_ref().unwrap()["notifications"]["taskIds"][0],
        task_id
    );

    // 3. Cancel the task → a `notifications/tasks` push for the cancelled state.
    let _ack = call_some(
        &mut svc,
        JsonRpcRequest::new(
            3,
            "tasks/cancel",
            Some(json!({ "taskId": task_id, "_meta": meta(conn, true) })),
        ),
    )
    .await;

    let pushed = as_notification(rx.recv().await.expect("notifications/tasks"));
    assert_eq!(pushed.method, "notifications/tasks");
    let params = pushed.params.as_ref().unwrap();
    assert_eq!(params["taskId"], task_id);
    assert_eq!(params["status"], "cancelled");
    // The notification is not a result — no `resultType`.
    assert!(params.get("resultType").is_none());
    // Every notification delivered on a listen stream MUST carry the
    // originating subscription's id verbatim (the listen request's id, 2).
    assert_eq!(
        params["_meta"]["io.modelcontextprotocol/subscriptionId"],
        json!(2)
    );
}

#[tokio::test]
async fn non_declaring_listen_with_task_ids_is_missing_capability() {
    let conn = "notif-test-undeclared-conn";
    let mut svc = dispatcher();
    let (tx, _rx) = mpsc::channel(16);
    let _guard = turbomcp_service::outbound::register(conn, tx);

    // subscriptions/listen with taskIds but WITHOUT declaring the extension.
    let out = call_some(
        &mut svc,
        JsonRpcRequest::new(
            1,
            "subscriptions/listen",
            Some(json!({
                "_meta": meta(conn, false),
                "notifications": { "taskIds": ["whatever"] },
            })),
        ),
    )
    .await;
    let JsonRpcMessage::Response(r) = out else {
        panic!("expected an error response")
    };
    let err = r.error.expect("missing-capability error");
    assert_eq!(
        err.code,
        turbomcp_core::codes::MISSING_REQUIRED_CLIENT_CAPABILITY
    );
    assert_eq!(
        err.data.unwrap()["requiredCapabilities"]["extensions"][EXTENSION_ID],
        json!({})
    );
}

#[tokio::test]
async fn listen_without_task_ids_is_unaffected_by_the_extension() {
    // A plain resources/tools listen (no taskIds) still works; the extension
    // returns NotApplicable and contributes nothing to the ack.
    let conn = "notif-test-plain-conn";
    let mut svc = dispatcher();
    let (tx, mut rx) = mpsc::channel(16);
    let _guard = turbomcp_service::outbound::register(conn, tx);

    let out = call_none(
        &mut svc,
        JsonRpcRequest::new(
            1,
            "subscriptions/listen",
            Some(json!({
                "_meta": meta(conn, true),
                "notifications": { "toolsListChanged": true },
            })),
        ),
    )
    .await;
    assert!(out.is_none());
    let ack = as_notification(rx.recv().await.expect("ack"));
    assert_eq!(
        ack.params.as_ref().unwrap()["notifications"]["toolsListChanged"],
        true
    );
    assert!(
        ack.params.as_ref().unwrap()["notifications"]
            .get("taskIds")
            .is_none()
    );
}
