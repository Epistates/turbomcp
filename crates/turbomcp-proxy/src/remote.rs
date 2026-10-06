//! [`RemoteServer`]: an upstream MCP server, served as if it were local.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::sync::broadcast;
use turbomcp_client::{CallOptions, Client, ClientBuilder, ConnectMode, NotificationHandler};
use turbomcp_core::{Implementation, McpError, McpResult, ProtocolVersion};
use turbomcp_protocol::{methods, neutral};
use turbomcp_server::bus::Change;
use turbomcp_server::{
    CallToolContext, CompleteContext, GetPromptContext, ListPromptsContext,
    ListResourceTemplatesContext, ListResourcesContext, ListToolsContext, McpServerCore,
    MethodRouter, ReadResourceContext, ServerBuilder, ServerNotifier, UriTemplate, WithCompletions,
    WithPrompts, WithResources, WithTools,
};
use turbomcp_service::Transport;

use crate::bridge::{Bridge, Completions, UpstreamHandlers};
use crate::process::ChildProcess;
use crate::{OutboundAuth, ProxyError, Upstream};

/// How long a looked-up catalogue is trusted before it is fetched again,
/// unless the upstream says it changed first.
const DEFAULT_CATALOG_TTL: Duration = Duration::from_secs(30);

/// How old a catalogue must be before a miss refetches it.
const MISS_REFETCH_AFTER: Duration = Duration::from_secs(1);

/// How many changes a slow [`RemoteServer::changes`] receiver may fall
/// behind before it skips the oldest.
const CHANGE_BUFFER: usize = 64;

/// An upstream MCP server, served as if it were local.
///
/// It implements the same capability traits a `#[server]` does, by
/// forwarding each call through a [`Client`] connected upstream, so it is the
/// same kind of object as a local server: serve it on its own (a protocol
/// bridge, stdio to HTTP, `2025-11-25` to `2026-07-28`), or mount it in a
/// [`Composite`](turbomcp_server::Composite) beside local tools and other
/// remotes, under the composite's authentication and visibility.
///
/// What it advertises is what the upstream advertised in its handshake: a
/// remote without prompts registers no prompts, so capabilities still can't
/// drift from what is actually there.
///
/// The caller's identity and token never travel upstream (no token
/// passthrough): the proxy authenticates as itself ([`OutboundAuth`]).
/// Cancellation, progress and trace context do travel, both ways.
///
/// Cheap to clone; clones share the upstream connection.
#[derive(Clone)]
pub struct RemoteServer {
    inner: Arc<Inner>,
}

struct Inner {
    label: String,
    client: Client,
    info: Implementation,
    instructions: Option<String>,
    capabilities: neutral::ServerCapabilities,
    tools: Catalog<neutral::Tool>,
    prompts: Catalog<neutral::Prompt>,
    resources: Catalog<neutral::Resource>,
    templates: Catalog<neutral::ResourceTemplate>,
    changes: broadcast::Sender<Change>,
    /// Bridge the upstream's requests for input to downstream callers.
    forward_input: bool,
    completions: Completions,
    child: tokio::sync::Mutex<Option<ChildProcess>>,
    listener: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl core::fmt::Debug for RemoteServer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RemoteServer")
            .field("upstream", &self.inner.label)
            .field("server", &self.inner.info.name)
            .field("protocol_version", self.inner.client.protocol_version())
            .finish_non_exhaustive()
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Ok(mut listener) = self.listener.lock()
            && let Some(task) = listener.take()
        {
            task.abort();
        }
    }
}

/// Configures and connects a [`RemoteServer`]; from [`RemoteServer::builder`].
#[derive(Debug)]
#[must_use = "a builder does nothing until connected"]
pub struct RemoteServerBuilder {
    upstream: Upstream,
    auth: OutboundAuth,
    client: ClientBuilder,
    catalog_ttl: Duration,
    shutdown_grace: Duration,
    forward_input: bool,
}

impl RemoteServerBuilder {
    /// Authenticate upstream with `auth`.
    pub fn auth(mut self, auth: OutboundAuth) -> Self {
        self.auth = auth;
        self
    }

