//! The client's subscription, logging, and task-listing surface — the methods
//! the server has always implemented but the client couldn't reach.
//!
//! `subscriptions/listen` is the interesting one: it is the only request whose
//! success is signalled by a *notification* (`acknowledged`) rather than a
//! JSON-RPC response, so the connection actor has to correlate the ack back to
//! the waiting request or the call would sit until its timeout.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{Map, Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, split};
use turbomcp_client::{
    Client, ClientBuilder, ClientError, ElicitationHandler, NotificationHandler,
};
use turbomcp_core::LogLevel;
use turbomcp_core::codec::SerdeJsonCodec;
use turbomcp_protocol::neutral;
use turbomcp_service::io::LineTransport;

/// A scripted server that answers with *raw frames*: each returned value is
/// written verbatim, so a handler can reply with a notification (or nothing)
/// instead of a response.
fn spawn_raw<F>(server_io: tokio::io::DuplexStream, mut frames_for: F)
where
    F: FnMut(&str, &Value) -> Vec<Value> + Send + 'static,
{
    tokio::spawn(async move {
        let (rd, mut wr) = split(server_io);
        let mut lines = BufReader::new(rd).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let frame: Value = serde_json::from_str(&line).expect("client sends valid json");
            let Some(method) = frame.get("method").and_then(Value::as_str) else {
                continue;
            };
            for out in frames_for(method, &frame) {
                wr.write_all(format!("{out}\n").as_bytes()).await.unwrap();
            }
        }
    });
}

/// A `{"jsonrpc":"2.0","id":…,"result":…}` frame echoing the request's id.
fn result_for(frame: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": frame.get("id").cloned().unwrap_or(Value::Null), "result": result })
}

fn discover_result() -> Value {
    json!({
        "capabilities": { "tools": {}, "resources": { "subscribe": true }, "logging": {} },
        "supportedVersions": ["2026-07-28"],
        "resultType": "complete", "cacheScope": "private", "ttlMs": 0
    })
}

/// Records every notification the client surfaced to the handler.
#[derive(Default)]
struct Spy {
    seen: Mutex<Vec<String>>,
}

#[async_trait]
impl ElicitationHandler for Spy {
    async fn elicit(&self, _request: neutral::ElicitParams) -> neutral::ElicitOutcome {
        neutral::ElicitOutcome::new(neutral::ElicitAction::Decline, Map::new())
    }
}

#[async_trait]
impl NotificationHandler for Spy {
    async fn on_notification(&self, method: String, _params: Option<Value>) {
        self.seen.lock().unwrap().push(method);
    }
}

async fn connect<F>(handler: Option<Arc<Spy>>, frames_for: F) -> Client
where
    F: FnMut(&str, &Value) -> Vec<Value> + Send + 'static,
{
    connect_with(
        ClientBuilder::new("subscriber", "1.0.0"),
        handler,
        frames_for,
    )
    .await
}

/// [`connect`], from a caller-configured builder.
async fn connect_with<F>(
    builder: ClientBuilder,
    handler: Option<Arc<Spy>>,
    mut frames_for: F,
) -> Client
where
    F: FnMut(&str, &Value) -> Vec<Value> + Send + 'static,
{
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    spawn_raw(server_io, move |method, frame| match method {
        "server/discover" => vec![result_for(frame, discover_result())],
        other => frames_for(other, frame),
    });
    finish(builder, client_io, handler).await
}

/// A scripted `2025-11-25` server: `initialize` is answered here and
/// `notifications/initialized` swallowed; everything else goes to `frames_for`.
async fn connect_legacy<F>(builder: ClientBuilder, mut frames_for: F) -> Client
where
    F: FnMut(&str, &Value) -> Vec<Value> + Send + 'static,
{
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    spawn_raw(server_io, move |method, frame| match method {
        "initialize" => vec![result_for(
            frame,
            json!({
                "protocolVersion": "2025-11-25",
                "capabilities": {
                    "tools": {}, "resources": { "subscribe": true }, "logging": {},
                    "tasks": { "list": {}, "cancel": {}, "requests": { "tools": { "call": {} } } }
                },
                "serverInfo": { "name": "legacy", "version": "1.0.0" }
            }),
        )],
        "notifications/initialized" => vec![],
        other => frames_for(other, frame),
    });
    finish(
        builder.with_connect_mode(turbomcp_client::ConnectMode::Legacy),
        client_io,
        None,
    )
    .await
}

