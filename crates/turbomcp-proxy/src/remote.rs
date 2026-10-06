//! [`RemoteServer`]: an upstream MCP server, served as if it were local.

use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::broadcast;
use turbomcp_client::{CallOptions, ClientBuilder, ConnectMode};
use turbomcp_core::{Implementation, McpError, McpResult, ProtocolVersion, RequestContext};
use turbomcp_protocol::neutral;
use turbomcp_server::bus::Change;
use turbomcp_server::{
    CallToolContext, CompleteContext, GetPromptContext, ListPromptsContext,
    ListResourceTemplatesContext, ListResourcesContext, ListToolsContext, McpServerCore,
    MethodRouter, ReadResourceContext, ServerBuilder, ServerNotifier, UriTemplate, WithCompletions,
    WithPrompts, WithResources, WithTools,
};
use turbomcp_service::{EndedSession, SessionObserver, Transport};

use crate::bridge::Bridge;
use crate::link::{Dial, Lease, Link, Linker};
use crate::pool::{DEFAULT_IDLE, DEFAULT_MAX, Pool, UpstreamKey};
use crate::{OutboundAuth, ProxyError, Upstream};

/// How long a looked-up catalogue is trusted before it is fetched again,
/// unless the upstream says it changed first.
const DEFAULT_CATALOG_TTL: Duration = Duration::from_secs(30);

/// How many changes a slow [`RemoteServer::changes`] receiver may fall
/// behind before it skips the oldest.
const CHANGE_BUFFER: usize = 64;

/// An upstream MCP server, served as if it were local.
///
/// It implements the same capability traits a `#[server]` does, by
/// forwarding each call through a [`Client`](turbomcp_client::Client)
/// connected upstream, so it is the same kind of object as a local server:
/// serve it on its own (a protocol bridge, stdio to HTTP, `2025-11-25` to
/// `2026-07-28`), or mount it in a [`Composite`](turbomcp_server::Composite)
/// beside local tools and other remotes, under the composite's
/// authentication and visibility.
///
/// What it advertises is what the upstream advertised in its handshake: a
/// remote without prompts registers no prompts, so capabilities still can't
/// drift from what is actually there.
///
/// The caller's identity and token never travel upstream (no token
/// passthrough): the proxy authenticates as itself ([`OutboundAuth`]).
/// Cancellation, progress, trace context and the upstream's requests for
/// input do travel. Which upstream connection serves a call is its
/// [`UpstreamKey`]; a connection that dies is replaced on the next call.
///
/// Cheap to clone; clones share the upstream connections.
#[derive(Clone)]
pub struct RemoteServer {
    inner: Arc<Shared>,
}

struct Shared {
    label: String,
    info: Implementation,
    instructions: Option<String>,
    capabilities: neutral::ServerCapabilities,
    version: ProtocolVersion,
    changes: broadcast::Sender<Change>,
    /// Bridge the upstream's requests for input to downstream callers.
    forward_input: bool,
    /// Keep the upstream tools' declared scopes, for the gateway to enforce.
    enforce_scopes: bool,
    key: UpstreamKey,
    pool: Pool,
}

impl core::fmt::Debug for RemoteServer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RemoteServer")
            .field("upstream", &self.inner.label)
            .field("server", &self.inner.info.name)
            .field("protocol_version", &self.inner.version)
            .field("key", &self.inner.key)
            .finish_non_exhaustive()
    }
}

/// How a builder reaches its upstream.
enum Target {
    Upstream(Upstream),
    Dial { label: String, dial: Dial },
}

/// Configures and connects a [`RemoteServer`]; from [`RemoteServer::builder`]
/// or [`RemoteServer::dial`].
#[must_use = "a builder does nothing until connected"]
pub struct RemoteServerBuilder {
    target: Target,
    auth: OutboundAuth,
    client: ClientBuilder,
    catalog_ttl: Duration,
    shutdown_grace: Duration,
    forward_input: bool,
    enforce_scopes: Option<bool>,
    key: Option<UpstreamKey>,
    serialize: bool,
    idle: Duration,
    max: u64,
    #[cfg(feature = "http")]
    network: Option<turbomcp_auth::NetworkPolicy>,
}

