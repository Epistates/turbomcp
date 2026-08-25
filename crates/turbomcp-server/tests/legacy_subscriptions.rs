//! Legacy (`2025-11-25`) subscriptions + the dual-stack exit piece: the same
//! dispatcher serves `resources/subscribe` on a legacy session and
//! `subscriptions/listen` on a draft connection, with one publish reaching
//! both — each on its own wire shape.

use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::mpsc;
use turbomcp_core::{
    Implementation, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest, McpResult,
};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, LegacySessionAdapter, ListResourcesContext, ListToolsContext, McpServerCore,
    MethodRouter, ReadResourceContext, ServerNotifier, VersionDispatcher, WithResources, WithTools,
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
struct Watched;

impl McpServerCore for Watched {
    fn server_info(&self) -> Implementation {
        Implementation::new("watched", "0.1.0")
    }
}

impl WithTools for Watched {
    async fn list_tools(
        &self,
        _ctx: &ListToolsContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        Ok(neutral::ListToolsResult::new(vec![]))
    }

    async fn call_tool(
        &self,
        _ctx: &CallToolContext,
        _params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        Ok(neutral::CallToolResult::text("ok"))
    }
}

impl WithResources for Watched {
    async fn list_resources(
        &self,
        _ctx: &ListResourcesContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListResourcesResult> {
        Ok(neutral::ListResourcesResult::new(vec![]))
    }

    async fn read_resource(
        &self,
        _ctx: &ReadResourceContext,
        params: neutral::ReadResourceParams,
    ) -> McpResult<neutral::ReadResourceResult> {
        Ok(neutral::ReadResourceResult::text(params.uri, "x"))
    }
}

struct Pipe {
    in_tx: mpsc::Sender<JsonRpcMessage>,
    out_rx: mpsc::UnboundedReceiver<JsonRpcMessage>,
    driver: tokio::task::JoinHandle<Result<(), turbomcp_service::ProtocolError>>,
}

/// One dispatcher; helper to attach any service stack to a fresh mock pipe.
fn pipe<Svc>(service: Svc) -> Pipe
where
    Svc: turbomcp_service::McpService + Clone + Send,
    Svc::Future: Send + 'static,
{
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

fn dispatcher() -> (VersionDispatcher<Watched>, ServerNotifier) {
    let d = VersionDispatcher::new(Watched, MethodRouter::new().with_tools().with_resources());
    let n = d.notifier();
    (d, n)
}

async fn recv(rx: &mut mpsc::UnboundedReceiver<JsonRpcMessage>) -> JsonRpcMessage {
    tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("a frame should arrive")
        .expect("pipe open")
}

async fn recv_result(rx: &mut mpsc::UnboundedReceiver<JsonRpcMessage>) -> Value {
    match recv(rx).await {
        JsonRpcMessage::Response(r) => {
            assert!(r.error.is_none(), "unexpected error: {:?}", r.error);
            r.result.expect("result")
        }
        other => panic!("expected a response, got {other:?}"),
    }
}

async fn recv_notification(
    rx: &mut mpsc::UnboundedReceiver<JsonRpcMessage>,
) -> JsonRpcNotification {
    match recv(rx).await {
        JsonRpcMessage::Notification(n) => n,
        other => panic!("expected a notification, got {other:?}"),
    }
}

fn initialize_frame(id: i64) -> JsonRpcMessage {
    JsonRpcRequest::new(
        id,
        "initialize",
        Some(json!({
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "legacy", "version": "1" },
        })),
    )
    .into()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn legacy_session_subscribes_and_receives_updates_and_list_changes() {
    let (dispatcher, notifier) = dispatcher();
    let mut p = pipe(LegacySessionAdapter::new(dispatcher));

    // Handshake; capabilities now advertise subscribe + listChanged.
    p.in_tx.send(initialize_frame(1)).await.unwrap();
    let init = recv_result(&mut p.out_rx).await;
    assert_eq!(init["capabilities"]["resources"]["subscribe"], true);
    assert_eq!(init["capabilities"]["resources"]["listChanged"], true);
    p.in_tx
        .send(JsonRpcNotification::new("notifications/initialized", None).into())
        .await
        .unwrap();

    // Subscribe to one URI (version-less frame: the adapter stamps the session).
    p.in_tx
        .send(
            JsonRpcRequest::new(2, "resources/subscribe", Some(json!({ "uri": "file://a" })))
                .into(),
        )
        .await
        .unwrap();
    let ok = recv_result(&mut p.out_rx).await;
    assert_eq!(ok, json!({}));

    // resources/updated arrives on the legacy wire — no subscriptionId.
    notifier.resource_updated("file://a").await;
    let n = recv_notification(&mut p.out_rx).await;
    assert_eq!(n.method, "notifications/resources/updated");
    let params = n.params.as_ref().unwrap();
    assert_eq!(params["uri"], "file://a");
    assert!(
        params.get("_meta").is_none(),
        "legacy notifications carry no subscriptionId meta"
    );

    // An update for a URI this session didn't subscribe to does NOT arrive…
    notifier.resource_updated("file://other").await;
    // …but list_changed reaches every legacy session without any subscribe.
    notifier.tools_list_changed();
    let n = recv_notification(&mut p.out_rx).await;
    assert_eq!(n.method, "notifications/tools/list_changed");

    // After unsubscribe, updates stop.
    p.in_tx
        .send(
            JsonRpcRequest::new(
                3,
                "resources/unsubscribe",
                Some(json!({ "uri": "file://a" })),
            )
            .into(),
        )
        .await
        .unwrap();
    let _ok = recv_result(&mut p.out_rx).await;
    notifier.resource_updated("file://a").await;
    assert!(
        tokio::time::timeout(Duration::from_millis(200), p.out_rx.recv())
            .await
            .is_err(),
        "unsubscribed URI produces nothing"
    );

    drop(p.in_tx);
    p.driver.await.unwrap().expect("clean shutdown");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn subscribe_without_session_or_on_modern_path_is_rejected() {
    let (dispatcher, _notifier) = dispatcher();
    let mut p = pipe(LegacySessionAdapter::new(dispatcher));

    // No initialize ran: declared-legacy subscribe → -32002 in-band.
    let req = JsonRpcRequest::new(
        1,
        "resources/subscribe",
        Some(json!({
            "uri": "file://a",
            "_meta": { "io.modelcontextprotocol/protocolVersion": "2025-11-25" },
        })),
    );
    p.in_tx.send(req.into()).await.unwrap();
    let JsonRpcMessage::Response(r) = recv(&mut p.out_rx).await else {
        panic!("expected response");
    };
    assert_eq!(r.error.expect("not initialized").code, -32002);

    // The draft path has no resources/subscribe (-32601): it uses
    // subscriptions/listen.
    let req = JsonRpcRequest::new(
        2,
        "resources/subscribe",
        Some(json!({
            "uri": "file://a",
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientCapabilities": {},
            },
        })),
    );
    p.in_tx.send(req.into()).await.unwrap();
    let JsonRpcMessage::Response(r) = recv(&mut p.out_rx).await else {
        panic!("expected response");
    };
    assert_eq!(r.error.expect("modern path").code, -32601);

    drop(p.in_tx);
    p.driver.await.unwrap().expect("clean shutdown");
}

/// THE dual-stack subscription exit test: one dispatcher, two live
/// connections — a legacy session subscribed via `resources/subscribe` and a
/// draft connection subscribed via `subscriptions/listen`. One publish, two
/// wire shapes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_publish_reaches_legacy_and_draft_subscribers_on_their_own_wires() {
    let (dispatcher, notifier) = dispatcher();
    let mut legacy = pipe(LegacySessionAdapter::new(dispatcher.clone()));
    let mut draft = pipe(dispatcher);

    // Legacy connection: handshake + subscribe.
    legacy.in_tx.send(initialize_frame(1)).await.unwrap();
    let _init = recv_result(&mut legacy.out_rx).await;
    legacy
        .in_tx
        .send(
            JsonRpcRequest::new(2, "resources/subscribe", Some(json!({ "uri": "file://a" })))
                .into(),
        )
        .await
        .unwrap();
    let _ok = recv_result(&mut legacy.out_rx).await;

    // Draft connection: listen with a matching resource filter.
    draft
        .in_tx
        .send(
            JsonRpcRequest::new(
                7,
                "subscriptions/listen",
                Some(json!({
                        "_meta": {
                    "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                    "io.modelcontextprotocol/clientCapabilities": {},
                },
                        "notifications": { "resourceSubscriptions": ["file://a"] },
                    })),
            )
            .into(),
        )
        .await
        .unwrap();
    let ack = recv_notification(&mut draft.out_rx).await;
    assert_eq!(ack.method, "notifications/subscriptions/acknowledged");

    // ONE publish.
    notifier.resource_updated("file://a").await;

    // Draft wire: subscriptionId stamped.
    let n = recv_notification(&mut draft.out_rx).await;
    assert_eq!(n.method, "notifications/resources/updated");
    let params = n.params.as_ref().unwrap();
    assert_eq!(params["uri"], "file://a");
    // The subscription id is the listen request's JSON-RPC id VERBATIM — a
    // number here, never a stringified copy.
    assert_eq!(
        params["_meta"]["io.modelcontextprotocol/subscriptionId"],
        serde_json::json!(7)
    );

    // Legacy wire: bare uri, no meta.
    let n = recv_notification(&mut legacy.out_rx).await;
    assert_eq!(n.method, "notifications/resources/updated");
    let params = n.params.as_ref().unwrap();
    assert_eq!(params["uri"], "file://a");
    assert!(params.get("_meta").is_none());

    drop(legacy.in_tx);
    drop(draft.in_tx);
    legacy.driver.await.unwrap().expect("clean shutdown");
    draft.driver.await.unwrap().expect("clean shutdown");
}
