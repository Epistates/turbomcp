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
use turbomcp::client::{
    Client, ClientBuilder, ConnectMode, ElicitationHandler, NotificationHandler,
};
use turbomcp::prelude::*;
use turbomcp::{LegacySessionAdapter, SerdeJsonCodec, serve};
use turbomcp_service::io::LineTransport;

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
impl ElicitationHandler for Spy {
    async fn elicit(&self, _request: neutral::ElicitParams) -> neutral::ElicitOutcome {
        neutral::ElicitOutcome::new(neutral::ElicitAction::Decline, Map::new())
    }
}

#[turbomcp::client::async_trait]
impl NotificationHandler for Spy {
    async fn on_notification(&self, method: String, params: Option<Value>) {
        self.seen.lock().unwrap().push((method, params));
    }
}

#[derive(Clone)]
struct Shared(Arc<Spy>);

#[turbomcp::client::async_trait]
impl ElicitationHandler for Shared {
    async fn elicit(&self, request: neutral::ElicitParams) -> neutral::ElicitOutcome {
        self.0.elicit(request).await
    }
}

#[turbomcp::client::async_trait]
impl NotificationHandler for Shared {
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
        .with_elicitation(Shared(Arc::clone(&spy)))
        .with_notifications(Shared(Arc::clone(&spy)))
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

    let subscription = client
        .listen(neutral::SubscriptionFilter::all_list_changed().with_resource("demo://watched"))
        .await
        .expect("the server acknowledges the subscription");
    let agreed = subscription.accepted();

    assert!(
        agreed.tools_list_changed,
        "tools are registered: {agreed:?}"
    );
    assert!(
        agreed.resources_list_changed,
        "resources are registered: {agreed:?}"
    );
    assert!(
        !agreed.prompts_list_changed,
        "this server has no prompts, so it must not agree to them: {agreed:?}"
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

/// `subscriptions/listen` arrived in 2026-07-28: on `2025-11-25` the client
/// refuses it locally and points at `resources/subscribe` instead.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn listen_is_refused_on_the_legacy_wire() {
    let (client, _spy) = connect(ConnectMode::Legacy).await;

    let err = client
        .listen(neutral::SubscriptionFilter::all_list_changed())
        .await
        .expect_err("listen does not exist on 2025-11-25");
    assert!(err.to_string().contains("subscribe_resource"), "{err}");

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

struct Running {
    client: Client,
    notifier: turbomcp::ServerNotifier,
    shutdown: turbomcp::CancellationToken,
    server: tokio::task::JoinHandle<Result<(), turbomcp::ProtocolError>>,
}

async fn run_modern() -> Running {
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let (s_rd, s_wr) = split(server_io);
    let server = turbomcp::Server::new(Watched.into_server().build());
    let notifier = server.notifier();
    let shutdown = turbomcp::CancellationToken::new();
    let pipe = turbomcp::Pipe::new(LineTransport::new(
        BufReader::new(s_rd),
        s_wr,
        SerdeJsonCodec,
    ))
    .config(turbomcp::ServeConfig {
        shutdown: shutdown.clone(),
        ..turbomcp::ServeConfig::default()
    });
    let server = tokio::spawn(server.serve(pipe));
    let (c_rd, c_wr) = split(client_io);
    let client = ClientBuilder::new("subscriber", "1.0.0")
        .with_connect_mode(ConnectMode::Modern)
        .connect(LineTransport::new(
            BufReader::new(c_rd),
            c_wr,
            SerdeJsonCodec,
        ))
        .await
        .expect("handshake");
    Running {
        client,
        notifier,
        shutdown,
        server,
    }
}

async fn next(
    sub: &mut turbomcp::client::Subscription,
) -> Option<turbomcp::client::SubscriptionEvent> {
    tokio::time::timeout(Duration::from_secs(5), sub.next())
        .await
        .expect("the subscription yields or ends")
}

/// Two subscriptions on one stdio connection each see their own
/// notifications: "clients MUST use this field to correlate notifications
/// with their originating subscription". The id used to be discarded.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_subscriptions_each_see_their_own() {
    use turbomcp::client::SubscriptionEvent;
    let running = run_modern().await;
    let mut tools = running
        .client
        .listen({
            let mut f = neutral::SubscriptionFilter::new();
            f.tools_list_changed = true;
            f
        })
        .await
        .unwrap();
    let mut watched = running
        .client
        .listen(neutral::SubscriptionFilter::new().with_resource("demo://watched"))
        .await
        .unwrap();
    assert_ne!(tools.id(), watched.id());

    running.notifier.tools_list_changed();
    running.notifier.resource_updated("demo://watched").await;

    assert_eq!(
        next(&mut tools).await,
        Some(SubscriptionEvent::ToolsListChanged)
    );
    assert_eq!(
        next(&mut watched).await,
        Some(SubscriptionEvent::ResourceUpdated {
            uri: "demo://watched".into()
        })
    );
    // Neither saw the other's.
    for sub in [&mut tools, &mut watched] {
        assert!(
            tokio::time::timeout(Duration::from_millis(100), sub.next())
                .await
                .is_err(),
            "{:?} saw a notification that was not its own",
            sub.id()
        );
    }
}

/// A server shutting down closes its subscriptions gracefully, and the
/// handle says so; the response that says it used to be dropped as unknown.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_graceful_close_ends_the_subscription_as_closed() {
    use turbomcp::client::SubscriptionEnd;
    let running = run_modern().await;
    let mut sub = running
        .client
        .listen(neutral::SubscriptionFilter::all_list_changed())
        .await
        .unwrap();
    running.shutdown.cancel();
    assert_eq!(next(&mut sub).await, None);
    assert_eq!(sub.end(), Some(SubscriptionEnd::Closed));
}

/// A connection that drops without the server closing the subscription is
/// "an unexpected disconnect, which the client MAY treat as a trigger to
/// reconnect": the handle tells the two apart.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dropped_connection_ends_the_subscription_as_lost() {
    use turbomcp::client::SubscriptionEnd;
    let running = run_modern().await;
    let mut sub = running
        .client
        .listen(neutral::SubscriptionFilter::all_list_changed())
        .await
        .unwrap();
    running.server.abort();
    assert_eq!(next(&mut sub).await, None);
    assert_eq!(sub.end(), Some(SubscriptionEnd::Lost));
}