async fn finish(
    builder: ClientBuilder,
    client_io: tokio::io::DuplexStream,
    handler: Option<Arc<Spy>>,
) -> Client {
    let (rd, wr) = split(client_io);
    let mut builder = builder.with_response_cache(false);
    if let Some(handler) = handler {
        let shared = HandlerArc(handler);
        builder = builder
            .with_elicitation(shared.clone())
            .with_notifications(shared);
    }
    builder
        .connect(LineTransport::new(BufReader::new(rd), wr, SerdeJsonCodec))
        .await
        .expect("handshake")
}

/// Handlers are consumed by value; this shares one `Spy` with the test.
#[derive(Clone)]
struct HandlerArc(Arc<Spy>);

#[async_trait]
impl ElicitationHandler for HandlerArc {
    async fn elicit(&self, request: neutral::ElicitParams) -> neutral::ElicitOutcome {
        self.0.elicit(request).await
    }
}

#[async_trait]
impl NotificationHandler for HandlerArc {
    async fn on_notification(&self, method: String, params: Option<Value>) {
        self.0.on_notification(method, params).await;
    }
}

/// The ack is the listen request's answer: it arrives as a notification naming
/// the request id in `_meta`, and the waiting call must resolve on it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn listen_resolves_on_the_acknowledgement_notification() {
    let spy = Arc::new(Spy::default());
    let client = connect(Some(Arc::clone(&spy)), |method, frame| match method {
        "subscriptions/listen" => {
            // No response — the ack notification is the answer, carrying the
            // listen request's id verbatim.
            let id = frame.get("id").cloned().unwrap_or(Value::Null);
            vec![json!({
                "jsonrpc": "2.0",
                "method": "notifications/subscriptions/acknowledged",
                "params": {
                    "_meta": { "io.modelcontextprotocol/subscriptionId": id },
                    // The server agreed to less than was asked for.
                    "notifications": { "toolsListChanged": true }
                }
            })]
        }
        other => panic!("unexpected method {other}"),
    })
    .await;

    let subscription = client
        .listen(
            neutral::SubscriptionFilter::all_list_changed().with_resource("file:///watched.txt"),
        )
        .await
        .expect("the acknowledgement resolves the listen");

    // The agreed subset is what the server said, not what we requested.
    let mut expected = neutral::SubscriptionFilter::new();
    expected.tools_list_changed = true;
    assert_eq!(*subscription.accepted(), expected);

    // The ack still reaches the handler like any other notification.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(
        spy.seen
            .lock()
            .unwrap()
            .iter()
            .any(|m| m == "notifications/subscriptions/acknowledged"),
        "the ack must also surface to on_notification"
    );
}

/// The filter goes out in the draft's wire shape: requested flags are `true`,
/// unrequested ones are absent (not `false`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn listen_sends_only_the_requested_filter_flags() {
    let sent = Arc::new(Mutex::new(Value::Null));
    let captured = Arc::clone(&sent);
    let client = connect(None, move |method, frame| match method {
        "subscriptions/listen" => {
            *captured.lock().unwrap() = frame["params"]["notifications"].clone();
            let id = frame.get("id").cloned().unwrap_or(Value::Null);
            vec![json!({
                "jsonrpc": "2.0",
                "method": "notifications/subscriptions/acknowledged",
                "params": {
                    "_meta": { "io.modelcontextprotocol/subscriptionId": id },
                    "notifications": {}
                }
            })]
        }
        other => panic!("unexpected method {other}"),
    })
    .await;

    let filter = neutral::SubscriptionFilter::new().with_resource("file:///a");
    client.listen(filter).await.expect("listen");

    let sent = sent.lock().unwrap().clone();
    assert_eq!(sent["resourceSubscriptions"], json!(["file:///a"]));
    assert!(
        sent.get("toolsListChanged").is_none(),
        "an unrequested flag must be absent, not false: {sent}"
    );
}

/// A listen the server rejects answers in band, like any other request.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rejected_listen_surfaces_the_error() {
    let client = connect(None, |method, frame| match method {
        "subscriptions/listen" => vec![json!({
            "jsonrpc": "2.0",
            "id": frame.get("id").cloned().unwrap_or(Value::Null),
            "error": { "code": -32602, "message": "bad filter" }
        })],
        other => panic!("unexpected method {other}"),
    })
    .await;

    let err = client
        .listen(neutral::SubscriptionFilter::all_list_changed())
        .await
        .expect_err("a rejected listen must fail");
    assert_eq!(err.rpc_code(), Some(-32602));
}

