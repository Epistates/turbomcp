//! Request and response context for rich metadata handling.
//!
//! Enhanced with client identification, session management, and request analytics
//! for comprehensive MCP application monitoring and management.
//!
//! # Examples
//!
//! ## Creating a basic request context
//!
//! ```
//! use turbomcp_core::RequestContext;
//!
//! let ctx = RequestContext::new();
//! println!("Request ID: {}", ctx.request_id);
//! assert!(!ctx.request_id.is_empty());
//! ```
//!
//! ## Building a context with metadata
//!
//! ```
//! use turbomcp_core::RequestContext;
//!
//! let ctx = RequestContext::new()
//!     .with_user_id("user123")
//!     .with_session_id("session456")
//!     .with_metadata("api_version", "2.0")
//!     .with_metadata("client", "web_app");
//!
//! assert_eq!(ctx.user_id, Some("user123".to_string()));
//! assert_eq!(ctx.session_id, Some("session456".to_string()));
//! assert_eq!(ctx.get_metadata("api_version"), Some(&serde_json::json!("2.0")));
//! ```
//!
//! ## Working with response contexts
//!
//! ```
//! use turbomcp_core::{RequestContext, ResponseContext};
//! use std::time::Duration;
//!
//! let request_ctx = RequestContext::with_id("req-123");
//! let duration = Duration::from_millis(250);
//!
//! // Successful response
//! let success_ctx = ResponseContext::success(&request_ctx.request_id, duration);
//!
//! // Error response
//! let error_ctx = ResponseContext::error(&request_ctx.request_id, duration, -32600, "Invalid Request");
//!
//! assert_eq!(success_ctx.request_id, "req-123");
//! assert_eq!(error_ctx.request_id, "req-123");
//! ```

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::Instant;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::types::Timestamp;

/// Trait for server-initiated requests (sampling, elicitation, roots)
/// This provides a type-safe way for tools to make requests to clients
pub trait ServerCapabilities: Send + Sync + fmt::Debug {
    /// Send a sampling/createMessage request to the client
    fn create_message(
        &self,
        request: serde_json::Value,
    ) -> futures::future::BoxFuture<
        '_,
        Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>>,
    >;

    /// Send an elicitation request to the client
    fn elicit(
        &self,
        request: serde_json::Value,
    ) -> futures::future::BoxFuture<
        '_,
        Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>>,
    >;

    /// List client's root capabilities
    fn list_roots(
        &self,
    ) -> futures::future::BoxFuture<
        '_,
        Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>>,
    >;
}

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

// ============================================================================
// Enhanced Context Types for New MCP Features
// ============================================================================

/// Context for server-initiated elicitation (user input) requests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitationContext {
    /// Unique elicitation request ID
    pub elicitation_id: String,
    /// Message presented to user
    pub message: String,
    /// Schema for user input validation (using protocol ElicitationSchema when available)
    pub schema: serde_json::Value,
    /// Human-readable prompt or question (deprecated, use message)
    #[deprecated(note = "Use message field instead")]
    pub prompt: Option<String>,
    /// Input constraints and hints
    pub constraints: Option<serde_json::Value>,
    /// Default values for fields
    pub defaults: Option<HashMap<String, serde_json::Value>>,
    /// Whether input is required or optional
    pub required: bool,
    /// Timeout for user response in milliseconds
    pub timeout_ms: Option<u64>,
    /// Cancellation support
    pub cancellable: bool,
    /// Client session information
    pub client_session: Option<ClientSession>,
    /// Timestamp of elicitation request
    pub requested_at: Timestamp,
    /// Current elicitation state
    pub state: ElicitationState,
    /// Custom elicitation metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

/// State of an elicitation request
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ElicitationState {
    /// Waiting for user response
    Pending,
    /// User provided input
    Accepted,
    /// User explicitly declined
    Declined,
    /// User cancelled/dismissed
    Cancelled,
    /// Response timeout exceeded
    TimedOut,
}

impl ElicitationContext {
    /// Create a new elicitation context
    pub fn new(message: String, schema: serde_json::Value) -> Self {
        Self {
            elicitation_id: Uuid::new_v4().to_string(),
            message,
            schema,
            #[allow(deprecated)]
            prompt: None,
            constraints: None,
            defaults: None,
            required: true,
            timeout_ms: Some(30000),
            cancellable: true,
            client_session: None,
            requested_at: Timestamp::now(),
            state: ElicitationState::Pending,
            metadata: HashMap::new(),
        }
    }

    /// Set the client session
    pub fn with_client_session(mut self, session: ClientSession) -> Self {
        self.client_session = Some(session);
        self
    }

    /// Set the timeout
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }

    /// Update the state
    pub fn set_state(&mut self, state: ElicitationState) {
        self.state = state;
    }

    /// Check if elicitation is complete
    pub fn is_complete(&self) -> bool {
        !matches!(self.state, ElicitationState::Pending)
    }
}

/// Context for completion/autocompletion requests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionContext {
    /// Unique completion request ID
    pub completion_id: String,
    /// Reference being completed (prompt, resource template, etc.)
    pub completion_ref: CompletionReference,
    /// Current argument being completed
    pub argument_name: Option<String>,
    /// Partial value being completed
    pub partial_value: Option<String>,
    /// Previously resolved arguments
    pub resolved_arguments: HashMap<String, String>,
    /// Available completion options
    pub completions: Vec<CompletionOption>,
    /// Cursor position for completion
    pub cursor_position: Option<usize>,
    /// Maximum number of completions to return
    pub max_completions: Option<usize>,
    /// Whether more completions are available
    pub has_more: bool,
    /// Total number of available completions
    pub total_completions: Option<usize>,
    /// Client capabilities for completion
    pub client_capabilities: Option<CompletionCapabilities>,
    /// Completion metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Client capabilities for completion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionCapabilities {
    /// Supports paginated completions
    pub supports_pagination: bool,
    /// Supports fuzzy matching
    pub supports_fuzzy: bool,
    /// Maximum batch size
    pub max_batch_size: usize,
    /// Supports rich completion items with descriptions
    pub supports_descriptions: bool,
}

impl CompletionContext {
    /// Create a new completion context
    pub fn new(completion_ref: CompletionReference) -> Self {
        Self {
            completion_id: Uuid::new_v4().to_string(),
            completion_ref,
            argument_name: None,
            partial_value: None,
            resolved_arguments: HashMap::new(),
            completions: Vec::new(),
            cursor_position: None,
            max_completions: Some(100),
            has_more: false,
            total_completions: None,
            client_capabilities: None,
            metadata: HashMap::new(),
        }
    }

