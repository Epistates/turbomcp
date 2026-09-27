//! The typed, version-negotiated MCP client.
//!
//! [`Client`] wraps a raw [`Connection`] with everything a protocol-aware client
//! needs: it runs the `initialize` / `server/discover` handshake, remembers the
//! negotiated [`ProtocolVersion`], stamps the modern `_meta` envelope onto every
//! outbound request, and decodes results from the negotiated version's wire
//! shape into version-stable [`neutral`] types.
//!
//! Build one with [`ClientBuilder`]:
//!
//! ```no_run
//! # async fn f(transport: impl turbomcp_service::Transport) -> turbomcp_client::ClientResult<()> {
//! use turbomcp_client::ClientBuilder;
//! let client = ClientBuilder::new("my-client", "1.0.0").connect(transport).await?;
//! let tools = client.list_tools(None).await?;
//! # Ok(()) }
//! ```

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};
use turbomcp_core::meta::keys;
use turbomcp_core::{Implementation, LogLevel, ProtocolVersion, codes};
use turbomcp_protocol::methods::{notification, request};
use turbomcp_protocol::neutral;
use turbomcp_protocol::v2025_11_25::types as legacy;
use turbomcp_protocol::v2026_07_28::types as v0728;
use turbomcp_service::{Transport, mcp_headers};

use crate::cache::ResponseCache;
use crate::connection::Connection;
use crate::error::{ClientError, ClientResult};
use crate::handler::{
    ClientHandlers, ElicitationHandler, NotificationHandler, RootsHandler, SamplingHandler,
    dispatch_server_request,
};

/// Cap on MRTR re-issue rounds — a guard against a server that keeps answering
/// `input_required` forever.
const MAX_MRTR_ROUNDS: usize = 16;

/// Cap on pages the `list_all_*` helpers will follow — a guard against a
/// server whose `nextCursor` never terminates. Set far above any real catalog
/// (even at one item per page) so it only ever trips on a broken server.
const MAX_LIST_PAGES: usize = 10_000;

/// `resultType: "task"` marks a `CreateTaskResult` (SEP-2663).
const RESULT_TYPE_TASK: &str = "task";

/// The Tasks extension's identifier (SEP-2663).
const TASKS_EXTENSION: &str = "io.modelcontextprotocol/tasks";
/// Poll cadence when the server suggests none, and the floor applied to a
/// server-suggested `pollIntervalMs` (protects the server from a zero value).
const DEFAULT_TASK_POLL_MS: u64 = 500;
const MIN_TASK_POLL_MS: u64 = 10;

/// What replaced per-resource subscriptions in `2026-07-28`.
const LISTEN_INSTEAD: &str =
    "use `listen` with `SubscriptionFilter::with_resource` on this revision";

/// Internal `_meta` key carrying the `#[mcp_header]` mirror map — header-name
/// portion → already-encoded header value — to emit as `Mcp-Param-*` headers.
/// Consumed and stripped by the HTTP transport (and sanitized server-side as
/// an `io.turbomcp.internal/*` key on other transports), so it never reaches
/// a handler.
pub(crate) const HEADER_PARAMS_META_KEY: &str = "io.turbomcp.internal/headerParams";

/// Internal `_meta` key carrying the negotiated protocol version for the HTTP
/// transport's `MCP-Protocol-Version` header (required on every POST by both
/// versions' transports specs). Stripped by the HTTP transport; sanitized at
/// every server boundary otherwise.
pub(crate) const NEGOTIATED_VERSION_META_KEY: &str = "io.turbomcp.internal/negotiatedVersion";

/// Marks a transport-synthesized error as "the response stream ended before
/// the response" (set in its `data`). 2026-07-28: "A broken response stream
/// loses the in-flight request; clients MUST re-issue it as a new request with
/// a new request ID."
pub(crate) const STREAM_LOST: &str = "io.turbomcp.internal/streamLost";

/// Methods safe to re-issue after their response stream was lost: they change
/// nothing on the server. `tools/call` is not among them — re-running a tool
/// could repeat its side effect — so its caller sees the error instead.
const REISSUABLE: &[&str] = &[
    request::TOOLS_LIST,
    request::RESOURCES_LIST,
    request::RESOURCES_TEMPLATES_LIST,
    request::RESOURCES_READ,
    request::PROMPTS_LIST,
    request::PROMPTS_GET,
    request::COMPLETION_COMPLETE,
    request::DISCOVER,
];

/// How long [`ConnectMode::Auto`] waits for its `server/discover` probe (or the
/// request timeout, if shorter) before concluding the server is legacy.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// How a [`Client`] decides which protocol version to speak.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConnectMode {
    /// Probe the modern (`server/discover`) path first, and fall back to the
    /// legacy `initialize` handshake when the server turns out to be legacy:
    /// any error that is not a recognized modern one, or no answer within a
    /// few seconds (2026-07-28 stdio/HTTP §Backward Compatibility). The
    /// default.
    #[default]
    Auto,
    /// Force the modern, stateless `2026-07-28` path (`server/discover`).
    Modern,
    /// Force the legacy `2025-11-25` path (`initialize` + `notifications/initialized`).
    Legacy,
}

/// Builds a [`Client`]: identity, advertised capabilities, connect mode, and
/// timeout, then [`connect`](ClientBuilder::connect)s over a transport.
#[derive(Clone)]
pub struct ClientBuilder {
    client_info: Implementation,
    experimental: Option<Map<String, Value>>,
    extensions: Option<Map<String, Value>>,
    connect_mode: ConnectMode,
    request_timeout: Duration,
    handler: ClientHandlers,
    response_cache: bool,
    log_level: Option<LogLevel>,
}

impl fmt::Debug for ClientBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientBuilder")
            .field("client_info", &self.client_info)
            .field("capabilities", &self.capabilities())
            .field("connect_mode", &self.connect_mode)
            .field("request_timeout", &self.request_timeout)
            // Handlers are user trait objects; which features are served is
            // the part that explains behaviour.
            .field("handler", &self.handler)
            .field("response_cache", &self.response_cache)
            .field("log_level", &self.log_level)
            .finish()
    }
}