/// An acknowledgement naming an id nobody is waiting on must not disturb the
/// connection — it is just a notification.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unmatched_acknowledgement_is_harmless() {
    let spy = Arc::new(Spy::default());
    let client = connect(Some(Arc::clone(&spy)), |method, frame| match method {
        "tools/list" => vec![
            json!({
                "jsonrpc": "2.0",
                "method": "notifications/subscriptions/acknowledged",
                "params": {
                    "_meta": { "io.modelcontextprotocol/subscriptionId": 9999 },
                    "notifications": {}
                }
            }),
            result_for(
                frame,
                json!({
                    "tools": [], "resultType": "complete",
                    "cacheScope": "private", "ttlMs": 0
                }),
            ),
        ],
        other => panic!("unexpected method {other}"),
    })
    .await;

    // The stray ack doesn't steal or break this request.
    let tools = client
        .list_tools(None)
        .await
        .expect("list_tools still works");
    assert!(tools.tools.is_empty());
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(
        spy.seen
            .lock()
            .unwrap()
            .iter()
            .any(|m| m.contains("acknowledged"))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn subscribe_and_unsubscribe_and_set_level_round_trip() {
    let calls = Arc::new(Mutex::new(Vec::<(String, Value)>::new()));
    let seen = Arc::clone(&calls);
    let client = connect_legacy(
        ClientBuilder::new("subscriber", "1.0.0"),
        move |method, frame| {
            seen.lock()
                .unwrap()
                .push((method.to_owned(), frame["params"].clone()));
            vec![result_for(frame, json!({}))]
        },
    )
    .await;

    client
        .subscribe_resource("file:///watched.txt")
        .await
        .expect("subscribe");
    client
        .unsubscribe_resource("file:///watched.txt")
        .await
        .expect("unsubscribe");
    #[expect(
        deprecated,
        reason = "SEP-2577 deprecates logging; still live on 2025-11-25"
    )]
    client
        .set_level(LogLevel::Warning)
        .await
        .expect("set_level");

    let calls = calls.lock().unwrap();
    assert_eq!(calls[0].0, "resources/subscribe");
    assert_eq!(calls[0].1["uri"], "file:///watched.txt");
    assert_eq!(calls[1].0, "resources/unsubscribe");
    assert_eq!(calls[2].0, "logging/setLevel");
    // The wire form is the spec's lowercase severity name.
    assert_eq!(calls[2].1["level"], "warning");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn list_all_tasks_follows_pages() {
    let client = connect_legacy(
        ClientBuilder::new("subscriber", "1.0.0"),
        |method, frame| match method {
            "tasks/list" => {
                let first = frame["params"].get("cursor").is_none();
                let result = if first {
                    json!({ "tasks": [{ "taskId": "t1" }], "nextCursor": "p2" })
                } else {
                    json!({ "tasks": [{ "taskId": "t2" }] })
                };
                vec![result_for(frame, result)]
            }
            other => panic!("unexpected method {other}"),
        },
    )
    .await;

    let tasks = client.list_all_tasks().await.expect("tasks paginate");
    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0]["taskId"], "t1");
    assert_eq!(tasks[1]["taskId"], "t2");

    // The single-page form is still available for manual cursor control.
    let page = client.task_list(None).await.expect("one page");
    assert_eq!(page["nextCursor"], "p2");
}

/// A server with no Tasks support answers `-32601`; that must surface, not be
/// mistaken for an empty task list.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn task_list_propagates_method_not_found() {
    let client = connect_legacy(
        ClientBuilder::new("subscriber", "1.0.0"),
        |method, frame| match method {
            "tasks/list" => vec![json!({
                "jsonrpc": "2.0",
                "id": frame.get("id").cloned().unwrap_or(Value::Null),
                "error": { "code": -32601, "message": "no tasks here" }
            })],
            other => panic!("unexpected method {other}"),
        },
    )
    .await;

    let err = client.list_all_tasks().await.expect_err("must propagate");
    assert!(matches!(&err, ClientError::Rpc(e) if e.code == -32601));
}