impl core::fmt::Debug for RemoteServerBuilder {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let target = match &self.target {
            Target::Upstream(upstream) => format!("{upstream:?}"),
            Target::Dial { label, .. } => format!("Dial({label})"),
        };
        f.debug_struct("RemoteServerBuilder")
            .field("target", &target)
            .field("auth", &self.auth)
            .field("key", &self.key)
            .field("forward_input", &self.forward_input)
            .field("serialize", &self.serialize)
            .finish_non_exhaustive()
    }
}

impl RemoteServerBuilder {
    fn new(target: Target) -> Self {
        Self {
            target,
            auth: OutboundAuth::None,
            client: ClientBuilder::new("turbomcp-proxy", env!("CARGO_PKG_VERSION")),
            catalog_ttl: DEFAULT_CATALOG_TTL,
            shutdown_grace: Duration::from_secs(2),
            forward_input: true,
            enforce_scopes: None,
            key: None,
            serialize: false,
            idle: DEFAULT_IDLE,
            max: DEFAULT_MAX,
            #[cfg(feature = "http")]
            network: None,
        }
    }

    /// Hold an HTTP or WebSocket upstream to `policy`: its scheme, and every
    /// address its name resolves to, checked at each connect (so a name
    /// rebound to an internal address is refused). Unset, the operator's
    /// URL is trusted as configured. Set
    /// [`NetworkPolicy::public_only`](turbomcp_auth::NetworkPolicy::public_only)
    /// when upstream URLs come from someone else.
    #[cfg(feature = "http")]
    #[cfg_attr(docsrs, doc(cfg(feature = "http")))]
    pub fn network_policy(mut self, policy: turbomcp_auth::NetworkPolicy) -> Self {
        self.network = Some(policy);
        self
    }

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
    /// as `ClientTelemetry`, extensions). Every upstream connection is made
    /// from it.
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

    /// Whether the gateway holds its callers to the scopes upstream tools
    /// declare (`#[tool(scopes(…))]`, carried in their `_meta`), filtering
    /// lists and refusing calls by them (default `false`, and `true` under
    /// [`OutboundAuth::TokenExchange`]). Those scopes are
    /// about the token presented upstream: the proxy's own, unless it
    /// exchanges the caller's. Turn this on when the gateway's callers and
    /// the upstream share one authorization server and scope vocabulary;
    /// off, the scopes are dropped from what the gateway serves, the
    /// upstream enforces them on the proxy's token, and the gateway's own
    /// access policy is its visibility and its own tools' scopes.
    pub fn enforce_upstream_scopes(mut self, enforce: bool) -> Self {
        self.enforce_scopes = Some(enforce);
        self
    }

    /// Which upstream connection serves a call. The default follows the
    /// upstream: [`UpstreamKey::Global`] where it attributes its requests
    /// for input itself (a `2026-07-28` upstream, or Streamable HTTP), and
    /// [`UpstreamKey::Principal`] otherwise (a `2025-*` stdio or WebSocket
    /// upstream, one connection per caller).
    pub fn key(mut self, key: UpstreamKey) -> Self {
        self.key = Some(key);
        self
    }

    /// Run the calls that may ask for input (`tools/call`, `prompts/get`,
    /// `resources/read`) one at a time per upstream connection (default
    /// off), so a request for input always names its call, even on a
    /// `2025-*` stdio upstream shared by concurrent callers. Costs those
    /// calls their concurrency.
    pub fn serialize_input(mut self, serialize: bool) -> Self {
        self.serialize = serialize;
        self
    }

    /// Close a keyed upstream connection no call has used for `idle`
    /// (default ten minutes).
    pub fn idle_timeout(mut self, idle: Duration) -> Self {
        self.idle = idle;
        self
    }

    /// Keep at most `max` upstream connections open (default 1000), closing
    /// the least recently used past it.
    pub fn max_connections(mut self, max: u64) -> Self {
        self.max = max;
        self
    }

    /// How long a stdio upstream gets to exit after its stdin closes, and
    /// again after `SIGTERM`, before it is killed (default 2 s each).
    pub fn shutdown_grace(mut self, grace: Duration) -> Self {
        self.shutdown_grace = grace;
        self
    }