impl ClientBuilder {
    /// Start a builder for a client identifying as `name`/`version`.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            client_info: Implementation::new(name, version),
            experimental: None,
            extensions: None,
            connect_mode: ConnectMode::Auto,
            request_timeout: crate::connection::DEFAULT_REQUEST_TIMEOUT,
            handler: ClientHandlers::default(),
            response_cache: true,
            log_level: None,
        }
    }

    /// What this client advertises, derived from its registered handlers.
    #[must_use]
    pub fn capabilities(&self) -> neutral::ClientCapabilities {
        let mut caps = self.handler.capabilities();
        caps.experimental = self.experimental.clone();
        caps.extensions = self.extensions.clone();
        caps
    }

    /// Answer `elicitation/create`, declaring `elicitation` (and
    /// `elicitation.url` when the handler says it supports URL mode).
    #[must_use]
    pub fn with_elicitation<H: ElicitationHandler>(mut self, handler: H) -> Self {
        self.handler.elicitation = Some(Arc::new(handler));
        self
    }

    /// Answer `sampling/createMessage`, declaring `sampling` (and whichever of
    /// `context` / `tools` the handler reports).
    #[must_use]
    pub fn with_sampling<H: SamplingHandler>(mut self, handler: H) -> Self {
        self.handler.sampling = Some(Arc::new(handler));
        self
    }

    /// Answer `roots/list`, declaring `roots` (and `roots.listChanged` when the
    /// handler says it emits the notification).
    #[must_use]
    pub fn with_roots<H: RootsHandler>(mut self, handler: H) -> Self {
        self.handler.roots = Some(Arc::new(handler));
        self
    }

    /// Observe server→client notifications. Declares no capability.
    #[must_use]
    pub fn with_notifications<H: NotificationHandler>(mut self, handler: H) -> Self {
        self.handler.notifications = Some(Arc::new(handler));
        self
    }

    /// Add non-standard `experimental` capabilities. The standard ones are
    /// derived from the registered handlers and cannot be set by hand: a
    /// declaration that disagrees with the implementation is a bug neither side
    /// can see, so there is one source for both.
    #[must_use]
    pub fn with_experimental(mut self, experimental: Map<String, Value>) -> Self {
        self.experimental = Some(experimental);
        self
    }

    /// Declare participation in an extension (`2026-07-28` `extensions`
    /// capability); older wires have no such field and drop it.
    #[must_use]
    pub fn with_extension(mut self, id: impl Into<String>, value: Value) -> Self {
        self.extensions
            .get_or_insert_with(Map::new)
            .insert(id.into(), value);
        self
    }

    /// Choose the connect mode (default [`ConnectMode::Auto`]).
    #[must_use]
    pub fn with_connect_mode(mut self, mode: ConnectMode) -> Self {
        self.connect_mode = mode;
        self
    }

    /// Set the per-request timeout (default 60s).
    ///
    /// Elapsing cancels the request on the server as well as failing it here,
    /// so a handler stops working rather than finishing an answer no one will
    /// read. Dropping a call's future does the same, which makes racing one
    /// against your own deadline safe. The transport decides the mechanism:
    /// `notifications/cancelled` on stdio and WebSocket, closing the request's
    /// response stream on HTTP.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Enable or disable the SEP-2549 response cache (default: enabled).
    ///
    /// When enabled, `*/list` and `resources/read` results whose server
    /// declared a positive `ttlMs` are served from memory until they expire
    /// or a `*_list_changed` / `resources/updated` notification invalidates
    /// them. Servers that don't opt in (`ttlMs: 0`, the wire default — and
    /// every `2025-11-25` server, whose wire has no cache fields) are never
    /// cached, so enabling this is safe against any server.
    #[must_use]
    pub fn with_response_cache(mut self, enabled: bool) -> Self {
        self.response_cache = enabled;
        self
    }

    /// Ask the server for `notifications/message` at `level` and above, on
    /// whichever revision gets negotiated.
    ///
    /// A server sends no log messages until a client opts in, and the two
    /// wires opt in differently: `2026-07-28` carries the level in every
    /// request's `_meta` ("If absent, the server MUST NOT send any
    /// `notifications/message` notifications for this request"), while
    /// `2025-11-25` has the session-wide `logging/setLevel`, which the
    /// handshake sends when the server declared `logging`. Messages arrive
    /// at [`NotificationHandler::on_notification`]. Logging is deprecated as
    /// of `2026-07-28` (SEP-2577) but remains in the spec.
    #[must_use]
    pub fn with_log_level(mut self, level: LogLevel) -> Self {
        self.log_level = Some(level);
        self
    }

    /// Spawn the connection over `transport`, run the handshake, and return a
    /// ready [`Client`].
    ///
    /// # Errors
    /// [`ClientError::Protocol`] if no supported version could be negotiated, or
    /// the underlying connection/handshake failure.
    pub async fn connect<T>(self, transport: T) -> ClientResult<Client>
    where
        T: Transport,
    {
        let cache = (self.response_cache && transport.allows_response_cache())
            .then(|| Arc::new(ResponseCache::default()));
        let conn = Connection::connect_with_cache(
            transport,
            self.request_timeout,
            self.handler.clone(),
            cache.clone(),
        );
        self.handshake(conn, cache).await
    }

    /// Drive the handshake per the configured mode, returning a [`Client`].
    async fn handshake(
        self,
        conn: Connection,
        cache: Option<Arc<ResponseCache>>,
    ) -> ClientResult<Client> {
        let outcome = match self.connect_mode {
            ConnectMode::Modern => self.modern_handshake(&conn).await?,
            ConnectMode::Legacy => self.legacy_handshake(&conn).await?,
            ConnectMode::Auto => match self.probe(&conn).await? {
                Some(modern) => modern,
                None => self.legacy_handshake(&conn).await?,
            },
        };

        // The actor spawns before the handshake, so it learns the revision
        // here. It shapes the *replies* to server→client requests, which differ
        // between wires.
        conn.set_negotiated_version(outcome.version.clone());

        // Precompute the modern `_meta` envelope (protocol version + identity)
        // merged into every request on the stateless draft path.
        let mut request_meta = Map::new();
        request_meta.insert(
            keys::PROTOCOL_VERSION.into(),
            json!(outcome.version.as_str()),
        );
        request_meta.insert(
            keys::CLIENT_INFO.into(),
            serde_json::to_value(&self.client_info).unwrap_or(Value::Null),
        );
        request_meta.insert(
            keys::CLIENT_CAPABILITIES.into(),
            self.capabilities().to_wire(outcome.version.clone()),
        );
        if let Some(level) = self.log_level
            && outcome.version.is_stateless()
        {
            request_meta.insert(keys::LOG_LEVEL.into(), json!(level));
        }

        let client = Client {
            conn,
            version: outcome.version,
            server_info: outcome.server_info,
            server_capabilities: outcome.server_capabilities,
            instructions: outcome.instructions,
            request_meta,
            handler: self.handler.clone(),
            tools: Arc::new(Mutex::new(HashMap::new())),
            cache,
        };
        if let Some(level) = self.log_level
            && !client.version.is_stateless()
            && client.server_supports("logging")
        {
            #[allow(deprecated)]
            client.set_level(level).await?;
        }
        Ok(client)
    }

    /// The modern, stateless handshake: a single `server/discover`.
    ///
    /// # Errors
    /// [`ClientError::Protocol`] when the server answers but its
    /// `supportedVersions` doesn't include the version this client speaks
    /// statelessly. `server/discover` is version-agnostic — a server that
    /// serves only stateful revisions still answers it — so a successful
    /// response is not on its own evidence that the draft path will work.
    /// [`ConnectMode::Auto`] treats this as "fall back to `initialize`".
    async fn modern_handshake(&self, conn: &Connection) -> ClientResult<Handshake> {
        let version = ProtocolVersion::LATEST;
        let result = self.discover(conn).await?;

        // The server lists what it serves; believe it. Proceeding on a
        // successful discover alone would connect "fine" and then fail every
        // subsequent request, since each one restates the version.
        if let Some(supported) = result.get("supportedVersions").and_then(Value::as_array)
            && !supported
                .iter()
                .filter_map(Value::as_str)
                .any(|v| v == version.as_str())
        {
            return Err(ClientError::Protocol(format!(
                "server does not serve `{version}` (it lists {supported:?})"
            )));
        }

        Ok(Handshake::from_result(version, &result))
    }

    /// [`ConnectMode::Auto`]'s era probe: `Some` for a modern server, `None`
    /// for a legacy one, `Err` when the answer rules out both.
    ///
    /// 2026-07-28 §Backward Compatibility (stdio): "The server returns a
    /// recognized modern JSON-RPC error such as `UnsupportedProtocolVersionError`:
    /// … Do **not** fall back to `initialize`. The server returns any other
    /// error, or does not respond within a reasonable timeout: the server is
    /// legacy … The fallback **MUST NOT** be keyed to one specific error code."
    /// And over HTTP a `400` whose body is not a recognized modern error means
    /// the same. Falling back only on `-32601` stranded python-sdk and FastMCP
    /// servers (`-32602`), go-sdk HTTP servers (a plain-text 400), and any
    /// server that ignores unknown methods (the full request timeout).
    async fn probe(&self, conn: &Connection) -> ClientResult<Option<Handshake>> {
        let wait = self.request_timeout.min(PROBE_TIMEOUT);
        let result = match tokio::time::timeout(wait, self.discover(conn)).await {
            // "does not respond within a reasonable timeout: the server is legacy"
            Err(_elapsed) => return Ok(None),
            Ok(Ok(result)) => result,
            Ok(Err(error)) => return probe_error(error),
        };
        let listed: Vec<ProtocolVersion> = result
            .get("supportedVersions")
            .and_then(Value::as_array)
            .map(|v| {
                v.iter()
                    .filter_map(Value::as_str)
                    .map(ProtocolVersion::from_wire)
                    .collect()
            })
            .unwrap_or_else(|| vec![ProtocolVersion::LATEST]);
        if listed.contains(&ProtocolVersion::LATEST) {
            return Ok(Some(Handshake::from_result(
                ProtocolVersion::LATEST,
                &result,
            )));
        }
        // Answered, but serving only revisions reached through `initialize`.
        if listed.iter().any(ProtocolVersion::is_stateful) {
            return Ok(None);
        }
        Err(ClientError::Protocol(format!(
            "server serves {listed:?}, none of which this client speaks"
        )))
    }

    /// One `server/discover`, retried once if the server says the version is
    /// unsupported while listing it (a server that changed its mind between
    /// probe and retry).
    async fn discover(&self, conn: &Connection) -> ClientResult<Value> {
        let version = ProtocolVersion::LATEST;
        let mut meta = Map::new();
        meta.insert(keys::PROTOCOL_VERSION.into(), json!(version.as_str()));
        meta.insert(
            keys::CLIENT_INFO.into(),
            serde_json::to_value(&self.client_info).unwrap_or(Value::Null),
        );
        meta.insert(
            keys::CLIENT_CAPABILITIES.into(),
            self.capabilities().to_wire(version.clone()),
        );
        let params = json!({ "_meta": Value::Object(meta) });

        let result = match conn.request(request::DISCOVER, Some(params.clone())).await {
            Err(error)
                if error.rpc_code() == Some(codes::UNSUPPORTED_PROTOCOL_VERSION)
                    && error
                        .as_rpc()
                        .and_then(|rpc| rpc.data.as_ref())
                        .and_then(|data| data.get("supported"))
                        .and_then(Value::as_array)
                        .is_some_and(|versions| {
                            versions
                                .iter()
                                .any(|v| v.as_str() == Some(version.as_str()))
                        }) =>
            {
                conn.request(request::DISCOVER, Some(params)).await?
            }
            other => other?,
        };
        Ok(result)
    }

    /// The legacy, stateful handshake: `initialize` then `notifications/initialized`.
    ///
    /// # Errors
    /// [`ClientError::Protocol`] when the server answers a version this build
    /// cannot speak — the lifecycle spec's instruction in that case is to
    /// disconnect, not to keep talking.
    async fn legacy_handshake(&self, conn: &Connection) -> ClientResult<Handshake> {
        let requested = ProtocolVersion::V2025_11_25;
        let params = json!({
            "protocolVersion": requested.as_str(),
            "capabilities": self.capabilities().to_wire(requested.clone()),
            "clientInfo": serde_json::to_value(&self.client_info).unwrap_or(Value::Null),
        });
        let result = conn.request(request::INITIALIZE, Some(params)).await?;

        // The *server* picks: it echoes what we asked for when it serves it,
        // otherwise it names another version it does serve (an older server
        // answers `2025-06-18`). Speaking on in the requested shapes regardless
        // would mean sending it fields it has never heard of, so adopt its
        // answer — or give up, which is what the spec says to do when the
        // answer is unusable.
        let negotiated = result
            .get("protocolVersion")
            .and_then(Value::as_str)
            .map_or_else(|| requested.clone(), ProtocolVersion::from_wire);
        if !negotiated.is_stateful() {
            return Err(ClientError::Protocol(format!(
                "server negotiated `{negotiated}`, which this client cannot speak \
                 over the `initialize` handshake (it serves {:?})",
                ProtocolVersion::STATEFUL,
            )));
        }

        // Per the lifecycle spec, the client confirms readiness before issuing
        // further requests.
        //
        // It carries the negotiated-version signal, which is the first frame
        // able to: `initialize` itself has nothing to declare yet. The HTTP
        // transport turns it into the `MCP-Protocol-Version` header the
        // transports spec requires from the first post-`initialize` message
        // onward — this notification is that message — and strips it from the
        // body; other transports sanitize it at the server boundary. Learning
        // the version here is also what lets the transport open its
        // server→client stream now rather than waiting for a request the
        // client may never make.
        // The actor shapes its replies to server requests by revision, and a
        // compliant server may send one the moment `initialized` arrives — so
        // it learns the revision first.
        conn.set_negotiated_version(negotiated.clone());
        conn.notify(
            notification::INITIALIZED,
            Some(json!({
                "_meta": { NEGOTIATED_VERSION_META_KEY: negotiated.as_str() },
            })),
        )
        .await?;
        Ok(Handshake::from_result(negotiated, &result))
    }
}

/// Whether `error` is the transport's "the response stream ended first".
fn stream_lost(error: &ClientError) -> bool {
    error
        .as_rpc()
        .and_then(|rpc| rpc.data.as_ref())
        .and_then(|data| data.get(STREAM_LOST))
        .is_some_and(|flag| flag == true)
}

/// What a failed `server/discover` probe says about the server.
fn probe_error(error: ClientError) -> ClientResult<Option<Handshake>> {
    // A recognized modern error: the server is modern, and falling back to
    // `initialize` is what the spec says not to do — unless the versions it
    // does serve are ones reached through `initialize`.
    if let Some(rpc) = error.as_rpc() {
        if rpc.code == codes::UNSUPPORTED_PROTOCOL_VERSION {
            let supported: Vec<ProtocolVersion> = rpc
                .data
                .as_ref()
                .and_then(|d| d.get("supported"))
                .and_then(Value::as_array)
                .map(|v| {
                    v.iter()
                        .filter_map(Value::as_str)
                        .map(ProtocolVersion::from_wire)
                        .collect()
                })
                .unwrap_or_default();
            if supported.iter().any(ProtocolVersion::is_stateful) {
                return Ok(None);
            }
            return Err(ClientError::Protocol(format!(
                "server serves {supported:?}, none of which this client speaks"
            )));
        }
        if matches!(
            rpc.code,
            codes::MISSING_REQUIRED_CLIENT_CAPABILITY | codes::HEADER_MISMATCH
        ) {
            return Err(error);
        }
        // Any other JSON-RPC error — `-32601`, `-32602`, `-32600`, an
        // implementation-defined code — is a legacy server that did not know
        // the method.
        return Ok(None);
    }
    match &error {
        // A 4xx with no recognized modern error in the body (go-sdk answers
        // with plain text): legacy. Authentication and rate limiting say
        // nothing about the era, so they stay errors.
        ClientError::Http(failure)
            if (400..500).contains(&failure.status)
                && !matches!(failure.status, 401 | 403 | 407 | 429) =>
        {
            Ok(None)
        }
        // The connection's own timeout is the probe timeout's slower cousin.
        ClientError::Timeout => Ok(None),
        _ => Err(error),
    }
}

/// The negotiated facts extracted from a handshake result.
struct Handshake {
    version: ProtocolVersion,
    server_info: Option<Implementation>,
    server_capabilities: Value,
    instructions: Option<String>,
}

