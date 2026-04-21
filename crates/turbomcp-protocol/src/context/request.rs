//! Request / response context types for the protocol layer.
//!
//! `RequestContext` is now a re-export of `turbomcp_core::RequestContext` —
//! v3.2 unified the previously-triplicated context types into a single
//! canonical one. The protocol-specific client-id and analytics helpers live
//! on extension traits in this module so callers keep their existing import
//! paths.
//!
//! Response-side analytics types (`ResponseContext`, `ResponseStatus`,
//! `RequestInfo`) remain protocol-owned.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::types::Timestamp;

pub use turbomcp_core::context::{RequestContext, TransportType};

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

/// Extension trait providing structured client-id handling.
///
/// The MCP transports capture a raw `client_id` string, but protocol-aware
/// code often wants the richer [`super::client::ClientId`] enum (which tracks
/// how the identity was proven — bearer token, session cookie, anonymous, etc.).
/// This trait bridges the two.
pub trait RequestContextExt {
    /// Set `client_id` from a structured [`super::client::ClientId`] and record
    /// the authentication method + authenticated flag in metadata.
    #[must_use]
    fn with_enhanced_client_id(self, client_id: super::client::ClientId) -> Self;

    /// Extract a client ID from headers/query params and apply it via
    /// [`Self::with_enhanced_client_id`].
    #[must_use]
    fn extract_client_id(
        self,
        extractor: &super::client::ClientIdExtractor,
        headers: Option<&HashMap<String, String>>,
        query_params: Option<&HashMap<String, String>>,
    ) -> Self;

    /// Rehydrate the structured [`super::client::ClientId`] from the context.
    fn get_enhanced_client_id(&self) -> Option<super::client::ClientId>;
}

impl RequestContextExt for RequestContext {
    fn with_enhanced_client_id(self, client_id: super::client::ClientId) -> Self {
        self.with_client_id(client_id.as_str())
            .with_metadata(
                "client_id_method",
                serde_json::Value::String(client_id.auth_method().to_string()),
            )
            .with_metadata(
                "client_authenticated",
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
                _ => super::client::ClientId::Header(id.clone()),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_context_builders() {
        let success = ResponseContext::success("req-1", std::time::Duration::from_millis(10));
        assert_eq!(success.request_id, "req-1");
        assert_eq!(success.status, ResponseStatus::Success);

        let err =
            ResponseContext::error("req-2", std::time::Duration::from_millis(5), -32000, "boom");
        assert!(matches!(err.status, ResponseStatus::Error { .. }));
    }

    #[test]
    fn request_info_lifecycle() {
        let info = RequestInfo::new(
            "client-1".into(),
            "tools/list".into(),
            serde_json::json!({}),
        )
        .complete_success(42)
        .with_status_code(200)
        .with_metadata("foo".into(), serde_json::json!("bar"));
        assert!(info.success);
        assert_eq!(info.response_time_ms, Some(42));
        assert_eq!(info.metadata.get("foo"), Some(&serde_json::json!("bar")));
    }
}
