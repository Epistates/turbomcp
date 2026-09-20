//! Unified request context for MCP handlers.
//!
//! This module provides the canonical [`RequestContext`] carried through every
//! MCP request. It is the single source of truth across the workspace:
//! `turbomcp-server`, `turbomcp-protocol`, and `turbomcp-wasm` all re-export
//! this type. `#[tool]`, `#[resource]`, and `#[prompt]` bodies receive
//! `&RequestContext`; calling `ctx.sample(...)`, `ctx.elicit_form(...)`,
//! `ctx.elicit_url(...)`, or `ctx.notify_client(...)` works as long as the
//! transport populated a bidirectional [`McpSession`].
//!
//! # Design
//!
//! - `alloc`-only fields are available in `no_std` builds (WASM, embedded).
//! - Richer runtime fields (`start_time`, `headers`, `cancellation_token`) are
//!   gated behind `#[cfg(feature = "std")]` and omitted from `no_std` builds.
//! - The session handle is held as `Arc<dyn McpSession>` so every transport
//!   can plug in without changing the type.

use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;

use hashbrown::HashMap as HashbrownMap;
use serde_json::Value;

use crate::auth::Principal;
use crate::error::{McpError, McpResult};
use crate::session::McpSession;

#[cfg(feature = "std")]
use crate::session::Cancellable;

#[cfg(feature = "std")]
use std::time::Instant;

use turbomcp_types::{ClientCapabilities, CreateMessageRequest, CreateMessageResult, ElicitResult};

/// Transport type identifier.
///
/// Indicates which transport received the request. This is useful for:
/// - Logging and metrics
/// - Transport-specific behavior (e.g., different timeouts)
/// - Debugging and tracing
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum TransportType {
    /// Standard I/O transport (default for CLI tools)
    #[default]
    Stdio,
    /// HTTP transport (REST or SSE)
    Http,
    /// WebSocket transport
    WebSocket,
    /// Raw TCP transport
    Tcp,
    /// Unix domain socket transport
    Unix,
    /// WebAssembly/Worker transport (Cloudflare Workers, etc.)
    Wasm,
    /// In-process channel transport (zero-copy, no serialization overhead)
    Channel,
    /// Unknown or custom transport
    Unknown,
}

impl TransportType {
    /// Returns true if this is a network-based transport.
    #[inline]
    pub fn is_network(&self) -> bool {
        matches!(self, Self::Http | Self::WebSocket | Self::Tcp)
    }

    /// Returns true if this is a local transport.
    #[inline]
    pub fn is_local(&self) -> bool {
        matches!(self, Self::Stdio | Self::Unix | Self::Channel)
    }

    /// Returns the transport name as a string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Stdio => "stdio",
            Self::Http => "http",
            Self::WebSocket => "websocket",
            Self::Tcp => "tcp",
            Self::Unix => "unix",
            Self::Wasm => "wasm",
            Self::Channel => "channel",
            Self::Unknown => "unknown",
        }
    }
}

impl core::fmt::Display for TransportType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Per-session minimum log severity, set by `logging/setLevel`.
///
/// The level has to outlive the request that set it — the client sets it once
/// and it governs every later `notifications/message` on that session — so it
/// cannot live in the per-request metadata map the way the progress token does.
///
/// Keyed by session id, and therefore `std`-only; `no_std` builds have no
/// session store and treat every level as wanted, which the spec permits
/// ("The receiver MAY ...").
#[cfg(feature = "std")]
static MIN_LOG_LEVEL: std::sync::LazyLock<
    std::sync::RwLock<std::collections::HashMap<String, usize>>,
> = std::sync::LazyLock::new(|| std::sync::RwLock::new(std::collections::HashMap::new()));

/// Forget a session's minimum log level.
///
/// Call when a session ends; otherwise the entry lives for the process. The
/// transports that own session lifetime are the right callers.
#[cfg(feature = "std")]
pub fn clear_min_log_level(session_id: &str) {
    if let Ok(mut map) = MIN_LOG_LEVEL.write() {
        map.remove(session_id);
    }
}

/// Metadata slot holding the client's `_meta.progressToken` for this request.
///
/// Kept in [`RequestContext::metadata`] rather than as a struct field so the
/// addition stays backwards compatible for callers that build the context with
/// a struct literal.
const PROGRESS_TOKEN_KEY: &str = "io.turbomcp/progressToken";

