//! Which upstream connection serves a call: one shared, one per caller, one
//! per session; serialized calls on a shared one; dead ones replaced.
#![cfg(all(feature = "client", feature = "proxy"))]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Map, Value, json};
use turbomcp::client::{Client, ClientBuilder, ConnectMode, ElicitationHandler, async_trait};
use turbomcp::prelude::*;
use turbomcp::proxy::{RemoteServer, RemoteServerBuilder, UpstreamKey};
use turbomcp::{Identity, McpRequest};

/// The server behind one upstream connection, numbered in opening order.
#[derive(Clone)]
struct Numbered {
    n: usize,
    /// Shared by every connection: holds concurrent calls until all are in.
    together: Arc<tokio::sync::Barrier>,
}

async fn ask(ctx: &CallToolContext) -> McpResult<String> {
    let outcome = ctx
        .client
        .elicit(
            "who",
            neutral::ElicitParams::new(
                "Who is this?",
                json!({ "type": "object", "properties": { "who": { "type": "string" } } }),
            ),
        )
        .await?;
    Ok(match outcome.content.get("who").and_then(Value::as_str) {
        Some(who) if outcome.accepted() => who.to_owned(),
        _ => format!("{:?}", outcome.action),
    })
}

#[server(name = "numbered", version = "1.0.0")]
impl Numbered {
    /// Which connection this is.
    #[tool]
    async fn whoami(&self) -> String {
        self.n.to_string()
    }

    /// Asks who is calling once both concurrent calls are in.
    #[tool]
    async fn who_together(&self, ctx: &CallToolContext) -> McpResult<String> {
        self.together.wait().await;
        ask(ctx).await
    }

    /// Asks who is calling, after a moment.
    #[tool]
    async fn who_slowly(&self, ctx: &CallToolContext) -> McpResult<String> {
        tokio::time::sleep(Duration::from_millis(50)).await;
        ask(ctx).await
    }
}

/// Opens a fresh in-process upstream server for each connection.
struct Farm {
    opened: AtomicUsize,
    ended: Arc<AtomicUsize>,
    servers: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    together: Arc<tokio::sync::Barrier>,
}

impl Farm {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            opened: AtomicUsize::new(0),
            ended: Arc::new(AtomicUsize::new(0)),
            servers: Mutex::new(Vec::new()),
            together: Arc::new(tokio::sync::Barrier::new(2)),
        })
    }

    fn opened(&self) -> usize {
        self.opened.load(Ordering::SeqCst)
    }

    /// A remote over this farm, its upstreams speaking `mode`.
    fn remote(self: &Arc<Self>, mode: ConnectMode) -> RemoteServerBuilder {
        let farm = Arc::clone(self);
        RemoteServer::dial("farm", move || {
            use turbomcp::Serve as _;
            use turbomcp::testing::TestServer as _;
            let n = farm.opened.fetch_add(1, Ordering::SeqCst) + 1;
            let (server_end, client_end) = turbomcp::memory::pair();
            let handle = Numbered {
                n,
                together: Arc::clone(&farm.together),
            }
            .into_server()
            .into_handle();
            let ended = Arc::clone(&farm.ended);
            farm.servers.lock().unwrap().push(tokio::spawn(async move {
                let _ = server_end.serve(handle).await;
                ended.fetch_add(1, Ordering::SeqCst);
            }));
            async move { Ok(client_end) }
        })
        .client(move |c| c.with_connect_mode(mode))
    }
}