    /// Negotiate the upstream revision with `mode` (default
    /// [`ConnectMode::Auto`]: `2026-07-28` where the upstream serves it).
    pub fn connect_mode(mut self, mode: ConnectMode) -> Self {
        self.client = self.client.with_connect_mode(mode);
        self
    }

    /// Fail an upstream request that takes longer than `timeout` (default:
    /// the client's).
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.client = self.client.with_timeout(timeout);
        self
    }

    /// Configure the upstream client further (its identity, observers such
    /// as `ClientTelemetry`, extensions).
    pub fn client(mut self, configure: impl FnOnce(ClientBuilder) -> ClientBuilder) -> Self {
        self.client = configure(self.client);
        self
    }

    /// Trust a fetched catalogue (used to resolve calls) for `ttl` before
    /// fetching it again (default 30 s). A change notification from the
    /// upstream refreshes it at once either way.
    pub fn catalog_ttl(mut self, ttl: Duration) -> Self {
        self.catalog_ttl = ttl;
        self
    }

    /// Whether the upstream may ask downstream callers for input (default
    /// `true`): its `elicitation/create`, `sampling/createMessage` and
    /// `roots/list` reach the caller whose call caused them, on whatever
    /// revision that caller speaks. Off, the proxy declares none of those
    /// capabilities upstream, so the upstream knows not to ask.
    pub fn forward_input(mut self, forward: bool) -> Self {
        self.forward_input = forward;
        self
    }

    /// How long a stdio upstream gets to exit after its stdin closes, and
    /// again after `SIGTERM`, before it is killed (default 2 s each).
    pub fn shutdown_grace(mut self, grace: Duration) -> Self {
        self.shutdown_grace = grace;
        self
    }

    /// Connect and run the handshake.
    ///
    /// # Errors
    /// The upstream failing to start, to connect, or to complete the
    /// handshake.
    pub async fn connect(self) -> Result<RemoteServer, ProxyError> {
        let label = self.upstream.label();
        let (tx, _) = broadcast::channel(CHANGE_BUFFER);
        let completions = crate::bridge::completions();
        let mut client = self.client.with_notifications(ChangeTap(tx.clone()));
        if self.forward_input {
            client = declare_input(client, &completions);
        }
        let (client, child) =
            crate::connect::upstream(&self.upstream, &self.auth, client, self.shutdown_grace)
                .await?;
        let parts = Parts {
            label,
            child,
            changes: tx,
            catalog_ttl: self.catalog_ttl,
            forward_input: self.forward_input,
            completions,
        };
        Ok(RemoteServer::assemble(client, parts).await)
    }
}

impl RemoteServer {
    /// Configure a connection to `upstream`.
    pub fn builder(upstream: Upstream) -> RemoteServerBuilder {
        RemoteServerBuilder {
            upstream,
            auth: OutboundAuth::None,
            client: ClientBuilder::new("turbomcp-proxy", env!("CARGO_PKG_VERSION")),
            catalog_ttl: DEFAULT_CATALOG_TTL,
            shutdown_grace: Duration::from_secs(2),
            forward_input: true,
        }
    }

    /// Connect to `upstream` with the defaults.
    ///
    /// # Errors
    /// As [`RemoteServerBuilder::connect`].
    pub async fn connect(upstream: Upstream) -> Result<Self, ProxyError> {
        Self::builder(upstream).connect().await
    }

    /// Connect over a transport of your own (an in-memory pair, a socket the
    /// embedding application opened), as `client` configures. The upstream's
    /// requests for input are bridged to downstream callers, as by default
    /// with [`builder`](Self::builder); handlers set on `client` for them are
    /// replaced.
    ///
    /// # Errors
    /// The handshake failing.
    pub async fn over<T: Transport>(
        label: impl Into<String>,
        client: ClientBuilder,
        transport: T,
    ) -> Result<Self, ProxyError> {
        let label = label.into();
        let (tx, _) = broadcast::channel(CHANGE_BUFFER);
        let completions = crate::bridge::completions();
        let client = declare_input(client, &completions)
            .with_notifications(ChangeTap(tx.clone()))
            .connect(transport)
            .await
            .map_err(|source| ProxyError::Connect {
                upstream: label.clone(),
                source: Box::new(source),
            })?;
        let parts = Parts {
            label,
            child: None,
            changes: tx,
            catalog_ttl: DEFAULT_CATALOG_TTL,
            forward_input: true,
            completions,
        };
        Ok(Self::assemble(client, parts).await)
    }

