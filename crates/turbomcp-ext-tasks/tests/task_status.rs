//! A taskified call's work reports on itself (SEP-2663 §Tasks: "Progress
//! descriptions for `working`"): `ctx.progress` reports become the task's
//! `statusMessage`, the only progress channel a task has, and `ctx.task`
//! sets it and the polling interval directly. Outside a task `ctx.task` is
//! inert.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::Semaphore;
use tower::{Service, ServiceExt};
use turbomcp_core::{Implementation, JsonRpcMessage, JsonRpcRequest, McpResult};
use turbomcp_ext_tasks::{EXTENSION_ID, TasksExtension};
use turbomcp_protocol::neutral::{self, TaskSupport};
use turbomcp_server::{
    CallToolContext, ListToolsContext, McpServerCore, MethodRouter, VersionDispatcher, WithTools,
};

/// Each tool reports, then holds the task in `working` until released.
#[derive(Clone)]
struct Reporter {
    gate: Arc<Semaphore>,
}

impl McpServerCore for Reporter {
    fn server_info(&self) -> Implementation {
        Implementation::new("reporter", "1.0.0")
    }
}

impl WithTools for Reporter {
    async fn list_tools(
        &self,
        _ctx: &ListToolsContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        let tool = |name| {
            neutral::Tool::new(name, json!({"type": "object"}))
                .with_task_support(TaskSupport::Optional)
        };
        Ok(neutral::ListToolsResult::new(vec![
            tool("progress"),
            tool("numeric"),
            tool("direct"),
            tool("whoami"),
        ]))
    }

    async fn call_tool(
        &self,
        ctx: &CallToolContext,
        params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        match params.name.as_str() {
            "progress" => ctx.progress.report(1.0, Some(4.0), Some("indexing")).await,
            "numeric" => ctx.progress.report(3.0, Some(4.0), None).await,
            "direct" => {
                ctx.task.set_status_message("waiting on the build").await;
                ctx.task.set_poll_interval(Duration::from_millis(250)).await;
            }
            "whoami" => {
                return Ok(neutral::CallToolResult::text(format!(
                    "task={} id={} heard={}",
                    ctx.task.is_task(),
                    ctx.task.id().is_some(),
                    ctx.progress.is_requested(),
                )));
            }
            _ => {}
        }
        let _permit = self.gate.acquire().await.expect("gate open");
        Ok(neutral::CallToolResult::text("done"))
    }
}

fn dispatcher(gate: &Arc<Semaphore>) -> VersionDispatcher<Reporter> {
    VersionDispatcher::new(
        Reporter {
            gate: Arc::clone(gate),
        },
        MethodRouter::new().with_tools(),
    )
    .with_extension(Arc::new(TasksExtension::new().poll_interval_ms(Some(10))))
}

fn meta(declares: bool) -> Value {
    let extensions = if declares {
        json!({ EXTENSION_ID: {} })
    } else {
        json!({})
    };
    json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": { "extensions": extensions },
    })
}

async fn call(svc: &mut VersionDispatcher<Reporter>, request: JsonRpcRequest) -> Value {
    let JsonRpcMessage::Response(r) = svc
        .ready()
        .await
        .unwrap()
        .call(request.into())
        .await
        .unwrap()
        .expect("a response")
    else {
        panic!("expected a response")
    };
    assert!(r.error.is_none(), "{:?}", r.error);
    r.result.expect("a result")
}

async fn start(svc: &mut VersionDispatcher<Reporter>, name: &str) -> String {
    let created = call(
        svc,
        JsonRpcRequest::new(
            1,
            "tools/call",
            Some(json!({ "name": name, "arguments": {}, "_meta": meta(true) })),
        ),
    )
    .await;
    assert_eq!(created["resultType"], "task", "{created}");
    created["taskId"].as_str().expect("taskId").to_owned()
}

/// Poll `tasks/get` until `done` holds (the work reports after the task is
/// created, on its own schedule).
async fn poll_until(
    svc: &mut VersionDispatcher<Reporter>,
    task_id: &str,
    done: impl Fn(&Value) -> bool,
) -> Value {
    let mut got = Value::Null;
    for i in 0..500 {
        got = call(
            svc,
            JsonRpcRequest::new(
                100 + i,
                "tasks/get",
                Some(json!({ "taskId": task_id, "_meta": meta(true) })),
            ),
        )
        .await;
        if done(&got) {
            return got;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    panic!("never saw it: {got}");
}

#[tokio::test]
async fn progress_reports_become_the_status_message() {
    let gate = Arc::new(Semaphore::new(0));
    let mut svc = dispatcher(&gate);

    let task_id = start(&mut svc, "progress").await;
    let got = poll_until(&mut svc, &task_id, |t| t["statusMessage"].is_string()).await;
    assert_eq!(got["status"], "working");
    assert_eq!(got["statusMessage"], "indexing");

    // Without a message, the numbers say it.
    let task_id = start(&mut svc, "numeric").await;
    let got = poll_until(&mut svc, &task_id, |t| t["statusMessage"].is_string()).await;
    assert_eq!(got["statusMessage"], "3/4");

    gate.add_permits(2);
}

#[tokio::test]
async fn ctx_task_sets_the_status_message_and_poll_interval() {
    let gate = Arc::new(Semaphore::new(0));
    let mut svc = dispatcher(&gate);
    let task_id = start(&mut svc, "direct").await;
    let got = poll_until(&mut svc, &task_id, |t| t["pollIntervalMs"] == 250).await;
    assert_eq!(got["statusMessage"], "waiting on the build");
    assert_ne!(
        got["lastUpdatedAt"], got["createdAt"],
        "an update bumps lastUpdatedAt"
    );

    // The terminal status replaces what the work said.
    gate.add_permits(1);
    let done = poll_until(&mut svc, &task_id, |t| t["status"] == "completed").await;
    assert!(done.get("statusMessage").is_none(), "{done}");
}

#[tokio::test]
async fn ctx_task_knows_whether_it_is_one() {
    let gate = Arc::new(Semaphore::new(0));
    let mut svc = dispatcher(&gate);

    let task_id = start(&mut svc, "whoami").await;
    let done = poll_until(&mut svc, &task_id, |t| t["status"] == "completed").await;
    assert_eq!(
        done["result"]["content"][0]["text"],
        "task=true id=true heard=true"
    );

    // A client without the extension gets the call inline, not as a task.
    let inline = call(
        &mut svc,
        JsonRpcRequest::new(
            2,
            "tools/call",
            Some(json!({ "name": "whoami", "arguments": {}, "_meta": meta(false) })),
        ),
    )
    .await;
    assert_eq!(
        inline["content"][0]["text"],
        "task=false id=false heard=false"
    );
}
