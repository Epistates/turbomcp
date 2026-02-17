//! Core types for Streamable HTTP transport.

#[cfg(not(feature = "std"))]
use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use core::fmt;
use core::str::FromStr;
#[cfg(feature = "std")]
use std::{
    string::{String, ToString},
    vec::Vec,
};

// Use hashbrown for no_std compatibility
#[cfg(not(feature = "std"))]
use hashbrown::HashMap;
#[cfg(feature = "std")]
use std::collections::HashMap;

/// HTTP methods supported by Streamable HTTP transport.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum HttpMethod {
    /// GET - Establish SSE stream for server-initiated messages
    Get,
    /// POST - Send JSON-RPC request, receive JSON or SSE response (default)
    #[default]
    Post,
    /// DELETE - Terminate session
    Delete,
    /// OPTIONS - CORS preflight
    Options,
}

/// Error type for parsing HTTP methods.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseHttpMethodError(pub String);

impl fmt::Display for ParseHttpMethodError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown HTTP method: {}", self.0)
    }
}

impl FromStr for HttpMethod {
    type Err = ParseHttpMethodError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "GET" => Ok(Self::Get),
            "POST" => Ok(Self::Post),
            "DELETE" => Ok(Self::Delete),
            "OPTIONS" => Ok(Self::Options),
            _ => Err(ParseHttpMethodError(s.to_string())),
        }
    }
}

impl HttpMethod {
    /// Parse from a string (case-insensitive).
    ///
    /// Returns `None` for unknown methods.
    pub fn parse(s: &str) -> Option<Self> {
        s.parse().ok()
    }

    /// Get the method as a string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Delete => "DELETE",
            Self::Options => "OPTIONS",
        }
    }
}

impl fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Origin validation result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OriginValidation {
    /// Origin is valid and allowed
    Valid,
    /// Origin is missing (may be allowed depending on config)
    Missing,
    /// Origin is invalid or not allowed
    Invalid(String),
}

impl OriginValidation {
    /// Check if the origin is valid.
    pub fn is_valid(&self) -> bool {
        matches!(self, Self::Valid)
    }

    /// Check if origin validation passed (valid or missing when not required).
    pub fn passed(&self, require_origin: bool) -> bool {
        match self {
            Self::Valid => true,
            Self::Missing => !require_origin,
            Self::Invalid(_) => false,
        }
    }

    /// Validate an origin against allowed origins.
    pub fn validate(origin: Option<&str>, allowed: &[String]) -> Self {
        match origin {
            None => Self::Missing,
            Some(_) if allowed.is_empty() => Self::Valid, // No restrictions
            Some(o) if allowed.iter().any(|a| a == o) => Self::Valid,
            Some(o) => Self::Invalid(o.to_string()),
        }
    }
}

/// Error type for Streamable HTTP operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StreamableError {
    /// Session not found
    SessionNotFound(String),
    /// Session expired
    SessionExpired(String),
    /// Session terminated
    SessionTerminated(String),
    /// Invalid request method
    InvalidMethod(String),
    /// Invalid origin
    InvalidOrigin(String),
    /// Missing origin (when required)
    MissingOrigin,
    /// Invalid JSON-RPC request
    InvalidRequest(String),
    /// Request body too large
    BodyTooLarge { size: usize, max: usize },
    /// Too many concurrent streams
    TooManyStreams { count: usize, max: usize },
    /// Storage error
    StorageError(String),
    /// Internal error
    InternalError(String),
}

