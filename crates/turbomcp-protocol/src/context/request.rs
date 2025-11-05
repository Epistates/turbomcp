//! Request and response context types for MCP request handling.
//!
//! This module contains the core context types used throughout the MCP protocol
//! implementation for tracking request metadata, response information, and analytics.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::Instant;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::capabilities::ServerToClientRequests;
use crate::types::Timestamp;

/// Context information for a single MCP request, carried through its entire lifecycle.
///
/// This struct contains essential metadata for processing, logging, and tracing a request,
/// including unique identifiers, authentication information, and mechanisms for
/// cancellation and server-initiated communication.
#[derive(Clone)]
pub struct RequestContext {
    /// A unique identifier for the request, typically a UUID.
    pub request_id: String,

    /// The identifier for the user making the request, if authenticated.
    pub user_id: Option<String>,

    /// The identifier for the session to which this request belongs.
    pub session_id: Option<String>,

    /// The identifier for the client application making the request.
    pub client_id: Option<String>,

    /// The timestamp when the request was received.
    pub timestamp: Timestamp,

    /// The `Instant` when request processing started, used for performance tracking.
    pub start_time: Instant,

    /// A collection of custom metadata for application-specific use cases.
    pub metadata: Arc<HashMap<String, serde_json::Value>>,

    /// The tracing span associated with this request for observability.
    #[cfg(feature = "tracing")]
    pub span: Option<tracing::Span>,

    /// A token that can be used to signal cancellation of the request.
    pub cancellation_token: Option<Arc<CancellationToken>>,

    /// An interface for making server-initiated requests back to the client (e.g., sampling, elicitation).
    /// This is hidden from public docs as it's an internal detail injected by the server.
    #[doc(hidden)]
    pub(crate) server_to_client: Option<Arc<dyn ServerToClientRequests>>,
}

impl fmt::Debug for RequestContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RequestContext")
            .field("request_id", &self.request_id)
            .field("user_id", &self.user_id)
            .field("session_id", &self.session_id)
            .field("client_id", &self.client_id)
            .field("timestamp", &self.timestamp)
            .field("metadata", &self.metadata)
            .field("server_to_client", &self.server_to_client.is_some())
            .finish()
    }
}

/// Context information generated after processing a request, containing response details.
#[derive(Debug, Clone)]
pub struct ResponseContext {
    /// The ID of the original request this response is for.
    pub request_id: String,

    /// The timestamp when the response was generated.
    pub timestamp: Timestamp,

    /// The total time taken to process the request.
    pub duration: std::time::Duration,

    /// The status of the response (e.g., Success, Error).
    pub status: ResponseStatus,

    /// A collection of custom metadata for the response.
    pub metadata: Arc<HashMap<String, serde_json::Value>>,
}

/// Represents the status of an MCP response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResponseStatus {
    /// The request was processed successfully.
    Success,
    /// An error occurred during request processing.
    Error {
        /// A numeric code indicating the error type.
        code: i32,
        /// A human-readable message describing the error.
        message: String,
    },
    /// The response is partial, indicating more data will follow (for streaming).
    Partial,
    /// The request was cancelled before completion.
    Cancelled,
}

/// Contains analytics information for a single request, used for monitoring and debugging.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestInfo {
    /// The timestamp when the request was received.
    pub timestamp: DateTime<Utc>,
    /// The identifier of the client that made the request.
    pub client_id: String,
    /// The name of the tool or method that was called.
    pub method_name: String,
    /// The parameters provided in the request, potentially sanitized for privacy.
    pub parameters: serde_json::Value,
    /// The total time taken to generate a response, in milliseconds.
    pub response_time_ms: Option<u64>,
    /// A boolean indicating whether the request was successful.
    pub success: bool,
    /// The error message, if the request failed.
    pub error_message: Option<String>,
    /// The HTTP status code, if the request was handled over HTTP.
    pub status_code: Option<u16>,
    /// Additional custom metadata for analytics.
    pub metadata: HashMap<String, serde_json::Value>,
}

impl RequestContext {
    /// Creates a new `RequestContext` with a generated UUIDv4 as the request ID.
    #[must_use]
    pub fn new() -> Self {
        Self {
            request_id: Uuid::new_v4().to_string(),
            user_id: None,
            session_id: None,
            client_id: None,
            timestamp: Timestamp::now(),
            start_time: Instant::now(),
            metadata: Arc::new(HashMap::new()),
            #[cfg(feature = "tracing")]
            span: None,
            cancellation_token: None,
            server_to_client: None,
        }
    }

    /// Creates a new `RequestContext` with a specific request ID.
    pub fn with_id(id: impl Into<String>) -> Self {
        Self {
            request_id: id.into(),
            ..Self::new()
        }
    }