/// Canonical per-request context.
///
/// Carries request identity, transport information, authentication principal,
/// arbitrary typed metadata, and — when the transport supports bidirectional
/// communication — an [`McpSession`] handle that enables server-to-client
/// operations such as sampling and elicitation.
///
/// # Thread Safety
///
/// `RequestContext` is `Send + Sync` on native targets. On WASM targets the
/// `Send`/`Sync` bounds are dropped (single-threaded runtime).
#[derive(Debug, Clone, Default)]
pub struct RequestContext {
    /// Unique request identifier (JSON-RPC id as string, or generated UUID).
    pub request_id: String,

    /// Transport type that received this request.
    pub transport: TransportType,

    /// Authenticated user identifier, if the request was authenticated.
    pub user_id: Option<String>,

    /// Session identifier for stateful transports (HTTP + session cookie, WS,
    /// Streamable HTTP, etc.).
    pub session_id: Option<String>,

    /// Client application identifier reported by the peer.
    pub client_id: Option<String>,

    /// Rich typed metadata (headers, trace IDs, custom per-request data).
    pub metadata: HashbrownMap<String, Value>,

    /// Authenticated principal, if auth is configured and succeeded.
    pub principal: Option<Principal>,

    /// Bidirectional session handle for server-to-client requests.
    ///
    /// Populated by the server dispatcher before routing; `None` on
    /// unidirectional transports (e.g., stateless HTTP) or when the request
    /// is being synthesized (tests, examples).
    pub session: Option<Arc<dyn McpSession>>,

    /// HTTP-layer headers for HTTP/WebSocket transports.
    ///
    /// Populated by the transport; `None` for non-HTTP transports. Uses
    /// `hashbrown::HashMap` so it stays available in `no_std` / WASM builds.
    pub headers: Option<HashbrownMap<String, String>>,

    /// Wall-clock moment at which the server began processing the request.
    ///
    /// Used for `elapsed()` measurements and tracing spans.
    #[cfg(feature = "std")]
    pub start_time: Option<Instant>,

    /// Cooperative-cancellation handle.
    ///
    /// Tool bodies should check `ctx.is_cancelled()` during long operations
    /// and abort early. The server wires a `tokio_util::sync::CancellationToken`
    /// in here (via the `Cancellable` blanket impl in `turbomcp-server`).
    #[cfg(feature = "std")]
    pub cancellation_token: Option<Arc<dyn Cancellable>>,
}

// ====================================================================
// Constructors
// ====================================================================

impl RequestContext {
    /// Create a new request context with a freshly generated UUID and Stdio transport.
    ///
    /// For WASM/no_std builds the request ID is empty; call
    /// [`Self::with_id`] explicitly to set one.
    pub fn new() -> Self {
        #[cfg(feature = "std")]
        {
            Self {
                request_id: uuid::Uuid::new_v4().to_string(),
                ..Default::default()
            }
        }
        #[cfg(not(feature = "std"))]
        {
            Self::default()
        }
    }

    /// Create a context with the given ID and transport.
    pub fn with_id_and_transport(request_id: impl Into<String>, transport: TransportType) -> Self {
        Self {
            request_id: request_id.into(),
            transport,
            ..Default::default()
        }
    }

    /// Create a context with an explicit request ID (Stdio transport).
    pub fn with_id(request_id: impl Into<String>) -> Self {
        Self {
            request_id: request_id.into(),
            ..Default::default()
        }
    }

    /// Create a context for STDIO transport with a fresh UUID.
    #[inline]
    pub fn stdio() -> Self {
        Self::new().with_transport(TransportType::Stdio)
    }

    /// Create a context for HTTP transport with a fresh UUID.
    #[inline]
    pub fn http() -> Self {
        Self::new().with_transport(TransportType::Http)
    }

    /// Create a context for WebSocket transport with a fresh UUID.
    #[inline]
    pub fn websocket() -> Self {
        Self::new().with_transport(TransportType::WebSocket)
    }

    /// Create a context for TCP transport with a fresh UUID.
    #[inline]
    pub fn tcp() -> Self {
        Self::new().with_transport(TransportType::Tcp)
    }

