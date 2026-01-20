//! Minimal request context for cross-platform MCP handlers.
//!
//! This module provides a `RequestContext` type that works on all platforms,
//! including `no_std` environments. Platform-specific extensions (cancellation
//! tokens, UUIDs, etc.) are provided by runtime crates (`turbomcp-server`, `turbomcp-wasm`).
//!
//! # Design Philosophy
//!
//! The context is intentionally minimal:
//! - Uses `BTreeMap` instead of `HashMap` for `no_std` compatibility
//! - No tokio-specific types (CancellationToken, etc.)
//! - Serializable for transport across boundaries
//! - Cloneable for async handler patterns
//!
//! # Example
//!
//! ```rust
//! use turbomcp_core::context::{RequestContext, TransportType};
//!
//! let ctx = RequestContext::new("request-1", TransportType::Http)
//!     .with_metadata("user-agent", "Mozilla/5.0")
//!     .with_metadata("x-request-id", "abc123");
//!
//! assert_eq!(ctx.transport, TransportType::Http);
//! assert_eq!(ctx.get_metadata("user-agent"), Some("Mozilla/5.0"));
//! ```

use crate::auth::Principal;
use alloc::collections::BTreeMap;
use alloc::string::String;
use serde::{Deserialize, Serialize};

/// Transport type identifier.
///
/// Indicates which transport received the request. This is useful for:
/// - Logging and metrics
/// - Transport-specific behavior (e.g., different timeouts)
/// - Debugging and tracing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
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
        matches!(self, Self::Stdio | Self::Unix)
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
            Self::Unknown => "unknown",
        }
    }
}

impl core::fmt::Display for TransportType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Minimal request context that works on all platforms.
///
/// This struct contains only the essential information needed to process
/// a request. Platform-specific extensions (cancellation tokens, UUIDs, etc.)
/// are provided by the runtime layer.
///
/// # Thread Safety
///
/// `RequestContext` is `Send + Sync` on native targets, enabling safe use
/// across async task boundaries. On WASM targets, thread safety is not required.
///
/// # Serialization
///
/// The context is designed to be serializable, enabling transport across
/// process boundaries (e.g., for distributed tracing).
#[derive(Debug, Clone, Default)]
pub struct RequestContext {
    /// Unique request identifier (JSON-RPC id as string, or generated UUID)
    pub request_id: String,
    /// Transport type that received this request
    pub transport: TransportType,
    /// Optional metadata (headers, user info, etc.)
    ///
    /// Uses `BTreeMap` for `no_std` compatibility and deterministic iteration.
    pub metadata: BTreeMap<String, String>,
    /// Authenticated principal (set after successful authentication)
    ///
    /// This field is `None` for unauthenticated requests or when
    /// authentication is not configured.
    pub principal: Option<Principal>,
}

impl RequestContext {
    /// Create a new request context with the given ID and transport.
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp_core::context::{RequestContext, TransportType};
    ///
    /// let ctx = RequestContext::new("req-123", TransportType::Http);
    /// assert_eq!(ctx.request_id, "req-123");
    /// ```
    pub fn new(request_id: impl Into<String>, transport: TransportType) -> Self {
        Self {
            request_id: request_id.into(),
            transport,
            metadata: BTreeMap::new(),
            principal: None,
        }
    }

    /// Create a context for STDIO transport.
    #[inline]
    pub fn stdio() -> Self {
        Self::new("", TransportType::Stdio)
    }

    /// Create a context for HTTP transport.
    #[inline]
    pub fn http() -> Self {
        Self::new("", TransportType::Http)
    }

    /// Create a context for WebSocket transport.
    #[inline]
    pub fn websocket() -> Self {
        Self::new("", TransportType::WebSocket)
    }

    /// Create a context for TCP transport.
    #[inline]
    pub fn tcp() -> Self {
        Self::new("", TransportType::Tcp)
    }

    /// Create a context for WASM transport.
    #[inline]
    pub fn wasm() -> Self {
        Self::new("", TransportType::Wasm)
    }

    /// Add metadata to the context.
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp_core::context::{RequestContext, TransportType};
    ///
    /// let ctx = RequestContext::new("1", TransportType::Http)
    ///     .with_metadata("user-agent", "MyClient/1.0")
    ///     .with_metadata("x-trace-id", "abc123");
    ///
    /// assert_eq!(ctx.get_metadata("user-agent"), Some("MyClient/1.0"));
    /// ```
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Add metadata to the context (mutable version).
    pub fn insert_metadata(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.metadata.insert(key.into(), value.into());
    }

    /// Get metadata value by key.
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp_core::context::{RequestContext, TransportType};
    ///
    /// let ctx = RequestContext::new("1", TransportType::Http)
    ///     .with_metadata("key", "value");
    ///
    /// assert_eq!(ctx.get_metadata("key"), Some("value"));
    /// assert_eq!(ctx.get_metadata("missing"), None);
    /// ```
    pub fn get_metadata(&self, key: &str) -> Option<&str> {
        self.metadata.get(key).map(|s| s.as_str())
    }

    /// Check if metadata contains a key.
    pub fn has_metadata(&self, key: &str) -> bool {
        self.metadata.contains_key(key)
    }

    /// Set the request ID.
    pub fn with_request_id(mut self, id: impl Into<String>) -> Self {
        self.request_id = id.into();
        self
    }

