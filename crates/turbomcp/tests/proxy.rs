//! The gateway: a `RemoteServer` serves an upstream as if it were local, on
//! every pair of revisions, mounted in a composite under its visibility, with
//! progress, cancellation and change notifications crossing the hop.
#![cfg(all(feature = "client", feature = "proxy"))]

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Map, Value, json};
use turbomcp::client::{CallOptions, ClientBuilder, ConnectMode, NotificationHandler, async_trait};
use turbomcp::prelude::*;
use turbomcp::proxy::{ProxyError, RemoteServer, Upstream};
use turbomcp::{Composite, ProtocolVersion};
use turbomcp_core::Implementation;

static CANCELLED: AtomicBool = AtomicBool::new(false);

#[derive(Clone)]
struct Upstairs;

#[server(name = "upstairs", version = "2.0.0", instructions = "Upstairs tools.")]
impl Upstairs {
    /// Add two numbers.
    #[tool]
    async fn add(&self, a: i64, b: i64) -> i64 {
        a + b
    }

    /// Reports progress, then answers.
    #[tool]
    async fn crunch(&self, ctx: &CallToolContext) -> String {
        ctx.progress.report(1.0, Some(2.0), Some("half")).await;
        ctx.progress.report(2.0, Some(2.0), Some("done")).await;
        "crunched".into()
    }

    /// Waits until cancelled.
    #[tool]
    async fn wait(&self, ctx: &CallToolContext) -> String {
        ctx.base.cancellation.cancelled().await;
        CANCELLED.store(true, Ordering::SeqCst);
        "cancelled".into()
    }

    /// For operators only.
    #[tool(tags("internal"))]
    async fn reindex(&self) -> String {
        "reindexed".into()
    }

    /// Greet someone.
    #[prompt]
    async fn greet(&self, name: String) -> String {
        format!("Hello, {name}!")
    }

    #[resource("config://app", mime_type = "application/json")]
    async fn config(&self) -> String {
        "{\"debug\":false}".into()
    }
}

/// Only tools.
#[derive(Clone)]
struct ToolsOnly;

#[server(name = "tools-only", version = "1.0.0")]
impl ToolsOnly {
    /// Echo.
    #[tool]
    async fn echo(&self, text: String) -> String {
        text
    }
}

/// `server` upstream of a `RemoteServer`, in memory, speaking `mode`.
async fn remote<S: turbomcp::testing::TestServer>(server: S, mode: ConnectMode) -> RemoteServer
where
    S::Handle: 'static,
{
    use turbomcp::Serve as _;
    let (server_end, client_end) = turbomcp::memory::pair();
    let handle = server.into_handle();
    tokio::spawn(async move {
        let _ = server_end.serve(handle).await;
    });
    RemoteServer::over(
        "upstairs",
        ClientBuilder::new("gateway", "1.0.0").with_connect_mode(mode),
        client_end,
    )
    .await
    .expect("connected upstream")
}