    /// Create a context for Unix domain socket transport with a fresh UUID.
    #[inline]
    pub fn unix() -> Self {
        Self::new().with_transport(TransportType::Unix)
    }

    /// Create a context for WASM transport with a fresh UUID.
    #[inline]
    pub fn wasm() -> Self {
        Self::new().with_transport(TransportType::Wasm)
    }

    /// Create a context for in-process channel transport with a fresh UUID.
    #[inline]
    pub fn channel() -> Self {
        Self::new().with_transport(TransportType::Channel)
    }
}

// ====================================================================
// Builders
// ====================================================================

impl RequestContext {
    /// Set the request ID.
    #[must_use]
    pub fn with_request_id(mut self, id: impl Into<String>) -> Self {
        self.request_id = id.into();
        self
    }

    /// Set the transport type.
    #[must_use]
    pub fn with_transport(mut self, transport: TransportType) -> Self {
        self.transport = transport;
        self
    }

    /// Set the authenticated user ID.
    #[must_use]
    pub fn with_user_id(mut self, user_id: impl Into<String>) -> Self {
        self.user_id = Some(user_id.into());
        self
    }

    /// Set the session ID.
    #[must_use]
    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    /// Set the client ID.
    #[must_use]
    pub fn with_client_id(mut self, client_id: impl Into<String>) -> Self {
        self.client_id = Some(client_id.into());
        self
    }

    /// Set the authenticated principal.
    #[must_use]
    pub fn with_principal(mut self, principal: Principal) -> Self {
        self.principal = Some(principal);
        self
    }

    /// Attach a metadata key/value pair.
    ///
    /// Accepts any value convertible to `serde_json::Value`, so string
    /// literals, numbers, and structured data all work.
    #[must_use]
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Attach the client's progress token for this request.
    ///
    /// Populated by the router from `params._meta.progressToken`. Per the MCP
    /// progress utility the token is `string | number`, so it is stored as the
    /// raw [`Value`] the client sent rather than being coerced.
    ///
    /// Handlers should read it through [`progress_token`](Self::progress_token)
    /// rather than reaching into [`metadata`](Self::metadata).
    #[must_use]
    pub fn with_progress_token(mut self, token: Value) -> Self {
        self.metadata.insert(PROGRESS_TOKEN_KEY.to_string(), token);
        self
    }

    /// Attach a bidirectional session handle.
    #[must_use]
    pub fn with_session(mut self, session: Arc<dyn McpSession>) -> Self {
        self.session = Some(session);
        self
    }

    /// Attach HTTP headers (case-sensitive keys; [`header`] does
    /// case-insensitive lookup).
    ///
    /// [`header`]: Self::header
    #[must_use]
    pub fn with_headers(mut self, headers: HashbrownMap<String, String>) -> Self {
        self.headers = Some(headers);
        self
    }

    /// Mark the request start time.
    #[cfg(feature = "std")]
    #[must_use]
    pub fn with_start_time(mut self, start: Instant) -> Self {
        self.start_time = Some(start);
        self
    }

    /// Attach a cancellation handle.
    #[cfg(feature = "std")]
    #[must_use]
    pub fn with_cancellation_token(mut self, token: Arc<dyn Cancellable>) -> Self {
        self.cancellation_token = Some(token);
        self
    }
}

// ====================================================================
// Mutable setters (for middleware that doesn't move the context)
// ====================================================================

impl RequestContext {
    /// Mutable metadata insert.
    pub fn insert_metadata(&mut self, key: impl Into<String>, value: impl Into<Value>) {
        self.metadata.insert(key.into(), value.into());
    }

    /// Mutable principal setter.
    pub fn set_principal(&mut self, principal: Principal) {
        self.principal = Some(principal);
    }

    /// Clear the authenticated principal.
    pub fn clear_principal(&mut self) {
        self.principal = None;
    }

    /// Mutable session setter.
    pub fn set_session(&mut self, session: Arc<dyn McpSession>) {
        self.session = Some(session);
    }
}

// ====================================================================
// Accessors
// ====================================================================