    /// Connect and run the handshake. This first connection tells the proxy
    /// what the upstream serves, and serves the first key that needs one.
    ///
    /// # Errors
    /// The upstream failing to start, to connect, or to complete the
    /// handshake.
    pub async fn connect(self) -> Result<RemoteServer, ProxyError> {
        #[cfg(feature = "oauth")]
        let for_callers = matches!(self.auth, OutboundAuth::TokenExchange(_));
        #[cfg(not(feature = "oauth"))]
        let for_callers = false;
        if for_callers && self.key == Some(UpstreamKey::Global) {
            return Err(ProxyError::Config(
                "token exchange acts for each caller, so it can't share one connection \
                 (UpstreamKey::Global)"
                    .into(),
            ));
        }
        let (label, dial, by_stream) = match self.target {
            Target::Upstream(upstream) => {
                let label = upstream.label();
                #[cfg(feature = "http")]
                let by_stream = matches!(upstream, Upstream::Http { .. });
                #[cfg(not(feature = "http"))]
                let by_stream = false;
                #[cfg(feature = "http")]
                let credential =
                    crate::connect::Credential::of(&upstream, &self.auth, self.network.as_ref())?;
                let options = Arc::new(crate::connect::ConnectOptions {
                    grace: self.shutdown_grace,
                    #[cfg(feature = "http")]
                    credential,
                    #[cfg(feature = "http")]
                    network: self.network,
                });
                let dial: Dial = Arc::new(move |client, subject| {
                    let (upstream, options) = (upstream.clone(), Arc::clone(&options));
                    Box::pin(async move {
                        crate::connect::upstream(&upstream, &options, client, subject.as_ref())
                            .await
                    })
                });
                (label, dial, by_stream)
            }
            Target::Dial { label, dial } => (label, dial, false),
        };
        let (changes, _) = broadcast::channel(CHANGE_BUFFER);
        let linker = Linker {
            label: label.clone(),
            dial,
            client: self.client,
            catalog_ttl: self.catalog_ttl,
            forward_input: self.forward_input,
            serialize: self.serialize,
            changes: changes.clone(),
        };
        let probe = linker.link(None).await?;
        let client = &probe.client;
        let version = client.protocol_version().clone();
        let key = self.key.unwrap_or(if for_callers {
            UpstreamKey::Principal
        } else if by_stream || !version.is_stateful() {
            UpstreamKey::Global
        } else {
            UpstreamKey::Principal
        });
        let shared = Shared {
            info: client
                .server_info()
                .cloned()
                .unwrap_or_else(|| Implementation::new(label.clone(), "unknown")),
            instructions: client.instructions().map(str::to_owned),
            capabilities: client.server_capabilities().clone(),
            version,
            changes,
            forward_input: self.forward_input,
            enforce_scopes: self.enforce_scopes.unwrap_or(for_callers),
            key,
            label,
            pool: {
                // The startup connection is the gateway's own: under token
                // exchange it must never serve a caller.
                let spare = if for_callers {
                    probe.shutdown().await;
                    None
                } else {
                    Some(probe)
                };
                Pool::new(key, linker, spare, (self.idle, self.max), for_callers)
            },
        };
        Ok(RemoteServer {
            inner: Arc::new(shared),
        })
    }
}

impl RemoteServer {
    /// Configure a connection to `upstream`.
    pub fn builder(upstream: Upstream) -> RemoteServerBuilder {
        RemoteServerBuilder::new(Target::Upstream(upstream))
    }

    /// Connect to `upstream` with the defaults.
    ///
    /// # Errors
    /// As [`RemoteServerBuilder::connect`].
    pub async fn connect(upstream: Upstream) -> Result<Self, ProxyError> {
        Self::builder(upstream).connect().await
    }

    /// Configure an upstream reached over transports of your own: `open`
    /// opens one (an in-process pair, a socket the embedding application
    /// dials) each time the proxy needs a connection.
    pub fn dial<F, Fut, T>(label: impl Into<String>, open: F) -> RemoteServerBuilder
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = std::io::Result<T>> + Send + 'static,
        T: Transport + 'static,
    {
        let label = label.into();
        let name = label.clone();
        let dial: Dial = Arc::new(move |client: ClientBuilder, _subject| {
            let opening = open();
            let upstream = name.clone();
            Box::pin(async move {
                let transport = opening.await.map_err(|source| ProxyError::Dial {
                    upstream: upstream.clone(),
                    source,
                })?;
                let client =
                    client
                        .connect(transport)
                        .await
                        .map_err(|source| ProxyError::Connect {
                            upstream,
                            source: Box::new(source),
                        })?;
                Ok((client, None))
            })
        });
        RemoteServerBuilder::new(Target::Dial { label, dial })
    }