    /// Add a completion option
    pub fn add_completion(&mut self, option: CompletionOption) {
        self.completions.push(option);
    }

    /// Set resolved arguments
    pub fn with_resolved_arguments(mut self, args: HashMap<String, String>) -> Self {
        self.resolved_arguments = args;
        self
    }
}

/// Reference type for completion context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CompletionReference {
    /// Completing a prompt argument
    Prompt {
        /// Prompt name
        name: String,
        /// Argument being completed
        argument: String,
    },
    /// Completing a resource template parameter
    ResourceTemplate {
        /// Template name
        name: String,
        /// Parameter being completed
        parameter: String,
    },
    /// Completing a tool argument
    Tool {
        /// Tool name
        name: String,
        /// Argument being completed
        argument: String,
    },
    /// Custom completion context
    Custom {
        /// Custom reference type
        ref_type: String,
        /// Reference metadata
        metadata: HashMap<String, serde_json::Value>,
    },
}

/// Completion option with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionOption {
    /// Completion value
    pub value: String,
    /// Human-readable label
    pub label: Option<String>,
    /// Completion type (value, keyword, function, etc.)
    pub completion_type: Option<String>,
    /// Additional documentation
    pub documentation: Option<String>,
    /// Sort priority (lower = higher priority)
    pub sort_priority: Option<i32>,
    /// Whether this option requires additional input
    pub insert_text: Option<String>,
}

/// Context for resource template operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceTemplateContext {
    /// Template name
    pub template_name: String,
    /// URI template pattern (RFC 6570)
    pub uri_template: String,
    /// Available template parameters
    pub parameters: HashMap<String, TemplateParameter>,
    /// Template description
    pub description: Option<String>,
    /// Template category/preset type
    pub preset_type: Option<String>,
    /// Template metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Template parameter definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateParameter {
    /// Parameter name
    pub name: String,
    /// Parameter type
    pub param_type: String,
    /// Whether parameter is required
    pub required: bool,
    /// Default value
    pub default: Option<serde_json::Value>,
    /// Parameter description
    pub description: Option<String>,
    /// Validation pattern (regex)
    pub pattern: Option<String>,
    /// Enum values (if applicable)
    pub enum_values: Option<Vec<serde_json::Value>>,
}

/// Context for ping/health monitoring requests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingContext {
    /// Ping origin (client or server)
    pub origin: PingOrigin,
    /// Expected response time threshold in milliseconds
    pub response_threshold_ms: Option<u64>,
    /// Custom ping payload
    pub payload: Option<serde_json::Value>,
    /// Health check metadata
    pub health_metadata: HashMap<String, serde_json::Value>,
    /// Connection quality metrics
    pub connection_metrics: Option<ConnectionMetrics>,
}

/// Ping origin identifier
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PingOrigin {
    /// Ping initiated by client
    Client,
    /// Ping initiated by server
    Server,
}

/// Connection quality metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionMetrics {
    /// Round-trip time in milliseconds
    pub rtt_ms: Option<f64>,
    /// Packet loss percentage (0.0-100.0)
    pub packet_loss: Option<f64>,
    /// Connection uptime in seconds
    pub uptime_seconds: Option<u64>,
    /// Bytes sent
    pub bytes_sent: Option<u64>,
    /// Bytes received  
    pub bytes_received: Option<u64>,
    /// Last successful communication timestamp
    pub last_success: Option<DateTime<Utc>>,
}

/// Enhanced context for bidirectional MCP communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BidirectionalContext {
    /// Communication direction
    pub direction: CommunicationDirection,
    /// Initiator of the request
    pub initiator: CommunicationInitiator,
    /// Whether response is expected
    pub expects_response: bool,
    /// Parent request ID (for server-initiated requests in response to client requests)
    pub parent_request_id: Option<String>,
    /// Request type for validation
    pub request_type: Option<String>,
    /// Server ID for server-initiated requests
    pub server_id: Option<String>,
    /// Correlation ID for request tracking
    pub correlation_id: String,
    /// Bidirectional communication metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

impl BidirectionalContext {
    /// Create a new bidirectional context
    pub fn new(direction: CommunicationDirection, initiator: CommunicationInitiator) -> Self {
        Self {
            direction,
            initiator,
            expects_response: true,
            parent_request_id: None,
            request_type: None,
            server_id: None,
            correlation_id: Uuid::new_v4().to_string(),
            metadata: HashMap::new(),
        }
    }

    /// Track request direction for proper routing
    pub fn with_direction(mut self, direction: CommunicationDirection) -> Self {
        self.direction = direction;
        self
    }

    /// Set the request type
    pub fn with_request_type(mut self, request_type: String) -> Self {
        self.request_type = Some(request_type);
        self
    }

    /// Validate request direction against protocol rules
    pub fn validate_direction(&self) -> Result<(), String> {
        if let Some(ref request_type) = self.request_type {
            match (request_type.as_str(), &self.direction) {
                // Server-initiated requests
                ("sampling/createMessage", CommunicationDirection::ServerToClient) => Ok(()),
                ("roots/list", CommunicationDirection::ServerToClient) => Ok(()),
                ("elicitation/create", CommunicationDirection::ServerToClient) => Ok(()),

                // Client-initiated requests
                ("completion/complete", CommunicationDirection::ClientToServer) => Ok(()),
                ("tools/call", CommunicationDirection::ClientToServer) => Ok(()),
                ("resources/read", CommunicationDirection::ClientToServer) => Ok(()),
                ("prompts/get", CommunicationDirection::ClientToServer) => Ok(()),

                // Bidirectional
                ("ping", _) => Ok(()), // Can go either direction

                // Invalid direction for request type
                (req, dir) => Err(format!(
                    "Invalid direction {:?} for request type '{}'",
                    dir, req
                )),
            }
        } else {
            Ok(()) // No request type to validate
        }
    }
}