impl RequestContext {
    /// Request ID.
    #[inline]
    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    /// Returns true when a non-empty request ID is set.
    #[inline]
    pub fn has_request_id(&self) -> bool {
        !self.request_id.is_empty()
    }

    /// Transport type.
    #[inline]
    pub fn transport(&self) -> TransportType {
        self.transport
    }

    /// Authenticated user ID, if present.
    #[inline]
    pub fn user_id(&self) -> Option<&str> {
        self.user_id.as_deref()
    }

    /// Session ID, if present.
    #[inline]
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// Client ID, if present.
    #[inline]
    pub fn client_id(&self) -> Option<&str> {
        self.client_id.as_deref()
    }

    /// Rich metadata lookup.
    #[inline]
    pub fn get_metadata(&self, key: &str) -> Option<&Value> {
        self.metadata.get(key)
    }

    /// Rich metadata lookup, downcast to `&str` for string values.
    pub fn get_metadata_str(&self, key: &str) -> Option<&str> {
        self.metadata.get(key).and_then(|v| v.as_str())
    }

    /// Returns true when a metadata key is set.
    #[inline]
    pub fn has_metadata(&self, key: &str) -> bool {
        self.metadata.contains_key(key)
    }

    /// Authenticated principal, if any.
    #[inline]
    pub fn principal(&self) -> Option<&Principal> {
        self.principal.as_ref()
    }

    /// Returns true when the request is authenticated.
    ///
    /// A request is considered authenticated when it has either a `principal`
    /// or a `user_id`. Callers with richer auth semantics should read the
    /// principal directly.
    pub fn is_authenticated(&self) -> bool {
        self.principal.is_some() || self.user_id.is_some()
    }

    /// Authenticated subject (principal subject, falling back to `user_id`).
    pub fn subject(&self) -> Option<&str> {
        self.principal
            .as_ref()
            .map(|p| p.subject.as_str())
            .or(self.user_id.as_deref())
    }

    /// Session handle, if attached.
    #[inline]
    pub fn session(&self) -> Option<&Arc<dyn McpSession>> {
        self.session.as_ref()
    }

    /// Returns true when a bidirectional session is attached.
    #[inline]
    pub fn has_session(&self) -> bool {
        self.session.is_some()
    }

    /// The progress token the client attached to this request, if any.
    ///
    /// Present only when the client asked for progress by including
    /// `params._meta.progressToken`. The MCP progress utility requires progress
    /// notifications to reference *only* tokens supplied in an active request,
    /// so a handler reporting progress must use this value and stay silent when
    /// it is `None`.
    ///
    /// The token is `string | number` per the specification, and is returned
    /// exactly as the client sent it.
    #[inline]
    pub fn progress_token(&self) -> Option<&Value> {
        self.metadata.get(PROGRESS_TOKEN_KEY)
    }

    /// Returns true when the client requested progress for this request.
    #[inline]
    pub fn wants_progress(&self) -> bool {
        self.progress_token().is_some()
    }

    /// Record the minimum log severity this session wants.
    ///
    /// Called by the router once `logging/setLevel` has been accepted, so that
    /// [`wants_log`](Self::wants_log) can filter later notifications. No-op
    /// without a session id, since there is nothing to key on.
    #[cfg(feature = "std")]
    pub fn set_min_log_level(&self, level: &str) {
        let (Some(session_id), Some(rank)) =
            (self.session_id.as_ref(), crate::log_level_rank(level))
        else {
            return;
        };
        if let Ok(mut map) = MIN_LOG_LEVEL.write() {
            map.insert(session_id.clone(), rank);
        }
    }

    /// Whether a message at `level` should be sent to this client.
    ///
    /// `true` until the client asks for something narrower with
    /// `logging/setLevel`, and `true` for any level name outside the eight the
    /// spec defines — filtering is a courtesy, and silently dropping an
    /// unrecognised level would be worse than sending it.
    #[cfg(feature = "std")]
    #[must_use]
    pub fn wants_log(&self, level: &str) -> bool {
        let Some(rank) = crate::log_level_rank(level) else {
            return true;
        };
        let Some(session_id) = self.session_id.as_ref() else {
            return true;
        };
        match MIN_LOG_LEVEL.read() {
            Ok(map) => map.get(session_id).is_none_or(|minimum| rank >= *minimum),
            Err(_) => true,
        }
    }

