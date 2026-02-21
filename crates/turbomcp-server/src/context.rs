//! Request context for MCP handlers.
//!
//! This module provides a server-specific request context extending the core
//! context with runtime features like cancellation, timing, and structured headers.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use tokio_util::sync::CancellationToken;
use turbomcp_core::error::McpResult;
use turbomcp_types::{CreateMessageRequest, CreateMessageResult, ElicitResult};
use uuid::Uuid;

// Re-export TransportType from core for unified type system (DRY)
pub use turbomcp_core::context::TransportType;

/// Trait for bidirectional session communication.
#[async_trait::async_trait]
pub trait McpSession: Send + Sync + std::fmt::Debug {
    /// Send a request to the client and wait for a response.
    async fn call(&self, method: &str, params: serde_json::Value) -> McpResult<serde_json::Value>;
    /// Send a notification to the client.
    async fn notify(&self, method: &str, params: serde_json::Value) -> McpResult<()>;
}

/// Context information for an MCP request.
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
    /// Session handle for bidirectional communication
    session: Option<Arc<dyn McpSession>>,
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
            session: None,
        }
    }

    /// Set the session handle for bidirectional communication.
    #[must_use]
    pub fn with_session(mut self, session: Arc<dyn McpSession>) -> Self {
        self.session = Some(session);
        self
    }

    /// Request user input via a form.
    pub async fn elicit_form(
        &self,
        message: impl Into<String>,
        schema: serde_json::Value,
    ) -> McpResult<ElicitResult> {
        let session = self.session.as_ref().ok_or_else(|| {
            turbomcp_core::error::McpError::capability_not_supported(
                "Server-to-client requests not available on this transport",
            )
        })?;

        let params = serde_json::json!({
            "mode": "form",
            "message": message.into(),
            "requestedSchema": schema
        });

        let result = session.call("elicitation/create", params).await?;
        serde_json::from_value(result).map_err(|e| {
            turbomcp_core::error::McpError::internal(format!(
                "Failed to parse elicit result: {}",
                e
            ))
        })
    }

    /// Request user action via a URL.
    pub async fn elicit_url(
        &self,
        message: impl Into<String>,
        url: impl Into<String>,
        elicitation_id: impl Into<String>,
    ) -> McpResult<ElicitResult> {
        let session = self.session.as_ref().ok_or_else(|| {
            turbomcp_core::error::McpError::capability_not_supported(
                "Server-to-client requests not available on this transport",
            )
        })?;

        let params = serde_json::json!({
            "mode": "url",
            "message": message.into(),
            "url": url.into(),
            "elicitationId": elicitation_id.into()
        });

        let result = session.call("elicitation/create", params).await?;
        serde_json::from_value(result).map_err(|e| {
            turbomcp_core::error::McpError::internal(format!(
                "Failed to parse elicit result: {}",
                e
            ))
        })
    }

    /// Request LLM sampling from the client.
    pub async fn sample(&self, request: CreateMessageRequest) -> McpResult<CreateMessageResult> {
        let session = self.session.as_ref().ok_or_else(|| {
            turbomcp_core::error::McpError::capability_not_supported(
                "Server-to-client requests not available on this transport",
            )
        })?;

        let params = serde_json::to_value(request).map_err(|e| {
            turbomcp_core::error::McpError::invalid_params(format!(
                "Failed to serialize sampling request: {}",
                e
            ))
        })?;

        let result = session.call("sampling/createMessage", params).await?;
        serde_json::from_value(result).map_err(|e| {
            turbomcp_core::error::McpError::internal(format!(
                "Failed to parse sampling result: {}",
                e
            ))
        })
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

    /// Convert to the core RequestContext type.
    ///
    /// This method creates a minimal context compatible with the unified
    /// `turbomcp_core::McpHandler` trait. Headers and auth fields are
    /// encoded as metadata with standard prefixes.
    #[must_use]
    pub fn to_core_context(&self) -> turbomcp_core::context::RequestContext {
        // TransportType is re-exported from core, so no conversion needed
        let mut core_ctx =
            turbomcp_core::context::RequestContext::new(&self.request_id, self.transport);

        // Copy headers as metadata with "header:" prefix
        if let Some(ref headers) = self.headers {
            for (key, value) in headers {
                core_ctx.insert_metadata(format!("header:{key}"), value.clone());
            }
        }

        // Copy auth/session fields as metadata
        if let Some(ref user_id) = self.user_id {
            core_ctx.insert_metadata("user_id", user_id.clone());
        }
        if let Some(ref session_id) = self.session_id {
            core_ctx.insert_metadata("session_id", session_id.clone());
        }
        if let Some(ref client_id) = self.client_id {
            core_ctx.insert_metadata("client_id", client_id.clone());
        }

        core_ctx
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
