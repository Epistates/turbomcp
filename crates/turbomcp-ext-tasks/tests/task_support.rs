//! A tool's own `taskSupport` drives the 2026-07-28 Tasks extension: with no
//! `task_tools` policy, `Optional` and `Required` tools become tasks for a
//! client that declares the extension, and a `Required` tool refuses a client
//! that doesn't with `-32021` (SEP-2663), naming the extension to declare.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tower::{Service, ServiceExt};
use turbomcp_core::{Implementation, JsonRpcMessage, JsonRpcRequest, McpResult};
use turbomcp_ext_tasks::{EXTENSION_ID, TasksExtension};
use turbomcp_protocol::neutral::{self, TaskSupport};
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
        let schema = json!({"type": "object"});
        Ok(neutral::ListToolsResult::new(vec![
            neutral::Tool::new("plain", schema.clone()),
            neutral::Tool::new("forbidden", schema.clone())
                .with_task_support(TaskSupport::Forbidden),
            neutral::Tool::new("optional", schema.clone()).with_task_support(TaskSupport::Optional),
            neutral::Tool::new("required", schema.clone()).with_task_support(TaskSupport::Required),
            neutral::Tool::new("slow", schema).with_task_support(TaskSupport::Required),
        ]))
    }

    async fn call_tool(
        &self,
        _ctx: &CallToolContext,
        params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        if params.name == "slow" {
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
        Ok(neutral::CallToolResult::text(params.name))
    }
}

fn dispatcher(extension: TasksExtension) -> VersionDispatcher<Tools> {
    VersionDispatcher::new(Tools, MethodRouter::new().with_tools())
        .with_extension(Arc::new(extension.poll_interval_ms(Some(10))))
}

async fn call_tool(svc: &mut VersionDispatcher<Tools>, name: &str, declares: bool) -> Value {
    let extensions = if declares {
        json!({ EXTENSION_ID: {} })
    } else {
        json!({})
    };
    let meta = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": { "extensions": extensions },
    });
    let request = JsonRpcRequest::new(
        1,
        "tools/call",
        Some(json!({ "name": name, "arguments": {}, "_meta": meta })),
    );
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
    json!({ "result": r.result, "error": r.error })
}

#[tokio::test]
async fn without_a_policy_the_tools_own_task_support_decides() {
    let mut svc = dispatcher(TasksExtension::new());
    for name in ["optional", "required"] {
        let out = call_tool(&mut svc, name, true).await;
        assert_eq!(out["result"]["resultType"], "task", "{name}: {out}");
    }
    for name in ["plain", "forbidden"] {
        let out = call_tool(&mut svc, name, true).await;
        assert_eq!(out["result"]["resultType"], "complete", "{name}: {out}");
        assert_eq!(out["result"]["content"][0]["text"], name);
    }
}

#[tokio::test]
async fn an_explicit_policy_overrides_task_support() {
    let mut svc = dispatcher(TasksExtension::new().task_tools(["plain"]));
    let out = call_tool(&mut svc, "plain", true).await;
    assert_eq!(out["result"]["resultType"], "task", "{out}");
    let out = call_tool(&mut svc, "optional", true).await;
    assert_eq!(out["result"]["resultType"], "complete", "{out}");
    // …except over a tool that has no synchronous path.
    let out = call_tool(&mut svc, "required", true).await;
    assert_eq!(out["result"]["resultType"], "task", "{out}");
}

#[tokio::test]
async fn an_optional_tool_runs_inline_for_a_client_without_the_extension() {
    let mut svc = dispatcher(TasksExtension::new());
    let out = call_tool(&mut svc, "optional", false).await;
    assert_eq!(out["result"]["resultType"], "complete", "{out}");
    assert_eq!(out["result"]["content"][0]["text"], "optional");
}

#[tokio::test]
async fn a_required_tool_refuses_a_client_without_the_extension() {
    let mut svc = dispatcher(TasksExtension::new());
    let out = call_tool(&mut svc, "required", false).await;
    assert!(out["result"].is_null(), "{out}");
    assert_eq!(out["error"]["code"], -32021, "{out}");
    assert_eq!(
        out["error"]["data"]["requiredCapabilities"]["extensions"],
        json!({ EXTENSION_ID: {} })
    );
}

#[tokio::test]
async fn a_required_tool_the_registry_cannot_take_is_an_error_not_an_inline_run() {
    let mut svc = dispatcher(TasksExtension::new().capacity(1));
    // Hold the one slot with a task that keeps running.
    let held = call_tool(&mut svc, "slow", true).await;
    assert_eq!(held["result"]["resultType"], "task", "{held}");
    let out = call_tool(&mut svc, "required", true).await;
    assert!(out["result"].is_null(), "{out}");
    assert_eq!(out["error"]["code"], -32603, "{out}");
}