    async fn assemble(client: Client, parts: Parts) -> Self {
        let Parts {
            label,
            child,
            changes,
            catalog_ttl,
            forward_input,
            completions,
        } = parts;
        let info = client
            .server_info()
            .cloned()
            .unwrap_or_else(|| Implementation::new(label.clone(), "unknown"));
        let remote = Self {
            inner: Arc::new(Inner {
                instructions: client.instructions().map(str::to_owned),
                capabilities: client.server_capabilities().clone(),
                info,
                label,
                tools: Catalog::new(catalog_ttl),
                prompts: Catalog::new(catalog_ttl),
                resources: Catalog::new(catalog_ttl),
                templates: Catalog::new(catalog_ttl),
                changes,
                forward_input,
                completions,
                child: tokio::sync::Mutex::new(child),
                listener: Mutex::new(None),
                client,
            }),
        };
        remote.listen().await;
        remote.invalidate_on_change();
        remote
    }

    /// On `2026-07-28`, change notifications come only to a
    /// `subscriptions/listen` stream; open one for whatever list changes the
    /// upstream says it announces. (Older revisions send them unasked, and
    /// [`ChangeTap`] already hears them.)
    async fn listen(&self) {
        let client = &self.inner.client;
        if client.protocol_version().is_stateful() {
            return;
        }
        let caps = &self.inner.capabilities;
        let mut filter = neutral::SubscriptionFilter::new();
        filter.tools_list_changed = caps.tools.as_ref().is_some_and(|c| c.list_changed);
        filter.prompts_list_changed = caps.prompts.as_ref().is_some_and(|c| c.list_changed);
        filter.resources_list_changed = caps.resources.as_ref().is_some_and(|c| c.list_changed);
        if !(filter.tools_list_changed
            || filter.prompts_list_changed
            || filter.resources_list_changed)
        {
            return;
        }
        match client.listen(filter).await {
            Ok(mut subscription) => {
                let tx = self.inner.changes.clone();
                let task = tokio::spawn(async move {
                    use turbomcp_client::SubscriptionEvent as Event;
                    while let Some(event) = subscription.next().await {
                        let change = match event {
                            Event::ToolsListChanged => Change::ToolsListChanged,
                            Event::PromptsListChanged => Change::PromptsListChanged,
                            Event::ResourcesListChanged => Change::ResourcesListChanged,
                            Event::ResourceUpdated { uri } => Change::ResourceUpdated { uri },
                            _ => continue,
                        };
                        let _ = tx.send(change);
                    }
                });
                *self.inner.listener.lock().expect("listener lock") = Some(task);
            }
            Err(e) => tracing::warn!(
                upstream = %self.inner.label,
                error = %e,
                "could not subscribe to the upstream's change notifications"
            ),
        }
    }

    /// Drop a catalogue the moment the upstream says its list changed.
    fn invalidate_on_change(&self) {
        let mut rx = self.inner.changes.subscribe();
        let weak = Arc::downgrade(&self.inner);
        tokio::spawn(async move {
            loop {
                let change = match rx.recv().await {
                    Ok(change) => Some(change),
                    Err(broadcast::error::RecvError::Lagged(_)) => None,
                    Err(broadcast::error::RecvError::Closed) => return,
                };
                let Some(inner) = weak.upgrade() else { return };
                match change {
                    Some(Change::ToolsListChanged) => inner.tools.invalidate(),
                    Some(Change::PromptsListChanged) => inner.prompts.invalidate(),
                    Some(Change::ResourcesListChanged) => {
                        inner.resources.invalidate();
                        inner.templates.invalidate();
                    }
                    // Missed some: anything may have changed.
                    None => {
                        inner.tools.invalidate();
                        inner.prompts.invalidate();
                        inner.resources.invalidate();
                        inner.templates.invalidate();
                    }
                    Some(_) => {}
                }
            }
        });
    }

