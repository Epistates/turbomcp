//! Phase 6 exit piece (stdio half): three concurrent `subscriptions/listen`
//! streams on one connection, multiplexed by `_meta.subscriptionId`
//! (subscriptions spec §Multiple Concurrent Subscriptions) — acknowledged
//! first, filters honored, teardown via `notifications/cancelled`.

use std::collections::BTreeSet;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::mpsc;
use turbomcp_core::{
    Implementation, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest, McpResult,
};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    ListResourcesContext, ListToolsContext, McpServerCore, MethodRouter, ReadResourceContext,
    ServerNotifier, VersionDispatcher, WithResources, WithTools,
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

/// Tools + resources, no prompts — so a `promptsListChanged` request is the
/// "server doesn't support it" case the ack must omit.
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
        _ctx: &turbomcp_server::CallToolContext,
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

fn listen(id: i64, notifications: Value) -> JsonRpcMessage {
    JsonRpcRequest::new(
        id,
        "subscriptions/listen",
        Some(json!({
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientCapabilities": {},
            },
            "notifications": notifications,
        })),
    )
    .into()
}

struct Harness {
    in_tx: mpsc::Sender<JsonRpcMessage>,
    out_rx: mpsc::UnboundedReceiver<JsonRpcMessage>,
    notifier: ServerNotifier,
    driver: tokio::task::JoinHandle<Result<(), turbomcp_service::ProtocolError>>,
}

fn spawn_harness() -> Harness {
    let service =
        VersionDispatcher::new(Watched, MethodRouter::new().with_tools().with_resources());
    let notifier = service.notifier();
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
        notifier,
        driver,
    }
}

async fn recv_notification(
    rx: &mut mpsc::UnboundedReceiver<JsonRpcMessage>,
) -> JsonRpcNotification {
    let msg = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("a notification should arrive")
        .expect("outbound open");
    match msg {
        JsonRpcMessage::Notification(n) => n,
        other => panic!("expected a notification, got {other:?}"),
    }
}

