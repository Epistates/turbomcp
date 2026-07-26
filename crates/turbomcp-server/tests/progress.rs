//! Progress notifications: a request carrying `_meta.progressToken` gets
//! `notifications/progress` on its own connection before the final response —
//! on both protocol versions — and a token-less request gets none.

use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::mpsc;
use turbomcp_core::{Implementation, JsonRpcMessage, JsonRpcRequest, McpResult};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, ListToolsContext, McpServerCore, MethodRouter, VersionDispatcher, WithTools,
};
use turbomcp_service::{ServeConfig, Transport, serve_with};

struct MockTransport {
    inbound: mpsc::Receiver<JsonRpcMessage>,
    outbound: mpsc::UnboundedSender<JsonRpcMessage>,
}

impl Transport for MockTransport {
    type Error = std::io::Error;

    async fn send(&mut self, msg: JsonRpcMessage) -> Result<(), Self::Error> {
        self.outbound
            .send(msg)
            .map_err(|_| std::io::Error::other("outbound closed"))
    }

    async fn recv(&mut self) -> Result<Option<JsonRpcMessage>, Self::Error> {
        Ok(self.inbound.recv().await)
    }

    async fn close(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[derive(Clone)]
struct Stepper;

impl McpServerCore for Stepper {
    fn server_info(&self) -> Implementation {
        Implementation::new("stepper", "0.1.0")
    }
}

impl WithTools for Stepper {
    async fn list_tools(
        &self,
        _ctx: &ListToolsContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        Ok(neutral::ListToolsResult::new(vec![]))
    }

    async fn call_tool(
        &self,
        ctx: &CallToolContext,
        _params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        ctx.progress.report(1.0, Some(2.0), Some("halfway")).await;
        ctx.progress.report(2.0, Some(2.0), None).await;
        Ok(neutral::CallToolResult::text("done"))
    }
}

/// Drive one request through the real serve loop (which asserts connection
/// identity) and collect everything the server writes back.
async fn run_one(request: Value) -> Vec<Value> {
    let (in_tx, in_rx) = mpsc::channel(8);
    let (out_tx, mut out_rx) = mpsc::unbounded_channel();
    let dispatcher = VersionDispatcher::new(Stepper, MethodRouter::new().with_tools());
    let transport = MockTransport {
        inbound: in_rx,
        outbound: out_tx,
    };
    let task = tokio::spawn(serve_with(transport, dispatcher, ServeConfig::default()));

    in_tx
        .send(serde_json::from_value(request).expect("valid request"))
        .await
        .unwrap();
    drop(in_tx); // EOF after the one request → serve drains and exits

    task.await.unwrap().expect("serve loop exits cleanly");

    let mut frames = Vec::new();
    while let Ok(msg) = out_rx.try_recv() {
        frames.push(serde_json::to_value(&msg).unwrap());
    }
    frames
}

fn draft_call(meta_extra: Value) -> Value {
    let mut meta = json!({ "io.modelcontextprotocol/protocolVersion": "2026-07-28" });
    if let (Some(m), Some(extra)) = (meta.as_object_mut(), meta_extra.as_object()) {
        for (k, v) in extra {
            m.insert(k.clone(), v.clone());
        }
    }
    json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": "step", "arguments": {}, "_meta": meta }
    })
}

#[tokio::test]
async fn progress_flows_before_the_response_on_the_draft() {
    let frames = run_one(draft_call(json!({ "progressToken": "tok-1" }))).await;
    assert_eq!(
        frames.len(),
        3,
        "two progress notifications + the response; got: {frames:#?}"
    );

    assert_eq!(frames[0]["method"], "notifications/progress");
    assert_eq!(frames[0]["params"]["progressToken"], "tok-1");
    assert_eq!(frames[0]["params"]["progress"], 1.0);
    assert_eq!(frames[0]["params"]["total"], 2.0);
    assert_eq!(frames[0]["params"]["message"], "halfway");

    assert_eq!(frames[1]["method"], "notifications/progress");
    assert_eq!(frames[1]["params"]["progress"], 2.0);
    assert!(frames[1]["params"].get("message").is_none());

    assert_eq!(frames[2]["id"], 1);
    assert_eq!(frames[2]["result"]["content"][0]["text"], "done");
}

#[tokio::test]
async fn integer_tokens_echo_verbatim() {
    let frames = run_one(draft_call(json!({ "progressToken": 42 }))).await;
    assert_eq!(frames[0]["params"]["progressToken"], 42);
}

/// The progress spec pins the token to a string or an integer. Anything else
/// is ignored rather than echoed: a notification is required to carry the
/// token *verbatim*, so honouring an object/float/null would put a value on
/// the wire that no client can correlate — and the field is client-supplied,
/// so the shape is not the server's to assume. The call itself must still
/// succeed; a malformed token is not grounds to fail the tool.
#[tokio::test]
async fn a_token_that_is_not_a_string_or_integer_is_ignored() {
    for token in [
        json!({ "nested": true }),
        json!([1, 2]),
        json!(1.5),
        json!(null),
        json!(true),
        json!(""),
    ] {
        let frames = run_one(draft_call(json!({ "progressToken": token }))).await;
        let expected = if token.is_string() { 3 } else { 1 };
        assert_eq!(
            frames.len(),
            expected,
            "token {token} should{} have produced notifications: {frames:#?}",
            if expected == 1 { " not" } else { "" }
        );
        assert_eq!(
            frames.last().unwrap()["result"]["content"][0]["text"],
            "done",
            "the call must still succeed for token {token}"
        );
    }
}

#[tokio::test]
async fn no_token_means_no_notifications() {
    let frames = run_one(draft_call(json!({}))).await;
    assert_eq!(frames.len(), 1, "just the response");
    assert_eq!(frames[0]["result"]["content"][0]["text"], "done");
}

#[tokio::test]
async fn progress_flows_on_the_legacy_path_too() {
    let (in_tx, in_rx) = mpsc::channel(8);
    let (out_tx, mut out_rx) = mpsc::unbounded_channel();
    let dispatcher = VersionDispatcher::new(Stepper, MethodRouter::new().with_tools());
    let service = turbomcp_server::LegacySessionAdapter::new(dispatcher);
    let transport = MockTransport {
        inbound: in_rx,
        outbound: out_tx,
    };
    let task = tokio::spawn(serve_with(transport, service, ServeConfig::default()));

    let init = json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "legacy", "version": "1" },
        }
    });
    in_tx
        .send(serde_json::from_value(init).unwrap())
        .await
        .unwrap();
    in_tx
        .send(JsonRpcMessage::from(JsonRpcRequest::new(
            2,
            "tools/call",
            Some(json!({
                "name": "step", "arguments": {},
                "_meta": { "progressToken": "leg-1" },
            })),
        )))
        .await
        .unwrap();
    drop(in_tx);
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("serve loop ends")
        .unwrap()
        .expect("clean exit");

    let mut frames = Vec::new();
    while let Ok(msg) = out_rx.try_recv() {
        frames.push(serde_json::to_value(&msg).unwrap());
    }
    // initialize response, two progress notifications, call response.
    assert_eq!(frames.len(), 4);
    assert_eq!(frames[1]["method"], "notifications/progress");
    assert_eq!(frames[1]["params"]["progressToken"], "leg-1");
    assert_eq!(frames[2]["params"]["progress"], 2.0);
    assert_eq!(frames[3]["result"]["content"][0]["text"], "done");
}