impl fmt::Display for StreamableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SessionNotFound(id) => write!(f, "Session not found: {id}"),
            Self::SessionExpired(id) => write!(f, "Session expired: {id}"),
            Self::SessionTerminated(id) => write!(f, "Session terminated: {id}"),
            Self::InvalidMethod(m) => write!(f, "Invalid method: {m}"),
            Self::InvalidOrigin(o) => write!(f, "Invalid origin: {o}"),
            Self::MissingOrigin => write!(f, "Origin header required"),
            Self::InvalidRequest(msg) => write!(f, "Invalid request: {msg}"),
            Self::BodyTooLarge { size, max } => {
                write!(f, "Request body too large: {size} bytes (max: {max})")
            }
            Self::TooManyStreams { count, max } => {
                write!(f, "Too many concurrent streams: {count} (max: {max})")
            }
            Self::StorageError(msg) => write!(f, "Storage error: {msg}"),
            Self::InternalError(msg) => write!(f, "Internal error: {msg}"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for StreamableError {}

/// A parsed Streamable HTTP request.
#[derive(Clone, Debug, Default)]
pub struct StreamableRequest {
    /// HTTP method
    pub method: HttpMethod,
    /// Session ID from header (if present)
    pub session_id: Option<String>,
    /// Last Event ID for replay (if present)
    pub last_event_id: Option<String>,
    /// Origin header for validation
    pub origin: Option<String>,
    /// Accept header (to determine SSE vs JSON response)
    pub accept: Option<String>,
    /// Request body (for POST)
    pub body: Option<String>,
    /// Additional headers for context extraction (authorization, x-request-id, etc.)
    pub headers: HashMap<String, String>,
}

impl StreamableRequest {
    /// Create a new GET request (SSE stream).
    pub fn get(session_id: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Get,
            session_id: Some(session_id.into()),
            last_event_id: None,
            origin: None,
            accept: Some("text/event-stream".to_string()),
            body: None,
            headers: HashMap::new(),
        }
    }

    /// Create a new POST request (JSON-RPC).
    pub fn post(body: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Post,
            session_id: None,
            last_event_id: None,
            origin: None,
            accept: None,
            body: Some(body.into()),
            headers: HashMap::new(),
        }
    }

    /// Create a new DELETE request (terminate session).
    pub fn delete(session_id: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Delete,
            session_id: Some(session_id.into()),
            last_event_id: None,
            origin: None,
            accept: None,
            body: None,
            headers: HashMap::new(),
        }
    }

    /// Set the session ID.
    pub fn with_session_id(mut self, id: impl Into<String>) -> Self {
        self.session_id = Some(id.into());
        self
    }

    /// Set the last event ID for replay.
    pub fn with_last_event_id(mut self, id: impl Into<String>) -> Self {
        self.last_event_id = Some(id.into());
        self
    }

    /// Set the origin header.
    pub fn with_origin(mut self, origin: impl Into<String>) -> Self {
        self.origin = Some(origin.into());
        self
    }

    /// Set the accept header.
    pub fn with_accept(mut self, accept: impl Into<String>) -> Self {
        self.accept = Some(accept.into());
        self
    }

    /// Check if client accepts SSE.
    pub fn accepts_sse(&self) -> bool {
        self.accept
            .as_ref()
            .is_some_and(|a| a.contains("text/event-stream"))
    }

    /// Check if this is a session-bound request.
    pub fn has_session(&self) -> bool {
        self.session_id.is_some()
    }

    /// Check if this is a replay request.
    pub fn is_replay(&self) -> bool {
        self.last_event_id.is_some()
    }

    /// Set headers for context extraction.
    pub fn with_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers = headers;
        self
    }

    /// Add a single header.
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }
}

/// Response type for Streamable HTTP.
#[derive(Clone, Debug)]
pub enum StreamableResponse {
    /// JSON response (for immediate replies)
    Json {
        /// HTTP status code
        status: u16,
        /// Session ID to include in header
        session_id: Option<String>,
        /// JSON body
        body: String,
    },
    /// SSE stream response
    Sse {
        /// Session ID to include in header
        session_id: Option<String>,
        /// Initial events to send (for replay)
        initial_events: Vec<String>,
    },
    /// Empty response (for DELETE)
    Empty {
        /// HTTP status code
        status: u16,
    },
    /// Error response
    Error {
        /// HTTP status code
        status: u16,
        /// Error message
        message: String,
    },
}

impl StreamableResponse {
    /// Create a JSON response.
    pub fn json(body: impl Into<String>) -> Self {
        Self::Json {
            status: 200,
            session_id: None,
            body: body.into(),
        }
    }

    /// Create a JSON response with session ID.
    pub fn json_with_session(body: impl Into<String>, session_id: impl Into<String>) -> Self {
        Self::Json {
            status: 200,
            session_id: Some(session_id.into()),
            body: body.into(),
        }
    }

    /// Create an SSE stream response.
    pub fn sse(session_id: impl Into<String>) -> Self {
        Self::Sse {
            session_id: Some(session_id.into()),
            initial_events: Vec::new(),
        }
    }

    /// Create an SSE stream response with replay events.
    pub fn sse_with_replay(session_id: impl Into<String>, events: Vec<String>) -> Self {
        Self::Sse {
            session_id: Some(session_id.into()),
            initial_events: events,
        }
    }

    /// Create an empty response.
    pub fn empty() -> Self {
        Self::Empty { status: 204 }
    }

    /// Create an error response.
    pub fn error(status: u16, message: impl Into<String>) -> Self {
        Self::Error {
            status,
            message: message.into(),
        }
    }