impl Handshake {
    /// Pull `serverInfo` / `capabilities` / `instructions` out of an
    /// `initialize` or `server/discover` result; missing fields degrade
    /// gracefully rather than fail the handshake.
    ///
    /// The two wires disagree on where the server identity lives, so both
    /// places are read. `initialize` (`2025-06-18`/`2025-11-25`) carries a
    /// top-level `serverInfo`; the frozen `2026-07-28` moved it into the
    /// result's `_meta` under `io.modelcontextprotocol/serverInfo`, having
    /// briefly promoted it back to a field during the RC. Preferring `_meta`
    /// costs nothing on the legacy wire, which never sets it.
    fn from_result(version: ProtocolVersion, result: &Value) -> Self {
        Self {
            version,
            server_info: result
                .get("_meta")
                .and_then(|m| m.get(keys::SERVER_INFO))
                .or_else(|| result.get("serverInfo"))
                .and_then(|v| serde_json::from_value(v.clone()).ok()),
            server_capabilities: result
                .get("capabilities")
                .cloned()
                .unwrap_or(Value::Object(Map::new())),
            instructions: result
                .get("instructions")
                .and_then(Value::as_str)
                .map(String::from),
        }
    }
}

/// A connected, version-negotiated MCP client.
///
/// Cheaply [`Clone`]able (clones share the connection). All methods speak
/// version-stable [`neutral`] types; the client handles version stamping and
/// wire decoding internally, so calling code never branches on the protocol
/// version.
#[derive(Clone)]
pub struct Client {
    conn: Connection,
    version: ProtocolVersion,
    server_info: Option<Implementation>,
    server_capabilities: Value,
    instructions: Option<String>,
    request_meta: Map<String, Value>,
    handler: ClientHandlers,
    /// Tool name → what `list_tools` last said about it: the `x-mcp-header`
    /// mirrors, its `taskSupport`, and its compiled `outputSchema`. Consulted
    /// on every `call_tool`.
    tools: Arc<Mutex<HashMap<String, ToolFacts>>>,
    /// The SEP-2549 response cache (`None` = disabled at build time). Shared
    /// with the connection actor, which invalidates on notifications.
    cache: Option<Arc<ResponseCache>>,
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `request_meta` is deliberately absent: it carries this client's
        // declared capabilities and whatever `_meta` the caller stamped, which
        // is the one field here that could hold something sensitive.
        f.debug_struct("Client")
            .field("version", &self.version)
            .field("server_info", &self.server_info)
            .field("server_capabilities", &self.server_capabilities)
            .field("instructions", &self.instructions)
            .field("handler", &self.handler)
            .field("response_cache", &self.cache.is_some())
            .finish_non_exhaustive()
    }
}

impl Client {
    /// The protocol version negotiated at connect time.
    #[must_use]
    pub fn protocol_version(&self) -> &ProtocolVersion {
        &self.version
    }

    /// The server's advertised identity, if it provided one.
    #[must_use]
    pub fn server_info(&self) -> Option<&Implementation> {
        self.server_info.as_ref()
    }

    /// The server's advertised capabilities (raw JSON, version-shaped).
    #[must_use]
    pub fn server_capabilities(&self) -> &Value {
        &self.server_capabilities
    }

    /// The server's optional usage instructions.
    #[must_use]
    pub fn instructions(&self) -> Option<&str> {
        self.instructions.as_deref()
    }

    /// Whether the server declared `capability`, which may name a
    /// sub-capability with a dotted path (`resources.subscribe`,
    /// `tools.listChanged`).
    ///
    /// A sub-capability is a boolean on the wire, so "declared" means present
    /// and not literally `false`: a server answering `{"subscribe": false}` has
    /// said no, and reading that as yes is how a client ends up calling a
    /// method the server will refuse.
    #[must_use]
    pub fn server_supports(&self, capability: &str) -> bool {
        capability
            .split('.')
            .try_fold(&self.server_capabilities, |node, segment| node.get(segment))
            .is_some_and(|v| !v.is_null() && v != &Value::Bool(false))
    }

    /// Refuse a call the server never advertised, before it goes on the wire.
    ///
    /// "Servers that support tools MUST declare the `tools` capability" — and
    /// the same for prompts, resources, completions and logging. Failing here
    /// names the missing capability instead of surfacing whatever the server
    /// happens to answer, which for an undeclared feature is usually a bare
    /// `-32601` that says nothing about why.
    fn require_server_capability(&self, capability: &str, method: &str) -> ClientResult<()> {
        if self.server_supports(capability) {
            return Ok(());
        }
        let advertised: Vec<&str> = self
            .server_capabilities
            .as_object()
            .map(|c| c.keys().map(String::as_str).collect())
            .unwrap_or_default();
        Err(ClientError::Protocol(format!(
            "this server did not declare `{capability}`, so `{method}` is not \
             available on this connection (it advertised {advertised:?}). Send \
             it anyway with `Client::request` if you know better."
        )))
    }

    /// Refuse a method the stateless revision removed, naming what replaced it.
    ///
    /// Sending it anyway only earns a `-32601` that doesn't say why. Every
    /// typed method that exists on one side of the `2026-07-28` line follows
    /// this rule; [`request`](Self::request) is the way around it.
    fn require_stateful(&self, method: &str, instead: &str) -> ClientResult<()> {
        if !self.version.is_stateless() {
            return Ok(());
        }
        Err(ClientError::Protocol(format!(
            "`{method}` was removed in {}; {instead}",
            self.version
        )))
    }

    /// Refuse a method that only exists from `2026-07-28` on, naming the
    /// older equivalent.
    fn require_stateless(&self, method: &str, instead: &str) -> ClientResult<()> {
        if self.version.is_stateless() {
            return Ok(());
        }
        Err(ClientError::Protocol(format!(
            "`{method}` does not exist in {} (it arrived in 2026-07-28); {instead}",
            self.version
        )))
    }

    /// The underlying raw connection, for advanced/escape-hatch use.
    #[must_use]
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Liveness check (`ping`).
    ///
    /// **`2025-11-25` and earlier only.** `2026-07-28` removed the method: the
    /// stateless model has no session to keep alive, so there is nothing for a
    /// ping to prove that the next real request would not. Calling it on that
    /// wire is a client bug, not a server error, so it fails locally rather
    /// than sending a request the server must answer `404`/`-32601`.
    ///
    /// # Errors
    /// [`ClientError::Protocol`] on the stateless wire; otherwise propagates
    /// connection failure.
    pub async fn ping(&self) -> ClientResult<()> {
        self.require_stateful(
            request::PING,
            "the next real request proves liveness just as well",
        )?;
        // Routed through `versioned_request` so the HTTP transport can stamp
        // the required `MCP-Protocol-Version` header on the POST.
        self.versioned_request(request::PING, Map::new())
            .await
            .map(|_| ())
    }

    /// Issue a raw request for `method` with `params`, stamped with the
    /// negotiated protocol version and this client's declared capabilities
    /// (the same envelope the typed methods use). The escape hatch for methods
    /// the typed API doesn't model — notably extension methods such as the
    /// draft Tasks extension's `tasks/get`/`tasks/update`/`tasks/cancel`
    /// (SEP-2663). Returns the raw result `Value`.
    ///
    /// # Errors
    /// Propagates RPC failures (the server's JSON-RPC error).
    pub async fn request(&self, method: &str, params: Map<String, Value>) -> ClientResult<Value> {
        self.versioned_request(method, params).await
    }

    /// Close the shared connection and wait for transport cleanup.
    pub async fn close(&self) {
        self.conn.close().await;
    }

    /// Drop every cached response (see
    /// [`ClientBuilder::with_response_cache`]). A no-op when the cache is
    /// disabled. Notifications already invalidate automatically; this is the
    /// manual escape hatch.
    pub fn clear_response_cache(&self) {
        if let Some(cache) = &self.cache {
            cache.clear();
        }
    }

    /// Issue a cacheable request (SEP-2549): serve a fresh cached result when
    /// one exists, otherwise hit the server and store the raw result per its
    /// declared `ttlMs`. `discriminator` distinguishes entries within a
    /// method (the pagination cursor for `*/list`, the URI for
    /// `resources/read`).
    async fn cached_request(
        &self,
        method: &str,
        params: Map<String, Value>,
        discriminator: Option<&str>,
    ) -> ClientResult<Value> {
        if let Some(cache) = &self.cache
            && let Some(hit) = cache.get(method, discriminator)
        {
            return Ok(hit);
        }
        let v = self.versioned_request(method, params).await?;
        if let Some(cache) = &self.cache {
            cache.store(method, discriminator, &v);
        }
        Ok(v)
    }

    /// Drive `fetch` across every page of a paginated list, concatenating the
    /// items. `fetch` takes the cursor for the page to request (`None` for the
    /// first) and returns that page's items plus its `nextCursor`.
    ///
    /// The cursor is opaque and server-chosen, so following it is unbounded by
    /// construction. Two guards keep a broken or hostile server from spinning
    /// this loop forever: a `nextCursor` that repeats the one just sent is not
    /// advancing, and [`MAX_LIST_PAGES`] caps the total. Both fail loudly
    /// rather than silently returning a partial list — a truncated result that
    /// looks complete is the failure mode these helpers exist to prevent.
    ///
    /// An *empty* `nextCursor` is followed like any other. "Clients MUST treat
    /// cursors as opaque tokens: don't make assumptions about cursor format" —
    /// refusing `""` is exactly such an assumption, and it broke every
    /// `list_all_*` call against a server that mints one. A cursor that is
    /// empty *and* non-advancing is still caught, one page later, by the
    /// repeat guard.
    async fn collect_pages<T, F, Fut>(&self, method: &str, mut fetch: F) -> ClientResult<Vec<T>>
    where
        F: FnMut(Option<String>) -> Fut,
        Fut: Future<Output = ClientResult<(Vec<T>, Option<String>)>>,
    {
        let mut items = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_LIST_PAGES {
            let (page, next) = fetch(cursor.clone()).await?;
            items.extend(page);
            match next {
                None => return Ok(items),
                Some(next) if Some(&next) == cursor.as_ref() => {
                    return Err(ClientError::Protocol(format!(
                        "{method} returned a nextCursor that does not advance; \
                         refusing to page forever"
                    )));
                }
                Some(next) => cursor = Some(next),
            }
        }
        Err(ClientError::Protocol(format!(
            "{method} exceeded {MAX_LIST_PAGES} pages; refusing to page further"
        )))
    }

    /// List the server's tools (one page; pass a `cursor` to continue).
    ///
    /// # Errors
    /// [`ClientError::Protocol`] if the server never declared `tools`;
    /// otherwise propagates RPC and decode failures.
    pub async fn list_tools(&self, cursor: Option<&str>) -> ClientResult<neutral::ListToolsResult> {
        self.require_server_capability("tools", request::TOOLS_LIST)?;
        let v = self
            .cached_request(request::TOOLS_LIST, list_params(cursor), cursor)
            .await?;
        let mut result: neutral::ListToolsResult =
            self.decode::<v0728::ListToolsResult, legacy::ListToolsResult, _>(v)?;
        // `x-mcp-header` exists on 2026-07-28 Streamable HTTP only: "Clients
        // using other transports (e.g., stdio) MAY ignore `x-mcp-header`
        // annotations entirely", and earlier revisions never defined it.
        // Where it applies, a tool whose annotations break the constraints
        // MUST be excluded (with a warning) so one bad definition doesn't
        // block the rest. Applying that everywhere dropped perfectly good
        // tools over stdio. (Last-seen page wins; tools paginate cleanly.)
        let mirrors_headers = self.version.is_stateless() && self.conn.consumes_internal_meta();
        let mut known = self.tools.lock().expect("tool facts poisoned");
        result.tools.retain(|tool| {
            let headers = if mirrors_headers {
                match header_params_from_schema(&tool.input_schema) {
                    Ok(headers) => headers,
                    Err(reason) => {
                        tracing::warn!(
                            tool = %tool.name,
                            %reason,
                            "rejecting tool definition: invalid x-mcp-header annotation"
                        );
                        known.remove(&tool.name);
                        return false;
                    }
                }
            } else {
                Vec::new()
            };
            known.insert(tool.name.clone(), ToolFacts::learn(tool, headers));
            true
        });
        drop(known);
        Ok(result)
    }