    /// The upstream connection.
    #[must_use]
    pub fn client(&self) -> &Client {
        &self.inner.client
    }

    /// What the upstream advertised in its handshake.
    #[must_use]
    pub fn capabilities(&self) -> &neutral::ServerCapabilities {
        &self.inner.capabilities
    }

    /// The revision the upstream speaks (the downstream speaks any).
    #[must_use]
    pub fn protocol_version(&self) -> &ProtocolVersion {
        self.inner.client.protocol_version()
    }

    /// The upstream's change notifications (`*/list_changed`,
    /// `resources/updated`), for wiring to a downstream server; see
    /// [`forward_changes_to`](Self::forward_changes_to).
    #[must_use]
    pub fn changes(&self) -> broadcast::Receiver<Change> {
        self.inner.changes.subscribe()
    }

    /// Announce the upstream's changes through `notifier`, the downstream
    /// server's (`Server::notifier`), so clients connected to it refresh
    /// their lists. Runs until the upstream connection or the notifier's
    /// server goes away.
    pub fn forward_changes_to(&self, notifier: ServerNotifier) {
        let mut rx = self.changes();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(Change::ToolsListChanged) => notifier.tools_list_changed(),
                    Ok(Change::PromptsListChanged) => notifier.prompts_list_changed(),
                    Ok(Change::ResourcesListChanged) => notifier.resources_list_changed(),
                    Ok(Change::ResourceUpdated { uri }) => notifier.resource_updated(&uri).await,
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        notifier.tools_list_changed();
                        notifier.prompts_list_changed();
                        notifier.resources_list_changed();
                    }
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        });
    }

    /// Serve it: a [`ServerBuilder`] with exactly the capabilities the
    /// upstream advertised, to serve on its own or to
    /// [`mount`](turbomcp_server::Composite::mount).
    #[must_use]
    pub fn into_server(self) -> ServerBuilder<Self> {
        let caps = &self.inner.capabilities;
        let mut router = MethodRouter::new();
        if caps.tools.is_some() {
            router = router.with_tools();
        }
        if caps.resources.is_some() {
            router = router.with_resources();
        }
        if caps.prompts.is_some() {
            router = router.with_prompts();
        }
        if caps.completions {
            router = router.with_completions();
        }
        ServerBuilder::from_parts(self, router)
    }

    /// End the upstream connection. A stdio upstream gets the spec's
    /// shutdown: its stdin closes, then `SIGTERM` if it hasn't exited within
    /// the grace period, then `SIGKILL` (its whole process group, so a
    /// wrapper such as `npx` doesn't leave the real server running).
    pub async fn shutdown(&self) {
        self.inner.client.close().await;
        if let Some(child) = self.inner.child.lock().await.take() {
            child.shutdown().await;
        }
    }

    /// The upstream error as the downstream should see it.
    fn upstream_error(&self, error: &turbomcp_client::ClientError) -> McpError {
        error.to_mcp_error(self.inner.client.protocol_version())
    }

    /// What one downstream call carries upstream: its cancellation, its
    /// progress relayed back, and (when input is forwarded) the upstream's
    /// requests for input put to its caller.
    fn forwarded(
        &self,
        base: &turbomcp_core::RequestContext,
        progress: &turbomcp_server::ProgressReporter,
        handle: &turbomcp_server::ClientHandle,
    ) -> Forwarded {
        let mut options = CallOptions::new().cancel_on(base.cancellation.clone());
        let bridge = self.inner.forward_input.then(|| {
            Bridge::new(
                handle.clone(),
                base.protocol_version.clone(),
                self.inner.completions.clone(),
            )
        });
        if let Some(bridge) = &bridge {
            options = options
                .with_elicitation(bridge.clone())
                .with_sampling(bridge.clone())
                .with_roots(bridge.clone());
        }
        let relay = progress
            .is_requested()
            .then(|| ProgressRelay::new(progress.clone()));
        if let Some(relay) = &relay {
            let tx = relay.tx.clone();
            options = options.on_progress(move |p: neutral::Progress| {
                // Advisory: a caller that can't keep up misses some.
                let _ = tx.try_send(Relayed::Progress(p));
            });
        }
        Forwarded {
            options,
            bridge,
            relay,
        }
    }

    /// Run a forwarded call, ending it (and the upstream request) with the
    /// input-required abort if its bridge asked a `2026-07-28` caller a
    /// question: the caller retries with the answer, and the call runs again.
    /// Progress the upstream reported reaches the caller before the result:
    /// after it, the caller would drop it.
    async fn forward<T>(
        &self,
        forwarded: &Forwarded,
        call: impl Future<Output = Result<T, turbomcp_client::ClientError>>,
    ) -> McpResult<T> {
        let aborted = async {
            match &forwarded.bridge {
                Some(bridge) => bridge.aborted().await,
                None => std::future::pending().await,
            }
        };
        let result = tokio::select! {
            result = call => result.map_err(|e| self.upstream_error(&e)),
            () = aborted => Err(McpError::InputRequired),
        };
        if let Some(relay) = &forwarded.relay {
            relay.flush().await;
        }
        result
    }
}

