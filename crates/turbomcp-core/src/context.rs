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

use turbomcp_types::{CreateMessageRequest, CreateMessageResult, ElicitResult};

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

    /// Send a JSON-RPC notification to the client.
    pub async fn notify_client(&self, method: impl AsRef<str>, params: Value) -> McpResult<()> {
        let session = self.require_session(method.as_ref())?;
        session.notify(method.as_ref(), params).await
    }

    fn require_session(&self, op: &str) -> McpResult<&Arc<dyn McpSession>> {
        self.session.as_ref().ok_or_else(|| {
            McpError::capability_not_supported(alloc::format!(
                "Bidirectional session required for {op} but transport does not support it"
            ))
        })
    }
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