/// Context for server-initiated requests (sampling, roots listing)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInitiatedContext {
    /// Type of server-initiated request
    pub request_type: ServerInitiatedType,
    /// Originating server ID
    pub server_id: String,
    /// Request correlation ID
    pub correlation_id: String,
    /// Client capabilities
    pub client_capabilities: Option<ClientCapabilities>,
    /// Request timestamp
    pub initiated_at: Timestamp,
    /// Request metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Type of server-initiated request
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ServerInitiatedType {
    /// Server-initiated sampling
    CreateMessage,
    /// Server-initiated roots listing
    ListRoots,
    /// Server-initiated user input
    Elicitation,
    /// Server-initiated health check
    Ping,
}

/// Client capabilities for server-initiated requests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientCapabilities {
    /// Supports sampling/message creation
    pub sampling: bool,
    /// Supports roots listing
    pub roots: bool,
    /// Supports elicitation
    pub elicitation: bool,
    /// Maximum concurrent server requests
    pub max_concurrent_requests: usize,
    /// Supported experimental features
    pub experimental: HashMap<String, bool>,
}

impl ServerInitiatedContext {
    /// Create a new server-initiated context
    pub fn new(request_type: ServerInitiatedType, server_id: String) -> Self {
        Self {
            request_type,
            server_id,
            correlation_id: Uuid::new_v4().to_string(),
            client_capabilities: None,
            initiated_at: Timestamp::now(),
            metadata: HashMap::new(),
        }
    }

    /// Set client capabilities
    pub fn with_capabilities(mut self, capabilities: ClientCapabilities) -> Self {
        self.client_capabilities = Some(capabilities);
        self
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: String, value: serde_json::Value) -> Self {
        self.metadata.insert(key, value);
        self
    }
}

/// Communication direction in MCP protocol
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommunicationDirection {
    /// Client to server (traditional)
    ClientToServer,
    /// Server to client (new MCP features)
    ServerToClient,
}

/// Communication initiator
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommunicationInitiator {
    /// Client initiated the communication
    Client,
    /// Server initiated the communication
    Server,
}