/// One forwarded call's options, input bridge, and progress relay.
struct Forwarded {
    options: CallOptions,
    bridge: Option<Bridge>,
    relay: Option<ProgressRelay>,
}

/// How many progress updates may wait to be relayed.
const PROGRESS_BUFFER: usize = 64;

enum Relayed {
    Progress(neutral::Progress),
    Flush(tokio::sync::oneshot::Sender<()>),
}

/// Relays an upstream call's progress to its downstream caller, in order.
struct ProgressRelay {
    tx: tokio::sync::mpsc::Sender<Relayed>,
}

impl ProgressRelay {
    fn new(reporter: turbomcp_server::ProgressReporter) -> Self {
        let (tx, mut rx) = tokio::sync::mpsc::channel(PROGRESS_BUFFER);
        tokio::spawn(async move {
            while let Some(relayed) = rx.recv().await {
                match relayed {
                    Relayed::Progress(p) => {
                        reporter
                            .report(p.progress, p.total, p.message.as_deref())
                            .await;
                    }
                    Relayed::Flush(done) => {
                        let _ = done.send(());
                    }
                }
            }
        });
        Self { tx }
    }

    /// Resolves once everything sent before it has been relayed.
    async fn flush(&self) {
        let (done, flushed) = tokio::sync::oneshot::channel();
        if self.tx.send(Relayed::Flush(done)).await.is_ok() {
            let _ = flushed.await;
        }
    }
}

/// The handlers the upstream client declares input capabilities with.
fn declare_input(client: ClientBuilder, completions: &Completions) -> ClientBuilder {
    let handlers = UpstreamHandlers {
        completions: completions.clone(),
    };
    client
        .with_elicitation(handlers.clone())
        .with_sampling(handlers.clone())
        .with_roots(handlers)
}

/// What [`RemoteServer::assemble`] puts together beside the client.
struct Parts {
    label: String,
    child: Option<ChildProcess>,
    changes: broadcast::Sender<Change>,
    catalog_ttl: Duration,
    forward_input: bool,
    completions: Completions,
}

impl McpServerCore for RemoteServer {
    fn server_info(&self) -> Implementation {
        self.inner.info.clone()
    }

    fn instructions(&self) -> Option<String> {
        self.inner.instructions.clone()
    }
}

impl WithTools for RemoteServer {
    async fn lookup_tool(
        &self,
        _ctx: &ListToolsContext,
        name: String,
    ) -> McpResult<Option<neutral::Tool>> {
        let client = &self.inner.client;
        self.inner
            .tools
            .find(&name, || async {
                let tools = client
                    .list_all_tools()
                    .await
                    .map_err(|e| self.upstream_error(&e))?;
                Ok(tools
                    .into_iter()
                    .map(|t| (t.name.clone(), downstream_tool(t)))
                    .collect())
            })
            .await
    }

    async fn list_tools(
        &self,
        _ctx: &ListToolsContext,
        params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        let mut page = self
            .inner
            .client
            .list_tools(params.cursor.as_deref())
            .await
            .map_err(|e| self.upstream_error(&e))?;
        page.tools = page.tools.into_iter().map(downstream_tool).collect();
        Ok(page)
    }

