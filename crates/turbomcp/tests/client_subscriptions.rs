//! The typed [`Client`]'s subscription surface against the **real** dispatcher,
//! on both wires.
//!
//! The client-crate tests script the server's frames; this one proves the two
//! halves actually agree — that the filter the client sends is the shape the
//! server parses, and that the acknowledgement the server emits is the one the
//! client correlates back to its waiting `listen`.

#![cfg(feature = "client")]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Map, Value};
use tokio::io::{BufReader, split};
use turbomcp::client::{Client, ClientBuilder, ClientHandler, ConnectMode};
use turbomcp::prelude::*;
use turbomcp::{LegacySessionAdapter, SerdeJsonCodec, serve};
use turbomcp_transport_stdio::LineTransport;

#[derive(Clone)]
struct Watched;

#[server(name = "watched", version = "1.0.0")]
impl Watched {
    /// A tool, so the server registers the tools capability.
    #[tool]
    async fn noop(&self) -> String {
        "ok".into()
    }

    /// A resource, so `resources` (and its `subscribe`) is advertised.
    #[resource("demo://watched")]
    async fn watched(&self) -> McpResult<String> {
        Ok("contents".into())
    }
}

/// Collects the notifications the client surfaces.
#[derive(Default)]
struct Spy {
    seen: Mutex<Vec<(String, Option<Value>)>>,
}

#[turbomcp::client::async_trait]
impl ClientHandler for Spy {
    async fn elicit(&self, _request: neutral::ElicitParams) -> neutral::ElicitOutcome {
        neutral::ElicitOutcome::new(neutral::ElicitAction::Decline, Map::new())
    }
    async fn on_notification(&self, method: String, params: Option<Value>) {
        self.seen.lock().unwrap().push((method, params));
    }
}

struct Shared(Arc<Spy>);

#[turbomcp::client::async_trait]
impl ClientHandler for Shared {
    async fn elicit(&self, request: neutral::ElicitParams) -> neutral::ElicitOutcome {
        self.0.elicit(request).await
    }
    async fn on_notification(&self, method: String, params: Option<Value>) {
        self.0.on_notification(method, params).await;
    }
}

async fn connect(mode: ConnectMode) -> (Client, Arc<Spy>) {
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let (s_rd, s_wr) = split(server_io);
    let transport = LineTransport::new(BufReader::new(s_rd), s_wr, SerdeJsonCodec);
    let service = LegacySessionAdapter::new(Watched.into_server().build());
    tokio::spawn(serve(transport, service));

    let spy = Arc::new(Spy::default());
    let (c_rd, c_wr) = split(client_io);
    let client = ClientBuilder::new("subscriber", "1.0.0")
        .with_connect_mode(mode)
        .with_handler(Shared(Arc::clone(&spy)))
        .connect(LineTransport::new(
            BufReader::new(c_rd),
            c_wr,
            SerdeJsonCodec,
        ))
        .await
        .expect("handshake");
    (client, spy)
}

/// The draft path: `listen` is answered by the server's acknowledgement, and
/// the agreed filter is intersected with what this server actually registered
/// — it has no prompts, so `promptsListChanged` must not come back.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn listen_negotiates_a_filter_with_the_real_server() {
    let (client, spy) = connect(ConnectMode::Modern).await;

    let agreed = client
        .listen(neutral::SubscriptionFilter::all_list_changed().with_resource("demo://watched"))
        .await
        .expect("the server acknowledges the subscription");

    assert_eq!(
        agreed.get("toolsListChanged"),
        Some(&Value::Bool(true)),
        "tools are registered, so this must be agreed: {agreed}"
    );
    assert_eq!(
        agreed.get("resourcesListChanged"),
        Some(&Value::Bool(true)),
        "resources are registered: {agreed}"
    );
    assert!(
        agreed
            .get("promptsListChanged")
            .is_none_or(|v| v == &Value::Bool(false)),
        "this server has no prompts, so it must not agree to them: {agreed}"
    );

    // The ack also reached the handler, stamped with the subscription id.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let seen = spy.seen.lock().unwrap();
    let ack = seen
        .iter()
        .find(|(m, _)| m == "notifications/subscriptions/acknowledged")
        .expect("ack surfaced to the handler");
    assert!(
        ack.1
            .as_ref()
            .and_then(|p| p.get("_meta"))
            .and_then(|m| m.get("io.modelcontextprotocol/subscriptionId"))
            .is_some(),
        "the ack carries its subscription id"
    );
}

/// `subscriptions/listen` is draft-only: the `2025-11-25` server answers
/// `-32601` and points the client at `resources/subscribe` instead.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn listen_is_method_not_found_on_the_legacy_wire() {
    let (client, _spy) = connect(ConnectMode::Legacy).await;

    let err = client
        .listen(neutral::SubscriptionFilter::all_list_changed())
        .await
        .expect_err("listen does not exist on 2025-11-25");
    assert_eq!(err.rpc_code(), Some(-32601));

    // The legacy equivalent does work on this wire.
    client
        .subscribe_resource("demo://watched")
        .await
        .expect("resources/subscribe is the legacy path");
    client
        .unsubscribe_resource("demo://watched")
        .await
        .expect("and unsubscribing works too");
}
