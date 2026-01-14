//! Request context for v3 handlers.
//!
//! This module provides a simplified request context for the v3 architecture,
//! containing essential information about the incoming request.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Type of transport used for the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TransportType {
    /// Standard input/output transport
    #[default]
    Stdio,
    /// HTTP with Server-Sent Events
    Http,
    /// WebSocket transport
    WebSocket,
    /// TCP socket transport
    Tcp,
    /// Unix domain socket transport
    Unix,
    /// WebAssembly (Cloudflare Workers, etc.)
    Wasm,
}

impl TransportType {
    /// Returns the transport type as a string.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Stdio => "stdio",
            Self::Http => "http",
            Self::WebSocket => "websocket",
            Self::Tcp => "tcp",
            Self::Unix => "unix",
            Self::Wasm => "wasm",
        }
    }
}

impl std::fmt::Display for TransportType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Context information for a v3 MCP request.
///
/// This is a simplified context compared to the full protocol RequestContext,
/// focusing on essential information for handler execution.
///
/// # Example
///
/// ```
/// use turbomcp_server::v3::{RequestContext, TransportType};
///
/// let ctx = RequestContext::new()
///     .with_transport(TransportType::Http)
///     .with_user_id("user-123");
///
/// assert_eq!(ctx.transport(), TransportType::Http);
/// assert_eq!(ctx.user_id(), Some("user-123"));
/// ```
#[derive(Debug, Clone)]
pub struct RequestContext {
    /// Unique request identifier
    request_id: String,
    /// Transport type used for this request
    transport: TransportType,
    /// Time when request processing started
    start_time: Instant,
    /// HTTP headers (if applicable)
    headers: Option<HashMap<String, String>>,
    /// User ID (if authenticated)
    user_id: Option<String>,
    /// Session ID
    session_id: Option<String>,
    /// Client ID
    client_id: Option<String>,
    /// Custom metadata
    metadata: HashMap<String, serde_json::Value>,
    /// Cancellation token for cooperative cancellation
    cancellation_token: Option<Arc<CancellationToken>>,
}

impl Default for RequestContext {
    fn default() -> Self {
        Self::new()
    }
}

impl RequestContext {
    /// Create a new request context with a generated UUID.
    #[must_use]
    pub fn new() -> Self {
        Self {
            request_id: Uuid::new_v4().to_string(),
            transport: TransportType::Stdio,
            start_time: Instant::now(),
            headers: None,
            user_id: None,
            session_id: None,
            client_id: None,
            metadata: HashMap::new(),
            cancellation_token: None,
        }
    }

    /// Create a new request context for STDIO transport.
    #[must_use]
    pub fn stdio() -> Self {
        Self::new().with_transport(TransportType::Stdio)
    }

    /// Create a new request context for HTTP transport.
    #[must_use]
    pub fn http() -> Self {
        Self::new().with_transport(TransportType::Http)
    }

    /// Create a new request context for WebSocket transport.
    #[must_use]
    pub fn websocket() -> Self {
        Self::new().with_transport(TransportType::WebSocket)
    }

    /// Create a new request context for TCP transport.
    #[must_use]
    pub fn tcp() -> Self {
        Self::new().with_transport(TransportType::Tcp)
    }

    /// Create a new request context for Unix socket transport.
    #[must_use]
    pub fn unix() -> Self {
        Self::new().with_transport(TransportType::Unix)
    }

    /// Create a new request context for WASM transport.
    #[must_use]
    pub fn wasm() -> Self {
        Self::new().with_transport(TransportType::Wasm)
    }

    /// Create a new request context with a specific request ID.
    #[must_use]
    pub fn with_id(id: impl Into<String>) -> Self {
        Self {
            request_id: id.into(),
            ..Self::new()
        }
    }

    /// Set the transport type.
    #[must_use]
    pub fn with_transport(mut self, transport: TransportType) -> Self {
        self.transport = transport;
        self
    }