    async fn call_tool(
        &self,
        ctx: &CallToolContext,
        params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        let forwarded = self.forwarded(&ctx.base, &ctx.progress, &ctx.client);
        let call =
            self.inner
                .client
                .call_tool_with(params.name, params.arguments, &forwarded.options);
        self.forward(&forwarded, call).await
    }
}

impl WithResources for RemoteServer {
    async fn lookup_resource(
        &self,
        _ctx: &ListResourcesContext,
        uri: String,
    ) -> McpResult<Option<neutral::Resource>> {
        let client = &self.inner.client;
        self.inner
            .resources
            .find(&uri, || async {
                let resources = client
                    .list_all_resources()
                    .await
                    .map_err(|e| self.upstream_error(&e))?;
                Ok(resources.into_iter().map(|r| (r.uri.clone(), r)).collect())
            })
            .await
    }

    async fn lookup_resource_template(
        &self,
        _ctx: &ListResourceTemplatesContext,
        uri: String,
    ) -> McpResult<Option<neutral::ResourceTemplate>> {
        let client = &self.inner.client;
        let all = self
            .inner
            .templates
            .all(|| async {
                let templates = client
                    .list_all_resource_templates()
                    .await
                    .map_err(|e| self.upstream_error(&e))?;
                Ok(templates
                    .into_iter()
                    .map(|t| (t.uri_template.clone(), t))
                    .collect())
            })
            .await?;
        Ok(all
            .values()
            .find(|t| {
                UriTemplate::parse(&t.uri_template)
                    .is_ok_and(|parsed| parsed.matches(&uri).is_some())
            })
            .cloned())
    }

    async fn list_resources(
        &self,
        _ctx: &ListResourcesContext,
        params: neutral::ListParams,
    ) -> McpResult<neutral::ListResourcesResult> {
        self.inner
            .client
            .list_resources(params.cursor.as_deref())
            .await
            .map_err(|e| self.upstream_error(&e))
    }

    async fn read_resource(
        &self,
        ctx: &ReadResourceContext,
        params: neutral::ReadResourceParams,
    ) -> McpResult<neutral::ReadResourceResult> {
        let forwarded = self.forwarded(&ctx.base, &ctx.progress, &ctx.client);
        let call = self
            .inner
            .client
            .read_resource_with(params.uri, &forwarded.options);
        self.forward(&forwarded, call).await
    }

    async fn list_resource_templates(
        &self,
        _ctx: &ListResourceTemplatesContext,
        params: neutral::ListParams,
    ) -> McpResult<neutral::ListResourceTemplatesResult> {
        self.inner
            .client
            .list_resource_templates(params.cursor.as_deref())
            .await
            .map_err(|e| self.upstream_error(&e))
    }
}

impl WithPrompts for RemoteServer {
    async fn lookup_prompt(
        &self,
        _ctx: &ListPromptsContext,
        name: String,
    ) -> McpResult<Option<neutral::Prompt>> {
        let client = &self.inner.client;
        self.inner
            .prompts
            .find(&name, || async {
                let prompts = client
                    .list_all_prompts()
                    .await
                    .map_err(|e| self.upstream_error(&e))?;
                Ok(prompts.into_iter().map(|p| (p.name.clone(), p)).collect())
            })
            .await
    }

    async fn list_prompts(
        &self,
        _ctx: &ListPromptsContext,
        params: neutral::ListParams,
    ) -> McpResult<neutral::ListPromptsResult> {
        self.inner
            .client
            .list_prompts(params.cursor.as_deref())
            .await
            .map_err(|e| self.upstream_error(&e))
    }

    async fn get_prompt(
        &self,
        ctx: &GetPromptContext,
        params: neutral::GetPromptParams,
    ) -> McpResult<neutral::GetPromptResult> {
        let forwarded = self.forwarded(&ctx.base, &ctx.progress, &ctx.client);
        let call =
            self.inner
                .client
                .get_prompt_with(params.name, params.arguments, &forwarded.options);
        self.forward(&forwarded, call).await
    }
}