/// Methods one revision removed or never had are refused before they reach
/// the wire, each naming what to use instead, rather than earning a bare
/// `-32601`. The scripted server panics on anything but discovery.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stateful_only_methods_are_refused_locally_on_2026_07_28() {
    let client = connect(None, |method, _| panic!("{method} reached the wire")).await;

    let refusals = [
        client.subscribe_resource("file:///a").await.map(drop),
        client.unsubscribe_resource("file:///a").await.map(drop),
        #[expect(deprecated, reason = "the refusal is the point")]
        client.set_level(LogLevel::Info).await.map(drop),
        client.task_list(None).await.map(drop),
        client.ping().await,
    ];
    for refusal in refusals {
        let err = refusal.expect_err("refused locally");
        assert!(
            matches!(&err, ClientError::Protocol(m) if m.contains("removed in 2026-07-28")),
            "{err}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stateless_only_methods_are_refused_locally_before_2026_07_28() {
    let client = connect_legacy(ClientBuilder::new("subscriber", "1.0.0"), |method, _| {
        panic!("{method} reached the wire")
    })
    .await;

    let listen = client
        .listen(neutral::SubscriptionFilter::all_list_changed())
        .await
        .expect_err("listen arrived in 2026-07-28");
    assert!(matches!(&listen, ClientError::Protocol(m) if m.contains("subscribe_resource")));
    let update = client
        .task_update("t1", Map::new())
        .await
        .expect_err("tasks/update is the SEP-2663 extension");
    assert!(matches!(update, ClientError::Protocol(_)));
}

/// `with_log_level` opts into server logs on either wire: the per-request
/// `_meta` level on 2026-07-28, `logging/setLevel` once at connect before.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_log_level_opts_in_on_both_wires() {
    let seen = Arc::new(Mutex::new(Vec::<Value>::new()));
    let record = Arc::clone(&seen);
    let modern = connect_with(
        ClientBuilder::new("subscriber", "1.0.0").with_log_level(LogLevel::Warning),
        None,
        move |method, frame| {
            assert_eq!(method, "tools/list");
            record.lock().unwrap().push(frame["params"].clone());
            vec![result_for(
                frame,
                json!({ "tools": [], "resultType": "complete", "cacheScope": "private", "ttlMs": 0 }),
            )]
        },
    )
    .await;
    modern.list_tools(None).await.expect("list");
    assert_eq!(
        seen.lock().unwrap()[0]["_meta"]["io.modelcontextprotocol/logLevel"],
        "warning"
    );

    let levels = Arc::new(Mutex::new(Vec::<Value>::new()));
    let record = Arc::clone(&levels);
    let _legacy = connect_legacy(
        ClientBuilder::new("subscriber", "1.0.0").with_log_level(LogLevel::Error),
        move |method, frame| {
            assert_eq!(method, "logging/setLevel");
            record
                .lock()
                .unwrap()
                .push(frame["params"]["level"].clone());
            vec![result_for(frame, json!({}))]
        },
    )
    .await;
    assert_eq!(*levels.lock().unwrap(), vec![json!("error")]);
}

/// Dropping a subscription tells the server, by the listen request's id:
/// "send `notifications/cancelled` referencing the `subscriptions/listen`
/// request ID (stdio)". There was no way to end one short of closing the
/// client.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_a_subscription_cancels_it_by_its_id() {
    let (cancelled_tx, mut cancelled_rx) = tokio::sync::mpsc::unbounded_channel();
    let client = connect(None, move |method, frame| match method {
        "subscriptions/listen" => {
            let id = frame.get("id").cloned().unwrap_or(Value::Null);
            vec![json!({
                "jsonrpc": "2.0",
                "method": "notifications/subscriptions/acknowledged",
                "params": {
                    "_meta": { "io.modelcontextprotocol/subscriptionId": id },
                    "notifications": { "toolsListChanged": true }
                }
            })]
        }
        "notifications/cancelled" => {
            let _ = cancelled_tx.send(frame["params"]["requestId"].clone());
            vec![]
        }
        other => panic!("unexpected method {other}"),
    })
    .await;

    let subscription = client
        .listen(neutral::SubscriptionFilter::all_list_changed())
        .await
        .expect("listen");
    let id = serde_json::to_value(subscription.id()).unwrap();
    drop(subscription);
    let cancelled = tokio::time::timeout(std::time::Duration::from_secs(5), cancelled_rx.recv())
        .await
        .expect("the client cancels the subscription")
        .unwrap();
    assert_eq!(cancelled, id);
}