    /// Every tool the server offers, following pagination to the last page.
    ///
    /// Prefer this to [`list_tools`](Self::list_tools) unless you are driving
    /// the cursor yourself: a paginating server answers `list_tools(None)`
    /// with only the *first* page, which is easy to mistake for the whole set.
    ///
    /// # Errors
    /// Propagates RPC and decode failures, and rejects a server whose cursor
    /// doesn't advance (see [`ClientError::Protocol`]).
    pub async fn list_all_tools(&self) -> ClientResult<Vec<neutral::Tool>> {
        self.collect_pages(request::TOOLS_LIST, |cursor| async move {
            let page = self.list_tools(cursor.as_deref()).await?;
            Ok((page.tools, page.next_cursor))
        })
        .await
    }

    /// Call a tool by name with an arguments object.
    ///
    /// # Errors
    /// Propagates RPC and decode failures. A *tool-level* failure is not an
    /// error here — it surfaces as `CallToolResult { is_error: true }`.
    pub async fn call_tool(
        &self,
        name: impl Into<String>,
        arguments: Map<String, Value>,
    ) -> ClientResult<neutral::CallToolResult> {
        self.call_tool_with(&name.into(), &arguments, None, None)
            .await
    }

    /// [`call_tool`](Self::call_tool), asking the server to report progress
    /// against `progress_token`.
    ///
    /// The token is opaque and caller-chosen (a string or an integer) and must
    /// be unique among this client's in-flight requests. Progress arrives as
    /// `notifications/progress` at
    /// [`NotificationHandler::on_notification`](crate::NotificationHandler::on_notification),
    /// each carrying the token back; the server is never obliged to send any.
    ///
    /// # Errors
    /// As [`call_tool`](Self::call_tool).
    pub async fn call_tool_with_progress(
        &self,
        name: impl Into<String>,
        arguments: Map<String, Value>,
        progress_token: impl Into<Value>,
    ) -> ClientResult<neutral::CallToolResult> {
        self.call_tool_with(&name.into(), &arguments, None, Some(&progress_token.into()))
            .await
    }

    /// Call a tool requesting task-augmented execution (core Tasks,
    /// `2025-11-25` spec §Creating Tasks).
    ///
    /// On `2025-11-25` the request carries the spec's `task` field when the
    /// spec allows it: the server declared `tasks.requests.tools.call` and the
    /// tool's `execution.taskSupport` is `optional` or `required`. The server
    /// answers a `CreateTaskResult` immediately and this method drives the
    /// lifecycle transparently: it polls `tasks/get` at the server-suggested
    /// cadence, calls `tasks/result` early when the task needs input (the
    /// server delivers that input request on the result's stream), and
    /// returns what the un-augmented call would have. Dropping the future
    /// before the task finishes sends `tasks/cancel`.
    ///
    /// Otherwise "clients MUST NOT attempt to invoke the tool as a task", so
    /// the call goes out plain and behaves exactly like
    /// [`call_tool`](Self::call_tool). If the tool hasn't been listed yet
    /// and the server does take task calls, this lists tools first to learn
    /// its `taskSupport`. (A `required` tool is augmented by every call path,
    /// `call_tool` included.)
    ///
    /// `ttl_ms` requests a retention window for the task and its result; the
    /// server reports (and may clamp) the TTL it actually applied, which also
    /// bounds how long this method will poll.
    ///
    /// On `2026-07-28` task augmentation is server-initiated (the SEP-2663
    /// Tasks *extension*), so no `task` field is sent and this behaves exactly
    /// like [`call_tool`](Self::call_tool), including transparently driving a
    /// `resultType: "task"` answer. `2025-06-18` has no Tasks at all.
    ///
    /// # Errors
    /// Propagates RPC and decode failures. A `failed` task surfaces the
    /// underlying call's JSON-RPC error as [`ClientError::Rpc`]; a `cancelled`
    /// task is a [`ClientError::Protocol`]; a task still unfinished at its
    /// server-reported TTL is a [`ClientError::Timeout`].
    pub async fn call_tool_task(
        &self,
        name: impl Into<String>,
        arguments: Map<String, Value>,
        ttl_ms: Option<i64>,
    ) -> ClientResult<neutral::CallToolResult> {
        let task = match ttl_ms {
            Some(ttl) => json!({ "ttl": ttl }),
            None => json!({}),
        };
        self.call_tool_with(&name.into(), &arguments, Some(&task), None)
            .await
    }

    /// The shared `tools/call` path: issue the (optionally task-augmented)
    /// call with the HeaderMismatch refresh-and-retry-once recovery, then settle
    /// whatever came back into a final `CallToolResult`.
    async fn call_tool_with(
        &self,
        name: &str,
        arguments: &Map<String, Value>,
        task: Option<&Value>,
        progress_token: Option<&Value>,
    ) -> ClientResult<neutral::CallToolResult> {
        self.require_server_capability("tools", request::TOOLS_CALL)?;
        let task = self.task_augmentation(name, task).await?;
        let build = |client: &Self| {
            let mut params = client.tool_call_params(name, arguments);
            if let Some(token) = progress_token {
                with_progress_token(&mut params, token.clone());
            }
            if let Some(task) = &task {
                params.insert("task".into(), task.clone());
            }
            params
        };
        let v = match self.mrtr_request(request::TOOLS_CALL, build(self)).await {
            // HeaderMismatch: our mirror headers may be built from a stale
            // schema. Per the transports spec, refresh `tools/list` (which
            // rebuilds the header cache) and retry once. Re-issuing is safe
            // only because a mismatch is refused before the tool runs — which
            // is why this is keyed to the one spec-allocated code, on the one
            // revision that has mirror headers. `-32001` (the RC's number) is
            // implementation-defined: FastMCP answers "Not found" with it, and
            // treating that as a header problem ran the tool a second time.
            Err(e)
                if self.version == ProtocolVersion::V2026_07_28
                    && e.rpc_code() == Some(codes::HEADER_MISMATCH) =>
            {
                let code = e.rpc_code().unwrap_or_default();
                tracing::warn!(
                    tool = %name,
                    code,
                    "HeaderMismatch; refreshing tools/list and retrying once"
                );
                self.clear_response_cache();
                self.list_all_tools().await?;
                self.mrtr_request(request::TOOLS_CALL, build(self)).await?
            }
            other => other?,
        };
        self.settle_tool_call(name, v).await
    }

    /// The `task` field this call carries, if any (`2025-11-25` §Tool-Level
    /// Negotiation). Three MUSTs decide it:
    ///
    /// - without `tasks.requests.tools.call` in the server's capabilities,
    ///   never augment, "regardless of the `execution.taskSupport` value";
    /// - a tool whose `taskSupport` is absent or `forbidden` is never invoked
    ///   as a task;
    /// - a `required` tool always is, whichever method the caller used.
    ///
    /// `2025-06-18` has no Tasks, and on `2026-07-28` the server decides
    /// (SEP-2663), so neither ever carries the field.
    async fn task_augmentation(
        &self,
        name: &str,
        requested: Option<&Value>,
    ) -> ClientResult<Option<Value>> {
        if self.version != ProtocolVersion::V2025_11_25
            || !self.server_supports("tasks.requests.tools.call")
        {
            return Ok(None);
        }
        let known = |client: &Self| {
            client
                .tools
                .lock()
                .expect("tool facts poisoned")
                .get(name)
                .map(|facts| facts.task_support)
        };
        // Whether this tool takes a task is only knowable from its
        // definition. Learn it once rather than guess: guessing wrong either
        // way is a `-32601` for a tool the server lists.
        let support = match known(self) {
            Some(support) => support,
            None => {
                self.list_all_tools().await?;
                known(self).flatten()
            }
        };
        Ok(match support {
            Some(neutral::TaskSupport::Required) => {
                Some(requested.cloned().unwrap_or_else(|| json!({})))
            }
            Some(neutral::TaskSupport::Optional) => requested.cloned(),
            Some(neutral::TaskSupport::Forbidden) | None => None,
        })
    }

    /// Settle a `tools/call` answer into its final result value and decode it.
    /// Either wire family may hand back a task instead of the result; both are
    /// driven transparently — use [`task_get`](Self::task_get) /
    /// [`task_cancel`](Self::task_cancel) directly to manage a lifecycle
    /// yourself.
    async fn settle_tool_call(
        &self,
        name: &str,
        mut v: Value,
    ) -> ClientResult<neutral::CallToolResult> {
        // Draft: a server MAY answer with a task handle instead of the result
        // (`resultType: "task"`, SEP-2663 — only ever sent to clients that
        // declared the Tasks extension capability). Per the SEP's guidance for
        // fixed-shape APIs, drive the polling flow and surface only the final
        // result.
        if v.get("resultType").and_then(Value::as_str) == Some(RESULT_TYPE_TASK) {
            v = self.drive_task(v).await?;
        }
        // Legacy: a task-augmented call answers `CreateTaskResult { task }`
        // (core Tasks, `2025-11-25`). `content` is required on a real
        // `CallToolResult`, so its absence + a `task` handle is unambiguous.
        else if v.get("content").is_none()
            && let Some(handle) = v.get("task")
            && handle.get("taskId").is_some()
        {
            v = self.drive_legacy_task(handle.clone()).await?;
        }
        let result: neutral::CallToolResult =
            self.decode::<v0728::CallToolResult, legacy::CallToolResult, _>(v)?;
        self.check_output(name, &result)?;
        Ok(result)
    }

    /// Hold a successful result to the tool's declared `outputSchema`.
    ///
    /// "Servers MUST provide structured results that conform to this schema"
    /// and "Clients SHOULD validate structured results against this schema."
    /// A result that doesn't is refused here rather than handed to whatever
    /// trusts the schema downstream, which is usually a model. A tool-level
    /// error (`isError`) carries no structured result and is left alone, as
    /// is a tool this client hasn't listed.
    fn check_output(&self, name: &str, result: &neutral::CallToolResult) -> ClientResult<()> {
        if result.is_error {
            return Ok(());
        }
        let Some(validator) = self
            .tools
            .lock()
            .expect("tool facts poisoned")
            .get(name)
            .and_then(|facts| facts.output.clone())
        else {
            return Ok(());
        };
        let violation = |reason: String| ClientError::OutputSchema {
            tool: name.to_owned(),
            reason,
        };
        let structured = result.structured_content.as_ref().ok_or_else(|| {
            violation("the tool declares an outputSchema but returned no structuredContent".into())
        })?;
        validator
            .validate(structured)
            .map_err(|e| violation(e.to_string()))
    }