    /// Sets the user ID for this context, returning the modified context.
    ///
    /// # Example
    /// ```
    /// # use turbomcp_protocol::context::RequestContext;
    /// let ctx = RequestContext::new().with_user_id("user-123");
    /// assert_eq!(ctx.user_id, Some("user-123".to_string()));
    /// ```
    #[must_use]
    pub fn with_user_id(mut self, user_id: impl Into<String>) -> Self {
        self.user_id = Some(user_id.into());
        self
    }

    /// Sets the session ID for this context, returning the modified context.
    #[must_use]
    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    /// Sets the client ID for this context, returning the modified context.
    #[must_use]
    pub fn with_client_id(mut self, client_id: impl Into<String>) -> Self {
        self.client_id = Some(client_id.into());
        self
    }

    /// Adds a key-value pair to the metadata, returning the modified context.
    ///
    /// # Example
    /// ```
    /// # use turbomcp_protocol::context::RequestContext;
    /// # use serde_json::json;
    /// let ctx = RequestContext::new().with_metadata("tenant", json!("acme-corp"));
    /// assert_eq!(ctx.get_metadata("tenant"), Some(&json!("acme-corp")));
    /// ```
    #[must_use]
    pub fn with_metadata(
        mut self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        Arc::make_mut(&mut self.metadata).insert(key.into(), value.into());
        self
    }

    /// Retrieves a value from the metadata by key.
    #[must_use]
    pub fn get_metadata(&self, key: &str) -> Option<&serde_json::Value> {
        self.metadata.get(key)
    }

    /// Returns the elapsed time since the request processing started.
    #[must_use]
    pub fn elapsed(&self) -> std::time::Duration {
        self.start_time.elapsed()
    }