/// A downstream caller: answers every question as `name`.
#[derive(Clone)]
struct Caller(&'static str);

#[async_trait]
impl ElicitationHandler for Caller {
    async fn elicit(&self, _request: neutral::ElicitParams) -> neutral::ElicitOutcome {
        let mut content = Map::new();
        content.insert("who".into(), json!(self.0));
        neutral::ElicitOutcome::new(neutral::ElicitAction::Accept, content)
    }
}

/// `name` connected to `remote`, authenticated as `name` unless anonymous.
async fn caller(remote: &RemoteServer, name: &'static str, authenticated: bool) -> Client {
    let server = remote
        .clone()
        .into_server()
        .layer(tower::util::MapRequestLayer::new(move |req: McpRequest| {
            if authenticated {
                req.with(Identity::Bearer {
                    sub: name.into(),
                    claims: Map::new(),
                })
            } else {
                req
            }
        }));
    turbomcp::testing::connect(
        server,
        ClientBuilder::new(name, "1.0.0")
            .with_connect_mode(ConnectMode::Legacy)
            .with_elicitation(Caller(name)),
    )
    .await
    .unwrap()
}

async fn call(client: &Client, tool: &str) -> String {
    client
        .call_tool(tool, Map::new())
        .await
        .unwrap_or_else(|e| panic!("{tool}: {e}"))
        .text_content()
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_default_key_follows_the_upstream() {
    let farm = Farm::new();
    let legacy = farm.remote(ConnectMode::Legacy).connect().await.unwrap();
    assert_eq!(legacy.key(), UpstreamKey::Principal);
    let modern = farm.remote(ConnectMode::Modern).connect().await.unwrap();
    assert_eq!(modern.key(), UpstreamKey::Global);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn principal_keying_gives_each_caller_its_own_upstream() {
    let farm = Farm::new();
    let remote = farm
        .remote(ConnectMode::Legacy)
        .key(UpstreamKey::Principal)
        .connect()
        .await
        .unwrap();
    let ada = caller(&remote, "ada", true).await;
    let bob = caller(&remote, "bob", true).await;
    let anon = caller(&remote, "anon", false).await;
    // The connection made at startup serves the first caller.
    assert_eq!(call(&ada, "whoami").await, "1");
    assert_eq!(call(&bob, "whoami").await, "2");
    assert_eq!(call(&ada, "whoami").await, "1");
    assert_eq!(call(&anon, "whoami").await, "3");
    assert_eq!(farm.opened(), 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn global_keying_shares_one_upstream() {
    let farm = Farm::new();
    let remote = farm
        .remote(ConnectMode::Legacy)
        .key(UpstreamKey::Global)
        .connect()
        .await
        .unwrap();
    let ada = caller(&remote, "ada", true).await;
    let bob = caller(&remote, "bob", true).await;
    assert_eq!(call(&ada, "whoami").await, "1");
    assert_eq!(call(&bob, "whoami").await, "1");
    assert_eq!(farm.opened(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_keying_closes_the_upstream_with_its_session() {
    let farm = Farm::new();
    let remote = farm
        .remote(ConnectMode::Legacy)
        .key(UpstreamKey::Session)
        .connect()
        .await
        .unwrap();
    let session = |name: &'static str| {
        let server = remote
            .clone()
            .into_server()
            .observe_sessions(Arc::new(remote.clone()));
        turbomcp::testing::connect(
            server,
            ClientBuilder::new(name, "1.0.0").with_connect_mode(ConnectMode::Legacy),
        )
    };
    let first = session("first").await.unwrap();
    let second = session("second").await.unwrap();
    assert_eq!(call(&first, "whoami").await, "1");
    assert_eq!(call(&second, "whoami").await, "2");

    second.close().await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while farm.ended.load(Ordering::SeqCst) < 1 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the session's upstream outlived it"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    // The other session's upstream is untouched.
    assert_eq!(call(&first, "whoami").await, "1");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn per_caller_upstreams_attribute_what_a_shared_one_cannot() {
    // `2025-11-25` upstreams on pipes: shared, two concurrent questions
    // would be refused; one connection each, each is its caller's.
    let farm = Farm::new();
    let remote = farm.remote(ConnectMode::Legacy).connect().await.unwrap();
    let ada = caller(&remote, "ada", true).await;
    let bob = caller(&remote, "bob", true).await;
    let (a, b) = tokio::join!(call(&ada, "who_together"), call(&bob, "who_together"));
    assert_eq!((a.as_str(), b.as_str()), ("ada", "bob"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serialized_calls_attribute_on_a_shared_upstream() {
    let farm = Farm::new();
    let remote = farm
        .remote(ConnectMode::Legacy)
        .key(UpstreamKey::Global)
        .serialize_input(true)
        .connect()
        .await
        .unwrap();
    let ada = caller(&remote, "ada", true).await;
    let bob = caller(&remote, "bob", true).await;
    let (a, b) = tokio::join!(call(&ada, "who_slowly"), call(&bob, "who_slowly"));
    assert_eq!((a.as_str(), b.as_str()), ("ada", "bob"));
    assert_eq!(farm.opened(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dead_upstream_is_replaced() {
    let farm = Farm::new();
    let remote = farm
        .remote(ConnectMode::Legacy)
        .key(UpstreamKey::Global)
        .connect()
        .await
        .unwrap();
    let ada = caller(&remote, "ada", true).await;
    assert_eq!(call(&ada, "whoami").await, "1");

    farm.servers.lock().unwrap()[0].abort();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        // A call racing the closure may fail; the next one reconnects.
        if let Ok(result) = ada.call_tool("whoami", Map::new()).await
            && result.text_content().as_deref() == Some("2")
        {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the dead upstream was never replaced"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