    /// Build `tools/call` params, attaching the `x-mcp-header` mirror signal
    /// (header-name → encoded value, from the `list_tools` cache) for the HTTP
    /// transport to emit as `Mcp-Param-*` headers. Values stay in `arguments`
    /// — headers are copies, the body is authoritative. A parameter absent
    /// from `arguments` (or non-primitive) is simply not mirrored, per the
    /// extraction rule.
    fn tool_call_params(&self, name: &str, arguments: &Map<String, Value>) -> Map<String, Value> {
        let mut mirrors = Map::new();
        if let Some(facts) = self.tools.lock().expect("tool facts poisoned").get(name) {
            for param in &facts.headers {
                let mut value: Option<&Value> = None;
                for (i, segment) in param.path.iter().enumerate() {
                    value = if i == 0 {
                        arguments.get(segment)
                    } else {
                        value.and_then(|v| v.get(segment))
                    };
                }
                if let Some(rendered) = value.and_then(mcp_headers::render_argument) {
                    mirrors.insert(
                        param.header.clone(),
                        json!(mcp_headers::encode_value(&rendered)),
                    );
                }
            }
        }

        let mut params = Map::new();
        params.insert("name".into(), json!(name));
        params.insert("arguments".into(), Value::Object(arguments.clone()));
        if !mirrors.is_empty() {
            let mut meta = Map::new();
            meta.insert(HEADER_PARAMS_META_KEY.into(), Value::Object(mirrors));
            params.insert("_meta".into(), Value::Object(meta));
        }
        params
    }

    /// List the server's resources (one page; pass a `cursor` to continue).
    ///
    /// # Errors
    /// Propagates RPC and decode failures.
    pub async fn list_resources(
        &self,
        cursor: Option<&str>,
    ) -> ClientResult<neutral::ListResourcesResult> {
        self.require_server_capability("resources", request::RESOURCES_LIST)?;
        let v = self
            .cached_request(request::RESOURCES_LIST, list_params(cursor), cursor)
            .await?;
        self.decode::<v0728::ListResourcesResult, legacy::ListResourcesResult, _>(v)
    }

    /// Every resource the server offers, following pagination to the last page.
    ///
    /// See [`list_all_tools`](Self::list_all_tools) for why this is usually
    /// the one you want.
    ///
    /// # Errors
    /// Propagates RPC and decode failures, and rejects a server whose cursor
    /// doesn't advance.
    pub async fn list_all_resources(&self) -> ClientResult<Vec<neutral::Resource>> {
        self.collect_pages(request::RESOURCES_LIST, |cursor| async move {
            let page = self.list_resources(cursor.as_deref()).await?;
            Ok((page.resources, page.next_cursor))
        })
        .await
    }

    /// Read a resource by URI.
    ///
    /// # Errors
    /// [`ClientError::Protocol`] if the server never declared `resources`;
    /// otherwise propagates RPC and decode failures.
    pub async fn read_resource(
        &self,
        uri: impl Into<String>,
    ) -> ClientResult<neutral::ReadResourceResult> {
        self.read_resource_inner(uri.into(), None).await
    }

    /// [`read_resource`](Self::read_resource), asking the server to report
    /// progress against `progress_token`. See
    /// [`call_tool_with_progress`](Self::call_tool_with_progress) for how the
    /// token is chosen and where the notifications arrive.
    ///
    /// # Errors
    /// As [`read_resource`](Self::read_resource).
    pub async fn read_resource_with_progress(
        &self,
        uri: impl Into<String>,
        progress_token: impl Into<Value>,
    ) -> ClientResult<neutral::ReadResourceResult> {
        self.read_resource_inner(uri.into(), Some(progress_token.into()))
            .await
    }

    async fn read_resource_inner(
        &self,
        uri: String,
        progress_token: Option<Value>,
    ) -> ClientResult<neutral::ReadResourceResult> {
        self.require_server_capability("resources", request::RESOURCES_READ)?;
        // `resources/read` runs the MRTR loop, so it can't share
        // `cached_request`; the cache wraps the *settled* result (never an
        // `input_required` intermediate). A progress request is still cacheable
        // — the token only decides whether the server narrates on the way.
        if let Some(cache) = &self.cache
            && let Some(hit) = cache.get(request::RESOURCES_READ, Some(&uri))
        {
            return self.decode::<v0728::ReadResourceResult, legacy::ReadResourceResult, _>(hit);
        }
        let mut params = Map::new();
        params.insert("uri".into(), json!(&uri));
        if let Some(token) = progress_token {
            with_progress_token(&mut params, token);
        }
        let v = self.mrtr_request(request::RESOURCES_READ, params).await?;
        if let Some(cache) = &self.cache {
            cache.store(request::RESOURCES_READ, Some(&uri), &v);
        }
        self.decode::<v0728::ReadResourceResult, legacy::ReadResourceResult, _>(v)
    }

    /// List the server's resource templates (one page; pass a `cursor` to continue).
    ///
    /// # Errors
    /// Propagates RPC and decode failures.
    pub async fn list_resource_templates(
        &self,
        cursor: Option<&str>,
    ) -> ClientResult<neutral::ListResourceTemplatesResult> {
        self.require_server_capability("resources", request::RESOURCES_TEMPLATES_LIST)?;
        let v = self
            .cached_request(
                request::RESOURCES_TEMPLATES_LIST,
                list_params(cursor),
                cursor,
            )
            .await?;
        self.decode::<v0728::ListResourceTemplatesResult, legacy::ListResourceTemplatesResult, _>(v)
    }

    /// Every resource template the server offers, following pagination to the
    /// last page.
    ///
    /// See [`list_all_tools`](Self::list_all_tools) for why this is usually
    /// the one you want.
    ///
    /// # Errors
    /// Propagates RPC and decode failures, and rejects a server whose cursor
    /// doesn't advance.
    pub async fn list_all_resource_templates(
        &self,
    ) -> ClientResult<Vec<neutral::ResourceTemplate>> {
        self.collect_pages(request::RESOURCES_TEMPLATES_LIST, |cursor| async move {
            let page = self.list_resource_templates(cursor.as_deref()).await?;
            Ok((page.resource_templates, page.next_cursor))
        })
        .await
    }

    /// List the server's prompts (one page; pass a `cursor` to continue).
    ///
    /// # Errors
    /// Propagates RPC and decode failures.
    pub async fn list_prompts(
        &self,
        cursor: Option<&str>,
    ) -> ClientResult<neutral::ListPromptsResult> {
        self.require_server_capability("prompts", request::PROMPTS_LIST)?;
        let v = self
            .cached_request(request::PROMPTS_LIST, list_params(cursor), cursor)
            .await?;
        self.decode::<v0728::ListPromptsResult, legacy::ListPromptsResult, _>(v)
    }

    /// Every prompt the server offers, following pagination to the last page.
    ///
    /// See [`list_all_tools`](Self::list_all_tools) for why this is usually
    /// the one you want.
    ///
    /// # Errors
    /// Propagates RPC and decode failures, and rejects a server whose cursor
    /// doesn't advance.
    pub async fn list_all_prompts(&self) -> ClientResult<Vec<neutral::Prompt>> {
        self.collect_pages(request::PROMPTS_LIST, |cursor| async move {
            let page = self.list_prompts(cursor.as_deref()).await?;
            Ok((page.prompts, page.next_cursor))
        })
        .await
    }

    /// Get a prompt by name with string arguments.
    ///
    /// # Errors
    /// [`ClientError::Protocol`] if the server never declared `prompts`;
    /// otherwise propagates RPC and decode failures.
    pub async fn get_prompt(
        &self,
        name: impl Into<String>,
        arguments: Map<String, Value>,
    ) -> ClientResult<neutral::GetPromptResult> {
        self.get_prompt_inner(name.into(), arguments, None).await
    }

    /// [`get_prompt`](Self::get_prompt), asking the server to report progress
    /// against `progress_token`. See
    /// [`call_tool_with_progress`](Self::call_tool_with_progress) for how the
    /// token is chosen and where the notifications arrive.
    ///
    /// # Errors
    /// As [`get_prompt`](Self::get_prompt).
    pub async fn get_prompt_with_progress(
        &self,
        name: impl Into<String>,
        arguments: Map<String, Value>,
        progress_token: impl Into<Value>,
    ) -> ClientResult<neutral::GetPromptResult> {
        self.get_prompt_inner(name.into(), arguments, Some(progress_token.into()))
            .await
    }

    async fn get_prompt_inner(
        &self,
        name: String,
        arguments: Map<String, Value>,
        progress_token: Option<Value>,
    ) -> ClientResult<neutral::GetPromptResult> {
        self.require_server_capability("prompts", request::PROMPTS_GET)?;
        let mut params = Map::new();
        params.insert("name".into(), json!(name));
        params.insert("arguments".into(), Value::Object(arguments));
        if let Some(token) = progress_token {
            with_progress_token(&mut params, token);
        }
        let v = self.mrtr_request(request::PROMPTS_GET, params).await?;
        self.decode::<v0728::GetPromptResult, legacy::GetPromptResult, _>(v)
    }

    /// Request completion suggestions for a prompt/resource argument.
    ///
    /// `reference` and `argument` are passed through as the spec shapes them
    /// (`{ type, name }` / `{ name, value }`).
    ///
    /// # Errors
    /// [`ClientError::Protocol`] if the server never declared `completions`;
    /// otherwise propagates RPC and decode failures.
    pub async fn complete(
        &self,
        reference: Value,
        argument: Value,
    ) -> ClientResult<neutral::CompleteResult> {
        self.complete_with_context(reference, argument, Map::new())
            .await
    }

    /// [`complete`](Self::complete), with the arguments already resolved
    /// earlier in the same form.
    ///
    /// Completing the second argument of a multi-argument prompt needs the
    /// first one: "what repository?" narrows "what branch?". The spec carries
    /// that as `context.arguments`, and without a way to send it the whole
    /// point of multi-argument completion is unreachable — the server parses
    /// the field and v4's own client had no parameter for it.
    ///
    /// # Errors
    /// [`ClientError::Protocol`] if the server never declared `completions`;
    /// otherwise propagates RPC and decode failures.
    pub async fn complete_with_context(
        &self,
        reference: Value,
        argument: Value,
        resolved: Map<String, Value>,
    ) -> ClientResult<neutral::CompleteResult> {
        self.require_server_capability("completions", request::COMPLETION_COMPLETE)?;
        let mut params = Map::new();
        params.insert("ref".into(), reference);
        params.insert("argument".into(), argument);
        if !resolved.is_empty() {
            let mut context = Map::new();
            context.insert("arguments".into(), Value::Object(resolved));
            params.insert("context".into(), Value::Object(context));
        }
        let v = self
            .versioned_request(request::COMPLETION_COMPLETE, params)
            .await?;
        self.decode::<v0728::CompleteResult, legacy::CompleteResult, _>(v)
    }

    // ---- subscriptions ------------------------------------------------------

