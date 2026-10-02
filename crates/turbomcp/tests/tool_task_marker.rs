//! Bucket-A A3: `#[tool(task)]` advertises per-tool `2025-11-25` task support.
//! A server that marks some tools reports `taskSupport: optional` for those and
//! `forbidden` for the rest (vs. the blanket `optional` when none are marked).
//! `#[tool(task = "required")]` reports `required`, and a call to it that
//! isn't a task is refused.

use serde_json::{Value, json};
use tower::{Service, ServiceExt};
use turbomcp::prelude::*;
use turbomcp::{JsonRpcMessage, JsonRpcRequest, LegacySessionAdapter, VersionDispatcher};

#[derive(Clone)]
struct Jobs;

#[server(name = "jobs", version = "1.0.0")]
impl Jobs {
    /// A long job that may run as a task.
    #[tool(description = "Slow job", task)]
    async fn slow(&self) -> String {
        "done".into()
    }

    /// A batch that only ever runs as a task.
    #[tool(description = "Batch job", task = "required")]
    async fn batch(&self) -> String {
        "batched".into()
    }

    /// A quick call that should not be taskified.
    #[tool(description = "Fast call")]
    async fn fast(&self) -> String {
        "ok".into()
    }
}

type Svc = LegacySessionAdapter<VersionDispatcher<Jobs>>;

async fn ok(svc: &mut Svc, req: JsonRpcRequest) -> Value {
    let out = svc
        .ready()
        .await
        .expect("ready")
        .call(req.into())
        .await
        .expect("call");
    let Some(JsonRpcMessage::Response(r)) = out else {
        panic!("expected response, got {out:?}")
    };
    assert!(r.error.is_none(), "unexpected error: {:?}", r.error);
    r.result.expect("result")
}

async fn initialize(svc: &mut Svc) {
    ok(
        svc,
        JsonRpcRequest::new(
            0,
            "initialize",
            Some(json!({
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": "c", "version": "1" },
            })),
        ),
    )
    .await;
}

#[tokio::test]
async fn tool_task_marker_sets_per_tool_task_support() {
    let mut svc = LegacySessionAdapter::new(Jobs.into_server().with_tasks().build());
    initialize(&mut svc).await;

    let list = ok(&mut svc, JsonRpcRequest::new(1, "tools/list", None)).await;
    let tools = list["tools"].as_array().expect("tools array");
    let support = |name: &str| {
        tools
            .iter()
            .find(|t| t["name"] == name)
            .and_then(|t| t["execution"]["taskSupport"].as_str())
            .unwrap_or("<none>")
            .to_string()
    };
    assert_eq!(support("slow"), "optional", "got {list}");
    assert_eq!(support("batch"), "required", "got {list}");
    assert_eq!(support("fast"), "forbidden", "got {list}");
}

#[tokio::test]
async fn a_required_tool_called_inline_is_method_not_found() {
    let mut svc = LegacySessionAdapter::new(Jobs.into_server().with_tasks().build());
    initialize(&mut svc).await;

    let out = svc
        .ready()
        .await
        .expect("ready")
        .call(JsonRpcRequest::new(2, "tools/call", Some(json!({ "name": "batch" }))).into())
        .await
        .expect("call");
    let Some(JsonRpcMessage::Response(r)) = out else {
        panic!("expected response, got {out:?}")
    };
    assert_eq!(r.error.expect("refused").code, -32601);
}
