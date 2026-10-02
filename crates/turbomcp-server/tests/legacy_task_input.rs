//! Mid-task input on `2025-11-25` at the wire: with no stream to send on, a
//! task's input request waits as `input_required`; a `tasks/result` call
//! carries it, stamped with `io.modelcontextprotocol/related-task`; and the
//! client's response resumes the handler.

use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::mpsc;
use tower::{Service, ServiceExt};
use turbomcp_core::{
    Implementation, JsonRpcMessage, JsonRpcRequest, JsonRpcResponse, McpRequest, McpResult,
};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, LegacySessionAdapter, ListToolsContext, McpServerCore, ServerBuilder,
    VersionDispatcher, WithTools,
};
use turbomcp_service::Peer;

#[derive(Clone)]
struct Desk;

impl McpServerCore for Desk {
    fn server_info(&self) -> Implementation {
        Implementation::new("desk", "1.0.0")
    }
}

impl WithTools for Desk {
    async fn list_tools(
        &self,
        _ctx: &ListToolsContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        Ok(neutral::ListToolsResult::new(vec![neutral::Tool::new(
            "sign",
            json!({ "type": "object" }),
        )]))
    }

    async fn call_tool(
        &self,
        ctx: &CallToolContext,
        _params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        let outcome = ctx
            .client
            .elicit(
                "confirm",
                neutral::ElicitParams::new("Sign?", json!({ "type": "object", "properties": {} })),
            )
            .await?;
        Ok(neutral::CallToolResult::text(if outcome.accepted() {
            "signed"
        } else {
            "unsigned"
        }))
    }
}

type Svc = LegacySessionAdapter<VersionDispatcher<Desk>>;

async fn reply(svc: &mut Svc, req: impl Into<McpRequest>) -> JsonRpcResponse {
    match svc.ready().await.unwrap().call(req.into()).await.unwrap() {
        Some(JsonRpcMessage::Response(r)) => r,
        other => panic!("expected a response, got {other:?}"),
    }
}

async fn status(svc: &mut Svc, task_id: &str) -> Value {
    reply(
        svc,
        JsonRpcRequest::new(9, "tasks/get", Some(json!({ "taskId": task_id }))),
    )
    .await
    .result
    .expect("tasks/get")["status"]
        .clone()
}

#[tokio::test]
async fn a_tasks_result_stream_carries_the_input_request() {
    let mut svc =
        LegacySessionAdapter::new(ServerBuilder::new(Desk).with_tools().with_tasks().build());
    reply(
        &mut svc,
        JsonRpcRequest::new(
            0,
            "initialize",
            Some(json!({
                "protocolVersion": "2025-11-25",
                "capabilities": { "elicitation": {} },
                "clientInfo": { "name": "c", "version": "1" },
            })),
        ),
    )
    .await;

    // No stream reaches this client, so the request waits on the task.
    let created = reply(
        &mut svc,
        JsonRpcRequest::new(
            1,
            "tools/call",
            Some(json!({ "name": "sign", "arguments": {}, "task": {} })),
        ),
    )
    .await;
    let task_id = created.result.unwrap()["task"]["taskId"]
        .as_str()
        .unwrap()
        .to_owned();
    tokio::time::timeout(Duration::from_secs(5), async {
        while status(&mut svc, &task_id).await != "input_required" {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("the task reads input_required");

    // `tasks/result` opens a stream; the request goes out on it.
    let (tx, mut stream) = mpsc::channel(8);
    let result = tokio::spawn({
        let mut svc = svc.clone();
        let task_id = task_id.clone();
        let peer = Peer::new("result-stream", &tx);
        async move {
            let call = McpRequest::new(JsonRpcRequest::new(
                2,
                "tasks/result",
                Some(json!({ "taskId": task_id })),
            ))
            .with(peer);
            reply(&mut svc, call).await
        }
    });
    let Some(JsonRpcMessage::Request(ask)) =
        tokio::time::timeout(Duration::from_secs(5), stream.recv())
            .await
            .expect("the input request arrives")
    else {
        panic!("expected a request");
    };
    assert_eq!(ask.method, "elicitation/create");
    let params = ask.params.expect("params");
    assert_eq!(params["message"], "Sign?");
    assert_eq!(
        params["_meta"]["io.modelcontextprotocol/related-task"],
        json!({ "taskId": task_id }),
    );

    // The client's answer resumes the handler, and the task finishes.
    let answer = JsonRpcResponse::success(
        ask.id,
        json!({
            "action": "accept",
            "content": {},
            "_meta": { "io.modelcontextprotocol/related-task": { "taskId": task_id } },
        }),
    );
    assert!(
        svc.ready()
            .await
            .unwrap()
            .call(JsonRpcMessage::Response(answer).into())
            .await
            .unwrap()
            .is_none()
    );
    let done = tokio::time::timeout(Duration::from_secs(5), result)
        .await
        .expect("tasks/result answers")
        .unwrap();
    let done = done.result.expect("the tool's result");
    assert_eq!(done["content"][0]["text"], "signed");
    assert_eq!(
        done["_meta"]["io.modelcontextprotocol/related-task"]["taskId"],
        task_id
    );
    drop(tx);
}