impl RequestContext {
    /// Create a new request context
    ///
    /// # Examples
    ///
    /// ```
    /// use turbomcp_core::RequestContext;
    ///
    /// let ctx = RequestContext::new();
    /// assert!(!ctx.request_id.is_empty());
    /// assert!(ctx.user_id.is_none());
    /// assert!(ctx.session_id.is_none());
    /// assert!(ctx.metadata.is_empty());
    /// ```
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
    /// Return true if the request is authenticated according to context metadata
    ///
    /// # Examples
    ///
    /// ```
    /// use turbomcp_core::RequestContext;
    ///
    /// let ctx = RequestContext::new()
    ///     .with_metadata("authenticated", true);
    /// assert!(ctx.is_authenticated());
    ///
    /// let unauth_ctx = RequestContext::new();
    /// assert!(!unauth_ctx.is_authenticated());
    /// ```
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        self.metadata
            .get("authenticated")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
    }

    /// Return user id if present
    #[must_use]
    pub fn user(&self) -> Option<&str> {
        self.user_id.as_deref()
    }

    /// Return roles from `auth.roles` metadata, if present
    #[must_use]
    pub fn roles(&self) -> Vec<String> {
        self.metadata
            .get("auth")
            .and_then(|v| v.get("roles"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(std::string::ToString::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Return true if the user has any of the required roles
    pub fn has_any_role<S: AsRef<str>>(&self, required: &[S]) -> bool {
        if required.is_empty() {
            return true;
        }
        let user_roles = self.roles();
        if user_roles.is_empty() {
            return false;
        }
        let set: std::collections::HashSet<_> = user_roles.into_iter().collect();
        required.iter().any(|r| set.contains(r.as_ref()))
    }

    /// Create a request context with specific ID
    pub fn with_id(id: impl Into<String>) -> Self {
        Self {
            request_id: id.into(),
            ..Self::new()
        }
    }

    /// Set the server capabilities for server-initiated requests
    /// This is used internally by turbomcp-server
    #[doc(hidden)]
    pub fn with_server_capabilities(mut self, capabilities: Arc<dyn ServerCapabilities>) -> Self {
        self.server_capabilities = Some(capabilities);
        self
    }

    /// Get the server capabilities if present
    #[doc(hidden)]
    pub fn server_capabilities(&self) -> Option<&Arc<dyn ServerCapabilities>> {
        self.server_capabilities.as_ref()
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

    /// Set cancellation token
    #[must_use]
    pub fn with_cancellation_token(mut self, token: Arc<CancellationToken>) -> Self {
        self.cancellation_token = Some(token);
        self
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

    /// Get metadata value
    #[must_use]
    pub fn get_metadata(&self, key: &str) -> Option<&serde_json::Value> {
        self.metadata.get(key)
    }

    /// Clone with new request ID (for sub-requests)
    #[must_use]
    pub fn derive(&self) -> Self {
        Self {
            request_id: Uuid::new_v4().to_string(),
            user_id: self.user_id.clone(),
            session_id: self.session_id.clone(),
            client_id: self.client_id.clone(),
            timestamp: Timestamp::now(),
            start_time: Instant::now(),
            metadata: self.metadata.clone(),
            #[cfg(feature = "tracing")]
            span: None,
            cancellation_token: self.cancellation_token.clone(),
            server_capabilities: self.server_capabilities.clone(),
        }
    }

    // ========================================================================
    // Enhanced Context Methods for New MCP Features
    // ========================================================================

    /// Add elicitation context for server-initiated user input
    #[must_use]
    pub fn with_elicitation_context(mut self, context: ElicitationContext) -> Self {
        Arc::make_mut(&mut self.metadata).insert(
            "elicitation_context".to_string(),
            serde_json::to_value(context).unwrap_or_default(),
        );
        self
    }

    /// Get elicitation context if present
    pub fn elicitation_context(&self) -> Option<ElicitationContext> {
        self.metadata
            .get("elicitation_context")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    /// Add completion context for autocompletion requests
    #[must_use]
    pub fn with_completion_context(mut self, context: CompletionContext) -> Self {
        Arc::make_mut(&mut self.metadata).insert(
            "completion_context".to_string(),
            serde_json::to_value(context).unwrap_or_default(),
        );
        self
    }

    /// Get completion context if present
    pub fn completion_context(&self) -> Option<CompletionContext> {
        self.metadata
            .get("completion_context")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    /// Add resource template context for parameterized resources
    #[must_use]
    pub fn with_resource_template_context(mut self, context: ResourceTemplateContext) -> Self {
        Arc::make_mut(&mut self.metadata).insert(
            "resource_template_context".to_string(),
            serde_json::to_value(context).unwrap_or_default(),
        );
        self
    }

    /// Get resource template context if present
    pub fn resource_template_context(&self) -> Option<ResourceTemplateContext> {
        self.metadata
            .get("resource_template_context")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    /// Add ping context for health monitoring
    #[must_use]
    pub fn with_ping_context(mut self, context: PingContext) -> Self {
        Arc::make_mut(&mut self.metadata).insert(
            "ping_context".to_string(),
            serde_json::to_value(context).unwrap_or_default(),
        );
        self
    }

    /// Get ping context if present
    pub fn ping_context(&self) -> Option<PingContext> {
        self.metadata
            .get("ping_context")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    /// Add bidirectional communication context
    #[must_use]
    pub fn with_bidirectional_context(mut self, context: BidirectionalContext) -> Self {
        Arc::make_mut(&mut self.metadata).insert(
            "bidirectional_context".to_string(),
            serde_json::to_value(context).unwrap_or_default(),
        );
        self
    }

    /// Get bidirectional context if present
    pub fn bidirectional_context(&self) -> Option<BidirectionalContext> {
        self.metadata
            .get("bidirectional_context")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    /// Check if this is a server-initiated request
    pub fn is_server_initiated(&self) -> bool {
        self.bidirectional_context()
            .map(|ctx| ctx.direction == CommunicationDirection::ServerToClient)
            .unwrap_or(false)
    }

    /// Check if this is a client-initiated request (default/traditional)
    pub fn is_client_initiated(&self) -> bool {
        !self.is_server_initiated()
    }

    /// Get communication direction
    pub fn communication_direction(&self) -> CommunicationDirection {
        self.bidirectional_context()
            .map(|ctx| ctx.direction)
            .unwrap_or(CommunicationDirection::ClientToServer)
    }

    /// Create context for server-initiated elicitation request
    pub fn for_elicitation(schema: serde_json::Value, prompt: Option<String>) -> Self {
        let message = prompt.unwrap_or_else(|| "Please provide input".to_string());
        let elicitation_ctx = ElicitationContext::new(message, schema);

        let bidirectional_ctx = BidirectionalContext::new(
            CommunicationDirection::ServerToClient,
            CommunicationInitiator::Server,
        )
        .with_request_type("elicitation/create".to_string());

        Self::new()
            .with_elicitation_context(elicitation_ctx)
            .with_bidirectional_context(bidirectional_ctx)
    }

    /// Create context for completion request
    pub fn for_completion(completion_ref: CompletionReference) -> Self {
        let completion_ctx = CompletionContext::new(completion_ref);

        Self::new().with_completion_context(completion_ctx)
    }

    /// Create context for resource template operation
    pub fn for_resource_template(template_name: String, uri_template: String) -> Self {
        let template_ctx = ResourceTemplateContext {
            template_name,
            uri_template,
            parameters: HashMap::new(),
            description: None,
            preset_type: None,
            metadata: HashMap::new(),
        };

        Self::new().with_resource_template_context(template_ctx)
    }

    /// Create context for ping request
    pub fn for_ping(origin: PingOrigin) -> Self {
        let ping_ctx = PingContext {
            origin: origin.clone(),
            response_threshold_ms: Some(5_000), // 5 second default
            payload: None,
            health_metadata: HashMap::new(),
            connection_metrics: None,
        };

        let direction = match origin {
            PingOrigin::Client => CommunicationDirection::ClientToServer,
            PingOrigin::Server => CommunicationDirection::ServerToClient,
        };
        let initiator = match origin {
            PingOrigin::Client => CommunicationInitiator::Client,
            PingOrigin::Server => CommunicationInitiator::Server,
        };

        let bidirectional_ctx =
            BidirectionalContext::new(direction, initiator).with_request_type("ping".to_string());

        Self::new()
            .with_ping_context(ping_ctx)
            .with_bidirectional_context(bidirectional_ctx)
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

    /// Create a cancelled response context
    pub fn cancelled(request_id: impl Into<String>, duration: std::time::Duration) -> Self {
        Self {
            request_id: request_id.into(),
            timestamp: Timestamp::now(),
            duration,
            status: ResponseStatus::Cancelled,
            metadata: Arc::new(HashMap::new()),
        }
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

    /// Check if response is successful
    #[must_use]
    pub const fn is_success(&self) -> bool {
        matches!(self.status, ResponseStatus::Success)
    }

    /// Check if response is an error
    #[must_use]
    pub const fn is_error(&self) -> bool {
        matches!(self.status, ResponseStatus::Error { .. })
    }

    /// Get error information if response is an error
    #[must_use]
    pub fn error_info(&self) -> Option<(i32, &str)> {
        match &self.status {
            ResponseStatus::Error { code, message } => Some((*code, message)),
            _ => None,
        }
    }
}

impl Default for RequestContext {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ResponseStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Success => write!(f, "Success"),
            Self::Error { code, message } => write!(f, "Error({code}: {message})"),
            Self::Partial => write!(f, "Partial"),
            Self::Cancelled => write!(f, "Cancelled"),
        }
    }
}

// ============================================================================
// Enhanced Client Management and Session Tracking
// ============================================================================

/// Client identification methods for enhanced request routing and analytics
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientId {
    /// Explicit client ID from header
    Header(String),
    /// Bearer token from Authorization header
    Token(String),
    /// Session cookie
    Session(String),
    /// Query parameter
    QueryParam(String),
    /// Hash of User-Agent (fallback)
    UserAgent(String),
    /// Anonymous client
    Anonymous,
}

impl ClientId {
    /// Get the string representation of the client ID
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Header(id)
            | Self::Token(id)
            | Self::Session(id)
            | Self::QueryParam(id)
            | Self::UserAgent(id) => id,
            Self::Anonymous => "anonymous",
        }
    }

    /// Check if the client is authenticated
    #[must_use]
    pub const fn is_authenticated(&self) -> bool {
        matches!(self, Self::Token(_) | Self::Session(_))
    }

    /// Get the authentication method
    #[must_use]
    pub const fn auth_method(&self) -> &'static str {
        match self {
            Self::Header(_) => "header",
            Self::Token(_) => "bearer_token",
            Self::Session(_) => "session_cookie",
            Self::QueryParam(_) => "query_param",
            Self::UserAgent(_) => "user_agent",
            Self::Anonymous => "anonymous",
        }
    }
}

/// Client session information for tracking and analytics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientSession {
    /// Unique client identifier
    pub client_id: String,
    /// Client name (optional, human-readable)
    pub client_name: Option<String>,
    /// When the client connected
    pub connected_at: DateTime<Utc>,
    /// Last activity timestamp
    pub last_activity: DateTime<Utc>,
    /// Number of requests made
    pub request_count: usize,
    /// Transport type (stdio, http, websocket, etc.)
    pub transport_type: String,
    /// Authentication status
    pub authenticated: bool,
    /// Client capabilities (optional)
    pub capabilities: Option<serde_json::Value>,
    /// Additional metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

impl ClientSession {
    /// Create a new client session
    #[must_use]
    pub fn new(client_id: String, transport_type: String) -> Self {
        let now = Utc::now();
        Self {
            client_id,
            client_name: None,
            connected_at: now,
            last_activity: now,
            request_count: 0,
            transport_type,
            authenticated: false,
            capabilities: None,
            metadata: HashMap::new(),
        }
    }

    /// Update activity timestamp and increment request count
    pub fn update_activity(&mut self) {
        self.last_activity = Utc::now();
        self.request_count += 1;
    }

    /// Set authentication status and client info
    pub fn authenticate(&mut self, client_name: Option<String>) {
        self.authenticated = true;
        self.client_name = client_name;
    }

    /// Set client capabilities
    pub fn set_capabilities(&mut self, capabilities: serde_json::Value) {
        self.capabilities = Some(capabilities);
    }

    /// Get session duration
    #[must_use]
    pub fn session_duration(&self) -> chrono::Duration {
        self.last_activity - self.connected_at
    }

    /// Check if session is idle (no activity for specified duration)
    #[must_use]
    pub fn is_idle(&self, idle_threshold: chrono::Duration) -> bool {
        Utc::now() - self.last_activity > idle_threshold
    }
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

/// Client identification extractor for various transport mechanisms
#[derive(Debug)]
pub struct ClientIdExtractor {
    /// Authentication tokens mapping token -> `client_id`
    auth_tokens: Arc<dashmap::DashMap<String, String>>,
}

impl ClientIdExtractor {
    /// Create a new client ID extractor
    #[must_use]
    pub fn new() -> Self {
        Self {
            auth_tokens: Arc::new(dashmap::DashMap::new()),
        }
    }

    /// Register an authentication token for a client
    pub fn register_token(&self, token: String, client_id: String) {
        self.auth_tokens.insert(token, client_id);
    }

    /// Remove an authentication token
    pub fn revoke_token(&self, token: &str) {
        self.auth_tokens.remove(token);
    }

    /// List all registered tokens (for admin purposes)
    #[must_use]
    pub fn list_tokens(&self) -> Vec<(String, String)> {
        self.auth_tokens
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().clone()))
            .collect()
    }

    /// Extract client ID from HTTP headers
    #[must_use]
    #[allow(clippy::significant_drop_tightening)]
    pub fn extract_from_http_headers(&self, headers: &HashMap<String, String>) -> ClientId {
        // 1. Check for explicit client ID header
        if let Some(client_id) = headers.get("x-client-id") {
            return ClientId::Header(client_id.clone());
        }

        // 2. Check for Authorization header with Bearer token
        if let Some(auth) = headers.get("authorization")
            && let Some(token) = auth.strip_prefix("Bearer ")
        {
            // Look up client ID from token
            let token_lookup = self.auth_tokens.iter().find(|e| e.key() == token);
            if let Some(entry) = token_lookup {
                let client_id = entry.value().clone();
                drop(entry); // Explicitly drop the lock guard early
                return ClientId::Token(client_id);
            }
            // Token not found - return the token itself as identifier
            return ClientId::Token(token.to_string());
        }

        // 3. Check for session cookie
        if let Some(cookie) = headers.get("cookie") {
            for cookie_part in cookie.split(';') {
                let parts: Vec<&str> = cookie_part.trim().splitn(2, '=').collect();
                if parts.len() == 2 && (parts[0] == "session_id" || parts[0] == "sessionid") {
                    return ClientId::Session(parts[1].to_string());
                }
            }
        }

        // 4. Use User-Agent hash as fallback
        if let Some(user_agent) = headers.get("user-agent") {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            user_agent.hash(&mut hasher);
            return ClientId::UserAgent(format!("ua_{:x}", hasher.finish()));
        }

        ClientId::Anonymous
    }

    /// Extract client ID from query parameters
    #[must_use]
    pub fn extract_from_query(&self, query_params: &HashMap<String, String>) -> Option<ClientId> {
        query_params
            .get("client_id")
            .map(|client_id| ClientId::QueryParam(client_id.clone()))
    }

    /// Extract client ID from multiple sources (with priority)
    #[must_use]
    pub fn extract_client_id(
        &self,
        headers: Option<&HashMap<String, String>>,
        query_params: Option<&HashMap<String, String>>,
    ) -> ClientId {
        // Try query parameters first (highest priority)
        if let Some(params) = query_params
            && let Some(client_id) = self.extract_from_query(params)
        {
            return client_id;
        }

        // Then try headers
        if let Some(headers) = headers {
            return self.extract_from_http_headers(headers);
        }

        ClientId::Anonymous
    }
}

impl Default for ClientIdExtractor {
    fn default() -> Self {
        Self::new()
    }
}

/// Extension trait to add enhanced client management to `RequestContext`
pub trait RequestContextExt {
    /// Set client ID using `ClientId` enum
    #[must_use]
    fn with_enhanced_client_id(self, client_id: ClientId) -> Self;

    /// Extract and set client ID from headers and query params
    #[must_use]
    fn extract_client_id(
        self,
        extractor: &ClientIdExtractor,
        headers: Option<&HashMap<String, String>>,
        query_params: Option<&HashMap<String, String>>,
    ) -> Self;

    /// Get the enhanced client ID
    fn get_enhanced_client_id(&self) -> Option<ClientId>;
}

impl RequestContextExt for RequestContext {
    fn with_enhanced_client_id(self, client_id: ClientId) -> Self {
        self.with_client_id(client_id.as_str())
            .with_metadata("client_id_method", client_id.auth_method())
            .with_metadata("client_authenticated", client_id.is_authenticated())
    }

    fn extract_client_id(
        self,
        extractor: &ClientIdExtractor,
        headers: Option<&HashMap<String, String>>,
        query_params: Option<&HashMap<String, String>>,
    ) -> Self {
        let client_id = extractor.extract_client_id(headers, query_params);
        self.with_enhanced_client_id(client_id)
    }

    fn get_enhanced_client_id(&self) -> Option<ClientId> {
        self.client_id.as_ref().map(|id| {
            let method = self
                .get_metadata("client_id_method")
                .and_then(|v| v.as_str())
                .unwrap_or("header");

            match method {
                "bearer_token" => ClientId::Token(id.clone()),
                "session_cookie" => ClientId::Session(id.clone()),
                "query_param" => ClientId::QueryParam(id.clone()),
                "user_agent" => ClientId::UserAgent(id.clone()),
                "anonymous" => ClientId::Anonymous,
                _ => ClientId::Header(id.clone()), // Default to header for "header" and unknown methods
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_context_creation() {
        let ctx = RequestContext::new();
        assert!(!ctx.request_id.is_empty());
        assert!(ctx.user_id.is_none());
        assert!(ctx.elapsed() < std::time::Duration::from_millis(100));
    }

    #[test]
    fn test_request_context_builder() {
        let ctx = RequestContext::new()
            .with_user_id("user123")
            .with_session_id("session456")
            .with_metadata("key", "value");

        assert_eq!(ctx.user_id, Some("user123".to_string()));
        assert_eq!(ctx.session_id, Some("session456".to_string()));
        assert_eq!(
            ctx.get_metadata("key"),
            Some(&serde_json::Value::String("value".to_string()))
        );
    }

    #[test]
    fn test_response_context_creation() {
        let duration = std::time::Duration::from_millis(100);

        let success_ctx = ResponseContext::success("req1", duration);
        assert!(success_ctx.is_success());
        assert!(!success_ctx.is_error());

        let error_ctx = ResponseContext::error("req2", duration, 500, "Internal error");
        assert!(!error_ctx.is_success());
        assert!(error_ctx.is_error());
        assert_eq!(error_ctx.error_info(), Some((500, "Internal error")));
    }

    #[test]
    fn test_context_derivation() {
        let parent_ctx = RequestContext::new()
            .with_user_id("user123")
            .with_metadata("key", "value");

        let child_ctx = parent_ctx.derive();

        // Should have new request ID
        assert_ne!(parent_ctx.request_id, child_ctx.request_id);

        // Should inherit user info and metadata
        assert_eq!(parent_ctx.user_id, child_ctx.user_id);
        assert_eq!(
            parent_ctx.get_metadata("key"),
            child_ctx.get_metadata("key")
        );
    }

    // Tests for enhanced client management

    #[test]
    fn test_client_id_extraction() {
        let extractor = ClientIdExtractor::new();

        // Test header extraction
        let mut headers = HashMap::new();
        headers.insert("x-client-id".to_string(), "test-client".to_string());

        let client_id = extractor.extract_from_http_headers(&headers);
        assert_eq!(client_id, ClientId::Header("test-client".to_string()));
        assert_eq!(client_id.as_str(), "test-client");
        assert_eq!(client_id.auth_method(), "header");
        assert!(!client_id.is_authenticated());
    }

    #[test]
    fn test_bearer_token_extraction() {
        let extractor = ClientIdExtractor::new();
        extractor.register_token("token123".to_string(), "client-1".to_string());

        let mut headers = HashMap::new();
        headers.insert("authorization".to_string(), "Bearer token123".to_string());

        let client_id = extractor.extract_from_http_headers(&headers);
        assert_eq!(client_id, ClientId::Token("client-1".to_string()));
        assert!(client_id.is_authenticated());
        assert_eq!(client_id.auth_method(), "bearer_token");
    }

    #[test]
    fn test_session_cookie_extraction() {
        let extractor = ClientIdExtractor::new();

        let mut headers = HashMap::new();
        headers.insert(
            "cookie".to_string(),
            "session_id=sess123; other=value".to_string(),
        );

        let client_id = extractor.extract_from_http_headers(&headers);
        assert_eq!(client_id, ClientId::Session("sess123".to_string()));
        assert!(client_id.is_authenticated());
    }

    #[test]
    fn test_user_agent_fallback() {
        let extractor = ClientIdExtractor::new();

        let mut headers = HashMap::new();
        headers.insert("user-agent".to_string(), "TestAgent/1.0".to_string());

        let client_id = extractor.extract_from_http_headers(&headers);
        if let ClientId::UserAgent(id) = client_id {
            assert!(id.starts_with("ua_"));
        } else {
            // Ensure test failure without panicking in production codepaths
            assert!(
                matches!(client_id, ClientId::UserAgent(_)),
                "Expected UserAgent ClientId"
            );
        }
    }

    #[test]
    fn test_client_session() {
        let mut session = ClientSession::new("test-client".to_string(), "http".to_string());
        assert!(!session.authenticated);
        assert_eq!(session.request_count, 0);

        session.update_activity();
        assert_eq!(session.request_count, 1);

        session.authenticate(Some("Test Client".to_string()));
        assert!(session.authenticated);
        assert_eq!(session.client_name, Some("Test Client".to_string()));

        // Test idle detection
        assert!(!session.is_idle(chrono::Duration::seconds(1)));
    }

    #[test]
    fn test_request_info() {
        let params = serde_json::json!({"param": "value"});
        let request = RequestInfo::new("client-1".to_string(), "test_method".to_string(), params);

        assert!(!request.success);
        assert!(request.response_time_ms.is_none());

        let completed = request.complete_success(150);
        assert!(completed.success);
        assert_eq!(completed.response_time_ms, Some(150));
        assert_eq!(completed.status_code, Some(200));
    }

    #[test]
    fn test_request_context_ext() {
        let extractor = ClientIdExtractor::new();

        let mut headers = HashMap::new();
        headers.insert("x-client-id".to_string(), "test-client".to_string());

        let ctx = RequestContext::new().extract_client_id(&extractor, Some(&headers), None);

        assert_eq!(ctx.client_id, Some("test-client".to_string()));
        assert_eq!(
            ctx.get_metadata("client_id_method"),
            Some(&serde_json::Value::String("header".to_string()))
        );
        assert_eq!(
            ctx.get_metadata("client_authenticated"),
            Some(&serde_json::Value::Bool(false))
        );

        let enhanced_id = ctx.get_enhanced_client_id();
        assert_eq!(
            enhanced_id,
            Some(ClientId::Header("test-client".to_string()))
        );
    }

    #[test]
    fn test_request_analytics() {
        let start = std::time::Instant::now();
        let request = RequestInfo::new(
            "client-123".to_string(),
            "get_data".to_string(),
            serde_json::json!({"filter": "active"}),
        );

        let response_time = start.elapsed().as_millis() as u64;
        let completed = request
            .complete_success(response_time)
            .with_metadata("cache_hit".to_string(), serde_json::json!(true));

        assert!(completed.success);
        assert!(completed.response_time_ms.is_some());
        assert_eq!(
            completed.metadata.get("cache_hit"),
            Some(&serde_json::json!(true))
        );
    }

    // ========================================================================
    // Tests for Enhanced Context Types (New MCP Features)
    // ========================================================================

    #[test]
    fn test_elicitation_context() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "integer"}
            }
        });

        let ctx = RequestContext::for_elicitation(
            schema.clone(),
            Some("Please enter your details".to_string()),
        );

        // Verify elicitation context
        let elicit_ctx = ctx.elicitation_context().unwrap();
        assert_eq!(elicit_ctx.schema, schema);
        assert_eq!(elicit_ctx.message, "Please enter your details".to_string());
        assert!(elicit_ctx.required);
        assert!(elicit_ctx.cancellable);
        assert_eq!(elicit_ctx.timeout_ms, Some(30_000));

        // Verify bidirectional context
        assert!(ctx.is_server_initiated());
        assert!(!ctx.is_client_initiated());
        assert_eq!(
            ctx.communication_direction(),
            CommunicationDirection::ServerToClient
        );

        let bi_ctx = ctx.bidirectional_context().unwrap();
        assert_eq!(bi_ctx.direction, CommunicationDirection::ServerToClient);
        assert_eq!(bi_ctx.initiator, CommunicationInitiator::Server);
        assert!(bi_ctx.expects_response);
    }

    #[test]
    fn test_completion_context() {
        let comp_ref = CompletionReference::Tool {
            name: "test_tool".to_string(),
            argument: "file_path".to_string(),
        };

        let ctx = RequestContext::for_completion(comp_ref.clone());
        let completion_ctx = ctx.completion_context().unwrap();

        assert!(matches!(
            completion_ctx.completion_ref,
            CompletionReference::Tool { .. }
        ));
        assert_eq!(completion_ctx.max_completions, Some(100));
        assert!(completion_ctx.completions.is_empty());

        // Test with completion options
        let completion_option = CompletionOption {
            value: "/home/user/document.txt".to_string(),
            label: Some("document.txt".to_string()),
            completion_type: Some("file".to_string()),
            documentation: Some("A text document".to_string()),
            sort_priority: Some(1),
            insert_text: Some("document.txt".to_string()),
        };

        let mut completion_ctx_with_options = CompletionContext::new(comp_ref);
        completion_ctx_with_options.argument_name = Some("file_path".to_string());
        completion_ctx_with_options.partial_value = Some("/home/user/".to_string());
        completion_ctx_with_options.completions = vec![completion_option];
        completion_ctx_with_options.cursor_position = Some(11);
        completion_ctx_with_options.max_completions = Some(10);

        let ctx_with_options =
            RequestContext::new().with_completion_context(completion_ctx_with_options);
        let retrieved_ctx = ctx_with_options.completion_context().unwrap();

        assert_eq!(retrieved_ctx.argument_name, Some("file_path".to_string()));
        assert_eq!(retrieved_ctx.partial_value, Some("/home/user/".to_string()));
        assert_eq!(retrieved_ctx.completions.len(), 1);
        assert_eq!(
            retrieved_ctx.completions[0].value,
            "/home/user/document.txt"
        );
        assert_eq!(retrieved_ctx.cursor_position, Some(11));
    }

    #[test]
    fn test_resource_template_context() {
        let template_name = "file_system".to_string();
        let uri_template = "file://{path}".to_string();

        let ctx =
            RequestContext::for_resource_template(template_name.clone(), uri_template.clone());
        let template_ctx = ctx.resource_template_context().unwrap();

        assert_eq!(template_ctx.template_name, template_name);
        assert_eq!(template_ctx.uri_template, uri_template);
        assert!(template_ctx.parameters.is_empty());

        // Test with parameters
        let mut parameters = HashMap::new();
        parameters.insert(
            "path".to_string(),
            TemplateParameter {
                name: "path".to_string(),
                param_type: "string".to_string(),
                required: true,
                default: None,
                description: Some("File system path".to_string()),
                pattern: Some(r"^[/\w.-]+$".to_string()),
                enum_values: None,
            },
        );

        let template_ctx_with_params = ResourceTemplateContext {
            template_name: "file_system_detailed".to_string(),
            uri_template: "file://{path}".to_string(),
            parameters,
            description: Some("Access file system resources".to_string()),
            preset_type: Some("file_system".to_string()),
            metadata: HashMap::new(),
        };

        let ctx_with_params =
            RequestContext::new().with_resource_template_context(template_ctx_with_params);
        let retrieved_ctx = ctx_with_params.resource_template_context().unwrap();

        assert_eq!(retrieved_ctx.parameters.len(), 1);
        let path_param = retrieved_ctx.parameters.get("path").unwrap();
        assert_eq!(path_param.param_type, "string");
        assert!(path_param.required);
        assert_eq!(path_param.description, Some("File system path".to_string()));
        assert_eq!(
            retrieved_ctx.description,
            Some("Access file system resources".to_string())
        );
    }

    #[test]
    fn test_ping_context_client_initiated() {
        let ctx = RequestContext::for_ping(PingOrigin::Client);
        let ping_ctx = ctx.ping_context().unwrap();

        assert_eq!(ping_ctx.origin, PingOrigin::Client);
        assert_eq!(ping_ctx.response_threshold_ms, Some(5_000));
        assert!(ping_ctx.payload.is_none());

        // Verify bidirectional context for client ping
        assert!(!ctx.is_server_initiated());
        assert!(ctx.is_client_initiated());
        assert_eq!(
            ctx.communication_direction(),
            CommunicationDirection::ClientToServer
        );

        let bi_ctx = ctx.bidirectional_context().unwrap();
        assert_eq!(bi_ctx.initiator, CommunicationInitiator::Client);
    }

    #[test]
    fn test_ping_context_server_initiated() {
        let ctx = RequestContext::for_ping(PingOrigin::Server);
        let ping_ctx = ctx.ping_context().unwrap();

        assert_eq!(ping_ctx.origin, PingOrigin::Server);

        // Verify bidirectional context for server ping
        assert!(ctx.is_server_initiated());
        assert!(!ctx.is_client_initiated());
        assert_eq!(
            ctx.communication_direction(),
            CommunicationDirection::ServerToClient
        );

        let bi_ctx = ctx.bidirectional_context().unwrap();
        assert_eq!(bi_ctx.initiator, CommunicationInitiator::Server);
    }

    #[test]
    fn test_connection_metrics() {
        let mut ping_ctx = PingContext {
            origin: PingOrigin::Client,
            response_threshold_ms: Some(1_000),
            payload: Some(serde_json::json!({"test": true})),
            health_metadata: HashMap::new(),
            connection_metrics: None,
        };

        // Add connection metrics
        let metrics = ConnectionMetrics {
            rtt_ms: Some(150.5),
            packet_loss: Some(0.1),
            uptime_seconds: Some(3600),
            bytes_sent: Some(1024),
            bytes_received: Some(2048),
            last_success: Some(Utc::now()),
        };

        ping_ctx.connection_metrics = Some(metrics);

        let ctx = RequestContext::new().with_ping_context(ping_ctx);
        let retrieved_ctx = ctx.ping_context().unwrap();
        let conn_metrics = retrieved_ctx.connection_metrics.unwrap();

        assert_eq!(conn_metrics.rtt_ms, Some(150.5));
        assert_eq!(conn_metrics.packet_loss, Some(0.1));
        assert_eq!(conn_metrics.uptime_seconds, Some(3600));
        assert_eq!(conn_metrics.bytes_sent, Some(1024));
        assert_eq!(conn_metrics.bytes_received, Some(2048));
        assert!(conn_metrics.last_success.is_some());
    }

    #[test]
    fn test_bidirectional_context_standalone() {
        let mut bi_ctx = BidirectionalContext::new(
            CommunicationDirection::ServerToClient,
            CommunicationInitiator::Server,
        );
        bi_ctx.expects_response = true;
        bi_ctx.parent_request_id = Some("parent-123".to_string());

        let ctx = RequestContext::new().with_bidirectional_context(bi_ctx.clone());

        assert!(ctx.is_server_initiated());
        assert_eq!(
            ctx.communication_direction(),
            CommunicationDirection::ServerToClient
        );

        let retrieved_ctx = ctx.bidirectional_context().unwrap();
        assert_eq!(
            retrieved_ctx.parent_request_id,
            Some("parent-123".to_string())
        );
        assert_eq!(
            retrieved_ctx.direction,
            CommunicationDirection::ServerToClient
        );
        assert_eq!(retrieved_ctx.initiator, CommunicationInitiator::Server);
        assert!(retrieved_ctx.expects_response);
    }

    #[test]
    fn test_completion_reference_serialization() {
        let prompt_ref = CompletionReference::Prompt {
            name: "test_prompt".to_string(),
            argument: "user_input".to_string(),
        };

        let template_ref = CompletionReference::ResourceTemplate {
            name: "api_endpoint".to_string(),
            parameter: "api_key".to_string(),
        };

        let tool_ref = CompletionReference::Tool {
            name: "file_reader".to_string(),
            argument: "path".to_string(),
        };

        let custom_ref = CompletionReference::Custom {
            ref_type: "database_query".to_string(),
            metadata: {
                let mut map = HashMap::new();
                map.insert("table".to_string(), serde_json::json!("users"));
                map
            },
        };

        // Test serialization round-trip
        let refs = vec![prompt_ref, template_ref, tool_ref, custom_ref];
        for ref_item in refs {
            let serialized = serde_json::to_value(&ref_item).unwrap();
            let deserialized: CompletionReference = serde_json::from_value(serialized).unwrap();

            match (&ref_item, &deserialized) {
                (
                    CompletionReference::Prompt {
                        name: n1,
                        argument: a1,
                    },
                    CompletionReference::Prompt {
                        name: n2,
                        argument: a2,
                    },
                ) => {
                    assert_eq!(n1, n2);
                    assert_eq!(a1, a2);
                }
                (
                    CompletionReference::ResourceTemplate {
                        name: n1,
                        parameter: p1,
                    },
                    CompletionReference::ResourceTemplate {
                        name: n2,
                        parameter: p2,
                    },
                ) => {
                    assert_eq!(n1, n2);
                    assert_eq!(p1, p2);
                }
                (
                    CompletionReference::Tool {
                        name: n1,
                        argument: a1,
                    },
                    CompletionReference::Tool {
                        name: n2,
                        argument: a2,
                    },
                ) => {
                    assert_eq!(n1, n2);
                    assert_eq!(a1, a2);
                }
                (
                    CompletionReference::Custom {
                        ref_type: t1,
                        metadata: m1,
                    },
                    CompletionReference::Custom {
                        ref_type: t2,
                        metadata: m2,
                    },
                ) => {
                    assert_eq!(t1, t2);
                    assert_eq!(m1.len(), m2.len());
                }
                _ => panic!("Serialization round-trip failed for CompletionReference"),
            }
        }
    }

    #[test]
    fn test_context_metadata_integration() {
        // Test that multiple context types can coexist
        let mut elicit_ctx = ElicitationContext::new(
            "Enter name".to_string(),
            serde_json::json!({"type": "string"}),
        );
        elicit_ctx.required = true;
        elicit_ctx.timeout_ms = Some(30_000);
        elicit_ctx.cancellable = true;

        let ping_ctx = PingContext {
            origin: PingOrigin::Server,
            response_threshold_ms: Some(2_000),
            payload: None,
            health_metadata: HashMap::new(),
            connection_metrics: None,
        };

        let ctx = RequestContext::new()
            .with_elicitation_context(elicit_ctx)
            .with_ping_context(ping_ctx)
            .with_metadata("custom_field", "custom_value");

        // Verify all contexts are preserved
        assert!(ctx.elicitation_context().is_some());
        assert!(ctx.ping_context().is_some());
        assert_eq!(
            ctx.get_metadata("custom_field"),
            Some(&serde_json::json!("custom_value"))
        );

        // Verify context-specific data
        let elicit = ctx.elicitation_context().unwrap();
        assert_eq!(elicit.message, "Enter name".to_string());

        let ping = ctx.ping_context().unwrap();
        assert_eq!(ping.response_threshold_ms, Some(2_000));
    }
}