    /// Open a notification subscription (`subscriptions/listen`, `2026-07-28`).
    ///
    /// This is how a draft-protocol client receives server→client
    /// notifications at all: the draft replaced both `resources/subscribe` and
    /// the HTTP GET stream with this one long-lived subscription. Notifications
    /// then arrive at [`NotificationHandler::on_notification`], each stamped with
    /// this subscription's id in `_meta`.
    ///
    /// Returns the acknowledgement's `notifications` object — the filter subset
    /// the server actually **agreed** to, which may be narrower than what you
    /// asked for (it intersects your filter with the capabilities it
    /// registered). Check it rather than assuming; a server with no prompts
    /// silently drops `promptsListChanged`.
    ///
    /// Unlike every other request, this one is answered by that acknowledgement
    /// rather than a JSON-RPC response — only a *failure* answers in band. The
    /// subscription lasts until the connection ends; the server closes it by
    /// ending the stream.
    ///
    /// Older revisions don't have it; use
    /// [`subscribe_resource`](Self::subscribe_resource) there instead.
    ///
    /// # Errors
    /// [`ClientError::Protocol`] before `2026-07-28`; otherwise propagates RPC
    /// failures, and [`ClientError::Timeout`] if neither an acknowledgement
    /// nor an error arrives within the request timeout.
    pub async fn listen(&self, filter: neutral::SubscriptionFilter) -> ClientResult<Value> {
        self.require_stateless(
            request::SUBSCRIPTIONS_LISTEN,
            "use `subscribe_resource`, and list-changed notifications arrive unasked",
        )?;
        let wire: v0728::SubscriptionFilter = filter.into();
        let mut params = Map::new();
        params.insert(
            "notifications".into(),
            serde_json::to_value(wire).map_err(|e| ClientError::Decode(e.to_string()))?,
        );
        let ack = self
            .versioned_request(request::SUBSCRIPTIONS_LISTEN, params)
            .await?;
        Ok(ack
            .get("notifications")
            .cloned()
            .unwrap_or(Value::Object(Map::new())))
    }

    /// Subscribe to updates for one resource (`resources/subscribe`,
    /// `2025-11-25`).
    ///
    /// `notifications/resources/updated` for `uri` then arrive at
    /// [`NotificationHandler::on_notification`]. The draft dropped this method in
    /// favor of [`listen`](Self::listen) with
    /// [`SubscriptionFilter::with_resource`](neutral::SubscriptionFilter::with_resource),
    /// and this refuses locally there.
    ///
    /// # Errors
    /// [`ClientError::Protocol`] on `2026-07-28`, or if the server never
    /// declared `resources.subscribe`; otherwise propagates RPC failures.
    pub async fn subscribe_resource(&self, uri: impl Into<String>) -> ClientResult<()> {
        self.require_stateful(request::RESOURCES_SUBSCRIBE, LISTEN_INSTEAD)?;
        self.require_server_capability("resources.subscribe", request::RESOURCES_SUBSCRIBE)?;
        let mut params = Map::new();
        params.insert("uri".into(), json!(uri.into()));
        self.versioned_request(request::RESOURCES_SUBSCRIBE, params)
            .await
            .map(drop)
    }

    /// Drop a [`subscribe_resource`](Self::subscribe_resource) subscription
    /// (`resources/unsubscribe`, `2025-11-25`).
    ///
    /// # Errors
    /// [`ClientError::Protocol`] on `2026-07-28`, or if the server never
    /// declared `resources.subscribe`; otherwise propagates RPC failures.
    pub async fn unsubscribe_resource(&self, uri: impl Into<String>) -> ClientResult<()> {
        self.require_stateful(request::RESOURCES_UNSUBSCRIBE, LISTEN_INSTEAD)?;
        self.require_server_capability("resources.subscribe", request::RESOURCES_UNSUBSCRIBE)?;
        let mut params = Map::new();
        params.insert("uri".into(), json!(uri.into()));
        self.versioned_request(request::RESOURCES_UNSUBSCRIBE, params)
            .await
            .map(drop)
    }

    // ---- logging ------------------------------------------------------------

    /// Set the minimum severity of `notifications/message` the server sends
    /// (`logging/setLevel`, `2025-11-25`).
    ///
    /// Until a client calls this the server sends no log messages at all, so
    /// on `2025-11-25` this is the opt-in for server logging; messages arrive
    /// at [`NotificationHandler::on_notification`]. `2026-07-28` replaced the
    /// RPC with a per-request level, which
    /// [`ClientBuilder::with_log_level`] sets on every revision.
    ///
    /// # Errors
    /// [`ClientError::Protocol`] on `2026-07-28`, or if the server never
    /// declared `logging`; otherwise propagates RPC failures.
    #[deprecated(note = "SEP-2577 deprecates logging; still functional on 2025-11-25")]
    pub async fn set_level(&self, level: LogLevel) -> ClientResult<()> {
        self.require_stateful(
            request::LOGGING_SET_LEVEL,
            "set the level per request with `ClientBuilder::with_log_level`",
        )?;
        self.require_server_capability("logging", request::LOGGING_SET_LEVEL)?;
        let mut params = Map::new();
        params.insert("level".into(), json!(level));
        self.versioned_request(request::LOGGING_SET_LEVEL, params)
            .await
            .map(drop)
    }

    /// Tell the server this client's root list changed
    /// (`notifications/roots/list_changed`).
    ///
    /// Call this whenever the set of roots a
    /// [`RootsHandler`](crate::RootsHandler) would return changes — the server
    /// re-reads them with `roots/list` when it cares. Only meaningful if that
    /// handler reports [`list_changed`](crate::RootsHandler::list_changed),
    /// since a server told otherwise will not have stopped polling.
    ///
    /// `2026-07-28` dropped the `roots.listChanged` capability, so this is a
    /// no-op there rather than a frame the peer has no rule for.
    ///
    /// # Errors
    /// [`ClientError::Closed`] if the connection is gone.
    pub async fn notify_roots_changed(&self) -> ClientResult<()> {
        if matches!(self.version, ProtocolVersion::V2026_07_28) {
            return Ok(());
        }
        self.conn
            .notify(notification::ROOTS_LIST_CHANGED, None)
            .await
    }

    /// Poll a task's current state (`tasks/get`, SEP-2663 Tasks extension).
    ///
    /// Returns the raw task object (the extension owns its wire types): a
    /// `Task` with status-specific fields inlined — `inputRequests` when
    /// `input_required`, `result` when `completed`, `error` when `failed`.
    ///
    /// # Errors
    /// Propagates RPC failures (`-32602` for an unknown task).
    pub async fn task_get(&self, task_id: &str) -> ClientResult<Value> {
        let mut params = Map::new();
        params.insert("taskId".into(), json!(task_id));
        self.versioned_request(request::TASKS_GET, params).await
    }

    /// Answer a task's outstanding `inputRequests` (`tasks/update`). Each key
    /// must name a currently-outstanding request from `tasks/get`; the server
    /// ignores unknown/already-answered keys and accepts partial sets.
    ///
    /// `2026-07-28` (SEP-2663) only: on `2025-11-25` the server delivers a
    /// task's input requests over the ordinary server→client channel instead.
    ///
    /// # Errors
    /// [`ClientError::Protocol`] before `2026-07-28`; otherwise propagates RPC
    /// failures (`-32602` for an unknown task).
    pub async fn task_update(
        &self,
        task_id: &str,
        input_responses: Map<String, Value>,
    ) -> ClientResult<Value> {
        self.require_stateless(
            request::TASKS_UPDATE,
            "input for a task arrives as an ordinary server request there",
        )?;
        let mut params = Map::new();
        params.insert("taskId".into(), json!(task_id));
        params.insert("inputResponses".into(), Value::Object(input_responses));
        self.versioned_request(request::TASKS_UPDATE, params).await
    }

    /// Request cooperative cancellation of a task (`tasks/cancel`). The ack is
    /// eventually consistent — the task MAY still finish, and client-side task
    /// state can be dropped immediately after this returns.
    ///
    /// # Errors
    /// Propagates RPC failures (`-32602` for an unknown task).
    pub async fn task_cancel(&self, task_id: &str) -> ClientResult<()> {
        let mut params = Map::new();
        params.insert("taskId".into(), json!(task_id));
        self.versioned_request(request::TASKS_CANCEL, params)
            .await
            .map(|_| ())
    }

    /// Enumerate this session's tasks (`tasks/list`), one page at a time.
    ///
    /// Returns the raw result (`{ "tasks": [...], "nextCursor": ... }`), since
    /// tasks are wire-owned. `2025-11-25` only: the `2026-07-28` Tasks
    /// extension removed enumeration, so a task is reachable only through
    /// the handle its creator was given. Use
    /// [`list_all_tasks`](Self::list_all_tasks) unless you are driving the
    /// cursor yourself.
    ///
    /// # Errors
    /// [`ClientError::Protocol`] on `2026-07-28`; otherwise propagates RPC
    /// failures (`-32601` if the server has no Tasks support).
    pub async fn task_list(&self, cursor: Option<&str>) -> ClientResult<Value> {
        self.require_stateful(
            request::TASKS_LIST,
            "keep the task ids your calls were handed",
        )?;
        self.versioned_request(request::TASKS_LIST, list_params(cursor))
            .await
    }

    /// Every task in this session, following pagination to the last page.
    ///
    /// # Errors
    /// Propagates RPC failures, and rejects a server whose cursor doesn't
    /// advance (see [`list_all_tools`](Self::list_all_tools)).
    pub async fn list_all_tasks(&self) -> ClientResult<Vec<Value>> {
        self.collect_pages(request::TASKS_LIST, |cursor| async move {
            let page = self.task_list(cursor.as_deref()).await?;
            let tasks = page
                .get("tasks")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let next = page
                .get("nextCursor")
                .and_then(Value::as_str)
                .map(str::to_owned);
            Ok((tasks, next))
        })
        .await
    }