    /// `no_std` builds keep no session store, so nothing is filtered.
    #[cfg(not(feature = "std"))]
    #[must_use]
    pub fn wants_log(&self, _level: &str) -> bool {
        true
    }

    /// All HTTP headers, if the transport captured any.
    #[inline]
    pub fn headers(&self) -> Option<&HashbrownMap<String, String>> {
        self.headers.as_ref()
    }

    /// Case-insensitive HTTP header lookup.
    pub fn header(&self, name: &str) -> Option<&str> {
        let headers = self.headers.as_ref()?;
        headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Elapsed time since the request started (if `start_time` was set).
    #[cfg(feature = "std")]
    pub fn elapsed(&self) -> Option<core::time::Duration> {
        self.start_time.map(|t| t.elapsed())
    }

    /// Returns true when the request has been marked for cancellation.
    #[cfg(feature = "std")]
    pub fn is_cancelled(&self) -> bool {
        self.cancellation_token
            .as_ref()
            .is_some_and(|c| c.is_cancelled())
    }

    /// Authenticated roles, sourced from the principal or from metadata.
    ///
    /// Looks at (in order): `principal.roles`, `metadata["auth"].roles[]`.
    pub fn roles(&self) -> Vec<String> {
        if let Some(p) = &self.principal
            && !p.roles.is_empty()
        {
            return p.roles.to_vec();
        }

        self.metadata
            .get("auth")
            .and_then(|auth| auth.get("roles"))
            .and_then(|r| r.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(ToString::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Returns true when the principal has any of the specified roles.
    /// An empty `required` list always returns true.
    pub fn has_any_role<S: AsRef<str>>(&self, required: &[S]) -> bool {
        if required.is_empty() {
            return true;
        }
        let roles = self.roles();
        required
            .iter()
            .any(|need| roles.iter().any(|have| have == need.as_ref()))
    }
}

// ====================================================================
// Server-to-client operations (require a session)
// ====================================================================

impl RequestContext {
    /// Request LLM sampling from the connected client.
    ///
    /// Requires a bidirectional session; returns
    /// [`McpError::capability_not_supported`] on unidirectional transports.
    pub async fn sample(&self, request: CreateMessageRequest) -> McpResult<CreateMessageResult> {
        let session = self.require_session("sampling/createMessage")?;
        self.require_sampling_capability(session, &request).await?;
        let params = serde_json::to_value(request).map_err(|e| {
            McpError::invalid_params(alloc::format!("Failed to serialize sampling request: {e}"))
        })?;
        let result = session.call("sampling/createMessage", params).await?;
        serde_json::from_value(result)
            .map_err(|e| McpError::internal(alloc::format!("Failed to parse sampling result: {e}")))
    }

    /// Request form-based user input from the client.
    pub async fn elicit_form(
        &self,
        message: impl Into<String>,
        schema: Value,
    ) -> McpResult<ElicitResult> {
        let session = self.require_session("elicitation/create")?;
        self.require_elicitation_capability(session, "form").await?;
        let params = serde_json::json!({
            "mode": "form",
            "message": message.into(),
            "requestedSchema": schema,
        });
        let result = session.call("elicitation/create", params).await?;
        serde_json::from_value(result).map_err(|e| {
            McpError::internal(alloc::format!("Failed to parse elicitation result: {e}"))
        })
    }

    /// Request URL-based user action from the client.
    pub async fn elicit_url(
        &self,
        message: impl Into<String>,
        url: impl Into<String>,
        elicitation_id: impl Into<String>,
    ) -> McpResult<ElicitResult> {
        let session = self.require_session("elicitation/create")?;
        self.require_elicitation_capability(session, "url").await?;
        let params = serde_json::json!({
            "mode": "url",
            "message": message.into(),
            "url": url.into(),
            "elicitationId": elicitation_id.into(),
        });
        let result = session.call("elicitation/create", params).await?;
        serde_json::from_value(result).map_err(|e| {
            McpError::internal(alloc::format!("Failed to parse elicitation result: {e}"))
        })
    }

    /// Ask the client which filesystem roots the server may operate within.
    ///
    /// Requires a bidirectional session. Returns
    /// [`McpError::capability_not_supported`] when the transport has none, or
    /// when the client did not declare the `roots` capability.
    pub async fn list_roots(&self) -> McpResult<Vec<turbomcp_types::Root>> {
        let session = self.require_session("roots/list")?;

        // Only enforce when capabilities are known; a session that cannot
        // report them (tests, in-process harnesses) should not be blocked.
        if let Some(caps) = session.client_capabilities().await?
            && caps.roots.is_none()
        {
            return Err(McpError::capability_not_supported(
                "client roots capability required for roots/list",
            ));
        }

        let result = session.call("roots/list", serde_json::json!({})).await?;
        let parsed: turbomcp_types::ListRootsResult = serde_json::from_value(result)
            .map_err(|e| McpError::internal(alloc::format!("Failed to parse roots result: {e}")))?;
        Ok(parsed.roots)
    }

    /// Tell subscribers that a resource's contents changed.
    ///
    /// Sends `notifications/resources/updated`. This is the obligation a server
    /// takes on by declaring `resources.subscribe` (via `#[subscribe]`): having
    /// accepted a subscription, it must emit this when the resource changes.
    ///
    /// The URI may name a sub-resource of the one the client actually
    /// subscribed to.
    pub async fn notify_resource_updated(&self, uri: impl Into<String>) -> McpResult<()> {
        self.notify_client(
            "notifications/resources/updated",
            serde_json::json!({ "uri": uri.into() }),
        )
        .await
    }

    /// Tell the client the tool list changed, so it should re-list.
    ///
    /// Only meaningful for servers advertising `tools.listChanged`.
    pub async fn notify_tools_list_changed(&self) -> McpResult<()> {
        self.notify_client("notifications/tools/list_changed", serde_json::json!({}))
            .await
    }

    /// Tell the client the resource list changed, so it should re-list.
    ///
    /// Only meaningful for servers advertising `resources.listChanged`.
    pub async fn notify_resources_list_changed(&self) -> McpResult<()> {
        self.notify_client(
            "notifications/resources/list_changed",
            serde_json::json!({}),
        )
        .await
    }

    /// Tell the client the prompt list changed, so it should re-list.
    ///
    /// Only meaningful for servers advertising `prompts.listChanged`.
    pub async fn notify_prompts_list_changed(&self) -> McpResult<()> {
        self.notify_client("notifications/prompts/list_changed", serde_json::json!({}))
            .await
    }

    /// Signal that an out-of-band URL elicitation has finished.
    ///
    /// Sends `notifications/elicitation/complete` with the `elicitationId` from
    /// the originating [`elicit_url`](Self::elicit_url) call, letting the client
    /// retry a request that failed with `URLElicitationRequiredError` or
    /// otherwise resume. Per the specification this goes only to the client that
    /// began the elicitation, which is exactly this request's session.
    pub async fn notify_elicitation_complete(
        &self,
        elicitation_id: impl Into<String>,
    ) -> McpResult<()> {
        self.notify_client(
            "notifications/elicitation/complete",
            serde_json::json!({ "elicitationId": elicitation_id.into() }),
        )
        .await
    }

    /// Send a JSON-RPC notification to the client.
    pub async fn notify_client(&self, method: impl AsRef<str>, params: Value) -> McpResult<()> {
        let session = self.require_session(method.as_ref())?;
        session.notify(method.as_ref(), params).await
    }

    /// Report progress for this request.
    ///
    /// Emits `notifications/progress` carrying the token the client supplied in
    /// `params._meta.progressToken`.
    ///
    /// # When nothing is sent
    ///
    /// This is a no-op returning `Ok(())` when the client did not request
    /// progress (no token), and when the transport has no bidirectional
    /// session. Both are normal: the MCP progress utility makes progress
    /// entirely optional, and forbids referencing a token the client never
    /// issued. Handlers can therefore call this unconditionally.
    ///
    /// Use [`wants_progress`](Self::wants_progress) to skip expensive
    /// instrumentation when nobody is listening.
    ///
    /// # Arguments
    ///
    /// * `progress` - Work done so far. Must increase across successive calls
    ///   for one token, even when `total` is unknown.
    /// * `total` - Total expected, when known.
    /// * `message` - Human-readable status.
    pub async fn report_progress(
        &self,
        progress: f64,
        total: Option<f64>,
        message: Option<&str>,
    ) -> McpResult<()> {
        // Absent token means the client never asked; sending anything here
        // would reference a token that was never issued.
        let Some(token) = self.progress_token() else {
            return Ok(());
        };
        if self.session.is_none() {
            return Ok(());
        }

        let mut params = serde_json::json!({
            "progressToken": token,
            "progress": progress,
        });
        if let Some(total) = total {
            params["total"] = serde_json::json!(total);
        }
        if let Some(message) = message {
            params["message"] = Value::String(message.to_string());
        }

        // Best-effort by design. The progress utility lets a receiver decline
        // to send any progress at all, so an undeliverable notification must
        // not fail the request it describes. This matters concretely on
        // Streamable HTTP, where the session exists for every post-init
        // request but the notification has nowhere to go until the client
        // opens its GET/SSE stream — propagating that turned "no progress
        // stream attached" into a failed tool call.
        let _ = self.notify_client("notifications/progress", params).await;
        Ok(())
    }

    fn require_session(&self, op: &str) -> McpResult<&Arc<dyn McpSession>> {
        self.session.as_ref().ok_or_else(|| {
            McpError::capability_not_supported(alloc::format!(
                "Bidirectional session required for {op} but transport does not support it"
            ))
        })
    }

    async fn require_sampling_capability(
        &self,
        session: &Arc<dyn McpSession>,
        request: &CreateMessageRequest,
    ) -> McpResult<()> {
        let Some(caps) = session.client_capabilities().await? else {
            return Ok(());
        };

        let Some(sampling) = caps.sampling.as_ref() else {
            return Err(McpError::capability_not_supported(
                "client sampling capability required for sampling/createMessage",
            ));
        };

        if (request.tools.is_some() || request.tool_choice.is_some()) && sampling.tools.is_none() {
            return Err(McpError::capability_not_supported(
                "client sampling.tools capability required for tool-enabled sampling/createMessage",
            ));
        }

        if request.task.is_some() && !client_supports_task_sampling(&caps) {
            return Err(McpError::capability_not_supported(
                "client tasks.requests.sampling.createMessage capability required for task-augmented sampling/createMessage",
            ));
        }

        Ok(())
    }

    async fn require_elicitation_capability(
        &self,
        session: &Arc<dyn McpSession>,
        mode: &str,
    ) -> McpResult<()> {
        let Some(caps) = session.client_capabilities().await? else {
            return Ok(());
        };

        let Some(elicitation) = caps.elicitation.as_ref() else {
            return Err(McpError::capability_not_supported(
                "client elicitation capability required for elicitation/create",
            ));
        };

        let supported = match mode {
            "form" => elicitation.supports_form(),
            "url" => elicitation.supports_url(),
            _ => false,
        };

        if supported {
            Ok(())
        } else {
            Err(McpError::capability_not_supported(alloc::format!(
                "client elicitation.{mode} capability required for elicitation/create"
            )))
        }
    }
}

fn client_supports_task_sampling(caps: &ClientCapabilities) -> bool {
    caps.tasks
        .as_ref()
        .and_then(|tasks| tasks.requests.as_ref())
        .and_then(|requests| requests.sampling.as_ref())
        .and_then(|sampling| sampling.create_message.as_ref())
        .is_some()
}

// ====================================================================
// Tests
// ====================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transport_type_display() {
        assert_eq!(TransportType::Stdio.to_string(), "stdio");
        assert_eq!(TransportType::Http.to_string(), "http");
        assert_eq!(TransportType::WebSocket.to_string(), "websocket");
        assert_eq!(TransportType::Tcp.to_string(), "tcp");
        assert_eq!(TransportType::Unix.to_string(), "unix");
        assert_eq!(TransportType::Wasm.to_string(), "wasm");
        assert_eq!(TransportType::Channel.to_string(), "channel");
        assert_eq!(TransportType::Unknown.to_string(), "unknown");
    }

    #[test]
    fn test_transport_type_classification() {
        assert!(TransportType::Http.is_network());
        assert!(TransportType::WebSocket.is_network());
        assert!(TransportType::Tcp.is_network());
        assert!(!TransportType::Stdio.is_network());

        assert!(TransportType::Stdio.is_local());
        assert!(TransportType::Unix.is_local());
        assert!(TransportType::Channel.is_local());
        assert!(!TransportType::Http.is_local());
    }

    #[test]
    fn test_request_context_new() {
        let ctx = RequestContext::with_id_and_transport("test-123", TransportType::Http);
        assert_eq!(ctx.request_id(), "test-123");
        assert_eq!(ctx.transport(), TransportType::Http);
        assert!(ctx.metadata.is_empty());
        assert!(!ctx.has_session());
    }

    #[test]
    fn test_request_context_factory_methods() {
        assert_eq!(RequestContext::stdio().transport(), TransportType::Stdio);
        assert_eq!(RequestContext::http().transport(), TransportType::Http);
        assert_eq!(
            RequestContext::websocket().transport(),
            TransportType::WebSocket
        );
        assert_eq!(RequestContext::tcp().transport(), TransportType::Tcp);
        assert_eq!(RequestContext::unix().transport(), TransportType::Unix);
        assert_eq!(RequestContext::wasm().transport(), TransportType::Wasm);
        assert_eq!(
            RequestContext::channel().transport(),
            TransportType::Channel
        );
    }

    #[test]
    fn test_request_context_metadata() {
        let ctx = RequestContext::with_id_and_transport("1", TransportType::Http)
            .with_metadata("key1", "value1")
            .with_metadata("count", 42);

        assert_eq!(ctx.get_metadata_str("key1"), Some("value1"));
        assert_eq!(ctx.get_metadata("count"), Some(&serde_json::json!(42)));
        assert_eq!(ctx.get_metadata("key3"), None);

        assert!(ctx.has_metadata("key1"));
        assert!(!ctx.has_metadata("key3"));
    }

    #[test]
    fn test_request_context_ids() {
        let ctx = RequestContext::with_id_and_transport("r", TransportType::Http)
            .with_user_id("u")
            .with_session_id("s")
            .with_client_id("c");

        assert_eq!(ctx.user_id(), Some("u"));
        assert_eq!(ctx.session_id(), Some("s"));
        assert_eq!(ctx.client_id(), Some("c"));
        assert!(ctx.is_authenticated());
    }

    #[test]
    fn test_request_context_principal() {
        let ctx = RequestContext::with_id_and_transport("1", TransportType::Http);
        assert!(!ctx.is_authenticated());
        assert!(ctx.principal().is_none());
        assert!(ctx.subject().is_none());

        let principal = Principal::new("user-123")
            .with_email("user@example.com")
            .with_role("admin");

        let ctx = ctx.with_principal(principal);
        assert!(ctx.is_authenticated());
        assert_eq!(ctx.subject(), Some("user-123"));
        assert!(ctx.principal().unwrap().has_role("admin"));
        assert_eq!(ctx.roles(), alloc::vec![String::from("admin")]);
        assert!(ctx.has_any_role(&["admin"]));
        assert!(!ctx.has_any_role(&["root"]));
    }

    #[test]
    fn test_request_context_default() {
        let ctx = RequestContext::default();
        assert!(ctx.request_id.is_empty());
        assert_eq!(ctx.transport, TransportType::Stdio);
        assert!(ctx.metadata.is_empty());
        assert!(!ctx.has_session());
    }

    #[test]
    fn test_request_context_headers() {
        let mut headers: HashbrownMap<String, String> = HashbrownMap::new();
        headers.insert("User-Agent".into(), "Test/1.0".into());
        let ctx =
            RequestContext::with_id_and_transport("1", TransportType::Http).with_headers(headers);

        assert_eq!(ctx.header("user-agent"), Some("Test/1.0"));
        assert_eq!(ctx.header("USER-AGENT"), Some("Test/1.0"));
        assert_eq!(ctx.header("missing"), None);
    }

    #[cfg(feature = "std")]
    #[tokio::test]
    async fn test_sampling_without_session_fails() {
        use turbomcp_types::CreateMessageRequest;
        let ctx = RequestContext::stdio();
        let err = ctx
            .sample(CreateMessageRequest::default())
            .await
            .unwrap_err();
        assert_eq!(err.kind, crate::error::ErrorKind::CapabilityNotSupported);
    }
}