    /// Checks if the request has been marked for cancellation.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancellation_token
            .as_ref()
            .is_some_and(|token| token.is_cancelled())
    }

    /// Sets the server-to-client requests interface for this context.
    ///
    /// This enables tools to make server-initiated requests (sampling, elicitation, roots)
    /// with full context propagation for tracing and attribution. This is typically called
    /// by the server implementation.
    #[must_use]
    pub fn with_server_to_client(mut self, capabilities: Arc<dyn ServerToClientRequests>) -> Self {
        self.server_to_client = Some(capabilities);
        self
    }

    /// Sets the cancellation token for cooperative cancellation.
    /// This is typically called by the server implementation.
    #[must_use]
    pub fn with_cancellation_token(mut self, token: Arc<CancellationToken>) -> Self {
        self.cancellation_token = Some(token);
        self
    }

    /// Returns the user ID from the request context, if available.
    #[must_use]
    pub fn user(&self) -> Option<&str> {
        self.user_id.as_deref()
    }

    /// Checks if the request is from an authenticated client.
    /// This is determined by metadata set during the authentication process.
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        self.get_metadata("client_authenticated")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    /// Returns the user roles from the request context, if available.
    /// Roles are typically populated from an authentication token.
    #[must_use]
    pub fn roles(&self) -> Vec<String> {
        self.get_metadata("auth")
            .and_then(|auth| auth.get("roles"))
            .and_then(|roles| roles.as_array())
            .map(|roles| {
                roles
                    .iter()
                    .filter_map(|role| role.as_str().map(ToString::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Checks if the user has any of the specified roles.
    /// Returns `true` if the required roles list is empty or if the user has at least one of the roles.
    pub fn has_any_role<S: AsRef<str>>(&self, required: &[S]) -> bool {
        if required.is_empty() {
            return true; // Empty requirement always passes
        }

        let user_roles = self.roles();
        required
            .iter()
            .any(|required_role| user_roles.contains(&required_role.as_ref().to_string()))
    }

    /// Gets the server-to-client requests interface.
    ///
    /// Returns `None` if not configured (e.g., for unidirectional transports).
    /// This is hidden from public docs as it's an internal detail for use by server tools.
    #[doc(hidden)]
    pub fn server_to_client(&self) -> Option<&Arc<dyn ServerToClientRequests>> {
        self.server_to_client.as_ref()
    }

    /// Returns all HTTP headers from the request, if available.
    ///
    /// Headers are automatically extracted by HTTP and WebSocket transports and stored
    /// in the context metadata. Returns `None` if not using an HTTP-based transport
    /// or if headers were not extracted.
    ///
    /// # Example
    /// ```
    /// # use turbomcp_protocol::RequestContext;
    /// # let ctx = RequestContext::new();
    /// if let Some(headers) = ctx.headers() {
    ///     for (name, value) in headers.iter() {
    ///         println!("{}: {}", name, value);
    ///     }
    /// }
    /// ```
    #[must_use]
    pub fn headers(&self) -> Option<HashMap<String, String>> {
        self.get_metadata("http_headers")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    /// Returns a specific HTTP header value by name (case-insensitive).
    ///
    /// This method performs case-insensitive header lookup, as per HTTP specification.
    /// Returns `None` if the header is not present or if not using an HTTP-based transport.
    ///
    /// # Example
    /// ```
    /// # use turbomcp_protocol::RequestContext;
    /// # let ctx = RequestContext::new();
    /// if let Some(user_agent) = ctx.header("user-agent") {
    ///     println!("User-Agent: {}", user_agent);
    /// }
    /// ```
    #[must_use]
    pub fn header(&self, name: &str) -> Option<String> {
        let headers = self.headers()?;
        let name_lower = name.to_lowercase();

        // HTTP headers are case-insensitive, so we need to search with lowercase comparison
        headers
            .iter()
            .find(|(key, _)| key.to_lowercase() == name_lower)
            .map(|(_, value)| value.clone())
    }

    /// Returns the transport type used for this request.
    ///
    /// Common transport types include: "http", "websocket", "stdio", "tcp", "unix".
    /// Returns `None` if transport metadata is not set.
    ///
    /// # Example
    /// ```
    /// # use turbomcp_protocol::RequestContext;
    /// # let ctx = RequestContext::new();
    /// if let Some(transport) = ctx.transport() {
    ///     println!("Request received via: {}", transport);
    /// }
    /// ```
    #[must_use]
    pub fn transport(&self) -> Option<String> {
        self.get_metadata("transport")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }
}

impl Default for RequestContext {
    fn default() -> Self {
        Self::new()
    }
}

impl ResponseContext {
    /// Creates a new `ResponseContext` for a successful operation.
    pub fn success(request_id: impl Into<String>, duration: std::time::Duration) -> Self {
        Self {
            request_id: request_id.into(),
            timestamp: Timestamp::now(),
            duration,
            status: ResponseStatus::Success,
            metadata: Arc::new(HashMap::new()),
        }
    }

    /// Creates a new `ResponseContext` for a failed operation.
    pub fn error(
        request_id: impl Into<String>,
        duration: std::time::Duration,
        code: i32,
        message: impl Into<String>,
    ) -> Self {
        Self {
            request_id: request_id.into(),
            timestamp: Timestamp::now(),
            duration,
            status: ResponseStatus::Error {
                code,
                message: message.into(),
            },
            metadata: Arc::new(HashMap::new()),
        }
    }
}

impl RequestInfo {
    /// Creates a new `RequestInfo` for analytics.
    #[must_use]
    pub fn new(client_id: String, method_name: String, parameters: serde_json::Value) -> Self {
        Self {
            timestamp: Utc::now(),
            client_id,
            method_name,
            parameters,
            response_time_ms: None,
            success: false,
            error_message: None,
            status_code: None,
            metadata: HashMap::new(),
        }
    }

    /// Marks the request as completed successfully and records the response time.
    #[must_use]
    pub const fn complete_success(mut self, response_time_ms: u64) -> Self {
        self.response_time_ms = Some(response_time_ms);
        self.success = true;
        self.status_code = Some(200);
        self
    }

    /// Marks the request as failed and records the response time and error message.
    #[must_use]
    pub fn complete_error(mut self, response_time_ms: u64, error: String) -> Self {
        self.response_time_ms = Some(response_time_ms);
        self.success = false;
        self.error_message = Some(error);
        self.status_code = Some(500);
        self
    }

    /// Sets the HTTP status code for this request.
    #[must_use]
    pub const fn with_status_code(mut self, code: u16) -> Self {
        self.status_code = Some(code);
        self
    }

    /// Adds a key-value pair to the analytics metadata.
    #[must_use]
    pub fn with_metadata(mut self, key: String, value: serde_json::Value) -> Self {
        self.metadata.insert(key, value);
        self
    }
}

/// An extension trait for `RequestContext` providing enhanced client ID handling.
pub trait RequestContextExt {
    /// Sets the client ID using the structured `ClientId` enum, which includes the method of identification.
    #[must_use]
    fn with_enhanced_client_id(self, client_id: super::client::ClientId) -> Self;

    /// Extracts a client ID from headers or query parameters and sets it on the context.
    #[must_use]
    fn extract_client_id(
        self,
        extractor: &super::client::ClientIdExtractor,
        headers: Option<&HashMap<String, String>>,
        query_params: Option<&HashMap<String, String>>,
    ) -> Self;

    /// Gets the structured `ClientId` enum from the context, if available.
    fn get_enhanced_client_id(&self) -> Option<super::client::ClientId>;
}

impl RequestContextExt for RequestContext {
    fn with_enhanced_client_id(self, client_id: super::client::ClientId) -> Self {
        self.with_client_id(client_id.as_str())
            .with_metadata(
                "client_id_method".to_string(),
                serde_json::Value::String(client_id.auth_method().to_string()),
            )
            .with_metadata(
                "client_authenticated".to_string(),
                serde_json::Value::Bool(client_id.is_authenticated()),
            )
    }

    fn extract_client_id(
        self,
        extractor: &super::client::ClientIdExtractor,
        headers: Option<&HashMap<String, String>>,
        query_params: Option<&HashMap<String, String>>,
    ) -> Self {
        let client_id = extractor.extract_client_id(headers, query_params);
        self.with_enhanced_client_id(client_id)
    }

    fn get_enhanced_client_id(&self) -> Option<super::client::ClientId> {
        self.client_id.as_ref().map(|id| {
            let method = self
                .get_metadata("client_id_method")
                .and_then(|v| v.as_str())
                .unwrap_or("header");

            match method {
                "bearer_token" => super::client::ClientId::Token(id.clone()),
                "session_cookie" => super::client::ClientId::Session(id.clone()),
                "query_param" => super::client::ClientId::QueryParam(id.clone()),
                "user_agent" => super::client::ClientId::UserAgent(id.clone()),
                "anonymous" => super::client::ClientId::Anonymous,
                _ => super::client::ClientId::Header(id.clone()), // Default to header for "header" and unknown methods
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_headers_returns_none_when_not_set() {
        let ctx = RequestContext::new();
        assert!(ctx.headers().is_none());
    }

    #[test]
    fn test_headers_returns_headers_when_set() {
        let mut headers_map = HashMap::new();
        headers_map.insert("user-agent".to_string(), "Test-Agent/1.0".to_string());
        headers_map.insert("content-type".to_string(), "application/json".to_string());

        let headers_json = serde_json::to_value(&headers_map).unwrap();
        let ctx = RequestContext::new()
            .with_metadata("http_headers", headers_json);

        let headers = ctx.headers();
        assert!(headers.is_some());

        let headers = headers.unwrap();
        assert_eq!(headers.len(), 2);
        assert_eq!(headers.get("user-agent"), Some(&"Test-Agent/1.0".to_string()));
        assert_eq!(headers.get("content-type"), Some(&"application/json".to_string()));
    }

    #[test]
    fn test_header_case_insensitive_lookup() {
        let mut headers_map = HashMap::new();
        headers_map.insert("User-Agent".to_string(), "Test-Agent/1.0".to_string());
        headers_map.insert("Content-Type".to_string(), "application/json".to_string());

        let headers_json = serde_json::to_value(&headers_map).unwrap();
        let ctx = RequestContext::new()
            .with_metadata("http_headers", headers_json);

        // Test case-insensitive lookup
        assert_eq!(ctx.header("user-agent"), Some("Test-Agent/1.0".to_string()));
        assert_eq!(ctx.header("USER-AGENT"), Some("Test-Agent/1.0".to_string()));
        assert_eq!(ctx.header("User-Agent"), Some("Test-Agent/1.0".to_string()));
        assert_eq!(ctx.header("content-type"), Some("application/json".to_string()));
        assert_eq!(ctx.header("CONTENT-TYPE"), Some("application/json".to_string()));
    }

    #[test]
    fn test_header_returns_none_when_not_found() {
        let mut headers_map = HashMap::new();
        headers_map.insert("user-agent".to_string(), "Test-Agent/1.0".to_string());

        let headers_json = serde_json::to_value(&headers_map).unwrap();
        let ctx = RequestContext::new()
            .with_metadata("http_headers", headers_json);

        assert_eq!(ctx.header("x-custom-header"), None);
    }

    #[test]
    fn test_header_returns_none_when_headers_not_set() {
        let ctx = RequestContext::new();
        assert_eq!(ctx.header("user-agent"), None);
    }

    #[test]
    fn test_transport_returns_none_when_not_set() {
        let ctx = RequestContext::new();
        assert!(ctx.transport().is_none());
    }

    #[test]
    fn test_transport_returns_transport_type() {
        let ctx = RequestContext::new()
            .with_metadata("transport", "http");

        assert_eq!(ctx.transport(), Some("http".to_string()));
    }

    #[test]
    fn test_multiple_transport_types() {
        let http_ctx = RequestContext::new()
            .with_metadata("transport", "http");
        assert_eq!(http_ctx.transport(), Some("http".to_string()));

        let ws_ctx = RequestContext::new()
            .with_metadata("transport", "websocket");
        assert_eq!(ws_ctx.transport(), Some("websocket".to_string()));

        let stdio_ctx = RequestContext::new()
            .with_metadata("transport", "stdio");
        assert_eq!(stdio_ctx.transport(), Some("stdio".to_string()));
    }
}