    /// Connect over one transport of your own, as `client` configures: one
    /// connection for every caller ([`UpstreamKey::Global`]), not reopened if
    /// it closes. The upstream's requests for input are bridged to
    /// downstream callers; handlers set on `client` for them are replaced.
    ///
    /// # Errors
    /// The handshake failing.
    pub async fn over<T: Transport + 'static>(
        label: impl Into<String>,
        client: ClientBuilder,
        transport: T,
    ) -> Result<Self, ProxyError> {
        let once = Mutex::new(Some(transport));
        Self::dial(label, move || {
            let transport = once.lock().ok().and_then(|mut slot| slot.take());
            async move {
                transport.ok_or_else(|| {
                    std::io::Error::other("the transport given to `RemoteServer::over` closed")
                })
            }
        })
        .client(|_| client)
        .key(UpstreamKey::Global)
        .connect()
        .await
    }

    /// What the upstream advertised in its handshake.
    #[must_use]
    pub fn capabilities(&self) -> &neutral::ServerCapabilities {
        &self.inner.capabilities
    }

    /// The revision the upstream speaks (the downstream speaks any).
    #[must_use]
    pub fn protocol_version(&self) -> &ProtocolVersion {
        &self.inner.version
    }

    /// Which upstream connection serves a call.
    #[must_use]
    pub fn key(&self) -> UpstreamKey {
        self.inner.key
    }

    /// The upstream's change notifications (`*/list_changed`,
    /// `resources/updated`), from every connection, for wiring to a
    /// downstream server; see [`forward_changes_to`](Self::forward_changes_to).
    #[must_use]
    pub fn changes(&self) -> broadcast::Receiver<Change> {
        self.inner.changes.subscribe()
    }

    /// Announce the upstream's changes through `notifier`, the downstream
    /// server's (`Server::notifier`), so clients connected to it refresh
    /// their lists. Runs until the remote or the notifier's server goes
    /// away.
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

    /// End every upstream connection. A stdio upstream gets the spec's
    /// shutdown: its stdin closes, then `SIGTERM` if it hasn't exited within
    /// the grace period, then `SIGKILL` (its whole process group, so a
    /// wrapper such as `npx` doesn't leave the real server running).
    pub async fn shutdown(&self) {
        self.inner.pool.shutdown().await;
    }

    /// The connection that serves `base`.
    async fn link(&self, base: &RequestContext) -> McpResult<Lease> {
        self.inner.pool.get(base).await
    }

    /// The upstream error as the downstream should see it.
    fn upstream_error(&self, error: &turbomcp_client::ClientError) -> McpError {
        error.to_mcp_error(&self.inner.version)
    }

    /// What one downstream call carries upstream: its cancellation, its
    /// progress relayed back, and (when input is forwarded) the upstream's
    /// requests for input put to its caller.
    fn forwarded(
        &self,
        link: &Link,
        base: &RequestContext,
        progress: &turbomcp_server::ProgressReporter,
        handle: &turbomcp_server::ClientHandle,
    ) -> Forwarded {
        let mut options = CallOptions::new().cancel_on(base.cancellation.clone());
        let bridge = self.inner.forward_input.then(|| {
            Bridge::new(
                handle.clone(),
                base.protocol_version.clone(),
                link.completions.clone(),
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
        link: &Link,
        forwarded: &Forwarded,
        call: impl Future<Output = Result<T, turbomcp_client::ClientError>>,
    ) -> McpResult<T> {
        let _turn = link.turn().await;
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

/// Ends a [`UpstreamKey::Session`] remote's upstream connection when its
/// downstream session ends: register it with
/// `ServerBuilder::observe_sessions`. (Unregistered, the connection closes
/// once idle.)
impl SessionObserver for RemoteServer {
    fn session_ended(&self, session: &EndedSession<'_>) {
        self.inner.pool.end_session(session.id);
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
        ctx: &ListToolsContext,
        name: String,
    ) -> McpResult<Option<neutral::Tool>> {
        let link = self.link(&ctx.base).await?;
        let client = &link.client;
        link.tools
            .find(&name, || async {
                let tools = client
                    .list_all_tools()
                    .await
                    .map_err(|e| self.upstream_error(&e))?;
                Ok(tools
                    .into_iter()
                    .map(|t| (t.name.clone(), self.downstream_tool(t)))
                    .collect())
            })
            .await
    }

    async fn list_tools(
        &self,
        ctx: &ListToolsContext,
        params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        let link = self.link(&ctx.base).await?;
        let mut page = link
            .client
            .list_tools(params.cursor.as_deref())
            .await
            .map_err(|e| self.upstream_error(&e))?;
        page.tools = page
            .tools
            .into_iter()
            .map(|t| self.downstream_tool(t))
            .collect();
        Ok(page)
    }

    async fn call_tool(
        &self,
        ctx: &CallToolContext,
        params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        let link = self.link(&ctx.base).await?;
        let forwarded = self.forwarded(&link, &ctx.base, &ctx.progress, &ctx.client);
        let call = link
            .client
            .call_tool_with(params.name, params.arguments, &forwarded.options);
        self.forward(&link, &forwarded, call).await
    }
}

impl WithResources for RemoteServer {
    async fn lookup_resource(
        &self,
        ctx: &ListResourcesContext,
        uri: String,
    ) -> McpResult<Option<neutral::Resource>> {
        let link = self.link(&ctx.base).await?;
        let client = &link.client;
        link.resources
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
        ctx: &ListResourceTemplatesContext,
        uri: String,
    ) -> McpResult<Option<neutral::ResourceTemplate>> {
        let link = self.link(&ctx.base).await?;
        let client = &link.client;
        let all = link
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
        ctx: &ListResourcesContext,
        params: neutral::ListParams,
    ) -> McpResult<neutral::ListResourcesResult> {
        let link = self.link(&ctx.base).await?;
        link.client
            .list_resources(params.cursor.as_deref())
            .await
            .map_err(|e| self.upstream_error(&e))
    }

    async fn read_resource(
        &self,
        ctx: &ReadResourceContext,
        params: neutral::ReadResourceParams,
    ) -> McpResult<neutral::ReadResourceResult> {
        let link = self.link(&ctx.base).await?;
        let forwarded = self.forwarded(&link, &ctx.base, &ctx.progress, &ctx.client);
        let call = link
            .client
            .read_resource_with(params.uri, &forwarded.options);
        self.forward(&link, &forwarded, call).await
    }

    async fn list_resource_templates(
        &self,
        ctx: &ListResourceTemplatesContext,
        params: neutral::ListParams,
    ) -> McpResult<neutral::ListResourceTemplatesResult> {
        let link = self.link(&ctx.base).await?;
        link.client
            .list_resource_templates(params.cursor.as_deref())
            .await
            .map_err(|e| self.upstream_error(&e))
    }
}

impl WithPrompts for RemoteServer {
    async fn lookup_prompt(
        &self,
        ctx: &ListPromptsContext,
        name: String,
    ) -> McpResult<Option<neutral::Prompt>> {
        let link = self.link(&ctx.base).await?;
        let client = &link.client;
        link.prompts
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
        ctx: &ListPromptsContext,
        params: neutral::ListParams,
    ) -> McpResult<neutral::ListPromptsResult> {
        let link = self.link(&ctx.base).await?;
        link.client
            .list_prompts(params.cursor.as_deref())
            .await
            .map_err(|e| self.upstream_error(&e))
    }

    async fn get_prompt(
        &self,
        ctx: &GetPromptContext,
        params: neutral::GetPromptParams,
    ) -> McpResult<neutral::GetPromptResult> {
        let link = self.link(&ctx.base).await?;
        let forwarded = self.forwarded(&link, &ctx.base, &ctx.progress, &ctx.client);
        let call = link
            .client
            .get_prompt_with(params.name, params.arguments, &forwarded.options);
        self.forward(&link, &forwarded, call).await
    }
}

impl WithCompletions for RemoteServer {
    async fn complete(
        &self,
        ctx: &CompleteContext,
        params: neutral::CompleteParams,
    ) -> McpResult<neutral::CompleteResult> {
        let link = self.link(&ctx.base).await?;
        link.client
            .complete(params)
            .await
            .map_err(|e| self.upstream_error(&e))
    }
}

impl RemoteServer {
    /// A tool as the downstream sees it. Its task support is the upstream's
    /// business: the upstream client drives an upstream task to its result,
    /// and whether the proxied call runs as a task downstream is the
    /// downstream server's own policy. Its declared scopes are the
    /// upstream's too, unless the gateway enforces them.
    fn downstream_tool(&self, mut tool: neutral::Tool) -> neutral::Tool {
        tool.task_support = None;
        if !self.inner.enforce_scopes {
            tool.meta.remove(turbomcp_core::meta::keys::SCOPES);
        }
        tool
    }
}