    /// Drive a `CreateTaskResult` to its terminal state (SEP-2663): poll
    /// `tasks/get` at the server's suggested interval, answer `input_required`
    /// requests through the registered handlers (deduplicating keys across
    /// polls, per spec) via `tasks/update`, and return the task's final
    /// `result` value. A `failed` task surfaces its JSON-RPC error; a
    /// `cancelled` task is a protocol error; a finite `ttlMs` acts as the
    /// spec's polling backstop.
    async fn drive_task(&self, mut current: Value) -> ClientResult<Value> {
        let task_id = current
            .get("taskId")
            .and_then(Value::as_str)
            .ok_or_else(|| ClientError::Decode("CreateTaskResult without a taskId".into()))?
            .to_owned();
        let mut guard = CancelTaskOnDrop::new(self, &task_id);
        // TTL backstop (spec: the client MAY consider the task unusable after
        // `createdAt + ttlMs`). Measured from now — at or after `createdAt`,
        // so never stricter than the spec allows. `null` ⇒ poll indefinitely.
        let deadline = current
            .get("ttlMs")
            .and_then(Value::as_u64)
            .map(|ms| std::time::Instant::now() + Duration::from_millis(ms));
        let mut answered: std::collections::HashSet<String> = std::collections::HashSet::new();
        loop {
            match current.get("status").and_then(Value::as_str) {
                Some("completed") => {
                    guard.disarm();
                    return current.get("result").cloned().ok_or_else(|| {
                        ClientError::Decode("completed task without a result".into())
                    });
                }
                Some("failed") => {
                    guard.disarm();
                    let err = current.get("error");
                    return Err(ClientError::Rpc(turbomcp_core::JsonRpcError {
                        code: err
                            .and_then(|e| e.get("code"))
                            .and_then(Value::as_i64)
                            .and_then(|c| i32::try_from(c).ok())
                            .unwrap_or(-32603),
                        message: err
                            .and_then(|e| e.get("message"))
                            .and_then(Value::as_str)
                            .unwrap_or("task failed")
                            .to_owned(),
                        data: err.and_then(|e| e.get("data")).cloned(),
                    }));
                }
                Some("cancelled") => {
                    guard.disarm();
                    return Err(ClientError::Protocol(format!(
                        "task {task_id} was cancelled"
                    )));
                }
                Some("input_required") => {
                    if self.handler.is_empty() {
                        return Err(ClientError::Protocol(
                            "task requires input but the client registered no handler".into(),
                        ));
                    }
                    let handler = &self.handler;
                    // Answer each outstanding request exactly once (the spec
                    // has clients dedup keys across consecutive polls; keys
                    // are unique over the task's lifetime).
                    let mut responses = Map::new();
                    if let Some(requests) = current.get("inputRequests").and_then(Value::as_object)
                    {
                        for (key, req) in requests {
                            if answered.contains(key) {
                                continue;
                            }
                            let req_method = req
                                .get("method")
                                .and_then(Value::as_str)
                                .unwrap_or_default();
                            let req_params = req.get("params").cloned();
                            let answer = dispatch_server_request(
                                handler,
                                &self.version,
                                req_method,
                                req_params,
                            )
                            .await
                            .map_err(|e| {
                                ClientError::Protocol(format!(
                                    "input handler failed: {}",
                                    e.message
                                ))
                            })?;
                            answered.insert(key.clone());
                            responses.insert(key.clone(), answer);
                        }
                    }
                    if !responses.is_empty() {
                        self.task_update(&task_id, responses).await?;
                    }
                }
                // `working` (or a status from a newer revision) → keep polling.
                _ => {}
            }
            if let Some(deadline) = deadline
                && std::time::Instant::now() >= deadline
            {
                return Err(ClientError::Timeout);
            }
            let interval = current
                .get("pollIntervalMs")
                .and_then(Value::as_u64)
                .unwrap_or(DEFAULT_TASK_POLL_MS)
                .max(MIN_TASK_POLL_MS);
            tokio::time::sleep(Duration::from_millis(interval)).await;
            current = self.task_get(&task_id).await?;
        }
    }

    /// Drive a `2025-11-25` core-Tasks handle to its terminal state (spec
    /// §Polling and §Result Retrieval): poll `tasks/get` at the
    /// server-suggested `pollInterval`, then fetch the outcome via
    /// `tasks/result`, which answers exactly what the underlying request
    /// would have, so a `failed` task surfaces its original JSON-RPC error
    /// through normal RPC propagation. A `cancelled` task is a protocol
    /// error; a finite `ttl` is the polling backstop.
    ///
    /// `input_required` calls `tasks/result` straight away ("the requestor
    /// SHOULD preemptively call `tasks/result`"): a server may deliver the
    /// pending elicitation on that request's stream, and the connection actor
    /// answers it there. Polling carries on alongside, so a `tasks/result`
    /// that outlives the request timeout just gets issued again.
    async fn drive_legacy_task(&self, mut current: Value) -> ClientResult<Value> {
        let task_id = current
            .get("taskId")
            .and_then(Value::as_str)
            .ok_or_else(|| ClientError::Decode("CreateTaskResult without a taskId".into()))?
            .to_owned();
        let mut guard = CancelTaskOnDrop::new(self, &task_id);
        // TTL backstop, measured from now (at or after `createdAt`, so never
        // stricter than the spec allows). Legacy types it `ttl` (ms);
        // `null` ⇒ poll indefinitely.
        let deadline = current
            .get("ttl")
            .and_then(Value::as_u64)
            .map(|ms| std::time::Instant::now() + Duration::from_millis(ms));
        let mut early: Option<futures::future::BoxFuture<'_, ClientResult<Value>>> = None;
        loop {
            match current.get("status").and_then(Value::as_str) {
                // Terminal either way: `tasks/result` answers the underlying
                // call's success value or its JSON-RPC error verbatim.
                Some("completed" | "failed") => {
                    guard.disarm();
                    return match early {
                        Some(pending) => pending.await,
                        None => self.task_result(&task_id).await,
                    };
                }
                Some("cancelled") => {
                    guard.disarm();
                    return Err(ClientError::Protocol(format!(
                        "task {task_id} was cancelled"
                    )));
                }
                Some("input_required") if early.is_none() => {
                    early = Some(Box::pin(self.task_result(&task_id)));
                }
                // `working`, or a status from a newer revision → keep polling.
                _ => {}
            }
            if let Some(deadline) = deadline
                && std::time::Instant::now() >= deadline
            {
                return Err(ClientError::Timeout);
            }
            let interval = current
                .get("pollInterval")
                .and_then(Value::as_u64)
                .unwrap_or(DEFAULT_TASK_POLL_MS)
                .max(MIN_TASK_POLL_MS);
            let nap = tokio::time::sleep(Duration::from_millis(interval));
            if let Some(pending) = early.as_mut() {
                tokio::select! {
                    outcome = pending => match outcome {
                        // Our own request timeout, not the task's: ask again
                        // on the next `input_required`.
                        Err(ClientError::Timeout) => early = None,
                        outcome => {
                            guard.disarm();
                            return outcome;
                        }
                    },
                    () = nap => {}
                }
            } else {
                nap.await;
            }
            current = self.task_get(&task_id).await?;
        }
    }

    /// `tasks/result` (`2025-11-25`): blocks until the task is terminal, then
    /// answers what the underlying request would have.
    async fn task_result(&self, task_id: &str) -> ClientResult<Value> {
        let mut params = Map::new();
        params.insert("taskId".into(), json!(task_id));
        self.versioned_request(request::TASKS_RESULT, params).await
    }

    /// Issue an MRTR-capable request (`tools/call`, `resources/read`,
    /// `prompts/get`), driving the draft input-required loop.
    ///
    /// On the modern path a server can answer `{ resultType: "input_required",
    /// inputRequests, requestState }`; this gathers each packaged request via the
    /// registered handlers and re-issue the call with `inputResponses` + the echoed
    /// `requestState`, until a real result comes back. On the legacy path the
    /// server elicits inline (handled by the connection actor), so the first
    /// result is final and the loop runs exactly once.
    async fn mrtr_request(
        &self,
        method: &str,
        original: Map<String, Value>,
    ) -> ClientResult<Value> {
        let mut params = original.clone();
        let mut state_only_rounds = 0u32;
        for _ in 0..MAX_MRTR_ROUNDS {
            let result = self.versioned_request(method, params).await?;
            if !self.result_is_input_required(&result)? {
                return Ok(result);
            }

            // Each retry is the original request plus what *this* round
            // asked for, and nothing from an earlier one: "If the
            // `InputRequiredResult` does not contain a `requestState` field,
            // the client MUST NOT include one in the retry." Keeping the last
            // round's state echoed a token the server had stopped issuing,
            // which it must reject.
            params = original.clone();
            let requests = result
                .get("inputRequests")
                .and_then(Value::as_object)
                .filter(|requests| !requests.is_empty());
            if let Some(requests) = requests {
                if self.handler.is_empty() {
                    return Err(ClientError::Protocol(
                        "server requires input (MRTR) but the client registered no handler".into(),
                    ));
                }
                // Answered concurrently: they are independent by construction,
                // and one slow human should not serialize the rest.
                let answers = futures::future::try_join_all(requests.iter().map(|(key, req)| {
                    let handler = &self.handler;
                    let version = &self.version;
                    async move {
                        let req_method = req
                            .get("method")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        let req_params = req.get("params").cloned();
                        dispatch_server_request(handler, version, req_method, req_params)
                            .await
                            .map(|answer| (key.clone(), answer))
                            .map_err(|e| {
                                ClientError::Protocol(format!(
                                    "input handler failed: {}",
                                    e.message
                                ))
                            })
                    }
                }))
                .await?;
                params.insert(
                    "inputResponses".into(),
                    Value::Object(answers.into_iter().collect()),
                );
            } else {
                // No questions, only a token to bring back: the server is
                // shedding load or pacing a long operation. "The client MAY
                // retry the original request immediately", but a back-to-back
                // loop is exactly the load it is shedding.
                state_only_rounds += 1;
                let backoff = Duration::from_millis(50 << state_only_rounds.min(6));
                tokio::time::sleep(backoff).await;
            }
            if let Some(state) = result.get("requestState") {
                params.insert("requestState".into(), state.clone());
            }
        }
        Err(ClientError::Protocol(format!(
            "MRTR did not converge after {MAX_MRTR_ROUNDS} rounds"
        )))
    }

    /// Classify a 2026-07-28 result by its `resultType`.
    ///
    /// "A `resultType` of any value unrecognized by the client MUST be
    /// considered invalid", and an absent one MUST be treated as `"complete"`.
    /// `"task"` is recognized only by a client that declared the Tasks
    /// extension, the only one a server may send it to. The stateful
    /// revisions have no `resultType`, so nothing is checked there.
    fn result_is_input_required(&self, result: &Value) -> ClientResult<bool> {
        if self.version.is_stateful() {
            return Ok(false);
        }
        match result.get("resultType").and_then(Value::as_str) {
            None | Some(neutral::result_type::COMPLETE) => Ok(false),
            Some(neutral::result_type::INPUT_REQUIRED) => Ok(true),
            Some(RESULT_TYPE_TASK) if self.declares_extension(TASKS_EXTENSION) => Ok(false),
            Some(other) => Err(ClientError::Protocol(format!(
                "server answered with an unrecognized resultType `{other}`"
            ))),
        }
    }

    /// Whether this client declared the extension `id`.
    fn declares_extension(&self, id: &str) -> bool {
        self.request_meta
            .get(keys::CLIENT_CAPABILITIES)
            .and_then(|caps| caps.get("extensions"))
            .and_then(Value::as_object)
            .is_some_and(|extensions| extensions.contains_key(id))
    }

    /// Issue a request, stamping the modern `_meta` envelope when the negotiated
    /// version is the stateless draft (legacy carries identity in the session).
    /// Every request also carries the internal negotiated-version signal for
    /// the HTTP transport's `MCP-Protocol-Version` header (required on all
    /// post-negotiation requests by both versions' transports specs); other
    /// transports sanitize it at the server boundary.
    async fn versioned_request(
        &self,
        method: &str,
        mut params: Map<String, Value>,
    ) -> ClientResult<Value> {
        let meta = params
            .entry("_meta")
            .or_insert_with(|| Value::Object(Map::new()));
        if let Some(meta) = meta.as_object_mut() {
            meta.insert(
                NEGOTIATED_VERSION_META_KEY.into(),
                json!(self.version.as_str()),
            );
            if self.version == ProtocolVersion::V2026_07_28 {
                // Merge the version envelope into any existing `_meta` (e.g. the
                // `#[mcp_header]` mirror signal) rather than clobbering it.
                for (key, value) in &self.request_meta {
                    meta.entry(key.clone()).or_insert_with(|| value.clone());
                }
            }
        }
        let params = Value::Object(params);
        match self.conn.request(method, Some(params.clone())).await {
            // The stream carrying the response broke first. `Connection` mints
            // a fresh id per request, which is what "re-issue it as a new
            // request with a new request ID" asks for; once is enough to ride
            // out a proxy or load balancer dropping one stream.
            Err(error) if stream_lost(&error) && REISSUABLE.contains(&method) => {
                tracing::debug!(method, "response stream lost; re-issuing once");
                self.conn.request(method, Some(params)).await
            }
            other => other,
        }
    }