    /// Returns true if this context has a valid (non-empty) request ID.
    pub fn has_request_id(&self) -> bool {
        !self.request_id.is_empty()
    }

    /// Set the authenticated principal.
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp_core::context::{RequestContext, TransportType};
    /// use turbomcp_core::auth::Principal;
    ///
    /// let ctx = RequestContext::new("1", TransportType::Http)
    ///     .with_principal(Principal::new("user-123"));
    ///
    /// assert!(ctx.principal().is_some());
    /// assert_eq!(ctx.principal().unwrap().subject, "user-123");
    /// ```
    pub fn with_principal(mut self, principal: Principal) -> Self {
        self.principal = Some(principal);
        self
    }

    /// Set the authenticated principal (mutable version).
    pub fn set_principal(&mut self, principal: Principal) {
        self.principal = Some(principal);
    }

    /// Get the authenticated principal, if any.
    ///
    /// Returns `None` if the request was not authenticated or if
    /// authentication is not configured.
    pub fn principal(&self) -> Option<&Principal> {
        self.principal.as_ref()
    }

    /// Returns true if this context has an authenticated principal.
    pub fn is_authenticated(&self) -> bool {
        self.principal.is_some()
    }

    /// Get the subject of the authenticated principal.
    ///
    /// Convenience method that returns `None` if not authenticated.
    pub fn subject(&self) -> Option<&str> {
        self.principal.as_ref().map(|p| p.subject.as_str())
    }

    /// Clear the authenticated principal.
    pub fn clear_principal(&mut self) {
        self.principal = None;
    }
}

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
        assert!(!TransportType::Http.is_local());
    }

    #[test]
    fn test_request_context_new() {
        let ctx = RequestContext::new("test-123", TransportType::Http);
        assert_eq!(ctx.request_id, "test-123");
        assert_eq!(ctx.transport, TransportType::Http);
        assert!(ctx.metadata.is_empty());
    }

    #[test]
    fn test_request_context_factory_methods() {
        assert_eq!(RequestContext::stdio().transport, TransportType::Stdio);
        assert_eq!(RequestContext::http().transport, TransportType::Http);
        assert_eq!(
            RequestContext::websocket().transport,
            TransportType::WebSocket
        );
        assert_eq!(RequestContext::tcp().transport, TransportType::Tcp);
        assert_eq!(RequestContext::wasm().transport, TransportType::Wasm);
    }

    #[test]
    fn test_request_context_metadata() {
        let ctx = RequestContext::new("1", TransportType::Http)
            .with_metadata("key1", "value1")
            .with_metadata("key2", "value2");

        assert_eq!(ctx.get_metadata("key1"), Some("value1"));
        assert_eq!(ctx.get_metadata("key2"), Some("value2"));
        assert_eq!(ctx.get_metadata("key3"), None);

        assert!(ctx.has_metadata("key1"));
        assert!(!ctx.has_metadata("key3"));
    }

    #[test]
    fn test_request_context_mutable_metadata() {
        let mut ctx = RequestContext::new("1", TransportType::Http);
        ctx.insert_metadata("key", "value");
        assert_eq!(ctx.get_metadata("key"), Some("value"));
    }

    #[test]
    fn test_request_context_request_id() {
        let ctx = RequestContext::new("", TransportType::Http);
        assert!(!ctx.has_request_id());

        let ctx = ctx.with_request_id("request-456");
        assert!(ctx.has_request_id());
        assert_eq!(ctx.request_id, "request-456");
    }

    #[test]
    fn test_request_context_default() {
        let ctx = RequestContext::default();
        assert_eq!(ctx.request_id, "");
        assert_eq!(ctx.transport, TransportType::Stdio);
        assert!(ctx.metadata.is_empty());
    }

    #[test]
    fn test_request_context_clone() {
        let ctx1 = RequestContext::new("1", TransportType::Http).with_metadata("key", "value");
        let ctx2 = ctx1.clone();

        assert_eq!(ctx1.request_id, ctx2.request_id);
        assert_eq!(ctx1.transport, ctx2.transport);
        assert_eq!(ctx1.get_metadata("key"), ctx2.get_metadata("key"));
    }

    #[test]
    fn test_request_context_principal() {
        let ctx = RequestContext::new("1", TransportType::Http);
        assert!(!ctx.is_authenticated());
        assert!(ctx.principal().is_none());
        assert!(ctx.subject().is_none());

        let principal = Principal::new("user-123")
            .with_email("user@example.com")
            .with_role("admin");

        let ctx = ctx.with_principal(principal);
        assert!(ctx.is_authenticated());
        assert!(ctx.principal().is_some());
        assert_eq!(ctx.subject(), Some("user-123"));
        assert_eq!(ctx.principal().unwrap().email, Some("user@example.com".to_string()));
        assert!(ctx.principal().unwrap().has_role("admin"));
    }

    #[test]
    fn test_request_context_principal_mutable() {
        let mut ctx = RequestContext::new("1", TransportType::Http);
        assert!(!ctx.is_authenticated());

        ctx.set_principal(Principal::new("user-456"));
        assert!(ctx.is_authenticated());
        assert_eq!(ctx.subject(), Some("user-456"));

        ctx.clear_principal();
        assert!(!ctx.is_authenticated());
    }
}
