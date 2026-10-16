//! The gateway: a `RemoteServer` serves an upstream as if it were local, on
//! every pair of revisions, mounted in a composite under its visibility, with
//! progress, cancellation and change notifications crossing the hop.
#![cfg(all(feature = "client", feature = "proxy"))]

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Map, Value, json};
use turbomcp::client::{
    CallOptions, ClientBuilder, ClientResult, ConnectMode, ElicitationHandler, NotificationHandler,
    RootsHandler, SamplingHandler, async_trait,
};
use turbomcp::prelude::*;
use turbomcp::proxy::{ProxyError, RemoteServer, Upstream};
use turbomcp::{Composite, ProtocolVersion};
use turbomcp_core::Implementation;

static CANCELLED: AtomicBool = AtomicBool::new(false);

fn who_schema() -> Value {
    json!({ "type": "object", "properties": { "who": { "type": "string" } } })
}

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
        // A cancelled handler is dropped, or sees its token, whichever the
        // dispatcher gets to first.
        struct Flag;
        impl Drop for Flag {
            fn drop(&mut self) {
                CANCELLED.store(true, Ordering::SeqCst);
            }
        }
        let _flag = Flag;
        ctx.base.cancellation.cancelled().await;
        "cancelled".into()
    }

    /// Asks the caller three things: to confirm, for a draft, for its roots.
    #[tool]
    async fn interview(&self, ctx: &CallToolContext) -> McpResult<String> {
        let confirmed = ctx
            .client
            .elicit(
                "who",
                neutral::ElicitParams::new("Who is this?", who_schema()),
            )
            .await?;
        let who = confirmed.content.get("who").and_then(Value::as_str);
        let reply = ctx
            .client
            .create_message(
                "draft",
                neutral::CreateMessageParams::new(
                    vec![neutral::SamplingMessage::text(
                        neutral::Role::User,
                        "Draft it",
                    )],
                    50,
                ),
            )
            .await?;
        let draft = match reply.content.first() {
            Some(neutral::SamplingContent::Media(neutral::Content::Text { text, .. })) => {
                text.clone()
            }
            other => format!("{other:?}"),
        };
        let roots = ctx.client.list_roots("roots").await?;
        let roots: Vec<&str> = roots.iter().map(|r| r.uri.as_str()).collect();
        Ok(format!(
            "who={} draft={draft} roots={}",
            who.unwrap_or("nobody"),
            roots.join(",")
        ))
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

/// Asks who is calling, with two calls in flight.
#[derive(Clone)]
struct Pair {
    together: Arc<tokio::sync::Barrier>,
    /// Hold each call after its question is judged (answered or refused),
    /// until both have been: one call alone in flight is unambiguous. (Not
    /// under MRTR, which re-executes a call after it asked.)
    hold_after_asking: bool,
}

impl Pair {
    fn new(hold_after_asking: bool) -> Self {
        Self {
            together: Arc::new(tokio::sync::Barrier::new(2)),
            hold_after_asking,
        }
    }
}

#[server(name = "pair", version = "1.0.0")]
impl Pair {
    /// Asks who is calling once both calls are in flight.
    #[tool]
    async fn who(&self, ctx: &CallToolContext) -> McpResult<String> {
        self.together.wait().await;
        let outcome = ctx
            .client
            .elicit(
                "who",
                neutral::ElicitParams::new("Who is this?", who_schema()),
            )
            .await;
        // Both calls stay in flight until both questions have been judged,
        // answered or refused: one that finished first would leave the
        // other's question unambiguous (one call in flight), and its
        // caller rightly asked.
        if self.hold_after_asking {
            self.together.wait().await;
        }
        let outcome = outcome?;
        Ok(match outcome.content.get("who").and_then(Value::as_str) {
            Some(who) if outcome.accepted() => who.to_owned(),
            _ => format!("{:?}", outcome.action),
        })
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

/// `server` upstream of a `RemoteServer` built by `configure`.
async fn remote_with<S: turbomcp::testing::TestServer>(
    server: S,
    configure: impl FnOnce(turbomcp::proxy::RemoteServerBuilder) -> turbomcp::proxy::RemoteServerBuilder,
) -> RemoteServer
where
    S::Handle: 'static,
{
    use turbomcp::Serve as _;
    let (server_end, client_end) = turbomcp::memory::pair();
    let handle = server.into_handle();
    tokio::spawn(async move {
        let _ = server_end.serve(handle).await;
    });
    let once = Mutex::new(Some(client_end));
    configure(RemoteServer::dial("upstairs", move || {
        let end = once.lock().unwrap().take();
        async move { end.ok_or_else(|| std::io::Error::other("used")) }
    }))
    .connect()
    .await
    .expect("connected upstream")
}

/// The gateway's policy judges tags the gateway gave, not the upstream: an
/// upstream that tagged its own tool `public` (or left `admin` off) would
/// otherwise decide what callers see. Its tags count only when trusted.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upstream_tags_count_only_when_the_gateway_trusts_them() {
    let names = |remote: RemoteServer| async move {
        let gateway = Composite::new(Implementation::new("gateway", "1.0.0"))
            .mount("up", remote.into_server())
            .unwrap()
            .into_server()
            .with_visibility(Arc::new(
                turbomcp::visibility::Visibility::new().hiding_tagged(["internal"]),
            ));
        let client = turbomcp::testing::connect(gateway, ClientBuilder::new("agent", "1.0.0"))
            .await
            .unwrap();
        client
            .list_all_tools()
            .await
            .unwrap()
            .into_iter()
            .map(|t| t.name)
            .collect::<Vec<_>>()
    };
    let untrusted = names(remote_with(Upstairs.into_server(), |b| b).await).await;
    assert!(
        untrusted.contains(&"up__reindex".to_owned()),
        "{untrusted:?}"
    );
    let tagged = names(remote_with(Upstairs.into_server(), |b| b.tags(["internal"])).await).await;
    assert!(tagged.is_empty(), "the gateway tagged them all: {tagged:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_composite_mounts_a_remote_beside_local_tools_under_its_visibility() {
    let remote = remote_with(Upstairs.into_server(), |b| b.trust_upstream_tags(true)).await;
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

/// A downstream caller: answers every question as `name`.
#[derive(Clone)]
struct Caller(&'static str);

#[async_trait]
impl ElicitationHandler for Caller {
    async fn elicit(&self, _request: neutral::ElicitParams) -> neutral::ElicitOutcome {
        neutral::ElicitOutcome::new(
            neutral::ElicitAction::Accept,
            args(json!({ "who": self.0 })),
        )
    }
}

#[async_trait]
impl SamplingHandler for Caller {
    async fn create_message(
        &self,
        _params: neutral::CreateMessageParams,
    ) -> ClientResult<neutral::CreateMessageResult> {
        Ok(neutral::CreateMessageResult::text(
            "model",
            format!("by {}", self.0),
        ))
    }
}

#[async_trait]
impl RootsHandler for Caller {
    async fn list_roots(&self) -> ClientResult<Vec<neutral::Root>> {
        Ok(vec![
            neutral::Root::new(format!("file:///{}", self.0)).unwrap(),
        ])
    }
}

fn caller(name: &'static str, mode: ConnectMode) -> ClientBuilder {
    ClientBuilder::new(name, "1.0.0")
        .with_connect_mode(mode)
        .with_elicitation(Caller(name))
        .with_sampling(Caller(name))
        .with_roots(Caller(name))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upstream_questions_reach_the_caller_on_every_pair_of_revisions() {
    for upstream in [ConnectMode::Modern, ConnectMode::Legacy] {
        for downstream in [ConnectMode::Modern, ConnectMode::Legacy] {
            let remote = remote_with(Upstairs.into_server(), |b| {
                b.client(move |c| c.with_connect_mode(upstream))
                    .key(turbomcp::proxy::UpstreamKey::Global)
                    .forward_sampling(true)
                    .forward_roots(true)
            })
            .await;
            let client =
                turbomcp::testing::connect(remote.into_server(), caller("ada", downstream))
                    .await
                    .unwrap();
            let pair = format!("{upstream:?} upstream, {downstream:?} downstream");
            let result = client
                .call_tool("interview", Map::new())
                .await
                .unwrap_or_else(|e| panic!("{pair}: {e}"));
            assert_eq!(
                result.text_content().as_deref(),
                Some("who=ada draft=by ada roots=file:///ada"),
                "{pair}"
            );
        }
    }
}

/// `who` called by `ada` and `bob` at once, through `remote`: each answer,
/// or what the failure said.
async fn ada_and_bob(
    remote: &RemoteServer,
    downstream: ConnectMode,
) -> (Result<String, String>, Result<String, String>) {
    let ada = turbomcp::testing::connect(remote.clone().into_server(), caller("ada", downstream))
        .await
        .unwrap();
    let bob = turbomcp::testing::connect(remote.clone().into_server(), caller("bob", downstream))
        .await
        .unwrap();
    let (a, b) = tokio::join!(
        ada.call_tool("who", Map::new()),
        bob.call_tool("who", Map::new()),
    );
    let text = |r: ClientResult<neutral::CallToolResult>| match r {
        Ok(r) if !r.is_error => Ok(r.text_content().unwrap_or_default()),
        Ok(r) => Err(r.text_content().unwrap_or_default()),
        Err(e) => Err(e.to_string()),
    };
    (text(a), text(b))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_callers_each_get_their_own_questions() {
    // A `2026-07-28` upstream returns each call's questions in its own result.
    for downstream in [ConnectMode::Modern, ConnectMode::Legacy] {
        let remote = remote(Pair::new(false).into_server(), ConnectMode::Modern).await;
        let (a, b) = ada_and_bob(&remote, downstream).await;
        assert_eq!(
            (a.as_deref(), b.as_deref()),
            (Ok("ada"), Ok("bob")),
            "{downstream:?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unattributable_question_is_refused_not_guessed() {
    // A `2025-11-25` upstream on a pipe says nothing about whose call an
    // `elicitation/create` is. With two in flight, neither caller is asked,
    // and the upstream hears an error, not a user who declined.
    let remote = remote(Pair::new(true).into_server(), ConnectMode::Legacy).await;
    let (a, b) = ada_and_bob(&remote, ConnectMode::Legacy).await;
    for answer in [a, b] {
        let refusal = answer.expect_err("nobody was asked, so nobody answered");
        assert!(refusal.contains("cannot tell which caller"), "{refusal}");
    }
}

/// Scope-gated upstream tools.
#[derive(Clone)]
struct Guarded;

#[server(name = "guarded", version = "1.0.0")]
impl Guarded {
    /// For admins.
    #[tool(scopes("admin"))]
    async fn purge(&self) -> String {
        "purged".into()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upstream_scopes_reach_the_gateway_only_when_it_enforces_them() {
    for enforce in [false, true] {
        use turbomcp::Serve as _;
        use turbomcp::testing::TestServer as _;
        let (server_end, client_end) = turbomcp::memory::pair();
        let handle = Guarded.into_server().into_handle();
        tokio::spawn(async move {
            let _ = server_end.serve(handle).await;
        });
        let once = Mutex::new(Some(client_end));
        let remote = RemoteServer::dial("guarded", move || {
            let end = once.lock().unwrap().take();
            async move { end.ok_or_else(|| std::io::Error::other("used")) }
        })
        .enforce_upstream_scopes(enforce)
        .connect()
        .await
        .unwrap();
        let client = turbomcp::testing::connect(remote.into_server(), ClientBuilder::new("a", "1"))
            .await
            .unwrap();
        let tools = client.list_all_tools().await.unwrap();
        let purge = tools.iter().find(|t| t.name == "purge");
        let declared = purge.is_some_and(|t| t.meta.contains_key("io.turbomcp/scopes"));
        // Listed either way (no visibility policy here); its scopes come
        // along only for the gateway to enforce.
        assert!(purge.is_some(), "enforce={enforce}");
        assert_eq!(declared, enforce, "enforce={enforce}");
    }
}

/// A downstream client's resource subscription reaches the upstream, on
/// either upstream revision, and the upstream's update comes back down. The
/// gateway used to answer `resources/subscribe` itself and subscribe to
/// nothing upstream, so no update ever arrived.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_subscription_through_the_gateway_reaches_the_upstream() {
    for mode in [ConnectMode::Legacy, ConnectMode::Modern] {
        let upstream = Upstairs
            .into_server()
            .layer(tower::layer::util::Identity::new());
        let upstream_notifier = upstream.notifier();
        let remote = remote(upstream, mode).await;
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
        assert!(
            client
                .server_capabilities()
                .resources
                .as_ref()
                .is_some_and(|r| r.subscribe),
            "the upstream offers subscriptions, so the gateway does"
        );
        client.subscribe_resource("config://app").await.unwrap();

        // The upstream subscription is made off the request path; keep
        // announcing until it is in place.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            upstream_notifier.resource_updated("config://app").await;
            tokio::time::sleep(Duration::from_millis(20)).await;
            if heard
                .0
                .lock()
                .unwrap()
                .iter()
                .any(|m| m == "notifications/resources/updated")
            {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "{mode:?} upstream: the update never came down"
            );
        }
    }
}

/// `ToolsOnly`, redeployed.
#[derive(Clone)]
struct ToolsOnlyV2;

#[server(name = "tools-only", version = "2.0.0")]
impl ToolsOnlyV2 {
    /// Echo, louder.
    #[tool]
    async fn echo(&self, text: String) -> String {
        text.to_uppercase()
    }
}

/// An upstream that comes back changed after its connection dies: the
/// gateway reconnects, and its downstream clients are told to list again
/// rather than keep the catalog of a server that is gone.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_changed_upstream_after_a_reconnect_is_announced() {
    use turbomcp::Serve as _;
    use turbomcp::testing::TestServer as _;
    let (first_server, first_client) = turbomcp::memory::pair();
    let first = tokio::spawn(async move {
        let _ = first_server
            .serve(ToolsOnly.into_server().into_handle())
            .await;
    });
    let (second_server, second_client) = turbomcp::memory::pair();
    tokio::spawn(async move {
        let _ = second_server
            .serve(ToolsOnlyV2.into_server().into_handle())
            .await;
    });
    let ends = Mutex::new(vec![second_client, first_client]);
    let remote = RemoteServer::dial("upstairs", move || {
        let end = ends.lock().unwrap().pop();
        async move { end.ok_or_else(|| std::io::Error::other("used")) }
    })
    .connect()
    .await
    .unwrap();
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
    let echo = || client.call_tool("echo", args(json!({ "text": "hi" })));
    assert_eq!(echo().await.unwrap().text_content().as_deref(), Some("hi"));

    first.abort();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(result) = echo().await
            && result.text_content().as_deref() == Some("HI")
        {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "never reconnected");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
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
            "the change was never announced"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Asks one question.
#[derive(Clone)]
struct Asker;

#[server(name = "asker", version = "1.0.0")]
impl Asker {
    /// Asks who is calling.
    #[tool]
    async fn ask(&self, ctx: &CallToolContext) -> McpResult<String> {
        let outcome = ctx
            .client
            .elicit(
                "who",
                neutral::ElicitParams::new("Who is this?", who_schema()),
            )
            .await?;
        Ok(format!("{:?}", outcome.action))
    }
}

/// Keeps the messages it was asked.
#[derive(Clone, Default)]
struct Asked(Arc<Mutex<Vec<String>>>);

#[async_trait]
impl ElicitationHandler for Asked {
    async fn elicit(&self, request: neutral::ElicitParams) -> neutral::ElicitOutcome {
        self.0.lock().unwrap().push(request.message);
        neutral::ElicitOutcome::new(neutral::ElicitAction::Accept, args(json!({ "who": "ada" })))
    }
}

/// A forwarded question names the upstream asking it, so a user of a
/// gateway over several can tell which one wants something of them.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_forwarded_question_names_its_upstream() {
    for attribute in [true, false] {
        let remote = remote_with(Asker.into_server(), |b| b.attribute_questions(attribute)).await;
        let asked = Asked::default();
        let client = turbomcp::testing::connect(
            remote.into_server(),
            ClientBuilder::new("ada", "1.0.0").with_elicitation(asked.clone()),
        )
        .await
        .unwrap();
        client.call_tool("ask", Map::new()).await.unwrap();
        let expected = if attribute {
            "upstairs: Who is this?"
        } else {
            "Who is this?"
        };
        assert_eq!(asked.0.lock().unwrap().as_slice(), [expected]);
    }
}

/// The resources `resources/updated` named, in order.
#[derive(Clone, Default)]
struct Updated(Arc<Mutex<Vec<String>>>);

#[async_trait]
impl NotificationHandler for Updated {
    async fn on_notification(&self, method: String, params: Option<Value>) {
        if method == "notifications/resources/updated"
            && let Some(uri) = params.as_ref().and_then(|p| p["uri"].as_str())
        {
            self.0.lock().unwrap().push(uri.to_owned());
        }
    }
}

/// An upstream announces changes only to resources it serves: URIs aren't
/// prefixed in a composite, so otherwise one upstream could announce
/// changes to another's, sending every subscriber to re-read it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_upstream_cannot_announce_another_servers_resource() {
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
    let updated = Updated::default();
    let client = turbomcp::testing::connect(
        downstream,
        ClientBuilder::new("a", "1")
            .with_connect_mode(ConnectMode::Legacy)
            .with_notifications(updated.clone()),
    )
    .await
    .unwrap();
    client
        .subscribe_resource("elsewhere://theirs")
        .await
        .unwrap();
    client.subscribe_resource("config://app").await.unwrap();

    // Each round announces the foreign URI first; updates are checked in
    // order, so once the upstream's own arrives the foreign one was judged.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !updated
        .0
        .lock()
        .unwrap()
        .iter()
        .any(|u| u == "config://app")
    {
        upstream_notifier
            .resource_updated("elsewhere://theirs")
            .await;
        upstream_notifier.resource_updated("config://app").await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(
            tokio::time::Instant::now() < deadline,
            "the upstream's own update never came"
        );
    }
    let updated = updated.0.lock().unwrap();
    assert!(
        !updated.iter().any(|u| u == "elsewhere://theirs"),
        "{updated:?}"
    );
}

/// Sampling runs the caller's model on the upstream's prompt, and roots name
/// the caller's files: neither is offered upstream unless the gateway says
/// so. Elicitation, which a person answers, is.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sampling_and_roots_reach_the_caller_only_when_allowed() {
    let remote = remote(Upstairs.into_server(), ConnectMode::Legacy).await;
    let client =
        turbomcp::testing::connect(remote.into_server(), caller("ada", ConnectMode::Legacy))
            .await
            .unwrap();
    let refused = match client.call_tool("interview", Map::new()).await {
        Ok(result) => {
            assert!(result.is_error, "{result:?}");
            result.text_content().unwrap_or_default()
        }
        Err(e) => e.to_string(),
    };
    assert!(refused.to_lowercase().contains("sampling"), "{refused}");
}

#[derive(Clone)]
struct Vendor;

#[server(name = "vendor", version = "1.0.0")]
impl Vendor {
    /// Answers with a key no revision of the spec defines.
    #[tool]
    async fn traced(&self) -> neutral::CallToolResult {
        let mut result = neutral::CallToolResult::text("ok");
        result.extra.insert(
            "x-vendor-trace".into(),
            json!({ "span": "abc", "hops": [1, 2] }),
        );
        result
    }
}

/// A result's top-level key that no revision defines (`schema.ts`:
/// `[key: string]: unknown`) reaches the caller through the gateway, on every
/// pair of revisions, instead of being dropped by the typed hop.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_vendor_key_on_a_result_crosses_the_gateway() {
    for upstream in [ConnectMode::Modern, ConnectMode::Legacy] {
        for downstream in [ConnectMode::Modern, ConnectMode::Legacy] {
            let remote = remote(Vendor.into_server(), upstream).await;
            let client = turbomcp::testing::connect(
                remote.into_server(),
                ClientBuilder::new("agent", "1.0.0").with_connect_mode(downstream),
            )
            .await
            .expect("downstream handshake");
            let result = client.call_tool("traced", Map::new()).await.unwrap();
            assert_eq!(
                result.extra.get("x-vendor-trace"),
                Some(&json!({ "span": "abc", "hops": [1, 2] })),
                "{upstream:?} upstream, {downstream:?} downstream"
            );
        }
    }
}