    /// Decode a result into a [`neutral`] type via the negotiated version's wire
    /// shape: deserialize as `D` (draft) or `L` (legacy), then convert.
    fn decode<D, L, N>(&self, value: Value) -> ClientResult<N>
    where
        D: DeserializeOwned + Into<N>,
        L: DeserializeOwned + Into<N>,
    {
        if self.version == ProtocolVersion::V2026_07_28 {
            serde_json::from_value::<D>(value)
                .map(Into::into)
                .map_err(|e| ClientError::Decode(e.to_string()))
        } else {
            serde_json::from_value::<L>(value)
                .map(Into::into)
                .map_err(|e| ClientError::Decode(e.to_string()))
        }
    }
}

/// Stamp `_meta.progressToken` onto a request's params, merging into whatever
/// `_meta` is already there (the `#[mcp_header]` mirror signal, typically).
fn with_progress_token(params: &mut Map<String, Value>, token: Value) {
    if let Some(meta) = params
        .entry("_meta")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
    {
        meta.insert("progressToken".into(), token);
    }
}

/// Build `*/list` params from an optional pagination cursor.
fn list_params(cursor: Option<&str>) -> Map<String, Value> {
    let mut params = Map::new();
    if let Some(cursor) = cursor {
        params.insert("cursor".into(), json!(cursor));
    }
    params
}

/// Sends `tasks/cancel` for a task its driver stopped waiting on.
///
/// "For task-augmented requests, the `tasks/cancel` request MUST be used
/// instead of the `notifications/cancelled` notification", so the ordinary
/// abandon path (which rightly withholds the notification for these) leaves
/// the server running the task to its TTL. Dropping the call's future, a
/// failed input handler, and the TTL backstop all end up here; a task that
/// reached a terminal state disarms it.
struct CancelTaskOnDrop<'a> {
    client: &'a Client,
    task_id: Option<String>,
}

impl<'a> CancelTaskOnDrop<'a> {
    fn new(client: &'a Client, task_id: &str) -> Self {
        Self {
            client,
            task_id: Some(task_id.to_owned()),
        }
    }

    fn disarm(&mut self) {
        self.task_id = None;
    }
}

impl Drop for CancelTaskOnDrop<'_> {
    fn drop(&mut self) {
        let Some(task_id) = self.task_id.take() else {
            return;
        };
        // `Drop` can't await, so the cancel goes out on its own task. One per
        // abandoned task, bounded by the request timeout, and nothing to do
        // outside a runtime (the connection is going with it).
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let client = self.client.clone();
        runtime.spawn(async move {
            if let Err(error) = client.task_cancel(&task_id).await {
                tracing::debug!(%task_id, %error, "tasks/cancel for an abandoned task failed");
            }
        });
    }
}

/// What a `tools/list` entry commits the server to, kept per tool name so
/// every `call_tool` can honour it.
#[derive(Clone, Default)]
struct ToolFacts {
    /// The `x-mcp-header` mirrors (empty off 2026-07-28 Streamable HTTP).
    headers: Vec<HeaderParam>,
    /// `execution.taskSupport`; `None` is the spec's "not present".
    task_support: Option<neutral::TaskSupport>,
    /// The compiled `outputSchema`, if the tool declares one that compiles.
    output: Option<Arc<jsonschema::Validator>>,
}

impl ToolFacts {
    fn learn(tool: &neutral::Tool, headers: Vec<HeaderParam>) -> Self {
        // A schema that doesn't compile can't be checked. That's the server's
        // bug, and dropping the tool over it would punish the caller for it,
        // so the tool stays callable and its results go unvalidated.
        let output = tool.output_schema.as_ref().and_then(|schema| {
            jsonschema::validator_for(schema)
                .map(Arc::new)
                .inspect_err(|e| {
                    tracing::warn!(
                        tool = %tool.name,
                        error = %e,
                        "outputSchema does not compile; this tool's results are not validated"
                    );
                })
                .ok()
        });
        Self {
            headers,
            task_support: tool.task_support,
            output,
        }
    }
}

/// One `x-mcp-header`-annotated tool parameter: the header-name portion
/// (mirrored as `Mcp-Param-{header}`) and the `properties` path to its value
/// in the call arguments.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HeaderParam {
    header: String,
    path: Vec<String>,
}

/// Collect a tool's `x-mcp-header` annotations, enforcing the transports
/// spec's constraints. `Err` names the violation — a client using Streamable
/// HTTP MUST then reject the whole tool definition (exclude it from
/// `tools/list` and warn).
///
/// Constraints checked: the annotation is a string (the obsolete boolean form
/// is tolerated as "use the property name"), a valid RFC 9110 field-name
/// token, case-insensitively unique within the schema, applied only to
/// primitive `string`/`integer`/`boolean` parameters (never `number`), and
/// only on properties *statically reachable* through chains of `properties`
/// keys — an annotation under `items`, composition/conditional keywords,
/// `$ref`, or `$defs` invalidates the tool.
fn header_params_from_schema(input_schema: &Value) -> Result<Vec<HeaderParam>, String> {
    let mut found = Vec::new();
    scan(input_schema, true, &mut Vec::new(), &mut found)?;
    let mut seen = std::collections::HashSet::new();
    for param in &found {
        if !seen.insert(param.header.to_ascii_lowercase()) {
            return Err(format!(
                "duplicate x-mcp-header name {:?} (names are case-insensitively unique)",
                param.header
            ));
        }
    }
    return Ok(found);

    /// Walk every node; `reachable` is true only along root→`properties`→…
    /// chains. An `x-mcp-header` on any other node is invalid.
    fn scan(
        node: &Value,
        reachable: bool,
        path: &mut Vec<String>,
        found: &mut Vec<HeaderParam>,
    ) -> Result<(), String> {
        let Value::Object(map) = node else {
            if let Value::Array(items) = node {
                for item in items {
                    scan(item, false, path, found)?;
                }
            }
            return Ok(());
        };
        if let Some(annotation) = map.get("x-mcp-header") {
            if !reachable || path.is_empty() {
                return Err(format!(
                    "x-mcp-header at {:?} is not statically reachable via `properties` chains",
                    path.join(".")
                ));
            }
            let header = match annotation {
                Value::String(s) => s.clone(),
                // Obsolete boolean form (pre-string SEP-2243 revisions):
                // treat as "mirror under the property's own name".
                Value::Bool(true) => path.last().cloned().unwrap_or_default(),
                _ => {
                    return Err(format!(
                        "invalid x-mcp-header value at {:?}",
                        path.join(".")
                    ));
                }
            };
            if !mcp_headers::is_valid_header_name(&header) {
                return Err(format!(
                    "x-mcp-header {header:?} at {:?} is not a valid header-name token",
                    path.join(".")
                ));
            }
            // One primitive, optionally nullable: the transports spec says a
            // `null` value omits the header, so nullable annotated parameters
            // exist, and `Option<T>` renders as `[T, "null"]`.
            let primitive =
                |t: &Value| matches!(t.as_str(), Some("string" | "integer" | "boolean"));
            let typed = match map.get("type") {
                Some(Value::Array(types)) => {
                    let mut non_null = types.iter().filter(|t| t.as_str() != Some("null"));
                    non_null.next().is_some_and(primitive) && non_null.next().is_none()
                }
                Some(t) => primitive(t),
                None => false,
            };
            if !typed {
                return Err(format!(
                    "x-mcp-header at {:?} requires a primitive string/integer/boolean parameter",
                    path.join(".")
                ));
            }
            found.push(HeaderParam {
                header,
                path: path.clone(),
            });
        }
        for (key, child) in map {
            if key == "properties" && reachable {
                if let Value::Object(props) = child {
                    for (name, prop) in props {
                        path.push(name.clone());
                        scan(prop, true, path, found)?;
                        path.pop();
                    }
                }
            } else if key != "x-mcp-header" {
                // Everything else (items, oneOf/anyOf/allOf/not, if/then/else,
                // $defs, …) breaks static reachability for what's below it.
                scan(child, false, path, found)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod header_param_tests {
    use super::*;

    #[test]
    fn collects_string_annotations_and_nested_paths() {
        let schema = json!({
            "type": "object",
            "properties": {
                "region": { "type": "string", "x-mcp-header": "Region" },
                "options": {
                    "type": "object",
                    "properties": {
                        "tier": { "type": "integer", "x-mcp-header": "Tier" }
                    }
                },
                "query": { "type": "string" }
            }
        });
        let mut params = header_params_from_schema(&schema).unwrap();
        params.sort_by(|a, b| a.header.cmp(&b.header));
        assert_eq!(
            params,
            vec![
                HeaderParam {
                    header: "Region".into(),
                    path: vec!["region".into()],
                },
                HeaderParam {
                    header: "Tier".into(),
                    path: vec!["options".into(), "tier".into()],
                },
            ]
        );
    }

    #[test]
    fn tolerates_the_obsolete_boolean_form() {
        let schema = json!({
            "type": "object",
            "properties": { "region": { "type": "string", "x-mcp-header": true } }
        });
        let params = header_params_from_schema(&schema).unwrap();
        assert_eq!(params[0].header, "region");
    }

    /// A `null` value omits the header, so a nullable primitive (how
    /// `Option<T>` renders) is a valid target. Anything else in the array
    /// is not.
    #[test]
    fn accepts_nullable_primitives_only() {
        let nullable = json!({
            "type": "object",
            "properties": { "zone": { "type": ["string", "null"], "x-mcp-header": "Zone" } }
        });
        assert_eq!(
            header_params_from_schema(&nullable).unwrap()[0].header,
            "Zone"
        );

        for bad in [
            json!(["string", "integer"]),
            json!(["null"]),
            json!(["number", "null"]),
        ] {
            let schema = json!({
                "type": "object",
                "properties": { "a": { "type": bad, "x-mcp-header": "A" } }
            });
            assert!(header_params_from_schema(&schema).is_err(), "{bad}");
        }
    }

    #[test]
    fn rejects_constraint_violations() {
        // Not a tchar token.
        let bad_name = json!({
            "type": "object",
            "properties": { "a": { "type": "string", "x-mcp-header": "has space" } }
        });
        assert!(header_params_from_schema(&bad_name).is_err());

        // `number` is not permitted (integers only).
        let number_type = json!({
            "type": "object",
            "properties": { "a": { "type": "number", "x-mcp-header": "A" } }
        });
        assert!(header_params_from_schema(&number_type).is_err());

        // Case-insensitively duplicate names.
        let dupes = json!({
            "type": "object",
            "properties": {
                "a": { "type": "string", "x-mcp-header": "Region" },
                "b": { "type": "string", "x-mcp-header": "region" }
            }
        });
        assert!(header_params_from_schema(&dupes).is_err());

        // Not statically reachable: inside a composition keyword.
        let unreachable = json!({
            "type": "object",
            "properties": {
                "a": {
                    "oneOf": [
                        { "type": "string", "x-mcp-header": "A" },
                        { "type": "integer" }
                    ]
                }
            }
        });
        assert!(header_params_from_schema(&unreachable).is_err());

        // Not statically reachable: inside `items`.
        let in_items = json!({
            "type": "object",
            "properties": {
                "a": {
                    "type": "array",
                    "items": { "type": "string", "x-mcp-header": "A" }
                }
            }
        });
        assert!(header_params_from_schema(&in_items).is_err());
    }
}
