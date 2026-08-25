//! Legacy (`2025-11-25`) inline bidirectional elicitation over a byte pipe:
//! the handler's `ctx.client.elicit(…)` goes out as a real
//! `elicitation/create` request on the same connection, the handler blocks,
//! and the client's response routes back to it.

use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::mpsc;
use turbomcp_core::{
    Implementation, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse,
    McpResult, RequestId,
};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, LegacySessionAdapter, ListToolsContext, McpServerCore, MethodRouter,
    VersionDispatcher, WithTools,
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
struct Confirmer;

impl McpServerCore for Confirmer {
    fn server_info(&self) -> Implementation {
        Implementation::new("confirmer", "0.1.0")
    }
}

impl WithTools for Confirmer {
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
        params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        let outcome = ctx
            .client
            .elicit(
                "confirm",
                neutral::ElicitParams::new(
                    format!("Run {}?", params.name),
                    json!({ "type": "object", "properties": { "ok": { "type": "boolean" } } }),
                ),
            )
            .await?;
        Ok(neutral::CallToolResult::text(format!(
            "confirmed={}",
            outcome.accepted()
        )))
    }
}

struct Pipe {
    in_tx: mpsc::Sender<JsonRpcMessage>,
    out_rx: mpsc::UnboundedReceiver<JsonRpcMessage>,
    driver: tokio::task::JoinHandle<Result<(), turbomcp_service::ProtocolError>>,
}

fn spawn_pipe() -> Pipe {
    let service = LegacySessionAdapter::new(VersionDispatcher::new(
        Confirmer,
        MethodRouter::new().with_tools(),
    ));
    let (in_tx, in_rx) = mpsc::channel(8);
    let (out_tx, out_rx) = mpsc::unbounded_channel();
    let transport = MockTransport {
        inbound: in_rx,
        outbound: out_tx,
    };
    let driver = tokio::spawn(serve_with(transport, service, ServeConfig::default()));
    Pipe {
        in_tx,
        out_rx,
        driver,
    }
}

async fn recv(rx: &mut mpsc::UnboundedReceiver<JsonRpcMessage>) -> JsonRpcMessage {
    tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("a frame should arrive")
        .expect("pipe open")
}

async fn handshake(p: &mut Pipe, capabilities: Value) {
    let init = JsonRpcRequest::new(
        1,
        "initialize",
        Some(json!({
            "protocolVersion": "2025-11-25",
            "capabilities": capabilities,
            "clientInfo": { "name": "bidi-client", "version": "1" },
        })),
    );
    p.in_tx.send(init.into()).await.unwrap();
    let JsonRpcMessage::Response(r) = recv(&mut p.out_rx).await else {
        panic!("expected initialize response");
    };
    assert!(r.error.is_none());
    p.in_tx
        .send(JsonRpcNotification::new("notifications/initialized", None).into())
        .await
        .unwrap();
}

fn call_frame(id: i64) -> JsonRpcMessage {
    JsonRpcRequest::new(
        id,
        "tools/call",
        Some(json!({ "name": "deploy", "arguments": {} })),
    )
    .into()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn inline_elicitation_round_trip_on_one_pipe() {
    let mut p = spawn_pipe();
    handshake(&mut p, json!({ "elicitation": {} })).await;

    // The call blocks server-side; the next outbound frame is the server's
    // own elicitation/create REQUEST.
    p.in_tx.send(call_frame(2)).await.unwrap();
    let JsonRpcMessage::Request(elicit) = recv(&mut p.out_rx).await else {
        panic!("expected the server-initiated elicitation request");
    };
    assert_eq!(elicit.method, "elicitation/create");
    let params = elicit.params.as_ref().unwrap();
    assert_eq!(params["mode"], "form");
    assert_eq!(params["message"], "Run deploy?");
    assert!(
        matches!(&elicit.id, RequestId::String(s) if s.starts_with("srv-")),
        "server-minted ids are namespaced uuids"
    );

    // Answer it; the original call now completes.
    let answer = JsonRpcResponse::success(
        elicit.id.clone(),
        json!({ "action": "accept", "content": { "ok": true } }),
    );
    p.in_tx.send(answer.into()).await.unwrap();

    let JsonRpcMessage::Response(done) = recv(&mut p.out_rx).await else {
        panic!("expected the tools/call response");
    };
    assert_eq!(done.id, RequestId::from(2i64));
    let result = done.result.expect("tool result");
    assert_eq!(result["content"][0]["text"], "confirmed=true");

    drop(p.in_tx);
    p.driver.await.unwrap().expect("clean shutdown");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_error_answer_fails_the_handler_not_the_connection() {
    let mut p = spawn_pipe();
    handshake(&mut p, json!({ "elicitation": {} })).await;

    p.in_tx.send(call_frame(2)).await.unwrap();
    let JsonRpcMessage::Request(elicit) = recv(&mut p.out_rx).await else {
        panic!("expected the elicitation request");
    };
    let refusal = JsonRpcResponse::error(
        elicit.id.clone(),
        turbomcp_core::JsonRpcError {
            code: -32601,
            message: "no UI available".to_owned(),
            data: None,
        },
    );
    p.in_tx.send(refusal.into()).await.unwrap();

    // The handler's `?` propagated it — the call answers an error response
    // (trait-level errors are protocol errors; `isError` conversion is the
    // macro layer's job) — and the connection keeps serving.
    let JsonRpcMessage::Response(done) = recv(&mut p.out_rx).await else {
        panic!("expected the tools/call response");
    };
    let err = done.error.expect("handler error propagates");
    assert!(err.message.contains("no UI available"), "got: {err:?}");

    p.in_tx
        .send(JsonRpcRequest::new(3, "ping", None).into())
        .await
        .unwrap();
    let JsonRpcMessage::Response(pong) = recv(&mut p.out_rx).await else {
        panic!("expected pong");
    };
    assert_eq!(pong.id, RequestId::from(3i64));

    drop(p.in_tx);
    p.driver.await.unwrap().expect("clean shutdown");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn undeclared_capability_refuses_to_elicit() {
    let mut p = spawn_pipe();
    // The client declared NO elicitation capability.
    handshake(&mut p, json!({})).await;

    p.in_tx.send(call_frame(2)).await.unwrap();
    // No elicitation request may be sent; the call fails immediately with
    // `MissingRequiredClientCapabilityError`, whose `data` names the
    // capability as a `ClientCapabilities` object so the client can merge it
    // into its own declaration and retry.
    let JsonRpcMessage::Response(done) = recv(&mut p.out_rx).await else {
        panic!("expected the tools/call response, not a server request");
    };
    let err = done.error.expect("undeclared capability is an error");
    assert_eq!(
        err.code,
        turbomcp_core::codes::MISSING_REQUIRED_CLIENT_CAPABILITY
    );
    assert!(err.message.contains("elicitation"), "got: {err:?}");
    let data = err.data.expect("names the required capability");
    assert_eq!(data["requiredCapabilities"], json!({ "elicitation": {} }));

    drop(p.in_tx);
    p.driver.await.unwrap().expect("clean shutdown");
}