fn args(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_remote_server_bridges_every_pair_of_revisions() {
    for upstream in [ConnectMode::Modern, ConnectMode::Legacy] {
        for downstream in [ConnectMode::Modern, ConnectMode::Legacy] {
            let remote = remote(Upstairs.into_server(), upstream).await;
            let client = turbomcp::testing::connect(
                remote.clone().into_server(),
                ClientBuilder::new("agent", "1.0.0").with_connect_mode(downstream),
            )
            .await
            .expect("downstream handshake");
            let pair = format!("{upstream:?} upstream, {downstream:?} downstream");
            assert_eq!(client.server_info().unwrap().name, "upstairs", "{pair}");
            assert_eq!(client.instructions(), Some("Upstairs tools."), "{pair}");

            let result = client
                .call_tool("add", args(json!({ "a": 2, "b": 3 })))
                .await
                .unwrap_or_else(|e| panic!("{pair}: {e}"));
            assert_eq!(result.text_content().as_deref(), Some("5"), "{pair}");

            let prompt = client
                .get_prompt("greet", BTreeMap::from([("name".into(), "Ada".into())]))
                .await
                .unwrap_or_else(|e| panic!("{pair}: {e}"));
            assert!(format!("{prompt:?}").contains("Hello, Ada!"), "{pair}");

            let read = client
                .read_resource("config://app")
                .await
                .unwrap_or_else(|e| panic!("{pair}: {e}"));
            assert!(format!("{read:?}").contains("debug"), "{pair}");

            // An unknown tool is unknown downstream too, as a protocol error.
            let unknown = client.call_tool("nope", Map::new()).await.unwrap_err();
            assert!(unknown.to_string().contains("nope"), "{pair}: {unknown}");
        }
    }
}

#[tokio::test]
async fn capabilities_follow_the_upstream() {
    let remote = remote(ToolsOnly.into_server(), ConnectMode::Modern).await;
    assert!(remote.capabilities().prompts.is_none());
    let client = turbomcp::testing::connect(remote.into_server(), ClientBuilder::new("a", "1"))
        .await
        .unwrap();
    let caps = client.server_capabilities();
    assert!(caps.tools.is_some());
    assert!(caps.prompts.is_none() && caps.resources.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_composite_mounts_a_remote_beside_local_tools_under_its_visibility() {
    let remote = remote(Upstairs.into_server(), ConnectMode::Legacy).await;
    let gateway = Composite::new(Implementation::new("gateway", "1.0.0"))
        .mount("up", remote.into_server())
        .unwrap()
        .mount("local", ToolsOnly.into_server())
        .unwrap()
        .into_server()
        .with_visibility(Arc::new(
            turbomcp::visibility::Visibility::new().hiding_tagged(["internal"]),
        ));
    let client = turbomcp::testing::connect(gateway, ClientBuilder::new("agent", "1.0.0"))
        .await
        .unwrap();
    let names: Vec<String> = client
        .list_all_tools()
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert!(names.contains(&"up__add".to_owned()), "{names:?}");
    assert!(names.contains(&"local__echo".to_owned()), "{names:?}");
    assert!(
        !names.iter().any(|n| n.contains("reindex")),
        "the upstream's internal tool is hidden: {names:?}"
    );
    let result = client
        .call_tool("up__add", args(json!({ "a": 20, "b": 22 })))
        .await
        .unwrap();
    assert_eq!(result.text_content().as_deref(), Some("42"));
    // Hidden means unreachable, and indistinguishable from absent.
    let hidden = client
        .call_tool("up__reindex", Map::new())
        .await
        .unwrap_err();
    let absent = client
        .call_tool("up__nothing", Map::new())
        .await
        .unwrap_err();
    assert_eq!(
        hidden.to_string().replace("reindex", "X"),
        absent.to_string().replace("nothing", "X")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn progress_crosses_the_hop() {
    let remote = remote(Upstairs.into_server(), ConnectMode::Modern).await;
    let client = turbomcp::testing::connect(remote.into_server(), ClientBuilder::new("a", "1"))
        .await
        .unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    let options = CallOptions::new().on_progress(move |p| sink.lock().unwrap().push(p.message));
    let result = client
        .call_tool_with("crunch", Map::new(), &options)
        .await
        .unwrap();
    assert_eq!(result.text_content().as_deref(), Some("crunched"));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while seen.lock().unwrap().len() < 2 {
        assert!(tokio::time::Instant::now() < deadline, "progress lost");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(
        *seen.lock().unwrap(),
        [Some("half".to_owned()), Some("done".to_owned())]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_crosses_the_hop() {
    let remote = remote(Upstairs.into_server(), ConnectMode::Legacy).await;
    let client = turbomcp::testing::connect(
        remote.into_server(),
        ClientBuilder::new("a", "1").with_connect_mode(ConnectMode::Legacy),
    )
    .await
    .unwrap();
    let options = CallOptions::new().timeout(Duration::from_millis(200));
    assert!(
        client
            .call_tool_with("wait", Map::new(), &options)
            .await
            .is_err()
    );
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !CANCELLED.load(Ordering::SeqCst) {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the upstream call was never cancelled"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// Collects the notifications the downstream client hears.
#[derive(Clone, Default)]
struct Heard(Arc<Mutex<Vec<String>>>);

#[async_trait]
impl NotificationHandler for Heard {
    async fn on_notification(&self, method: String, _params: Option<Value>) {
        self.0.lock().unwrap().push(method);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upstream_changes_reach_downstream_clients() {
    let upstream = Upstairs
        .into_server()
        .layer(tower::layer::util::Identity::new());
    let upstream_notifier = upstream.notifier();
    let remote = remote(upstream, ConnectMode::Legacy).await;

    let downstream = remote
        .clone()
        .into_server()
        .layer(tower::layer::util::Identity::new());
    remote.forward_changes_to(downstream.notifier());
    let heard = Heard::default();
    let client = turbomcp::testing::connect(
        downstream,
        ClientBuilder::new("a", "1")
            .with_connect_mode(ConnectMode::Legacy)
            .with_notifications(heard.clone()),
    )
    .await
    .unwrap();
    assert_eq!(client.protocol_version(), &ProtocolVersion::V2025_11_25);

    upstream_notifier.tools_list_changed();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !heard
        .0
        .lock()
        .unwrap()
        .iter()
        .any(|m| m == "notifications/tools/list_changed")
    {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the change was lost"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn an_upstream_that_cannot_start_says_so() {
    let err = RemoteServer::connect(Upstream::stdio("/nonexistent/mcp-server", ["--x"]))
        .await
        .unwrap_err();
    assert!(matches!(err, ProxyError::Spawn { .. }), "{err}");
}