    /// Create a 400 Bad Request response.
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::error(400, message)
    }

    /// Create a 401 Unauthorized response.
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::error(401, message)
    }

    /// Create a 403 Forbidden response.
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::error(403, message)
    }

    /// Create a 404 Not Found response.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::error(404, message)
    }

    /// Create a 413 Payload Too Large response.
    pub fn payload_too_large(message: impl Into<String>) -> Self {
        Self::error(413, message)
    }

    /// Create a 429 Too Many Requests response.
    pub fn too_many_requests(message: impl Into<String>) -> Self {
        Self::error(429, message)
    }

    /// Create a 500 Internal Server Error response.
    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::error(500, message)
    }

    /// Get the HTTP status code.
    pub fn status(&self) -> u16 {
        match self {
            Self::Json { status, .. } => *status,
            Self::Sse { .. } => 200,
            Self::Empty { status } => *status,
            Self::Error { status, .. } => *status,
        }
    }

    /// Check if this is a success response.
    pub fn is_success(&self) -> bool {
        self.status() < 400
    }
}

/// Convert a `StreamableError` to a `StreamableResponse`.
impl From<StreamableError> for StreamableResponse {
    fn from(err: StreamableError) -> Self {
        match err {
            StreamableError::SessionNotFound(_) => Self::not_found(err.to_string()),
            StreamableError::SessionExpired(_) => Self::error(410, err.to_string()), // Gone
            StreamableError::SessionTerminated(_) => Self::error(410, err.to_string()),
            StreamableError::InvalidMethod(_) => Self::error(405, err.to_string()),
            StreamableError::InvalidOrigin(_) | StreamableError::MissingOrigin => {
                Self::forbidden(err.to_string())
            }
            StreamableError::InvalidRequest(_) => Self::bad_request(err.to_string()),
            StreamableError::BodyTooLarge { .. } => Self::payload_too_large(err.to_string()),
            StreamableError::TooManyStreams { .. } => Self::too_many_requests(err.to_string()),
            StreamableError::StorageError(_) | StreamableError::InternalError(_) => {
                Self::internal_error(err.to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_method_parse() {
        assert_eq!(HttpMethod::parse("GET"), Some(HttpMethod::Get));
        assert_eq!(HttpMethod::parse("post"), Some(HttpMethod::Post));
        assert_eq!(HttpMethod::parse("DELETE"), Some(HttpMethod::Delete));
        assert_eq!(HttpMethod::parse("PATCH"), None);

        // Also test FromStr trait
        assert_eq!("GET".parse::<HttpMethod>().ok(), Some(HttpMethod::Get));
        assert!("UNKNOWN".parse::<HttpMethod>().is_err());
    }

    #[test]
    fn test_origin_validation() {
        let allowed = vec!["https://example.com".to_string()];

        assert!(OriginValidation::validate(Some("https://example.com"), &allowed).is_valid());
        assert!(!OriginValidation::validate(Some("https://evil.com"), &allowed).is_valid());
        assert_eq!(
            OriginValidation::validate(None, &allowed),
            OriginValidation::Missing
        );

        // Empty allowed list = all allowed
        assert!(OriginValidation::validate(Some("https://any.com"), &[]).is_valid());
    }

    #[test]
    fn test_origin_validation_passed() {
        assert!(OriginValidation::Valid.passed(true));
        assert!(OriginValidation::Valid.passed(false));
        assert!(!OriginValidation::Missing.passed(true));
        assert!(OriginValidation::Missing.passed(false));
        assert!(!OriginValidation::Invalid("x".into()).passed(false));
    }

    #[test]
    fn test_streamable_request_builders() {
        let get = StreamableRequest::get("session-123")
            .with_last_event_id("evt-5")
            .with_origin("https://example.com");

        assert_eq!(get.method, HttpMethod::Get);
        assert_eq!(get.session_id, Some("session-123".to_string()));
        assert!(get.accepts_sse());
        assert!(get.is_replay());

        let post = StreamableRequest::post(r#"{"jsonrpc": "2.0"}"#);
        assert_eq!(post.method, HttpMethod::Post);
        assert!(!post.has_session());
    }

    #[test]
    fn test_streamable_response_status() {
        assert_eq!(StreamableResponse::json("{}").status(), 200);
        assert_eq!(StreamableResponse::sse("sess").status(), 200);
        assert_eq!(StreamableResponse::empty().status(), 204);
        assert_eq!(StreamableResponse::bad_request("x").status(), 400);
        assert_eq!(StreamableResponse::not_found("x").status(), 404);
    }

    #[test]
    fn test_error_to_response() {
        let err = StreamableError::SessionNotFound("abc".into());
        let resp: StreamableResponse = err.into();
        assert_eq!(resp.status(), 404);

        let err = StreamableError::BodyTooLarge {
            size: 2000,
            max: 1000,
        };
        let resp: StreamableResponse = err.into();
        assert_eq!(resp.status(), 413);
    }
}
