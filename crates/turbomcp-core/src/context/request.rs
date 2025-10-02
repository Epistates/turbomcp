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

use super::capabilities::ServerCapabilities;
use crate::types::Timestamp;

/// Context information for request processing
#[derive(Clone)]
pub struct RequestContext {
    /// Unique request identifier
    pub request_id: String,

    /// User identifier (if authenticated)
    pub user_id: Option<String>,

    /// Session identifier
    pub session_id: Option<String>,

    /// Client identifier
    pub client_id: Option<String>,

    /// Request timestamp
    pub timestamp: Timestamp,

    /// Request start time for performance tracking
    pub start_time: Instant,

    /// Custom metadata
    pub metadata: Arc<HashMap<String, serde_json::Value>>,

    /// Tracing span context
    #[cfg(feature = "tracing")]
    pub span: Option<tracing::Span>,

    /// Cancellation token
    pub cancellation_token: Option<Arc<CancellationToken>>,

    /// Server capabilities for server-initiated requests
    /// This is used by turbomcp-server to provide access to sampling
    #[doc(hidden)]
    pub(crate) server_capabilities: Option<Arc<dyn ServerCapabilities>>,
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
            .field("server_capabilities", &self.server_capabilities.is_some())
            .finish()
    }
}

/// Context information for response processing
#[derive(Debug, Clone)]
pub struct ResponseContext {
    /// Original request ID
    pub request_id: String,

    /// Response timestamp
    pub timestamp: Timestamp,

    /// Processing duration
    pub duration: std::time::Duration,

    /// Response status
    pub status: ResponseStatus,

    /// Custom metadata
    pub metadata: Arc<HashMap<String, serde_json::Value>>,
}

/// Response status information
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResponseStatus {
    /// Successful response
    Success,
    /// Error response
    Error {
        /// Error code
        code: i32,
        /// Error message
        message: String,
    },
    /// Partial response (streaming)
    Partial,
    /// Cancelled response
    Cancelled,
}

/// Request analytics information for monitoring and debugging
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestInfo {
    /// Request timestamp
    pub timestamp: DateTime<Utc>,
    /// Client identifier
    pub client_id: String,
    /// Tool or method name
    pub method_name: String,
    /// Request parameters (sanitized for privacy)
    pub parameters: serde_json::Value,
    /// Response time in milliseconds
    pub response_time_ms: Option<u64>,
    /// Success status
    pub success: bool,
    /// Error message if failed
    pub error_message: Option<String>,
    /// HTTP status code (if applicable)
    pub status_code: Option<u16>,
    /// Additional metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

impl RequestContext {
    /// Create a new request context
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
            server_capabilities: None,
        }
    }

    /// Create a request context with specific ID
    pub fn with_id(id: impl Into<String>) -> Self {
        Self {
            request_id: id.into(),
            ..Self::new()
        }
    }

    /// Set the user ID
    #[must_use]
    pub fn with_user_id(mut self, user_id: impl Into<String>) -> Self {
        self.user_id = Some(user_id.into());
        self
    }

    /// Set the session ID
    #[must_use]
    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    /// Set the client ID
    #[must_use]
    pub fn with_client_id(mut self, client_id: impl Into<String>) -> Self {
        self.client_id = Some(client_id.into());
        self
    }

    /// Add metadata
    #[must_use]
    pub fn with_metadata(
        mut self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        Arc::make_mut(&mut self.metadata).insert(key.into(), value.into());
        self
    }

    /// Get metadata value
    #[must_use]
    pub fn get_metadata(&self, key: &str) -> Option<&serde_json::Value> {
        self.metadata.get(key)
    }

    /// Get elapsed time since request started
    #[must_use]
    pub fn elapsed(&self) -> std::time::Duration {
        self.start_time.elapsed()
    }

    /// Check if request is cancelled
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancellation_token
            .as_ref()
            .is_some_and(|token| token.is_cancelled())
    }

    /// Set server capabilities for server-initiated requests
    /// This is used by turbomcp-server to inject its capabilities
    #[must_use]
    pub fn with_server_capabilities(mut self, capabilities: Arc<dyn ServerCapabilities>) -> Self {
        self.server_capabilities = Some(capabilities);
        self
    }

    /// Set cancellation token for cooperative cancellation
    /// This is used by turbomcp-server for request cancellation
    #[must_use]
    pub fn with_cancellation_token(mut self, token: Arc<CancellationToken>) -> Self {
        self.cancellation_token = Some(token);
        self
    }

    /// Get user ID from request context
    #[must_use]
    pub fn user(&self) -> Option<&str> {
        self.user_id.as_deref()
    }

    /// Check if request is authenticated
    /// This checks if the client ID represents an authenticated client
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        self.get_metadata("client_authenticated")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    /// Get user roles from request context
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

    /// Check if user has any of the required roles
    pub fn has_any_role<S: AsRef<str>>(&self, required: &[S]) -> bool {
        if required.is_empty() {
            return true; // Empty requirement always passes
        }

        let user_roles = self.roles();
        required
            .iter()
            .any(|required_role| user_roles.contains(&required_role.as_ref().to_string()))
    }

    /// Get the server capabilities if present
    #[doc(hidden)]
    pub fn server_capabilities(&self) -> Option<&Arc<dyn ServerCapabilities>> {
        self.server_capabilities.as_ref()
    }
}

impl Default for RequestContext {
    fn default() -> Self {
        Self::new()
    }
}

impl ResponseContext {
    /// Create a successful response context
    pub fn success(request_id: impl Into<String>, duration: std::time::Duration) -> Self {
        Self {
            request_id: request_id.into(),
            timestamp: Timestamp::now(),
            duration,
            status: ResponseStatus::Success,
            metadata: Arc::new(HashMap::new()),
        }
    }

    /// Create an error response context
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
    /// Create a new request info
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

    /// Mark the request as completed successfully
    #[must_use]
    pub const fn complete_success(mut self, response_time_ms: u64) -> Self {
        self.response_time_ms = Some(response_time_ms);
        self.success = true;
        self.status_code = Some(200);
        self
    }

    /// Mark the request as failed
    #[must_use]
    pub fn complete_error(mut self, response_time_ms: u64, error: String) -> Self {
        self.response_time_ms = Some(response_time_ms);
        self.success = false;
        self.error_message = Some(error);
        self.status_code = Some(500);
        self
    }

    /// Set HTTP status code
    #[must_use]
    pub const fn with_status_code(mut self, code: u16) -> Self {
        self.status_code = Some(code);
        self
    }

    /// Add metadata
    #[must_use]
    pub fn with_metadata(mut self, key: String, value: serde_json::Value) -> Self {
        self.metadata.insert(key, value);
        self
    }
}

/// Extension trait for RequestContext with enhanced client ID handling
pub trait RequestContextExt {
    /// Set client ID using `ClientId` enum
    #[must_use]
    fn with_enhanced_client_id(self, client_id: super::client::ClientId) -> Self;

    /// Extract and set client ID from headers and query params
    #[must_use]
    fn extract_client_id(
        self,
        extractor: &super::client::ClientIdExtractor,
        headers: Option<&HashMap<String, String>>,
        query_params: Option<&HashMap<String, String>>,
    ) -> Self;

    /// Get the enhanced client ID
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