impl WithCompletions for RemoteServer {
    async fn complete(
        &self,
        _ctx: &CompleteContext,
        params: neutral::CompleteParams,
    ) -> McpResult<neutral::CompleteResult> {
        self.inner
            .client
            .complete(params)
            .await
            .map_err(|e| self.upstream_error(&e))
    }
}

/// A tool as the downstream sees it. Its task support is the upstream's
/// business: the upstream client drives an upstream task to its result, and
/// whether the proxied call runs as a task downstream is the downstream
/// server's own policy.
fn downstream_tool(mut tool: neutral::Tool) -> neutral::Tool {
    tool.task_support = None;
    tool
}

/// Hears the upstream's change notifications on the stateful revisions.
struct ChangeTap(broadcast::Sender<Change>);

#[async_trait::async_trait]
impl NotificationHandler for ChangeTap {
    async fn on_notification(&self, method: String, params: Option<Value>) {
        use methods::notification as n;
        let change = match method.as_str() {
            n::TOOLS_LIST_CHANGED => Change::ToolsListChanged,
            n::PROMPTS_LIST_CHANGED => Change::PromptsListChanged,
            n::RESOURCES_LIST_CHANGED => Change::ResourcesListChanged,
            n::RESOURCES_UPDATED => match params
                .as_ref()
                .and_then(|p| p.get("uri"))
                .and_then(Value::as_str)
            {
                Some(uri) => Change::ResourceUpdated {
                    uri: uri.to_owned(),
                },
                None => return,
            },
            _ => return,
        };
        let _ = self.0.send(change);
    }
}

/// A catalogue as fetched, and when.
type Snapshot<T> = (Instant, Arc<HashMap<String, T>>);

/// A looked-up catalogue: what resolves a call to its component without an
/// upstream round trip per call. Refreshed when stale, when the upstream
/// says it changed, and once on a miss (a component added upstream without
/// a notification is found on first use).
struct Catalog<T> {
    ttl: Duration,
    cached: Mutex<Option<Snapshot<T>>>,
    refresh: tokio::sync::Mutex<()>,
}

impl<T: Clone> Catalog<T> {
    fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            cached: Mutex::new(None),
            refresh: tokio::sync::Mutex::new(()),
        }
    }

    fn invalidate(&self) {
        *self.cached.lock().expect("catalog lock") = None;
    }

    fn fresh(&self) -> Option<Arc<HashMap<String, T>>> {
        self.cached
            .lock()
            .expect("catalog lock")
            .as_ref()
            .filter(|(at, _)| at.elapsed() < self.ttl)
            .map(|(_, map)| Arc::clone(map))
    }

    /// The whole catalogue, fetched with `fetch` when it isn't fresh.
    async fn all<F, Fut>(&self, fetch: F) -> McpResult<Arc<HashMap<String, T>>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = McpResult<HashMap<String, T>>>,
    {
        if let Some(map) = self.fresh() {
            return Ok(map);
        }
        // One fetch for a stampede of lookups.
        let _refreshing = self.refresh.lock().await;
        if let Some(map) = self.fresh() {
            return Ok(map);
        }
        let map = Arc::new(fetch().await?);
        *self.cached.lock().expect("catalog lock") = Some((Instant::now(), Arc::clone(&map)));
        Ok(map)
    }

    /// The entry under `key`, refetching once if a fresh catalogue lacks it.
    async fn find<F, Fut>(&self, key: &str, fetch: F) -> McpResult<Option<T>>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = McpResult<HashMap<String, T>>>,
    {
        let map = self.all(&fetch).await?;
        if let Some(found) = map.get(key) {
            return Ok(Some(found.clone()));
        }
        // A miss refetches only a catalogue older than a second, so a caller
        // naming components that don't exist can't turn each request into an
        // upstream listing.
        let recent = self
            .cached
            .lock()
            .expect("catalog lock")
            .as_ref()
            .is_some_and(|(at, _)| at.elapsed() < MISS_REFETCH_AFTER);
        if recent {
            return Ok(None);
        }
        self.invalidate();
        Ok(self.all(&fetch).await?.get(key).cloned())
    }
}