fn subscription_id(n: &JsonRpcNotification) -> String {
    // The id rides verbatim (string or number); these tests use numeric
    // listen ids, so render for comparison.
    n.params.as_ref().unwrap()["_meta"]["io.modelcontextprotocol/subscriptionId"]
        .as_i64()
        .expect("every stream message carries its numeric subscription id")
        .to_string()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn three_concurrent_subscriptions_demux_by_subscription_id() {
    let mut h = spawn_harness();

    // 1. Subscription 10: tools list + one resource URI.
    h.in_tx
        .send(listen(
            10,
            json!({ "toolsListChanged": true, "resourceSubscriptions": ["file://a"] }),
        ))
        .await
        .unwrap();
    let ack = recv_notification(&mut h.out_rx).await;
    assert_eq!(ack.method, "notifications/subscriptions/acknowledged");
    assert_eq!(subscription_id(&ack), "10");
    let agreed = &ack.params.as_ref().unwrap()["notifications"];
    assert_eq!(agreed["toolsListChanged"], true);
    assert_eq!(agreed["resourceSubscriptions"][0], "file://a");

    // 2. Subscription 20: tools, plus prompts — which this server doesn't
    //    serve, so the ack omits it (spec §Acknowledgment).
    h.in_tx
        .send(listen(
            20,
            json!({ "toolsListChanged": true, "promptsListChanged": true }),
        ))
        .await
        .unwrap();
    let ack = recv_notification(&mut h.out_rx).await;
    assert_eq!(subscription_id(&ack), "20");
    let agreed = ack.params.as_ref().unwrap()["notifications"]
        .as_object()
        .unwrap();
    assert_eq!(agreed["toolsListChanged"], true);
    assert!(
        !agreed.contains_key("promptsListChanged"),
        "unsupported notification types are omitted from the ack"
    );

    // 3. Subscription 30: a different resource URI only.
    h.in_tx
        .send(listen(30, json!({ "resourceSubscriptions": ["file://b"] })))
        .await
        .unwrap();
    let ack = recv_notification(&mut h.out_rx).await;
    assert_eq!(subscription_id(&ack), "30");

    // A resource update for file://a reaches ONLY subscription 10.
    h.notifier.resource_updated("file://a").await;
    let n = recv_notification(&mut h.out_rx).await;
    assert_eq!(n.method, "notifications/resources/updated");
    assert_eq!(subscription_id(&n), "10");
    assert_eq!(n.params.as_ref().unwrap()["uri"], "file://a");

    // A tools change fans out to 10 and 20 (order across subscriptions is
    // unspecified), never 30.
    h.notifier.tools_list_changed();
    let first = recv_notification(&mut h.out_rx).await;
    let second = recv_notification(&mut h.out_rx).await;
    assert_eq!(first.method, "notifications/tools/list_changed");
    assert_eq!(second.method, "notifications/tools/list_changed");
    let got: BTreeSet<String> = [subscription_id(&first), subscription_id(&second)].into();
    assert_eq!(got, BTreeSet::from(["10".to_owned(), "20".to_owned()]));

    // Cancelling the listen request id tears subscription 20 down (spec
    // §Cancellation, stdio); the next tools change reaches only 10.
    let cancel: JsonRpcMessage =
        JsonRpcNotification::new("notifications/cancelled", Some(json!({ "requestId": 20 })))
            .into();
    h.in_tx.send(cancel).await.unwrap();
    // The cancel must be processed before the publish snapshot is taken; the
    // events are on one ordered pipe but handlers run concurrently, so give
    // the no-op-fast cancel a beat.
    tokio::time::sleep(Duration::from_millis(100)).await;

    h.notifier.tools_list_changed();
    let n = recv_notification(&mut h.out_rx).await;
    assert_eq!(subscription_id(&n), "10");
    assert!(
        tokio::time::timeout(Duration::from_millis(200), h.out_rx.recv())
            .await
            .is_err(),
        "subscription 20 is gone; no second notification"
    );

    drop(h.in_tx);
    h.driver.await.unwrap().expect("clean shutdown on EOF");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn listen_burst_coalesces_list_changed() {
    let mut h = spawn_harness();

    h.in_tx
        .send(listen(1, json!({ "toolsListChanged": true })))
        .await
        .unwrap();
    let ack = recv_notification(&mut h.out_rx).await;
    assert_eq!(ack.method, "notifications/subscriptions/acknowledged");

    for _ in 0..10 {
        h.notifier.tools_list_changed();
    }
    let n = recv_notification(&mut h.out_rx).await;
    assert_eq!(n.method, "notifications/tools/list_changed");
    assert!(
        tokio::time::timeout(Duration::from_millis(300), h.out_rx.recv())
            .await
            .is_err(),
        "the burst coalesced into a single notification"
    );

    drop(h.in_tx);
    h.driver.await.unwrap().expect("clean shutdown on EOF");
}

#[tokio::test]
async fn listen_without_a_streaming_connection_is_rejected_in_band() {
    use tower::{Service, ServiceExt};
    // Calling the dispatcher directly (no serve driver, no writer) — the one
    // place listen DOES answer: an in-band -32600.
    let mut svc =
        VersionDispatcher::new(Watched, MethodRouter::new().with_tools().with_resources());
    let out = svc
        .ready()
        .await
        .unwrap()
        .call(listen(1, json!({ "toolsListChanged": true })))
        .await
        .unwrap()
        .expect("an error response");
    let JsonRpcMessage::Response(r) = out else {
        panic!("expected response");
    };
    assert_eq!(r.error.expect("rejected").code, -32600);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_listen_filter_is_invalid_params() {
    let mut h = spawn_harness();
    // `notifications` is required by the schema.
    let bad: JsonRpcMessage = JsonRpcRequest::new(
        1,
        "subscriptions/listen",
        Some(json!({
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientCapabilities": {},
            },
        })),
    )
    .into();
    h.in_tx.send(bad).await.unwrap();
    let msg = tokio::time::timeout(Duration::from_secs(5), h.out_rx.recv())
        .await
        .expect("an error response")
        .unwrap();
    let JsonRpcMessage::Response(r) = msg else {
        panic!("expected response");
    };
    assert_eq!(r.error.expect("invalid params").code, -32602);

    drop(h.in_tx);
    h.driver.await.unwrap().expect("clean shutdown on EOF");
}
