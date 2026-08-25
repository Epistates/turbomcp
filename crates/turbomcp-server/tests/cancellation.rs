//! Cancellation wiring (Phase 6a): `notifications/cancelled` through the
//! `serve` driver fires the in-flight request's token, the handler's
//! `ctx.cancellation` observes it, and the response is suppressed — while
//! finished, unknown, and cross-connection requests are unaffected
//! (cancellation spec §Behavior Requirements).

use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::{Notify, mpsc};
use tower::{Service, ServiceExt};
use turbomcp_core::{
    Implementation, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse,
    McpResult, RequestId, meta,
};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, ListToolsContext, McpServerCore, MethodRouter, VersionDispatcher, WithTools,
};
use turbomcp_service::{ServeConfig, Transport, serve_with};

// ---- mock transport (same shape as the -service driver tests) -----------------

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

// ---- server under test ---------------------------------------------------------

/// `block` parks until its context token fires (signalling `cancelled` on the
/// way out via the test channel); `fast` answers immediately.
#[derive(Clone)]
struct Blocker {
    /// Receives `()` once the blocked handler has started.
    started: mpsc::UnboundedSender<()>,
    /// Receives `()` when the blocked handler's `ctx.cancellation` fires.
    cancelled: mpsc::UnboundedSender<()>,
}

impl McpServerCore for Blocker {
    fn server_info(&self) -> Implementation {
        Implementation::new("blocker", "0.1.0")
    }
}

impl WithTools for Blocker {
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
        if params.name == "block" {
            let _ = self.started.send(());
            // The dispatcher drops this future on cancellation; the spawned
            // watcher proves the *token itself* fired (so detached work a
            // handler spawned would also see it).
            let token = ctx.base.cancellation.clone();
            let cancelled = self.cancelled.clone();
            tokio::spawn(async move {
                token.cancelled().await;
                let _ = cancelled.send(());
            });
            // Park forever; only cancellation ends this call.
            Notify::new().notified().await;
            unreachable!("the blocked tool never completes");
        }
        Ok(neutral::CallToolResult::text("fast-result"))
    }
}

struct Harness {
    in_tx: mpsc::Sender<JsonRpcMessage>,
    out_rx: mpsc::UnboundedReceiver<JsonRpcMessage>,
    started: mpsc::UnboundedReceiver<()>,
    cancelled: mpsc::UnboundedReceiver<()>,
    driver: tokio::task::JoinHandle<Result<(), turbomcp_service::ProtocolError>>,
}

fn spawn_harness() -> Harness {
    let (started_tx, started) = mpsc::unbounded_channel();
    let (cancelled_tx, cancelled) = mpsc::unbounded_channel();
    let service = VersionDispatcher::new(
        Blocker {
            started: started_tx,
            cancelled: cancelled_tx,
        },
        MethodRouter::new().with_tools(),
    );
    let (in_tx, in_rx) = mpsc::channel(8);
    let (out_tx, out_rx) = mpsc::unbounded_channel();
    let transport = MockTransport {
        inbound: in_rx,
        outbound: out_tx,
    };
    let driver = tokio::spawn(serve_with(transport, service, ServeConfig::default()));
    Harness {
        in_tx,
        out_rx,
        started,
        cancelled,
        driver,
    }
}

fn call_tool(id: i64, name: &str) -> JsonRpcMessage {
    JsonRpcRequest::new(
        id,
        "tools/call",
        Some(json!({
            "name": name,
            "arguments": {},
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientCapabilities": {},
            },
        })),
    )
    .into()
}

fn cancelled_note(request_id: Value) -> JsonRpcMessage {
    JsonRpcNotification::new(
        "notifications/cancelled",
        Some(json!({ "requestId": request_id, "reason": "test says stop" })),
    )
    .into()
}

async fn recv_reply(rx: &mut mpsc::UnboundedReceiver<JsonRpcMessage>) -> JsonRpcResponse {
    let msg = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("a reply should arrive")
        .expect("outbound open");
    match msg {
        JsonRpcMessage::Response(r) => r,
        other => panic!("expected a response, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_request_fires_token_and_suppresses_response() {
    let mut h = spawn_harness();

    h.in_tx.send(call_tool(1, "block")).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), h.started.recv())
        .await
        .expect("blocked handler should start")
        .unwrap();

    h.in_tx.send(cancelled_note(json!(1))).await.unwrap();

    // The handler's own token observed the cancellation…
    tokio::time::timeout(Duration::from_secs(5), h.cancelled.recv())
        .await
        .expect("ctx.cancellation should fire")
        .unwrap();

    // …and the next outbound frame is the LATER request's reply: nothing was
    // ever written for the cancelled id 1 (the writer is ordered, so a
    // suppressed response simply never appears ahead of id 2).
    h.in_tx.send(call_tool(2, "fast")).await.unwrap();
    let reply = recv_reply(&mut h.out_rx).await;
    assert_eq!(reply.id, RequestId::from(2i64));
    assert!(reply.error.is_none());

    drop(h.in_tx);
    h.driver.await.unwrap().expect("clean shutdown on EOF");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_after_completion_and_unknown_ids_are_ignored() {
    let mut h = spawn_harness();

    // A request that already finished…
    h.in_tx.send(call_tool(1, "fast")).await.unwrap();
    let reply = recv_reply(&mut h.out_rx).await;
    assert_eq!(reply.id, RequestId::from(1i64));

    // …and one that never existed: both cancels are silently ignored.
    h.in_tx.send(cancelled_note(json!(1))).await.unwrap();
    h.in_tx.send(cancelled_note(json!(999))).await.unwrap();
    h.in_tx
        .send(cancelled_note(json!("not-seen")))
        .await
        .unwrap();

    h.in_tx.send(call_tool(2, "fast")).await.unwrap();
    let reply = recv_reply(&mut h.out_rx).await;
    assert_eq!(reply.id, RequestId::from(2i64));
    assert!(reply.error.is_none());

    drop(h.in_tx);
    h.driver.await.unwrap().expect("clean shutdown on EOF");
}

/// Without a driver in front (no connection identity), the dispatcher accepts
/// the notification but fires nothing — the cross-connection / sessionless
/// (HTTP) posture, where stream-close is the only cancellation signal.
#[tokio::test]
async fn cancelled_without_connection_identity_is_inert() {
    let (started_tx, _started) = mpsc::unbounded_channel();
    let (cancelled_tx, mut cancelled) = mpsc::unbounded_channel();
    let mut svc = VersionDispatcher::new(
        Blocker {
            started: started_tx,
            cancelled: cancelled_tx,
        },
        MethodRouter::new().with_tools(),
    );

    // Even a forged connection id can't reach the registry from here: nothing
    // was registered under it. (The real boundaries additionally strip the
    // forged key before dispatch — see the -service driver tests.)
    let mut note = cancelled_note(json!(1));
    meta::set_request_meta(&mut note, meta::internal::CONNECTION_ID, json!("conn-1"));
    let out = svc.ready().await.unwrap().call(note).await.unwrap();
    assert!(out.is_none(), "notifications never produce a reply");
    assert!(
        cancelled.try_recv().is_err(),
        "no token may fire without a matching in-flight registration"
    );
}