    /// Set the HTTP headers.
    #[must_use]
    pub fn with_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers = Some(headers);
        self
    }

    /// Set the user ID.
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

    /// Add a metadata key-value pair.
    #[must_use]
    pub fn with_metadata(
        mut self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Set the cancellation token.
    #[must_use]
    pub fn with_cancellation_token(mut self, token: Arc<CancellationToken>) -> Self {
        self.cancellation_token = Some(token);
        self
    }

    /// Get the request ID.
    #[must_use]
    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    /// Get the transport type.
    #[must_use]
    pub fn transport(&self) -> TransportType {
        self.transport
    }

    /// Get all HTTP headers.
    #[must_use]
    pub fn headers(&self) -> Option<&HashMap<String, String>> {
        self.headers.as_ref()
    }

    /// Get a specific HTTP header (case-insensitive).
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        let headers = self.headers.as_ref()?;
        let name_lower = name.to_lowercase();
        headers
            .iter()
            .find(|(key, _)| key.to_lowercase() == name_lower)
            .map(|(_, value)| value.as_str())
    }

    /// Get the user ID.
    #[must_use]
    pub fn user_id(&self) -> Option<&str> {
        self.user_id.as_deref()
    }

    /// Get the session ID.
    #[must_use]
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// Get the client ID.
    #[must_use]
    pub fn client_id(&self) -> Option<&str> {
        self.client_id.as_deref()
    }

    /// Get a metadata value.
    #[must_use]
    pub fn get_metadata(&self, key: &str) -> Option<&serde_json::Value> {
        self.metadata.get(key)
    }

    /// Get the elapsed time since request processing started.
    #[must_use]
    pub fn elapsed(&self) -> std::time::Duration {
        self.start_time.elapsed()
    }

    /// Check if the request has been cancelled.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancellation_token
            .as_ref()
            .is_some_and(|t| t.is_cancelled())
    }

    /// Check if the user is authenticated.
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        self.user_id.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_context() {
        let ctx = RequestContext::new();
        assert!(!ctx.request_id().is_empty());
        assert_eq!(ctx.transport(), TransportType::Stdio);
        assert!(!ctx.is_cancelled());
    }

    #[test]
    fn test_with_id() {
        let ctx = RequestContext::with_id("test-123");
        assert_eq!(ctx.request_id(), "test-123");
    }

    #[test]
    fn test_transport_types() {
        let ctx = RequestContext::new().with_transport(TransportType::Http);
        assert_eq!(ctx.transport(), TransportType::Http);
        assert_eq!(ctx.transport().as_str(), "http");
    }

    #[test]
    fn test_headers() {
        let mut headers = HashMap::new();
        headers.insert("User-Agent".to_string(), "Test/1.0".to_string());
        headers.insert("Content-Type".to_string(), "application/json".to_string());

        let ctx = RequestContext::new().with_headers(headers);

        assert!(ctx.headers().is_some());
        // Case-insensitive lookup
        assert_eq!(ctx.header("user-agent"), Some("Test/1.0"));
        assert_eq!(ctx.header("USER-AGENT"), Some("Test/1.0"));
        assert_eq!(ctx.header("content-type"), Some("application/json"));
        assert_eq!(ctx.header("x-custom"), None);
    }

    #[test]
    fn test_user_id() {
        let ctx = RequestContext::new().with_user_id("user-123");
        assert_eq!(ctx.user_id(), Some("user-123"));
        assert!(ctx.is_authenticated());
    }

    #[test]
    fn test_metadata() {
        let ctx = RequestContext::new()
            .with_metadata("key1", "value1")
            .with_metadata("key2", serde_json::json!(42));

        assert_eq!(
            ctx.get_metadata("key1"),
            Some(&serde_json::Value::String("value1".to_string()))
        );
        assert_eq!(ctx.get_metadata("key2"), Some(&serde_json::json!(42)));
        assert_eq!(ctx.get_metadata("key3"), None);
    }

    #[test]
    fn test_cancellation() {
        let token = Arc::new(CancellationToken::new());
        let ctx = RequestContext::new().with_cancellation_token(token.clone());

        assert!(!ctx.is_cancelled());
        token.cancel();
        assert!(ctx.is_cancelled());
    }
}
